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

use crate::indexer::graphrag::types::{CodeRelationship, Provenance, RelationType};
use crate::indexer::languages::{self, CallTarget, Language, TypeRelationKind};
use anyhow::{Context, Result};
use std::collections::HashMap;

/// A declared symbol extracted from one tree-sitter node.
#[derive(Debug, Clone)]
pub struct SymbolDecl {
	pub name: String,
	/// Nearest syntactic type/module that owns this declaration.
	pub owner: Option<String>,
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

#[derive(Debug, Clone)]
pub struct ImportBinding {
	/// Local identifier used at call sites.
	pub local_name: String,
	/// Original identifier in the imported file; None means a module namespace.
	pub imported_name: Option<String>,
	/// Import path consumed by the language's existing resolver.
	pub import_path: String,
}

/// Everything the graph builder needs from ONE tree-sitter parse of a file.
#[derive(Debug, Clone, Default)]
pub struct FileAstData {
	pub imports: Vec<String>,
	pub import_bindings: Vec<ImportBinding>,
	pub exports: Vec<String>,
	/// (0-based line, structured callee) pairs.
	pub calls: Vec<(u32, CallTarget)>,
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
	pub import_bindings: Vec<ImportBinding>,
	pub calls: Vec<(u32, CallTarget)>,
	pub type_relations: Vec<TypeRelationDecl>,
	pub symbols: Vec<SymbolDecl>,
}

impl SymbolFileData {
	pub fn from_ast(path: String, language: String, data: &FileAstData) -> Self {
		Self {
			path,
			language,
			imports: data.imports.clone(),
			import_bindings: data.import_bindings.clone(),
			calls: data.calls.clone(),
			type_relations: data.type_relations.clone(),
			symbols: data.symbols.clone(),
		}
	}
}

/// Generate stable, human-readable symbol ids for one file. Owned methods use
/// `{path}::{owner}::{name}` so an LLM can distinguish `Service::run` from
/// `Worker::run`; only true collisions (such as overloads on the same owner)
/// receive a source-line suffix.
pub fn symbol_node_ids(path: &str, symbols: &[SymbolDecl]) -> Vec<String> {
	let mut counts: HashMap<(Option<&str>, &str), usize> = HashMap::new();
	for symbol in symbols {
		*counts
			.entry((symbol.owner.as_deref(), symbol.name.as_str()))
			.or_default() += 1;
	}

	symbols
		.iter()
		.map(|symbol| {
			let base = if let Some(owner) = &symbol.owner {
				format!("{}::{}::{}", path, owner, symbol.name)
			} else {
				format!("{}::{}", path, symbol.name)
			};
			if counts
				.get(&(symbol.owner.as_deref(), symbol.name.as_str()))
				.copied()
				.unwrap_or(0)
				> 1
			{
				format!("{}@{}", base, symbol.start_line + 1)
			} else {
				base
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
		// starts_with, not contains: "definition" contains "init".
		|| kind.starts_with("init")
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
	let root = tree.root_node();
	walk_ast(
		root,
		&contents,
		lang_impl.as_ref(),
		&symbol_kinds,
		&mut data,
	);

	for embedded in lang_impl.extract_embedded_sources(root, &contents) {
		let Some(embedded_impl) = languages::get_language(embedded.language) else {
			continue;
		};
		let mut embedded_parser = tree_sitter::Parser::new();
		embedded_parser.set_language(&embedded_impl.get_ts_language())?;
		let Some(embedded_tree) = embedded_parser.parse(&embedded.contents, None) else {
			continue;
		};
		let mut embedded_data = FileAstData::default();
		walk_ast(
			embedded_tree.root_node(),
			&embedded.contents,
			embedded_impl.as_ref(),
			&embedded_impl.get_symbol_kinds(),
			&mut embedded_data,
		);
		for (line, _) in &mut embedded_data.calls {
			*line += embedded.start_line;
		}
		for relation in &mut embedded_data.type_relations {
			relation.line += embedded.start_line;
		}
		for symbol in &mut embedded_data.symbols {
			symbol.start_line += embedded.start_line;
			symbol.end_line += embedded.start_line;
		}
		data.imports.extend(embedded_data.imports);
		data.import_bindings.extend(embedded_data.import_bindings);
		data.exports.extend(embedded_data.exports);
		data.calls.extend(embedded_data.calls);
		data.type_relations.extend(embedded_data.type_relations);
		data.symbols.extend(embedded_data.symbols);
	}
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
	data.import_bindings.extend(extract_import_bindings(
		lang_impl.name(),
		node,
		contents,
		&imports,
	));
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
					kind: lang_impl
						.extract_declaration_kind(node, contents)
						.unwrap_or_else(|| symbol_kind_from_node_kind(node.kind()))
						.to_string(),
					name,
					owner: lang_impl.extract_symbol_owner(node, contents),
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

fn extract_import_bindings(
	language: &str,
	node: tree_sitter::Node,
	contents: &str,
	imports: &[String],
) -> Vec<ImportBinding> {
	if imports.is_empty() {
		return Vec::new();
	}
	let Ok(text) = node.utf8_text(contents.as_bytes()) else {
		return Vec::new();
	};
	let text = text.trim();
	let mut bindings = Vec::new();

	match language {
		"python" if text.starts_with("import ") => {
			for item in text.trim_start_matches("import ").split(',') {
				let mut parts = item.split_whitespace();
				let Some(path) = parts.next() else { continue };
				if parts.next() == Some("as") {
					if let Some(alias) = parts.next() {
						bindings.push(ImportBinding {
							local_name: alias.to_string(),
							imported_name: None,
							import_path: path.to_string(),
						});
					}
				}
			}
		}
		"python" if text.starts_with("from ") => {
			if let Some((module, names)) = text.trim_start_matches("from ").split_once(" import ") {
				for item in names.trim_matches(['(', ')']).split(',') {
					let mut parts = item.split_whitespace();
					let Some(imported) = parts.next() else {
						continue;
					};
					let local = if parts.next() == Some("as") {
						parts.next().unwrap_or(imported)
					} else {
						imported
					};
					bindings.push(ImportBinding {
						local_name: local.to_string(),
						imported_name: Some(imported.to_string()),
						import_path: module.trim().to_string(),
					});
				}
			}
		}
		"javascript" | "typescript" => {
			let Some(path) = imports.first() else {
				return bindings;
			};
			let clause = text
				.trim_start_matches("import")
				.split(" from ")
				.next()
				.unwrap_or_default()
				.trim();
			if let Some(alias) = clause.strip_prefix("* as ") {
				bindings.push(ImportBinding {
					local_name: alias.trim().to_string(),
					imported_name: None,
					import_path: path.clone(),
				});
			}
			if let Some(start) = clause.find('{') {
				if let Some(end) = clause.rfind('}') {
					for item in clause[start + 1..end].split(',') {
						let mut parts = item.trim().trim_start_matches("type ").split_whitespace();
						let Some(imported) = parts.next() else {
							continue;
						};
						let local = if parts.next() == Some("as") {
							parts.next().unwrap_or(imported)
						} else {
							imported
						};
						bindings.push(ImportBinding {
							local_name: local.to_string(),
							imported_name: Some(imported.to_string()),
							import_path: path.clone(),
						});
					}
				}
			}
		}
		"go" => {
			for line in text.lines() {
				let parts: Vec<_> = line
					.trim()
					.trim_start_matches("import")
					.trim_matches(['(', ')'])
					.split_whitespace()
					.collect();
				if parts.len() == 2 && !matches!(parts[0], "_" | ".") {
					bindings.push(ImportBinding {
						local_name: parts[0].to_string(),
						imported_name: None,
						import_path: parts[1].trim_matches('"').to_string(),
					});
				}
			}
		}
		"rust" | "php" if text.contains(" as ") => {
			let cleaned = text.trim_start_matches("use ").trim_end_matches(';').trim();
			if let Some((path, alias)) = cleaned.rsplit_once(" as ") {
				let imported = path
					.rsplit([':', '\\'])
					.find(|part| !part.is_empty())
					.unwrap_or(path);
				bindings.push(ImportBinding {
					local_name: alias.trim().to_string(),
					imported_name: Some(imported.to_string()),
					import_path: imports.first().cloned().unwrap_or_else(|| path.to_string()),
				});
			}
		}
		_ => {}
	}

	bindings
}

/// Symbol lookup structures shared by symbol-edge discovery.
struct SymbolIndex {
	/// Symbol name → symbol node ids declaring it (project-wide).
	by_name: HashMap<String, Vec<String>>,
	/// Owning file path → (symbol name → symbol node ids).
	by_file: HashMap<String, HashMap<String, Vec<String>>>,
	/// Owning type/module + symbol name → symbol node ids.
	by_owner: HashMap<(String, String), Vec<String>>,
	/// File + owning type/module + symbol name → symbol node ids.
	by_file_owner: HashMap<(String, String, String), Vec<String>>,
}

impl SymbolIndex {
	fn build(files: &[SymbolFileData]) -> Self {
		let mut index = SymbolIndex {
			by_name: HashMap::new(),
			by_file: HashMap::new(),
			by_owner: HashMap::new(),
			by_file_owner: HashMap::new(),
		};
		for file in files {
			let ids = symbol_node_ids(&file.path, &file.symbols);
			for (symbol, id) in file.symbols.iter().zip(ids) {
				index
					.by_name
					.entry(symbol.name.clone())
					.or_default()
					.push(id.clone());
				index
					.by_file
					.entry(file.path.clone())
					.or_default()
					.entry(symbol.name.clone())
					.or_default()
					.push(id.clone());
				if let Some(owner) = &symbol.owner {
					index
						.by_owner
						.entry((owner.clone(), symbol.name.clone()))
						.or_default()
						.push(id.clone());
					index
						.by_file_owner
						.entry((file.path.clone(), owner.clone(), symbol.name.clone()))
						.or_default()
						.push(id);
				}
			}
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
	) -> Vec<(&str, Provenance, f32)> {
		// 1. Same file — direct AST fact.
		if let Some(targets) = self
			.by_file
			.get(source_path)
			.and_then(|m| m.get(target_name))
		{
			if targets.len() == 1 {
				return vec![(targets[0].as_str(), Provenance::Extracted, 0.9)];
			}
			if allow_ambiguous {
				return targets
					.iter()
					.map(|id| (id.as_str(), Provenance::Ambiguous, 0.4))
					.collect();
			}
			return Vec::new();
		}

		// 2. Files the source imports declare the target. Collect across every
		// import before deciding: returning the first match makes import order
		// silently choose between two equally plausible declarations.
		let mut imported_targets = Vec::new();
		for imported in imported_files {
			if let Some(targets) = self
				.by_file
				.get(imported.as_str())
				.and_then(|m| m.get(target_name))
			{
				imported_targets.extend(targets);
			}
		}
		imported_targets.sort_unstable();
		imported_targets.dedup();
		if imported_targets.len() == 1 {
			return vec![(imported_targets[0].as_str(), Provenance::Extracted, 0.85)];
		}
		if !imported_targets.is_empty() {
			if allow_ambiguous {
				return imported_targets
					.into_iter()
					.map(|id| (id.as_str(), Provenance::Ambiguous, 0.45))
					.collect();
			}
			return Vec::new();
		}

		// 3. Unique global declaration.
		match self.by_name.get(target_name) {
			Some(ids) if ids.len() == 1 => {
				vec![(ids[0].as_str(), Provenance::Inferred, 0.6)]
			}
			// Multiple candidates: only type relations (extends/implements)
			// record them — call sites hit ubiquitous method names (`new`,
			// `get`) and would balloon the edge set with noise.
			Some(ids) if allow_ambiguous => ids
				.iter()
				.map(|id| (id.as_str(), Provenance::Ambiguous, 0.4))
				.collect(),
			_ => Vec::new(),
		}
	}

	fn resolve_call(
		&self,
		source_path: &str,
		source_owner: Option<&str>,
		target: &CallTarget,
		imports: &[(String, String)],
		bindings: &[(ImportBinding, String)],
	) -> Vec<(&str, Provenance, f32)> {
		let imported_files: Vec<String> = imports.iter().map(|(_, path)| path.clone()).collect();
		let mut bound_targets = Vec::new();
		for (binding, path) in bindings {
			if target.qualifier.is_none() && binding.local_name == target.name {
				let imported_name = binding.imported_name.as_deref().unwrap_or(&target.name);
				if let Some(ids) = self
					.by_file
					.get(path)
					.and_then(|symbols| symbols.get(imported_name))
				{
					bound_targets.extend(ids);
				}
			}
			if target.qualifier.as_deref() == Some(binding.local_name.as_str()) {
				let ids = if let Some(owner) = &binding.imported_name {
					self.by_file_owner
						.get(&(path.clone(), owner.clone(), target.name.clone()))
				} else {
					self.by_file
						.get(path)
						.and_then(|symbols| symbols.get(&target.name))
				};
				if let Some(ids) = ids {
					bound_targets.extend(ids);
				}
			}
		}
		bound_targets.sort_unstable();
		bound_targets.dedup();
		if bound_targets.len() == 1 {
			return vec![(bound_targets[0].as_str(), Provenance::Extracted, 0.95)];
		}
		if bound_targets.len() > 1 {
			return Vec::new();
		}

		let Some(qualifier) = target.qualifier.as_deref() else {
			return self.resolve(source_path, &target.name, &imported_files, false);
		};
		let qualifier_leaf = qualifier
			.rsplit("::")
			.next()
			.unwrap_or(qualifier)
			.trim_start_matches('$');

		if matches!(qualifier_leaf, "self" | "Self" | "this") {
			if let Some(owner) = source_owner {
				if let Some(ids) = self.by_file_owner.get(&(
					source_path.to_string(),
					owner.to_string(),
					target.name.clone(),
				)) {
					if ids.len() == 1 {
						return vec![(ids[0].as_str(), Provenance::Extracted, 0.95)];
					}
				}
			}
			return Vec::new();
		}

		// Static/type-qualified call (`Service::run`, `Service.run`).
		if let Some(ids) = self.by_file_owner.get(&(
			source_path.to_string(),
			qualifier_leaf.to_string(),
			target.name.clone(),
		)) {
			if ids.len() == 1 {
				return vec![(ids[0].as_str(), Provenance::Extracted, 0.95)];
			}
		}
		if let Some(ids) = self
			.by_owner
			.get(&(qualifier_leaf.to_string(), target.name.clone()))
		{
			if ids.len() == 1 {
				return vec![(ids[0].as_str(), Provenance::Extracted, 0.9)];
			}
			let imported: Vec<_> = ids
				.iter()
				.filter(|id| {
					imported_files
						.iter()
						.any(|path| id.starts_with(path.as_str()))
				})
				.collect();
			if imported.len() == 1 {
				return vec![(imported[0].as_str(), Provenance::Extracted, 0.85)];
			}
		}

		// Module-qualified call: constrain lookup to the matching imported file.
		for (raw_import, imported_file) in imports {
			let import_leaf = raw_import
				.rsplit([':', '.', '/'])
				.next()
				.unwrap_or(raw_import)
				.trim_matches(|character: char| !character.is_alphanumeric() && character != '_');
			let file_stem = std::path::Path::new(imported_file)
				.file_stem()
				.and_then(|stem| stem.to_str())
				.unwrap_or_default();
			if qualifier_leaf == import_leaf || qualifier_leaf == file_stem {
				if let Some(ids) = self
					.by_file
					.get(imported_file)
					.and_then(|symbols| symbols.get(&target.name))
				{
					if ids.len() == 1 {
						return vec![(ids[0].as_str(), Provenance::Extracted, 0.9)];
					}
				}
			}
		}

		// Unknown instance receiver: retain only a uniquely resolvable scoped
		// target and mark it inferred; never fan out ubiquitous method names.
		self.resolve(source_path, &target.name, &imported_files, false)
			.into_iter()
			.map(|(id, _, confidence)| (id, Provenance::Inferred, confidence.min(0.65)))
			.collect()
	}
}

/// Discover symbol→symbol edges (calls / extends / implements) for the given
/// source files against the full graph. Pure tree-sitter + name resolution —
/// no LLM. File-level edges are produced separately by `relationships.rs`.
pub fn discover_symbol_relationships(new_files: &[SymbolFileData]) -> Vec<CodeRelationship> {
	let index = SymbolIndex::build(new_files);
	let all_files: Vec<String> = new_files.iter().map(|file| file.path.clone()).collect();
	let registry = languages::resolution_utils::FileRegistry::new(&all_files);

	let mut relationships = Vec::new();

	for source_file in new_files {
		if source_file.language == "markdown" {
			continue;
		}
		let Some(lang_impl) = languages::get_language(&source_file.language) else {
			continue;
		};

		// Resolve each import to a concrete project file once per source file.
		let resolved_imports: Vec<(String, String)> = source_file
			.imports
			.iter()
			.filter_map(|import| {
				lang_impl
					.resolve_import(import, &source_file.path, &registry)
					.map(|path| (import.clone(), path))
			})
			.collect();
		let resolved_bindings: Vec<(ImportBinding, String)> = source_file
			.import_bindings
			.iter()
			.filter_map(|binding| {
				lang_impl
					.resolve_import(&binding.import_path, &source_file.path, &registry)
					.map(|path| (binding.clone(), path))
			})
			.collect();
		let imported_files: Vec<String> = resolved_imports
			.iter()
			.map(|(_, path)| path.clone())
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
				.map(|(index, symbol)| (symbol_ids[index].as_str(), symbol))
		};

		for (line, callee) in &source_file.calls {
			if let Some((source_id, source_symbol)) = owning_symbol(*line, true) {
				let callee_display = callee
					.qualifier
					.as_ref()
					.map(|qualifier| format!("{}::{}", qualifier, callee.name))
					.unwrap_or_else(|| callee.name.clone());
				if callee.qualifier.is_none() && callee.name == source_symbol.name {
					relationships.push(CodeRelationship {
						source: source_id.to_string(),
						target: source_id.to_string(),
						relation_type: RelationType::Calls,
						description: format!("{} calls itself", source_symbol.name),
						confidence: 1.0,
						weight: 0.8,
						provenance: Provenance::Extracted,
					});
					continue;
				}
				for (target, provenance, confidence) in index.resolve_call(
					&source_file.path,
					source_symbol.owner.as_deref(),
					callee,
					&resolved_imports,
					&resolved_bindings,
				) {
					relationships.push(CodeRelationship {
						source: source_id.to_string(),
						target: target.to_string(),
						relation_type: RelationType::Calls,
						description: format!("{} calls {}", source_symbol.name, callee_display),
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
						.map(|(index, symbol)| (symbol_ids[index].as_str(), symbol))
				})
			});
			if let Some((source_id, source_symbol)) = owner {
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
							source_symbol.name,
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
			owner: None,
			start_line,
			end_line,
		}
	}

	fn owned_declaration(
		owner: &str,
		name: &str,
		kind: &str,
		start_line: u32,
		end_line: u32,
	) -> SymbolDecl {
		SymbolDecl {
			owner: Some(owner.to_string()),
			..declaration(name, kind, start_line, end_line)
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
		assert_eq!(new.owner.as_deref(), Some("Config"));
		let helper = find("helper").expect("helper symbol");
		assert_eq!(helper.kind, "function");
		let main_fn = find("main_fn").expect("main_fn symbol");
		assert_eq!(main_fn.kind, "function");
		assert!(helper.start_line < main_fn.start_line);
		assert!(main_fn.end_line >= main_fn.start_line);

		// Call sites extracted with lines
		assert!(
			data.calls.iter().any(|(_, call)| call.name == "helper"),
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
			import_bindings: Vec::new(),
			calls: ["helper", "unique_fn", "new"]
				.into_iter()
				.map(|callee| {
					(
						5,
						CallTarget {
							name: callee.to_string(),
							qualifier: None,
						},
					)
				})
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
		let b = SymbolFileData {
			path: "src/b.rs".to_string(),
			language: "rust".to_string(),
			imports: Vec::new(),
			import_bindings: Vec::new(),
			calls: Vec::new(),
			type_relations: Vec::new(),
			symbols: vec![
				declaration("unique_fn", "function", 0, 1),
				declaration("new", "function", 2, 3),
				declaration("Base", "struct", 4, 5),
			],
		};
		let c = SymbolFileData {
			path: "src/c.rs".to_string(),
			language: "rust".to_string(),
			imports: Vec::new(),
			import_bindings: Vec::new(),
			calls: Vec::new(),
			type_relations: Vec::new(),
			symbols: vec![
				declaration("new", "function", 0, 1),
				declaration("Base", "struct", 2, 3),
			],
		};

		let edges = discover_symbol_relationships(&[a, b, c]);

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

	#[test]
	fn qualified_and_self_calls_resolve_to_the_correct_owner() {
		let file = SymbolFileData {
			path: "src/services.rs".to_string(),
			language: "rust".to_string(),
			imports: Vec::new(),
			import_bindings: Vec::new(),
			calls: vec![
				(
					5,
					CallTarget {
						name: "run".to_string(),
						qualifier: Some("self".to_string()),
					},
				),
				(
					25,
					CallTarget {
						name: "run".to_string(),
						qualifier: Some("Worker".to_string()),
					},
				),
			],
			type_relations: Vec::new(),
			symbols: vec![
				owned_declaration("Service", "execute", "function", 0, 10),
				owned_declaration("Service", "run", "function", 12, 16),
				owned_declaration("Worker", "dispatch", "function", 20, 30),
				owned_declaration("Worker", "run", "function", 32, 36),
			],
		};

		let edges = discover_symbol_relationships(&[file]);
		assert!(edges.iter().any(|edge| {
			edge.source == "src/services.rs::Service::execute"
				&& edge.target == "src/services.rs::Service::run"
				&& edge.provenance == Provenance::Extracted
		}));
		assert!(edges.iter().any(|edge| {
			edge.source == "src/services.rs::Worker::dispatch"
				&& edge.target == "src/services.rs::Worker::run"
				&& edge.provenance == Provenance::Extracted
		}));
	}

	#[test]
	fn imported_symbol_alias_resolves_to_original_declaration() {
		let source = SymbolFileData {
			path: "src/main.js".to_string(),
			language: "javascript".to_string(),
			imports: vec!["./utils".to_string()],
			import_bindings: vec![ImportBinding {
				local_name: "h".to_string(),
				imported_name: Some("helper".to_string()),
				import_path: "./utils".to_string(),
			}],
			calls: vec![(
				2,
				CallTarget {
					name: "h".to_string(),
					qualifier: None,
				},
			)],
			type_relations: Vec::new(),
			symbols: vec![declaration("main", "function", 0, 5)],
		};
		let target = SymbolFileData {
			path: "src/utils.js".to_string(),
			language: "javascript".to_string(),
			imports: Vec::new(),
			import_bindings: Vec::new(),
			calls: Vec::new(),
			type_relations: Vec::new(),
			symbols: vec![declaration("helper", "function", 0, 1)],
		};

		let edges = discover_symbol_relationships(&[source, target]);
		assert!(edges.iter().any(|edge| {
			edge.source == "src/main.js::main"
				&& edge.target == "src/utils.js::helper"
				&& edge.relation_type == RelationType::Calls
		}));
	}

	#[test]
	fn svelte_embedded_script_builds_symbols_and_calls() {
		let path = std::env::temp_dir().join(format!(
			"octocode_symbols_svelte_test_{}.svelte",
			std::process::id()
		));
		std::fs::write(
			&path,
			"<script>\nfunction helper() {}\nfunction run() { helper(); }\n</script>\n",
		)
		.unwrap();

		let data = extract_symbols_from_file(path.to_str().unwrap(), "svelte").unwrap();
		std::fs::remove_file(&path).unwrap();

		assert!(data.symbols.iter().any(|symbol| symbol.name == "helper"));
		assert!(data.symbols.iter().any(|symbol| symbol.name == "run"));
		assert!(data.calls.iter().any(|(_, call)| call.name == "helper"));
	}

	#[test]
	fn elixir_builds_live_symbols_imported_calls_and_protocol_edges() {
		let root = std::env::temp_dir().join(format!(
			"octocode_symbols_elixir_test_{}",
			std::process::id()
		));
		let fixture = root.join("lib/fixture");
		std::fs::create_dir_all(&fixture).unwrap();
		let files = [
			(
				"accounts.ex",
				r#"defmodule Fixture.Accounts do
  alias Fixture.Repo
  import Fixture.Validation
  def fetch_user(id), do: Repo.get(User, id) |> validate_result()
end
"#,
			),
			(
				"repo.ex",
				"defmodule Fixture.Repo do\n  def get(schema, id), do: {schema, id}\nend\n",
			),
			(
				"validation.ex",
				"defmodule Fixture.Validation do\n  def validate_result(value), do: value\nend\n",
			),
			(
				"renderable.ex",
				r#"defprotocol Fixture.Renderable do
  def render(value)
end
defimpl Fixture.Renderable, for: Fixture.User do
  def render(user), do: user.email
end
"#,
			),
		];

		let mut graph_files = Vec::new();
		for (name, code) in files {
			let path = fixture.join(name);
			std::fs::write(&path, code).unwrap();
			let data = extract_symbols_from_file(path.to_str().unwrap(), "elixir").unwrap();
			graph_files.push(SymbolFileData::from_ast(
				path.to_string_lossy().into_owned(),
				"elixir".to_string(),
				&data,
			));
		}
		std::fs::remove_dir_all(&root).unwrap();

		let accounts = graph_files
			.iter()
			.find(|file| file.path.ends_with("accounts.ex"))
			.unwrap();
		assert!(accounts.symbols.iter().any(|symbol| {
			symbol.name == "fetch_user" && symbol.owner.as_deref() == Some("Fixture.Accounts")
		}));
		assert!(accounts.imports.contains(&"Fixture.Repo".to_string()));
		assert!(accounts.imports.contains(&"Fixture.Validation".to_string()));

		let relationships = discover_symbol_relationships(&graph_files);
		assert!(relationships.iter().any(|relationship| {
			relationship.relation_type == RelationType::Calls
				&& relationship
					.source
					.ends_with("::Fixture.Accounts::fetch_user")
				&& relationship.target.ends_with("::Fixture.Repo::get")
		}));
		assert!(relationships.iter().any(|relationship| {
			relationship.relation_type == RelationType::Calls
				&& relationship
					.source
					.ends_with("::Fixture.Accounts::fetch_user")
				&& relationship
					.target
					.ends_with("::Fixture.Validation::validate_result")
		}));
		assert!(relationships.iter().any(|relationship| {
			relationship.relation_type == RelationType::Implements
				&& relationship
					.source
					.ends_with("::Fixture.Renderable for Fixture.User")
				&& relationship.target.ends_with("::Fixture.Renderable")
		}));
	}

	#[test]
	fn supported_object_languages_extract_method_owners() {
		let cases = [
			("javascript", "class Service { run() {} }", "run"),
			("typescript", "class Service { run(): void {} }", "run"),
			(
				"python",
				"class Service:\n    def run(self):\n        pass\n",
				"run",
			),
			(
				"go",
				"package p\ntype Service struct{}\nfunc (s *Service) Run() {}\n",
				"Run",
			),
			("java", "class Service { void run() {} }", "run"),
			("cpp", "class Service { void run() {} };", "run"),
			("php", "<?php class Service { function run() {} }", "run"),
			("ruby", "class Service\n  def run\n  end\nend\n", "run"),
			("lua", "function Service.run() end", "run"),
			("swift", "class Service { func run() {} }", "run"),
			(
				"elixir",
				"defmodule Service do\n  def run(), do: :ok\nend\n",
				"run",
			),
		];

		for (index, (language, code, method)) in cases.into_iter().enumerate() {
			let path = std::env::temp_dir().join(format!(
				"octocode_owner_test_{}_{}",
				std::process::id(),
				index
			));
			std::fs::write(&path, code).unwrap();
			let data = extract_symbols_from_file(path.to_str().unwrap(), language).unwrap();
			std::fs::remove_file(&path).unwrap();
			let declaration = data
				.symbols
				.iter()
				.find(|symbol| symbol.name == method)
				.unwrap_or_else(|| panic!("{language} should extract method {method}: {data:?}"));
			assert_eq!(
				declaration.owner.as_deref(),
				Some("Service"),
				"{language} should attribute {method} to Service"
			);
		}
	}
}
