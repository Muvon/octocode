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

// GraphRAG module entry point

pub mod ai;
pub mod builder;
pub mod database;
pub mod relationships;
pub mod types;
pub mod utils;

#[cfg(test)]
mod tests;

// Re-export the main types and interfaces for backward compatibility
pub use builder::GraphBuilder;
pub use types::{CodeGraph, CodeNode, CodeRelationship, FunctionInfo};
pub use utils::{
	cosine_similarity, detect_project_root, detect_project_root_from, find_node_id,
	graphrag_nodes_to_markdown, graphrag_nodes_to_text, normalize_node_id,
	render_graphrag_nodes_json, to_relative_path,
};

// GraphRAG implementation for all operations (backward compatibility + new operations)
use crate::config::Config;
use anyhow::Result;
use std::path::PathBuf;

#[derive(Clone)]
pub struct GraphRAG {
	config: Config,
	working_dir: PathBuf,
}

impl GraphRAG {
	pub fn new(config: Config, working_dir: PathBuf) -> Self {
		Self {
			config,
			working_dir,
		}
	}

	/// Search for nodes (backward compatibility)
	pub async fn search(&self, query: &str) -> Result<String> {
		let builder =
			GraphBuilder::new_with_quiet(self.config.clone(), &self.working_dir, true).await?;
		let nodes = builder.search_nodes(query).await?;
		Ok(graphrag_nodes_to_text(&nodes))
	}

	/// Get node details by ID
	pub async fn get_node(&self, node_id: &str) -> Result<String> {
		let builder =
			GraphBuilder::new_with_quiet(self.config.clone(), &self.working_dir, true).await?;
		let graph = builder.get_graph().await?;
		match find_node_id(&graph, node_id) {
			Some(resolved_id) => {
				let node = &graph.nodes[resolved_id];
				Ok(format!(
					"Node: {}\nID: {}\nKind: {}\nPath: {}\nDescription: {}\nSymbols: {}\n",
					node.name,
					node.id,
					node.kind,
					node.path,
					node.description,
					node.symbols.join(", ")
				))
			}
			None => Err(anyhow::anyhow!("Node not found: {}", node_id)),
		}
	}

	/// Get relationships for a node
	pub async fn get_relationships(&self, node_id: &str) -> Result<String> {
		let builder =
			GraphBuilder::new_with_quiet(self.config.clone(), &self.working_dir, true).await?;
		let graph = builder.get_graph().await?;

		let resolved_id = find_node_id(&graph, node_id)
			.ok_or_else(|| anyhow::anyhow!("Node not found: {}", node_id))?;

		let relationships: Vec<_> = graph
			.relationships
			.iter()
			.filter(|rel| rel.source == resolved_id || rel.target == resolved_id)
			.collect();

		if relationships.is_empty() {
			return Ok(format!("No relationships found for node: {}", resolved_id));
		}

		let mut output = format!(
			"Relationships for {} ({} total):\n\n",
			resolved_id,
			relationships.len()
		);

		// Outgoing relationships
		let outgoing: Vec<_> = relationships
			.iter()
			.filter(|rel| rel.source == resolved_id)
			.collect();
		if !outgoing.is_empty() {
			output.push_str("Outgoing:\n");
			for rel in outgoing {
				let target_name = graph
					.nodes
					.get(&rel.target)
					.map(|n| n.name.clone())
					.unwrap_or_else(|| rel.target.clone());
				output.push_str(&format!(
					"  {} → {} ({}): {}\n",
					rel.relation_type, target_name, rel.target, rel.description
				));
			}
			output.push('\n');
		}

		// Incoming relationships
		let incoming: Vec<_> = relationships
			.iter()
			.filter(|rel| rel.target == resolved_id)
			.collect();
		if !incoming.is_empty() {
			output.push_str("Incoming:\n");
			for rel in incoming {
				let source_name = graph
					.nodes
					.get(&rel.source)
					.map(|n| n.name.clone())
					.unwrap_or_else(|| rel.source.clone());
				output.push_str(&format!(
					"  {} ← {} ({}): {}\n",
					rel.relation_type, source_name, rel.source, rel.description
				));
			}
		}
		Ok(output)
	}

	/// Find paths between two nodes
	pub async fn find_path(
		&self,
		source_id: &str,
		target_id: &str,
		max_depth: usize,
	) -> Result<String> {
		let builder =
			GraphBuilder::new_with_quiet(self.config.clone(), &self.working_dir, true).await?;
		let graph = builder.get_graph().await?;

		// Resolve node IDs with fuzzy matching
		let resolved_source = find_node_id(&graph, source_id)
			.ok_or_else(|| anyhow::anyhow!("Source node not found: {}", source_id))?;
		let resolved_target = find_node_id(&graph, target_id)
			.ok_or_else(|| anyhow::anyhow!("Target node not found: {}", target_id))?;

		let paths = builder
			.find_paths(resolved_source, resolved_target, max_depth)
			.await?;

		if paths.is_empty() {
			return Ok(format!(
				"No paths found between {} and {} within depth {}",
				source_id, target_id, max_depth
			));
		}

		let mut output = format!(
			"Paths from {} to {} ({} found):\n\n",
			source_id,
			target_id,
			paths.len()
		);
		for (i, path) in paths.iter().enumerate() {
			output.push_str(&format!("Path {}:\n", i + 1));
			for (j, node_id) in path.iter().enumerate() {
				let node_name = graph
					.nodes
					.get(node_id)
					.map(|n| n.name.clone())
					.unwrap_or_else(|| node_id.clone());
				if j > 0 {
					let prev_id = &path[j - 1];
					let rel = graph
						.relationships
						.iter()
						.find(|r| r.source == *prev_id && r.target == *node_id);
					if let Some(rel) = rel {
						output.push_str(&format!(" --{}-> ", rel.relation_type));
					} else {
						output.push_str(" -> ");
					}
				}
				output.push_str(&format!("{} ({})", node_name, node_id));
			}
			output.push_str("\n\n");
		}
		Ok(output)
	}

	/// Get graph overview
	pub async fn overview(&self) -> Result<String> {
		let builder =
			GraphBuilder::new_with_quiet(self.config.clone(), &self.working_dir, true).await?;
		let graph = builder.get_graph().await?;

		let node_count = graph.nodes.len();
		let relationship_count = graph.relationships.len();

		// Count node types
		let mut node_types = std::collections::HashMap::new();
		for node in graph.nodes.values() {
			*node_types.entry(node.kind.clone()).or_insert(0) += 1;
		}

		// Count relationship types
		let mut rel_types = std::collections::HashMap::new();
		for rel in &graph.relationships {
			*rel_types.entry(rel.relation_type.clone()).or_insert(0) += 1;
		}

		let mut output = format!(
			"GraphRAG Overview: {} nodes, {} relationships\n\n",
			node_count, relationship_count
		);

		output.push_str("Node Types:\n");
		for (kind, count) in node_types.iter() {
			output.push_str(&format!("  {}: {}\n", kind, count));
		}

		output.push_str("\nRelationship Types:\n");
		for (rel_type, count) in rel_types.iter() {
			output.push_str(&format!("  {}: {}\n", rel_type, count));
		}
		Ok(output)
	}

	/// Access to config for compatibility
	pub fn config(&self) -> &Config {
		&self.config
	}
}
