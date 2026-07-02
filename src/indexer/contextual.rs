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
/// Provides imports, exports, and sibling symbols used to enrich the LLM
/// description prompt. Full-file content was trialed (Anthropic Contextual
/// Retrieval, Sept 2024) but measured neutral on our CSN Ruby + synx evals
/// and burned ~16× more LLM tokens, so it's not stored here. Exports stay —
/// they're cheap, already parsed, and help the LLM describe a chunk's role
/// in the larger module.
#[derive(Debug, Clone, Default)]
pub struct FileContext {
	pub imports: Vec<String>,
	pub exports: Vec<String>,
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

/// Strip the embedding-input preamble (description, structural context,
/// blank separator) produced by `build_enriched_embedding_input` and
/// `process_*_blocks_batch` from a stored `content` value. Used at display
/// time so search results show raw code with correct line numbers while
/// the on-disk column keeps the enriched text for BM25/reranker alignment.
///
/// The preamble always contains a `# File: ` header line followed by zero
/// or more `# Language:` / `# Defines:` / context lines and ends with a
/// blank separator before the original content. If no such header is
/// detected, the input is returned unchanged so legacy/raw rows still
/// render correctly.
pub fn strip_enriched_preamble(content: &str) -> &str {
	let file_pos = if content.starts_with("# File: ") {
		Some(0)
	} else {
		content.find("\n# File: ").map(|p| p + 1)
	};
	let Some(file_pos) = file_pos else {
		return content;
	};

	let tail = &content[file_pos..];
	if let Some(blank_pos) = tail.find("\n\n") {
		let abs = file_pos + blank_pos + 2;
		return content.get(abs..).unwrap_or("");
	}
	content
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
///
/// Batches up to `contextual_batch_size` chunks per LLM call, mixing across
/// files. Each chunk in the prompt carries its file path, language, imports,
/// exports, sibling symbols, and own symbols — cheap structural context that
/// helps the LLM situate the chunk without us paying for full-file content.
///
/// We trialed Anthropic-style full-document context (Sept 2024 blog) and it
/// measured neutral on CSN Ruby and synx while burning ~16× more LLM tokens,
/// so we dropped it. Exports stayed because they're free at parse time and
/// give the LLM a hint about the module's role in the codebase.
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

	let batch_size = config.index.contextual_batch_size.max(1);

	let indexed: Vec<(usize, &CodeBlock)> = blocks.iter().enumerate().collect();
	for sub in indexed.chunks(batch_size) {
		let batch_descriptions = generate_descriptions_batch(&client, sub, file_context, &siblings)
			.await
			.map_err(|e| {
				anyhow::anyhow!(
					"Contextual description batch failed for {} chunks: {}. \
					 Stopping indexing to prevent storing data without LLM descriptions.",
					sub.len(),
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

/// Generate descriptions for a cross-file batch of code blocks.
///
/// Each chunk in the prompt carries its own `Imports / Exports / Also in file`
/// header so the LLM can describe its purpose without us needing to attach the
/// whole file body. Exports are the structural piece kept from the trialed
/// Anthropic full-file path — they're cheap (already parsed) and tell the LLM
/// what API surface this file contributes to the module.
async fn generate_descriptions_batch(
	client: &LlmClient,
	batch: &[(usize, &CodeBlock)],
	file_context: &FileContextMap,
	siblings: &HashMap<String, Vec<String>>,
) -> Result<HashMap<usize, String>> {
	let mut descriptions = HashMap::new();
	if batch.is_empty() {
		return Ok(descriptions);
	}

	let mut prompt = String::new();
	prompt.push_str(&format!(
		"Write a description for each of the {} code chunks below. Each chunk includes its file's \
		 imports/exports/sibling-symbols header. Output 1-2 sentences per chunk using \
		 search-friendly terms.\n\n",
		batch.len()
	));

	for (i, (_global_idx, block)) in batch.iter().enumerate() {
		let chunk_num = i + 1;
		let file_ctx = file_context.get(&block.path);
		let imports_line = match file_ctx {
			Some(c) if !c.imports.is_empty() => format!("Imports: {}\n", c.imports.join(", ")),
			_ => String::new(),
		};
		let exports_line = match file_ctx {
			Some(c) if !c.exports.is_empty() => format!("Exports: {}\n", c.exports.join(", ")),
			_ => String::new(),
		};
		let other_syms: Vec<&str> = match file_ctx
			.map(|c| &c.all_symbols)
			.or(siblings.get(&block.path))
		{
			Some(syms) => syms
				.iter()
				.filter(|s| !block.symbols.contains(s))
				.map(|s| s.as_str())
				.collect(),
			None => Vec::new(),
		};
		let siblings_line = if other_syms.is_empty() {
			String::new()
		} else {
			format!("Also in file: {}\n", other_syms.join(", "))
		};
		let sym_line = if block.symbols.is_empty() {
			"(no symbols)".to_string()
		} else {
			block.symbols.join(", ")
		};
		prompt.push_str(&format!(
			"=== CHUNK {} ===\nFile: {}\nLanguage: {}\n{}{}{}Symbols: {}\nCode:\n<chunk>\n{}\n</chunk>\n\n",
			chunk_num,
			block.path,
			block.language,
			imports_line,
			exports_line,
			siblings_line,
			sym_line,
			truncate_code_for_context(&block.content, 1500),
		));
	}

	prompt.push_str(&format!(
		"Output format: a JSON object where keys are chunk numbers as strings, values are descriptions.\n\
		 Output ONLY the JSON object. Do not wrap in code fences. Start with {{ and end with }}.\n\n\
		 Example output for 2 chunks:\n\
		 {{\"1\": \"Validates JWT bearer tokens for API authentication middleware\", \
		 \"2\": \"Initializes database connection pool with retry and timeout configuration\"}}\n\n\
		 Now produce the JSON for the {} chunks above.",
		batch.len()
	));

	let messages = vec![
		Message::system(CONTEXTUAL_SYSTEM_PROMPT),
		Message::user(&prompt),
	];

	let json = client.chat_completion_json(messages, None).await?;

	if let Some(obj) = json.as_object() {
		for (i, (global_idx, _block)) in batch.iter().enumerate() {
			let chunk_key = format!("{}", i + 1);
			if let Some(desc) = obj.get(&chunk_key).and_then(|v| v.as_str()) {
				let trimmed = if desc.len() > 300 {
					let mut end = 297;
					while end > 0 && !desc.is_char_boundary(end) {
						end -= 1;
					}
					format!("{}...", &desc[..end])
				} else {
					desc.to_string()
				};
				descriptions.insert(*global_idx, trimmed);
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
	fn test_strip_enriched_preamble_code_with_description() {
		let block = CodeBlock {
			path: "src/auth.rs".to_string(),
			language: "rust".to_string(),
			content: "fn verify() {}".to_string(),
			symbols: vec!["verify".to_string()],
			start_line: 10,
			end_line: 10,
			hash: "h".to_string(),
			distance: None,
		};
		let enriched = build_enriched_embedding_input(&block, Some("Verifies a JWT token"));
		let stripped = strip_enriched_preamble(&enriched);
		assert_eq!(stripped, "fn verify() {}");
	}

	#[test]
	fn test_strip_enriched_preamble_code_no_description() {
		let block = CodeBlock {
			path: "src/main.rs".to_string(),
			language: "rust".to_string(),
			content: "fn main() {\n    println!(\"hi\");\n}".to_string(),
			symbols: vec!["main".to_string()],
			start_line: 1,
			end_line: 3,
			hash: "h".to_string(),
			distance: None,
		};
		let enriched = build_enriched_embedding_input(&block, None);
		let stripped = strip_enriched_preamble(&enriched);
		assert_eq!(stripped, "fn main() {\n    println!(\"hi\");\n}");
	}

	#[test]
	fn test_strip_enriched_preamble_text_block() {
		// Matches the format process_text_blocks_batch writes.
		let enriched = "# File: README.md\n\nThis is content.\nSecond line.";
		assert_eq!(
			strip_enriched_preamble(enriched),
			"This is content.\nSecond line."
		);
	}

	#[test]
	fn test_strip_enriched_preamble_doc_with_context() {
		// Matches process_document_blocks_batch when context is non-empty.
		let enriched = "# File: spec.md\nIntroduction > Overview\n\n# Heading\n\nBody.";
		assert_eq!(strip_enriched_preamble(enriched), "# Heading\n\nBody.");
	}

	#[test]
	fn test_strip_enriched_preamble_raw_content_passthrough() {
		// Legacy rows without preamble must round-trip unchanged.
		let raw = "fn foo() {}\nfn bar() {}";
		assert_eq!(strip_enriched_preamble(raw), raw);
	}

	#[test]
	fn test_strip_enriched_preamble_code_with_python_hash_comment() {
		// First substantive line of stored code can start with `#` (Python comment).
		// Strip must use the structural `# File:` marker, not blanket `#` lines.
		let enriched =
			"# File: app.py\n# Language: python\n\n# TODO: refactor\ndef main():\n    pass";
		assert_eq!(
			strip_enriched_preamble(enriched),
			"# TODO: refactor\ndef main():\n    pass"
		);
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
