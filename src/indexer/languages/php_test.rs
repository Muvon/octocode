use crate::indexer::code_region_extractor::extract_meaningful_regions;
use crate::indexer::languages::php::Php;
use crate::indexer::languages::Language;
use tree_sitter::Parser;

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
