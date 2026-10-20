// Copyright 2026 Muvon Un Limited
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Multi-repository MCP server (`octocode mcp --multi`).
//!
//! Scans the immediate subdirectories of the root path for git repositories and
//! exposes the regular Octocode tool set over a single MCP endpoint (stdio or
//! HTTP). Every tool gains a `project` argument — injected dynamically into the
//! schema and description, only in this mode — that selects which repository the
//! call runs against. Each repository gets its own lazily-built handler with its
//! own store and background watcher/index threads, mapped to the same per-project
//! storage the single-repo `mcp` command uses.
//!
//! This replaces the former `mcp-proxy` command: instead of routing by URL path
//! (which required HTTP), routing is by tool argument, so it works over stdio too.

use anyhow::Result;
use rmcp::{
	handler::server::ServerHandler,
	model::{
		CallToolRequestParams, CallToolResponse, Implementation, ListToolsResult, PaginatedRequestParams,
		ProtocolVersion, ServerCapabilities, ServerConfig, Tool,
	},
	service::RequestContext,
	ErrorData, RoleServer, ServiceExt,
};
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};
use tracing::{debug, warn};

use crate::config::Config;
use crate::indexer;
use crate::mcp::logging::init_mcp_logging;
use crate::mcp::server::{list_tools_result, BackgroundServices, McpServer};
use crate::mcp::session::{SessionRouter, Sessions};

#[cfg(test)]
#[path = "multi_tests.rs"]
mod multi_tests;

const INSTANCE_CLEANUP_INTERVAL_MS: u64 = 300_000; // 5 minutes
const INSTANCE_IDLE_TIMEOUT_MS: u64 = 1_800_000; // 30 minutes

// ---------------------------------------------------------------------------
// Per-repo instances
// ---------------------------------------------------------------------------

/// A live per-repository handler plus its background services. Dropping this
/// (on idle cleanup) aborts the repo's watcher and indexing threads.
struct RepoInstance {
	server: McpServer,
	last_accessed: Instant,
	_bg: BackgroundServices,
}

/// Per-repository handlers built on first use, keyed by repository path and
/// dropped after `INSTANCE_IDLE_TIMEOUT_MS` idle. Multi mode and session routing
/// share one pool, so a repository served both ways runs one watcher and indexer.
#[derive(Clone)]
pub(crate) struct RepoPool {
	pub(crate) config: Config,
	pub(crate) no_git: bool,
	debug: bool,
	instances: Arc<Mutex<HashMap<PathBuf, RepoInstance>>>,
}

impl RepoPool {
	pub(crate) fn new(config: Config, no_git: bool, debug: bool) -> Self {
		Self {
			config,
			no_git,
			debug,
			instances: Arc::new(Mutex::new(HashMap::new())),
		}
	}

	/// Resolve (and lazily build) the handler for the repository at `repo_path`.
	pub(crate) async fn get(&self, repo_path: &Path) -> Result<McpServer, ErrorData> {
		// Fast path: already built.
		{
			let mut guard = self.instances.lock().await;
			if let Some(inst) = guard.get_mut(repo_path) {
				inst.last_accessed = Instant::now();
				return Ok(inst.server.clone());
			}
		}

		// Build outside the lock (store creation + index bootstrap are async).
		// A concurrent first-call for the same repo may also build; `or_insert`
		// keeps the first and drops the loser's background services. The initial
		// index is lock-guarded, so the rare double-build is harmless.
		let server = McpServer::new_repo_core(self.config.clone(), repo_path.to_path_buf());
		// The single-repo server's gate: index in-process only when enabled, and
		// outside a git repository root only with --no-git or require_git = false.
		// Otherwise serve the existing per-repo index without a watcher/indexer.
		let index = self.config.index.mcp_index
			&& (self.no_git || !self.config.index.require_git || indexer::git::is_git_repo_root(repo_path));
		let bg = if index {
			McpServer::start_repo_services(
				self.config.clone(),
				repo_path.to_path_buf(),
				self.no_git,
				self.debug,
				server.runtime_graph_cache(),
			)
			.await
			.map_err(|e| {
				ErrorData::internal_error(
					format!("Failed to start services for '{}': {}", repo_path.display(), e),
					None,
				)
			})?
		} else {
			BackgroundServices::none()
		};

		let mut guard = self.instances.lock().await;
		let inst = guard.entry(repo_path.to_path_buf()).or_insert(RepoInstance {
			server,
			last_accessed: Instant::now(),
			_bg: bg,
		});
		inst.last_accessed = Instant::now();
		Ok(inst.server.clone())
	}

	/// Periodically drop instances idle past the timeout, aborting their threads.
	pub(crate) fn spawn_idle_cleanup(&self) {
		let instances = self.instances.clone();
		tokio::spawn(async move {
			let mut interval = tokio::time::interval(Duration::from_millis(INSTANCE_CLEANUP_INTERVAL_MS));
			loop {
				interval.tick().await;
				let mut guard = instances.lock().await;
				let before = guard.len();
				guard.retain(|_, inst| inst.last_accessed.elapsed() <= Duration::from_millis(INSTANCE_IDLE_TIMEOUT_MS));
				let removed = before - guard.len();
				if removed > 0 {
					debug!("Cleaned up {} idle repo instance(s)", removed);
				}
			}
		});
	}
}

// ---------------------------------------------------------------------------
// Projects under a directory
// ---------------------------------------------------------------------------

/// The repositories one level under a directory, and the tool list that routes
/// to them through an injected `project` argument.
pub(crate) struct Projects {
	/// project key (subdirectory name) -> absolute repo path
	repos: HashMap<String, PathBuf>,
	tools: Vec<Tool>,
}

impl Projects {
	/// Discover the repositories one level under `root` and inject the `project`
	/// argument into `repo_tools`, the tool set of a per-repo handler.
	pub(crate) fn discover(root: &Path, no_git: bool, repo_tools: Vec<Tool>) -> Result<Self> {
		let repos = discover_repos(root, no_git)?;
		let mut keys: Vec<String> = repos.keys().cloned().collect();
		keys.sort();
		let tools = inject_project_arg(repo_tools, &keys);
		Ok(Self { repos, tools })
	}

	pub(crate) fn tools(&self) -> &[Tool] {
		&self.tools
	}

	fn get_tool(&self, name: &str) -> Option<Tool> {
		self.tools.iter().find(|t| t.name.as_ref() == name).cloned()
	}

	/// Comma-separated sorted list of valid project keys, for error/help text.
	fn project_list(&self) -> String {
		let mut keys: Vec<&str> = self.repos.keys().map(String::as_str).collect();
		keys.sort();
		if keys.is_empty() {
			"(none discovered)".to_string()
		} else {
			keys.join(", ")
		}
	}

	/// Take the `project` argument out of `request` — the per-repo tool sees only
	/// its own parameters — and resolve it to the repository path.
	pub(crate) fn route(&self, request: &mut CallToolRequestParams) -> Result<PathBuf, ErrorData> {
		let project = request
			.arguments
			.as_ref()
			.and_then(|args| args.get("project"))
			.and_then(|v| v.as_str())
			.map(str::to_string)
			.ok_or_else(|| {
				ErrorData::invalid_params(
					format!(
						"Missing required 'project' argument. Available repositories: {}",
						self.project_list()
					),
					None,
				)
			})?;
		if let Some(args) = request.arguments.as_mut() {
			args.remove("project");
		}
		self.repos.get(&project).cloned().ok_or_else(|| {
			ErrorData::invalid_params(
				format!(
					"Unknown project '{}'. Available repositories: {}",
					project,
					self.project_list()
				),
				None,
			)
		})
	}
}

// ---------------------------------------------------------------------------
// MultiServer
// ---------------------------------------------------------------------------

/// Multi-repository MCP server. Cloned per HTTP session; all clones share the
/// discovered projects and the per-repo handler pool.
#[derive(Clone)]
pub struct MultiServer {
	root: PathBuf,
	projects: Arc<Projects>,
	pool: RepoPool,
}

impl MultiServer {
	/// Discover repositories one level under `root_path`, build the injected
	/// tool list, and prepare logging. Per-repo handlers are built on demand.
	pub async fn new(config: Config, root_path: PathBuf, no_git: bool, debug: bool) -> Result<Self> {
		init_mcp_logging(root_path.clone(), debug)?;

		// Build the base tool set from a template handler over the root path,
		// then inject the `project` argument into every tool.
		let template = McpServer::new_repo_core(config.clone(), root_path.clone());
		let projects = Projects::discover(&root_path, no_git, template.list_tool_defs())?;

		Ok(Self {
			root: root_path,
			projects: Arc::new(projects),
			pool: RepoPool::new(config, no_git, debug),
		})
	}

	/// This server behind session routing; sessions in other directories share
	/// its repo pool.
	fn routed(&self) -> SessionRouter<Self> {
		SessionRouter::new(self.clone(), Sessions::with_pool(self.root.clone(), self.pool.clone()))
	}

	/// Serve over stdio (default MCP transport).
	pub async fn run_stdio(self) -> Result<()> {
		self.pool.spawn_idle_cleanup();
		let transport = rmcp::transport::io::stdio();
		let service = self.routed().serve(transport).await?;
		service.waiting().await?;
		Ok(())
	}

	/// Serve over Streamable HTTP. Each session clones the server, sharing the
	/// discovered projects and the repo pool.
	pub async fn run_http(self, bind_addr: &str) -> Result<()> {
		use hyper_util::rt::TokioIo;
		use hyper_util::service::TowerToHyperService;
		use rmcp::transport::streamable_http_server::{session::local::LocalSessionManager, StreamableHttpService};

		self.pool.spawn_idle_cleanup();

		let server = self.routed();
		let service = StreamableHttpService::new(
			move || Ok(server.clone()),
			Arc::new(LocalSessionManager::default()),
			Default::default(),
		);

		let addr: SocketAddr = bind_addr
			.parse()
			.map_err(|e| anyhow::anyhow!("Invalid bind address '{}': {}", bind_addr, e))?;

		let listener = tokio::net::TcpListener::bind(addr).await?;
		debug!("Multi MCP HTTP server listening on {}", addr);

		loop {
			// Don't let one bad accept() (e.g. a client resetting mid-handshake)
			// take down the whole server — log and keep serving.
			let (stream, remote_addr) = match listener.accept().await {
				Ok(conn) => conn,
				Err(e) => {
					warn!("Failed to accept connection: {}", e);
					// Back off briefly: persistent errors (e.g. EMFILE when out
					// of file descriptors) return instantly and would otherwise
					// spin this loop at 100% CPU.
					tokio::time::sleep(std::time::Duration::from_millis(100)).await;
					continue;
				}
			};
			let service = service.clone();
			tokio::spawn(async move {
				let io = TokioIo::new(stream);
				let hyper_service = TowerToHyperService::new(service);
				if let Err(e) = hyper::server::conn::http1::Builder::new()
					.serve_connection(io, hyper_service)
					.await
				{
					debug!("Connection error from {}: {}", remote_addr, e);
				}
			});
		}
	}
}

impl ServerHandler for MultiServer {
	fn get_info(&self) -> ServerConfig {
		let capabilities = ServerCapabilities::builder().enable_tools().build();
		let instructions = format!(
			"Multi-repository Octocode MCP server. {} repositories are available ({}); \
			 every tool requires a `project` argument naming the target repository. \
			 Use 'semantic_search' for conceptual lookups, 'structural_search' for \
			 syntax and occurrences, and the always-available 'graphrag' for symbol \
			 relationships and dependency paths.",
			self.projects.repos.len(),
			self.projects.project_list()
		);

		ServerConfig::new(capabilities)
			.with_protocol_version(ProtocolVersion::V_2026_07_28)
			.with_server_info(
				Implementation::new("octocode-mcp", env!("CARGO_PKG_VERSION"))
					.with_description("Multi-repository semantic code search server with per-repo routing"),
			)
			.with_instructions(instructions)
	}

	async fn list_tools(
		&self,
		_request: Option<PaginatedRequestParams>,
		context: RequestContext<RoleServer>,
	) -> Result<ListToolsResult, ErrorData> {
		Ok(list_tools_result(self.projects.tools().to_vec(), &context))
	}

	fn get_tool(&self, name: &str) -> Option<Tool> {
		self.projects.get_tool(name)
	}

	async fn call_tool(
		&self,
		mut request: CallToolRequestParams,
		context: RequestContext<RoleServer>,
	) -> Result<CallToolResponse, ErrorData> {
		let repo_path = self.projects.route(&mut request)?;
		let server = self.pool.get(&repo_path).await?;
		server.dispatch_tool(request, context).await
	}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Discover projects in the immediate children of `root` (one level deep),
/// keyed by directory name. Default mode requires each child to be a git repo
/// (contains `.git`); `--no-git` treats every (non-hidden) subdirectory as a
/// project, matching how single-repo `mcp --no-git` indexes outside git.
fn discover_repos(root: &Path, no_git: bool) -> Result<HashMap<String, PathBuf>> {
	let mut repos = HashMap::new();

	for entry in std::fs::read_dir(root)?.flatten() {
		let path = entry.path();
		if !path.is_dir() {
			continue;
		}
		let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
			continue;
		};
		// Skip hidden directories (.git, .cache, …) in both modes.
		if name.starts_with('.') {
			continue;
		}
		// Default mode requires a git repo; --no-git accepts any subdirectory.
		if !no_git && !path.join(".git").exists() {
			continue;
		}
		repos.insert(name.to_string(), path.to_path_buf());
	}

	Ok(repos)
}

/// Whether at least one qualifying project exists among the immediate children
/// of `root`, using the exact same discovery rules as multi mode. Lets
/// `mcp --auto` pick a mode without constructing a full `MultiServer` (which
/// initializes logging and builds tool schemas).
pub fn has_child_repos(root: &Path, no_git: bool) -> bool {
	discover_repos(root, no_git).is_ok_and(|repos| !repos.is_empty())
}

/// Clone the base tools and inject a required `project` argument into each one's
/// schema, plus a hint in the description. Done only in multi mode, leaving the
/// single-repo tool definitions untouched.
fn inject_project_arg(mut tools: Vec<Tool>, keys: &[String]) -> Vec<Tool> {
	let list = if keys.is_empty() {
		"(none discovered)".to_string()
	} else {
		keys.join(", ")
	};

	for tool in &mut tools {
		// Amend the description.
		let base_desc = tool.description.as_deref().unwrap_or("");
		tool.description = Some(Cow::Owned(format!(
			"{base_desc}\n\n[multi-repo] Set `project` to the repository to target. Available: {list}."
		)));

		// Build the `project` property; constrain to known keys when we have any.
		let mut prop = json!({
			"type": "string",
			"description": format!("Repository to run this tool against. One of: {list}."),
		});
		if !keys.is_empty() {
			prop["enum"] = json!(keys);
		}

		let mut schema: Map<String, Value> = (*tool.input_schema).clone();

		match schema.get_mut("properties") {
			Some(Value::Object(props)) => {
				props.insert("project".to_string(), prop);
			}
			_ => {
				schema.insert("properties".to_string(), json!({ "project": prop }));
			}
		}

		match schema.get_mut("required") {
			Some(Value::Array(required)) => {
				if !required.iter().any(|v| v.as_str() == Some("project")) {
					required.push(json!("project"));
				}
			}
			_ => {
				schema.insert("required".to_string(), json!(["project"]));
			}
		}

		tool.input_schema = Arc::new(schema);
	}

	tools
}
