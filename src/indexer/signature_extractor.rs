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

//! Signature extraction module for code analysis
//!
//! This module handles the extraction of meaningful code signatures from source files
//! using tree-sitter parsing. It identifies functions, classes, structs, and other
//! meaningful declarations along with their metadata and documentation.

use anyhow::Result;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser};

use crate::indexer::{languages, path_utils};

/// Represents a file with its extracted signatures
#[derive(Debug, Serialize, Clone)]
pub struct FileSignature {
	pub path: String,
	pub language: String,
	pub file_comment: Option<String>,
	pub signatures: Vec<SignatureItem>,
}

/// Represents a single code signature (function, class, etc.)
#[derive(Debug, Serialize, Clone)]
pub struct SignatureItem {
	pub kind: String,                // e.g., "function", "struct", "class", etc.
	pub name: String,                // Name of the item
	pub signature: String,           // Full signature
	pub description: Option<String>, // Comment if available
	pub start_line: usize,           // Start line number
	pub end_line: usize,             // End line number
}

/// Extract signatures from multiple files
pub fn extract_file_signatures(files: &[PathBuf]) -> Result<Vec<FileSignature>> {
	let mut all_signatures = Vec::new();
	let mut parser = Parser::new();
	let current_dir = std::env::current_dir()?;

	for file_path in files {
		if let Some(language) = detect_language(file_path) {
			// Get the language implementation
			let lang_impl = match languages::get_language(language) {
				Some(impl_) => impl_,
				None => continue, // Skip unsupported languages
			};

			// Set the parser language
			parser.set_language(&lang_impl.get_ts_language())?;

			// Read file contents
			if let Ok(contents) = fs::read_to_string(file_path) {
				// Create a relative path for display using our utility
				let display_path = path_utils::PathUtils::for_display(file_path, &current_dir);

				// Parse the file
				let tree = parser
					.parse(&contents, None)
					.unwrap_or_else(|| parser.parse("", None).unwrap());

				// Extract signatures from the file
				let signatures =
					extract_signatures(tree.root_node(), &contents, lang_impl.as_ref());

				// Extract file-level comment if present
				let file_comment = extract_file_comment(tree.root_node(), &contents);

				// Add to our results
				all_signatures.push(FileSignature {
					path: display_path,
					language: lang_impl.name().to_string(),
					file_comment,
					signatures,
				});
			}
		}
	}

	Ok(all_signatures)
}

/// Extract signatures from a parsed file
fn extract_signatures(
	node: Node,
	contents: &str,
	lang_impl: &dyn languages::Language,
) -> Vec<SignatureItem> {
	let mut signatures = Vec::new();
	let meaningful_kinds = lang_impl.get_meaningful_kinds();

	// Create a visitor function to traverse the tree
	fn visit_node(
		node: Node,
		contents: &str,
		lang_impl: &dyn languages::Language,
		meaningful_kinds: &[&str],
		signatures: &mut Vec<SignatureItem>,
	) {
		let node_kind = node.kind();

		// Check if this node is a meaningful declaration
		if meaningful_kinds.contains(&node_kind) {
			// Get the line numbers
			let start_line = node.start_position().row;
			let end_line = node.end_position().row;

			// Extract the name of the item (function name, struct name, etc.)
			let name = extract_name(node, contents, lang_impl);

			// Extract the preceding comment if available
			let description = extract_preceding_comment(node, contents);

			if let Some(name) = name {
				// Get the full signature text
				let sig_text = node_text(node, contents);

				// Map tree-sitter node kinds to our simplified kinds
				let kind = map_node_kind_to_simple(node_kind);

				signatures.push(SignatureItem {
					kind,
					name,
					signature: sig_text,
					description,
					start_line,
					end_line,
				});
			}
		}

		// Recursively process children
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				visit_node(
					cursor.node(),
					contents,
					lang_impl,
					meaningful_kinds,
					signatures,
				);
				if !cursor.goto_next_sibling() {
					break;
				}
			}
		}
	}

	// Start traversal from the root
	visit_node(
		node,
		contents,
		lang_impl,
		&meaningful_kinds,
		&mut signatures,
	);

	// Sort by line number for a consistent order
	signatures.sort_by_key(|sig| sig.start_line);

	signatures
}

/// Extract the name of a declaration node (function, class, etc.)
fn extract_name(node: Node, contents: &str, lang_impl: &dyn languages::Language) -> Option<String> {
	// Look for identifier nodes
	for child in node.children(&mut node.walk()) {
		if child.kind() == "identifier"
			|| child.kind().contains("name")
			|| child.kind().contains("function_name")
		{
			if let Ok(name) = child.utf8_text(contents.as_bytes()) {
				if !name.is_empty() {
					return Some(name.to_string());
				}
			}
		}
	}

	// Fall back to using language-specific symbol extraction
	let symbols = lang_impl.extract_symbols(node, contents);
	symbols.into_iter().next()
}

/// Extract a preceding comment if available
fn extract_preceding_comment(node: Node, contents: &str) -> Option<String> {
	if let Some(parent) = node.parent() {
		let mut siblings = Vec::new();
		let mut cursor = parent.walk();

		if cursor.goto_first_child() {
			loop {
				let current = cursor.node();
				if current.id() == node.id() {
					break;
				}
				siblings.push(current);
				if !cursor.goto_next_sibling() {
					break;
				}
			}
		}

		// Check the last sibling before our node
		if let Some(last) = siblings.last() {
			if last.kind().contains("comment") {
				if let Ok(comment) = last.utf8_text(contents.as_bytes()) {
					// Clean up comment markers
					let comment = comment
						.trim()
						.trim_start_matches("/")
						.trim_start_matches("*")
						.trim_start_matches("/")
						.trim_end_matches("*/")
						.trim();
					return Some(comment.to_string());
				}
			}
		}
	}
	None
}

/// Extract a file-level comment (usually at the top of the file)
fn extract_file_comment(root: Node, contents: &str) -> Option<String> {
	let mut cursor = root.walk();
	if cursor.goto_first_child() {
		// Check if the first node is a comment
		let first = cursor.node();
		if first.kind().contains("comment") {
			if let Ok(comment) = first.utf8_text(contents.as_bytes()) {
				// Clean up comment markers
				let comment = comment
					.trim()
					.trim_start_matches("/")
					.trim_start_matches("*")
					.trim_start_matches("/")
					.trim_end_matches("*/")
					.trim();
				return Some(comment.to_string());
			}
		}
	}
	None
}

/// Get the full text of a node
fn node_text(node: Node, contents: &str) -> String {
	if let Ok(text) = node.utf8_text(contents.as_bytes()) {
		text.to_string()
	} else {
		// Fall back to byte range if UTF-8 conversion fails
		let start_byte = node.start_byte();
		let end_byte = node.end_byte();
		let content_bytes = contents.as_bytes();

		if start_byte < end_byte && end_byte <= content_bytes.len() {
			String::from_utf8_lossy(&content_bytes[start_byte..end_byte]).to_string()
		} else {
			String::new()
		}
	}
}

/// Map tree-sitter node kinds to simpler, unified kinds for display
fn map_node_kind_to_simple(kind: &str) -> String {
	match kind {
		k if k.contains("function") => "function".to_string(),
		k if k.contains("method") => "method".to_string(),
		k if k.contains("class") => "class".to_string(),
		k if k.contains("struct") => "struct".to_string(),
		k if k.contains("enum") => "enum".to_string(),
		k if k.contains("interface") => "interface".to_string(),
		k if k.contains("trait") => "trait".to_string(),
		k if k.contains("mod") || k.contains("module") => "module".to_string(),
		k if k.contains("const") => "constant".to_string(),
		k if k.contains("macro") => "macro".to_string(),
		k if k.contains("type") => "type".to_string(),
		_ => kind.to_string(), // Fall back to the original kind
	}
}

/// Detect language based on file extension (re-exported from file_utils)
fn detect_language(path: &Path) -> Option<&str> {
	use crate::indexer::file_utils::FileUtils;
	FileUtils::detect_language(path)
}
