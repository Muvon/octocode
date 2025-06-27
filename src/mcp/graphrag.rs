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
use tracing::debug;

use crate::config::Config;
use crate::embedding::truncate_output;
use crate::indexer::graphrag::GraphRAG;
use crate::mcp::types::{McpError, McpTool};

/// GraphRAG tool provider
#[derive(Clone)]
pub struct GraphRagProvider {
	graphrag: GraphRAG,
	working_directory: std::path::PathBuf,
}

impl GraphRagProvider {
	pub fn new(config: Config, working_directory: std::path::PathBuf) -> Option<Self> {
		if config.graphrag.enabled {
			Some(Self {
				graphrag: GraphRAG::new(config),
				working_directory,
			})
		} else {
			None
		}
	}

	/// Get the tool definition for graphrag_search
	pub fn get_tool_definition() -> McpTool {
		McpTool {
			name: "graphrag_search".to_string(),
			description: "Advanced relationship-aware search using GraphRAG (Graph Retrieval-Augmented Generation). This tool understands code relationships, dependencies, and semantic connections between different parts of the codebase. USE THIS TOOL for complex architectural queries about component interactions, data flows, dependency relationships, and cross-cutting concerns. DO NOT use for simple code searches - use semantic_search instead for finding specific functions or classes.".to_string(),
			input_schema: json!({
				"type": "object",
				"properties": {
					"query": {
						"type": "string",
						"description": "Complex architectural query about code relationships, dependencies, or system interactions. GOOD examples: 'How does user authentication flow through the system?', 'What components depend on the database layer?', 'Show me the data flow for order processing', 'Find all error handling patterns across modules', 'How are configuration settings propagated through the application?'. BAD examples: 'find login function', 'get user class', 'show database code' (use semantic_search for these)",
						"minLength": 10,
						"maxLength": 1000
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
		}
	}

	/// Execute the graphrag_search tool
	pub async fn execute_search(&self, arguments: &Value) -> Result<String, McpError> {
		let query = arguments
			.get("query")
			.and_then(|v| v.as_str())
			.ok_or_else(|| McpError::invalid_params("Missing required parameter 'query': must be a detailed question about code relationships or architecture", "graphrag_search"))?;

		// Validate query length for GraphRAG (should be more detailed)
		if query.len() < 10 {
			return Err(McpError::invalid_params("Invalid GraphRAG query: must be at least 10 characters long and describe relationships or architecture", "graphrag_search"));
		}
		if query.len() > 1000 {
			return Err(McpError::invalid_params(
				"Invalid GraphRAG query: must be no more than 1000 characters long",
				"graphrag_search",
			));
		}

		// Parse max_tokens parameter
		let max_tokens = arguments
			.get("max_tokens")
			.and_then(|v| v.as_u64())
			.unwrap_or(2000) as usize;

		// Use structured logging instead of console output for MCP protocol compliance
		debug!(
			query = %query,
			working_directory = %self.working_directory.display(),
			"Executing GraphRAG search"
		);

		// Change to the working directory for the search
		let original_dir = std::env::current_dir().map_err(|e| {
			McpError::internal_error(
				format!("Failed to get current directory: {}", e),
				"graphrag_search",
			)
		})?;
		std::env::set_current_dir(&self.working_directory).map_err(|e| {
			McpError::internal_error(
				format!("Failed to change directory: {}", e),
				"graphrag_search",
			)
		})?;

		let results = self.graphrag.search(query).await.map_err(|e| {
			McpError::internal_error(format!("GraphRAG search failed: {}", e), "graphrag_search")
		})?;

		// Restore original directory
		std::env::set_current_dir(&original_dir).map_err(|e| {
			McpError::internal_error(
				format!("Failed to restore directory: {}", e),
				"graphrag_search",
			)
		})?;

		// Apply token truncation if needed
		Ok(truncate_output(&results, max_tokens))
	}
}
