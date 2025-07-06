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

//! JavaScript language implementation for the indexer

use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct JavaScript {}

impl Language for JavaScript {
	fn name(&self) -> &'static str {
		"javascript"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_javascript::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec![
			"function_declaration",
			"method_definition",
			"arrow_function",
			// Removed: "class_declaration" - too large, not semantic
			// Individual methods inside classes will be captured via method_definition
			"import_statement",
			"export_statement",
		]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"function_declaration" | "method_definition" => {
				// Extract name of the function or method
				for child in node.children(&mut node.walk()) {
					if child.kind() == "identifier" || child.kind().contains("name") {
						if let Ok(n) = child.utf8_text(contents.as_bytes()) {
							symbols.push(n.to_string());
						}
						break;
					}
				}

				// Look for variable declarations within the function/method body
				for child in node.children(&mut node.walk()) {
					if child.kind() == "statement_block" {
						self.extract_js_variable_declarations(child, contents, &mut symbols);
						break;
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
			_ => self.extract_identifiers(node, contents, &mut symbols),
		}

		// Deduplicate symbols before returning
		symbols.sort();
		symbols.dedup();

		symbols
	}

	fn extract_identifiers(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		let kind = node.kind();
		// Check if this is a valid identifier and not a property identifier
		if (kind.contains("identifier") || kind.contains("name")) && kind != "property_identifier" {
			if let Ok(text) = node.utf8_text(contents.as_bytes()) {
				let t = text.trim();
				if !t.is_empty() && !symbols.contains(&t.to_string()) {
					symbols.push(t.to_string());
				}
			}
		}

		// For JavaScript avoid excessive recursion into certain nodes
		// that tend to duplicate identifiers
		if node.kind() == "member_expression" || node.kind() == "property_access_expression" {
			// For member expressions, only take the object part (leftmost identifier)
			let mut cursor = node.walk();
			if cursor.goto_first_child() {
				// Process just the first child (object)
				self.extract_identifiers(cursor.node(), contents, symbols);
				return;
			}
		}

		// Continue with normal recursion for other nodes
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

		// JavaScript-specific semantic groups
		let semantic_groups = [
			// Functions and methods
			&[
				"function_declaration",
				"method_definition",
				"arrow_function",
			] as &[&str],
			// Classes and constructors
			&["class_declaration", "method_definition"],
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
				// Handle: import { foo, bar } from 'module'
				// Handle: import foo from 'module'
				// Handle: import * as foo from 'module'
				if let Ok(import_text) = node.utf8_text(contents.as_bytes()) {
					if let Some(imported_paths) = parse_js_import_statement_full_path(import_text) {
						imports.extend(imported_paths);
					}
				}
			}
			"export_statement" => {
				// Handle: export { foo, bar }
				// Handle: export function foo() {}
				// Handle: export default foo
				if let Ok(export_text) = node.utf8_text(contents.as_bytes()) {
					if let Some(exported_items) = parse_js_export_statement(export_text) {
						exports.extend(exported_items);
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
		use super::resolution_utils::FileRegistry;

		let registry = FileRegistry::new(all_files);

		if import_path.starts_with("./") || import_path.starts_with("../") {
			// Relative import
			self.resolve_relative_import(import_path, source_file, &registry)
		} else if import_path.starts_with('/') {
			// Absolute import from project root
			self.resolve_absolute_import(import_path, &registry)
		} else {
			// Module import - look in node_modules or as relative
			self.resolve_module_import(import_path, source_file, &registry)
		}
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["js", "jsx", "mjs"]
	}
}

impl JavaScript {
	/// Find matching JavaScript file with robust path comparison (same pattern as Rust/PHP)
	fn find_matching_js_file(
		&self,
		target_path: &std::path::Path,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		let target_str = target_path.to_string_lossy().to_string();

		// Try exact string match first (fastest)
		if let Some(exact_match) = registry.get_all_files().iter().find(|f| *f == &target_str) {
			return Some(exact_match.clone());
		}

		// Try with common JS extensions if not present
		let extensions = ["js", "jsx", "mjs", "ts", "tsx"];
		for ext in &extensions {
			let with_ext = if target_str.ends_with(&format!(".{}", ext)) {
				target_str.clone()
			} else {
				format!("{}.{}", target_str, ext)
			};

			if let Some(exact_match) = registry.get_all_files().iter().find(|f| *f == &with_ext) {
				return Some(exact_match.clone());
			}
		}

		// Try index files in directory
		let index_candidates = [
			"index.js",
			"index.jsx",
			"index.mjs",
			"index.ts",
			"index.tsx",
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
			for js_file in registry.get_all_files() {
				if let Ok(canonical_js) = std::path::Path::new(js_file).canonicalize() {
					let canonical_js_str = canonical_js.to_string_lossy().to_string();
					if canonical_str == canonical_js_str {
						return Some(js_file.clone());
					}
				}
			}
		}

		// Try relative path matching for different path prefixes
		if let Some(target_file_name) = target_path.file_name() {
			if let Some(target_parent) = target_path.parent() {
				for js_file in registry.get_all_files() {
					let js_path = std::path::Path::new(js_file);
					if let Some(js_file_name) = js_path.file_name() {
						if let Some(js_parent) = js_path.parent() {
							// Match if filename and relative parent path match
							if target_file_name == js_file_name {
								if let (Some(target_parent_str), Some(js_parent_str)) =
									(target_parent.to_str(), js_parent.to_str())
								{
									// Check if the parent paths end with the same structure
									if target_parent_str.ends_with(js_parent_str)
										|| js_parent_str.ends_with(target_parent_str)
									{
										return Some(js_file.clone());
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

	/// Extract JavaScript variable declarations within a block
	#[allow(clippy::only_used_in_recursion)]
	pub fn extract_js_variable_declarations(
		&self,
		node: Node,
		contents: &str,
		symbols: &mut Vec<String>,
	) {
		let mut cursor = node.walk();
		// Look through all children
		if cursor.goto_first_child() {
			loop {
				let child = cursor.node();
				// Look for variable or lexical declarations
				if child.kind() == "variable_declaration" || child.kind() == "lexical_declaration" {
					// For each declarator in the declaration
					for var_decl in child.children(&mut child.walk()) {
						if var_decl.kind() == "variable_declarator" {
							// Get the identifier from the declarator
							for decl_child in var_decl.children(&mut var_decl.walk()) {
								if decl_child.kind() == "identifier" {
									if let Ok(name) = decl_child.utf8_text(contents.as_bytes()) {
										let t = name.trim();
										if !t.is_empty() && !symbols.contains(&t.to_string()) {
											symbols.push(t.to_string());
										}
									}
									break; // Only take the first identifier (the variable name)
								}
							}
						}
					}
				}
				// Recursive search in nested blocks (if, for, while loops, etc.)
				else if child.kind() == "statement_block" || child.kind().contains("statement") {
					self.extract_js_variable_declarations(child, contents, symbols);
				}

				if !cursor.goto_next_sibling() {
					break;
				}
			}
		}
	}
}

// Helper functions for JavaScript import/export parsing

// Extract full import paths for GraphRAG (not just imported item names)
pub fn parse_js_import_statement_full_path(import_text: &str) -> Option<Vec<String>> {
	let mut imports = Vec::new();
	let cleaned = import_text.trim();

	// Extract the module path from 'from' clause
	if cleaned.contains(" from ") {
		if let Some(from_pos) = cleaned.find(" from ") {
			let module_part = &cleaned[from_pos + 6..]; // Skip " from "
											   // Remove all quotes and semicolon around module path
			let module_path = module_part
				.trim()
				.trim_start_matches('\'')
				.trim_start_matches('"')
				.trim_end_matches(';')
				.trim_end_matches('\'')
				.trim_end_matches('"');
			if !module_path.is_empty() {
				imports.push(module_path.to_string());
			}
		}
	}

	if !imports.is_empty() {
		Some(imports)
	} else {
		None
	}
}

pub fn parse_js_import_statement(import_text: &str) -> Option<Vec<String>> {
	let mut imports = Vec::new();
	let cleaned = import_text.trim();

	// Handle: import { foo, bar } from 'module'
	if let Some(start) = cleaned.find('{') {
		if let Some(end) = cleaned.find('}') {
			let items = &cleaned[start + 1..end];
			for item in items.split(',') {
				let item = item.trim();
				// Handle: foo as bar -> extract 'foo'
				let name = if let Some(as_pos) = item.find(" as ") {
					&item[..as_pos]
				} else {
					item
				};
				if !name.is_empty() {
					imports.push(name.to_string());
				}
			}
			return Some(imports);
		}
	}

	// Handle: import foo from 'module'
	if cleaned.starts_with("import ") && cleaned.contains(" from ") {
		if let Some(from_pos) = cleaned.find(" from ") {
			let import_part = &cleaned[7..from_pos].trim(); // Skip "import "
			if !import_part.starts_with('{') && !import_part.starts_with('*') {
				imports.push(import_part.to_string());
				return Some(imports);
			}
		}
	}

	// Handle: import * as foo from 'module'
	if cleaned.contains("* as ") {
		if let Some(as_pos) = cleaned.find("* as ") {
			if let Some(from_pos) = cleaned.find(" from ") {
				let alias = &cleaned[as_pos + 5..from_pos].trim();
				imports.push(alias.to_string());
				return Some(imports);
			}
		}
	}

	None
}

pub fn parse_js_export_statement(export_text: &str) -> Option<Vec<String>> {
	let mut exports = Vec::new();
	let cleaned = export_text.trim();

	// Get the first line for parsing (export statements are usually on the first line)
	let first_line = cleaned.lines().next().unwrap_or(cleaned);

	// Handle: export { foo, bar }
	if let Some(start) = first_line.find('{') {
		if let Some(end) = first_line.find('}') {
			let items = &first_line[start + 1..end];
			for item in items.split(',') {
				let item = item.trim();
				// Handle: foo as bar -> extract 'foo'
				let name = if let Some(as_pos) = item.find(" as ") {
					&item[..as_pos]
				} else {
					item
				};
				if !name.is_empty() {
					exports.push(name.to_string());
				}
			}
			return Some(exports);
		}
	}

	// Handle: export function foo() {} or export const foo = ...
	if let Some(rest) = first_line.strip_prefix("export ") {
		// Skip "export "
		if rest.starts_with("function ")
			|| rest.starts_with("const ")
			|| rest.starts_with("let ")
			|| rest.starts_with("var ")
		{
			// Extract identifier after keyword
			let parts: Vec<&str> = rest.split_whitespace().collect();
			if parts.len() >= 2 {
				let name = parts[1].trim_end_matches('(').trim_end_matches('=');
				exports.push(name.to_string());
				return Some(exports);
			}
		}

		// Handle: export default class ClassName
		if rest.starts_with("default class ") {
			let parts: Vec<&str> = rest.split_whitespace().collect();
			if parts.len() >= 3 {
				let name = parts[2].trim_end_matches('{');
				exports.push(name.to_string());
				return Some(exports);
			}
		}

		// Handle: export class ClassName
		if rest.starts_with("class ") {
			let parts: Vec<&str> = rest.split_whitespace().collect();
			if parts.len() >= 2 {
				let name = parts[1].trim_end_matches('{');
				exports.push(name.to_string());
				return Some(exports);
			}
		}
	}

	None
}

impl JavaScript {
	/// Resolve relative imports like ./utils or ../components/Button with enhanced path matching
	fn resolve_relative_import(
		&self,
		import_path: &str,
		source_file: &str,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		use super::resolution_utils::resolve_relative_path;

		let relative_path = resolve_relative_path(source_file, import_path)?;

		// Use enhanced file matching
		self.find_matching_js_file(&relative_path, registry)
	}

	/// Resolve absolute imports from project root with enhanced path matching
	fn resolve_absolute_import(
		&self,
		import_path: &str,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		let path = std::path::Path::new(import_path);

		// Use enhanced file matching
		self.find_matching_js_file(path, registry)
	}

	/// Enhanced module import resolution with better patterns
	fn resolve_module_import(
		&self,
		import_path: &str,
		source_file: &str,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		// Try multiple resolution strategies for module imports

		// 1. Try as relative import from current directory
		let relative_import = format!("./{}", import_path);
		if let Some(found) = self.resolve_relative_import(&relative_import, source_file, registry) {
			return Some(found);
		}

		// 2. Try in common source directories
		let source_dirs = ["src", "lib", "components", "utils", "modules"];
		for src_dir in &source_dirs {
			let src_path = std::path::Path::new(src_dir).join(import_path);
			if let Some(found) = self.find_matching_js_file(&src_path, registry) {
				return Some(found);
			}
		}

		// 3. Try node_modules resolution (simplified)
		let node_modules_path = std::path::Path::new("node_modules").join(import_path);
		if let Some(found) = self.find_matching_js_file(&node_modules_path, registry) {
			return Some(found);
		}

		// 4. Try package.json main field simulation (common patterns)
		let package_patterns = [
			format!("node_modules/{}/index.js", import_path),
			format!("node_modules/{}/lib/index.js", import_path),
			format!("node_modules/{}/src/index.js", import_path),
			format!("node_modules/{}/dist/index.js", import_path),
		];

		for pattern in &package_patterns {
			let package_path = std::path::Path::new(pattern);
			if let Some(found) = self.find_matching_js_file(package_path, registry) {
				return Some(found);
			}
		}

		None
	}
}
