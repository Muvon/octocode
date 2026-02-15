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

//! Python language implementation for the indexer

use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct Python {}

impl Language for Python {
	fn name(&self) -> &'static str {
		"python"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_python::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec![
			"function_definition",
			"class_definition",
			"import_statement",
			"import_from_statement",
			// Removed: "class_definition" - too large, not semantic
			// Individual methods inside classes will be extracted separately if needed
		]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"function_definition" => {
				// Extract function name
				if let Some(name) = super::extract_symbol_by_kind(node, contents, "identifier") {
					symbols.push(name);
				}

				// Extract variable assignments within the function
				for child in node.children(&mut node.walk()) {
					if child.kind() == "block" {
						self.extract_python_variables(child, contents, &mut symbols);
						break;
					}
				}
			}
			_ => self.extract_identifiers(node, contents, &mut symbols),
		}

		super::deduplicate_symbols(&mut symbols);
		symbols
	}

	fn extract_identifiers(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		super::extract_identifiers_default(node, contents, symbols, |kind, text| {
			// Include identifiers but exclude private ones (starting with _)
			kind == "identifier" && !text.starts_with('_')
		});
	}

	fn are_node_types_equivalent(&self, type1: &str, type2: &str) -> bool {
		// Python-specific semantic groups
		let semantic_groups = [
			// Functions and methods
			&["function_definition"] as &[&str],
			// Classes
			&["class_definition"],
			// Import statements
			&["import_statement", "import_from_statement"],
		];

		super::check_semantic_groups(type1, type2, &semantic_groups)
	}

	fn get_node_type_description(&self, node_type: &str) -> &'static str {
		match node_type {
			"function_definition" => "function declarations",
			"class_definition" => "class declarations",
			"import_statement" | "import_from_statement" => "import statements",
			_ => "declarations",
		}
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		let mut imports = Vec::new();
		let mut exports = Vec::new();

		match node.kind() {
			"import_statement" => {
				// Handle: import module
				// Handle: import module as alias
				if let Ok(import_text) = node.utf8_text(contents.as_bytes()) {
					if let Some(imported_paths) =
						parse_python_import_statement_full_path(import_text)
					{
						imports.extend(imported_paths);
					}
				}
			}
			"import_from_statement" => {
				// Handle: from module import item1, item2
				// Handle: from module import *
				if let Ok(import_text) = node.utf8_text(contents.as_bytes()) {
					if let Some(imported_paths) =
						parse_python_from_import_statement_full_path(import_text)
					{
						imports.extend(imported_paths);
					}
				}
			}
			"function_definition" | "class_definition" => {
				// In Python, everything at module level is "exported" by default
				// Check if this is at module level (parent is module)
				if let Some(parent) = node.parent() {
					if parent.kind() == "module" {
						// Extract the name
						for child in node.children(&mut node.walk()) {
							if child.kind() == "identifier" {
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
		use super::resolution_utils::FileRegistry;

		let registry = FileRegistry::new(all_files);

		if import_path.starts_with('.') {
			// Relative import: .module or ..module
			self.resolve_relative_import(import_path, source_file, &registry)
		} else {
			// Absolute import - look from project root or in same package
			self.resolve_absolute_import(import_path, source_file, &registry)
		}
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["py"]
	}
}

impl Python {
	/// Extract variable assignments in Python blocks
	#[allow(clippy::only_used_in_recursion)]
	fn extract_python_variables(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				let child = cursor.node();

				// Handle different types of assignments
				match child.kind() {
					"assignment" => {
						// For regular assignments like 'x = 10'
						let mut child_cursor = child.walk();
						if child_cursor.goto_first_child() {
							// First child is the target
							let target = child_cursor.node();
							if target.kind() == "identifier" {
								if let Ok(name) = target.utf8_text(contents.as_bytes()) {
									if !name.starts_with("_")
										&& !symbols.contains(&name.to_string())
									{
										symbols.push(name.to_string());
									}
								}
							}
						}
					}
					"expression_statement" => {
						// Check for augmented assignments like 'x += 1'
						for expr_child in child.children(&mut child.walk()) {
							if expr_child.kind() == "augmented_assignment" {
								let mut aug_cursor = expr_child.walk();
								if aug_cursor.goto_first_child() {
									// First child is target
									let target = aug_cursor.node();
									if target.kind() == "identifier" {
										if let Ok(name) = target.utf8_text(contents.as_bytes()) {
											if !name.starts_with("_")
												&& !symbols.contains(&name.to_string())
											{
												symbols.push(name.to_string());
											}
										}
									}
								}
							}
						}
					}
					"for_statement" | "while_statement" | "if_statement" | "try_statement"
					| "with_statement" => {
						// Recursive search in nested blocks
						for stmt_child in child.children(&mut child.walk()) {
							if stmt_child.kind() == "block" {
								self.extract_python_variables(stmt_child, contents, symbols);
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
}

// Helper functions for Python import/export parsing

// Extract full import paths for GraphRAG (not just imported item names)
fn parse_python_import_statement_full_path(import_text: &str) -> Option<Vec<String>> {
	let mut imports = Vec::new();
	let cleaned = import_text.trim();

	// Handle: import module
	// Handle: import module as alias
	if let Some(rest) = cleaned.strip_prefix("import ") {
		// Extract the module name (before any 'as' clause)
		for item in rest.split(',') {
			let item = item.trim();
			let module_name = if let Some(as_pos) = item.find(" as ") {
				&item[..as_pos]
			} else {
				item
			};
			if !module_name.is_empty() {
				imports.push(module_name.to_string());
			}
		}
		return Some(imports);
	}

	None
}

fn parse_python_from_import_statement_full_path(import_text: &str) -> Option<Vec<String>> {
	let mut imports = Vec::new();
	let cleaned = import_text.trim();

	// Handle: from module import item
	// Extract the module name from 'from' clause
	if cleaned.starts_with("from ") && cleaned.contains(" import ") {
		if let Some(import_pos) = cleaned.find(" import ") {
			let module_part = &cleaned[5..import_pos]; // Skip "from " and take until " import"
			if !module_part.is_empty() {
				imports.push(module_part.to_string());
			}
		}
		return Some(imports);
	}

	None
}

impl Python {
	/// Resolve relative imports like .module or ..module
	fn resolve_relative_import(
		&self,
		import_path: &str,
		source_file: &str,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		let dots = import_path.chars().take_while(|&c| c == '.').count();
		let module_name = &import_path[dots..];

		let source_path = std::path::Path::new(source_file);
		let mut target_dir = source_path.parent()?;

		// Move up directories based on number of dots
		for _ in 1..dots {
			target_dir = target_dir.parent()?;
		}

		self.find_python_module(module_name, target_dir, registry)
	}

	/// Resolve absolute imports
	fn resolve_absolute_import(
		&self,
		import_path: &str,
		source_file: &str,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		let source_path = std::path::Path::new(source_file);
		let source_dir = source_path.parent()?;

		// Try from same package first
		if let Some(result) = self.find_python_module(import_path, source_dir, registry) {
			return Some(result);
		}

		// Try from project root
		if let Some(project_root) = super::resolution_utils::find_project_root(source_file) {
			let root_path = std::path::Path::new(&project_root);
			self.find_python_module(import_path, root_path, registry)
		} else {
			None
		}
	}

	/// Find a Python module by name in a directory
	fn find_python_module(
		&self,
		module_name: &str,
		base_dir: &std::path::Path,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		let module_parts: Vec<&str> = module_name.split('.').collect();
		let mut current_path = base_dir.to_path_buf();

		for (i, part) in module_parts.iter().enumerate() {
			if i == module_parts.len() - 1 {
				// Last part - look for .py file or __init__.py in directory
				let file_path = current_path.join(format!("{}.py", part));
				if let Some(found) = registry.find_exact_file(&file_path.to_string_lossy()) {
					return Some(found);
				}

				let init_path = current_path.join(part).join("__init__.py");
				if let Some(found) = registry.find_exact_file(&init_path.to_string_lossy()) {
					return Some(found);
				}
			} else {
				current_path = current_path.join(part);
			}
		}

		None
	}
}
