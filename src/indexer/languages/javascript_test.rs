use crate::indexer::code_region_extractor::extract_meaningful_regions;
use crate::indexer::languages::javascript::JavaScript;
use crate::indexer::languages::Language;
use tree_sitter::Parser;

fn parse_regions(source: &str) -> Vec<crate::indexer::code_region_extractor::CodeRegion> {
	let js_lang = JavaScript {};
	let mut parser = Parser::new();
	parser.set_language(&js_lang.get_ts_language()).unwrap();

	let tree = parser.parse(source, None).unwrap();
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), source, &js_lang, &mut regions);
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
