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

fn parse_tree(source: &str) -> tree_sitter::Tree {
	let mut parser = Parser::new();
	parser.set_language(&Bash {}.get_ts_language()).unwrap();
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

fn first_of_kind<'a>(tree: &'a tree_sitter::Tree, kind: &str) -> Node<'a> {
	let mut found = Vec::new();
	nodes_of_kind(tree.root_node(), kind, &mut found);
	*found
		.first()
		.unwrap_or_else(|| panic!("no {kind} node in source"))
}

fn registry(files: &[&str]) -> crate::indexer::languages::resolution_utils::FileRegistry {
	let owned: Vec<String> = files.iter().map(|f| f.to_string()).collect();
	crate::indexer::languages::resolution_utils::FileRegistry::new(&owned)
}

#[test]
fn language_metadata_is_wired_up() {
	let bash = Bash {};
	assert_eq!(bash.name(), "bash");
	assert_eq!(bash.get_file_extensions(), vec!["sh", "bash"]);
	assert!(bash
		.get_meaningful_kinds()
		.contains(&"redirected_statement"));
	// Only functions declare a symbol; a bare command declares nothing.
	assert_eq!(bash.get_symbol_kinds(), vec!["function_definition"]);
}

#[test]
fn node_type_descriptions_cover_every_arm() {
	let bash = Bash {};
	assert_eq!(
		bash.get_node_type_description("function_definition"),
		"function declarations"
	);
	assert_eq!(
		bash.get_node_type_description("variable_assignment"),
		"variable assignments"
	);
	assert_eq!(
		bash.get_node_type_description("command"),
		"command declarations"
	);
	assert_eq!(
		bash.get_node_type_description("simple_command"),
		"command declarations"
	);
	assert_eq!(bash.get_node_type_description("whatever"), "declarations");
}

#[test]
fn commands_form_one_semantic_group_and_functions_another() {
	let bash = Bash {};
	assert!(bash.are_node_types_equivalent("command", "simple_command"));
	assert!(bash.are_node_types_equivalent("function_definition", "function_definition"));
	assert!(!bash.are_node_types_equivalent("command", "function_definition"));
}

#[test]
fn a_function_name_comes_from_the_name_field() {
	let source = "run_build() {
	echo hi
}
";
	let tree = parse_tree(source);
	let node = first_of_kind(&tree, "function_definition");
	assert_eq!(
		Bash {}.extract_declaration_name(node, source).as_deref(),
		Some("run_build")
	);

	// A command node declares nothing, so the default scan must not fire.
	let command_source = "echo hi
";
	let command_tree = parse_tree(command_source);
	assert_eq!(
		Bash {}.extract_declaration_name(first_of_kind(&command_tree, "command"), command_source),
		None
	);
}

#[test]
fn function_symbols_include_the_name_and_its_local_assignments() {
	let source = "run_build() {
	TARGET=release
	OUT=dist
	echo $TARGET
}
";
	let tree = parse_tree(source);
	let symbols = Bash {}.extract_symbols(first_of_kind(&tree, "function_definition"), source);
	assert!(symbols.contains(&"run_build".to_string()), "{symbols:?}");
	assert!(symbols.contains(&"TARGET".to_string()), "{symbols:?}");
	assert!(symbols.contains(&"OUT".to_string()), "{symbols:?}");
}

#[test]
fn a_non_function_node_falls_back_to_command_and_variable_names() {
	let source = "echo \"$HOME\"\n";
	let tree = parse_tree(source);
	let symbols = Bash {}.extract_symbols(first_of_kind(&tree, "command"), source);
	assert!(symbols.contains(&"echo".to_string()), "{symbols:?}");
}

#[test]
fn source_statements_inside_a_function_become_imports() {
	let source = "setup() {
	source ./lib/helpers.sh
	. \"./lib/other.sh\"
}
";
	let tree = parse_tree(source);
	let (imports, exports) =
		Bash {}.extract_imports_exports(first_of_kind(&tree, "function_definition"), source);
	assert!(
		imports.contains(&"./lib/helpers.sh".to_string()),
		"{imports:?}"
	);
	assert!(
		imports.contains(&"./lib/other.sh".to_string()),
		"{imports:?}"
	);
	assert!(exports.is_empty(), "bash has no explicit exports");
}

#[test]
fn a_non_function_node_reports_no_imports() {
	let source = "source ./lib/helpers.sh
";
	let tree = parse_tree(source);
	let (imports, _) = Bash {}.extract_imports_exports(first_of_kind(&tree, "command"), source);
	assert!(imports.is_empty());
}

#[test]
fn a_command_becomes_a_call_target_but_source_does_not() {
	let bash = Bash {};

	let source = "deploy --now
";
	let tree = parse_tree(source);
	let calls = bash.extract_function_calls(first_of_kind(&tree, "command"), source);
	assert_eq!(calls.len(), 1);
	assert_eq!(calls[0].name, "deploy");

	for text in ["source ./a.sh\n", ". ./a.sh\n"] {
		let tree = parse_tree(text);
		assert!(bash
			.extract_function_calls(first_of_kind(&tree, "command"), text)
			.is_empty());
	}

	// A leading environment assignment must not be mistaken for the command.
	let prefixed = "DEBUG=1 deploy
";
	let tree = parse_tree(prefixed);
	let calls = bash.extract_function_calls(first_of_kind(&tree, "command"), prefixed);
	assert_eq!(calls[0].name, "deploy");
}

#[test]
fn a_non_command_node_yields_no_calls() {
	let source = "run() {
	echo hi
}
";
	let tree = parse_tree(source);
	assert!(Bash {}
		.extract_function_calls(first_of_kind(&tree, "function_definition"), source)
		.is_empty());
}

#[test]
fn relative_and_absolute_sources_resolve_against_the_registry() {
	let bash = Bash {};
	let files = registry(&["scripts/build.sh", "scripts/lib/helpers.sh"]);

	assert_eq!(
		bash.resolve_import("./lib/helpers.sh", "scripts/build.sh", &files)
			.as_deref(),
		Some("scripts/lib/helpers.sh")
	);
	assert_eq!(
		bash.resolve_import("scripts/lib/helpers.sh", "scripts/build.sh", &files)
			.as_deref(),
		Some("scripts/lib/helpers.sh")
	);
	assert_eq!(
		bash.resolve_import("./nowhere.sh", "scripts/build.sh", &files),
		None
	);
}
