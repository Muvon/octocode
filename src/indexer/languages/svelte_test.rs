use crate::indexer::code_region_extractor::extract_meaningful_regions;
use crate::indexer::languages::resolution_utils::FileRegistry;
use crate::indexer::languages::svelte::Svelte;
use crate::indexer::languages::Language;
use tree_sitter::{Node, Parser, Tree};

fn parse_regions(source: &str) -> Vec<crate::indexer::code_region_extractor::CodeRegion> {
	let svelte_lang = Svelte {};
	let mut parser = Parser::new();
	parser.set_language(&svelte_lang.get_ts_language()).unwrap();

	let tree = parser.parse(source, None).unwrap();
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), source, &svelte_lang, &mut regions);
	regions
}

fn parse_tree(source: &str) -> Tree {
	let mut parser = Parser::new();
	parser.set_language(&Svelte {}.get_ts_language()).unwrap();
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

fn all_of_kind<'a>(tree: &'a Tree, kind: &str) -> Vec<Node<'a>> {
	let mut found = Vec::new();
	nodes_of_kind(tree.root_node(), kind, &mut found);
	found
}

fn first_of_kind<'a>(tree: &'a Tree, kind: &str) -> Node<'a> {
	all_of_kind(tree, kind)
		.into_iter()
		.next()
		.unwrap_or_else(|| panic!("no `{kind}` node in the parsed tree"))
}

fn as_strs(values: &[String]) -> Vec<&str> {
	values.iter().map(String::as_str).collect()
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

#[test]
fn language_metadata_matches_the_svelte_grammar() {
	let svelte = Svelte {};

	assert_eq!(svelte.name(), "svelte");
	assert_eq!(svelte.get_file_extensions(), ["svelte"]);
	assert_eq!(
		svelte.get_meaningful_kinds(),
		["script_element", "style_element", "element"]
	);
	assert_eq!(svelte.descend_first_kinds(), ["element"]);
	// Script declarations become graph symbols through the embedded
	// JavaScript/TypeScript sources, so Svelte contributes none of its own.
	assert!(svelte.get_symbol_kinds().is_empty());
}

#[test]
fn node_type_descriptions_cover_every_svelte_group() {
	let svelte = Svelte {};

	for kind in [
		"function_declaration",
		"method_definition",
		"arrow_function",
	] {
		assert_eq!(
			svelte.get_node_type_description(kind),
			"function declarations"
		);
	}
	for kind in ["variable_declaration", "lexical_declaration"] {
		assert_eq!(
			svelte.get_node_type_description(kind),
			"variable declarations"
		);
	}
	for kind in ["reactive_statement", "reactive_declaration"] {
		assert_eq!(
			svelte.get_node_type_description(kind),
			"reactive declarations"
		);
	}
	assert_eq!(
		svelte.get_node_type_description("class_declaration"),
		"class declarations"
	);
	for kind in ["component", "element"] {
		assert_eq!(
			svelte.get_node_type_description(kind),
			"component declarations"
		);
	}
	assert_eq!(
		svelte.get_node_type_description("script_element"),
		"script blocks"
	);
	assert_eq!(
		svelte.get_node_type_description("style_element"),
		"style blocks"
	);
	// Unknown kinds fall back rather than borrowing the shared trait default,
	// so even "function"-containing kinds land on the generic label here.
	assert_eq!(svelte.get_node_type_description("raw_text"), "declarations");
	assert_eq!(
		svelte.get_node_type_description("function_expression"),
		"declarations"
	);
}

#[test]
fn equivalent_node_types_follow_the_svelte_semantic_groups() {
	let svelte = Svelte {};

	assert!(svelte.are_node_types_equivalent("raw_text", "raw_text"));
	assert!(svelte.are_node_types_equivalent("function_declaration", "arrow_function"));
	assert!(svelte.are_node_types_equivalent("method_definition", "function_declaration"));
	assert!(svelte.are_node_types_equivalent("variable_declaration", "lexical_declaration"));
	assert!(svelte.are_node_types_equivalent("lexical_declaration", "reactive_declaration"));
	assert!(svelte.are_node_types_equivalent("reactive_statement", "reactive_declaration"));
	assert!(svelte.are_node_types_equivalent("component", "element"));
	assert!(svelte.are_node_types_equivalent("script_element", "style_element"));

	assert!(!svelte.are_node_types_equivalent("element", "function_declaration"));
	assert!(!svelte.are_node_types_equivalent("script_element", "element"));
	assert!(!svelte.are_node_types_equivalent("reactive_statement", "variable_declaration"));
}

#[test]
fn script_symbols_come_from_line_based_javascript_patterns() {
	let source = r#"<script>
	import { onMount } from 'svelte';
	export let title = 'hello';
	export const version = '1.0';
	let count = 0;
	function greet(who) {
		return 'hi ' + who;
	}
	$: doubled = count * 2;
	$: if (count > 10) {
		flag();
	}
</script>
"#;

	let tree = parse_tree(source);
	let script = first_of_kind(&tree, "script_element");
	let symbols = Svelte {}.extract_symbols(script, source);

	// `$: if (...)` has no assignment, so the reactive branch emits a marker
	// name instead of a real identifier.
	assert_eq!(
		as_strs(&symbols),
		[
			"count",
			"doubled",
			"greet",
			"reactive_if",
			"title",
			"version"
		]
	);
}

#[test]
fn destructured_declarations_yield_each_bound_name() {
	let source = r#"<script>
	let { count = 0, name } = $props();
	const [first, second] = pair();
	const { key: renamed, ...rest } = obj;
	export let { a, b } = init();
</script>
"#;

	let tree = parse_tree(source);
	let script = first_of_kind(&tree, "script_element");
	let symbols = Svelte {}.extract_symbols(script, source);

	// Defaults inside the pattern (`count = 0`) must not be mistaken for the
	// assignment operator, renames keep the local binding, rest elements drop
	// their leading dots.
	assert_eq!(
		as_strs(&symbols),
		["a", "b", "count", "first", "name", "renamed", "rest", "second"]
	);
}

#[test]
fn declarations_without_a_spaced_initializer_are_skipped() {
	let source = r#"<script>
	let bare;
	let tight=1;
	let ok = 1;
	var also = 2;
</script>
"#;

	let tree = parse_tree(source);
	let script = first_of_kind(&tree, "script_element");
	let symbols = Svelte {}.extract_symbols(script, source);

	// The text scanner requires at least three whitespace-separated tokens
	// after the keyword, so `let tight=1;` is dropped along with the
	// genuinely nameless `let bare;`.
	assert_eq!(as_strs(&symbols), ["also", "ok"]);
}

#[test]
fn style_symbols_are_selectors_without_a_colon() {
	let source = r#"<style>
	.title, .subtitle {
		color: red;
	}
	a:hover {
		color: blue;
	}
	div {
		margin: 0;
	}
</style>
"#;

	let tree = parse_tree(source);
	let style = first_of_kind(&tree, "style_element");
	let symbols = Svelte {}.extract_symbols(style, source);

	// Any selector containing ':' is skipped, so pseudo-class rules such as
	// `a:hover` never become symbols.
	assert_eq!(as_strs(&symbols), [".subtitle", ".title", "div"]);
}

#[test]
fn element_symbols_are_component_names_and_directive_attributes() {
	let source = r#"<div class="wrapper" id="main">
	<Card on:click={handle} bind:value={v}>x</Card>
	<Header>y</Header>
	<Modal on:close={h} />
</div>
"#;

	let tree = parse_tree(source);
	let svelte = Svelte {};
	let elements = all_of_kind(&tree, "element");
	assert_eq!(elements.len(), 4);

	// Plain wrapper: no directive attributes, lowercase tag name.
	assert!(svelte.extract_symbols(elements[0], source).is_empty());
	assert!(!svelte.is_meaningful_node(elements[0], source));

	assert_eq!(
		as_strs(&svelte.extract_symbols(elements[1], source)),
		["Card", "bind:value", "on:click"]
	);
	assert!(svelte.is_meaningful_node(elements[1], source));

	// `<Header>` is a capitalized component, but `is_html_tag` lowercases
	// before matching so it collides with the HTML `header` element and is
	// dropped. Asserting the behaviour as written, not as intended.
	assert!(svelte.extract_symbols(elements[2], source).is_empty());

	// A self-closing tag parses as `self_closing_tag`; walking only `start_tag`
	// lost the component name and every directive on it.
	assert!(!svelte.extract_symbols(elements[3], source).is_empty());
	assert!(svelte.is_meaningful_node(elements[3], source));
}

#[test]
fn identifier_fallback_collects_attribute_names_but_not_tag_names() {
	let source = r#"<div class="wrapper" id="main">text</div>
"#;

	let tree = parse_tree(source);
	let start_tag = first_of_kind(&tree, "start_tag");

	// `start_tag` is not one of the three handled kinds, so extract_symbols
	// falls through to the generic identifier walk. `class` is filtered out as a
	// JavaScript keyword, so only `id` survives.
	assert_eq!(
		as_strs(&Svelte {}.extract_symbols(start_tag, source)),
		["id"]
	);
}

#[test]
fn script_imports_keep_their_path_when_the_line_ends_in_a_semicolon() {
	let source = r#"<script>
	import Card from './Card.svelte'
	import { onMount } from 'svelte';
</script>
"#;

	let tree = parse_tree(source);
	let script = first_of_kind(&tree, "script_element");
	let (imports, exports) = Svelte {}.extract_imports_exports(script, source);

	// A trailing `;` is stripped before the quote check, so a semicolon-
	// terminated import is no longer invisible.
	assert_eq!(as_strs(&imports), ["./Card.svelte", "svelte"]);
	assert!(exports.is_empty());
}

#[test]
fn script_exports_cover_declarations_functions_and_classes() {
	let source = r#"<script>
	export let title = 'hi'
	export default thing
	export function helper() {}
	export class Widget {}
	export { title }
</script>
"#;

	let tree = parse_tree(source);
	let script = first_of_kind(&tree, "script_element");
	let (imports, exports) = Svelte {}.extract_imports_exports(script, source);

	assert!(imports.is_empty());
	// `export { title }` is deliberately skipped — the brace form falls into
	// the catch-all branch, which refuses names starting with '{'.
	assert_eq!(as_strs(&exports), ["title", "default", "helper", "Widget"]);
}

#[test]
fn script_tag_attributes_containing_a_gt_do_not_truncate_the_body() {
	let source = r#"<script data-cond="a>b">
	import Card from './Card.svelte'
</script>
"#;

	let tree = parse_tree(source);
	let script = first_of_kind(&tree, "script_element");
	let (imports, _) = Svelte {}.extract_imports_exports(script, source);

	assert_eq!(as_strs(&imports), ["./Card.svelte"]);
}

#[test]
fn imports_and_exports_are_only_read_from_script_elements() {
	let source = r#"<style>
	.a { color: red; }
</style>
<div on:click={go}>x</div>
"#;

	let tree = parse_tree(source);
	let svelte = Svelte {};

	let (style_imports, style_exports) =
		svelte.extract_imports_exports(first_of_kind(&tree, "style_element"), source);
	assert!(style_imports.is_empty() && style_exports.is_empty());

	let (el_imports, el_exports) =
		svelte.extract_imports_exports(first_of_kind(&tree, "element"), source);
	assert!(el_imports.is_empty() && el_exports.is_empty());
}

#[test]
fn embedded_sources_report_the_script_language_and_raw_text_offset() {
	let source = r#"<script>
	const a = 1;
</script>

<script context="module" lang="ts">
	const b = 2;
</script>
"#;

	let tree = parse_tree(source);
	let sources = Svelte {}.extract_embedded_sources(tree.root_node(), source);

	assert_eq!(sources.len(), 2);
	assert_eq!(sources[0].language, "javascript");
	assert_eq!(sources[0].start_line, 0);
	assert!(sources[0].contents.contains("const a = 1;"));

	assert_eq!(sources[1].language, "typescript");
	assert_eq!(sources[1].start_line, 4);
	assert!(sources[1].contents.contains("const b = 2;"));
}

#[test]
fn typescript_is_detected_from_every_accepted_lang_attribute_spelling() {
	for source in [
		"<script lang=\"ts\">\n\tconst a = 1;\n</script>\n",
		"<script lang='ts'>\n\tconst a = 1;\n</script>\n",
		"<script lang=\"typescript\">\n\tconst a = 1;\n</script>\n",
	] {
		let tree = parse_tree(source);
		let sources = Svelte {}.extract_embedded_sources(tree.root_node(), source);
		assert_eq!(sources.len(), 1);
		assert_eq!(sources[0].language, "typescript", "source: {source}");
	}
}

#[test]
fn empty_script_blocks_expand_to_no_regions() {
	let svelte = Svelte {};

	let source = "<script></script>\n";
	let tree = parse_tree(source);
	let script = first_of_kind(&tree, "script_element");
	assert_eq!(
		svelte
			.expand_meaningful_node(script, source)
			.map(|r| r.len()),
		Some(0)
	);
	assert!(parse_regions(source).is_empty());

	// An empty script still reports an (empty) embedded source, because
	// `extract_embedded_sources` does not check for blank contents.
	let sources = svelte.extract_embedded_sources(tree.root_node(), source);
	assert_eq!(sources.len(), 1);
	assert!(sources[0].contents.is_empty());
}

#[test]
fn only_script_and_style_elements_are_expanded() {
	let source = "<div on:click={go}>x</div>\n";
	let tree = parse_tree(source);
	let element = first_of_kind(&tree, "element");

	assert!(Svelte {}.expand_meaningful_node(element, source).is_none());
}

#[test]
fn typescript_script_blocks_are_parsed_with_the_typescript_grammar() {
	let source = r#"
<script lang="ts">
	interface Shape {
		kind: string;
		size: number;
	}
</script>
"#;

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"expected one embedded region, got {:?}",
		regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
	);
	// `interface_declaration` only exists in the TypeScript grammar, so this
	// also proves `lang="ts"` selected it over JavaScript.
	assert_eq!(regions[0].node_kind, "interface_declaration");
	assert!(regions[0].content.contains("interface Shape"));
	// Sub-region rows are shifted by the raw_text offset so they address the
	// .svelte file, not the extracted script.
	assert_eq!(regions[0].start_line, 2);
	assert_eq!(regions[0].end_line, 5);
}

#[test]
fn sibling_components_merge_into_one_component_declarations_block() {
	let source = r#"<div>
	<Alpha on:click={a}>x</Alpha>
	<Beta on:click={b}>y</Beta>
</div>
"#;

	let regions = parse_regions(source);

	assert_eq!(
		regions.len(),
		1,
		"expected the two single-line elements to merge, got {:?}",
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert_eq!(regions[0].node_kind, "element");
	// The merge header uses `get_node_type_description` for the node kind.
	assert!(
		regions[0]
			.content
			.starts_with("// Merged component declarations (2 declarations)"),
		"unexpected merged content: {}",
		regions[0].content
	);
	assert_eq!(as_strs(&regions[0].symbols), ["Alpha", "Beta", "on:click"]);
}

#[test]
fn svelte_reports_no_calls_or_type_relations_of_its_own() {
	let source = r#"<script>
	class Widget extends Base {}
	run();
</script>
<div on:click={go}>x</div>
"#;

	let tree = parse_tree(source);
	let svelte = Svelte {};

	// Svelte's container nodes hold script text verbatim; callables and type
	// heritage reach the graph through `extract_embedded_sources` instead.
	for kind in ["script_element", "element", "raw_text"] {
		let node = first_of_kind(&tree, kind);
		assert!(svelte.extract_function_calls(node, source).is_empty());
		assert!(svelte.extract_type_relations(node, source).is_empty());
	}
}

fn svelte_registry() -> FileRegistry {
	let files = vec![
		"src/App.svelte".to_string(),
		"src/lib/Card.svelte".to_string(),
		"src/lib/helpers.js".to_string(),
		"src/lib/utils/index.js".to_string(),
	];
	FileRegistry::new(&files)
}

#[test]
fn relative_imports_resolve_against_the_importing_file() {
	let registry = svelte_registry();

	assert_eq!(
		Svelte {}
			.resolve_import("./lib/Card.svelte", "src/App.svelte", &registry)
			.as_deref(),
		Some("src/lib/Card.svelte")
	);
}

#[test]
fn extension_less_imports_resolve_by_trying_javascript_extensions() {
	let registry = svelte_registry();

	assert_eq!(
		Svelte {}
			.resolve_import("./lib/helpers", "src/App.svelte", &registry)
			.as_deref(),
		Some("src/lib/helpers.js")
	);
}

#[test]
fn barrel_imports_resolve_to_the_index_file() {
	let registry = svelte_registry();

	assert_eq!(
		Svelte {}
			.resolve_import("./lib/utils", "src/App.svelte", &registry)
			.as_deref(),
		Some("src/lib/utils/index.js")
	);
}

#[test]
fn third_party_imports_that_match_no_indexed_file_stay_unresolved() {
	let registry = svelte_registry();

	assert_eq!(
		Svelte {}.resolve_import("svelte", "src/App.svelte", &registry),
		None
	);
}
