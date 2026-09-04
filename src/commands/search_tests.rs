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
	use super::super::*;
	use octocode::store::{DocumentBlock, TextBlock};

	fn text_block(path: &str, content: &str, distance: Option<f32>) -> TextBlock {
		TextBlock {
			path: path.to_string(),
			language: "text".to_string(),
			content: content.to_string(),
			start_line: 3,
			end_line: 9,
			hash: format!("t-{path}"),
			distance,
		}
	}

	fn document_block(path: &str, content: &str, distance: Option<f32>) -> DocumentBlock {
		DocumentBlock {
			path: path.to_string(),
			title: "Section".to_string(),
			content: content.to_string(),
			context: vec!["# Doc".to_string()],
			level: 2,
			start_line: 1,
			end_line: 8,
			hash: format!("d-{path}"),
			distance,
		}
	}

	fn lines(count: usize) -> String {
		(1..=count)
			.map(|i| format!("line {i}"))
			.collect::<Vec<_>>()
			.join("\n")
	}

	#[test]
	fn only_the_three_detail_levels_are_accepted() {
		for level in ["signatures", "partial", "full"] {
			assert_eq!(validate_detail_level(level).unwrap(), level);
		}
		let err = validate_detail_level("verbose").unwrap_err();
		assert!(err.contains("Invalid detail level 'verbose'"), "{err}");
	}

	#[test]
	fn at_least_one_query_is_required() {
		let err = validate_queries(&[]).unwrap_err().to_string();
		assert!(err.contains("At least one query"), "{err}");
	}

	#[test]
	fn queries_are_length_bounded() {
		let short = validate_queries(&["ab".to_string()])
			.unwrap_err()
			.to_string();
		assert!(short.contains("at least 3 characters"), "{short}");

		let long = validate_queries(&["x".repeat(501)])
			.unwrap_err()
			.to_string();
		assert!(long.contains("no more than 500 characters"), "{long}");

		validate_queries(&["a valid query".to_string()]).expect("a normal query is accepted");
	}

	#[test]
	fn too_many_queries_are_rejected() {
		let many: Vec<String> = (0..64).map(|i| format!("query number {i}")).collect();
		let err = validate_queries(&many).unwrap_err().to_string();
		assert!(err.contains("Maximum"), "{err}");
	}

	#[test]
	fn the_offending_query_index_is_reported() {
		let err = validate_queries(&["fine query".to_string(), "no".to_string()])
			.unwrap_err()
			.to_string();
		assert!(err.contains("Query 2"), "{err}");
	}

	#[test]
	fn text_blocks_render_at_every_detail_level() {
		let config = Config::default();
		render_text_blocks_with_config(&[], &config, "partial");

		let blocks = vec![
			text_block("notes.txt", &lines(3), Some(0.25)),
			text_block("notes.txt", &lines(30), None),
			text_block("other.txt", "single line", Some(0.5)),
		];
		for level in ["signatures", "partial", "full", "unknown"] {
			render_text_blocks_with_config(&blocks, &config, level);
		}
	}

	#[test]
	fn document_blocks_render_at_every_detail_level() {
		let config = Config::default();
		render_document_blocks_with_config(&[], &config, "partial");

		let blocks = vec![
			document_block("README.md", &lines(3), Some(0.25)),
			document_block("README.md", &lines(30), None),
			document_block("GUIDE.md", "single line", Some(0.5)),
		];
		for level in ["signatures", "partial", "full", "unknown"] {
			render_document_blocks_with_config(&blocks, &config, level);
		}
	}
}
