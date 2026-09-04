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
			crate::utils::truncate_at_char_boundary(&block.content, 1500),
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
					format!("{}...", crate::utils::truncate_at_char_boundary(desc, 297))
				} else {
					desc.to_string()
				};
				descriptions.insert(*global_idx, trimmed);
			}
		}
	}

	Ok(descriptions)
}

#[cfg(test)]
#[path = "contextual_tests.rs"]
mod contextual_tests;
