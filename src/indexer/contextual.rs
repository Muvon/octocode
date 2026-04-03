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

//! Contextual chunk enrichment for improved semantic search
//!
//! Implements two levels of chunk enrichment:
//! 1. Structural context (always on): file path, language, symbols prepended before embedding
//! 2. LLM-generated descriptions (optional): natural language descriptions of code intent
//!
//! Based on Anthropic's Contextual Retrieval technique, which reduces retrieval
//! failure rates by 35-67% by bridging the vocabulary gap between natural language
//! queries and code tokens.

use crate::config::Config;
use crate::llm::{LlmClient, Message};
use crate::store::CodeBlock;
use anyhow::Result;
use std::collections::HashMap;

const CONTEXTUAL_SYSTEM_PROMPT: &str = "\
You are a code search indexing system. Your task: for each code chunk, write a 1-2 sentence \
natural language description of what it does and why it exists. \
These descriptions are used for search retrieval — use terms a developer would search for. \
Be specific to the code's domain and purpose. Do NOT repeat or paraphrase the code itself.";

/// File-level context extracted during tree-sitter parsing.
/// Provides imports and all symbols for a file, used to enrich LLM description prompts.
#[derive(Debug, Clone, Default)]
pub struct FileContext {
	pub imports: Vec<String>,
	pub all_symbols: Vec<String>,
}

/// Map of file path -> file-level context for LLM description enrichment
pub type FileContextMap = HashMap<String, FileContext>;

/// Build the structural context preamble for a code block.
/// This is ALWAYS prepended to embedding input regardless of LLM descriptions.
pub fn build_structural_context(block: &CodeBlock) -> String {
	let mut parts = Vec::with_capacity(4);
	parts.push(format!("# File: {}", block.path));
	parts.push(format!("# Language: {}", block.language));
	if !block.symbols.is_empty() {
		parts.push(format!("# Defines: {}", block.symbols.join(", ")));
	}
	parts.join("\n")
}

/// Build the full enriched embedding input for a code block.
/// Combines: [LLM description] + structural context + code content
pub fn build_enriched_embedding_input(block: &CodeBlock, description: Option<&str>) -> String {
	let mut parts = Vec::with_capacity(4);

	// LLM-generated description first (highest semantic signal)
	if let Some(desc) = description {
		if !desc.is_empty() {
			parts.push(desc.to_string());
			parts.push(String::new()); // blank line separator
		}
	}

	// Structural context
	parts.push(build_structural_context(block));

	// Blank line separator before code
	parts.push(String::new());

	// Actual code content
	parts.push(block.content.clone());

	parts.join("\n")
}

/// Generate contextual descriptions for a batch of code blocks using LLM.
/// Returns a map of block index -> description string.
/// Processes blocks in sub-batches according to config.contextual_batch_size.
///
/// `file_context` provides file-level imports and sibling symbols to enrich the LLM prompt.
pub async fn generate_contextual_descriptions(
	blocks: &[CodeBlock],
	config: &Config,
	file_context: &FileContextMap,
) -> Result<HashMap<usize, String>> {
	let mut descriptions = HashMap::new();

	// When contextual descriptions are enabled, LLM is required — no silent fallback.
	let client = LlmClient::with_model(config, &config.index.contextual_model).map_err(|e| {
		anyhow::anyhow!(
			"LLM required for contextual descriptions but unavailable (model: {}): {}",
			config.index.contextual_model,
			e
		)
	})?;

	// Build per-file sibling symbols from blocks in this batch
	// (complements file_context which has symbols from all regions in the file)
	let siblings = build_siblings_map(blocks);

	let batch_size = config.index.contextual_batch_size;

	for chunk_start in (0..blocks.len()).step_by(batch_size) {
		let chunk_end = (chunk_start + batch_size).min(blocks.len());
		let batch = &blocks[chunk_start..chunk_end];

		// LLM call includes retry with exponential backoff (in LlmClient).
		// If it still fails after retries, propagate error to stop indexing.
		let batch_descriptions =
			generate_descriptions_batch(&client, batch, chunk_start, file_context, &siblings)
				.await
				.map_err(|e| {
					anyhow::anyhow!(
						"Contextual description batch failed for chunks {}-{}: {}. \
						 Stopping indexing to prevent storing data without LLM descriptions.",
						chunk_start,
						chunk_end - 1,
						e
					)
				})?;
		descriptions.extend(batch_descriptions);
	}

	Ok(descriptions)
}

/// Build a map of file path -> all symbols from blocks in this batch.
/// Used to provide "sibling" context (other functions in the same file).
fn build_siblings_map(blocks: &[CodeBlock]) -> HashMap<String, Vec<String>> {
	let mut siblings: HashMap<String, Vec<String>> = HashMap::new();
	for block in blocks {
		let entry = siblings.entry(block.path.clone()).or_default();
		for symbol in &block.symbols {
			if !entry.contains(symbol) {
				entry.push(symbol.clone());
			}
		}
	}
	siblings
}

/// Generate descriptions for a single sub-batch of code blocks.
async fn generate_descriptions_batch(
	client: &LlmClient,
	batch: &[CodeBlock],
	global_offset: usize,
	file_context: &FileContextMap,
	siblings: &HashMap<String, Vec<String>>,
) -> Result<HashMap<usize, String>> {
	let mut descriptions = HashMap::new();
	let mut prompt = String::new();

	for (i, block) in batch.iter().enumerate() {
		let chunk_num = i + 1;

		// Get file-level context for this block's file
		let file_ctx = file_context.get(&block.path);

		// Build imports line
		let imports_line = if let Some(ctx) = file_ctx {
			if !ctx.imports.is_empty() {
				format!("Imports: {}\n", ctx.imports.join(", "))
			} else {
				String::new()
			}
		} else {
			String::new()
		};

		// Build siblings line (other functions in same file, excluding current block's symbols)
		let siblings_line = if let Some(all_syms) = file_ctx
			.map(|c| &c.all_symbols)
			.or(siblings.get(&block.path))
		{
			let other_syms: Vec<&str> = all_syms
				.iter()
				.filter(|s| !block.symbols.contains(s))
				.map(|s| s.as_str())
				.collect();
			if !other_syms.is_empty() {
				format!("Also in file: {}\n", other_syms.join(", "))
			} else {
				String::new()
			}
		} else {
			String::new()
		};

		prompt.push_str(&format!(
			"=== CHUNK {} ===\nFile: {}\nLanguage: {}\n{}{}Symbols: {}\nCode:\n{}\n\n",
			chunk_num,
			block.path,
			block.language,
			imports_line,
			siblings_line,
			if block.symbols.is_empty() {
				"(none)".to_string()
			} else {
				block.symbols.join(", ")
			},
			truncate_code_for_context(&block.content, 1500),
		));
	}

	prompt.push_str(&format!(
        "Write a description for each of the {} chunks above.\n\n\
         Rules:\n\
         - Each description: 1-2 sentences, specific to the code's purpose\n\
         - Use search-friendly terms (e.g. \"JWT authentication\", \"database connection pooling\")\n\
         - Mention the domain/context when imports or sibling functions reveal it\n\n\
         Output format: a JSON object where keys are chunk numbers as strings, values are descriptions.\n\
         Output ONLY the JSON object. Do not wrap in code fences. Start with {{ and end with }}.\n\n\
         Example output:\n\
         {{\"1\": \"Validates JWT bearer tokens for API authentication middleware\", \
         \"2\": \"Initializes database connection pool with retry and timeout configuration\"}}",
        batch.len()
    ));

	let messages = vec![
		Message::system(CONTEXTUAL_SYSTEM_PROMPT),
		Message::user(&prompt),
	];

	let json = client.chat_completion_json(messages).await?;

	if let Some(obj) = json.as_object() {
		for (i, _block) in batch.iter().enumerate() {
			let chunk_key = format!("{}", i + 1);
			if let Some(desc) = obj.get(&chunk_key).and_then(|v| v.as_str()) {
				let trimmed = if desc.len() > 300 {
					format!("{}...", &desc[..297])
				} else {
					desc.to_string()
				};
				descriptions.insert(global_offset + i, trimmed);
			}
		}
	}

	Ok(descriptions)
}

/// Truncate code content for LLM context to avoid excessive token usage
fn truncate_code_for_context(content: &str, max_chars: usize) -> &str {
	if content.len() <= max_chars {
		content
	} else {
		// Find a safe UTF-8 boundary
		let mut end = max_chars;
		while end > 0 && !content.is_char_boundary(end) {
			end -= 1;
		}
		&content[..end]
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_build_structural_context() {
		let block = CodeBlock {
			path: "src/auth/middleware.rs".to_string(),
			language: "rust".to_string(),
			content: "fn verify_token() {}".to_string(),
			symbols: vec!["verify_token".to_string()],
			start_line: 1,
			end_line: 1,
			hash: "test".to_string(),
			distance: None,
		};

		let context = build_structural_context(&block);
		assert!(context.contains("# File: src/auth/middleware.rs"));
		assert!(context.contains("# Language: rust"));
		assert!(context.contains("# Defines: verify_token"));
	}

	#[test]
	fn test_build_structural_context_no_symbols() {
		let block = CodeBlock {
			path: "src/utils.rs".to_string(),
			language: "rust".to_string(),
			content: "const VERSION: &str = \"1.0\";".to_string(),
			symbols: vec![],
			start_line: 1,
			end_line: 1,
			hash: "test".to_string(),
			distance: None,
		};

		let context = build_structural_context(&block);
		assert!(context.contains("# File: src/utils.rs"));
		assert!(!context.contains("# Defines:"));
	}

	#[test]
	fn test_build_enriched_embedding_input_with_description() {
		let block = CodeBlock {
			path: "src/auth/jwt.rs".to_string(),
			language: "rust".to_string(),
			content: "fn decode_token(t: &str) -> Claims { }".to_string(),
			symbols: vec!["decode_token".to_string()],
			start_line: 10,
			end_line: 12,
			hash: "test".to_string(),
			distance: None,
		};

		let result = build_enriched_embedding_input(
			&block,
			Some("Decodes and validates a JWT token, extracting user claims"),
		);

		// Description comes first
		assert!(result.starts_with("Decodes and validates"));
		// Then structural context
		assert!(result.contains("# File: src/auth/jwt.rs"));
		assert!(result.contains("# Defines: decode_token"));
		// Then code
		assert!(result.contains("fn decode_token"));
	}

	#[test]
	fn test_build_enriched_embedding_input_without_description() {
		let block = CodeBlock {
			path: "src/main.rs".to_string(),
			language: "rust".to_string(),
			content: "fn main() {}".to_string(),
			symbols: vec!["main".to_string()],
			start_line: 1,
			end_line: 1,
			hash: "test".to_string(),
			distance: None,
		};

		let result = build_enriched_embedding_input(&block, None);

		// Starts with structural context (no description)
		assert!(result.starts_with("# File: src/main.rs"));
		assert!(result.contains("fn main()"));
	}

	#[test]
	fn test_truncate_code_for_context() {
		let short = "fn foo() {}";
		assert_eq!(truncate_code_for_context(short, 1500), short);

		let long = "x".repeat(2000);
		assert_eq!(truncate_code_for_context(&long, 100).len(), 100);
	}

	#[test]
	fn test_build_siblings_map() {
		let blocks = vec![
			CodeBlock {
				path: "src/auth.rs".to_string(),
				language: "rust".to_string(),
				content: String::new(),
				symbols: vec!["verify".to_string(), "decode".to_string()],
				start_line: 1,
				end_line: 5,
				hash: "h1".to_string(),
				distance: None,
			},
			CodeBlock {
				path: "src/auth.rs".to_string(),
				language: "rust".to_string(),
				content: String::new(),
				symbols: vec!["refresh".to_string()],
				start_line: 10,
				end_line: 15,
				hash: "h2".to_string(),
				distance: None,
			},
			CodeBlock {
				path: "src/db.rs".to_string(),
				language: "rust".to_string(),
				content: String::new(),
				symbols: vec!["connect".to_string()],
				start_line: 1,
				end_line: 3,
				hash: "h3".to_string(),
				distance: None,
			},
		];

		let siblings = build_siblings_map(&blocks);
		assert_eq!(siblings.get("src/auth.rs").unwrap().len(), 3);
		assert!(siblings
			.get("src/auth.rs")
			.unwrap()
			.contains(&"verify".to_string()));
		assert!(siblings
			.get("src/auth.rs")
			.unwrap()
			.contains(&"refresh".to_string()));
		assert_eq!(siblings.get("src/db.rs").unwrap().len(), 1);
	}
}
