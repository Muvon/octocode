use crate::indexer::code_region_extractor::extract_meaningful_regions;
use crate::indexer::languages::json::Json;
use crate::indexer::languages::Language;
use tree_sitter::Parser;

fn parse_regions(source: &str) -> Vec<crate::indexer::code_region_extractor::CodeRegion> {
	let json_lang = Json {};
	let mut parser = Parser::new();
	parser.set_language(&json_lang.get_ts_language()).unwrap();

	let tree = parser.parse(source, None).unwrap();
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), source, &json_lang, &mut regions);
	regions
}

#[test]
fn test_nested_object_splits_into_multiple_regions() {
	// Both top-level values are containers (not a bare scalar) so each gets
	// its own fallback region when it has no further nesting itself; a bare
	// scalar sibling would have no container to descend into and would be
	// silently dropped when its sibling's nested content is found first —
	// see the flat-object gap documented on Json::descend_first_kinds. Each
	// nested object spans more than 2 lines so the smart single-line merge
	// pass (unrelated to this fix) doesn't recombine them into one
	// "// Merged ..." block, which would defeat this test's purpose.
	let source = r#"{
  "a": {
    "b": 1,
    "c": 2
  },
  "d": {
    "e": 4,
    "f": 5
  }
}"#;

	let regions = parse_regions(source);

	assert!(
		regions.len() > 1,
		"expected more than one region for a nested object, got {} (regions: {:?})",
		regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);

	let nested_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.content.contains("\"b\"") && r.content.contains("\"c\""))
		.collect();
	assert_eq!(
		nested_regions.len(),
		1,
		"expected exactly one region for the nested {{\"b\":1,\"c\":2}} object, got {} (regions: {:?})",
		nested_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
}

#[test]
fn test_flat_object_with_no_nesting_still_produces_one_region() {
	let source = r#"{"a": 1, "b": 2, "c": 3}"#;

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"a flat object with no nested object/array should still produce exactly one region so content isn't lost, got {} (regions: {:?})",
		regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert!(regions[0].content.contains("\"a\": 1"));
	assert!(regions[0].content.contains("\"c\": 3"));
}

fn parse_tree(source: &str) -> tree_sitter::Tree {
	let mut parser = Parser::new();
	parser.set_language(&Json {}.get_ts_language()).unwrap();
	parser.parse(source, None).unwrap()
}

fn first_of_kind<'a>(tree: &'a tree_sitter::Tree, kind: &str) -> tree_sitter::Node<'a> {
	fn walk<'a>(node: tree_sitter::Node<'a>, kind: &str, out: &mut Option<tree_sitter::Node<'a>>) {
		if out.is_none() && node.kind() == kind {
			*out = Some(node);
		}
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			walk(child, kind, out);
		}
	}
	let mut found = None;
	walk(tree.root_node(), kind, &mut found);
	found.unwrap_or_else(|| panic!("no {kind} node in source"))
}

#[test]
fn language_metadata_is_wired_up() {
	let json = Json {};
	assert_eq!(json.name(), "json");
	assert_eq!(json.get_file_extensions(), vec!["json"]);
	assert_eq!(json.get_meaningful_kinds(), vec!["object", "array"]);
	// Containers are descended first so a whole file does not become one region.
	assert_eq!(json.descend_first_kinds(), vec!["object", "array"]);
}

#[test]
fn node_type_descriptions_cover_every_arm() {
	let json = Json {};
	assert_eq!(json.get_node_type_description("object"), "JSON objects");
	assert_eq!(json.get_node_type_description("array"), "JSON arrays");
	assert_eq!(json.get_node_type_description("string"), "JSON strings");
	assert_eq!(json.get_node_type_description("number"), "JSON numbers");
	assert_eq!(json.get_node_type_description("true"), "JSON booleans");
	assert_eq!(json.get_node_type_description("false"), "JSON booleans");
	assert_eq!(json.get_node_type_description("null"), "JSON null values");
	assert_eq!(
		json.get_node_type_description("pair"),
		"JSON key-value pairs"
	);
	assert_eq!(
		json.get_node_type_description("whatever"),
		"JSON structures"
	);
}

#[test]
fn structures_and_scalars_form_two_semantic_groups() {
	let json = Json {};
	assert!(json.are_node_types_equivalent("object", "array"));
	assert!(json.are_node_types_equivalent("string", "number"));
	assert!(json.are_node_types_equivalent("true", "null"));
	assert!(json.are_node_types_equivalent("pair", "pair"));
	assert!(!json.are_node_types_equivalent("object", "string"));
}

#[test]
fn object_keys_are_extracted_recursively_and_deduplicated() {
	let source = r#"{
  "name": "octocode",
  "nested": {
    "name": "inner",
    "depth": 2
  },
  "list": [
    { "item": 1 }
  ]
}"#;
	let tree = parse_tree(source);
	let symbols = Json {}.extract_symbols(first_of_kind(&tree, "object"), source);
	// `item` is absent: `extract_json_keys` descends into an array value but then
	// only looks for `pair` children, so keys of objects nested inside arrays are
	// not reached. Known gap, asserted so a change to it is visible.
	assert_eq!(
		symbols,
		vec![
			"depth".to_string(),
			"list".to_string(),
			"name".to_string(),
			"nested".to_string(),
		],
		"keys must be sorted and deduplicated"
	);
}

#[test]
fn a_non_object_node_falls_back_to_the_identifier_walk() {
	// The fallback walk keys off `parent_kind == "pair"`, which is true for the
	// value string as well as the key, so both come back.
	let source = r#"[{ "key": "value" }]"#;
	let tree = parse_tree(source);
	let symbols = Json {}.extract_symbols(first_of_kind(&tree, "array"), source);
	assert_eq!(symbols, vec!["key".to_string(), "value".to_string()]);
}

#[test]
fn an_object_node_collects_keys_only() {
	// The object path uses `extract_json_keys`, which takes the first child of
	// each pair — so unlike the identifier fallback it never picks up values.
	let source = r#"{ "key": "value" }"#;
	let tree = parse_tree(source);
	let symbols = Json {}.extract_symbols(first_of_kind(&tree, "object"), source);
	assert_eq!(symbols, vec!["key".to_string()]);
}

#[test]
fn an_empty_key_is_dropped() {
	let source = r#"{ "": 1, "kept": 2 }"#;
	let tree = parse_tree(source);
	let symbols = Json {}.extract_symbols(first_of_kind(&tree, "object"), source);
	assert_eq!(symbols, vec!["kept".to_string()]);
}

#[test]
fn json_declares_no_imports_to_resolve() {
	let files = vec!["a.json".to_string()];
	let registry = crate::indexer::languages::resolution_utils::FileRegistry::new(&files);
	assert_eq!(
		Json {}.resolve_import("./b.json", "a.json", &registry),
		None
	);
}
