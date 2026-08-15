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

// Live structural graph extraction: one node per declared symbol (function,
// class, struct, trait, ...), derived purely from tree-sitter with zero LLM.
// Also hosts the unified single-pass AST extractor that replaced the three
// separate per-file parses (imports/exports, call sites, type relations).

use crate::indexer::graphrag::types::{CodeNode, CodeRelationship, Provenance, RelationType};
use crate::indexer::languages::{self, Language, TypeRelationKind};
use anyhow::{Context, Result};
use std::collections::HashMap;

/// A declared symbol extracted from one tree-sitter node.
#[derive(Debug, Clone)]
pub struct SymbolDecl {
	pub name: String,
	/// Coarse symbol kind: "function", "class", "struct", "trait", "interface",
	/// "enum", "module", "macro", "const", "type", or "symbol".
	pub kind: String,
	/// 0-based start line of the declaration node.
	pub start_line: u32,
	/// 0-based end line of the declaration node.
	pub end_line: u32,
}

#[derive(Debug, Clone)]
pub struct TypeRelationDecl {
	pub line: u32,
	pub source_name: Option<String>,
	pub kind: TypeRelationKind,
	pub target_name: String,
}

/// Everything the graph builder needs from ONE tree-sitter parse of a file.
#[derive(Debug, Clone, Default)]
pub struct FileAstData {
	pub imports: Vec<String>,
	pub exports: Vec<String>,
	/// (0-based line, callee name) pairs.
	pub calls: Vec<(u32, String)>,
	pub type_relations: Vec<TypeRelationDecl>,
	/// Declared symbols with line ranges, in source order.
	pub symbols: Vec<SymbolDecl>,
}

/// AST-derived relationship input for one source file. Source ranges stay
/// available until edge discovery so calls can be attributed to their owning
/// declaration without LLM assistance or a second heuristic metadata format.
#[derive(Debug, Clone)]
pub struct SymbolFileData {
	pub path: String,
	pub language: String,
	pub imports: Vec<String>,
	pub calls: Vec<(u32, String)>,
	pub type_relations: Vec<TypeRelationDecl>,
	pub symbols: Vec<SymbolDecl>,
}

impl SymbolFileData {
	pub fn from_ast(path: String, language: String, data: &FileAstData) -> Self {
		Self {
			path,
			language,
			imports: data.imports.clone(),
			calls: data.calls.clone(),
			type_relations: data.type_relations.clone(),
			symbols: data.symbols.clone(),
		}
	}
}

/// Generate symbol ids for one file. The common case keeps the concise
/// `{path}::{name}` form; only same-file name collisions receive a source-line
/// suffix so methods with the same name never overwrite each other.
pub fn symbol_node_ids(path: &str, symbols: &[SymbolDecl]) -> Vec<String> {
	let mut counts: HashMap<&str, usize> = HashMap::new();
	for symbol in symbols {
		*counts.entry(symbol.name.as_str()).or_default() += 1;
	}

	symbols
		.iter()
		.map(|symbol| {
			if counts.get(symbol.name.as_str()).copied().unwrap_or(0) > 1 {
				format!("{}::{}@{}", path, symbol.name, symbol.start_line + 1)
			} else {
				format!("{}::{}", path, symbol.name)
			}
		})
		.collect()
}

/// Map a tree-sitter node kind to a coarse symbol kind for the `kind` column.
pub fn symbol_kind_from_node_kind(kind: &str) -> &'static str {
	if kind.contains("function")
		|| kind.contains("method")
		|| kind.contains("procedure")
		|| kind.contains("constructor")
	{
		"function"
	} else if kind.contains("interface") {
		"interface"
	} else if kind.contains("class") {
		"class"
	} else if kind.contains("struct") {
		"struct"
	} else if kind.contains("trait") {
		"trait"
	} else if kind.contains("enum") {
		"enum"
	} else if kind.contains("mod")
		|| kind.contains("module")
		|| kind.contains("namespace")
		|| kind.contains("package")
	{
		"module"
	} else if kind.contains("macro") {
		"macro"
	} else if kind.contains("const") || kind.contains("variable") {
		"const"
	} else if kind.contains("type") {
		"type"
	} else {
		"symbol"
	}
}

/// Unified single-pass AST extraction: imports, exports, call sites, type
/// relations, and symbol declarations from ONE tree-sitter parse. Replaces
/// the three separate parses (one per extractor) that ran per file.
pub fn extract_symbols_from_file(file_path: &str, language: &str) -> Result<FileAstData> {
	let lang_impl = languages::get_language(language).ok_or_else(|| {
		anyhow::anyhow!("Failed to get language implementation for: {}", language)
	})?;

	let contents = std::fs::read_to_string(file_path)
		.with_context(|| format!("Failed to read file for AST extraction: {}", file_path))?;

	let mut parser = tree_sitter::Parser::new();
	parser.set_language(&lang_impl.get_ts_language())?;
	let tree = parser
		.parse(&contents, None)
		.ok_or_else(|| anyhow::anyhow!("Failed to parse file: {}", file_path))?;

	let symbol_kinds = lang_impl.get_symbol_kinds();
	let mut data = FileAstData::default();
	walk_ast(
		tree.root_node(),
		&contents,
		lang_impl.as_ref(),
		&symbol_kinds,
		&mut data,
	);
	Ok(data)
}

/// Single DFS over the AST collecting every extractor's output per node.
/// Visits the same nodes in the same order as the three separate walks it
/// replaced, so the aggregated per-category output is identical.
fn walk_ast(
	node: tree_sitter::Node,
	contents: &str,
	lang_impl: &dyn Language,
	symbol_kinds: &[&str],
	data: &mut FileAstData,
) {
	let (imports, exports) = lang_impl.extract_imports_exports(node, contents);
	data.imports.extend(imports);
	data.exports.extend(exports);

	let line = node.start_position().row as u32; // 0-based, matches FunctionInfo
	for callee in lang_impl.extract_function_calls(node, contents) {
		data.calls.push((line, callee));
	}
	let type_relations = lang_impl.extract_type_relations(node, contents);
	if !type_relations.is_empty() {
		let source_name = lang_impl.extract_type_relation_source(node, contents);
		for (kind, target_name) in type_relations {
			data.type_relations.push(TypeRelationDecl {
				line,
				source_name: source_name.clone(),
				kind,
				target_name,
			});
		}
	}

	if symbol_kinds.contains(&node.kind()) {
		if let Some(name) = lang_impl.extract_declaration_name(node, contents) {
			let name = name.trim().to_string();
			if !name.is_empty() {
				data.symbols.push(SymbolDecl {
					kind: symbol_kind_from_node_kind(node.kind()).to_string(),
					name,
					start_line: node.start_position().row as u32,
					end_line: node.end_position().row as u32,
				});
			}
		}
	}

	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		walk_ast(child, contents, lang_impl, symbol_kinds, data);
	}
}

/// Symbol lookup structures shared by symbol-edge discovery.
struct SymbolIndex<'a> {
	/// Symbol name → symbol node ids declaring it (project-wide).
	by_name: HashMap<&'a str, Vec<&'a str>>,
	/// Owning file path → (symbol name → symbol node ids).
	by_file: HashMap<&'a str, HashMap<&'a str, Vec<&'a str>>>,
}

impl<'a> SymbolIndex<'a> {
	fn build(all_nodes: &'a [CodeNode]) -> Self {
		let mut index = SymbolIndex {
			by_name: HashMap::new(),
			by_file: HashMap::new(),
		};
		for node in all_nodes {
			if !node.is_symbol_node() || node.language == "markdown" {
				continue;
			}
			index
				.by_name
				.entry(node.name.as_str())
				.or_default()
				.push(node.id.as_str());
			index
				.by_file
				.entry(node.path.as_str())
				.or_default()
				.entry(node.name.as_str())
				.or_default()
				.push(node.id.as_str());
		}
		index
	}

	/// Resolve a referenced name to symbol node ids, scoped: same file first,
	/// then files the source imports, then a unique global declaration.
	/// Returns (target id, provenance, confidence) triples.
	fn resolve(
		&self,
		source_path: &str,
		target_name: &str,
		imported_files: &[String],
		allow_ambiguous: bool,
	) -> Vec<(&'a str, Provenance, f32)> {
		// 1. Same file — direct AST fact.
		if let Some(targets) = self
			.by_file
			.get(source_path)
			.and_then(|m| m.get(target_name))
		{
			if targets.len() == 1 {
				return vec![(targets[0], Provenance::Extracted, 0.9)];
			}
			if allow_ambiguous {
				return targets
					.iter()
					.map(|id| (*id, Provenance::Ambiguous, 0.4))
					.collect();
			}
			return Vec::new();
		}

		// 2. A file the source imports declares the target.
		for imported in imported_files {
			if let Some(targets) = self
				.by_file
				.get(imported.as_str())
				.and_then(|m| m.get(target_name))
			{
				if targets.len() == 1 {
					return vec![(targets[0], Provenance::Extracted, 0.85)];
				}
			}
		}

		// 3. Unique global declaration.
		match self.by_name.get(target_name) {
			Some(ids) if ids.len() == 1 => vec![(ids[0], Provenance::Inferred, 0.6)],
			// Multiple candidates: only type relations (extends/implements)
			// record them — call sites hit ubiquitous method names (`new`,
			// `get`) and would balloon the edge set with noise.
			Some(ids) if allow_ambiguous => ids
				.iter()
				.map(|id| (&**id, Provenance::Ambiguous, 0.4))
				.collect(),
			_ => Vec::new(),
		}
	}
}

/// Discover symbol→symbol edges (calls / extends / implements) for the given
/// source files against the full graph. Pure tree-sitter + name resolution —
/// no LLM. File-level edges are produced separately by `relationships.rs`.
pub fn discover_symbol_relationships(
	new_files: &[SymbolFileData],
	all_nodes: &[CodeNode],
) -> Vec<CodeRelationship> {
	let index = SymbolIndex::build(all_nodes);
	let all_files: Vec<String> = all_nodes
		.iter()
		.filter(|n| !n.is_symbol_node())
		.map(|n| n.path.clone())
		.collect();

	let mut relationships = Vec::new();

	for source_file in new_files {
		if source_file.language == "markdown" {
			continue;
		}
		let Some(lang_impl) = languages::get_language(&source_file.language) else {
			continue;
		};

		// Resolve each import to a concrete project file once per source file.
		let imported_files: Vec<String> = source_file
			.imports
			.iter()
			.filter_map(|imp| lang_impl.resolve_import(imp, &source_file.path, &all_files))
			.collect();

		let symbol_ids = symbol_node_ids(&source_file.path, &source_file.symbols);
		let owning_symbol = |line: u32, functions_only: bool| {
			source_file
				.symbols
				.iter()
				.enumerate()
				.filter(|(_, symbol)| {
					line >= symbol.start_line
						&& line <= symbol.end_line
						&& (!functions_only || symbol.kind == "function")
				})
				.min_by_key(|(_, symbol)| symbol.end_line.saturating_sub(symbol.start_line))
				.map(|(index, symbol)| (symbol_ids[index].as_str(), symbol.name.as_str()))
		};

		for (line, callee) in &source_file.calls {
			if let Some((source_id, source_name)) = owning_symbol(*line, true) {
				for (target, provenance, confidence) in
					index.resolve(&source_file.path, callee, &imported_files, false)
				{
					relationships.push(CodeRelationship {
						source: source_id.to_string(),
						target: target.to_string(),
						relation_type: RelationType::Calls,
						description: format!("{} calls {}", source_name, callee),
						confidence,
						weight: 0.8,
						provenance,
					});
				}
			}
		}

		for relation in &source_file.type_relations {
			let owner = owning_symbol(relation.line, false).or_else(|| {
				relation.source_name.as_deref().and_then(|name| {
					source_file
						.symbols
						.iter()
						.enumerate()
						.filter(|(_, symbol)| symbol.name == name)
						.min_by_key(|(_, symbol)| symbol.end_line.saturating_sub(symbol.start_line))
						.map(|(index, symbol)| (symbol_ids[index].as_str(), symbol.name.as_str()))
				})
			});
			if let Some((source_id, source_name)) = owner {
				let relation_type = match relation.kind {
					TypeRelationKind::Extends => RelationType::Extends,
					TypeRelationKind::Implements => RelationType::Implements,
				};
				for (target, provenance, confidence) in index.resolve(
					&source_file.path,
					&relation.target_name,
					&imported_files,
					true,
				) {
					relationships.push(CodeRelationship {
						source: source_id.to_string(),
						target: target.to_string(),
						relation_type: relation_type.clone(),
						description: format!(
							"{} {} {}",
							source_name,
							relation_type.as_str(),
							relation.target_name
						),
						confidence,
						weight: 1.0,
						provenance,
					});
				}
			}
		}
	}

	relationships.sort_unstable_by(|a, b| {
		(&a.source, &a.target, &a.relation_type).cmp(&(&b.source, &b.target, &b.relation_type))
	});
	relationships.dedup_by(|a, b| {
		a.source == b.source && a.target == b.target && a.relation_type == b.relation_type
	});

	relationships
}

#[cfg(test)]
mod tests {
	use super::*;

	fn declaration(name: &str, kind: &str, start_line: u32, end_line: u32) -> SymbolDecl {
		SymbolDecl {
			name: name.to_string(),
			kind: kind.to_string(),
			start_line,
			end_line,
		}
	}

	fn file_node(path: &str) -> CodeNode {
		CodeNode {
			id: path.to_string(),
			name: path.to_string(),
			kind: "source_file".to_string(),
			path: path.to_string(),
			description: String::new(),
			symbols: Vec::new(),
			hash: String::new(),
			embedding: Vec::new(),
			imports: Vec::new(),
			exports: Vec::new(),
			functions: Vec::new(),
			size_lines: 0,
			language: "rust".to_string(),
		}
	}

	fn symbol_node(path: &str, name: &str) -> CodeNode {
		CodeNode {
			id: format!("{}::{}", path, name),
			name: name.to_string(),
			kind: "function".to_string(),
			path: path.to_string(),
			description: String::new(),
			symbols: Vec::new(),
			hash: String::new(),
			embedding: Vec::new(),
			imports: Vec::new(),
			exports: Vec::new(),
			functions: Vec::new(),
			size_lines: 0,
			language: "rust".to_string(),
		}
	}

	#[test]
	fn test_symbol_kind_from_node_kind() {
		assert_eq!(symbol_kind_from_node_kind("function_item"), "function");
		assert_eq!(symbol_kind_from_node_kind("method_definition"), "function");
		assert_eq!(symbol_kind_from_node_kind("class_declaration"), "class");
		assert_eq!(symbol_kind_from_node_kind("struct_item"), "struct");
		assert_eq!(symbol_kind_from_node_kind("trait_item"), "trait");
		assert_eq!(
			symbol_kind_from_node_kind("interface_declaration"),
			"interface"
		);
		assert_eq!(symbol_kind_from_node_kind("enum_item"), "enum");
		assert_eq!(symbol_kind_from_node_kind("mod_item"), "module");
		assert_eq!(symbol_kind_from_node_kind("const_item"), "const");
		assert_eq!(symbol_kind_from_node_kind("macro_definition"), "macro");
		assert_eq!(symbol_kind_from_node_kind("type_item"), "type");
		assert_eq!(symbol_kind_from_node_kind("whatever"), "symbol");
	}

	#[test]
	fn test_extract_symbols_from_file_rust() {
		let path =
			std::env::temp_dir().join(format!("octocode_symbols_test_{}.rs", std::process::id()));
		let code = r#"use std::collections::HashMap;

pub struct Config {
    pub name: String,
}

pub trait Named {}

impl Named for Config {}

impl Config {
    pub fn new() -> Self {
        Config { name: String::new() }
    }
}

fn helper() -> u32 {
    42
}

pub fn main_fn() {
    let v = helper();
    let m = HashMap::new();
    println!("{}", v);
}
"#;
		std::fs::write(&path, code).unwrap();

		let data = extract_symbols_from_file(path.to_str().unwrap(), "rust").unwrap();
		std::fs::remove_file(&path).unwrap();

		// Imports extracted
		assert!(
			data.imports
				.iter()
				.any(|i| i == "std::collections::HashMap"),
			"imports: {:?}",
			data.imports
		);

		// Symbol declarations with kinds and ranges
		let find = |name: &str| data.symbols.iter().find(|s| s.name == name);
		let config = find("Config").expect("Config symbol");
		assert_eq!(config.kind, "struct");
		let new = find("new").expect("new symbol");
		assert_eq!(new.kind, "function");
		let helper = find("helper").expect("helper symbol");
		assert_eq!(helper.kind, "function");
		let main_fn = find("main_fn").expect("main_fn symbol");
		assert_eq!(main_fn.kind, "function");
		assert!(helper.start_line < main_fn.start_line);
		assert!(main_fn.end_line >= main_fn.start_line);

		// Call sites extracted with lines
		assert!(
			data.calls.iter().any(|(_, c)| c == "helper"),
			"calls: {:?}",
			data.calls
		);

		let implementation = data
			.type_relations
			.iter()
			.find(|relation| relation.kind == TypeRelationKind::Implements)
			.expect("Config implements Named relation");
		assert_eq!(implementation.source_name.as_deref(), Some("Config"));
		assert_eq!(implementation.target_name, "Named");
	}

	#[test]
	fn duplicate_symbol_names_receive_distinct_stable_ids() {
		let symbols = vec![
			declaration("new", "function", 4, 8),
			declaration("new", "function", 20, 24),
			declaration("run", "function", 30, 35),
		];
		assert_eq!(
			symbol_node_ids("src/lib.rs", &symbols),
			vec!["src/lib.rs::new@5", "src/lib.rs::new@21", "src/lib.rs::run"]
		);
	}

	#[test]
	fn test_discover_symbol_relationships() {
		// Files: a.rs (run calls helper + unique_fn + ambiguous `new`),
		// b.rs and c.rs both declare `new` and `Base`.
		let a = SymbolFileData {
			path: "src/a.rs".to_string(),
			language: "rust".to_string(),
			imports: Vec::new(),
			calls: ["helper", "unique_fn", "new"]
				.into_iter()
				.map(|callee| (5, callee.to_string()))
				.collect(),
			type_relations: vec![TypeRelationDecl {
				line: 5,
				source_name: Some("run".to_string()),
				kind: TypeRelationKind::Extends,
				target_name: "Base".to_string(),
			}],
			symbols: vec![
				declaration("run", "function", 0, 10),
				declaration("helper", "function", 20, 25),
			],
		};
		let a_file = file_node("src/a.rs");
		let b = file_node("src/b.rs");
		let c = file_node("src/c.rs");

		let all_nodes = vec![
			a_file,
			b,
			c,
			symbol_node("src/a.rs", "run"),
			symbol_node("src/a.rs", "helper"),
			symbol_node("src/b.rs", "unique_fn"),
			symbol_node("src/b.rs", "new"),
			symbol_node("src/c.rs", "new"),
			symbol_node("src/b.rs", "Base"),
			symbol_node("src/c.rs", "Base"),
		];

		let edges = discover_symbol_relationships(&[a], &all_nodes);

		let find = |source: &str, target: &str, rel: RelationType| {
			edges
				.iter()
				.find(|e| e.source == source && e.target == target && e.relation_type == rel)
		};

		// Same-file call → Extracted
		let same_file = find("src/a.rs::run", "src/a.rs::helper", RelationType::Calls)
			.expect("same-file call edge");
		assert_eq!(same_file.provenance, Provenance::Extracted);

		// Unique global call → Inferred
		let global = find("src/a.rs::run", "src/b.rs::unique_fn", RelationType::Calls)
			.expect("global-unique call edge");
		assert_eq!(global.provenance, Provenance::Inferred);

		// Ambiguous call (`new` in b.rs and c.rs) → no edge
		assert!(find("src/a.rs::run", "src/b.rs::new", RelationType::Calls).is_none());
		assert!(find("src/a.rs::run", "src/c.rs::new", RelationType::Calls).is_none());

		// Ambiguous extends (`Base` in b.rs and c.rs) → Ambiguous edges to both
		let amb_b = find("src/a.rs::run", "src/b.rs::Base", RelationType::Extends)
			.expect("ambiguous extends edge to b");
		assert_eq!(amb_b.provenance, Provenance::Ambiguous);
		let amb_c = find("src/a.rs::run", "src/c.rs::Base", RelationType::Extends)
			.expect("ambiguous extends edge to c");
		assert_eq!(amb_c.provenance, Provenance::Ambiguous);
	}
}
