// Copyright 2025 Muvon Un Limited
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! PHP language implementation for the indexer
use crate::utils::path::PathNormalizer;

use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct Php {}

impl Language for Php {
	fn name(&self) -> &'static str {
		"php"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_php::LANGUAGE_PHP.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec![
			"function_definition",
			"method_declaration",
			"class_declaration",
			"namespace_definition",
			"namespace_use_declaration",
			// Removed: "trait_declaration" - too large, not semantic
			// Removed: "interface_declaration" - too large, not semantic
		]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"function_definition" | "method_declaration" => {
				// Extract the name of the function or method
				for child in node.children(&mut node.walk()) {
					if child.kind() == "name" {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							symbols.push(name.to_string());
						}
						break;
					}
				}
			}
			_ => self.extract_identifiers(node, contents, &mut symbols),
		}

		// Deduplicate symbols before returning
		symbols.sort();
		symbols.dedup();

		symbols
	}

	fn extract_identifiers(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		let kind = node.kind();
		// Check if this is a valid identifier or name
		if kind == "name" || kind == "variable_name" {
			if let Ok(text) = node.utf8_text(contents.as_bytes()) {
				let t = text.trim();
				// For PHP variables, remove the $ prefix
				let t = if let Some(stripped) = t.strip_prefix('$') {
					stripped
				} else {
					t
				};

				if !t.is_empty() && !symbols.contains(&t.to_string()) {
					symbols.push(t.to_string());
				}
			}
		}

		// Continue with recursive traversal
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				self.extract_identifiers(cursor.node(), contents, symbols);
				if !cursor.goto_next_sibling() {
					break;
				}
			}
		}
	}

	fn are_node_types_equivalent(&self, type1: &str, type2: &str) -> bool {
		// Direct match
		if type1 == type2 {
			return true;
		}

		// PHP-specific semantic groups
		let semantic_groups = [
			// Functions and methods
			&["function_definition", "method_declaration"] as &[&str],
			// Class-related declarations
			&[
				"class_declaration",
				"trait_declaration",
				"interface_declaration",
			],
			// Properties and constants
			&["property_declaration", "const_declaration"],
			// Namespace and use statements
			&["namespace_definition", "use_declaration"],
		];

		// Check if both types belong to the same semantic group
		for group in &semantic_groups {
			let contains_type1 = group.contains(&type1);
			let contains_type2 = group.contains(&type2);

			if contains_type1 && contains_type2 {
				return true;
			}
		}

		false
	}

	fn get_node_type_description(&self, node_type: &str) -> &'static str {
		match node_type {
			"function_definition" | "method_declaration" => "function declarations",
			"class_declaration" => "class declarations",
			"trait_declaration" => "trait declarations",
			"interface_declaration" => "interface declarations",
			"property_declaration" => "property declarations",
			"const_declaration" => "constant declarations",
			"namespace_definition" => "namespace declarations",
			"use_declaration" => "use statements",
			_ => "declarations",
		}
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		let mut imports = Vec::new();
		let mut exports = Vec::new();

		match node.kind() {
			"namespace_use_declaration" => {
				// Handle: use Namespace\Class;
				// Handle: use Namespace\Class as Alias;
				if let Ok(use_text) = node.utf8_text(contents.as_bytes()) {
					if let Some(imported_items) = parse_php_use_statement(use_text) {
						imports.extend(imported_items);
					}
				}
			}
			"function_definition"
			| "method_declaration"
			| "class_declaration"
			| "namespace_definition" => {
				// In PHP, all top-level items are potentially exportable
				// Extract the name as a potential export
				for child in node.children(&mut node.walk()) {
					if child.kind() == "name" {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							exports.push(name.to_string());
							break;
						}
					}
				}
			}
			_ => {}
		}

		(imports, exports)
	}

	fn resolve_import(
		&self,
		import_path: &str,
		source_file: &str,
		all_files: &[String],
	) -> Option<String> {
		use super::resolution_utils::{resolve_relative_path, FileRegistry};

		let registry = FileRegistry::new(all_files);

		if import_path.starts_with("./") || import_path.starts_with("../") {
			// Relative path - resolve directly
			if let Some(relative_path) = resolve_relative_path(source_file, import_path) {
				return self.find_matching_php_file(&relative_path, &registry);
			}
		} else if import_path.ends_with(".php") || !import_path.contains("/") {
			// Simple filename like "Config.php" - look in same directory as source
			let source_path = std::path::Path::new(source_file);
			if let Some(source_dir) = source_path.parent() {
				let target_path = source_dir.join(import_path);
				if let Some(found) = self.find_matching_php_file(&target_path, &registry) {
					return Some(found);
				}
			}
			// Also try namespace resolution as fallback
			let file_path = PathNormalizer::normalize_separators(import_path);
			return self.resolve_namespace_import(&file_path, source_file, &registry);
		} else {
			// Convert namespace to file path and try PSR-4 patterns
			let file_path = PathNormalizer::normalize_separators(import_path);
			return self.resolve_namespace_import(&file_path, source_file, &registry);
		}

		None
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["php"]
	}
}

// Helper function for PHP use statement parsing
fn parse_php_use_statement(use_text: &str) -> Option<Vec<String>> {
	let mut imports = Vec::new();
	let cleaned = use_text.trim();

	// Handle: use Namespace\Class;
	// Handle: use Namespace\Class as Alias;
	if let Some(rest) = cleaned.strip_prefix("use ") {
		let rest = rest.trim_end_matches(';'); // Skip trailing ";"

		// Handle: use Namespace\Class as Alias;
		if let Some(as_pos) = rest.find(" as ") {
			let class_path = &rest[..as_pos];
			if let Some(class_name) = class_path.split('\\').next_back() {
				imports.push(class_name.to_string());
			}
		} else {
			// Handle: use Namespace\Class;
			if let Some(class_name) = rest.split('\\').next_back() {
				imports.push(class_name.to_string());
			}
		}
		return Some(imports);
	}

	None
}

impl Php {
	/// Find matching PHP file with robust path comparison (same pattern as Rust)
	fn find_matching_php_file(
		&self,
		target_path: &std::path::Path,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		let target_str = target_path.to_string_lossy().to_string();

		// Try exact string match first (fastest) with cross-platform normalization
		if let Some(exact_match) = crate::utils::path::PathNormalizer::find_path_in_collection(
			&target_str,
			registry.get_all_files(),
		) {
			return Some(exact_match.to_string());
		}

		// Try with .php extension if not present
		let with_php_ext = if target_str.ends_with(".php") {
			target_str.clone()
		} else {
			format!("{}.php", target_str)
		};

		if let Some(exact_match) = crate::utils::path::PathNormalizer::find_path_in_collection(
			&with_php_ext,
			registry.get_all_files(),
		) {
			return Some(exact_match.to_string());
		}

		// Try normalized path comparison for cross-platform compatibility
		if let Ok(canonical_target) = target_path.canonicalize() {
			let canonical_str = canonical_target.to_string_lossy().to_string();
			for php_file in registry.get_all_files() {
				if let Ok(canonical_php) = std::path::Path::new(php_file).canonicalize() {
					let canonical_php_str = canonical_php.to_string_lossy().to_string();
					if canonical_str == canonical_php_str {
						return Some(php_file.clone());
					}
				}
			}
		}

		// Try relative path matching for different path prefixes
		if let Some(target_file_name) = target_path.file_name() {
			if let Some(target_parent) = target_path.parent() {
				for php_file in registry.get_all_files() {
					let php_path = std::path::Path::new(php_file);
					if let Some(php_file_name) = php_path.file_name() {
						if let Some(php_parent) = php_path.parent() {
							// Match if filename and relative parent path match
							if target_file_name == php_file_name {
								if let (Some(target_parent_str), Some(php_parent_str)) =
									(target_parent.to_str(), php_parent.to_str())
								{
									// Check if the parent paths end with the same structure
									if target_parent_str.ends_with(php_parent_str)
										|| php_parent_str.ends_with(target_parent_str)
									{
										return Some(php_file.clone());
									}
								}
							}
						}
					}
				}
			}
		}

		None
	}

	/// Enhanced namespace import resolution with PSR-4 autoloading patterns
	fn resolve_namespace_import(
		&self,
		file_path: &str,
		source_file: &str,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		let source_path = std::path::Path::new(source_file);
		let source_dir = source_path.parent()?;

		// PSR-4 autoloading patterns - try multiple namespace-to-path mappings
		let namespace_parts: Vec<&str> = file_path.split('/').collect();

		// Try different PSR-4 patterns working backwards from full path
		for end_idx in (1..=namespace_parts.len()).rev() {
			let partial_path = namespace_parts[0..end_idx].join("/");

			// Common PSR-4 patterns
			let candidates = vec![
				// Direct mapping: App\Config -> src/App/Config.php
				format!("src/{}.php", partial_path),
				format!("lib/{}.php", partial_path),
				format!("app/{}.php", partial_path),
				// Lowercase variants: App\Config -> src/app/config.php
				format!("src/{}.php", partial_path.to_lowercase()),
				format!("lib/{}.php", partial_path.to_lowercase()),
				format!("app/{}.php", partial_path.to_lowercase()),
				// Direct file: Config -> Config.php
				format!("{}.php", partial_path),
				// Index file: Config -> Config/index.php
				format!("{}/index.php", partial_path),
				// Vendor autoloading: Vendor\Package\Class -> vendor/package/src/Class.php
				format!("vendor/{}.php", partial_path.to_lowercase()),
				format!(
					"vendor/{}/src/{}.php",
					namespace_parts.first().unwrap_or(&"").to_lowercase(),
					namespace_parts[1..].join("/")
				),
			];

			// Try each candidate pattern
			for candidate in &candidates {
				// Try absolute from project root
				if let Some(found) = registry.find_exact_file(candidate) {
					return Some(found);
				}

				// Try relative to source directory
				let relative_path = source_dir.join(candidate);
				if let Some(found) = self.find_matching_php_file(&relative_path, registry) {
					return Some(found);
				}
			}
		}

		None
	}
}
