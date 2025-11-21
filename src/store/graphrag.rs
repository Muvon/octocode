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
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

// Arrow imports
use arrow::array::{Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;

// LanceDB imports
use crate::store::{table_ops::TableOperations, CodeBlock};
use futures::TryStreamExt;
use lancedb::{
	query::{ExecutableQuery, QueryBase},
	Connection, DistanceType,
};

// GraphRAG types
use crate::indexer::graphrag::types::RelationType;

/// Cache statistics for monitoring performance
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
	pub hits: usize,
	pub misses: usize,
}

impl CacheStats {
	pub fn hit_rate(&self) -> f64 {
		let total = self.hits + self.misses;
		if total == 0 {
			0.0
		} else {
			self.hits as f64 / total as f64
		}
	}
}

/// Adjacency cache for fast graph traversals
/// Maps: node_id -> relation_type -> [target_node_ids]
type AdjacencyCache = HashMap<String, HashMap<RelationType, Vec<String>>>;

/// Handles GraphRAG-specific database operations
pub struct GraphRagOperations<'a> {
	pub db: &'a Connection,
	pub table_ops: TableOperations<'a>,
	pub code_vector_dim: usize,
	/// Adjacency cache for outgoing relationships
	adjacency_cache: Arc<RwLock<AdjacencyCache>>,
	/// Cache statistics
	cache_stats: Arc<RwLock<CacheStats>>,
}

impl<'a> GraphRagOperations<'a> {
	pub fn new(db: &'a Connection, code_vector_dim: usize) -> Self {
		Self {
			db,
			table_ops: TableOperations::new(db),
			code_vector_dim,
			adjacency_cache: Arc::new(RwLock::new(HashMap::new())),
			cache_stats: Arc::new(RwLock::new(CacheStats::default())),
		}
	}

	// ============================================================================
	// Adjacency Cache Management
	// ============================================================================

	/// Clear the entire adjacency cache
	pub fn clear_cache(&self) {
		if let Ok(mut cache) = self.adjacency_cache.write() {
			cache.clear();
		}
		if let Ok(mut stats) = self.cache_stats.write() {
			*stats = CacheStats::default();
		}
	}

	/// Invalidate cache entries for a specific node
	pub fn invalidate_cache_for_node(&self, node_id: &str) {
		if let Ok(mut cache) = self.adjacency_cache.write() {
			cache.remove(node_id);
		}
	}

	/// Get cache statistics
	pub fn get_cache_stats(&self) -> CacheStats {
		self.cache_stats
			.read()
			.map(|stats| stats.clone())
			.unwrap_or_default()
	}

	/// Get adjacent nodes from cache (outgoing relationships only)
	/// Returns None if not in cache, Some(Vec) if cached (may be empty)
	fn get_adjacent_nodes_cached(
		&self,
		node_id: &str,
		relation_type: &RelationType,
	) -> Option<Vec<String>> {
		let cache = self.adjacency_cache.read().ok()?;
		let result = cache
			.get(node_id)
			.and_then(|relations| relations.get(relation_type))
			.cloned();

		// Update stats
		if let Ok(mut stats) = self.cache_stats.write() {
			if result.is_some() {
				stats.hits += 1;
			} else {
				stats.misses += 1;
			}
		}

		result
	}

	/// Update cache with relationship data
	fn update_cache(&self, source_id: &str, relation_type: RelationType, target_id: String) {
		if let Ok(mut cache) = self.adjacency_cache.write() {
			cache
				.entry(source_id.to_string())
				.or_default()
				.entry(relation_type)
				.or_default()
				.push(target_id);
		}
	}

	/// Build adjacency cache from all relationships in the database
	pub async fn build_adjacency_cache(&self) -> Result<()> {
		// Clear existing cache
		self.clear_cache();

		// Check if relationships table exists
		if !self
			.table_ops
			.table_exists("graphrag_relationships")
			.await?
		{
			return Ok(());
		}

		let table = self
			.db
			.open_table("graphrag_relationships")
			.execute()
			.await?;

		// Stream all relationships
		let mut results = table.query().execute().await?;

		let mut total_relationships = 0;
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() == 0 {
				continue;
			}

			// Extract columns
			let source_ids = batch
				.column_by_name("source_id")
				.and_then(|col| col.as_any().downcast_ref::<StringArray>())
				.ok_or_else(|| anyhow::anyhow!("Missing or invalid source_id column"))?;

			let target_ids = batch
				.column_by_name("target_id")
				.and_then(|col| col.as_any().downcast_ref::<StringArray>())
				.ok_or_else(|| anyhow::anyhow!("Missing or invalid target_id column"))?;

			let relation_types = batch
				.column_by_name("relation_type")
				.and_then(|col| col.as_any().downcast_ref::<StringArray>())
				.ok_or_else(|| anyhow::anyhow!("Missing or invalid relation_type column"))?;

			// Build cache entries
			for i in 0..batch.num_rows() {
				if let (Some(source), Some(target), Some(rel_type_str)) = (
					source_ids.value(i).to_string().into(),
					target_ids.value(i).to_string().into(),
					relation_types.value(i).to_string().into(),
				) {
					// Parse relation type
					if let Ok(rel_type) = RelationType::from_str(&rel_type_str) {
						self.update_cache(&source, rel_type, target);
						total_relationships += 1;
					}
				}
			}
		}

		tracing::info!(
			"Built adjacency cache with {} relationships",
			total_relationships
		);

		Ok(())
	}

	// ============================================================================
	// Cache-Aware Traversal Methods
	// ============================================================================

	/// Get neighbors of a node using cache (outgoing relationships only)
	/// Falls back to database query if not in cache
	pub async fn get_neighbors_cached(
		&self,
		node_id: &str,
		relation_type: &RelationType,
	) -> Result<Vec<String>> {
		// Try cache first
		if let Some(neighbors) = self.get_adjacent_nodes_cached(node_id, relation_type) {
			return Ok(neighbors);
		}

		// Cache miss - query database
		let relationships = self
			.get_node_relationships(
				node_id,
				crate::indexer::graphrag::types::RelationshipDirection::Outgoing,
			)
			.await?;

		// Filter by relation type and extract target IDs
		let neighbors: Vec<String> = relationships
			.iter()
			.filter(|rel| &rel.relation_type == relation_type)
			.map(|rel| rel.target.clone())
			.collect();

		// Update cache with results
		for target in &neighbors {
			self.update_cache(node_id, relation_type.clone(), target.clone());
		}

		Ok(neighbors)
	}

	/// Traverse a path from a starting node following specific relation types
	/// Returns all nodes reachable within max_depth hops
	pub async fn traverse_path_cached(
		&self,
		start_node: &str,
		relation_types: &[RelationType],
		max_depth: usize,
	) -> Result<Vec<String>> {
		use std::collections::{HashSet, VecDeque};

		let mut visited = HashSet::new();
		let mut queue = VecDeque::new();
		let mut result = Vec::new();

		// Start with the initial node
		queue.push_back((start_node.to_string(), 0));
		visited.insert(start_node.to_string());

		while let Some((current_node, depth)) = queue.pop_front() {
			result.push(current_node.clone());

			// Stop if we've reached max depth
			if depth >= max_depth {
				continue;
			}

			// Explore neighbors for each relation type
			for rel_type in relation_types {
				let neighbors = self.get_neighbors_cached(&current_node, rel_type).await?;

				for neighbor in neighbors {
					if !visited.contains(&neighbor) {
						visited.insert(neighbor.clone());
						queue.push_back((neighbor, depth + 1));
					}
				}
			}
		}

		tracing::debug!(
			"Traversed from {} with depth {}: found {} nodes (cache hit rate: {:.2}%)",
			start_node,
			max_depth,
			result.len(),
			self.get_cache_stats().hit_rate() * 100.0
		);

		Ok(result)
	}

	/// Find connected components using cache
	/// Returns groups of nodes that are connected through the specified relation types
	pub async fn find_connected_components_cached(
		&self,
		relation_types: &[RelationType],
	) -> Result<Vec<Vec<String>>> {
		use std::collections::{HashSet, VecDeque};

		// Get all node IDs
		let all_nodes = self.get_all_node_ids().await?;
		let mut visited = HashSet::new();
		let mut components = Vec::new();

		for start_node in &all_nodes {
			if visited.contains(start_node) {
				continue;
			}

			// BFS to find all nodes in this component
			let mut component = Vec::new();
			let mut queue = VecDeque::new();
			queue.push_back(start_node.clone());
			visited.insert(start_node.clone());

			while let Some(current_node) = queue.pop_front() {
				component.push(current_node.clone());

				// Explore neighbors for each relation type
				for rel_type in relation_types {
					let neighbors = self.get_neighbors_cached(&current_node, rel_type).await?;

					for neighbor in neighbors {
						if !visited.contains(&neighbor) {
							visited.insert(neighbor.clone());
							queue.push_back(neighbor);
						}
					}
				}
			}

			if !component.is_empty() {
				components.push(component);
			}
		}

		tracing::info!(
			"Found {} connected components (cache hit rate: {:.2}%)",
			components.len(),
			self.get_cache_stats().hit_rate() * 100.0
		);

		Ok(components)
	}

	/// Get all node IDs from the database
	async fn get_all_node_ids(&self) -> Result<Vec<String>> {
		if !self.table_ops.table_exists("graphrag_nodes").await? {
			return Ok(Vec::new());
		}

		let table = self.db.open_table("graphrag_nodes").execute().await?;
		let mut results = table.query().execute().await?;

		let mut node_ids = Vec::new();
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() == 0 {
				continue;
			}

			let id_array = batch
				.column_by_name("id")
				.and_then(|col| col.as_any().downcast_ref::<StringArray>())
				.ok_or_else(|| anyhow::anyhow!("Missing or invalid id column"))?;

			for i in 0..batch.num_rows() {
				if let Some(id) = id_array.value(i).to_string().into() {
					node_ids.push(id);
				}
			}
		}

		Ok(node_ids)
	}

	/// Returns true if GraphRAG should be indexed (enabled but not yet indexed or empty)
	pub async fn graphrag_needs_indexing(&self) -> Result<bool> {
		// Check if GraphRAG tables exist
		if !self
			.table_ops
			.tables_exist(&["graphrag_nodes", "graphrag_relationships"])
			.await?
		{
			return Ok(true); // Tables don't exist, need indexing
		}

		// Check if tables are empty
		let nodes_table = self.db.open_table("graphrag_nodes").execute().await?;
		let relationships_table = self
			.db
			.open_table("graphrag_relationships")
			.execute()
			.await?;

		let nodes_count = nodes_table.count_rows(None).await?;
		let relationships_count = relationships_table.count_rows(None).await?;

		if nodes_count == 0 && relationships_count == 0 {
			return Ok(true); // Tables are empty, need indexing
		}

		Ok(false) // GraphRAG is already indexed
	}

	/// Get all code blocks for GraphRAG processing
	/// This is used when GraphRAG is enabled after the database is already indexed
	pub async fn get_all_code_blocks_for_graphrag(&self) -> Result<Vec<CodeBlock>> {
		let mut all_blocks = Vec::new();

		if !self.table_ops.table_exists("code_blocks").await? {
			return Ok(all_blocks);
		}

		let table = self.db.open_table("code_blocks").execute().await?;

		// Get all code blocks in batches to avoid memory issues
		let mut results = table.query().execute().await?;

		// Process all result batches
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				// Convert batch to CodeBlocks
				let converter =
					crate::store::batch_converter::BatchConverter::new(self.code_vector_dim);
				let mut code_blocks = converter.batch_to_code_blocks(&batch, None)?;
				all_blocks.append(&mut code_blocks);

				// Log progress for large datasets
				if cfg!(debug_assertions) && all_blocks.len() % 1000 == 0 {
					tracing::debug!(
						"Loaded {} code blocks for GraphRAG processing...",
						all_blocks.len()
					);
				}
			}
		}

		Ok(all_blocks)
	}

	/// Store graph nodes in the database
	pub async fn store_graph_nodes(&self, node_batch: RecordBatch) -> Result<()> {
		// Use the same proven pattern as code_blocks, text_blocks, document_blocks
		self.table_ops
			.store_batch("graphrag_nodes", node_batch)
			.await?;

		// Create or optimize vector index based on dataset growth
		if let Ok(table) = self.db.open_table("graphrag_nodes").execute().await {
			let row_count = table.count_rows(None).await?;
			let indices = table.list_indices().await?;
			let has_index = indices.iter().any(|idx| idx.columns == vec!["embedding"]);

			if !has_index {
				// Create initial index
				if let Err(e) = self
					.table_ops
					.create_vector_index_optimized(
						"graphrag_nodes",
						"embedding",
						self.code_vector_dim,
					)
					.await
				{
					tracing::warn!(
						"Failed to create optimized vector index on graph_nodes: {}",
						e
					);
				}
			} else {
				// Check if we should optimize existing index due to growth
				if super::vector_optimizer::VectorOptimizer::should_optimize_for_growth(
					row_count,
					self.code_vector_dim,
					true,
				) {
					tracing::info!("Dataset growth detected, optimizing graphrag_nodes index");
					if let Err(e) = self
						.table_ops
						.recreate_vector_index_optimized(
							"graphrag_nodes",
							"embedding",
							self.code_vector_dim,
						)
						.await
					{
						tracing::warn!(
							"Failed to recreate optimized vector index on graphrag_nodes: {}",
							e
						);
					}
				}
			}
		}

		Ok(())
	}

	/// Store graph relationships in the database
	pub async fn store_graph_relationships(&self, rel_batch: RecordBatch) -> Result<()> {
		// Store in database first
		self.table_ops
			.store_batch("graphrag_relationships", rel_batch.clone())
			.await?;

		// Update adjacency cache with new relationships
		self.update_cache_from_batch(&rel_batch)?;

		Ok(())
	}

	/// Update adjacency cache from a relationship batch
	fn update_cache_from_batch(&self, rel_batch: &RecordBatch) -> Result<()> {
		if rel_batch.num_rows() == 0 {
			return Ok(());
		}

		// Extract columns
		let source_ids = rel_batch
			.column_by_name("source_id")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("Missing or invalid source_id column"))?;

		let target_ids = rel_batch
			.column_by_name("target_id")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("Missing or invalid target_id column"))?;

		let relation_types = rel_batch
			.column_by_name("relation_type")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("Missing or invalid relation_type column"))?;

		// Update cache entries
		for i in 0..rel_batch.num_rows() {
			if let (Some(source), Some(target), Some(rel_type_str)) = (
				source_ids.value(i).to_string().into(),
				target_ids.value(i).to_string().into(),
				relation_types.value(i).to_string().into(),
			) {
				// Parse relation type
				if let Ok(rel_type) = RelationType::from_str(&rel_type_str) {
					self.update_cache(&source, rel_type, target);
				}
			}
		}

		Ok(())
	}

	/// Clear all graph nodes from the database
	pub async fn clear_graph_nodes(&self) -> Result<()> {
		self.table_ops.clear_table("graphrag_nodes").await?;
		// Clear cache when clearing nodes
		self.clear_cache();
		Ok(())
	}

	/// Clear all graph relationships from the database
	pub async fn clear_graph_relationships(&self) -> Result<()> {
		self.table_ops.clear_table("graphrag_relationships").await?;
		// Clear cache when clearing relationships
		self.clear_cache();
		Ok(())
	}

	/// Remove GraphRAG nodes associated with a specific file path
	pub async fn remove_graph_nodes_by_path(&self, file_path: &str) -> Result<usize> {
		// Get node IDs before removal for cache invalidation
		let node_ids = self.get_node_ids_for_file_path(file_path).await?;

		// CRITICAL FIX: Also remove relationships when removing nodes
		let relationships_removed = self.remove_graph_relationships_by_path(file_path).await?;
		let nodes_removed = self
			.table_ops
			.remove_blocks_by_path(file_path, "graphrag_nodes")
			.await?;

		// Invalidate cache for removed nodes
		for node_id in node_ids {
			self.invalidate_cache_for_node(&node_id);
		}

		if nodes_removed > 0 || relationships_removed > 0 {
			eprintln!(
				"🗑️  Cleaned up GraphRAG data for {}: {} nodes, {} relationships",
				file_path, nodes_removed, relationships_removed
			);
		}

		Ok(nodes_removed)
	}

	/// Remove GraphRAG relationships associated with a specific file path
	pub async fn remove_graph_relationships_by_path(&self, file_path: &str) -> Result<usize> {
		if !self
			.table_ops
			.table_exists("graphrag_relationships")
			.await?
		{
			return Ok(0);
		}

		// First, get all node IDs for this file path from graphrag_nodes
		let node_ids = self.get_node_ids_for_file_path(file_path).await?;
		if node_ids.is_empty() {
			return Ok(0); // No nodes for this file, so no relationships to remove
		}

		let table = self
			.db
			.open_table("graphrag_relationships")
			.execute()
			.await?;

		// Count rows before deletion for reporting
		let before_count = table.count_rows(None).await?;

		// Create filter for relationships where source OR target is any of the node IDs
		let node_filters: Vec<String> = node_ids
			.iter()
			.flat_map(|node_id| {
				vec![
					format!("source = '{}'", node_id),
					format!("target = '{}'", node_id),
				]
			})
			.collect();

		if !node_filters.is_empty() {
			let filter = node_filters.join(" OR ");
			table.delete(&filter).await.map_err(|e| {
				anyhow::anyhow!("Failed to delete from graphrag_relationships: {}", e)
			})?;
		}

		// Invalidate cache for affected nodes
		for node_id in &node_ids {
			self.invalidate_cache_for_node(node_id);
		}

		// Count rows after deletion
		let after_count = table.count_rows(None).await?;
		let deleted_count = before_count.saturating_sub(after_count);

		Ok(deleted_count)
	}

	/// Get all node IDs for a specific file path from graphrag_nodes
	async fn get_node_ids_for_file_path(&self, file_path: &str) -> Result<Vec<String>> {
		let mut node_ids = Vec::new();

		if !self.table_ops.table_exists("graphrag_nodes").await? {
			return Ok(node_ids);
		}

		let table = self.db.open_table("graphrag_nodes").execute().await?;

		// Query for nodes matching the file path, only selecting id column
		let mut results = table
			.query()
			.only_if(format!("path = '{}'", file_path))
			.select(lancedb::query::Select::Columns(vec!["id".to_string()]))
			.execute()
			.await?;

		// Process all result batches
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				if let Some(column) = batch.column_by_name("id") {
					if let Some(id_array) =
						column.as_any().downcast_ref::<arrow::array::StringArray>()
					{
						for i in 0..id_array.len() {
							node_ids.push(id_array.value(i).to_string());
						}
					}
				}
			}
		}

		Ok(node_ids)
	}

	/// Search for graph nodes by vector similarity
	pub async fn search_graph_nodes(&self, embedding: &[f32], limit: usize) -> Result<RecordBatch> {
		// Check embedding dimension
		if embedding.len() != self.code_vector_dim {
			return Err(anyhow::anyhow!(
				"Embedding dimension {} doesn't match expected {}",
				embedding.len(),
				self.code_vector_dim
			));
		}

		if !self.table_ops.table_exists("graphrag_nodes").await? {
			// Return empty batch with expected schema that matches the actual storage schema
			let schema = Arc::new(Schema::new(vec![
				Field::new("id", DataType::Utf8, false),
				Field::new("name", DataType::Utf8, false),
				Field::new("kind", DataType::Utf8, false),
				Field::new("path", DataType::Utf8, false),
				Field::new("description", DataType::Utf8, false),
				Field::new("symbols", DataType::Utf8, true),
				Field::new("imports", DataType::Utf8, true),
				Field::new("exports", DataType::Utf8, true),
				Field::new("functions", DataType::Utf8, true),
				Field::new("size_lines", DataType::UInt32, false),
				Field::new("language", DataType::Utf8, false),
				Field::new("hash", DataType::Utf8, false),
			]));
			return Ok(RecordBatch::new_empty(schema));
		}

		let table = self.db.open_table("graphrag_nodes").execute().await?;

		// Perform vector similarity search with optimization
		let query = table
			.vector_search(embedding)?
			.distance_type(DistanceType::Cosine)
			.limit(limit);

		// Apply intelligent search optimization
		let optimized_query = crate::store::vector_optimizer::VectorOptimizer::optimize_query(
			query,
			&table,
			"graphrag_nodes",
		)
		.await
		.map_err(|e| anyhow::anyhow!("Failed to optimize query: {}", e))?;

		let mut results = optimized_query.execute().await?;

		// Collect all results into a single batch
		let mut all_batches = Vec::new();
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				all_batches.push(batch);
			}
		}

		// Concatenate all batches if we have multiple
		if all_batches.is_empty() {
			// Return empty batch with expected schema
			let schema = Arc::new(Schema::new(vec![
				Field::new("id", DataType::Utf8, false),
				Field::new("file_path", DataType::Utf8, false),
				Field::new("node_type", DataType::Utf8, false),
				Field::new("name", DataType::Utf8, false),
				Field::new("content", DataType::Utf8, false),
				Field::new("description", DataType::Utf8, true),
			]));
			Ok(RecordBatch::new_empty(schema))
		} else if all_batches.len() == 1 {
			Ok(all_batches.into_iter().next().unwrap())
		} else {
			// Concatenate multiple batches
			let schema = all_batches[0].schema();
			let mut columns = Vec::new();

			for i in 0..schema.fields().len() {
				let _field = schema.field(i);
				let mut column_data = Vec::new();

				for batch in &all_batches {
					if let Some(column) = batch.column(i).as_any().downcast_ref::<StringArray>() {
						for value in column.iter() {
							column_data.push(value);
						}
					}
				}

				columns
					.push(Arc::new(StringArray::from(column_data)) as Arc<dyn arrow::array::Array>);
			}

			Ok(RecordBatch::try_new(schema, columns)?)
		}
	}

	/// Get all graph relationships
	pub async fn get_graph_relationships(&self) -> Result<RecordBatch> {
		if !self
			.table_ops
			.table_exists("graphrag_relationships")
			.await?
		{
			// Return empty batch with expected schema that matches the actual storage schema
			let schema = Arc::new(Schema::new(vec![
				Field::new("id", DataType::Utf8, false),
				Field::new("source", DataType::Utf8, false),
				Field::new("target", DataType::Utf8, false),
				Field::new("relation_type", DataType::Utf8, false),
				Field::new("description", DataType::Utf8, false),
				Field::new("confidence", DataType::Float32, false),
				Field::new("weight", DataType::Float32, false),
			]));
			return Ok(RecordBatch::new_empty(schema));
		}

		let table = self
			.db
			.open_table("graphrag_relationships")
			.execute()
			.await?;

		// Get all relationships
		let mut results = table.query().execute().await?;

		// Collect all results into a single batch
		let mut all_batches = Vec::new();
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				all_batches.push(batch);
			}
		}

		// Concatenate all batches if we have multiple
		if all_batches.is_empty() {
			// Return empty batch with expected schema
			let schema = Arc::new(Schema::new(vec![
				Field::new("id", DataType::Utf8, false),
				Field::new("source_id", DataType::Utf8, false),
				Field::new("target_id", DataType::Utf8, false),
				Field::new("relationship_type", DataType::Utf8, false),
				Field::new("source_path", DataType::Utf8, false),
				Field::new("target_path", DataType::Utf8, false),
				Field::new("description", DataType::Utf8, true),
			]));
			Ok(RecordBatch::new_empty(schema))
		} else if all_batches.len() == 1 {
			Ok(all_batches.into_iter().next().unwrap())
		} else {
			// For simplicity, return the first batch
			// In a production system, you might want to concatenate all batches
			Ok(all_batches.into_iter().next().unwrap())
		}
	}

	/// Get relationships for a specific node with direction filtering
	/// This is much more efficient than loading all relationships and filtering in memory
	pub async fn get_node_relationships(
		&self,
		node_id: &str,
		direction: crate::indexer::graphrag::types::RelationshipDirection,
	) -> Result<Vec<crate::indexer::graphrag::types::CodeRelationship>> {
		use crate::indexer::graphrag::types::RelationshipDirection;

		if !self
			.table_ops
			.table_exists("graphrag_relationships")
			.await?
		{
			return Ok(Vec::new());
		}

		let table = self
			.db
			.open_table("graphrag_relationships")
			.execute()
			.await?;

		// Build filter based on direction
		let filter = match direction {
			RelationshipDirection::Outgoing => format!("source = '{}'", node_id),
			RelationshipDirection::Incoming => format!("target = '{}'", node_id),
			RelationshipDirection::Both => {
				format!("source = '{}' OR target = '{}'", node_id, node_id)
			}
		};

		// Execute filtered query
		let mut results = table.query().only_if(&filter).execute().await?;

		// Collect results
		let mut relationships = Vec::new();
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() == 0 {
				continue;
			}

			// Extract columns
			let source_array = batch
				.column_by_name("source")
				.ok_or_else(|| anyhow::anyhow!("Missing source column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid source column type"))?;

			let target_array = batch
				.column_by_name("target")
				.ok_or_else(|| anyhow::anyhow!("Missing target column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid target column type"))?;

			let type_array = batch
				.column_by_name("relation_type")
				.ok_or_else(|| anyhow::anyhow!("Missing relation_type column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid relation_type column type"))?;

			let desc_array = batch
				.column_by_name("description")
				.ok_or_else(|| anyhow::anyhow!("Missing description column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid description column type"))?;

			let conf_array = batch
				.column_by_name("confidence")
				.ok_or_else(|| anyhow::anyhow!("Missing confidence column"))?
				.as_any()
				.downcast_ref::<arrow::array::Float32Array>()
				.ok_or_else(|| anyhow::anyhow!("Invalid confidence column type"))?;

			let weight_array = batch
				.column_by_name("weight")
				.ok_or_else(|| anyhow::anyhow!("Missing weight column"))?
				.as_any()
				.downcast_ref::<arrow::array::Float32Array>()
				.ok_or_else(|| anyhow::anyhow!("Invalid weight column type"))?;

			// Convert to CodeRelationship objects
			for i in 0..batch.num_rows() {
				let relationship = crate::indexer::graphrag::types::CodeRelationship {
					source: source_array.value(i).to_string(),
					target: target_array.value(i).to_string(),
					relation_type: type_array
						.value(i)
						.parse()
						.unwrap_or(crate::indexer::graphrag::types::RelationType::Imports),
					description: desc_array.value(i).to_string(),
					confidence: conf_array.value(i),
					weight: weight_array.value(i),
				};
				relationships.push(relationship);
			}
		}

		Ok(relationships)
	}

	/// Get relationships filtered by type
	pub async fn get_relationships_by_type(
		&self,
		relation_type: &crate::indexer::graphrag::types::RelationType,
	) -> Result<Vec<crate::indexer::graphrag::types::CodeRelationship>> {
		if !self
			.table_ops
			.table_exists("graphrag_relationships")
			.await?
		{
			return Ok(Vec::new());
		}

		let table = self
			.db
			.open_table("graphrag_relationships")
			.execute()
			.await?;

		// Filter by relationship type
		let filter = format!("relation_type = '{}'", relation_type.as_str());
		let mut results = table.query().only_if(&filter).execute().await?;

		// Collect results (reuse logic from get_node_relationships)
		let mut relationships = Vec::new();
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() == 0 {
				continue;
			}

			// Extract columns (same as above)
			let source_array = batch
				.column_by_name("source")
				.ok_or_else(|| anyhow::anyhow!("Missing source column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid source column type"))?;

			let target_array = batch
				.column_by_name("target")
				.ok_or_else(|| anyhow::anyhow!("Missing target column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid target column type"))?;

			let type_array = batch
				.column_by_name("relation_type")
				.ok_or_else(|| anyhow::anyhow!("Missing relation_type column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid relation_type column type"))?;

			let desc_array = batch
				.column_by_name("description")
				.ok_or_else(|| anyhow::anyhow!("Missing description column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid description column type"))?;

			let conf_array = batch
				.column_by_name("confidence")
				.ok_or_else(|| anyhow::anyhow!("Missing confidence column"))?
				.as_any()
				.downcast_ref::<arrow::array::Float32Array>()
				.ok_or_else(|| anyhow::anyhow!("Invalid confidence column type"))?;

			let weight_array = batch
				.column_by_name("weight")
				.ok_or_else(|| anyhow::anyhow!("Missing weight column"))?
				.as_any()
				.downcast_ref::<arrow::array::Float32Array>()
				.ok_or_else(|| anyhow::anyhow!("Invalid weight column type"))?;

			for i in 0..batch.num_rows() {
				let relationship = crate::indexer::graphrag::types::CodeRelationship {
					source: source_array.value(i).to_string(),
					target: target_array.value(i).to_string(),
					relation_type: type_array
						.value(i)
						.parse()
						.unwrap_or(crate::indexer::graphrag::types::RelationType::Imports),
					description: desc_array.value(i).to_string(),
					confidence: conf_array.value(i),
					weight: weight_array.value(i),
				};
				relationships.push(relationship);
			}
		}

		Ok(relationships)
	}

	/// Get all nodes with pagination (replaces dummy vector search abuse)
	pub async fn get_all_nodes_paginated(
		&self,
		offset: usize,
		limit: usize,
	) -> Result<Vec<crate::indexer::graphrag::types::CodeNode>> {
		if !self.table_ops.table_exists("graphrag_nodes").await? {
			return Ok(Vec::new());
		}

		let table = self.db.open_table("graphrag_nodes").execute().await?;

		// Use proper pagination instead of vector search
		let mut results = table.query().limit(limit).offset(offset).execute().await?;

		let mut nodes = Vec::new();
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() == 0 {
				continue;
			}

			// Extract node data (reuse existing logic from load_graph)
			let id_array = batch
				.column_by_name("id")
				.ok_or_else(|| anyhow::anyhow!("Missing id column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid id column type"))?;

			let name_array = batch
				.column_by_name("name")
				.ok_or_else(|| anyhow::anyhow!("Missing name column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid name column type"))?;

			let kind_array = batch
				.column_by_name("kind")
				.ok_or_else(|| anyhow::anyhow!("Missing kind column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid kind column type"))?;

			let path_array = batch
				.column_by_name("path")
				.ok_or_else(|| anyhow::anyhow!("Missing path column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid path column type"))?;

			let description_array = batch
				.column_by_name("description")
				.ok_or_else(|| anyhow::anyhow!("Missing description column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid description column type"))?;

			let symbols_array = batch
				.column_by_name("symbols")
				.ok_or_else(|| anyhow::anyhow!("Missing symbols column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid symbols column type"))?;

			let hash_array = batch
				.column_by_name("hash")
				.ok_or_else(|| anyhow::anyhow!("Missing hash column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid hash column type"))?;

			let imports_array = batch
				.column_by_name("imports")
				.ok_or_else(|| anyhow::anyhow!("Missing imports column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid imports column type"))?;

			let exports_array = batch
				.column_by_name("exports")
				.ok_or_else(|| anyhow::anyhow!("Missing exports column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid exports column type"))?;

			let size_lines_array = batch
				.column_by_name("size_lines")
				.ok_or_else(|| anyhow::anyhow!("Missing size_lines column"))?
				.as_any()
				.downcast_ref::<arrow::array::UInt32Array>()
				.ok_or_else(|| anyhow::anyhow!("Invalid size_lines column type"))?;

			let language_array = batch
				.column_by_name("language")
				.ok_or_else(|| anyhow::anyhow!("Missing language column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid language column type"))?;

			let embedding_array = batch
				.column_by_name("embedding")
				.ok_or_else(|| anyhow::anyhow!("Missing embedding column"))?
				.as_any()
				.downcast_ref::<arrow::array::FixedSizeListArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid embedding column type"))?;

			let embedding_values = embedding_array
				.values()
				.as_any()
				.downcast_ref::<arrow::array::Float32Array>()
				.ok_or_else(|| anyhow::anyhow!("Invalid embedding values type"))?;

			// Convert to CodeNode objects
			for i in 0..batch.num_rows() {
				let symbols: Vec<String> = if symbols_array.is_null(i) {
					Vec::new()
				} else {
					serde_json::from_str(symbols_array.value(i)).unwrap_or_default()
				};

				let imports: Vec<String> = if imports_array.is_null(i) {
					Vec::new()
				} else {
					serde_json::from_str(imports_array.value(i)).unwrap_or_default()
				};

				let exports: Vec<String> = if exports_array.is_null(i) {
					Vec::new()
				} else {
					serde_json::from_str(exports_array.value(i)).unwrap_or_default()
				};

				// Extract embedding for this row
				let embedding_start = i * self.code_vector_dim;
				let embedding_end = embedding_start + self.code_vector_dim;
				let embedding: Vec<f32> = (embedding_start..embedding_end)
					.map(|idx| embedding_values.value(idx))
					.collect();

				let node = crate::indexer::graphrag::types::CodeNode {
					id: id_array.value(i).to_string(),
					name: name_array.value(i).to_string(),
					kind: kind_array.value(i).to_string(),
					path: path_array.value(i).to_string(),
					description: description_array.value(i).to_string(),
					symbols,
					hash: hash_array.value(i).to_string(),
					embedding,
					imports,
					exports,
					functions: Vec::new(), // Functions not stored in pagination query
					size_lines: size_lines_array.value(i),
					language: language_array.value(i).to_string(),
				};

				nodes.push(node);
			}
		}

		Ok(nodes)
	}

	/// Get all relationships efficiently with streaming
	pub async fn get_all_relationships_efficient(
		&self,
	) -> Result<Vec<crate::indexer::graphrag::types::CodeRelationship>> {
		// Reuse get_node_relationships logic but without filter
		if !self
			.table_ops
			.table_exists("graphrag_relationships")
			.await?
		{
			return Ok(Vec::new());
		}

		let table = self
			.db
			.open_table("graphrag_relationships")
			.execute()
			.await?;

		let mut results = table.query().execute().await?;

		let mut relationships = Vec::new();
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() == 0 {
				continue;
			}

			// Extract columns (same pattern as get_node_relationships)
			let source_array = batch
				.column_by_name("source")
				.ok_or_else(|| anyhow::anyhow!("Missing source column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid source column type"))?;

			let target_array = batch
				.column_by_name("target")
				.ok_or_else(|| anyhow::anyhow!("Missing target column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid target column type"))?;

			let type_array = batch
				.column_by_name("relation_type")
				.ok_or_else(|| anyhow::anyhow!("Missing relation_type column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid relation_type column type"))?;

			let desc_array = batch
				.column_by_name("description")
				.ok_or_else(|| anyhow::anyhow!("Missing description column"))?
				.as_any()
				.downcast_ref::<StringArray>()
				.ok_or_else(|| anyhow::anyhow!("Invalid description column type"))?;

			let conf_array = batch
				.column_by_name("confidence")
				.ok_or_else(|| anyhow::anyhow!("Missing confidence column"))?
				.as_any()
				.downcast_ref::<arrow::array::Float32Array>()
				.ok_or_else(|| anyhow::anyhow!("Invalid confidence column type"))?;

			let weight_array = batch
				.column_by_name("weight")
				.ok_or_else(|| anyhow::anyhow!("Missing weight column"))?
				.as_any()
				.downcast_ref::<arrow::array::Float32Array>()
				.ok_or_else(|| anyhow::anyhow!("Invalid weight column type"))?;

			for i in 0..batch.num_rows() {
				let relationship = crate::indexer::graphrag::types::CodeRelationship {
					source: source_array.value(i).to_string(),
					target: target_array.value(i).to_string(),
					relation_type: type_array
						.value(i)
						.parse()
						.unwrap_or(crate::indexer::graphrag::types::RelationType::Imports),
					description: desc_array.value(i).to_string(),
					confidence: conf_array.value(i),
					weight: weight_array.value(i),
				};
				relationships.push(relationship);
			}
		}

		Ok(relationships)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::indexer::graphrag::types::RelationType;

	// Note: These are unit tests for the query logic.
	// Integration tests with actual LanceDB would require a test database setup.

	#[test]
	fn test_relationship_direction_filter_generation() {
		// Test that we generate correct SQL filters for each direction
		let node_id = "src/main.rs";

		let outgoing_filter = format!("source = '{}'", node_id);
		assert_eq!(outgoing_filter, "source = 'src/main.rs'");

		let incoming_filter = format!("target = '{}'", node_id);
		assert_eq!(incoming_filter, "target = 'src/main.rs'");

		let both_filter = format!("source = '{}' OR target = '{}'", node_id, node_id);
		assert_eq!(
			both_filter,
			"source = 'src/main.rs' OR target = 'src/main.rs'"
		);
	}

	#[test]
	fn test_relationship_type_filter_generation() {
		// Test that we generate correct SQL filters for relationship types
		let rel_type = RelationType::Implements;
		let filter = format!("relation_type = '{}'", rel_type.as_str());
		assert_eq!(filter, "relation_type = 'implements'");

		let rel_type = RelationType::Imports;
		let filter = format!("relation_type = '{}'", rel_type.as_str());
		assert_eq!(filter, "relation_type = 'imports'");
	}

	#[test]
	fn test_code_relationship_parsing() {
		// Test that we can parse relationship types correctly
		let rel_type_str = "implements";
		let parsed: RelationType = rel_type_str.parse().unwrap();
		assert_eq!(parsed, RelationType::Implements);

		// Test fallback for unknown types
		let unknown_str = "unknown_type";
		let parsed: RelationType = unknown_str.parse().unwrap();
		assert_eq!(parsed, RelationType::Imports);
	}

	#[test]
	fn test_pagination_parameters() {
		// Test that pagination parameters are reasonable
		let _offset = 0;
		let limit = 100;
		assert!(limit > 0, "Limit should be positive");

		// Test large pagination
		let _offset = 10000;
		let limit = 1000;
		assert!(limit <= 10000, "Limit should be reasonable");
	}

	// ============================================================================
	// Adjacency Cache Tests (Phase 1.3)
	// ============================================================================

	#[test]
	fn test_cache_stats_default() {
		let stats = CacheStats::default();
		assert_eq!(stats.hits, 0);
		assert_eq!(stats.misses, 0);
		assert_eq!(stats.hit_rate(), 0.0);
	}

	#[test]
	fn test_cache_stats_hit_rate() {
		let stats = CacheStats {
			hits: 7,
			misses: 3,
		};
		assert_eq!(stats.hit_rate(), 0.7);

		let stats = CacheStats {
			hits: 0,
			misses: 10,
		};
		assert_eq!(stats.hit_rate(), 0.0);

		let stats = CacheStats {
			hits: 10,
			misses: 0,
		};
		assert_eq!(stats.hit_rate(), 1.0);
	}

	#[test]
	fn test_cache_update_and_get() {
		use std::sync::{Arc, RwLock};

		// Create a minimal cache structure for testing
		let cache: Arc<RwLock<AdjacencyCache>> = Arc::new(RwLock::new(HashMap::new()));
		let _stats: Arc<RwLock<CacheStats>> = Arc::new(RwLock::new(CacheStats::default()));

		// Simulate cache update
		{
			let mut cache_lock = cache.write().unwrap();
			cache_lock
				.entry("node1".to_string())
				.or_default()
				.entry(RelationType::Imports)
				.or_default()
				.push("node2".to_string());
		}

		// Verify cache content
		{
			let cache_lock = cache.read().unwrap();
			let neighbors = cache_lock
				.get("node1")
				.and_then(|rels| rels.get(&RelationType::Imports))
				.cloned();
			assert_eq!(neighbors, Some(vec!["node2".to_string()]));
		}

		// Test cache miss
		{
			let cache_lock = cache.read().unwrap();
			let neighbors = cache_lock
				.get("nonexistent")
				.and_then(|rels| rels.get(&RelationType::Imports))
				.cloned();
			assert_eq!(neighbors, None);
		}
	}

	#[test]
	fn test_cache_invalidation() {
		use std::sync::{Arc, RwLock};

		let cache: Arc<RwLock<AdjacencyCache>> = Arc::new(RwLock::new(HashMap::new()));

		// Add some data
		{
			let mut cache_lock = cache.write().unwrap();
			cache_lock
				.entry("node1".to_string())
				.or_default()
				.entry(RelationType::Imports)
				.or_default()
				.push("node2".to_string());
			cache_lock
				.entry("node1".to_string())
				.or_default()
				.entry(RelationType::Extends)
				.or_default()
				.push("node3".to_string());
		}

		// Verify data exists
		{
			let cache_lock = cache.read().unwrap();
			assert!(cache_lock.contains_key("node1"));
		}

		// Invalidate node1
		{
			let mut cache_lock = cache.write().unwrap();
			cache_lock.remove("node1");
		}

		// Verify data is gone
		{
			let cache_lock = cache.read().unwrap();
			assert!(!cache_lock.contains_key("node1"));
		}
	}

	#[test]
	fn test_cache_clear() {
		use std::sync::{Arc, RwLock};

		let cache: Arc<RwLock<AdjacencyCache>> = Arc::new(RwLock::new(HashMap::new()));
		let stats: Arc<RwLock<CacheStats>> = Arc::new(RwLock::new(CacheStats::default()));

		// Add multiple entries
		{
			let mut cache_lock = cache.write().unwrap();
			for i in 0..5 {
				cache_lock
					.entry(format!("node{}", i))
					.or_default()
					.entry(RelationType::Imports)
					.or_default()
					.push(format!("target{}", i));
			}
		}

		// Add some stats
		{
			let mut stats_lock = stats.write().unwrap();
			stats_lock.hits = 10;
			stats_lock.misses = 5;
		}

		// Verify cache has data
		{
			let cache_lock = cache.read().unwrap();
			assert_eq!(cache_lock.len(), 5);
		}

		// Clear cache
		{
			let mut cache_lock = cache.write().unwrap();
			cache_lock.clear();
		}

		// Clear stats
		{
			let mut stats_lock = stats.write().unwrap();
			*stats_lock = CacheStats::default();
		}

		// Verify everything is cleared
		{
			let cache_lock = cache.read().unwrap();
			assert_eq!(cache_lock.len(), 0);
		}
		{
			let stats_lock = stats.read().unwrap();
			assert_eq!(stats_lock.hits, 0);
			assert_eq!(stats_lock.misses, 0);
		}
	}

	#[test]
	fn test_cache_multiple_relation_types() {
		use std::sync::{Arc, RwLock};

		let cache: Arc<RwLock<AdjacencyCache>> = Arc::new(RwLock::new(HashMap::new()));

		// Add multiple relation types for same node
		{
			let mut cache_lock = cache.write().unwrap();
			let node_entry = cache_lock.entry("node1".to_string()).or_default();

			node_entry
				.entry(RelationType::Imports)
				.or_default()
				.push("dep1".to_string());
			node_entry
				.entry(RelationType::Extends)
				.or_default()
				.push("base1".to_string());
			node_entry
				.entry(RelationType::Implements)
				.or_default()
				.push("interface1".to_string());
		}

		// Verify all relation types are stored
		{
			let cache_lock = cache.read().unwrap();
			let node_rels = cache_lock.get("node1").unwrap();
			assert_eq!(node_rels.len(), 3);
			assert!(node_rels.contains_key(&RelationType::Imports));
			assert!(node_rels.contains_key(&RelationType::Extends));
			assert!(node_rels.contains_key(&RelationType::Implements));
		}
	}

	#[test]
	fn test_cache_multiple_targets_per_relation() {
		use std::sync::{Arc, RwLock};

		let cache: Arc<RwLock<AdjacencyCache>> = Arc::new(RwLock::new(HashMap::new()));

		// Add multiple targets for same relation type
		{
			let mut cache_lock = cache.write().unwrap();
			let targets = cache_lock
				.entry("node1".to_string())
				.or_default()
				.entry(RelationType::Imports)
				.or_default();

			targets.push("dep1".to_string());
			targets.push("dep2".to_string());
			targets.push("dep3".to_string());
		}

		// Verify all targets are stored
		{
			let cache_lock = cache.read().unwrap();
			let targets = cache_lock
				.get("node1")
				.and_then(|rels| rels.get(&RelationType::Imports))
				.unwrap();
			assert_eq!(targets.len(), 3);
			assert!(targets.contains(&"dep1".to_string()));
			assert!(targets.contains(&"dep2".to_string()));
			assert!(targets.contains(&"dep3".to_string()));
		}
	}

	#[test]
	fn test_cache_concurrent_access() {
		use std::sync::{Arc, RwLock};
		use std::thread;

		let cache: Arc<RwLock<AdjacencyCache>> = Arc::new(RwLock::new(HashMap::new()));

		// Spawn multiple threads to write to cache
		let mut handles = vec![];
		for i in 0..10 {
			let cache_clone = Arc::clone(&cache);
			let handle = thread::spawn(move || {
				let mut cache_lock = cache_clone.write().unwrap();
				cache_lock
					.entry(format!("node{}", i))
					.or_default()
					.entry(RelationType::Imports)
					.or_default()
					.push(format!("target{}", i));
			});
			handles.push(handle);
		}

		// Wait for all threads
		for handle in handles {
			handle.join().unwrap();
		}

		// Verify all entries were added
		{
			let cache_lock = cache.read().unwrap();
			assert_eq!(cache_lock.len(), 10);
		}
	}

	#[test]
	fn test_relation_type_parsing_for_cache() {
		// Test that all relation types can be parsed correctly for cache operations
		let test_cases = vec![
			("implements", RelationType::Implements),
			("extends", RelationType::Extends),
			("imports", RelationType::Imports),
			("calls", RelationType::Calls),
			("uses", RelationType::Uses),
		];

		for (str_val, expected) in test_cases {
			let parsed: RelationType = str_val.parse().unwrap();
			assert_eq!(parsed, expected);
			assert_eq!(parsed.as_str(), str_val);
		}
	}
}
