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

//! Language support module for the indexer
//! Provides a common interface for language-specific parsing and symbol extraction

use tree_sitter::Node;

// Import all language modules
mod bash;
mod cpp;
#[cfg(test)]
mod cpp_test;
mod css;
mod elixir;
#[cfg(test)]
mod elixir_test;
mod go;
mod java;
mod javascript;
mod json;
mod lua;
mod markdown;
mod php;
#[cfg(test)]
mod php_test;
mod python;
pub mod resolution_utils;
mod ruby;
mod rust;
mod svelte;
mod swift;
#[cfg(test)]
mod swift_test;
mod typescript;

// Re-export language modules
pub use bash::Bash;
pub use cpp::Cpp;
pub use css::Css;
pub use elixir::Elixir;
pub use go::Go;
pub use java::Java;
pub use javascript::JavaScript;
pub use json::Json;
pub use lua::Lua;
pub use markdown::Markdown;
pub use php::Php;
pub use python::Python;
pub use ruby::Ruby;
pub use rust::Rust;
pub use svelte::Svelte;
pub use swift::Swift;
pub use typescript::TypeScript;

/// Kind of type-level relationship a language parser may report from an AST node.
/// Used by GraphRAG to emit Extends / Implements edges between files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeRelationKind {
	/// Inheritance: subclass extends superclass; trait/interface inherits from another.
	Extends,
	/// Interface or trait implementation: a concrete type satisfies an interface/trait.
	Implements,
}

/// A callable reference extracted from a language AST.
///
/// `name` is always the terminal callable (`run` in `service.run()`), while
/// `qualifier` preserves the receiver/module/type (`service`). Keeping both
/// avoids turning every segment of a qualified expression into a bogus call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallTarget {
	pub name: String,
	pub qualifier: Option<String>,
}

/// A code region embedded in another supported language (for example a
/// JavaScript/TypeScript `<script>` inside Svelte).
#[derive(Debug, Clone)]
pub struct EmbeddedSource {
	pub language: &'static str,
	pub contents: String,
	pub start_line: u32,
}

/// Common trait for all language parsers
pub trait Language: Send + Sync {
	/// Name of the language
	fn name(&self) -> &'static str;

	/// Get tree-sitter language for parsing
	fn get_ts_language(&self) -> tree_sitter::Language;

	/// Returns node kinds considered meaningful for this language
	fn get_meaningful_kinds(&self) -> Vec<&'static str>;

	/// Whether a node whose kind is listed in `get_meaningful_kinds` should
	/// become an indexed code region. Most grammars have dedicated declaration
	/// kinds; macro-oriented grammars such as Elixir need to inspect the node.
	fn is_meaningful_node(&self, node: Node, contents: &str) -> bool {
		let _ = (node, contents);
		true
	}

	/// Whether a candidate node should appear in signature views. Defaults to
	/// the indexing decision, but languages may expose container declarations in
	/// signatures while chunking only their nested members.
	fn is_signature_node(&self, node: Node, contents: &str) -> bool {
		self.is_meaningful_node(node, contents)
	}

	/// Node kinds that become symbol-level GraphRAG nodes. Defaults to the
	/// chunking kinds; languages override when chunking deliberately drops
	/// large containers (classes/interfaces/enums) or keeps non-declaration
	/// nodes (imports, calls, statements) that must not become symbols.
	fn get_symbol_kinds(&self) -> Vec<&'static str> {
		self.get_meaningful_kinds()
	}

	/// Extract symbols from a node
	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String>;

	/// Extract identifiers from a node (helper method)
	fn extract_identifiers(&self, node: Node, contents: &str, symbols: &mut Vec<String>);

	/// Extract import/export information for GraphRAG (separate from symbols)
	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		// Default implementation returns empty - languages can override
		let _ = (node, contents);
		(Vec::new(), Vec::new())
	}

	/// Extract function/method call names from a node.
	/// Returns callee names if this node represents a function call.
	/// The recursive walk and line tracking is handled by the caller.
	fn extract_function_calls(&self, node: Node, contents: &str) -> Vec<CallTarget> {
		let _ = (node, contents);
		Vec::new()
	}

	/// Owning type/module for a declared symbol, when it is syntactically known.
	/// The default handles the common class/module/impl ancestor shapes used by
	/// the supported tree-sitter grammars. Languages with receiver syntax on the
	/// declaration itself (notably Go) override this method.
	fn extract_symbol_owner(&self, node: Node, contents: &str) -> Option<String> {
		find_graph_symbol_owner(node, contents)
	}

	/// Extract embedded code regions that should participate in the live graph.
	fn extract_embedded_sources(&self, root: Node, contents: &str) -> Vec<EmbeddedSource> {
		let _ = (root, contents);
		Vec::new()
	}

	/// Extract type-level relationships (extends / implements) declared at this node.
	/// Returns (kind, target_name) pairs — e.g. for `class Foo extends Bar implements Baz`,
	/// the class declaration node should yield `[(Extends, "Bar"), (Implements, "Baz")]`.
	/// The recursive walk is handled by the caller.
	fn extract_type_relations(
		&self,
		node: Node,
		contents: &str,
	) -> Vec<(TypeRelationKind, String)> {
		let _ = (node, contents);
		Vec::new()
	}

	/// Name of the declaration that owns type relationships emitted for this
	/// node. Most languages put the declaration name directly on the same node;
	/// languages with separate implementation blocks can override this.
	fn extract_type_relation_source(&self, node: Node, contents: &str) -> Option<String> {
		self.extract_declaration_name(node, contents)
	}

	/// Declared name of the symbol at this node, for symbol-level GraphRAG
	/// nodes. Unlike `extract_symbols` (which enriches and sorts), this must
	/// return exactly the name the declaration introduces, or None when the
	/// node has no single declarable name. The default covers the dominant
	/// tree-sitter convention: a direct child whose kind is or contains
	/// "identifier", "name", or "type_identifier".
	fn extract_declaration_name(&self, node: Node, contents: &str) -> Option<String> {
		extract_symbol_by_kinds(node, contents, &["identifier", "name", "type_identifier"])
	}

	/// Name shown by signature views. Usually identical to the graph symbol
	/// name, but declaration forms that do not create a standalone graph symbol
	/// may still expose a useful signature name.
	fn extract_signature_name(&self, node: Node, contents: &str) -> Option<String> {
		self.extract_declaration_name(node, contents)
	}

	/// Coarse declaration kind when it cannot be inferred from the grammar's
	/// node kind. Callers fall back to their normal node-kind mapping.
	fn extract_declaration_kind(&self, node: Node, contents: &str) -> Option<&'static str> {
		let _ = (node, contents);
		None
	}

	/// Check if two node types are semantically equivalent for grouping
	/// This allows each language to define its own semantic relationships
	fn are_node_types_equivalent(&self, type1: &str, type2: &str) -> bool {
		// Default implementation: only exact matches
		type1 == type2
	}

	/// Get a descriptive name for a node type
	/// This allows each language to provide user-friendly descriptions
	fn get_node_type_description(&self, node_type: &str) -> &'static str {
		// Default fallback descriptions
		match node_type {
			t if t.contains("function") => "function declarations",
			t if t.contains("method") => "function declarations",
			t if t.contains("class") => "class/interface declarations",
			t if t.contains("struct") => "type definitions",
			t if t.contains("enum") => "type definitions",
			t if t.contains("mod") || t.contains("module") => "module declarations",
			t if t.contains("const") => "constant declarations",
			t if t.contains("var") || t.contains("let") => "variable declarations",
			t if t.contains("type") => "type declarations",
			t if t.contains("trait") => "trait declarations",
			t if t.contains("impl") => "implementation blocks",
			t if t.contains("macro") => "macro definitions",
			t if t.contains("namespace") => "namespace declarations",
			t if t.contains("comment") => "comments",
			_ => "declarations",
		}
	}

	/// Resolve import paths to actual file paths
	/// Returns the resolved file path if found, None otherwise
	fn resolve_import(
		&self,
		import_path: &str,
		source_file: &str,
		all_files: &[String],
	) -> Option<String>;

	/// Get file extensions supported by this language
	fn get_file_extensions(&self) -> Vec<&'static str>;
}

/// Gets a language implementation by its name
pub fn get_language(name: &str) -> Option<Box<dyn Language>> {
	match name {
		"rust" => Some(Box::new(Rust {})),
		"javascript" => Some(Box::new(JavaScript {})),
		"typescript" => Some(Box::new(TypeScript {})),
		"python" => Some(Box::new(Python {})),
		"go" => Some(Box::new(Go {})),
		"java" => Some(Box::new(Java {})),
		"cpp" => Some(Box::new(Cpp {})),
		"php" => Some(Box::new(Php {})),
		"bash" => Some(Box::new(Bash {})),
		"ruby" => Some(Box::new(Ruby {})),
		"lua" => Some(Box::new(Lua {})),
		"json" => Some(Box::new(Json {})),
		"svelte" => Some(Box::new(Svelte {})),
		"swift" => Some(Box::new(Swift {})),
		"css" => Some(Box::new(Css {})),
		"elixir" => Some(Box::new(Elixir {})),
		"markdown" => Some(Box::new(Markdown {})),
		_ => None,
	}
}

// ============================================================================
// SHARED HELPER FUNCTIONS FOR LANGUAGE IMPLEMENTATIONS
// ============================================================================

/// Helper function to deduplicate and sort symbols
/// Used by all language implementations of extract_symbols
pub fn deduplicate_symbols(symbols: &mut Vec<String>) {
	symbols.sort();
	symbols.dedup();
}

/// Default implementation for extracting identifiers recursively
/// Languages can call this with custom filtering logic
///
/// # Arguments
/// * `node` - The tree-sitter node to extract from
/// * `contents` - The source code contents
/// * `symbols` - Mutable vector to collect symbols into
/// * `should_include` - Optional filter function returning true if identifier should be included
///
/// # Example
/// ```ignore
/// extract_identifiers_default(node, contents, symbols, |kind, text| {
///     kind.contains("identifier") && !text.starts_with("_")
/// });
/// ```
pub fn extract_identifiers_default<F>(
	node: Node,
	contents: &str,
	symbols: &mut Vec<String>,
	should_include: F,
) where
	F: Fn(&str, &str) -> bool + Copy,
{
	let kind = node.kind();
	if let Ok(text) = node.utf8_text(contents.as_bytes()) {
		let trimmed = text.trim();
		if !trimmed.is_empty()
			&& should_include(kind, trimmed)
			&& !symbols.iter().any(|s| s.as_str() == trimmed)
		{
			symbols.push(trimmed.to_string());
		}
	}

	// Recursively traverse children
	let mut cursor = node.walk();
	if cursor.goto_first_child() {
		loop {
			extract_identifiers_default(cursor.node(), contents, symbols, should_include);
			if !cursor.goto_next_sibling() {
				break;
			}
		}
	}
}

/// Check if two node types belong to the same semantic group
/// Used by are_node_types_equivalent implementations
///
/// # Arguments
/// * `type1` - First node type
/// * `type2` - Second node type
/// * `semantic_groups` - Array of node type groups that should be considered equivalent
///
/// # Example
/// ```ignore
/// let groups = [
///     &["function_item", "function_declaration"] as &[&str],
///     &["struct_item", "class_declaration"],
/// ];
/// check_semantic_groups("function_item", "function_declaration", &groups) // returns true
/// ```
pub fn check_semantic_groups(type1: &str, type2: &str, semantic_groups: &[&[&str]]) -> bool {
	// Direct match
	if type1 == type2 {
		return true;
	}

	// Check if both types belong to the same semantic group
	for group in semantic_groups {
		let contains_type1 = group.contains(&type1);
		let contains_type2 = group.contains(&type2);

		if contains_type1 && contains_type2 {
			return true;
		}
	}

	false
}

/// Extract a symbol from a node by finding a child with a specific kind
/// Common pattern used across multiple languages
///
/// # Arguments
/// * `node` - Parent node to search
/// * `contents` - Source code contents
/// * `target_kind` - The kind of child node to find (e.g., "identifier", "name")
///
/// # Returns
/// The extracted symbol text, or None if not found
pub fn extract_symbol_by_kind(node: Node, contents: &str, target_kind: &str) -> Option<String> {
	for child in node.children(&mut node.walk()) {
		if child.kind() == target_kind {
			if let Ok(text) = child.utf8_text(contents.as_bytes()) {
				return Some(text.to_string());
			}
		}
	}
	None
}

/// Extract the simple (unqualified, non-generic) type name from a possibly
/// qualified or generic type expression. Examples:
/// `std::collections::HashMap<K, V>` → `HashMap`,
/// `com.example.Foo` → `Foo`,
/// `Foo<T>` → `Foo`, `Bar` → `Bar`.
pub fn simple_type_name(text: &str) -> Option<String> {
	let stripped = text.split('<').next().unwrap_or(text);
	let after_colons = stripped.rsplit("::").next().unwrap_or(stripped);
	let after_dots = after_colons.rsplit('.').next().unwrap_or(after_colons);
	let trimmed = after_dots
		.trim()
		.trim_matches(|character: char| !character.is_alphanumeric() && character != '_');
	if trimmed.is_empty() {
		None
	} else {
		Some(trimmed.to_string())
	}
}

/// Parse a textual callee expression without losing its qualifier.
///
/// Handles the shared forms used by the supported languages: `foo`,
/// `module.foo`, `Type::method`, `ptr->method`, and optional chaining. Generic
/// arguments on the callable are removed before splitting.
pub fn extract_call_target(text: &str) -> Option<CallTarget> {
	let mut trimmed = text.trim().trim_start_matches('&').trim_start_matches('*');
	if trimmed.is_empty() {
		return None;
	}

	let mut without_generics = String::with_capacity(trimmed.len());
	let mut generic_depth = 0u32;
	for character in trimmed.chars() {
		match character {
			'<' => generic_depth += 1,
			'>' if generic_depth > 0 => generic_depth -= 1,
			_ if generic_depth == 0 => without_generics.push(character),
			_ => {}
		}
	}
	trimmed = without_generics.trim();
	let normalized = trimmed.replace("?.", ".").replace("->", ".");
	let segments: Vec<&str> = normalized
		.split(['.', ':'])
		.map(str::trim)
		.filter(|segment| !segment.is_empty())
		.collect();
	let (name, qualifier_segments) = segments.split_last()?;
	let name =
		name.trim_matches(|character: char| !character.is_alphanumeric() && character != '_');
	if name.is_empty()
		|| !name
			.chars()
			.all(|character| character.is_alphanumeric() || character == '_')
	{
		return None;
	}
	let qualifier = if qualifier_segments.is_empty() {
		None
	} else {
		if qualifier_segments.iter().any(|segment| {
			!segment.chars().all(|character| {
				character.is_alphanumeric() || matches!(character, '_' | '$' | '@' | '#')
			})
		}) {
			return None;
		}
		Some(qualifier_segments.join("::"))
	};
	Some(CallTarget {
		name: name.to_string(),
		qualifier,
	})
}

/// Find the nearest enclosing declaration that provides a stable method owner.
pub fn find_graph_symbol_owner(node: Node, contents: &str) -> Option<String> {
	let mut current = node.parent();
	while let Some(parent) = current {
		let kind = parent.kind();
		let is_owner = !kind.contains("body")
			&& (kind.contains("class")
				|| kind.contains("struct")
				|| kind.contains("interface")
				|| kind.contains("trait")
				|| kind.contains("module")
				|| kind.contains("namespace")
				|| kind.contains("extension")
				|| kind == "impl_item");
		if is_owner {
			for field in ["type", "name"] {
				if let Some(name_node) = parent.child_by_field_name(field) {
					if let Ok(text) = name_node.utf8_text(contents.as_bytes()) {
						if let Some(name) = simple_type_name(text) {
							return Some(name);
						}
					}
				}
			}
			return extract_symbol_by_kinds(
				parent,
				contents,
				&["identifier", "name", "type_identifier", "constant"],
			)
			.and_then(|name| simple_type_name(&name));
		}
		current = parent.parent();
	}
	None
}

/// Extract a symbol from a node by finding a child matching any of multiple kinds
/// Useful when multiple node kinds can represent names (e.g., "identifier" or "name")
///
/// # Arguments
/// * `node` - Parent node to search
/// * `contents` - Source code contents
/// * `target_kinds` - Array of acceptable child node kinds
///
/// # Returns
/// The first matching symbol text, or None if not found
pub fn extract_symbol_by_kinds(
	node: Node,
	contents: &str,
	target_kinds: &[&str],
) -> Option<String> {
	for child in node.children(&mut node.walk()) {
		if target_kinds.contains(&child.kind())
			|| target_kinds.iter().any(|k| child.kind().contains(k))
		{
			if let Ok(text) = child.utf8_text(contents.as_bytes()) {
				return Some(text.to_string());
			}
		}
	}
	None
}

/// Walk up the parent chain from `node` looking for an enclosing container of any
/// `container_kinds` (e.g. `impl_item`, `class_definition`, `class_declaration`).
/// When one is found, return its name by looking at its direct children for any of
/// `name_kinds` and, for Rust-style trait impls, also try the `type` field. The
/// returned name is normalized via `simple_type_name` so generics/qualifiers drop.
///
/// Used to enrich method symbols with their owning type — e.g. a `mark_set` method
/// declared inside `impl Suppression { ... }` gets `Suppression` added to its
/// symbol list so BM25 and dense retrieval can hit "Suppression mark_set" queries
/// without depending on the LLM contextual description mentioning the receiver.
pub fn find_enclosing_container_name(
	node: Node,
	contents: &str,
	container_kinds: &[&str],
	name_kinds: &[&str],
) -> Option<String> {
	let mut cur = node.parent();
	while let Some(parent) = cur {
		if container_kinds.contains(&parent.kind()) {
			// Prefer Rust's `type` field on impl_item (works for both `impl Foo` and
			// `impl Trait for Foo`). Falls through to name-kind scan for other langs.
			if let Some(type_field) = parent.child_by_field_name("type") {
				if let Ok(text) = type_field.utf8_text(contents.as_bytes()) {
					if let Some(name) = simple_type_name(text) {
						return Some(name);
					}
				}
			}
			// Generic: first direct child whose kind matches any name_kinds entry.
			for child in parent.children(&mut parent.walk()) {
				if name_kinds.iter().any(|k| child.kind() == *k) {
					if let Ok(text) = child.utf8_text(contents.as_bytes()) {
						if let Some(name) = simple_type_name(text) {
							return Some(name);
						}
					}
				}
			}
			// Container found but unnamed (anonymous class/struct) — stop walking,
			// don't bubble up to grandparents which would attribute the method to
			// the wrong owner.
			return None;
		}
		cur = parent.parent();
	}
	None
}

#[cfg(test)]
mod graph_extraction_tests {
	use super::*;

	#[test]
	fn structured_callee_preserves_terminal_name_and_qualifier() {
		for (input, expected_name, expected_qualifier) in [
			("helper", "helper", None),
			("service.run", "run", Some("service")),
			("Service::new", "new", Some("Service")),
			("std::vector<Item>::make", "make", Some("std::vector")),
			("ptr->flush", "flush", Some("ptr")),
			("client?.send", "send", Some("client")),
		] {
			let target = extract_call_target(input).expect("callee should parse");
			assert_eq!(target.name, expected_name);
			assert_eq!(target.qualifier.as_deref(), expected_qualifier);
		}
	}

	#[test]
	fn dynamic_callee_is_dropped_instead_of_inventing_a_symbol() {
		assert!(extract_call_target("obj[method]").is_none());
		assert!(extract_call_target("condition ? first : second").is_none());
	}
}
