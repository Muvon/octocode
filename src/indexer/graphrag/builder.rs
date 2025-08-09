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

// GraphRAG core builder implementation

use crate::config::Config;
use crate::embedding::{
	calculate_unique_content_hash, create_embedding_provider_from_parts,
	types::parse_provider_model, EmbeddingProvider,
};
use crate::indexer::graphrag::ai::AIEnhancements;
use crate::indexer::graphrag::database::DatabaseOperations;
use crate::indexer::graphrag::relationships::RelationshipDiscovery;
use crate::indexer::graphrag::types::{CodeGraph, CodeNode, CodeRelationship};
use crate::indexer::graphrag::utils::{cosine_similarity, detect_project_root, to_relative_path};
use crate::state::SharedState;
use crate::store::{CodeBlock, Store};
use anyhow::{Context, Result};
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

// Manages the creation and storage of the code graph with project-relative paths
pub struct GraphBuilder {
	config: Config,
	graph: Arc<RwLock<CodeGraph>>,
	embedding_provider: Arc<Box<dyn EmbeddingProvider>>,
	store: Store,
	project_root: PathBuf, // Project root for relative path calculations
	ai_enhancements: Option<AIEnhancements>,
	quiet: bool, // Quiet flag for suppressing console output
}

impl GraphBuilder {
	pub async fn new(config: Config) -> Result<Self> {
		Self::new_with_quiet(config, false).await
	}

	pub async fn new_with_quiet(config: Config, quiet: bool) -> Result<Self> {
		// Detect project root (look for common indicators)
		let project_root = detect_project_root()?;

		// Initialize embedding provider from config (using text model for graph descriptions)
		// GraphRAG uses text embeddings for file descriptions and relationships, not code embeddings
		let model_string = &config.embedding.text_model;
		let (provider_type, model) = parse_provider_model(model_string);
		let embedding_provider = Arc::new(
			create_embedding_provider_from_parts(&provider_type, &model)
				.await
				.context("Failed to initialize embedding provider from config")?,
		);

		// Initialize the store for database access
		let store = Store::new().await?;

		// Load existing graph from database
		let db_ops = DatabaseOperations::new(&store);
		let graph = Arc::new(RwLock::new(db_ops.load_graph(&project_root, quiet).await?));

		// Initialize AI enhancements if enabled
		let client = Client::new();
		let ai_enhancements = if config.graphrag.use_llm {
			Some(AIEnhancements::new(config.clone(), client.clone(), quiet))
		} else {
			None
		};

		Ok(Self {
			config,
			graph,
			embedding_provider,
			store,
			project_root,
			ai_enhancements,
			quiet,
		})
	}

	// Legacy method for backward compatibility
	pub async fn new_with_ai_enhancements(
		config: Config,
		_use_ai_enhancements: bool,
	) -> Result<Self> {
		// Note: _use_ai_enhancements parameter is ignored, using config.graphrag.use_llm instead
		Self::new(config).await
	}

	// Check if LLM enhancements are enabled
	fn llm_enabled(&self) -> bool {
		self.config.graphrag.use_llm
	}

	// Convert absolute path to relative path from project root
	fn to_relative_path(&self, absolute_path: &str) -> Result<String> {
		to_relative_path(absolute_path, &self.project_root)
	}

	// Generate an embedding for node content
	async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
		self.embedding_provider.generate_embedding(text).await
	}

	// Process files efficiently using existing code blocks for better performance
	pub async fn process_files_from_codeblocks(
		&self,
		code_blocks: &[CodeBlock],
		state: Option<SharedState>,
	) -> Result<()> {
		let mut new_nodes: Vec<CodeNode> = Vec::new();
		let mut pending_embeddings = Vec::new(); // For batch embedding generation
		let mut ai_batch_queue: Vec<crate::indexer::graphrag::ai::FileForAI> = Vec::new(); // For batch AI processing
		let mut ai_descriptions: HashMap<String, String> = HashMap::new(); // Store AI descriptions by file_path
		let mut processed_count = 0;
		let mut skipped_count = 0;
		let mut batches_processed = 0;

		// Group code blocks by file for efficient processing
		let mut files_to_blocks: HashMap<String, Vec<&CodeBlock>> = HashMap::new();
		for block in code_blocks {
			files_to_blocks
				.entry(block.path.clone())
				.or_default()
				.push(block);
		}

		// Process each file
		for (file_path, file_blocks) in files_to_blocks {
			// Convert to relative path
			let relative_path = match self.to_relative_path(&file_path) {
				Ok(path) => path,
				Err(_) => {
					if !self.quiet {
						eprintln!("Warning: Skipping file outside project root: {}", file_path);
					}
					continue;
				}
			};

			// Calculate file hash based on all blocks
			let combined_content: String = file_blocks
				.iter()
				.map(|b| b.content.as_str())
				.collect::<Vec<_>>()
				.join("\n");
			let content_hash = calculate_unique_content_hash(&combined_content, &file_path);

			// Check if we already have this file with the same hash
			let graph = self.graph.read().await;
			let needs_processing = match graph.nodes.get(&relative_path) {
				Some(existing_node) if existing_node.hash == content_hash => {
					skipped_count += 1;
					false
				}
				_ => true,
			};
			drop(graph);

			if needs_processing {
				// CRITICAL FIX: Clean up old GraphRAG data for this file if it exists
				// This ensures we don't have stale data when a file is reprocessed
				if let Err(e) = self.store.remove_graph_nodes_by_path(&relative_path).await {
					if !self.quiet {
						eprintln!(
							"Warning: Failed to clean up old GraphRAG data for {}: {}",
							relative_path, e
						);
					}
				}

				// CRITICAL FIX: Also remove from in-memory graph to prevent duplicates
				{
					let mut graph = self.graph.write().await;
					if graph.nodes.remove(&relative_path).is_some() && !self.quiet {
						eprintln!("🗑️  Removed stale in-memory node: {}", relative_path);
					}
				}

				// Extract file information efficiently
				let file_name = Path::new(&file_path)
					.file_stem()
					.and_then(|s| s.to_str())
					.unwrap_or("unknown")
					.to_string();

				// Determine file kind based on path patterns
				let kind = RelationshipDiscovery::determine_file_kind(&relative_path);

				// Extract language from the first block (should be consistent)
				let language = file_blocks
					.first()
					.map(|b| b.language.clone())
					.unwrap_or_else(|| "unknown".to_string());

				// Collect all symbols from all blocks
				let mut all_symbols = HashSet::new();
				let mut all_functions = Vec::new();
				let mut total_lines = 0;

				for block in &file_blocks {
					all_symbols.extend(block.symbols.iter().cloned());
					total_lines = total_lines.max(block.end_line);

					// Extract function information from this block
					if let Ok(functions) =
						RelationshipDiscovery::extract_functions_from_block(block)
					{
						all_functions.extend(functions);
					}
				}

				let symbols: Vec<String> = all_symbols.into_iter().collect();

				// Extract imports and exports using language-specific AST parsing
				let (imports, exports) = self
					.extract_imports_exports_from_file(&file_path, &language)
					.await
					.unwrap_or_else(|e| {
						if !self.quiet {
							eprintln!(
								"⚠️  Import/export extraction failed for {}: {}",
								relative_path, e
							);
						}
						// Fallback to old method if AST parsing fails
						RelationshipDiscovery::extract_imports_exports_efficient(
							&symbols,
							&language,
							&relative_path,
						)
					});

				if !self.quiet && (!imports.is_empty() || !exports.is_empty()) {
					eprintln!(
						"📦 Found {} imports, {} exports in {}",
						imports.len(),
						exports.len(),
						relative_path
					);
					if !imports.is_empty() {
						eprintln!("  Imports: {:?}", imports);
					}
					if !exports.is_empty() {
						eprintln!("  Exports: {:?}", exports);
					}
				}

				// Generate description - collect for batch AI processing when enabled
				let description = if self.llm_enabled()
					&& self.should_use_ai_for_description(&symbols, total_lines as u32, &language)
				{
					if !self.quiet {
						eprintln!(
							"🤖 Collecting for AI batch: {} ({} lines, {} symbols)",
							relative_path,
							total_lines,
							symbols.len()
						);
					}

					// Collect file for batch processing
					let content_sample = self.build_content_sample_for_ai(&file_blocks);
					let file_for_ai = crate::indexer::graphrag::ai::FileForAI {
						file_id: relative_path.clone(), // FIXED: Use relative_path to match node.id
						file_path: file_path.clone(),
						language: language.clone(),
						symbols: symbols.clone(),
						content_sample,
						function_count: symbols
							.iter()
							.filter(|s| {
								s.contains("fn ") || s.contains("function ") || s.contains("def ")
							})
							.count(),
						class_count: symbols
							.iter()
							.filter(|s| {
								s.contains("class ")
									|| s.contains("struct ") || s.contains("interface ")
							})
							.count(),
					};
					ai_batch_queue.push(file_for_ai);

					// Process AI batch when it reaches configured size
					if ai_batch_queue.len() >= self.config.graphrag.llm.ai_batch_size {
						if !self.quiet {
							eprintln!("🚀 Processing AI batch: {} files", ai_batch_queue.len());
						}

						// Call batch AI extraction through AI enhancements
						if let Some(ref ai_enhancements) = self.ai_enhancements {
							match ai_enhancements
								.extract_ai_descriptions_batch(&ai_batch_queue)
								.await
							{
								Ok(batch_descriptions) => {
									// Store all descriptions from batch
									for (file_path, description) in batch_descriptions {
										ai_descriptions.insert(file_path, description);
									}
									if !self.quiet {
										eprintln!(
											"✅ AI batch processing completed: {} descriptions",
											ai_descriptions.len()
										);
									}
								}
								Err(e) => {
									if !self.quiet {
										eprintln!("⚠️  AI batch processing failed: {}", e);
									}
								}
							}
						}

						// Clear the processed batch
						ai_batch_queue.clear();

						// Update node descriptions immediately with newly generated AI descriptions
						if !ai_descriptions.is_empty() {
							// Update nodes in the graph with AI descriptions
							{
								let mut graph = self.graph.write().await;
								for (file_path, ai_description) in &ai_descriptions {
									if let Some(node) = graph.nodes.get_mut(file_path) {
										node.description = ai_description.clone();
									}
								}
							}

							// Also update any pending nodes that haven't been added to graph yet
							for node in &mut new_nodes {
								if let Some(ai_description) = ai_descriptions.get(&node.id) {
									node.description = ai_description.clone();
								}
							}
						}
					}

					// For now, use simple description - will be replaced after batch processing
					RelationshipDiscovery::generate_simple_description(
						&file_name,
						&language,
						&symbols,
						total_lines as u32,
					)
				} else {
					if !self.quiet && self.llm_enabled() {
						eprintln!(
							"📝 Using simple description for: {} (AI criteria not met)",
							relative_path
						);
					}
					RelationshipDiscovery::generate_simple_description(
						&file_name,
						&language,
						&symbols,
						total_lines as u32,
					)
				};

				// Generate summary text for embedding (much lighter than full content)
				let summary_text =
					format!("{} {} symbols: {}", file_name, language, symbols.join(" "));

				// Store summary text for batch embedding generation
				pending_embeddings.push(summary_text);

				// Create the file node without embedding (will be added later)
				let node = CodeNode {
					id: relative_path.clone(),
					name: file_name,
					kind,
					path: relative_path.clone(),
					description,
					symbols,
					imports,
					exports,
					functions: all_functions,
					hash: content_hash,
					embedding: Vec::new(), // Will be filled after batch embedding
					size_lines: total_lines as u32,
					language,
				};

				new_nodes.push(node);
				processed_count += 1;

				// Update state if provided
				if let Some(ref state) = state {
					let mut state_guard = state.write();
					state_guard.status_message = format!("Processing file: {}", file_path);
				}

				// Check if we should process batch (same logic as normal indexing)
				if self.should_process_batch(&pending_embeddings) {
					self.process_nodes_batch(
						&mut new_nodes,
						&mut pending_embeddings,
						&mut batches_processed,
						&ai_descriptions, // Pass AI descriptions
					)
					.await?;
				}
			}
		}

		// Process any remaining AI batch queue
		if !ai_batch_queue.is_empty() {
			if !self.quiet {
				eprintln!(
					"🚀 Processing final AI batch: {} files",
					ai_batch_queue.len()
				);
			}

			// Call batch AI extraction through AI enhancements
			if let Some(ref ai_enhancements) = self.ai_enhancements {
				match ai_enhancements
					.extract_ai_descriptions_batch(&ai_batch_queue)
					.await
				{
					Ok(batch_descriptions) => {
						// Store all descriptions from batch
						for (file_path, description) in batch_descriptions {
							ai_descriptions.insert(file_path, description);
						}
						if !self.quiet {
							eprintln!(
								"✅ Final batch AI processing completed: {} descriptions",
								ai_descriptions.len()
							);
						}
					}
					Err(e) => {
						if !self.quiet {
							eprintln!("⚠️  Final batch AI processing failed: {}", e);
						}
					}
				}
			}
		}

		// CRITICAL FIX: Update node descriptions with AI-generated descriptions
		if !ai_descriptions.is_empty() {
			if !self.quiet {
				eprintln!(
					"🔄 Updating {} nodes with AI-generated descriptions",
					ai_descriptions.len()
				);
			}

			// Update nodes in the graph with AI descriptions
			{
				let mut graph = self.graph.write().await;
				for (file_path, ai_description) in &ai_descriptions {
					if let Some(node) = graph.nodes.get_mut(file_path) {
						node.description = ai_description.clone();
						if !self.quiet {
							eprintln!("✅ Updated description for: {}", file_path);
						}
					}
				}
			}

			// Also update any pending nodes that haven't been added to graph yet
			for node in &mut new_nodes {
				if let Some(ai_description) = ai_descriptions.get(&node.id) {
					node.description = ai_description.clone();
					if !self.quiet {
						eprintln!("✅ Updated pending node description for: {}", node.id);
					}
				}
			}

			if !self.quiet {
				eprintln!("🎉 AI description updates complete!");
			}
		}

		// CRITICAL FIX: Replace placeholder descriptions with AI descriptions before final persistence
		if !ai_descriptions.is_empty() && !new_nodes.is_empty() {
			if !self.quiet {
				eprintln!(
					"🔄 Applying AI descriptions to {} pending nodes before persistence",
					new_nodes.len()
				);
			}

			for node in &mut new_nodes {
				if let Some(ai_description) = ai_descriptions.get(&node.id) {
					node.description = ai_description.clone();
					if !self.quiet {
						eprintln!("✅ Applied AI description to: {}", node.id);
					}
				} else {
					// Replace placeholder with simple description if AI failed
					let file_name = std::path::Path::new(&node.path)
						.file_stem()
						.and_then(|s| s.to_str())
						.unwrap_or("unknown");
					node.description = crate::indexer::graphrag::relationships::RelationshipDiscovery::generate_simple_description(
						file_name,
						&node.language,
						&node.symbols,
						node.size_lines,
					);
					if !self.quiet {
						eprintln!("⚠️  Fallback to simple description for: {}", node.id);
					}
				}
			}
		}

		// Process any remaining nodes in the final batch
		if !new_nodes.is_empty() {
			self.process_nodes_batch(
				&mut new_nodes,
				&mut pending_embeddings,
				&mut batches_processed,
				&ai_descriptions, // Pass AI descriptions
			)
			.await?;
		}

		// Discover relationships incrementally for processed nodes
		// This ensures relationships are stored during processing, not just at the end
		if processed_count > 0 {
			// Collect all processed nodes for relationship discovery
			let all_processed_nodes = {
				let graph = self.graph.read().await;
				graph.nodes.values().cloned().collect::<Vec<CodeNode>>()
			};

			if !all_processed_nodes.is_empty() {
				// Process relationships in batches to avoid storing everything at the end
				let relationship_batch_size = self.config.index.embeddings_batch_size * 4; // Larger batches for relationships

				let all_relationships = if self.llm_enabled() {
					// Enhanced relationship discovery with optional AI for complex cases
					self.discover_relationships_with_ai_enhancement(&all_processed_nodes)
						.await?
				} else {
					// Fast rule-based relationship discovery only
					self.discover_relationships_efficiently(&all_processed_nodes)
						.await?
				};

				// Store relationships in batches for incremental storage
				if !all_relationships.is_empty() {
					let mut relationship_batches_processed = 0;

					for relationship_batch in all_relationships.chunks(relationship_batch_size) {
						// Add relationships to in-memory graph
						{
							let mut graph = self.graph.write().await;
							graph
								.relationships
								.extend(relationship_batch.iter().cloned());
						}

						// Save relationship batch to database incrementally
						let db_ops = DatabaseOperations::new(&self.store);
						db_ops
							.save_graph_incremental(&[], relationship_batch)
							.await?;

						relationship_batches_processed += 1;

						// Flush relationships periodically (same logic as nodes)
						if relationship_batches_processed >= self.config.index.flush_frequency {
							self.store.flush().await?;
							relationship_batches_processed = 0;
						}

						// Update state to show relationship processing progress
						if let Some(ref state) = state {
							let mut state_guard = state.write();
							state_guard.status_message = format!(
								"Processing relationships: {} of {} batches completed",
								(relationship_batches_processed + 1),
								all_relationships.len().div_ceil(relationship_batch_size)
							);
						}
					}
				}
			}
		}

		// Final flush to ensure all data is persisted
		self.store.flush().await?;

		// Update final state
		if let Some(state) = state {
			let mut state_guard = state.write();
			state_guard.status_message = format!(
				"GraphRAG processing complete: {} files processed ({} skipped)",
				processed_count, skipped_count
			);
			// CRITICAL FIX: Update the graphrag_blocks counter
			state_guard.graphrag_blocks += processed_count;
		} else {
			println!(
				"GraphRAG: Processed {} files ({} skipped)",
				processed_count, skipped_count
			);
		}

		Ok(())
	}

	// Enhanced relationship discovery with optional AI for complex cases
	async fn discover_relationships_with_ai_enhancement(
		&self,
		new_files: &[CodeNode],
	) -> Result<Vec<CodeRelationship>> {
		if let Some(ref ai) = self.ai_enhancements {
			// Get all nodes for context
			let all_nodes = {
				let graph = self.graph.read().await;
				graph.nodes.values().cloned().collect::<Vec<CodeNode>>()
			};
			ai.discover_relationships_with_ai_enhancement(new_files, &all_nodes)
				.await
		} else {
			// Fallback to efficient discovery without AI
			self.discover_relationships_efficiently(new_files).await
		}
	}

	// Discover relationships efficiently without AI for most cases
	async fn discover_relationships_efficiently(
		&self,
		new_files: &[CodeNode],
	) -> Result<Vec<CodeRelationship>> {
		// Get all nodes from the graph for relationship discovery
		let all_nodes = {
			let graph = self.graph.read().await;
			graph.nodes.values().cloned().collect::<Vec<CodeNode>>()
		};

		RelationshipDiscovery::discover_relationships_efficiently(new_files, &all_nodes).await
	}

	// Determine if a file is complex enough to benefit from AI analysis
	fn should_use_ai_for_description(
		&self,
		symbols: &[String],
		lines: u32,
		language: &str,
	) -> bool {
		if let Some(ref ai) = self.ai_enhancements {
			ai.should_use_ai_for_description(symbols, lines, language)
		} else {
			false
		}
	}

	// Build a meaningful content sample for AI analysis (not full file content)
	fn build_content_sample_for_ai(&self, file_blocks: &[&CodeBlock]) -> String {
		if let Some(ref ai) = self.ai_enhancements {
			ai.build_content_sample_for_ai(file_blocks)
		} else {
			String::new()
		}
	}

	// Legacy method for backward compatibility - now uses efficient code block processing
	pub async fn process_code_blocks(
		&self,
		code_blocks: &[CodeBlock],
		state: Option<SharedState>,
	) -> Result<()> {
		// Use the new efficient method that processes code blocks directly
		self.process_files_from_codeblocks(code_blocks, state).await
	}

	// Build GraphRAG from existing database when enabled after indexing
	// This solves the critical issue where GraphRAG is enabled after database is already indexed
	pub async fn build_from_existing_database(&self, state: Option<SharedState>) -> Result<()> {
		// Update state to show we're building GraphRAG from existing data
		if let Some(ref state) = state {
			let mut state_guard = state.write();
			state_guard.status_message = "Building GraphRAG from existing database...".to_string();
		}

		// Clear existing GraphRAG data to avoid duplicates
		if let Err(e) = self.store.clear_graph_nodes().await {
			eprintln!("Warning: Failed to clear existing graph nodes: {}", e);
		}
		if let Err(e) = self.store.clear_graph_relationships().await {
			eprintln!(
				"Warning: Failed to clear existing graph relationships: {}",
				e
			);
		}

		// Clear in-memory graph to keep it in sync with database
		{
			let mut graph = self.graph.write().await;
			graph.nodes.clear();
			graph.relationships.clear();
		}

		// Get all existing code blocks from the database
		let all_code_blocks = self.store.get_all_code_blocks_for_graphrag().await?;

		if all_code_blocks.is_empty() {
			if let Some(ref state) = state {
				let mut state_guard = state.write();
				state_guard.status_message =
					"No code blocks found in database for GraphRAG".to_string();
			}
			return Ok(());
		}

		// Update state with the number of blocks to process
		if let Some(ref state) = state {
			let mut state_guard = state.write();
			state_guard.status_message = format!(
				"Processing {} code blocks for GraphRAG...",
				all_code_blocks.len()
			);
		}

		// Process the code blocks to build the graph
		self.process_files_from_codeblocks(&all_code_blocks, state.clone())
			.await?;

		// Final flush to ensure all data is persisted
		self.store.flush().await?;

		// Update final state
		if let Some(ref state) = state {
			let mut state_guard = state.write();
			state_guard.status_message = format!(
				"GraphRAG built from existing database: {} blocks processed",
				all_code_blocks.len()
			);
			// CRITICAL FIX: Update the graphrag_blocks counter
			state_guard.graphrag_blocks += all_code_blocks.len();
		} else {
			println!(
				"GraphRAG: Built from existing database with {} code blocks",
				all_code_blocks.len()
			);
		}

		Ok(())
	}

	// Get the full graph
	pub async fn get_graph(&self) -> Result<CodeGraph> {
		let graph = self.graph.read().await;

		// If the in-memory graph is empty, load from database
		if graph.nodes.is_empty() && graph.relationships.is_empty() {
			drop(graph); // Release read lock before loading

			// Load graph from database
			let db_ops = DatabaseOperations::new(&self.store);
			let loaded_graph = db_ops
				.load_graph(std::path::Path::new("."), self.quiet)
				.await?;

			// Update in-memory graph with loaded data
			{
				let mut graph_write = self.graph.write().await;
				*graph_write = loaded_graph.clone();
			}

			Ok(loaded_graph)
		} else {
			Ok(graph.clone())
		}
	}

	// Search the graph for nodes matching a query
	pub async fn search_nodes(&self, query: &str) -> Result<Vec<CodeNode>> {
		// First check if we have any nodes in memory
		let in_memory_nodes = {
			let graph = self.graph.read().await;
			!graph.nodes.is_empty()
		};

		if in_memory_nodes {
			// Use in-memory search if nodes are loaded
			return self.search_nodes_in_memory(query).await;
		} else {
			// Use database search if nodes are only in database
			return self.search_nodes_in_database(query).await;
		}
	}

	// Search for nodes in memory
	async fn search_nodes_in_memory(&self, query: &str) -> Result<Vec<CodeNode>> {
		// Generate an embedding for the query
		let query_embedding = self.generate_embedding(query).await?;

		// Find similar nodes
		let graph = self.graph.read().await;
		let nodes_array = graph.nodes.values().cloned().collect::<Vec<CodeNode>>();
		drop(graph);

		// Calculate similarity to each node
		let mut similarities: Vec<(f32, CodeNode)> = Vec::new();
		let query_lower = query.to_lowercase();

		for node in nodes_array {
			// Calculate semantic similarity
			let similarity = cosine_similarity(&query_embedding, &node.embedding);

			// Check if the query is a substring of various node fields
			// This handles specific cases like searching for "impl"
			let name_contains = node.name.to_lowercase().contains(&query_lower);
			let kind_contains = node.kind.to_lowercase().contains(&query_lower);
			let desc_contains = node.description.to_lowercase().contains(&query_lower);
			let symbols_contain = node
				.symbols
				.iter()
				.any(|s| s.to_lowercase().contains(&query_lower));

			// Use a lower threshold for semantic similarity (0.5 instead of 0.6)
			// OR include if the query is a substring of any important field
			if similarity > 0.5
				|| name_contains
				|| kind_contains
				|| desc_contains
				|| symbols_contain
			{
				// Boost similarity score for exact matches to ensure they appear at the top
				let boosted_similarity = if name_contains || kind_contains || symbols_contain {
					// Ensure exact matches get higher priority
					0.9_f32.max(similarity)
				} else {
					similarity
				};

				similarities.push((boosted_similarity, node));
			}
		}

		// Sort by similarity (highest first)
		similarities.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

		// Return the nodes (without the similarity scores)
		let results = similarities.into_iter().map(|(_, node)| node).collect();

		Ok(results)
	}

	// Search for nodes in database
	async fn search_nodes_in_database(&self, query: &str) -> Result<Vec<CodeNode>> {
		// Generate an embedding for the query
		let query_embedding = self.generate_embedding(query).await?;

		let db_ops = DatabaseOperations::new(&self.store);
		db_ops
			.search_nodes_in_database(&query_embedding, query)
			.await
	}

	// Find paths between nodes in the graph
	pub async fn find_paths(
		&self,
		source_id: &str,
		target_id: &str,
		max_depth: usize,
	) -> Result<Vec<Vec<String>>> {
		let graph = self.graph.read().await;

		// Ensure both nodes exist
		if !graph.nodes.contains_key(source_id) || !graph.nodes.contains_key(target_id) {
			return Ok(Vec::new());
		}

		// Build an adjacency list for easier traversal
		let mut adjacency_list: HashMap<String, Vec<String>> = HashMap::new();
		for rel in &graph.relationships {
			adjacency_list
				.entry(rel.source.clone())
				.or_default()
				.push(rel.target.clone());
		}

		// Use BFS to find paths
		let mut queue = Vec::new();
		queue.push(vec![source_id.to_string()]);

		let mut paths = Vec::new();

		while let Some(path) = queue.pop() {
			let current = path.last().unwrap();

			// Found a path to target
			if current == target_id {
				paths.push(path);
				continue;
			}

			// Stop if we've reached max depth
			if path.len() > max_depth {
				continue;
			}

			// Explore neighbors
			if let Some(neighbors) = adjacency_list.get(current) {
				for neighbor in neighbors {
					// Avoid cycles
					if !path.contains(neighbor) {
						let mut new_path = path.clone();
						new_path.push(neighbor.clone());
						queue.push(new_path);
					}
				}
			}
		}

		Ok(paths)
	}

	// Check if we should process batch (same logic as normal indexing)
	fn should_process_batch(&self, pending_embeddings: &[String]) -> bool {
		// Use the same batch size logic as normal indexing
		let batch_size = self.config.index.embeddings_batch_size;
		let max_tokens = self.config.index.embeddings_max_tokens_per_batch;

		if pending_embeddings.len() >= batch_size {
			return true;
		}

		// Check token count (approximate)
		let total_tokens: usize = pending_embeddings.iter().map(|s| s.len() / 4).sum(); // Rough token estimate
		total_tokens >= max_tokens
	}

	// Process a batch of nodes with embeddings and persist them
	async fn process_nodes_batch(
		&self,
		nodes: &mut Vec<CodeNode>,
		pending_embeddings: &mut Vec<String>,
		batches_processed: &mut usize,
		ai_descriptions: &HashMap<String, String>, // Add AI descriptions parameter
	) -> Result<()> {
		if nodes.is_empty() || pending_embeddings.is_empty() {
			return Ok(());
		}

		// CRITICAL FIX: Apply AI descriptions before persistence
		for node in nodes.iter_mut() {
			if let Some(ai_description) = ai_descriptions.get(&node.id) {
				node.description = ai_description.clone();
			} else if node.description.starts_with("AI_PENDING:") {
				// Replace placeholder with simple description if AI failed
				let file_name = std::path::Path::new(&node.path)
					.file_stem()
					.and_then(|s| s.to_str())
					.unwrap_or("unknown");
				node.description = crate::indexer::graphrag::relationships::RelationshipDiscovery::generate_simple_description(
					file_name,
					&node.language,
					&node.symbols,
					node.size_lines,
				);
			}
		}

		// Generate embeddings in batch (same as normal indexing)
		let embeddings = crate::embedding::generate_embeddings_batch(
			pending_embeddings.clone(),
			false, // Use text embeddings for GraphRAG descriptions
			&self.config,
			crate::embedding::types::InputType::Document,
		)
		.await?;

		// Assign embeddings to nodes
		for (node, embedding) in nodes.iter_mut().zip(embeddings.iter()) {
			node.embedding = embedding.clone();
		}

		// CRITICAL FIX: Add nodes to the graph with deduplication check
		{
			let mut graph = self.graph.write().await;
			for node in nodes.iter() {
				// Check if node already exists to prevent duplicates
				if let Some(existing_node) = graph.nodes.get(&node.id) {
					if !self.quiet {
						eprintln!("⚠️  Preventing duplicate node insertion: {} (existing hash: {}, new hash: {})",
							node.id, existing_node.hash, node.hash);
					}
					// Only replace if the hash is different (content changed)
					if existing_node.hash != node.hash {
						graph.nodes.insert(node.id.clone(), node.clone());
						if !self.quiet {
							eprintln!("🔄 Updated node with new content: {}", node.id);
						}
					}
				} else {
					graph.nodes.insert(node.id.clone(), node.clone());
					if !self.quiet {
						eprintln!("➕ Added new node: {}", node.id);
					}
				}
			}
		}

		// Persist nodes to database (same as normal indexing)
		let db_ops = DatabaseOperations::new(&self.store);
		db_ops.save_graph_incremental(nodes, &[]).await?;

		// Clear the batches
		nodes.clear();
		pending_embeddings.clear();
		*batches_processed += 1;

		// Use the same flush logic as normal indexing
		self.flush_if_needed(batches_processed).await?;

		Ok(())
	}

	// Flush if needed (same logic as normal indexing)
	async fn flush_if_needed(&self, batches_processed: &mut usize) -> Result<()> {
		if *batches_processed >= self.config.index.flush_frequency {
			self.store.flush().await?;
			*batches_processed = 0;
		}
		Ok(())
	}

	// Extract imports/exports using language-specific AST parsing
	pub async fn extract_imports_exports_from_file(
		&self,
		file_path: &str,
		language: &str,
	) -> Result<(Vec<String>, Vec<String>)> {
		use crate::indexer::languages;
		use std::fs;
		use tree_sitter::Parser;

		// Get language implementation
		let lang_impl = languages::get_language(language).ok_or_else(|| {
			anyhow::anyhow!("Failed to get language implementation for: {}", language)
		})?;

		// Read file content
		let contents = fs::read_to_string(file_path)?;

		// Parse with tree-sitter
		let mut parser = Parser::new();
		parser.set_language(&lang_impl.get_ts_language())?;
		let tree = parser
			.parse(&contents, None)
			.ok_or_else(|| anyhow::anyhow!("Failed to parse file"))?;

		let mut all_imports = Vec::new();
		let mut all_exports = Vec::new();

		// Walk through all nodes and extract imports/exports
		let cursor = tree.walk();
		extract_imports_exports_recursive(
			cursor.node(),
			&contents,
			lang_impl.as_ref(),
			&mut all_imports,
			&mut all_exports,
		);

		Ok((all_imports, all_exports))
	}
}

// Recursively extract imports/exports from AST nodes
fn extract_imports_exports_recursive(
	node: tree_sitter::Node,
	contents: &str,
	lang_impl: &dyn crate::indexer::languages::Language,
	all_imports: &mut Vec<String>,
	all_exports: &mut Vec<String>,
) {
	// Extract imports/exports from current node
	let (imports, exports) = lang_impl.extract_imports_exports(node, contents);
	all_imports.extend(imports);
	all_exports.extend(exports);

	// Recursively process children
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		extract_imports_exports_recursive(child, contents, lang_impl, all_imports, all_exports);
	}
}
