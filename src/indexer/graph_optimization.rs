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

// Graph Optimization module for Octocode GraphRAG
// Provides optimized graph extraction and retrieval for AI sessions

use crate::indexer::graphrag::{CodeGraph, CodeNode, CodeRelationship};
use crate::store::CodeBlock;
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// A structure that holds an optimized task-focused subgraph
#[derive(Debug, Clone)]
pub struct TaskFocusedSubgraph {
	pub nodes: Vec<CodeNode>,
	pub relationships: Vec<CodeRelationship>,
	pub relevant_files: HashSet<String>,
	pub key_concepts: HashMap<String, f32>, // Concept to relevance score mapping
}

impl Default for TaskFocusedSubgraph {
	fn default() -> Self {
		Self::new()
	}
}

impl TaskFocusedSubgraph {
	/// Creates a new empty TaskFocusedSubgraph
	pub fn new() -> Self {
		Self {
			nodes: Vec::new(),
			relationships: Vec::new(),
			relevant_files: HashSet::new(),
			key_concepts: HashMap::new(),
		}
	}

	/// Returns the number of tokens this subgraph would require
	pub fn estimated_token_count(&self) -> usize {
		// Rough estimate of tokens per node and relationship
		// This is an approximation and should be adjusted based on observed token usage
		const TOKENS_PER_NODE: usize = 100; // Includes id, name, description, etc.
		const TOKENS_PER_RELATIONSHIP: usize = 50; // Includes source, target, type, description

		let node_tokens = self.nodes.len() * TOKENS_PER_NODE;
		let relationship_tokens = self.relationships.len() * TOKENS_PER_RELATIONSHIP;

		node_tokens + relationship_tokens
	}

	/// Converts the subgraph to a concise markdown representation
	pub fn to_markdown(&self) -> String {
		let mut markdown = String::new();

		// Add heading
		markdown.push_str(&format!(
			"# Code Knowledge Graph: {} nodes, {} relationships\n\n",
			self.nodes.len(),
			self.relationships.len()
		));

		// Add key concepts section if we have any
		if !self.key_concepts.is_empty() {
			markdown.push_str("## Key Concepts\n\n");

			// Sort concepts by relevance score (highest first)
			let mut concepts: Vec<_> = self.key_concepts.iter().collect();
			concepts.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

			// Display top concepts (limit to 10 to save tokens)
			for (concept, relevance) in concepts.iter().take(10) {
				markdown.push_str(&format!(
					"- **{}** (relevance: {:.2})\n",
					concept, relevance
				));
			}
			markdown.push('\n');
		}

		// Add relevant files section
		if !self.relevant_files.is_empty() {
			markdown.push_str("## Relevant Files\n\n");

			let mut files: Vec<_> = self.relevant_files.iter().collect();
			files.sort(); // Sort alphabetically for consistent display

			for file in files.iter().take(15) {
				// Limit to 15 files to save tokens
				markdown.push_str(&format!("- `{}`\n", file));
			}

			if self.relevant_files.len() > 15 {
				markdown.push_str(&format!(
					"- *(and {} more files)*\n",
					self.relevant_files.len() - 15
				));
			}

			markdown.push('\n');
		}

		// Add important nodes section (limited selection)
		if !self.nodes.is_empty() {
			markdown.push_str("## Key Components\n\n");

			// Sort nodes by a heuristic of importance (could be customized)
			// Here we're just taking a few examples from different kinds
			let mut node_by_kind: HashMap<String, Vec<&CodeNode>> = HashMap::new();

			for node in &self.nodes {
				node_by_kind
					.entry(node.kind.clone())
					.or_default()
					.push(node);
			}

			let mut total_nodes_shown = 0;
			const MAX_NODES_TO_SHOW: usize = 20; // Limit to conserve tokens

			for (kind, nodes) in node_by_kind.iter() {
				markdown.push_str(&format!("### {}s\n\n", kind.to_uppercase()));

				for node in nodes.iter().take(5) {
					markdown.push_str(&format!("- **{}**: {}\n", node.name, node.description));
					total_nodes_shown += 1;

					if total_nodes_shown >= MAX_NODES_TO_SHOW {
						break;
					}
				}

				if nodes.len() > 5 {
					markdown.push_str(&format!("- *(and {} more {}s)*\n", nodes.len() - 5, kind));
				}

				markdown.push('\n');

				if total_nodes_shown >= MAX_NODES_TO_SHOW {
					break;
				}
			}
		}

		// Add relationship examples section (very limited)
		if !self.relationships.is_empty() {
			markdown.push_str("## Relationships\n\n");

			// Group relationships by type
			let mut rels_by_type: HashMap<String, Vec<&CodeRelationship>> = HashMap::new();

			for rel in &self.relationships {
				rels_by_type
					.entry(rel.relation_type.to_string())
					.or_default()
					.push(rel);
			}

			// Show a limited number of relationship types and examples
			let mut rel_types: Vec<_> = rels_by_type.iter().collect();
			rel_types.sort_by(|a, b| b.1.len().cmp(&a.1.len())); // Sort by frequency

			for (rel_type, rels) in rel_types.iter().take(5) {
				// Show top 5 relationship types
				markdown.push_str(&format!("### {} relationships\n\n", rel_type));

				// Show just a few examples of each type
				for rel in rels.iter().take(3) {
					// Extract just the function/class name from the full path+id
					let source_name = rel.source.split('/').next_back().unwrap_or(&rel.source);
					let target_name = rel.target.split('/').next_back().unwrap_or(&rel.target);

					markdown.push_str(&format!("- `{}` → `{}`\n", source_name, target_name));
				}

				if rels.len() > 3 {
					markdown.push_str(&format!(
						"- *(and {} more {} relationships)*\n",
						rels.len() - 3,
						rel_type
					));
				}

				markdown.push('\n');
			}
		}

		markdown
	}

	/// Add a node to the subgraph
	pub fn add_node(&mut self, node: CodeNode) {
		// Track the file
		self.relevant_files.insert(node.path.clone());

		// Only add if not already present
		if !self.nodes.iter().any(|n| n.id == node.id) {
			self.nodes.push(node);
		}
	}

	/// Add a relationship to the subgraph
	pub fn add_relationship(&mut self, relationship: CodeRelationship) {
		// Only add if not already present
		if !self.relationships.iter().any(|r| {
			r.source == relationship.source
				&& r.target == relationship.target
				&& r.relation_type == relationship.relation_type
		}) {
			self.relationships.push(relationship);
		}
	}

	/// Add a key concept with its relevance score
	pub fn add_key_concept(&mut self, concept: String, relevance: f32) {
		self.key_concepts.insert(concept, relevance);
	}
}

/// Functions for extracting task-specific subgraphs
pub struct GraphOptimizer {
	// Uses token budget to limit the size of the extracted subgraph
	pub max_token_budget: usize,
}

impl GraphOptimizer {
	pub fn new(max_token_budget: usize) -> Self {
		Self { max_token_budget }
	}

	/// Extract an optimized subgraph given a task description and the full graph
	pub async fn extract_task_subgraph(
		&self,
		_task_description: &str,
		query_embedding: &[f32],
		full_graph: &CodeGraph,
	) -> Result<TaskFocusedSubgraph> {
		let mut subgraph = TaskFocusedSubgraph::new();

		// 1. Find the most relevant nodes based on the task description
		let relevant_nodes = self.find_relevant_nodes(query_embedding, full_graph, 20)?;

		// 2. Add these nodes to our subgraph
		for (node, relevance) in &relevant_nodes {
			subgraph.add_node(node.clone());

			// Extract concepts from the node name and description
			self.extract_key_concepts(&mut subgraph, node, *relevance);

			// Check if we've reached our token budget
			if subgraph.estimated_token_count() > self.max_token_budget {
				break;
			}
		}

		// 3. Find direct relationships between these nodes
		let node_ids: HashSet<String> = relevant_nodes
			.iter()
			.map(|(node, _)| node.id.clone())
			.collect();

		for relationship in &full_graph.relationships {
			if node_ids.contains(&relationship.source) && node_ids.contains(&relationship.target) {
				subgraph.add_relationship(relationship.clone());
			}
		}

		// 4. Add additional important nodes that are directly related to our relevant nodes
		let mut additional_nodes = HashSet::new();

		for relationship in &full_graph.relationships {
			// If one endpoint is in our subgraph, consider adding the other
			if node_ids.contains(&relationship.source) && !node_ids.contains(&relationship.target) {
				additional_nodes.insert(relationship.target.clone());
			} else if node_ids.contains(&relationship.target)
				&& !node_ids.contains(&relationship.source)
			{
				additional_nodes.insert(relationship.source.clone());
			}

			// Check if we're exceeding our token budget
			if subgraph.estimated_token_count() > self.max_token_budget {
				break;
			}
		}

		// Add a limited number of these additional nodes
		let mut added = 0;
		for node_id in additional_nodes {
			if let Some(node) = full_graph.nodes.get(&node_id) {
				if added < 20 {
					// Limit to 20 additional nodes
					subgraph.add_node(node.clone());
					added += 1;

					// Add relationships that include this node
					for relationship in &full_graph.relationships {
						if (relationship.source == node_id || relationship.target == node_id)
							&& subgraph.nodes.iter().any(|n| n.id == relationship.source)
							&& subgraph.nodes.iter().any(|n| n.id == relationship.target)
						{
							subgraph.add_relationship(relationship.clone());
						}
					}
				} else {
					break;
				}
			}
		}

		Ok(subgraph)
	}

	/// Find the most relevant nodes for a query
	fn find_relevant_nodes(
		&self,
		query_embedding: &[f32],
		graph: &CodeGraph,
		limit: usize,
	) -> Result<Vec<(CodeNode, f32)>> {
		let mut similarities = Vec::new();

		for node in graph.nodes.values() {
			let similarity = cosine_similarity(query_embedding, &node.embedding);
			similarities.push((node.clone(), similarity));
		}

		// Sort by similarity (highest first)
		similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

		// Return top matches
		Ok(similarities.into_iter().take(limit).collect())
	}

	/// Extract key concepts from a node and add them to the subgraph
	fn extract_key_concepts(
		&self,
		subgraph: &mut TaskFocusedSubgraph,
		node: &CodeNode,
		relevance: f32,
	) {
		// Add the node name as a concept
		subgraph.add_key_concept(node.name.clone(), relevance);

		// Add the node kind as a concept
		subgraph.add_key_concept(node.kind.clone(), relevance * 0.8);

		// Extract additional concepts from node description
		// This is a simple approach - in a real implementation, you might use NLP
		// to extract more meaningful concepts
		let words: Vec<&str> = node
			.description
			.split_whitespace()
			.filter(|w| w.len() > 4) // Only consider longer words as potential concepts
			.collect();

		for word in words {
			// Skip common words, focus on technical terms
			if is_likely_technical_term(word) {
				subgraph.add_key_concept(
					word.trim_matches(|c: char| !c.is_alphanumeric())
						.to_string(),
					relevance * 0.5,
				);
			}
		}
	}

	/// Generate a summarized view of the code based on a task description
	pub async fn generate_task_focused_view(
		&self,
		task_description: &str,
		query_embedding: &[f32],
		full_graph: &CodeGraph,
		code_blocks: &[CodeBlock],
	) -> Result<String> {
		// 1. Extract the task-specific subgraph
		let subgraph = self
			.extract_task_subgraph(task_description, query_embedding, full_graph)
			.await?;

		// 2. Convert to markdown representation
		let graph_markdown = subgraph.to_markdown();

		// 3. Find the most relevant code snippets based on the key concepts
		let relevant_snippets =
			self.find_relevant_code_snippets(query_embedding, &subgraph, code_blocks, 5)?;

		// 4. Combine the graph overview with code snippets
		let mut result = String::new();

		result.push_str("# Task-Focused Code Overview\n\n");
		result.push_str(&format!("**Task:** {}\n\n", task_description));

		// Add the graph summary
		result.push_str("## Knowledge Graph Summary\n\n");
		result.push_str(&graph_markdown);

		// Add relevant code snippets
		if !relevant_snippets.is_empty() {
			result.push_str("## Relevant Code Snippets\n\n");

			for (idx, (block, similarity)) in relevant_snippets.iter().enumerate() {
				result.push_str(&format!(
					"### Snippet {} (Relevance: {:.2})\n\n",
					idx + 1,
					similarity
				));
				result.push_str(&format!("File: `{}`\n\n", block.path));

				// Add symbols if available
				if !block.symbols.is_empty() {
					let display_symbols: Vec<_> =
						block.symbols.iter().filter(|s| !s.contains('_')).collect();

					if !display_symbols.is_empty() {
						result.push_str("**Symbols:** ");
						for (i, symbol) in display_symbols.iter().enumerate() {
							if i > 0 {
								result.push_str(", ");
							}
							result.push_str(&format!("`{}`", symbol));
						}
						result.push_str("\n\n");
					}
				}

				// Add the code with syntax highlighting
				result.push_str("```");
				if !block.language.is_empty() && block.language != "text" {
					result.push_str(&block.language);
				}
				result.push('\n');

				// Truncate long code blocks to save tokens
				let lines: Vec<&str> = block.content.lines().collect();
				if lines.len() > 20 {
					// Show first 10 lines
					for line in lines.iter().take(10) {
						result.push_str(&format!("{}{}", line, "\n"));
					}
					// Add indicator of omitted lines
					result.push_str(&format!(
						"// ... {} lines omitted ...{}\n",
						lines.len() - 20,
						if !lines.is_empty() {
							" for brevity"
						} else {
							""
						}
					));
					// Show last 10 lines
					for line in lines.iter().skip(lines.len() - 10) {
						result.push_str(&format!("{}{}", line, "\n"));
					}
				} else {
					// Show the entire block if it's not too long
					result.push_str(&block.content);
					if !block.content.ends_with('\n') {
						result.push('\n');
					}
				}

				result.push_str("```\n\n");
			}
		}

		Ok(result)
	}

	/// Find the most relevant code snippets for the task
	fn find_relevant_code_snippets(
		&self,
		query_embedding: &[f32],
		subgraph: &TaskFocusedSubgraph,
		code_blocks: &[CodeBlock],
		limit: usize,
	) -> Result<Vec<(CodeBlock, f32)>> {
		let mut relevant_blocks = Vec::new();

		// Filter blocks to only those in our subgraph's relevant files
		let filtered_blocks: Vec<_> = code_blocks
			.iter()
			.filter(|block| subgraph.relevant_files.contains(&block.path))
			.collect();

		// Score each block by relevance to our query
		for block in filtered_blocks {
			// Check if the block contains any key concepts
			let contains_key_concept = !block.symbols.is_empty()
				&& block
					.symbols
					.iter()
					.any(|symbol| subgraph.key_concepts.contains_key(symbol));

			// If contains key concepts, boost the similarity score
			let mut similarity =
				cosine_similarity(query_embedding, &generate_block_embedding(block)?);
			if contains_key_concept {
				similarity *= 1.5; // Boost blocks containing key concepts
			}

			// Only include reasonably relevant blocks
			if similarity > 0.5 {
				relevant_blocks.push((block.clone(), similarity));
			}
		}

		// Sort by relevance
		relevant_blocks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

		// Take the top N most relevant blocks
		Ok(relevant_blocks.into_iter().take(limit).collect())
	}
}

/// Generate an embedding for a code block using its content and symbols
fn generate_block_embedding(block: &CodeBlock) -> Result<Vec<f32>> {
	// In a real implementation, you would call the embedding model here
	// For now, we'll just use the block's content hash as a placeholder
	// This would be replaced with actual embedding generation

	// Placeholder implementation - in real code, call the embedding model
	let mut result = vec![0.0; 128]; // Assuming 128-dimensional embeddings

	// Use the hash to create a deterministic "fake" embedding
	// This is only for demonstration
	let hash_bytes = block.hash.as_bytes();
	for (i, byte) in hash_bytes.iter().enumerate() {
		let idx = i % result.len();
		result[idx] = (*byte as f32) / 255.0;
	}

	// Normalize the embedding
	let norm: f32 = result.iter().map(|v| v * v).sum::<f32>().sqrt();
	if norm > 0.0 {
		for val in result.iter_mut() {
			*val /= norm;
		}
	}

	Ok(result)
}

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
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

/// Check if a word is likely to be a technical term
fn is_likely_technical_term(word: &str) -> bool {
	let word = word
		.trim_matches(|c: char| !c.is_alphanumeric())
		.to_lowercase();

	// Skip common English words
	let common_words = [
		"about", "after", "again", "below", "could", "every", "first", "found", "great", "house",
		"large", "learn", "never", "other", "place", "plant", "point", "right", "small", "sound",
		"spell", "still", "study", "their", "there", "these", "thing", "think", "three", "water",
		"where", "which", "world", "would", "write",
	];

	if common_words.contains(&word.as_str()) {
		return false;
	}

	// Words with mixed case are likely technical terms (camelCase, PascalCase)
	let has_mixed_case =
		word.chars().any(|c| c.is_uppercase()) && word.chars().any(|c| c.is_lowercase());

	// Words with underscores are likely technical terms
	let has_underscore = word.contains('_');

	has_mixed_case || has_underscore || word.len() > 6
}
