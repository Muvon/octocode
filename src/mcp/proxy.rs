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

//! MCP Proxy Server — multi-repo router using rmcp transport.
//!
//! Routes HTTP requests by URL path (`/org/repo`) to per-repository
//! `McpServer` instances, each served via rmcp's `StreamableHttpService`.

use anyhow::Result;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use rmcp::transport::streamable_http_server::{
	session::local::LocalSessionManager, StreamableHttpService,
};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::Duration;
use tracing::debug;

use crate::config::Config;
use crate::mcp::logging::{init_mcp_logging, log_critical_anyhow_error};
use crate::mcp::server::McpServer;

const INSTANCE_CLEANUP_INTERVAL_MS: u64 = 300_000; // 5 minutes
const INSTANCE_IDLE_TIMEOUT_MS: u64 = 1_800_000; // 30 minutes

// ---------------------------------------------------------------------------
// Per-repo instance: wraps a StreamableHttpService backed by McpServer
// ---------------------------------------------------------------------------

type McpHttpService = StreamableHttpService<McpServer, LocalSessionManager>;

#[derive(Clone)]
struct ProxyMcpInstance {
	service: McpHttpService,
	last_accessed: Arc<Mutex<Instant>>,
}

impl ProxyMcpInstance {
	fn new(config: Config, working_directory: PathBuf) -> Result<Self> {
		let server = McpServer::new_for_proxy(config, working_directory)?;

		let service = StreamableHttpService::new(
			move || Ok(server.clone()),
			Arc::new(LocalSessionManager::default()),
			Default::default(),
		);

		Ok(Self {
			service,
			last_accessed: Arc::new(Mutex::new(Instant::now())),
		})
	}

	async fn touch(&self) {
		*self.last_accessed.lock().await = Instant::now();
	}

	async fn is_idle(&self) -> bool {
		let last_accessed = *self.last_accessed.lock().await;
		last_accessed.elapsed() > Duration::from_millis(INSTANCE_IDLE_TIMEOUT_MS)
	}
}

// ---------------------------------------------------------------------------
// McpProxyServer
// ---------------------------------------------------------------------------

/// MCP Proxy Server — manages per-repo MCP instances behind a single HTTP endpoint.
pub struct McpProxyServer {
	bind_addr: SocketAddr,
	root_path: PathBuf,
	debug: bool,
	auto_index: bool,
	instances: Arc<Mutex<HashMap<String, ProxyMcpInstance>>>,
	/// Active indexing tasks per repo. Prevents concurrent indexing of the same repo.
	indexing_tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

impl McpProxyServer {
	pub async fn new(
		bind_addr: SocketAddr,
		root_path: PathBuf,
		debug_mode: bool,
		auto_index: bool,
	) -> Result<Self> {
		init_mcp_logging(root_path.clone(), debug_mode)?;

		if debug_mode {
			println!("🔍 Initializing MCP Proxy Server...");
		}

		Ok(Self {
			bind_addr,
			root_path,
			debug: debug_mode,
			auto_index,
			instances: Arc::new(Mutex::new(HashMap::new())),
			indexing_tasks: Arc::new(Mutex::new(HashMap::new())),
		})
	}

	pub async fn run(&mut self) -> Result<()> {
		let listener = tokio::net::TcpListener::bind(&self.bind_addr)
			.await
			.map_err(|e| anyhow::anyhow!("Failed to bind to {}: {}", self.bind_addr, e))?;

		if self.debug {
			println!("🌐 MCP Proxy Server listening on {}", self.bind_addr);
			println!(
				"📂 Scanning for git repositories under: {}",
				self.root_path.display()
			);
		}

		self.discover_and_log_repositories().await?;

		// Startup auto-index queue
		if self.auto_index {
			let repositories = self.discover_repositories().await?;
			if !repositories.is_empty() {
				let cpu_count = std::thread::available_parallelism()
					.map(|n| n.get())
					.unwrap_or(4);
				let semaphore = Arc::new(Semaphore::new(cpu_count));

				println!(
					"🔄 Auto-index: queuing {} repositories (concurrency: {})",
					repositories.len(),
					cpu_count
				);

				for repo_path in repositories {
					let semaphore = semaphore.clone();
					let debug = self.debug;
					let relative_key = repo_path
						.strip_prefix(&self.root_path)
						.unwrap_or(&repo_path)
						.to_string_lossy()
						.to_string();
					let handle = tokio::spawn(async move {
						let _permit = match semaphore.acquire().await {
							Ok(permit) => permit,
							Err(_) => return,
						};
						if let Err(e) = run_indexing_for_repo(&repo_path, debug).await {
							debug!(
								"Startup auto-index failed for {}: {}",
								repo_path.display(),
								e
							);
						}
					});
					self.indexing_tasks
						.lock()
						.await
						.insert(relative_key, handle);
				}
			}
		}

		// Idle instance cleanup task
		let instances_for_cleanup = self.instances.clone();
		let tasks_for_cleanup = self.indexing_tasks.clone();
		tokio::spawn(async move {
			let mut interval =
				tokio::time::interval(Duration::from_millis(INSTANCE_CLEANUP_INTERVAL_MS));
			loop {
				interval.tick().await;
				cleanup_idle_instances(&instances_for_cleanup, &tasks_for_cleanup).await;
			}
		});

		println!(
			"✅ Proxy server ready! Send requests to http://{}/org/repo",
			self.bind_addr
		);

		// Shared state for the connection handler
		let instances = self.instances.clone();
		let root_path = self.root_path.clone();
		let debug = self.debug;
		let auto_index = self.auto_index;
		let indexing_tasks = self.indexing_tasks.clone();

		loop {
			match listener.accept().await {
				Ok((stream, remote_addr)) => {
					let instances = instances.clone();
					let root_path = root_path.clone();
					let indexing_tasks = indexing_tasks.clone();

					tokio::spawn(async move {
						let io = TokioIo::new(stream);

						let instances = instances.clone();
						let root_path = root_path.clone();
						let indexing_tasks = indexing_tasks.clone();

						let service = hyper::service::service_fn(move |req: Request<Incoming>| {
							let instances = instances.clone();
							let root_path = root_path.clone();
							let indexing_tasks = indexing_tasks.clone();
							async move {
								handle_request(
									req,
									instances,
									root_path,
									debug,
									auto_index,
									indexing_tasks,
								)
								.await
							}
						});

						if let Err(e) = hyper::server::conn::http1::Builder::new()
							.serve_connection(io, service)
							.await
						{
							debug!("Connection error from {}: {}", remote_addr, e);
						}
					});
				}
				Err(e) => {
					log_critical_anyhow_error(
						"Proxy server accept error",
						&anyhow::anyhow!("{}", e),
					);
					break;
				}
			}
		}

		Ok(())
	}

	async fn discover_and_log_repositories(&self) -> Result<()> {
		let repositories = self.discover_repositories().await?;

		if repositories.is_empty() {
			println!(
				"⚠️  No git repositories found under {}",
				self.root_path.display()
			);
			println!("💡 Create git repositories in subdirectories to enable proxy routing");
			println!("💡 Example: mkdir -p org/repo && cd org/repo && git init");
		} else {
			println!("📁 Found {} git repositories:", repositories.len());
			for repo_path in &repositories {
				let relative_path = repo_path
					.strip_prefix(&self.root_path)
					.unwrap_or(repo_path)
					.to_string_lossy();
				println!(
					"   📂 {} → http://{}/{}",
					repo_path.display(),
					self.bind_addr,
					relative_path
				);
			}
			println!("🔄 Repositories will be loaded on-demand when accessed via HTTP");
		}

		Ok(())
	}

	async fn discover_repositories(&self) -> Result<Vec<PathBuf>> {
		let mut repositories = Vec::new();
		find_git_repos_recursive(&self.root_path, &mut repositories)?;
		repositories.sort();
		Ok(repositories)
	}
}

// ---------------------------------------------------------------------------
// Request handling — routes by URL path to per-repo StreamableHttpService
// ---------------------------------------------------------------------------

async fn handle_request(
	req: Request<Incoming>,
	instances: Arc<Mutex<HashMap<String, ProxyMcpInstance>>>,
	root_path: PathBuf,
	debug: bool,
	auto_index: bool,
	indexing_tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
) -> Result<Response<http_body_util::combinators::BoxBody<Bytes, Infallible>>, Infallible> {
	// CORS preflight
	if req.method() == hyper::Method::OPTIONS {
		return Ok(cors_response(
			Response::builder()
				.status(204)
				.body(Full::new(Bytes::new()).boxed())
				.expect("valid response"),
		));
	}

	// Extract repo path from URL (e.g. "/org/repo" → "org/repo")
	let url_path = req.uri().path();
	let repo_key = url_path.trim_start_matches('/');

	if repo_key.is_empty() {
		return Ok(error_response(
			404,
			"Repository path not found in URL. Use format: POST /org/repo",
		));
	}

	if debug {
		debug!("Routing request to repository: {}", repo_key);
	}

	// Get or create the per-repo MCP service
	let instance = match get_or_create_instance(
		&instances,
		repo_key,
		&root_path,
		debug,
		auto_index,
		&indexing_tasks,
	)
	.await
	{
		Ok(inst) => inst,
		Err(e) => {
			debug!("Failed to get MCP instance for {}: {}", repo_key, e);
			return Ok(error_response(
				404,
				&format!("Repository not found: {}", repo_key),
			));
		}
	};

	instance.touch().await;

	// Forward the request to rmcp's StreamableHttpService
	let response = instance.service.clone().handle(req).await;
	Ok(cors_response(response))
}

fn cors_response<B>(mut response: Response<B>) -> Response<B> {
	let headers = response.headers_mut();
	headers.insert(
		hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN,
		"*".parse().unwrap(),
	);
	headers.insert(
		hyper::header::ACCESS_CONTROL_ALLOW_METHODS,
		"GET, POST, DELETE, OPTIONS".parse().unwrap(),
	);
	headers.insert(
		hyper::header::ACCESS_CONTROL_ALLOW_HEADERS,
		"Content-Type, Accept, Mcp-Session-Id, Last-Event-ID, Mcp-Protocol-Version"
			.parse()
			.unwrap(),
	);
	headers.insert(
		hyper::header::ACCESS_CONTROL_EXPOSE_HEADERS,
		"Mcp-Session-Id".parse().unwrap(),
	);
	response
}

fn error_response(
	status: u16,
	message: &str,
) -> Response<http_body_util::combinators::BoxBody<Bytes, Infallible>> {
	cors_response(
		Response::builder()
			.status(status)
			.header(hyper::header::CONTENT_TYPE, "text/plain")
			.body(Full::new(Bytes::from(message.to_string())).boxed())
			.expect("valid response"),
	)
}

// ---------------------------------------------------------------------------
// Instance management
// ---------------------------------------------------------------------------

async fn get_or_create_instance(
	instances: &Arc<Mutex<HashMap<String, ProxyMcpInstance>>>,
	repo_key: &str,
	root_path: &Path,
	debug: bool,
	auto_index: bool,
	indexing_tasks: &Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
) -> Result<ProxyMcpInstance> {
	let mut guard = instances.lock().await;

	if let Some(instance) = guard.get(repo_key) {
		debug!("Reusing existing MCP instance for: {}", repo_key);
		return Ok(instance.clone());
	}

	// Validate repository path
	let full_path = root_path.join(repo_key);
	if !full_path.is_dir() {
		return Err(anyhow::anyhow!(
			"Directory not found: {}",
			full_path.display()
		));
	}
	if !full_path.join(".git").exists() {
		return Err(anyhow::anyhow!(
			"Not a git repository: {}",
			full_path.display()
		));
	}

	println!("🚀 Bootstrapping MCP instance for repository: {}", repo_key);
	println!("   📂 Path: {}", full_path.display());

	let config = Config::load()?;
	let instance = ProxyMcpInstance::new(config, full_path.clone())?;

	guard.insert(repo_key.to_string(), instance.clone());

	// Spawn one-shot indexer — skip if startup queue is still running for this repo
	if auto_index {
		let mut tasks = indexing_tasks.lock().await;
		let already_running = tasks.get(repo_key).is_some_and(|h| !h.is_finished());

		if already_running {
			debug!(
				"Skipping on-connect indexer for {} — startup indexer still running",
				repo_key
			);
		} else {
			let repo_key_owned = repo_key.to_string();
			let handle = tokio::spawn(async move {
				if let Err(e) = run_indexing_for_repo(&full_path, debug).await {
					debug!("Auto-index failed for {}: {}", repo_key_owned, e);
				}
			});
			tasks.insert(repo_key.to_string(), handle);
		}
	}

	if debug {
		println!("✅ MCP instance ready for: {}", repo_key);
	}
	Ok(instance)
}

async fn cleanup_idle_instances(
	instances: &Arc<Mutex<HashMap<String, ProxyMcpInstance>>>,
	indexing_tasks: &Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
) {
	let mut instances_guard = instances.lock().await;
	let mut to_remove = Vec::new();

	for (repo_path, instance) in instances_guard.iter() {
		if instance.is_idle().await {
			to_remove.push(repo_path.clone());
		}
	}

	for repo_path in &to_remove {
		instances_guard.remove(repo_path);
	}

	drop(instances_guard);

	if !to_remove.is_empty() {
		let mut tasks_guard = indexing_tasks.lock().await;
		for repo_path in &to_remove {
			if let Some(handle) = tasks_guard.remove(repo_path) {
				if !handle.is_finished() {
					handle.abort();
					debug!("Aborted indexer for idle instance: {}", repo_path);
				}
			}
		}
	}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn find_git_repos_recursive(dir: &Path, repositories: &mut Vec<PathBuf>) -> Result<()> {
	if dir.join(".git").exists() {
		repositories.push(dir.to_path_buf());
		return Ok(());
	}

	if let Ok(read_dir) = std::fs::read_dir(dir) {
		for entry in read_dir.flatten() {
			let path = entry.path();
			if path.is_dir() {
				if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
					if name.starts_with('.')
						|| name == "node_modules"
						|| name == "target"
						|| name == "build"
					{
						continue;
					}
				}
				find_git_repos_recursive(&path, repositories)?;
			}
		}
	}

	Ok(())
}

/// Run indexing for a single repository. Safe for concurrent use across different repos.
async fn run_indexing_for_repo(repo_path: &Path, debug: bool) -> Result<()> {
	if debug {
		println!("📇 Auto-indexing: {}", repo_path.display());
	}

	let config = Config::load()?;
	let index_path = crate::storage::get_project_database_path(repo_path)?;
	crate::storage::ensure_project_storage_exists(repo_path)?;
	let store = crate::store::Store::new_with_path(index_path).await?;
	store.initialize_collections().await?;

	super::server::perform_indexing(&store, &config, repo_path, false).await
}
