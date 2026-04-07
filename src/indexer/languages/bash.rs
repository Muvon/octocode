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

//! Bash language implementation for the indexer

use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct Bash {}

impl Language for Bash {
	fn name(&self) -> &'static str {
		"bash"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_bash::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec!["function_definition", "command"] // command for source/. statements
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"function_definition" => {
				// Find the function name
				for child in node.children(&mut node.walk()) {
					if child.kind() == "name" {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							symbols.push(name.to_string());
						}
						break;
					}
				}

				// Extract variables from function body
				self.extract_bash_variables(node, contents, &mut symbols);
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
		// Check if this is a variable name or command name
		if kind == "variable_name" || kind == "command_name" {
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

		// Bash-specific semantic groups
		let semantic_groups = [
			// Functions
			&["function_definition"] as &[&str],
			// Variable assignments and declarations
			&["variable_assignment"],
			// Commands and aliases
			&["command", "simple_command"],
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
			"variable_assignment" => "variable assignments",
			"command" | "simple_command" => "command declarations",
			_ => "declarations",
		}
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		let mut imports = Vec::new();
		let exports = Vec::new(); // Bash doesn't have explicit exports, only environment variables

		if node.kind() == "function_definition" {
			// Look for source or . commands in function body
			Self::extract_bash_sources_from_node(node, contents, &mut imports);
		}

		(imports, exports)
	}

	fn extract_function_calls(&self, node: Node, contents: &str) -> Vec<String> {
		if node.kind() == "command" {
			// First child is the command name — skip "source" and "." (imports)
			if let Some(cmd_node) = node.child(0) {
				if let Ok(text) = cmd_node.utf8_text(contents.as_bytes()) {
					let name = text.trim();
					if name == "source" || name == "." {
						return Vec::new();
					}
					return vec![name.to_string()];
				}
			}
		}
		Vec::new()
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
			// Relative source
			self.resolve_relative_source(import_path, source_file, &registry)
		} else {
			// Absolute source
			registry.find_exact_file(import_path)
		}
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["sh", "bash"]
	}
}

impl Bash {
	/// Extract variable assignments in bash
	fn extract_bash_variables(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				let child = cursor.node();

				if child.kind() == "variable_assignment" {
					for var_child in child.children(&mut child.walk()) {
						if var_child.kind() == "variable_name" {
							if let Ok(var_name) = var_child.utf8_text(contents.as_bytes()) {
								symbols.push(var_name.to_string());
							}
							break;
						}
					}
				}

				if !cursor.goto_next_sibling() {
					break;
				}
			}
		}
	}

	// Helper function to extract source commands from bash
	fn extract_bash_sources_from_node(node: Node, contents: &str, imports: &mut Vec<String>) {
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				let child = cursor.node();

				// Look for commands that might be source or .
				if child.kind() == "command" {
					if let Ok(command_text) = child.utf8_text(contents.as_bytes()) {
						if let Some(source_file) = Self::parse_bash_source(command_text) {
							imports.push(source_file);
						}
					}
				}

				// Recursively check child nodes
				Self::extract_bash_sources_from_node(child, contents, imports);

				if !cursor.goto_next_sibling() {
					break;
				}
			}
		}
	}

	// Helper function to parse bash source commands
	fn parse_bash_source(command_text: &str) -> Option<String> {
		let trimmed = command_text.trim();

		// Handle "source file.sh" or ". file.sh"
		if let Some(stripped) = trimmed.strip_prefix("source ") {
			if let Some(filename) = stripped.split_whitespace().next() {
				return Some(filename.trim_matches('"').trim_matches('\'').to_string());
			}
		} else if let Some(stripped) = trimmed.strip_prefix(". ") {
			if let Some(filename) = stripped.split_whitespace().next() {
				return Some(filename.trim_matches('"').trim_matches('\'').to_string());
			}
		}

		None
	}

	/// Resolve relative source statements
	fn resolve_relative_source(
		&self,
		import_path: &str,
		source_file: &str,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		use super::resolution_utils::resolve_relative_path;

		let relative_path = resolve_relative_path(source_file, import_path)?;
		registry.find_exact_file(&relative_path.to_string_lossy())
	}
}
