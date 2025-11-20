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
use crate::indexer::search::{
	search_codebase_with_details_multi_query_text, search_codebase_with_details_text,
};
use crate::indexer::{extract_file_signatures, render_signatures_text, NoindexWalker, PathUtils};
use crate::mcp::types::{McpError, McpTool};
use octolib::embedding::constants::MAX_QUERIES;

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
			description: "Search codebase using semantic search to find relevant code snippets, functions, classes, documentation, or text content. This tool finds code by understanding what it does, not by exact symbol names. Use descriptive phrases about functionality and behavior, not function names or single words. Multiple related queries in one call like ['user authentication flow', 'login validation', 'jwt token handling'] finds comprehensive results across all related concepts. Examples: ['user authentication flow', 'password validation logic'], ['database connection pooling', 'query result caching']. Returns 3 most relevant results by default with file paths, 1-indexed line ranges, relevance scores, and code blocks with 1-indexed line numbers prefixed to each line.".to_string(),
			input_schema: json!({
				"type": "object",
				"properties": {
					"query": {
						"oneOf": [
							{
								"type": "string",
								"description": "Single search query - use for specific conceptual searches. Describe what the code does or how it works, not symbol names or single words",
								"minLength": 10,
								"maxLength": 500
							},
							{
								"type": "array",
								"items": {
									"type": "string",
									"minLength": 10,
									"maxLength": 500
								},
								"minItems": 1,
								"maxItems": 5,
								"description": "Recommended: Array of related conceptual search terms for comprehensive results. Example: ['user authentication workflow', 'login session handling', 'password validation rules'] finds all auth-related code in one search"
							}
						],
						"description": "Prefer array of related terms for comprehensive search: ['user authentication flow', 'login session management', 'password validation'] finds all related functionality. Single string only for very specific searches. Use descriptive phrases about what the code does, not exact symbol names. Examples: ['user authentication flow', 'password validation logic'], ['database connection pooling', 'query result caching']. This is semantic search - describe functionality and behavior, not code symbols."
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
					},
					"threshold": {
						"type": "number",
						"description": "Similarity threshold (0.0-1.0). Higher values = more similar results only. Defaults to config.search.similarity_threshold",
						"minimum": 0.0,
						"maximum": 1.0
					},
					"language": {
						"type": "string",
						"description": "Filter by programming language (only affects code blocks). Supported languages: rust, javascript, typescript, python, go, cpp, php, bash, ruby, json, svelte, css"
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

	/// Get the tool definition for view_signatures
	pub fn get_view_signatures_tool_definition() -> McpTool {
		McpTool {
			name: "view_signatures".to_string(),
			description: "Extract and view function signatures, class definitions, and other meaningful code structures from files. Shows method signatures, class definitions, interfaces, and other declarations without full implementation details. Perfect for getting an overview of code structure and available APIs. Output includes 1-indexed line ranges and signature code with 1-indexed line numbers prefixed to each line.\nSupported Languages:\nRust, JavaScript, TypeScript, Python, Go, C++, PHP, Ruby, Bash, JSON, CSS, Svelte, Markdown.\nSignatures are the structural definitions of code elements without their implementation details. They include function declarations, method headers, class definitions, interfaces, types, and other high-level constructs that define the API and architecture of code. Signatures provide a concise overview of what functionality exists and how it can be accessed, without showing the actual implementation logic.".to_string(),
			input_schema: json!({
				"type": "object",
				"properties": {
					"files": {
						"type": "array",
						"description": "Array of file paths or glob patterns to analyze for signatures. Examples: ['src/main.rs'], ['**/*.py'], ['src/**/*.ts', 'lib/**/*.js'], ['**/*.css'], ['components/**/*.svelte'], ['**/*.{rs,py,js,ts,css,svelte,md}']",
						"items": {
							"type": "string",
							"description": "File path or glob pattern. Can be exact paths like 'src/main.rs' or patterns like '**/*.py' to match multiple files. Supports all programming languages: .rs, .js, .ts, .py, .go, .cpp, .php, .rb, .sh, .json, .css, .scss, .sass, .svelte, .md"
						},
						"minItems": 1,
						"maxItems": 100
					},
					"max_tokens": {
						"type": "integer",
						"description": "Maximum tokens allowed in output before truncation (default: 2000, set to 0 for unlimited)",
						"minimum": 0,
						"default": 2000
					}
				},
				"required": ["files"],
				"additionalProperties": false
			}),
		}
	}

	/// Execute the semantic_search tool
	pub async fn execute_search(&self, arguments: &Value) -> Result<String, McpError> {
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
						"semantic_search",
					));
				}

				queries
			}
			_ => {
				return Err(McpError::invalid_params(
					"Missing required parameter 'query': must be a string or array of strings describing what to search for",
					"semantic_search"
				));
			}
		};

		// Validate queries
		if queries.len() > MAX_QUERIES {
			return Err(McpError::invalid_params(
				format!("Too many queries: maximum {} queries allowed, got {}. Use fewer, more specific terms.", MAX_QUERIES, queries.len()),
				"semantic_search"
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
					"semantic_search",
				));
			}
			if query.len() > 500 {
				return Err(McpError::invalid_params(
					format!(
						"Invalid query {}: must be no more than 500 characters long",
						i + 1
					),
					"semantic_search",
				));
			}
			if query.is_empty() {
				return Err(McpError::invalid_params(
					format!(
						"Invalid query {}: cannot be empty or whitespace only",
						i + 1
					),
					"semantic_search",
				));
			}
		}

		let mode = arguments
			.get("mode")
			.and_then(|v| v.as_str())
			.unwrap_or("all");

		// Validate mode
		if !["code", "text", "docs", "all"].contains(&mode) {
			return Err(McpError::invalid_params(
				format!(
					"Invalid mode '{}': must be one of 'code', 'text', 'docs', or 'all'",
					mode
				),
				"semantic_search",
			));
		}

		let detail_level = arguments
			.get("detail_level")
			.and_then(|v| v.as_str())
			.unwrap_or("partial");

		// Validate detail_level
		if !["signatures", "partial", "full"].contains(&detail_level) {
			return Err(McpError::invalid_params(
				format!(
					"Invalid detail_level '{}': must be one of 'signatures', 'partial', or 'full'",
					detail_level
				),
				"semantic_search",
			));
		}

		let max_results = arguments
			.get("max_results")
			.and_then(|v| v.as_u64())
			.unwrap_or(3) as usize;

		// Validate max_results
		if !(1..=20).contains(&max_results) {
			return Err(McpError::invalid_params(
				format!(
					"Invalid max_results '{}': must be between 1 and 20",
					max_results
				),
				"semantic_search",
			));
		}

		let similarity_threshold = arguments
			.get("threshold")
			.and_then(|v| v.as_f64())
			.map(|v| v as f32)
			.unwrap_or(self.config.search.similarity_threshold);

		// Validate similarity threshold
		if !(0.0..=1.0).contains(&similarity_threshold) {
			return Err(McpError::invalid_params(
				format!(
					"Invalid similarity threshold '{}': must be between 0.0 and 1.0",
					similarity_threshold
				),
				"semantic_search",
			));
		}

		// Parse and validate language filter if provided
		let language_filter = if let Some(language_value) = arguments.get("language") {
			let language = language_value.as_str().ok_or_else(|| {
				McpError::invalid_params(
					"Invalid language parameter: must be a string",
					"semantic_search",
				)
			})?;

			// Validate language using existing language registry
			use crate::indexer::languages;
			if languages::get_language(language).is_none() {
				return Err(McpError::invalid_params(
					format!("Invalid language '{}': supported languages are rust, javascript, typescript, python, go, cpp, php, bash, ruby, json, svelte, css", language),
					"semantic_search"
				));
			}

			Some(language.to_string())
		} else {
			None
		};

		// Parse max_tokens parameter
		let max_tokens = arguments
			.get("max_tokens")
			.and_then(|v| v.as_u64())
			.unwrap_or(2000) as usize;

		// Use structured logging instead of console output for MCP protocol compliance
		debug!(
			queries = ?queries,
			mode = %mode,
			detail_level = %detail_level,
			max_results = %max_results,
			similarity_threshold = %similarity_threshold,
			language_filter = ?language_filter,
			working_directory = %self.working_directory.display(),
			"Executing semantic code search with {} queries",
			queries.len()
		);

		// Change to the working directory for the search with enhanced error handling
		let original_dir = match std::env::current_dir() {
			Ok(dir) => dir,
			Err(e) => {
				return Err(McpError::internal_error(
					format!("Failed to get current directory: {}", e),
					"semantic_search",
				));
			}
		};

		if let Err(e) = std::env::set_current_dir(&self.working_directory) {
			return Err(McpError::internal_error(
				format!(
					"Failed to change to working directory '{}': {}",
					self.working_directory.display(),
					e
				),
				"semantic_search",
			)
			.with_details(format!("Path: {}", self.working_directory.display())));
		}

		// Use the enhanced search functionality with multi-query support - TEXT FORMAT for token efficiency
		let results = if queries.len() == 1 {
			// Single query - use text function for token efficiency
			search_codebase_with_details_text(
				&queries[0],
				mode,
				detail_level,
				max_results,
				similarity_threshold,
				language_filter.as_deref(),
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
				similarity_threshold,
				language_filter.as_deref(),
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

		// Apply token truncation if needed
		match results {
			Ok(output) => Ok(truncate_output(&output, max_tokens)),
			Err(e) => Err(McpError::internal_error(
				format!("Search operation failed: {}", e),
				"semantic_search",
			)),
		}
	}

	/// Execute the view_signatures tool
	pub async fn execute_view_signatures(&self, arguments: &Value) -> Result<String, McpError> {
		let files_array = arguments
			.get("files")
			.and_then(|v| v.as_array())
			.ok_or_else(|| McpError::invalid_params("Missing required parameter 'files': must be an array of file paths or glob patterns", "view_signatures"))?;

		// Validate files array
		if files_array.is_empty() {
			return Err(McpError::invalid_params(
				"Invalid files parameter: array must contain at least one file path or pattern",
				"view_signatures",
			));
		}
		if files_array.len() > 100 {
			return Err(McpError::invalid_params(
				"Invalid files parameter: array must contain no more than 100 patterns",
				"view_signatures",
			));
		}

		// Extract file patterns with enhanced validation
		let mut file_patterns = Vec::new();
		for file_value in files_array {
			let pattern = file_value.as_str().ok_or_else(|| {
				McpError::invalid_params(
					"Invalid file pattern: all items in files array must be strings",
					"view_signatures",
				)
			})?;

			if pattern.trim().is_empty() {
				return Err(McpError::invalid_params(
					"Invalid file pattern: patterns cannot be empty",
					"view_signatures",
				));
			}

			// Ensure clean UTF-8 for file patterns
			let clean_pattern = String::from_utf8_lossy(pattern.as_bytes()).to_string();
			let pattern = clean_pattern.trim();

			if pattern.len() > 500 {
				return Err(McpError::invalid_params(
					format!(
						"Invalid file pattern '{}': must be no more than 500 characters long",
						pattern
					),
					"view_signatures",
				));
			}

			// Basic path traversal protection
			if pattern.contains("..") && (pattern.contains("../") || pattern.contains("..\\")) {
				return Err(McpError::invalid_params(
					format!(
						"Invalid file pattern '{}': path traversal not allowed",
						pattern
					),
					"view_signatures",
				));
			}

			file_patterns.push(pattern.to_string());
		}

		// Parse max_tokens parameter
		let max_tokens = arguments
			.get("max_tokens")
			.and_then(|v| v.as_u64())
			.unwrap_or(2000) as usize;

		// Use structured logging instead of console output for MCP protocol compliance
		debug!(
			file_patterns = ?file_patterns,
			working_directory = %self.working_directory.display(),
			"Executing view_signatures"
		);

		// Get files matching patterns
		let mut matching_files = std::collections::HashSet::new();

		// Compile all glob patterns first
		let mut compiled_patterns = Vec::new();
		for pattern in &file_patterns {
			let glob_pattern = match globset::Glob::new(pattern) {
				Ok(g) => g.compile_matcher(),
				Err(e) => {
					return Err(McpError::invalid_params(
						format!("Invalid glob pattern '{}': {}", pattern, e),
						"view_signatures",
					));
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
			return Ok("No matching files found for the specified patterns.".to_string());
		}

		// Extract signatures from matching files
		let signatures = match extract_file_signatures(&matching_files) {
			Ok(sigs) => sigs,
			Err(e) => {
				return Err(McpError::internal_error(
					format!("Failed to extract signatures: {}", e),
					"view_signatures",
				));
			}
		};

		// Return text format for token efficiency
		let text_output = render_signatures_text(&signatures);

		// Apply token truncation if needed
		Ok(truncate_output(&text_output, max_tokens))
	}
}
