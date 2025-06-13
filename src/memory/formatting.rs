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

// Shared memory formatting functions for CLI and MCP

use crate::memory::MemorySearchResult;

/// Format memory search results as text (token-efficient, for MCP)
pub fn format_memories_as_text(results: &[MemorySearchResult]) -> String {
	if results.is_empty() {
		return "No stored memories match your query. Try using different search terms, removing filters, or checking if any memories have been stored yet.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!("MEMORIES ({} found)\n\n", results.len()));

	for (i, result) in results.iter().enumerate() {
		output.push_str(&format!(
			"{}. {} | Score: {:.2}\n",
			i + 1,
			result.memory.title,
			result.relevance_score
		));

		output.push_str(&format!(
			"Type: {} | Importance: {:.1} | Created: {}\n",
			result.memory.memory_type,
			result.memory.metadata.importance,
			result.memory.created_at.format("%Y-%m-%d %H:%M:%S UTC")
		));

		if !result.memory.metadata.tags.is_empty() {
			output.push_str(&format!(
				"Tags: {}\n",
				result.memory.metadata.tags.join(", ")
			));
		}

		if !result.memory.metadata.related_files.is_empty() {
			output.push_str(&format!(
				"Files: {}\n",
				result.memory.metadata.related_files.join(", ")
			));
		}

		if let Some(git_commit) = &result.memory.metadata.git_commit {
			output.push_str(&format!("Git: {}\n", git_commit));
		}

		output.push_str(&format!("ID: {}\n", result.memory.id));

		// Add content as-is without any modification
		output.push_str(&result.memory.content);
		if !result.memory.content.ends_with('\n') {
			output.push('\n');
		}

		output.push_str(&format!("Why: {}\n\n", result.selection_reason));
	}

	output
}

/// Format memory search results as markdown
pub fn format_memories_as_markdown(results: &[MemorySearchResult]) -> String {
	if results.is_empty() {
		return "No stored memories match your query. Try using different search terms, removing filters, or checking if any memories have been stored yet.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!("# Memories ({} found)\n\n", results.len()));

	for (i, result) in results.iter().enumerate() {
		output.push_str(&format!(
			"## {}. {} (Score: {:.2})\n\n",
			i + 1,
			result.memory.title,
			result.relevance_score
		));

		output.push_str(&format!(
			"**Type:** {} | **Importance:** {:.1} | **Created:** {}\n\n",
			result.memory.memory_type,
			result.memory.metadata.importance,
			result.memory.created_at.format("%Y-%m-%d %H:%M:%S UTC")
		));

		if !result.memory.metadata.tags.is_empty() {
			output.push_str(&format!(
				"**Tags:** {}\n\n",
				result.memory.metadata.tags.join(", ")
			));
		}

		if !result.memory.metadata.related_files.is_empty() {
			output.push_str(&format!(
				"**Files:** {}\n\n",
				result.memory.metadata.related_files.join(", ")
			));
		}

		if let Some(git_commit) = &result.memory.metadata.git_commit {
			output.push_str(&format!("**Git:** {}\n\n", git_commit));
		}

		output.push_str(&format!("**ID:** {}\n\n", result.memory.id));

		// Add content as-is without any modification
		output.push_str("**Content:**\n\n");
		output.push_str(&result.memory.content);
		if !result.memory.content.ends_with('\n') {
			output.push('\n');
		}
		output.push('\n');

		output.push_str(&format!("**Why:** {}\n\n---\n\n", result.selection_reason));
	}

	output
}

/// Format plain Memory objects for CLI (used by recent, by-type, etc.)
pub fn format_plain_memories_for_cli(memories: &[crate::memory::Memory], format: &str) {
	match format {
		"json" => {
			println!("{}", serde_json::to_string_pretty(memories).unwrap());
		}
		"text" => {
			// Convert to search results format for consistent text formatting
			let fake_results: Vec<MemorySearchResult> = memories
				.iter()
				.map(|m| MemorySearchResult {
					memory: m.clone(),
					relevance_score: 1.0, // No relevance score for plain memories
					selection_reason: "Listed by query".to_string(),
				})
				.collect();
			print!("{}", format_memories_as_text(&fake_results));
		}
		"md" | "markdown" => {
			// Convert to search results format for consistent markdown formatting
			let fake_results: Vec<MemorySearchResult> = memories
				.iter()
				.map(|m| MemorySearchResult {
					memory: m.clone(),
					relevance_score: 1.0, // No relevance score for plain memories
					selection_reason: "Listed by query".to_string(),
				})
				.collect();
			print!("{}", format_memories_as_markdown(&fake_results));
		}
		"compact" => {
			println!("🧠 {} memories:", memories.len());
			for memory in memories {
				println!(
					"- [{}] {} - {}",
					memory.memory_type, memory.title, memory.id
				);
			}
		}
		_ => {
			println!("🧠 {} memories:\n", memories.len());
			for memory in memories {
				println!("Memory ID: {}", memory.id);
				println!("Title: {}", memory.title);
				println!("Type: {}", memory.memory_type);
				println!("Importance: {:.2}", memory.metadata.importance);
				println!("Created: {}", memory.created_at.format("%Y-%m-%d %H:%M:%S"));
				if !memory.metadata.tags.is_empty() {
					println!("Tags: {}", memory.metadata.tags.join(", "));
				}
				println!("Content: {}", memory.content);
				println!();
			}
		}
	}
}

/// Format memory search results for CLI (with emojis and formatting)
pub fn format_memories_for_cli(results: &[MemorySearchResult], format: &str) {
	match format {
		"json" => {
			println!("{}", serde_json::to_string_pretty(results).unwrap());
		}
		"text" => {
			// Use token-efficient text format
			print!("{}", format_memories_as_text(results));
		}
		"md" | "markdown" => {
			// Use markdown format
			print!("{}", format_memories_as_markdown(results));
		}
		"compact" => {
			println!("🧠 {} memories:", results.len());
			for result in results {
				println!(
					"- [{}] {} (Score: {:.2}) - {}",
					result.memory.memory_type,
					result.memory.title,
					result.relevance_score,
					result.memory.id
				);
			}
		}
		_ => {
			println!("🧠 {} memories:\n", results.len());
			for result in results {
				println!("Memory ID: {}", result.memory.id);
				println!("Title: {}", result.memory.title);
				println!("Type: {}", result.memory.memory_type);
				println!("Relevance: {:.2}", result.relevance_score);
				println!("Importance: {:.2}", result.memory.metadata.importance);
				println!(
					"Created: {}",
					result.memory.created_at.format("%Y-%m-%d %H:%M:%S")
				);
				if !result.memory.metadata.tags.is_empty() {
					println!("Tags: {}", result.memory.metadata.tags.join(", "));
				}
				println!("Content: {}", result.memory.content);
				println!("Why selected: {}", result.selection_reason);
				println!();
			}
		}
	}
}
