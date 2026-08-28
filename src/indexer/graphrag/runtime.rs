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

//! Always-on, zero-LLM structural graph. It is built lazily from the existing
//! tree-sitter language adapters, cached in memory, and invalidated by the MCP
//! watcher or a repository stamp. Optional persisted GraphRAG data only enriches
//! this live base graph.

use super::symbols::{
	discover_symbol_relationships, extract_symbols_from_file, symbol_node_ids, SymbolFileData,
};
use super::types::{CodeGraph, CodeNode, CodeRelationship, Provenance, RelationType};
use crate::indexer::{languages, NoindexWalker};
use anyhow::{Context, Result};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepositoryStamp {
	fingerprint: u64,
}

#[derive(Debug, Clone)]
struct SourceFile {
	absolute_path: PathBuf,
	relative_path: String,
	language: String,
}

struct CachedGraph {
	stamp: RepositoryStamp,
	graph: Arc<CodeGraph>,
}

/// Cloneable cache handle shared by the graph provider and file watcher.
#[derive(Clone, Default)]
pub struct RuntimeGraphCache {
	value: Arc<parking_lot::RwLock<Option<CachedGraph>>>,
	build_lock: Arc<tokio::sync::Mutex<()>>,
}

impl RuntimeGraphCache {
	/// Drop the cached graph immediately. The next graph operation rebuilds it
	/// from current source, so file-watcher hot reload never serves stale edges.
	pub fn invalidate(&self) {
		*self.value.write() = None;
	}

	pub async fn graph(&self, root: &Path) -> Result<Arc<CodeGraph>> {
		let scan_root = root.to_path_buf();
		let (files, stamp) = tokio::task::spawn_blocking(move || scan_sources(&scan_root))
			.await
			.context("Structural graph source scan task failed")?;

		if let Some(graph) = self.cached(&stamp) {
			return Ok(graph);
		}

		// Serialize cold/stale rebuilds. Concurrent MCP requests reuse the graph
		// produced by the first request instead of parsing the repository twice.
		let _build_guard = self.build_lock.lock().await;
		if let Some(graph) = self.cached(&stamp) {
			return Ok(graph);
		}

		let graph = tokio::task::spawn_blocking(move || build_graph(files))
			.await
			.context("Structural graph build task failed")?;
		let graph = Arc::new(graph);
		*self.value.write() = Some(CachedGraph {
			stamp,
			graph: Arc::clone(&graph),
		});
		Ok(graph)
	}

	fn cached(&self, stamp: &RepositoryStamp) -> Option<Arc<CodeGraph>> {
		self.value
			.read()
			.as_ref()
			.filter(|cached| cached.stamp == *stamp)
			.map(|cached| Arc::clone(&cached.graph))
	}
}

fn scan_sources(root: &Path) -> (Vec<SourceFile>, RepositoryStamp) {
	let mut files = Vec::new();
	let mut stamp_items = Vec::new();

	for result in NoindexWalker::create_walker(root).build() {
		let entry = match result {
			Ok(entry) => entry,
			Err(_) => continue,
		};
		if !entry.file_type().is_some_and(|kind| kind.is_file()) {
			continue;
		}
		let path = entry.path();
		let Some(language) = supported_source_language(path) else {
			continue;
		};

		let metadata = match entry.metadata() {
			Ok(metadata) => metadata,
			Err(_) => continue,
		};
		// Match structural_search's generated/bundle safety limit.
		if metadata.len() > 5_000_000 {
			continue;
		}
		let relative_path = path
			.strip_prefix(root)
			.unwrap_or(path)
			.to_string_lossy()
			.to_string();
		let modified_nanos = metadata
			.modified()
			.ok()
			.and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
			.map(|duration| duration.as_nanos())
			.unwrap_or_default();
		stamp_items.push((relative_path.clone(), metadata.len(), modified_nanos));

		files.push(SourceFile {
			absolute_path: path.to_path_buf(),
			relative_path,
			language: language.to_string(),
		});
	}

	files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
	stamp_items.sort_unstable_by(|a, b| a.0.cmp(&b.0));
	let mut hasher = DefaultHasher::new();
	stamp_items.hash(&mut hasher);
	let stamp = RepositoryStamp {
		fingerprint: hasher.finish(),
	};
	(files, stamp)
}

fn supported_source_language(path: &Path) -> Option<&'static str> {
	let language = crate::indexer::detect_language(path)?;
	languages::get_language(language)
		.is_some()
		.then_some(language)
}

fn build_graph(files: Vec<SourceFile>) -> CodeGraph {
	let cursor = AtomicUsize::new(0);
	let parsed = parking_lot::Mutex::new(Vec::new());
	let workers = std::thread::available_parallelism()
		.map(|count| count.get())
		.unwrap_or(4)
		.min(16)
		.min(files.len().max(1));

	std::thread::scope(|scope| {
		for _ in 0..workers {
			scope.spawn(|| {
				let mut local = Vec::new();
				loop {
					let index = cursor.fetch_add(1, Ordering::Relaxed);
					let Some(file) = files.get(index) else {
						break;
					};
					let path = file.absolute_path.to_string_lossy();
					if let Ok(ast) = extract_symbols_from_file(&path, &file.language) {
						local.push((file.clone(), ast));
					}
				}
				parsed.lock().extend(local);
			});
		}
	});

	let mut parsed = parsed.into_inner();
	parsed.sort_by(|a, b| a.0.relative_path.cmp(&b.0.relative_path));
	let mut graph = CodeGraph::default();
	let mut symbol_files = Vec::with_capacity(parsed.len());

	for (file, ast) in &parsed {
		let file_name = Path::new(&file.relative_path)
			.file_name()
			.and_then(|name| name.to_str())
			.unwrap_or(&file.relative_path)
			.to_string();
		let size_lines = ast
			.symbols
			.iter()
			.map(|symbol| symbol.end_line + 1)
			.max()
			.unwrap_or(0);
		let file_node = CodeNode {
			id: file.relative_path.clone(),
			name: file_name,
			kind: "source_file".to_string(),
			path: file.relative_path.clone(),
			description: String::new(),
			symbols: ast
				.symbols
				.iter()
				.map(|symbol| symbol.name.clone())
				.collect(),
			hash: String::new(),
			embedding: Vec::new(),
			imports: ast.imports.clone(),
			exports: ast.exports.clone(),
			functions: Vec::new(),
			size_lines,
			language: file.language.clone(),
		};
		graph.nodes.insert(file_node.id.clone(), file_node);

		let symbol_ids = symbol_node_ids(&file.relative_path, &ast.symbols);
		for (symbol, symbol_id) in ast.symbols.iter().zip(symbol_ids) {
			graph.nodes.insert(
				symbol_id.clone(),
				CodeNode {
					id: symbol_id.clone(),
					name: symbol.name.clone(),
					kind: symbol.kind.clone(),
					path: file.relative_path.clone(),
					description: if let Some(owner) = &symbol.owner {
						format!(
							"{} {}::{} in {}:{}",
							symbol.kind,
							owner,
							symbol.name,
							file.relative_path,
							symbol.start_line + 1
						)
					} else {
						format!(
							"{} {} in {}:{}",
							symbol.kind,
							symbol.name,
							file.relative_path,
							symbol.start_line + 1
						)
					},
					symbols: std::iter::once(symbol.name.clone())
						.chain(symbol.owner.clone())
						.collect(),
					hash: String::new(),
					embedding: Vec::new(),
					imports: Vec::new(),
					exports: Vec::new(),
					functions: Vec::new(),
					size_lines: symbol.end_line.saturating_sub(symbol.start_line) + 1,
					language: file.language.clone(),
				},
			);
			graph.relationships.push(CodeRelationship {
				source: file.relative_path.clone(),
				target: symbol_id,
				relation_type: RelationType::Contains,
				description: format!("{} contains {}", file.relative_path, symbol.name),
				confidence: 1.0,
				weight: RelationType::Contains.importance_weight(),
				provenance: Provenance::Extracted,
			});
		}

		symbol_files.push(SymbolFileData::from_ast(
			file.relative_path.clone(),
			file.language.clone(),
			ast,
		));
	}

	let all_paths: Vec<String> = parsed
		.iter()
		.map(|(file, _)| file.relative_path.clone())
		.collect();
	for file in &symbol_files {
		let Some(language) = languages::get_language(&file.language) else {
			continue;
		};
		for import in &file.imports {
			if let Some(target) = language.resolve_import(import, &file.path, &all_paths) {
				graph.relationships.push(CodeRelationship {
					source: file.path.clone(),
					target,
					relation_type: RelationType::Imports,
					description: format!("{} imports {}", file.path, import),
					confidence: 1.0,
					weight: RelationType::Imports.importance_weight(),
					provenance: Provenance::Extracted,
				});
			}
		}
	}

	graph
		.relationships
		.extend(discover_symbol_relationships(&symbol_files));
	graph.relationships.sort_unstable_by(|a, b| {
		(&a.source, &a.target, &a.relation_type).cmp(&(&b.source, &b.target, &b.relation_type))
	});
	graph.relationships.dedup_by(|a, b| {
		a.source == b.source && a.target == b.target && a.relation_type == b.relation_type
	});
	graph
}

/// Overlay the optional persisted file graph onto the live structural graph.
/// Runtime symbols remain authoritative and stale persisted symbol rows are
/// ignored; enrichment contributes file descriptions and additional file-level
/// relationships only while their endpoints still exist in current source.
pub fn merge_enrichment(base: &CodeGraph, enriched: CodeGraph, root: &Path) -> CodeGraph {
	let mut graph = base.clone();
	for (id, node) in enriched.nodes {
		if node.is_symbol_node() {
			continue;
		}
		if let Some(current) = graph.nodes.get_mut(&id) {
			if !node.description.is_empty() {
				current.description = node.description;
			}
		} else if root.join(&node.path).is_file() {
			graph.nodes.insert(id, node);
		}
	}

	for relationship in enriched.relationships {
		if relationship.source.contains("::") || relationship.target.contains("::") {
			continue;
		}
		if graph.nodes.contains_key(&relationship.source)
			&& graph.nodes.contains_key(&relationship.target)
		{
			graph.relationships.push(relationship);
		}
	}
	graph.relationships.sort_unstable_by(|a, b| {
		(&a.source, &a.target, &a.relation_type).cmp(&(&b.source, &b.target, &b.relation_type))
	});
	graph.relationships.dedup_by(|a, b| {
		a.source == b.source && a.target == b.target && a.relation_type == b.relation_type
	});
	graph
}

/// Deterministic lexical seed lookup for the runtime graph. Relationship words
/// are ignored so `what calls parse_config` seeds `parse_config`, not noise.
pub fn search_nodes(graph: &CodeGraph, query: &str, limit: usize) -> Vec<CodeNode> {
	const STOPWORDS: &[&str] = &[
		"a", "an", "and", "are", "by", "calls", "does", "for", "from", "how", "in", "is", "of",
		"or", "the", "to", "uses", "what", "where", "who",
	];
	let terms: Vec<String> = query
		.split(|character: char| !character.is_alphanumeric() && character != '_')
		.map(str::to_lowercase)
		.filter(|term| !term.is_empty() && !STOPWORDS.contains(&term.as_str()))
		.collect();
	if terms.is_empty() {
		return Vec::new();
	}

	let mut scored = Vec::new();
	for node in graph.nodes.values() {
		let name = node.name.to_lowercase();
		let id = node.id.to_lowercase();
		let path = node.path.to_lowercase();
		let aliases: Vec<String> = node
			.symbols
			.iter()
			.map(|symbol| symbol.to_lowercase())
			.collect();
		let mut score = 0u32;
		for term in &terms {
			if name == *term {
				score += 100;
			} else if name.starts_with(term) {
				score += 40;
			} else if name.contains(term) {
				score += 20;
			} else if aliases.iter().any(|alias| alias == term) {
				score += 30;
			} else if aliases.iter().any(|alias| alias.contains(term)) {
				score += 10;
			} else if id.contains(term) || path.contains(term) {
				score += 5;
			}
		}
		if score > 0 {
			scored.push((score, node));
		}
	}

	scored.sort_unstable_by(|(score_a, node_a), (score_b, node_b)| {
		score_b
			.cmp(score_a)
			.then_with(|| node_a.id.len().cmp(&node_b.id.len()))
			.then_with(|| node_a.id.cmp(&node_b.id))
	});
	scored
		.into_iter()
		.take(limit)
		.map(|(_, node)| node.clone())
		.collect()
}

/// Bounded directed path search shared by runtime graph operations. A hard
/// result cap prevents highly connected symbol graphs from enumerating an
/// unbounded number of simple paths.
pub fn find_paths(
	graph: &CodeGraph,
	source_id: &str,
	target_id: &str,
	max_depth: usize,
) -> Vec<Vec<String>> {
	use std::collections::{HashMap, VecDeque};

	const MAX_PATHS: usize = 20;
	// Total-work bound: the path cap alone only triggers on reaching the
	// target, so an unreachable target on a dense symbol graph would still
	// enumerate every simple path up to max_depth.
	const MAX_EXPANSIONS: usize = 100_000;
	let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
	for relationship in &graph.relationships {
		adjacency
			.entry(relationship.source.as_str())
			.or_default()
			.push(relationship.target.as_str());
	}
	for targets in adjacency.values_mut() {
		targets.sort_unstable();
		targets.dedup();
	}

	let mut queue = VecDeque::from([vec![source_id.to_string()]]);
	let mut paths = Vec::new();
	let mut expansions = 0usize;
	while let Some(path) = queue.pop_front() {
		expansions += 1;
		if expansions > MAX_EXPANSIONS {
			break;
		}
		let current = path.last().map(String::as_str).unwrap_or_default();
		if current == target_id {
			paths.push(path);
			if paths.len() >= MAX_PATHS {
				break;
			}
			continue;
		}
		if path.len().saturating_sub(1) >= max_depth {
			continue;
		}
		for neighbor in adjacency.get(current).into_iter().flatten() {
			if path.iter().any(|node| node.as_str() == *neighbor) {
				continue;
			}
			let mut next = path.clone();
			next.push((*neighbor).to_string());
			queue.push_back(next);
		}
	}
	paths
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn runtime_source_detection_covers_embedded_and_document_languages() {
		assert_eq!(
			supported_source_language(Path::new("src/App.svelte")),
			Some("svelte")
		);
		assert_eq!(
			supported_source_language(Path::new("docs/design.md")),
			Some("markdown")
		);
		assert_eq!(
			supported_source_language(Path::new("lib/accounts.ex")),
			Some("elixir")
		);
	}

	fn graph_node(id: &str, name: &str, kind: &str, path: &str) -> CodeNode {
		CodeNode {
			id: id.to_string(),
			name: name.to_string(),
			kind: kind.to_string(),
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
	fn lexical_search_prefers_exact_symbol_over_path_match() {
		let mut graph = CodeGraph::default();
		for (id, name) in [
			("src/parse_config.rs", "parse_config.rs"),
			("src/config.rs::parse_config", "parse_config"),
		] {
			graph.nodes.insert(
				id.to_string(),
				graph_node(id, name, "function", "src/config.rs"),
			);
		}

		let results = search_nodes(&graph, "what calls parse_config", 5);
		assert_eq!(results[0].id, "src/config.rs::parse_config");
	}

	#[test]
	fn lexical_search_uses_symbol_owner_to_disambiguate_methods() {
		let mut graph = CodeGraph::default();
		for (id, owner) in [
			("src/service.rs::Service::run", "Service"),
			("src/worker.rs::Worker::run", "Worker"),
		] {
			let mut node = graph_node(id, "run", "function", id.split("::").next().unwrap());
			node.symbols = vec!["run".to_string(), owner.to_string()];
			graph.nodes.insert(id.to_string(), node);
		}

		let results = search_nodes(&graph, "Service run", 5);
		assert_eq!(results[0].id, "src/service.rs::Service::run");
	}

	#[test]
	fn path_search_is_bounded_and_avoids_cycles() {
		let mut graph = CodeGraph::default();
		for (source, target) in [("a", "b"), ("b", "a"), ("b", "c")] {
			graph.relationships.push(CodeRelationship {
				source: source.to_string(),
				target: target.to_string(),
				relation_type: RelationType::Calls,
				description: String::new(),
				confidence: 1.0,
				weight: 1.0,
				provenance: Provenance::Extracted,
			});
		}

		assert!(find_paths(&graph, "a", "c", 1).is_empty());
		assert_eq!(find_paths(&graph, "a", "c", 2), vec![vec!["a", "b", "c"]]);
	}

	#[test]
	fn persisted_enrichment_adds_file_edges_but_not_stale_symbols() {
		let mut base = CodeGraph::default();
		for id in ["src/a.rs", "src/b.rs"] {
			base.nodes
				.insert(id.to_string(), graph_node(id, id, "source_file", id));
		}
		base.nodes.insert(
			"src/a.rs::run".to_string(),
			graph_node("src/a.rs::run", "run", "function", "src/a.rs"),
		);

		let mut enriched = CodeGraph::default();
		let mut file = graph_node("src/a.rs", "a.rs", "source_file", "src/a.rs");
		file.description = "Enriched file description".to_string();
		enriched.nodes.insert(file.id.clone(), file);
		enriched.nodes.insert(
			"src/a.rs::old".to_string(),
			graph_node("src/a.rs::old", "old", "function", "src/a.rs"),
		);
		for (source, target, relation_type) in [
			(
				"src/a.rs",
				"src/b.rs",
				RelationType::ArchitecturalDependency,
			),
			("src/a.rs::old", "src/a.rs::run", RelationType::Calls),
		] {
			enriched.relationships.push(CodeRelationship {
				source: source.to_string(),
				target: target.to_string(),
				relation_type,
				description: String::new(),
				confidence: 1.0,
				weight: 1.0,
				provenance: Provenance::Inferred,
			});
		}

		let graph = merge_enrichment(&base, enriched, Path::new("."));
		assert_eq!(
			graph.nodes["src/a.rs"].description,
			"Enriched file description"
		);
		assert!(!graph.nodes.contains_key("src/a.rs::old"));
		assert!(graph.relationships.iter().any(|relationship| {
			relationship.relation_type == RelationType::ArchitecturalDependency
		}));
		assert!(!graph
			.relationships
			.iter()
			.any(|relationship| relationship.source == "src/a.rs::old"));
	}

	#[test]
	fn builds_symbol_call_graph_directly_from_source() {
		let root = std::env::temp_dir().join(format!(
			"octocode-runtime-graph-{}-{}",
			std::process::id(),
			std::time::SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.expect("system clock should be after Unix epoch")
				.as_nanos()
		));
		std::fs::create_dir_all(&root).expect("temporary source directory should be created");
		std::fs::write(
			root.join("lib.rs"),
			"fn helper() {}\nfn run() { helper(); }\n",
		)
		.expect("temporary Rust source should be written");

		let (files, _) = scan_sources(&root);
		let graph = build_graph(files);
		std::fs::remove_dir_all(&root).expect("temporary source directory should be removed");

		assert!(graph.nodes.contains_key("lib.rs::helper"));
		assert!(graph.nodes.contains_key("lib.rs::run"));
		assert!(graph.relationships.iter().any(|relationship| {
			relationship.source == "lib.rs::run"
				&& relationship.target == "lib.rs::helper"
				&& relationship.relation_type == RelationType::Calls
		}));
	}

	#[test]
	fn builds_elixir_live_graph_with_import_call_and_protocol_edges() {
		let root = std::env::temp_dir().join(format!(
			"octocode-runtime-elixir-graph-{}-{}",
			std::process::id(),
			std::time::SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.expect("system clock should be after Unix epoch")
				.as_nanos()
		));
		let fixture = root.join("lib/fixture");
		std::fs::create_dir_all(&fixture).expect("temporary Elixir directory should be created");
		for (name, source) in [
			(
				"accounts.ex",
				"defmodule Fixture.Accounts do\n  alias Fixture.Repo\n  def fetch(id), do: Repo.get(User, id)\nend\n",
			),
			(
				"repo.ex",
				"defmodule Fixture.Repo do\n  def get(schema, id), do: {schema, id}\nend\n",
			),
			(
				"renderable.ex",
				"defprotocol Fixture.Renderable do\n  def render(value)\nend\ndefimpl Fixture.Renderable, for: Fixture.User do\n  def render(user), do: user.email\nend\n",
			),
			(
				"accounts_test.exs",
				"defmodule Fixture.AccountsTest do\n  alias Fixture.Accounts\n  test \"fetches an account\" do\n    Accounts.fetch(42)\n  end\nend\n",
			),
		] {
			std::fs::write(fixture.join(name), source)
				.expect("temporary Elixir source should be written");
		}

		let (files, _) = scan_sources(&root);
		let graph = build_graph(files);
		std::fs::remove_dir_all(&root).expect("temporary source directory should be removed");

		assert!(graph.nodes.contains_key("lib/fixture/accounts.ex"));
		assert!(graph
			.nodes
			.contains_key("lib/fixture/accounts.ex::Fixture.Accounts::fetch"));
		assert!(graph.nodes.contains_key(
			"lib/fixture/accounts_test.exs::Fixture.AccountsTest::fetches an account"
		));
		assert!(graph.relationships.iter().any(|relationship| {
			relationship.source == "lib/fixture/accounts.ex"
				&& relationship.target == "lib/fixture/repo.ex"
				&& relationship.relation_type == RelationType::Imports
		}));
		assert!(graph.relationships.iter().any(|relationship| {
			relationship.source == "lib/fixture/accounts.ex::Fixture.Accounts::fetch"
				&& relationship.target == "lib/fixture/repo.ex::Fixture.Repo::get"
				&& relationship.relation_type == RelationType::Calls
		}));
		assert!(graph.relationships.iter().any(|relationship| {
			relationship.source == "lib/fixture/renderable.ex::Fixture.Renderable for Fixture.User"
				&& relationship.target == "lib/fixture/renderable.ex::Fixture.Renderable"
				&& relationship.relation_type == RelationType::Implements
		}));
		assert!(graph.relationships.iter().any(|relationship| {
			relationship.source
				== "lib/fixture/accounts_test.exs::Fixture.AccountsTest::fetches an account"
				&& relationship.target == "lib/fixture/accounts.ex::Fixture.Accounts::fetch"
				&& relationship.relation_type == RelationType::Calls
		}));
	}
}
