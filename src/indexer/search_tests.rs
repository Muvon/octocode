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
	use crate::store::{CodeBlock, CommitBlock, DocumentBlock, TextBlock};

	fn lines(count: usize, prefix: &str) -> String {
		(1..=count)
			.map(|i| format!("{prefix}{i}"))
			.collect::<Vec<_>>()
			.join("\n")
	}

	fn code(path: &str, content: &str, distance: Option<f32>) -> CodeBlock {
		CodeBlock {
			path: path.to_string(),
			language: "rust".to_string(),
			content: content.to_string(),
			symbols: vec!["alpha".to_string(), "beta_type".to_string()],
			start_line: 10,
			end_line: 20,
			hash: format!("h-{path}"),
			distance,
		}
	}

	fn text(path: &str, content: &str) -> TextBlock {
		TextBlock {
			path: path.to_string(),
			language: "text".to_string(),
			content: content.to_string(),
			start_line: 5,
			end_line: 9,
			hash: format!("t-{path}"),
			distance: Some(0.2),
		}
	}

	fn doc(path: &str, content: &str) -> DocumentBlock {
		DocumentBlock {
			path: path.to_string(),
			title: "Section".to_string(),
			content: content.to_string(),
			context: vec![],
			level: 2,
			start_line: 3,
			end_line: 7,
			hash: format!("d-{path}"),
			distance: Some(0.4),
		}
	}

	fn commit(hash: &str, files: &str) -> CommitBlock {
		CommitBlock {
			hash: hash.to_string(),
			author: "dev".to_string(),
			date: 1_700_000_000,
			message: "subject line\n\nbody paragraph".to_string(),
			content: "subject line".to_string(),
			files: files.to_string(),
			description: "ai description".to_string(),
			distance: Some(0.1),
		}
	}

	fn query_result(index: usize, blocks: Vec<CodeBlock>) -> QuerySearchResult {
		QuerySearchResult {
			query_index: index,
			code_blocks: blocks,
			doc_blocks: vec![],
			text_blocks: vec![],
			commit_blocks: vec![],
		}
	}

	#[test]
	fn every_formatter_reports_an_empty_result_set() {
		assert_eq!(
			format_code_search_results_as_text(&[], "partial"),
			"No code results found."
		);
		assert_eq!(
			format_text_search_results_as_text(&[], "partial"),
			"No text results found."
		);
		assert_eq!(
			format_doc_search_results_as_text(&[], "partial"),
			"No documentation results found."
		);
		assert_eq!(
			format_commit_search_results_as_text(&[], "partial"),
			"No commit results found."
		);
		assert_eq!(
			format_combined_search_results_as_text(&[], &[], &[], "partial"),
			"No results found."
		);
	}

	#[test]
	fn code_results_carry_similarity_and_public_symbols_only() {
		let out = format_code_search_results_as_text(
			&[code("src/a.rs", "fn a() {}", Some(0.25))],
			"partial",
		);
		assert!(out.starts_with("CODE RESULTS (1)\n"));
		assert!(out.contains("1. src/a.rs"));
		assert!(out.contains("Similarity 0.750"));
		assert!(out.contains("Symbols: alpha"));
		assert!(!out.contains("beta_type"));
	}

	#[test]
	fn a_code_result_without_a_distance_omits_the_similarity_line() {
		let out =
			format_code_search_results_as_text(&[code("src/a.rs", "fn a() {}", None)], "partial");
		assert!(!out.contains("Similarity"));
	}

	#[test]
	fn code_detail_levels_select_how_much_content_is_shown() {
		let body = lines(30, "line");
		let block = code("src/a.rs", &body, Some(0.1));

		let signatures =
			format_code_search_results_as_text(std::slice::from_ref(&block), "signatures");
		assert!(signatures.contains("10: line1"));
		assert!(!signatures.contains("more lines"));

		let partial = format_code_search_results_as_text(std::slice::from_ref(&block), "partial");
		assert!(partial.contains("... (26 more lines)"));
		assert!(partial.contains("39: line30"));

		let full = format_code_search_results_as_text(std::slice::from_ref(&block), "full");
		assert!(full.contains("15: line6"));
		assert!(!full.contains("more lines"));

		// An unknown level renders the header only.
		let unknown = format_code_search_results_as_text(&[block], "unknown");
		assert!(unknown.contains("1. src/a.rs"));
		assert!(!unknown.contains("line1"));
	}

	#[test]
	fn text_results_render_at_every_detail_level() {
		let block = text("notes.txt", &lines(30, "n"));
		assert!(
			format_text_search_results_as_text(std::slice::from_ref(&block), "signatures")
				.contains("5: n1")
		);
		assert!(
			format_text_search_results_as_text(std::slice::from_ref(&block), "partial")
				.contains("more lines")
		);
		assert!(
			format_text_search_results_as_text(std::slice::from_ref(&block), "full")
				.contains("34: n30")
		);
		assert!(
			format_text_search_results_as_text(&[block], "unknown").contains("TEXT RESULTS (1)")
		);
	}

	#[test]
	fn doc_results_include_the_title_level_and_line_range() {
		let block = doc("README.md", &lines(30, "d"));
		let out = format_doc_search_results_as_text(std::slice::from_ref(&block), "partial");
		assert!(out.starts_with("DOCUMENTATION RESULTS (1)\n"));
		assert!(out.contains("Section (Level 2)"));
		assert!(out.contains("| 3-7"));
		assert!(out.contains("Similarity 0.600"));

		assert!(
			format_doc_search_results_as_text(std::slice::from_ref(&block), "signatures")
				.contains("3: d1")
		);
		assert!(
			format_doc_search_results_as_text(std::slice::from_ref(&block), "full")
				.contains("32: d30")
		);
		assert!(format_doc_search_results_as_text(&[block], "unknown")
			.contains("DOCUMENTATION RESULTS"));
	}

	#[test]
	fn commit_results_shorten_the_hash_unless_full_detail_is_asked_for() {
		let block = commit("0123456789abcdef", "[\"src/a.rs\"]");

		let partial = format_commit_search_results_as_text(std::slice::from_ref(&block), "partial");
		assert!(partial.contains("1. 01234567 (2023-11-14) by dev"));
		assert!(partial.contains("Message: subject line"));
		assert!(!partial.contains("body paragraph"));
		assert!(partial.contains("Files: src/a.rs"));
		assert!(partial.contains("Description: ai description"));

		let full = format_commit_search_results_as_text(std::slice::from_ref(&block), "full");
		assert!(full.contains("1. 0123456789abcdef"));
		assert!(full.contains("body paragraph"));

		let signatures = format_commit_search_results_as_text(&[block], "signatures");
		assert!(signatures.contains("Message: subject line"));
		assert!(!signatures.contains("Files:"));
	}

	#[test]
	fn a_commit_with_unparseable_files_json_still_renders() {
		let out =
			format_commit_search_results_as_text(&[commit("abcdef1234", "not json")], "partial");
		assert!(out.contains("Message: subject line"));
		assert!(!out.contains("Files:"));
	}

	#[test]
	fn a_short_commit_hash_is_not_sliced_out_of_bounds() {
		let out = format_commit_search_results_as_text(&[commit("abc", "[]")], "partial");
		assert!(out.contains("1. abc "), "{out}");
	}

	#[test]
	fn combined_output_concatenates_only_the_non_empty_sections() {
		let out = format_combined_search_results_as_text(
			&[code("src/a.rs", "fn a() {}", Some(0.1))],
			&[text("notes.txt", "note")],
			&[doc("README.md", "docs")],
			"partial",
		);
		assert!(out.starts_with("SEARCH RESULTS (3 total)\n\n"));
		// Documentation is rendered first, then code, then text.
		assert!(out.find("DOCUMENTATION RESULTS") < out.find("CODE RESULTS"));
		assert!(out.find("CODE RESULTS") < out.find("TEXT RESULTS"));

		let code_only = format_combined_search_results_as_text(
			&[code("src/a.rs", "fn a() {}", Some(0.1))],
			&[],
			&[],
			"partial",
		);
		assert!(!code_only.contains("DOCUMENTATION RESULTS"));
		assert!(!code_only.contains("TEXT RESULTS"));
	}

	#[test]
	fn cli_and_json_renderers_run_over_every_detail_level() {
		let config = Config::default();
		render_code_blocks(&[]);
		render_code_blocks_with_config(&[], &config, "partial");

		let short = code("src/a.rs", &lines(3, "s"), Some(0.2));
		let long = code("src/b.rs", &lines(40, "l"), None);
		let blocks = vec![short, long];
		for level in ["signatures", "partial", "full", "unknown"] {
			render_code_blocks_with_config(&blocks, &config, level);
		}
		render_code_blocks(&blocks);
		render_results_json(&blocks).expect("blocks must serialize");
	}

	#[test]
	fn full_detail_cli_rendering_runs_over_the_configured_budget_without_panicking() {
		let mut config = Config::default();
		config.search.search_block_max_characters = 40;
		render_code_blocks_with_config(
			&[code("src/a.rs", &lines(40, "verylongline"), None)],
			&config,
			"full",
		);
	}

	#[test]
	fn a_short_block_is_previewed_in_full() {
		let preview = get_code_preview_with_lines("fn a() {}\nlet x = 1;", 7, "rust");
		assert_eq!(preview, "7: fn a() {}\n8: let x = 1;");
	}

	#[test]
	fn a_code_preview_skips_the_leading_comment_block() {
		let content = format!("// header\n// more header\n{}", lines(20, "code"));
		let preview = get_code_preview_with_lines(&content, 1, "rust");
		assert!(preview.starts_with("3: code1"), "{preview}");
		assert!(preview.contains("more lines"));
		assert!(preview.contains("22: code20"));
	}

	#[test]
	fn a_text_preview_skips_leading_blank_lines() {
		let content = format!("\n\n{}", lines(20, "t"));
		let preview = get_text_preview_with_lines(&content, 1);
		assert!(preview.starts_with("3: t1"), "{preview}");
		assert!(preview.contains("more lines"));
	}

	#[test]
	fn a_preview_with_only_a_few_trailing_lines_lists_them_all() {
		// 12 lines: 4 shown up front, 8 remain — more than the 3-line tail, so the
		// separator path runs. With 6 remaining it lists them instead.
		let content = lines(11, "x");
		let preview = get_doc_preview_with_lines(&content, 1);
		assert!(preview.contains("1: x1"));
		assert!(preview.contains("11: x11"));
	}

	#[test]
	fn a_doc_preview_of_short_content_is_returned_whole() {
		let preview = get_doc_preview_with_lines("only\ntwo", 4);
		assert_eq!(preview, "4: only\n5: two");
	}

	#[test]
	fn identifier_shaped_queries_tilt_towards_keyword_matching() {
		assert_eq!(classify_query_weights("parse_remote", 0.8, 0.2), (0.3, 0.7));
		assert_eq!(classify_query_weights("Store::new", 0.8, 0.2), (0.3, 0.7));
		assert_eq!(classify_query_weights("parseRemote", 0.8, 0.2), (0.3, 0.7));
		assert_eq!(classify_query_weights("foo.bar()", 0.8, 0.2), (0.3, 0.7));
	}

	#[test]
	fn natural_language_queries_keep_the_configured_weights() {
		assert_eq!(
			classify_query_weights("how does indexing work", 0.8, 0.2),
			(0.8, 0.2)
		);
		// Lowercase and punctuation-free short queries are not identifiers either.
		assert_eq!(classify_query_weights("parse remote", 0.8, 0.2), (0.8, 0.2));
		assert_eq!(classify_query_weights("", 0.8, 0.2), (0.8, 0.2));
	}

	#[test]
	fn all_mode_fusion_leaves_a_result_set_under_the_cap_alone() {
		let mut code_blocks = vec![code("src/a.rs", "a", Some(0.1))];
		let mut text_blocks = vec![text("notes.txt", "n")];
		let mut doc_blocks = vec![doc("README.md", "d")];
		fuse_all_mode_results(&mut code_blocks, &mut text_blocks, &mut doc_blocks, 10);
		assert_eq!(code_blocks.len(), 1);
		assert_eq!(text_blocks.len(), 1);
		assert_eq!(doc_blocks.len(), 1);
	}

	#[test]
	fn all_mode_fusion_samples_every_modality_when_capping() {
		let mut code_blocks: Vec<CodeBlock> = (0..5)
			.map(|i| code(&format!("src/{i}.rs"), "a", Some(0.01 * i as f32)))
			.collect();
		let mut text_blocks: Vec<TextBlock> =
			(0..5).map(|i| text(&format!("n{i}.txt"), "n")).collect();
		let mut doc_blocks: Vec<DocumentBlock> =
			(0..5).map(|i| doc(&format!("d{i}.md"), "d")).collect();

		fuse_all_mode_results(&mut code_blocks, &mut text_blocks, &mut doc_blocks, 3);

		assert_eq!(code_blocks.len() + text_blocks.len() + doc_blocks.len(), 3);
		// Rank-based fusion keeps each list's best item rather than starving a
		// modality whose raw distances happen to be larger.
		assert_eq!(code_blocks.len(), 1);
		assert_eq!(text_blocks.len(), 1);
		assert_eq!(doc_blocks.len(), 1);
		assert_eq!(code_blocks[0].path, "src/0.rs");
	}

	#[test]
	fn cross_query_deduplication_keeps_one_entry_per_hash() {
		let shared = code("src/shared.rs", "shared", Some(0.2));
		let results = vec![
			query_result(0, vec![shared.clone(), code("src/a.rs", "a", Some(0.3))]),
			query_result(1, vec![shared, code("src/b.rs", "b", Some(0.4))]),
		];

		let (code_blocks, docs, texts, commits) = deduplicate_and_merge_results(results, None);
		assert_eq!(code_blocks.len(), 3);
		// The block both queries agreed on is ranked first.
		assert_eq!(code_blocks[0].path, "src/shared.rs");
		assert!(docs.is_empty() && texts.is_empty() && commits.is_empty());
	}

	#[test]
	fn deduplication_applies_the_distance_threshold() {
		let results = vec![query_result(
			0,
			vec![
				code("src/near.rs", "a", Some(0.1)),
				code("src/far.rs", "b", Some(0.9)),
			],
		)];
		let (code_blocks, _, _, _) = deduplicate_and_merge_results(results, Some(0.5));
		assert_eq!(code_blocks.len(), 1);
		assert_eq!(code_blocks[0].path, "src/near.rs");
	}

	#[test]
	fn deduplicating_nothing_returns_empty_lists() {
		let (code_blocks, docs, texts, commits) = deduplicate_and_merge_results(vec![], None);
		assert!(code_blocks.is_empty());
		assert!(docs.is_empty());
		assert!(texts.is_empty());
		assert!(commits.is_empty());
	}

	#[test]
	fn a_block_of_exactly_ten_lines_is_previewed_whole() {
		// Ten lines is the boundary of the "short content" fast path.
		let preview = get_code_preview_with_lines(&lines(10, "x"), 3, "rust");
		assert_eq!(preview.lines().count(), 10);
		assert_eq!(preview.lines().next().unwrap(), "3: x1");
		assert_eq!(preview.lines().last().unwrap(), "12: x10");
		assert!(!preview.contains("more lines"));
	}

	#[test]
	fn a_code_preview_whose_tail_is_short_lists_every_remaining_line() {
		// Four comment lines are skipped, four lines are shown, and the three
		// that remain are listed rather than summarised.
		let content = format!("// a\n// b\n// c\n// d\n{}", lines(7, "code"));
		assert_eq!(
			get_code_preview_with_lines(&content, 1, "rust"),
			"5: code1\n6: code2\n7: code3\n8: code4\n9: code5\n10: code6\n11: code7"
		);
	}

	#[test]
	fn a_text_preview_whose_tail_is_short_lists_every_remaining_line() {
		let content = format!("\n\n\n\n{}", lines(7, "t"));
		assert_eq!(
			get_text_preview_with_lines(&content, 1),
			"5: t1\n6: t2\n7: t3\n8: t4\n9: t5\n10: t6\n11: t7"
		);
	}

	#[test]
	fn a_doc_preview_summarises_a_long_tail_and_keeps_the_last_three_lines() {
		assert_eq!(
			get_doc_preview_with_lines(&lines(12, "d"), 1),
			"1: d1\n2: d2\n3: d3\n4: d4\n... (8 more lines)\n10: d10\n11: d11\n12: d12"
		);
	}

	#[test]
	fn a_block_that_is_nothing_but_comments_is_previewed_from_its_first_line() {
		// No non-comment line exists, so the skip loop finds no start and the
		// preview falls back to the top of the block.
		let content = (1..=12)
			.map(|i| format!("// note {i}"))
			.collect::<Vec<_>>()
			.join("\n");
		let preview = get_code_preview_with_lines(&content, 1, "rust");
		assert!(preview.starts_with("1: // note 1"), "{preview}");
		assert!(preview.contains("... (8 more lines)"), "{preview}");
		assert!(preview.ends_with("12: // note 12"), "{preview}");
	}

	#[test]
	fn a_long_query_keeps_the_configured_weights_even_with_code_punctuation() {
		// The identifier heuristic only fires for queries of three words or less.
		assert_eq!(
			classify_query_weights("where is Store::new called from", 0.8, 0.2),
			(0.8, 0.2)
		);
		// Exactly three words with a symbol still counts as an identifier lookup.
		assert_eq!(
			classify_query_weights("Store::new call site", 0.8, 0.2),
			(0.3, 0.7)
		);
	}

	#[test]
	fn cross_query_fusion_keeps_the_closest_copy_of_a_duplicated_block() {
		// Same hash, different cosine: the better (lower) distance becomes the
		// representative while both appearances still add to the fused score.
		let far = code("src/shared.rs", "shared", Some(0.9));
		let near = code("src/shared.rs", "shared", Some(0.1));
		let results = vec![
			query_result(0, vec![far, code("src/other.rs", "o", Some(0.2))]),
			query_result(1, vec![near]),
		];

		let (code_blocks, _, _, _) = deduplicate_and_merge_results(results, None);
		assert_eq!(code_blocks.len(), 2);
		assert_eq!(code_blocks[0].path, "src/shared.rs");
		assert_eq!(
			code_blocks[0].distance,
			Some(0.1),
			"the closer copy must survive deduplication"
		);
	}

	#[test]
	fn cross_query_fusion_breaks_score_ties_deterministically_by_hash() {
		// Two blocks seen once each at the same rank tie on fused score and have
		// no distance to separate them, so the hash decides the order.
		let mut first = code("src/zeta.rs", "z", None);
		first.hash = "hash-b".to_string();
		let mut second = code("src/alpha.rs", "a", None);
		second.hash = "hash-a".to_string();

		let (code_blocks, _, _, _) = deduplicate_and_merge_results(
			vec![query_result(0, vec![first]), query_result(1, vec![second])],
			None,
		);
		assert_eq!(code_blocks.len(), 2);
		assert_eq!(code_blocks[0].hash, "hash-a");
		assert_eq!(code_blocks[1].hash, "hash-b");
	}

	#[test]
	fn a_block_without_a_distance_survives_the_threshold_filter() {
		// Threshold filtering only drops blocks that actually carry a cosine.
		let results = vec![query_result(
			0,
			vec![
				code("src/unscored.rs", "u", None),
				code("src/far.rs", "f", Some(0.9)),
			],
		)];
		let (code_blocks, _, _, _) = deduplicate_and_merge_results(results, Some(0.5));
		assert_eq!(code_blocks.len(), 1);
		assert_eq!(code_blocks[0].path, "src/unscored.rs");
	}

	mod execution {
		use super::super::super::*;
		use crate::embedding::SearchModeEmbeddings;
		use crate::store::mod_tests::{
			code_block, commit_block, document_block, embedding, test_store, text_block, CODE_DIM,
			TEXT_DIM,
		};
		use crate::store::Store;

		async fn populated() -> (tempfile::TempDir, Store) {
			let (dir, store) = test_store().await;
			store
				.store_code_blocks(
					&[code_block("src/a.rs", "h1"), code_block("src/b.rs", "h2")],
					&[embedding(CODE_DIM, 0), embedding(CODE_DIM, 1)],
				)
				.await
				.unwrap();
			store
				.store_text_blocks(&[text_block("notes.txt", "t1")], &[embedding(TEXT_DIM, 0)])
				.await
				.unwrap();
			store
				.store_document_blocks(
					&[document_block("README.md", "d1")],
					&[embedding(TEXT_DIM, 0)],
				)
				.await
				.unwrap();
			store
				.store_commit_blocks(&[commit_block("c1")], &[embedding(TEXT_DIM, 0)])
				.await
				.unwrap();
			(dir, store)
		}

		fn both_embeddings() -> SearchModeEmbeddings {
			SearchModeEmbeddings {
				code_embeddings: Some(embedding(CODE_DIM, 0)),
				text_embeddings: Some(embedding(TEXT_DIM, 0)),
			}
		}

		fn params(mode: &'static str, hybrid: bool) -> SingleQuerySearchParams<'static> {
			SingleQuerySearchParams {
				mode,
				limit: 10,
				distance_threshold: None,
				language_filter: None,
				hybrid_enabled: hybrid,
				vector_weight: 0.7,
				keyword_weight: 0.3,
			}
		}

		#[tokio::test]
		async fn each_mode_queries_only_its_own_modality() {
			let (_dir, store) = populated().await;

			let code = execute_single_search_with_embeddings(
				&store,
				both_embeddings(),
				&params("code", false),
				0,
				None,
			)
			.await
			.unwrap();
			assert_eq!(code.code_blocks.len(), 2);
			assert!(code.text_blocks.is_empty());
			assert!(code.doc_blocks.is_empty());
			assert!(code.commit_blocks.is_empty());

			let docs = execute_single_search_with_embeddings(
				&store,
				both_embeddings(),
				&params("docs", false),
				1,
				None,
			)
			.await
			.unwrap();
			assert_eq!(docs.doc_blocks.len(), 1);
			assert_eq!(docs.query_index, 1);

			let text = execute_single_search_with_embeddings(
				&store,
				both_embeddings(),
				&params("text", false),
				2,
				None,
			)
			.await
			.unwrap();
			assert_eq!(text.text_blocks.len(), 1);

			let commits = execute_single_search_with_embeddings(
				&store,
				both_embeddings(),
				&params("commits", false),
				3,
				None,
			)
			.await
			.unwrap();
			assert_eq!(commits.commit_blocks.len(), 1);
		}

		#[tokio::test]
		async fn all_mode_gathers_every_modality_and_caps_the_total() {
			let (_dir, store) = populated().await;
			let mut p = params("all", false);
			p.limit = 2;

			let result =
				execute_single_search_with_embeddings(&store, both_embeddings(), &p, 0, None)
					.await
					.unwrap();
			let total =
				result.code_blocks.len() + result.text_blocks.len() + result.doc_blocks.len();
			assert_eq!(total, 2, "got {result:?}");
		}

		#[tokio::test]
		async fn a_missing_embedding_leaves_that_modality_empty() {
			let (_dir, store) = populated().await;
			let code_only = SearchModeEmbeddings {
				code_embeddings: Some(embedding(CODE_DIM, 0)),
				text_embeddings: None,
			};

			let result = execute_single_search_with_embeddings(
				&store,
				code_only,
				&params("all", false),
				0,
				None,
			)
			.await
			.unwrap();
			assert!(!result.code_blocks.is_empty());
			assert!(result.text_blocks.is_empty());
			assert!(result.doc_blocks.is_empty());

			let text_only = SearchModeEmbeddings {
				code_embeddings: None,
				text_embeddings: None,
			};
			let empty = execute_single_search_with_embeddings(
				&store,
				text_only,
				&params("code", false),
				0,
				None,
			)
			.await
			.unwrap();
			assert!(empty.code_blocks.is_empty());
		}

		#[tokio::test]
		async fn the_hybrid_path_runs_for_every_mode_when_a_query_is_supplied() {
			let (_dir, store) = populated().await;
			for mode in ["code", "docs", "text", "commits", "all"] {
				execute_single_search_with_embeddings(
					&store,
					both_embeddings(),
					&params(mode, true),
					0,
					Some("from_h1"),
				)
				.await
				.unwrap_or_else(|e| panic!("hybrid {mode} failed: {e}"));
			}
		}

		#[tokio::test]
		async fn a_blank_query_falls_back_to_the_vector_path() {
			let (_dir, store) = populated().await;
			let result = execute_single_search_with_embeddings(
				&store,
				both_embeddings(),
				&params("code", true),
				0,
				Some("   "),
			)
			.await
			.unwrap();
			assert_eq!(result.code_blocks.len(), 2);
		}

		#[tokio::test]
		async fn a_language_filter_is_applied_to_code_results() {
			let (_dir, store) = populated().await;
			let mut p = params("code", false);
			p.language_filter = Some("python");
			let result =
				execute_single_search_with_embeddings(&store, both_embeddings(), &p, 0, None)
					.await
					.unwrap();
			assert!(result.code_blocks.is_empty());
		}

		#[tokio::test]
		async fn an_unknown_mode_is_rejected() {
			let (_dir, store) = populated().await;
			let err = execute_single_search_with_embeddings(
				&store,
				both_embeddings(),
				&params("sideways", false),
				0,
				None,
			)
			.await
			.unwrap_err();
			assert!(err.to_string().contains("Invalid search mode"), "{err}");
		}

		#[tokio::test]
		async fn parallel_searches_return_one_result_per_query() {
			let (_dir, store) = populated().await;
			let config = Config::default();
			let params = SearchParams {
				mode: "code",
				max_results: 5,
				similarity_threshold: 1.0,
				language_filter: None,
				config: &config,
				branch_ctx: None,
			};

			let results = execute_parallel_searches(
				&store,
				vec![
					("first query".to_string(), both_embeddings()),
					("second query".to_string(), both_embeddings()),
				],
				&params,
			)
			.await
			.unwrap();

			assert_eq!(results.len(), 2);
			let mut indexes: Vec<_> = results.iter().map(|r| r.query_index).collect();
			indexes.sort();
			assert_eq!(indexes, vec![0, 1]);
		}

		#[tokio::test]
		async fn a_project_without_a_branch_index_has_no_branch_context() {
			let (dir, store) = populated().await;
			assert!(detect_branch_search_context(&store, dir.path())
				.await
				.is_none());
		}
	}
}
