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

//! Batch processing utilities for embedding operations
//!
//! This module handles the efficient processing of code, text, and document blocks
//! in batches for embedding generation and storage.

use crate::config::Config;
use crate::embedding::count_tokens;
use crate::indexer::languages;
use crate::indexer::signature_extractor::{extract_signatures, SignatureItem};
use crate::mcp::logging::log_performance_metrics;
use crate::store::{CodeBlock, DocumentBlock, Store, TextBlock};
use anyhow::Result;
use std::collections::HashMap;
use tree_sitter::Parser;

/// Format code block with file context for embedding generation
fn format_code_block_with_context(block: &CodeBlock, signatures: &[SignatureItem]) -> String {
	let mut context_parts = Vec::new();

	// Add file information
	context_parts.push(format!("File: {}", block.path));
	context_parts.push(format!("Language: {}", block.language));

	// Add file signatures if available
	if !signatures.is_empty() {
		context_parts.push(String::from("\nFile Structure:"));
		for sig in signatures {
			// Format signature based on kind
			let sig_line = match sig.kind.as_str() {
				"function" | "method" => format!("- {} {}()", sig.kind, sig.name),
				"class" | "struct" | "interface" | "trait" => {
					format!("- {} {}", sig.kind, sig.name)
				}
				"type" | "enum" => format!("- {} {}", sig.kind, sig.name),
				_ => format!("- {}", sig.name),
			};
			context_parts.push(sig_line);
		}
	}

	// Add symbols from the current block if available
	if !block.symbols.is_empty() {
		context_parts.push(String::from("\nBlock Symbols:"));
		for symbol in &block.symbols {
			context_parts.push(format!("- {}", symbol));
		}
	}

	// Add the actual code block content
	context_parts.push(String::from("\nCode:"));
	context_parts.push(block.content.clone());

	context_parts.join("\n")
}

/// Extract signatures from file content for context
fn extract_file_signatures_for_context(
	_file_path: &str,
	contents: &str,
	language: &str,
) -> Vec<SignatureItem> {
	let mut parser = Parser::new();

	// Get the language implementation
	let lang_impl = match languages::get_language(language) {
		Some(impl_) => impl_,
		None => return Vec::new(), // Return empty for unsupported languages
	};

	// Set the parser language
	if parser.set_language(&lang_impl.get_ts_language()).is_err() {
		return Vec::new();
	}

	// Parse the file
	let tree = parser
		.parse(contents, None)
		.unwrap_or_else(|| parser.parse("", None).unwrap());

	// Extract signatures from the file
	extract_signatures(tree.root_node(), contents, lang_impl.as_ref())
}
/// Process a batch of code blocks for embedding and storage
pub async fn process_code_blocks_batch(
	store: &Store,
	blocks: &[CodeBlock],
	config: &Config,
) -> Result<()> {
	let start_time = std::time::Instant::now();

	// Group blocks by file path to extract signatures once per file
	let mut blocks_by_file: HashMap<String, Vec<&CodeBlock>> = HashMap::new();
	for block in blocks {
		blocks_by_file
			.entry(block.path.clone())
			.or_default()
			.push(block);
	}

	// Extract signatures for each file and prepare enriched content for embeddings
	let mut enriched_contents = Vec::new();
	for (file_path, file_blocks) in blocks_by_file {
		// Try to read the file to get signatures (if file still exists)
		let signatures = if let Ok(contents) = std::fs::read_to_string(&file_path) {
			// Detect language from the first block (all blocks from same file have same language)
			if let Some(first_block) = file_blocks.first() {
				extract_file_signatures_for_context(&file_path, &contents, &first_block.language)
			} else {
				Vec::new()
			}
		} else {
			// File doesn't exist or can't be read, use empty signatures
			Vec::new()
		};

		// Create enriched content for each block in this file
		for block in file_blocks {
			let enriched_content = format_code_block_with_context(block, &signatures);
			enriched_contents.push(enriched_content);
		}
	}

	// Generate embeddings with enriched content
	let embeddings = crate::embedding::generate_embeddings_batch(
		enriched_contents,
		true,
		config,
		crate::embedding::types::InputType::Document,
	)
	.await?;

	// Store blocks with their embeddings (original blocks unchanged)
	store.store_code_blocks(blocks, &embeddings).await?;

	let duration_ms = start_time.elapsed().as_millis() as u64;
	log_performance_metrics("code_blocks_batch", duration_ms, blocks.len(), None);

	Ok(())
}

/// Process a batch of text blocks for embedding and storage
pub async fn process_text_blocks_batch(
	store: &Store,
	blocks: &[TextBlock],
	config: &Config,
) -> Result<()> {
	let start_time = std::time::Instant::now();
	let contents: Vec<String> = blocks.iter().map(|b| b.content.clone()).collect();
	let embeddings = crate::embedding::generate_embeddings_batch(
		contents,
		false,
		config,
		crate::embedding::types::InputType::Document,
	)
	.await?;
	store.store_text_blocks(blocks, &embeddings).await?;

	let duration_ms = start_time.elapsed().as_millis() as u64;
	log_performance_metrics("text_blocks_batch", duration_ms, blocks.len(), None);

	Ok(())
}

/// Process a batch of document blocks for embedding and storage
pub async fn process_document_blocks_batch(
	store: &Store,
	blocks: &[DocumentBlock],
	config: &Config,
) -> Result<()> {
	let start_time = std::time::Instant::now();
	let contents: Vec<String> = blocks
		.iter()
		.map(|b| {
			if !b.context.is_empty() {
				format!("{}\n\n{}", b.context.join("\n"), b.content)
			} else {
				b.content.clone()
			}
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
	fn test_format_code_block_with_context() {
		// Create a sample code block
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

		// Create sample signatures
		let signatures = vec![
			SignatureItem {
				kind: "function".to_string(),
				name: "main".to_string(),
				signature: "fn main()".to_string(),
				description: None,
				start_line: 1,
				end_line: 3,
			},
			SignatureItem {
				kind: "struct".to_string(),
				name: "Config".to_string(),
				signature: "struct Config".to_string(),
				description: None,
				start_line: 5,
				end_line: 10,
			},
		];

		// Format with context
		let formatted = format_code_block_with_context(&block, &signatures);

		// Verify the formatted output contains expected elements
		assert!(formatted.contains("File: src/main.rs"));
		assert!(formatted.contains("Language: rust"));
		assert!(formatted.contains("File Structure:"));
		assert!(formatted.contains("- function main()"));
		assert!(formatted.contains("- struct Config"));
		assert!(formatted.contains("Block Symbols:"));
		assert!(formatted.contains("- main"));
		assert!(formatted.contains("Code:"));
		assert!(formatted.contains("fn main()"));
		assert!(formatted.contains("Hello, world!"));

		// Test with empty signatures
		let formatted_no_sigs = format_code_block_with_context(&block, &[]);
		assert!(formatted_no_sigs.contains("File: src/main.rs"));
		assert!(!formatted_no_sigs.contains("File Structure:"));
	}

	#[test]
	fn test_format_code_block_without_symbols() {
		// Create a code block without symbols
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

		let formatted = format_code_block_with_context(&block, &[]);

		// Should not have Block Symbols section
		assert!(!formatted.contains("Block Symbols:"));
		assert!(formatted.contains("File: src/utils.rs"));
		assert!(formatted.contains("const VERSION"));
	}
}
