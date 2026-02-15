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

//! Language support module for the indexer
//! Provides a common interface for language-specific parsing and symbol extraction

use tree_sitter::Node;

// Import all language modules
mod bash;
mod cpp;
mod css;
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
mod typescript;

// Re-export language modules
pub use bash::Bash;
pub use cpp::Cpp;
pub use css::Css;
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
pub use typescript::TypeScript;

/// Common trait for all language parsers
pub trait Language: Send + Sync {
	/// Name of the language
	fn name(&self) -> &'static str;

	/// Get tree-sitter language for parsing
	fn get_ts_language(&self) -> tree_sitter::Language;

	/// Returns node kinds considered meaningful for this language
	fn get_meaningful_kinds(&self) -> Vec<&'static str>;

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
		"css" => Some(Box::new(Css {})),
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
			&& !symbols.contains(&trimmed.to_string())
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
