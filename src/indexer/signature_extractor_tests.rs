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
	use tempfile::TempDir;

	fn parse(lang: &str, source: &str) -> (tree_sitter::Tree, Box<dyn languages::Language>) {
		let lang_impl = languages::get_language(lang).expect("language");
		let mut parser = Parser::new();
		parser
			.set_language(&lang_impl.get_ts_language())
			.expect("grammar");
		let tree = parser.parse(source, None).expect("parse");
		(tree, lang_impl)
	}

	fn names(sigs: &[SignatureItem]) -> Vec<&str> {
		sigs.iter().map(|s| s.name.as_str()).collect()
	}

	// --- detect_language -------------------------------------------------

	#[test]
	fn language_comes_from_the_file_extension_and_unknown_ones_are_none() {
		assert_eq!(detect_language(Path::new("a/b/lib.rs")), Some("rust"));
		assert_eq!(detect_language(Path::new("main.py")), Some("python"));
		assert_eq!(detect_language(Path::new("README.md")), Some("markdown"));
		assert_eq!(detect_language(Path::new("notes.zzz")), None);
		assert_eq!(detect_language(Path::new("no_extension")), None);
	}

	// --- map_node_kind_to_simple -----------------------------------------

	#[test]
	fn node_kinds_collapse_to_a_small_display_vocabulary() {
		for (kind, expected) in [
			("function_definition", "function"),
			("method_declaration", "method"),
			("class_specifier", "class"),
			("struct_item", "struct"),
			("enum_declaration", "enum"),
			("interface_declaration", "interface"),
			("trait_item", "trait"),
			("mod_item", "module"),
			("module_definition", "module"),
			("const_item", "constant"),
			("macro_definition", "macro"),
			("type_alias_declaration", "type"),
		] {
			assert_eq!(map_node_kind_to_simple(kind), expected, "kind {kind}");
		}
	}

	#[test]
	fn an_unrecognised_node_kind_is_passed_through_unchanged() {
		assert_eq!(map_node_kind_to_simple("declaration"), "declaration");
		assert_eq!(map_node_kind_to_simple(""), "");
	}

	#[test]
	fn the_first_matching_arm_wins_when_a_kind_names_two_concepts() {
		// "function" is tested before "method", so a kind naming both maps to
		// function. Locked down because the arm order is the only thing
		// deciding it.
		assert_eq!(map_node_kind_to_simple("method_function"), "function");
	}

	#[test]
	fn a_cpp_namespace_maps_to_namespace_rather_than_its_raw_kind() {
		let source = "namespace app { int x; }\n";
		let (tree, _) = parse("cpp", source);
		let namespace = tree.root_node().child(0).expect("namespace node");
		assert_eq!(namespace.kind(), "namespace_definition");
		assert_eq!(
			map_node_kind_to_simple_with_context(namespace, source),
			"namespace"
		);
	}

	// --- clean_comment_text ----------------------------------------------

	#[test]
	fn comment_markers_are_stripped_for_every_comment_flavour() {
		assert_eq!(clean_comment_text("/// doc"), "doc");
		assert_eq!(clean_comment_text("//! inner doc"), "inner doc");
		// A space before the bang means it is prose, not a doc marker.
		assert_eq!(clean_comment_text("// !note"), "!note");
		assert_eq!(clean_comment_text("   // padded   "), "padded");
		assert_eq!(clean_comment_text("/* one liner */"), "one liner");
		assert_eq!(clean_comment_text("# shell style"), "# shell style");
		assert_eq!(clean_comment_text(""), "");
	}

	#[test]
	fn a_block_comment_without_leading_stars_keeps_its_lines() {
		assert_eq!(clean_comment_text("/*\nfirst\nsecond\n*/"), "first\nsecond");
	}

	// --- node_text --------------------------------------------------------

	#[test]
	fn node_text_returns_the_exact_source_span() {
		let source = "fn alpha() -> u8 { 1 }\n";
		let (tree, _) = parse("rust", source);
		let function = tree.root_node().child(0).expect("fn node");
		assert_eq!(node_text(function, source), "fn alpha() -> u8 { 1 }");
	}

	// --- contains_function_declarator -------------------------------------

	#[test]
	fn a_reference_returning_declaration_still_finds_its_function_declarator() {
		let source = "int& make();\n";
		let (tree, _) = parse("cpp", source);
		let declaration = tree.root_node().child(0).expect("declaration");
		assert!(contains_function_declarator(declaration));
	}

	#[test]
	fn a_declaration_with_no_declarator_at_any_depth_reports_false() {
		let source = "int counter = 0;\n";
		let (tree, _) = parse("cpp", source);
		let declaration = tree.root_node().child(0).expect("declaration");
		assert!(!contains_function_declarator(declaration));
	}

	// --- extract_name -----------------------------------------------------

	#[test]
	fn a_declaration_name_comes_from_the_language_implementation() {
		let source = "struct Point { x: u8 }\n";
		let (tree, lang_impl) = parse("rust", source);
		let item = tree.root_node().child(0).expect("struct");
		assert_eq!(
			extract_name(item, source, lang_impl.as_ref()).as_deref(),
			Some("Point")
		);
	}

	// --- extract_preceding_comment / extract_file_comment -----------------

	#[test]
	fn a_node_with_no_comment_above_it_has_no_description() {
		let source = "fn alone() {}\n";
		let (tree, lang_impl) = parse("rust", source);
		let sigs = extract_signatures(tree.root_node(), source, lang_impl.as_ref());
		let alone = sigs.iter().find(|s| s.name == "alone").expect("alone");
		assert_eq!(alone.description, None);
	}

	#[test]
	fn a_non_comment_sibling_between_the_doc_and_the_node_breaks_the_link() {
		let source = "// about the constant\nconst A: u8 = 1;\nfn after() {}\n";
		let (tree, lang_impl) = parse("rust", source);
		let sigs = extract_signatures(tree.root_node(), source, lang_impl.as_ref());
		let after = sigs.iter().find(|s| s.name == "after").expect("after");
		assert_eq!(after.description, None);
	}

	#[test]
	fn a_file_whose_first_item_is_code_has_no_file_comment() {
		let source = "fn first() {}\n// trailing note\n";
		let (tree, _) = parse("rust", source);
		assert_eq!(extract_file_comment(tree.root_node(), source), None);
	}

	#[test]
	fn an_empty_file_has_no_file_comment() {
		let source = "";
		let (tree, _) = parse("rust", source);
		assert_eq!(extract_file_comment(tree.root_node(), source), None);
	}

	#[test]
	fn a_blank_line_gap_larger_than_one_ends_the_file_comment_block() {
		let source = "// header\n\n\n// unrelated\nfn foo() {}\n";
		let (tree, _) = parse("rust", source);
		assert_eq!(
			extract_file_comment(tree.root_node(), source).as_deref(),
			Some("header")
		);
	}

	// --- extract_markdown_signatures --------------------------------------

	#[test]
	fn each_heading_becomes_a_signature_carrying_its_level_and_body() {
		let markdown = "# Title\n\nIntro text.\n\n## Section\n\nBody text.\n";
		let sigs = extract_markdown_signatures(markdown);
		assert_eq!(names(&sigs), ["Title", "Section"]);
		assert_eq!(sigs[0].kind, "heading1");
		assert_eq!(sigs[1].kind, "heading2");
		assert_eq!(sigs[0].signature, "# Title\n\nIntro text.");
		assert_eq!(sigs[0].start_line, 0);
		// Trailing blank lines are trimmed off the captured body.
		assert_eq!(sigs[0].end_line, 2);
		assert_eq!(sigs[1].signature, "## Section\n\nBody text.");
	}

	#[test]
	fn a_heading_with_no_text_after_the_hashes_is_skipped() {
		let sigs = extract_markdown_signatures("#\n\n###   \n\n# Real\n");
		assert_eq!(names(&sigs), ["Real"]);
	}

	#[test]
	fn a_heading_body_stops_after_twenty_captured_lines() {
		let body: String = (1..=40).map(|i| format!("line {i}\n")).collect();
		let sigs = extract_markdown_signatures(&format!("# Long\n{body}"));
		assert_eq!(sigs.len(), 1);
		assert_eq!(sigs[0].signature.lines().count(), 20);
		assert_eq!(sigs[0].end_line, 19);
	}

	#[test]
	fn a_tilde_fence_hides_its_hash_lines_just_like_a_backtick_fence() {
		let markdown = "# Heading\n\n~~~\n# not a heading\n~~~\n\n## After\n";
		assert_eq!(
			names(&extract_markdown_signatures(markdown)),
			["Heading", "After"]
		);
	}

	#[test]
	fn markdown_with_no_headings_yields_no_signatures() {
		assert!(extract_markdown_signatures("just prose\nand more\n").is_empty());
		assert!(extract_markdown_signatures("").is_empty());
	}

	// --- extract_markdown_file_comment ------------------------------------

	#[test]
	fn yaml_frontmatter_becomes_the_file_comment_verbatim() {
		let markdown = "---\ntitle: Guide\nauthor: nobody\n---\n\n# Heading\n";
		assert_eq!(
			extract_markdown_file_comment(markdown).as_deref(),
			Some("title: Guide\nauthor: nobody")
		);
	}

	#[test]
	fn empty_frontmatter_falls_through_to_the_first_paragraph() {
		let markdown = "---\n---\n\nThe intro paragraph.\n";
		assert_eq!(
			extract_markdown_file_comment(markdown).as_deref(),
			Some("The intro paragraph.")
		);
	}

	#[test]
	fn the_first_paragraph_after_a_heading_is_the_file_comment() {
		let markdown = "# Title\n\nFirst line.\nSecond line.\n\nLater paragraph.\n";
		assert_eq!(
			extract_markdown_file_comment(markdown).as_deref(),
			Some("First line. Second line.")
		);
	}

	#[test]
	fn the_file_comment_paragraph_is_capped_at_three_lines() {
		let markdown = "one\ntwo\nthree\nfour\n";
		assert_eq!(
			extract_markdown_file_comment(markdown).as_deref(),
			Some("one two three")
		);
	}

	#[test]
	fn a_heading_only_document_has_no_file_comment() {
		assert_eq!(extract_markdown_file_comment("# Only\n## Headings\n"), None);
		assert_eq!(extract_markdown_file_comment(""), None);
	}

	// --- extract_file_signatures ------------------------------------------

	#[test]
	fn signatures_are_reported_relative_to_the_supplied_base_dir() {
		let dir = TempDir::new().expect("tempdir");
		let nested = dir.path().join("src/inner");
		std::fs::create_dir_all(&nested).expect("mkdir");
		let file = nested.join("lib.rs");
		std::fs::write(&file, "//! Crate docs\n\npub fn helper() -> u8 {\n\t1\n}\n")
			.expect("write");

		let out = extract_file_signatures(&[file], dir.path()).expect("extract");
		assert_eq!(out.len(), 1);
		// Compared as a `Path`: the reported string keeps the platform's own
		// separator, so Windows yields `src\inner\lib.rs` for the same file.
		assert_eq!(Path::new(&out[0].path), Path::new("src/inner/lib.rs"));
		assert_eq!(out[0].language, "rust");
		assert_eq!(out[0].file_comment.as_deref(), Some("Crate docs"));
		assert_eq!(names(&out[0].signatures), ["helper"]);
	}

	#[test]
	fn a_markdown_file_is_summarised_by_its_headings_not_by_tree_sitter() {
		let dir = TempDir::new().expect("tempdir");
		let file = dir.path().join("GUIDE.md");
		std::fs::write(&file, "Intro paragraph.\n\n# Install\n\nRun it.\n").expect("write");

		let out = extract_file_signatures(&[file], dir.path()).expect("extract");
		assert_eq!(out[0].language, "markdown");
		assert_eq!(out[0].file_comment.as_deref(), Some("Intro paragraph."));
		assert_eq!(names(&out[0].signatures), ["Install"]);
	}

	#[test]
	fn files_with_an_unknown_extension_or_a_missing_path_are_skipped() {
		let dir = TempDir::new().expect("tempdir");
		let unknown = dir.path().join("data.zzz");
		std::fs::write(&unknown, "irrelevant").expect("write");
		let missing = dir.path().join("gone.rs");

		let out = extract_file_signatures(&[unknown, missing], dir.path()).expect("extract");
		assert!(out.is_empty());
	}

	#[test]
	fn a_base_dir_the_file_is_not_under_falls_back_to_the_bare_file_name() {
		let dir = TempDir::new().expect("tempdir");
		let file = dir.path().join("lonely.rs");
		std::fs::write(&file, "pub fn solo() {}\n").expect("write");

		let elsewhere = TempDir::new().expect("tempdir2");
		let out = extract_file_signatures(&[file], elsewhere.path()).expect("extract");
		assert_eq!(out[0].path, "lonely.rs");
	}
}
