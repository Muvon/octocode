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

use clap::{Args, ValueEnum};

use octocode::config::Config;
use octocode::indexer;
use octocode::indexer::graphrag::find_node_id;
use octocode::store::Store;

use crate::commands::OutputFormat;

#[derive(Args, Debug)]
pub struct GraphRAGArgs {
	/// The operation to perform on the GraphRAG knowledge graph
	#[arg(value_enum)]
	pub operation: GraphRAGOperation,

	/// The query to search for (used with the search operation)
	#[arg(long)]
	pub query: Option<String>,

	/// The node ID to get information about (used with get_node and get_relationships operations)
	#[arg(long)]
	pub node_id: Option<String>,

	/// The source node ID for path finding (used with find_path operation)
	#[arg(long)]
	pub source_id: Option<String>,

	/// The target node ID for path finding (used with find_path operation)
	#[arg(long)]
	pub target_id: Option<String>,

	/// The maximum path depth to consider (used with find_path operation)
	#[arg(long, default_value = "3")]
	pub max_depth: usize,

	/// Output format
	#[arg(long, value_enum, default_value = "cli")]
	pub format: OutputFormat,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum GraphRAGOperation {
	/// Search for nodes that match a semantic query
	Search,
	/// Get detailed information about a specific node
	GetNode,
	/// Get relationships involving a specific node
	GetRelationships,
	/// Find paths between two nodes in the graph
	FindPath,
	/// Get an overview of the entire graph structure
	Overview,
}

/// Execute a GraphRAG command
pub async fn execute(
	_store: &Store,
	args: &GraphRAGArgs,
	config: &Config,
) -> Result<(), anyhow::Error> {
	// Check if GraphRAG is enabled in the config
	if !config.graphrag.enabled {
		eprintln!("Error: GraphRAG is not enabled in your configuration.");
		eprintln!("To enable it, run:\n  octocode config --graphrag-enable true");
		eprintln!("Then run 'octocode index' to build the knowledge graph.");
		return Ok(());
	}

	// Initialize the GraphBuilder
	let graph_builder = match indexer::GraphBuilder::new(config.clone()).await {
		Ok(builder) => builder,
		Err(e) => {
			eprintln!("Failed to initialize the GraphRAG system: {}", e);
			return Ok(());
		}
	};

	// Get the current graph from the builder (this will load from database in the future)
	let graph = match graph_builder.get_graph().await {
		Ok(g) => g,
		Err(e) => {
			eprintln!("Failed to load the GraphRAG knowledge graph: {}", e);
			return Ok(());
		}
	};

	// If the graph is empty, advise to run indexing
	if graph.nodes.is_empty() {
		eprintln!("GraphRAG knowledge graph is empty.");
		eprintln!("Run 'octocode index' to build the knowledge graph.");
		return Ok(());
	}

	// Execute the requested operation
	match args.operation {
		GraphRAGOperation::Search => {
			// Validate required parameters
			let query = match &args.query {
				Some(q) => q,
				None => {
					eprintln!("Error: 'query' parameter is required for search operation.");
					eprintln!("Example: octocode graphrag search --query \"find all database connections\"");
					return Ok(());
				}
			};

			// Search for nodes
			println!("Searching for: {}", query);
			let nodes = graph_builder.search_nodes(query).await?;

			// Display results in the requested format
			if args.format.is_json() {
				// Use JSON format
				indexer::graphrag::render_graphrag_nodes_json(&nodes)?
			} else if args.format.is_md() {
				// Use markdown format
				let markdown = indexer::graphrag::graphrag_nodes_to_markdown(&nodes);
				println!("{}", markdown);
			} else if args.format.is_text() {
				// Use minimal text format for token efficiency
				let text_output = indexer::graphrag::graphrag_nodes_to_text(&nodes);
				println!("{}", text_output);
			} else if args.format.is_cli() {
				// CLI format - use the rich text format
				let text_output = indexer::graphrag::graphrag_nodes_to_text(&nodes);
				println!("{}", text_output);
			} else {
				// Fallback to CLI format
				let text_output = indexer::graphrag::graphrag_nodes_to_text(&nodes);
				println!("{}", text_output);
			}
		}
		GraphRAGOperation::GetNode => {
			// Validate required parameters
			let node_id = match &args.node_id {
				Some(id) => id,
				None => {
					eprintln!("Error: 'node_id' parameter is required for get_node operation.");
					eprintln!("Example: octocode graphrag get-node --node_id \"src/main.rs/main\"");
					return Ok(());
				}
			};

			// Get the graph
			let graph = graph_builder.get_graph().await?;

			// Get node details with fuzzy matching
			match find_node_id(&graph, node_id) {
				Some(resolved_id) => {
					let node = &graph.nodes[resolved_id];
					println!("\u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550} Node: {} \u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}", node.name);
					println!("\u{2551} ID: {}", node.id);
					println!("\u{2551} Kind: {}", node.kind);
					println!("\u{2551} Path: {}", node.path);
					println!("\u{2551} Description: {}", node.description);
					if !node.symbols.is_empty() {
						println!("\u{2551} Symbols:");
						for symbol in &node.symbols {
							println!("\u{2551}   - {}", symbol);
						}
					}
					println!("\u{255a}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}");
				}
				None => println!("Node not found: {}", node_id),
			}
		}
		GraphRAGOperation::GetRelationships => {
			// Validate required parameters
			let node_id = match &args.node_id {
				Some(id) => id,
				None => {
					eprintln!(
						"Error: 'node_id' parameter is required for get_relationships operation."
					);
					eprintln!("Example: octocode graphrag get-relationships --node_id \"src/main.rs/main\"");
					return Ok(());
				}
			};

			// Get the graph
			let graph = graph_builder.get_graph().await?;

			// Resolve node ID with fuzzy matching
			let resolved_id = match find_node_id(&graph, node_id) {
				Some(id) => id.to_string(),
				None => {
					println!("Node not found: {}", node_id);
					return Ok(());
				}
			};

			// Find relationships where this node is either source or target
			let relationships: Vec<_> = graph
				.relationships
				.iter()
				.filter(|rel| rel.source == resolved_id || rel.target == resolved_id)
				.collect();

			if relationships.is_empty() {
				println!("No relationships found for node: {}", resolved_id);
			} else {
				println!(
					"Found {} relationships for node {}:\n",
					relationships.len(),
					resolved_id
				);

				// Outgoing relationships
				let outgoing: Vec<_> = relationships
					.iter()
					.filter(|rel| rel.source == resolved_id)
					.collect();

				if !outgoing.is_empty() {
					println!("Outgoing Relationships:");
					for rel in outgoing {
						let target_name = graph
							.nodes
							.get(&rel.target)
							.map(|n| n.name.clone())
							.unwrap_or_else(|| rel.target.clone());

						println!(
							"  - {} \u{2192} {} ({}): {}",
							rel.relation_type, target_name, rel.target, rel.description
						);
					}
					println!();
				}

				// Incoming relationships
				let incoming: Vec<_> = relationships
					.iter()
					.filter(|rel| rel.target == resolved_id)
					.collect();

				if !incoming.is_empty() {
					println!("Incoming Relationships:");
					for rel in incoming {
						let source_name = graph
							.nodes
							.get(&rel.source)
							.map(|n| n.name.clone())
							.unwrap_or_else(|| rel.source.clone());

						println!(
							"  - {} \u{2190} {} ({}): {}",
							rel.relation_type, source_name, rel.source, rel.description
						);
					}
				}
			}
		}
		GraphRAGOperation::FindPath => {
			// Validate required parameters
			let source_id = match &args.source_id {
				Some(id) => id,
				None => {
					eprintln!("Error: 'source_id' parameter is required for find_path operation.");
					eprintln!("Example: octocode graphrag find-path --source-id \"src/main.rs/main\" --target-id \"src/config.rs/load\"");
					return Ok(());
				}
			};

			let target_id = match &args.target_id {
				Some(id) => id,
				None => {
					eprintln!("Error: 'target_id' parameter is required for find_path operation.");
					eprintln!("Example: octocode graphrag find-path --source-id \"src/main.rs/main\" --target-id \"src/config.rs/load\"");
					return Ok(());
				}
			};

			// Get the graph for node name lookup and ID resolution
			let graph = graph_builder.get_graph().await?;

			// Resolve node IDs with fuzzy matching
			let resolved_source = match find_node_id(&graph, source_id) {
				Some(id) => id.to_string(),
				None => {
					println!("Source node not found: {}", source_id);
					return Ok(());
				}
			};
			let resolved_target = match find_node_id(&graph, target_id) {
				Some(id) => id.to_string(),
				None => {
					println!("Target node not found: {}", target_id);
					return Ok(());
				}
			};

			// Find paths
			println!(
				"Finding paths from {} to {} (max depth: {})...",
				resolved_source, resolved_target, args.max_depth
			);
			let paths = graph_builder
				.find_paths(&resolved_source, &resolved_target, args.max_depth)
				.await?;

			// Display results
			if paths.is_empty() {
				println!("No paths found between these nodes within the specified depth.");
			} else {
				println!("Found {} paths:\n", paths.len());

				for (i, path) in paths.iter().enumerate() {
					println!("Path {}:", i + 1);

					// Display each node in the path
					for (j, node_id) in path.iter().enumerate() {
						let node_name = graph
							.nodes
							.get(node_id)
							.map(|n| n.name.clone())
							.unwrap_or_else(|| node_id.clone());

						if j > 0 {
							// Look up the relationship
							let prev_id = &path[j - 1];
							let rel = graph
								.relationships
								.iter()
								.find(|r| r.source == *prev_id && r.target == *node_id);

							if let Some(rel) = rel {
								print!(" --{}-> ", rel.relation_type);
							} else {
								print!(" -> ");
							}
						}

						print!("{} ({})", node_name, node_id);
					}
					println!("\n");
				}
			}
		}
		GraphRAGOperation::Overview => {
			// Get the graph
			let graph = graph_builder.get_graph().await?;

			// Get statistics
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

			// Display overview
			println!("GraphRAG Knowledge Graph Overview");
			println!("=================================\n");
			println!(
				"The knowledge graph contains {} nodes and {} relationships.\n",
				node_count, relationship_count
			);

			// Node type statistics
			println!("Node Types:");
			for (kind, count) in node_types.iter() {
				println!("  - {}: {} nodes", kind, count);
			}
			println!();

			// Relationship type statistics
			println!("Relationship Types:");
			for (rel_type, count) in rel_types.iter() {
				println!("  - {}: {} relationships", rel_type, count);
			}
		}
	}

	Ok(())
}
