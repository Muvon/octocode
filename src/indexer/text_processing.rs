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

/// Text chunk with line information
#[derive(Debug, Clone)]
pub struct TextChunkWithLines {
	pub content: String,
	pub start_line: usize,
	pub end_line: usize,
}

/// Text processing utilities for chunking and analysis
pub struct TextProcessor;

impl TextProcessor {
	/// Chunk text into overlapping segments with line tracking
	pub fn chunk_text(content: &str, chunk_size: usize, overlap: usize) -> Vec<TextChunkWithLines> {
		let mut chunks = Vec::new();
		let lines: Vec<&str> = content.lines().collect();

		if lines.is_empty() {
			return chunks;
		}

		let mut start_idx = 0;
		let mut current_line = 1;

		while start_idx < lines.len() {
			let mut end_idx = std::cmp::min(start_idx + chunk_size, lines.len());
			let mut current_content = String::new();
			let mut char_count = 0;

			// Build chunk content and track character count
			for (idx, line) in lines
				.iter()
				.enumerate()
				.skip(start_idx)
				.take(end_idx - start_idx)
			{
				if char_count + line.len() + 1 > chunk_size && !current_content.is_empty() {
					end_idx = start_idx + idx;
					break;
				}
				if !current_content.is_empty() {
					current_content.push('\n');
					char_count += 1;
				}
				current_content.push_str(line);
				char_count += line.len();
			}

			if !current_content.is_empty() {
				let chunk = TextChunkWithLines {
					content: current_content,
					start_line: current_line,
					end_line: current_line + (end_idx - start_idx).saturating_sub(1),
				};
				chunks.push(chunk);
			}

			// Move to next chunk with overlap
			if end_idx >= lines.len() {
				break;
			}

			let next_start = if overlap > 0 && end_idx > overlap {
				end_idx - overlap
			} else {
				end_idx
			};

			current_line += next_start - start_idx;
			start_idx = next_start;
		}

		chunks
	}
}
