use crate::indexer::code_region_extractor::extract_meaningful_regions;
use crate::indexer::languages::bash::Bash;
use crate::indexer::languages::Language;
use tree_sitter::{Node, Parser};

fn parse_regions(source: &str) -> Vec<crate::indexer::code_region_extractor::CodeRegion> {
	let bash_lang = Bash {};
	let mut parser = Parser::new();
	parser.set_language(&bash_lang.get_ts_language()).unwrap();

	let tree = parser.parse(source, None).unwrap();
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), source, &bash_lang, &mut regions);
	regions
}

#[test]
fn test_heredoc_body_is_included_in_region() {
	let source = "cat <<EOF > out.txt\nsome line here\nEOF\n";

	let regions = parse_regions(source);

	let matching: Vec<_> = regions
		.iter()
		.filter(|r| r.content.contains("some line here"))
		.collect();
	assert_eq!(
		matching.len(),
		1,
		"expected exactly one region containing the heredoc body, got {} (regions: {:?})",
		matching.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
}

#[test]
fn test_plain_redirect_produces_exactly_one_region() {
	let source = "echo hi > file.txt\n";

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"a plain redirected command should still produce exactly one region (no double-counting), got {} (regions: {:?})",
		regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert!(regions[0].content.contains("echo hi > file.txt"));
}

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
fn test_nested_function_source_detected_exactly_once() {
	let source = r#"outer() {
    source outer_lib.sh
    inner() {
        source inner_lib.sh
    }
}
"#;
	let bash_lang = Bash {};
	let mut parser = Parser::new();
	parser.set_language(&bash_lang.get_ts_language()).unwrap();
	let tree = parser.parse(source, None).unwrap();

	let imports = collect_all_imports_via_full_tree_walk(source, &bash_lang, tree.root_node());

	let inner_count = imports
		.iter()
		.filter(|s| s.as_str() == "inner_lib.sh")
		.count();
	assert_eq!(
		inner_count, 1,
		"nested function's own source statement should be detected exactly once, got {} in {:?}",
		inner_count, imports
	);

	let outer_count = imports
		.iter()
		.filter(|s| s.as_str() == "outer_lib.sh")
		.count();
	assert_eq!(
		outer_count, 1,
		"outer function's source statement should be detected exactly once, got {} in {:?}",
		outer_count, imports
	);
}
