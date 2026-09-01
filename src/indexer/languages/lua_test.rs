use crate::indexer::languages::lua::Lua;
use crate::indexer::languages::Language;
use tree_sitter::{Node, Parser};

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
