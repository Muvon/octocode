// Copyright 2026 Muvon Un Limited
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

//! Swift language implementation for the indexer.
//!
//! The tree-sitter-swift grammar represents class/struct/enum/extension/actor
//! declarations all as `class_declaration` nodes, distinguished by their
//! `declaration_kind` field ("class", "struct", "enum", "extension", "actor").
//! Protocol declarations use the separate `protocol_declaration` node.

use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct Swift {}

impl Language for Swift {
	fn name(&self) -> &'static str {
		"swift"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_swift::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec![
			// Top-level and nested declarations
			"function_declaration",
			"class_declaration", // covers class / struct / enum / extension / actor
			"protocol_declaration",
			"init_declaration",
			"property_declaration",
			"typealias_declaration",
			"subscript_declaration",
			"import_declaration",
		]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"function_declaration" => {
				// name field holds simple_identifier nodes
				self.extract_names_from_field(node, contents, "name", &mut symbols);

				// Surface enclosing type name for method lookup (e.g. "Foo.bar")
				if let Some(owner) = super::find_enclosing_container_name(
					node,
					contents,
					&["class_declaration", "protocol_declaration"],
					&["type_identifier", "simple_identifier"],
				) {
					symbols.push(owner);
				}
			}
			"class_declaration" => {
				// name field: type_identifier (for simple names) or user_type
				self.extract_type_name(node, contents, &mut symbols);
			}
			"protocol_declaration" => {
				if let Some(name_node) = node.child_by_field_name("name") {
					if let Ok(name) = name_node.utf8_text(contents.as_bytes()) {
						symbols.push(name.to_string());
					}
				}
			}
			"init_declaration" => {
				// init declarations carry the enclosing type name as their primary symbol
				if let Some(owner) = super::find_enclosing_container_name(
					node,
					contents,
					&["class_declaration", "protocol_declaration"],
					&["type_identifier", "simple_identifier"],
				) {
					symbols.push(owner);
				}
				symbols.push("init".to_string());
			}
			"property_declaration" => {
				// name field is a `pattern` containing simple_identifier children
				if let Some(name_node) = node.child_by_field_name("name") {
					self.collect_simple_identifiers(name_node, contents, &mut symbols);
				}

				// Surface enclosing type so "Foo.bar" resolves
				if let Some(owner) = super::find_enclosing_container_name(
					node,
					contents,
					&["class_declaration", "protocol_declaration"],
					&["type_identifier", "simple_identifier"],
				) {
					symbols.push(owner);
				}
			}
			"typealias_declaration" => {
				self.extract_names_from_field(node, contents, "name", &mut symbols);
			}
			"subscript_declaration" => {
				symbols.push("subscript".to_string());
				if let Some(owner) = super::find_enclosing_container_name(
					node,
					contents,
					&["class_declaration", "protocol_declaration"],
					&["type_identifier", "simple_identifier"],
				) {
					symbols.push(owner);
				}
			}
			"import_declaration" => {
				// Collect identifier children as the module path
				let mut cursor = node.walk();
				for child in node.children(&mut cursor) {
					if child.kind() == "identifier" {
						if let Ok(text) = child.utf8_text(contents.as_bytes()) {
							symbols.push(text.to_string());
						}
					}
				}
			}
			_ => self.extract_identifiers(node, contents, &mut symbols),
		}

		super::deduplicate_symbols(&mut symbols);
		symbols
	}

	fn extract_identifiers(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		super::extract_identifiers_default(node, contents, symbols, |kind, _text| {
			kind == "simple_identifier" || kind == "type_identifier" || kind == "identifier"
		});
	}

	fn are_node_types_equivalent(&self, type1: &str, type2: &str) -> bool {
		if type1 == type2 {
			return true;
		}

		let semantic_groups: &[&[&str]] = &[
			// All named-type declarations map to the same node kind in this grammar
			&["class_declaration"],
			// Functions and initializers
			&["function_declaration", "init_declaration"],
			// Protocol declarations
			&["protocol_declaration"],
			// Property and subscript
			&["property_declaration", "subscript_declaration"],
			// Type aliases
			&["typealias_declaration"],
			// Imports
			&["import_declaration"],
		];

		super::check_semantic_groups(type1, type2, semantic_groups)
	}

	fn get_node_type_description(&self, node_type: &str) -> &'static str {
		match node_type {
			"function_declaration" => "function declarations",
			"class_declaration" => "type declarations (class/struct/enum/extension/actor)",
			"protocol_declaration" => "protocol declarations",
			"init_declaration" => "initializer declarations",
			"property_declaration" => "property declarations",
			"typealias_declaration" => "type alias declarations",
			"subscript_declaration" => "subscript declarations",
			"import_declaration" => "import statements",
			_ => "declarations",
		}
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		let mut imports = Vec::new();
		let mut exports = Vec::new();

		match node.kind() {
			"import_declaration" => {
				// Collect the full module path (e.g. "Foundation", "UIKit")
				let mut cursor = node.walk();
				let mut parts = Vec::new();
				for child in node.children(&mut cursor) {
					if child.kind() == "identifier" {
						if let Ok(text) = child.utf8_text(contents.as_bytes()) {
							parts.push(text.to_string());
						}
					}
				}
				if !parts.is_empty() {
					imports.push(parts.join("."));
				}
			}
			// Swift uses access modifiers (public/open) for exports — extract those
			"function_declaration"
			| "class_declaration"
			| "protocol_declaration"
			| "property_declaration"
			| "typealias_declaration"
			| "subscript_declaration"
			| "init_declaration"
				if is_swift_public(node, contents) =>
			{
				let mut name_symbols = Vec::new();
				match node.kind() {
					"function_declaration" => {
						self.extract_names_from_field(node, contents, "name", &mut name_symbols);
					}
					"class_declaration" => {
						self.extract_type_name(node, contents, &mut name_symbols);
					}
					"protocol_declaration" => {
						if let Some(name_node) = node.child_by_field_name("name") {
							if let Ok(name) = name_node.utf8_text(contents.as_bytes()) {
								name_symbols.push(name.to_string());
							}
						}
					}
					"property_declaration" => {
						if let Some(name_node) = node.child_by_field_name("name") {
							self.collect_simple_identifiers(name_node, contents, &mut name_symbols);
						}
					}
					"typealias_declaration" => {
						self.extract_names_from_field(node, contents, "name", &mut name_symbols);
					}
					_ => {}
				}
				exports.extend(name_symbols);
			}
			_ => {}
		}

		(imports, exports)
	}

	fn extract_function_calls(&self, node: Node, contents: &str) -> Vec<String> {
		if node.kind() == "call_expression" {
			// The first child of call_expression is the callee expression
			if let Some(callee) = node.child(0) {
				if let Ok(text) = callee.utf8_text(contents.as_bytes()) {
					return super::extract_callee_identifiers(text);
				}
			}
		}
		Vec::new()
	}

	fn extract_type_relations(
		&self,
		node: Node,
		contents: &str,
	) -> Vec<(super::TypeRelationKind, String)> {
		let mut out = Vec::new();

		match node.kind() {
			// class Foo: Bar, Baz — Bar could be a superclass (Extends) or protocol (Implements).
			// We conservatively emit Implements for all; the first one _may_ be Extends for classes,
			// but without semantic analysis we can't distinguish struct conformances from class
			// inheritance. Consistent behaviour matches what other languages do (Go embedding → Extends).
			"class_declaration" => {
				let mut cursor = node.walk();
				for child in node.children(&mut cursor) {
					if child.kind() == "inheritance_specifier" {
						if let Some(from_node) = child.child_by_field_name("inherits_from") {
							if let Some(name) = extract_swift_type_name(from_node, contents) {
								// First specifier for a class could be the superclass.
								// We emit Implements for all — consistent, simple, correct enough.
								out.push((super::TypeRelationKind::Implements, name));
							}
						}
					}
				}
			}
			// protocol Foo: Bar — protocol inheritance
			"protocol_declaration" => {
				let mut cursor = node.walk();
				for child in node.children(&mut cursor) {
					if child.kind() == "inheritance_specifier" {
						if let Some(from_node) = child.child_by_field_name("inherits_from") {
							if let Some(name) = extract_swift_type_name(from_node, contents) {
								out.push((super::TypeRelationKind::Extends, name));
							}
						}
					}
				}
			}
			_ => {}
		}

		out
	}

	fn resolve_import(
		&self,
		import_path: &str,
		source_file: &str,
		all_files: &[String],
	) -> Option<String> {
		use super::resolution_utils::FileRegistry;

		let registry = FileRegistry::new(all_files);
		let swift_files = registry.get_files_with_extensions(&self.get_file_extensions());

		if import_path.starts_with("./") || import_path.starts_with("../") {
			// Relative import
			self.resolve_relative_import(import_path, source_file, &swift_files)
		} else {
			// Module-level import — look for a directory or file matching the module name
			self.resolve_module_import(import_path, &swift_files)
		}
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["swift"]
	}
}

// ── Private helpers ──────────────────────────────────────────────────────────

impl Swift {
	/// Extract all `simple_identifier` nodes from a named field of `node`.
	fn extract_names_from_field(
		&self,
		node: Node,
		contents: &str,
		field: &str,
		symbols: &mut Vec<String>,
	) {
		// Field "name" may have multiple types; collect all simple_identifier children.
		let mut cursor = node.walk();
		for child in node.children_by_field_name(field, &mut cursor) {
			match child.kind() {
				"simple_identifier" => {
					if let Ok(text) = child.utf8_text(contents.as_bytes()) {
						symbols.push(text.to_string());
					}
				}
				"type_identifier" => {
					if let Ok(text) = child.utf8_text(contents.as_bytes()) {
						symbols.push(text.to_string());
					}
				}
				"user_type" => {
					// user_type → type_identifier children
					self.collect_type_identifier(child, contents, symbols);
				}
				_ => {}
			}
		}
	}

	/// Extract the type name from a `class_declaration` node.
	fn extract_type_name(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		let mut cursor = node.walk();
		for child in node.children_by_field_name("name", &mut cursor) {
			match child.kind() {
				"type_identifier" => {
					if let Ok(text) = child.utf8_text(contents.as_bytes()) {
						symbols.push(text.to_string());
					}
				}
				"user_type" => {
					self.collect_type_identifier(child, contents, symbols);
				}
				_ => {}
			}
		}
	}

	/// Collect `type_identifier` text from a `user_type` node.
	fn collect_type_identifier(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			if child.kind() == "type_identifier" {
				if let Ok(text) = child.utf8_text(contents.as_bytes()) {
					symbols.push(text.to_string());
				}
			}
		}
	}

	/// Collect `simple_identifier` leaves from a `pattern` subtree.
	fn collect_simple_identifiers(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		if node.kind() == "simple_identifier" {
			if let Ok(text) = node.utf8_text(contents.as_bytes()) {
				symbols.push(text.to_string());
			}
			return;
		}
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			self.collect_simple_identifiers(child, contents, symbols);
		}
	}

	/// Resolve a relative path import.
	fn resolve_relative_import(
		&self,
		import_path: &str,
		source_file: &str,
		swift_files: &[String],
	) -> Option<String> {
		use super::resolution_utils::resolve_relative_path;
		let base_path = resolve_relative_path(source_file, import_path)?;
		let base_str = base_path.to_string_lossy();

		for file in swift_files {
			let norm = file.replace('\\', "/");
			// Match with or without .swift extension
			if norm == *base_str
				|| norm == format!("{}.swift", base_str)
				|| norm.trim_end_matches(".swift") == base_str.trim_end_matches(".swift")
			{
				return Some(file.clone());
			}
		}
		None
	}

	/// Resolve a Swift module/framework import by matching against file paths.
	/// e.g. `import Foundation` looks for a `Foundation` directory or `Foundation.swift`.
	fn resolve_module_import(&self, module_name: &str, swift_files: &[String]) -> Option<String> {
		let module_lower = module_name.to_lowercase();
		for file in swift_files {
			let norm = file.replace('\\', "/");
			let file_lower = norm.to_lowercase();
			// Match `Foundation/...` or `Foundation.swift`
			if file_lower.contains(&format!("/{}/", module_lower))
				|| file_lower.ends_with(&format!("/{}.swift", module_lower))
			{
				return Some(file.clone());
			}
		}
		None
	}
}

// ── Free helpers ─────────────────────────────────────────────────────────────

/// Returns true if the node has a `public` or `open` visibility modifier.
fn is_swift_public(node: Node, contents: &str) -> bool {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		if child.kind() == "modifiers" {
			let mut mod_cursor = child.walk();
			for modifier in child.children(&mut mod_cursor) {
				if modifier.kind() == "visibility_modifier" {
					if let Ok(text) = modifier.utf8_text(contents.as_bytes()) {
						if text == "public" || text == "open" {
							return true;
						}
					}
				}
			}
		}
	}
	false
}

/// Extract a plain type name string from a `user_type` or `type_identifier` node.
fn extract_swift_type_name(node: Node, contents: &str) -> Option<String> {
	match node.kind() {
		"type_identifier" => node.utf8_text(contents.as_bytes()).ok().map(String::from),
		"user_type" => {
			// user_type contains one or more type_identifier children (qualified names)
			let mut parts = Vec::new();
			let mut cursor = node.walk();
			for child in node.children(&mut cursor) {
				if child.kind() == "type_identifier" {
					if let Ok(text) = child.utf8_text(contents.as_bytes()) {
						parts.push(text.to_string());
					}
				}
			}
			if parts.is_empty() {
				None
			} else {
				Some(parts.join("."))
			}
		}
		_ => {
			// Fallback: grab raw text and strip generic parameters
			if let Ok(text) = node.utf8_text(contents.as_bytes()) {
				super::simple_type_name(text)
			} else {
				None
			}
		}
	}
}
