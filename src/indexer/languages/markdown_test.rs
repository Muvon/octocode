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

use crate::indexer::languages::markdown::Markdown;
use crate::indexer::languages::Language;
use tree_sitter::{Parser, Tree};

/// Markdown has no grammar of its own — it borrows the JSON one as a
/// placeholder and does all its work on the raw text, so any parseable input
/// gives a usable root node.
fn tree() -> Tree {
	let mut parser = Parser::new();
	parser
		.set_language(&Markdown.get_ts_language())
		.expect("json grammar");
	parser.parse("{}", None).expect("parse")
}

#[test]
fn the_language_identifies_itself_and_the_extensions_it_owns() {
	let md = Markdown;
	assert_eq!(md.name(), "markdown");
	assert_eq!(md.get_file_extensions(), vec!["md", "markdown"]);
	assert_eq!(
		md.get_node_type_description("anything"),
		"markdown headings"
	);
}

#[test]
fn no_node_kind_is_meaningful_because_parsing_is_text_based() {
	assert!(Markdown.get_meaningful_kinds().is_empty());
}

#[test]
fn every_non_empty_heading_becomes_a_symbol_at_any_level() {
	let tree = tree();
	let contents = "# Top\n\nprose\n\n### Deep Heading\n#\n#    \n## Trailing #\n";
	assert_eq!(
		Markdown.extract_symbols(tree.root_node(), contents),
		vec!["Top", "Deep Heading", "Trailing #"]
	);
}

#[test]
fn a_document_without_headings_has_no_symbols() {
	let tree = tree();
	assert_eq!(
		Markdown.extract_symbols(tree.root_node(), "just prose\n"),
		Vec::<String>::new()
	);
}

#[test]
fn identifier_extraction_is_a_no_op_for_markdown() {
	let tree = tree();
	let mut symbols = vec!["pre-existing".to_string()];
	Markdown.extract_identifiers(tree.root_node(), "# Heading\n", &mut symbols);
	assert_eq!(symbols, vec!["pre-existing"]);
}

#[test]
fn the_root_node_reports_links_as_imports_and_headings_as_exports() {
	let tree = tree();
	let contents = "# Guide\n\nSee [Other](./other.md).\n\n## Details\n";
	let (imports, exports) = Markdown.extract_imports_exports(tree.root_node(), contents);
	assert_eq!(imports, vec!["./other.md"]);
	assert_eq!(exports, vec!["Guide", "Details"]);
}

#[test]
fn a_non_root_node_reports_nothing_so_the_walk_does_not_duplicate_links() {
	let tree = tree();
	let child = tree.root_node().child(0).expect("a child of the object");
	assert!(child.parent().is_some());
	let contents = "# Guide\n\nSee [Other](./other.md).\n";
	assert_eq!(
		Markdown.extract_imports_exports(child, contents),
		(Vec::new(), Vec::new())
	);
}

#[test]
fn an_import_that_escapes_above_the_repository_root_resolves_to_nothing() {
	let files = vec!["docs/guide.md".to_string()];
	let registry = crate::indexer::languages::resolution_utils::FileRegistry::new(&files);
	assert_eq!(
		Markdown.resolve_import("../../outside.md", "docs/guide.md", &registry),
		None
	);
}
