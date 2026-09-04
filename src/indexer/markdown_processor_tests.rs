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
	use crate::config::Config;

	const DOC: &str = "\
# Project

Intro paragraph describing the project in a couple of sentences.

## Install

Run the installer and follow the prompts on screen.

### Requirements

A recent toolchain and a working network connection.

## Usage

Call the binary with a subcommand.
";

	fn levels(hierarchy: &DocumentHierarchy) -> Vec<usize> {
		hierarchy.sections.iter().map(|s| s.level).collect()
	}

	#[test]
	fn the_hierarchy_mirrors_the_heading_levels() {
		let hierarchy = parse_document_hierarchy(DOC);
		assert_eq!(levels(&hierarchy), vec![1, 2, 3, 2]);

		let nested = &hierarchy.sections[2];
		assert!(
			nested.context.iter().any(|c| c.contains("Install")),
			"a level-3 section must carry its parent headings: {:?}",
			nested.context
		);
	}

	#[test]
	fn a_document_with_no_headings_still_yields_a_section() {
		let hierarchy = parse_document_hierarchy("just a paragraph of prose\nand another line\n");
		assert!(!hierarchy.sections.is_empty());
	}

	#[test]
	fn an_empty_document_has_no_sections() {
		assert!(parse_document_hierarchy("").sections.is_empty());
	}

	#[test]
	fn a_hash_inside_a_fenced_block_is_not_a_heading() {
		// Every section needs a body: an empty trailing section is dropped.
		let doc = "# Real\n\nprose\n\n```sh\n# not a heading\n```\n\n## Also real\n\nmore prose\n";
		let hierarchy = parse_document_hierarchy(doc);
		assert_eq!(
			levels(&hierarchy),
			vec![1, 2],
			"a comment inside a fence must not open a section"
		);
	}

	#[test]
	fn parent_and_child_links_follow_the_heading_nesting() {
		// `parse_document_hierarchy` already wires the links; calling the builder
		// again would append the same children a second time.
		let hierarchy = parse_document_hierarchy(DOC);

		// Section 2 is "### Requirements", nested under section 1 ("## Install"),
		// which in turn hangs off section 0 ("# Project").
		assert_eq!(hierarchy.sections[0].parent, None);
		assert_eq!(hierarchy.sections[1].parent, Some(0));
		assert_eq!(hierarchy.sections[2].parent, Some(1));
		assert_eq!(hierarchy.sections[3].parent, Some(0));
		assert_eq!(hierarchy.sections[1].children, vec![2]);
		assert_eq!(hierarchy.root_sections, vec![0]);
	}

	#[test]
	fn parsing_produces_blocks_carrying_their_heading_context() {
		let config = Config::default();
		let blocks = parse_markdown_content(DOC, "README.md", &config);
		assert!(!blocks.is_empty());
		assert!(blocks.iter().all(|b| b.path == "README.md"));
		assert!(blocks.iter().all(|b| !b.hash.is_empty()));
		assert!(blocks.iter().all(|b| b.end_line >= b.start_line));
		assert!(blocks.iter().all(|b| !b.content.trim().is_empty()));
	}

	#[test]
	fn an_empty_document_produces_no_blocks() {
		let config = Config::default();
		assert!(parse_markdown_content("", "README.md", &config).is_empty());
	}

	#[test]
	fn identical_content_at_different_paths_hashes_differently() {
		let config = Config::default();
		let a = parse_markdown_content(DOC, "a.md", &config);
		let b = parse_markdown_content(DOC, "b.md", &config);
		assert_eq!(a.len(), b.len());
		assert_ne!(a[0].hash, b[0].hash);
	}

	#[test]
	fn a_large_section_is_split_across_several_chunks() {
		let mut config = Config::default();
		config.index.chunk_size = 200;
		config.index.chunk_overlap = 0;

		let body: String = (1..=60)
			.map(|i| format!("Sentence number {i} of the long section."))
			.collect::<Vec<_>>()
			.join("\n");
		let doc = format!("# Long\n\n{body}\n");

		let blocks = parse_markdown_content(&doc, "long.md", &config);
		assert!(blocks.len() > 1, "expected the section to be split");
		assert!(blocks
			.iter()
			.all(|b| b.path == "long.md" && !b.content.trim().is_empty()));
	}

	#[test]
	fn a_document_that_ends_inside_a_fence_still_produces_blocks() {
		let config = Config::default();
		let doc = "# Title\n\nintro text here\n\n```rust\nfn unterminated() {\n";
		let blocks = parse_markdown_content(doc, "broken.md", &config);
		assert!(
			!blocks.is_empty(),
			"an unterminated fence must not drop the file"
		);
	}

	#[test]
	fn seven_hashes_are_plain_text_not_a_heading() {
		// CommonMark caps ATX headings at six `#`.
		let doc = "# Real\n\nprose here\n\n####### not a heading\n";
		let hierarchy = parse_document_hierarchy(doc);
		assert_eq!(levels(&hierarchy), vec![1]);
		assert!(hierarchy.sections[0]
			.content
			.contains("####### not a heading"));
	}

	#[test]
	fn content_before_the_first_heading_is_kept_as_its_own_section() {
		let doc = "preamble prose that precedes any heading\n\n# First\n\nbody text\n";
		let hierarchy = parse_document_hierarchy(doc);
		assert_eq!(levels(&hierarchy), vec![1, 1]);
		assert_eq!(
			hierarchy.sections[0].content,
			"preamble prose that precedes any heading"
		);
		assert!(
			hierarchy.sections[0].context.is_empty(),
			"a preamble has no heading of its own"
		);
		assert_eq!(hierarchy.sections[1].context, vec!["# First".to_string()]);
	}

	#[test]
	fn a_heading_with_no_body_is_dropped() {
		let doc = "# Empty\n\n## Also empty\n\n## Filled\n\nactual content here\n";
		let hierarchy = parse_document_hierarchy(doc);
		assert_eq!(levels(&hierarchy), vec![2]);
		assert_eq!(hierarchy.sections[0].content, "actual content here");
	}

	// ---- section/chunk helpers ----

	fn section(
		level: usize,
		title: &str,
		content: &str,
		start: usize,
		end: usize,
	) -> HeaderSection {
		HeaderSection {
			level,
			content: content.to_string(),
			context: vec![format!("{} {}", "#".repeat(level), title)],
			start_line: start,
			end_line: end,
			children: Vec::new(),
			parent: None,
		}
	}

	fn chunk(title: &str, content: &str, start: usize, end: usize) -> ChunkResult {
		ChunkResult {
			title: title.to_string(),
			storage_content: content.to_string(),
			context: vec![title.to_string()],
			level: 2,
			start_line: start,
			end_line: end,
		}
	}

	#[test]
	fn the_target_chunk_size_shrinks_as_headings_get_deeper() {
		let hierarchy = DocumentHierarchy::new();
		assert_eq!(hierarchy.get_target_chunk_size(1, 1200), 2400);
		assert_eq!(hierarchy.get_target_chunk_size(2, 1200), 1200);
		assert_eq!(hierarchy.get_target_chunk_size(3, 1200), 900);
		assert_eq!(hierarchy.get_target_chunk_size(4, 1200), 600);
		assert_eq!(hierarchy.get_target_chunk_size(5, 1200), 400);
		assert_eq!(hierarchy.get_target_chunk_size(6, 1200), 400);
	}

	#[test]
	fn a_section_title_falls_back_when_the_section_carries_no_heading() {
		let mut hierarchy = DocumentHierarchy::new();
		hierarchy.add_section(section(2, "Install", "body", 0, 3));
		let mut headless = section(1, "unused", "body", 4, 6);
		headless.context.clear();
		hierarchy.add_section(headless);

		assert_eq!(hierarchy.get_section_title(0), "## Install");
		assert_eq!(hierarchy.get_section_title(1), "Untitled Section");
	}

	#[test]
	fn merging_two_sections_keeps_the_first_ones_heading_and_spans_both_ranges() {
		let mut hierarchy = DocumentHierarchy::new();
		hierarchy.add_section(section(2, "First", "alpha", 10, 12));
		hierarchy.add_section(section(3, "Second", "beta", 13, 20));

		let merged = hierarchy.merge_sections_safe(0, 1);
		assert_eq!(merged.title, "## First");
		assert_eq!(merged.storage_content, "alpha\n\nbeta");
		assert_eq!(merged.context, vec!["## First".to_string()]);
		assert_eq!(merged.level, 2, "the shallower level wins");
		assert_eq!((merged.start_line, merged.end_line), (10, 20));
	}

	#[test]
	fn merging_across_an_unclosed_fence_avoids_inserting_a_blank_line() {
		// A blank line inside a fenced block would break the code block, so the
		// merge uses a single newline when the first section leaves a fence open.
		let mut hierarchy = DocumentHierarchy::new();
		hierarchy.add_section(section(2, "Open", "```rust\nfn f() {", 0, 2));
		hierarchy.add_section(section(2, "Close", "}\n```", 3, 5));

		let merged = hierarchy.merge_sections_safe(0, 1);
		assert_eq!(merged.storage_content, "```rust\nfn f() {\n}\n```");
		assert!(!merged.storage_content.contains("\n\n"));
	}

	#[test]
	fn merging_no_sections_yields_the_empty_placeholder_chunk() {
		let hierarchy = DocumentHierarchy::new();
		let merged = hierarchy.merge_multiple_sections(&[]);
		assert_eq!(merged.title, "Empty Section");
		assert_eq!(merged.storage_content, "");
		assert_eq!(
			(merged.level, merged.start_line, merged.end_line),
			(1, 0, 0)
		);
	}

	#[test]
	fn merging_several_sections_interleaves_each_heading_with_its_body() {
		let mut hierarchy = DocumentHierarchy::new();
		hierarchy.add_section(section(3, "One", "alpha", 5, 7));
		hierarchy.add_section(section(3, "Two", "beta", 8, 11));
		hierarchy.add_section(section(3, "Three", "gamma", 12, 30));

		let merged = hierarchy.merge_multiple_sections(&[0, 1, 2]);
		assert_eq!(merged.title, "### One");
		assert_eq!(
			merged.storage_content,
			"### One\n\nalpha\n\n### Two\n\nbeta\n\n### Three\n\ngamma"
		);
		assert_eq!(merged.level, 3);
		assert_eq!((merged.start_line, merged.end_line), (5, 30));
	}

	#[test]
	fn a_chunk_for_a_single_section_copies_its_body_verbatim() {
		let mut hierarchy = DocumentHierarchy::new();
		hierarchy.add_section(section(2, "Solo", "just this body", 4, 9));

		let chunk = hierarchy.create_chunk_for_section(0);
		assert_eq!(chunk.title, "## Solo");
		assert_eq!(chunk.storage_content, "just this body");
		assert_eq!((chunk.level, chunk.start_line, chunk.end_line), (2, 4, 9));
	}

	#[test]
	fn a_section_absorbs_only_its_still_unprocessed_children() {
		let mut hierarchy = DocumentHierarchy::new();
		hierarchy.add_section(section(1, "Parent", "parent body", 0, 2));
		hierarchy.add_section(section(2, "Kept", "kept body", 3, 5));
		hierarchy.add_section(section(2, "Taken", "taken body", 6, 12));
		hierarchy.sections[0].children = vec![1, 2];

		let merged = hierarchy.merge_section_with_children(0, &[false, false, true]);
		assert_eq!(
			merged.storage_content, "parent body\n\n## Kept\n\nkept body",
			"the already-processed child must not be pulled in again"
		);
		assert_eq!(
			(merged.start_line, merged.end_line),
			(0, 5),
			"the span covers only the absorbed children"
		);
	}

	#[test]
	fn marking_a_section_marks_its_whole_subtree() {
		let mut hierarchy = DocumentHierarchy::new();
		for i in 0..4 {
			hierarchy.add_section(section(1 + i, "H", "body", i, i));
		}
		hierarchy.sections[0].children = vec![1];
		hierarchy.sections[1].children = vec![2];
		// Index 3 is a separate root.
		let mut processed = vec![false; 4];

		hierarchy.mark_section_tree_processed(0, &mut processed);
		assert_eq!(processed, vec![true, true, true, false]);
	}

	#[test]
	fn the_best_child_merge_prefers_the_largest_group_that_still_fits() {
		let mut hierarchy = DocumentHierarchy::new();
		for i in 0..3 {
			hierarchy.add_section(section(2, "H", &"x".repeat(30), i, i));
		}

		let best = hierarchy.find_best_child_merge(&[0, 1, 2], 100);
		assert_eq!(best.indices, vec![0, 1, 2], "3 x 30 chars fits under 100");

		// With a tighter budget only two children fit.
		let best = hierarchy.find_best_child_merge(&[0, 1, 2], 65);
		assert_eq!(best.indices, vec![0, 1]);
	}

	#[test]
	fn the_best_child_merge_never_groups_more_than_four_children() {
		let mut hierarchy = DocumentHierarchy::new();
		for i in 0..6 {
			hierarchy.add_section(section(2, "H", "x", i, i));
		}

		let best = hierarchy.find_best_child_merge(&[0, 1, 2, 3, 4, 5], 10_000);
		assert_eq!(best.indices, vec![0, 1, 2, 3]);
	}

	#[test]
	fn a_child_too_large_to_merge_is_returned_on_its_own() {
		let mut hierarchy = DocumentHierarchy::new();
		hierarchy.add_section(section(2, "Huge", &"x".repeat(500), 0, 0));
		hierarchy.add_section(section(2, "Also huge", &"y".repeat(500), 1, 1));

		let best = hierarchy.find_best_child_merge(&[0, 1], 100);
		assert_eq!(best.indices, vec![0]);
		assert_eq!(best.efficiency, 0.0);
	}

	#[test]
	fn nearby_tiny_chunks_are_merged_and_span_both_line_ranges() {
		let hierarchy = DocumentHierarchy::new();
		let merged = hierarchy
			.try_merge_tiny_chunks(
				&chunk("## A", "alpha", 10, 12),
				&chunk("## B", "beta", 14, 18),
			)
			.expect("chunks 2 lines apart must merge");

		assert_eq!(merged.title, "## A");
		assert_eq!(merged.storage_content, "alpha\n\n## B\nbeta");
		assert_eq!((merged.start_line, merged.end_line), (10, 18));
	}

	#[test]
	fn tiny_chunks_emitted_out_of_document_order_still_get_a_forward_line_range() {
		// bottom_up_chunking emits the deepest level first, so `first` can sit
		// after `second` in the document. The merged range must not invert.
		let hierarchy = DocumentHierarchy::new();
		let merged = hierarchy
			.try_merge_tiny_chunks(
				&chunk("## A", "alpha", 30, 34),
				&chunk("## B", "beta", 29, 31),
			)
			.expect("chunks 5 lines apart or closer must merge");
		assert_eq!((merged.start_line, merged.end_line), (29, 34));
		assert!(merged.start_line <= merged.end_line);
	}

	#[test]
	fn tiny_chunks_far_apart_in_the_document_are_left_alone() {
		let hierarchy = DocumentHierarchy::new();
		assert!(hierarchy
			.try_merge_tiny_chunks(
				&chunk("## A", "alpha", 10, 12),
				&chunk("## B", "beta", 18, 20)
			)
			.is_none());
	}

	#[test]
	fn a_trailing_tiny_chunk_is_folded_into_the_one_before_it() {
		let hierarchy = DocumentHierarchy::new();
		// base_chunk_size 40 => anything under 10 chars counts as tiny.
		let chunks = vec![
			chunk("## Big", &"x".repeat(50), 0, 5),
			chunk("## Also big", &"y".repeat(50), 6, 11),
			chunk("## Tiny", "z", 12, 13),
		];

		let result = hierarchy.post_process_tiny_chunks(chunks, 40);
		assert_eq!(result.len(), 2, "the trailing tiny chunk is absorbed");
		assert!(result[1].storage_content.ends_with("## Tiny\nz"));
		assert_eq!((result[1].start_line, result[1].end_line), (6, 13));
	}

	#[test]
	fn chunks_that_are_all_large_enough_pass_through_untouched() {
		let hierarchy = DocumentHierarchy::new();
		let chunks = vec![
			chunk("## A", &"x".repeat(50), 0, 5),
			chunk("## B", &"y".repeat(50), 6, 11),
		];

		let result = hierarchy.post_process_tiny_chunks(chunks.clone(), 40);
		assert_eq!(result.len(), 2);
		assert_eq!(result[0].storage_content, chunks[0].storage_content);
		assert_eq!(result[1].storage_content, chunks[1].storage_content);
	}

	#[test]
	fn splitting_an_oversized_leaf_shifts_line_numbers_by_the_section_start() {
		let mut hierarchy = DocumentHierarchy::new();
		let body: String = (0..40)
			.map(|i| format!("line {i} of the body"))
			.collect::<Vec<_>>()
			.join("\n");
		hierarchy.add_section(section(2, "Leaf", &body, 100, 141));

		let pieces = hierarchy.split_oversized_leaf_section(0, 10, 0);
		assert!(pieces.len() > 1, "an oversized leaf must be split");
		assert!(pieces.iter().all(|p| p.title == "## Leaf" && p.level == 2));
		assert!(
			pieces.iter().all(|p| p.start_line >= 100),
			"line numbers are offset by the section's own start line"
		);
		assert!(pieces
			.iter()
			.all(|p| p.end_line >= p.start_line && !p.storage_content.is_empty()));
	}

	#[test]
	fn an_out_of_range_section_index_has_no_next_sibling() {
		let hierarchy = DocumentHierarchy::new();
		assert_eq!(hierarchy.find_next_sibling(0), None);
		assert_eq!(hierarchy.find_next_sibling(99), None);
	}

	#[test]
	fn a_deeper_heading_does_not_end_the_search_for_a_next_sibling() {
		let mut hierarchy = DocumentHierarchy::new();
		hierarchy.add_section(section(2, "First", "a", 0, 0));
		hierarchy.add_section(section(3, "Nested", "b", 1, 1));
		hierarchy.add_section(section(2, "Second", "c", 2, 2));
		hierarchy.add_section(section(1, "Top", "d", 3, 3));
		hierarchy.add_section(section(2, "After top", "e", 4, 4));

		assert_eq!(hierarchy.find_next_sibling(0), Some(2));
		// A shallower heading (index 3) ends the scan before index 4.
		assert_eq!(hierarchy.find_next_sibling(2), None);
	}
}
