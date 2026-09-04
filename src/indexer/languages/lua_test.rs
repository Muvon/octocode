use crate::indexer::code_region_extractor::{extract_meaningful_regions, CodeRegion};
use crate::indexer::languages::lua::Lua;
use crate::indexer::languages::resolution_utils::FileRegistry;
use crate::indexer::languages::{CallTarget, Language};
use tree_sitter::{Node, Parser, Tree};

/// Mirrors the full-tree DFS both `differential_processor::walk_for_imports_exports`
/// and GraphRAG's `walk_ast` perform: call `extract_imports_exports` on every
/// node in the tree independently, without relying on any self-recursion inside
/// the language implementation.
fn collect_all_imports_via_full_tree_walk(
	source: &str,
	lang: &dyn Language,
	root: Node,
) -> Vec<String> {
	let mut imports = Vec::new();
	let mut stack = vec![root];
	while let Some(n) = stack.pop() {
		let (imp, _exp) = lang.extract_imports_exports(n, source);
		imports.extend(imp);
		let mut cursor = n.walk();
		for child in n.children(&mut cursor) {
			stack.push(child);
		}
	}
	imports
}

#[test]
fn test_nested_function_require_detected_exactly_once() {
	let source = r#"function outer()
	local x = require("outer_mod")
	local function inner()
		local y = require("inner_mod")
	end
end
"#;
	let lua_lang = Lua {};
	let mut parser = Parser::new();
	parser.set_language(&lua_lang.get_ts_language()).unwrap();
	let tree = parser.parse(source, None).unwrap();

	let imports = collect_all_imports_via_full_tree_walk(source, &lua_lang, tree.root_node());

	let inner_count = imports.iter().filter(|s| s.as_str() == "inner_mod").count();
	assert_eq!(
		inner_count, 1,
		"nested function's own require() should be detected exactly once, got {} in {:?}",
		inner_count, imports
	);

	let outer_count = imports.iter().filter(|s| s.as_str() == "outer_mod").count();
	assert_eq!(
		outer_count, 1,
		"outer function's require() should be detected exactly once, got {} in {:?}",
		outer_count, imports
	);
}

fn parse_tree(source: &str) -> Tree {
	let mut parser = Parser::new();
	parser.set_language(&Lua {}.get_ts_language()).unwrap();
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

/// The `index`-th node of `kind` in document order.
fn nth_node<'a>(tree: &'a Tree, kind: &str, index: usize) -> Node<'a> {
	let mut found = Vec::new();
	nodes_of_kind(tree.root_node(), kind, &mut found);
	*found
		.get(index)
		.unwrap_or_else(|| panic!("no {kind} node at index {index}"))
}

fn first_node<'a>(tree: &'a Tree, kind: &str) -> Node<'a> {
	nth_node(tree, kind, 0)
}

fn parse_regions(source: &str) -> Vec<CodeRegion> {
	let lua = Lua {};
	let tree = parse_tree(source);
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), source, &lua, &mut regions);
	regions
}

fn registry(files: &[&str]) -> FileRegistry {
	let owned: Vec<String> = files.iter().map(|f| f.to_string()).collect();
	FileRegistry::new(&owned)
}

#[test]
fn the_language_is_named_lua_and_owns_the_lua_extension() {
	let lua = Lua {};
	assert_eq!(lua.name(), "lua");
	assert_eq!(lua.get_file_extensions(), vec!["lua"]);
}

#[test]
fn the_chunking_kinds_list_a_kind_this_grammar_never_produces() {
	let lua = Lua {};
	assert_eq!(
		lua.get_meaningful_kinds(),
		vec![
			"function_declaration",
			"function_definition",
			"local_function"
		]
	);

	// tree-sitter-lua 0.5 aliases `local function f()` to `function_declaration`,
	// so the `local_function` entry above can never match a real node.
	let source = "local function f()\n\treturn 1\nend\n";
	let tree = parse_tree(source);
	let mut local_functions = Vec::new();
	nodes_of_kind(tree.root_node(), "local_function", &mut local_functions);
	assert!(local_functions.is_empty());
	let mut declarations = Vec::new();
	nodes_of_kind(tree.root_node(), "function_declaration", &mut declarations);
	assert_eq!(declarations.len(), 1);
}

#[test]
fn symbol_kinds_cover_only_named_function_declarations() {
	assert_eq!(Lua {}.get_symbol_kinds(), vec!["function_declaration"]);
}

#[test]
fn every_node_type_description_arm_is_reachable() {
	let lua = Lua {};
	assert_eq!(
		lua.get_node_type_description("function_declaration"),
		"Function Declaration"
	);
	assert_eq!(
		lua.get_node_type_description("function_definition"),
		"Function Definition"
	);
	assert_eq!(
		lua.get_node_type_description("local_function"),
		"Local Function"
	);
	assert_eq!(
		lua.get_node_type_description("assignment_statement"),
		"Variable Assignment"
	);
	assert_eq!(
		lua.get_node_type_description("local_declaration"),
		"Local Variable Declaration"
	);
	assert_eq!(
		lua.get_node_type_description("table_constructor"),
		"Table Constructor"
	);
	assert_eq!(lua.get_node_type_description("field"), "Table Field");
	assert_eq!(
		lua.get_node_type_description("return_statement"),
		"Return Statement"
	);
	assert_eq!(
		lua.get_node_type_description("function_call"),
		"Function Call"
	);
	assert_eq!(lua.get_node_type_description("identifier"), "Identifier");
	assert_eq!(lua.get_node_type_description("block"), "Code Block");
	assert_eq!(
		lua.get_node_type_description("variable_list"),
		"Variable List"
	);
	assert_eq!(
		lua.get_node_type_description("expression_list"),
		"Expression List"
	);
	assert_eq!(lua.get_node_type_description("chunk"), "Unknown Node Type");
}

#[test]
fn the_three_function_kinds_are_mutually_equivalent() {
	let lua = Lua {};
	for (first, second) in [
		("function_declaration", "function_definition"),
		("function_definition", "function_declaration"),
		("function_declaration", "local_function"),
		("local_function", "function_declaration"),
		("function_definition", "local_function"),
		("local_function", "function_definition"),
		("function_call", "function_call"),
	] {
		assert!(
			lua.are_node_types_equivalent(first, second),
			"{first} and {second} should be equivalent"
		);
	}
	assert!(!lua.are_node_types_equivalent("function_declaration", "function_call"));
	assert!(!lua.are_node_types_equivalent("block", "chunk"));
}

#[test]
fn a_local_function_contributes_its_own_name_and_its_body_locals() {
	let source = "local function helper(a, b)\n\tlocal sum\n\tsum = a + b\n\treturn sum\nend\n";
	let tree = parse_tree(source);
	let node = first_node(&tree, "function_declaration");
	assert_eq!(
		Lua {}.extract_symbols(node, source),
		vec!["helper".to_string(), "sum".to_string()]
	);
}

#[test]
fn a_dotted_function_name_contributes_its_name_and_owner() {
	// `M.encode` is a `dot_index_expression`, not a bare `identifier`, so the
	// declared name has to come from the `name` field.
	let source = "local M = {}\nfunction M.encode(value)\n\tlocal cache\n\tcache = value\n\treturn cache\nend\n";
	let tree = parse_tree(source);
	let node = first_node(&tree, "function_declaration");
	assert_eq!(
		Lua {}.extract_symbols(node, source),
		vec!["M".to_string(), "cache".to_string(), "encode".to_string()]
	);
}

#[test]
fn an_initialised_local_is_collected_by_the_body_scan() {
	// `local x = 1` parses as variable_declaration > assignment_statement >
	// variable_list, so the inner level has to be walked too.
	let source = "local function build()\n\tlocal initialized = 1\n\tlocal uninitialized\n\tassigned = 2\n\treturn initialized\nend\n";
	let tree = parse_tree(source);
	let node = first_node(&tree, "function_declaration");
	assert_eq!(
		Lua {}.extract_symbols(node, source),
		vec![
			"assigned".to_string(),
			"build".to_string(),
			"initialized".to_string(),
			"uninitialized".to_string(),
		]
	);
}

#[test]
fn a_qualified_declaration_splits_into_a_name_and_an_owner() {
	let lua = Lua {};
	for (source, name, owner) in [
		("local function helper()\n\treturn 1\nend\n", "helper", None),
		(
			"function M.encode(v)\n\treturn v\nend\n",
			"encode",
			Some("M".to_string()),
		),
		(
			"function M:decode(v)\n\treturn v\nend\n",
			"decode",
			Some("M".to_string()),
		),
	] {
		let tree = parse_tree(source);
		let node = first_node(&tree, "function_declaration");
		assert_eq!(
			lua.extract_declaration_name(node, source),
			Some(name.to_string())
		);
		assert_eq!(lua.extract_symbol_owner(node, source), owner);
	}
}

#[test]
fn identifier_extraction_keeps_every_occurrence() {
	// Unlike the other languages' extractors this one does not deduplicate;
	// `extract_symbols` sorts and dedups afterwards instead.
	let source = "local a = a\n";
	let tree = parse_tree(source);
	let mut symbols = Vec::new();
	Lua {}.extract_identifiers(tree.root_node(), source, &mut symbols);
	assert_eq!(symbols, vec!["a".to_string(), "a".to_string()]);
}

#[test]
fn every_require_spelling_is_recorded_as_an_import() {
	let lua = Lua {};
	let source =
		"local a = require('json')\nlocal b = require \"pkg.sub\"\nlocal c = require(\"utils\")\n";
	let tree = parse_tree(source);
	for (index, expected) in ["json", "pkg.sub", "utils"].into_iter().enumerate() {
		let (imports, exports) =
			lua.extract_imports_exports(nth_node(&tree, "function_call", index), source);
		assert_eq!(imports, vec![expected.to_string()]);
		assert!(exports.is_empty());
	}
}

#[test]
fn a_call_that_is_not_require_contributes_no_imports() {
	let source = "local c = other(\"x\")\n";
	let tree = parse_tree(source);
	let node = first_node(&tree, "function_call");
	let (imports, exports) = Lua {}.extract_imports_exports(node, source);
	assert!(imports.is_empty());
	assert!(exports.is_empty());
}

#[test]
fn a_module_level_return_becomes_the_export_list() {
	let lua = Lua {};
	for (source, expected) in [
		("local M = {}\nreturn M\n", vec!["M".to_string()]),
		(
			"local a, b = 1, 2\nreturn a, b\n",
			vec!["a".to_string(), "b".to_string()],
		),
		(
			"return { name = \"x\", size = 2 }\n",
			vec!["name".to_string(), "size".to_string()],
		),
	] {
		let tree = parse_tree(source);
		let node = first_node(&tree, "return_statement");
		let (imports, exports) = lua.extract_imports_exports(node, source);
		assert!(imports.is_empty());
		assert_eq!(exports, expected, "unexpected exports for {source:?}");
	}
}

#[test]
fn a_table_field_whose_value_is_an_identifier_is_exported_twice() {
	// Both the field name and its identifier value are direct `identifier`
	// children of the `field` node, so each contributes an export.
	let source = "local add = 1\nreturn { add = add }\n";
	let tree = parse_tree(source);
	let node = first_node(&tree, "return_statement");
	let (_, exports) = Lua {}.extract_imports_exports(node, source);
	assert_eq!(exports, vec!["add".to_string(), "add".to_string()]);
}

#[test]
fn a_return_inside_a_function_is_not_a_module_export() {
	let source = "local function f()\n\treturn 1\nend\nreturn f\n";
	let tree = parse_tree(source);
	let lua = Lua {};
	let (_, inner) = lua.extract_imports_exports(nth_node(&tree, "return_statement", 0), source);
	assert!(inner.is_empty(), "function-local return should not export");
	let (_, outer) = lua.extract_imports_exports(nth_node(&tree, "return_statement", 1), source);
	assert_eq!(outer, vec!["f".to_string()]);
}

#[test]
fn calls_keep_their_table_as_qualifier_and_require_is_skipped() {
	let source = "obj:method(1)\nobj.field.deep(2)\nplain(3)\nrequire(\"json\")\n";
	let tree = parse_tree(source);
	let lua = Lua {};
	assert_eq!(
		lua.extract_function_calls(nth_node(&tree, "function_call", 0), source),
		vec![CallTarget {
			name: "method".to_string(),
			qualifier: Some("obj".to_string()),
		}]
	);
	assert_eq!(
		lua.extract_function_calls(nth_node(&tree, "function_call", 1), source),
		vec![CallTarget {
			name: "deep".to_string(),
			qualifier: Some("obj::field".to_string()),
		}]
	);
	assert_eq!(
		lua.extract_function_calls(nth_node(&tree, "function_call", 2), source),
		vec![CallTarget {
			name: "plain".to_string(),
			qualifier: None,
		}]
	);
	// `require` is an import, not a call edge.
	assert!(lua
		.extract_function_calls(nth_node(&tree, "function_call", 3), source)
		.is_empty());
}

#[test]
fn a_node_that_is_not_a_function_call_yields_no_call_targets() {
	let source = "local x = 1\n";
	let tree = parse_tree(source);
	let node = first_node(&tree, "variable_declaration");
	assert!(Lua {}.extract_function_calls(node, source).is_empty());
}

#[test]
fn a_relative_require_resolves_against_the_source_directory() {
	let files = registry(&["src/main.lua", "src/mod.lua", "src/utils/helpers.lua"]);
	assert_eq!(
		Lua {}.resolve_import("./mod", "src/main.lua", &files),
		Some("src/mod.lua".to_string())
	);
}

#[test]
fn a_dotted_module_path_maps_to_nested_directories() {
	let files = registry(&["src/main.lua", "src/utils/helpers.lua"]);
	assert_eq!(
		Lua {}.resolve_import("utils.helpers", "src/main.lua", &files),
		Some("src/utils/helpers.lua".to_string())
	);
}

#[test]
fn a_bare_module_name_resolves_to_a_sibling_file_or_its_init_lua() {
	let files = registry(&["src/main.lua", "src/json.lua", "src/pkg/init.lua"]);
	let lua = Lua {};
	assert_eq!(
		lua.resolve_import("json", "src/main.lua", &files),
		Some("src/json.lua".to_string())
	);
	assert_eq!(
		lua.resolve_import("pkg", "src/main.lua", &files),
		Some("src/pkg/init.lua".to_string())
	);
}

#[test]
fn an_unknown_module_resolves_to_nothing() {
	let files = registry(&["src/main.lua", "src/json.lua"]);
	assert_eq!(
		Lua {}.resolve_import("nonexistent", "src/main.lua", &files),
		None
	);
}

#[test]
fn each_top_level_function_becomes_its_own_region() {
	let source = "local function first()\n\tlocal a\n\ta = 1\n\ta = a + 1\n\treturn a\nend\n\nlocal function second()\n\tlocal b\n\tb = 2\n\tb = b + 2\n\treturn b\nend\n";
	let regions = parse_regions(source);
	assert_eq!(
		regions
			.iter()
			.map(|r| r.node_kind.as_str())
			.collect::<Vec<_>>(),
		vec!["function_declaration", "function_declaration"]
	);
	assert_eq!(
		regions[0].symbols,
		vec!["a".to_string(), "first".to_string()]
	);
	assert_eq!(
		regions[1].symbols,
		vec!["b".to_string(), "second".to_string()]
	);
}

#[test]
fn a_nested_function_stays_inside_its_parent_region() {
	let source = "local function outer()\n\tlocal total\n\ttotal = 1\n\tlocal function inner()\n\t\tlocal x\n\t\tx = 2\n\t\treturn x\n\tend\n\treturn inner\nend\n";
	let regions = parse_regions(source);
	assert_eq!(
		regions.len(),
		1,
		"nested functions must not split the parent"
	);
	assert_eq!(
		regions[0].symbols,
		vec![
			"inner".to_string(),
			"outer".to_string(),
			"total".to_string()
		]
	);
}

#[test]
fn an_anonymous_function_expression_is_still_a_region() {
	let source =
		"local handler = function(event)\n\tlocal out\n\tout = event\n\tout = out\n\treturn out\nend\n";
	let regions = parse_regions(source);
	assert_eq!(regions.len(), 1);
	assert_eq!(regions[0].node_kind, "function_definition");
	// The binding name lives outside the node, so only body locals are captured.
	assert_eq!(regions[0].symbols, vec!["out".to_string()]);
	assert!(regions[0].content.starts_with("function(event)"));
}

#[test]
fn consecutive_single_line_functions_merge_into_one_described_block() {
	let source = "function a() return 1 end\nfunction b() return 2 end\n";
	let regions = parse_regions(source);
	assert_eq!(regions.len(), 1);
	assert!(
		regions[0]
			.content
			.starts_with("// Merged Function Declaration (2 declarations)\n"),
		"merged block should carry the Lua description: {:?}",
		regions[0].content
	);
}
