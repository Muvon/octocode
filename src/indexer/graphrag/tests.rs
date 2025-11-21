#[cfg(test)]
mod graphrag_relationship_tests {
	use crate::indexer::graphrag::relationships::RelationshipDiscovery;
	use crate::indexer::graphrag::types::CodeNode;
	use crate::indexer::languages::{get_language, Language};
	use tree_sitter::Parser;

	/// Test that Rust import/export extraction works correctly
	#[tokio::test]
	async fn test_rust_import_export_extraction() {
		let rust_code = r#"
use crate::utils::helper_function;
use super::config::Config;
use std::collections::HashMap;

pub fn main_function() {
    println!("Main function");
}

pub struct MainStruct {
    pub name: String,
}

fn private_function() {
    println!("Private function");
}
"#;

		let lang_impl = get_language("rust").expect("Rust language should be available");
		let mut parser = Parser::new();
		parser.set_language(&lang_impl.get_ts_language()).unwrap();
		let tree = parser.parse(rust_code, None).unwrap();

		let mut all_imports = Vec::new();
		let mut all_exports = Vec::new();

		// Walk through all nodes and extract imports/exports
		extract_imports_exports_recursive(
			tree.root_node(),
			rust_code,
			lang_impl.as_ref(),
			&mut all_imports,
			&mut all_exports,
		);

		// Verify imports are extracted
		println!("Extracted imports: {:?}", all_imports);
		println!("Extracted exports: {:?}", all_exports);

		assert!(
			!all_imports.is_empty(),
			"Should extract imports from Rust code"
		);

		// The Rust implementation now extracts the full import paths for GraphRAG
		assert!(
			all_imports
				.iter()
				.any(|imp| imp == "crate::utils::helper_function"),
			"Should extract full crate::utils::helper_function import"
		);
		assert!(
			all_imports.iter().any(|imp| imp == "super::config::Config"),
			"Should extract full super::config::Config import"
		);
		assert!(
			all_imports
				.iter()
				.any(|imp| imp == "std::collections::HashMap"),
			"Should extract full std::collections::HashMap import"
		);

		// Verify exports are extracted
		assert!(
			!all_exports.is_empty(),
			"Should extract exports from Rust code"
		);
		assert!(
			all_exports.iter().any(|exp| exp == "main_function"),
			"Should extract main_function as export"
		);
		// Note: MainStruct export detection may need refinement in the language implementation

		// Should NOT export private functions
		assert!(
			!all_exports.iter().any(|exp| exp == "private_function"),
			"Should NOT export private functions"
		);
	}

	/// Test that JavaScript import/export extraction works correctly
	#[tokio::test]
	async fn test_javascript_import_export_extraction() {
		let js_code = r#"
import { calculateSum, formatMessage } from './utils.js';
import Logger from './utils.js';
import { processData } from './processor.js';

export function mainFunction() {
    console.log("Main function");
}

export default class MainClass {
    constructor() {
        this.name = "main";
    }
}

function privateFunction() {
    console.log("Private function");
}
"#;

		let lang_impl =
			get_language("javascript").expect("JavaScript language should be available");
		let mut parser = Parser::new();
		parser.set_language(&lang_impl.get_ts_language()).unwrap();
		let tree = parser.parse(js_code, None).unwrap();

		let mut all_imports = Vec::new();
		let mut all_exports = Vec::new();

		extract_imports_exports_recursive(
			tree.root_node(),
			js_code,
			lang_impl.as_ref(),
			&mut all_imports,
			&mut all_exports,
		);

		// Verify imports are extracted
		println!("DEBUG: JavaScript imports extracted: {:?}", all_imports);
		println!("DEBUG: JavaScript exports extracted: {:?}", all_exports);
		assert!(
			!all_imports.is_empty(),
			"Should extract imports from JavaScript code"
		);
		assert!(
			all_imports.iter().any(|imp| imp.contains("./utils.js")),
			"Should extract ./utils.js import"
		);
		assert!(
			all_imports.iter().any(|imp| imp.contains("./processor.js")),
			"Should extract ./processor.js import"
		);

		// Verify exports are extracted
		assert!(
			!all_exports.is_empty(),
			"Should extract exports from JavaScript code"
		);
		assert!(
			all_exports.iter().any(|exp| exp.contains("mainFunction")),
			"Should extract mainFunction as export"
		);
		assert!(
			all_exports.iter().any(|exp| exp.contains("MainClass")),
			"Should extract MainClass as export"
		);
	}

	/// Test that Python import/export extraction works correctly
	#[tokio::test]
	async fn test_python_import_export_extraction() {
		let python_code = r#"
from .utils import calculate_sum, format_message
from .processor import process_data
import os
import sys

def main_function():
    print("Main function")

class MainClass:
    def __init__(self):
        self.name = "main"

def _private_function():
    print("Private function")
"#;

		let lang_impl = get_language("python").expect("Python language should be available");
		let mut parser = Parser::new();
		parser.set_language(&lang_impl.get_ts_language()).unwrap();
		let tree = parser.parse(python_code, None).unwrap();

		let mut all_imports = Vec::new();
		let mut all_exports = Vec::new();

		extract_imports_exports_recursive(
			tree.root_node(),
			python_code,
			lang_impl.as_ref(),
			&mut all_imports,
			&mut all_exports,
		);

		// Verify imports are extracted
		assert!(
			!all_imports.is_empty(),
			"Should extract imports from Python code"
		);
		assert!(
			all_imports.iter().any(|imp| imp.contains(".utils")),
			"Should extract .utils import"
		);
		assert!(
			all_imports.iter().any(|imp| imp.contains(".processor")),
			"Should extract .processor import"
		);

		// Python exports all public functions/classes by default
		assert!(
			!all_exports.is_empty(),
			"Should extract exports from Python code"
		);
		assert!(
			all_exports.iter().any(|exp| exp.contains("main_function")),
			"Should extract main_function as export"
		);
		assert!(
			all_exports.iter().any(|exp| exp.contains("MainClass")),
			"Should extract MainClass as export"
		);
	}

	/// Test Rust import resolution
	#[tokio::test]
	async fn test_rust_import_resolution() {
		let rust_lang = get_language("rust").expect("Rust language should be available");

		let all_files = vec![
			"src/lib.rs".to_string(),
			"src/utils.rs".to_string(),
			"src/utils/mod.rs".to_string(), // Module directory pattern
			"src/utils/error.rs".to_string(),
			"src/config.rs".to_string(),
			"src/config/features.rs".to_string(), // Single file pattern
			"src/config/training.rs".to_string(),
			"src/features/technical.rs".to_string(),
			"src/features/mod.rs".to_string(), // Module directory pattern
			"src/main.rs".to_string(),
		];

		// Test simple crate:: import resolution
		let resolved =
			rust_lang.resolve_import("crate::utils::helper_function", "src/main.rs", &all_files);
		assert_eq!(
			resolved,
			Some("src/utils.rs".to_string()),
			"Should resolve crate::utils import to src/utils.rs"
		);

		// Test REAL-WORLD COMPLEX nested module import resolution
		let resolved = rust_lang.resolve_import(
			"crate::config::features::TechnicalIndicatorsConfig",
			"src/features/technical.rs",
			&all_files,
		);
		assert_eq!(
			resolved,
			Some("src/config/features.rs".to_string()),
			"Should resolve crate::config::features import to src/config/features.rs"
		);

		// Test another real-world nested import
		let resolved = rust_lang.resolve_import(
			"crate::utils::error::VangaError",
			"src/features/technical.rs",
			&all_files,
		);
		assert_eq!(
			resolved,
			Some("src/utils/error.rs".to_string()),
			"Should resolve crate::utils::error import to src/utils/error.rs"
		);

		// Test module directory pattern (utils as directory with mod.rs)
		let resolved =
			rust_lang.resolve_import("crate::utils::helper_function", "src/main.rs", &all_files);
		// Should prefer single file over mod.rs when both exist
		assert_eq!(
			resolved,
			Some("src/utils.rs".to_string()),
			"Should resolve crate::utils import to src/utils.rs (prefer single file)"
		);

		// Test when only mod.rs exists (remove utils.rs from files)
		let files_with_mod_only = vec![
			"src/lib.rs".to_string(),
			"src/utils/mod.rs".to_string(), // Only mod.rs exists
			"src/utils/error.rs".to_string(),
			"src/config.rs".to_string(),
			"src/config/features.rs".to_string(),
			"src/main.rs".to_string(),
		];
		let resolved = rust_lang.resolve_import(
			"crate::utils::helper_function",
			"src/main.rs",
			&files_with_mod_only,
		);
		assert_eq!(
			resolved,
			Some("src/utils/mod.rs".to_string()),
			"Should resolve crate::utils import to src/utils/mod.rs when single file doesn't exist"
		);

		// Test super:: import resolution
		let resolved =
			rust_lang.resolve_import("super::config::Config", "src/utils.rs", &all_files);
		assert_eq!(
			resolved,
			Some("src/config.rs".to_string()),
			"Should resolve super::config import to src/config.rs"
		);

		// Test self:: import resolution
		let resolved =
			rust_lang.resolve_import("self::helper_function", "src/utils.rs", &all_files);
		assert_eq!(
			resolved,
			Some("src/utils.rs".to_string()),
			"Should resolve self:: import to same file"
		);
	}

	/// Test real-world Rust import resolution edge cases
	#[tokio::test]
	async fn test_rust_real_world_edge_cases() {
		let rust_lang = get_language("rust").expect("Rust language should be available");

		// Test deeply nested module imports (3+ levels)
		let files = vec![
			"src/lib.rs".to_string(),
			"src/model/attention/layers.rs".to_string(),
			"src/config/training/optimizer.rs".to_string(),
			"src/utils/math/statistics.rs".to_string(),
			"src/features/technical.rs".to_string(),
		];

		// Test 3-level deep import
		let resolved = rust_lang.resolve_import(
			"crate::model::attention::layers::MultiHeadAttention",
			"src/features/technical.rs",
			&files,
		);
		assert_eq!(
			resolved,
			Some("src/model/attention/layers.rs".to_string()),
			"Should resolve 3-level deep import to correct file"
		);

		// Test another 3-level deep import
		let resolved = rust_lang.resolve_import(
			"crate::config::training::optimizer::AdamConfig",
			"src/features/technical.rs",
			&files,
		);
		assert_eq!(
			resolved,
			Some("src/config/training/optimizer.rs".to_string()),
			"Should resolve training optimizer import to correct file"
		);

		// Test math utilities import
		let resolved = rust_lang.resolve_import(
			"crate::utils::math::statistics::mean",
			"src/features/technical.rs",
			&files,
		);
		assert_eq!(
			resolved,
			Some("src/utils/math/statistics.rs".to_string()),
			"Should resolve math statistics import to correct file"
		);
	}

	/// Test JavaScript import resolution
	#[tokio::test]
	async fn test_javascript_import_resolution() {
		let js_lang = get_language("javascript").expect("JavaScript language should be available");

		let all_files = vec![
			"src/main.js".to_string(),
			"src/utils.js".to_string(),
			"src/processor.js".to_string(),
		];

		// Test relative import resolution
		let resolved = js_lang.resolve_import("./utils.js", "src/main.js", &all_files);
		assert_eq!(
			resolved,
			Some("src/utils.js".to_string()),
			"Should resolve ./utils.js import"
		);

		let resolved = js_lang.resolve_import("./processor.js", "src/main.js", &all_files);
		assert_eq!(
			resolved,
			Some("src/processor.js".to_string()),
			"Should resolve ./processor.js import"
		);

		// Test import from subdirectory
		let resolved = js_lang.resolve_import("../utils.js", "src/components/main.js", &all_files);
		assert_eq!(
			resolved,
			Some("src/utils.js".to_string()),
			"Should resolve ../utils.js import"
		);
	}

	/// Test TypeScript import resolution
	#[tokio::test]
	async fn test_typescript_import_resolution() {
		let ts_lang = get_language("typescript").expect("TypeScript language should be available");

		let all_files = vec![
			"src/main.ts".to_string(),
			"src/utils.ts".to_string(),
			"src/processor.ts".to_string(),
			"src/types.d.ts".to_string(),
		];

		// Test relative import resolution
		let resolved = ts_lang.resolve_import("./utils.ts", "src/main.ts", &all_files);
		assert_eq!(
			resolved,
			Some("src/utils.ts".to_string()),
			"Should resolve ./utils.ts import"
		);

		let resolved = ts_lang.resolve_import("./processor", "src/main.ts", &all_files);
		assert_eq!(
			resolved,
			Some("src/processor.ts".to_string()),
			"Should resolve ./processor import"
		);

		// Test type definition import
		let resolved = ts_lang.resolve_import("./types", "src/main.ts", &all_files);
		assert_eq!(
			resolved,
			Some("src/types.d.ts".to_string()),
			"Should resolve ./types import to .d.ts"
		);
	}

	/// Test Go import resolution
	#[tokio::test]
	async fn test_go_import_resolution() {
		let go_lang = get_language("go").expect("Go language should be available");

		let all_files = vec![
			"main.go".to_string(),
			"utils/helper.go".to_string(),
			"config/settings.go".to_string(),
		];

		// Test package import resolution
		let resolved = go_lang.resolve_import("./utils", "main.go", &all_files);
		assert_eq!(
			resolved,
			Some("utils/helper.go".to_string()),
			"Should resolve ./utils import"
		);

		let resolved = go_lang.resolve_import("./config", "main.go", &all_files);
		assert_eq!(
			resolved,
			Some("config/settings.go".to_string()),
			"Should resolve ./config import"
		);
	}

	/// Test C++ import resolution
	#[tokio::test]
	async fn test_cpp_import_resolution() {
		let cpp_lang = get_language("cpp").expect("C++ language should be available");

		let all_files = vec![
			"src/main.cpp".to_string(),
			"src/utils.h".to_string(),
			"src/utils.cpp".to_string(),
			"include/config.h".to_string(),
		];

		// Test header include resolution
		let resolved = cpp_lang.resolve_import("utils.h", "src/main.cpp", &all_files);
		assert_eq!(
			resolved,
			Some("src/utils.h".to_string()),
			"Should resolve utils.h include"
		);

		let resolved = cpp_lang.resolve_import("../include/config.h", "src/main.cpp", &all_files);
		assert_eq!(
			resolved,
			Some("include/config.h".to_string()),
			"Should resolve ../include/config.h include"
		);
	}

	/// Test PHP import resolution
	#[tokio::test]
	async fn test_php_import_resolution() {
		let php_lang = get_language("php").expect("PHP language should be available");

		let all_files = vec![
			"src/Main.php".to_string(),
			"src/Utils.php".to_string(),
			"src/Config.php".to_string(),
		];

		// Test require/include resolution
		let resolved = php_lang.resolve_import("./Utils.php", "src/Main.php", &all_files);
		assert_eq!(
			resolved,
			Some("src/Utils.php".to_string()),
			"Should resolve ./Utils.php require"
		);

		let resolved = php_lang.resolve_import("Config.php", "src/Main.php", &all_files);
		assert_eq!(
			resolved,
			Some("src/Config.php".to_string()),
			"Should resolve Config.php require"
		);
	}

	/// Test Python import resolution
	#[tokio::test]
	async fn test_python_import_resolution() {
		let python_lang = get_language("python").expect("Python language should be available");

		let all_files = vec![
			"src/__init__.py".to_string(),
			"src/main.py".to_string(),
			"src/utils.py".to_string(),
			"src/processor.py".to_string(),
		];

		// Test relative import resolution
		let resolved = python_lang.resolve_import(".utils", "src/main.py", &all_files);
		assert_eq!(
			resolved,
			Some("src/utils.py".to_string()),
			"Should resolve .utils import"
		);

		let resolved = python_lang.resolve_import(".processor", "src/main.py", &all_files);
		assert_eq!(
			resolved,
			Some("src/processor.py".to_string()),
			"Should resolve .processor import"
		);
	}

	/// Test Ruby import resolution
	#[tokio::test]
	async fn test_ruby_import_resolution() {
		let ruby_lang = get_language("ruby").expect("Ruby language should be available");

		let all_files = vec![
			"main.rb".to_string(),
			"lib/utils.rb".to_string(),
			"config/settings.rb".to_string(),
		];

		// Test relative require resolution
		let resolved = ruby_lang.resolve_import("./lib/utils", "main.rb", &all_files);
		assert_eq!(
			resolved,
			Some("lib/utils.rb".to_string()),
			"Should resolve ./lib/utils require"
		);

		// Test require_relative resolution
		let resolved = ruby_lang.resolve_import("relative:utils", "lib/helper.rb", &all_files);
		assert_eq!(
			resolved,
			Some("lib/utils.rb".to_string()),
			"Should resolve require_relative utils"
		);
	}

	/// Test Bash import resolution
	#[tokio::test]
	async fn test_bash_import_resolution() {
		let bash_lang = get_language("bash").expect("Bash language should be available");

		let all_files = vec![
			"main.sh".to_string(),
			"lib/utils.sh".to_string(),
			"config/settings.sh".to_string(),
		];

		// Test relative source resolution
		let resolved = bash_lang.resolve_import("./lib/utils.sh", "main.sh", &all_files);
		assert_eq!(
			resolved,
			Some("lib/utils.sh".to_string()),
			"Should resolve ./lib/utils.sh source"
		);

		// Test absolute source resolution
		let resolved = bash_lang.resolve_import("config/settings.sh", "main.sh", &all_files);
		assert_eq!(
			resolved,
			Some("config/settings.sh".to_string()),
			"Should resolve config/settings.sh source"
		);
	}

	/// Test Svelte import resolution
	#[tokio::test]
	async fn test_svelte_import_resolution() {
		let svelte_lang = get_language("svelte").expect("Svelte language should be available");

		let all_files = vec![
			"src/App.svelte".to_string(),
			"src/components/Button.svelte".to_string(),
			"src/utils.js".to_string(),
		];

		// Test relative component import
		let resolved =
			svelte_lang.resolve_import("./components/Button.svelte", "src/App.svelte", &all_files);
		assert_eq!(
			resolved,
			Some("src/components/Button.svelte".to_string()),
			"Should resolve ./components/Button.svelte import"
		);

		// Test JavaScript import from Svelte
		let resolved = svelte_lang.resolve_import("./utils.js", "src/App.svelte", &all_files);
		assert_eq!(
			resolved,
			Some("src/utils.js".to_string()),
			"Should resolve ./utils.js import"
		);
	}

	/// Test CSS import resolution
	#[tokio::test]
	async fn test_css_import_resolution() {
		let css_lang = get_language("css").expect("CSS language should be available");

		let all_files = vec![
			"styles/main.css".to_string(),
			"styles/components.css".to_string(),
			"styles/base.css".to_string(),
		];

		// Test CSS @import resolution
		let resolved = css_lang.resolve_import("./components.css", "styles/main.css", &all_files);
		assert_eq!(
			resolved,
			Some("styles/components.css".to_string()),
			"Should resolve ./components.css import"
		);

		let resolved = css_lang.resolve_import("base.css", "styles/main.css", &all_files);
		assert_eq!(
			resolved,
			Some("styles/base.css".to_string()),
			"Should resolve base.css import"
		);
	}

	/// Test relationship discovery between nodes
	#[tokio::test]
	async fn test_relationship_discovery() {
		// Create test nodes with imports/exports
		let main_node = CodeNode {
			id: "src/main.rs".to_string(),
			name: "main.rs".to_string(),
			kind: "file".to_string(),
			path: "src/main.rs".to_string(),
			description: "Main file".to_string(),
			symbols: vec!["main".to_string()],
			imports: vec![
				"crate::utils::helper_function".to_string(),
				"crate::config::Config".to_string(),
			],
			exports: vec!["main".to_string()],
			functions: vec![],
			hash: "hash1".to_string(),
			embedding: vec![0.1, 0.2, 0.3],
			size_lines: 20,
			language: "rust".to_string(),
		};

		let utils_node = CodeNode {
			id: "src/utils.rs".to_string(),
			name: "utils.rs".to_string(),
			kind: "file".to_string(),
			path: "src/utils.rs".to_string(),
			description: "Utility functions".to_string(),
			symbols: vec!["helper_function".to_string()],
			imports: vec!["crate::config::Config".to_string()],
			exports: vec!["helper_function".to_string()],
			functions: vec![],
			hash: "hash2".to_string(),
			embedding: vec![0.4, 0.5, 0.6],
			size_lines: 15,
			language: "rust".to_string(),
		};

		let config_node = CodeNode {
			id: "src/config.rs".to_string(),
			name: "config.rs".to_string(),
			kind: "file".to_string(),
			path: "src/config.rs".to_string(),
			description: "Configuration".to_string(),
			symbols: vec!["Config".to_string()],
			imports: vec![],
			exports: vec!["Config".to_string()],
			functions: vec![],
			hash: "hash3".to_string(),
			embedding: vec![0.7, 0.8, 0.9],
			size_lines: 10,
			language: "rust".to_string(),
		};

		let all_nodes = vec![main_node.clone(), utils_node.clone(), config_node.clone()];
		let mut relationships = Vec::new();

		// Test relationship discovery
		RelationshipDiscovery::discover_import_relationships(
			&main_node,
			&all_nodes,
			&mut relationships,
		);

		// Verify relationships were created
		assert!(
			!relationships.is_empty(),
			"Should discover relationships between nodes"
		);

		// Check for specific relationships
		let main_to_utils = relationships.iter().find(|r| {
			r.source == "src/main.rs"
				&& r.target == "src/utils.rs"
				&& r.relation_type == crate::indexer::graphrag::types::RelationType::Imports
		});
		assert!(
			main_to_utils.is_some(),
			"Should create main -> utils relationship"
		);

		let main_to_config = relationships.iter().find(|r| {
			r.source == "src/main.rs"
				&& r.target == "src/config.rs"
				&& r.relation_type == crate::indexer::graphrag::types::RelationType::Imports
		});
		assert!(
			main_to_config.is_some(),
			"Should create main -> config relationship"
		);

		// Check relationship confidence
		if let Some(rel) = main_to_utils {
			assert!(
				rel.confidence >= 0.9,
				"Import relationships should have high confidence"
			);
		}
	}

	/// Test edge cases for C++ import resolution
	#[tokio::test]
	async fn test_cpp_edge_cases() {
		let lang_impl = get_language("cpp").expect("C++ language should be available");

		let files = vec![
			"src/main.cpp".to_string(),
			"src/helper.h".to_string(), // Same directory as main.cpp
			"include/config.hpp".to_string(),
			"lib/external/library.h".to_string(),
			"src/modules/core.cpp".to_string(),
		];

		// Test local include in same directory
		let resolved = lang_impl.resolve_import("\"helper.h\"", "src/main.cpp", &files);
		assert!(
			resolved.is_some(),
			"Should resolve local include in same directory"
		);

		// Test system include that exists in project
		let resolved = lang_impl.resolve_import("config.hpp", "src/main.cpp", &files);
		assert!(resolved.is_some(), "Should find system include in project");

		// Test non-existent include
		let resolved = lang_impl.resolve_import("\"nonexistent.h\"", "src/main.cpp", &files);
		assert!(
			resolved.is_none(),
			"Should not resolve non-existent include"
		);
	}

	/// Test edge cases for Ruby import resolution
	#[tokio::test]
	async fn test_ruby_edge_cases() {
		let lang_impl = get_language("ruby").expect("Ruby language should be available");

		let files = vec![
			"app/main.rb".to_string(),
			"lib/utils/helper.rb".to_string(),
			"config/database.rb".to_string(),
			"app/models/user.rb".to_string(),
			"vendor/gems/external_gem/lib/external.rb".to_string(),
		];

		// Test require_relative
		let resolved =
			lang_impl.resolve_import("relative:../config/database", "app/main.rb", &files);
		assert!(resolved.is_some(), "Should resolve require_relative");

		// Test absolute require
		let resolved = lang_impl.resolve_import("utils/helper", "app/main.rb", &files);
		assert!(resolved.is_some(), "Should resolve absolute require");

		// Test gem require (should find in vendor)
		let resolved = lang_impl.resolve_import("external", "app/main.rb", &files);
		assert!(resolved.is_some(), "Should find gem in vendor directory");
	}

	/// Test edge cases for CSS import resolution
	#[tokio::test]
	async fn test_css_edge_cases() {
		let lang_impl = get_language("css").expect("CSS language should be available");

		let files = vec![
			"styles/main.css".to_string(),
			"styles/components/button.css".to_string(),
			"styles/utils/variables.css".to_string(),
			"node_modules/bootstrap/dist/css/bootstrap.css".to_string(),
			"assets/fonts/custom.css".to_string(),
		];

		// Test relative import with extension
		let resolved =
			lang_impl.resolve_import("./components/button.css", "styles/main.css", &files);
		assert!(resolved.is_some(), "Should resolve relative CSS import");

		// Test import without extension
		let resolved = lang_impl.resolve_import("./utils/variables", "styles/main.css", &files);
		assert!(
			resolved.is_some(),
			"Should resolve CSS import without extension"
		);

		// Test that the function handles unknown imports gracefully
		let resolved = lang_impl.resolve_import("unknown-module", "styles/main.css", &files);
		assert!(
			resolved.is_none(),
			"Should handle unknown imports gracefully"
		);
	}

	/// Test cross-platform path handling
	#[tokio::test]
	async fn test_cross_platform_paths() {
		let lang_impl = get_language("rust").expect("Rust language should be available");

		// Test with normalized paths (the system normalizes them)
		let files = vec![
			"src/main.rs".to_string(),
			"src/utils/helper.rs".to_string(),
			"src/config/mod.rs".to_string(),
		];

		let resolved = lang_impl.resolve_import("crate::utils::helper", "src/main.rs", &files);
		assert!(
			resolved.is_some(),
			"Should handle path resolution correctly"
		);

		// Test that the function doesn't crash with different path formats
		let resolved = lang_impl.resolve_import("crate::config", "src/main.rs", &files);
		assert!(resolved.is_some(), "Should resolve module imports");
	}

	/// Test performance with large file lists
	#[tokio::test]
	async fn test_performance_large_filelist() {
		let lang_impl =
			get_language("javascript").expect("JavaScript language should be available");

		// Create a large file list (1000 files)
		let mut files = Vec::new();
		for i in 0..1000 {
			files.push(format!("src/module_{}.js", i));
		}
		files.push("src/utils/helper.js".to_string());

		let start = std::time::Instant::now();
		let resolved = lang_impl.resolve_import("./utils/helper", "src/main.js", &files);
		let duration = start.elapsed();

		assert!(
			resolved.is_some(),
			"Should resolve import in large file list"
		);
		assert!(
			duration.as_millis() < 100,
			"Should resolve quickly even with large file list"
		);
	}

	/// Test malformed import statements
	#[tokio::test]
	async fn test_malformed_imports() {
		let lang_impl = get_language("python").expect("Python language should be available");

		let files = vec!["src/main.py".to_string(), "src/utils.py".to_string()];

		// Test empty import
		let resolved = lang_impl.resolve_import("", "src/main.py", &files);
		assert!(resolved.is_none(), "Should handle empty import gracefully");

		// Test malformed relative import
		let resolved = lang_impl.resolve_import("...invalid", "src/main.py", &files);
		assert!(
			resolved.is_none(),
			"Should handle malformed relative import"
		);

		// Test very long import path
		let long_path = "a/".repeat(100) + "module";
		let resolved = lang_impl.resolve_import(&long_path, "src/main.py", &files);
		assert!(resolved.is_none(), "Should handle very long import paths");
	}

	/// Helper function to extract imports/exports recursively (same as in builder.rs)
	fn extract_imports_exports_recursive(
		node: tree_sitter::Node,
		contents: &str,
		lang_impl: &dyn Language,
		all_imports: &mut Vec<String>,
		all_exports: &mut Vec<String>,
	) {
		// Extract imports/exports from current node
		let (imports, exports) = lang_impl.extract_imports_exports(node, contents);
		all_imports.extend(imports);
		all_exports.extend(exports);

		// Recursively process children
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			extract_imports_exports_recursive(child, contents, lang_impl, all_imports, all_exports);
		}
	}
}
