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
			sanitize_content(&result.memory.title),
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

		// Add content as-is without truncation for text mode
		let content = sanitize_content(&result.memory.content);
		output.push_str(&content);
		if !content.ends_with('\n') {
			output.push('\n');
		}

		output.push_str(&format!(
			"Why: {}\n\n",
			sanitize_content(&result.selection_reason)
		));
	}

	output
}

/// Format memory search results for CLI (with emojis and formatting)
pub fn format_memories_for_cli(results: &[MemorySearchResult], format: &str) {
	match format {
		"json" => {
			println!("{}", serde_json::to_string_pretty(results).unwrap());
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

/// Sanitize content by removing control characters while preserving safe Unicode
pub fn sanitize_content(content: &str) -> String {
	content
		.chars()
		.filter(|&c| !c.is_control() && (c.is_ascii_graphic() || is_safe_unicode(c)))
		.collect()
}

/// Check if a character is a safe Unicode character (including emojis)
pub fn is_safe_unicode(c: char) -> bool {
	// Allow a broader range of Unicode characters, including emojis
	let code = c as u32;
	// Emoji ranges and other safe Unicode ranges
	matches!(code,
		// Emoji ranges
		0x1F600..=0x1F64F | // Emoticons
		0x1F300..=0x1F5FF | // Misc Symbols and Pictographs
		0x1F680..=0x1F6FF | // Transport and Map
		0x1F1E6..=0x1F1FF | // Regional indicators
		0x2600..=0x26FF   | // Misc symbols
		0x2700..=0x27BF   | // Dingbats

		// Allow variation selectors and zero-width joiner
		0xFE0F | 0x200D    |

		// Some additional safe Unicode ranges
		0x0080..=0x00FF   | // Latin-1 Supplement
		0x0100..=0x017F   | // Latin Extended-A
		0x0180..=0x024F   | // Latin Extended-B
		0x0370..=0x03FF   | // Greek and Coptic
		0x0400..=0x04FF   | // Cyrillic
		0x0530..=0x058F   | // Armenian
		0x0590..=0x05FF   | // Hebrew
		0x0600..=0x06FF   | // Arabic
		0x0900..=0x097F   | // Devanagari
		0x4E00..=0x9FFF     // CJK Unified Ideographs
	)
}
