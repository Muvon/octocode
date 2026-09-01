use crate::indexer::code_region_extractor::extract_meaningful_regions;
use crate::indexer::languages::ruby::Ruby;
use crate::indexer::languages::Language;
use tree_sitter::Parser;

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
