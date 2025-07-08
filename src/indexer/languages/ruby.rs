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

//! Ruby language implementation for the indexer

use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct Ruby {}

impl Language for Ruby {
	fn name(&self) -> &'static str {
		"ruby"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_ruby::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec!["method", "class", "module", "call"] // call for require/load statements
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"method" | "class" | "module" => {
				// Find method, class, or module name
				for child in node.children(&mut node.walk()) {
					if child.kind() == "identifier" || child.kind() == "constant" {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							symbols.push(name.to_string());
						}
						break;
					}
				}

				// For methods, extract local variables
				if node.kind() == "method" {
					for child in node.children(&mut node.walk()) {
						if child.kind() == "body_statement" || child.kind() == "do_block" {
							self.extract_ruby_variables(child, contents, &mut symbols);
							break;
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
		// Check if this is a valid identifier or constant
		if kind == "identifier" || kind == "constant" {
			if let Ok(text) = node.utf8_text(contents.as_bytes()) {
				let t = text.trim();
				if !t.is_empty() && !symbols.contains(&t.to_string()) && !t.starts_with('@') {
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

		// Ruby-specific semantic groups
		let semantic_groups = [
			// Methods and functions
			&["method"] as &[&str],
			// Classes and modules
			&["class", "module"],
			// Constants and variables
			&["assignment", "multiple_assignment"],
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
			"method" => "method declarations",
			"class" => "class declarations",
			"module" => "module declarations",
			"assignment" | "multiple_assignment" => "variable assignments",
			_ => "declarations",
		}
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		let mut imports = Vec::new();
		let exports = Vec::new(); // Ruby doesn't have explicit exports like ES6

		// Look for method calls that might be require or load
		if node.kind() == "call" {
			if let Ok(call_text) = node.utf8_text(contents.as_bytes()) {
				if let Some(required_file) = Self::parse_ruby_require(call_text) {
					imports.push(required_file);
				}
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

		if import_path.starts_with("relative:") {
			// require_relative import
			let relative_path = import_path.strip_prefix("relative:")?;
			self.resolve_relative_require(relative_path, source_file, &registry)
		} else if import_path.starts_with("./") || import_path.starts_with("../") {
			// Relative require
			self.resolve_relative_require(import_path, source_file, &registry)
		} else {
			// Absolute require
			self.resolve_absolute_require(import_path, &registry)
		}
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["rb"]
	}
}

impl Ruby {
	/// Extract local variable assignments in Ruby
	#[allow(clippy::only_used_in_recursion)]
	fn extract_ruby_variables(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				let child = cursor.node();

				if child.kind() == "assignment" {
					// Extract variable name from assignment
					for assign_child in child.children(&mut child.walk()) {
						if assign_child.kind() == "identifier" {
							if let Ok(name) = assign_child.utf8_text(contents.as_bytes()) {
								// Skip instance/class variables (starting with @ or @@)
								if !name.starts_with('@') && !symbols.contains(&name.to_string()) {
									symbols.push(name.to_string());
								}
							}
							break; // Only take the left side (the variable name)
						}
					}
				} else {
					// Recursive search in nested structures
					self.extract_ruby_variables(child, contents, symbols);
				}

				if !cursor.goto_next_sibling() {
					break;
				}
			}
		}
	}

	// Ruby has require and load statements for imports

	// Helper function to parse Ruby require/load statements
	fn parse_ruby_require(call_text: &str) -> Option<String> {
		let trimmed = call_text.trim();

		// Handle require "file" or require 'file'
		if trimmed.starts_with("require ") {
			let require_part = trimmed.strip_prefix("require ").unwrap().trim(); // Remove "require "
			if let Some(filename) = Self::extract_ruby_string_literal(require_part) {
				return Some(filename);
			}
		}

		// Handle require_relative "file" or require_relative 'file'
		if trimmed.starts_with("require_relative ") {
			let require_part = trimmed.strip_prefix("require_relative ").unwrap().trim(); // Remove "require_relative "
			if let Some(filename) = Self::extract_ruby_string_literal(require_part) {
				return Some(format!("relative:{}", filename)); // Mark as relative import
			}
		}

		// Handle load "file" or load 'file'
		if trimmed.starts_with("load ") {
			let load_part = trimmed.strip_prefix("load ").unwrap().trim(); // Remove "load "
			if let Some(filename) = Self::extract_ruby_string_literal(load_part) {
				return Some(filename);
			}
		}

		None
	}

	// Helper to extract Ruby string literals
	fn extract_ruby_string_literal(text: &str) -> Option<String> {
		let text = text.trim();
		if (text.starts_with('"') && text.ends_with('"'))
			|| (text.starts_with('\'') && text.ends_with('\''))
		{
			Some(text[1..text.len() - 1].to_string())
		} else {
			None
		}
	}
}

impl Ruby {
	/// Resolve relative require statements
	fn resolve_relative_require(
		&self,
		import_path: &str,
		source_file: &str,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		use super::resolution_utils::resolve_relative_path;

		let relative_path = resolve_relative_path(source_file, import_path)?;
		registry.find_file_with_extensions(&relative_path, &self.get_file_extensions())
	}

	/// Resolve absolute require statements
	fn resolve_absolute_require(
		&self,
		import_path: &str,
		registry: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		let path = std::path::Path::new(import_path);

		// Try direct path first
		if let Some(result) = registry.find_file_with_extensions(path, &self.get_file_extensions())
		{
			return Some(result);
		}

		// Try common Ruby load paths
		let load_paths = ["lib", "app", "config"];
		for load_path in &load_paths {
			let full_path = std::path::Path::new(load_path).join(path);
			if let Some(result) =
				registry.find_file_with_extensions(&full_path, &self.get_file_extensions())
			{
				return Some(result);
			}
		}

		// Try vendor gems with deeper search
		for file in registry.get_all_files() {
			if file.contains("vendor/gems") && file.ends_with(&format!("{}.rb", import_path)) {
				return Some(file.clone());
			}
		}

		None
	}
}
