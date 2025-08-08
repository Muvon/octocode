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

//! Lua language implementation for the indexer

use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct Lua {}

impl Language for Lua {
	fn name(&self) -> &'static str {
		"lua"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_lua::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec![
			"function_declaration",
			"function_definition",
			"local_function",
			// Only meaningful assignments - not every single assignment
			// Individual fields and simple assignments will be filtered out
			// Module system - only module-level returns are meaningful
		]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"function_declaration" | "function_definition" | "local_function" => {
				// Extract function name
				for child in node.children(&mut node.walk()) {
					if child.kind() == "identifier" {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							symbols.push(name.to_string());
						}
						break;
					}
				}

				// Extract local variables and nested functions within the function body
				for child in node.children(&mut node.walk()) {
					if child.kind() == "block" {
						self.extract_lua_block_symbols(child, contents, &mut symbols);
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
		if kind == "identifier" {
			if let Ok(text) = node.utf8_text(contents.as_bytes()) {
				let t = text.trim();
				if !t.is_empty() && self.is_valid_lua_identifier(t) {
					symbols.push(t.to_string());
				}
			}
		}

		// Recursively extract from children
		for child in node.children(&mut node.walk()) {
			self.extract_identifiers(child, contents, symbols);
		}
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		let mut imports = Vec::new();
		let mut exports = Vec::new();

		match node.kind() {
			"function_call" => {
				// Check for require() calls
				if let Some(function_name) = self.get_function_call_name(node, contents) {
					if function_name == "require" {
						if let Some(module_name) = self.get_require_module_name(node, contents) {
							imports.push(module_name);
						}
					}
				}
			}
			"return_statement" => {
				// Check for module exports (return statement at module level)
				if self.is_module_level_return(node) {
					// Extract what's being returned as exports
					for child in node.children(&mut node.walk()) {
						if child.kind() == "expression_list" {
							self.extract_lua_exports_from_expression_list(
								child,
								contents,
								&mut exports,
							);
						} else if child.kind() == "table_constructor" {
							self.extract_lua_exports_from_table(child, contents, &mut exports);
						} else if child.kind() == "identifier" {
							if let Ok(name) = child.utf8_text(contents.as_bytes()) {
								exports.push(name.to_string());
							}
						}
					}
				}
			}
			_ => {}
		}

		// Recursively check children
		for child in node.children(&mut node.walk()) {
			let (child_imports, child_exports) = self.extract_imports_exports(child, contents);
			imports.extend(child_imports);
			exports.extend(child_exports);
		}

		(imports, exports)
	}

	fn are_node_types_equivalent(&self, type1: &str, type2: &str) -> bool {
		match (type1, type2) {
			// Function declarations are equivalent
			("function_declaration", "function_definition") => true,
			("function_definition", "function_declaration") => true,
			("function_declaration", "local_function") => true,
			("local_function", "function_declaration") => true,
			("function_definition", "local_function") => true,
			("local_function", "function_definition") => true,

			// Default: exact match
			_ => type1 == type2,
		}
	}

	fn get_node_type_description(&self, node_type: &str) -> &'static str {
		match node_type {
			"function_declaration" => "Function Declaration",
			"function_definition" => "Function Definition",
			"local_function" => "Local Function",
			"assignment_statement" => "Variable Assignment",
			"local_declaration" => "Local Variable Declaration",
			"table_constructor" => "Table Constructor",
			"field" => "Table Field",
			"return_statement" => "Return Statement",
			"function_call" => "Function Call",
			"identifier" => "Identifier",
			"block" => "Code Block",
			"variable_list" => "Variable List",
			"expression_list" => "Expression List",
			_ => "Unknown Node Type",
		}
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["lua"]
	}

	fn resolve_import(
		&self,
		import_path: &str,
		source_file: &str,
		all_files: &[String],
	) -> Option<String> {
		use crate::indexer::languages::resolution_utils::FileRegistry;

		let registry = FileRegistry::new(all_files);
		let base_dir = std::path::Path::new(source_file).parent()?;

		// Handle relative imports (e.g., require("./module") or require("../utils"))
		if import_path.starts_with("./") || import_path.starts_with("../") {
			let relative_path = base_dir.join(&import_path[2..]);
			let lua_file = format!("{}.lua", relative_path.to_string_lossy());
			if let Some(found) = registry.find_exact_file(&lua_file) {
				return Some(found);
			}
		}

		// Handle absolute module imports (e.g., require("json"), require("utils.helpers"))
		let module_parts: Vec<&str> = import_path.split('.').collect();
		let mut current_path = base_dir.to_path_buf();

		for (i, part) in module_parts.iter().enumerate() {
			if i == module_parts.len() - 1 {
				// Last part - look for .lua file or init.lua in directory
				let file_path = current_path.join(format!("{}.lua", part));
				if let Some(found) = registry.find_exact_file(&file_path.to_string_lossy()) {
					return Some(found);
				}

				let init_path = current_path.join(part).join("init.lua");
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

impl Lua {
	/// Extract symbols from a Lua block (function body, control structure body, etc.)
	fn extract_lua_block_symbols(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		for child in node.children(&mut node.walk()) {
			match child.kind() {
				"local_declaration" | "assignment_statement" => {
					// Extract variable names from declarations/assignments
					for grandchild in child.children(&mut child.walk()) {
						if grandchild.kind() == "variable_list" {
							self.extract_lua_variable_list(grandchild, contents, symbols);
						}
					}
				}
				"function_declaration" | "function_definition" | "local_function" => {
					// Extract nested function names
					for grandchild in child.children(&mut child.walk()) {
						if grandchild.kind() == "identifier" {
							if let Ok(name) = grandchild.utf8_text(contents.as_bytes()) {
								symbols.push(name.to_string());
							}
							break;
						}
					}
				}
				_ => {
					// Recursively process other nodes
					self.extract_lua_block_symbols(child, contents, symbols);
				}
			}
		}
	}

	/// Extract variable names from a variable list
	fn extract_lua_variable_list(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		for child in node.children(&mut node.walk()) {
			if child.kind() == "identifier" {
				if let Ok(name) = child.utf8_text(contents.as_bytes()) {
					if self.is_valid_lua_identifier(name) {
						symbols.push(name.to_string());
					}
				}
			}
		}
	}

	/// Check if a string is a valid Lua identifier
	fn is_valid_lua_identifier(&self, text: &str) -> bool {
		if text.is_empty() {
			return false;
		}

		// Lua keywords that should be excluded
		const LUA_KEYWORDS: &[&str] = &[
			"and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in",
			"local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
		];

		if LUA_KEYWORDS.contains(&text) {
			return false;
		}

		// Must start with letter or underscore
		let first_char = text.chars().next().unwrap();
		if !first_char.is_ascii_alphabetic() && first_char != '_' {
			return false;
		}

		// Rest must be alphanumeric or underscore
		text.chars()
			.skip(1)
			.all(|c| c.is_ascii_alphanumeric() || c == '_')
	}

	/// Get the function name from a function call node
	fn get_function_call_name(&self, node: Node, contents: &str) -> Option<String> {
		for child in node.children(&mut node.walk()) {
			if child.kind() == "identifier" {
				if let Ok(name) = child.utf8_text(contents.as_bytes()) {
					return Some(name.to_string());
				}
			}
		}
		None
	}

	/// Extract module name from require() call
	fn get_require_module_name(&self, node: Node, contents: &str) -> Option<String> {
		for child in node.children(&mut node.walk()) {
			if child.kind() == "arguments" {
				for grandchild in child.children(&mut child.walk()) {
					if grandchild.kind() == "string" {
						if let Ok(module_name) = grandchild.utf8_text(contents.as_bytes()) {
							// Remove quotes from string
							let cleaned = module_name.trim_matches('"').trim_matches('\'');
							return Some(cleaned.to_string());
						}
					}
				}
			}
		}
		None
	}

	/// Check if a return statement is at module level (not inside a function)
	fn is_module_level_return(&self, node: Node) -> bool {
		let mut current = node.parent();
		while let Some(parent) = current {
			match parent.kind() {
				"function_declaration" | "function_definition" | "local_function" => {
					return false; // Inside a function
				}
				"chunk" => {
					return true; // At module level
				}
				_ => {
					current = parent.parent();
				}
			}
		}
		true // Default to module level if we can't determine
	}

	/// Extract exports from expression list in return statement
	fn extract_lua_exports_from_expression_list(
		&self,
		node: Node,
		contents: &str,
		exports: &mut Vec<String>,
	) {
		for child in node.children(&mut node.walk()) {
			match child.kind() {
				"identifier" => {
					if let Ok(name) = child.utf8_text(contents.as_bytes()) {
						exports.push(name.to_string());
					}
				}
				"table_constructor" => {
					self.extract_lua_exports_from_table(child, contents, exports);
				}
				_ => {}
			}
		}
	}

	/// Extract exports from table constructor in return statement
	fn extract_lua_exports_from_table(
		&self,
		node: Node,
		contents: &str,
		exports: &mut Vec<String>,
	) {
		for child in node.children(&mut node.walk()) {
			if child.kind() == "field" {
				for grandchild in child.children(&mut child.walk()) {
					if grandchild.kind() == "identifier" {
						if let Ok(name) = grandchild.utf8_text(contents.as_bytes()) {
							if self.is_valid_lua_identifier(name) {
								exports.push(name.to_string());
							}
						}
					}
				}
			}
		}
	}
}
