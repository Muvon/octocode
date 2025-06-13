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

// Render code blocks in a user-friendly format
pub fn render_code_blocks(blocks: &[CodeBlock]) {
	render_code_blocks_with_config(blocks, &Config::default());
}

// Render code blocks in a user-friendly format with configuration
pub fn render_code_blocks_with_config(blocks: &[CodeBlock], config: &Config) {
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
			// Deduplicate symbols in display
			let mut display_symbols = block.symbols.clone();
			display_symbols.sort();
			display_symbols.dedup();

			for symbol in display_symbols {
				// Only show non-type symbols to users
				if !symbol.contains("_") {
					println!("║   • {}", symbol);
				}
			}
		}

		println!("║ Content:");
		println!("║ ┌────────────────────────────────────");

		// Use smart truncation based on configuration
		let max_chars = config.search.search_block_max_characters;
		let (content, was_truncated) =
			crate::indexer::truncate_content_smartly(&block.content, max_chars);

		// Display content with proper indentation
		for line in content.lines() {
			println!("║ │ {}", line);
		}

		// Add note if content was truncated
		if was_truncated {
			println!("║ │ [Content truncated - limit: {} chars]", max_chars);
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

// Enhanced search function for MCP server with detail level control - returns formatted markdown results
pub async fn search_codebase_with_details(
	query: &str,
	mode: &str,
	detail_level: &str,
	max_results: usize,
	config: &Config,
) -> Result<String> {
	// Initialize store
	let store = Store::new().await?;

	// Generate embeddings for the query using centralized logic
	let search_embeddings =
		crate::embedding::generate_search_embeddings(query, mode, config).await?;

	// Perform the search based on mode
	match mode {
		"code" => {
			let embeddings = search_embeddings.code_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No code embeddings generated for code search mode")
			})?;
			let results = store
				.get_code_blocks_with_config(
					embeddings,
					Some(max_results),
					Some(config.search.similarity_threshold),
				)
				.await?;
			Ok(format_code_search_results_with_detail(
				&results,
				detail_level,
			))
		}
		"text" => {
			let embeddings = search_embeddings.text_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No text embeddings generated for text search mode")
			})?;
			let results = store
				.get_text_blocks_with_config(
					embeddings,
					Some(max_results),
					Some(config.search.similarity_threshold),
				)
				.await?;
			Ok(format_text_search_results_as_markdown(&results))
		}
		"docs" => {
			let embeddings = search_embeddings.text_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No text embeddings generated for docs search mode")
			})?;
			let results = store
				.get_document_blocks_with_config(
					embeddings,
					Some(max_results),
					Some(config.search.similarity_threshold),
				)
				.await?;
			Ok(format_doc_search_results_as_markdown(&results))
		}
		"all" => {
			// "all" mode - search across all types with limited results per type
			let code_embeddings = search_embeddings.code_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No code embeddings generated for all search mode")
			})?;
			let text_embeddings = search_embeddings.text_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No text embeddings generated for all search mode")
			})?;

			let results_per_type = max_results.div_ceil(3); // Distribute results across types
			let code_results = store
				.get_code_blocks_with_config(
					code_embeddings,
					Some(results_per_type),
					Some(config.search.similarity_threshold),
				)
				.await?;
			let text_results = store
				.get_text_blocks_with_config(
					text_embeddings.clone(),
					Some(results_per_type),
					Some(config.search.similarity_threshold),
				)
				.await?;
			let doc_results = store
				.get_document_blocks_with_config(
					text_embeddings,
					Some(results_per_type),
					Some(config.search.similarity_threshold),
				)
				.await?;

			// Format combined results with detail level for code
			Ok(format_combined_search_results_with_detail(
				&code_results,
				&text_results,
				&doc_results,
				detail_level,
			))
		}
		_ => Err(anyhow::anyhow!(
			"Invalid search mode '{}'. Use 'all', 'code', 'docs', or 'text'.",
			mode
		)),
	}
}

// Search function for MCP server - returns formatted markdown results
pub async fn search_codebase(query: &str, mode: &str, config: &Config) -> Result<String> {
	// Initialize store
	let store = Store::new().await?;

	// Generate embeddings for the query using centralized logic
	let search_embeddings =
		crate::embedding::generate_search_embeddings(query, mode, config).await?;

	// Perform the search based on mode
	match mode {
		"code" => {
			let embeddings = search_embeddings.code_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No code embeddings generated for code search mode")
			})?;
			let results = store
				.get_code_blocks_with_config(
					embeddings,
					Some(config.search.max_results),
					Some(config.search.similarity_threshold),
				)
				.await?;
			Ok(format_code_search_results_as_markdown(&results))
		}
		"text" => {
			let embeddings = search_embeddings.text_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No text embeddings generated for text search mode")
			})?;
			let results = store
				.get_text_blocks_with_config(
					embeddings,
					Some(config.search.max_results),
					Some(config.search.similarity_threshold),
				)
				.await?;
			Ok(format_text_search_results_as_markdown(&results))
		}
		"docs" => {
			let embeddings = search_embeddings.text_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No text embeddings generated for docs search mode")
			})?;
			let results = store
				.get_document_blocks_with_config(
					embeddings,
					Some(config.search.max_results),
					Some(config.search.similarity_threshold),
				)
				.await?;
			Ok(format_doc_search_results_as_markdown(&results))
		}
		"all" => {
			// "all" mode - search across all types with proper embeddings
			let code_embeddings = search_embeddings.code_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No code embeddings generated for all search mode")
			})?;
			let text_embeddings = search_embeddings.text_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No text embeddings generated for all search mode")
			})?;

			let code_results = store
				.get_code_blocks_with_config(
					code_embeddings,
					Some(config.search.max_results),
					Some(config.search.similarity_threshold),
				)
				.await?;
			let text_results = store
				.get_text_blocks_with_config(
					text_embeddings.clone(),
					Some(config.search.max_results),
					Some(config.search.similarity_threshold),
				)
				.await?;
			let doc_results = store
				.get_document_blocks_with_config(
					text_embeddings,
					Some(config.search.max_results),
					Some(config.search.similarity_threshold),
				)
				.await?;

			// Format combined results
			Ok(format_combined_search_results_as_markdown(
				&code_results,
				&text_results,
				&doc_results,
			))
		}
		_ => Err(anyhow::anyhow!(
			"Invalid search mode '{}'. Use 'all', 'code', 'docs', or 'text'.",
			mode
		)),
	}
}

// Format code search results with detail level control as markdown for MCP
fn format_code_search_results_with_detail(blocks: &[CodeBlock], detail_level: &str) -> String {
	if blocks.is_empty() {
		return "No code results found.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!(
		"# Code Search Results\n\nFound {} code blocks:\n\n",
		blocks.len()
	));

	// Group blocks by file path for better organization
	let mut blocks_by_file: std::collections::HashMap<String, Vec<&CodeBlock>> =
		std::collections::HashMap::new();

	for block in blocks {
		// Ensure we display relative paths
		let display_path = ensure_relative_path(&block.path);
		blocks_by_file.entry(display_path).or_default().push(block);
	}

	// Format results organized by file
	for (file_path, file_blocks) in blocks_by_file.iter() {
		output.push_str(&format!("## File: {}\n\n", file_path));

		for (idx, block) in file_blocks.iter().enumerate() {
			output.push_str(&format!(
				"### Block {} of {} in file\n\n",
				idx + 1,
				file_blocks.len()
			));
			output.push_str(&format!("- **Language**: {}\n", block.language));
			output.push_str(&format!(
				"- **Lines**: {}-{}\n",
				block.start_line, block.end_line
			));

			// Show similarity score if available
			if let Some(distance) = block.distance {
				output.push_str(&format!("- **Similarity**: {:.4}\n", 1.0 - distance));
			}

			if !block.symbols.is_empty() {
				output.push_str("- **Symbols**: ");
				// Deduplicate symbols in display
				let mut display_symbols = block.symbols.clone();
				display_symbols.sort();
				display_symbols.dedup();

				let relevant_symbols: Vec<String> = display_symbols
					.iter()
					.filter(|symbol| !symbol.contains("_"))
					.cloned()
					.collect();

				if !relevant_symbols.is_empty() {
					output.push_str(&relevant_symbols.join(", "));
				}
				output.push('\n');
			}

			output.push_str("\n**Content:**\n\n");

			// Apply detail level formatting
			match detail_level {
				"signatures" => {
					// Show clean preview without comments and with key parts
					let preview = get_code_preview(&block.content, &block.language);
					output.push_str("```");
					output.push_str(&block.language);
					output.push('\n');
					output.push_str(&preview);
					output.push_str("\n```\n\n");
				}
				"full" => {
					// Show complete content
					output.push_str("```");
					output.push_str(&block.language);
					output.push('\n');
					output.push_str(&block.content);
					output.push_str("\n```\n\n");
				}
				_ => {
					// "partial" - default smart truncation
					output.push_str("```");
					output.push_str(&block.language);
					output.push('\n');
					output.push_str(&block.content);
					output.push_str("\n```\n\n");
				}
			}
		}
	}

	output
}

// Format code search results as markdown for MCP
fn format_code_search_results_as_markdown(blocks: &[CodeBlock]) -> String {
	if blocks.is_empty() {
		return "No code results found.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!(
		"# Code Search Results\n\nFound {} code blocks:\n\n",
		blocks.len()
	));

	// Group blocks by file path for better organization
	let mut blocks_by_file: std::collections::HashMap<String, Vec<&CodeBlock>> =
		std::collections::HashMap::new();

	for block in blocks {
		blocks_by_file
			.entry(block.path.clone())
			.or_default()
			.push(block);
	}

	// Format results organized by file
	for (file_path, file_blocks) in blocks_by_file.iter() {
		output.push_str(&format!("## File: {}\n\n", file_path));

		for (idx, block) in file_blocks.iter().enumerate() {
			output.push_str(&format!(
				"### Block {} of {} in file\n\n",
				idx + 1,
				file_blocks.len()
			));
			output.push_str(&format!("- **Language**: {}\n", block.language));
			output.push_str(&format!(
				"- **Lines**: {}-{}\n",
				block.start_line, block.end_line
			));

			// Show similarity score if available
			if let Some(distance) = block.distance {
				output.push_str(&format!("- **Similarity**: {:.4}\n", 1.0 - distance));
			}

			if !block.symbols.is_empty() {
				output.push_str("- **Symbols**: ");
				// Deduplicate symbols in display
				let mut display_symbols = block.symbols.clone();
				display_symbols.sort();
				display_symbols.dedup();

				let relevant_symbols: Vec<String> = display_symbols
					.iter()
					.filter(|symbol| !symbol.contains("_"))
					.cloned()
					.collect();

				if !relevant_symbols.is_empty() {
					output.push_str(&relevant_symbols.join(", "));
				}
				output.push('\n');
			}

			output.push_str("\n**Content:**\n\n");
			output.push_str("```");
			output.push_str(&block.language);
			output.push('\n');
			output.push_str(&block.content);
			output.push_str("\n```\n\n");
		}
	}

	output
}

// Format text search results as markdown for MCP
fn format_text_search_results_as_markdown(blocks: &[crate::store::TextBlock]) -> String {
	if blocks.is_empty() {
		return "No text results found.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!(
		"# Text Search Results\n\nFound {} text blocks:\n\n",
		blocks.len()
	));

	// Group blocks by file path for better organization
	let mut blocks_by_file: std::collections::HashMap<String, Vec<&crate::store::TextBlock>> =
		std::collections::HashMap::new();

	for block in blocks {
		blocks_by_file
			.entry(block.path.clone())
			.or_default()
			.push(block);
	}

	// Format results organized by file
	for (file_path, file_blocks) in blocks_by_file.iter() {
		output.push_str(&format!("## File: {}\n\n", file_path));

		for (idx, block) in file_blocks.iter().enumerate() {
			output.push_str(&format!(
				"### Block {} of {} in file\n\n",
				idx + 1,
				file_blocks.len()
			));
			output.push_str(&format!("- **Language**: {}\n", block.language));
			output.push_str(&format!(
				"- **Lines**: {}-{}\n",
				block.start_line, block.end_line
			));

			// Show similarity score if available
			if let Some(distance) = block.distance {
				output.push_str(&format!("- **Similarity**: {:.4}\n", 1.0 - distance));
			}

			output.push_str("\n**Content:**\n\n");
			output.push_str("```");
			output.push_str(&block.language);
			output.push('\n');
			output.push_str(&block.content);
			output.push_str("\n```\n\n");
		}
	}

	output
}

// Format document search results as markdown for MCP
fn format_doc_search_results_as_markdown(blocks: &[crate::store::DocumentBlock]) -> String {
	if blocks.is_empty() {
		return "No documentation results found.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!(
		"# Documentation Search Results\n\nFound {} documentation sections:\n\n",
		blocks.len()
	));

	// Group blocks by file path for better organization
	let mut blocks_by_file: std::collections::HashMap<String, Vec<&crate::store::DocumentBlock>> =
		std::collections::HashMap::new();

	for block in blocks {
		blocks_by_file
			.entry(block.path.clone())
			.or_default()
			.push(block);
	}

	// Format results organized by file
	for (file_path, file_blocks) in blocks_by_file.iter() {
		output.push_str(&format!("## File: {}\n\n", file_path));

		for (idx, block) in file_blocks.iter().enumerate() {
			output.push_str(&format!(
				"### Section {} of {} in file\n\n",
				idx + 1,
				file_blocks.len()
			));
			output.push_str(&format!("- **Title**: {}\n", block.title));
			output.push_str(&format!("- **Level**: {}\n", block.level));
			output.push_str(&format!(
				"- **Lines**: {}-{}\n",
				block.start_line, block.end_line
			));

			// Show similarity score if available
			if let Some(distance) = block.distance {
				output.push_str(&format!("- **Similarity**: {:.4}\n", 1.0 - distance));
			}

			output.push_str("\n**Content:**\n\n");
			output.push_str(&block.content);
			output.push_str("\n\n");
		}
	}

	output
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
		output.push_str(&format!("Lines {}-{}", block.start_line, block.end_line));

		if let Some(distance) = block.distance {
			output.push_str(&format!(" | Similarity {:.3}", 1.0 - distance));
		}
		output.push('\n');

		// Add symbols if available
		if !block.symbols.is_empty() {
			let mut display_symbols = block.symbols.clone();
			display_symbols.sort();
			display_symbols.dedup();
			let relevant_symbols: Vec<String> = display_symbols
				.iter()
				.filter(|symbol| !symbol.contains("_"))
				.cloned()
				.collect();

			if !relevant_symbols.is_empty() {
				output.push_str(&format!("Symbols: {}\n", relevant_symbols.join(", ")));
			}
		}

		// Add content as-is without truncation for text mode - only efficient labels
		match detail_level {
			"signatures" => {
				// Extract just function/class signatures
				let preview = get_code_preview(&block.content, &block.language);
				if !preview.is_empty() {
					if let Some(first_line) = preview.lines().next() {
						output.push_str(&format!("{}\n", first_line.trim()));
					}
				}
			}
			"partial" | "full" => {
				// Full content as-is without truncation
				output.push_str(&block.content);
				if !block.content.ends_with('\n') {
					output.push('\n');
				}
			}
			_ => {}
		}
		output.push('\n');
	}

	output
}

// Format text search results as text for MCP
pub fn format_text_search_results_as_text(blocks: &[crate::store::TextBlock]) -> String {
	if blocks.is_empty() {
		return "No text results found.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!("TEXT RESULTS ({})\n", blocks.len()));

	for (idx, block) in blocks.iter().enumerate() {
		output.push_str(&format!("{}. {}\n", idx + 1, block.path));
		output.push_str(&format!("Lines {}-{}", block.start_line, block.end_line));

		if let Some(distance) = block.distance {
			output.push_str(&format!(" | Similarity {:.3}", 1.0 - distance));
		}
		output.push('\n');

		// Add content as-is without truncation
		output.push_str(&block.content);
		if !block.content.ends_with('\n') {
			output.push('\n');
		}
		output.push('\n');
	}

	output
}

// Format document search results as text for MCP
pub fn format_doc_search_results_as_text(blocks: &[crate::store::DocumentBlock]) -> String {
	if blocks.is_empty() {
		return "No documentation results found.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!("DOCUMENTATION RESULTS ({})\n", blocks.len()));

	for (idx, block) in blocks.iter().enumerate() {
		output.push_str(&format!("{}. {}\n", idx + 1, block.path));
		output.push_str(&format!("{} (Level {})", block.title, block.level));
		output.push_str(&format!(" | Lines {}-{}", block.start_line, block.end_line));

		if let Some(distance) = block.distance {
			output.push_str(&format!(" | Similarity {:.3}", 1.0 - distance));
		}
		output.push('\n');

		// Add content as-is without truncation
		output.push_str(&block.content);
		if !block.content.ends_with('\n') {
			output.push('\n');
		}
		output.push('\n');
	}

	output
}

// Format combined search results as text for MCP with detail level control
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
		output.push_str(&format_doc_search_results_as_text(doc_blocks));
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
		output.push_str(&format_text_search_results_as_text(text_blocks));
	}

	output
}

// Format combined search results as markdown for MCP with detail level control
fn format_combined_search_results_with_detail(
	code_blocks: &[CodeBlock],
	text_blocks: &[crate::store::TextBlock],
	doc_blocks: &[crate::store::DocumentBlock],
	detail_level: &str,
) -> String {
	let mut output = String::new();
	output.push_str("# Combined Search Results\n\n");

	let total_results = code_blocks.len() + text_blocks.len() + doc_blocks.len();
	if total_results == 0 {
		return "No results found.".to_string();
	}

	output.push_str(&format!("Found {} total results:\n\n", total_results));

	// Documentation Results
	if !doc_blocks.is_empty() {
		output.push_str(&format_doc_search_results_as_markdown(doc_blocks));
		output.push('\n');
	}

	// Code Results with detail level
	if !code_blocks.is_empty() {
		output.push_str(&format_code_search_results_with_detail(
			code_blocks,
			detail_level,
		));
		output.push('\n');
	}

	// Text Results
	if !text_blocks.is_empty() {
		output.push_str(&format_text_search_results_as_markdown(text_blocks));
	}

	output
}

// Format combined search results as markdown for MCP
fn format_combined_search_results_as_markdown(
	code_blocks: &[CodeBlock],
	text_blocks: &[crate::store::TextBlock],
	doc_blocks: &[crate::store::DocumentBlock],
) -> String {
	let mut output = String::new();
	output.push_str("# Combined Search Results\n\n");

	let total_results = code_blocks.len() + text_blocks.len() + doc_blocks.len();
	if total_results == 0 {
		return "No results found.".to_string();
	}

	output.push_str(&format!("Found {} total results:\n\n", total_results));

	// Documentation Results
	if !doc_blocks.is_empty() {
		output.push_str(&format_doc_search_results_as_markdown(doc_blocks));
		output.push('\n');
	}

	// Code Results
	if !code_blocks.is_empty() {
		output.push_str(&format_code_search_results_as_markdown(code_blocks));
		output.push('\n');
	}

	// Text Results
	if !text_blocks.is_empty() {
		output.push_str(&format_text_search_results_as_markdown(text_blocks));
	}

	output
}

// Enhanced search function for MCP server with detail level control - returns formatted text results (token-efficient)
pub async fn search_codebase_with_details_text(
	query: &str,
	mode: &str,
	detail_level: &str,
	max_results: usize,
	config: &Config,
) -> Result<String> {
	// Initialize store
	let store = Store::new().await?;

	// Generate embeddings for the query using centralized logic
	let search_embeddings =
		crate::embedding::generate_search_embeddings(query, mode, config).await?;

	// Perform the search based on mode
	match mode {
		"code" => {
			let embeddings = search_embeddings.code_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No code embeddings generated for code search mode")
			})?;
			let results = store
				.get_code_blocks_with_config(
					embeddings,
					Some(max_results),
					Some(config.search.similarity_threshold),
				)
				.await?;
			Ok(format_code_search_results_as_text(&results, detail_level))
		}
		"text" => {
			let embeddings = search_embeddings.text_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No text embeddings generated for text search mode")
			})?;
			let results = store
				.get_text_blocks_with_config(
					embeddings,
					Some(max_results),
					Some(config.search.similarity_threshold),
				)
				.await?;
			Ok(format_text_search_results_as_text(&results))
		}
		"docs" => {
			let embeddings = search_embeddings.text_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No text embeddings generated for docs search mode")
			})?;
			let results = store
				.get_document_blocks_with_config(
					embeddings,
					Some(max_results),
					Some(config.search.similarity_threshold),
				)
				.await?;
			Ok(format_doc_search_results_as_text(&results))
		}
		"all" => {
			// "all" mode - search across all types with limited results per type
			let code_embeddings = search_embeddings.code_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No code embeddings generated for all search mode")
			})?;
			let text_embeddings = search_embeddings.text_embeddings.ok_or_else(|| {
				anyhow::anyhow!("No text embeddings generated for all search mode")
			})?;

			let results_per_type = max_results.div_ceil(3); // Distribute results across types
			let code_results = store
				.get_code_blocks_with_config(
					code_embeddings,
					Some(results_per_type),
					Some(config.search.similarity_threshold),
				)
				.await?;
			let text_results = store
				.get_text_blocks_with_config(
					text_embeddings.clone(),
					Some(results_per_type),
					Some(config.search.similarity_threshold),
				)
				.await?;
			let doc_results = store
				.get_document_blocks_with_config(
					text_embeddings,
					Some(results_per_type),
					Some(config.search.similarity_threshold),
				)
				.await?;

			// Format combined results with detail level for code
			Ok(format_combined_search_results_as_text(
				&code_results,
				&text_results,
				&doc_results,
				detail_level,
			))
		}
		_ => Err(anyhow::anyhow!(
			"Invalid search mode '{}'. Use 'all', 'code', 'docs', or 'text'.",
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
	config: &Config,
) -> Result<String> {
	// Initialize store
	let store = Store::new().await?;

	// Validate queries (same as CLI)
	if queries.is_empty() {
		return Err(anyhow::anyhow!("At least one query is required"));
	}
	if queries.len() > 3 {
		return Err(anyhow::anyhow!(
			"Maximum 3 queries allowed, got {}. Use fewer, more specific terms.",
			queries.len()
		));
	}

	// Generate batch embeddings for all queries
	let embeddings = generate_batch_embeddings_for_queries_mcp(queries, mode, config).await?;

	// Zip queries with embeddings
	let query_embeddings: Vec<_> = queries
		.iter()
		.cloned()
		.zip(embeddings.into_iter())
		.collect();

	// Execute parallel searches
	let search_results =
		execute_parallel_searches_mcp(&store, query_embeddings, mode, max_results).await?;

	// Convert similarity threshold (use default from config)
	let distance_threshold = 1.0 - config.search.similarity_threshold;

	// Deduplicate and merge with multi-query bonuses
	let (mut code_blocks, mut doc_blocks, mut text_blocks) =
		deduplicate_and_merge_results_mcp(search_results, queries, distance_threshold);

	// Apply global result limits
	code_blocks.truncate(max_results);
	doc_blocks.truncate(max_results);
	text_blocks.truncate(max_results);

	// Format results based on mode with detail level control
	match mode {
		"code" => Ok(format_code_search_results_as_text(
			&code_blocks,
			detail_level,
		)),
		"text" => Ok(format_text_search_results_as_text(&text_blocks)),
		"docs" => Ok(format_doc_search_results_as_text(&doc_blocks)),
		"all" => Ok(format_combined_search_results_as_text(
			&code_blocks,
			&text_blocks,
			&doc_blocks,
			detail_level,
		)),
		_ => Err(anyhow::anyhow!(
			"Invalid search mode '{}'. Use 'all', 'code', 'docs', or 'text'.",
			mode
		)),
	}
}

// Get a clean preview of code content by skipping comments and showing key parts
fn get_code_preview(content: &str, _language: &str) -> String {
	let lines: Vec<&str> = content.lines().collect();

	// If content is short, just return it all
	if lines.len() <= 10 {
		return content.to_string();
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

	// Take first 3-4 lines of actual code
	let preview_start = 4;
	let preview_end = 3;

	let mut result = Vec::new();

	// Add first few lines
	for line in lines.iter().skip(start_idx).take(preview_start) {
		result.push(*line);
	}

	// If there's more content, add separator and last few lines
	if start_idx + preview_start < lines.len() {
		let remaining_lines = lines.len() - (start_idx + preview_start);
		if remaining_lines > preview_end {
			result.push("// ... [content omitted] ...");

			// Add last few lines
			for line in lines.iter().skip(lines.len() - preview_end) {
				result.push(*line);
			}
		} else {
			// Just add the remaining lines
			for line in lines.iter().skip(start_idx + preview_start) {
				result.push(*line);
			}
		}
	}

	result.join("\n")
}

// Ensure path is relative to current working directory for display
fn ensure_relative_path(path: &str) -> String {
	if let Ok(current_dir) = std::env::current_dir() {
		if let Ok(absolute_path) = std::path::Path::new(path).canonicalize() {
			if let Ok(relative) = absolute_path.strip_prefix(&current_dir) {
				return relative.to_string_lossy().to_string();
			}
		}
	}

	// If path is already relative or we can't determine relative path, return as-is
	path.to_string()
}

// Enhanced search function for MCP server with multi-query support and detail level control
pub async fn search_codebase_with_details_multi_query(
	queries: &[String],
	mode: &str,
	detail_level: &str,
	max_results: usize,
	config: &Config,
) -> Result<String> {
	// Initialize store
	let store = Store::new().await?;

	// Validate queries (same as CLI)
	if queries.is_empty() {
		return Err(anyhow::anyhow!("At least one query is required"));
	}
	if queries.len() > 3 {
		return Err(anyhow::anyhow!(
			"Maximum 3 queries allowed, got {}. Use fewer, more specific terms.",
			queries.len()
		));
	}

	// Generate batch embeddings for all queries
	let embeddings = generate_batch_embeddings_for_queries_mcp(queries, mode, config).await?;

	// Zip queries with embeddings
	let query_embeddings: Vec<_> = queries
		.iter()
		.cloned()
		.zip(embeddings.into_iter())
		.collect();

	// Execute parallel searches
	let search_results =
		execute_parallel_searches_mcp(&store, query_embeddings, mode, max_results).await?;

	// Convert similarity threshold (use default from config)
	let distance_threshold = 1.0 - config.search.similarity_threshold;

	// Deduplicate and merge with multi-query bonuses
	let (mut code_blocks, mut doc_blocks, mut text_blocks) =
		deduplicate_and_merge_results_mcp(search_results, queries, distance_threshold);

	// Apply global result limits
	code_blocks.truncate(max_results);
	doc_blocks.truncate(max_results);
	text_blocks.truncate(max_results);

	// Format results based on mode with detail level control
	match mode {
		"code" => Ok(format_code_search_results_with_detail(
			&code_blocks,
			detail_level,
		)),
		"text" => Ok(format_text_search_results_as_markdown(&text_blocks)),
		"docs" => Ok(format_doc_search_results_as_markdown(&doc_blocks)),
		"all" => Ok(format_combined_search_results_with_detail(
			&code_blocks,
			&text_blocks,
			&doc_blocks,
			detail_level,
		)),
		_ => Err(anyhow::anyhow!(
			"Invalid search mode '{}'. Use 'all', 'code', 'docs', or 'text'.",
			mode
		)),
	}
}

// Helper functions for MCP multi-query search
async fn generate_batch_embeddings_for_queries_mcp(
	queries: &[String],
	mode: &str,
	config: &Config,
) -> Result<Vec<crate::embedding::SearchModeEmbeddings>> {
	match mode {
		"code" => {
			let code_embeddings =
				crate::embedding::generate_embeddings_batch(queries.to_vec(), true, config).await?;
			Ok(code_embeddings
				.into_iter()
				.map(|emb| crate::embedding::SearchModeEmbeddings {
					code_embeddings: Some(emb),
					text_embeddings: None,
				})
				.collect())
		}
		"docs" | "text" => {
			let text_embeddings =
				crate::embedding::generate_embeddings_batch(queries.to_vec(), false, config)
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
				let embeddings =
					crate::embedding::generate_embeddings_batch(queries.to_vec(), true, config)
						.await?;
				Ok(embeddings
					.into_iter()
					.map(|emb| crate::embedding::SearchModeEmbeddings {
						code_embeddings: Some(emb.clone()),
						text_embeddings: Some(emb),
					})
					.collect())
			} else {
				let (code_embeddings, text_embeddings) = tokio::try_join!(
					crate::embedding::generate_embeddings_batch(queries.to_vec(), true, config),
					crate::embedding::generate_embeddings_batch(queries.to_vec(), false, config)
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

#[derive(Debug)]
struct QuerySearchResultMcp {
	query_index: usize,
	code_blocks: Vec<CodeBlock>,
	doc_blocks: Vec<crate::store::DocumentBlock>,
	text_blocks: Vec<crate::store::TextBlock>,
}

async fn execute_parallel_searches_mcp(
	store: &Store,
	query_embeddings: Vec<(String, crate::embedding::SearchModeEmbeddings)>,
	mode: &str,
	max_results: usize,
) -> Result<Vec<QuerySearchResultMcp>> {
	let per_query_limit = (max_results * 2) / query_embeddings.len().max(1);

	let search_futures: Vec<_> = query_embeddings
		.into_iter()
		.enumerate()
		.map(|(index, (query, embeddings))| async move {
			execute_single_search_with_embeddings_mcp(
				store,
				&query,
				embeddings,
				mode,
				per_query_limit,
				index,
			)
			.await
		})
		.collect();

	futures::future::try_join_all(search_futures).await
}

async fn execute_single_search_with_embeddings_mcp(
	store: &Store,
	query: &str,
	embeddings: crate::embedding::SearchModeEmbeddings,
	mode: &str,
	limit: usize,
	query_index: usize,
) -> Result<QuerySearchResultMcp> {
	let (code_blocks, doc_blocks, text_blocks) = match mode {
		"code" => {
			let code_embeddings = embeddings
				.code_embeddings
				.ok_or_else(|| anyhow::anyhow!("No code embeddings for code search"))?;
			let mut blocks = store
				.get_code_blocks_with_config(code_embeddings, Some(limit), Some(1.01))
				.await?;
			blocks = crate::reranker::Reranker::rerank_code_blocks(blocks, query);
			crate::reranker::Reranker::tf_idf_boost(&mut blocks, query);
			(blocks, vec![], vec![])
		}
		"docs" => {
			let text_embeddings = embeddings
				.text_embeddings
				.ok_or_else(|| anyhow::anyhow!("No text embeddings for docs search"))?;
			let mut blocks = store
				.get_document_blocks_with_config(text_embeddings, Some(limit), Some(1.01))
				.await?;
			blocks = crate::reranker::Reranker::rerank_document_blocks(blocks, query);
			(vec![], blocks, vec![])
		}
		"text" => {
			let text_embeddings = embeddings
				.text_embeddings
				.ok_or_else(|| anyhow::anyhow!("No text embeddings for text search"))?;
			let mut blocks = store
				.get_text_blocks_with_config(text_embeddings, Some(limit), Some(1.01))
				.await?;
			blocks = crate::reranker::Reranker::rerank_text_blocks(blocks, query);
			(vec![], vec![], blocks)
		}
		"all" => {
			let code_embeddings = embeddings
				.code_embeddings
				.ok_or_else(|| anyhow::anyhow!("No code embeddings for all search"))?;
			let text_embeddings = embeddings
				.text_embeddings
				.ok_or_else(|| anyhow::anyhow!("No text embeddings for all search"))?;

			let (mut code_blocks, mut doc_blocks, mut text_blocks) = tokio::try_join!(
				store.get_code_blocks_with_config(code_embeddings, Some(limit), Some(1.01)),
				store.get_document_blocks_with_config(
					text_embeddings.clone(),
					Some(limit),
					Some(1.01)
				),
				store.get_text_blocks_with_config(text_embeddings, Some(limit), Some(1.01))
			)?;

			code_blocks = crate::reranker::Reranker::rerank_code_blocks(code_blocks, query);
			doc_blocks = crate::reranker::Reranker::rerank_document_blocks(doc_blocks, query);
			text_blocks = crate::reranker::Reranker::rerank_text_blocks(text_blocks, query);

			crate::reranker::Reranker::tf_idf_boost(&mut code_blocks, query);

			(code_blocks, doc_blocks, text_blocks)
		}
		_ => unreachable!(),
	};

	Ok(QuerySearchResultMcp {
		query_index,
		code_blocks,
		doc_blocks,
		text_blocks,
	})
}

fn deduplicate_and_merge_results_mcp(
	search_results: Vec<QuerySearchResultMcp>,
	queries: &[String],
	threshold: f32,
) -> (
	Vec<CodeBlock>,
	Vec<crate::store::DocumentBlock>,
	Vec<crate::store::TextBlock>,
) {
	use std::cmp::Ordering;
	use std::collections::HashMap;

	// Deduplicate code blocks
	let mut code_map: HashMap<String, (CodeBlock, Vec<usize>)> = HashMap::new();

	for result in &search_results {
		for block in &result.code_blocks {
			match code_map.entry(block.hash.clone()) {
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

	// Similar logic for doc and text blocks...
	let mut doc_map: HashMap<String, (crate::store::DocumentBlock, Vec<usize>)> = HashMap::new();
	let mut text_map: HashMap<String, (crate::store::TextBlock, Vec<usize>)> = HashMap::new();

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
					}
				}
			}
		}

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
					}
				}
			}
		}
	}

	// Apply multi-query bonuses and filter
	let mut final_code_blocks: Vec<CodeBlock> = code_map
		.into_values()
		.map(|(mut block, query_indices)| {
			apply_multi_query_bonus_code_mcp(&mut block, &query_indices, queries.len());
			block
		})
		.filter(|block| {
			if let Some(distance) = block.distance {
				distance <= threshold
			} else {
				true
			}
		})
		.collect();

	let mut final_doc_blocks: Vec<crate::store::DocumentBlock> = doc_map
		.into_values()
		.map(|(mut block, query_indices)| {
			apply_multi_query_bonus_doc_mcp(&mut block, &query_indices, queries.len());
			block
		})
		.filter(|block| {
			if let Some(distance) = block.distance {
				distance <= threshold
			} else {
				true
			}
		})
		.collect();

	let mut final_text_blocks: Vec<crate::store::TextBlock> = text_map
		.into_values()
		.map(|(mut block, query_indices)| {
			apply_multi_query_bonus_text_mcp(&mut block, &query_indices, queries.len());
			block
		})
		.filter(|block| {
			if let Some(distance) = block.distance {
				distance <= threshold
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

	(final_code_blocks, final_doc_blocks, final_text_blocks)
}

fn apply_multi_query_bonus_code_mcp(
	block: &mut CodeBlock,
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

fn apply_multi_query_bonus_doc_mcp(
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

fn apply_multi_query_bonus_text_mcp(
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
