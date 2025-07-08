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

//! TypeScript language implementation for the indexer

use crate::indexer::languages::{JavaScript, Language};
use tree_sitter::Node;

pub struct TypeScript {}

impl Language for TypeScript {
	fn name(&self) -> &'static str {
		"typescript"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec![
			"function_declaration",
			"method_definition",
			"class_declaration",
			"arrow_function",
			"interface_declaration",
			"type_alias_declaration",
			"import_statement",
			"export_statement",
		]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"function_declaration"
			| "method_definition"
			| "class_declaration"
			| "interface_declaration"
			| "type_alias_declaration" => {
				// Extract name of the function, method, class, interface or type
				for child in node.children(&mut node.walk()) {
					if child.kind() == "identifier" || child.kind().contains("name") {
						if let Ok(n) = child.utf8_text(contents.as_bytes()) {
							symbols.push(n.to_string());
						}
						break;
					}
				}

				// For functions and methods, look for variable declarations within the body
				if node.kind() == "function_declaration" || node.kind() == "method_definition" {
					for child in node.children(&mut node.walk()) {
						if child.kind() == "statement_block" {
							let js = JavaScript {};
							js.extract_js_variable_declarations(child, contents, &mut symbols);
							break;
						}
					}
				}
			}
			"arrow_function" => {
				// Extract parent variable name for arrow functions
				if let Some(parent) = node.parent() {
					if parent.kind() == "variable_declarator" {
						for child in parent.children(&mut parent.walk()) {
							if child.kind() == "identifier" {
								if let Ok(n) = child.utf8_text(contents.as_bytes()) {
									symbols.push(n.to_string());
								}
								break;
							}
						}
					}
				}
			}
			_ => {
				let js = JavaScript {};
				js.extract_identifiers(node, contents, &mut symbols);
			}
		}

		// Deduplicate symbols before returning
		symbols.sort();
		symbols.dedup();

		symbols
	}

	fn extract_identifiers(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		// Reuse JavaScript's identifier extraction logic
		let js = JavaScript {};
		js.extract_identifiers(node, contents, symbols);
	}

	fn are_node_types_equivalent(&self, type1: &str, type2: &str) -> bool {
		// Direct match
		if type1 == type2 {
			return true;
		}

		// TypeScript-specific semantic groups
		let semantic_groups = [
			// Functions and methods
			&[
				"function_declaration",
				"method_definition",
				"arrow_function",
			] as &[&str],
			// Classes and interfaces
			&["class_declaration", "interface_declaration"],
			// Type definitions
			&["type_alias_declaration", "interface_declaration"],
			// Import/export statements
			&["import_statement", "export_statement"],
			// Variable declarations
			&["variable_declaration", "lexical_declaration"],
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
			"function_declaration" | "method_definition" | "arrow_function" => {
				"function declarations"
			}
			"class_declaration" => "class declarations",
			"interface_declaration" => "interface declarations",
			"type_alias_declaration" => "type declarations",
			"import_statement" | "export_statement" => "import/export statements",
			"variable_declaration" | "lexical_declaration" => "variable declarations",
			_ => "declarations",
		}
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		let mut imports = Vec::new();
		let mut exports = Vec::new();

		match node.kind() {
			"import_statement" => {
				// TypeScript supports same import patterns as JavaScript plus type imports
				// Handle: import type { Foo } from 'module'
				// Handle: import { type Foo, Bar } from 'module'
				if let Ok(import_text) = node.utf8_text(contents.as_bytes()) {
					if let Some(imported_items) = parse_ts_import_statement(import_text) {
						imports.extend(imported_items);
					}
				}
			}
			"export_statement" => {
				// TypeScript supports same export patterns as JavaScript plus type exports
				// Handle: export type { Foo }
				// Handle: export { type Foo, Bar }
				if let Ok(export_text) = node.utf8_text(contents.as_bytes()) {
					if let Some(exported_items) = parse_ts_export_statement(export_text) {
						exports.extend(exported_items);
					}
				}
			}
			"function_declaration"
			| "method_definition"
			| "arrow_function"
			| "class_declaration"
			| "interface_declaration"
			| "type_alias_declaration" => {
				// Check if this declaration is exported
				let parent = node.parent();
				if let Some(p) = parent {
					if p.kind() == "export_statement" {
						// Extract declaration name as export
						for child in node.children(&mut node.walk()) {
							if child.kind() == "identifier" || child.kind() == "type_identifier" {
								if let Ok(name) = child.utf8_text(contents.as_bytes()) {
									exports.push(name.to_string());
									break;
								}
							}
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
			// Relative import - handle TypeScript-specific extensions with enhanced matching
			if let Some(relative_path) = resolve_relative_path(source_file, import_path) {
				// Use enhanced TypeScript file matching
				return self.find_matching_ts_file(&relative_path, &registry);
			}
		} else {
			// Use enhanced JavaScript resolution for absolute/module imports
			// but also try TypeScript-specific patterns
			let js = super::javascript::JavaScript {};
			if let Some(js_result) = js.resolve_import(import_path, source_file, all_files) {
				return Some(js_result);
			}

			// Try TypeScript-specific module resolution
			return self.resolve_ts_module_import(import_path, source_file, &registry);
		}

		None
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["ts", "tsx"]
	}
}

impl TypeScript {
	/// Find matching TypeScript file with robust path comparison (enhanced for TS)
	fn find_matching_ts_file(
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

		// Try cross-platform string comparison (normalize separators)
		let normalized_target = target_str.replace('\\', "/");
		for ts_file in registry.get_all_files() {
			let normalized_ts = ts_file.replace('\\', "/");
			if normalized_target == normalized_ts {
				return Some(ts_file.clone());
			}
		}

		// Try with TypeScript extensions (prioritize .ts over .js for TS projects)
		let ts_extensions = ["ts", "tsx", "d.ts", "js", "jsx", "mjs"];
		for ext in &ts_extensions {
			let with_ext = if target_str.ends_with(&format!(".{}", ext)) {
				target_str.clone()
			} else {
				format!("{}.{}", target_str, ext)
			};
			if let Some(exact_match) = crate::utils::path::PathNormalizer::find_path_in_collection(
				&with_ext,
				registry.get_all_files(),
			) {
				return Some(exact_match.to_string());
			}
		}

		// Try index files in directory (TypeScript-first)
		let index_candidates = [
			"index.ts",
			"index.tsx",
			"index.d.ts",
			"index.js",
			"index.jsx",
		];
		for index_file in &index_candidates {
			let index_path = target_path.join(index_file);
			let index_str = index_path.to_string_lossy().to_string();
			if let Some(exact_match) = registry.get_all_files().iter().find(|f| *f == &index_str) {
				return Some(exact_match.clone());
			}
		}

		// Try normalized path comparison for cross-platform compatibility
		if let Ok(canonical_target) = target_path.canonicalize() {
			let canonical_str = canonical_target.to_string_lossy().to_string();
			for ts_file in registry.get_all_files() {
				if let Ok(canonical_ts) = std::path::Path::new(ts_file).canonicalize() {
					let canonical_ts_str = canonical_ts.to_string_lossy().to_string();
					if canonical_str == canonical_ts_str {
						return Some(ts_file.clone());
					}
				}
			}
		}

		// Try relative path matching for different path prefixes
		if let Some(target_file_name) = target_path.file_name() {
			if let Some(target_parent) = target_path.parent() {
				for ts_file in registry.get_all_files() {
					let ts_path = std::path::Path::new(ts_file);
					if let Some(ts_file_name) = ts_path.file_name() {
						if let Some(ts_parent) = ts_path.parent() {
							// Match if filename and relative parent path match
							if target_file_name == ts_file_name {
								if let (Some(target_parent_str), Some(ts_parent_str)) =
									(target_parent.to_str(), ts_parent.to_str())
								{
									// Cross-platform path comparison using PathNormalizer
									if crate::utils::path::PathNormalizer::paths_equal(
										target_parent_str,
										ts_parent_str,
									) {
										return Some(ts_file.clone());
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

	/// TypeScript-specific module resolution (types, @types, etc.)
	fn resolve_ts_module_import(
		&self,
		import_path: &str,
		_source_file: &str,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		// Try TypeScript-specific patterns

		// 1. Try @types packages
		if !import_path.starts_with('@') {
			let types_path = format!("node_modules/@types/{}", import_path);
			let types_index = std::path::Path::new(&types_path).join("index.d.ts");
			if let Some(found) = self.find_matching_ts_file(&types_index, registry) {
				return Some(found);
			}
		}

		// 2. Try in common TypeScript source directories
		let ts_source_dirs = ["src", "lib", "types", "typings", "components", "utils"];
		for src_dir in &ts_source_dirs {
			let src_path = std::path::Path::new(src_dir).join(import_path);
			if let Some(found) = self.find_matching_ts_file(&src_path, registry) {
				return Some(found);
			}
		}

		// 3. Try node_modules with TypeScript files
		let node_modules_patterns = [
			format!("node_modules/{}/index.d.ts", import_path),
			format!("node_modules/{}/lib/index.d.ts", import_path),
			format!("node_modules/{}/src/index.ts", import_path),
			format!("node_modules/{}/dist/index.d.ts", import_path),
			format!("node_modules/{}/types/index.d.ts", import_path),
		];

		for pattern in &node_modules_patterns {
			let package_path = std::path::Path::new(pattern);
			if let Some(found) = self.find_matching_ts_file(package_path, registry) {
				return Some(found);
			}
		}

		None
	}
}

// Helper functions for TypeScript import/export parsing
fn parse_ts_import_statement(import_text: &str) -> Option<Vec<String>> {
	let mut imports = Vec::new();
	let cleaned = import_text.trim();

	// Handle TypeScript type imports: import type { Foo } from 'module'
	if let Some(type_import) = cleaned.strip_prefix("import type ") {
		// Skip "import type "
		if let Some(start) = type_import.find('{') {
			if let Some(end) = type_import.find('}') {
				let items = &type_import[start + 1..end];
				for item in items.split(',') {
					let item = item.trim();
					if !item.is_empty() {
						imports.push(item.to_string());
					}
				}
				return Some(imports);
			}
		}
	}

	// Handle mixed imports: import { type Foo, Bar } from 'module'
	if let Some(start) = cleaned.find('{') {
		if let Some(end) = cleaned.find('}') {
			let items = &cleaned[start + 1..end];
			for item in items.split(',') {
				let item = item.trim();
				// Remove 'type' keyword if present
				let name = if let Some(stripped) = item.strip_prefix("type ") {
					stripped
				} else {
					item
				};
				// Handle: foo as bar -> extract 'foo'
				let name = if let Some(as_pos) = name.find(" as ") {
					&name[..as_pos]
				} else {
					name
				};
				if !name.is_empty() {
					imports.push(name.to_string());
				}
			}
			return Some(imports);
		}
	}

	// Fall back to JavaScript parsing for regular imports
	// Reuse JavaScript logic for standard cases
	parse_js_import_statement(import_text)
}

fn parse_ts_export_statement(export_text: &str) -> Option<Vec<String>> {
	let mut exports = Vec::new();
	let cleaned = export_text.trim();

	// Handle TypeScript type exports: export type { Foo }
	if let Some(type_export) = cleaned.strip_prefix("export type ") {
		// Skip "export type "
		if let Some(start) = type_export.find('{') {
			if let Some(end) = type_export.find('}') {
				let items = &type_export[start + 1..end];
				for item in items.split(',') {
					let item = item.trim();
					if !item.is_empty() {
						exports.push(item.to_string());
					}
				}
				return Some(exports);
			}
		}
	}

	// Handle: export interface Foo {} or export type Foo = ...
	if let Some(rest) = cleaned.strip_prefix("export ") {
		// Skip "export "
		if rest.starts_with("interface ") || rest.starts_with("type ") {
			let parts: Vec<&str> = rest.split_whitespace().collect();
			if parts.len() >= 2 {
				let name = parts[1].trim_end_matches('=').trim_end_matches('{');
				exports.push(name.to_string());
				return Some(exports);
			}
		}
	}

	// Fall back to JavaScript parsing for regular exports
	if let Some(js_exports) = parse_js_export_statement(export_text) {
		exports.extend(js_exports);
	}

	Some(exports)
}

// Helper functions for TypeScript import/export parsing
// Re-export JavaScript helper functions for TypeScript to use
use super::javascript::{parse_js_export_statement, parse_js_import_statement};
