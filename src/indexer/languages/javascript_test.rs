use crate::indexer::code_region_extractor::extract_meaningful_regions;
use crate::indexer::languages::javascript::JavaScript;
use crate::indexer::languages::resolution_utils::FileRegistry;
use crate::indexer::languages::{Language, TypeRelationKind};
use tree_sitter::{Node, Parser, Tree};

fn parse_regions(source: &str) -> Vec<crate::indexer::code_region_extractor::CodeRegion> {
	let js_lang = JavaScript {};
	let mut parser = Parser::new();
	parser.set_language(&js_lang.get_ts_language()).unwrap();

	let tree = parser.parse(source, None).unwrap();
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), source, &js_lang, &mut regions);
	regions
}

#[test]
fn test_exported_class_splits_into_individual_methods() {
	// Bodies are multi-line/non-trivial so the smart single-line merge pass
	// (unrelated to this fix) doesn't recombine the two methods into one
	// "// Merged ..." block, which would defeat this test's purpose.
	let source = r#"
export class Foo {
	method1() {
		const x = 1;
		const y = 2;
		return x + y;
	}
	method2() {
		const x = 3;
		const y = 4;
		return x + y;
	}
}
"#;

	let regions = parse_regions(source);

	let method_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "method_definition")
		.collect();
	assert_eq!(
		method_regions.len(),
		2,
		"expected 2 method_definition regions from the exported class, got {} (regions: {:?})",
		method_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);

	let export_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "export_statement")
		.collect();
	assert!(
		export_regions.is_empty(),
		"exported class should not collapse into a single export_statement blob region, got: {:?}",
		export_regions
			.iter()
			.map(|r| &r.content)
			.collect::<Vec<_>>()
	);
}

#[test]
fn test_export_default_named_class_splits_into_individual_methods() {
	let source = r#"
export default class Bar {
	m() {}
}
"#;

	let regions = parse_regions(source);

	let method_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "method_definition")
		.collect();
	assert_eq!(
		method_regions.len(),
		1,
		"expected 1 method_definition region from the exported default class, got {} (regions: {:?})",
		method_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);

	let export_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "export_statement")
		.collect();
	assert!(
		export_regions.is_empty(),
		"export default class should not collapse into a single export_statement blob region, got: {:?}",
		export_regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
}

#[test]
fn test_export_function_regression_unchanged() {
	let source = "export function foo() {}\n";

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"export function should still produce exactly one region, got {} (regions: {:?})",
		regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "export_statement");
	assert!(regions[0].content.contains("function foo"));
}

#[test]
fn test_export_const_regression_unchanged() {
	let source = "export const x = 1;\n";

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"export const should still produce exactly one region, got {} (regions: {:?})",
		regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "export_statement");
	assert!(regions[0].content.contains("const x = 1"));
}

#[test]
fn test_export_default_expression_regression_unchanged() {
	let source = "const foo = 1;\nexport default foo;\n";

	let regions = parse_regions(source);

	let export_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "export_statement")
		.collect();
	assert_eq!(
		export_regions.len(),
		1,
		"export default <expr> should still produce exactly one export_statement region, got {} (regions: {:?})",
		export_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert!(export_regions[0].content.contains("export default foo"));
}

#[test]
fn test_export_named_braces_regression_unchanged() {
	let source = "const existingName = 1;\nexport { existingName };\n";

	let regions = parse_regions(source);

	let export_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "export_statement")
		.collect();
	assert_eq!(
		export_regions.len(),
		1,
		"export {{ name }} should still produce exactly one export_statement region, got {} (regions: {:?})",
		export_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert!(export_regions[0]
		.content
		.contains("export { existingName }"));
}

// ── harness ──────────────────────────────────────────────────────────────────

fn parse_js(source: &str) -> Tree {
	let mut parser = Parser::new();
	parser
		.set_language(&JavaScript {}.get_ts_language())
		.expect("JavaScript grammar should load");
	parser
		.parse(source, None)
		.expect("JavaScript source should parse")
}

fn collect_nodes<'tree>(node: Node<'tree>, kinds: &[&str], out: &mut Vec<Node<'tree>>) {
	if kinds.contains(&node.kind()) {
		out.push(node);
	}
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		collect_nodes(child, kinds, out);
	}
}

fn nodes_of_kinds<'tree>(tree: &'tree Tree, kinds: &[&str]) -> Vec<Node<'tree>> {
	let mut found = Vec::new();
	collect_nodes(tree.root_node(), kinds, &mut found);
	found
}

fn first_node<'tree>(tree: &'tree Tree, kind: &str) -> Node<'tree> {
	*nodes_of_kinds(tree, &[kind])
		.first()
		.unwrap_or_else(|| panic!("parsed source has no `{kind}` node"))
}

fn symbols_of(source: &str, kind: &str) -> Vec<String> {
	let tree = parse_js(source);
	JavaScript {}.extract_symbols(first_node(&tree, kind), source)
}

fn imports_of(source: &str) -> Vec<String> {
	let tree = parse_js(source);
	JavaScript {}
		.extract_imports_exports(first_node(&tree, "import_statement"), source)
		.0
}

fn exports_of(source: &str) -> Vec<String> {
	let tree = parse_js(source);
	JavaScript {}
		.extract_imports_exports(first_node(&tree, "export_statement"), source)
		.1
}

fn calls_of(source: &str) -> Vec<(String, Option<String>)> {
	let tree = parse_js(source);
	let javascript = JavaScript {};
	nodes_of_kinds(&tree, &["call_expression", "new_expression"])
		.into_iter()
		.flat_map(|node| javascript.extract_function_calls(node, source))
		.map(|target| (target.name, target.qualifier))
		.collect()
}

fn relations_of(source: &str, kind: &str) -> Vec<(TypeRelationKind, String)> {
	let tree = parse_js(source);
	JavaScript {}.extract_type_relations(first_node(&tree, kind), source)
}

fn registry_of(files: &[&str]) -> FileRegistry {
	let owned: Vec<String> = files.iter().map(|file| (*file).to_string()).collect();
	FileRegistry::new(&owned)
}

// ── language metadata ────────────────────────────────────────────────────────

#[test]
fn language_reports_its_name_and_supported_extensions() {
	let javascript = JavaScript {};
	assert_eq!(javascript.name(), "javascript");
	assert_eq!(javascript.get_file_extensions(), vec!["js", "jsx", "mjs"]);
}

#[test]
fn chunking_kinds_drop_classes_while_symbol_kinds_restore_them() {
	let javascript = JavaScript {};
	assert_eq!(
		javascript.get_meaningful_kinds(),
		vec![
			"function_declaration",
			"method_definition",
			"arrow_function",
			"import_statement",
			"export_statement",
		]
	);
	assert_eq!(
		javascript.get_symbol_kinds(),
		vec![
			"function_declaration",
			"method_definition",
			"arrow_function",
			"class_declaration",
		]
	);
}

#[test]
fn node_type_descriptions_cover_every_mapped_kind_and_the_fallback() {
	let javascript = JavaScript {};
	for (node_type, expected) in [
		("function_declaration", "function declarations"),
		("method_definition", "function declarations"),
		("arrow_function", "function declarations"),
		("class_declaration", "class declarations"),
		("import_statement", "import/export statements"),
		("export_statement", "import/export statements"),
		("variable_declaration", "variable declarations"),
		("lexical_declaration", "variable declarations"),
		("call_expression", "declarations"),
	] {
		assert_eq!(
			javascript.get_node_type_description(node_type),
			expected,
			"unexpected description for {node_type}"
		);
	}
}

#[test]
fn node_types_are_equivalent_only_within_the_same_semantic_group() {
	let javascript = JavaScript {};
	for (left, right) in [
		("function_declaration", "arrow_function"),
		("method_definition", "class_declaration"),
		("import_statement", "export_statement"),
		("variable_declaration", "lexical_declaration"),
		("unknown_kind", "unknown_kind"),
	] {
		assert!(
			javascript.are_node_types_equivalent(left, right),
			"{left} and {right} should be equivalent"
		);
	}

	for (left, right) in [
		("function_declaration", "class_declaration"),
		("arrow_function", "class_declaration"),
		("import_statement", "function_declaration"),
		("lexical_declaration", "method_definition"),
		("unknown_kind", "other_kind"),
	] {
		assert!(
			!javascript.are_node_types_equivalent(left, right),
			"{left} and {right} should not be equivalent"
		);
	}
}

#[test]
fn only_class_wrapping_export_statements_are_skipped_as_meaningful() {
	for (source, expected) in [
		("export class Foo {\n\tm() {}\n}\n", false),
		("export default class Bar {\n\tm() {}\n}\n", false),
		("export function foo() {}\n", true),
		("export const x = 1;\n", true),
		("const a = 1;\nexport { a };\n", true),
	] {
		let tree = parse_js(source);
		assert_eq!(
			JavaScript {}.is_meaningful_node(first_node(&tree, "export_statement"), source),
			expected,
			"for {source:?}"
		);
	}
}

// ── symbol extraction ────────────────────────────────────────────────────────

#[test]
fn function_declaration_symbols_include_name_and_nested_variables() {
	let source = r#"
function topLevel(a, b) {
	const sum = a + b;
	var legacy = 1;
	if (a) {
		let scoped = 2;
	}
	return sum;
}
"#;
	assert_eq!(
		symbols_of(source, "function_declaration"),
		vec!["legacy", "scoped", "sum", "topLevel"]
	);
}

#[test]
fn method_definition_symbols_include_the_owning_class() {
	let source = r#"
class Widget extends Base {
	render() {
		const node = document.createElement('div');
		let count = 0;
		return node;
	}
}
"#;
	assert_eq!(
		symbols_of(source, "method_definition"),
		vec!["Widget", "count", "node", "render"]
	);
}

#[test]
fn arrow_function_symbols_are_only_the_variable_it_is_bound_to() {
	let source = "const arrowFn = (x) => {\n\tconst inner = x * 2;\n\treturn inner;\n};\n";
	// The arrow_function arm stops at the binding name; body variables are not
	// walked the way function/method bodies are.
	assert_eq!(symbols_of(source, "arrow_function"), vec!["arrowFn"]);
}

#[test]
fn unmapped_node_kinds_fall_back_to_identifier_extraction() {
	let source = "import { alpha, beta as gamma } from \"../lib/util\";\n";
	// Only the individual identifiers: `named_imports` also matches the
	// `kind.contains("name")` filter and would otherwise leak the whole brace
	// list in as one symbol.
	assert_eq!(
		symbols_of(source, "import_statement"),
		vec!["alpha", "beta", "gamma"]
	);
}

#[test]
fn identifier_extraction_keeps_the_object_and_drops_the_property() {
	let member_source = "const x = ns.Base;\n";
	let member_tree = parse_js(member_source);
	let mut member_symbols = Vec::new();
	JavaScript {}.extract_identifiers(
		first_node(&member_tree, "member_expression"),
		member_source,
		&mut member_symbols,
	);
	assert_eq!(member_symbols, vec!["ns"]);

	let call_source = "obj.method();\n";
	let call_tree = parse_js(call_source);
	let mut call_symbols = Vec::new();
	JavaScript {}.extract_identifiers(
		first_node(&call_tree, "call_expression"),
		call_source,
		&mut call_symbols,
	);
	assert_eq!(call_symbols, vec!["obj"]);
}

#[test]
fn declaration_names_cover_bound_arrows_methods_functions_and_classes() {
	for (source, kind, expected) in [
		(
			"const arrowFn = (x) => x;\n",
			"arrow_function",
			Some("arrowFn"),
		),
		// An arrow passed straight to a call has no binding to name it.
		("run((x) => x);\n", "arrow_function", None),
		(
			"class Widget {\n\trender() {}\n}\n",
			"method_definition",
			Some("render"),
		),
		(
			"function topLevel() {}\n",
			"function_declaration",
			Some("topLevel"),
		),
		("class Widget {}\n", "class_declaration", Some("Widget")),
	] {
		let tree = parse_js(source);
		let name = JavaScript {}.extract_declaration_name(first_node(&tree, kind), source);
		assert_eq!(name.as_deref(), expected, "for {source:?}");
	}
}

// ── imports / exports ────────────────────────────────────────────────────────

#[test]
fn every_import_form_with_a_from_clause_yields_the_module_path() {
	assert_eq!(
		imports_of("import defaultExport from './mod.js';\n"),
		vec!["./mod.js"]
	);
	assert_eq!(
		imports_of("import { alpha, beta as gamma } from \"../lib/util\";\n"),
		vec!["../lib/util"]
	);
	assert_eq!(
		imports_of("import * as ns from 'namespace-mod';\n"),
		vec!["namespace-mod"]
	);
	assert_eq!(
		imports_of("import {\n\talpha,\n\tbeta\n} from './mod.js';\n"),
		vec!["./mod.js"]
	);
}

#[test]
fn side_effect_imports_still_yield_their_module_path() {
	// A bare side-effect import has no `from` clause; keying only off that
	// dropped the dependency entirely.
	assert_eq!(
		imports_of("import 'side-effect.css';\n"),
		vec!["side-effect.css"]
	);
}

#[test]
fn export_clauses_report_the_local_name_of_each_alias() {
	assert_eq!(
		exports_of("const alpha = 1;\nconst gamma = 2;\nexport { alpha, gamma as delta };\n"),
		vec!["alpha", "gamma"]
	);
}

#[test]
fn exported_declarations_report_the_declared_name() {
	assert_eq!(
		exports_of("export const exportedConst = 2;\n"),
		vec!["exportedConst"]
	);
	assert_eq!(
		exports_of("export default class Bar {\n\tm() {}\n}\n"),
		vec!["Bar"]
	);
	assert_eq!(exports_of("export class Foo {\n\tm() {}\n}\n"), vec!["Foo"]);
}

#[test]
fn an_exported_function_name_carries_no_parameter_list() {
	assert_eq!(
		exports_of("export function exported() {\n\treturn 1;\n}\n"),
		vec!["exported"]
	);
	assert_eq!(exports_of("export const value = 1;\n"), vec!["value"]);
}

#[test]
fn a_single_line_exported_function_is_still_read_as_a_declaration() {
	// The declaration branch runs before the `{ … }` named-export branch, so a
	// one-line body is not mistaken for an export list.
	assert_eq!(
		exports_of("export function inline() { return 1; }\n"),
		vec!["inline"]
	);
}

#[test]
fn export_default_of_an_expression_reports_no_name() {
	assert!(exports_of("const foo = 1;\nexport default foo;\n").is_empty());
}

// ── calls and type relations ─────────────────────────────────────────────────

#[test]
fn call_extraction_covers_bare_member_optional_chain_and_new_expressions() {
	let source = r#"
function run() {
	helper();
	obj.method();
	opt?.chain();
	new Widget(3);
}
"#;
	let extracted = calls_of(source);
	let calls: Vec<(&str, Option<&str>)> = extracted
		.iter()
		.map(|(name, qualifier)| (name.as_str(), qualifier.as_deref()))
		.collect();
	assert_eq!(
		calls,
		vec![
			("helper", None),
			("method", Some("obj")),
			("chain", Some("opt")),
			("Widget", None),
		]
	);
}

#[test]
fn non_call_nodes_produce_no_call_targets() {
	let source = "function run() {\n\thelper();\n}\n";
	let tree = parse_js(source);
	assert!(JavaScript {}
		.extract_function_calls(first_node(&tree, "function_declaration"), source)
		.is_empty());
}

#[test]
fn class_heritage_is_reported_as_an_extends_relation() {
	for (source, expected) in [
		("class Widget extends Base {}\n", "Base"),
		("class Mixed extends ns.Base {}\n", "Base"),
		// A mixin factory call is not a plain type name; the text is cut at the
		// opening paren, leaving the factory's own name.
		("class Mixin extends factory(Base) {}\n", "factory"),
	] {
		let extracted = relations_of(source, "class_declaration");
		let relations: Vec<(TypeRelationKind, &str)> = extracted
			.iter()
			.map(|(kind, name)| (*kind, name.as_str()))
			.collect();
		assert_eq!(
			relations,
			vec![(TypeRelationKind::Extends, expected)],
			"for {source:?}"
		);
	}
}

#[test]
fn classes_without_heritage_and_non_class_nodes_have_no_relations() {
	assert!(relations_of("class Plain {}\n", "class_declaration").is_empty());
	assert!(relations_of("function run() {}\n", "function_declaration").is_empty());
}

// ── import resolution ────────────────────────────────────────────────────────

#[test]
fn relative_imports_resolve_against_the_importing_file() {
	let javascript = JavaScript {};
	let registry = registry_of(&["src/utils.js", "src/lib/helper.js"]);
	assert_eq!(
		javascript.resolve_import("./utils", "src/app.js", &registry),
		Some("src/utils.js".to_string())
	);
	assert_eq!(
		javascript.resolve_import("../lib/helper.js", "src/app/main.js", &registry),
		Some("src/lib/helper.js".to_string())
	);
}

#[test]
fn extension_less_imports_try_every_javascript_extension_then_the_barrel_file() {
	let javascript = JavaScript {};
	assert_eq!(
		javascript.resolve_import("./Button", "src/app.js", &registry_of(&["src/Button.jsx"])),
		Some("src/Button.jsx".to_string())
	);
	assert_eq!(
		javascript.resolve_import(
			"./components",
			"src/app.js",
			&registry_of(&["src/components/index.js"])
		),
		Some("src/components/index.js".to_string())
	);
}

#[test]
fn root_absolute_imports_match_project_relative_files() {
	let registry = registry_of(&["lib/config.js"]);
	assert_eq!(
		JavaScript {}.resolve_import("/lib/config.js", "src/app.js", &registry),
		Some("lib/config.js".to_string())
	);
}

#[test]
fn bare_module_imports_try_source_directories_then_node_modules() {
	let javascript = JavaScript {};
	assert_eq!(
		javascript.resolve_import("helper", "main.js", &registry_of(&["lib/helper.js"])),
		Some("lib/helper.js".to_string())
	);
	assert_eq!(
		javascript.resolve_import(
			"lodash",
			"src/app.js",
			&registry_of(&["node_modules/lodash/index.js"])
		),
		Some("node_modules/lodash/index.js".to_string())
	);
}

#[test]
fn third_party_modules_absent_from_the_project_do_not_resolve() {
	let registry = registry_of(&["src/app.js", "src/util.js"]);
	assert_eq!(
		JavaScript {}.resolve_import("react", "src/app.js", &registry),
		None
	);
}

// ── region extraction ────────────────────────────────────────────────────────

#[test]
fn consecutive_imports_merge_into_one_import_export_block() {
	let source = "import a from './a.js';\nimport b from './b.js';\nimport c from './c.js';\n";
	let regions = parse_regions(source);
	assert_eq!(regions.len(), 1, "expected one merged import block");
	assert_eq!(regions[0].node_kind, "import_statement");
	assert!(regions[0]
		.content
		.starts_with("// Merged import/export statements (3 declarations)\n"));
	assert!(regions[0].content.contains("import c from './c.js';"));
}
