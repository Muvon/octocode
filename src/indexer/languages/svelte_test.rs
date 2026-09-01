use crate::indexer::code_region_extractor::extract_meaningful_regions;
use crate::indexer::languages::svelte::Svelte;
use crate::indexer::languages::Language;
use tree_sitter::Parser;

fn parse_regions(source: &str) -> Vec<crate::indexer::code_region_extractor::CodeRegion> {
	let svelte_lang = Svelte {};
	let mut parser = Parser::new();
	parser.set_language(&svelte_lang.get_ts_language()).unwrap();

	let tree = parser.parse(source, None).unwrap();
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), source, &svelte_lang, &mut regions);
	regions
}

#[test]
fn test_script_block_splits_into_individual_functions() {
	let source = r#"<script>
	function calculateTotal(items) {
		let total = 0;
		for (const item of items) {
			total += item.price;
		}
		return total;
	}

	function formatCurrency(amount) {
		const rounded = Math.round(amount * 100) / 100;
		return '$' + rounded.toFixed(2);
	}
</script>
"#;

	let regions = parse_regions(source);

	let function_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "function_declaration")
		.collect();

	assert!(
		function_regions.len() >= 2,
		"expected at least 2 function_declaration regions from the real JS grammar, got {} (all regions: {:?})",
		function_regions.len(),
		regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
	);

	let script_blob_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "script_element")
		.collect();
	assert!(
		script_blob_regions.is_empty(),
		"script content should not collapse into a single script_element blob region"
	);
}

#[test]
fn test_plain_wrappers_around_single_component_produce_no_region_of_their_own() {
	let source = r#"<div>
	<div>
		<div>
			<Button on:click={handleClick}>Click me</Button>
		</div>
	</div>
</div>
"#;

	let regions = parse_regions(source);

	// No region should contain the outer wrapper's own text (i.e. all three
	// nested <div> layers), which would indicate the old blob behavior.
	for region in &regions {
		assert!(
			!region.content.contains("<div>\n\t<div>"),
			"a plain wrapper div should not produce a region containing the whole nested wrapper subtree, got: {}",
			region.content
		);
	}

	// Exactly the inner Button element should have become its own region.
	let button_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.content.contains("on:click"))
		.collect();
	assert_eq!(
		button_regions.len(),
		1,
		"expected exactly one region for the inner <Button on:click>, got {} (regions: {:?})",
		button_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert!(
		!button_regions[0].content.contains("<div>"),
		"the Button's region should not include any of the outer wrapper divs, got: {}",
		button_regions[0].content
	);
}

#[test]
fn test_meaningful_wrapper_with_nested_meaningful_child_only_keeps_the_inner_region() {
	// Accepted tradeoff: the outer <div on:click> is itself meaningful (has
	// its own directive), but because a nested meaningful element is found
	// first, only the inner element becomes a region — the outer wrapper's
	// own directive is not separately captured. See descend_first_kinds
	// fallback rule in code_region_extractor.rs.
	let source = r#"<div on:click={outerHandler}>
	<Button on:click={innerHandler}>Click me</Button>
</div>
"#;

	let regions = parse_regions(source);

	let element_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "element")
		.collect();

	assert_eq!(
		element_regions.len(),
		1,
		"expected only the inner element to become a region, got {} (regions: {:?})",
		element_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert!(
		element_regions[0].content.contains("innerHandler"),
		"the surviving region should be the inner Button, got: {}",
		element_regions[0].content
	);
	assert!(
		!element_regions[0].content.contains("outerHandler"),
		"the outer wrapper's own directive is expected to be dropped by design, got: {}",
		element_regions[0].content
	);
}

#[test]
fn test_plain_wrapper_with_no_meaningful_descendant_falls_back_to_one_region() {
	let source = r#"<div>
	{someLongExpressionThatRepresentsNonTrivialInlineContentInThisTemplate}
</div>
"#;

	let regions = parse_regions(source);

	let element_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "element")
		.collect();

	assert_eq!(
		element_regions.len(),
		1,
		"a plain wrapper with no meaningful descendant should still get exactly one fallback region so its content isn't lost, got {} (regions: {:?})",
		element_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert!(
		element_regions[0]
			.content
			.contains("someLongExpressionThatRepresentsNonTrivialInlineContentInThisTemplate"),
		"the fallback region should contain the div's inline content, got: {}",
		element_regions[0].content
	);
}

#[test]
fn test_style_block_splits_into_individual_rules() {
	let source = r#"<style>
	.title {
		color: red;
		font-size: 2rem;
		margin-bottom: 1rem;
	}
	.subtitle {
		color: blue;
		font-weight: 600;
	}
</style>
"#;

	let regions = parse_regions(source);

	let rule_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "rule_set")
		.collect();

	assert!(
		rule_regions.len() >= 2,
		"expected at least 2 rule_set regions from the real CSS grammar, got {} (all regions: {:?})",
		rule_regions.len(),
		regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
	);

	let style_blob_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "style_element")
		.collect();
	assert!(
		style_blob_regions.is_empty(),
		"style content should not collapse into a single style_element blob region"
	);
}
