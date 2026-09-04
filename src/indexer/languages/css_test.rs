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
mod css_tests {
	use crate::indexer::code_region_extractor::{extract_meaningful_regions, CodeRegion};
	use crate::indexer::languages::css::Css;
	use crate::indexer::languages::resolution_utils::FileRegistry;
	use crate::indexer::languages::{self, Language};
	use tree_sitter::{Node, Parser, Tree};

	fn parse_regions(source: &str) -> Vec<CodeRegion> {
		let lang = languages::get_language("css").expect("CSS language not registered");
		let mut parser = Parser::new();
		parser.set_language(&lang.get_ts_language()).unwrap();
		let tree = parser.parse(source, None).unwrap();
		let mut regions = Vec::new();
		extract_meaningful_regions(tree.root_node(), source, lang.as_ref(), &mut regions);
		regions
	}

	#[test]
	fn test_media_block_splits_into_individual_rules() {
		// Non-trivial content so the smart single-line merge pass doesn't recombine them.
		let source = r#"@media (max-width: 768px) {
	.a {
		color: red;
		background: white;
		font-size: 14px;
		margin: 0;
	}
	.b {
		color: blue;
		background: black;
		font-size: 16px;
		padding: 0;
	}
}
"#;
		let regions = parse_regions(source);

		let media_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "media_statement")
			.collect();
		assert_eq!(
			media_regions.len(),
			0,
			"media block with rules inside should not collapse into one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		let rule_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "rule_set")
			.collect();
		assert_eq!(
			rule_regions.len(),
			2,
			"expected a region per rule inside @media, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}

	#[test]
	fn test_font_face_stays_single_region() {
		let source = r#"@font-face {
	font-family: "X";
	src: url(x.woff);
}
"#;
		let regions = parse_regions(source);

		let at_rule_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "at_rule")
			.collect();
		assert_eq!(
			at_rule_regions.len(),
			1,
			"leaf at-rule with only declarations should remain its own single region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}

	fn parse(source: &str) -> Tree {
		let mut parser = Parser::new();
		parser
			.set_language(&Css {}.get_ts_language())
			.expect("css grammar");
		parser.parse(source, None).expect("parse")
	}

	/// Depth-first walk collecting every node of `kind`, in document order.
	fn nodes_of_kind<'a>(node: Node<'a>, kind: &str, out: &mut Vec<Node<'a>>) {
		if node.kind() == kind {
			out.push(node);
		}
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			nodes_of_kind(child, kind, out);
		}
	}

	fn first_node<'a>(tree: &'a Tree, kind: &str) -> Node<'a> {
		let mut found = Vec::new();
		nodes_of_kind(tree.root_node(), kind, &mut found);
		*found
			.first()
			.unwrap_or_else(|| panic!("no {kind} node in source"))
	}

	fn registry(files: &[&str]) -> FileRegistry {
		let owned: Vec<String> = files.iter().map(|f| f.to_string()).collect();
		FileRegistry::new(&owned)
	}

	/// Paths under `demo/` (plus a root-level `vendor.css` for the exact-match
	/// branch) that do not exist on disk, so resolution is decided purely by
	/// the registry contents.
	fn demo_styles() -> FileRegistry {
		registry(&[
			"demo/styles/main.css",
			"demo/styles/base.css",
			"demo/shared/reset.scss",
			"vendor.css",
		])
	}

	#[test]
	fn the_parser_is_named_css_and_claims_every_stylesheet_extension() {
		let lang = Css {};
		assert_eq!(lang.name(), "css");
		assert_eq!(lang.get_file_extensions(), vec!["css", "scss", "sass"]);
		// The same implementation is what the registry hands out for "css".
		assert_eq!(languages::get_language("css").unwrap().name(), "css");
	}

	#[test]
	fn nesting_at_rules_are_descended_first_while_keyframes_are_chunked_whole() {
		let lang = Css {};
		let kinds = lang.get_meaningful_kinds();
		assert_eq!(
			kinds,
			vec![
				"rule_set",
				"at_rule",
				"keyframes_statement",
				"media_statement",
				"import_statement",
			]
		);
		// CSS does not override get_symbol_kinds, so both tiers are identical.
		assert_eq!(lang.get_symbol_kinds(), kinds);
		assert_eq!(
			lang.descend_first_kinds(),
			vec!["at_rule", "media_statement"]
		);
		assert!(!lang.descend_first_kinds().contains(&"keyframes_statement"));
	}

	#[test]
	fn node_type_descriptions_cover_the_mapped_kinds_and_fall_back() {
		let lang = Css {};
		for (kind, description) in [
			("rule_set", "CSS rules"),
			("at_rule", "at-rule declarations"),
			("keyframes_statement", "at-rule declarations"),
			("media_statement", "at-rule declarations"),
			("import_statement", "at-rule declarations"),
			("selector", "CSS selectors"),
			("selectors", "CSS selectors"),
			("class_selector", "CSS selectors"),
			("id_selector", "CSS selectors"),
			("declaration", "CSS declarations"),
		] {
			assert_eq!(
				lang.get_node_type_description(kind),
				description,
				"description for {kind}"
			);
		}
	}

	#[test]
	fn node_types_are_equivalent_only_within_their_semantic_group() {
		let lang = Css {};
		assert!(lang.are_node_types_equivalent("rule_set", "selectors"));
		assert!(lang.are_node_types_equivalent("at_rule", "media_statement"));
		assert!(lang.are_node_types_equivalent("import_statement", "supports_statement"));
		assert!(lang.are_node_types_equivalent("class_selector", "id_selector"));
		assert!(lang.are_node_types_equivalent("declaration", "declaration"));

		assert!(!lang.are_node_types_equivalent("rule_set", "at_rule"));
		assert!(!lang.are_node_types_equivalent("tag_name", "rule_set"));
	}

	#[test]
	fn a_rule_set_reports_its_selectors_as_symbols() {
		for (source, expected) in [
			(".card { color: red; }\n", vec![".card"]),
			("#main { color: red; }\n", vec!["#main"]),
			("span { color: red; }\n", vec!["span"]),
			("* { color: red; }\n", vec!["*"]),
			(".a .b { color: red; }\n", vec![".a", ".b"]),
			// The grammar nests the class selector inside the id selector, so the
			// combined text is what gets reported.
			(".card#main { color: red; }\n", vec![".card#main"]),
		] {
			let tree = parse(source);
			let node = first_node(&tree, "rule_set");
			assert_eq!(
				Css {}.extract_symbols(node, source),
				expected,
				"symbols for {source:?}"
			);
		}
	}

	#[test]
	fn a_pseudo_selector_reports_both_itself_and_the_selector_it_decorates() {
		// The pseudo name is a `class_name` and the decorated selector is nested
		// inside the pseudo node, so both levels have to be walked.
		let source = ".card:hover { color: red; }\n";
		let tree = parse(source);
		let node = first_node(&tree, "rule_set");
		let symbols = Css {}.extract_symbols(node, source);
		assert!(symbols.contains(&":hover".to_string()), "{symbols:?}");
		assert!(symbols.contains(&".card".to_string()), "{symbols:?}");
	}

	#[test]
	fn at_rules_only_report_a_keyframes_name() {
		for (source, kind, expected) in [
			(
				"@keyframes slide { from { left: 0; } }\n",
				"keyframes_statement",
				vec!["slide"],
			),
			("@font-face { font-family: \"X\"; }\n", "at_rule", vec![]),
			(
				"@media screen { .a { color: red; } }\n",
				"media_statement",
				vec![],
			),
			("@import \"base.css\";\n", "import_statement", vec![]),
		] {
			let tree = parse(source);
			let node = first_node(&tree, kind);
			assert_eq!(
				Css {}.extract_symbols(node, source),
				expected,
				"symbols for {source:?}"
			);
		}
	}

	#[test]
	fn identifier_extraction_collects_selector_and_property_names() {
		let source = ".card { color: red; }\n";
		let tree = parse(source);
		let node = first_node(&tree, "rule_set");
		let mut symbols = Vec::new();
		Css {}.extract_identifiers(node, source, &mut symbols);
		// `class_name` matches before its own `identifier` child, which is then
		// skipped as a duplicate; values are never collected.
		assert_eq!(symbols, vec!["card", "color"]);
	}

	#[test]
	fn every_at_import_spelling_yields_the_imported_path() {
		for (source, expected) in [
			("@import \"base.css\";\n", "base.css"),
			("@import 'base.css';\n", "base.css"),
			("@import url(theme.css);\n", "theme.css"),
			("@import url(\"vendor/reset.css\");\n", "vendor/reset.css"),
		] {
			let tree = parse(source);
			let node = first_node(&tree, "import_statement");
			let (imports, exports) = Css {}.extract_imports_exports(node, source);
			assert_eq!(imports, vec![expected], "imports for {source:?}");
			assert!(exports.is_empty(), "CSS declares no exports");
		}
	}

	#[test]
	fn a_rule_set_is_neither_an_import_nor_an_export() {
		let source = ".card { color: red; }\n";
		let tree = parse(source);
		let node = first_node(&tree, "rule_set");
		let (imports, exports) = Css {}.extract_imports_exports(node, source);
		assert!(imports.is_empty());
		assert!(exports.is_empty());
	}

	#[test]
	fn a_relative_import_resolves_against_the_importing_stylesheet() {
		let files = demo_styles();
		assert_eq!(
			Css {}.resolve_import("./base.css", "demo/styles/main.css", &files),
			Some("demo/styles/base.css".to_string())
		);
	}

	#[test]
	fn an_extensionless_relative_import_tries_each_stylesheet_extension() {
		let files = demo_styles();
		assert_eq!(
			Css {}.resolve_import("../shared/reset", "demo/styles/main.css", &files),
			Some("demo/shared/reset.scss".to_string())
		);
	}

	#[test]
	fn a_bare_filename_resolves_next_to_the_source_before_falling_back() {
		let files = demo_styles();
		let lang = Css {};
		assert_eq!(
			lang.resolve_import("base.css", "demo/styles/main.css", &files),
			Some("demo/styles/base.css".to_string())
		);
		// Not a sibling, so the project-wide exact match is used instead.
		assert_eq!(
			lang.resolve_import("vendor.css", "demo/styles/main.css", &files),
			Some("vendor.css".to_string())
		);
	}

	#[test]
	fn an_import_of_a_missing_stylesheet_does_not_resolve() {
		let files = demo_styles();
		let lang = Css {};
		assert_eq!(
			lang.resolve_import("./missing.css", "demo/styles/main.css", &files),
			None
		);
		assert_eq!(
			lang.resolve_import("missing.css", "demo/styles/main.css", &files),
			None
		);
	}

	#[test]
	fn a_supports_block_is_transparent_and_only_its_rules_are_chunked() {
		// `supports_statement` is not a meaningful kind at all, so the walk
		// descends through it without ever considering it as a region.
		let source = "@supports (display: grid) {\n\t.grid {\n\t\tdisplay: grid;\n\t\tgap: 4px;\n\t\tmargin: 0;\n\t\tpadding: 0;\n\t}\n}\n";
		let regions = parse_regions(source);

		assert_eq!(
			regions.len(),
			1,
			"got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
		assert_eq!(regions[0].node_kind, "rule_set");
		assert_eq!(regions[0].symbols, vec![".grid"]);
	}

	#[test]
	fn a_charset_statement_produces_no_region_at_all() {
		let regions = parse_regions("@charset \"utf-8\";\n");
		assert!(
			regions.is_empty(),
			"got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}

	#[test]
	fn a_keyframes_block_stays_a_single_region_named_after_the_animation() {
		let source = "@keyframes slide {\n\tfrom {\n\t\tleft: 0;\n\t\ttop: 0;\n\t}\n\tto {\n\t\tleft: 10px;\n\t\ttop: 10px;\n\t}\n}\n";
		let regions = parse_regions(source);

		assert_eq!(
			regions.len(),
			1,
			"got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
		assert_eq!(regions[0].node_kind, "keyframes_statement");
		assert_eq!(regions[0].symbols, vec!["slide"]);
	}

	#[test]
	fn consecutive_import_statements_merge_into_one_labelled_region() {
		let source = "@import \"a.css\";\n@import \"b.css\";\n@import \"c.css\";\n";
		let regions = parse_regions(source);

		assert_eq!(
			regions.len(),
			1,
			"got {:?}",
			regions.iter().map(|r| &r.content).collect::<Vec<_>>()
		);
		assert_eq!(regions[0].node_kind, "import_statement");
		assert!(
			regions[0]
				.content
				.starts_with("// Merged at-rule declarations (3 declarations)\n"),
			"got: {:?}",
			regions[0].content
		);
		// An import statement declares no symbol, so the extractor synthesises
		// one per region from the node kind and start line.
		assert_eq!(
			regions[0].symbols,
			vec![
				"import_statement_0",
				"import_statement_1",
				"import_statement_2",
			]
		);
	}
}
