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

// GraphRAG utility functions

use crate::indexer::graphrag::types::CodeNode;
use anyhow::Result;
use std::path::{Path, PathBuf};

// Calculate cosine similarity between two vectors
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
	if a.len() != b.len() {
		return 0.0;
	}

	let mut dot_product = 0.0;
	let mut a_norm = 0.0;
	let mut b_norm = 0.0;

	for i in 0..a.len() {
		dot_product += a[i] * b[i];
		a_norm += a[i] * a[i];
		b_norm += b[i] * b[i];
	}

	a_norm = a_norm.sqrt();
	b_norm = b_norm.sqrt();

	if a_norm == 0.0 || b_norm == 0.0 {
		return 0.0;
	}

	dot_product / (a_norm * b_norm)
}

// Detect project root by looking for common indicators
pub fn detect_project_root() -> Result<PathBuf> {
	let current_dir = std::env::current_dir()?;
	let mut dir = current_dir.as_path();

	// Look for common project root indicators
	let indicators = [
		"Cargo.toml",
		"package.json",
		".git",
		"pyproject.toml",
		"go.mod",
		"pom.xml",
		"build.gradle",
		"composer.json",
	];

	loop {
		for indicator in &indicators {
			if dir.join(indicator).exists() {
				return Ok(dir.to_path_buf());
			}
		}

		match dir.parent() {
			Some(parent) => dir = parent,
			None => break,
		}
	}

	// Fallback to current directory if no indicators found
	Ok(current_dir)
}

// Convert absolute path to relative path from project root
pub fn to_relative_path(absolute_path: &str, project_root: &Path) -> Result<String> {
	let abs_path = PathBuf::from(absolute_path);

	// Canonicalize both paths to handle symlinks and case sensitivity issues
	let canonical_abs = abs_path.canonicalize().unwrap_or_else(|_| {
		// If canonicalization fails, try to make it absolute relative to project root
		if abs_path.is_relative() {
			project_root.join(&abs_path)
		} else {
			abs_path
		}
	});

	let canonical_root = project_root
		.canonicalize()
		.unwrap_or_else(|_| project_root.to_path_buf());

	let relative = canonical_abs.strip_prefix(&canonical_root).map_err(|_| {
		anyhow::anyhow!(
			"Path {} (canonical: {}) is not within project root {} (canonical: {})",
			absolute_path,
			canonical_abs.display(),
			project_root.display(),
			canonical_root.display()
		)
	})?;

	Ok(relative.to_string_lossy().to_string())
}

// Render GraphRAG nodes to JSON format
pub fn render_graphrag_nodes_json(nodes: &[CodeNode]) -> Result<(), anyhow::Error> {
	let json = serde_json::to_string_pretty(nodes)?;
	println!("{}", json);
	Ok(())
}

// Render GraphRAG nodes to text format (token-efficient, for MCP and CLI)
pub fn graphrag_nodes_to_text(nodes: &[CodeNode]) -> String {
	if nodes.is_empty() {
		return "No matching nodes found.".to_string();
	}

	let mut output = String::new();
	output.push_str(&format!("GRAPHRAG NODES ({} found)\n\n", nodes.len()));

	// Group nodes by file path for better organization
	let mut nodes_by_file: std::collections::HashMap<String, Vec<&CodeNode>> =
		std::collections::HashMap::new();

	for node in nodes {
		nodes_by_file
			.entry(node.path.clone())
			.or_default()
			.push(node);
	}

	// Print results organized by file
	for (file_path, file_nodes) in nodes_by_file.iter() {
		output.push_str(&format!("FILE: {}\n", file_path));

		for node in file_nodes {
			output.push_str(&format!("  {} {}\n", node.kind, node.name));
			output.push_str(&format!("  ID: {}\n", node.id));
			output.push_str(&format!("  Description: {}\n", node.description));

			if !node.symbols.is_empty() {
				output.push_str("  Symbols:\n");
				// Display symbols
				let mut display_symbols = node.symbols.clone();
				display_symbols.sort();
				display_symbols.dedup();

				for symbol in display_symbols {
					// Only show non-type symbols to users
					if !symbol.contains("_") {
						output.push_str(&format!("    - {}\n", symbol));
					}
				}
			}
			output.push('\n');
		}
		output.push('\n');
	}

	output
}

// Render GraphRAG nodes to Markdown format
pub fn graphrag_nodes_to_markdown(nodes: &[CodeNode]) -> String {
	let mut markdown = String::new();

	if nodes.is_empty() {
		markdown.push_str("No matching nodes found.");
		return markdown;
	}

	markdown.push_str(&format!("# Found {} GraphRAG nodes\n\n", nodes.len()));

	// Group nodes by file path for better organization
	let mut nodes_by_file: std::collections::HashMap<String, Vec<&CodeNode>> =
		std::collections::HashMap::new();

	for node in nodes {
		nodes_by_file
			.entry(node.path.clone())
			.or_default()
			.push(node);
	}

	// Print results organized by file
	for (file_path, file_nodes) in nodes_by_file.iter() {
		markdown.push_str(&format!("## File: {}\n\n", file_path));

		for node in file_nodes {
			markdown.push_str(&format!("### {} `{}`\n", node.kind, node.name));
			markdown.push_str(&format!("**ID:** {}  \n", node.id));
			markdown.push_str(&format!("**Description:** {}  \n", node.description));

			if !node.symbols.is_empty() {
				markdown.push_str("**Symbols:**  \n");
				// Display symbols
				let mut display_symbols = node.symbols.clone();
				display_symbols.sort();
				display_symbols.dedup();

				for symbol in display_symbols {
					// Only show non-type symbols to users
					if !symbol.contains("_") {
						markdown.push_str(&format!("- `{}`  \n", symbol));
					}
				}
			}

			markdown.push('\n');
		}

		markdown.push_str("---\n\n");
	}

	markdown
}

// Check if two symbols match (accounting for common patterns)
pub fn symbols_match(import: &str, export: &str) -> bool {
	// Direct match
	if import == export {
		return true;
	}

	// Clean symbol names (remove prefixes/suffixes)
	let clean_import = import
		.trim_start_matches("import_")
		.trim_start_matches("use_")
		.trim_start_matches("from_");
	let clean_export = export
		.trim_start_matches("export_")
		.trim_start_matches("pub_")
		.trim_start_matches("public_");

	clean_import == clean_export
}

// Check if paths have parent-child relationship
pub fn is_parent_child_relationship(path1: &str, path2: &str) -> bool {
	let path1_parts: Vec<&str> = path1.split('/').collect();
	let path2_parts: Vec<&str> = path2.split('/').collect();

	// One should be exactly one level deeper than the other
	if path1_parts.len().abs_diff(path2_parts.len()) == 1 {
		let (shorter, longer) = if path1_parts.len() < path2_parts.len() {
			(path1_parts, path2_parts)
		} else {
			(path2_parts, path1_parts)
		};

		// Check if all parts of shorter path match the beginning of longer path
		shorter.iter().zip(longer.iter()).all(|(a, b)| a == b)
	} else {
		false
	}
}
