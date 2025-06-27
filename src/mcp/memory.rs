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
use serde_json::{json, Value};
use std::sync::Arc;

use tokio::sync::Mutex;

use tracing::{debug, warn};

use crate::config::Config;
use crate::constants::MAX_QUERIES;
use crate::embedding::truncate_output;
use crate::mcp::logging::log_critical_anyhow_error;
use crate::mcp::types::{McpError, McpTool};
use crate::memory::{MemoryManager, MemoryQuery, MemoryType};

/// Memory tools provider
#[derive(Clone)]
pub struct MemoryProvider {
	memory_manager: Arc<Mutex<MemoryManager>>,
	working_directory: std::path::PathBuf,
}

impl MemoryProvider {
	pub async fn new(config: &Config, working_directory: std::path::PathBuf) -> Option<Self> {
		match MemoryManager::new(config).await {
			Ok(manager) => Some(Self {
				memory_manager: Arc::new(Mutex::new(manager)),
				working_directory,
			}),
			Err(e) => {
				warn!(
					error = %e,
					"Failed to initialize memory manager"
				);
				None
			}
		}
	}

	/// Get all tool definitions for memory operations
	pub fn get_tool_definitions() -> Vec<McpTool> {
		vec![
			McpTool {
				name: "memorize".to_string(),
				description: "Store important information, insights, or context in memory for future reference.".to_string(),
				input_schema: json!({
					"type": "object",
					"properties": {
						"title": {
							"type": "string",
							"description": "Short, descriptive title for the memory",
							"minLength": 5,
							"maxLength": 200
						},
						"content": {
							"type": "string",
							"description": "Detailed content to remember - explanations, code snippets, insights, decisions, etc.",
							"minLength": 10,
							"maxLength": 10000
						},
						"memory_type": {
							"type": "string",
							"description": "Category of memory for better organization",
							"enum": ["code", "architecture", "bug_fix", "feature", "documentation", "user_preference", "decision", "learning", "configuration", "testing", "performance", "security", "insight"],
							"default": "code"
						},
						"importance": {
							"type": "number",
							"description": "Importance score from 0.0 to 1.0 (higher = more important for retention)",
							"minimum": 0.0,
							"maximum": 1.0,
							"default": 0.5
						},
						"tags": {
							"type": "array",
							"description": "Optional tags for categorization and easier searching",
							"items": {
								"type": "string"
							},
							"maxItems": 10
						},
						"related_files": {
							"type": "array",
							"description": "Optional file paths related to this memory",
							"items": {
								"type": "string"
							},
							"maxItems": 20
						}
					},
					"required": ["title", "content"],
					"additionalProperties": false
				}),
			},
			McpTool {
				name: "remember".to_string(),
				description: "Search and retrieve stored memories using semantic search. Find relevant past information, decisions, patterns, or context based on your query.".to_string(),
				input_schema: json!({
					"type": "object",
					"properties": {
						"query": {
							"oneOf": [
								{
									"type": "string",
									"description": "Single search query - use for specific searches. Express in human terms for semantic search",
									"minLength": 3,
									"maxLength": 500
								},
								{
									"type": "array",
									"items": {
										"type": "string",
										"minLength": 3,
										"maxLength": 500
									},
									"minItems": 1,
									"maxItems": 5,
									"description": "RECOMMENDED: Array of related search terms for comprehensive results. Example: ['authentication patterns', 'login implementation', 'user session management'] finds all auth-related memories in one search"
								}
							],
							"description": "PREFER ARRAY OF RELATED TERMS: ['user authentication patterns', 'login session management', 'password validation'] for comprehensive search. Single string only for very specific searches. Use multi-term for: Feature exploration: ['database patterns', 'query optimization', 'data persistence'], Related concepts: ['error handling', 'exception recovery', 'failure patterns'], System understanding: ['architecture decisions', 'design patterns', 'implementation choices']. Use descriptive phrases for semantic search."
						},
						"memory_types": {
							"type": "array",
							"description": "Optional filter by memory types",
							"items": {
								"type": "string",
								"enum": ["code", "architecture", "bug_fix", "feature", "documentation", "user_preference", "decision", "learning", "configuration", "testing", "performance", "security", "insight"]
							}
						},
						"tags": {
							"type": "array",
							"description": "Optional filter by tags",
							"items": {
								"type": "string"
							}
						},
						"related_files": {
							"type": "array",
							"description": "Optional filter by related files",
							"items": {
								"type": "string"
							}
						},
				"limit": {
					"type": "integer",
					"description": "Maximum number of memories to return",
					"minimum": 1,
					"maximum": 5,
					"default": 5
				},
				"max_tokens": {
					"type": "integer",
					"description": "Maximum tokens allowed in output before truncation (default: 2000, set to 0 for unlimited)",
					"minimum": 0,
					"default": 2000
				}
					},
					"required": ["query"],
					"additionalProperties": false
				}),
			},
			McpTool {
				name: "forget".to_string(),
				description: "Permanently remove specific memories by ID or forget multiple memories matching certain criteria.".to_string(),
				input_schema: json!({
					"type": "object",
					"properties": {
						"memory_id": {
							"type": "string",
							"description": "Specific memory ID to forget (get this from remember results)"
						},
						"query": {
							"type": "string",
							"description": "Query to find memories to forget (alternative to memory_id)"
						},
						"memory_types": {
							"type": "array",
							"description": "Filter by memory types when using query",
							"items": {
								"type": "string",
								"enum": ["code", "architecture", "bug_fix", "feature", "documentation", "user_preference", "decision", "learning", "configuration", "testing", "performance", "security", "insight"]
							}
						},
						"tags": {
							"type": "array",
							"description": "Filter by tags when using query",
							"items": {
								"type": "string"
							}
						},
						"confirm": {
							"type": "boolean",
							"description": "Must be true to confirm deletion",
							"const": true
						}
					},
					"required": ["confirm"],
					"additionalProperties": false
				}),
			}
		]
	}

	/// Execute the memorize tool with enhanced error handling
	pub async fn execute_memorize(&self, arguments: &Value) -> Result<String, McpError> {
		// Validate input parameters exist before processing
		let title = arguments
			.get("title")
			.and_then(|v| v.as_str())
			.ok_or_else(|| {
				McpError::invalid_params("Missing required parameter 'title'", "memorize")
			})?;

		let content = arguments
			.get("content")
			.and_then(|v| v.as_str())
			.ok_or_else(|| {
				McpError::invalid_params("Missing required parameter 'content'", "memorize")
			})?;

		// Ensure clean UTF-8 content using lossy conversion
		let clean_title = String::from_utf8_lossy(title.as_bytes()).to_string();
		let clean_content = String::from_utf8_lossy(content.as_bytes()).to_string();
		let title = clean_title.as_str();
		let content = clean_content.as_str();

		// Validate lengths directly on original content
		if title.len() < 5 || title.len() > 200 {
			return Err(McpError::invalid_params(
				"Title must be between 5 and 200 characters",
				"memorize",
			));
		}
		if content.len() < 10 || content.len() > 10000 {
			return Err(McpError::invalid_params(
				"Content must be between 10 and 10000 characters",
				"memorize",
			));
		}

		let memory_type_str = arguments
			.get("memory_type")
			.and_then(|v| v.as_str())
			.unwrap_or("code");

		let memory_type = MemoryType::from(memory_type_str.to_string());

		let importance = arguments
			.get("importance")
			.and_then(|v| v.as_f64())
			.map(|v| {
				// Clamp importance to valid range
				(v as f32).clamp(0.0, 1.0)
			});

		// Process tags with error handling and UTF-8 safety
		let tags = arguments.get("tags").and_then(|v| v.as_array()).map(|arr| {
			arr.iter()
				.filter_map(|v| {
					v.as_str().and_then(|s| {
						// Ensure clean UTF-8 and validate tag
						let clean_tag = String::from_utf8_lossy(s.as_bytes()).to_string();
						let tag = clean_tag.trim();

						if tag.is_empty() {
							None // Skip empty tags
						} else {
							// Limit tag length
							let final_tag = if tag.chars().count() > 50 {
								tag.chars().take(50).collect()
							} else {
								tag.to_string()
							};
							Some(final_tag)
						}
					})
				})
				.take(10) // Limit number of tags
				.collect::<Vec<String>>()
		});

		let related_files = arguments
			.get("related_files")
			.and_then(|v| v.as_array())
			.map(|arr| {
				arr.iter()
					.filter_map(|v| {
						v.as_str().and_then(|s| {
							// Ensure clean UTF-8 and validate file path
							let clean_path = String::from_utf8_lossy(s.as_bytes()).to_string();
							let path = clean_path.trim();

							if path.is_empty() || path.len() > 500 {
								None // Skip empty or overly long paths
							} else {
								Some(path.to_string())
							}
						})
					})
					.take(20) // Limit number of files
					.collect::<Vec<String>>()
			});

		// Use structured logging instead of console output for MCP protocol compliance
		debug!(
			title = %title,
			memory_type = ?memory_type,
			importance = ?importance,
			"Memorizing new content"
		);

		// Change to working directory for Git context with error handling
		let original_dir = std::env::current_dir().map_err(|e| {
			McpError::internal_error(
				format!("Failed to get current directory: {}", e),
				"memorize",
			)
		})?;

		if let Err(e) = std::env::set_current_dir(&self.working_directory) {
			return Err(McpError::internal_error(
				format!("Failed to change to working directory: {}", e),
				"memorize",
			)
			.with_details(format!("Path: {}", self.working_directory.display())));
		}

		let memory_result = {
			// Lock memory manager for storing - removed timeout to allow embedding generation to complete
			let mut manager_guard = self.memory_manager.lock().await;

			manager_guard
				.memorize(
					memory_type,
					title.to_string(),
					content.to_string(),
					importance,
					tags,
					related_files,
				)
				.await
				.map_err(|e| {
					McpError::internal_error(format!("Failed to store memory: {}", e), "memorize")
				})?
		};

		// Restore original directory regardless of result
		if let Err(e) = std::env::set_current_dir(&original_dir) {
			warn!(
				error = %e,
				"Failed to restore original directory"
			);
		}

		let memory = memory_result;

		// Return plain text response for MCP protocol compliance
		// Return minimal response for MCP protocol compliance - just success and ID
		Ok(format!("Memory stored: {}", memory.id))
	}

	/// Execute the remember tool
	pub async fn execute_remember(&self, arguments: &Value) -> Result<String, McpError> {
		// Parse queries - handle both string and array inputs
		let queries: Vec<String> = match arguments.get("query") {
			Some(Value::String(s)) => vec![s.clone()],
			Some(Value::Array(arr)) => {
				let queries: Vec<String> = arr
					.iter()
					.filter_map(|v| v.as_str().map(String::from))
					.collect();

				if queries.is_empty() {
					return Err(McpError::invalid_params(
						"Invalid query array: must contain at least one non-empty string",
						"remember",
					));
				}

				queries
			}
			_ => {
				return Err(McpError::invalid_params(
					"Missing required parameter 'query': must be a string or array of strings describing what to search for",
					"remember"
				));
			}
		};

		// Validate queries
		if queries.len() > MAX_QUERIES {
			return Err(McpError::invalid_params(
				format!("Too many queries: maximum {} queries allowed, got {}. Use fewer, more specific terms.", MAX_QUERIES, queries.len()),
				"remember"
			));
		}

		for (i, query) in queries.iter().enumerate() {
			// Ensure clean UTF-8 and validate query
			let clean_query = String::from_utf8_lossy(query.as_bytes()).to_string();
			let query = clean_query.trim();

			if query.len() < 3 {
				return Err(McpError::invalid_params(
					format!(
						"Invalid query {}: must be at least 3 characters long",
						i + 1
					),
					"remember",
				));
			}
			if query.len() > 500 {
				return Err(McpError::invalid_params(
					format!(
						"Invalid query {}: must be no more than 500 characters long",
						i + 1
					),
					"remember",
				));
			}
			if query.is_empty() {
				return Err(McpError::invalid_params(
					format!(
						"Invalid query {}: cannot be empty or whitespace only",
						i + 1
					),
					"remember",
				));
			}
		}

		// Parse memory types filter
		let memory_types =
			if let Some(types_array) = arguments.get("memory_types").and_then(|v| v.as_array()) {
				let types: Vec<MemoryType> = types_array
					.iter()
					.filter_map(|v| v.as_str())
					.map(|s| MemoryType::from(s.to_string()))
					.collect();
				if types.is_empty() {
					None
				} else {
					Some(types)
				}
			} else {
				None
			};

		// Parse tags filter
		let tags = if let Some(tags_array) = arguments.get("tags").and_then(|v| v.as_array()) {
			let tag_list: Vec<String> = tags_array
				.iter()
				.filter_map(|v| v.as_str().map(|s| s.to_string()))
				.collect();
			if tag_list.is_empty() {
				None
			} else {
				Some(tag_list)
			}
		} else {
			None
		};

		// Parse related files filter
		let related_files =
			if let Some(files_array) = arguments.get("related_files").and_then(|v| v.as_array()) {
				let file_list: Vec<String> = files_array
					.iter()
					.filter_map(|v| v.as_str().map(|s| s.to_string()))
					.collect();
				if file_list.is_empty() {
					None
				} else {
					Some(file_list)
				}
			} else {
				None
			};

		// Set limit
		let limit = arguments
			.get("limit")
			.and_then(|v| v.as_u64())
			.map(|v| v as usize)
			.unwrap_or(5);

		// Parse max_tokens parameter
		let max_tokens = arguments
			.get("max_tokens")
			.and_then(|v| v.as_u64())
			.unwrap_or(2000) as usize;

		let memory_query = MemoryQuery {
			memory_types,
			tags,
			related_files,
			limit: Some(limit.min(50)),
			..Default::default()
		};

		// Use structured logging instead of console output for MCP protocol compliance
		debug!(
			queries = ?queries,
			limit = memory_query.limit.unwrap_or(10),
			"Remembering memories with {} queries",
			queries.len()
		);

		let results = {
			// Lock memory manager for searching - removed timeout to allow operations to complete
			let manager_guard = self.memory_manager.lock().await;

			// Use multi-query method for comprehensive search
			if queries.len() == 1 {
				manager_guard
					.remember(&queries[0], Some(memory_query))
					.await
					.map_err(|e| {
						McpError::internal_error(
							format!("Failed to search memories: {}", e),
							"remember",
						)
					})?
			} else {
				manager_guard
					.remember_multi(&queries, Some(memory_query))
					.await
					.map_err(|e| {
						McpError::internal_error(
							format!("Failed to search memories: {}", e),
							"remember",
						)
					})?
			}
		};

		if results.is_empty() {
			return Ok("No stored memories match your query. Try using different search terms, removing filters, or checking if any memories have been stored yet.".to_string());
		}

		// Use shared formatting function for token efficiency
		let output = crate::memory::format_memories_as_text(&results);

		// Apply token truncation if needed
		Ok(truncate_output(&output, max_tokens))
	}

	/// Execute the forget tool
	pub async fn execute_forget(&self, arguments: &Value) -> Result<String, McpError> {
		// Check confirm parameter
		if !arguments
			.get("confirm")
			.and_then(|v| v.as_bool())
			.unwrap_or(false)
		{
			return Ok(
				"❌ Missing required confirmation: set 'confirm' to true to proceed with deletion"
					.to_string(),
			);
		}

		// Handle specific memory ID deletion
		if let Some(memory_id) = arguments.get("memory_id").and_then(|v| v.as_str()) {
			// Validate memory ID format
			if memory_id.trim().is_empty() || memory_id.len() > 100 {
				return Ok("❌ Invalid memory ID format".to_string());
			}

			// Use structured logging instead of console output for MCP protocol compliance
			debug!(
				memory_id = %memory_id,
				"Forgetting memory by ID"
			);

			// Execute deletion - removed timeout to allow operation to complete
			let res = {
				let mut manager_guard = self.memory_manager.lock().await;
				manager_guard.forget(memory_id).await
			};
			match res {
				Ok(_) => Ok(format!(
					"✅ Memory deleted successfully\n\nMemory ID: {}",
					memory_id
				)),
				Err(e) => {
					log_critical_anyhow_error("Memory deletion failed", &e);
					Ok(format!("❌ Failed to delete memory: {}", e))
				}
			}
		}
		// Handle query-based deletion
		else if let Some(query) = arguments.get("query").and_then(|v| v.as_str()) {
			// Ensure clean UTF-8 query using lossy conversion
			let clean_query = String::from_utf8_lossy(query.as_bytes()).to_string();
			let query = clean_query.as_str();

			if query.len() < 3 || query.len() > 500 {
				return Ok("❌ Query must be between 3 and 500 characters".to_string());
			}
			// Parse memory types filter
			let memory_types = if let Some(types_array) =
				arguments.get("memory_types").and_then(|v| v.as_array())
			{
				let types: Vec<MemoryType> = types_array
					.iter()
					.filter_map(|v| v.as_str())
					.map(|s| MemoryType::from(s.to_string()))
					.collect();
				if types.is_empty() {
					None
				} else {
					Some(types)
				}
			} else {
				None
			};

			// Parse tags filter
			let tags = if let Some(tags_array) = arguments.get("tags").and_then(|v| v.as_array()) {
				let tag_list: Vec<String> = tags_array
					.iter()
					.filter_map(|v| v.as_str().map(|s| s.to_string()))
					.collect();
				if tag_list.is_empty() {
					None
				} else {
					Some(tag_list)
				}
			} else {
				None
			};

			let memory_query = MemoryQuery {
				query_text: Some(query.to_string()),
				memory_types,
				tags,
				..Default::default()
			};

			// Use structured logging instead of console output for MCP protocol compliance
			debug!(
				query = %query,
				"Forgetting memories matching query"
			);

			let res = {
				let mut manager_guard = self.memory_manager.lock().await;
				manager_guard.forget_matching(memory_query).await
			};
			match res {
				Ok(deleted_count) => Ok(format!(
					"✅ {} memories deleted successfully\n\nQuery: \"{}\"",
					deleted_count, query
				)),
				Err(e) => Ok(format!("❌ Failed to delete memories: {}", e)),
			}
		} else {
			Ok("❌ Either 'memory_id' or 'query' must be provided".to_string())
		}
	}
}
