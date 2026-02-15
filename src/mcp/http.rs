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

//! HTTP server implementation for MCP over HTTP
//! Provides HTTP transport layer for MCP protocol as an alternative to stdin/stdout

use anyhow::Result;
use serde_json::json;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::debug;

use super::graphrag::GraphRagProvider;
use super::semantic_code::SemanticCodeProvider;
use super::types::{parse_mcp_error, JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpError};

const MCP_MAX_REQUEST_SIZE: usize = 10_485_760; // 10MB maximum request size

/// HTTP server state for handling MCP requests over HTTP
pub struct HttpServerState {
	pub semantic_code: SemanticCodeProvider,
	pub graphrag: Option<GraphRagProvider>,
	pub lsp: Option<Arc<Mutex<crate::mcp::lsp::LspProvider>>>,
}

/// Handle a single HTTP connection
pub async fn handle_http_connection(
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
	crate::mcp::logging::log_mcp_request(
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
	crate::mcp::logging::log_mcp_response(
		&request_method,
		response.error.is_none(),
		request_id.as_ref(),
		Some(duration_ms),
	);

	// Notifications (no id) must not receive a JSON-RPC response.
	if request_id.is_none() {
		return send_http_no_content(&mut stream).await;
	}

	send_http_response(&mut stream, &response).await
}

/// Send HTTP error response
pub async fn send_http_error(stream: &mut TcpStream, status: u16, message: &str) -> Result<()> {
	let status_text = match status {
		400 => "Bad Request",
		404 => "Not Found",
		500 => "Internal Server Error",
		_ => "Error",
	};
	let response = format!(
		"HTTP/1.1 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
		status,
		status_text,
		message.len(),
		message
	);
	stream.write_all(response.as_bytes()).await?;
	Ok(())
}

/// Send HTTP JSON-RPC response
pub async fn send_http_response(stream: &mut TcpStream, response: &JsonRpcResponse) -> Result<()> {
	let json_response = serde_json::to_string(response)?;
	let http_response = format!(
		"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
		json_response.len(),
		json_response
	);
	stream.write_all(http_response.as_bytes()).await?;
	Ok(())
}

/// Send HTTP 204 for JSON-RPC notifications
pub async fn send_http_no_content(stream: &mut TcpStream) -> Result<()> {
	let http_response = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\n\r\n";
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
				"description": "Semantic code search server with vector embeddings and optional GraphRAG support"
			},
			"instructions": "This server provides modular AI tools: semantic code search and GraphRAG (if available). Use 'semantic_search' for code/documentation searches and 'graphrag' (if enabled) for relationship queries."
		})),
		error: None,
	}
}

fn handle_tools_list_http(request: &JsonRpcRequest, state: &HttpServerState) -> JsonRpcResponse {
	let mut tools = vec![
		SemanticCodeProvider::get_tool_definition(),
		SemanticCodeProvider::get_view_signatures_tool_definition(),
	];

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
		"graphrag" => match &state.graphrag {
			Some(provider) => provider.execute(arguments).await,
			None => Err(McpError::method_not_found("GraphRAG is not enabled in the current configuration. Please enable GraphRAG in octocode.toml to use relationship-aware search.", "graphrag")),
		},
		// LSP tools
		"lsp_goto_definition" => match &state.lsp {
			Some(provider) => {
				let mut lsp_guard = provider.lock().await;
				lsp_guard.execute_goto_definition(arguments).await
			}
			None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_goto_definition")),
		},
		"lsp_hover" => match &state.lsp {
			Some(provider) => {
				let mut lsp_guard = provider.lock().await;
				lsp_guard.execute_hover(arguments).await
			}
			None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_hover")),
		},
		"lsp_find_references" => match &state.lsp {
			Some(provider) => {
				let mut lsp_guard = provider.lock().await;
				lsp_guard.execute_find_references(arguments).await
			}
			None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_find_references")),
		},
		"lsp_document_symbols" => match &state.lsp {
			Some(provider) => {
				let mut lsp_guard = provider.lock().await;
				lsp_guard.execute_document_symbols(arguments).await
			}
			None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_document_symbols")),
		},
		"lsp_workspace_symbols" => match &state.lsp {
			Some(provider) => {
				let mut lsp_guard = provider.lock().await;
				lsp_guard.execute_workspace_symbols(arguments).await
			}
			None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_workspace_symbols")),
		},
		"lsp_completion" => match &state.lsp {
			Some(provider) => {
				let mut lsp_guard = provider.lock().await;
				lsp_guard.execute_completion(arguments).await
			}
			None => Err(McpError::method_not_found("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.", "lsp_completion")),
		},
		_ => {
			let available_tools = format!("semantic_search, view_signatures{}{}",
				if state.graphrag.is_some() { ", graphrag" } else { "" },
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
