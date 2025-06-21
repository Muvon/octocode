// Copyright 2025 Muvon Un Limited
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

use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::Duration;
use tracing::debug;

use crate::config::Config;
use crate::mcp::graphrag::GraphRagProvider;
use crate::mcp::logging::{
	init_mcp_logging, log_critical_anyhow_error, log_mcp_request, log_mcp_response,
};
use crate::mcp::memory::MemoryProvider;
use crate::mcp::semantic_code::SemanticCodeProvider;
use crate::mcp::types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};

// Reuse constants from server.rs for consistency
const MCP_MAX_REQUEST_SIZE: usize = 10_485_760; // 10MB maximum request size
const INSTANCE_CLEANUP_INTERVAL_MS: u64 = 300_000; // 5 minutes
const INSTANCE_IDLE_TIMEOUT_MS: u64 = 1_800_000; // 30 minutes

/// Lightweight MCP instance for a single repository
/// Reuses all the provider logic from the main MCP server
#[derive(Clone)]
struct ProxyMcpInstance {
	semantic_code: SemanticCodeProvider,
	graphrag: Option<GraphRagProvider>,
	memory: Option<MemoryProvider>,
	last_accessed: Arc<Mutex<Instant>>,
}

impl ProxyMcpInstance {
	async fn new(config: Config, working_directory: PathBuf, _debug: bool) -> Result<Self> {
		// Skip logging initialization in proxy mode since the main proxy server handles logging
		// Individual repository instances don't need separate logging as they're part of the same process
		// The `MCP_LOG_DIR` OnceLock can only be set once per process, so subsequent calls would fail

		// Reuse exact same provider initialization as McpServer::new
		let semantic_code = SemanticCodeProvider::new(config.clone(), working_directory.clone());
		let graphrag = GraphRagProvider::new(config.clone(), working_directory.clone());
		let memory = MemoryProvider::new(&config, working_directory.clone()).await;

		Ok(Self {
			semantic_code,
			graphrag,
			memory,
			last_accessed: Arc::new(Mutex::new(Instant::now())),
		})
	}

	async fn handle_request(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
		// Update last accessed time
		*self.last_accessed.lock().await = Instant::now();

		// Reuse exact same request handling logic from McpServer
		match request.method.as_str() {
			"initialize" => self.handle_initialize(request),
			"tools/list" => self.handle_tools_list(request),
			"tools/call" => self.handle_tools_call(request).await,
			"ping" => self.handle_ping(request),
			_ => JsonRpcResponse {
				jsonrpc: "2.0".to_string(),
				id: request.id.clone(),
				result: None,
				error: Some(JsonRpcError {
					code: -32601,
					message: "Method not found".to_string(),
					data: None,
				}),
			},
		}
	}

	// Reuse exact same handlers from McpServer - copy the logic to avoid complex refactoring
	fn handle_initialize(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
		JsonRpcResponse {
			jsonrpc: "2.0".to_string(),
			id: request.id.clone(),
			result: Some(serde_json::json!({
				"protocolVersion": "2024-11-05",
				"capabilities": {
					"tools": {
						"listChanged": false
					}
				},
				"serverInfo": {
					"name": "octocode-mcp-proxy",
					"version": "0.1.0",
					"description": "Multi-repository MCP proxy server with semantic search, memory, and GraphRAG"
				},
				"instructions": "This proxy provides MCP tools for multiple repositories. Access via URL path: /org/repo for repository at {root}/org/repo."
			})),
			error: None,
		}
	}

	fn handle_tools_list(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
		let mut tools = vec![
			SemanticCodeProvider::get_tool_definition(),
			SemanticCodeProvider::get_view_signatures_tool_definition(),
		];

		// Add memory tools if available
		if self.memory.is_some() {
			tools.extend(MemoryProvider::get_tool_definitions());
		}

		// Add GraphRAG tools if available
		if self.graphrag.is_some() {
			tools.push(GraphRagProvider::get_tool_definition());
		}

		JsonRpcResponse {
			jsonrpc: "2.0".to_string(),
			id: request.id.clone(),
			result: Some(serde_json::json!({
				"tools": tools
			})),
			error: None,
		}
	}

	async fn handle_tools_call(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
		let params = match &request.params {
			Some(params) => params,
			None => {
				return JsonRpcResponse {
					jsonrpc: "2.0".to_string(),
					id: request.id.clone(),
					result: None,
					error: Some(JsonRpcError {
						code: -32602,
						message: "Invalid params: missing parameters object".to_string(),
						data: Some(serde_json::json!({
							"details": "Tool calls require a 'params' object with 'name' and 'arguments' fields"
						})),
					}),
				};
			}
		};

		let tool_name = match params.get("name").and_then(|v| v.as_str()) {
			Some(name) => name,
			None => {
				return JsonRpcResponse {
					jsonrpc: "2.0".to_string(),
					id: request.id.clone(),
					result: None,
					error: Some(JsonRpcError {
						code: -32602,
						message: "Invalid params: missing tool name".to_string(),
						data: Some(serde_json::json!({
							"details": "Required field 'name' must be provided with the tool name to call"
						})),
					}),
				};
			}
		};

		let default_args = serde_json::json!({});
		let arguments = params.get("arguments").unwrap_or(&default_args);

		// Validate arguments size to prevent memory exhaustion
		if let Ok(args_str) = serde_json::to_string(arguments) {
			if args_str.len() > MCP_MAX_REQUEST_SIZE {
				return JsonRpcResponse {
					jsonrpc: "2.0".to_string(),
					id: request.id.clone(),
					result: None,
					error: Some(JsonRpcError {
						code: -32602,
						message: "Tool arguments too large".to_string(),
						data: Some(serde_json::json!({
							"max_size": MCP_MAX_REQUEST_SIZE,
							"actual_size": args_str.len()
						})),
					}),
				};
			}
		}

		// Execute tools - reuse exact same logic as McpServer
		let result = match tool_name {
			"semantic_search" => self.semantic_code.execute_search(arguments).await,
			"view_signatures" => self.semantic_code.execute_view_signatures(arguments).await,
			"graphrag_search" => match &self.graphrag {
				Some(provider) => provider.execute_search(arguments).await,
				None => Err(anyhow::anyhow!("GraphRAG is not enabled in the current configuration. Please enable GraphRAG in octocode.toml to use relationship-aware search.")),
			},
			"memorize" => match &self.memory {
				Some(provider) => provider.execute_memorize(arguments).await,
				None => Err(anyhow::anyhow!("Memory system is not available")),
			},
			"remember" => match &self.memory {
				Some(provider) => provider.execute_remember(arguments).await,
				None => Err(anyhow::anyhow!("Memory system is not available")),
			},
			"forget" => match &self.memory {
				Some(provider) => provider.execute_forget(arguments).await,
				None => Err(anyhow::anyhow!("Memory system is not available")),
			},
			_ => {
				let available_tools = format!("semantic_search, view_signatures{}{}",
					if self.graphrag.is_some() { ", graphrag_search" } else { "" },
					if self.memory.is_some() { ", memorize, remember, forget" } else { "" }
				);
				Err(anyhow::anyhow!("Unknown tool '{}'. Available tools: {}", tool_name, available_tools))
			}
		};

		match result {
			Ok(content) => JsonRpcResponse {
				jsonrpc: "2.0".to_string(),
				id: request.id.clone(),
				result: Some(serde_json::json!({
					"content": [{
						"type": "text",
						"text": content
					}]
				})),
				error: None,
			},
			Err(e) => {
				let error_message = e.to_string();
				let error_code =
					if error_message.contains("Missing") || error_message.contains("Invalid") {
						-32602 // Invalid params
					} else if error_message.contains("not enabled")
						|| error_message.contains("not available")
					{
						-32601 // Method not found (feature not available)
					} else {
						-32603 // Internal error
					};

				JsonRpcResponse {
					jsonrpc: "2.0".to_string(),
					id: request.id.clone(),
					result: None,
					error: Some(JsonRpcError {
						code: error_code,
						message: format!("Tool execution failed: {}", error_message),
						data: Some(serde_json::json!({
							"tool": tool_name,
							"error_type": match error_code {
								-32602 => "invalid_params",
								-32601 => "feature_unavailable",
								_ => "execution_error"
							}
						})),
					}),
				}
			}
		}
	}

	fn handle_ping(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
		JsonRpcResponse {
			jsonrpc: "2.0".to_string(),
			id: request.id.clone(),
			result: Some(serde_json::json!({})),
			error: None,
		}
	}

	async fn is_idle(&self) -> bool {
		let last_accessed = *self.last_accessed.lock().await;
		last_accessed.elapsed() > Duration::from_millis(INSTANCE_IDLE_TIMEOUT_MS)
	}
}

/// MCP Proxy Server - manages multiple MCP instances for different repositories
pub struct McpProxyServer {
	bind_addr: SocketAddr,
	root_path: PathBuf,
	debug: bool,
	instances: Arc<Mutex<HashMap<String, ProxyMcpInstance>>>,
}

impl McpProxyServer {
	pub async fn new(bind_addr: SocketAddr, root_path: PathBuf, debug_mode: bool) -> Result<Self> {
		// Initialize logging for the proxy server
		init_mcp_logging(root_path.clone(), debug_mode)?;

		// Console output only in debug mode to maintain protocol compliance
		if debug_mode {
			println!("🔍 Initializing MCP Proxy Server...");
		}

		Ok(Self {
			bind_addr,
			root_path,
			debug: debug_mode,
			instances: Arc::new(Mutex::new(HashMap::new())),
		})
	}

	pub async fn run(&mut self) -> Result<()> {
		let listener = TcpListener::bind(&self.bind_addr)
			.await
			.map_err(|e| anyhow::anyhow!("Failed to bind to {}: {}", self.bind_addr, e))?;

		if self.debug {
			println!("🌐 MCP Proxy Server listening on {}", self.bind_addr);
			println!(
				"📂 Scanning for git repositories under: {}",
				self.root_path.display()
			);
		}

		// Discover and log available repositories
		self.discover_and_log_repositories().await?;

		// Start cleanup task for idle instances
		let instances_for_cleanup = self.instances.clone();
		tokio::spawn(async move {
			let mut interval =
				tokio::time::interval(Duration::from_millis(INSTANCE_CLEANUP_INTERVAL_MS));
			loop {
				interval.tick().await;
				Self::cleanup_idle_instances(&instances_for_cleanup).await;
			}
		});

		println!(
			"✅ Proxy server ready! Send requests to http://{}/org/repo",
			self.bind_addr
		);

		// Accept connections
		loop {
			match listener.accept().await {
				Ok((stream, addr)) => {
					let instances = self.instances.clone();
					let root_path = self.root_path.clone();
					let debug = self.debug;

					tokio::spawn(async move {
						if let Err(e) =
							Self::handle_connection(stream, instances, root_path, debug).await
						{
							debug!("Connection error from {}: {}", addr, e);
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

		// Use std::fs for sync operations since we're doing simple directory traversal
		Self::find_git_repos_recursive(&self.root_path, &mut repositories)?;

		repositories.sort();
		Ok(repositories)
	}

	fn find_git_repos_recursive(dir: &Path, repositories: &mut Vec<PathBuf>) -> Result<()> {
		// Check if current directory is a git repo
		if dir.join(".git").exists() {
			repositories.push(dir.to_path_buf());
			return Ok(()); // Don't recurse into git repos
		}

		// Recursively check subdirectories
		if let Ok(read_dir) = std::fs::read_dir(dir) {
			for entry in read_dir.flatten() {
				let path = entry.path();
				if path.is_dir() {
					// Skip hidden directories and common non-repo directories
					if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
						if name.starts_with('.')
							|| name == "node_modules"
							|| name == "target" || name == "build"
						{
							continue;
						}
					}
					Self::find_git_repos_recursive(&path, repositories)?;
				}
			}
		}

		Ok(())
	}

	async fn handle_connection(
		mut stream: TcpStream,
		instances: Arc<Mutex<HashMap<String, ProxyMcpInstance>>>,
		root_path: PathBuf,
		debug: bool,
	) -> Result<()> {
		let mut buffer = vec![0; 8192];
		let bytes_read = stream.read(&mut buffer).await?;

		if bytes_read == 0 {
			return Ok(());
		}

		let request_str = String::from_utf8_lossy(&buffer[..bytes_read]);

		if debug {
			println!("📥 Raw HTTP request:\n{}", request_str);
		}

		// Parse HTTP request
		let mut lines = request_str.lines();
		let request_line = lines.next().unwrap_or("");

		// Extract repository path from URL
		let repo_path = match Self::extract_repo_path(request_line) {
			Some(path) => {
				if debug {
					println!("🔀 Routing request to repository: {}", path);
				}
				path
			}
			None => {
				if debug {
					println!(
						"❌ Invalid request - no repository path found in: {}",
						request_line
					);
				}
				Self::send_http_error(
					&mut stream,
					404,
					"Repository path not found in URL. Use format: POST /org/repo",
				)
				.await?;
				return Ok(());
			}
		};

		// Find content length and extract JSON body
		let mut content_length = 0;
		let mut body_start = 0;
		let mut header_end_found = false;

		// Find headers and body boundary
		let header_body_split = request_str
			.find("\r\n\r\n")
			.or_else(|| request_str.find("\n\n"));

		if let Some(split_pos) = header_body_split {
			// Parse headers to find content-length
			let headers_part = &request_str[..split_pos];
			for line in headers_part.lines() {
				if line.to_lowercase().starts_with("content-length:") {
					if let Some(len_str) = line.split(':').nth(1) {
						content_length = len_str.trim().parse().unwrap_or(0);
					}
				}
			}

			// Calculate body start position in bytes
			if request_str.contains("\r\n\r\n") {
				body_start = split_pos + 4; // Skip \r\n\r\n
			} else {
				body_start = split_pos + 2; // Skip \n\n
			}
			header_end_found = true;
		}

		if !header_end_found {
			Self::send_http_error(&mut stream, 400, "Invalid HTTP request format").await?;
			return Ok(());
		}

		let json_body = if content_length > 0 && body_start < bytes_read {
			let body_bytes =
				&buffer[body_start..std::cmp::min(body_start + content_length, bytes_read)];
			String::from_utf8_lossy(body_bytes).to_string()
		} else {
			Self::send_http_error(&mut stream, 400, "Missing or invalid request body").await?;
			return Ok(());
		};

		if debug {
			println!("📄 Extracted JSON body: {}", json_body);
		}

		// Parse JSON-RPC request
		let request: JsonRpcRequest = match serde_json::from_str(&json_body) {
			Ok(req) => req,
			Err(e) => {
				if debug {
					println!("❌ Failed to parse JSON-RPC request: {}", e);
				}
				Self::send_http_error(&mut stream, 400, "Invalid JSON-RPC request").await?;
				return Ok(());
			}
		};

		// Log the request
		log_mcp_request(
			&request.method,
			request.params.as_ref(),
			request.id.as_ref(),
		);

		let start_time = std::time::Instant::now();
		let request_id = request.id.clone();
		let request_method = request.method.clone();

		// Get or create MCP instance for this repository
		let instance =
			match Self::get_or_create_instance(&instances, &repo_path, &root_path, debug).await {
				Ok(instance) => instance,
				Err(e) => {
					debug!("Failed to get MCP instance for {}: {}", repo_path, e);
					Self::send_http_error(
						&mut stream,
						404,
						&format!("Repository not found: {}", repo_path),
					)
					.await?;
					return Ok(());
				}
			};

		// Handle the request
		let response = instance.handle_request(&request).await;

		// Log the response
		let duration_ms = start_time.elapsed().as_millis() as u64;
		log_mcp_response(
			&request_method,
			response.error.is_none(),
			request_id.as_ref(),
			Some(duration_ms),
		);

		// Send HTTP response
		Self::send_http_response(&mut stream, &response).await
	}

	fn extract_repo_path(request_line: &str) -> Option<String> {
		// Extract path from "POST /org/repo HTTP/1.1" or "POST /org/repo/subpath HTTP/1.1"
		let parts: Vec<&str> = request_line.split_whitespace().collect();
		if parts.len() >= 2 && parts[0] == "POST" {
			let url_path = parts[1];
			if url_path.starts_with('/') && url_path.len() > 1 {
				// Remove leading slash and return the repository path
				return Some(url_path[1..].to_string());
			}
		}
		None
	}

	async fn get_or_create_instance(
		instances: &Arc<Mutex<HashMap<String, ProxyMcpInstance>>>,
		repo_path: &str,
		root_path: &Path,
		debug: bool,
	) -> Result<ProxyMcpInstance> {
		let mut instances_guard = instances.lock().await;

		// Return existing instance if found
		if let Some(instance) = instances_guard.get(repo_path) {
			debug!("Reusing existing MCP instance for: {}", repo_path);
			return Ok(instance.clone());
		}

		// Validate repository path
		let full_path = root_path.join(repo_path);
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

		// Create new instance
		println!(
			"🚀 Bootstrapping MCP instance for repository: {}",
			repo_path
		);
		println!("   📂 Path: {}", full_path.display());

		let config = Config::load()?;
		let instance = ProxyMcpInstance::new(config, full_path, debug).await?;

		// Store and return
		instances_guard.insert(repo_path.to_string(), instance.clone());

		if debug {
			println!("✅ MCP instance ready for: {}", repo_path);
		}
		Ok(instance)
	}

	async fn cleanup_idle_instances(instances: &Arc<Mutex<HashMap<String, ProxyMcpInstance>>>) {
		let mut instances_guard = instances.lock().await;
		let mut to_remove = Vec::new();

		for (repo_path, instance) in instances_guard.iter() {
			if instance.is_idle().await {
				to_remove.push(repo_path.clone());
			}
		}

		// Note: cleanup logging removed to maintain protocol compliance
		// Only log to structured logging, not console

		for repo_path in to_remove {
			instances_guard.remove(&repo_path);
		}
	}

	async fn send_http_error(stream: &mut TcpStream, status: u16, message: &str) -> Result<()> {
		let status_text = match status {
			400 => "Bad Request",
			404 => "Not Found",
			500 => "Internal Server Error",
			_ => "Error",
		};

		let response = format!(
			"HTTP/1.1 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
			status, status_text, message.len(), message
		);

		stream.write_all(response.as_bytes()).await?;
		Ok(())
	}

	async fn send_http_response(stream: &mut TcpStream, response: &JsonRpcResponse) -> Result<()> {
		let json_response = serde_json::to_string(response)?;

		let http_response = format!(
			"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\n\r\n{}",
			json_response.len(),
			json_response
		);

		stream.write_all(http_response.as_bytes()).await?;
		Ok(())
	}
}
