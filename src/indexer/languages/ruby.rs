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

//! Ruby language implementation for the indexer

use crate::indexer::code_region_extractor::{extract_meaningful_regions, CodeRegion};
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
		// `class` and `module` are intentionally excluded so large containers don't
		// collapse into a single chunk. Methods inside them are captured individually
		// as `method`/`singleton_method` via recursion. `call` stays for require/load statements.
		vec!["method", "singleton_method", "call"]
	}

	fn expand_meaningful_node(&self, node: Node, contents: &str) -> Option<Vec<CodeRegion>> {
		// A DSL `call` with a do/end or {} block (e.g. RSpec `describe "x" do
		// ... end`) can wrap other block-taking calls (`it`, `context`, ...).
		// `is_meaningful_node` is never false for `call` in Ruby (unlike
		// Elixir), so gate on the block itself instead: only a call that has
		// one is a candidate, and only OTHER block-taking calls found inside
		// it count as nested content — a plain call reached along the way
		// (e.g. `expect(x).to eq(1)`) is left alone rather than promoted to
		// its own region, so it stays folded into whichever block/region
		// already contains it. Returning None when no nested block-call is
		// found falls back to capturing the whole call as one region, e.g. a
		// plain `it("a") { ... }` with no further nested blocks, or a call
		// with no block at all (`puts x`, `require 'foo'`).
		if node.kind() != "call" {
			return None;
		}
		let block = node.child_by_field_name("block")?;
		let mut nested_calls = Vec::new();
		collect_block_calls(block, &mut nested_calls);
		if nested_calls.is_empty() {
			return None;
		}
		let mut sub_regions = Vec::new();
		for call in nested_calls {
			extract_meaningful_regions(call, contents, self, &mut sub_regions);
		}
		(!sub_regions.is_empty()).then_some(sub_regions)
	}

	fn get_symbol_kinds(&self) -> Vec<&'static str> {
		// Symbol tier restores class/module containers and drops `call`: a
		// call's method-name child matches the default name scan, which would
		// turn every call site into a bogus symbol node.
		vec!["method", "singleton_method", "class", "module"]
	}

	fn extract_declaration_name(&self, node: Node, contents: &str) -> Option<String> {
		if matches!(node.kind(), "class" | "module") {
			// Names are `constant` nodes; qualified names (`Foo::Bar`) are
			// `scope_resolution` nodes whose LAST `constant` is the defined one.
			let mut cursor = node.walk();
			for child in node.children(&mut cursor) {
				let name_node = match child.kind() {
					"constant" => Some(child),
					"scope_resolution" => {
						let mut inner = child.walk();
						child
							.children(&mut inner)
							.filter(|c| c.kind() == "constant")
							.last()
					}
					_ => None,
				};
				if let Some(name_node) = name_node {
					return name_node
						.utf8_text(contents.as_bytes())
						.ok()
						.map(String::from);
				}
			}
			return None;
		}
		super::extract_symbol_by_kinds(node, contents, &["identifier", "name", "type_identifier"])
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"method" | "singleton_method" | "class" | "module" => {
				// Find method, class, or module name. A namespaced definition
				// (`module Outer::Inner`) is named by a `scope_resolution`, so
				// matching only identifier/constant leaves it with no symbol.
				for child in node.children(&mut node.walk()) {
					if matches!(child.kind(), "identifier" | "constant" | "scope_resolution") {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							symbols.push(name.to_string());
						}
						break;
					}
				}

				// For methods, extract local variables and the enclosing class/module
				// name so queries like "Foo#bar" / "Foo.bar" resolve via BM25/dense.
				if node.kind() == "method" || node.kind() == "singleton_method" {
					if let Some(owner) = super::find_enclosing_container_name(
						node,
						contents,
						&["class", "module"],
						&["constant", "identifier"],
					) {
						symbols.push(owner);
					}
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
				// Dedup happens once in extract_symbols via sort+dedup on the
				// full result, so no need to scan on every push here.
				if !t.is_empty() && !t.starts_with('@') {
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

	fn extract_function_calls(&self, node: Node, contents: &str) -> Vec<super::CallTarget> {
		if node.kind() == "call" {
			// The `method` field is the actual callee name; for `receiver.method(...)`
			// calls, children appear in source order (receiver, operator, method), so
			// picking the first identifier/constant child would return the receiver.
			if let Some(method) = node.child_by_field_name("method") {
				if let Ok(text) = method.utf8_text(contents.as_bytes()) {
					let name = text.trim();
					// Skip require/require_relative/load — those are imports
					if name == "require" || name == "require_relative" || name == "load" {
						return Vec::new();
					}
					let raw = node
						.child_by_field_name("receiver")
						.and_then(|receiver| receiver.utf8_text(contents.as_bytes()).ok())
						.map(|receiver| format!("{}.{}", receiver, name))
						.unwrap_or_else(|| name.to_string());
					return super::extract_call_target(&raw).into_iter().collect();
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
		// `class Foo < Bar` → Foo extends Bar.
		// Module mixins (`include M`, `extend M`) are call expressions and
		// would require pattern-matching on call sites; deferred for now.
		let mut out = Vec::new();
		if node.kind() == "class" {
			if let Some(superclass) = node.child_by_field_name("superclass") {
				let mut cursor = superclass.walk();
				for child in superclass.children(&mut cursor) {
					if matches!(child.kind(), "constant" | "scope_resolution") {
						if let Ok(text) = child.utf8_text(contents.as_bytes()) {
							if let Some(name) = super::simple_type_name(text) {
								out.push((super::TypeRelationKind::Extends, name));
							}
						}
					}
				}
			}
		}
		out
	}

	fn resolve_import(
		&self,
		import_path: &str,
		source_file: &str,
		all_files: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		let registry = all_files;

		if import_path.starts_with("relative:") {
			// require_relative import
			let relative_path = import_path.strip_prefix("relative:")?;
			self.resolve_relative_require(relative_path, source_file, registry)
		} else if import_path.starts_with("./") || import_path.starts_with("../") {
			// Relative require
			self.resolve_relative_require(import_path, source_file, registry)
		} else {
			// Absolute require
			self.resolve_absolute_require(import_path, registry)
		}
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["rb"]
	}
}

/// Recursively collect `call` nodes that themselves have a block, without
/// treating a plain (blockless) call's own arguments as a place more such
/// calls could legitimately live. This is what lets `expand_meaningful_node`
/// recurse into a DSL wrapper's (`describe`/`context`) nested block-taking
/// calls while leaving ordinary calls found along the way untouched.
fn collect_block_calls<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		if child.kind() == "call" && child.child_by_field_name("block").is_some() {
			out.push(child);
		} else {
			collect_block_calls(child, out);
		}
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
								if !name.starts_with('@')
									&& !symbols.iter().any(|s| s.as_str() == name)
								{
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
		if text.len() >= 2
			&& ((text.starts_with('"') && text.ends_with('"'))
				|| (text.starts_with('\'') && text.ends_with('\'')))
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
