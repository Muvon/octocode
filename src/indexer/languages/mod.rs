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
mod javascript;
mod json;
mod php;
mod python;
mod ruby;
mod rust;
mod svelte;
mod typescript;

// Re-export language modules
pub use bash::Bash;
pub use cpp::Cpp;
pub use css::Css;
pub use go::Go;
pub use javascript::JavaScript;
pub use json::Json;
pub use php::Php;
pub use python::Python;
pub use ruby::Ruby;
pub use rust::Rust;
pub use svelte::Svelte;
pub use typescript::TypeScript;

/// Common trait for all language parsers
pub trait Language {
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
}

/// Gets a language implementation by its name
pub fn get_language(name: &str) -> Option<Box<dyn Language>> {
	match name {
		"rust" => Some(Box::new(Rust {})),
		"javascript" => Some(Box::new(JavaScript {})),
		"typescript" => Some(Box::new(TypeScript {})),
		"python" => Some(Box::new(Python {})),
		"go" => Some(Box::new(Go {})),
		"cpp" => Some(Box::new(Cpp {})),
		"php" => Some(Box::new(Php {})),
		"bash" => Some(Box::new(Bash {})),
		"ruby" => Some(Box::new(Ruby {})),
		"json" => Some(Box::new(Json {})),
		"svelte" => Some(Box::new(Svelte {})),
		"css" => Some(Box::new(Css {})),
		_ => None,
	}
}
