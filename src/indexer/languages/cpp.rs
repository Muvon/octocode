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

//! C++ language implementation for the indexer

use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct Cpp {}

impl Language for Cpp {
	fn name(&self) -> &'static str {
		"cpp"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_cpp::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec![
			"function_definition",
			"declaration", // For function declarations in headers
			"class_specifier",
			"struct_specifier",
			"enum_specifier",
			"namespace_definition",
			"preproc_include", // For #include statements
		]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"function_definition" => {
				// Find function name
				for child in node.children(&mut node.walk()) {
					if child.kind() == "function_declarator" {
						for decl_child in child.children(&mut child.walk()) {
							if decl_child.kind() == "identifier" {
								if let Ok(name) = decl_child.utf8_text(contents.as_bytes()) {
									symbols.push(name.to_string());
								}
								break;
							}
						}
						break;
					}
				}

				// Extract variables from function body
				for child in node.children(&mut node.walk()) {
					if child.kind() == "compound_statement" {
						self.extract_cpp_variables(child, contents, &mut symbols);
						break;
					}
				}
			}
			"declaration" => {
				// Handle both function declarations and variable declarations
				let mut found_function = false;

				// First, check if this is a function declaration
				for child in node.children(&mut node.walk()) {
					if child.kind() == "function_declarator" {
						found_function = true;
						for decl_child in child.children(&mut child.walk()) {
							if decl_child.kind() == "identifier" {
								if let Ok(name) = decl_child.utf8_text(contents.as_bytes()) {
									symbols.push(name.to_string());
								}
								break;
							}
						}
						break;
					}
				}

				// If not a function declaration, extract variable names
				if !found_function {
					for child in node.children(&mut node.walk()) {
						if child.kind() == "init_declarator" || child.kind() == "declarator" {
							for decl_child in child.children(&mut child.walk()) {
								if decl_child.kind() == "identifier" {
									if let Ok(name) = decl_child.utf8_text(contents.as_bytes()) {
										symbols.push(name.to_string());
									}
									break;
								}
							}
						}
					}
				}
			}
			"class_specifier" | "struct_specifier" | "enum_specifier" => {
				// Find class/struct/enum name
				for child in node.children(&mut node.walk()) {
					if child.kind() == "name" || child.kind() == "type_identifier" {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							symbols.push(name.to_string());
						}
						break;
					}
				}

				// Extract member names
				self.extract_cpp_members(node, contents, &mut symbols);
			}
			"namespace_definition" => {
				// Find namespace name
				for child in node.children(&mut node.walk()) {
					if child.kind() == "identifier" || child.kind() == "namespace_identifier" {
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
		// Check if this is a valid identifier
		if kind == "identifier" || kind == "type_identifier" || kind == "field_identifier" {
			if let Ok(text) = node.utf8_text(contents.as_bytes()) {
				let t = text.trim();
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

		// C++-specific semantic groups
		let semantic_groups = [
			// Functions and methods
			&["function_definition"] as &[&str],
			// Type definitions
			&["class_specifier", "struct_specifier", "enum_specifier"],
			// Namespaces
			&["namespace_definition"],
			// Templates
			&["template_declaration"],
			// Variable declarations (group standalone variable declarations together)
			&["declaration"],
			// Preprocessor directives
			&[
				"preproc_include",
				"preproc_define",
				"preproc_ifdef",
				"preproc_ifndef",
			],
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
			"function_definition" => "function declarations",
			"class_specifier" => "class declarations",
			"struct_specifier" => "struct declarations",
			"enum_specifier" => "enum declarations",
			"namespace_definition" => "namespace declarations",
			"template_declaration" => "template declarations",
			"declaration" => "variable declarations",
			"preproc_include" | "preproc_define" | "preproc_ifdef" | "preproc_ifndef" => {
				"preprocessor directives"
			}
			_ => "declarations",
		}
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		let mut imports = Vec::new();
		let mut exports = Vec::new();

		match node.kind() {
			// Extract imports from #include statements
			"preproc_include" => {
				if let Ok(include_text) = node.utf8_text(contents.as_bytes()) {
					if let Some(header) = Self::parse_cpp_include(include_text) {
						imports.push(header);
					}
				}
			}
			// Extract exports from function definitions and declarations
			"function_definition" | "declaration" => {
				// For function definitions, always consider them as exports
				if node.kind() == "function_definition" {
					if let Some(function_name) = self.extract_function_name(node, contents) {
						exports.push(function_name);
					}
				} else {
					// For declarations, check if it's a function declaration
					let mut is_function_declaration = false;
					for child in node.children(&mut node.walk()) {
						if child.kind() == "function_declarator" {
							is_function_declaration = true;
							if let Some(function_name) = self.extract_function_name(node, contents)
							{
								exports.push(function_name);
							}
							break;
						}
					}

					// If not a function declaration, it might be a variable declaration
					// Export global variables and constants
					if !is_function_declaration {
						if let Some(var_names) = self.extract_variable_names(node, contents) {
							exports.extend(var_names);
						}
					}
				}
			}
			// Extract exports from class, struct, and enum declarations
			"class_specifier" | "struct_specifier" | "enum_specifier" => {
				if let Some(type_name) = self.extract_type_name(node, contents) {
					exports.push(type_name);
				}
			}
			// Extract exports from namespace declarations
			"namespace_definition" => {
				if let Some(namespace_name) = self.extract_namespace_name(node, contents) {
					exports.push(namespace_name);
				}
			}
			// Extract exports from template declarations
			"template_declaration" => {
				// Template declarations can contain functions, classes, etc.
				// Look for the actual declaration inside the template
				for child in node.children(&mut node.walk()) {
					match child.kind() {
						"function_definition" | "class_specifier" | "struct_specifier" => {
							if let Some(template_name) = self.extract_template_name(child, contents)
							{
								exports.push(format!("template<{}>", template_name));
							}
						}
						_ => {}
					}
				}
			}
			// Extract exports from macro definitions
			"preproc_define" => {
				if let Some(macro_name) = self.extract_macro_name(node, contents) {
					exports.push(macro_name);
				}
			}
			// Extract exports from typedef declarations
			"type_definition" => {
				if let Some(typedef_name) = self.extract_typedef_name(node, contents) {
					exports.push(typedef_name);
				}
			}
			_ => {
				// For other node types, don't extract anything
				// This prevents noise and focuses on meaningful exports
			}
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

		if import_path.starts_with('"') && import_path.ends_with('"') {
			// Local include: #include "header.h"
			let header_name = &import_path[1..import_path.len() - 1];
			self.resolve_local_include(header_name, source_file, &registry)
		} else if import_path.starts_with('<') && import_path.ends_with('>') {
			// System include: #include <header.h> - try to find in project
			let header_name = &import_path[1..import_path.len() - 1];
			registry.find_exact_file(header_name)
		} else {
			// Direct path or unquoted header - try local resolution first, then search common directories
			if let Some(local_result) =
				self.resolve_local_include(import_path, source_file, &registry)
			{
				Some(local_result)
			} else if let Some(exact_result) = registry.find_exact_file(import_path) {
				Some(exact_result)
			} else {
				// Search in common include directories
				let include_dirs = ["include", "inc", "headers", "lib"];
				for include_dir in &include_dirs {
					let full_path = std::path::Path::new(include_dir).join(import_path);
					if let Some(result) = registry.find_exact_file(&full_path.to_string_lossy()) {
						return Some(result);
					}
				}
				None
			}
		}
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["cpp", "cc", "cxx", "c++", "c", "h", "hpp"]
	}
}

impl Cpp {
	/// Extract variable declarations in C++ compound statements
	#[allow(clippy::only_used_in_recursion)]
	fn extract_cpp_variables(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				let child = cursor.node();

				match child.kind() {
					"declaration" => {
						// Handle variable declarations
						for decl_child in child.children(&mut child.walk()) {
							if decl_child.kind() == "init_declarator"
								|| decl_child.kind() == "declarator"
							{
								for init_child in decl_child.children(&mut decl_child.walk()) {
									if init_child.kind() == "identifier" {
										if let Ok(name) = init_child.utf8_text(contents.as_bytes())
										{
											if !symbols.contains(&name.to_string()) {
												symbols.push(name.to_string());
											}
										}
										break;
									}
								}
							}
						}
					}
					"compound_statement" => {
						// Recursively process nested blocks
						self.extract_cpp_variables(child, contents, symbols);
					}
					"if_statement" | "for_statement" | "while_statement" | "do_statement" => {
						// Process compound statements in control structures
						for stmt_child in child.children(&mut child.walk()) {
							if stmt_child.kind() == "compound_statement" {
								self.extract_cpp_variables(stmt_child, contents, symbols);
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

	/// Extract members from class/struct/enum
	fn extract_cpp_members(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				let child = cursor.node();

				match child.kind() {
					"field_declaration" => {
						// Extract field names
						for field_child in child.children(&mut child.walk()) {
							if field_child.kind() == "field_identifier"
								|| field_child.kind() == "identifier"
							{
								if let Ok(name) = field_child.utf8_text(contents.as_bytes()) {
									if !symbols.contains(&name.to_string()) {
										symbols.push(name.to_string());
									}
								}
							}
						}
					}
					"function_definition" => {
						// Handle method definitions
						for fn_child in child.children(&mut child.walk()) {
							if fn_child.kind() == "function_declarator" {
								for decl_child in fn_child.children(&mut fn_child.walk()) {
									if decl_child.kind() == "identifier" {
										if let Ok(name) = decl_child.utf8_text(contents.as_bytes())
										{
											if !symbols.contains(&name.to_string()) {
												symbols.push(name.to_string());
											}
										}
										break;
									}
								}
								break;
							}
						}
					}
					"enum_specifier" => {
						// Extract enum constant names
						for enum_child in child.children(&mut child.walk()) {
							if enum_child.kind() == "enumerator_list" {
								for enum_list_child in enum_child.children(&mut enum_child.walk()) {
									if enum_list_child.kind() == "enumerator" {
										for enumerator_child in
											enum_list_child.children(&mut enum_list_child.walk())
										{
											if enumerator_child.kind() == "identifier" {
												if let Ok(name) =
													enumerator_child.utf8_text(contents.as_bytes())
												{
													if !symbols.contains(&name.to_string()) {
														symbols.push(name.to_string());
													}
												}
												break;
											}
										}
									}
								}
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

	// C++ has #include statements for imports

	// Helper function to parse C++ include statements
	fn parse_cpp_include(include_text: &str) -> Option<String> {
		let trimmed = include_text.trim();

		// Handle #include <header.h> or #include "header.h"
		if trimmed.starts_with("#include") {
			let include_part = trimmed.strip_prefix("#include").unwrap().trim(); // Remove "#include"

			// Handle <header.h>
			if include_part.starts_with('<') && include_part.ends_with('>') {
				return Some(include_part[1..include_part.len() - 1].to_string());
			}

			// Handle "header.h"
			if include_part.starts_with('"') && include_part.ends_with('"') {
				return Some(include_part[1..include_part.len() - 1].to_string());
			}
		}

		None
	}
}

impl Cpp {
	/// Extract function name from function definition or declaration
	fn extract_function_name(&self, node: Node, contents: &str) -> Option<String> {
		for child in node.children(&mut node.walk()) {
			if child.kind() == "function_declarator" {
				for decl_child in child.children(&mut child.walk()) {
					if decl_child.kind() == "identifier" {
						if let Ok(name) = decl_child.utf8_text(contents.as_bytes()) {
							return Some(name.to_string());
						}
					}
				}
			}
		}
		None
	}

	/// Extract variable names from declaration nodes
	fn extract_variable_names(&self, node: Node, contents: &str) -> Option<Vec<String>> {
		let mut var_names = Vec::new();

		for child in node.children(&mut node.walk()) {
			if child.kind() == "init_declarator" || child.kind() == "declarator" {
				for decl_child in child.children(&mut child.walk()) {
					if decl_child.kind() == "identifier" {
						if let Ok(name) = decl_child.utf8_text(contents.as_bytes()) {
							var_names.push(name.to_string());
						}
					}
				}
			}
		}

		if var_names.is_empty() {
			None
		} else {
			Some(var_names)
		}
	}

	/// Extract type name from class, struct, or enum declarations
	fn extract_type_name(&self, node: Node, contents: &str) -> Option<String> {
		for child in node.children(&mut node.walk()) {
			if child.kind() == "type_identifier" || child.kind() == "identifier" {
				if let Ok(name) = child.utf8_text(contents.as_bytes()) {
					return Some(name.to_string());
				}
			}
		}
		None
	}

	/// Extract namespace name from namespace definition
	fn extract_namespace_name(&self, node: Node, contents: &str) -> Option<String> {
		for child in node.children(&mut node.walk()) {
			if child.kind() == "identifier" || child.kind() == "namespace_identifier" {
				if let Ok(name) = child.utf8_text(contents.as_bytes()) {
					return Some(name.to_string());
				}
			}
		}
		None
	}

	/// Extract template name from template declarations
	fn extract_template_name(&self, node: Node, contents: &str) -> Option<String> {
		match node.kind() {
			"function_definition" => self.extract_function_name(node, contents),
			"class_specifier" | "struct_specifier" => self.extract_type_name(node, contents),
			_ => None,
		}
	}

	/// Extract macro name from preprocessor define
	fn extract_macro_name(&self, node: Node, contents: &str) -> Option<String> {
		// Look for the identifier after #define
		for child in node.children(&mut node.walk()) {
			if child.kind() == "identifier" {
				if let Ok(name) = child.utf8_text(contents.as_bytes()) {
					return Some(name.to_string());
				}
			}
		}
		None
	}

	/// Extract typedef name from type definition
	fn extract_typedef_name(&self, node: Node, contents: &str) -> Option<String> {
		// Look for the new type name in typedef
		for child in node.children(&mut node.walk()) {
			if child.kind() == "type_identifier" {
				if let Ok(name) = child.utf8_text(contents.as_bytes()) {
					return Some(name.to_string());
				}
			}
		}
		None
	}

	/// Resolve local includes relative to source file
	fn resolve_local_include(
		&self,
		header_name: &str,
		source_file: &str,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		let source_path = std::path::Path::new(source_file);
		let source_dir = source_path.parent()?;
		let header_path = source_dir.join(header_name);

		// Try to normalize the path to handle .. components
		let normalized_path =
			crate::utils::path::PathNormalizer::normalize_path(&header_path.to_string_lossy());

		registry.find_exact_file(&normalized_path)
	}
}
