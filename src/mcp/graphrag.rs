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

use anyhow::Result;
use serde_json::{json, Value};
use tracing::debug;

use crate::config::Config;
use crate::indexer::branch::BranchManifest;
use crate::indexer::graphrag::find_node_id;
use crate::indexer::graphrag::runtime::{self, RuntimeGraphCache};
use crate::indexer::{self, graphrag::GraphRAG};
use crate::mcp::types::McpError;

#[derive(Debug, Clone)]
pub enum GraphRAGOperation {
	Search,
	GetNode,
	GetRelationships,
	FindPath,
	Overview,
}

#[derive(Debug, Clone)]
pub enum OutputFormat {
	Text,
	Json,
	Md,
	Cli,
}

impl OutputFormat {
	pub fn is_json(&self) -> bool {
		matches!(self, OutputFormat::Json)
	}

	pub fn is_md(&self) -> bool {
		matches!(self, OutputFormat::Md)
	}

	pub fn is_text(&self) -> bool {
		matches!(self, OutputFormat::Text)
	}

	pub fn is_cli(&self) -> bool {
		matches!(self, OutputFormat::Cli)
	}
}

#[derive(Debug, Clone)]
pub struct GraphRAGArgs {
	pub operation: GraphRAGOperation,
	pub query: Option<String>,
	pub node_id: Option<String>,
	pub source_id: Option<String>,
	pub target_id: Option<String>,
	pub max_depth: usize,
	pub format: OutputFormat,
}

/// GraphRAG tool provider
#[derive(Clone)]
pub struct GraphRagProvider {
	graphrag: GraphRAG,
	working_directory: std::path::PathBuf,
	branch_manifest: Option<BranchManifest>,
	runtime_cache: RuntimeGraphCache,
}

impl GraphRagProvider {
	pub fn new(config: Config, working_directory: std::path::PathBuf) -> Self {
		let branch_manifest = if config.graphrag.enabled {
			crate::indexer::branch::detect_branch_context(&working_directory).and_then(
				|branch_name| {
					let branch_dir =
						crate::storage::get_branch_dir(&working_directory, &branch_name).ok()?;
					crate::indexer::branch::load_manifest(&branch_dir).ok()?
				},
			)
		} else {
			None
		};

		Self {
			graphrag: GraphRAG::new(config, working_directory.clone()),
			working_directory,
			branch_manifest,
			runtime_cache: RuntimeGraphCache::default(),
		}
	}

	pub fn runtime_cache(&self) -> RuntimeGraphCache {
		self.runtime_cache.clone()
	}

	/// Execute the graphrag tool with any operation
	pub async fn execute(&self, arguments: &Value) -> Result<String, McpError> {
		// Parse and validate operation
		let operation_str = arguments
			.get("operation")
			.and_then(|v| v.as_str())
			.ok_or_else(|| McpError::invalid_params("Missing required parameter 'operation': must be one of 'search', 'get-node', 'get-relationships', 'find-path', 'overview'", "graphrag"))?;

		let operation = match operation_str {
			"search" => GraphRAGOperation::Search,
			"get-node" => GraphRAGOperation::GetNode,
			"get-relationships" => GraphRAGOperation::GetRelationships,
			"find-path" => GraphRAGOperation::FindPath,
			"overview" => GraphRAGOperation::Overview,
			_ => return Err(McpError::invalid_params(
				format!("Invalid operation '{}': must be one of 'search', 'get-node', 'get-relationships', 'find-path', 'overview'", operation_str),
				"graphrag"
			))
		};

		// Validate operation-specific parameters
		let (query, node_id, source_id, target_id) = match operation {
			GraphRAGOperation::Search => {
				let query = arguments
					.get("query")
					.and_then(|v| v.as_str())
					.ok_or_else(|| McpError::invalid_params("Missing required parameter 'query' for search operation: must be a detailed question about code relationships or architecture", "graphrag"))?;

				if query.trim().is_empty() {
					return Err(McpError::invalid_params(
						"Invalid query: must not be empty",
						"graphrag",
					));
				}
				if query.len() > 1000 {
					return Err(McpError::invalid_params(
						"Invalid query: must be no more than 1000 characters long",
						"graphrag",
					));
				}

				(Some(query.to_string()), None, None, None)
			}
			GraphRAGOperation::GetNode | GraphRAGOperation::GetRelationships => {
				let node_id = arguments
					.get("node_id")
					.and_then(|v| v.as_str())
					.ok_or_else(|| McpError::invalid_params(
						format!("Missing required parameter 'node_id' for {} operation: must be a valid node identifier", operation_str),
						"graphrag"
					))?;

				(None, Some(node_id.to_string()), None, None)
			}
			GraphRAGOperation::FindPath => {
				let source_id = arguments
					.get("source_id")
					.and_then(|v| v.as_str())
					.ok_or_else(|| McpError::invalid_params("Missing required parameter 'source_id' for find-path operation: must be a valid node identifier", "graphrag"))?;

				let target_id = arguments
					.get("target_id")
					.and_then(|v| v.as_str())
					.ok_or_else(|| McpError::invalid_params("Missing required parameter 'target_id' for find-path operation: must be a valid node identifier", "graphrag"))?;

				(
					None,
					None,
					Some(source_id.to_string()),
					Some(target_id.to_string()),
				)
			}
			GraphRAGOperation::Overview => (None, None, None, None),
		};

		// Parse optional parameters
		let max_depth = arguments
			.get("max_depth")
			.and_then(|v| v.as_u64())
			.unwrap_or(3);
		if !(1..=10).contains(&max_depth) {
			return Err(McpError::invalid_params(
				"Invalid max_depth: must be between 1 and 10",
				"graphrag",
			));
		}
		let max_depth = max_depth as usize;

		let format_str = arguments
			.get("format")
			.and_then(|v| v.as_str())
			.unwrap_or("text");

		let format = match format_str {
			"text" => OutputFormat::Text,
			"json" => OutputFormat::Json,
			"markdown" => OutputFormat::Md,
			_ => {
				return Err(McpError::invalid_params(
					format!(
						"Invalid format '{}': must be one of 'text', 'json', 'markdown'",
						format_str
					),
					"graphrag",
				))
			}
		};

		// Create GraphRAGArgs structure for reusing CLI logic
		let args = GraphRAGArgs {
			operation,
			query,
			node_id,
			source_id,
			target_id,
			max_depth,
			format,
		};

		// Use structured logging for MCP protocol compliance
		debug!(
			operation = %operation_str,
			working_directory = %self.working_directory.display(),
			"Executing GraphRAG operation"
		);

		// Execute the GraphRAG operation. All stores are resolved from
		// `self.working_directory`, so there's no process-wide CWD change.
		let result = self.execute_graphrag_operation(&args).await.map_err(|e| {
			McpError::internal_error(format!("GraphRAG operation failed: {}", e), "graphrag")
		})?;

		Ok(result)
	}

	/// Execute GraphRAG operation using CLI logic with MCP-optimized output
	async fn execute_graphrag_operation(&self, args: &GraphRAGArgs) -> Result<String> {
		let config = self.graphrag.config();
		let graph_builder = if config.graphrag.enabled {
			match indexer::GraphBuilder::new_with_quiet(
				config.clone(),
				&self.working_directory,
				true,
			)
			.await
			{
				Ok(builder) => {
					// Branch-aware persisted enrichment.
					if let Some(ref manifest) = self.branch_manifest {
						let main_commit =
							match crate::store::Store::new_at(&self.working_directory).await {
								Ok(store) => store.get_last_commit_hash().await.ok().flatten(),
								Err(_) => None,
							};
						if crate::indexer::branch::manifest_is_coherent_with(
							manifest,
							main_commit.as_deref(),
						) {
							let overridden = manifest.overridden_paths();
							builder.apply_branch_filter(&overridden).await;
							if let Ok(branch_store) = crate::store::Store::new_for_branch_at(
								&self.working_directory,
								&manifest.branch_name,
							)
							.await
							{
								if let Err(error) = builder.merge_branch_graph(&branch_store).await
								{
									tracing::warn!(%error, "Failed to merge branch GraphRAG data");
								}
							}
						} else {
							tracing::warn!(
								branch = %manifest.branch_name,
								recorded = %manifest.base_db_commit,
								main_now = ?main_commit,
								"Branch GraphRAG overlay skipped: branch DB doesn't match current main commit."
							);
						}
					}
					Some(builder)
				}
				Err(error) => {
					tracing::warn!(%error, "Persisted GraphRAG unavailable; using live structural graph");
					None
				}
			}
		} else {
			None
		};

		let runtime_graph = self.runtime_cache.graph(&self.working_directory).await?;
		let (graph, enrichment_active) = if let Some(builder) = &graph_builder {
			match builder.get_graph().await {
				Ok(enriched) => {
					let active = !enriched.nodes.is_empty() || !enriched.relationships.is_empty();
					(
						std::sync::Arc::new(runtime::merge_enrichment(
							&runtime_graph,
							enriched,
							&self.working_directory,
						)),
						active,
					)
				}
				Err(error) => {
					tracing::warn!(%error, "Failed to load persisted GraphRAG; using live structural graph");
					(runtime_graph, false)
				}
			}
		} else {
			(runtime_graph, false)
		};

		// Check if graph is empty
		if graph.nodes.is_empty() {
			return Err(anyhow::anyhow!(
				"No supported source symbols were found for graph search"
			));
		}

		// Execute the requested operation and capture output
		match args.operation {
			GraphRAGOperation::Search => {
				let query = args
					.query
					.as_deref()
					.ok_or_else(|| anyhow::anyhow!("Search query is missing"))?;
				let nodes = if let Some(builder) = &graph_builder {
					let semantic_nodes = match builder.search_nodes(query).await {
						Ok(nodes) => nodes,
						Err(error) => {
							tracing::warn!(%error, "Semantic graph seed search failed; using lexical seeds");
							Vec::new()
						}
					};
					// Symbols are live AST nodes, never embedding rows. Seed them by
					// deterministic name/path lookup, then append semantic file hits.
					let mut nodes = runtime::search_nodes(&graph, query, 20);
					let mut seen: std::collections::HashSet<String> =
						nodes.iter().map(|node| node.id.clone()).collect();
					for node in semantic_nodes
						.into_iter()
						.filter(|node| !node.is_symbol_node())
					{
						if seen.insert(node.id.clone()) {
							nodes.push(node);
						}
					}
					nodes.truncate(50);
					nodes
				} else {
					runtime::search_nodes(&graph, query, 50)
				};

				// Render based on format
				match args.format {
					OutputFormat::Json => {
						let json_output = serde_json::to_string_pretty(&nodes)
							.map_err(|e| anyhow::anyhow!("JSON serialization failed: {}", e))?;
						Ok(json_output)
					}
					OutputFormat::Md => Ok(indexer::graphrag::graphrag_nodes_to_markdown(&nodes)),
					_ => {
						// Default to text format for token efficiency
						Ok(indexer::graphrag::graphrag_nodes_to_text(&nodes))
					}
				}
			}
			GraphRAGOperation::GetNode => {
				let node_id = args
					.node_id
					.as_deref()
					.ok_or_else(|| anyhow::anyhow!("Node id is missing"))?;
				match find_node_id(&graph, node_id) {
					Some(resolved_id) => {
						let node = &graph.nodes[resolved_id];
						match args.format {
							OutputFormat::Json => {
								Ok(serde_json::to_string_pretty(node)
									.map_err(|e| anyhow::anyhow!("JSON serialization failed: {}", e))?)
							},
							OutputFormat::Md => {
								Ok(format!(
									"# Node: {}\n\n**ID:** {}\n**Kind:** {}\n**Path:** {}\n**Description:** {}\n\n**Symbols:**\n{}\n",
									node.name,
									node.id,
									node.kind,
									node.path,
									node.description,
									node.symbols.iter().map(|s| format!("- {}", s)).collect::<Vec<_>>().join("\n")
								))
							},
							_ => {
								// Text format for token efficiency
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
						}
					}
					None => Err(anyhow::anyhow!("Node not found: {}", node_id)),
				}
			}
			GraphRAGOperation::GetRelationships => {
				let node_id = args
					.node_id
					.as_deref()
					.ok_or_else(|| anyhow::anyhow!("Node id is missing"))?;

				// Resolve node ID with fuzzy matching
				let resolved_id = find_node_id(&graph, node_id)
					.ok_or_else(|| anyhow::anyhow!("Node not found: {}", node_id))?;

				// Find relationships
				let relationships: Vec<_> = graph
					.relationships
					.iter()
					.filter(|rel| rel.source == resolved_id || rel.target == resolved_id)
					.collect();

				if relationships.is_empty() {
					return Ok(format!("No relationships found for node: {}", resolved_id));
				}

				match args.format {
					OutputFormat::Json => Ok(serde_json::to_string_pretty(&relationships)
						.map_err(|e| anyhow::anyhow!("JSON serialization failed: {}", e))?),
					OutputFormat::Md => {
						let mut output = format!("# Relationships for {}\n\n", resolved_id);

						// Outgoing relationships
						let outgoing: Vec<_> = relationships
							.iter()
							.filter(|rel| rel.source == resolved_id)
							.collect();
						if !outgoing.is_empty() {
							output.push_str("## Outgoing Relationships\n\n");
							for rel in outgoing {
								let target_name = graph
									.nodes
									.get(&rel.target)
									.map(|n| n.name.clone())
									.unwrap_or_else(|| rel.target.clone());
								output.push_str(&format!(
									"- **{}** → {} ({}): {}\n",
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
							output.push_str("## Incoming Relationships\n\n");
							for rel in incoming {
								let source_name = graph
									.nodes
									.get(&rel.source)
									.map(|n| n.name.clone())
									.unwrap_or_else(|| rel.source.clone());
								output.push_str(&format!(
									"- **{}** ← {} ({}): {}\n",
									rel.relation_type, source_name, rel.source, rel.description
								));
							}
						}
						Ok(output)
					}
					_ => {
						// Text format for token efficiency
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
				}
			}
			GraphRAGOperation::FindPath => {
				let source_id_input = args
					.source_id
					.as_deref()
					.ok_or_else(|| anyhow::anyhow!("Source node id is missing"))?;
				let target_id_input = args
					.target_id
					.as_deref()
					.ok_or_else(|| anyhow::anyhow!("Target node id is missing"))?;

				// Resolve source and target with fuzzy matching
				let source_id = find_node_id(&graph, source_id_input)
					.ok_or_else(|| anyhow::anyhow!("Source node not found: {}", source_id_input))?
					.to_string();
				let target_id = find_node_id(&graph, target_id_input)
					.ok_or_else(|| anyhow::anyhow!("Target node not found: {}", target_id_input))?
					.to_string();

				// Find paths
				let paths = runtime::find_paths(&graph, &source_id, &target_id, args.max_depth);

				if paths.is_empty() {
					return Ok(format!(
						"No paths found between {} and {} within depth {}",
						source_id, target_id, args.max_depth
					));
				}

				// Build a (source, target) -> relationship index once so each path
				// hop below is an O(1) lookup instead of a linear scan of all edges.
				// `.rev()` makes the first edge in original order win (matches the
				// previous `.find()` first-match semantics).
				let edge_index: std::collections::HashMap<(&str, &str), _> = graph
					.relationships
					.iter()
					.rev()
					.map(|r| ((r.source.as_str(), r.target.as_str()), r))
					.collect();

				match args.format {
					OutputFormat::Json => {
						// Create structured path data
						let path_data: Vec<_> = paths
							.iter()
							.enumerate()
							.map(|(i, path)| {
								json!({
									"path_index": i + 1,
									"nodes": path.iter().map(|node_id| {
										let node_name = graph.nodes.get(node_id)
											.map(|n| n.name.clone())
											.unwrap_or_else(|| node_id.clone());
										json!({"id": node_id, "name": node_name})
									}).collect::<Vec<_>>()
								})
							})
							.collect();
						Ok(serde_json::to_string_pretty(&path_data)
							.map_err(|e| anyhow::anyhow!("JSON serialization failed: {}", e))?)
					}
					OutputFormat::Md => {
						let mut output = format!(
							"# Paths from {} to {}\n\nFound {} paths:\n\n",
							source_id,
							target_id,
							paths.len()
						);
						for (i, path) in paths.iter().enumerate() {
							output.push_str(&format!("## Path {}\n\n", i + 1));
							for (j, node_id) in path.iter().enumerate() {
								let node_name = graph
									.nodes
									.get(node_id)
									.map(|n| n.name.clone())
									.unwrap_or_else(|| node_id.clone());
								if j > 0 {
									let prev_id = &path[j - 1];
									let rel = edge_index
										.get(&(prev_id.as_str(), node_id.as_str()))
										.copied();
									if let Some(rel) = rel {
										output.push_str(&format!(" --{}-> ", rel.relation_type));
									} else {
										output.push_str(" -> ");
									}
								}
								output.push_str(&format!("**{}** ({})", node_name, node_id));
							}
							output.push_str("\n\n");
						}
						Ok(output)
					}
					_ => {
						// Text format for token efficiency
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
									let rel = edge_index
										.get(&(prev_id.as_str(), node_id.as_str()))
										.copied();
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
				}
			}
			GraphRAGOperation::Overview => {
				let graph_mode = if enrichment_active {
					"persisted_enriched"
				} else {
					"runtime_structural"
				};
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

				match args.format {
					OutputFormat::Json => {
						let overview = json!({
							"mode": graph_mode,
							"node_count": node_count,
							"relationship_count": relationship_count,
							"node_types": node_types,
							"relationship_types": rel_types
						});
						Ok(serde_json::to_string_pretty(&overview)
							.map_err(|e| anyhow::anyhow!("JSON serialization failed: {}", e))?)
					}
					OutputFormat::Md => {
						let mut output = format!("# Code Graph Overview\n\nMode: `{}`\n\nThe graph contains {} nodes and {} relationships.\n\n", graph_mode, node_count, relationship_count);

						output.push_str("## Node Types\n\n");
						for (kind, count) in node_types.iter() {
							output.push_str(&format!("- **{}**: {} nodes\n", kind, count));
						}

						output.push_str("\n## Relationship Types\n\n");
						for (rel_type, count) in rel_types.iter() {
							output.push_str(&format!(
								"- **{}**: {} relationships\n",
								rel_type, count
							));
						}
						Ok(output)
					}
					_ => {
						// Text format for token efficiency
						let mut output = format!(
							"Code Graph Overview ({}): {} nodes, {} relationships\n\n",
							graph_mode, node_count, relationship_count
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
				}
			}
		}
	}
}
