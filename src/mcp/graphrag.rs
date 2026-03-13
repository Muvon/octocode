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

use anyhow::Result;
use serde_json::{json, Value};
use tracing::debug;

use crate::config::Config;
use crate::indexer::{self, graphrag::GraphRAG};
use crate::mcp::types::{McpError, McpTool};

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
}

impl GraphRagProvider {
	pub fn new(config: Config, working_directory: std::path::PathBuf) -> Option<Self> {
		if config.graphrag.enabled {
			Some(Self {
				graphrag: GraphRAG::new(config),
				working_directory,
			})
		} else {
			None
		}
	}

	/// Get the tool definition for graphrag
	pub fn get_tool_definition() -> McpTool {
		McpTool {
			name: "graphrag".to_string(),
			description: "Knowledge graph operations over the indexed codebase. Use for architectural queries: component relationships, dependency chains, data flows. For simple code lookup use semantic_search instead.".to_string(),
			input_schema: json!({
				"type": "object",
				"properties": {
					"operation": {
						"type": "string",
						"enum": ["search", "get-node", "get-relationships", "find-path", "overview"],
						"description": "'search' (semantic node search), 'get-node' (node details), 'get-relationships' (node connections), 'find-path' (path between two nodes), 'overview' (graph stats)"
					},
					"query": {
						"type": "string",
						"description": "Search query for 'search' operation",
						"minLength": 10,
						"maxLength": 1000
					},
					"node_id": {
						"type": "string",
						"description": "Node ID for 'get-node'/'get-relationships' (format: 'path/to/file' or 'path/to/file/symbol')"
					},
					"source_id": {
						"type": "string",
						"description": "Source node ID for 'find-path'"
					},
					"target_id": {
						"type": "string",
						"description": "Target node ID for 'find-path'"
					},
					"max_depth": {
						"type": "integer",
						"description": "Max path depth for 'find-path' (default: 3)",
						"minimum": 1,
						"maximum": 10,
						"default": 3
					},
					"format": {
						"type": "string",
						"enum": ["text", "json", "markdown"],
						"description": "Output format (default: 'text')",
						"default": "text"
					},
				},
				"required": ["operation"],
				"additionalProperties": false
			}),
		}
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

				if query.len() < 10 {
					return Err(McpError::invalid_params("Invalid query: must be at least 10 characters long and describe relationships or architecture", "graphrag"));
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
			.unwrap_or(3) as usize;

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

		// Change to the working directory for the operation
		let original_dir = std::env::current_dir().map_err(|e| {
			McpError::internal_error(
				format!("Failed to get current directory: {}", e),
				"graphrag",
			)
		})?;
		std::env::set_current_dir(&self.working_directory).map_err(|e| {
			McpError::internal_error(format!("Failed to change directory: {}", e), "graphrag")
		})?;

		// Execute the GraphRAG operation using CLI logic
		let result = self.execute_graphrag_operation(&args).await.map_err(|e| {
			McpError::internal_error(format!("GraphRAG operation failed: {}", e), "graphrag")
		})?;

		// Restore original directory
		std::env::set_current_dir(&original_dir).map_err(|e| {
			McpError::internal_error(format!("Failed to restore directory: {}", e), "graphrag")
		})?;

		Ok(result)
	}

	/// Execute GraphRAG operation using CLI logic with MCP-optimized output
	async fn execute_graphrag_operation(&self, args: &GraphRAGArgs) -> Result<String> {
		// Check if GraphRAG is enabled (this should always be true since we're created conditionally)
		let config = self.graphrag.config();
		if !config.graphrag.enabled {
			return Err(anyhow::anyhow!("GraphRAG is not enabled in configuration"));
		}

		// Initialize the GraphBuilder
		let graph_builder = indexer::GraphBuilder::new_with_quiet(config.clone(), true)
			.await
			.map_err(|e| anyhow::anyhow!("Failed to initialize GraphRAG system: {}", e))?;

		// Get the current graph
		let graph = graph_builder
			.get_graph()
			.await
			.map_err(|e| anyhow::anyhow!("Failed to load GraphRAG knowledge graph: {}", e))?;

		// Check if graph is empty
		if graph.nodes.is_empty() {
			return Err(anyhow::anyhow!("GraphRAG knowledge graph is empty. Run 'octocode index' to build the knowledge graph."));
		}

		// Execute the requested operation and capture output
		match args.operation {
			GraphRAGOperation::Search => {
				let query = args.query.as_ref().unwrap(); // Validated in caller
				let nodes = graph_builder
					.search_nodes(query)
					.await
					.map_err(|e| anyhow::anyhow!("Search failed: {}", e))?;

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
				let node_id = args.node_id.as_ref().unwrap(); // Validated in caller
				match graph.nodes.get(node_id) {
					Some(node) => {
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
				let node_id = args.node_id.as_ref().unwrap(); // Validated in caller

				// Check if node exists
				if !graph.nodes.contains_key(node_id) {
					return Err(anyhow::anyhow!("Node not found: {}", node_id));
				}

				// Find relationships
				let relationships: Vec<_> = graph
					.relationships
					.iter()
					.filter(|rel| rel.source == *node_id || rel.target == *node_id)
					.collect();

				if relationships.is_empty() {
					return Ok(format!("No relationships found for node: {}", node_id));
				}

				match args.format {
					OutputFormat::Json => Ok(serde_json::to_string_pretty(&relationships)
						.map_err(|e| anyhow::anyhow!("JSON serialization failed: {}", e))?),
					OutputFormat::Md => {
						let mut output = format!("# Relationships for {}\n\n", node_id);

						// Outgoing relationships
						let outgoing: Vec<_> = relationships
							.iter()
							.filter(|rel| rel.source == *node_id)
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
							.filter(|rel| rel.target == *node_id)
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
							node_id,
							relationships.len()
						);

						// Outgoing relationships
						let outgoing: Vec<_> = relationships
							.iter()
							.filter(|rel| rel.source == *node_id)
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
							.filter(|rel| rel.target == *node_id)
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
				let source_id = args.source_id.as_ref().unwrap(); // Validated in caller
				let target_id = args.target_id.as_ref().unwrap(); // Validated in caller

				// Find paths
				let paths = graph_builder
					.find_paths(source_id, target_id, args.max_depth)
					.await
					.map_err(|e| anyhow::anyhow!("Path finding failed: {}", e))?;

				if paths.is_empty() {
					return Ok(format!(
						"No paths found between {} and {} within depth {}",
						source_id, target_id, args.max_depth
					));
				}

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
				}
			}
			GraphRAGOperation::Overview => {
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
							"node_count": node_count,
							"relationship_count": relationship_count,
							"node_types": node_types,
							"relationship_types": rel_types
						});
						Ok(serde_json::to_string_pretty(&overview)
							.map_err(|e| anyhow::anyhow!("JSON serialization failed: {}", e))?)
					}
					OutputFormat::Md => {
						let mut output = format!("# GraphRAG Knowledge Graph Overview\n\nThe knowledge graph contains {} nodes and {} relationships.\n\n", node_count, relationship_count);

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
				}
			}
		}
	}
}
