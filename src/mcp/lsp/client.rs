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

//! LSP client communication handling

use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{oneshot, Mutex, RwLock};
use tracing::{debug, error, info, warn};

use super::protocol::{
	LspIncomingNotification, LspIncomingRequest, LspMessage, LspNotification, LspRequest,
	LspResponse,
};

/// Progress tracking state
#[derive(Debug, Clone)]
pub struct ProgressState {
	pub token: String,
	pub title: String,
	pub message: Option<String>,
	pub percentage: Option<u32>,
	pub is_complete: bool,
}

/// LSP client for communicating with external LSP server process
pub struct LspClient {
	process: Arc<Mutex<Option<Child>>>,
	stdin: Arc<Mutex<Option<ChildStdin>>>,
	request_id_counter: AtomicU32,
	pending_requests: Arc<Mutex<HashMap<u32, oneshot::Sender<LspResponse>>>>,
	command: String,
	working_directory: std::path::PathBuf,
	// Progress tracking
	progress_states: Arc<RwLock<HashMap<String, ProgressState>>>,
	indexing_complete: Arc<RwLock<bool>>,
}

impl LspClient {
	/// Create new LSP client with command and working directory
	pub fn new(command: String, working_directory: std::path::PathBuf) -> Self {
		Self {
			process: Arc::new(Mutex::new(None)),
			stdin: Arc::new(Mutex::new(None)),
			request_id_counter: AtomicU32::new(1),
			pending_requests: Arc::new(Mutex::new(HashMap::new())),
			command,
			working_directory,
			progress_states: Arc::new(RwLock::new(HashMap::new())),
			indexing_complete: Arc::new(RwLock::new(false)),
		}
	}

	/// Start the LSP server process and communication loop
	pub async fn start(&self) -> Result<()> {
		debug!("Starting LSP server with command: {}", self.command);

		// Parse command into program and arguments
		let parts: Vec<&str> = self.command.split_whitespace().collect();
		if parts.is_empty() {
			return Err(anyhow::anyhow!("Empty LSP command"));
		}

		let program = parts[0];
		let args = &parts[1..];

		// Spawn LSP process
		let mut child = tokio::process::Command::new(program)
			.args(args)
			.current_dir(&self.working_directory)
			.stdin(std::process::Stdio::piped())
			.stdout(std::process::Stdio::piped())
			.stderr(std::process::Stdio::null()) // Ignore stderr to avoid noise
			.spawn()
			.map_err(|e| anyhow::anyhow!("Failed to start LSP server '{}': {}", program, e))?;

		// Take stdin and stdout
		let stdin = child
			.stdin
			.take()
			.ok_or_else(|| anyhow::anyhow!("Failed to get stdin"))?;
		let stdout = child
			.stdout
			.take()
			.ok_or_else(|| anyhow::anyhow!("Failed to get stdout"))?;

		// Store process and stdin
		*self.process.lock().await = Some(child);
		*self.stdin.lock().await = Some(stdin);

		// Start communication loop
		let pending_requests = self.pending_requests.clone();
		let progress_states = self.progress_states.clone();
		let indexing_complete = self.indexing_complete.clone();
		tokio::spawn(Self::communication_loop(
			stdout,
			pending_requests,
			progress_states,
			indexing_complete,
		));

		debug!("LSP server started successfully");
		Ok(())
	}

	/// Send request to LSP server and wait for response
	pub async fn send_request(&self, mut request: LspRequest) -> Result<LspResponse> {
		let request_id = self.request_id_counter.fetch_add(1, Ordering::SeqCst);
		request.id = request_id;

		// Create response channel
		let (tx, rx) = oneshot::channel();

		// Store pending request
		{
			let mut pending = self.pending_requests.lock().await;
			pending.insert(request_id, tx);
		}

		// Send request
		self.send_message(&request).await?;

		debug!("Sent LSP request: {}", request.method);

		// Wait for response (no timeout - handled externally)
		let response = rx.await.map_err(|_| {
			anyhow::anyhow!("LSP request channel closed for method: {}", request.method)
		})?;

		// Check for errors in response
		if let Some(error) = &response.error {
			return Err(anyhow::anyhow!(
				"LSP error {}: {} (method: {})",
				error.code,
				error.message,
				request.method
			));
		}

		Ok(response)
	}

	/// Send notification to LSP server (no response expected)
	pub async fn send_notification(&self, notification: LspNotification) -> Result<()> {
		self.send_message(&notification).await
	}

	/// Send JSON-RPC message to LSP server
	async fn send_message<T: serde::Serialize>(&self, message: &T) -> Result<()> {
		let json = serde_json::to_string(message)?;
		let content = format!("Content-Length: {}\r\n\r\n{}", json.len(), json);

		debug!("Sending LSP message: {}", json);

		let mut stdin_guard = self.stdin.lock().await;
		if let Some(stdin) = stdin_guard.as_mut() {
			stdin.write_all(content.as_bytes()).await?;
			stdin.flush().await?;
			Ok(())
		} else {
			Err(anyhow::anyhow!("LSP server not started"))
		}
	}

	/// Communication loop for reading responses from LSP server
	async fn communication_loop(
		stdout: ChildStdout,
		pending_requests: Arc<Mutex<HashMap<u32, oneshot::Sender<LspResponse>>>>,
		progress_states: Arc<RwLock<HashMap<String, ProgressState>>>,
		indexing_complete: Arc<RwLock<bool>>,
	) {
		let mut reader = BufReader::new(stdout);

		loop {
			match Self::read_lsp_message(&mut reader).await {
				Ok(Some(message)) => {
					match message {
						LspMessage::Response(response) => {
							debug!("Received LSP response: {:?}", response);

							// Handle response
							if let Some(id) = response.id {
								let mut pending = pending_requests.lock().await;
								if let Some(tx) = pending.remove(&id) {
									if tx.send(response).is_err() {
										warn!("Failed to send response to waiting request {}", id);
									}
								} else {
									warn!("Received response for unknown request ID: {}", id);
								}
							}
						}
						LspMessage::Notification(notification) => {
							Self::handle_notification(
								&notification,
								&progress_states,
								&indexing_complete,
							)
							.await;
						}
						LspMessage::IncomingRequest(request) => {
							Self::handle_incoming_request(&request).await;
						}
					}
				}
				Ok(None) => {
					debug!("LSP server closed connection");
					break;
				}
				Err(e) => {
					error!("Error reading from LSP server: {}", e);
					break;
				}
			}
		}

		debug!("LSP communication loop ended");
	}

	/// Handle incoming notifications from LSP server
	async fn handle_notification(
		notification: &LspIncomingNotification,
		progress_states: &Arc<RwLock<HashMap<String, ProgressState>>>,
		indexing_complete: &Arc<RwLock<bool>>,
	) {
		match notification.method.as_str() {
			"$/progress" => {
				if let Some(params) = &notification.params {
					if let Err(e) = Self::handle_progress_notification(
						params,
						progress_states,
						indexing_complete,
					)
					.await
					{
						warn!("Failed to handle progress notification: {}", e);
					}
				}
			}
			"rust-analyzer/serverStatus" => {
				if let Some(params) = &notification.params {
					info!("Rust-analyzer status: {:?}", params);
				}
			}
			"window/logMessage" => {
				if let Some(params) = &notification.params {
					if let Ok(log_params) =
						serde_json::from_value::<lsp_types::LogMessageParams>(params.clone())
					{
						match log_params.typ {
							lsp_types::MessageType::ERROR => error!("LSP: {}", log_params.message),
							lsp_types::MessageType::WARNING => warn!("LSP: {}", log_params.message),
							lsp_types::MessageType::INFO => info!("LSP: {}", log_params.message),
							lsp_types::MessageType::LOG => debug!("LSP: {}", log_params.message),
							_ => debug!("LSP: {}", log_params.message),
						}
					} else {
						debug!("LSP log: {:?}", params);
					}
				}
			}
			"window/showMessage" => {
				if let Some(params) = &notification.params {
					if let Ok(show_params) =
						serde_json::from_value::<lsp_types::ShowMessageParams>(params.clone())
					{
						match show_params.typ {
							lsp_types::MessageType::ERROR => {
								error!("LSP Message: {}", show_params.message)
							}
							lsp_types::MessageType::WARNING => {
								warn!("LSP Message: {}", show_params.message)
							}
							lsp_types::MessageType::INFO => {
								info!("LSP Message: {}", show_params.message)
							}
							lsp_types::MessageType::LOG => {
								debug!("LSP Message: {}", show_params.message)
							}
							_ => debug!("LSP Message: {}", show_params.message),
						}
					} else {
						debug!("LSP show message: {:?}", params);
					}
				}
			}
			_ => {
				debug!(
					"LSP notification {}: {:?}",
					notification.method, notification.params
				);
			}
		}
	}

	/// Handle incoming requests from LSP server (server-to-client requests)
	async fn handle_incoming_request(request: &LspIncomingRequest) {
		match request.method.as_str() {
			"window/workDoneProgress/create" => {
				// Server is requesting to create a progress token
				// We should respond with success to allow progress reporting
				debug!(
					"LSP server requesting progress token creation: {:?}",
					request.params
				);

				// For now, we just acknowledge - the actual progress will come via $/progress notifications
				// In a full implementation, we'd send a response back, but since we're not tracking
				// the stdin channel here, we'll just log it
				info!("LSP server created progress token for work done progress reporting");
			}
			"window/showMessageRequest" => {
				debug!("LSP server show message request: {:?}", request.params);
			}
			_ => {
				debug!(
					"Unhandled LSP incoming request: {} (id: {})",
					request.method, request.id
				);
			}
		}
	}
	async fn handle_progress_notification(
		params: &Value,
		progress_states: &Arc<RwLock<HashMap<String, ProgressState>>>,
		indexing_complete: &Arc<RwLock<bool>>,
	) -> Result<()> {
		// Parse progress notification
		let token = params
			.get("token")
			.and_then(|t| t.as_str())
			.unwrap_or_default()
			.to_string();

		let value = params
			.get("value")
			.ok_or_else(|| anyhow::anyhow!("Progress notification missing 'value' field"))?;

		let kind = value
			.get("kind")
			.and_then(|k| k.as_str())
			.ok_or_else(|| anyhow::anyhow!("Progress notification missing 'kind' field"))?;

		match kind {
			"begin" => {
				let title = value
					.get("title")
					.and_then(|t| t.as_str())
					.unwrap_or("Unknown")
					.to_string();
				let message = value
					.get("message")
					.and_then(|m| m.as_str())
					.map(String::from);
				let percentage = value
					.get("percentage")
					.and_then(|p| p.as_u64())
					.map(|p| p as u32);

				let state = ProgressState {
					token: token.clone(),
					title: title.clone(),
					message,
					percentage,
					is_complete: false,
				};

				{
					let mut states = progress_states.write().await;
					states.insert(token.clone(), state);
				}

				info!("LSP Progress started: {} (token: {})", title, token);
			}
			"report" => {
				let message = value
					.get("message")
					.and_then(|m| m.as_str())
					.map(String::from);
				let percentage = value
					.get("percentage")
					.and_then(|p| p.as_u64())
					.map(|p| p as u32);

				{
					let mut states = progress_states.write().await;
					if let Some(state) = states.get_mut(&token) {
						if let Some(msg) = message {
							state.message = Some(msg);
						}
						if let Some(pct) = percentage {
							state.percentage = Some(pct);
						}
						debug!(
							"LSP Progress update: {} - {}%",
							state.title,
							state.percentage.unwrap_or(0)
						);
					}
				}
			}
			"end" => {
				let message = value
					.get("message")
					.and_then(|m| m.as_str())
					.map(String::from);

				{
					let mut states = progress_states.write().await;
					if let Some(state) = states.get_mut(&token) {
						state.is_complete = true;
						if let Some(msg) = message {
							state.message = Some(msg);
						}
						info!("LSP Progress completed: {} (token: {})", state.title, token);

						// Check if this looks like indexing completion
						if state.title.to_lowercase().contains("index")
							|| state.title.to_lowercase().contains("loading")
							|| state.title.to_lowercase().contains("analyzing")
						{
							info!("LSP indexing appears to be complete");
							*indexing_complete.write().await = true;
						}
					}
				}

				// Clean up completed progress
				{
					let mut states = progress_states.write().await;
					states.remove(&token);
				}
			}
			_ => {
				debug!("Unknown progress kind: {}", kind);
			}
		}

		Ok(())
	}

	/// Read a single LSP message from the stream
	async fn read_lsp_message(reader: &mut BufReader<ChildStdout>) -> Result<Option<LspMessage>> {
		// Read headers
		let mut content_length = 0;
		let mut buffer = String::new();

		loop {
			buffer.clear();
			match reader.read_line(&mut buffer).await? {
				0 => return Ok(None), // EOF
				_ => {
					let line = buffer.trim();
					if line.is_empty() {
						// Empty line indicates end of headers
						break;
					} else if line.starts_with("Content-Length:") {
						content_length = line
							.strip_prefix("Content-Length:")
							.ok_or_else(|| anyhow::anyhow!("Invalid Content-Length header"))?
							.trim()
							.parse::<usize>()?;
					}
					// Ignore other headers like Content-Type
				}
			}
		}

		if content_length == 0 {
			return Err(anyhow::anyhow!("Missing or invalid Content-Length header"));
		}

		// Read exact content length
		let mut content = vec![0u8; content_length];
		reader.read_exact(&mut content).await?;

		// Parse JSON
		let content_str = String::from_utf8(content)?;
		debug!("Received LSP message: {}", content_str);

		let message: LspMessage = serde_json::from_str(&content_str)?;

		Ok(Some(message))
	}

	/// Check if LSP server has completed initial indexing
	pub async fn is_indexing_complete(&self) -> bool {
		*self.indexing_complete.read().await
	}

	/// Check if server is ready for requests (non-blocking)
	pub async fn is_ready_for_requests(&self) -> bool {
		// Server is ready if indexing is complete OR if no progress is currently active
		let indexing_complete = *self.indexing_complete.read().await;
		if indexing_complete {
			return true;
		}

		// Check if there are any active progress operations
		let states = self.progress_states.read().await;
		let has_active_progress = states.values().any(|s| !s.is_complete);

		// If no active progress, assume ready
		!has_active_progress
	}
	pub async fn stop(&self) -> Result<()> {
		debug!("Stopping LSP server");

		let mut process_guard = self.process.lock().await;
		if let Some(mut process) = process_guard.take() {
			// Try to terminate gracefully
			if let Err(e) = process.kill().await {
				warn!("Failed to kill LSP process: {}", e);
			}

			// Wait for process to exit
			if let Err(e) = process.wait().await {
				warn!("Failed to wait for LSP process: {}", e);
			}
		}

		// Clear stdin
		*self.stdin.lock().await = None;

		// Clear pending requests
		let mut pending = self.pending_requests.lock().await;
		pending.clear();

		debug!("LSP server stopped");
		Ok(())
	}
}

impl Drop for LspClient {
	fn drop(&mut self) {
		// Note: We can't call async stop() in Drop, but the process will be killed
		// when the Child is dropped
	}
}

impl Clone for LspClient {
	fn clone(&self) -> Self {
		Self {
			process: self.process.clone(),
			stdin: self.stdin.clone(),
			request_id_counter: AtomicU32::new(self.request_id_counter.load(Ordering::SeqCst)),
			pending_requests: self.pending_requests.clone(),
			command: self.command.clone(),
			working_directory: self.working_directory.clone(),
			progress_states: self.progress_states.clone(),
			indexing_complete: self.indexing_complete.clone(),
		}
	}
}
