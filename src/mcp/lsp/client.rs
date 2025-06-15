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
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{oneshot, Mutex};
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info, warn};

use super::protocol::{
	LspIncomingNotification, LspMessage, LspNotification, LspRequest, LspResponse,
};

/// LSP client for communicating with external LSP server process
pub struct LspClient {
	process: Arc<Mutex<Option<Child>>>,
	stdin: Arc<Mutex<Option<ChildStdin>>>,
	request_id_counter: AtomicU32,
	pending_requests: Arc<Mutex<HashMap<u32, oneshot::Sender<LspResponse>>>>,
	command: String,
	working_directory: std::path::PathBuf,
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
		tokio::spawn(Self::communication_loop(stdout, pending_requests));

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

		// Wait for response with timeout
		let response = timeout(Duration::from_secs(30), rx)
			.await
			.map_err(|_| anyhow::anyhow!("LSP request timeout"))?
			.map_err(|_| anyhow::anyhow!("LSP request channel closed"))?;

		// Check for errors in response
		if let Some(error) = &response.error {
			return Err(anyhow::anyhow!(
				"LSP error {}: {}",
				error.code,
				error.message
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
							Self::handle_notification(&notification).await;
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
	async fn handle_notification(notification: &LspIncomingNotification) {
		match notification.method.as_str() {
			"$/progress" => {
				if let Some(params) = &notification.params {
					debug!("LSP Progress: {:?}", params);
				}
			}
			"rust-analyzer/serverStatus" => {
				if let Some(params) = &notification.params {
					info!("Rust-analyzer status: {:?}", params);
				}
			}
			"window/logMessage" => {
				if let Some(params) = &notification.params {
					debug!("LSP log: {:?}", params);
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

	/// Stop the LSP server process
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
		}
	}
}
