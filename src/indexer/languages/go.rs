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

//! Go language implementation for the indexer

use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct Go {}

impl Language for Go {
	fn name(&self) -> &'static str {
		"go"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_go::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec![
			"function_declaration",
			"method_declaration",
			"type_declaration",
			"const_declaration",
			"var_declaration",
			"import_declaration",
		]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"function_declaration" | "method_declaration" => {
				// Extract function or method name
				if let Some(name) = super::extract_symbol_by_kinds(
					node,
					contents,
					&["identifier", "field_identifier"],
				) {
					symbols.push(name);
				}

				// Extract variables declared in function body
				for child in node.children(&mut node.walk()) {
					if child.kind() == "block" {
						self.extract_go_variables(child, contents, &mut symbols);
						break;
					}
				}
			}
			"type_declaration" => {
				// Extract type name
				for child in node.children(&mut node.walk()) {
					if child.kind() == "type_spec" {
						if let Some(name) =
							super::extract_symbol_by_kind(child, contents, "identifier")
						{
							symbols.push(name);
						}
						break;
					}
				}
			}
			"struct_type" | "interface_type" => {
				// Extract field names within structs or interfaces
				self.extract_struct_interface_fields(node, contents, &mut symbols);
			}
			_ => self.extract_identifiers(node, contents, &mut symbols),
		}

		super::deduplicate_symbols(&mut symbols);
		symbols
	}

	fn extract_identifiers(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		super::extract_identifiers_default(node, contents, symbols, |kind, _text| {
			// Include identifiers and field identifiers
			kind == "identifier" || kind == "field_identifier"
		});
	}

	fn are_node_types_equivalent(&self, type1: &str, type2: &str) -> bool {
		// Go-specific semantic groups
		let semantic_groups = [
			// Functions and methods
			&["function_declaration", "method_declaration"] as &[&str],
			// Type definitions
			&["type_declaration", "struct_type", "interface_type"],
			// Variable and constant declarations
			&[
				"var_declaration",
				"const_declaration",
				"short_var_declaration",
			],
			// Import statements
			&["import_declaration"],
		];

		super::check_semantic_groups(type1, type2, &semantic_groups)
	}

	fn get_node_type_description(&self, node_type: &str) -> &'static str {
		match node_type {
			"function_declaration" | "method_declaration" => "function declarations",
			"type_declaration" => "type declarations",
			"struct_type" => "struct definitions",
			"interface_type" => "interface definitions",
			"var_declaration" | "const_declaration" | "short_var_declaration" => {
				"variable declarations"
			}
			"import_declaration" => "import statements",
			_ => "declarations",
		}
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		let mut imports = Vec::new();
		let mut exports = Vec::new();

		match node.kind() {
			"import_declaration" => {
				// Handle: import "package"
				// Handle: import alias "package"
				// Handle: import ( "package1"; "package2" )
				if let Ok(import_text) = node.utf8_text(contents.as_bytes()) {
					if let Some(imported_items) = parse_go_import_statement(import_text) {
						imports.extend(imported_items);
					}
				}
			}
			"function_declaration"
			| "method_declaration"
			| "type_declaration"
			| "const_declaration"
			| "var_declaration" => {
				// In Go, exported items start with uppercase letter
				// Extract the name and check if it's exported
				for child in node.children(&mut node.walk()) {
					if child.kind() == "identifier" || child.kind() == "field_identifier" {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							// Go convention: exported names start with uppercase
							if name.chars().next().is_some_and(|c| c.is_uppercase()) {
								exports.push(name.to_string());
							}
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
		use super::resolution_utils::FileRegistry;

		let registry = FileRegistry::new(all_files);
		let go_files = registry.get_files_with_extensions(&self.get_file_extensions());

		if import_path.starts_with("./") || import_path.starts_with("../") {
			// Relative import
			self.resolve_relative_import(import_path, source_file, &go_files)
		} else {
			// Absolute import - look for package directory
			self.resolve_package_import(import_path, &go_files)
		}
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["go"]
	}
}

impl Go {
	/// Extract variable declarations in Go blocks
	#[allow(clippy::only_used_in_recursion)]
	fn extract_go_variables(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		// Traverse the block looking for variable declarations
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				let child = cursor.node();

				match child.kind() {
					"short_var_declaration" => {
						// Handle short variables like x := 10
						for var_child in child.children(&mut child.walk()) {
							if var_child.kind() == "expression_list" {
								for expr in var_child.children(&mut var_child.walk()) {
									if expr.kind() == "identifier" {
										if let Ok(name) = expr.utf8_text(contents.as_bytes()) {
											if !symbols.contains(&name.to_string()) {
												symbols.push(name.to_string());
											}
										}
									}
								}
								break; // Only process the left side of :=
							}
						}
					}
					"var_declaration" => {
						// Handle var x = 10 or var x int = 10
						for spec in child.children(&mut child.walk()) {
							if spec.kind() == "var_spec" {
								for spec_child in spec.children(&mut spec.walk()) {
									if spec_child.kind() == "identifier" {
										if let Ok(name) = spec_child.utf8_text(contents.as_bytes())
										{
											if !symbols.contains(&name.to_string()) {
												symbols.push(name.to_string());
											}
										}
									}
								}
							}
						}
					}
					"const_declaration" => {
						// Handle const declarations
						for spec in child.children(&mut child.walk()) {
							if spec.kind() == "const_spec" {
								for spec_child in spec.children(&mut spec.walk()) {
									if spec_child.kind() == "identifier" {
										if let Ok(name) = spec_child.utf8_text(contents.as_bytes())
										{
											if !symbols.contains(&name.to_string()) {
												symbols.push(name.to_string());
											}
										}
									}
								}
							}
						}
					}
					"block" => {
						// Recursively process nested blocks
						self.extract_go_variables(child, contents, symbols);
					}
					"if_statement" | "for_statement" | "switch_statement" => {
						// Process blocks inside control structures
						for stmt_child in child.children(&mut child.walk()) {
							if stmt_child.kind() == "block" {
								self.extract_go_variables(stmt_child, contents, symbols);
							}
						}
					}
					_ => {}
				}

				if !cursor.goto_next_sibling() {
					break;
				}
			}
		}
	}

	/// Extract field names from struct or interface types
	fn extract_struct_interface_fields(
		&self,
		node: Node,
		contents: &str,
		symbols: &mut Vec<String>,
	) {
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				let child = cursor.node();

				if child.kind() == "field_declaration" {
					for field_child in child.children(&mut child.walk()) {
						if field_child.kind() == "field_identifier" {
							if let Ok(name) = field_child.utf8_text(contents.as_bytes()) {
								if !symbols.contains(&name.to_string()) {
									symbols.push(name.to_string());
								}
							}
						}
					}
				} else if child.kind() == "method_spec" {
					// For interface methods
					for method_child in child.children(&mut child.walk()) {
						if method_child.kind() == "field_identifier" {
							if let Ok(name) = method_child.utf8_text(contents.as_bytes()) {
								if !symbols.contains(&name.to_string()) {
									symbols.push(name.to_string());
								}
							}
						}
					}
				}

				if !cursor.goto_next_sibling() {
					break;
				}
			}
		}
	}
}
// Helper function for parsing Go import statements
fn parse_go_import_statement(import_text: &str) -> Option<Vec<String>> {
	let mut imports = Vec::new();
	let cleaned = import_text.trim();

	// Handle single import: import "package" or import alias "package"
	if cleaned.starts_with("import ") && !cleaned.contains('(') {
		let rest = &cleaned[7..].trim(); // Skip "import "

		// Handle: import alias "package"
		let parts: Vec<&str> = rest.split_whitespace().collect();
		if parts.len() == 2 {
			// Has alias
			let alias = parts[0];
			imports.push(alias.to_string());
		} else if parts.len() == 1 {
			// No alias, extract package name from path
			let package_path = parts[0].trim_matches('"');
			if let Some(package_name) = package_path.split('/').next_back() {
				imports.push(package_name.to_string());
			}
		}
		return Some(imports);
	}

	// Handle grouped imports: import ( ... )
	if cleaned.contains('(') && cleaned.contains(')') {
		if let Some(start) = cleaned.find('(') {
			if let Some(end) = cleaned.rfind(')') {
				let imports_block = &cleaned[start + 1..end];
				for line in imports_block.lines() {
					let line = line.trim();
					if line.is_empty() || line.starts_with("//") {
						continue;
					}

					// Handle: alias "package" or "package"
					let parts: Vec<&str> = line.split_whitespace().collect();
					if parts.len() == 2 {
						// Has alias
						let alias = parts[0];
						imports.push(alias.to_string());
					} else if parts.len() == 1 {
						// No alias, extract package name from path
						let package_path = parts[0].trim_matches('"');
						if let Some(package_name) = package_path.split('/').next_back() {
							imports.push(package_name.to_string());
						}
					}
				}
				return Some(imports);
			}
		}
	}

	None
}

impl Go {
	/// Resolve relative imports in Go
	fn resolve_relative_import(
		&self,
		import_path: &str,
		source_file: &str,
		go_files: &[String],
	) -> Option<String> {
		use super::resolution_utils::resolve_relative_path;

		let relative_path = resolve_relative_path(source_file, import_path)?;

		// Look for any .go file in the target directory
		for go_file in go_files {
			let file_path = std::path::Path::new(go_file);
			if let Some(file_dir) = file_path.parent() {
				if file_dir == relative_path {
					return Some(go_file.clone());
				}
			}
		}

		None
	}

	/// Resolve package imports by name
	fn resolve_package_import(&self, package_name: &str, go_files: &[String]) -> Option<String> {
		// Look for package directory by name
		for go_file in go_files {
			let file_path = std::path::Path::new(go_file);
			if let Some(file_dir) = file_path.parent() {
				if let Some(dir_name) = file_dir.file_name() {
					if dir_name.to_string_lossy() == package_name {
						return Some(go_file.clone());
					}
				}
			}
		}

		None
	}
}
