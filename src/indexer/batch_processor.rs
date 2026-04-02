// Copyright 2025 Muvon Un Limited
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Batch processing utilities for embedding operations
//!
//! This module handles the efficient processing of code, text, and document blocks
//! in batches for embedding generation and storage.

use crate::config::Config;
use crate::embedding::count_tokens;
use crate::indexer::contextual::{
	build_enriched_embedding_input, generate_contextual_descriptions, FileContextMap,
};
use crate::mcp::logging::log_performance_metrics;
use crate::store::{CodeBlock, DocumentBlock, Store, TextBlock};
use anyhow::Result;
use std::collections::HashMap;

/// Tracks file metadata for atomic storage after batch persistence
/// This ensures file metadata is only stored AFTER blocks are successfully persisted
#[derive(Default)]
pub struct FileMetadataBatch {
	/// Map of file path -> modification time
	pending_files: HashMap<String, u64>,
}

impl FileMetadataBatch {
	/// Create a new empty batch
	pub fn new() -> Self {
		Self::default()
	}

	/// Add a file's metadata to the pending batch
	pub fn add(&mut self, file_path: &str, mtime: u64) {
		self.pending_files.insert(file_path.to_string(), mtime);
	}

	/// Merge another batch into this one
	pub fn extend(&mut self, other: &FileMetadataBatch) {
		self.pending_files.extend(other.pending_files.clone());
	}

	/// Check if empty
	pub fn is_empty(&self) -> bool {
		self.pending_files.is_empty()
	}

	/// Clear all pending metadata
	pub fn clear(&mut self) {
		self.pending_files.clear();
	}

	/// Persist all pending file metadata to the store
	/// This should only be called AFTER blocks are successfully stored
	pub async fn persist(&self, store: &Store) -> Result<()> {
		for (file_path, mtime) in &self.pending_files {
			store.store_file_metadata(file_path, *mtime).await?;
		}
		Ok(())
	}
}

/// Process a batch of code blocks for embedding and storage
/// Also stores file metadata atomically after successful block storage
pub async fn process_code_blocks_batch(
	store: &Store,
	blocks: &[CodeBlock],
	config: &Config,
	file_metadata: &FileMetadataBatch,
	file_context: &FileContextMap,
) -> Result<()> {
	let start_time = std::time::Instant::now();

	// Step 1: Generate LLM contextual descriptions if enabled
	let descriptions = if config.index.contextual_descriptions {
		generate_contextual_descriptions(blocks, config, file_context).await?
	} else {
		HashMap::new()
	};

	// Step 2: Build enriched embedding input
	// Always prepends structural context (file path, language, symbols).
	// Optionally prepends LLM-generated natural language description.
	// The stored content remains the original code - enrichment is for embedding only.
	let contents: Vec<String> = blocks
		.iter()
		.enumerate()
		.map(|(i, block)| {
			build_enriched_embedding_input(block, descriptions.get(&i).map(|s| s.as_str()))
		})
		.collect();

	// Generate embeddings with symmetric input type (None) for code-to-code search
	let embeddings = crate::embedding::generate_embeddings_batch(
		contents,
		true,
		config,
		crate::embedding::types::InputType::None,
	)
	.await?;

	// Store blocks with their embeddings (original blocks unchanged)
	store.store_code_blocks(blocks, &embeddings).await?;

	// CRITICAL: Store file metadata AFTER successful block storage
	// This ensures atomicity - if blocks are stored, metadata is stored
	file_metadata.persist(store).await?;

	let duration_ms = start_time.elapsed().as_millis() as u64;
	log_performance_metrics("code_blocks_batch", duration_ms, blocks.len(), None);

	Ok(())
}

/// Process a batch of text blocks for embedding and storage
/// Also stores file metadata atomically after successful block storage
pub async fn process_text_blocks_batch(
	store: &Store,
	blocks: &[TextBlock],
	config: &Config,
	file_metadata: &FileMetadataBatch,
) -> Result<()> {
	let start_time = std::time::Instant::now();
	// Prepend file path context for text blocks (structural enrichment)
	let contents: Vec<String> = blocks
		.iter()
		.map(|b| format!("# File: {}\n\n{}", b.path, b.content))
		.collect();
	let embeddings = crate::embedding::generate_embeddings_batch(
		contents,
		false,
		config,
		crate::embedding::types::InputType::Document,
	)
	.await?;
	store.store_text_blocks(blocks, &embeddings).await?;

	// CRITICAL: Store file metadata AFTER successful block storage
	file_metadata.persist(store).await?;

	let duration_ms = start_time.elapsed().as_millis() as u64;
	log_performance_metrics("text_blocks_batch", duration_ms, blocks.len(), None);

	Ok(())
}

/// Process a batch of document blocks for embedding and storage
/// Also stores file metadata atomically after successful block storage
pub async fn process_document_blocks_batch(
	store: &Store,
	blocks: &[DocumentBlock],
	config: &Config,
	file_metadata: &FileMetadataBatch,
) -> Result<()> {
	let start_time = std::time::Instant::now();
	let contents: Vec<String> = blocks
		.iter()
		.map(|b| {
			let mut parts = Vec::new();
			// Always prepend file path for structural context
			parts.push(format!("# File: {}", b.path));
			if !b.context.is_empty() {
				parts.push(b.context.join("\n"));
			}
			parts.push(String::new());
			parts.push(b.content.clone());
			parts.join("\n")
		})
		.collect();
	let embeddings = crate::embedding::generate_embeddings_batch(
		contents,
		false,
		config,
		crate::embedding::types::InputType::Document,
	)
	.await?;
	store.store_document_blocks(blocks, &embeddings).await?;

	// CRITICAL: Store file metadata AFTER successful block storage
	file_metadata.persist(store).await?;

	let duration_ms = start_time.elapsed().as_millis() as u64;
	log_performance_metrics("document_blocks_batch", duration_ms, blocks.len(), None);

	Ok(())
}

/// Check if a batch should be processed based on size and token limits
pub fn should_process_batch<T>(
	batch: &[T],
	get_content: impl Fn(&T) -> &str,
	config: &Config,
) -> bool {
	if batch.is_empty() {
		return false;
	}

	// Check size limit
	if batch.len() >= config.index.embeddings_batch_size {
		return true;
	}

	// Check token limit
	let total_tokens: usize = batch
		.iter()
		.map(|item| count_tokens(get_content(item)))
		.sum();

	total_tokens >= config.index.embeddings_max_tokens_per_batch
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_code_block_enriched_formatting() {
		use crate::indexer::contextual::build_enriched_embedding_input;

		let block = CodeBlock {
			path: "src/main.rs".to_string(),
			language: "rust".to_string(),
			content: "fn main() {\n    println!(\"Hello, world!\");\n}".to_string(),
			symbols: vec!["main".to_string()],
			start_line: 1,
			end_line: 3,
			hash: "test_hash".to_string(),
			distance: None,
		};

		let formatted = build_enriched_embedding_input(&block, None);

		// Structural context is prepended
		assert!(formatted.contains("# File: src/main.rs"));
		assert!(formatted.contains("# Language: rust"));
		assert!(formatted.contains("# Defines: main"));
		// Code content is preserved
		assert!(formatted.contains("fn main()"));
		assert!(formatted.contains("Hello, world!"));
	}

	#[test]
	fn test_code_block_enriched_with_description() {
		use crate::indexer::contextual::build_enriched_embedding_input;

		let block = CodeBlock {
			path: "src/utils.rs".to_string(),
			language: "rust".to_string(),
			content: "const VERSION: &str = \"1.0.0\";".to_string(),
			symbols: vec![],
			start_line: 1,
			end_line: 1,
			hash: "test_hash2".to_string(),
			distance: None,
		};

		let formatted =
			build_enriched_embedding_input(&block, Some("Application version constant"));

		// Description comes first
		assert!(formatted.starts_with("Application version constant"));
		// Structural context present
		assert!(formatted.contains("# File: src/utils.rs"));
		// Code content preserved
		assert!(formatted.contains("const VERSION"));
	}
}
