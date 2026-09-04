use crate::indexer::code_region_extractor::extract_meaningful_regions;
use crate::indexer::languages::resolution_utils::FileRegistry;
use crate::indexer::languages::typescript::TypeScript;
use crate::indexer::languages::{CallTarget, Language, TypeRelationKind};
use tree_sitter::{Node, Parser, Tree};

fn parse_regions(source: &str) -> Vec<crate::indexer::code_region_extractor::CodeRegion> {
	let ts_lang = TypeScript {};
	let mut parser = Parser::new();
	parser.set_language(&ts_lang.get_ts_language()).unwrap();

	let tree = parser.parse(source, None).unwrap();
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), source, &ts_lang, &mut regions);
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

#[test]
fn test_export_interface_regression_unchanged() {
	// Parsed alone (rather than alongside the type alias below) so the
	// smart single-line merge pass — unrelated to this fix — doesn't
	// recombine the two adjacent short export statements into one
	// "// Merged ..." block, which would defeat this test's purpose.
	let source = "export interface Foo { x: number }\n";

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"export interface should still produce exactly one region, got {} (regions: {:?})",
		regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "export_statement");
	assert!(regions[0].content.contains("interface Foo"));
}

#[test]
fn test_export_type_alias_regression_unchanged() {
	let source = "export type Bar = string;\n";

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"export type alias should still produce exactly one region, got {} (regions: {:?})",
		regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "export_statement");
	assert!(regions[0].content.contains("type Bar"));
}

fn parse_tree(source: &str) -> Tree {
	let mut parser = Parser::new();
	parser
		.set_language(&TypeScript {}.get_ts_language())
		.unwrap();
	parser.parse(source, None).unwrap()
}

fn nodes_of_kind<'a>(node: Node<'a>, kind: &str, out: &mut Vec<Node<'a>>) {
	if node.kind() == kind {
		out.push(node);
	}
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		nodes_of_kind(child, kind, out);
	}
}

fn all_of_kind<'a>(tree: &'a Tree, kind: &str) -> Vec<Node<'a>> {
	let mut found = Vec::new();
	nodes_of_kind(tree.root_node(), kind, &mut found);
	found
}

fn first_of_kind<'a>(tree: &'a Tree, kind: &str) -> Node<'a> {
	all_of_kind(tree, kind)
		.into_iter()
		.next()
		.unwrap_or_else(|| panic!("no `{kind}` node in the parsed tree"))
}

fn as_strs(values: &[String]) -> Vec<&str> {
	values.iter().map(String::as_str).collect()
}

const DECLARATIONS: &str = r#"class Repo {
	find(id: number): number {
		const row = id;
		return row;
	}
}

interface Shape {
	size: number;
}

type Alias = string;

enum Colors {
	Red,
	Green
}

const build = (n: number) => {
	const made = n;
	return made;
};

function top(x: number): number {
	const kept = x;
	return kept;
}
"#;

#[test]
fn language_metadata_matches_the_typescript_grammar() {
	let ts = TypeScript {};

	assert_eq!(ts.name(), "typescript");
	assert_eq!(ts.get_file_extensions(), ["ts", "tsx"]);
	// Chunking deliberately drops class/enum containers so methods chunk
	// individually; the symbol tier adds them back and drops import/export.
	assert_eq!(
		ts.get_meaningful_kinds(),
		[
			"function_declaration",
			"method_definition",
			"arrow_function",
			"interface_declaration",
			"type_alias_declaration",
			"import_statement",
			"export_statement",
		]
	);
	assert_eq!(
		ts.get_symbol_kinds(),
		[
			"function_declaration",
			"method_definition",
			"arrow_function",
			"class_declaration",
			"interface_declaration",
			"type_alias_declaration",
			"enum_declaration",
		]
	);
}

#[test]
fn node_type_descriptions_cover_every_typescript_group() {
	let ts = TypeScript {};

	for kind in [
		"function_declaration",
		"method_definition",
		"arrow_function",
	] {
		assert_eq!(ts.get_node_type_description(kind), "function declarations");
	}
	assert_eq!(
		ts.get_node_type_description("class_declaration"),
		"class declarations"
	);
	assert_eq!(
		ts.get_node_type_description("interface_declaration"),
		"interface declarations"
	);
	assert_eq!(
		ts.get_node_type_description("type_alias_declaration"),
		"type declarations"
	);
	for kind in ["import_statement", "export_statement"] {
		assert_eq!(
			ts.get_node_type_description(kind),
			"import/export statements"
		);
	}
	for kind in ["variable_declaration", "lexical_declaration"] {
		assert_eq!(ts.get_node_type_description(kind), "variable declarations");
	}
	// The override replaces the trait default outright, so kinds it does not
	// list get the generic label even when they read like a type or class.
	assert_eq!(
		ts.get_node_type_description("enum_declaration"),
		"declarations"
	);
	assert_eq!(
		ts.get_node_type_description("abstract_class_declaration"),
		"declarations"
	);
}

#[test]
fn equivalent_node_types_follow_the_typescript_semantic_groups() {
	let ts = TypeScript {};

	assert!(ts.are_node_types_equivalent("enum_declaration", "enum_declaration"));
	assert!(ts.are_node_types_equivalent("function_declaration", "arrow_function"));
	assert!(ts.are_node_types_equivalent("method_definition", "function_declaration"));
	assert!(ts.are_node_types_equivalent("class_declaration", "interface_declaration"));
	assert!(ts.are_node_types_equivalent("type_alias_declaration", "interface_declaration"));
	assert!(ts.are_node_types_equivalent("import_statement", "export_statement"));
	assert!(ts.are_node_types_equivalent("variable_declaration", "lexical_declaration"));

	// interface bridges the class and type groups, but class and type alias
	// are never equivalent to each other.
	assert!(!ts.are_node_types_equivalent("class_declaration", "type_alias_declaration"));
	assert!(!ts.are_node_types_equivalent("function_declaration", "class_declaration"));
	assert!(!ts.are_node_types_equivalent("enum_declaration", "class_declaration"));
}

#[test]
fn declaration_symbols_carry_names_body_variables_and_the_owning_class() {
	let tree = parse_tree(DECLARATIONS);
	let ts = TypeScript {};

	assert_eq!(
		as_strs(&ts.extract_symbols(first_of_kind(&tree, "class_declaration"), DECLARATIONS)),
		["Repo"]
	);
	// Methods add both the locals declared in their body and their owning
	// class so "Repo find" style queries hit lexically.
	assert_eq!(
		as_strs(&ts.extract_symbols(first_of_kind(&tree, "method_definition"), DECLARATIONS)),
		["Repo", "find", "row"]
	);
	assert_eq!(
		as_strs(&ts.extract_symbols(first_of_kind(&tree, "interface_declaration"), DECLARATIONS)),
		["Shape"]
	);
	assert_eq!(
		as_strs(&ts.extract_symbols(first_of_kind(&tree, "type_alias_declaration"), DECLARATIONS)),
		["Alias"]
	);
	// Arrow functions only borrow the name of the variable they are bound to;
	// their body locals are not collected.
	assert_eq!(
		as_strs(&ts.extract_symbols(first_of_kind(&tree, "arrow_function"), DECLARATIONS)),
		["build"]
	);
	assert_eq!(
		as_strs(&ts.extract_symbols(first_of_kind(&tree, "function_declaration"), DECLARATIONS)),
		["kept", "top"]
	);
	// `enum_declaration` is a symbol kind but has no arm in extract_symbols,
	// so it falls through to the generic identifier walk — which skips the
	// members because they parse as `property_identifier`.
	assert_eq!(
		as_strs(&ts.extract_symbols(first_of_kind(&tree, "enum_declaration"), DECLARATIONS)),
		["Colors"]
	);
}

#[test]
fn methods_of_an_abstract_class_lose_their_owner() {
	let source = r#"abstract class Base {
	run(): void {
		const v = 1;
		return;
	}
}
"#;

	let tree = parse_tree(source);
	let method = first_of_kind(&tree, "method_definition");

	// The owner lookup only recognises `class_declaration`, and an abstract
	// class parses as `abstract_class_declaration`, so "Base" is missing.
	assert_eq!(
		as_strs(&TypeScript {}.extract_symbols(method, source)),
		["run", "v"]
	);
}

#[test]
fn declaration_names_come_from_the_declaring_node() {
	let tree = parse_tree(DECLARATIONS);
	let ts = TypeScript {};

	assert_eq!(
		ts.extract_declaration_name(first_of_kind(&tree, "function_declaration"), DECLARATIONS)
			.as_deref(),
		Some("top")
	);
	assert_eq!(
		ts.extract_declaration_name(first_of_kind(&tree, "class_declaration"), DECLARATIONS)
			.as_deref(),
		Some("Repo")
	);
	assert_eq!(
		ts.extract_declaration_name(first_of_kind(&tree, "interface_declaration"), DECLARATIONS)
			.as_deref(),
		Some("Shape")
	);
	assert_eq!(
		ts.extract_declaration_name(first_of_kind(&tree, "method_definition"), DECLARATIONS)
			.as_deref(),
		Some("find")
	);
}

#[test]
fn arrow_functions_are_named_only_when_bound_to_a_variable() {
	let source = "const build = (n: number) => n;\nrun(() => 2);\n";

	let tree = parse_tree(source);
	let ts = TypeScript {};
	let arrows = all_of_kind(&tree, "arrow_function");
	assert_eq!(arrows.len(), 2);

	assert_eq!(
		ts.extract_declaration_name(arrows[0], source).as_deref(),
		Some("build")
	);
	// An arrow passed straight into a call has an `arguments` parent, so it
	// declares no name at all.
	assert_eq!(ts.extract_declaration_name(arrows[1], source), None);
	assert!(ts.extract_symbols(arrows[1], source).is_empty());
}

#[test]
fn identifier_extraction_keeps_the_receiver_and_drops_the_property() {
	let source = "service.send(1);\n";

	let tree = parse_tree(source);
	let member = first_of_kind(&tree, "member_expression");
	let mut symbols = Vec::new();
	TypeScript {}.extract_identifiers(member, source, &mut symbols);

	assert_eq!(as_strs(&symbols), ["service"]);
}

#[test]
fn import_statements_yield_the_module_path() {
	let source = r#"import type { Foo } from './foo';
import { type Bar, Baz as Qux } from './bar';
import Default from './default';
import * as ns from './ns';
import './side-effect';
"#;

	let tree = parse_tree(source);
	let ts = TypeScript {};
	let statements = all_of_kind(&tree, "import_statement");
	assert_eq!(statements.len(), 5);

	let names = |index: usize| {
		let (imports, exports) = ts.extract_imports_exports(statements[index], source);
		assert!(exports.is_empty());
		imports
	};

	// The module path, not the bound names: `resolve_import` is the only
	// consumer and it resolves a path to a file.
	assert_eq!(as_strs(&names(0)), ["./foo"]);
	assert_eq!(as_strs(&names(1)), ["./bar"]);
	assert_eq!(as_strs(&names(2)), ["./default"]);
	assert_eq!(as_strs(&names(3)), ["./ns"]);
	// A side-effect import still records the dependency.
	assert_eq!(as_strs(&names(4)), ["./side-effect"]);
}

#[test]
fn a_typescript_import_resolves_to_the_file_it_names() {
	let source = "import { Helper } from './helper';\n";
	let tree = parse_tree(source);
	let ts = TypeScript {};
	let (imports, _) =
		ts.extract_imports_exports(all_of_kind(&tree, "import_statement")[0], source);

	let files = vec!["src/main.ts".to_string(), "src/helper.ts".to_string()];
	let registry = crate::indexer::languages::resolution_utils::FileRegistry::new(&files);
	assert_eq!(
		ts.resolve_import(&imports[0], "src/main.ts", &registry),
		Some("src/helper.ts".to_string())
	);
}

#[test]
fn export_statements_yield_the_exported_names() {
	let source = r#"export type { Foo };
export interface Named extends Base { x: number }
export type Alias = string;
export { a, b };
export default thing;
"#;

	let tree = parse_tree(source);
	let ts = TypeScript {};
	let statements = all_of_kind(&tree, "export_statement");
	assert_eq!(statements.len(), 5);

	let names = |index: usize| {
		let (imports, exports) = ts.extract_imports_exports(statements[index], source);
		assert!(imports.is_empty());
		exports
	};

	assert_eq!(as_strs(&names(0)), ["Foo"]);
	assert_eq!(as_strs(&names(1)), ["Named"]);
	assert_eq!(as_strs(&names(2)), ["Alias"]);
	assert_eq!(as_strs(&names(3)), ["a", "b"]);
	// `export default <expr>` has no name-bearing form the text parser
	// recognises, so it reports nothing.
	assert!(names(4).is_empty());
}

#[test]
fn exported_function_names_come_back_clean_from_the_text_parser() {
	let ts = TypeScript {};

	let multi_line = "export function exported() {\n\treturn 1;\n}\n";
	let tree = parse_tree(multi_line);
	let (_, exports) =
		ts.extract_imports_exports(first_of_kind(&tree, "export_statement"), multi_line);
	assert_eq!(as_strs(&exports), ["exported"]);

	// A body on the same line must not be read as a named-export list.
	let single_line = "export function tight() {}\n";
	let tree = parse_tree(single_line);
	let (_, exports) =
		ts.extract_imports_exports(first_of_kind(&tree, "export_statement"), single_line);
	assert_eq!(as_strs(&exports), ["tight"]);
}

#[test]
fn exported_declaration_nodes_report_their_own_name_from_the_ast() {
	let source =
		"export function exported() {\n\treturn 1;\n}\nfunction plain() {\n\treturn 2;\n}\n";

	let tree = parse_tree(source);
	let ts = TypeScript {};
	let functions = all_of_kind(&tree, "function_declaration");
	assert_eq!(functions.len(), 2);

	// Reached through the declaration node the name comes from the AST rather
	// than from the statement text.
	let (imports, exports) = ts.extract_imports_exports(functions[0], source);
	assert!(imports.is_empty());
	assert_eq!(as_strs(&exports), ["exported"]);

	// A declaration whose parent is not an export_statement exports nothing.
	let (_, exports) = ts.extract_imports_exports(functions[1], source);
	assert!(exports.is_empty());
}

#[test]
fn function_calls_keep_the_receiver_as_a_qualifier() {
	let source = r#"function run(): void {
	helper();
	service.send(1);
	const w = new Widget(2);
}
"#;

	let tree = parse_tree(source);
	let ts = TypeScript {};
	let calls = all_of_kind(&tree, "call_expression");
	assert_eq!(calls.len(), 2);

	assert_eq!(
		ts.extract_function_calls(calls[0], source),
		vec![CallTarget {
			name: "helper".to_string(),
			qualifier: None,
		}]
	);
	assert_eq!(
		ts.extract_function_calls(calls[1], source),
		vec![CallTarget {
			name: "send".to_string(),
			qualifier: Some("service".to_string()),
		}]
	);
	assert_eq!(
		ts.extract_function_calls(first_of_kind(&tree, "new_expression"), source),
		vec![CallTarget {
			name: "Widget".to_string(),
			qualifier: None,
		}]
	);
	// Nodes that are not call sites contribute nothing.
	assert!(ts
		.extract_function_calls(first_of_kind(&tree, "lexical_declaration"), source)
		.is_empty());
}

#[test]
fn class_heritage_yields_both_extends_and_implements_relations() {
	let source = "class Impl extends BaseClass implements Iface, Other {}\n";

	let tree = parse_tree(source);

	assert_eq!(
		TypeScript {}.extract_type_relations(first_of_kind(&tree, "class_declaration"), source),
		vec![
			(TypeRelationKind::Extends, "BaseClass".to_string()),
			(TypeRelationKind::Implements, "Iface".to_string()),
			(TypeRelationKind::Implements, "Other".to_string()),
		]
	);
}

#[test]
fn generic_heritage_targets_drop_their_type_arguments() {
	let source = "class Generic extends Wrapped<T> implements Ifc<K> {}\n";

	let tree = parse_tree(source);

	assert_eq!(
		TypeScript {}.extract_type_relations(first_of_kind(&tree, "class_declaration"), source),
		vec![
			(TypeRelationKind::Extends, "Wrapped".to_string()),
			(TypeRelationKind::Implements, "Ifc".to_string()),
		]
	);
}

#[test]
fn interface_inheritance_yields_one_extends_relation_per_parent() {
	let source = "interface Named extends Base, Second {}\n";

	let tree = parse_tree(source);

	assert_eq!(
		TypeScript {}.extract_type_relations(first_of_kind(&tree, "interface_declaration"), source),
		vec![
			(TypeRelationKind::Extends, "Base".to_string()),
			(TypeRelationKind::Extends, "Second".to_string()),
		]
	);
}

#[test]
fn types_without_heritage_yield_no_relations() {
	let source = "interface Plain { y: string }\ntype Alias = string;\nfunction free() {}\n";

	let tree = parse_tree(source);
	let ts = TypeScript {};

	for kind in [
		"interface_declaration",
		"type_alias_declaration",
		"function_declaration",
	] {
		assert!(
			ts.extract_type_relations(first_of_kind(&tree, kind), source)
				.is_empty(),
			"{kind} should declare no type relations"
		);
	}
}

#[test]
fn export_statements_are_transparent_only_for_a_plain_class() {
	let ts = TypeScript {};

	let source = "export class Plain {}\n";
	let tree = parse_tree(source);
	assert!(!ts.is_meaningful_node(first_of_kind(&tree, "export_statement"), source));

	let source = "export function fine() {}\n";
	let tree = parse_tree(source);
	assert!(ts.is_meaningful_node(first_of_kind(&tree, "export_statement"), source));

	// `abstract class` parses as its own node kind, which the class check
	// does not match, so the whole export stays one block.
	let source = "export abstract class Abs {}\n";
	let tree = parse_tree(source);
	assert!(ts.is_meaningful_node(first_of_kind(&tree, "export_statement"), source));
}

#[test]
fn exported_abstract_class_collapses_into_one_export_statement_region() {
	let source = r#"
export abstract class Service {
	run(): number {
		const a = 1;
		const b = 2;
		return a + b;
	}
}
"#;

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"expected one region, got {:?}",
		regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
	);
	// Documents the asymmetry with `export class`, which does split into
	// method_definition regions.
	assert_eq!(regions[0].node_kind, "export_statement");
	assert!(regions[0].content.contains("run(): number"));
}

#[test]
fn interfaces_and_type_aliases_chunk_as_separate_regions() {
	let source = r#"interface Shape {
	kind: string;
	size: number;
}

type Alias = {
	a: string;
	b: number;
};
"#;

	let regions = parse_regions(source);

	assert_eq!(
		regions
			.iter()
			.map(|r| r.node_kind.as_str())
			.collect::<Vec<_>>(),
		["interface_declaration", "type_alias_declaration"]
	);
}

#[test]
fn a_bound_arrow_function_chunks_under_its_variable_name() {
	let source = r#"const handler = (event: MouseEvent) => {
	const target = event.target;
	log(target);
	return target;
};
"#;

	let regions = parse_regions(source);

	assert_eq!(regions.len(), 1);
	assert_eq!(regions[0].node_kind, "arrow_function");
	assert_eq!(as_strs(&regions[0].symbols), ["handler"]);
	// The region starts at the arrow itself, not at `const handler`.
	assert!(regions[0].content.starts_with("(event: MouseEvent)"));
}

fn ts_registry() -> FileRegistry {
	let files = vec![
		"src/app.ts".to_string(),
		"src/util.ts".to_string(),
		"src/models/index.ts".to_string(),
		"src/legacy.js".to_string(),
	];
	FileRegistry::new(&files)
}

#[test]
fn relative_imports_resolve_against_the_importing_file() {
	let registry = ts_registry();

	assert_eq!(
		TypeScript {}
			.resolve_import("./util.ts", "src/app.ts", &registry)
			.as_deref(),
		Some("src/util.ts")
	);
}

#[test]
fn extension_less_imports_prefer_a_typescript_file() {
	let registry = ts_registry();

	assert_eq!(
		TypeScript {}
			.resolve_import("./util", "src/app.ts", &registry)
			.as_deref(),
		Some("src/util.ts")
	);
	assert_eq!(
		TypeScript {}
			.resolve_import("./legacy", "src/app.ts", &registry)
			.as_deref(),
		Some("src/legacy.js")
	);
}

#[test]
fn barrel_imports_resolve_to_the_index_file() {
	let registry = ts_registry();

	assert_eq!(
		TypeScript {}
			.resolve_import("./models", "src/app.ts", &registry)
			.as_deref(),
		Some("src/models/index.ts")
	);
}

#[test]
fn third_party_imports_that_match_no_indexed_file_stay_unresolved() {
	let registry = ts_registry();

	assert_eq!(
		TypeScript {}.resolve_import("react", "src/app.ts", &registry),
		None
	);
}
