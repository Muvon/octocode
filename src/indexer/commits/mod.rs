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

use anyhow::Result;
use std::path::Path;

use crate::config::Config;
use crate::embedding;
use crate::indexer::git_utils::{CommitEntry, GitUtils};
use crate::llm::{LlmClient, Message};
use crate::state::SharedState;
use crate::store::{CommitBlock, Store};

const COMMIT_DESCRIPTION_SYSTEM_PROMPT: &str =
	"You are a senior software engineer. For each commit, write a concise 2-3 sentence summary \
	 of WHAT changed and WHY. Focus on the intent and impact, not individual line changes.";

const MAX_DIFF_CHARS: usize = 8000;

/// Index commits from the default branch into the store.
pub async fn index_commits(
	config: &Config,
	store: &Store,
	repo_path: &Path,
	state: SharedState,
	quiet: bool,
) -> Result<()> {
	// Detect default branch
	let branch = match GitUtils::get_default_branch(repo_path) {
		Ok(b) => b,
		Err(e) => {
			tracing::warn!("Could not detect default branch: {}", e);
			return Ok(());
		}
	};

	// Get last indexed commit
	let last_commit = store.get_commits_last_commit_hash().await?;

	// Get new commits since last indexed
	let commits = GitUtils::get_commit_log(repo_path, &branch, last_commit.as_deref())?;

	if commits.is_empty() {
		tracing::debug!("No new commits to index");
		return Ok(());
	}

	if !quiet {
		eprintln!(
			"📝 Indexing {} commits from branch '{}'...",
			commits.len(),
			branch
		);
	}

	{
		let mut state_guard = state.write();
		state_guard.status_message = format!("Indexing {} commits...", commits.len());
	}

	// Get changed files for each commit
	let mut commit_blocks: Vec<CommitBlock> = Vec::with_capacity(commits.len());

	// Generate AI descriptions if LLM enabled
	let descriptions = if config.commits.use_llm {
		generate_descriptions(config, repo_path, &commits, quiet).await?
	} else {
		std::collections::HashMap::new()
	};

	for entry in &commits {
		let files =
			GitUtils::get_changed_files_for_commit(repo_path, &entry.hash).unwrap_or_default();
		let files_json = serde_json::to_string(&files).unwrap_or_else(|_| "[]".to_string());

		let description = descriptions.get(&entry.hash).cloned().unwrap_or_default();

		// Compose searchable content
		let mut content = entry.message.clone();
		if !files.is_empty() {
			content.push_str("\n\nFiles: ");
			content.push_str(&files.join(", "));
		}
		if !description.is_empty() {
			content.push_str("\n\n");
			content.push_str(&description);
		}

		commit_blocks.push(CommitBlock {
			hash: entry.hash.clone(),
			author: entry.author.clone(),
			date: entry.date,
			message: entry.message.clone(),
			content,
			files: files_json,
			description,
			distance: None,
		});
	}

	// Batch embed using text model
	let texts: Vec<String> = commit_blocks.iter().map(|b| b.content.clone()).collect();

	let batch_size = config.index.embeddings_batch_size;
	for chunk_start in (0..texts.len()).step_by(batch_size) {
		let chunk_end = (chunk_start + batch_size).min(texts.len());
		let text_chunk = texts[chunk_start..chunk_end].to_vec();
		let block_chunk = &commit_blocks[chunk_start..chunk_end];

		let embeddings = embedding::generate_embeddings_batch(
			text_chunk,
			false, // text model, not code
			config,
			embedding::types::InputType::Document,
		)
		.await?;

		store.store_commit_blocks(block_chunk, &embeddings).await?;
	}

	// Save last indexed commit hash (newest = last in our reversed list)
	if let Some(latest) = commits.last() {
		store.store_commits_last_commit_hash(&latest.hash).await?;
	}

	if !quiet {
		eprintln!("✅ Indexed {} commits", commits.len());
	}

	{
		let mut state_guard = state.write();
		state_guard.status_message = String::new();
	}

	Ok(())
}

/// Generate AI descriptions for commits using LLM
async fn generate_descriptions(
	config: &Config,
	repo_path: &Path,
	commits: &[CommitEntry],
	quiet: bool,
) -> Result<std::collections::HashMap<String, String>> {
	let mut descriptions = std::collections::HashMap::new();

	let client = match LlmClient::from_config(config) {
		Ok(c) => c,
		Err(e) => {
			if !quiet {
				eprintln!("⚠️  LLM not available for commit descriptions: {}", e);
			}
			return Ok(descriptions);
		}
	};

	// Process in batches
	let batch_size = 5;
	for chunk in commits.chunks(batch_size) {
		let mut prompt = String::new();

		for (i, entry) in chunk.iter().enumerate() {
			let diff = GitUtils::get_commit_diff(repo_path, &entry.hash, MAX_DIFF_CHARS)
				.unwrap_or_default();

			prompt.push_str(&format!(
				"=== COMMIT {} ===\nHash: {}\nMessage: {}\nDiff:\n{}\n\n",
				i + 1,
				&entry.hash[..8.min(entry.hash.len())],
				entry.message,
				diff
			));
		}

		prompt.push_str(&format!(
			"Provide a JSON object with commit short hashes as keys and description strings as values \
			 for each of the {} commits above.",
			chunk.len()
		));

		let messages = vec![
			Message::system(COMMIT_DESCRIPTION_SYSTEM_PROMPT),
			Message::user(&prompt),
		];

		match client.chat_completion_json(messages, None).await {
			Ok(json) => {
				if let Some(obj) = json.as_object() {
					for entry in chunk {
						let short_hash = &entry.hash[..8.min(entry.hash.len())];
						if let Some(desc) = obj.get(short_hash).and_then(|v| v.as_str()) {
							let trimmed = if desc.len() > 300 {
								format!("{}...", &desc[..297])
							} else {
								desc.to_string()
							};
							descriptions.insert(entry.hash.clone(), trimmed);
						}
					}
				}
			}
			Err(e) => {
				if !quiet {
					eprintln!(
						"⚠️  AI description batch failed for {} commits: {}",
						chunk.len(),
						e
					);
				}
			}
		}
	}

	Ok(descriptions)
}

#[cfg(test)]
mod tests {
	use crate::indexer::git_utils::CommitEntry;
	use crate::store::CommitBlock;

	#[test]
	fn test_commit_block_content_composition() {
		let entry = CommitEntry {
			hash: "abc12345".to_string(),
			author: "Alice".to_string(),
			date: 1700000000,
			message: "feat: add retry logic".to_string(),
		};
		let files = vec!["src/client.rs".to_string(), "src/retry.rs".to_string()];
		let files_json = serde_json::to_string(&files).unwrap();
		let description = "Adds exponential backoff retry".to_string();

		let mut content = entry.message.clone();
		content.push_str("\n\nFiles: ");
		content.push_str(&files.join(", "));
		content.push_str("\n\n");
		content.push_str(&description);

		let block = CommitBlock {
			hash: entry.hash.clone(),
			author: entry.author.clone(),
			date: entry.date,
			message: entry.message.clone(),
			content: content.clone(),
			files: files_json,
			description,
			distance: None,
		};

		assert_eq!(block.hash, "abc12345");
		assert!(block.content.contains("feat: add retry logic"));
		assert!(block.content.contains("src/client.rs"));
		assert!(block.content.contains("Adds exponential backoff retry"));
	}

	#[test]
	fn test_commit_block_content_without_llm() {
		let message = "fix: resolve null pointer".to_string();
		let files = ["src/main.rs".to_string()];

		let mut content = message.clone();
		content.push_str("\n\nFiles: ");
		content.push_str(&files.join(", "));

		assert!(content.contains("fix: resolve null pointer"));
		assert!(content.contains("src/main.rs"));
		assert!(!content.contains("\n\n\n")); // no empty description section
	}

	#[test]
	fn test_commit_entry_parsing() {
		// Simulate git log line parsing
		let line = "abc12345deadbeef|Alice|1700000000|feat: add feature";
		let parts: Vec<&str> = line.splitn(4, '|').collect();

		assert_eq!(parts.len(), 4);
		assert_eq!(parts[0], "abc12345deadbeef");
		assert_eq!(parts[1], "Alice");
		assert_eq!(parts[2], "1700000000");
		assert_eq!(parts[3], "feat: add feature");
	}
}
