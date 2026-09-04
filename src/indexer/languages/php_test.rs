use crate::indexer::code_region_extractor::{extract_meaningful_regions, CodeRegion};
use crate::indexer::languages::php::Php;
use crate::indexer::languages::resolution_utils::FileRegistry;
use crate::indexer::languages::{CallTarget, Language, TypeRelationKind};
use tree_sitter::{Node, Parser, Tree};

#[test]
fn test_php_method_chunking() {
	let php_code = r#"<?php

namespace Test\Example;

use Some\Other\Class;

/**
 * Test class for PHP method chunking
 */
class BasePayload
{
    private $request;
    private $settings;

    /**
     * Create payload from request
     */
    public function fromRequest($request)
    {
        $this->request = $request;
        return $this;
    }

    /**
     * Get handler class name
     */
    public function getHandlerClassName(): string
    {
        return static::class . 'Handler';
    }

    /**
     * Get processor information
     */
    public function getInfo(): array
    {
        return [
            'name' => 'BasePayload',
            'version' => '1.0.0'
        ];
    }

    /**
     * Get available processors
     */
    public function getProcessors(): array
    {
        return [
            'default' => DefaultProcessor::class,
            'advanced' => AdvancedProcessor::class
        ];
    }

    /**
     * Check if request matches
     */
    public function hasMatch(): bool
    {
        return !empty($this->request);
    }

    /**
     * Set settings
     */
    public function setSettings($settings): void
    {
        $this->settings = $settings;
    }
}

/**
 * Standalone function outside class
 */
function standalone_function($param)
{
    return $param * 2;
}
"#;

	let php_lang = Php {};
	let mut parser = Parser::new();
	parser.set_language(&php_lang.get_ts_language()).unwrap();

	let tree = parser.parse(php_code, None).unwrap();
	let mut regions = Vec::new();

	extract_meaningful_regions(tree.root_node(), php_code, &php_lang, &mut regions);

	// Print regions for debugging
	println!("Found {} regions:", regions.len());
	for (i, region) in regions.iter().enumerate() {
		println!(
			"Region {}: {} (lines {}-{})",
			i + 1,
			region.node_kind,
			region.start_line + 1,
			region.end_line + 1
		);
		println!("  Symbols: {:?}", region.symbols);
		println!(
			"  Content preview: {}",
			region
				.content
				.lines()
				.take(3)
				.collect::<Vec<_>>()
				.join("\\n")
		);
		println!();
	}

	// Verify we have individual methods, not entire class
	let method_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "method_declaration")
		.collect();

	let function_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "function_definition")
		.collect();

	let class_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "class_declaration")
		.collect();

	// Assertions
	assert_eq!(
		class_regions.len(),
		0,
		"Should not have any class_declaration regions"
	);
	assert_eq!(
		method_regions.len(),
		6,
		"Should have 6 individual method regions"
	);
	assert_eq!(
		function_regions.len(),
		1,
		"Should have 1 standalone function region"
	);

	// Verify method names are extracted correctly
	let method_names: Vec<String> = method_regions
		.iter()
		.flat_map(|r| &r.symbols)
		.cloned()
		.collect();

	let expected_methods = vec![
		"fromRequest",
		"getHandlerClassName",
		"getInfo",
		"getProcessors",
		"hasMatch",
		"setSettings",
	];

	for expected_method in expected_methods {
		assert!(
			method_names.contains(&expected_method.to_string()),
			"Should contain method: {}",
			expected_method
		);
	}

	// Verify standalone function
	let function_names: Vec<String> = function_regions
		.iter()
		.flat_map(|r| &r.symbols)
		.cloned()
		.collect();

	assert!(
		function_names.contains(&"standalone_function".to_string()),
		"Should contain standalone function"
	);

	// Verify no region is excessively large (no more than ~15 lines per method)
	for region in &regions {
		let line_count = region.end_line - region.start_line + 1;
		assert!(
			line_count <= 20,
			"Region {} should not exceed 20 lines, got {}",
			region.node_kind,
			line_count
		);
	}

	println!("✅ All PHP method chunking tests passed!");
}

#[test]
fn test_php_no_massive_class_chunks() {
	// Test case based on the user's BasePayload.php issue
	let php_code = r#"<?php

/*
 Copyright (c) 2024-present, Manticore Software LTD (https://manticoresearch.com)
*/

use Manticoresearch\Buddy\Core\ManticoreSearch\Settings;
use Manticoresearch\Buddy\Core\Network\Request;
use Manticoresearch\Buddy\Core\Process\BaseProcessor;
use Manticoresearch\Buddy\Core\Tool\SqlQueryParser;

/**
 * @phpstan-template T of array
 */
class BasePayload
{
	protected Request $request;
	protected Settings $manticoreSettings;
	protected ?SqlQueryParser $sqlQueryParser = null;

	public static function fromRequest(Request $request): static
	{
		$self = new static();
		$self->request = $request;
		return $self;
	}

	public function getHandlerClassName(): string
	{
		$ns = substr(static::class, 0, strrpos(static::class, '\\'));
		return $ns . '\\Handler';
	}

	public function getInfo(): array
	{
		return [
			'name' => 'BasePayload',
			'version' => '1.0.0'
		];
	}

	public function getProcessors(): array
	{
		return [
			BaseProcessor::class,
		];
	}

	public function hasMatch(): bool
	{
		return true;
	}

	public function getRequiredVersion(): string
	{
		return '1.0.0';
	}

	public function setSettings(Settings $settings): static
	{
		$this->manticoreSettings = $settings;
		return $this;
	}

	public function getSettings(): Settings
	{
		return $this->manticoreSettings;
	}

	public function setParser(SqlQueryParser $sqlQueryParser): static
	{
		$this->sqlQueryParser = $sqlQueryParser;
		return $this;
	}
}
"#;

	let php_lang = Php {};
	let mut parser = Parser::new();
	parser.set_language(&php_lang.get_ts_language()).unwrap();

	let tree = parser.parse(php_code, None).unwrap();
	let mut regions = Vec::new();

	extract_meaningful_regions(tree.root_node(), php_code, &php_lang, &mut regions);

	// Print regions for debugging
	println!(
		"Found {} regions for BasePayload-like class:",
		regions.len()
	);
	for (i, region) in regions.iter().enumerate() {
		let line_count = region.end_line - region.start_line + 1;
		println!(
			"Region {}: {} (lines {}-{}, {} lines total)",
			i + 1,
			region.node_kind,
			region.start_line + 1,
			region.end_line + 1,
			line_count
		);
		println!("  Symbols: {:?}", region.symbols);
		println!();
	}

	// Critical assertions to prevent regression
	let class_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "class_declaration")
		.collect();

	let method_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "method_declaration")
		.collect();

	// MAIN ASSERTION: No massive class chunks
	assert_eq!(class_regions.len(), 0,
		"❌ REGRESSION: Found {} class_declaration regions! This means entire classes are being chunked again.",
		class_regions.len());

	// Should have individual methods instead
	assert!(
		method_regions.len() >= 7,
		"Should have at least 7 individual method regions, got {}",
		method_regions.len()
	);

	// Verify no region is excessively large (the original issue was 84 lines)
	for region in &regions {
		let line_count = region.end_line - region.start_line + 1;
		assert!(line_count <= 25,
			"❌ REGRESSION: Region '{}' has {} lines (too large)! Original issue was 84-line class chunks.",
			region.node_kind, line_count);
	}

	// Verify we have the expected method names
	let method_names: Vec<String> = method_regions
		.iter()
		.flat_map(|r| &r.symbols)
		.cloned()
		.collect();

	let expected_methods = vec![
		"fromRequest",
		"getHandlerClassName",
		"getInfo",
		"getProcessors",
		"hasMatch",
		"getRequiredVersion",
		"setSettings",
		"getSettings",
		"setParser",
	];

	for expected_method in expected_methods {
		assert!(
			method_names.contains(&expected_method.to_string()),
			"Should contain method: {}",
			expected_method
		);
	}

	println!("✅ PHP class chunking fix verified - no more massive class chunks!");
}

#[test]
fn test_php_meaningful_kinds_excludes_class() {
	let php_lang = Php {};
	let meaningful_kinds = php_lang.get_meaningful_kinds();

	// Critical assertion: class_declaration should NOT be in meaningful kinds
	assert!(!meaningful_kinds.contains(&"class_declaration"),
		"❌ REGRESSION: class_declaration found in meaningful_kinds! This will cause massive class chunks again.");

	// Should still have method and function declarations
	assert!(
		meaningful_kinds.contains(&"method_declaration"),
		"method_declaration should be in meaningful_kinds"
	);
	assert!(
		meaningful_kinds.contains(&"function_definition"),
		"function_definition should be in meaningful_kinds"
	);

	println!(
		"✅ PHP meaningful_kinds configuration verified - class_declaration properly excluded"
	);
}

#[test]
fn test_braced_namespace_splits_into_individual_functions() {
	// Non-trivial content so the smart single-line merge pass doesn't recombine them.
	let php_code = r#"<?php

namespace Foo {
    function f() {
        $x = 1;
        $y = 2;
        $z = $x + $y;
        return $z;
    }
    function g() {
        $a = 10;
        $b = 20;
        $c = $a * $b;
        return $c;
    }
}
"#;

	let php_lang = Php {};
	let mut parser = Parser::new();
	parser.set_language(&php_lang.get_ts_language()).unwrap();

	let tree = parser.parse(php_code, None).unwrap();
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), php_code, &php_lang, &mut regions);

	let namespace_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "namespace_definition")
		.collect();
	assert_eq!(
		namespace_regions.len(),
		0,
		"braced namespace with functions inside should not collapse into one region, got {:?}",
		regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
	);

	let function_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "function_definition")
		.collect();
	assert_eq!(
		function_regions.len(),
		2,
		"expected a region per function inside braced namespace"
	);
}

#[test]
fn test_unbraced_namespace_stays_single_region() {
	let php_code = r#"<?php

namespace Foo\Bar;

function standalone() {
    return 1;
}
"#;

	let php_lang = Php {};
	let mut parser = Parser::new();
	parser.set_language(&php_lang.get_ts_language()).unwrap();

	let tree = parser.parse(php_code, None).unwrap();
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), php_code, &php_lang, &mut regions);

	let namespace_regions: Vec<_> = regions
		.iter()
		.filter(|r| r.node_kind == "namespace_definition")
		.collect();
	assert_eq!(
		namespace_regions.len(),
		1,
		"unbraced namespace declaration should remain its own single region unchanged, got {:?}",
		regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
	);
}

fn parse_tree(source: &str) -> Tree {
	let mut parser = Parser::new();
	parser.set_language(&Php {}.get_ts_language()).unwrap();
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

/// The `index`-th node of `kind` in document order.
fn nth_node<'a>(tree: &'a Tree, kind: &str, index: usize) -> Node<'a> {
	let mut found = Vec::new();
	nodes_of_kind(tree.root_node(), kind, &mut found);
	*found
		.get(index)
		.unwrap_or_else(|| panic!("no {kind} node at index {index}"))
}

fn first_node<'a>(tree: &'a Tree, kind: &str) -> Node<'a> {
	nth_node(tree, kind, 0)
}

fn parse_regions(source: &str) -> Vec<CodeRegion> {
	let php = Php {};
	let tree = parse_tree(source);
	let mut regions = Vec::new();
	extract_meaningful_regions(tree.root_node(), source, &php, &mut regions);
	regions
}

fn registry(files: &[&str]) -> FileRegistry {
	let owned: Vec<String> = files.iter().map(|f| f.to_string()).collect();
	FileRegistry::new(&owned)
}

#[test]
fn the_language_is_named_php_and_owns_the_php_extension() {
	let php = Php {};
	assert_eq!(php.name(), "php");
	assert_eq!(php.get_file_extensions(), vec!["php"]);
}

#[test]
fn the_symbol_tier_restores_the_containers_chunking_drops() {
	let php = Php {};
	assert_eq!(
		php.get_meaningful_kinds(),
		vec![
			"function_definition",
			"method_declaration",
			"namespace_definition",
			"namespace_use_declaration",
		]
	);
	assert_eq!(
		php.get_symbol_kinds(),
		vec![
			"function_definition",
			"method_declaration",
			"class_declaration",
			"interface_declaration",
			"trait_declaration",
			"enum_declaration",
		]
	);
	assert_eq!(php.descend_first_kinds(), vec!["namespace_definition"]);
}

#[test]
fn every_node_type_description_arm_is_reachable() {
	let php = Php {};
	assert_eq!(
		php.get_node_type_description("function_definition"),
		"function declarations"
	);
	assert_eq!(
		php.get_node_type_description("method_declaration"),
		"function declarations"
	);
	assert_eq!(
		php.get_node_type_description("class_declaration"),
		"class declarations"
	);
	assert_eq!(
		php.get_node_type_description("trait_declaration"),
		"trait declarations"
	);
	assert_eq!(
		php.get_node_type_description("interface_declaration"),
		"interface declarations"
	);
	assert_eq!(
		php.get_node_type_description("property_declaration"),
		"property declarations"
	);
	assert_eq!(
		php.get_node_type_description("const_declaration"),
		"constant declarations"
	);
	assert_eq!(
		php.get_node_type_description("namespace_definition"),
		"namespace declarations"
	);
	assert_eq!(
		php.get_node_type_description("use_declaration"),
		"use statements"
	);
	assert_eq!(
		php.get_node_type_description("namespace_use_declaration"),
		"declarations"
	);
}

#[test]
fn semantic_groups_join_functions_with_methods_but_not_with_classes() {
	let php = Php {};
	assert!(php.are_node_types_equivalent("function_definition", "method_declaration"));
	assert!(php.are_node_types_equivalent("class_declaration", "trait_declaration"));
	assert!(php.are_node_types_equivalent("interface_declaration", "class_declaration"));
	assert!(php.are_node_types_equivalent("property_declaration", "const_declaration"));
	assert!(php.are_node_types_equivalent("namespace_definition", "use_declaration"));
	assert!(php.are_node_types_equivalent("enum_declaration", "enum_declaration"));

	assert!(!php.are_node_types_equivalent("function_definition", "class_declaration"));
	assert!(!php.are_node_types_equivalent("enum_declaration", "class_declaration"));
	assert!(!php.are_node_types_equivalent("comment", "namespace_definition"));
}

#[test]
fn a_method_symbol_list_carries_the_name_of_its_container() {
	let source = r#"<?php
trait Loggable {
    public function log($m) { return $m; }
}
enum Suit {
    public function color(): string { return 'red'; }
}
class Plain {
    public function bare() { return 1; }
}
"#;
	let tree = parse_tree(source);
	let php = Php {};
	for (index, expected) in [
		vec!["Loggable".to_string(), "log".to_string()],
		vec!["Suit".to_string(), "color".to_string()],
		vec!["Plain".to_string(), "bare".to_string()],
	]
	.into_iter()
	.enumerate()
	{
		assert_eq!(
			php.extract_symbols(nth_node(&tree, "method_declaration", index), source),
			expected
		);
	}
}

#[test]
fn a_standalone_function_contributes_only_its_own_name() {
	let source = "<?php\nfunction helper($x) {\n    return $x + 1;\n}\n";
	let tree = parse_tree(source);
	let node = first_node(&tree, "function_definition");
	assert_eq!(
		Php {}.extract_symbols(node, source),
		vec!["helper".to_string()]
	);
}

#[test]
fn an_unhandled_node_kind_falls_back_to_identifier_extraction() {
	let source =
		"<?php\nfunction f() {\n  $total = 1;\n  $total = $total + 1;\n  return $total;\n}\n";
	let tree = parse_tree(source);
	let body = first_node(&tree, "compound_statement");
	// The `$` prefix is stripped and repeats are collapsed.
	assert_eq!(
		Php {}.extract_symbols(body, source),
		vec!["total".to_string()]
	);
}

#[test]
fn identifier_extraction_strips_the_dollar_prefix_and_deduplicates() {
	let source =
		"<?php\nfunction f() {\n  $total = 1;\n  $total = $total + 1;\n  return $total;\n}\n";
	let tree = parse_tree(source);
	let body = first_node(&tree, "compound_statement");
	let mut symbols = Vec::new();
	Php {}.extract_identifiers(body, source, &mut symbols);
	assert_eq!(symbols, vec!["total".to_string()]);
}

#[test]
fn every_use_statement_form_yields_slash_separated_paths() {
	let source = r#"<?php
use App\Contracts\Jsonable;
use App\Support\Str as StrHelper;
use App\Http\{Request, Response as Resp};
"#;
	let tree = parse_tree(source);
	let php = Php {};
	assert_eq!(
		php.extract_imports_exports(nth_node(&tree, "namespace_use_declaration", 0), source)
			.0,
		vec!["App/Contracts/Jsonable".to_string()]
	);
	assert_eq!(
		php.extract_imports_exports(nth_node(&tree, "namespace_use_declaration", 1), source)
			.0,
		vec!["App/Support/Str".to_string()]
	);
	assert_eq!(
		php.extract_imports_exports(nth_node(&tree, "namespace_use_declaration", 2), source)
			.0,
		vec![
			"App/Http/Request".to_string(),
			"App/Http/Response".to_string(),
		]
	);
}

#[test]
fn a_use_function_statement_drops_the_keyword_from_the_import_path() {
	// The `function`/`const` modifier must be stripped along with `use `, or the
	// resolver is handed an unresolvable path.
	let source = "<?php\nuse function App\\Helpers\\slugify;\nuse const App\\LIMIT;\n";
	let tree = parse_tree(source);
	let php = Php {};
	assert_eq!(
		php.extract_imports_exports(nth_node(&tree, "namespace_use_declaration", 0), source)
			.0,
		vec!["App/Helpers/slugify".to_string()]
	);
	assert_eq!(
		php.extract_imports_exports(nth_node(&tree, "namespace_use_declaration", 1), source)
			.0,
		vec!["App/LIMIT".to_string()]
	);
}

#[test]
fn every_named_declaration_exports_its_own_name() {
	let source = r#"<?php
namespace App\Models;

interface Jsonable {}

trait Loggable {}

enum Status {}

class User
{
    public function greet() { return 1; }
}

function helper() { return 1; }
"#;
	let tree = parse_tree(source);
	let php = Php {};
	for (kind, expected) in [
		("namespace_definition", "App\\Models"),
		("interface_declaration", "Jsonable"),
		("trait_declaration", "Loggable"),
		("enum_declaration", "Status"),
		("class_declaration", "User"),
		("method_declaration", "greet"),
		("function_definition", "helper"),
	] {
		let (imports, exports) = php.extract_imports_exports(first_node(&tree, kind), source);
		assert!(imports.is_empty(), "{kind} should not import anything");
		assert_eq!(
			exports,
			vec![expected.to_string()],
			"wrong export for {kind}"
		);
	}
}

#[test]
fn a_trait_use_inside_a_class_is_neither_an_import_nor_an_export() {
	// `use Loggable;` inside a class body is a `use_declaration`, not a
	// `namespace_use_declaration`, so it falls through the match untouched.
	let source = "<?php\nclass User\n{\n    use Loggable;\n}\n";
	let tree = parse_tree(source);
	let node = first_node(&tree, "use_declaration");
	let (imports, exports) = Php {}.extract_imports_exports(node, source);
	assert!(imports.is_empty());
	assert!(exports.is_empty());
}

#[test]
fn each_call_syntax_keeps_its_receiver_as_the_qualifier() {
	let source = r#"<?php
class User
{
    public function greet($who)
    {
        strtoupper($who);
        $this->log($who);
        StrHelper::slug($who);
    }
}
"#;
	let tree = parse_tree(source);
	let php = Php {};
	assert_eq!(
		php.extract_function_calls(first_node(&tree, "function_call_expression"), source),
		vec![CallTarget {
			name: "strtoupper".to_string(),
			qualifier: None,
		}]
	);
	assert_eq!(
		php.extract_function_calls(first_node(&tree, "member_call_expression"), source),
		vec![CallTarget {
			name: "log".to_string(),
			qualifier: Some("$this".to_string()),
		}]
	);
	assert_eq!(
		php.extract_function_calls(first_node(&tree, "scoped_call_expression"), source),
		vec![CallTarget {
			name: "slug".to_string(),
			qualifier: Some("StrHelper".to_string()),
		}]
	);
}

#[test]
fn a_node_that_is_not_a_call_yields_no_call_targets() {
	let source = "<?php\nclass User\n{\n    private $name;\n}\n";
	let tree = parse_tree(source);
	let node = first_node(&tree, "property_declaration");
	assert!(Php {}.extract_function_calls(node, source).is_empty());
}

#[test]
fn a_class_reports_both_its_base_and_every_interface_it_implements() {
	let source = "<?php\nclass User extends BaseModel implements Jsonable, Countable {}\n";
	let tree = parse_tree(source);
	let node = first_node(&tree, "class_declaration");
	assert_eq!(
		Php {}.extract_type_relations(node, source),
		vec![
			(TypeRelationKind::Extends, "BaseModel".to_string()),
			(TypeRelationKind::Implements, "Jsonable".to_string()),
			(TypeRelationKind::Implements, "Countable".to_string()),
		]
	);
}

#[test]
fn an_interface_extends_every_parent_and_an_enum_only_implements() {
	let source = r#"<?php
interface Jsonable extends Arrayable, Countable {}

enum Status implements HasLabel {}
"#;
	let tree = parse_tree(source);
	let php = Php {};
	assert_eq!(
		php.extract_type_relations(first_node(&tree, "interface_declaration"), source),
		vec![
			(TypeRelationKind::Extends, "Arrayable".to_string()),
			(TypeRelationKind::Extends, "Countable".to_string()),
		]
	);
	assert_eq!(
		php.extract_type_relations(first_node(&tree, "enum_declaration"), source),
		vec![(TypeRelationKind::Implements, "HasLabel".to_string())]
	);
}

#[test]
fn a_plain_class_and_a_trait_report_no_type_relations() {
	let source = "<?php\nclass Plain {}\n\ntrait Loggable {}\n";
	let tree = parse_tree(source);
	let php = Php {};
	assert!(php
		.extract_type_relations(first_node(&tree, "class_declaration"), source)
		.is_empty());
	assert!(php
		.extract_type_relations(first_node(&tree, "trait_declaration"), source)
		.is_empty());
}

#[test]
fn a_fully_qualified_parent_reports_only_its_terminal_name() {
	// `simple_type_name` treats `\` as a namespace separator, like `::` and `.`.
	let source = "<?php\nclass A extends \\Vendor\\Base implements \\Vendor\\I {}\n";
	let tree = parse_tree(source);
	let node = first_node(&tree, "class_declaration");
	assert_eq!(
		Php {}.extract_type_relations(node, source),
		vec![
			(TypeRelationKind::Extends, "Base".to_string()),
			(TypeRelationKind::Implements, "I".to_string()),
		]
	);
}

#[test]
fn a_relative_include_resolves_against_the_source_directory() {
	let files = registry(&[
		"src/App/Models/User.php",
		"src/App/Config.php",
		"src/App/Models/Post.php",
	]);
	assert_eq!(
		Php {}.resolve_import("../Config.php", "src/App/Models/User.php", &files),
		Some("src/App/Config.php".to_string())
	);
}

#[test]
fn a_bare_filename_resolves_next_to_the_source_file() {
	let files = registry(&["src/App/Models/User.php", "src/App/Models/Post.php"]);
	assert_eq!(
		Php {}.resolve_import("Post.php", "src/App/Models/User.php", &files),
		Some("src/App/Models/Post.php".to_string())
	);
}

#[test]
fn a_namespace_import_resolves_through_the_psr_4_candidates() {
	let files = registry(&["src/App/Models/User.php", "src/App/Config.php"]);
	assert_eq!(
		Php {}.resolve_import("App/Config", "src/App/Models/User.php", &files),
		Some("src/App/Config.php".to_string())
	);
}

#[test]
fn an_unknown_third_party_namespace_resolves_to_nothing() {
	let files = registry(&["src/App/Models/User.php", "src/App/Config.php"]);
	assert_eq!(
		Php {}.resolve_import("Guzzle/Http/Client", "src/App/Models/User.php", &files),
		None
	);
}

#[test]
fn consecutive_use_statements_merge_under_the_fallback_description() {
	let source = "<?php\n\nuse App\\A;\nuse App\\B;\n\nfunction solo() {\n    $x = 1;\n    $y = 2;\n    return $x + $y;\n}\n";
	let regions = parse_regions(source);
	assert_eq!(
		regions
			.iter()
			.map(|r| r.node_kind.as_str())
			.collect::<Vec<_>>(),
		vec!["namespace_use_declaration", "function_definition"]
	);
	assert!(
		regions[0]
			.content
			.starts_with("// Merged declarations (2 declarations)\n"),
		"namespace_use_declaration has no description arm, so the fallback applies: {:?}",
		regions[0].content
	);
	assert_eq!(
		regions[0].symbols,
		vec!["A".to_string(), "App".to_string(), "B".to_string()]
	);
	assert_eq!(regions[1].symbols, vec!["solo".to_string()]);
}
