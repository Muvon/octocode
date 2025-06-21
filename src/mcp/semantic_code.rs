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
use crate::indexer::search::{
	search_codebase_with_details_multi_query_text, search_codebase_with_details_text,
};
use crate::indexer::{extract_file_signatures, render_signatures_text, NoindexWalker, PathUtils};
use crate::mcp::types::McpTool;

/// Semantic code search tool provider
#[derive(Clone)]
pub struct SemanticCodeProvider {
	config: Config,
	working_directory: std::path::PathBuf,
}

impl SemanticCodeProvider {
	pub fn new(config: Config, working_directory: std::path::PathBuf) -> Self {
		Self {
			config,
			working_directory,
		}
	}

	/// Get the tool definition for semantic_search
	pub fn get_tool_definition() -> McpTool {
		McpTool {
			name: "semantic_search".to_string(),
			description: "Search codebase using semantic search to find relevant code snippets, functions, classes, documentation, or text content. Multiple related queries in one call like ['user authentication flow', 'login validation', 'jwt token handling'] finds comprehensive results across all related concepts. 3x more efficient than separate searches with better context and relationships. Use descriptive phrases, NOT code syntax. Examples: ['user authentication flow', 'password validation logic'], ['database connection pooling', 'query result caching']. This is SEMANTIC search - use concepts and descriptions, NOT exact symbol names. Returns 3 most relevant results by default with file paths, 1-indexed line ranges, relevance scores, and code blocks with 1-indexed line numbers prefixed to each line.".to_string(),
			input_schema: json!({
				"type": "object",
				"properties": {
					"query": {
						"oneOf": [
							{
								"type": "string",
								"description": "Single search query - use ONLY for very specific searches. Express in human terms for semantic search, NOT exact code symbols",
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
								"description": "RECOMMENDED: Array of related search terms for comprehensive results. Example: ['user authentication flow', 'login middleware logic', 'jwt token validation'] finds all auth-related code in one search"
							}
						],
						"description": "PREFER ARRAY OF RELATED TERMS: ['user authentication flow', 'login session management', 'password validation'] for comprehensive search. Single string only for very specific searches. Use multi-term for: Feature exploration: ['database connection handling', 'query result processing', 'data persistence layer'], Related concepts: ['error handling patterns', 'exception recovery logic', 'failure notification'], System understanding: ['chat message routing', 'session state management', 'user interaction flow']. Use descriptive phrases, NOT code syntax. Examples: ['user authentication flow', 'password validation logic'], ['database connection pooling', 'query result caching']. This is SEMANTIC search - use concepts and descriptions, NOT exact symbol names."
					},
					"mode": {
						"type": "string",
						"description": "Scope of search to limit results to specific content types",
						"enum": ["code", "text", "docs", "all"],
						"default": "all",
						"enumDescriptions": {
							"code": "Search only in code blocks (functions, classes, methods, etc.)",
							"text": "Search only in plain text files and text content",
							"docs": "Search only in documentation files (README, markdown, etc.)",
							"all": "Search across all content types for comprehensive results"
						}
					},
					"detail_level": {
						"type": "string",
						"description": "Level of detail to include in code results for token efficiency",
						"enum": ["signatures", "partial", "full"],
						"default": "partial",
						"enumDescriptions": {
							"signatures": "Function/class signatures only (most token-efficient, good for overview)",
							"partial": "Smart truncated content with key parts (balanced approach)",
							"full": "Complete function/class bodies (use when full implementation needed)"
						}
					},
					"max_results": {
						"type": "integer",
						"description": "Maximum number of results to return (default: 3 for efficiency)",
						"minimum": 1,
						"maximum": 20,
						"default": 3
					}
				},
				"required": ["query"],
				"additionalProperties": false
			}),
		}
	}

	/// Get the tool definition for view_signatures
	pub fn get_view_signatures_tool_definition() -> McpTool {
		McpTool {
			name: "view_signatures".to_string(),
			description: "Extract and view function signatures, class definitions, and other meaningful code structures from files. Shows method signatures, class definitions, interfaces, and other declarations without full implementation details. Perfect for getting an overview of code structure and available APIs. Output includes 1-indexed line ranges and signature code with 1-indexed line numbers prefixed to each line.\n\nSupported Languages:\n- Rust (.rs): functions, structs, enums, traits, modules, constants, macros\n- JavaScript (.js): functions, methods, arrow functions\n- TypeScript (.ts, .tsx, .jsx): functions, methods, classes, interfaces, types\n- Python (.py): functions, classes, methods\n- Go (.go): functions, structs, interfaces, methods\n- C++ (.cpp, .cc, .cxx, .hpp, .h): functions, classes, structs, namespaces\n- PHP (.php): functions, classes, methods, traits\n- Ruby (.rb): classes, methods, modules\n- Bash (.sh, .bash): functions\n- JSON (.json): structure and key definitions\n- CSS (.css, .scss, .sass): rules, selectors, at-rules, keyframes\n- Svelte (.svelte): script blocks, style blocks, component elements with directives\n\nFor each supported language, extracts the most semantically meaningful constructs while filtering out noise and implementation details.".to_string(),
			input_schema: json!({
				"type": "object",
				"properties": {
					"files": {
						"type": "array",
						"description": "Array of file paths or glob patterns to analyze for signatures. Examples: ['src/main.rs'], ['**/*.py'], ['src/**/*.ts', 'lib/**/*.js'], ['**/*.css'], ['components/**/*.svelte'], ['**/*.{rs,py,js,ts,css,svelte}']",
						"items": {
							"type": "string",
							"description": "File path or glob pattern. Can be exact paths like 'src/main.rs' or patterns like '**/*.py' to match multiple files. Supports all programming languages: .rs, .js, .ts, .py, .go, .cpp, .php, .rb, .sh, .json, .css, .scss, .sass, .svelte"
						},
						"minItems": 1,
						"maxItems": 100
					}
				},
				"required": ["files"],
				"additionalProperties": false
			}),
		}
	}

	/// Execute the semantic_search tool
	pub async fn execute_search(&self, arguments: &Value) -> Result<String> {
		// Parse queries - handle both string and array inputs
		let queries: Vec<String> = match arguments.get("query") {
			Some(Value::String(s)) => vec![s.clone()],
			Some(Value::Array(arr)) => {
				let queries: Vec<String> = arr
					.iter()
					.filter_map(|v| v.as_str().map(String::from))
					.collect();

				if queries.is_empty() {
					return Err(anyhow::anyhow!(
						"Invalid query array: must contain at least one non-empty string"
					));
				}

				queries
			}
			_ => {
				return Err(anyhow::anyhow!(
					"Missing required parameter 'query': must be a string or array of strings describing what to search for"
				));
			}
		};

		// Validate queries
		if queries.len() > 5 {
			return Err(anyhow::anyhow!(
				"Too many queries: maximum 5 queries allowed, got {}. Use fewer, more specific terms.",
				queries.len()
			));
		}

		for (i, query) in queries.iter().enumerate() {
			// Ensure clean UTF-8 and validate query
			let clean_query = String::from_utf8_lossy(query.as_bytes()).to_string();
			let query = clean_query.trim();

			if query.len() < 3 {
				return Err(anyhow::anyhow!(
					"Invalid query {}: must be at least 3 characters long",
					i + 1
				));
			}
			if query.len() > 500 {
				return Err(anyhow::anyhow!(
					"Invalid query {}: must be no more than 500 characters long",
					i + 1
				));
			}
			if query.is_empty() {
				return Err(anyhow::anyhow!(
					"Invalid query {}: cannot be empty or whitespace only",
					i + 1
				));
			}
		}

		let mode = arguments
			.get("mode")
			.and_then(|v| v.as_str())
			.unwrap_or("all");

		// Validate mode
		if !["code", "text", "docs", "all"].contains(&mode) {
			return Err(anyhow::anyhow!(
				"Invalid mode '{}': must be one of 'code', 'text', 'docs', or 'all'",
				mode
			));
		}

		let detail_level = arguments
			.get("detail_level")
			.and_then(|v| v.as_str())
			.unwrap_or("partial");

		// Validate detail_level
		if !["signatures", "partial", "full"].contains(&detail_level) {
			return Err(anyhow::anyhow!(
				"Invalid detail_level '{}': must be one of 'signatures', 'partial', or 'full'",
				detail_level
			));
		}

		let max_results = arguments
			.get("max_results")
			.and_then(|v| v.as_u64())
			.unwrap_or(3) as usize;

		// Validate max_results
		if !(1..=20).contains(&max_results) {
			return Err(anyhow::anyhow!(
				"Invalid max_results '{}': must be between 1 and 20",
				max_results
			));
		}

		// Use structured logging instead of console output for MCP protocol compliance
		debug!(
			queries = ?queries,
			mode = %mode,
			detail_level = %detail_level,
			max_results = %max_results,
			working_directory = %self.working_directory.display(),
			"Executing semantic code search with {} queries",
			queries.len()
		);

		// Change to the working directory for the search with enhanced error handling
		let original_dir = match std::env::current_dir() {
			Ok(dir) => dir,
			Err(e) => {
				return Err(anyhow::anyhow!("Failed to get current directory: {}", e));
			}
		};

		if let Err(e) = std::env::set_current_dir(&self.working_directory) {
			return Err(anyhow::anyhow!(
				"Failed to change to working directory '{}': {}",
				self.working_directory.display(),
				e
			));
		}

		// Use the enhanced search functionality with multi-query support - TEXT FORMAT for token efficiency
		let results = if queries.len() == 1 {
			// Single query - use text function for token efficiency
			search_codebase_with_details_text(
				&queries[0],
				mode,
				detail_level,
				max_results,
				&self.config,
			)
			.await
		} else {
			// Multi-query - use text function for token efficiency
			search_codebase_with_details_multi_query_text(
				&queries,
				mode,
				detail_level,
				max_results,
				&self.config,
			)
			.await
		};

		// Restore original directory with enhanced error handling
		if let Err(e) = std::env::set_current_dir(&original_dir) {
			// Log error but don't fail the operation
			debug!(
				error = %e,
				original_dir = %original_dir.display(),
				"Failed to restore original directory"
			);
		}

		results
	}

	/// Execute the view_signatures tool
	pub async fn execute_view_signatures(&self, arguments: &Value) -> Result<String> {
		let files_array = arguments
			.get("files")
			.and_then(|v| v.as_array())
			.ok_or_else(|| anyhow::anyhow!("Missing required parameter 'files': must be an array of file paths or glob patterns"))?;

		// Validate files array
		if files_array.is_empty() {
			return Err(anyhow::anyhow!(
				"Invalid files parameter: array must contain at least one file path or pattern"
			));
		}
		if files_array.len() > 100 {
			return Err(anyhow::anyhow!(
				"Invalid files parameter: array must contain no more than 100 patterns"
			));
		}

		// Extract file patterns with enhanced validation
		let mut file_patterns = Vec::new();
		for file_value in files_array {
			let pattern = file_value.as_str().ok_or_else(|| {
				anyhow::anyhow!("Invalid file pattern: all items in files array must be strings")
			})?;

			if pattern.trim().is_empty() {
				return Err(anyhow::anyhow!(
					"Invalid file pattern: patterns cannot be empty"
				));
			}

			// Ensure clean UTF-8 for file patterns
			let clean_pattern = String::from_utf8_lossy(pattern.as_bytes()).to_string();
			let pattern = clean_pattern.trim();

			if pattern.len() > 500 {
				return Err(anyhow::anyhow!(
					"Invalid file pattern '{}': must be no more than 500 characters long",
					pattern
				));
			}

			// Basic path traversal protection
			if pattern.contains("..") && (pattern.contains("../") || pattern.contains("..\\")) {
				return Err(anyhow::anyhow!(
					"Invalid file pattern '{}': path traversal not allowed",
					pattern
				));
			}

			file_patterns.push(pattern.to_string());
		}

		// Use structured logging instead of console output for MCP protocol compliance
		debug!(
			file_patterns = ?file_patterns,
			working_directory = %self.working_directory.display(),
			"Executing view_signatures"
		);

		// Change to the working directory for processing with enhanced error handling
		let original_dir = match std::env::current_dir() {
			Ok(dir) => dir,
			Err(e) => {
				return Err(anyhow::anyhow!("Failed to get current directory: {}", e));
			}
		};

		if let Err(e) = std::env::set_current_dir(&self.working_directory) {
			return Err(anyhow::anyhow!(
				"Failed to change to working directory '{}': {}",
				self.working_directory.display(),
				e
			));
		}

		// Get files matching patterns
		let mut matching_files = std::collections::HashSet::new();

		// Compile all glob patterns first
		let mut compiled_patterns = Vec::new();
		for pattern in &file_patterns {
			let glob_pattern = match globset::Glob::new(pattern) {
				Ok(g) => g.compile_matcher(),
				Err(e) => {
					// Restore directory before returning error
					if let Err(restore_err) = std::env::set_current_dir(&original_dir) {
						debug!(
							error = %restore_err,
							"Failed to restore directory after glob error"
						);
					}
					return Err(anyhow::anyhow!("Invalid glob pattern '{}': {}", pattern, e));
				}
			};
			compiled_patterns.push(glob_pattern);
		}

		// Walk the directory tree once and test all patterns
		let walker = NoindexWalker::create_walker(&self.working_directory).build();

		for result in walker {
			let entry = match result {
				Ok(entry) => entry,
				Err(_) => continue,
			};

			// Skip directories, only process files
			if !entry.file_type().is_some_and(|ft| ft.is_file()) {
				continue;
			}

			// Get relative path once
			let relative_path =
				PathUtils::to_relative_string(entry.path(), &self.working_directory);

			// Test against all patterns
			for glob_pattern in &compiled_patterns {
				if glob_pattern.is_match(&relative_path) {
					matching_files.insert(entry.path().to_path_buf());
					break; // No need to test other patterns for this file
				}
			}
		}

		// Convert HashSet back to Vec
		let matching_files: Vec<_> = matching_files.into_iter().collect();

		if matching_files.is_empty() {
			// Restore directory before returning
			if let Err(restore_err) = std::env::set_current_dir(&original_dir) {
				debug!(
					error = %restore_err,
					"Failed to restore directory when no files found"
				);
			}
			return Ok("No matching files found for the specified patterns.".to_string());
		}

		// Extract signatures from matching files
		let signatures = match extract_file_signatures(&matching_files) {
			Ok(sigs) => sigs,
			Err(e) => {
				// Restore directory before returning error
				if let Err(restore_err) = std::env::set_current_dir(&original_dir) {
					debug!(
						error = %restore_err,
						"Failed to restore directory after signature extraction error"
					);
				}
				return Err(anyhow::anyhow!("Failed to extract signatures: {}", e));
			}
		};

		// Restore original directory with enhanced error handling
		if let Err(e) = std::env::set_current_dir(&original_dir) {
			// Log error but don't fail the operation
			debug!(
				error = %e,
				original_dir = %original_dir.display(),
				"Failed to restore original directory after signature extraction"
			);
		}

		// Return text format for token efficiency
		let text_output = render_signatures_text(&signatures);
		Ok(text_output)
	}
}
