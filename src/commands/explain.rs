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

use clap::Args;
use std::path::Path;

use octocode::config::Config;
use octocode::indexer;
use octocode::llm::{LlmClient, Message};
use octocode::store::Store;

use crate::commands::OutputFormat;

#[derive(Debug, Args)]
pub struct ExplainArgs {
	/// Target to explain: file path, file:symbol, or search query
	#[arg(required = true)]
	pub target: String,

	/// Output format: 'cli', 'json', 'text', or 'md'
	#[arg(short, long, default_value = "cli")]
	pub format: OutputFormat,
}

const EXPLAIN_SYSTEM_PROMPT: &str = "\
You are a senior developer explaining code to a teammate who hasn't seen this codebase before.
Be concise and architectural — explain WHAT it does, WHY it exists, and HOW it fits in the system.
Do NOT explain line by line. Focus on purpose, design decisions, and relationships.
If you're uncertain about something, say so explicitly.";

pub async fn execute(
	store: &Store,
	args: &ExplainArgs,
	config: &Config,
) -> Result<(), anyhow::Error> {
	let current_dir = std::env::current_dir()?;

	// Step 1: Resolve target to code content + context
	let (target_label, code_content, context) =
		resolve_target(store, &args.target, &current_dir, config).await?;

	// Step 2: Build LLM prompt
	let mut user_prompt = format!("Target: {}\n\n", target_label);

	user_prompt.push_str("Code:\n```\n");
	// Truncate code to avoid token overflow
	if code_content.len() > 6000 {
		user_prompt.push_str(&code_content[..6000]);
		user_prompt.push_str("\n... (truncated)\n");
	} else {
		user_prompt.push_str(&code_content);
	}
	user_prompt.push_str("\n```\n\n");

	if !context.is_empty() {
		user_prompt.push_str("Codebase context:\n");
		user_prompt.push_str(&context);
		user_prompt.push('\n');
	}

	user_prompt.push_str(
		"Explain:\n\
		 1. What this code does (1-2 sentences)\n\
		 2. Why it exists — what problem it solves\n\
		 3. How it fits in the codebase — dependencies and consumers\n\
		 4. Key design decisions or patterns",
	);

	// Step 3: Call LLM
	let client = LlmClient::from_config(config)?;
	let messages = vec![
		Message::system(EXPLAIN_SYSTEM_PROMPT),
		Message::user(&user_prompt),
	];

	let explanation = client.chat_completion(messages).await?;

	// Step 4: Output
	if args.format.is_json() {
		let json = serde_json::json!({
			"target": target_label,
			"explanation": explanation,
		});
		println!("{}", serde_json::to_string_pretty(&json)?);
	} else {
		println!("{}\n", target_label);
		println!("{}", explanation);
	}

	Ok(())
}

/// Resolve the target string to (label, code_content, context_from_index)
async fn resolve_target(
	store: &Store,
	target: &str,
	current_dir: &Path,
	config: &Config,
) -> Result<(String, String, String), anyhow::Error> {
	// Case 1: file:symbol syntax
	if let Some((file_part, symbol)) = target.split_once(':') {
		let file_path = current_dir.join(file_part);
		if file_path.exists() {
			let content = std::fs::read_to_string(&file_path)?;
			let label = format!("{}:{}", file_part, symbol);

			// Try to find the symbol in the file
			let symbol_content = extract_symbol_from_content(&content, symbol);
			let code = symbol_content.unwrap_or(content.clone());

			// Get context from search
			let context = gather_context(store, file_part, Some(symbol), config).await;

			return Ok((label, code, context));
		}
	}

	// Case 2: file path exists on disk
	let file_path = current_dir.join(target);
	if file_path.exists() {
		let content = std::fs::read_to_string(&file_path)?;
		let label = target.to_string();
		let context = gather_context(store, target, None, config).await;
		return Ok((label, content, context));
	}

	// Case 3: treat as search query
	let search_result = indexer::search::search_codebase_with_details_text(
		target,
		"code",
		"full",
		3,
		config.search.similarity_threshold,
		None,
		config,
	)
	.await?;

	if search_result.trim().is_empty() || search_result.contains("No results") {
		return Err(anyhow::anyhow!(
			"No code found for '{}'. Try a file path or different query.",
			target
		));
	}

	Ok((format!("Query: {}", target), search_result, String::new()))
}

/// Try to extract a specific symbol's code from file content
fn extract_symbol_from_content(content: &str, symbol: &str) -> Option<String> {
	// Simple heuristic: find line containing the symbol name as a definition
	let lines: Vec<&str> = content.lines().collect();
	for (i, line) in lines.iter().enumerate() {
		if line.contains(symbol)
			&& (line.contains("fn ")
				|| line.contains("struct ")
				|| line.contains("enum ")
				|| line.contains("impl ")
				|| line.contains("class ")
				|| line.contains("def ")
				|| line.contains("function ")
				|| line.contains("const ")
				|| line.contains("pub ")
				|| line.contains("export "))
		{
			// Take from this line to the next blank line or end of scope
			let start = i;
			let mut end = (i + 50).min(lines.len()); // max 50 lines
			let mut brace_depth = 0i32;
			for (j, line) in lines[i..].iter().enumerate() {
				brace_depth += line.chars().filter(|c| *c == '{').count() as i32;
				brace_depth -= line.chars().filter(|c| *c == '}').count() as i32;
				if j > 0 && brace_depth <= 0 {
					end = i + j + 1;
					break;
				}
			}
			return Some(lines[start..end].join("\n"));
		}
	}
	None
}

/// Gather context from the index for a file/symbol
async fn gather_context(
	store: &Store,
	file_path: &str,
	symbol: Option<&str>,
	config: &Config,
) -> String {
	let mut context = String::new();

	// Try semantic search for related code
	let query = if let Some(sym) = symbol {
		format!("{} {}", file_path, sym)
	} else {
		file_path.to_string()
	};

	if let Ok(related) = indexer::search::search_codebase_with_details_text(
		&query,
		"code",
		"signatures",
		5,
		0.3, // low threshold to find related code
		None,
		config,
	)
	.await
	{
		if !related.trim().is_empty() && !related.contains("No results") {
			context.push_str("Related code:\n");
			// Truncate to avoid overwhelming the LLM
			if related.len() > 2000 {
				context.push_str(&related[..2000]);
				context.push_str("\n...(truncated)\n");
			} else {
				context.push_str(&related);
			}
		}
	}

	// Try GraphRAG relationships if available
	if let Ok(node_rels) = store
		.get_node_relationships(
			file_path,
			octocode::indexer::graphrag::types::RelationshipDirection::Both,
		)
		.await
	{
		if !node_rels.is_empty() {
			context.push_str("\nRelationships:\n");
			for rel in node_rels.iter().take(10) {
				context.push_str(&format!(
					"  {} --[{}]--> {}\n",
					rel.source, rel.relation_type, rel.target
				));
			}
		}
	}

	context
}
