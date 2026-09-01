use crate::indexer::code_region_extractor::extract_meaningful_regions;
use crate::indexer::languages::rust::Rust;
use crate::indexer::languages::Language;
use tree_sitter::Parser;

fn parse_regions(source: &str) -> Vec<crate::indexer::code_region_extractor::CodeRegion> {
	let rust_lang = Rust {};
	let mut parser = Parser::new();
	parser.set_language(&rust_lang.get_ts_language()).unwrap();

	let tree = parser.parse(source, None).unwrap();
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), source, &rust_lang, &mut regions);
	regions
}

#[test]
fn test_inline_mod_splits_into_individual_functions() {
	// Bodies are multi-line/non-trivial so the smart single-line merge pass
	// (unrelated to this fix) doesn't recombine the two functions into one
	// "// Merged ..." block, which would defeat this test's purpose.
	let source = r#"
mod foo {
	fn a() {
		let x = 1;
		let y = 2;
		x + y
	}
	fn b() {
		let x = 3;
		let y = 4;
		x + y
	}
}
"#;

	let regions = parse_regions(source);

	let function_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "function_item")
		.collect();
	assert_eq!(
		function_regions.len(),
		2,
		"expected 2 function_item regions inside the mod, got {} (regions: {:?})",
		function_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);

	let mod_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "mod_item")
		.collect();
	assert!(
		mod_regions.is_empty(),
		"mod body should not collapse into a single mod_item blob region, got: {:?}",
		mod_regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
}

#[test]
fn test_bodyless_mod_still_produces_its_own_region() {
	let source = "mod foo;\n";

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"a bodyless mod should still produce exactly one fallback region, got {} (regions: {:?})",
		regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "mod_item");
	assert!(regions[0].content.contains("mod foo;"));
}

#[test]
fn test_trait_with_default_method_splits_into_method_region() {
	let source = r#"
trait Greeter {
	fn greet(&self) {
		println!("hi");
	}
}
"#;

	let regions = parse_regions(source);

	let function_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "function_item")
		.collect();
	assert_eq!(
		function_regions.len(),
		1,
		"expected the default method to become its own function_item region, got {} (regions: {:?})",
		function_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);

	let trait_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "trait_item")
		.collect();
	assert!(
		trait_regions.is_empty(),
		"trait with a default method should not also collapse into a whole-trait region, got: {:?}",
		trait_regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
}

#[test]
fn test_signature_only_trait_still_produces_one_whole_trait_region() {
	let source = r#"
trait Greeter {
	fn greet(&self);
}
"#;

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"a signature-only trait should still produce exactly one fallback region, got {} (regions: {:?})",
		regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "trait_item");
	assert!(regions[0].content.contains("fn greet(&self);"));
}

#[test]
fn test_preceding_comment_attaches_to_following_function() {
	let source = r#"
// This function does the thing
fn foo() {
	let x = 1;
	let y = 2;
	x + y
}
"#;

	let regions = parse_regions(source);
	let function_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "function_item")
		.collect();
	assert_eq!(
		function_regions.len(),
		1,
		"expected exactly one function_item region, got {} (regions: {:?})",
		function_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert!(
		function_regions[0]
			.content
			.contains("// This function does the thing"),
		"preceding doc comment should be attached to the function region, got: {:?}",
		function_regions[0].content
	);
}

#[test]
fn test_function_without_preceding_comment_does_not_absorb_prior_sibling() {
	let source = r#"
fn bar() {
	let a = 10;
	let b = 20;
	a + b
}

fn baz() {
	let c = 30;
	let d = 40;
	c + d
}
"#;

	let regions = parse_regions(source);
	let function_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "function_item")
		.collect();
	assert_eq!(
		function_regions.len(),
		2,
		"expected 2 function_item regions, got {} (regions: {:?})",
		function_regions.len(),
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);

	let baz_region = function_regions
		.iter()
		.find(|r| r.content.contains("fn baz()"))
		.expect("baz region should exist");
	assert!(
		!baz_region.content.contains("fn bar()"),
		"baz's region should not absorb the unrelated preceding sibling bar's content: {:?}",
		baz_region.content
	);
}
