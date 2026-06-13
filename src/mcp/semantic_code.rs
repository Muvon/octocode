// Copyright 2026 Muvon Un Limited
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
use serde_json::Value;
use tracing::debug;

use crate::config::Config;
use crate::indexer::search::{
	search_codebase_with_details_multi_query_text, search_codebase_with_details_text,
	DetailSearchOptions,
};
use crate::indexer::{extract_file_signatures, render_signatures_text, NoindexWalker, PathUtils};
use crate::mcp::types::McpError;
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
		if !["code", "text", "docs", "commits", "all"].contains(&mode) {
			return Err(McpError::invalid_params(
				format!(
					"Invalid mode '{}': must be one of 'code', 'text', 'docs', 'commits', or 'all'",
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
		let language_filter = if let Some(language_value) =
			arguments.get("language").filter(|v| !v.is_null())
		{
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

		// Search the project at its known path. The store and branch context are
		// resolved from `working_directory`, so there's no process-wide CWD change
		// and concurrent searches across different repos can't race.
		let options = DetailSearchOptions {
			mode,
			detail_level,
			max_results,
			similarity_threshold,
			language_filter: language_filter.as_deref(),
			config: &self.config,
			working_directory: &self.working_directory,
		};
		let results = if queries.len() == 1 {
			// Single query - use text function for token efficiency
			search_codebase_with_details_text(&queries[0], &options).await
		} else {
			// Multi-query - use text function for token efficiency
			search_codebase_with_details_multi_query_text(&queries, &options).await
		};

		match results {
			Ok(output) => Ok(self.steer_if_empty(output)),
			Err(e) => Err(McpError::internal_error(
				format!("Search operation failed: {}", e),
				"semantic_search",
			)),
		}
	}

	/// In read-only mode (`index.mcp_index = false`) the index may be empty or stale,
	/// so a no-match result is appended with a one-line nudge toward the structural
	/// tools rather than leaving the model to retry semantic search fruitlessly.
	fn steer_if_empty(&self, output: String) -> String {
		if self.config.index.mcp_index {
			return output;
		}
		let empty = matches!(
			output.trim(),
			"No results found."
				| "No code results found."
				| "No text results found."
				| "No documentation results found."
				| "No commit results found."
		);
		if empty {
			format!(
				"{output}\n\nThe semantic index is read-only here (index.mcp_index = false) and \
				 may be empty or stale. Use `structural_search` for known symbols/patterns/usages \
				 and `view_signatures` to map files, or fall back to grep."
			)
		} else {
			output
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

		// Use structured logging instead of console output for MCP protocol compliance
		debug!(
			file_patterns = ?file_patterns,
			working_directory = %self.working_directory.display(),
			"Executing view_signatures"
		);

		// Resolve patterns: direct file paths first, then glob-walk for wildcards
		let mut matching_files = std::collections::HashSet::new();
		let mut glob_patterns = Vec::new();

		for pattern in &file_patterns {
			let pattern_path = if std::path::Path::new(pattern).is_relative() {
				self.working_directory.join(pattern)
			} else {
				std::path::PathBuf::from(pattern)
			};

			if pattern_path.is_file() {
				// Direct file path — use it immediately, no glob walking needed
				matching_files.insert(pattern_path);
			} else {
				// Not a direct file — treat as glob pattern
				let glob_pattern = match globset::Glob::new(pattern) {
					Ok(g) => g.compile_matcher(),
					Err(e) => {
						return Err(McpError::invalid_params(
							format!("Invalid glob pattern '{}': {}", pattern, e),
							"view_signatures",
						));
					}
				};
				glob_patterns.push(glob_pattern);
			}
		}

		// Only walk the directory tree if there are actual glob patterns to resolve
		if !glob_patterns.is_empty() {
			let walker = NoindexWalker::create_walker(&self.working_directory).build();

			for result in walker {
				let entry = match result {
					Ok(entry) => entry,
					Err(_) => continue,
				};

				if !entry.file_type().is_some_and(|ft| ft.is_file()) {
					continue;
				}

				let relative_path =
					PathUtils::to_relative_string(entry.path(), &self.working_directory);

				for glob_pattern in &glob_patterns {
					if glob_pattern.is_match(&relative_path) {
						matching_files.insert(entry.path().to_path_buf());
						break;
					}
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

		Ok(text_output)
	}
}
