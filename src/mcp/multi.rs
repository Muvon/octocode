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
		CallToolRequestParams, CallToolResult, Implementation, ListToolsResult,
		PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
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
use tracing::debug;

use crate::config::Config;
use crate::mcp::logging::init_mcp_logging;
use crate::mcp::server::{BackgroundServices, McpServer};

const INSTANCE_CLEANUP_INTERVAL_MS: u64 = 300_000; // 5 minutes
const INSTANCE_IDLE_TIMEOUT_MS: u64 = 1_800_000; // 30 minutes

// ---------------------------------------------------------------------------
// Per-repo instance
// ---------------------------------------------------------------------------

/// A live per-repository handler plus its background services. Dropping this
/// (on idle cleanup) aborts the repo's watcher and indexing threads.
struct RepoInstance {
	server: McpServer,
	last_accessed: Instant,
	_bg: BackgroundServices,
}

// ---------------------------------------------------------------------------
// MultiServer
// ---------------------------------------------------------------------------

/// Multi-repository MCP server. Cloned per HTTP session; all clones share the
/// discovered repo map and the lazily-built instance map.
#[derive(Clone)]
pub struct MultiServer {
	config: Config,
	no_git: bool,
	debug: bool,
	/// project key (subdirectory name) -> absolute repo path
	repos: Arc<HashMap<String, PathBuf>>,
	/// Lazily-built per-repo instances, keyed by project.
	instances: Arc<Mutex<HashMap<String, RepoInstance>>>,
	/// Tool list with the `project` argument injected, computed once at startup.
	tools: Arc<Vec<Tool>>,
}

impl MultiServer {
	/// Discover repositories one level under `root_path`, build the injected
	/// tool list, and prepare logging. Per-repo handlers are built on demand.
	pub async fn new(
		config: Config,
		root_path: PathBuf,
		no_git: bool,
		debug: bool,
	) -> Result<Self> {
		init_mcp_logging(root_path.clone(), debug)?;

		let repos = discover_repos(&root_path)?;
		let mut keys: Vec<String> = repos.keys().cloned().collect();
		keys.sort();

		// Build the base tool set from a template handler over the root path,
		// then inject the `project` argument into every tool.
		let template = McpServer::new_repo_core(config.clone(), root_path.clone());
		let tools = inject_project_arg(template.list_tool_defs(), &keys);

		Ok(Self {
			config,
			no_git,
			debug,
			repos: Arc::new(repos),
			instances: Arc::new(Mutex::new(HashMap::new())),
			tools: Arc::new(tools),
		})
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

	/// Resolve (and lazily build) the per-repo handler for `project`.
	async fn get_server(&self, project: &str) -> Result<McpServer, ErrorData> {
		let repo_path = self.repos.get(project).cloned().ok_or_else(|| {
			ErrorData::invalid_params(
				format!(
					"Unknown project '{}'. Available repositories: {}",
					project,
					self.project_list()
				),
				None,
			)
		})?;

		// Fast path: already built.
		{
			let mut guard = self.instances.lock().await;
			if let Some(inst) = guard.get_mut(project) {
				inst.last_accessed = Instant::now();
				return Ok(inst.server.clone());
			}
		}

		// Build outside the lock (store creation + index bootstrap are async).
		// A concurrent first-call for the same project may also build; `or_insert`
		// keeps the first and drops the loser's background services. The initial
		// index is lock-guarded, so the rare double-build is harmless.
		let server = McpServer::new_repo_core(self.config.clone(), repo_path.clone());
		let bg =
			McpServer::start_repo_services(self.config.clone(), repo_path, self.no_git, self.debug)
				.await
				.map_err(|e| {
					ErrorData::internal_error(
						format!("Failed to start services for '{}': {}", project, e),
						None,
					)
				})?;

		let mut guard = self.instances.lock().await;
		let inst = guard.entry(project.to_string()).or_insert(RepoInstance {
			server,
			last_accessed: Instant::now(),
			_bg: bg,
		});
		inst.last_accessed = Instant::now();
		Ok(inst.server.clone())
	}

	/// Periodically drop instances idle past the timeout, aborting their threads.
	fn spawn_idle_cleanup(&self) {
		let instances = self.instances.clone();
		tokio::spawn(async move {
			let mut interval =
				tokio::time::interval(Duration::from_millis(INSTANCE_CLEANUP_INTERVAL_MS));
			loop {
				interval.tick().await;
				let mut guard = instances.lock().await;
				let before = guard.len();
				guard.retain(|_, inst| {
					inst.last_accessed.elapsed() <= Duration::from_millis(INSTANCE_IDLE_TIMEOUT_MS)
				});
				let removed = before - guard.len();
				if removed > 0 {
					debug!("Cleaned up {} idle repo instance(s)", removed);
				}
			}
		});
	}

	/// Serve over stdio (default MCP transport).
	pub async fn run_stdio(self) -> Result<()> {
		self.spawn_idle_cleanup();
		let transport = rmcp::transport::io::stdio();
		let service = self.serve(transport).await?;
		service.waiting().await?;
		Ok(())
	}

	/// Serve over Streamable HTTP. Each session clones the server, sharing the
	/// discovered repos and instance map.
	pub async fn run_http(self, bind_addr: &str) -> Result<()> {
		use hyper_util::rt::TokioIo;
		use hyper_util::service::TowerToHyperService;
		use rmcp::transport::streamable_http_server::{
			session::local::LocalSessionManager, StreamableHttpService,
		};

		self.spawn_idle_cleanup();

		let server = self.clone();
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
			let (stream, remote_addr) = listener.accept().await?;
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
	fn get_info(&self) -> ServerInfo {
		let capabilities = ServerCapabilities::builder().enable_tools().build();
		let instructions = format!(
			"Multi-repository Octocode MCP server. {} repositories are available ({}); \
			 every tool requires a `project` argument naming the target repository. \
			 Use 'semantic_search' for code/documentation searches and 'graphrag' \
			 (if enabled) for relationship queries.",
			self.repos.len(),
			self.project_list()
		);

		ServerInfo::new(capabilities)
			.with_server_info(
				Implementation::new("octocode-mcp", env!("CARGO_PKG_VERSION")).with_description(
					"Multi-repository semantic code search server with per-repo routing",
				),
			)
			.with_instructions(instructions)
	}

	async fn list_tools(
		&self,
		_request: Option<PaginatedRequestParams>,
		_context: RequestContext<RoleServer>,
	) -> Result<ListToolsResult, ErrorData> {
		Ok(ListToolsResult::with_all_items((*self.tools).clone()))
	}

	fn get_tool(&self, name: &str) -> Option<Tool> {
		self.tools.iter().find(|t| t.name == name).cloned()
	}

	async fn call_tool(
		&self,
		mut request: CallToolRequestParams,
		context: RequestContext<RoleServer>,
	) -> Result<CallToolResult, ErrorData> {
		// Pull the routing key out of the arguments.
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

		// Strip it so the inner tool sees only its own parameters.
		if let Some(args) = request.arguments.as_mut() {
			args.remove("project");
		}

		let server = self.get_server(&project).await?;
		server.dispatch_tool(request, context).await
	}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find git repositories in the immediate children of `root` (one level deep).
/// Each repo is keyed by its directory name.
fn discover_repos(root: &Path) -> Result<HashMap<String, PathBuf>> {
	let mut repos = HashMap::new();

	for entry in std::fs::read_dir(root)?.flatten() {
		let path = entry.path();
		if !path.is_dir() || !path.join(".git").exists() {
			continue;
		}
		if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
			repos.insert(name.to_string(), path);
		}
	}

	Ok(repos)
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
				if !required.iter().any(|v| v == "project") {
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
