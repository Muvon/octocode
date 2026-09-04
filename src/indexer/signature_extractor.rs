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

/// Extract signatures from multiple files.
///
/// `base_dir` is the root the reported paths are made relative to. It must be
/// the directory the caller is serving — the MCP tools work against an explicit
/// working directory that need not be the process CWD, and resolving against
/// the CWD there collapses every path to a bare file name the client cannot
/// locate.
pub fn extract_file_signatures(files: &[PathBuf], base_dir: &Path) -> Result<Vec<FileSignature>> {
	let mut all_signatures = Vec::new();
	let mut parser = Parser::new();
	let current_dir = base_dir.to_path_buf();

	for file_path in files {
		if let Some(language) = detect_language(file_path) {
			// Read file contents
			if let Ok(contents) = fs::read_to_string(file_path) {
				// Create a relative path for display using our utility
				let display_path = path_utils::PathUtils::for_display(file_path, &current_dir);

				// Handle markdown files specially (no tree-sitter parsing)
				if language == "markdown" {
					let signatures = extract_markdown_signatures(&contents);
					let file_comment = extract_markdown_file_comment(&contents);

					all_signatures.push(FileSignature {
						path: display_path,
						language: "markdown".to_string(),
						file_comment,
						signatures,
					});
				} else {
					// Get the language implementation
					let lang_impl = match languages::get_language(language) {
						Some(impl_) => impl_,
						None => continue, // Skip unsupported languages
					};

					// Set the parser language
					parser.set_language(&lang_impl.get_ts_language())?;

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
	}

	Ok(all_signatures)
}

/// Extract signatures from a parsed file
pub fn extract_signatures(
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
		if meaningful_kinds.contains(&node_kind) && lang_impl.is_signature_node(node, contents) {
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
				let kind = lang_impl
					.extract_declaration_kind(node, contents)
					.map(str::to_string)
					.unwrap_or_else(|| map_node_kind_to_simple_with_context(node, contents));

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
	if let Some(name) = lang_impl.extract_signature_name(node, contents) {
		return Some(name);
	}

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

/// Maximum number of blank source lines allowed between two consecutive
/// comment siblings for them to still be considered part of the same doc
/// block. Prevents an unrelated, separated comment from merging into a
/// distinct nearby doc block.
const MAX_BLANK_LINES_BETWEEN_COMMENTS: usize = 1;

/// Clean up comment markers from a single comment node's raw text.
///
/// Block comments (`/* ... */` or `/** ... */`) have their opening/closing
/// markers stripped and a leading `*` (with following space) stripped from
/// each inner line, since tree-sitter represents these as one node spanning
/// multiple lines. Line comments (`//`, or any other single-line syntax)
/// keep the previous single-pass trimming behavior.
fn clean_comment_text(comment: &str) -> String {
	let trimmed = comment.trim();
	if trimmed.starts_with("/*") {
		let inner = trimmed
			.trim_start_matches("/**")
			.trim_start_matches("/*")
			.trim_end_matches("*/");
		inner
			.lines()
			.map(|line| {
				let line = line.trim();
				match line.strip_prefix('*') {
					Some(rest) => rest.trim_start(),
					None => line,
				}
			})
			.collect::<Vec<_>>()
			.join("\n")
			.trim()
			.to_string()
	} else {
		let cleaned = trimmed
			.trim_start_matches('/')
			.trim_start_matches('*')
			.trim_start_matches('/');
		// `//!` inner doc comments leave the `!` behind, so the file-level
		// description of every Rust module that uses them started with one.
		// Only a tight `//!` is affected: `// !note` keeps its space and is
		// left alone.
		cleaned
			.strip_prefix('!')
			.unwrap_or(cleaned)
			.trim_end_matches("*/")
			.trim()
			.to_string()
	}
}

/// Extract a preceding comment if available
fn extract_preceding_comment(node: Node, contents: &str) -> Option<String> {
	let parent = node.parent()?;

	// Track only the immediately-preceding sibling instead of buffering all.
	let mut prev: Option<Node> = None;
	let mut cursor = parent.walk();

	if cursor.goto_first_child() {
		loop {
			let current = cursor.node();
			if current.id() == node.id() {
				break;
			}
			prev = Some(current);
			if !cursor.goto_next_sibling() {
				break;
			}
		}
	}

	let last = prev?;
	if !last.kind().contains("comment") {
		return None;
	}

	// A multi-line `//`-style doc comment is one sibling node per line in
	// virtually all C-family tree-sitter grammars. Walk backward through as
	// many consecutive comment siblings as belong to the same block.
	let mut comment_nodes = vec![last];
	let mut current = last;
	while let Some(sibling) = current.prev_sibling() {
		if !sibling.kind().contains("comment") {
			break;
		}
		let gap = current
			.start_position()
			.row
			.saturating_sub(sibling.end_position().row + 1);
		if gap > MAX_BLANK_LINES_BETWEEN_COMMENTS {
			break;
		}
		comment_nodes.push(sibling);
		current = sibling;
	}
	comment_nodes.reverse();

	let lines: Vec<String> = comment_nodes
		.into_iter()
		.filter_map(|n| n.utf8_text(contents.as_bytes()).ok())
		.map(clean_comment_text)
		.collect();

	if lines.is_empty() {
		None
	} else {
		Some(lines.join("\n"))
	}
}

/// Extract a file-level comment (usually at the top of the file)
fn extract_file_comment(root: Node, contents: &str) -> Option<String> {
	let mut cursor = root.walk();
	if !cursor.goto_first_child() {
		return None;
	}

	let first = cursor.node();
	if !first.kind().contains("comment") {
		return None;
	}

	// Same chaining as `extract_preceding_comment`, but forward from the
	// file's leading comment(s) since there is no preceding sibling here.
	let mut comment_nodes = vec![first];
	let mut current = first;
	while let Some(sibling) = current.next_sibling() {
		if !sibling.kind().contains("comment") {
			break;
		}
		let gap = sibling
			.start_position()
			.row
			.saturating_sub(current.end_position().row + 1);
		if gap > MAX_BLANK_LINES_BETWEEN_COMMENTS {
			break;
		}
		comment_nodes.push(sibling);
		current = sibling;
	}

	let lines: Vec<String> = comment_nodes
		.into_iter()
		.filter_map(|n| n.utf8_text(contents.as_bytes()).ok())
		.map(clean_comment_text)
		.collect();

	if lines.is_empty() {
		None
	} else {
		Some(lines.join("\n"))
	}
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

/// Map tree-sitter node kinds to simpler, unified kinds for display with context
fn map_node_kind_to_simple_with_context(node: Node, _contents: &str) -> String {
	let kind = node.kind();

	// Special handling for C++ declaration nodes
	if kind == "declaration" && contains_function_declarator(node) {
		return "function".to_string();
	}

	// Special handling for namespace_definition
	if kind == "namespace_definition" {
		return "namespace".to_string();
	}

	// Fall back to the regular mapping
	map_node_kind_to_simple(kind)
}

/// Recursively search a declaration's subtree for a `function_declarator`.
/// Pointer/reference-returning declarations (e.g. `int* foo();`) wrap it
/// inside an intermediate `pointer_declarator`/`reference_declarator`, so a
/// direct-children-only check would miss it.
fn contains_function_declarator(node: Node) -> bool {
	for child in node.children(&mut node.walk()) {
		if child.kind() == "function_declarator" || contains_function_declarator(child) {
			return true;
		}
	}
	false
}

/// Detect language based on file extension (re-exported from file_utils)
fn detect_language(path: &Path) -> Option<&str> {
	use crate::indexer::file_utils::FileUtils;
	FileUtils::detect_language(path)
}

/// Extract signatures from markdown content
fn extract_markdown_signatures(contents: &str) -> Vec<SignatureItem> {
	use crate::indexer::markdown_processor::detect_code_fence;

	let mut signatures = Vec::new();
	let lines: Vec<&str> = contents.lines().collect();
	let mut fence_marker: Option<String> = None;

	for (line_idx, line) in lines.iter().enumerate() {
		let trimmed = line.trim();

		// Track fenced code blocks so a `#`-prefixed line inside one (a shell
		// shebang, a Python comment, a C #include, ...) isn't mistaken for a
		// markdown heading.
		if let Some(fence) = detect_code_fence(trimmed) {
			match &fence_marker {
				None => fence_marker = Some(fence),
				Some(marker) if trimmed.starts_with(marker.as_str()) => fence_marker = None,
				_ => {}
			}
			continue;
		}

		if fence_marker.is_some() {
			continue;
		}

		// Check if this is a heading
		if trimmed.starts_with('#') {
			let heading_level = trimmed.chars().take_while(|&c| c == '#').count();
			let heading_text = trimmed.trim_start_matches('#').trim();

			if !heading_text.is_empty() {
				// Extract heading + content until next heading (like code blocks)
				let mut content_lines = vec![*line];
				let mut end_line = line_idx;

				// Look ahead for content until next heading or end of file
				for i in 1.. {
					if line_idx + i >= lines.len() {
						break;
					}

					let next_line = lines[line_idx + i];
					let next_trimmed = next_line.trim();

					// Stop if we hit another heading
					if next_trimmed.starts_with('#') && !next_trimmed.starts_with("```") {
						break;
					}

					content_lines.push(next_line);
					end_line = line_idx + i;

					// Limit content to reasonable size (like code blocks)
					if content_lines.len() >= 20 {
						break;
					}
				}

				// Remove trailing empty lines for cleaner display
				while content_lines.len() > 1 && content_lines.last().unwrap().trim().is_empty() {
					content_lines.pop();
					end_line -= 1;
				}

				let signature_content = content_lines.join("\n");

				signatures.push(SignatureItem {
					kind: format!("heading{}", heading_level),
					name: heading_text.to_string(),
					signature: signature_content,
					description: None,
					start_line: line_idx,
					end_line,
				});
			}
		}
	}

	signatures
}

/// Extract file-level comment from markdown (usually first paragraph or frontmatter)
fn extract_markdown_file_comment(contents: &str) -> Option<String> {
	let lines: Vec<&str> = contents.lines().collect();

	if lines.is_empty() {
		return None;
	}

	// Check for YAML frontmatter
	if lines[0].trim() == "---" {
		let mut comment_lines = Vec::new();
		for line in lines.iter().skip(1) {
			if line.trim() == "---" {
				break;
			}
			comment_lines.push(*line);
		}
		if !comment_lines.is_empty() {
			return Some(comment_lines.join("\n"));
		}
	}

	// Look for first non-heading paragraph
	let mut comment_lines = Vec::new();
	let mut found_content = false;

	for line in &lines {
		let trimmed = line.trim();

		// Skip headings
		if trimmed.starts_with('#') {
			if found_content {
				break; // Stop at next heading
			}
			continue;
		}

		// A `---` line is a frontmatter delimiter or a horizontal rule, never
		// prose. Without this an empty or unterminated frontmatter block was
		// reported verbatim as the document's description.
		if trimmed == "---" {
			if found_content {
				break;
			}
			continue;
		}

		// Empty line handling
		if trimmed.is_empty() {
			if found_content {
				break; // Stop at first empty line after content
			}
			continue;
		}

		// Found content
		found_content = true;
		comment_lines.push(*line);

		// Limit to first paragraph (3 lines max)
		if comment_lines.len() >= 3 {
			break;
		}
	}

	if comment_lines.is_empty() {
		None
	} else {
		Some(comment_lines.join(" ").trim().to_string())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn parse(lang: &str, source: &str) -> (tree_sitter::Tree, Box<dyn languages::Language>) {
		let lang_impl = languages::get_language(lang).unwrap();
		let mut parser = Parser::new();
		parser.set_language(&lang_impl.get_ts_language()).unwrap();
		let tree = parser.parse(source, None).unwrap();
		(tree, lang_impl)
	}

	#[test]
	fn multiline_line_comment_block_captures_all_lines_in_order() {
		let source = "// line1\n// line2\n// line3\nfn foo() {}\n";
		let (tree, lang_impl) = parse("rust", source);
		let sigs = extract_signatures(tree.root_node(), source, lang_impl.as_ref());
		let foo = sigs.iter().find(|s| s.name == "foo").unwrap();
		assert_eq!(foo.description.as_deref(), Some("line1\nline2\nline3"));
	}

	#[test]
	fn distinct_comment_groups_are_not_merged() {
		// Two blank lines separate the unrelated comment from the doc block
		// that actually precedes `bar`, which is only one blank line above it.
		let source = "// unrelated comment\n\n\n// doc line1\n// doc line2\nfn bar() {}\n";
		let (tree, lang_impl) = parse("rust", source);
		let sigs = extract_signatures(tree.root_node(), source, lang_impl.as_ref());
		let bar = sigs.iter().find(|s| s.name == "bar").unwrap();
		assert_eq!(bar.description.as_deref(), Some("doc line1\ndoc line2"));
	}

	#[test]
	fn clean_comment_text_strips_block_comment_markers_per_line() {
		let input = "/**\n * This is a function.\n * It does X.\n */";
		assert_eq!(clean_comment_text(input), "This is a function.\nIt does X.");
	}

	#[test]
	fn clean_comment_text_leaves_line_comment_unchanged() {
		assert_eq!(clean_comment_text("// comment"), "comment");
	}

	#[test]
	fn block_comment_before_function_is_cleaned() {
		let source = "/**\n * This is a function.\n * It does X.\n */\nfn baz() {}\n";
		let (tree, lang_impl) = parse("rust", source);
		let sigs = extract_signatures(tree.root_node(), source, lang_impl.as_ref());
		let baz = sigs.iter().find(|s| s.name == "baz").unwrap();
		assert_eq!(
			baz.description.as_deref(),
			Some("This is a function.\nIt does X.")
		);
	}

	#[test]
	fn file_comment_chains_consecutive_leading_lines() {
		let source = "// header line1\n// header line2\nfn foo() {}\n";
		let (tree, _lang_impl) = parse("rust", source);
		let file_comment = extract_file_comment(tree.root_node(), source);
		assert_eq!(file_comment.as_deref(), Some("header line1\nheader line2"));
	}

	#[test]
	fn markdown_heading_inside_fenced_code_block_is_ignored() {
		let markdown = "# Real Heading\n\nSome text.\n\n```bash\n#!/bin/bash\necho hi\n```\n\n## Another Heading\n";
		let sigs = extract_markdown_signatures(markdown);
		let names: Vec<&str> = sigs.iter().map(|s| s.name.as_str()).collect();
		assert_eq!(names, vec!["Real Heading", "Another Heading"]);
	}

	#[test]
	fn cpp_pointer_returning_declaration_is_classified_as_function() {
		let source = "int* foo();\n";
		let (tree, lang_impl) = parse("cpp", source);
		let sigs = extract_signatures(tree.root_node(), source, lang_impl.as_ref());
		let foo = sigs.iter().find(|s| s.name == "foo").unwrap();
		assert_eq!(foo.kind, "function");
	}

	#[test]
	fn cpp_plain_variable_declaration_is_unaffected() {
		let source = "int x;\n";
		let (tree, lang_impl) = parse("cpp", source);
		let sigs = extract_signatures(tree.root_node(), source, lang_impl.as_ref());
		let x = sigs.iter().find(|s| s.name == "x").unwrap();
		assert_eq!(x.kind, "declaration");
	}
}

#[cfg(test)]
#[path = "signature_extractor_tests.rs"]
mod signature_extractor_tests;
