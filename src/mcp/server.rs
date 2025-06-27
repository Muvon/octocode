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
use serde_json::json;
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};
use tracing::{debug, info, trace, warn};

use crate::config::Config;
use crate::indexer;
use crate::mcp::graphrag::GraphRagProvider;
use crate::mcp::logging::{
	init_mcp_logging, log_critical_anyhow_error, log_critical_error, log_indexing_operation,
	log_mcp_request, log_mcp_response, log_watcher_event,
};
use crate::mcp::memory::MemoryProvider;
use crate::mcp::semantic_code::SemanticCodeProvider;
use crate::mcp::types::{parse_mcp_error, JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpError};
use crate::state;
use crate::store::Store;
use crate::watcher_config::{
	IgnorePatterns, DEFAULT_ADDITIONAL_DELAY_MS, MCP_DEFAULT_DEBOUNCE_MS, MIN_DEBOUNCE_MS,
};

// Configurable debounce settings (code-only configuration for now)
//
// You can modify these constants to tune the MCP server behavior:
// - MCP_DEBOUNCE_MS: How long to wait after the last file change before triggering reindex
// - MCP_MAX_PENDING_EVENTS: Maximum number of file events to queue (prevents memory issues)
// - MCP_INDEX_TIMEOUT_MS: Maximum time to wait for indexing to complete before timing out
// - MCP_ENABLE_VERBOSE_EVENTS: Whether to log individual file events (useful for debugging)
// - MCP_MAX_REQUEST_SIZE: Maximum size of incoming JSON-RPC requests (prevents memory exhaustion)
// - MCP_IO_TIMEOUT_MS: Timeout for individual stdin/stdout operations (prevents hanging on broken pipes, NOT for server lifecycle)
//
const MCP_DEBOUNCE_MS: u64 = MCP_DEFAULT_DEBOUNCE_MS; // 2000ms = 2 seconds
const MCP_MAX_PENDING_EVENTS: usize = 100;
const MCP_INDEX_TIMEOUT_MS: u64 = 300_000; // 5 minutes
const MCP_ENABLE_VERBOSE_EVENTS: bool = false; // Set to true for detailed event logging
const MCP_MAX_REQUEST_SIZE: usize = 10_485_760; // 10MB maximum request size
const MCP_IO_TIMEOUT_MS: u64 = 30_000; // 30 seconds for individual I/O operations (NOT for server lifecycle)

/// MCP Server implementation with modular tool providers
pub struct McpServer {
	semantic_code: SemanticCodeProvider,
	graphrag: Option<GraphRagProvider>,
	memory: Option<MemoryProvider>,
	lsp: Option<Arc<Mutex<crate::mcp::lsp::LspProvider>>>,
	debug: bool,
	working_directory: std::path::PathBuf,
	no_git: bool,
	watcher_handle: Option<tokio::task::JoinHandle<()>>,
	index_handle: Option<tokio::task::JoinHandle<()>>,
	indexing_in_progress: Arc<AtomicBool>,
	store: Store,
	config: Config,
	index_rx: Option<mpsc::Receiver<()>>,
}

impl McpServer {
	pub async fn new(
		config: Config,
		debug: bool,
		working_directory: std::path::PathBuf,
		no_git: bool,
		lsp_command: Option<String>,
	) -> Result<Self> {
		// Change to the working directory at server startup
		std::env::set_current_dir(&working_directory).map_err(|e| {
			anyhow::anyhow!(
				"Failed to change to working directory '{}': {}",
				working_directory.display(),
				e
			)
		})?;

		// Initialize the store for the MCP server
		let store = Store::new().await?;
		store.initialize_collections().await?;

		// Initialize logging
		init_mcp_logging(working_directory.clone(), debug)?;

		let semantic_code = SemanticCodeProvider::new(config.clone(), working_directory.clone());
		let graphrag = GraphRagProvider::new(config.clone(), working_directory.clone());
		let memory = MemoryProvider::new(&config, working_directory.clone()).await;

		// Initialize LSP provider if command is provided (lazy initialization)
		let lsp = if let Some(command) = lsp_command {
			tracing::info!(
				"LSP provider will be initialized lazily with command: {}",
				command
			);
			let provider = Arc::new(Mutex::new(crate::mcp::lsp::LspProvider::new(
				working_directory.clone(),
				command,
			)));

			// Start LSP initialization in background (non-blocking)
			let provider_clone = provider.clone();
			tokio::spawn(async move {
				let mut provider_guard = provider_clone.lock().await;
				if let Err(e) = provider_guard.start_initialization().await {
					tracing::warn!("LSP initialization failed: {}", e);
				}
			});

			Some(provider)
		} else {
			None
		};

		Ok(Self {
			semantic_code,
			graphrag,
			memory,
			lsp,
			debug,
			working_directory,
			no_git,
			watcher_handle: None,
			index_handle: None,
			indexing_in_progress: Arc::new(AtomicBool::new(false)),
			store,
			config,
			index_rx: None,
		})
	}

	pub async fn run(&mut self) -> Result<()> {
		// Set up panic handler to prevent server crashes from tool execution
		let original_hook = panic::take_hook();
		panic::set_hook(Box::new(move |panic_info| {
			log_critical_anyhow_error("Panic in MCP server", &anyhow::anyhow!("{}", panic_info));
			// Call original hook for debugging
			original_hook(panic_info);
		}));

		// Start the file watcher as a completely independent background task
		self.start_watcher().await?;

		// Log server startup details using structured logging (no console output for MCP protocol compliance)
		info!(
			debug_mode = self.debug,
			debounce_ms = MCP_DEBOUNCE_MS,
			timeout_ms = MCP_INDEX_TIMEOUT_MS,
			max_events = MCP_MAX_PENDING_EVENTS,
			max_request_size_mb = MCP_MAX_REQUEST_SIZE / 1_048_576,
			io_timeout_ms = MCP_IO_TIMEOUT_MS,
			"MCP Server started"
		);

		// Get the index receiver for handling indexing requests
		let mut index_rx = self.index_rx.take().unwrap();

		// Handle MCP protocol communication (stdin/stdout) with error resilience
		// This runs independently of file watching and indexing
		let stdin = tokio::io::stdin();
		let stdout = tokio::io::stdout();
		let mut reader = BufReader::new(stdin);
		let mut writer = stdout;

		let mut line = String::with_capacity(1024); // Pre-allocate reasonable buffer
		let mut consecutive_errors = 0u32;
		const MAX_CONSECUTIVE_ERRORS: u32 = 10;

		loop {
			line.clear();

			tokio::select! {
				// Handle MCP protocol messages from stdin with timeout and error recovery
				result = tokio::time::timeout(
					Duration::from_millis(MCP_IO_TIMEOUT_MS),
					reader.read_line(&mut line)
				) => {
					match result {
						Ok(Ok(0)) => {
							// EOF reached - normal shutdown
							debug!("MCP Server: EOF received, shutting down gracefully");
							break;
						}
						Ok(Ok(bytes_read)) => {
							// Check for oversized requests to prevent memory exhaustion
							if bytes_read > MCP_MAX_REQUEST_SIZE {
								log_critical_anyhow_error(
									"Request size limit exceeded",
									&anyhow::anyhow!("Request size {} exceeds limit {}", bytes_read, MCP_MAX_REQUEST_SIZE)
								);

								// Send error response for oversized request
								if let Err(e) = self.send_error_response(
									&mut writer,
									None,
									-32700,
									"Request too large",
									Some(json!({"max_size": MCP_MAX_REQUEST_SIZE}))
								).await {
									log_critical_anyhow_error("Failed to send error response", &e);
								}
								continue;
							}

							// Process the request with panic recovery
							match self.handle_request_safe(&line).await {
								Ok(Some(response)) => {
									// Send response with error handling
									if let Err(e) = self.send_response(&mut writer, &response).await {
										log_critical_anyhow_error("Failed to send response", &e);
										consecutive_errors += 1;
										if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
											log_critical_anyhow_error(
												"Too many consecutive errors",
												&anyhow::anyhow!("Shutting down after {} consecutive errors", consecutive_errors)
											);
											break;
										}
									} else {
										consecutive_errors = 0; // Reset on successful send
									}
								}
								Ok(None) => {
									// No response needed (e.g., empty request)
									consecutive_errors = 0;
								}
								Err(e) => {
									log_critical_anyhow_error("Request handling failed", &e);
									consecutive_errors += 1;

									// Try to send error response
									if let Err(send_err) = self.send_error_response(
										&mut writer,
										None,
										-32603,
										"Internal server error",
										Some(json!({"error": e.to_string()}))
									).await {
										log_critical_anyhow_error("Failed to send error response", &send_err);
									}

									if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
										log_critical_anyhow_error(
											"Too many consecutive errors",
											&anyhow::anyhow!("Shutting down after {} consecutive errors", consecutive_errors)
										);
										break;
									}
								}
							}
						}
						Ok(Err(e)) => {
							// I/O error reading from stdin
							if self.is_broken_pipe_error(&e) {
								debug!("MCP Server: Broken pipe detected, shutting down gracefully");
								break;
							} else {
								log_critical_error("Error reading from stdin", &e);
								consecutive_errors += 1;
								if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
									break;
								}
								// Brief delay before retrying
								tokio::time::sleep(Duration::from_millis(100)).await;
							}
						}
				Err(_) => {
					// Timeout on stdin read - this is normal when no client requests are pending
					// MCP servers should wait indefinitely for client requests, not terminate on timeouts
					trace!("MCP Server: Timeout reading from stdin (normal - waiting for client requests)");
					// Do NOT increment consecutive_errors for timeouts - this is expected behavior
					// Reset consecutive_errors since timeout is not an actual error
					consecutive_errors = 0;
				}
					}
				}

				// Handle indexing requests from file watcher (runs independently)
				Some(_) = index_rx.recv() => {
					debug!("Processing index request");

					// Additional delay to ensure all file operations are complete
					sleep(Duration::from_millis(DEFAULT_ADDITIONAL_DELAY_MS)).await;

					// Perform direct indexing with timeout protection
					let indexing_result = tokio::time::timeout(
						Duration::from_millis(MCP_INDEX_TIMEOUT_MS),
						perform_indexing(&self.store, &self.config, &self.working_directory, self.no_git)
					).await;

					match indexing_result {
						Ok(Ok(())) => {
							info!("Reindex completed successfully");

							// Update LSP with changed files if LSP is enabled
							if let Some(ref lsp_provider) = self.lsp {
								let mut lsp_guard = lsp_provider.lock().await;
								if let Err(e) = Self::update_lsp_after_indexing(&mut lsp_guard, &self.working_directory).await {
									debug!("LSP update after indexing failed: {}", e);
								}
							}
						}
						Ok(Err(e)) => {
							log_critical_anyhow_error("Reindex error", &e);
						}
						Err(_) => {
							log_critical_anyhow_error(
								"Reindex timeout",
								&anyhow::anyhow!("Reindex timed out after {}ms", MCP_INDEX_TIMEOUT_MS)
							);
						}
					}

					// Always reset the indexing flag, even on error/timeout
					self.indexing_in_progress.store(false, Ordering::SeqCst);
				}
			}
		}

		// Cleanup: abort background tasks
		if let Some(handle) = self.watcher_handle.take() {
			handle.abort();
		}
		if let Some(handle) = self.index_handle.take() {
			handle.abort();
		}

		debug!("MCP Server stopped");

		Ok(())
	}

	/// Run MCP server over HTTP instead of stdin/stdout
	pub async fn run_http(&mut self, bind_addr: &str) -> Result<()> {
		// Set up panic handler to prevent server crashes from tool execution
		let original_hook = panic::take_hook();
		panic::set_hook(Box::new(move |panic_info| {
			log_critical_anyhow_error("Panic in MCP server", &anyhow::anyhow!("{}", panic_info));
			// Call original hook for debugging
			original_hook(panic_info);
		}));

		// Start the file watcher as a completely independent background task
		self.start_watcher().await?;

		// Parse bind address
		let addr = bind_addr
			.parse::<std::net::SocketAddr>()
			.map_err(|e| anyhow::anyhow!("Invalid bind address '{}': {}", bind_addr, e))?;

		// Log server startup details
		info!(
			debug_mode = self.debug,
			bind_address = %addr,
			debounce_ms = MCP_DEBOUNCE_MS,
			timeout_ms = MCP_INDEX_TIMEOUT_MS,
			max_events = MCP_MAX_PENDING_EVENTS,
			max_request_size_mb = MCP_MAX_REQUEST_SIZE / 1_048_576,
			io_timeout_ms = MCP_IO_TIMEOUT_MS,
			"MCP Server started in HTTP mode"
		);

		// Get the index receiver for handling indexing requests
		let mut index_rx = self.index_rx.take().unwrap();

		// Create shared server state for HTTP handlers
		let server_state = Arc::new(Mutex::new(HttpServerState {
			semantic_code: self.semantic_code.clone(),
			graphrag: self.graphrag.clone(),
			memory: self.memory.clone(),
			lsp: self.lsp.clone(),
		}));

		// Start HTTP server
		let listener = TcpListener::bind(&addr)
			.await
			.map_err(|e| anyhow::anyhow!("Failed to bind to {}: {}", addr, e))?;

		info!("MCP HTTP server listening on {}", addr);

		// Clone state for the server task
		let state_for_server = server_state.clone();
		let mut server_handle = tokio::spawn(async move {
			loop {
				match listener.accept().await {
					Ok((stream, addr)) => {
						let state = state_for_server.clone();
						tokio::spawn(async move {
							if let Err(e) = handle_http_connection(stream, state).await {
								debug!("HTTP connection error from {}: {}", addr, e);
							}
						});
					}
					Err(e) => {
						log_critical_anyhow_error(
							"HTTP server accept error",
							&anyhow::anyhow!("{}", e),
						);
						break;
					}
				}
			}
		});

		// Handle indexing requests from file watcher (runs independently)
		loop {
			tokio::select! {
				Some(_) = index_rx.recv() => {
					debug!("Processing index request");

					// Additional delay to ensure all file operations are complete
					sleep(Duration::from_millis(DEFAULT_ADDITIONAL_DELAY_MS)).await;

					// Perform direct indexing with timeout protection
					let indexing_result = tokio::time::timeout(
						Duration::from_millis(MCP_INDEX_TIMEOUT_MS),
						perform_indexing(&self.store, &self.config, &self.working_directory, self.no_git)
					).await;

					match indexing_result {
						Ok(Ok(())) => {
							info!("Reindex completed successfully");

							// Update LSP with changed files if LSP is enabled
							if let Some(ref lsp_provider) = self.lsp {
								let mut lsp_guard = lsp_provider.lock().await;
								if let Err(e) = Self::update_lsp_after_indexing(&mut lsp_guard, &self.working_directory).await {
									debug!("LSP update after indexing failed: {}", e);
								}
							}
						}
						Ok(Err(e)) => {
							log_critical_anyhow_error("Reindex error", &e);
						}
						Err(_) => {
							log_critical_anyhow_error(
								"Reindex timeout",
								&anyhow::anyhow!("Reindex timed out after {}ms", MCP_INDEX_TIMEOUT_MS)
							);
						}
					}

					// Always reset the indexing flag, even on error/timeout
					self.indexing_in_progress.store(false, Ordering::SeqCst);
				}

				// Check if HTTP server is still running
				_ = &mut server_handle => {
					warn!("HTTP server task completed unexpectedly");
					break;
				}
			}
		}

		// Cleanup: abort background tasks
		if let Some(handle) = self.watcher_handle.take() {
			handle.abort();
		}
		if let Some(handle) = self.index_handle.take() {
			handle.abort();
		}
		server_handle.abort();

		debug!("MCP HTTP Server stopped");

		Ok(())
	}

	async fn start_watcher(&mut self) -> Result<()> {
		let (file_tx, file_rx) = mpsc::channel(MCP_MAX_PENDING_EVENTS);
		let (index_tx, index_rx) = mpsc::channel(10);
		let working_dir = self.working_directory.clone();
		let debug = self.debug;

		// Start file watcher in background
		let watcher_handle = tokio::spawn(async move {
			if let Err(e) = run_watcher(file_tx, working_dir, debug).await {
				log_critical_anyhow_error("Watcher error", &e);
			}
		});

		// Start improved debouncing handler that properly accumulates events
		let indexing_in_progress = self.indexing_in_progress.clone();
		let debug_mode = self.debug;
		let index_handle = tokio::spawn(async move {
			let mut file_rx = file_rx;
			let mut last_event_time = None::<Instant>;
			let mut pending_events = 0u32;

			loop {
				// Wait for either a file event or timeout
				let timeout_duration = Duration::from_millis(MCP_DEBOUNCE_MS);

				tokio::select! {
					// New file event received
					event_result = file_rx.recv() => {
						match event_result {
						Some(_) => {
							pending_events += 1;
							last_event_time = Some(Instant::now());

							log_watcher_event("file_change", None, pending_events as usize);
						}
							None => {
								debug!("File watcher channel closed, stopping debouncer");
								break;
							}
						}
					}

					// Debounce timeout - check if we should trigger indexing
					_ = sleep(timeout_duration), if last_event_time.is_some() => {
						if let Some(last_time) = last_event_time {
							// Check if enough time has passed since the last event
							if last_time.elapsed() >= timeout_duration && pending_events > 0 {
								// Try to acquire indexing lock
								if indexing_in_progress
									.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
									.is_ok()
								{
									if debug_mode {
										debug!(
							pending_events = pending_events,
							"Debounce period completed, requesting reindex"
						);
									}

									// Log the debounce trigger
								log_watcher_event("debounce_trigger", None, pending_events as usize);

								// Send indexing request to main loop
									if (index_tx.send(()).await).is_err() {
										if debug_mode {
											debug!("Failed to send index request - server may be shutting down");
										}
										indexing_in_progress.store(false, Ordering::SeqCst);
										break;
									}

									// Reset counters
									pending_events = 0;
									last_event_time = None;
								} else if debug_mode {
									debug!("Indexing already in progress, will retry after current indexing completes");
									// Don't reset counters, will retry later
								}
							}
						}
					}
				}
			}
		});

		// Store the index receiver for handling in the main loop
		self.index_rx = Some(index_rx);
		self.watcher_handle = Some(watcher_handle);
		self.index_handle = Some(index_handle);
		Ok(())
	}

	/// Safe request handling with comprehensive error recovery
	async fn handle_request_safe(&mut self, line: &str) -> Result<Option<JsonRpcResponse>> {
		let line = line.trim();
		if line.is_empty() {
			return Ok(None);
		}

		// Ensure clean UTF-8 by using lossy conversion to handle any potential issues
		let clean_line = String::from_utf8_lossy(line.as_bytes()).to_string();
		let line = clean_line.as_str();

		// Parse request with enhanced error handling
		let parsed_request: Result<JsonRpcRequest, _> =
			panic::catch_unwind(|| serde_json::from_str(line)).unwrap_or_else(|_| {
				Err(serde_json::Error::io(std::io::Error::new(
					std::io::ErrorKind::InvalidData,
					"JSON parsing panicked",
				)))
			});

		let request: JsonRpcRequest = match parsed_request {
			Ok(req) => {
				// Log the request with proper method and ID
				log_mcp_request(&req.method, req.params.as_ref(), req.id.as_ref());
				req
			}
			Err(e) => {
				log_critical_error("Request parsing", &e);
				return Ok(Some(JsonRpcResponse {
					jsonrpc: "2.0".to_string(),
					id: None,
					result: None,
					error: Some(JsonRpcError {
						code: -32700,
						message: format!("Parse error: {}", e),
						data: None,
					}),
				}));
			}
		};

		let start_time = std::time::Instant::now();
		let request_id = request.id.clone();
		let request_id_for_error = request_id.clone(); // Clone for error handling
		let request_method = request.method.clone(); // Clone for error handling

		// Execute request with comprehensive panic recovery (timeout control left to external MCP client)
		let response = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
			// Create a new async runtime for the panic-safe execution
			// Note: This is a workaround since we can't easily catch panics in async code
			tokio::task::block_in_place(|| {
				tokio::runtime::Handle::current().block_on(async {
					// Execute request without internal timeout - let external MCP client control timeouts
					match request.method.as_str() {
						"initialize" => self.handle_initialize(&request).await,
						"tools/list" => self.handle_tools_list(&request).await,
						"tools/call" => self.handle_tools_call(&request).await,
						"ping" => self.handle_ping(&request).await,
						_ => JsonRpcResponse {
							jsonrpc: "2.0".to_string(),
							id: request.id,
							result: None,
							error: Some(JsonRpcError {
								code: -32601,
								message: "Method not found".to_string(),
								data: Some(json!({
									"method": request.method,
									"available_methods": ["initialize", "tools/list", "tools/call", "ping"]
								})),
							}),
						},
					}
				})
			})
		})) {
			Ok(response) => response,
			Err(_) => {
				log_critical_anyhow_error(
					"Request handler panicked",
					&anyhow::anyhow!("Method '{}' caused a panic", request_method),
				);
				JsonRpcResponse {
					jsonrpc: "2.0".to_string(),
					id: request_id_for_error,
					result: None,
					error: Some(JsonRpcError {
						code: -32603,
						message: "Internal server error (panic recovered)".to_string(),
						data: Some(json!({"method": request_method})),
					}),
				}
			}
		};

		// Log the response with timing
		let duration_ms = start_time.elapsed().as_millis() as u64;
		log_mcp_response(
			&request_method,
			response.error.is_none(),
			request_id.as_ref(),
			Some(duration_ms),
		);

		Ok(Some(response))
	}

	/// Helper method to detect broken pipe errors
	fn is_broken_pipe_error(&self, error: &std::io::Error) -> bool {
		use std::io::ErrorKind;
		matches!(
			error.kind(),
			ErrorKind::BrokenPipe
				| ErrorKind::ConnectionAborted
				| ErrorKind::ConnectionReset
				| ErrorKind::UnexpectedEof
		)
	}

	/// Safe response sending with error handling and size limits
	async fn send_response(
		&self,
		writer: &mut tokio::io::Stdout,
		response: &JsonRpcResponse,
	) -> Result<()> {
		// Serialize response with panic recovery and size checking
		let response_json = match panic::catch_unwind(|| serde_json::to_string(response)) {
			Ok(Ok(json)) => {
				// Check response size to prevent memory issues
				if json.len() > MCP_MAX_REQUEST_SIZE {
					log_critical_anyhow_error(
						"Response too large",
						&anyhow::anyhow!(
							"Response size {} exceeds limit {}",
							json.len(),
							MCP_MAX_REQUEST_SIZE
						),
					);
					// Create a minimal error response instead
					r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Response too large"}}"#.to_string()
				} else {
					json
				}
			}
			Ok(Err(e)) => {
				log_critical_error("Response serialization failed", &e);
				// Create a minimal error response
				r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Response serialization failed"}}"#.to_string()
			}
			Err(_) => {
				log_critical_anyhow_error(
					"Response serialization panicked",
					&anyhow::anyhow!("JSON serialization panicked"),
				);
				r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Response serialization panicked"}}"#.to_string()
			}
		};

		// Send with timeout to prevent hanging on broken pipes
		tokio::time::timeout(Duration::from_millis(MCP_IO_TIMEOUT_MS), async {
			writer.write_all(response_json.as_bytes()).await?;
			writer.write_all(b"\n").await?;
			writer.flush().await
		})
		.await
		.map_err(|_| anyhow::anyhow!("Response send timeout"))??;

		Ok(())
	}

	/// Helper method to send error responses
	async fn send_error_response(
		&self,
		writer: &mut tokio::io::Stdout,
		id: Option<&serde_json::Value>,
		code: i32,
		message: &str,
		data: Option<serde_json::Value>,
	) -> Result<()> {
		let error_response = JsonRpcResponse {
			jsonrpc: "2.0".to_string(),
			id: id.cloned(),
			result: None,
			error: Some(JsonRpcError {
				code,
				message: message.to_string(),
				data,
			}),
		};

		self.send_response(writer, &error_response).await
	}

	async fn handle_initialize(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
		JsonRpcResponse {
			jsonrpc: "2.0".to_string(),
			id: request.id.clone(),
			result: Some(json!({
				"protocolVersion": "2024-11-05",
				"capabilities": {
					"tools": {
						"listChanged": false
					}
				},
				"serverInfo": {
					"name": "octocode-mcp",
					"version": "0.1.0",
					"description": "Semantic code search server with vector embeddings, memory system, and optional GraphRAG support"
				},
				"instructions": "This server provides modular AI tools: semantic code search, memory management, and GraphRAG. Use 'semantic_search' for code/documentation searches, memory tools for storing/retrieving context, and 'graphrag_search' (if available) for relationship queries."
			})),
			error: None,
		}
	}

	async fn handle_tools_list(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
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

		// Add LSP tools if LSP provider is configured (always show tools when --with-lsp is used)
		if self.lsp.is_some() {
			tools.extend(crate::mcp::lsp::LspProvider::get_tool_definitions());
		}

		JsonRpcResponse {
			jsonrpc: "2.0".to_string(),
			id: request.id.clone(),
			result: Some(json!({
				"tools": tools
			})),
			error: None,
		}
	}

	async fn handle_tools_call(&mut self, request: &JsonRpcRequest) -> JsonRpcResponse {
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
						data: Some(json!({
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
						data: Some(json!({
							"details": "Required field 'name' must be provided with the tool name to call"
						})),
					}),
				};
			}
		};

		let default_args = json!({});
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
						data: Some(json!({
							"max_size": MCP_MAX_REQUEST_SIZE,
							"actual_size": args_str.len()
						})),
					}),
				};
			}
		}

		let result = match tool_name {
			"semantic_search" => self.semantic_code.execute_search(arguments).await,
			"view_signatures" => self.semantic_code.execute_view_signatures(arguments).await,
			"graphrag_search" => match &self.graphrag {
				Some(provider) => provider.execute_search(arguments).await,
				None => Err(McpError::method_not_found("GraphRAG is not enabled in the current configuration. Please enable GraphRAG in octocode.toml to use relationship-aware search.", "graphrag_search")),
			},
			"memorize" => match &self.memory {
				Some(provider) => provider.execute_memorize(arguments).await,
				None => Err(McpError::method_not_found("Memory system is not available", "memorize")),
			},
			"remember" => match &self.memory {
				Some(provider) => provider.execute_remember(arguments).await,
				None => Err(McpError::method_not_found("Memory system is not available", "remember")),
			},
			"forget" => match &self.memory {
				Some(provider) => provider.execute_forget(arguments).await,
				None => Err(McpError::method_not_found("Memory system is not available", "forget")),
			},
			// LSP tools
			"lsp_goto_definition" => match &self.lsp {
				Some(provider) => {
					let mut lsp_guard = provider.lock().await;
					lsp_guard.execute_goto_definition(arguments).await
				},
				None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_goto_definition")),
			},
			"lsp_hover" => match &self.lsp {
				Some(provider) => {
					let mut lsp_guard = provider.lock().await;
					lsp_guard.execute_hover(arguments).await
				},
				None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_hover")),
			},
			"lsp_find_references" => match &self.lsp {
				Some(provider) => {
					let mut lsp_guard = provider.lock().await;
					lsp_guard.execute_find_references(arguments).await
				},
				None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_find_references")),
			},
			"lsp_document_symbols" => match &self.lsp {
				Some(provider) => {
					let mut lsp_guard = provider.lock().await;
					lsp_guard.execute_document_symbols(arguments).await
				},
				None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_document_symbols")),
			},
			"lsp_workspace_symbols" => match &self.lsp {
				Some(provider) => {
					let mut lsp_guard = provider.lock().await;
					lsp_guard.execute_workspace_symbols(arguments).await
				},
				None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_workspace_symbols")),
			},
			"lsp_completion" => match &self.lsp {
				Some(provider) => {
					let mut lsp_guard = provider.lock().await;
					lsp_guard.execute_completion(arguments).await
				},
				None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_completion")),
			},
			_ => {
				let available_tools = format!("semantic_search, view_signatures{}{}{}",
					if self.graphrag.is_some() { ", graphrag_search" } else { "" },
					if self.memory.is_some() { ", memorize, remember, forget" } else { "" },
					if self.lsp.is_some() { ", lsp_goto_definition, lsp_hover, lsp_find_references, lsp_document_symbols, lsp_workspace_symbols, lsp_completion" } else { "" }
				);
				Err(McpError::method_not_found(format!("Unknown tool '{}'. Available tools: {}", tool_name, available_tools), tool_name))
			}
		};

		match result {
			Ok(content) => JsonRpcResponse {
				jsonrpc: "2.0".to_string(),
				id: request.id.clone(),
				result: Some(json!({
					"content": [{
						"type": "text",
						"text": content
					}]
				})),
				error: None,
			},
			Err(e) => {
				// Try to parse MCP-compliant error first
				let error_message = e.to_string();
				if let Some(mcp_error) = parse_mcp_error(&error_message) {
					JsonRpcResponse {
						jsonrpc: "2.0".to_string(),
						id: request.id.clone(),
						result: None,
						error: Some(mcp_error),
					}
				} else {
					// Handle McpError directly
					JsonRpcResponse {
						jsonrpc: "2.0".to_string(),
						id: request.id.clone(),
						result: None,
						error: Some(e.into_jsonrpc()),
					}
				}
			}
		}
	}

	async fn handle_ping(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
		JsonRpcResponse {
			jsonrpc: "2.0".to_string(),
			id: request.id.clone(),
			result: Some(json!({})),
			error: None,
		}
	}

	/// Update LSP server with recently changed files
	async fn update_lsp_after_indexing(
		lsp_provider: &mut crate::mcp::lsp::LspProvider,
		working_directory: &std::path::Path,
	) -> Result<()> {
		use crate::indexer::{detect_language, NoindexWalker, PathUtils};

		debug!("Updating LSP server with changed files");

		// Use existing file walker that respects .gitignore and .noindex
		let walker = NoindexWalker::create_walker(working_directory).build();
		let mut files_updated = 0;

		for result in walker {
			let entry = match result {
				Ok(entry) => entry,
				Err(_) => continue,
			};

			// Skip directories, only process files
			if !entry.file_type().is_some_and(|ft| ft.is_file()) {
				continue;
			}

			// Only process files that have a detected language (code files)
			if detect_language(entry.path()).is_some() {
				let relative_path = PathUtils::to_relative_string(entry.path(), working_directory);

				// Try to update the file in LSP
				if let Err(e) = lsp_provider.update_file(&relative_path).await {
					debug!("Failed to update file {} in LSP: {}", relative_path, e);
				} else {
					files_updated += 1;
				}
			}
		}

		debug!("LSP update completed: {} files updated", files_updated);
		Ok(())
	}
}

impl Drop for McpServer {
	fn drop(&mut self) {
		// Ensure background tasks are cleaned up
		if let Some(handle) = self.watcher_handle.take() {
			handle.abort();
		}
		if let Some(handle) = self.index_handle.take() {
			handle.abort();
		}
	}
}

// Helper functions
async fn perform_indexing(
	store: &Store,
	config: &Config,
	working_directory: &std::path::Path,
	no_git: bool,
) -> Result<()> {
	let start_time = std::time::Instant::now();
	log_indexing_operation("direct_reindex_start", None, None, true);

	// Create shared state for indexing (same as watch command)
	let state = state::create_shared_state();
	state.write().current_directory = working_directory.to_path_buf();

	// Get git root for optimization (same as watch command)
	let git_repo_root = if !no_git {
		indexer::git::find_git_root(working_directory)
	} else {
		None
	};

	// Perform the indexing directly (same as watch command in quiet mode)
	let indexing_result = indexer::index_files_with_quiet(
		store,
		state.clone(),
		config,
		git_repo_root.as_deref(),
		true,
	)
	.await;

	let duration_ms = start_time.elapsed().as_millis() as u64;

	match indexing_result {
		Ok(()) => {
			log_indexing_operation("direct_reindex_complete", None, Some(duration_ms), true);
			Ok(())
		}
		Err(e) => {
			log_indexing_operation("direct_reindex_complete", None, Some(duration_ms), false);
			log_critical_error("Direct indexing", e.as_ref());
			Err(e)
		}
	}
}
async fn run_watcher(
	tx: mpsc::Sender<()>,
	working_dir: std::path::PathBuf,
	debug: bool,
) -> Result<()> {
	use notify::RecursiveMode;
	use notify_debouncer_mini::{new_debouncer, DebouncedEvent};

	let (debouncer_tx, mut debouncer_rx) = mpsc::channel(MCP_MAX_PENDING_EVENTS);

	// Create ignore patterns manager with error handling
	let ignore_patterns = IgnorePatterns::new(working_dir.clone());

	// Use minimal debounce for the file watcher itself - we handle the real debouncing in the event handler
	let mut debouncer = new_debouncer(
		Duration::from_millis(MIN_DEBOUNCE_MS),
		move |res: Result<Vec<DebouncedEvent>, notify::Error>| match res {
			Ok(events) => {
				// Filter out events from irrelevant paths using ignore patterns
				let relevant_events: Vec<_> = events
					.iter()
					.filter(|event| !ignore_patterns.should_ignore_path(&event.path))
					.collect();

				if !relevant_events.is_empty() {
					// Log file watcher events using our structured logging
					log_watcher_event("file_change_batch", None, relevant_events.len());

					if debug && MCP_ENABLE_VERBOSE_EVENTS {
						trace!(
							event_count = relevant_events.len(),
							"File watcher detected relevant events"
						);
						for event in &relevant_events {
							trace!(
								event_kind = ?event.kind,
								event_path = ?event.path,
								"File watcher event detail"
							);
						}
					}

					// Send notification for each relevant event batch with error handling
					if let Err(e) = debouncer_tx.try_send(()) {
						warn!(
							error = ?e,
							"Failed to send file event - channel may be full"
						);
						// Don't panic on channel send failure - just log and continue
					}
				}
			}
			Err(e) => {
				log_critical_error("File watcher error", &e);
				// Continue running even on watcher errors
			}
		},
	)
	.map_err(|e| anyhow::anyhow!("Failed to create file watcher: {}", e))?;

	// Watch directory with error handling
	if let Err(e) = debouncer
		.watcher()
		.watch(&working_dir, RecursiveMode::Recursive)
	{
		log_critical_error("Failed to start watching directory", &e);
		return Err(anyhow::anyhow!("Failed to watch directory: {}", e));
	}

	debug!(
		working_dir = %working_dir.display(),
		debounce_ms = MCP_DEBOUNCE_MS,
		additional_delay_ms = DEFAULT_ADDITIONAL_DELAY_MS,
		"File watcher started with ignore patterns loaded"
	);

	// Forward events from debouncer to the main event handler with error recovery
	let mut consecutive_errors = 0u32;
	const MAX_WATCHER_ERRORS: u32 = 5;

	while let Some(()) = debouncer_rx.recv().await {
		match tx.send(()).await {
			Ok(()) => {
				consecutive_errors = 0; // Reset on successful send
			}
			Err(e) => {
				consecutive_errors += 1;
				log_critical_error("Event channel send failed", &e);

				debug!(
					consecutive_errors = consecutive_errors,
					"Event channel closed or failed"
				);

				if consecutive_errors >= MAX_WATCHER_ERRORS {
					log_critical_anyhow_error(
						"Too many watcher errors",
						&anyhow::anyhow!(
							"Stopping file watcher after {} consecutive errors",
							consecutive_errors
						),
					);
					break;
				}

				// Brief delay before retrying
				tokio::time::sleep(Duration::from_millis(100)).await;
			}
		}
	}

	debug!("File watcher stopped");

	Ok(())
}

// HTTP server types and handlers for MCP over HTTP

/// Shared state for HTTP handlers
#[derive(Clone)]
struct HttpServerState {
	semantic_code: SemanticCodeProvider,
	graphrag: Option<GraphRagProvider>,
	memory: Option<MemoryProvider>,
	lsp: Option<Arc<Mutex<crate::mcp::lsp::LspProvider>>>,
}

/// Handle a single HTTP connection
async fn handle_http_connection(
	mut stream: TcpStream,
	state: Arc<Mutex<HttpServerState>>,
) -> Result<()> {
	let mut buffer = vec![0; 8192];
	let bytes_read = stream.read(&mut buffer).await?;

	if bytes_read == 0 {
		return Ok(());
	}

	let request_str = String::from_utf8_lossy(&buffer[..bytes_read]);

	// Parse HTTP request - simple implementation for MCP over HTTP
	let mut lines = request_str.lines();
	let request_line = lines.next().unwrap_or("");

	// Check if it's a POST request to /mcp
	if !request_line.starts_with("POST /mcp") && !request_line.starts_with("POST / ") {
		// Send 404 for non-MCP endpoints
		let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
		stream.write_all(response.as_bytes()).await?;
		return Ok(());
	}

	// Find content length
	let mut content_length = 0;
	let mut body_start = 0;

	for (i, line) in lines.enumerate() {
		if line.is_empty() {
			// Calculate where body starts in the original buffer
			let lines_before_body: Vec<&str> = request_str.lines().take(i + 2).collect();
			body_start = lines_before_body.join("\n").len() + 1; // +1 for the final \n
			break;
		}
		if line.to_lowercase().starts_with("content-length:") {
			if let Some(len_str) = line.split(':').nth(1) {
				content_length = len_str.trim().parse().unwrap_or(0);
			}
		}
	}

	// Extract JSON body
	let json_body = if content_length > 0 && body_start < bytes_read {
		let body_bytes =
			&buffer[body_start..std::cmp::min(body_start + content_length, bytes_read)];
		String::from_utf8_lossy(body_bytes).to_string()
	} else {
		return send_http_error(&mut stream, 400, "Missing or invalid request body").await;
	};

	// Parse JSON-RPC request
	let request: JsonRpcRequest = match serde_json::from_str(&json_body) {
		Ok(req) => req,
		Err(e) => {
			debug!("Failed to parse JSON-RPC request: {}", e);
			return send_http_error(&mut stream, 400, "Invalid JSON-RPC request").await;
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

	// Get server state
	let server_state = state.lock().await;

	// Handle the request
	let response = match request.method.as_str() {
		"initialize" => handle_initialize_http(&request),
		"tools/list" => handle_tools_list_http(&request, &server_state),
		"tools/call" => handle_tools_call_http(&request, &server_state).await,
		"ping" => handle_ping_http(&request),
		_ => JsonRpcResponse {
			jsonrpc: "2.0".to_string(),
			id: request.id,
			result: None,
			error: Some(JsonRpcError {
				code: -32601,
				message: "Method not found".to_string(),
				data: None,
			}),
		},
	};

	// Log the response
	let duration_ms = start_time.elapsed().as_millis() as u64;
	log_mcp_response(
		&request_method,
		response.error.is_none(),
		request_id.as_ref(),
		Some(duration_ms),
	);

	// Send HTTP response
	send_http_response(&mut stream, &response).await
}

/// Send HTTP error response
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

/// Send HTTP JSON-RPC response
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

fn handle_initialize_http(request: &JsonRpcRequest) -> JsonRpcResponse {
	JsonRpcResponse {
		jsonrpc: "2.0".to_string(),
		id: request.id.clone(),
		result: Some(json!({
			"protocolVersion": "2024-11-05",
			"capabilities": {
				"tools": {
					"listChanged": false
				}
			},
			"serverInfo": {
				"name": "octocode-mcp",
				"version": "0.1.0",
				"description": "Semantic code search server with vector embeddings, memory system, and optional GraphRAG support"
			},
			"instructions": "This server provides modular AI tools: semantic code search, memory management, and GraphRAG. Use 'semantic_search' for code/documentation searches, memory tools for storing/retrieving context, and 'graphrag_search' (if available) for relationship queries."
		})),
		error: None,
	}
}

fn handle_tools_list_http(request: &JsonRpcRequest, state: &HttpServerState) -> JsonRpcResponse {
	let mut tools = vec![
		SemanticCodeProvider::get_tool_definition(),
		SemanticCodeProvider::get_view_signatures_tool_definition(),
	];

	// Add memory tools if available
	if state.memory.is_some() {
		tools.extend(MemoryProvider::get_tool_definitions());
	}

	// Add GraphRAG tools if available
	if state.graphrag.is_some() {
		tools.push(GraphRagProvider::get_tool_definition());
	}

	// Add LSP tools if LSP provider is configured
	if state.lsp.is_some() {
		tools.extend(crate::mcp::lsp::LspProvider::get_tool_definitions());
	}

	JsonRpcResponse {
		jsonrpc: "2.0".to_string(),
		id: request.id.clone(),
		result: Some(json!({
			"tools": tools
		})),
		error: None,
	}
}

async fn handle_tools_call_http(
	request: &JsonRpcRequest,
	state: &HttpServerState,
) -> JsonRpcResponse {
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
					data: Some(json!({
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
					data: Some(json!({
						"details": "Required field 'name' must be provided with the tool name to call"
					})),
				}),
			};
		}
	};

	let default_args = json!({});
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
					data: Some(json!({
						"max_size": MCP_MAX_REQUEST_SIZE,
						"actual_size": args_str.len()
					})),
				}),
			};
		}
	}

	let result = match tool_name {
		"semantic_search" => state.semantic_code.execute_search(arguments).await,
		"view_signatures" => state.semantic_code.execute_view_signatures(arguments).await,
		"graphrag_search" => match &state.graphrag {
			Some(provider) => provider.execute_search(arguments).await,
			None => Err(McpError::method_not_found("GraphRAG is not enabled in the current configuration. Please enable GraphRAG in octocode.toml to use relationship-aware search.", "graphrag_search")),
		},
		"memorize" => match &state.memory {
			Some(provider) => provider.execute_memorize(arguments).await,
			None => Err(McpError::method_not_found("Memory system is not available", "memorize")),
		},
		"remember" => match &state.memory {
			Some(provider) => provider.execute_remember(arguments).await,
			None => Err(McpError::method_not_found("Memory system is not available", "remember")),
		},
		"forget" => match &state.memory {
			Some(provider) => provider.execute_forget(arguments).await,
			None => Err(McpError::method_not_found("Memory system is not available", "forget")),
		},
		// LSP tools
		"lsp_goto_definition" => match &state.lsp {
			Some(provider) => {
				let mut lsp_guard = provider.lock().await;
				lsp_guard.execute_goto_definition(arguments).await
			},
			None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_goto_definition")),
		},
		"lsp_hover" => match &state.lsp {
			Some(provider) => {
				let mut lsp_guard = provider.lock().await;
				lsp_guard.execute_hover(arguments).await
			},
			None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_hover")),
		},
		"lsp_find_references" => match &state.lsp {
			Some(provider) => {
				let mut lsp_guard = provider.lock().await;
				lsp_guard.execute_find_references(arguments).await
			},
			None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_find_references")),
		},
		"lsp_document_symbols" => match &state.lsp {
			Some(provider) => {
				let mut lsp_guard = provider.lock().await;
				lsp_guard.execute_document_symbols(arguments).await
			},
			None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_document_symbols")),
		},
		"lsp_workspace_symbols" => match &state.lsp {
			Some(provider) => {
				let mut lsp_guard = provider.lock().await;
				lsp_guard.execute_workspace_symbols(arguments).await
			},
			None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_workspace_symbols")),
		},
		"lsp_completion" => match &state.lsp {
			Some(provider) => {
				let mut lsp_guard = provider.lock().await;
				lsp_guard.execute_completion(arguments).await
			},
			None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_completion")),
		},
		_ => {
			let available_tools = format!("semantic_search, view_signatures{}{}{}",
				if state.graphrag.is_some() { ", graphrag_search" } else { "" },
				if state.memory.is_some() { ", memorize, remember, forget" } else { "" },
				if state.lsp.is_some() { ", lsp_goto_definition, lsp_hover, lsp_find_references, lsp_document_symbols, lsp_workspace_symbols, lsp_completion" } else { "" }
			);
			Err(McpError::method_not_found(format!("Unknown tool '{}'. Available tools: {}", tool_name, available_tools), tool_name))
		}
	};

	match result {
		Ok(content) => JsonRpcResponse {
			jsonrpc: "2.0".to_string(),
			id: request.id.clone(),
			result: Some(json!({
				"content": [{
					"type": "text",
					"text": content
				}]
			})),
			error: None,
		},
		Err(e) => {
			let error_message = e.to_string();

			// Try to parse MCP-compliant error first
			if let Some(mcp_error) = parse_mcp_error(&error_message) {
				JsonRpcResponse {
					jsonrpc: "2.0".to_string(),
					id: request.id.clone(),
					result: None,
					error: Some(mcp_error),
				}
			} else {
				// Handle McpError directly
				JsonRpcResponse {
					jsonrpc: "2.0".to_string(),
					id: request.id.clone(),
					result: None,
					error: Some(e.into_jsonrpc()),
				}
			}
		}
	}
}

fn handle_ping_http(request: &JsonRpcRequest) -> JsonRpcResponse {
	JsonRpcResponse {
		jsonrpc: "2.0".to_string(),
		id: request.id.clone(),
		result: Some(json!({})),
		error: None,
	}
}
