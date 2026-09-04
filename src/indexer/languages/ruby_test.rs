use crate::indexer::code_region_extractor::extract_meaningful_regions;
use crate::indexer::languages::resolution_utils::FileRegistry;
use crate::indexer::languages::ruby::Ruby;
use crate::indexer::languages::{CallTarget, Language, TypeRelationKind};
use tree_sitter::{Node, Parser, Tree};

fn parse_regions(source: &str) -> Vec<crate::indexer::code_region_extractor::CodeRegion> {
	let ruby_lang = Ruby {};
	let mut parser = Parser::new();
	parser.set_language(&ruby_lang.get_ts_language()).unwrap();

	let tree = parser.parse(source, None).unwrap();
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), source, &ruby_lang, &mut regions);
	regions
}

#[test]
fn test_describe_block_splits_into_individual_it_calls() {
	// Bodies are multi-statement/non-trivial so the smart single-line merge
	// pass (unrelated to this fix) doesn't recombine the two `it` blocks
	// into one "// Merged ..." block, which would defeat this test's purpose.
	let source = r#"describe "x" do
  it("a") do
    result = subject.call("a")
    expect(result).to eq("value-a")
  end

  it("b") do
    result = subject.call("b")
    expect(result).to eq("value-b")
  end
end
"#;

	let regions = parse_regions(source);

	let it_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.content.trim_start().starts_with("it("))
		.collect();
	assert_eq!(
		it_regions.len(),
		2,
		"expected 2 separate regions for the it() calls, got {} (regions: {:?})",
		it_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);

	let describe_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.content.trim_start().starts_with("describe"))
		.collect();
	assert!(
		describe_regions.is_empty(),
		"describe block should not collapse into a single blob region, got: {:?}",
		describe_regions
			.iter()
			.map(|r| &r.content)
			.collect::<Vec<_>>()
	);
}

#[test]
fn test_plain_call_still_produces_its_own_region() {
	let source = "puts \"hi\"\n";

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"a plain one-line call should still produce exactly one fallback region, got {} (regions: {:?})",
		regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "call");
	assert!(regions[0].content.contains("puts \"hi\""));
}

fn parse(source: &str) -> Tree {
	let mut parser = Parser::new();
	parser
		.set_language(&Ruby {}.get_ts_language())
		.expect("ruby grammar");
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

/// `resolve_absolute_require` probes the conventional `lib`/`app`/`config`
/// load paths verbatim, so these entries must not be prefixed. None of these
/// directories exist in this repository, which keeps the canonicalize
/// fallbacks in `find_exact_file` inert and the tests hermetic.
fn demo_app() -> FileRegistry {
	registry(&[
		"app/models/invoice.rb",
		"app/models/helpers/util.rb",
		"lib/billing.rb",
		"vendor/gems/money/lib/money.rb",
	])
}

#[test]
fn the_parser_is_named_ruby_and_claims_the_rb_extension() {
	let lang = Ruby {};
	assert_eq!(lang.name(), "ruby");
	assert_eq!(lang.get_file_extensions(), vec!["rb"]);
}

#[test]
fn containers_are_excluded_from_chunking_but_restored_as_symbol_kinds() {
	let lang = Ruby {};
	assert_eq!(
		lang.get_meaningful_kinds(),
		vec!["method", "singleton_method", "call"]
	);
	assert_eq!(
		lang.get_symbol_kinds(),
		vec!["method", "singleton_method", "class", "module"]
	);
	// `call` must not become a symbol node: every call site would turn into one.
	assert!(!lang.get_symbol_kinds().contains(&"call"));
	assert!(lang.descend_first_kinds().is_empty());
}

#[test]
fn node_type_descriptions_cover_the_mapped_kinds_and_fall_back() {
	let lang = Ruby {};
	for (kind, description) in [
		("method", "method declarations"),
		("class", "class declarations"),
		("module", "module declarations"),
		("assignment", "variable assignments"),
		("multiple_assignment", "variable assignments"),
		("singleton_method", "declarations"),
		("call", "declarations"),
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
	let lang = Ruby {};
	assert!(lang.are_node_types_equivalent("method", "method"));
	assert!(lang.are_node_types_equivalent("call", "call"));
	assert!(lang.are_node_types_equivalent("class", "module"));
	assert!(lang.are_node_types_equivalent("module", "class"));
	assert!(lang.are_node_types_equivalent("assignment", "multiple_assignment"));

	assert!(!lang.are_node_types_equivalent("method", "singleton_method"));
	assert!(!lang.are_node_types_equivalent("method", "class"));
	assert!(!lang.are_node_types_equivalent("call", "assignment"));
}

#[test]
fn a_method_carries_its_owner_and_its_local_variables() {
	let source =
		"class Invoice\n  def total\n    subtotal = 10\n    tax = 2\n    subtotal + tax\n  end\nend\n";
	let tree = parse(source);
	let node = first_node(&tree, "method");
	assert_eq!(
		Ruby {}.extract_symbols(node, source),
		vec!["Invoice", "subtotal", "tax", "total"]
	);
}

#[test]
fn a_singleton_method_carries_its_owner() {
	let source = "class Invoice\n  def self.build\n    new\n  end\nend\n";
	let tree = parse(source);
	let node = first_node(&tree, "singleton_method");
	assert_eq!(
		Ruby {}.extract_symbols(node, source),
		vec!["Invoice", "build"]
	);
}

#[test]
fn a_class_or_module_yields_its_constant_name() {
	for (source, kind, expected) in [
		("class Plain\nend\n", "class", "Plain"),
		("module Billing\nend\n", "module", "Billing"),
	] {
		let tree = parse(source);
		let node = first_node(&tree, kind);
		assert_eq!(
			Ruby {}.extract_symbols(node, source),
			vec![expected.to_string()],
			"symbols for {kind}"
		);
	}
}

#[test]
fn a_namespaced_definition_reports_its_qualified_name() {
	// The name of `module Outer::Inner` is a `scope_resolution` node, so both
	// tiers have to understand it.
	let source = "module Outer::Inner\nend\n";
	let tree = parse(source);
	let node = first_node(&tree, "module");
	let lang = Ruby {};
	assert_eq!(
		lang.extract_symbols(node, source),
		vec!["Outer::Inner".to_string()]
	);
	assert_eq!(
		lang.extract_declaration_name(node, source),
		Some("Inner".to_string())
	);
}

#[test]
fn declaration_names_cover_classes_modules_and_both_method_forms() {
	for (source, kind, expected) in [
		("class Plain\nend\n", "class", "Plain"),
		("module Billing\nend\n", "module", "Billing"),
		("def total\nend\n", "method", "total"),
		(
			"class Invoice\n  def self.build\n  end\nend\n",
			"singleton_method",
			"build",
		),
	] {
		let tree = parse(source);
		let node = first_node(&tree, kind);
		assert_eq!(
			Ruby {}.extract_declaration_name(node, source),
			Some(expected.to_string()),
			"declaration name for {kind}"
		);
	}
}

#[test]
fn an_unhandled_node_kind_falls_back_to_identifier_extraction() {
	let source = "puts message\n";
	let tree = parse(source);
	let node = first_node(&tree, "call");
	assert_eq!(
		Ruby {}.extract_symbols(node, source),
		vec!["message", "puts"]
	);
}

#[test]
fn identifier_extraction_skips_instance_variables_and_leaves_duplicates() {
	let source = "def calc\n  @cache = 1\n  value = 1\n  value + value\nend\n";
	let tree = parse(source);
	let node = first_node(&tree, "method");
	let mut symbols = Vec::new();
	Ruby {}.extract_identifiers(node, source, &mut symbols);
	// `@cache` is an `instance_variable` node, so it is never a candidate, and
	// this helper deliberately leaves deduplication to `extract_symbols`.
	assert_eq!(symbols, vec!["calc", "value", "value", "value"]);
}

#[test]
fn require_and_load_calls_become_imports() {
	for (source, expected) in [
		("require \"json\"\n", "json"),
		(
			"require_relative \"helpers/util\"\n",
			"relative:helpers/util",
		),
		("load 'legacy.rb'\n", "legacy.rb"),
	] {
		let tree = parse(source);
		let node = first_node(&tree, "call");
		let (imports, exports) = Ruby {}.extract_imports_exports(node, source);
		assert_eq!(imports, vec![expected], "imports for {source:?}");
		assert!(exports.is_empty(), "ruby declares no explicit exports");
	}
}

#[test]
fn an_ordinary_call_is_neither_an_import_nor_an_export() {
	let source = "puts \"hi\"\n";
	let tree = parse(source);
	let node = first_node(&tree, "call");
	let (imports, exports) = Ruby {}.extract_imports_exports(node, source);
	assert!(imports.is_empty());
	assert!(exports.is_empty());
}

#[test]
fn require_style_calls_are_not_reported_as_function_calls() {
	for source in [
		"require \"json\"\n",
		"require_relative \"helpers/util\"\n",
		"load 'legacy.rb'\n",
	] {
		let tree = parse(source);
		let node = first_node(&tree, "call");
		assert!(
			Ruby {}.extract_function_calls(node, source).is_empty(),
			"{source:?} is an import, not a call"
		);
	}
}

#[test]
fn calls_keep_their_receiver_as_a_qualifier() {
	let source = "def go\n  helper()\n  client.send_it(1)\n  Widget.new\nend\n";
	let tree = parse(source);
	let mut calls = Vec::new();
	nodes_of_kind(tree.root_node(), "call", &mut calls);
	let lang = Ruby {};
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
				name: "send_it".to_string(),
				qualifier: Some("client".to_string()),
			},
			CallTarget {
				name: "new".to_string(),
				qualifier: Some("Widget".to_string()),
			},
		]
	);
}

#[test]
fn a_node_that_is_not_a_call_reports_no_calls() {
	let source = "value = 1\n";
	let tree = parse(source);
	let node = first_node(&tree, "assignment");
	assert!(Ruby {}.extract_function_calls(node, source).is_empty());
}

#[test]
fn a_class_with_a_superclass_reports_an_extends_relation() {
	let source = "class Invoice < Base::Record\nend\n";
	let tree = parse(source);
	let node = first_node(&tree, "class");
	assert_eq!(
		Ruby {}.extract_type_relations(node, source),
		vec![(TypeRelationKind::Extends, "Record".to_string())]
	);
}

#[test]
fn a_plain_class_or_module_declares_no_type_relations() {
	for (source, kind) in [
		("class Plain\nend\n", "class"),
		("module M\nend\n", "module"),
	] {
		let tree = parse(source);
		let node = first_node(&tree, kind);
		assert!(
			Ruby {}.extract_type_relations(node, source).is_empty(),
			"relations for {kind}"
		);
	}
}

#[test]
fn symbol_owner_walks_up_to_the_enclosing_class_or_module() {
	let source =
		"module Billing\n  class Invoice\n    def total\n    end\n  end\n\n  def helper\n  end\nend\n";
	let tree = parse(source);
	let mut methods = Vec::new();
	nodes_of_kind(tree.root_node(), "method", &mut methods);
	let lang = Ruby {};
	assert_eq!(
		lang.extract_symbol_owner(methods[0], source),
		Some("Invoice".to_string())
	);
	assert_eq!(
		lang.extract_symbol_owner(methods[1], source),
		Some("Billing".to_string())
	);
}

#[test]
fn a_relative_require_resolves_against_the_requiring_files_directory() {
	let files = demo_app();
	let lang = Ruby {};
	let source = "app/models/invoice.rb";
	assert_eq!(
		lang.resolve_import("relative:helpers/util", source, &files),
		Some("app/models/helpers/util.rb".to_string())
	);
	assert_eq!(
		lang.resolve_import("./helpers/util", source, &files),
		Some("app/models/helpers/util.rb".to_string())
	);
}

#[test]
fn an_absolute_require_falls_back_to_the_conventional_load_paths() {
	assert_eq!(
		Ruby {}.resolve_import("billing", "app/models/invoice.rb", &demo_app()),
		Some("lib/billing.rb".to_string())
	);
}

#[test]
fn an_absolute_require_finally_searches_vendored_gems() {
	assert_eq!(
		Ruby {}.resolve_import("money", "app/models/invoice.rb", &demo_app()),
		Some("vendor/gems/money/lib/money.rb".to_string())
	);
}

#[test]
fn an_unknown_require_does_not_resolve() {
	assert_eq!(
		Ruby {}.resolve_import("nonexistent", "app/models/invoice.rb", &demo_app()),
		None
	);
}

#[test]
fn consecutive_require_calls_merge_into_one_labelled_region() {
	let source = "require \"json\"\nrequire \"csv\"\nrequire \"set\"\n";
	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"got {:?}",
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "call");
	// `call` has no dedicated description, so the generic fallback is used.
	assert!(
		regions[0]
			.content
			.starts_with("// Merged declarations (3 declarations)\n"),
		"got: {:?}",
		regions[0].content
	);
}

#[test]
fn a_class_body_splits_into_one_region_per_method_form() {
	let source = "class Invoice\n  def total\n    subtotal = 10\n    tax = 2\n    subtotal + tax\n  end\n\n  def self.build\n    record = new\n    record.save\n    record\n  end\nend\n";
	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		2,
		"got {:?}",
		regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "method");
	assert_eq!(regions[1].node_kind, "singleton_method");
	// Matching a method ends the walk, so the `record.save` call inside it does
	// not become a region of its own.
	assert!(regions.iter().all(|r| r.node_kind != "call"));
}

#[test]
fn a_block_call_without_nested_block_calls_stays_a_single_region() {
	let source =
		"it(\"a\") do\n  result = subject.call(\"a\")\n  expect(result).to eq(\"value-a\")\nend\n";
	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"plain calls reached inside the block must not be promoted, got {:?}",
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "call");
	assert!(regions[0].content.starts_with("it(\"a\") do"));
}
