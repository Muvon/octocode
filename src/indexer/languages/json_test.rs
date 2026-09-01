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
