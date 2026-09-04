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
	use crate::config::Config;
	use crate::indexer::render_utils::*;
	use crate::indexer::signature_extractor::{FileSignature, SignatureItem};
	use crate::store::{CodeBlock, DocumentBlock, TextBlock};

	fn signature(kind: &str, name: &str, body: &str, start: usize, end: usize) -> SignatureItem {
		SignatureItem {
			kind: kind.to_string(),
			name: name.to_string(),
			signature: body.to_string(),
			description: None,
			start_line: start,
			end_line: end,
		}
	}

	fn file_signature(items: Vec<SignatureItem>) -> FileSignature {
		FileSignature {
			path: "src/lib.rs".to_string(),
			language: "rust".to_string(),
			file_comment: None,
			signatures: items,
		}
	}

	fn code_block(path: &str, content: &str) -> CodeBlock {
		CodeBlock {
			path: path.to_string(),
			language: "rust".to_string(),
			content: content.to_string(),
			symbols: vec!["alpha".to_string(), "beta_gamma".to_string()],
			start_line: 1,
			end_line: 10,
			hash: "hash".to_string(),
			distance: Some(0.25),
		}
	}

	#[test]
	fn truncation_disabled_when_budget_is_zero() {
		let content = "a".repeat(10_000);
		let (out, truncated) = truncate_content_smartly(&content, 0);
		assert!(!truncated);
		assert_eq!(out, content);
	}

	#[test]
	fn content_within_budget_is_untouched() {
		let (out, truncated) = truncate_content_smartly("short", 100);
		assert!(!truncated);
		assert_eq!(out, "short");
	}

	#[test]
	fn single_long_line_keeps_head_and_tail() {
		let content = format!("{}{}", "A".repeat(300), "Z".repeat(300));
		let (out, truncated) = truncate_content_smartly(&content, 90);
		assert!(truncated);
		assert!(out.starts_with('A'));
		assert!(out.ends_with('Z'));
		assert!(out.contains("characters omitted"));
	}

	#[test]
	fn multi_line_truncation_reports_omitted_line_count() {
		let content: String = (1..=200)
			.map(|i| format!("line-number-{i}"))
			.collect::<Vec<_>>()
			.join("\n");
		let (out, truncated) = truncate_content_smartly(&content, 200);
		assert!(truncated);
		assert!(out.contains("more lines"));
		assert!(out.starts_with("line-number-1\n"));
		assert!(out.ends_with("line-number-200"));
	}

	#[test]
	fn a_budget_smaller_than_the_omission_marker_leaves_only_the_marker() {
		// The marker itself is reserved out of the budget, so a tiny limit has no
		// room left for real lines.
		let (out, truncated) = truncate_content_smartly("aa\nbb", 4);
		assert!(truncated);
		assert_eq!(out, "[... 2 more lines ...]");
	}

	#[test]
	fn empty_signature_list_renders_a_placeholder_in_every_format() {
		assert_eq!(signatures_to_markdown(&[]), "No signatures found.");
		assert_eq!(render_signatures_text(&[]), "No signatures found.");
	}

	#[test]
	fn markdown_signatures_include_line_ranges_and_language_fence() {
		let sigs = vec![file_signature(vec![
			signature("function", "single", "fn single()", 4, 4),
			signature("struct", "spanning", "struct S {\n\tx: u8,\n}", 10, 12),
		])];
		let md = signatures_to_markdown(&sigs);
		assert!(md.contains("# Found signatures in 1 files"));
		assert!(md.contains("### function `single` (line 4)"));
		assert!(md.contains("### struct `spanning` (line 10-12)"));
		assert!(md.contains("```rust"));
	}

	#[test]
	fn markdown_signatures_elide_the_middle_of_long_bodies() {
		let body = (1..=9)
			.map(|i| format!("body{i}"))
			.collect::<Vec<_>>()
			.join("\n");
		let md = signatures_to_markdown(&[file_signature(vec![signature(
			"function", "long", &body, 1, 9,
		)])]);
		assert!(md.contains("// ... 5 more lines"));
		assert!(md.contains("body1"));
		assert!(md.contains("body9"));
		assert!(!md.contains("body5"));
	}

	#[test]
	fn markdown_signatures_render_file_and_item_descriptions_as_quotes() {
		let mut item = signature("function", "documented", "fn documented()", 2, 2);
		item.description = Some("does\nthings".to_string());
		let mut fs = file_signature(vec![item]);
		fs.file_comment = Some("module\ndoc".to_string());
		let md = signatures_to_markdown(&[fs]);
		assert!(md.contains("### File description"));
		assert!(md.contains("> module\n> doc"));
		assert!(md.contains("> does\n> things"));
	}

	#[test]
	fn markdown_signatures_note_files_without_items() {
		let md = signatures_to_markdown(&[file_signature(vec![])]);
		assert!(md.contains("*No signatures found in this file.*"));
	}

	#[test]
	fn text_signatures_number_every_rendered_line() {
		let text = render_signatures_text(&[file_signature(vec![signature(
			"function",
			"two_liner",
			"fn two_liner() {\n}",
			7,
			8,
		)])]);
		assert!(text.contains("SIGNATURES (1 files)"));
		assert!(text.contains("7: fn two_liner() {"));
		assert!(text.contains("8: }"));
	}

	#[test]
	fn text_signatures_elide_long_bodies_but_keep_the_tail_numbering() {
		let body = (0..8)
			.map(|i| format!("l{i}"))
			.collect::<Vec<_>>()
			.join("\n");
		let text = render_signatures_text(&[file_signature(vec![signature(
			"function", "long", &body, 100, 107,
		)])]);
		assert!(text.contains("// ... 4 more lines"));
		assert!(text.contains("100: l0"));
		assert!(text.contains("107: l7"));
	}

	#[test]
	fn empty_block_lists_render_their_own_placeholders() {
		assert_eq!(
			code_blocks_to_markdown(&[]),
			"No code blocks found for the query."
		);
		assert_eq!(
			text_blocks_to_markdown(&[]),
			"No text blocks found for the query."
		);
		assert_eq!(
			document_blocks_to_markdown(&[]),
			"No documentation found for the query."
		);
	}

	#[test]
	fn code_blocks_group_by_file_and_show_similarity() {
		let blocks = vec![
			code_block("src/a.rs", "fn a() {}"),
			code_block("src/a.rs", "fn b() {}"),
			code_block("src/z.rs", "fn c() {}"),
		];
		let md = code_blocks_to_markdown(&blocks);
		assert!(md.contains("# Found 3 code blocks"));
		assert!(md.contains("## File: src/a.rs"));
		assert!(md.contains("## File: src/z.rs"));
		assert!(md.contains("### Block 2 of 2"));
		assert!(md.contains("**Similarity:** 0.7500"));
		// Symbols carrying an underscore are internal and stay hidden.
		assert!(md.contains("- `alpha`"));
		assert!(!md.contains("beta_gamma"));
	}

	#[test]
	fn code_blocks_flag_truncated_content_with_the_active_limit() {
		let mut config = Config::default();
		config.search.search_block_max_characters = 60;
		let content: String = (1..=40)
			.map(|i| format!("statement_{i}();"))
			.collect::<Vec<_>>()
			.join("\n");
		let md = code_blocks_to_markdown_with_config(&[code_block("src/a.rs", &content)], &config);
		assert!(md.contains("// Content truncated (limit: 60 chars)"));
	}

	#[test]
	fn text_blocks_render_relevance_and_lines() {
		let block = TextBlock {
			path: "notes.txt".to_string(),
			language: "text".to_string(),
			content: "hello".to_string(),
			start_line: 3,
			end_line: 5,
			hash: "h".to_string(),
			distance: Some(0.1),
		};
		let md = text_blocks_to_markdown(&[block]);
		assert!(md.contains("# Found 1 text blocks"));
		assert!(md.contains("**Lines:** 3-5"));
		assert!(md.contains("**Relevance:** 0.9000"));
	}

	#[test]
	fn document_blocks_render_title_level_and_truncation_note() {
		let mut config = Config::default();
		config.search.search_block_max_characters = 50;
		let block = DocumentBlock {
			path: "README.md".to_string(),
			title: "Install".to_string(),
			content: (1..=30)
				.map(|i| format!("paragraph line {i}"))
				.collect::<Vec<_>>()
				.join("\n"),
			context: vec![],
			level: 2,
			start_line: 1,
			end_line: 30,
			hash: "h".to_string(),
			distance: None,
		};
		let md = document_blocks_to_markdown_with_config(&[block], &config);
		assert!(md.contains("### Install (Section 1 of 1)"));
		assert!(md.contains("**Level:** 2"));
		assert!(md.contains("*Content truncated (limit: 50 chars)*"));
		assert!(!md.contains("**Relevance:**"));
	}

	#[test]
	fn render_to_markdown_passes_the_content_through() {
		assert_eq!(render_to_markdown("ignored", "body"), "body");
	}

	#[test]
	fn cli_and_json_renderers_run_without_panicking() {
		let mut item = signature("function", "cli", "fn cli() {\n}", 1, 2);
		item.description = Some("desc".to_string());
		let long = (0..8)
			.map(|i| format!("x{i}"))
			.collect::<Vec<_>>()
			.join("\n");
		let mut fs = file_signature(vec![
			item,
			signature("function", "one_liner", "fn one_liner();", 5, 5),
			signature("function", "long", &long, 20, 27),
		]);
		fs.file_comment = Some("top".to_string());
		render_signatures_cli(&[]);
		render_signatures_cli(&[fs.clone(), file_signature(vec![])]);
		render_signatures_json(&[fs]).expect("signatures must serialize");
	}
}
