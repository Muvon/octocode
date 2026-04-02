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

// Module for search functionality

use crate::config::Config;
use crate::store::{CodeBlock, Store};
use anyhow::Result;
use std::collections::HashSet;

/// Helper function to format symbols for display without cloning
/// Returns a formatted string with deduplicated, sorted, and filtered symbols
fn format_symbols_for_display(symbols: &[String]) -> String {
	let mut unique_symbols: Vec<&str> = symbols
		.iter()
		.map(|s| s.as_str())
		.filter(|s| !s.contains('_'))
		.collect();
	unique_symbols.sort();
	unique_symbols.dedup();
	unique_symbols.join(", ")
}

// Render code blocks in a user-friendly format
pub fn render_code_blocks(blocks: &[CodeBlock]) {
	render_code_blocks_with_config(blocks, &Config::default(), "partial");
}

// Render code blocks in a user-friendly format with configuration
pub fn render_code_blocks_with_config(blocks: &[CodeBlock], config: &Config, detail_level: &str) {
	if blocks.is_empty() {
		println!("No code blocks found for the query.");
		return;
	}

	println!("Found {} code blocks:\n", blocks.len());

	// Display results in their sorted order (most relevant first)
	for (idx, block) in blocks.iter().enumerate() {
		println!(
			"╔══════════════════ File: {} ══════════════════",
			block.path
		);
		println!("║");
		println!("║ Result {} of {}", idx + 1, blocks.len());
		println!("║ Language: {}", block.language);
		println!("║ Lines: {}-{}", block.start_line, block.end_line);

		// Show similarity score if available
		if let Some(distance) = block.distance {
			println!("║ Similarity: {:.4}", 1.0 - distance);
		}

		if !block.symbols.is_empty() {
			println!("║ Symbols:");
			let formatted = format_symbols_for_display(&block.symbols);
			if !formatted.is_empty() {
				for symbol in formatted.split(", ") {
					println!("║   • {}", symbol);
				}
			}
		}

		println!("║ Content:");
		println!("║ ┌────────────────────────────────────");

		// Use detail level for content display (consistent with text/doc CLI format)
		match detail_level {
			"signatures" => {
				// Show smart preview for signatures mode
				let lines: Vec<&str> = block.content.lines().collect();
				if !lines.is_empty() {
					if let Some(first_line) = lines.first() {
						println!("║ │ {:4} │ {}", block.start_line, first_line.trim());
					}
				}
			}
			"partial" => {
				// Show smart truncated content with line numbers (CLI format with │)
				let lines: Vec<&str> = block.content.lines().collect();
				if lines.len() <= 10 {
					// Show all lines if content is short
					for (i, line) in lines.iter().enumerate() {
						println!("║ │ {:4} │ {}", block.start_line + i + 1, line);
					}
				} else {
					// Smart truncation: first 4 lines
					for (i, line) in lines.iter().take(4).enumerate() {
						println!("║ │ {:4} │ {}", block.start_line + i + 1, line);
					}

					// Show separator with count
					let omitted_lines = lines.len() - 7; // 4 start + 3 end
					if omitted_lines > 0 {
						println!("║ │      │ ... ({} more lines)", omitted_lines);
					}

					// Last 3 lines
					let last_3_start = lines.len() - 3;
					for (i, line) in lines.iter().skip(last_3_start).enumerate() {
						println!(
							"║ │ {:4} │ {}",
							block.start_line + last_3_start + i + 1,
							line
						);
					}
				}
			}
			"full" => {
				// Show full content with line numbers (fallback to config-based truncation if too large)
				let max_chars = config.search.search_block_max_characters;
				if max_chars > 0 && block.content.len() > max_chars {
					let (content, was_truncated) =
						crate::indexer::truncate_content_smartly(&block.content, max_chars);
					// Add line numbers to truncated content with │ format
					for (i, line) in content.lines().enumerate() {
						println!("║ │ {:4} │ {}", block.start_line + i, line);
					}
					if was_truncated {
						println!(
							"║ │      │ [Content truncated - limit: {} chars]",
							max_chars
						);
					}
				} else {
					// Show full content with line numbers using │ format
					for (i, line) in block.content.lines().enumerate() {
						println!("║ │ {:4} │ {}", block.start_line + i, line);
					}
				}
			}
			_ => {
				// Default to partial
				let lines: Vec<&str> = block.content.lines().collect();
				if lines.len() <= 10 {
					for (i, line) in lines.iter().enumerate() {
						println!("║ │ {:4} │ {}", block.start_line + i + 1, line);
					}
				} else {
					// Smart truncation: first 4 lines
					for (i, line) in lines.iter().take(4).enumerate() {
						println!("║ │ {:4} │ {}", block.start_line + i + 1, line);
					}

					let omitted_lines = lines.len() - 7;
					if omitted_lines > 0 {
						println!("║ │      │ ... ({} more lines)", omitted_lines);
					}

					// Last 3 lines
					let last_3_start = lines.len() - 3;
					for (i, line) in lines.iter().skip(last_3_start).enumerate() {
						println!(
							"║ │ {:4} │ {}",
							block.start_line + last_3_start + i + 1,
							line
						);
					}
				}
			}
		}

		println!("║ └────────────────────────────────────");
		println!("╚════════════════════════════════════════\n");
	}
}

// Render search results as JSON
pub fn render_results_json(results: &[CodeBlock]) -> Result<(), anyhow::Error> {
	let json = serde_json::to_string_pretty(results)?;
	println!("{}", json);
	Ok(())
}

// Expand symbols in code blocks to include related code while maintaining relevance order
pub async fn expand_symbols(
	store: &Store,
	code_blocks: Vec<CodeBlock>,
) -> Result<Vec<CodeBlock>, anyhow::Error> {
	// We'll keep original blocks at the top with their original order
	let mut expanded_blocks = Vec::new();
	let mut original_hashes = HashSet::new();

	// Add original blocks and keep track of their hashes
	for block in &code_blocks {
		expanded_blocks.push(block.clone());
		original_hashes.insert(block.hash.clone());
	}

	let mut symbol_refs = Vec::new();

	// Collect all symbols from the code blocks
	for block in &code_blocks {
		for symbol in &block.symbols {
			// Skip the type symbols (like "function_definition") and only include actual named symbols
			if !symbol.contains("_") && symbol.chars().next().is_some_and(|c| c.is_alphabetic()) {
				symbol_refs.push(symbol.clone());
			}
		}
	}

	// Deduplicate symbols
	symbol_refs.sort();
	symbol_refs.dedup();

	println!("Found {} unique symbols to expand", symbol_refs.len());

	// Store expanded blocks to sort them by symbol count later
	let mut additional_blocks = Vec::new();

	// For each symbol, find code blocks that contain it
	for symbol in &symbol_refs {
		// Use a reference to avoid moving symbol_refs
		if let Some(block) = store.get_code_block_by_symbol(symbol).await? {
			// Check if we already have this block (avoid duplicates)
			if !original_hashes.contains(&block.hash)
				&& !additional_blocks
					.iter()
					.any(|b: &CodeBlock| b.hash == block.hash)
			{
				// Add dependencies we haven't seen before
				additional_blocks.push(block);
			}
		}
	}

	// Sort additional blocks by symbol count (more symbols = more relevant)
	// This is a heuristic to put more complex/relevant blocks first
	additional_blocks.sort_by(|a, b| {
		// First try to sort by number of matching symbols (more matches = more relevant)
		let a_matches = a.symbols.iter().filter(|s| symbol_refs.contains(s)).count();
		let b_matches = b.symbols.iter().filter(|s| symbol_refs.contains(s)).count();

		// Primary sort by symbol match count (descending)
		let match_cmp = b_matches.cmp(&a_matches);

		if match_cmp == std::cmp::Ordering::Equal {
			// Secondary sort by file path and line number when match counts are equal
			let path_cmp = a.path.cmp(&b.path);
			if path_cmp == std::cmp::Ordering::Equal {
				a.start_line.cmp(&b.start_line)
			} else {
				path_cmp
			}
		} else {
			match_cmp
		}
	});

	// Add the sorted additional blocks to our results
	expanded_blocks.extend(additional_blocks);

	Ok(expanded_blocks)
}

// Token-efficient text formatting functions for MCP

// Format code search results as text for MCP with detail level control
pub fn format_code_search_results_as_text(blocks: &[CodeBlock], detail_level: &str) -> String {
	if blocks.is_empty() {
		return "No code results found.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!("CODE RESULTS ({})\n", blocks.len()));

	for (idx, block) in blocks.iter().enumerate() {
		output.push_str(&format!("{}. {}\n", idx + 1, block.path));
		// output.push_str(&format!("{}-{}", block.start_line, block.end_line));

		if let Some(distance) = block.distance {
			output.push_str(&format!(" | Similarity {:.3}", 1.0 - distance));
		}
		output.push('\n');
		// Add symbols if available
		if !block.symbols.is_empty() {
			let formatted = format_symbols_for_display(&block.symbols);
			if !formatted.is_empty() {
				output.push_str(&format!("Symbols: {}\n", formatted));
			}
		}

		// Add content as-is without truncation for text mode - only efficient labels
		match detail_level {
			"signatures" => {
				// Extract just function/class signatures
				let preview =
					get_code_preview_with_lines(&block.content, block.start_line, &block.language);
				if !preview.is_empty() {
					if let Some(first_line) = preview.lines().next() {
						output.push_str(&format!("{}\n", first_line));
					}
				}
			}
			"partial" => {
				// Smart truncated content with line numbers
				let preview =
					get_code_preview_with_lines(&block.content, block.start_line, &block.language);
				output.push_str(&preview);
				if !preview.ends_with('\n') {
					output.push('\n');
				}
			}
			"full" => {
				// Full content with line numbers
				let content_with_lines = block
					.content
					.lines()
					.enumerate()
					.map(|(i, line)| format!("{}: {}", block.start_line + i, line))
					.collect::<Vec<_>>()
					.join("\n");
				output.push_str(&content_with_lines);
				if !content_with_lines.ends_with('\n') {
					output.push('\n');
				}
			}
			_ => {}
		}
		output.push('\n');
	}

	output
}

// Format text search results as text for MCP with detail level control
pub fn format_text_search_results_as_text(
	blocks: &[crate::store::TextBlock],
	detail_level: &str,
) -> String {
	if blocks.is_empty() {
		return "No text results found.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!("TEXT RESULTS ({})\n", blocks.len()));

	for (idx, block) in blocks.iter().enumerate() {
		output.push_str(&format!("{}. {}\n", idx + 1, block.path));
		// output.push_str(&format!("{}-{}", block.start_line, block.end_line));

		if let Some(distance) = block.distance {
			output.push_str(&format!(" | Similarity {:.3}", 1.0 - distance));
		}
		output.push('\n');

		// Add content with line numbers based on detail level
		match detail_level {
			"signatures" => {
				// Show smart preview for signatures mode
				let preview = get_text_preview_with_lines(&block.content, block.start_line);
				if !preview.is_empty() {
					if let Some(first_line) = preview.lines().next() {
						output.push_str(&format!("{}\n", first_line));
					}
				}
			}
			"partial" => {
				// Smart truncated content with line numbers
				let preview = get_text_preview_with_lines(&block.content, block.start_line);
				output.push_str(&preview);
				if !preview.ends_with('\n') {
					output.push('\n');
				}
			}
			"full" => {
				// Full content with line numbers
				let content_with_lines = block
					.content
					.lines()
					.enumerate()
					.map(|(i, line)| format!("{}: {}", block.start_line + i, line))
					.collect::<Vec<_>>()
					.join("\n");
				output.push_str(&content_with_lines);
				if !content_with_lines.ends_with('\n') {
					output.push('\n');
				}
			}
			_ => {}
		}
		output.push('\n');
	}

	output
}

// Format document search results as text for MCP with detail level control
pub fn format_doc_search_results_as_text(
	blocks: &[crate::store::DocumentBlock],
	detail_level: &str,
) -> String {
	if blocks.is_empty() {
		return "No documentation results found.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!("DOCUMENTATION RESULTS ({})\n", blocks.len()));

	for (idx, block) in blocks.iter().enumerate() {
		output.push_str(&format!("{}. {}\n", idx + 1, block.path));
		output.push_str(&format!("{} (Level {})", block.title, block.level));
		output.push_str(&format!(" | {}-{}", block.start_line, block.end_line));

		if let Some(distance) = block.distance {
			output.push_str(&format!(" | Similarity {:.3}", 1.0 - distance));
		}
		output.push('\n');

		// Add content with line numbers based on detail level
		match detail_level {
			"signatures" => {
				// Show smart preview for signatures mode
				let preview = get_doc_preview_with_lines(&block.content, block.start_line);
				if !preview.is_empty() {
					if let Some(first_line) = preview.lines().next() {
						output.push_str(&format!("{}\n", first_line));
					}
				}
			}
			"partial" => {
				// Smart truncated content with line numbers
				let preview = get_doc_preview_with_lines(&block.content, block.start_line);
				output.push_str(&preview);
				if !preview.ends_with('\n') {
					output.push('\n');
				}
			}
			"full" => {
				// Full content with line numbers
				let content_with_lines = block
					.content
					.lines()
					.enumerate()
					.map(|(i, line)| format!("{}: {}", block.start_line + i, line))
					.collect::<Vec<_>>()
					.join("\n");
				output.push_str(&content_with_lines);
				if !content_with_lines.ends_with('\n') {
					output.push('\n');
				}
			}
			_ => {}
		}
	}

	output
}

// Format combined search results as text for MCP with detail level control
pub fn format_commit_search_results_as_text(
	blocks: &[crate::store::CommitBlock],
	detail_level: &str,
) -> String {
	if blocks.is_empty() {
		return "No commit results found.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!("COMMIT RESULTS ({})\n", blocks.len()));

	for (idx, block) in blocks.iter().enumerate() {
		let short_hash = &block.hash[..8.min(block.hash.len())];
		let date = chrono::DateTime::from_timestamp(block.date, 0)
			.map(|dt| dt.format("%Y-%m-%d").to_string())
			.unwrap_or_else(|| block.date.to_string());

		output.push_str(&format!(
			"{}. {} ({}) by {}\n",
			idx + 1,
			short_hash,
			date,
			block.author,
		));

		if let Some(distance) = block.distance {
			output.push_str(&format!("   Similarity: {:.3}\n", 1.0 - distance));
		}

		// Subject line (first line of message)
		let subject = block.message.lines().next().unwrap_or(&block.message);

		match detail_level {
			"signatures" => {
				// Compact: just subject line
				output.push_str(&format!("   Message: {}\n", subject));
			}
			"full" => {
				// Full: complete message body + files + description
				output.push_str(&format!("   Message: {}\n", block.message));

				let files: Vec<String> = serde_json::from_str(&block.files).unwrap_or_default();
				if !files.is_empty() {
					output.push_str(&format!("   Files: {}\n", files.join(", ")));
				}

				if !block.description.is_empty() {
					output.push_str(&format!("   Description: {}\n", block.description));
				}
			}
			_ => {
				// partial (default): subject + files
				output.push_str(&format!("   Message: {}\n", subject));

				let files: Vec<String> = serde_json::from_str(&block.files).unwrap_or_default();
				if !files.is_empty() {
					output.push_str(&format!("   Files: {}\n", files.join(", ")));
				}

				if !block.description.is_empty() {
					output.push_str(&format!("   Description: {}\n", block.description));
				}
			}
		}

		output.push('\n');
	}

	output
}

pub fn format_combined_search_results_as_text(
	code_blocks: &[CodeBlock],
	text_blocks: &[crate::store::TextBlock],
	doc_blocks: &[crate::store::DocumentBlock],
	detail_level: &str,
) -> String {
	let total_results = code_blocks.len() + text_blocks.len() + doc_blocks.len();
	if total_results == 0 {
		return "No results found.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!("SEARCH RESULTS ({} total)\n\n", total_results));

	// Documentation Results
	if !doc_blocks.is_empty() {
		output.push_str(&format_doc_search_results_as_text(doc_blocks, detail_level));
		output.push('\n');
	}

	// Code Results with detail level
	if !code_blocks.is_empty() {
		output.push_str(&format_code_search_results_as_text(
			code_blocks,
			detail_level,
		));
		output.push('\n');
	}

	// Text Results
	if !text_blocks.is_empty() {
		output.push_str(&format_text_search_results_as_text(
			text_blocks,
			detail_level,
		));
	}

	output
}

// Enhanced search function for MCP server with detail level control - returns formatted text results (token-efficient)
pub async fn search_codebase_with_details_text(
	query: &str,
	mode: &str,
	detail_level: &str,
	max_results: usize,
	similarity_threshold: f32,
	language_filter: Option<&str>,
	config: &Config,
) -> Result<String> {
	// Initialize store
	let store = Store::new().await?;

	// Generate embeddings for the query using centralized logic
	let search_embeddings =
		crate::embedding::generate_search_embeddings(query, mode, config).await?;

	// Convert similarity threshold to distance threshold for store operations
	let distance_threshold = 1.0 - similarity_threshold;

	// Determine candidate limit: fetch more when reranker is enabled
	let candidate_limit = if config.search.reranker.enabled {
		config.search.reranker.top_k_candidates
	} else {
		max_results
	};

	// Perform the search based on mode
	let (mut code_blocks, mut text_blocks, mut doc_blocks, mut commit_blocks) = match mode {
		"code" => {
			let embeddings = search_embeddings.code_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No code embeddings generated for code search mode")
			})?;
			let results = store
				.get_code_blocks_with_language_filter(
					embeddings,
					Some(candidate_limit),
					Some(distance_threshold),
					language_filter,
				)
				.await?;
			(results, vec![], vec![], vec![])
		}
		"text" => {
			let embeddings = search_embeddings.text_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No text embeddings generated for text search mode")
			})?;
			let results = store
				.get_text_blocks_with_config(
					embeddings,
					Some(candidate_limit),
					Some(distance_threshold),
				)
				.await?;
			(vec![], results, vec![], vec![])
		}
		"docs" => {
			let embeddings = search_embeddings.text_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No text embeddings generated for docs search mode")
			})?;
			let results = store
				.get_document_blocks_with_config(
					embeddings,
					Some(candidate_limit),
					Some(distance_threshold),
				)
				.await?;
			(vec![], vec![], results, vec![])
		}
		"commits" => {
			let embeddings = search_embeddings.text_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No text embeddings generated for commits search mode")
			})?;
			let results = store
				.get_commit_blocks_with_config(
					embeddings,
					Some(candidate_limit),
					Some(distance_threshold),
				)
				.await?;
			(vec![], vec![], vec![], results)
		}
		"all" => {
			let code_embeddings = search_embeddings.code_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No code embeddings generated for all search mode")
			})?;
			let text_embeddings = search_embeddings.text_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No text embeddings generated for all search mode")
			})?;

			let results_per_type = candidate_limit.div_ceil(3);
			let code_results = store
				.get_code_blocks_with_language_filter(
					code_embeddings,
					Some(results_per_type),
					Some(distance_threshold),
					language_filter,
				)
				.await?;
			let text_results = store
				.get_text_blocks_with_config(
					text_embeddings.clone(),
					Some(results_per_type),
					Some(distance_threshold),
				)
				.await?;
			let doc_results = store
				.get_document_blocks_with_config(
					text_embeddings,
					Some(results_per_type),
					Some(distance_threshold),
				)
				.await?;
			(code_results, text_results, doc_results, vec![])
		}
		_ => {
			return Err(anyhow::anyhow!(
				"Invalid search mode '{}'. Use 'all', 'code', 'docs', 'commits', or 'text'.",
				mode
			))
		}
	};

	// Apply reranker if enabled
	if config.search.reranker.enabled {
		code_blocks = crate::reranker::rerank_code_blocks_with_octolib(
			query,
			code_blocks,
			&config.search.reranker,
		)
		.await?;
		text_blocks = crate::reranker::rerank_text_blocks_with_octolib(
			query,
			text_blocks,
			&config.search.reranker,
		)
		.await?;
		doc_blocks = crate::reranker::rerank_doc_blocks_with_octolib(
			query,
			doc_blocks,
			&config.search.reranker,
		)
		.await?;
		commit_blocks = crate::reranker::rerank_commit_blocks_with_octolib(
			query,
			commit_blocks,
			&config.search.reranker,
		)
		.await?;
	} else {
		code_blocks.truncate(max_results);
		text_blocks.truncate(max_results);
		doc_blocks.truncate(max_results);
		commit_blocks.truncate(max_results);
	}

	match mode {
		"code" => Ok(format_code_search_results_as_text(
			&code_blocks,
			detail_level,
		)),
		"text" => Ok(format_text_search_results_as_text(
			&text_blocks,
			detail_level,
		)),
		"docs" => Ok(format_doc_search_results_as_text(&doc_blocks, detail_level)),
		"commits" => Ok(format_commit_search_results_as_text(
			&commit_blocks,
			detail_level,
		)),
		"all" => Ok(format_combined_search_results_as_text(
			&code_blocks,
			&text_blocks,
			&doc_blocks,
			detail_level,
		)),
		_ => Err(anyhow::anyhow!(
			"Invalid search mode '{}'. Use 'all', 'code', 'docs', 'commits', or 'text'.",
			mode
		)),
	}
}

// Enhanced search function for MCP server with multi-query support and detail level control - returns text results
pub async fn search_codebase_with_details_multi_query_text(
	queries: &[String],
	mode: &str,
	detail_level: &str,
	max_results: usize,
	similarity_threshold: f32,
	language_filter: Option<&str>,
	config: &Config,
) -> Result<String> {
	// Initialize store
	let store = Store::new().await?;

	// Validate queries (same as CLI)
	if queries.is_empty() {
		return Err(anyhow::anyhow!("At least one query is required"));
	}
	if queries.len() > octolib::embedding::constants::MAX_QUERIES {
		return Err(anyhow::anyhow!(
			"Maximum {} queries allowed, got {}. Use fewer, more specific terms.",
			crate::constants::MAX_QUERIES,
			queries.len()
		));
	}

	// Generate batch embeddings for all queries
	let embeddings = generate_batch_embeddings_for_queries(queries, mode, config).await?;

	// Zip queries with embeddings
	let query_embeddings: Vec<_> = queries
		.iter()
		.cloned()
		.zip(embeddings.into_iter())
		.collect();

	// Convert similarity threshold to distance threshold for store operations
	let distance_threshold = 1.0 - similarity_threshold;

	// Execute parallel searches - Pass original similarity_threshold, conversion happens inside
	let search_results = execute_parallel_searches(
		&store,
		query_embeddings,
		mode,
		max_results,
		similarity_threshold,
		language_filter,
		config,
	)
	.await?;

	// Deduplicate and merge with multi-query bonuses
	let (mut code_blocks, mut doc_blocks, mut text_blocks, mut commit_blocks) =
		deduplicate_and_merge_results(search_results, queries, distance_threshold);

	// Apply reranker if enabled
	if config.search.reranker.enabled && !queries.is_empty() {
		let query = queries.join(" ");
		code_blocks = crate::reranker::rerank_code_blocks_with_octolib(
			&query,
			code_blocks,
			&config.search.reranker,
		)
		.await?;
		doc_blocks = crate::reranker::rerank_doc_blocks_with_octolib(
			&query,
			doc_blocks,
			&config.search.reranker,
		)
		.await?;
		text_blocks = crate::reranker::rerank_text_blocks_with_octolib(
			&query,
			text_blocks,
			&config.search.reranker,
		)
		.await?;
		commit_blocks = crate::reranker::rerank_commit_blocks_with_octolib(
			&query,
			commit_blocks,
			&config.search.reranker,
		)
		.await?;
	} else {
		// Apply global result limits (reranker already limits via final_top_k)
		code_blocks.truncate(max_results);
		doc_blocks.truncate(max_results);
		text_blocks.truncate(max_results);
		commit_blocks.truncate(max_results);
	}

	// Format results based on mode with detail level control
	match mode {
		"code" => Ok(format_code_search_results_as_text(
			&code_blocks,
			detail_level,
		)),
		"text" => Ok(format_text_search_results_as_text(
			&text_blocks,
			detail_level,
		)),
		"docs" => Ok(format_doc_search_results_as_text(&doc_blocks, detail_level)),
		"commits" => Ok(format_commit_search_results_as_text(
			&commit_blocks,
			detail_level,
		)),
		"all" => Ok(format_combined_search_results_as_text(
			&code_blocks,
			&text_blocks,
			&doc_blocks,
			detail_level,
		)),
		_ => Err(anyhow::anyhow!(
			"Invalid search mode '{}'. Use 'all', 'code', 'docs', 'commits', or 'text'.",
			mode
		)),
	}
}

// Get a clean preview of text content with line numbers and smart truncation
fn get_text_preview_with_lines(content: &str, start_line: usize) -> String {
	let lines: Vec<&str> = content.lines().collect();

	// If content is short, just return it all with line numbers
	if lines.len() <= 10 {
		return lines
			.iter()
			.enumerate()
			.map(|(i, line)| format!("{}: {}", start_line + i, line))
			.collect::<Vec<_>>()
			.join("\n");
	}

	// Skip leading empty lines
	let mut start_idx = 0;
	for (i, line) in lines.iter().enumerate() {
		let trimmed = line.trim();
		if !trimmed.is_empty() {
			start_idx = i;
			break;
		}
	}

	// Take first 4 lines of actual content
	let preview_start = 4;
	let preview_end = 3;

	let mut result = Vec::new();

	// Add first few lines with line numbers
	for (i, line) in lines.iter().skip(start_idx).take(preview_start).enumerate() {
		result.push(format!("{}: {}", start_line + start_idx + i, line));
	}

	// If there's more content, add separator and last few lines
	if start_idx + preview_start < lines.len() {
		let remaining_lines = lines.len() - (start_idx + preview_start);
		if remaining_lines > preview_end {
			result.push(format!("... ({} more lines)", remaining_lines));

			// Add last few lines with correct line numbers
			let end_start_idx = lines.len() - preview_end;
			for (i, line) in lines.iter().skip(end_start_idx).enumerate() {
				result.push(format!("{}: {}", start_line + end_start_idx + i, line));
			}
		} else {
			// Just add the remaining lines with line numbers
			for (i, line) in lines.iter().skip(start_idx + preview_start).enumerate() {
				result.push(format!(
					"{}: {}",
					start_line + start_idx + preview_start + i,
					line
				));
			}
		}
	}

	result.join("\n")
}

// Get a clean preview of document content with line numbers and smart truncation
fn get_doc_preview_with_lines(content: &str, start_line: usize) -> String {
	let lines: Vec<&str> = content.lines().collect();

	// If content is short, just return it all with line numbers
	if lines.len() <= 10 {
		return lines
			.iter()
			.enumerate()
			.map(|(i, line)| format!("{}: {}", start_line + i, line))
			.collect::<Vec<_>>()
			.join("\n");
	}

	// Skip leading empty lines
	let mut start_idx = 0;
	for (i, line) in lines.iter().enumerate() {
		let trimmed = line.trim();
		if !trimmed.is_empty() {
			start_idx = i;
			break;
		}
	}

	// Take first 4 lines of actual content
	let preview_start = 4;
	let preview_end = 3;

	let mut result = Vec::new();

	// Add first few lines with line numbers
	for (i, line) in lines.iter().skip(start_idx).take(preview_start).enumerate() {
		result.push(format!("{}: {}", start_line + start_idx + i, line));
	}

	// If there's more content, add separator and last few lines
	if start_idx + preview_start < lines.len() {
		let remaining_lines = lines.len() - (start_idx + preview_start);
		if remaining_lines > preview_end {
			result.push(format!("... ({} more lines)", remaining_lines));

			// Add last few lines with correct line numbers
			let end_start_idx = lines.len() - preview_end;
			for (i, line) in lines.iter().skip(end_start_idx).enumerate() {
				result.push(format!("{}: {}", start_line + end_start_idx + i, line));
			}
		} else {
			// Just add the remaining lines with line numbers
			for (i, line) in lines.iter().skip(start_idx + preview_start).enumerate() {
				result.push(format!(
					"{}: {}",
					start_line + start_idx + preview_start + i,
					line
				));
			}
		}
	}

	result.join("\n")
}

// Get a clean preview of code content with line numbers and smart truncation
fn get_code_preview_with_lines(content: &str, start_line: usize, _language: &str) -> String {
	let lines: Vec<&str> = content.lines().collect();

	// If content is short, just return it all with line numbers
	if lines.len() <= 10 {
		return lines
			.iter()
			.enumerate()
			.map(|(i, line)| format!("{}: {}", start_line + i, line))
			.collect::<Vec<_>>()
			.join("\n");
	}

	// Skip leading comments and empty lines
	let mut start_idx = 0;
	for (i, line) in lines.iter().enumerate() {
		let trimmed = line.trim();

		// Skip empty lines
		if trimmed.is_empty() {
			continue;
		}

		// Skip common comment patterns across languages
		if trimmed.starts_with("//") ||     // C-style, Rust, JS, etc.
		   trimmed.starts_with("#") ||      // Python, Shell, Ruby, etc.
		   trimmed.starts_with("/*") ||     // C-style block comments
		   trimmed.starts_with("*") ||      // Continuation of block comments
		   trimmed.starts_with("<!--") ||   // HTML comments
		   trimmed.starts_with("--") ||     // SQL, Lua comments
		   trimmed.starts_with("%") ||      // LaTeX, Erlang comments
		   trimmed.starts_with(";") ||      // Lisp, assembly comments
		   trimmed.starts_with("\"\"\"") ||  // Python docstrings
		   trimmed.starts_with("'''")
		{
			// Python docstrings
			continue;
		}

		// Found first non-comment line
		start_idx = i;
		break;
	}

	// Take first 4 lines of actual code
	let preview_start = 4;
	let preview_end = 3;

	let mut result = Vec::new();

	// Add first few lines with line numbers
	for (i, line) in lines.iter().skip(start_idx).take(preview_start).enumerate() {
		result.push(format!("{}: {}", start_line + start_idx + i, line));
	}

	// If there's more content, add separator and last few lines
	if start_idx + preview_start < lines.len() {
		let remaining_lines = lines.len() - (start_idx + preview_start);
		if remaining_lines > preview_end {
			result.push(format!("... ({} more lines)", remaining_lines));

			// Add last few lines with correct line numbers
			let end_start_idx = lines.len() - preview_end;
			for (i, line) in lines.iter().skip(end_start_idx).enumerate() {
				result.push(format!("{}: {}", start_line + end_start_idx + i, line));
			}
		} else {
			// Just add the remaining lines with line numbers
			for (i, line) in lines.iter().skip(start_idx + preview_start).enumerate() {
				result.push(format!(
					"{}: {}",
					start_line + start_idx + preview_start + i,
					line
				));
			}
		}
	}

	result.join("\n")
}

// ============================================================================
// SHARED MULTI-QUERY SEARCH FUNCTIONS (Used by both CLI and MCP)
// ============================================================================

#[derive(Debug, Clone)]
pub struct QuerySearchResult {
	pub query_index: usize,
	pub code_blocks: Vec<crate::store::CodeBlock>,
	pub doc_blocks: Vec<crate::store::DocumentBlock>,
	pub text_blocks: Vec<crate::store::TextBlock>,
	pub commit_blocks: Vec<crate::store::CommitBlock>,
}

pub async fn generate_batch_embeddings_for_queries(
	queries: &[String],
	mode: &str,
	config: &Config,
) -> Result<Vec<crate::embedding::SearchModeEmbeddings>> {
	match mode {
		"code" => {
			// Batch generate code embeddings for all queries (symmetric: None for code-to-code)
			let code_embeddings = crate::embedding::generate_embeddings_batch(
				queries.to_vec(),
				true,
				config,
				crate::embedding::types::InputType::None,
			)
			.await?;
			Ok(code_embeddings
				.into_iter()
				.map(|emb| crate::embedding::SearchModeEmbeddings {
					code_embeddings: Some(emb),
					text_embeddings: None,
				})
				.collect())
		}
		"docs" | "text" | "commits" => {
			// Batch generate text embeddings for all queries
			let text_embeddings = crate::embedding::generate_embeddings_batch(
				queries.to_vec(),
				false,
				config,
				crate::embedding::types::InputType::Query,
			)
			.await?;
			Ok(text_embeddings
				.into_iter()
				.map(|emb| crate::embedding::SearchModeEmbeddings {
					code_embeddings: None,
					text_embeddings: Some(emb),
				})
				.collect())
		}
		"all" => {
			let code_model = &config.embedding.code_model;
			let text_model = &config.embedding.text_model;

			if code_model == text_model {
				// Same model - generate once and reuse (efficient!)
				let embeddings = crate::embedding::generate_embeddings_batch(
					queries.to_vec(),
					true,
					config,
					crate::embedding::types::InputType::None,
				)
				.await?;
				Ok(embeddings
					.into_iter()
					.map(|emb| crate::embedding::SearchModeEmbeddings {
						code_embeddings: Some(emb.clone()),
						text_embeddings: Some(emb),
					})
					.collect())
			} else {
				// Different models - generate both types in parallel (code=None, text=Query)
				let (code_embeddings, text_embeddings) = tokio::try_join!(
					crate::embedding::generate_embeddings_batch(
						queries.to_vec(),
						true,
						config,
						crate::embedding::types::InputType::None
					),
					crate::embedding::generate_embeddings_batch(
						queries.to_vec(),
						false,
						config,
						crate::embedding::types::InputType::Query
					)
				)?;

				Ok(code_embeddings
					.into_iter()
					.zip(text_embeddings.into_iter())
					.map(
						|(code_emb, text_emb)| crate::embedding::SearchModeEmbeddings {
							code_embeddings: Some(code_emb),
							text_embeddings: Some(text_emb),
						},
					)
					.collect())
			}
		}
		_ => Err(anyhow::anyhow!("Invalid search mode: {}", mode)),
	}
}

pub async fn execute_single_search_with_embeddings(
	store: &Store,
	embeddings: crate::embedding::SearchModeEmbeddings,
	mode: &str,
	per_query_limit: usize,
	query_index: usize,
	similarity_threshold: f32,
	language_filter: Option<&str>,
) -> Result<QuerySearchResult> {
	// Convert similarity threshold to distance threshold for store operations
	let distance_threshold = 1.0 - similarity_threshold;

	let mut code_blocks = Vec::new();
	let mut doc_blocks = Vec::new();
	let mut text_blocks = Vec::new();
	let mut commit_blocks = Vec::new();

	match mode {
		"code" => {
			if let Some(code_emb) = embeddings.code_embeddings {
				code_blocks = store
					.get_code_blocks_with_language_filter(
						code_emb,
						Some(per_query_limit),
						Some(distance_threshold),
						language_filter,
					)
					.await?;
			}
		}
		"docs" => {
			if let Some(text_emb) = embeddings.text_embeddings {
				doc_blocks = store
					.get_document_blocks_with_config(
						text_emb,
						Some(per_query_limit),
						Some(distance_threshold),
					)
					.await?;
			}
		}
		"text" => {
			if let Some(text_emb) = embeddings.text_embeddings {
				text_blocks = store
					.get_text_blocks_with_config(
						text_emb,
						Some(per_query_limit),
						Some(distance_threshold),
					)
					.await?;
			}
		}
		"commits" => {
			if let Some(text_emb) = embeddings.text_embeddings {
				commit_blocks = store
					.get_commit_blocks_with_config(
						text_emb,
						Some(per_query_limit),
						Some(distance_threshold),
					)
					.await?;
			}
		}
		"all" => {
			let results_per_type = per_query_limit.div_ceil(3);

			if let Some(code_emb) = embeddings.code_embeddings {
				code_blocks = store
					.get_code_blocks_with_language_filter(
						code_emb,
						Some(results_per_type),
						Some(distance_threshold),
						language_filter,
					)
					.await?;
			}

			if let Some(text_emb) = embeddings.text_embeddings {
				let text_emb_clone = text_emb.clone();

				let (text_result, doc_result) = tokio::try_join!(
					store.get_text_blocks_with_config(
						text_emb,
						Some(results_per_type),
						Some(distance_threshold),
					),
					store.get_document_blocks_with_config(
						text_emb_clone,
						Some(results_per_type),
						Some(distance_threshold),
					)
				)?;

				text_blocks = text_result;
				doc_blocks = doc_result;
			}
		}
		_ => return Err(anyhow::anyhow!("Invalid search mode: {}", mode)),
	}

	Ok(QuerySearchResult {
		query_index,
		code_blocks,
		doc_blocks,
		text_blocks,
		commit_blocks,
	})
}

pub async fn execute_parallel_searches(
	store: &Store,
	query_embeddings: Vec<(String, crate::embedding::SearchModeEmbeddings)>,
	mode: &str,
	max_results: usize,
	similarity_threshold: f32,
	language_filter: Option<&str>,
	config: &Config,
) -> Result<Vec<QuerySearchResult>> {
	let per_query_limit = if config.search.reranker.enabled {
		config.search.reranker.top_k_candidates
	} else {
		(max_results * 2) / query_embeddings.len().max(1)
	};

	let search_futures: Vec<_> = query_embeddings
		.into_iter()
		.enumerate()
		.map(|(index, (_, embeddings))| async move {
			execute_single_search_with_embeddings(
				store,
				embeddings,
				mode,
				per_query_limit,
				index,
				similarity_threshold,
				language_filter,
			)
			.await
		})
		.collect();

	// Execute all searches concurrently
	futures::future::try_join_all(search_futures).await
}

pub fn apply_multi_query_bonus_code(
	block: &mut crate::store::CodeBlock,
	query_indices: &[usize],
	total_queries: usize,
) {
	if query_indices.len() > 1 && total_queries > 1 {
		let coverage_ratio = query_indices.len() as f32 / total_queries as f32;
		let bonus_factor = 1.0 - (coverage_ratio * 0.1).min(0.2); // Up to 20% bonus

		if let Some(distance) = block.distance {
			block.distance = Some(distance * bonus_factor);
		}
	}
}

pub fn apply_multi_query_bonus_doc(
	block: &mut crate::store::DocumentBlock,
	query_indices: &[usize],
	total_queries: usize,
) {
	if query_indices.len() > 1 && total_queries > 1 {
		let coverage_ratio = query_indices.len() as f32 / total_queries as f32;
		let bonus_factor = 1.0 - (coverage_ratio * 0.1).min(0.2);

		if let Some(distance) = block.distance {
			block.distance = Some(distance * bonus_factor);
		}
	}
}

pub fn apply_multi_query_bonus_text(
	block: &mut crate::store::TextBlock,
	query_indices: &[usize],
	total_queries: usize,
) {
	if query_indices.len() > 1 && total_queries > 1 {
		let coverage_ratio = query_indices.len() as f32 / total_queries as f32;
		let bonus_factor = 1.0 - (coverage_ratio * 0.1).min(0.2);

		if let Some(distance) = block.distance {
			block.distance = Some(distance * bonus_factor);
		}
	}
}

pub fn apply_multi_query_bonus_commit(
	block: &mut crate::store::CommitBlock,
	query_indices: &[usize],
	total_queries: usize,
) {
	if query_indices.len() > 1 && total_queries > 1 {
		let coverage_ratio = query_indices.len() as f32 / total_queries as f32;
		let bonus_factor = 1.0 - (coverage_ratio * 0.1).min(0.2);

		if let Some(distance) = block.distance {
			block.distance = Some(distance * bonus_factor);
		}
	}
}

pub fn deduplicate_and_merge_results(
	search_results: Vec<QuerySearchResult>,
	queries: &[String],
	distance_threshold: f32,
) -> (
	Vec<crate::store::CodeBlock>,
	Vec<crate::store::DocumentBlock>,
	Vec<crate::store::TextBlock>,
	Vec<crate::store::CommitBlock>,
) {
	use std::cmp::Ordering;
	use std::collections::HashMap;

	// Deduplicate code blocks
	let mut code_map: HashMap<String, (crate::store::CodeBlock, Vec<usize>)> = HashMap::new();

	for result in &search_results {
		for block in &result.code_blocks {
			match code_map.entry(block.hash.clone()) {
				std::collections::hash_map::Entry::Vacant(e) => {
					e.insert((block.clone(), vec![result.query_index]));
				}
				std::collections::hash_map::Entry::Occupied(mut e) => {
					let (existing_block, query_indices) = e.get_mut();
					query_indices.push(result.query_index);
					// Keep block with better score (lower distance)
					if block.distance < existing_block.distance {
						*existing_block = block.clone();
						existing_block.start_line = block.start_line + 1;
						existing_block.end_line = block.end_line + 1;
					}
				}
			}
		}
	}

	// Deduplicate document blocks
	let mut doc_map: HashMap<String, (crate::store::DocumentBlock, Vec<usize>)> = HashMap::new();

	for result in &search_results {
		for block in &result.doc_blocks {
			match doc_map.entry(block.hash.clone()) {
				std::collections::hash_map::Entry::Vacant(e) => {
					e.insert((block.clone(), vec![result.query_index]));
				}
				std::collections::hash_map::Entry::Occupied(mut e) => {
					let (existing_block, query_indices) = e.get_mut();
					query_indices.push(result.query_index);
					if block.distance < existing_block.distance {
						*existing_block = block.clone();
						existing_block.start_line = block.start_line + 1;
						existing_block.end_line = block.end_line + 1;
					}
				}
			}
		}
	}

	// Deduplicate text blocks
	let mut text_map: HashMap<String, (crate::store::TextBlock, Vec<usize>)> = HashMap::new();

	for result in &search_results {
		for block in &result.text_blocks {
			match text_map.entry(block.hash.clone()) {
				std::collections::hash_map::Entry::Vacant(e) => {
					e.insert((block.clone(), vec![result.query_index]));
				}
				std::collections::hash_map::Entry::Occupied(mut e) => {
					let (existing_block, query_indices) = e.get_mut();
					query_indices.push(result.query_index);
					if block.distance < existing_block.distance {
						*existing_block = block.clone();
						existing_block.start_line = block.start_line + 1;
						existing_block.end_line = block.end_line + 1;
					}
				}
			}
		}
	}

	// Apply multi-query bonuses and filter
	let mut final_code_blocks: Vec<crate::store::CodeBlock> = code_map
		.into_values()
		.map(|(mut block, query_indices)| {
			apply_multi_query_bonus_code(&mut block, &query_indices, queries.len());
			block
		})
		.filter(|block| {
			if let Some(distance) = block.distance {
				distance <= distance_threshold
			} else {
				true
			}
		})
		.collect();

	let mut final_doc_blocks: Vec<crate::store::DocumentBlock> = doc_map
		.into_values()
		.map(|(mut block, query_indices)| {
			apply_multi_query_bonus_doc(&mut block, &query_indices, queries.len());
			block
		})
		.filter(|block| {
			if let Some(distance) = block.distance {
				distance <= distance_threshold
			} else {
				true
			}
		})
		.collect();

	let mut final_text_blocks: Vec<crate::store::TextBlock> = text_map
		.into_values()
		.map(|(mut block, query_indices)| {
			apply_multi_query_bonus_text(&mut block, &query_indices, queries.len());
			block
		})
		.filter(|block| {
			if let Some(distance) = block.distance {
				distance <= distance_threshold
			} else {
				true
			}
		})
		.collect();

	// Sort by relevance
	final_code_blocks.sort_by(|a, b| match (a.distance, b.distance) {
		(Some(dist_a), Some(dist_b)) => dist_a.partial_cmp(&dist_b).unwrap_or(Ordering::Equal),
		(Some(_), None) => Ordering::Less,
		(None, Some(_)) => Ordering::Greater,
		(None, None) => Ordering::Equal,
	});

	final_doc_blocks.sort_by(|a, b| match (a.distance, b.distance) {
		(Some(dist_a), Some(dist_b)) => dist_a.partial_cmp(&dist_b).unwrap_or(Ordering::Equal),
		(Some(_), None) => Ordering::Less,
		(None, Some(_)) => Ordering::Greater,
		(None, None) => Ordering::Equal,
	});

	final_text_blocks.sort_by(|a, b| match (a.distance, b.distance) {
		(Some(dist_a), Some(dist_b)) => dist_a.partial_cmp(&dist_b).unwrap_or(Ordering::Equal),
		(Some(_), None) => Ordering::Less,
		(None, Some(_)) => Ordering::Greater,
		(None, None) => Ordering::Equal,
	});

	// Deduplicate commit blocks
	let mut commit_map: HashMap<String, (crate::store::CommitBlock, Vec<usize>)> = HashMap::new();

	for result in &search_results {
		for block in &result.commit_blocks {
			match commit_map.entry(block.hash.clone()) {
				std::collections::hash_map::Entry::Vacant(e) => {
					e.insert((block.clone(), vec![result.query_index]));
				}
				std::collections::hash_map::Entry::Occupied(mut e) => {
					let (existing_block, query_indices) = e.get_mut();
					query_indices.push(result.query_index);
					if block.distance < existing_block.distance {
						*existing_block = block.clone();
					}
				}
			}
		}
	}

	let mut final_commit_blocks: Vec<crate::store::CommitBlock> = commit_map
		.into_values()
		.map(|(mut block, query_indices)| {
			apply_multi_query_bonus_commit(&mut block, &query_indices, queries.len());
			block
		})
		.filter(|block| {
			if let Some(distance) = block.distance {
				distance <= distance_threshold
			} else {
				true
			}
		})
		.collect();

	final_commit_blocks.sort_by(|a, b| match (a.distance, b.distance) {
		(Some(dist_a), Some(dist_b)) => dist_a.partial_cmp(&dist_b).unwrap_or(Ordering::Equal),
		(Some(_), None) => Ordering::Less,
		(None, Some(_)) => Ordering::Greater,
		(None, None) => Ordering::Equal,
	});

	(
		final_code_blocks,
		final_doc_blocks,
		final_text_blocks,
		final_commit_blocks,
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_format_symbols_for_display_empty() {
		let symbols: Vec<String> = vec![];
		let result = format_symbols_for_display(&symbols);
		assert_eq!(result, "");
	}

	#[test]
	fn test_format_symbols_for_display_filters_types() {
		let symbols = vec![
			"function_definition".to_string(), // contains _, filtered out
			"my_function".to_string(),         // contains _, filtered out
			"class_declaration".to_string(),   // contains _, filtered out
			"MyClass".to_string(),             // no _, kept
		];
		let result = format_symbols_for_display(&symbols);
		// Should filter out symbols containing underscores
		assert_eq!(result, "MyClass");
	}

	#[test]
	fn test_format_symbols_for_display_deduplicates() {
		let symbols = vec![
			"foo".to_string(),
			"bar".to_string(),
			"foo".to_string(),
			"baz".to_string(),
		];
		let result = format_symbols_for_display(&symbols);
		// Should deduplicate and sort
		assert_eq!(result, "bar, baz, foo");
	}

	#[test]
	fn test_format_symbols_for_display_sorts() {
		let symbols = vec![
			"zebra".to_string(),
			"apple".to_string(),
			"mango".to_string(),
		];
		let result = format_symbols_for_display(&symbols);
		assert_eq!(result, "apple, mango, zebra");
	}

	#[test]
	fn test_format_symbols_for_display_mixed() {
		let symbols = vec![
			"my_func".to_string(),       // contains _, filtered out
			"type_alias".to_string(),    // contains _, filtered out
			"AnotherFunc".to_string(),   // no _, kept
			"my_func".to_string(),       // duplicate, filtered out
			"interface_def".to_string(), // contains _, filtered out
			"SimpleFunc".to_string(),    // no _, kept
		];
		let result = format_symbols_for_display(&symbols);
		// Should filter types with underscores and sort
		assert_eq!(result, "AnotherFunc, SimpleFunc");
	}

	#[test]
	fn test_format_commit_search_results_empty() {
		let blocks: Vec<crate::store::CommitBlock> = vec![];
		let result = format_commit_search_results_as_text(&blocks, "partial");
		assert_eq!(result, "No commit results found.");
	}

	#[test]
	fn test_format_commit_search_results() {
		let blocks = vec![crate::store::CommitBlock {
			hash: "abc12345deadbeef".to_string(),
			author: "Alice".to_string(),
			date: 1700000000, // 2023-11-14
			message: "feat: add retry logic\n\nDetailed body here".to_string(),
			content: "feat: add retry logic\n\nFiles: src/client.rs".to_string(),
			files: r#"["src/client.rs","src/retry.rs"]"#.to_string(),
			description: "Adds exponential backoff".to_string(),
			distance: Some(0.15),
		}];

		// Partial: shows subject + files + description, not full body
		let result = format_commit_search_results_as_text(&blocks, "partial");
		assert!(result.contains("COMMIT RESULTS (1)"));
		assert!(result.contains("abc12345"));
		assert!(result.contains("Alice"));
		assert!(result.contains("feat: add retry logic"));
		assert!(!result.contains("Detailed body here"));
		assert!(result.contains("src/client.rs"));
		assert!(result.contains("Adds exponential backoff"));

		// Full: shows complete message body
		let result_full = format_commit_search_results_as_text(&blocks, "full");
		assert!(result_full.contains("Detailed body here"));
		assert!(result_full.contains("src/client.rs"));

		// Signatures: compact, just subject
		let result_sig = format_commit_search_results_as_text(&blocks, "signatures");
		assert!(result_sig.contains("feat: add retry logic"));
		assert!(!result_sig.contains("Detailed body here"));
		assert!(!result_sig.contains("src/client.rs"));
	}

	#[test]
	fn test_apply_multi_query_bonus_commit() {
		let mut block = crate::store::CommitBlock {
			hash: "abc".to_string(),
			author: "A".to_string(),
			date: 0,
			message: "m".to_string(),
			content: "c".to_string(),
			files: "[]".to_string(),
			description: String::new(),
			distance: Some(0.5),
		};

		// Single query match — no bonus
		apply_multi_query_bonus_commit(&mut block, &[0], 3);
		assert_eq!(block.distance, Some(0.5));

		// Multi-query match — applies bonus
		apply_multi_query_bonus_commit(&mut block, &[0, 1], 3);
		assert!(block.distance.unwrap() < 0.5);
	}
}
