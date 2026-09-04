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

#[cfg(test)]
mod tests {
	use crate::indexer::text_processing::TextProcessor;

	#[test]
	fn empty_input_yields_no_chunks() {
		assert!(TextProcessor::chunk_text("", 100, 10).is_empty());
		assert!(TextProcessor::chunk_text("\n\n\n", 100, 10).is_empty());
	}

	#[test]
	fn single_chunk_covers_short_content() {
		let chunks = TextProcessor::chunk_text("alpha\nbeta\ngamma", 1000, 0);
		assert_eq!(chunks.len(), 1);
		assert_eq!(chunks[0].content, "alpha\nbeta\ngamma");
		assert_eq!(chunks[0].start_line, 1);
		assert_eq!(chunks[0].end_line, 3);
	}

	#[test]
	fn chunks_cover_every_line_without_gaps() {
		let content: String = (1..=50)
			.map(|i| format!("line {i}"))
			.collect::<Vec<_>>()
			.join("\n");
		let chunks = TextProcessor::chunk_text(&content, 10, 2);
		assert!(!chunks.is_empty());
		// Every source line must appear in at least one chunk.
		for i in 1..=50 {
			let needle = format!("line {i}");
			assert!(
				chunks
					.iter()
					.any(|c| c.content.lines().any(|l| l == needle)),
				"line {i} missing from all chunks"
			);
		}
	}

	#[test]
	fn line_numbers_do_not_drift_with_overlap() {
		let content: String = (1..=30)
			.map(|i| format!("l{i}"))
			.collect::<Vec<_>>()
			.join("\n");
		let chunks = TextProcessor::chunk_text(&content, 5, 2);
		for chunk in &chunks {
			let first = chunk.content.lines().next().unwrap();
			assert_eq!(
				first,
				format!("l{}", chunk.start_line),
				"start_line {} does not match first content line {first}",
				chunk.start_line
			);
			assert!(chunk.end_line >= chunk.start_line);
			assert!(chunk.end_line <= 30);
		}
	}

	#[test]
	fn overlap_larger_than_chunk_still_makes_forward_progress() {
		let content: String = (1..=20)
			.map(|i| format!("row{i}"))
			.collect::<Vec<_>>()
			.join("\n");
		// overlap >= chunk_size would otherwise stall the cursor forever.
		let chunks = TextProcessor::chunk_text(&content, 3, 10);
		assert!(!chunks.is_empty());
		assert!(chunks.len() < 100, "chunker failed to terminate promptly");
		let mut prev = 0;
		for chunk in &chunks {
			assert!(chunk.start_line > prev, "start_line did not advance");
			prev = chunk.start_line;
		}
	}

	#[test]
	fn oversized_single_line_is_still_emitted() {
		let long = "x".repeat(500);
		let chunks = TextProcessor::chunk_text(&long, 10, 0);
		assert_eq!(chunks.len(), 1);
		assert_eq!(chunks[0].content, long);
		assert_eq!(chunks[0].start_line, 1);
	}

	#[test]
	fn chunk_size_caps_character_count_when_lines_are_short() {
		let content: String = (0..40).map(|_| "abcd").collect::<Vec<_>>().join("\n");
		let chunks = TextProcessor::chunk_text(&content, 20, 0);
		assert!(chunks.len() > 1);
		for chunk in &chunks {
			assert!(
				chunk.content.len() <= 20,
				"chunk exceeded budget: {} bytes",
				chunk.content.len()
			);
		}
	}
}
