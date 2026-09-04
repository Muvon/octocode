use crate::indexer::code_region_extractor::extract_meaningful_regions;
use crate::indexer::languages::resolution_utils::FileRegistry;
use crate::indexer::languages::rust::Rust;
use crate::indexer::languages::{CallTarget, Language, TypeRelationKind};
use tree_sitter::{Node, Parser, Tree};

fn parse_regions(source: &str) -> Vec<crate::indexer::code_region_extractor::CodeRegion> {
	let rust_lang = Rust {};
	let mut parser = Parser::new();
	parser.set_language(&rust_lang.get_ts_language()).unwrap();

	let tree = parser.parse(source, None).unwrap();
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), source, &rust_lang, &mut regions);
	regions
}

#[test]
fn test_inline_mod_splits_into_individual_functions() {
	// Bodies are multi-line/non-trivial so the smart single-line merge pass
	// (unrelated to this fix) doesn't recombine the two functions into one
	// "// Merged ..." block, which would defeat this test's purpose.
	let source = r#"
mod foo {
	fn a() {
		let x = 1;
		let y = 2;
		x + y
	}
	fn b() {
		let x = 3;
		let y = 4;
		x + y
	}
}
"#;

	let regions = parse_regions(source);

	let function_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "function_item")
		.collect();
	assert_eq!(
		function_regions.len(),
		2,
		"expected 2 function_item regions inside the mod, got {} (regions: {:?})",
		function_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);

	let mod_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "mod_item")
		.collect();
	assert!(
		mod_regions.is_empty(),
		"mod body should not collapse into a single mod_item blob region, got: {:?}",
		mod_regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
}

#[test]
fn test_bodyless_mod_still_produces_its_own_region() {
	let source = "mod foo;\n";

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"a bodyless mod should still produce exactly one fallback region, got {} (regions: {:?})",
		regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "mod_item");
	assert!(regions[0].content.contains("mod foo;"));
}

#[test]
fn test_trait_with_default_method_splits_into_method_region() {
	let source = r#"
trait Greeter {
	fn greet(&self) {
		println!("hi");
	}
}
"#;

	let regions = parse_regions(source);

	let function_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "function_item")
		.collect();
	assert_eq!(
		function_regions.len(),
		1,
		"expected the default method to become its own function_item region, got {} (regions: {:?})",
		function_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);

	let trait_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "trait_item")
		.collect();
	assert!(
		trait_regions.is_empty(),
		"trait with a default method should not also collapse into a whole-trait region, got: {:?}",
		trait_regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
}

#[test]
fn test_signature_only_trait_still_produces_one_whole_trait_region() {
	let source = r#"
trait Greeter {
	fn greet(&self);
}
"#;

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"a signature-only trait should still produce exactly one fallback region, got {} (regions: {:?})",
		regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "trait_item");
	assert!(regions[0].content.contains("fn greet(&self);"));
}

#[test]
fn test_preceding_comment_attaches_to_following_function() {
	let source = r#"
// This function does the thing
fn foo() {
	let x = 1;
	let y = 2;
	x + y
}
"#;

	let regions = parse_regions(source);
	let function_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "function_item")
		.collect();
	assert_eq!(
		function_regions.len(),
		1,
		"expected exactly one function_item region, got {} (regions: {:?})",
		function_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert!(
		function_regions[0]
			.content
			.contains("// This function does the thing"),
		"preceding doc comment should be attached to the function region, got: {:?}",
		function_regions[0].content
	);
}

#[test]
fn test_function_without_preceding_comment_does_not_absorb_prior_sibling() {
	let source = r#"
fn bar() {
	let a = 10;
	let b = 20;
	a + b
}

fn baz() {
	let c = 30;
	let d = 40;
	c + d
}
"#;

	let regions = parse_regions(source);
	let function_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "function_item")
		.collect();
	assert_eq!(
		function_regions.len(),
		2,
		"expected 2 function_item regions, got {} (regions: {:?})",
		function_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);

	let baz_region = function_regions
		.iter()
		.find(|r| r.content.contains("fn baz()"))
		.expect("baz region should exist");
	assert!(
		!baz_region.content.contains("fn bar()"),
		"baz's region should not absorb the unrelated preceding sibling bar's content: {:?}",
		baz_region.content
	);
}

fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	parser
		.set_language(&Rust {}.get_ts_language())
		.expect("rust grammar");
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

/// A crate laid out under `demo/`, which does not exist on disk, so the
/// canonicalize fallbacks in `find_matching_file` / `find_crate_root` stay
/// inert and resolution is decided purely by the registry contents.
fn demo_crate() -> FileRegistry {
	registry(&[
		"demo/src/lib.rs",
		"demo/src/config.rs",
		"demo/src/config/features.rs",
		"demo/src/graph/mod.rs",
		"demo/src/graph/node.rs",
	])
}

#[test]
fn the_parser_is_named_rust_and_claims_the_rs_extension() {
	let lang = Rust {};
	assert_eq!(lang.name(), "rust");
	assert_eq!(lang.get_file_extensions(), vec!["rs"]);
}

#[test]
fn impl_blocks_are_excluded_from_chunking_and_symbol_kinds_mirror_them() {
	let lang = Rust {};
	let kinds = lang.get_meaningful_kinds();
	assert!(kinds.contains(&"function_item"));
	assert!(kinds.contains(&"macro_definition"));
	assert!(
		!kinds.contains(&"impl_item"),
		"impl blocks are deliberately not chunked as one region"
	);
	// Rust does not override get_symbol_kinds, so both tiers are identical.
	assert_eq!(lang.get_symbol_kinds(), kinds);
	assert_eq!(lang.descend_first_kinds(), vec!["mod_item", "trait_item"]);
}

#[test]
fn every_node_type_description_arm_has_a_rust_specific_wording() {
	let lang = Rust {};
	for (kind, description) in [
		("mod_item", "module declarations"),
		("use_declaration", "import statements"),
		("extern_crate_item", "import statements"),
		("struct_item", "type definitions"),
		("enum_item", "type definitions"),
		("union_item", "type definitions"),
		("type_item", "type declarations"),
		("function_item", "function declarations"),
		("const_item", "constant declarations"),
		("static_item", "constant declarations"),
		("trait_item", "trait declarations"),
		("impl_item", "implementation blocks"),
		("macro_definition", "macro definitions"),
		("macro_rules", "macro definitions"),
		("expression_statement", "declarations"),
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
	let lang = Rust {};
	assert!(lang.are_node_types_equivalent("mod_item", "use_declaration"));
	assert!(lang.are_node_types_equivalent("struct_item", "enum_item"));
	assert!(lang.are_node_types_equivalent("union_item", "type_item"));
	assert!(lang.are_node_types_equivalent("trait_item", "impl_item"));
	assert!(lang.are_node_types_equivalent("const_item", "static_item"));
	assert!(lang.are_node_types_equivalent("function_item", "function_item"));

	assert!(!lang.are_node_types_equivalent("function_item", "struct_item"));
	assert!(!lang.are_node_types_equivalent("mod_item", "const_item"));
	assert!(!lang.are_node_types_equivalent("trait_item", "macro_definition"));
}

#[test]
fn a_trait_impl_method_carries_both_the_receiver_type_and_the_trait() {
	let source = "impl Render for Widget {\n\tfn render(&self) {\n\t\tlet _ = 1;\n\t}\n}\n";
	let tree = parse(source);
	let node = first_node(&tree, "function_item");
	assert_eq!(
		Rust {}.extract_symbols(node, source),
		vec!["Render", "Widget", "render"]
	);
}

#[test]
fn an_inherent_impl_method_carries_only_the_receiver_type() {
	let source = "impl Widget {\n\tfn paint(&self) {\n\t\tlet _ = 1;\n\t}\n}\n";
	let tree = parse(source);
	let node = first_node(&tree, "function_item");
	assert_eq!(
		Rust {}.extract_symbols(node, source),
		vec!["Widget", "paint"]
	);
}

#[test]
fn a_free_function_has_only_its_own_name_as_a_symbol() {
	let source = "fn helper() {\n\tlet _ = 1;\n}\n";
	let tree = parse(source);
	let node = first_node(&tree, "function_item");
	assert_eq!(Rust {}.extract_symbols(node, source), vec!["helper"]);
}

#[test]
fn type_module_const_and_macro_declarations_yield_their_declared_name() {
	for (source, kind, expected) in [
		(
			"pub struct Widget { pub id: u32 }\n",
			"struct_item",
			"Widget",
		),
		("pub enum Color { Red }\n", "enum_item", "Color"),
		(
			"pub trait Render { fn go(&self); }\n",
			"trait_item",
			"Render",
		),
		("mod helpers;\n", "mod_item", "helpers"),
		("pub const LIMIT: usize = 10;\n", "const_item", "LIMIT"),
		(
			"macro_rules! shout { () => {} }\n",
			"macro_definition",
			"shout",
		),
	] {
		let tree = parse(source);
		let node = first_node(&tree, kind);
		assert_eq!(
			Rust {}.extract_symbols(node, source),
			vec![expected.to_string()],
			"symbols for {kind}"
		);
	}
}

#[test]
fn an_unhandled_node_kind_falls_back_to_identifier_extraction() {
	let source = "use crate::config::Config;\n";
	let tree = parse(source);
	let node = first_node(&tree, "use_declaration");
	let symbols = Rust {}.extract_symbols(node, source);
	// The fallback keeps whole scoped paths as well as their segments, because
	// `scoped_identifier` itself matches the "contains identifier" filter.
	assert!(symbols.contains(&"Config".to_string()), "{symbols:?}");
	assert!(symbols.contains(&"config".to_string()), "{symbols:?}");
	assert!(
		symbols.contains(&"crate::config::Config".to_string()),
		"{symbols:?}"
	);
}

#[test]
fn identifier_extraction_visits_every_identifier_shaped_node_once() {
	let source = "fn helper(alpha: u32) -> u32 {\n\talpha\n}\n";
	let tree = parse(source);
	let node = first_node(&tree, "function_item");
	let mut symbols = Vec::new();
	Rust {}.extract_identifiers(node, source, &mut symbols);
	assert_eq!(symbols, vec!["helper", "alpha"]);
}

#[test]
fn a_trait_impl_reports_an_implements_relation_owned_by_the_concrete_type() {
	let source = "impl Render for Widget {\n\tfn render(&self) {}\n}\n";
	let tree = parse(source);
	let node = first_node(&tree, "impl_item");
	let lang = Rust {};
	assert_eq!(
		lang.extract_type_relations(node, source),
		vec![(TypeRelationKind::Implements, "Render".to_string())]
	);
	assert_eq!(
		lang.extract_type_relation_source(node, source),
		Some("Widget".to_string())
	);
}

#[test]
fn an_inherent_impl_reports_no_type_relation() {
	let source = "impl Widget {\n\tfn paint(&self) {}\n}\n";
	let tree = parse(source);
	let node = first_node(&tree, "impl_item");
	let lang = Rust {};
	assert!(lang.extract_type_relations(node, source).is_empty());
	assert_eq!(
		lang.extract_type_relation_source(node, source),
		Some("Widget".to_string())
	);
}

#[test]
fn a_trait_with_supertraits_reports_one_extends_relation_per_bound() {
	let source = "trait Render: Draw + Clone {\n\tfn go(&self);\n}\n";
	let tree = parse(source);
	let node = first_node(&tree, "trait_item");
	let lang = Rust {};
	assert_eq!(
		lang.extract_type_relations(node, source),
		vec![
			(TypeRelationKind::Extends, "Draw".to_string()),
			(TypeRelationKind::Extends, "Clone".to_string()),
		]
	);
	assert_eq!(
		lang.extract_type_relation_source(node, source),
		Some("Render".to_string())
	);
}

#[test]
fn a_struct_declares_no_type_relations() {
	let source = "struct Widget { id: u32 }\n";
	let tree = parse(source);
	let node = first_node(&tree, "struct_item");
	assert!(Rust {}.extract_type_relations(node, source).is_empty());
}

#[test]
fn qualified_and_bare_calls_keep_their_terminal_name_and_qualifier() {
	let source = "fn go() {\n\thelper();\n\tWidget::new();\n\tself.paint();\n}\n";
	let tree = parse(source);
	let mut calls = Vec::new();
	nodes_of_kind(tree.root_node(), "call_expression", &mut calls);
	let lang = Rust {};
	let targets: Vec<CallTarget> = calls
		.iter()
		.flat_map(|node| lang.extract_function_calls(*node, source))
		.collect();
	assert_eq!(
		targets,
		vec![
			CallTarget {
				name: "helper".to_string(),
				qualifier: None,
			},
			CallTarget {
				name: "new".to_string(),
				qualifier: Some("Widget".to_string()),
			},
			CallTarget {
				name: "paint".to_string(),
				qualifier: Some("self".to_string()),
			},
		]
	);
}

#[test]
fn macro_invocations_are_reported_as_calls_without_the_bang() {
	let source = "fn go() {\n\tprintln!(\"x\");\n\tcrate::utils::debug_log!(\"y\");\n}\n";
	let tree = parse(source);
	let mut invocations = Vec::new();
	nodes_of_kind(tree.root_node(), "macro_invocation", &mut invocations);
	let lang = Rust {};
	let targets: Vec<CallTarget> = invocations
		.iter()
		.flat_map(|node| lang.extract_function_calls(*node, source))
		.collect();
	assert_eq!(
		targets,
		vec![
			CallTarget {
				name: "println".to_string(),
				qualifier: None,
			},
			CallTarget {
				name: "debug_log".to_string(),
				qualifier: Some("crate::utils".to_string()),
			},
		]
	);
}

#[test]
fn a_node_that_is_not_a_call_reports_no_calls() {
	let source = "fn go() {\n\tlet x = 1;\n}\n";
	let tree = parse(source);
	let node = first_node(&tree, "let_declaration");
	assert!(Rust {}.extract_function_calls(node, source).is_empty());
}

#[test]
fn use_statements_expand_into_one_fully_qualified_import_per_leaf() {
	for (source, expected) in [
		(
			"use crate::config::Config;\n",
			vec!["crate::config::Config"],
		),
		(
			"use crate::a::{B, c::D};\n",
			vec!["crate::a::B", "crate::a::c::D"],
		),
		(
			"use crate::a::{b::{C, D}, E};\n",
			vec!["crate::a::b::C", "crate::a::b::D", "crate::a::E"],
		),
		("use std::sync::Arc as StdArc;\n", vec!["std::sync::Arc"]),
		("use crate::utils::*;\n", vec!["crate::utils"]),
	] {
		let tree = parse(source);
		let node = first_node(&tree, "use_declaration");
		let (imports, exports) = Rust {}.extract_imports_exports(node, source);
		assert_eq!(imports, expected, "imports for {source:?}");
		assert!(exports.is_empty(), "exports for {source:?}");
	}
}

#[test]
fn a_pub_use_reexport_still_produces_an_import() {
	// The visibility modifier in front of `use` must be skipped, or the whole
	// re-export expands to nothing and creates no GraphRAG edge.
	for source in [
		"pub use crate::api::Client;\n",
		"pub(crate) use crate::api::Client;\n",
	] {
		let tree = parse(source);
		let node = first_node(&tree, "use_declaration");
		let (imports, _) = Rust {}.extract_imports_exports(node, source);
		assert_eq!(imports, vec!["crate::api::Client"], "for {source:?}");
	}
}

#[test]
fn a_bodyless_mod_becomes_a_self_relative_import_while_a_mod_body_does_not() {
	let source = "pub mod public_mod;\nmod private_mod;\nmod inner { fn a() { let _ = 1; } }\n";
	let tree = parse(source);
	let mut mods = Vec::new();
	nodes_of_kind(tree.root_node(), "mod_item", &mut mods);
	let lang = Rust {};

	let (imports, exports) = lang.extract_imports_exports(mods[0], source);
	assert_eq!(imports, vec!["self::public_mod"]);
	assert_eq!(exports, vec!["public_mod"]);

	let (imports, exports) = lang.extract_imports_exports(mods[1], source);
	assert_eq!(imports, vec!["self::private_mod"]);
	assert!(exports.is_empty());

	// A mod with a `declaration_list` body lives in this file, so it is not a
	// dependency on another one.
	let (imports, exports) = lang.extract_imports_exports(mods[2], source);
	assert!(imports.is_empty());
	assert!(exports.is_empty());
}

#[test]
fn only_pub_items_are_reported_as_exports() {
	for (source, kind, expected) in [
		(
			"pub struct Widget { id: u32 }\n",
			"struct_item",
			vec!["Widget"],
		),
		("pub enum Color { Red }\n", "enum_item", vec!["Color"]),
		(
			"pub trait Render { fn go(&self); }\n",
			"trait_item",
			vec!["Render"],
		),
		("pub fn go() {}\n", "function_item", vec!["go"]),
		("pub const LIMIT: usize = 1;\n", "const_item", vec!["LIMIT"]),
		("struct Hidden { id: u32 }\n", "struct_item", vec![]),
		("fn hidden() {}\n", "function_item", vec![]),
	] {
		let tree = parse(source);
		let node = first_node(&tree, kind);
		let (imports, exports) = Rust {}.extract_imports_exports(node, source);
		assert!(imports.is_empty(), "imports for {source:?}");
		assert_eq!(exports, expected, "exports for {source:?}");
	}
}

#[test]
fn symbol_owner_walks_up_to_the_impl_or_trait_but_not_to_a_module() {
	let source = "impl Widget {\n\tfn paint(&self) {}\n}\ntrait Render {\n\tfn go(&self) {}\n}\nmod inner {\n\tfn free() {}\n}\n";
	let tree = parse(source);
	let mut functions = Vec::new();
	nodes_of_kind(tree.root_node(), "function_item", &mut functions);
	let lang = Rust {};
	assert_eq!(
		lang.extract_symbol_owner(functions[0], source),
		Some("Widget".to_string())
	);
	assert_eq!(
		lang.extract_symbol_owner(functions[1], source),
		Some("Render".to_string())
	);
	// `mod_item` is not one of the container kinds `find_graph_symbol_owner`
	// recognises, so a module-level function reports no owner.
	assert_eq!(lang.extract_symbol_owner(functions[2], source), None);
}

#[test]
fn declaration_names_come_from_the_declarations_own_name_child() {
	for (source, kind, expected) in [
		("struct Widget { id: u32 }\n", "struct_item", "Widget"),
		("fn helper() {}\n", "function_item", "helper"),
		("trait Render { fn go(&self); }\n", "trait_item", "Render"),
	] {
		let tree = parse(source);
		let node = first_node(&tree, kind);
		assert_eq!(
			Rust {}.extract_declaration_name(node, source),
			Some(expected.to_string()),
			"declaration name for {kind}"
		);
	}
}

#[test]
fn a_crate_path_resolves_to_the_deepest_matching_module_file() {
	let files = demo_crate();
	assert_eq!(
		Rust {}.resolve_import("crate::config::features::Item", "demo/src/main.rs", &files),
		Some("demo/src/config/features.rs".to_string())
	);
}

#[test]
fn a_crate_path_falls_back_to_a_module_directory_mod_rs() {
	let files = demo_crate();
	assert_eq!(
		Rust {}.resolve_import("crate::graph::Missing", "demo/src/lib.rs", &files),
		Some("demo/src/graph/mod.rs".to_string())
	);
}

#[test]
fn a_single_segment_crate_path_falls_back_to_the_crate_root() {
	// `crate::Url` names an item declared in lib.rs itself, so there is no
	// module file to match; the crate root is the file that defines it.
	let files = demo_crate();
	assert_eq!(
		Rust {}.resolve_import("crate::Url", "demo/src/graph/node.rs", &files),
		Some("demo/src/lib.rs".to_string())
	);
}

#[test]
fn a_multi_segment_crate_path_that_matches_nothing_resolves_to_nothing() {
	// The root fallback is deliberately limited to one segment: a longer path
	// that resolved nothing is a module we failed to find, and pointing it at
	// the root would invent an edge that does not exist.
	let files = demo_crate();
	assert_eq!(
		Rust {}.resolve_import("crate::missing::Thing", "demo/src/lib.rs", &files),
		None
	);
}

#[test]
fn a_super_path_resolves_against_the_source_files_own_directory() {
	let files = demo_crate();
	assert_eq!(
		Rust {}.resolve_import("super::node", "demo/src/graph/mod.rs", &files),
		Some("demo/src/graph/node.rs".to_string())
	);
}

#[test]
fn a_self_path_resolves_back_to_the_source_file_itself() {
	let files = demo_crate();
	let lang = Rust {};
	assert_eq!(
		lang.resolve_import("self::helpers", "demo/src/config.rs", &files),
		Some("demo/src/config.rs".to_string())
	);
	// A source file the registry has never seen cannot resolve to itself.
	assert_eq!(
		lang.resolve_import("self::helpers", "demo/src/absent.rs", &files),
		None
	);
}

#[test]
fn an_unqualified_import_resolves_to_a_sibling_file() {
	let files = demo_crate();
	assert_eq!(
		Rust {}.resolve_import("node", "demo/src/graph/mod.rs", &files),
		Some("demo/src/graph/node.rs".to_string())
	);
}

#[test]
fn a_third_party_crate_path_does_not_resolve_to_a_project_file() {
	let files = demo_crate();
	assert_eq!(
		Rust {}.resolve_import("serde::Serialize", "demo/src/lib.rs", &files),
		None
	);
}

#[test]
fn consecutive_single_line_consts_merge_into_one_labelled_region() {
	let source = "pub const A: usize = 1;\npub const B: usize = 2;\n";
	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"two single-line consts should merge, got {:?}",
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "const_item");
	assert!(
		regions[0]
			.content
			.starts_with("// Merged constant declarations (2 declarations)\n"),
		"got: {:?}",
		regions[0].content
	);
	assert_eq!(regions[0].symbols, vec!["A", "B"]);
}

#[test]
fn an_impl_block_yields_one_region_per_method_and_none_for_the_block() {
	let source = "impl Widget {\n\tfn a(&self) {\n\t\tlet x = 1;\n\t\tlet y = 2;\n\t\tx + y\n\t}\n\n\tfn b(&self) {\n\t\tlet x = 3;\n\t\tlet y = 4;\n\t\tx + y\n\t}\n}\n";
	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		2,
		"expected one region per method, got {:?}",
		regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
	);
	assert!(regions.iter().all(|r| r.node_kind == "function_item"));
	assert!(
		regions
			.iter()
			.all(|r| r.symbols.contains(&"Widget".to_string())),
		"each method region should carry its receiver type, got {:?}",
		regions.iter().map(|r| &r.symbols).collect::<Vec<_>>()
	);
}
