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

//! Rust language implementation for the indexer

use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct Rust {}

impl Language for Rust {
	fn name(&self) -> &'static str {
		"rust"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_rust::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec![
			"function_item",
			"struct_item",
			"enum_item",
			// Removed: "impl_item" - can be very large, not semantic
			// Individual functions inside impl blocks will be captured separately
			"trait_item",
			"mod_item",
			"const_item",
			"macro_definition",
		]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"function_item" => {
				// Extract function name
				if let Some(name) = super::extract_symbol_by_kind(node, contents, "identifier") {
					symbols.push(name);
				}
			}
			"struct_item" | "enum_item" | "trait_item" | "mod_item" | "const_item"
			| "macro_definition" => {
				// Extract type/module/const name - can be "identifier" or contain "name"
				if let Some(name) =
					super::extract_symbol_by_kinds(node, contents, &["identifier", "name"])
				{
					symbols.push(name);
				}
			}
			_ => self.extract_identifiers(node, contents, &mut symbols),
		}

		super::deduplicate_symbols(&mut symbols);
		symbols
	}

	fn extract_identifiers(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		super::extract_identifiers_default(node, contents, symbols, |kind, _text| {
			// Include identifiers and names
			kind.contains("identifier") || kind.contains("name")
		});
	}

	fn are_node_types_equivalent(&self, type1: &str, type2: &str) -> bool {
		// Rust-specific semantic groups
		let semantic_groups = [
			// Module related
			&["mod_item", "use_declaration", "extern_crate_item"] as &[&str],
			// Type definitions
			&["struct_item", "enum_item", "union_item", "type_item"],
			// Functions
			&["function_item"],
			// Constants and statics
			&["const_item", "static_item"],
			// Traits and implementations
			&["trait_item", "impl_item"],
			// Macros
			&["macro_definition", "macro_rules"],
		];

		super::check_semantic_groups(type1, type2, &semantic_groups)
	}

	fn get_node_type_description(&self, node_type: &str) -> &'static str {
		match node_type {
			"mod_item" => "module declarations",
			"use_declaration" | "extern_crate_item" => "import statements",
			"struct_item" | "enum_item" | "union_item" => "type definitions",
			"type_item" => "type declarations",
			"function_item" => "function declarations",
			"const_item" | "static_item" => "constant declarations",
			"trait_item" => "trait declarations",
			"impl_item" => "implementation blocks",
			"macro_definition" | "macro_rules" => "macro definitions",
			_ => "declarations",
		}
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		let mut imports = Vec::new();
		let mut exports = Vec::new();

		match node.kind() {
			"use_declaration" => {
				// Extract use statement for GraphRAG import detection.
				// expand_rust_use_paths handles grouped braces, globs, and aliases.
				if let Ok(use_text) = node.utf8_text(contents.as_bytes()) {
					imports.extend(expand_rust_use_paths(use_text));
				}
			}
			"mod_item" => {
				// A mod_item with no body block is a file-level module declaration
				// like `mod foo;` or `pub mod foo;` — it creates a direct dependency
				// on src/foo.rs or src/foo/mod.rs. Emit as a self-relative import so
				// resolve_import can find the actual file.
				let has_body = node
					.children(&mut node.walk())
					.any(|c| c.kind() == "declaration_list");
				if !has_body {
					// Extract the module name identifier
					for child in node.children(&mut node.walk()) {
						if child.kind() == "identifier" {
							if let Ok(name) = child.utf8_text(contents.as_bytes()) {
								imports.push(format!("self::{}", name));
							}
							break;
						}
					}
				}
				// Also check if it's a public module (export)
				let mut cursor = node.walk();
				for child in node.children(&mut cursor) {
					if child.kind() == "visibility_modifier" {
						if let Ok(vis_text) = child.utf8_text(contents.as_bytes()) {
							if vis_text.contains("pub") {
								for name_child in node.children(&mut node.walk()) {
									if name_child.kind() == "identifier" {
										if let Ok(name) = name_child.utf8_text(contents.as_bytes())
										{
											exports.push(name.to_string());
										}
										break;
									}
								}
							}
						}
						break;
					}
				}
			}
			"function_item" | "struct_item" | "enum_item" | "trait_item" | "const_item"
			| "macro_definition" => {
				// Check if this item is public (exported)
				let mut cursor = node.walk();
				for child in node.children(&mut cursor) {
					if child.kind() == "visibility_modifier" {
						if let Ok(vis_text) = child.utf8_text(contents.as_bytes()) {
							if vis_text.contains("pub") {
								// Extract the item name as an export
								for name_child in node.children(&mut node.walk()) {
									if name_child.kind() == "identifier" {
										if let Ok(name) = name_child.utf8_text(contents.as_bytes())
										{
											exports.push(name.to_string());
											break;
										}
									}
								}
							}
						}
						break;
					}
				}
			}
			_ => {}
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
		let rust_files = registry.get_files_with_extensions(&self.get_file_extensions());

		// Handle different Rust import patterns
		if import_path.starts_with("crate::") {
			// Absolute crate path: crate::module::Item
			let module_path = import_path.strip_prefix("crate::")?;
			self.resolve_crate_import(module_path, source_file, &rust_files)
		} else if import_path.starts_with("super::") {
			// Parent module: super::module::Item
			let module_path = import_path.strip_prefix("super::")?;
			self.resolve_super_import(module_path, source_file, &rust_files)
		} else if import_path.starts_with("self::") {
			// Current module: self::module::Item
			let module_path = import_path.strip_prefix("self::")?;
			self.resolve_self_import(module_path, source_file, &rust_files)
		} else if import_path.contains("::") {
			// External crate or absolute path
			self.resolve_crate_import(import_path, source_file, &rust_files)
		} else {
			// Simple import - look for file in same directory
			self.resolve_simple_import(import_path, source_file, &rust_files)
		}
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["rs"]
	}
}

// Expand a Rust `use` statement into fully-qualified individual paths.
//
// Handles:
//   - Simple:   `use crate::config::Config;`          → ["crate::config::Config"]
//   - Grouped:  `use crate::a::{B, c::D};`            → ["crate::a::B", "crate::a::c::D"]
//   - Nested:   `use crate::a::{b::{C, D}, E};`       → ["crate::a::b::C", "crate::a::b::D", "crate::a::E"]
//   - Glob:     `use crate::utils::*;`                → ["crate::utils"]  (resolve the module itself)
//   - Alias:    `use std::sync::Arc as StdArc;`       → ["std::sync::Arc"]
fn expand_rust_use_paths(use_text: &str) -> Vec<String> {
	let cleaned = match use_text.trim().strip_prefix("use ") {
		Some(s) => s.trim_end_matches(';').trim(),
		None => return Vec::new(),
	};
	let mut results = Vec::new();
	expand_use_tree(cleaned, "", &mut results);
	results
}

// Recursively expand a use-tree fragment with the accumulated prefix.
fn expand_use_tree(fragment: &str, prefix: &str, out: &mut Vec<String>) {
	let fragment = fragment.trim();

	// Brace group: find the matching closing brace
	if let Some(brace_start) = fragment.find('{') {
		// Everything before the brace is the new prefix segment
		let new_prefix = if prefix.is_empty() {
			fragment[..brace_start].trim_end_matches(':').to_string()
		} else {
			format!(
				"{}::{}",
				prefix,
				fragment[..brace_start].trim_end_matches(':')
			)
		};

		// Find the matching closing brace (handles nesting)
		let inner = &fragment[brace_start + 1..];
		if let Some(brace_end) = find_matching_brace(inner) {
			let group = &inner[..brace_end];
			// Split group by top-level commas (not inside nested braces)
			for item in split_top_level_commas(group) {
				expand_use_tree(item.trim(), &new_prefix, out);
			}
		}
	} else {
		// No braces — leaf path segment
		// Strip alias: `Arc as StdArc` → `Arc`
		let without_alias = if let Some(pos) = fragment.find(" as ") {
			&fragment[..pos]
		} else {
			fragment
		};

		// Strip glob: `utils::*` → `utils`
		let without_glob = without_alias.trim_end_matches('*').trim_end_matches("::");

		if without_glob.is_empty() {
			return;
		}

		let full = if prefix.is_empty() {
			without_glob.to_string()
		} else {
			format!("{}::{}", prefix, without_glob)
		};

		out.push(full);
	}
}

// Find the index of the closing `}` that matches the opening `{` already consumed.
fn find_matching_brace(s: &str) -> Option<usize> {
	let mut depth = 1usize;
	for (i, c) in s.char_indices() {
		match c {
			'{' => depth += 1,
			'}' => {
				depth -= 1;
				if depth == 0 {
					return Some(i);
				}
			}
			_ => {}
		}
	}
	None
}

// Split a comma-separated list respecting nested brace groups.
fn split_top_level_commas(s: &str) -> Vec<&str> {
	let mut parts = Vec::new();
	let mut depth = 0usize;
	let mut start = 0;
	for (i, c) in s.char_indices() {
		match c {
			'{' => depth += 1,
			'}' => depth = depth.saturating_sub(1),
			',' if depth == 0 => {
				parts.push(&s[start..i]);
				start = i + 1;
			}
			_ => {}
		}
	}
	parts.push(&s[start..]);
	parts
}

impl Rust {
	/// Resolve crate-relative imports like crate::module::Item
	fn resolve_crate_import(
		&self,
		module_path: &str,
		source_file: &str,
		rust_files: &[String],
	) -> Option<String> {
		let parts: Vec<&str> = module_path.split("::").collect();
		if parts.is_empty() {
			return None;
		}

		// Find crate root
		let crate_root = self.find_crate_root(source_file, rust_files)?;
		let crate_dir = std::path::Path::new(&crate_root).parent()?;

		// ENHANCED RESOLUTION: Try all possible module path combinations with better matching
		// For crate::config::features::TechnicalIndicatorsConfig, try:
		// 1. src/config/features.rs (most common)
		// 2. src/config/features/mod.rs (module directory)
		// 3. src/config.rs (parent module)
		// Work backwards from longest to shortest path
		for end_idx in (1..=parts.len()).rev() {
			let module_parts = &parts[0..end_idx];
			let module_path_str = module_parts.join("/");

			// Try as nested file path: config/features → src/config/features.rs
			let file_path = crate_dir.join(&module_path_str).with_extension("rs");
			if let Some(resolved) = self.find_matching_file(&file_path, rust_files) {
				return Some(resolved);
			}

			// Try as module directory: config/features → src/config/features/mod.rs
			let mod_path = crate_dir.join(&module_path_str).join("mod.rs");
			if let Some(resolved) = self.find_matching_file(&mod_path, rust_files) {
				return Some(resolved);
			}
		}

		None
	}

	/// Find matching file with both exact and normalized path comparison
	fn find_matching_file(
		&self,
		target_path: &std::path::Path,
		rust_files: &[String],
	) -> Option<String> {
		let target_str = target_path.to_string_lossy().to_string();

		// Try exact string match first (fastest)
		if let Some(exact_match) = rust_files.iter().find(|f| *f == &target_str) {
			return Some(exact_match.clone());
		}

		// Try cross-platform string comparison using PathNormalizer
		if let Some(found) =
			crate::utils::path::PathNormalizer::find_path_in_collection(&target_str, rust_files)
		{
			return Some(found.to_string());
		}

		// Try normalized path comparison for cross-platform compatibility (for real files)
		if let Ok(canonical_target) = target_path.canonicalize() {
			let canonical_str = canonical_target.to_string_lossy().to_string();
			for rust_file in rust_files {
				if let Ok(canonical_rust) = std::path::Path::new(rust_file).canonicalize() {
					let canonical_rust_str = canonical_rust.to_string_lossy().to_string();
					if canonical_str == canonical_rust_str {
						return Some(rust_file.clone());
					}
				}
			}
		}

		// Try relative path matching (handle cases where paths might have different prefixes)
		if let Some(target_file_name) = target_path.file_name() {
			if let Some(target_parent) = target_path.parent() {
				for rust_file in rust_files {
					let rust_path = std::path::Path::new(rust_file);
					if let Some(rust_file_name) = rust_path.file_name() {
						if let Some(rust_parent) = rust_path.parent() {
							// Match if filename and relative parent path match
							if target_file_name == rust_file_name {
								if let (Some(target_parent_str), Some(rust_parent_str)) =
									(target_parent.to_str(), rust_parent.to_str())
								{
									// Cross-platform path comparison using PathNormalizer
									if crate::utils::path::PathNormalizer::paths_equal(
										target_parent_str,
										rust_parent_str,
									) {
										return Some(rust_file.clone());
									}
								}
							}
						}
					}
				}
			}
		}

		None
	}

	/// Resolve super:: imports (parent module)
	fn resolve_super_import(
		&self,
		module_path: &str,
		source_file: &str,
		rust_files: &[String],
	) -> Option<String> {
		let source_path = std::path::Path::new(source_file);
		let source_dir = source_path.parent()?;

		// For super::, we look in the same directory as the source file
		// This is because in Rust, super:: refers to the parent module,
		// which is typically in the same directory for flat module structures
		self.resolve_relative_import(module_path, source_dir, rust_files)
	}

	/// Resolve self:: imports (current module)
	fn resolve_self_import(
		&self,
		_module_path: &str,
		source_file: &str,
		rust_files: &[String],
	) -> Option<String> {
		// For self::item, we want to resolve to the current file
		// since self:: refers to the current module
		if rust_files.iter().any(|f| f == source_file) {
			Some(source_file.to_string())
		} else {
			None
		}
	}

	/// Resolve simple imports in same directory
	fn resolve_simple_import(
		&self,
		import_path: &str,
		source_file: &str,
		rust_files: &[String],
	) -> Option<String> {
		let source_path = std::path::Path::new(source_file);
		let source_dir = source_path.parent()?;
		let target_file = source_dir.join(format!("{}.rs", import_path));

		self.find_matching_file(&target_file, rust_files)
	}

	/// Resolve relative imports from a base directory
	fn resolve_relative_import(
		&self,
		module_path: &str,
		base_dir: &std::path::Path,
		rust_files: &[String],
	) -> Option<String> {
		let parts: Vec<&str> = module_path.split("::").collect();
		if parts.is_empty() {
			return None;
		}

		// For GraphRAG, we want to resolve to the file containing the import
		// Try different combinations of parts to find the actual file
		for end_idx in (1..=parts.len()).rev() {
			let module_parts = &parts[0..end_idx];
			let mut current_path = base_dir.to_path_buf();

			// Build path from module parts
			for (i, part) in module_parts.iter().enumerate() {
				if i == module_parts.len() - 1 {
					// Last part - try as file
					let file_path = current_path.join(format!("{}.rs", part));
					if let Some(resolved) = self.find_matching_file(&file_path, rust_files) {
						return Some(resolved);
					}

					// Try as module directory with mod.rs
					let mod_path = current_path.join(part).join("mod.rs");
					if let Some(resolved) = self.find_matching_file(&mod_path, rust_files) {
						return Some(resolved);
					}
				} else {
					current_path = current_path.join(part);
				}
			}
		}

		None
	}

	/// Find the crate root (lib.rs or main.rs) with proper path normalization
	fn find_crate_root(&self, source_file: &str, rust_files: &[String]) -> Option<String> {
		let source_path = std::path::Path::new(source_file);
		let mut current_dir = source_path.parent()?;

		// Normalize all rust_files paths for comparison
		let normalized_files: Vec<String> = rust_files
			.iter()
			.filter_map(|f| {
				std::path::Path::new(f)
					.canonicalize()
					.ok()
					.and_then(|p| p.to_str().map(|s| s.to_string()))
			})
			.collect();

		loop {
			// Look for lib.rs or main.rs with proper path normalization
			for root_file in &["lib.rs", "main.rs"] {
				let root_path = current_dir.join(root_file);

				// Try exact string match first (fastest)
				let root_path_str = root_path.to_string_lossy().to_string();
				if rust_files.iter().any(|f| f == &root_path_str) {
					return Some(root_path_str);
				}

				// Try normalized path comparison for cross-platform compatibility
				// Try normalized path comparison for cross-platform compatibility
				if let Ok(canonical_root) = root_path.canonicalize() {
					let canonical_str = canonical_root.to_string_lossy().to_string();
					if normalized_files.iter().any(|f| f == &canonical_str) {
						// Return the original path format from rust_files
						for original in rust_files {
							if let Ok(canonical_f) = std::path::Path::new(original).canonicalize() {
								if canonical_f.to_string_lossy() == canonical_str {
									return Some(original.clone());
								}
							}
						}
					}
				}
			}

			// Move up one directory
			current_dir = current_dir.parent()?;
		}
	}
}
