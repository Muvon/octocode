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
}
