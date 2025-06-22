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

//! Differential processing utilities for incremental updates
//!
//! This module handles the efficient processing of files by only updating
//! blocks that have changed, removing obsolete blocks, and maintaining
//! consistency in the database.

use crate::config::Config;
use crate::embedding::{calculate_content_hash_with_lines, calculate_unique_content_hash};
use crate::indexer::code_region_extractor::extract_meaningful_regions;
use crate::indexer::file_processor::chunk_text;
use crate::indexer::languages;
use crate::indexer::markdown_processor::parse_markdown_content;
use crate::state::SharedState;
use crate::store::{CodeBlock, DocumentBlock, Store, TextBlock};
use anyhow::Result;
use std::collections::HashSet;
use tree_sitter::Parser;

/// Context for file processing to reduce the number of function arguments
pub struct ProcessFileContext<'a> {
	pub store: &'a Store,
	pub config: &'a Config,
	pub state: SharedState,
}

/// Differential processing for code files - only updates changed blocks
pub async fn process_file_differential(
	ctx: &ProcessFileContext<'_>,
	contents: &str,
	file_path: &str,
	language: &str,
	code_blocks_batch: &mut Vec<CodeBlock>,
	_text_blocks_batch: &mut [TextBlock], // Unused for code files
	all_code_blocks: &mut Vec<CodeBlock>,
) -> Result<()> {
	let mut parser = Parser::new();

	// Get force_reindex flag from state
	let force_reindex = ctx.state.read().force_reindex;

	// Get the language implementation
	let lang_impl = match languages::get_language(language) {
		Some(impl_) => impl_,
		None => return Ok(()), // Skip unsupported languages
	};

	// Set the parser language
	parser.set_language(&lang_impl.get_ts_language())?;

	let tree = parser
		.parse(contents, None)
		.unwrap_or_else(|| parser.parse("", None).unwrap());
	let mut code_regions = Vec::new();

	extract_meaningful_regions(
		tree.root_node(),
		contents,
		lang_impl.as_ref(),
		&mut code_regions,
	);

	// If not force reindexing, get existing hashes for this file to compare
	let existing_hashes = if force_reindex {
		Vec::new()
	} else {
		ctx.store
			.get_file_blocks_metadata(file_path, "code_blocks")
			.await?
	};

	// Create set of new hashes for this file
	let mut new_hashes = HashSet::new();
	let mut graphrag_blocks_added = 0;

	for region in code_regions {
		// Use a hash that includes content, path, and line ranges
		let content_hash = calculate_content_hash_with_lines(
			&region.content,
			file_path,
			region.start_line,
			region.end_line,
		);
		new_hashes.insert(content_hash.clone());

		// Skip the check if force_reindex is true
		let exists = !force_reindex
			&& ctx
				.store
				.content_exists(&content_hash, "code_blocks")
				.await?;
		if !exists {
			let code_block = CodeBlock {
				path: file_path.to_string(),
				hash: content_hash.clone(),
				language: lang_impl.name().to_string(),
				content: region.content.clone(),
				symbols: region.symbols.clone(),
				start_line: region.start_line,
				end_line: region.end_line,
				distance: None, // No relevance score when indexing
			};

			// Add to batch for embedding
			code_blocks_batch.push(code_block.clone());

			// Add to all code blocks for GraphRAG
			if ctx.config.graphrag.enabled {
				all_code_blocks.push(code_block);
				graphrag_blocks_added += 1;
			}
		} else if ctx.config.graphrag.enabled {
			// If skipping because block exists, but we need for GraphRAG, fetch from store
			if let Ok(existing_block) = ctx.store.get_code_block_by_hash(&content_hash).await {
				// Add the existing block to the GraphRAG collection
				all_code_blocks.push(existing_block);
				graphrag_blocks_added += 1;
			}
		}
	}

	// Remove blocks that no longer exist (only if not force reindexing)
	if !force_reindex && !existing_hashes.is_empty() {
		let hashes_to_remove: Vec<String> = existing_hashes
			.into_iter()
			.filter(|hash| !new_hashes.contains(hash))
			.collect();

		if !hashes_to_remove.is_empty() {
			ctx.store
				.remove_blocks_by_hashes(&hashes_to_remove, "code_blocks")
				.await?;
		}
	}

	// Update GraphRAG state if enabled and blocks were added
	if ctx.config.graphrag.enabled && graphrag_blocks_added > 0 {
		let mut state_guard = ctx.state.write();
		state_guard.graphrag_blocks += graphrag_blocks_added;
	}

	Ok(())
}

/// Differential processing for text files - only updates changed blocks
/// FIXED: Now stores simple path without chunk numbers
pub async fn process_text_file_differential(
	store: &Store,
	contents: &str,
	file_path: &str,
	text_blocks_batch: &mut Vec<TextBlock>,
	config: &Config,
	state: SharedState,
) -> Result<()> {
	let force_reindex = state.read().force_reindex;

	// Get existing text block hashes for this file (including chunked versions)
	let existing_hashes = if force_reindex {
		Vec::new()
	} else {
		// Get blocks for this file path (the chunks will have same path now)
		store
			.get_file_blocks_metadata(file_path, "text_blocks")
			.await?
	};

	// Split content into chunks using configuration values
	let chunks = chunk_text(
		contents,
		config.index.chunk_size,
		config.index.chunk_overlap,
	);
	let mut new_hashes = HashSet::new();

	for (chunk_idx, chunk_with_lines) in chunks.iter().enumerate() {
		// Use chunk index in hash for uniqueness but keep path clean
		let chunk_hash = calculate_unique_content_hash(
			&chunk_with_lines.content,
			&format!("{}#{}", file_path, chunk_idx),
		);
		new_hashes.insert(chunk_hash.clone());

		// Skip the check if force_reindex is true
		let exists = !force_reindex && store.content_exists(&chunk_hash, "text_blocks").await?;
		if !exists {
			text_blocks_batch.push(TextBlock {
				path: file_path.to_string(),
				language: "text".to_string(),
				content: chunk_with_lines.content.clone(),
				start_line: chunk_with_lines.start_line, // Use directly from TextChunkWithLines
				end_line: chunk_with_lines.end_line,     // Use directly from TextChunkWithLines
				hash: chunk_hash,
				distance: None,
			});
		}
	}

	// Remove blocks that no longer exist (only if not force reindexing)
	if !force_reindex && !existing_hashes.is_empty() {
		let hashes_to_remove: Vec<String> = existing_hashes
			.into_iter()
			.filter(|hash| !new_hashes.contains(hash))
			.collect();

		if !hashes_to_remove.is_empty() {
			store
				.remove_blocks_by_hashes(&hashes_to_remove, "text_blocks")
				.await?;
		}
	}

	Ok(())
}

/// Differential processing for markdown files - only updates changed blocks
pub async fn process_markdown_file_differential(
	store: &Store,
	contents: &str,
	file_path: &str,
	document_blocks_batch: &mut Vec<DocumentBlock>,
	config: &Config,
	state: SharedState,
) -> Result<()> {
	// Get force_reindex flag from state
	let force_reindex = state.read().force_reindex;

	// Get existing document block hashes for this file
	let existing_hashes = if force_reindex {
		Vec::new()
	} else {
		store
			.get_file_blocks_metadata(file_path, "document_blocks")
			.await?
	};

	// Parse markdown content into document blocks using context-aware chunking
	let document_blocks = parse_markdown_content(contents, file_path, config);
	let mut new_hashes = HashSet::new();

	for doc_block in document_blocks {
		new_hashes.insert(doc_block.hash.clone());

		// Check if this document block already exists (unless force reindex)
		let exists = !force_reindex
			&& store
				.content_exists(&doc_block.hash, "document_blocks")
				.await?;
		if !exists {
			document_blocks_batch.push(doc_block);
		}
	}

	// Remove blocks that no longer exist (only if not force reindexing)
	if !force_reindex && !existing_hashes.is_empty() {
		let hashes_to_remove: Vec<String> = existing_hashes
			.into_iter()
			.filter(|hash| !new_hashes.contains(hash))
			.collect();

		if !hashes_to_remove.is_empty() {
			store
				.remove_blocks_by_hashes(&hashes_to_remove, "document_blocks")
				.await?;
		}
	}

	Ok(())
}

/// Legacy processing for code files (non-differential)
/// Processes a single file, extracting code blocks and adding them to the batch
pub async fn process_file(
	ctx: &ProcessFileContext<'_>,
	contents: &str,
	file_path: &str,
	language: &str,
	code_blocks_batch: &mut Vec<CodeBlock>,
	_text_blocks_batch: &mut [TextBlock], // Unused for code files - only used for unsupported files
	all_code_blocks: &mut Vec<CodeBlock>,
) -> Result<()> {
	let mut parser = Parser::new();

	// Get force_reindex flag from state
	let force_reindex = ctx.state.read().force_reindex;

	// Get the language implementation
	let lang_impl = match languages::get_language(language) {
		Some(impl_) => impl_,
		None => return Ok(()), // Skip unsupported languages
	};

	// Set the parser language
	parser.set_language(&lang_impl.get_ts_language())?;

	let tree = parser
		.parse(contents, None)
		.unwrap_or_else(|| parser.parse("", None).unwrap());
	let mut code_regions = Vec::new();

	extract_meaningful_regions(
		tree.root_node(),
		contents,
		lang_impl.as_ref(),
		&mut code_regions,
	);

	// Track the number of blocks we added to all_code_blocks for GraphRAG
	let mut graphrag_blocks_added = 0;

	for region in code_regions {
		// Use a hash that includes content, path, and line ranges
		let content_hash = calculate_content_hash_with_lines(
			&region.content,
			file_path,
			region.start_line,
			region.end_line,
		);

		// Skip the check if force_reindex is true
		let exists = !force_reindex
			&& ctx
				.store
				.content_exists(&content_hash, "code_blocks")
				.await?;
		if !exists {
			let code_block = CodeBlock {
				path: file_path.to_string(),
				hash: content_hash,
				language: lang_impl.name().to_string(),
				content: region.content.clone(),
				symbols: region.symbols.clone(),
				start_line: region.start_line,
				end_line: region.end_line,
				distance: None, // No relevance score when indexing
			};

			// Add to batch for embedding
			code_blocks_batch.push(code_block.clone());

			// Add to all code blocks for GraphRAG
			if ctx.config.graphrag.enabled {
				all_code_blocks.push(code_block);
				graphrag_blocks_added += 1;
			}
		} else if ctx.config.graphrag.enabled {
			// If skipping because block exists, but we need for GraphRAG, fetch from store
			if let Ok(existing_block) = ctx.store.get_code_block_by_hash(&content_hash).await {
				// Add the existing block to the GraphRAG collection
				all_code_blocks.push(existing_block);
				graphrag_blocks_added += 1;
			}
		}
	}

	// Update GraphRAG state if enabled and blocks were added
	if ctx.config.graphrag.enabled && graphrag_blocks_added > 0 {
		let mut state_guard = ctx.state.write();
		state_guard.graphrag_blocks += graphrag_blocks_added;
	}

	// Note: We DON'T create text blocks for code files - only for unsupported file types
	// Code files are already indexed as semantic code blocks above

	Ok(())
}
