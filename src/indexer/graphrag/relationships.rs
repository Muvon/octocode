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

// GraphRAG relationship discovery logic

use crate::indexer::graphrag::types::{CodeNode, CodeRelationship, FunctionInfo};
use crate::indexer::graphrag::utils::is_parent_child_relationship;
use crate::store::CodeBlock;
use anyhow::Result;
use std::path::Path;

pub struct RelationshipDiscovery;

impl RelationshipDiscovery {
	// Discover relationships efficiently without AI for most cases
	pub async fn discover_relationships_efficiently(
		new_files: &[CodeNode],
		all_nodes: &[CodeNode],
	) -> Result<Vec<CodeRelationship>> {
		let mut relationships = Vec::new();

		// Pre-build lookup indexes for O(1) symbol resolution instead of O(N) scans
		let mut symbol_index: std::collections::HashMap<String, Vec<&str>> =
			std::collections::HashMap::new();
		for node in all_nodes {
			for sym in node.exports.iter().chain(node.symbols.iter()) {
				symbol_index.entry(sym.clone()).or_default().push(&node.id);
			}
		}

		// Pre-build path index for parent-child lookups
		let all_node_ids: Vec<(&str, &str)> = all_nodes
			.iter()
			.map(|n| (n.id.as_str(), n.path.as_str()))
			.collect();

		for source_file in new_files {
			// 1. Import/Export relationships via pre-built index (O(1) per import)
			for import in &source_file.imports {
				// Check both the raw import and cleaned versions
				let candidates = Self::get_import_candidates(import, &symbol_index);
				for target_id in candidates {
					if target_id != source_file.id {
						relationships.push(CodeRelationship {
							source: source_file.id.clone(),
							target: target_id.to_string(),
							relation_type: crate::indexer::graphrag::types::RelationType::Imports,
							description: format!("Imports {} from {}", import, target_id),
							confidence: 0.9,
							weight: 1.0,
						});
					}
				}
			}

			// 2. Hierarchical module relationships (high confidence)
			for &(other_id, other_path) in &all_node_ids {
				if other_id == source_file.id {
					continue;
				}

				if is_parent_child_relationship(&source_file.path, other_path) {
					let (parent, child) = if source_file.path.len() < other_path.len() {
						(source_file.id.as_str(), other_id)
					} else {
						(other_id, source_file.id.as_str())
					};

					relationships.push(CodeRelationship {
						source: parent.to_string(),
						target: child.to_string(),
						relation_type: crate::indexer::graphrag::types::RelationType::ParentModule,
						description: "Hierarchical module relationship".to_string(),
						confidence: 0.8,
						weight: 0.7,
					});
				}
			}

			// 3. Language-specific pattern relationships
			Self::discover_language_specific_relationships(
				source_file,
				all_nodes,
				&mut relationships,
			);
		}

		// Deduplicate relationships
		relationships.sort_by(|a, b| {
			(a.source.clone(), a.target.clone(), a.relation_type.clone()).cmp(&(
				b.source.clone(),
				b.target.clone(),
				b.relation_type.clone(),
			))
		});
		relationships.dedup_by(|a, b| {
			a.source == b.source && a.target == b.target && a.relation_type == b.relation_type
		});

		Ok(relationships)
	}

	/// Resolve import string against the pre-built symbol index.
	/// Returns matching node IDs (deduplicated).
	fn get_import_candidates<'a>(
		import: &str,
		symbol_index: &'a std::collections::HashMap<String, Vec<&'a str>>,
	) -> Vec<&'a str> {
		let mut results = Vec::new();

		// Direct match
		if let Some(ids) = symbol_index.get(import) {
			results.extend(ids.iter().copied());
		}

		// Try cleaned variants (strip common prefixes)
		let clean_import = import
			.trim_start_matches("import_")
			.trim_start_matches("use_")
			.trim_start_matches("from_");

		if clean_import != import {
			if let Some(ids) = symbol_index.get(clean_import) {
				results.extend(ids.iter().copied());
			}
		}

		// Deduplicate
		results.sort_unstable();
		results.dedup();
		results
	}

	// Discover language-specific relationships with import resolution
	fn discover_language_specific_relationships(
		source_file: &CodeNode,
		all_nodes: &[CodeNode],
		relationships: &mut Vec<CodeRelationship>,
	) {
		// First, resolve imports to create semantic relationships
		Self::discover_import_relationships(source_file, all_nodes, relationships);

		// Then add language-specific patterns as fallback
		match source_file.language.as_str() {
			"rust" => {
				Self::discover_rust_relationships(source_file, all_nodes, relationships);
			}
			"javascript" | "typescript" => {
				Self::discover_js_ts_relationships(source_file, all_nodes, relationships);
			}
			"python" => {
				Self::discover_python_relationships(source_file, all_nodes, relationships);
			}
			"go" => {
				Self::discover_go_relationships(source_file, all_nodes, relationships);
			}
			"php" => {
				Self::discover_php_relationships(source_file, all_nodes, relationships);
			}
			_ => {
				// Generic patterns for other languages
			}
		}
	}

	// Discover semantic relationships through import resolution
	pub fn discover_import_relationships(
		source_file: &CodeNode,
		all_nodes: &[CodeNode],
		relationships: &mut Vec<CodeRelationship>,
	) {
		// Create a map for quick file lookup by path
		let file_map: std::collections::HashMap<String, &CodeNode> = all_nodes
			.iter()
			.map(|node| (node.path.clone(), node))
			.collect();

		// Get all file paths for resolution
		let all_files: Vec<String> = all_nodes.iter().map(|node| node.path.clone()).collect();

		// Get language implementation for import resolution
		if let Some(lang_impl) = crate::indexer::languages::get_language(&source_file.language) {
			// Resolve each import to create direct relationships
			for import_path in &source_file.imports {
				if let Some(resolved_path) =
					lang_impl.resolve_import(import_path, &source_file.path, &all_files)
				{
					// Find the target node
					if let Some(target_node) = file_map.get(&resolved_path) {
						// Create semantic import relationship
						relationships.push(CodeRelationship {
							source: source_file.id.clone(),
							target: target_node.id.clone(),
							relation_type: crate::indexer::graphrag::types::RelationType::Imports,
							description: format!(
								"Direct import: {} -> {}",
								import_path, resolved_path
							),
							confidence: 0.95, // High confidence for resolved imports
							weight: 1.0,
						});

						// Create reverse export relationship if target exports to source
						for export_item in &target_node.exports {
							if import_path.contains(export_item) || export_item == "*" {
								relationships.push(CodeRelationship {
									source: target_node.id.clone(),
									target: source_file.id.clone(),
									relation_type:
										crate::indexer::graphrag::types::RelationType::Imports,
									description: format!(
										"Exports {} to {}",
										export_item, source_file.path
									),
									confidence: 0.9,
									weight: 0.8,
								});
							}
						}
					}
				}
			}
		}
	}

	// Rust-specific relationship patterns
	fn discover_rust_relationships(
		source_file: &CodeNode,
		all_nodes: &[CodeNode],
		relationships: &mut Vec<CodeRelationship>,
	) {
		for other_file in all_nodes {
			if other_file.id == source_file.id || other_file.language != "rust" {
				continue;
			}

			// Check for mod.rs patterns
			if source_file.name == "mod"
				&& other_file
					.path
					.starts_with(&source_file.path.replace("/mod.rs", "/"))
			{
				relationships.push(CodeRelationship {
					source: source_file.id.clone(),
					target: other_file.id.clone(),
					relation_type: crate::indexer::graphrag::types::RelationType::ParentModule,
					description: "Rust module declaration".to_string(),
					confidence: 0.8,
					weight: 0.8,
				});
			}

			// Check for lib.rs patterns
			if source_file.name == "lib" || source_file.name == "main" {
				let source_dir = Path::new(&source_file.path)
					.parent()
					.map(|p| p.to_string_lossy().to_string())
					.unwrap_or_default();
				if other_file.path.starts_with(&source_dir) {
					relationships.push(CodeRelationship {
						source: source_file.id.clone(),
						target: other_file.id.clone(),
						relation_type: crate::indexer::graphrag::types::RelationType::ParentModule,
						description: "Rust crate root relationship".to_string(),
						confidence: 0.7,
						weight: 0.6,
					});
				}
			}
		}
	}

	// JavaScript/TypeScript-specific relationship patterns
	fn discover_js_ts_relationships(
		source_file: &CodeNode,
		all_nodes: &[CodeNode],
		relationships: &mut Vec<CodeRelationship>,
	) {
		for other_file in all_nodes {
			if other_file.id == source_file.id
				|| !["javascript", "typescript"].contains(&other_file.language.as_str())
			{
				continue;
			}

			// Check for index.js patterns
			if source_file.name == "index" {
				let source_dir = Path::new(&source_file.path)
					.parent()
					.map(|p| p.to_string_lossy().to_string())
					.unwrap_or_default();
				if other_file.path.starts_with(&source_dir) && other_file.name != "index" {
					relationships.push(CodeRelationship {
						source: source_file.id.clone(),
						target: other_file.id.clone(),
						relation_type: crate::indexer::graphrag::types::RelationType::ParentModule,
						description: "JavaScript index module relationship".to_string(),
						confidence: 0.7,
						weight: 0.6,
					});
				}
			}
		}
	}

	// Python-specific relationship patterns
	fn discover_python_relationships(
		source_file: &CodeNode,
		all_nodes: &[CodeNode],
		relationships: &mut Vec<CodeRelationship>,
	) {
		for other_file in all_nodes {
			if other_file.id == source_file.id || other_file.language != "python" {
				continue;
			}

			// Check for __init__.py patterns
			if source_file.name == "__init__" {
				let source_dir = Path::new(&source_file.path)
					.parent()
					.map(|p| p.to_string_lossy().to_string())
					.unwrap_or_default();
				if other_file.path.starts_with(&source_dir) && other_file.name != "__init__" {
					relationships.push(CodeRelationship {
						source: source_file.id.clone(),
						target: other_file.id.clone(),
						relation_type: crate::indexer::graphrag::types::RelationType::ParentModule,
						description: "Python package initialization".to_string(),
						confidence: 0.8,
						weight: 0.7,
					});
				}
			}
		}
	}
	// Go-specific relationship patterns
	fn discover_go_relationships(
		source_file: &CodeNode,
		all_nodes: &[CodeNode],
		relationships: &mut Vec<CodeRelationship>,
	) {
		for other_file in all_nodes {
			if other_file.id == source_file.id || other_file.language != "go" {
				continue;
			}

			// Check for package relationships
			let source_package = Self::extract_go_package(&source_file.path);
			let other_package = Self::extract_go_package(&other_file.path);

			if source_package == other_package && !source_package.is_empty() {
				relationships.push(CodeRelationship {
					source: source_file.id.clone(),
					target: other_file.id.clone(),
					relation_type: crate::indexer::graphrag::types::RelationType::SiblingModule,
					description: format!("Go package relationship: {}", source_package),
					confidence: 0.8,
					weight: 0.7,
				});
			}
		}
	}

	// PHP-specific relationship patterns
	fn discover_php_relationships(
		source_file: &CodeNode,
		all_nodes: &[CodeNode],
		relationships: &mut Vec<CodeRelationship>,
	) {
		for other_file in all_nodes {
			if other_file.id == source_file.id || other_file.language != "php" {
				continue;
			}

			// Check for namespace relationships
			let source_namespace = Self::extract_php_namespace(&source_file.path);
			let other_namespace = Self::extract_php_namespace(&other_file.path);

			if source_namespace == other_namespace && !source_namespace.is_empty() {
				relationships.push(CodeRelationship {
					source: source_file.id.clone(),
					target: other_file.id.clone(),
					relation_type: crate::indexer::graphrag::types::RelationType::SiblingModule,
					description: format!("PHP namespace relationship: {}", source_namespace),
					confidence: 0.8,
					weight: 0.7,
				});
			}
		}
	}

	// Helper methods for language-specific patterns

	fn extract_go_package(file_path: &str) -> String {
		if let Some(parent) = Path::new(file_path).parent() {
			if let Some(package_name) = parent.file_name() {
				return package_name.to_string_lossy().to_string();
			}
		}
		String::new()
	}

	fn extract_php_namespace(file_path: &str) -> String {
		// Extract namespace from file path structure
		let path = Path::new(file_path);
		if let Some(parent) = path.parent() {
			// Convert path to namespace-like structure
			parent.to_string_lossy().replace('/', "\\")
		} else {
			String::new()
		}
	}

	// Extract function information from a code block efficiently
	pub fn extract_functions_from_block(block: &CodeBlock) -> Result<Vec<FunctionInfo>> {
		let mut functions = Vec::new();

		// Look for function patterns in symbols
		for symbol in &block.symbols {
			if symbol.contains("function_") || symbol.contains("method_") {
				// Parse the symbol to extract function info
				if let Some(function_info) = Self::parse_function_symbol(symbol, block) {
					functions.push(function_info);
				}
			}
		}

		Ok(functions)
	}

	// Parse function symbol to create FunctionInfo
	fn parse_function_symbol(symbol: &str, block: &CodeBlock) -> Option<FunctionInfo> {
		// Simple pattern matching for common function symbol formats
		// This can be expanded based on your language implementations

		symbol
			.strip_prefix("function_")
			.map(|function_name| FunctionInfo {
				name: function_name.to_string(),
				signature: format!("{}(...)", function_name), // Simplified
				start_line: block.start_line as u32,
				end_line: block.end_line as u32,
				calls: Vec::new(), // Will be populated during relationship discovery
				called_by: Vec::new(),
				parameters: Vec::new(), // Could be extracted from content if needed
				return_type: None,
			})
	}

	// Extract imports/exports efficiently based on language patterns and symbols
	pub fn extract_imports_exports_efficient(
		symbols: &[String],
		_language: &str,
		_relative_path: &str,
	) -> (Vec<String>, Vec<String>) {
		// This function is now deprecated in favor of language-specific extraction
		// during AST parsing. For backward compatibility, treat all symbols as exports
		let mut exports = Vec::new();

		for symbol in symbols {
			if !symbol.is_empty() && !symbol.starts_with("IMPORT:") {
				exports.push(symbol.clone());
			}
		}

		// Return empty imports since real import extraction happens at AST level
		(Vec::new(), exports)
	}
	// Determine file kind based on path patterns
	// Determine file kind based on path patterns
	pub fn determine_file_kind(relative_path: &str) -> String {
		if relative_path.contains("/src/") || relative_path.contains("/lib/") {
			"source_file".to_string()
		} else if relative_path.contains("/test")
			|| relative_path.contains("_test.")
			|| relative_path.contains(".test.")
		{
			"test_file".to_string()
		} else if relative_path.ends_with(".md")
			|| relative_path.ends_with(".txt")
			|| relative_path.ends_with(".rst")
		{
			"documentation".to_string()
		} else if relative_path.contains("/config") || relative_path.contains(".config") {
			"config_file".to_string()
		} else if relative_path.contains("/examples") || relative_path.contains("/demo") {
			"example_file".to_string()
		} else {
			"file".to_string()
		}
	}

	// Generate simple description without AI for speed (fallback and default)
	pub fn generate_simple_description(
		file_name: &str,
		language: &str,
		symbols: &[String],
		lines: u32,
	) -> String {
		let function_count = symbols
			.iter()
			.filter(|s| s.contains("function_") || s.contains("method_"))
			.count();
		let class_count = symbols
			.iter()
			.filter(|s| s.contains("class_") || s.contains("struct_"))
			.count();

		if function_count > 0 && class_count > 0 {
			format!(
				"{} {} file with {} functions and {} classes ({} lines)",
				file_name, language, function_count, class_count, lines
			)
		} else if function_count > 0 {
			format!(
				"{} {} file with {} functions ({} lines)",
				file_name, language, function_count, lines
			)
		} else if class_count > 0 {
			format!(
				"{} {} file with {} classes ({} lines)",
				file_name, language, class_count, lines
			)
		} else {
			format!("{} {} file ({} lines)", file_name, language, lines)
		}
	}
}
