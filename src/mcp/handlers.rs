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

//! Shared MCP request handlers - eliminates duplication between server and proxy

use super::graphrag::GraphRagProvider;
use super::semantic_code::SemanticCodeProvider;
use super::types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpError};

const MCP_MAX_REQUEST_SIZE: usize = 10_485_760; // 10MB maximum request size

/// Handle MCP initialize request
pub fn handle_initialize(
	request: &JsonRpcRequest,
	server_name: &str,
	instructions: &str,
) -> JsonRpcResponse {
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
				"name": server_name,
				"version": "0.1.0",
				"description": "MCP server with semantic search and GraphRAG"
			},
			"instructions": instructions
		})),
		error: None,
	}
}

/// Handle MCP tools/list request
pub fn handle_tools_list(
	request: &JsonRpcRequest,
	_semantic_code: &SemanticCodeProvider,
	graphrag: &Option<GraphRagProvider>,
) -> JsonRpcResponse {
	let mut tools = vec![
		SemanticCodeProvider::get_tool_definition(),
		SemanticCodeProvider::get_view_signatures_tool_definition(),
	];

	// Add GraphRAG tools if available
	if graphrag.is_some() {
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

/// Handle MCP tools/call request
pub async fn handle_tools_call(
	request: &JsonRpcRequest,
	semantic_code: &SemanticCodeProvider,
	graphrag: &Option<GraphRagProvider>,
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

	// Execute tools
	let result = match tool_name {
		"semantic_search" => semantic_code.execute_search(arguments).await,
		"view_signatures" => semantic_code.execute_view_signatures(arguments).await,
		"graphrag" => match graphrag {
			Some(provider) => provider.execute(arguments).await,
			None => Err(McpError::method_not_found(
				"GraphRAG is not enabled in the current configuration. Please enable GraphRAG in octocode.toml to use relationship-aware search.",
				"graphrag"
			)),
		},
		_ => {
			let available_tools = format!(
				"semantic_search, view_signatures{}",
				if graphrag.is_some() { ", graphrag" } else { "" }
			);
			Err(McpError::method_not_found(
				format!("Unknown tool '{}'. Available tools: {}", tool_name, available_tools),
				"tools_call"
			))
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

/// Handle MCP ping request
pub fn handle_ping(request: &JsonRpcRequest) -> JsonRpcResponse {
	JsonRpcResponse {
		jsonrpc: "2.0".to_string(),
		id: request.id.clone(),
		result: Some(serde_json::json!({})),
		error: None,
	}
}
