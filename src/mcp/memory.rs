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
use tokio::time::Duration;

use crate::config::Config;
use crate::mcp::logging::{log_critical_anyhow_error, log_critical_error};
use crate::mcp::types::McpTool;
use crate::memory::{MemoryManager, MemoryQuery, MemoryType};

/// Memory tools provider
pub struct MemoryProvider {
	memory_manager: Arc<Mutex<MemoryManager>>,
	working_directory: std::path::PathBuf,
	debug: bool,
}

impl MemoryProvider {
	pub async fn new(
		config: &Config,
		working_directory: std::path::PathBuf,
		debug: bool,
	) -> Option<Self> {
		match MemoryManager::new(config).await {
			Ok(manager) => Some(Self {
				memory_manager: Arc::new(Mutex::new(manager)),
				working_directory,
				debug,
			}),
			Err(e) => {
				if debug {
					eprintln!("Warning: Failed to initialize memory manager: {}", e);
				}
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
							"description": "Detailed content to remember – explanations, code snippets, insights, decisions, etc.",
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
				description: "Search and retrieve stored memories using semantic search. Find relevant past information, decisions, patterns, or context based on your query. ".to_string(),
				input_schema: json!({
					"type": "object",
					"properties": {
						"query": {
							"type": "string",
							"description": "What you want to remember or search for in stored memories",
							"minLength": 3,
							"maxLength": 500
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
							"maximum": 50,
							"default": 10
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

	/// Sanitize text content to remove Unicode emojis and other problematic characters
	/// that could cause string slicing panics in consuming systems
	fn sanitize_content(text: &str) -> String {
		// Preserve emojis, but remove control characters and other problematic Unicode symbols
		text.chars()
			.filter(|&c| {
				// Keep printable characters, including emojis
				!c.is_control() && (c.is_ascii_graphic() || Self::is_safe_unicode(c))
			})
			.collect()
	}

	/// Check if a character is a safe Unicode character (including emojis)
	fn is_safe_unicode(c: char) -> bool {
		// Allow a broader range of Unicode characters, including emojis
		let code = c as u32;
		// Emoji ranges and other safe Unicode ranges
		matches!(code,
			// Emoji ranges
			0x1F600..=0x1F64F | // Emoticons
			0x1F300..=0x1F5FF | // Misc Symbols and Pictographs
			0x1F680..=0x1F6FF | // Transport and Map
			0x1F1E6..=0x1F1FF | // Regional indicators
			0x2600..=0x26FF   | // Misc symbols
			0x2700..=0x27BF   | // Dingbats

			// Allow variation selectors and zero-width joiner
			0xFE0F | 0x200D    |

			// Some additional safe Unicode ranges
			0x0080..=0x00FF   | // Latin-1 Supplement
			0x0100..=0x017F   | // Latin Extended-A
			0x0180..=0x024F   | // Latin Extended-B
			0x0370..=0x03FF   | // Greek and Coptic
			0x0400..=0x04FF   | // Cyrillic
			0x0530..=0x058F   | // Armenian
			0x0590..=0x05FF   | // Hebrew
			0x0600..=0x06FF   | // Arabic
			0x0900..=0x097F   | // Devanagari
			0x4E00..=0x9FFF     // CJK Unified Ideographs
		)
	}

	/// Execute the memorize tool with enhanced error handling
	pub async fn execute_memorize(&self, arguments: &Value) -> Result<String> {
		// Validate input parameters exist before processing
		let title = arguments
			.get("title")
			.and_then(|v| v.as_str())
			.ok_or_else(|| anyhow::anyhow!("Missing required parameter 'title'"))?;

		let content = arguments
			.get("content")
			.and_then(|v| v.as_str())
			.ok_or_else(|| anyhow::anyhow!("Missing required parameter 'content'"))?;

		// Validate UTF-8 to prevent panics
		if !title.is_ascii() && std::str::from_utf8(title.as_bytes()).is_err() {
			return Err(anyhow::anyhow!("Invalid UTF-8 in title"));
		}
		if !content.is_ascii() && std::str::from_utf8(content.as_bytes()).is_err() {
			return Err(anyhow::anyhow!("Invalid UTF-8 in content"));
		}

		// Sanitize input content to prevent Unicode issues
		let title = Self::sanitize_content(title);
		let content = Self::sanitize_content(content);

		// Validate lengths
		if title.len() < 5 || title.len() > 200 {
			return Err(anyhow::anyhow!(
				"Title must be between 5 and 200 characters"
			));
		}
		if content.len() < 10 || content.len() > 10000 {
			return Err(anyhow::anyhow!(
				"Content must be between 10 and 10000 characters"
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

		// Process tags with error handling
		let tags = arguments.get("tags").and_then(|v| v.as_array()).map(|arr| {
			arr.iter()
				.filter_map(|v| {
					v.as_str().map(|s| {
						let sanitized = Self::sanitize_content(s);
						// Limit tag length to prevent issues
						if sanitized.len() > 50 {
							sanitized[..50].to_string()
						} else {
							sanitized
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
					.filter_map(|v| v.as_str().map(|s| s.to_string()))
					.take(20) // Limit number of files
					.collect::<Vec<String>>()
			});

		if self.debug {
			eprintln!(
				"Memorizing: title='{}', type='{:?}', importance={:?}",
				title, memory_type, importance
			);
		}

		// Change to working directory for Git context with error handling
		let original_dir = std::env::current_dir()
			.map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;

		if let Err(e) = std::env::set_current_dir(&self.working_directory) {
			return Err(anyhow::anyhow!(
				"Failed to change to working directory: {}",
				e
			));
		}

		let memory_result = {
			// Use timeout to prevent hanging
			let memory_future = async {
				let mut manager = self.memory_manager.lock().await;
				manager
					.memorize(
						memory_type,
						title.to_string(),
						content.to_string(),
						importance,
						tags,
						related_files,
					)
					.await
			};

			tokio::time::timeout(Duration::from_secs(30), memory_future)
				.await
				.map_err(|_| anyhow::anyhow!("Memory operation timed out"))?
		};

		// Restore original directory regardless of result
		if let Err(e) = std::env::set_current_dir(&original_dir) {
			eprintln!("Warning: Failed to restore original directory: {}", e);
		}

		let memory = memory_result?;

		// Return compact JSON response for token efficiency with safe serialization
		let response = json!({
			"success": 1,
			"memory_id": memory.id,
			"message": "Memory stored successfully"
		});

		// Ensure proper JSON encoding to avoid string slicing issues
		// Use to_string_pretty for better error handling with Unicode
		match serde_json::to_string_pretty(&response) {
			Ok(json_str) => Ok(json_str),
			Err(e) => {
				// Fallback to minimal response if JSON encoding fails
				log_critical_error("Memory response serialization failed", &e);
				Ok(format!(
					r#"{{"success": 1, "memory_id": "{}", "message": "Memory stored successfully"}}"#,
					memory.id
				))
			}
		}
	}

	/// Execute the remember tool
	pub async fn execute_remember(&self, arguments: &Value) -> Result<String> {
		let query = arguments
			.get("query")
			.and_then(|v| v.as_str())
			.ok_or_else(|| anyhow::anyhow!("Missing required parameter 'query'"))?;

		if query.len() < 3 || query.len() > 500 {
			return Err(anyhow::anyhow!(
				"Query must be between 3 and 500 characters"
			));
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
			.unwrap_or(10);

		let memory_query = MemoryQuery {
			memory_types,
			tags,
			related_files,
			limit: Some(limit.min(50)),
			..Default::default()
		};

		if self.debug {
			eprintln!(
				"Remembering: query='{}', limit={}",
				query,
				memory_query.limit.unwrap_or(10)
			);
		}

		let results = {
			let manager = self.memory_manager.lock().await;
			manager.remember(query, Some(memory_query)).await?
		};

		if results.is_empty() {
			let response = json!({
				"success": 1,
				"memories_found": 0,
				"message": "No stored memories match your query. Try using different search terms, removing filters, or checking if any memories have been stored yet."
			});
			return Ok(serde_json::to_string(&response)?);
		}

		// Create a structured response that is JSON-safe
		let mut memories_data = Vec::new();

		for (i, result) in results.iter().enumerate() {
			let memory_data = json!({
				"index": i + 1,
				"relevance_score": result.relevance_score,
				"memory_id": result.memory.id,
				"title": Self::sanitize_content(&result.memory.title),
				"memory_type": result.memory.memory_type.to_string(),
				"importance": result.memory.metadata.importance,
				"created_at": result.memory.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
				"git_commit": result.memory.metadata.git_commit.as_deref().unwrap_or("None"),
				"tags": if result.memory.metadata.tags.is_empty() {
					"None".to_string()
				} else {
					result.memory.metadata.tags.join(", ")
				},
				"related_files": if result.memory.metadata.related_files.is_empty() {
					"None".to_string()
				} else {
					result.memory.metadata.related_files.join(", ")
				},
				"content": Self::sanitize_content(&result.memory.content),
				"selection_reason": Self::sanitize_content(&result.selection_reason)
			});
			memories_data.push(memory_data);
		}

		// Create a structured JSON response
		let response = json!({
			"success": 1,
			"memories_found": results.len(),
			"memories": memories_data
		});

		// Return properly encoded JSON to avoid string slicing issues
		Ok(serde_json::to_string_pretty(&response)?)
	}

	/// Execute the forget tool
	pub async fn execute_forget(&self, arguments: &Value) -> Result<String> {
		// Check confirm parameter
		if !arguments
			.get("confirm")
			.and_then(|v| v.as_bool())
			.unwrap_or(false)
		{
			let response = json!({
				"success": 0,
				"error": "Missing required confirmation: set 'confirm' to true to proceed with deletion"
			});
			return Ok(serde_json::to_string(&response)?);
		}

		// Handle specific memory ID deletion
		if let Some(memory_id) = arguments.get("memory_id").and_then(|v| v.as_str()) {
			// Validate memory ID format
			if memory_id.trim().is_empty() || memory_id.len() > 100 {
				let response = json!({
					"success": 0,
					"error": "Invalid memory ID format"
				});
				return Ok(serde_json::to_string(&response)?);
			}

			if self.debug {
				eprintln!("Forgetting memory: id='{}'", memory_id);
			}

			// Execute deletion with timeout
			let res = {
				let delete_future = async {
					let mut manager = self.memory_manager.lock().await;
					manager.forget(memory_id).await
				};

				match tokio::time::timeout(Duration::from_secs(30), delete_future).await {
					Ok(result) => result,
					Err(_) => Err(anyhow::anyhow!("Memory deletion timed out")),
				}
			};
			match res {
				Ok(_) => {
					let response = json!({
						"success": 1,
						"memory_id": memory_id,
						"message": "Memory deleted successfully"
					});
					Ok(serde_json::to_string(&response)?)
				}
				Err(e) => {
					log_critical_anyhow_error("Memory deletion failed", &e);
					let response = json!({
						"success": 0,
						"memory_id": memory_id,
						"error": format!("Failed to delete memory: {}", e)
					});
					Ok(serde_json::to_string(&response)?)
				}
			}
		}
		// Handle query-based deletion
		else if let Some(query) = arguments.get("query").and_then(|v| v.as_str()) {
			// Validate UTF-8 to prevent panics
			if !query.is_ascii() && std::str::from_utf8(query.as_bytes()).is_err() {
				let response = json!({
					"success": 0,
					"error": "Invalid UTF-8 in query"
				});
				return Ok(serde_json::to_string(&response)?);
			}

			if query.len() < 3 || query.len() > 500 {
				let response = json!({
					"success": 0,
					"error": "Query must be between 3 and 500 characters"
				});
				return Ok(serde_json::to_string(&response)?);
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

			if self.debug {
				eprintln!("Forgetting memories matching: query='{}'", query);
			}

			let res = {
				let mut manager = self.memory_manager.lock().await;
				manager.forget_matching(memory_query).await
			};
			match res {
				Ok(deleted_count) => {
					let response = json!({
						"success": 1,
						"deleted_count": deleted_count,
						"message": format!("{} memories deleted successfully", deleted_count)
					});
					Ok(serde_json::to_string(&response)?)
				}
				Err(e) => {
					let response = json!({
						"success": 0,
						"error": format!("Failed to delete memories: {}", e)
					});
					Ok(serde_json::to_string(&response)?)
				}
			}
		} else {
			let response = json!({
				"success": 0,
				"error": "Either 'memory_id' or 'query' must be provided"
			});
			Ok(serde_json::to_string(&response)?)
		}
	}
}
