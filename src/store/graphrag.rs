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
use std::sync::Arc;

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

/// Handles GraphRAG-specific database operations
pub struct GraphRagOperations<'a> {
	pub db: &'a Connection,
	pub table_ops: TableOperations<'a>,
	pub code_vector_dim: usize,
}

impl<'a> GraphRagOperations<'a> {
	pub fn new(db: &'a Connection, code_vector_dim: usize) -> Self {
		Self {
			db,
			table_ops: TableOperations::new(db),
			code_vector_dim,
		}
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
		// Open or create the table
		self.table_ops
			.store_batch("graphrag_relationships", rel_batch)
			.await
	}

	/// Clear all graph nodes from the database
	pub async fn clear_graph_nodes(&self) -> Result<()> {
		self.table_ops.clear_table("graphrag_nodes").await
	}

	/// Clear all graph relationships from the database
	pub async fn clear_graph_relationships(&self) -> Result<()> {
		self.table_ops.clear_table("graphrag_relationships").await
	}

	/// Remove GraphRAG nodes associated with a specific file path
	pub async fn remove_graph_nodes_by_path(&self, file_path: &str) -> Result<usize> {
		// CRITICAL FIX: Also remove relationships when removing nodes
		let relationships_removed = self.remove_graph_relationships_by_path(file_path).await?;
		let nodes_removed = self
			.table_ops
			.remove_blocks_by_path(file_path, "graphrag_nodes")
			.await?;

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
}
