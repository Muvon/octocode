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

//! File processing utilities for text and markdown files
//!
//! This module handles the processing of text and markdown files, including
//! chunking, content validation, and block creation for indexing.

use crate::config::Config;
use crate::embedding::calculate_unique_content_hash;
use crate::indexer::file_utils::FileUtils;
use crate::indexer::markdown_processor::parse_markdown_content;
use crate::indexer::text_processing::{TextChunkWithLines, TextProcessor};
use crate::state::SharedState;
use crate::store::{DocumentBlock, Store, TextBlock};
use anyhow::Result;

/// Check if a file extension is allowed for text indexing
pub fn is_allowed_text_extension(path: &std::path::Path) -> bool {
	FileUtils::is_allowed_text_extension(path)
}

/// Check if a file is a markdown file
pub fn is_markdown_file(path: &std::path::Path) -> bool {
	if let Some(extension) = path.extension() {
		if let Some(ext_str) = extension.to_str() {
			let ext_lower = ext_str.to_lowercase();
			return ext_lower == "md" || ext_lower == "markdown";
		}
	}
	false
}

/// Check if a file contains readable text
pub fn is_text_file(contents: &str) -> bool {
	FileUtils::is_text_file(contents)
}

/// Chunk text content using configuration parameters
pub fn chunk_text(content: &str, chunk_size: usize, overlap: usize) -> Vec<TextChunkWithLines> {
	TextProcessor::chunk_text(content, chunk_size, overlap)
}

/// Process an unsupported file as chunked text blocks
/// Only processes files with whitelisted extensions to avoid indexing
/// binary files, lock files, and other non-useful content.
/// Supported extensions: txt, log, xml, html, css, sql, csv, yaml, toml, ini, conf, etc.
/// Chunk size: 2000 characters with 200 character overlap.
/// FIXED: Now stores simple path without chunk numbers for better display
pub async fn process_text_file(
	store: &Store,
	contents: &str,
	file_path: &str,
	text_blocks_batch: &mut Vec<TextBlock>,
	config: &Config,
	state: SharedState,
) -> Result<()> {
	let force_reindex = state.read().force_reindex;

	// Split content into chunks using configuration values
	let chunks = chunk_text(
		contents,
		config.index.chunk_size,
		config.index.chunk_overlap,
	);

	for (chunk_idx, chunk_with_lines) in chunks.iter().enumerate() {
		// Use chunk index in hash for uniqueness but keep path clean
		let chunk_hash = calculate_unique_content_hash(
			&chunk_with_lines.content,
			&format!("{}#{}", file_path, chunk_idx),
		);

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

	Ok(())
}

/// Process a markdown file, extracting document blocks
pub async fn process_markdown_file(
	store: &Store,
	contents: &str,
	file_path: &str,
	document_blocks_batch: &mut Vec<DocumentBlock>,
	config: &Config,
	state: SharedState,
) -> Result<()> {
	// Get force_reindex flag from state
	let force_reindex = state.read().force_reindex;

	// Parse markdown content into document blocks using context-aware chunking
	let document_blocks = parse_markdown_content(contents, file_path, config);

	for doc_block in document_blocks {
		// Check if this document block already exists (unless force reindex)
		let exists = !force_reindex
			&& store
				.content_exists(&doc_block.hash, "document_blocks")
				.await?;
		if !exists {
			document_blocks_batch.push(doc_block);
		}
	}

	Ok(())
}
