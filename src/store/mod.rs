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
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// Arrow imports
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;

// LanceDB imports
use futures::TryStreamExt;
use lancedb::{
	connect,
	query::{ExecutableQuery, QueryBase},
	Connection, DistanceType,
};

// Import modular components
use self::{
	batch_converter::BatchConverter, debug::DebugOperations, graphrag::GraphRagOperations,
	metadata::MetadataOperations, table_ops::TableOperations,
};

pub mod batch_converter;
pub mod debug;
pub mod graphrag;
pub mod metadata;
pub mod table_ops;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CodeBlock {
	pub path: String,
	pub language: String,
	pub content: String,
	pub symbols: Vec<String>,
	pub start_line: usize,
	pub end_line: usize,
	pub hash: String,
	// Optional distance field for relevance sorting (higher is more relevant)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub distance: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TextBlock {
	pub path: String,
	pub language: String,
	pub content: String,
	pub start_line: usize,
	pub end_line: usize,
	pub hash: String,
	// Optional distance field for relevance sorting (higher is more relevant)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub distance: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DocumentBlock {
	pub path: String,
	pub title: String,
	pub content: String,      // Storage content only
	pub context: Vec<String>, // Hierarchical context (optional)
	pub level: usize,
	pub start_line: usize,
	pub end_line: usize,
	pub hash: String,
	// Optional distance field for relevance sorting (higher is more relevant)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub distance: Option<f32>,
}

pub struct Store {
	db: Connection,
	code_vector_dim: usize, // Size of code embedding vectors
	text_vector_dim: usize, // Size of text embedding vectors
}

// Implementing Drop for the Store
impl Drop for Store {
	fn drop(&mut self) {
		if cfg!(debug_assertions) {
			tracing::debug!("Store instance dropped, database connection released");
		}
	}
}

impl Store {
	pub async fn new() -> Result<Self> {
		// Get current directory
		let current_dir = std::env::current_dir()?;

		// Get the project database path using the new storage system
		let index_path = crate::storage::get_project_database_path(&current_dir)?;

		// Ensure the directory exists
		crate::storage::ensure_project_storage_exists(&current_dir)?;

		// Ensure the database directory exists
		if !index_path.exists() {
			std::fs::create_dir_all(&index_path)?;
		}

		// Convert the path to a string for the file-based database
		let storage_path = index_path
			.to_str()
			.ok_or_else(|| anyhow::anyhow!("Invalid database path"))?;

		// Load the config to get the embedding provider and model info
		let config = crate::config::Config::load()?;

		// Get vector dimensions from both code and text model configurations
		let (code_provider, code_model) =
			crate::embedding::parse_provider_model(&config.embedding.code_model);
		let code_vector_dim = config
			.embedding
			.get_vector_dimension(&code_provider, &code_model);

		let (text_provider, text_model) =
			crate::embedding::parse_provider_model(&config.embedding.text_model);
		let text_vector_dim = config
			.embedding
			.get_vector_dimension(&text_provider, &text_model);

		// Connect to LanceDB
		let db = connect(storage_path).execute().await?;

		// Check if tables exist and if their schema matches the current configuration
		let table_names = db.table_names().execute().await?;

		// Check for schema mismatches and recreate tables if necessary
		for table_name in [
			"code_blocks",
			"text_blocks",
			"document_blocks",
			"graphrag_nodes",
		] {
			if table_names.contains(&table_name.to_string()) {
				if let Ok(table) = db.open_table(table_name).execute().await {
					if let Ok(schema) = table.schema().await {
						// Check if embedding field has the right dimension
						if let Ok(field) = schema.field_with_name("embedding") {
							if let DataType::FixedSizeList(_, size) = field.data_type() {
								let expected_dim = match table_name {
									"code_blocks" | "graphrag_nodes" => code_vector_dim as i32,
									"text_blocks" | "document_blocks" => text_vector_dim as i32,
									_ => continue,
								};

								if size != &expected_dim {
									tracing::warn!("Schema mismatch detected for table '{}': expected dimension {}, found {}. Dropping table for recreation.",
										table_name, expected_dim, size);
									drop(table); // Release table handle before dropping
									if let Err(e) = db.drop_table(table_name).await {
										tracing::warn!(
											"Failed to drop table {}: {}",
											table_name,
											e
										);
									}
								}
							}
						}
					}
				}
			}
		}

		Ok(Self {
			db,
			code_vector_dim,
			text_vector_dim,
		})
	}

	pub async fn initialize_collections(&self) -> Result<()> {
		// Check if tables exist, if not create them
		let table_names = self.db.table_names().execute().await?;

		// Create code_blocks table if it doesn't exist
		if !table_names.contains(&"code_blocks".to_string()) {
			let schema = Arc::new(Schema::new(vec![
				Field::new("id", DataType::Utf8, false),
				Field::new("path", DataType::Utf8, false),
				Field::new("language", DataType::Utf8, false),
				Field::new("content", DataType::Utf8, false),
				Field::new("symbols", DataType::Utf8, true),
				Field::new("start_line", DataType::UInt32, false),
				Field::new("end_line", DataType::UInt32, false),
				Field::new("hash", DataType::Utf8, false),
				Field::new(
					"embedding",
					DataType::FixedSizeList(
						Arc::new(Field::new("item", DataType::Float32, true)),
						self.code_vector_dim as i32,
					),
					true,
				),
			]));

			let _table = self
				.db
				.create_empty_table("code_blocks", schema)
				.execute()
				.await?;
		}

		// Create text_blocks table if it doesn't exist
		if !table_names.contains(&"text_blocks".to_string()) {
			let schema = Arc::new(Schema::new(vec![
				Field::new("id", DataType::Utf8, false),
				Field::new("path", DataType::Utf8, false),
				Field::new("language", DataType::Utf8, false),
				Field::new("content", DataType::Utf8, false),
				Field::new("start_line", DataType::UInt32, false),
				Field::new("end_line", DataType::UInt32, false),
				Field::new("hash", DataType::Utf8, false),
				Field::new(
					"embedding",
					DataType::FixedSizeList(
						Arc::new(Field::new("item", DataType::Float32, true)),
						self.text_vector_dim as i32,
					),
					true,
				),
			]));

			let _table = self
				.db
				.create_empty_table("text_blocks", schema)
				.execute()
				.await?;
		}

		// Create document_blocks table if it doesn't exist
		if !table_names.contains(&"document_blocks".to_string()) {
			let schema = Arc::new(Schema::new(vec![
				Field::new("id", DataType::Utf8, false),
				Field::new("path", DataType::Utf8, false),
				Field::new("title", DataType::Utf8, false),
				Field::new("content", DataType::Utf8, false),
				Field::new(
					"context",
					DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
					true,
				),
				Field::new("level", DataType::UInt32, false),
				Field::new("start_line", DataType::UInt32, false),
				Field::new("end_line", DataType::UInt32, false),
				Field::new("hash", DataType::Utf8, false),
				Field::new(
					"embedding",
					DataType::FixedSizeList(
						Arc::new(Field::new("item", DataType::Float32, true)),
						self.text_vector_dim as i32,
					),
					true,
				),
			]));

			let _table = self
				.db
				.create_empty_table("document_blocks", schema)
				.execute()
				.await?;
		}

		Ok(())
	}

	// Delegate operations to modular components
	pub async fn content_exists(&self, hash: &str, collection: &str) -> Result<bool> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.content_exists(hash, collection).await
	}

	pub async fn store_code_blocks(
		&self,
		blocks: &[CodeBlock],
		embeddings: &[Vec<f32>],
	) -> Result<()> {
		let converter = BatchConverter::new(self.code_vector_dim);
		let batch = converter.code_block_to_batch(blocks, embeddings)?;

		let table_ops = TableOperations::new(&self.db);
		table_ops.store_batch("code_blocks", batch).await?;

		// Create vector index if needed
		if let Ok(table) = self.db.open_table("code_blocks").execute().await {
			let row_count = table.count_rows(None).await?;
			let has_index = table
				.list_indices()
				.await?
				.iter()
				.any(|idx| idx.columns == vec!["embedding"]);

			if !has_index && row_count > 256 {
				if let Err(e) = table_ops
					.create_vector_index("code_blocks", "embedding", DistanceType::Cosine)
					.await
				{
					tracing::warn!("Failed to create vector index: {}", e);
				}
			}
		}

		Ok(())
	}

	pub async fn store_text_blocks(
		&self,
		blocks: &[TextBlock],
		embeddings: &[Vec<f32>],
	) -> Result<()> {
		let converter = BatchConverter::new(self.text_vector_dim);
		let batch = converter.text_block_to_batch(blocks, embeddings)?;

		let table_ops = TableOperations::new(&self.db);
		table_ops.store_batch("text_blocks", batch).await?;

		// Create vector index if needed
		if let Ok(table) = self.db.open_table("text_blocks").execute().await {
			let row_count = table.count_rows(None).await?;
			let has_index = table
				.list_indices()
				.await?
				.iter()
				.any(|idx| idx.columns == vec!["embedding"]);

			if !has_index && row_count > 256 {
				if let Err(e) = table_ops
					.create_vector_index("text_blocks", "embedding", DistanceType::Cosine)
					.await
				{
					tracing::warn!("Failed to create vector index: {}", e);
				}
			}
		}

		Ok(())
	}

	pub async fn store_document_blocks(
		&self,
		blocks: &[DocumentBlock],
		embeddings: &[Vec<f32>],
	) -> Result<()> {
		let converter = BatchConverter::new(self.text_vector_dim);
		let batch = converter.document_block_to_batch(blocks, embeddings)?;

		let table_ops = TableOperations::new(&self.db);
		table_ops.store_batch("document_blocks", batch).await?;

		// Create vector index if needed
		if let Ok(table) = self.db.open_table("document_blocks").execute().await {
			let row_count = table.count_rows(None).await?;
			let has_index = table
				.list_indices()
				.await?
				.iter()
				.any(|idx| idx.columns == vec!["embedding"]);

			if !has_index && row_count > 256 {
				if let Err(e) = table_ops
					.create_vector_index("document_blocks", "embedding", DistanceType::Cosine)
					.await
				{
					tracing::warn!("Failed to create vector index: {}", e);
				}
			}
		}

		Ok(())
	}

	// Search operations with distance conversion
	pub async fn get_code_blocks(&self, embedding: Vec<f32>) -> Result<Vec<CodeBlock>> {
		self.get_code_blocks_with_config(embedding, None, None)
			.await
	}

	pub async fn get_code_blocks_with_config(
		&self,
		embedding: Vec<f32>,
		limit: Option<usize>,
		distance_threshold: Option<f32>,
	) -> Result<Vec<CodeBlock>> {
		let table_ops = TableOperations::new(&self.db);
		if !table_ops.table_exists("code_blocks").await? {
			return Ok(Vec::new());
		}

		let table = self.db.open_table("code_blocks").execute().await?;
		let mut results = table
			.query()
			.nearest_to(embedding)?
			.limit(limit.unwrap_or(10))
			.execute()
			.await?;

		let mut all_code_blocks = Vec::new();
		let converter = BatchConverter::new(self.code_vector_dim);

		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				let mut code_blocks = converter.batch_to_code_blocks(&batch, None)?;

				// Apply distance threshold if specified
				if let Some(threshold) = distance_threshold {
					code_blocks.retain(|block| block.distance.is_none_or(|d| d >= threshold));
				}

				all_code_blocks.append(&mut code_blocks);
			}
		}

		Ok(all_code_blocks)
	}

	// Similar implementations for text and document blocks...
	pub async fn get_text_blocks(&self, embedding: Vec<f32>) -> Result<Vec<TextBlock>> {
		self.get_text_blocks_with_config(embedding, None, None)
			.await
	}

	pub async fn get_text_blocks_with_config(
		&self,
		embedding: Vec<f32>,
		limit: Option<usize>,
		distance_threshold: Option<f32>,
	) -> Result<Vec<TextBlock>> {
		let table_ops = TableOperations::new(&self.db);
		if !table_ops.table_exists("text_blocks").await? {
			return Ok(Vec::new());
		}

		let table = self.db.open_table("text_blocks").execute().await?;
		let mut results = table
			.query()
			.nearest_to(embedding)?
			.limit(limit.unwrap_or(10))
			.execute()
			.await?;

		let mut all_text_blocks = Vec::new();
		let converter = BatchConverter::new(self.text_vector_dim);

		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				let mut text_blocks = converter.batch_to_text_blocks(&batch, None)?;

				// Apply distance threshold if specified
				if let Some(threshold) = distance_threshold {
					text_blocks.retain(|block| block.distance.is_none_or(|d| d >= threshold));
				}

				all_text_blocks.append(&mut text_blocks);
			}
		}

		Ok(all_text_blocks)
	}

	pub async fn get_document_blocks(&self, embedding: Vec<f32>) -> Result<Vec<DocumentBlock>> {
		self.get_document_blocks_with_config(embedding, None, None)
			.await
	}

	pub async fn get_document_blocks_with_config(
		&self,
		embedding: Vec<f32>,
		limit: Option<usize>,
		distance_threshold: Option<f32>,
	) -> Result<Vec<DocumentBlock>> {
		let table_ops = TableOperations::new(&self.db);
		if !table_ops.table_exists("document_blocks").await? {
			return Ok(Vec::new());
		}

		let table = self.db.open_table("document_blocks").execute().await?;
		let mut results = table
			.query()
			.nearest_to(embedding)?
			.limit(limit.unwrap_or(10))
			.execute()
			.await?;

		let mut all_document_blocks = Vec::new();
		let converter = BatchConverter::new(self.text_vector_dim);

		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				let mut document_blocks = converter.batch_to_document_blocks(&batch, None)?;

				// Apply distance threshold if specified
				if let Some(threshold) = distance_threshold {
					document_blocks.retain(|block| block.distance.is_none_or(|d| d >= threshold));
				}

				all_document_blocks.append(&mut document_blocks);
			}
		}

		Ok(all_document_blocks)
	}

	// Delegate other operations to modular components
	pub async fn remove_blocks_by_path(&self, file_path: &str) -> Result<()> {
		let table_ops = TableOperations::new(&self.db);
		table_ops
			.remove_blocks_by_path(file_path, "code_blocks")
			.await?;
		table_ops
			.remove_blocks_by_path(file_path, "text_blocks")
			.await?;
		table_ops
			.remove_blocks_by_path(file_path, "document_blocks")
			.await?;
		Ok(())
	}

	pub async fn get_all_indexed_file_paths(&self) -> Result<std::collections::HashSet<String>> {
		let table_ops = TableOperations::new(&self.db);
		table_ops
			.get_all_indexed_file_paths(&["code_blocks", "text_blocks", "document_blocks"])
			.await
	}

	pub async fn flush(&self) -> Result<()> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.flush_all_tables().await
	}

	pub async fn close(self) -> Result<()> {
		// The database connection is closed automatically when the Store is dropped
		Ok(())
	}

	pub async fn clear_all_tables(&self) -> Result<()> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.clear_all_tables().await
	}

	pub async fn clear_code_table(&self) -> Result<()> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.clear_table("code_blocks").await
	}

	pub async fn clear_docs_table(&self) -> Result<()> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.clear_table("document_blocks").await
	}

	pub async fn clear_text_table(&self) -> Result<()> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.clear_table("text_blocks").await
	}

	pub fn get_code_vector_dim(&self) -> usize {
		self.code_vector_dim
	}

	// Metadata operations
	pub async fn store_git_metadata(&self, commit_hash: &str) -> Result<()> {
		let metadata_ops = MetadataOperations::new(&self.db);
		metadata_ops.store_git_metadata(commit_hash).await
	}

	pub async fn get_last_commit_hash(&self) -> Result<Option<String>> {
		let metadata_ops = MetadataOperations::new(&self.db);
		metadata_ops.get_last_commit_hash().await
	}

	pub async fn store_file_metadata(&self, file_path: &str, mtime: u64) -> Result<()> {
		let metadata_ops = MetadataOperations::new(&self.db);
		metadata_ops.store_file_metadata(file_path, mtime).await
	}

	pub async fn get_file_mtime(&self, file_path: &str) -> Result<Option<u64>> {
		let metadata_ops = MetadataOperations::new(&self.db);
		metadata_ops.get_file_mtime(file_path).await
	}

	pub async fn get_all_file_metadata(&self) -> Result<std::collections::HashMap<String, u64>> {
		let metadata_ops = MetadataOperations::new(&self.db);
		metadata_ops.get_all_file_metadata().await
	}

	pub async fn clear_git_metadata(&self) -> Result<()> {
		let metadata_ops = MetadataOperations::new(&self.db);
		metadata_ops.clear_git_metadata().await
	}

	// GraphRAG operations
	pub async fn graphrag_needs_indexing(&self) -> Result<bool> {
		let graphrag_ops = GraphRagOperations::new(&self.db, self.code_vector_dim);
		graphrag_ops.graphrag_needs_indexing().await
	}

	pub async fn get_all_code_blocks_for_graphrag(&self) -> Result<Vec<CodeBlock>> {
		let graphrag_ops = GraphRagOperations::new(&self.db, self.code_vector_dim);
		graphrag_ops.get_all_code_blocks_for_graphrag().await
	}

	pub async fn store_graph_nodes(&self, node_batch: RecordBatch) -> Result<()> {
		let graphrag_ops = GraphRagOperations::new(&self.db, self.code_vector_dim);
		graphrag_ops.store_graph_nodes(node_batch).await
	}

	pub async fn store_graph_relationships(&self, rel_batch: RecordBatch) -> Result<()> {
		let graphrag_ops = GraphRagOperations::new(&self.db, self.code_vector_dim);
		graphrag_ops.store_graph_relationships(rel_batch).await
	}

	pub async fn clear_graph_nodes(&self) -> Result<()> {
		let graphrag_ops = GraphRagOperations::new(&self.db, self.code_vector_dim);
		graphrag_ops.clear_graph_nodes().await
	}

	pub async fn clear_graph_relationships(&self) -> Result<()> {
		let graphrag_ops = GraphRagOperations::new(&self.db, self.code_vector_dim);
		graphrag_ops.clear_graph_relationships().await
	}

	pub async fn remove_graph_nodes_by_path(&self, file_path: &str) -> Result<usize> {
		let graphrag_ops = GraphRagOperations::new(&self.db, self.code_vector_dim);
		graphrag_ops.remove_graph_nodes_by_path(file_path).await
	}

	pub async fn remove_graph_relationships_by_path(&self, file_path: &str) -> Result<usize> {
		let graphrag_ops = GraphRagOperations::new(&self.db, self.code_vector_dim);
		graphrag_ops
			.remove_graph_relationships_by_path(file_path)
			.await
	}

	pub async fn search_graph_nodes(&self, embedding: &[f32], limit: usize) -> Result<RecordBatch> {
		let graphrag_ops = GraphRagOperations::new(&self.db, self.code_vector_dim);
		graphrag_ops.search_graph_nodes(embedding, limit).await
	}

	pub async fn get_graph_relationships(&self) -> Result<RecordBatch> {
		let graphrag_ops = GraphRagOperations::new(&self.db, self.code_vector_dim);
		graphrag_ops.get_graph_relationships().await
	}

	// Debug operations
	pub async fn list_indexed_files(&self) -> Result<()> {
		let debug_ops = DebugOperations::new(&self.db, self.code_vector_dim);
		debug_ops.list_indexed_files().await
	}

	pub async fn show_file_chunks(&self, file_path: &str) -> Result<()> {
		let debug_ops = DebugOperations::new(&self.db, self.code_vector_dim);
		debug_ops.show_file_chunks(file_path).await
	}

	// Additional methods for backward compatibility
	pub async fn get_code_block_by_symbol(&self, symbol: &str) -> Result<Option<CodeBlock>> {
		let table_ops = TableOperations::new(&self.db);
		if !table_ops.table_exists("code_blocks").await? {
			return Ok(None);
		}

		let table = self.db.open_table("code_blocks").execute().await?;
		let mut results = table
			.query()
			.only_if(format!("symbols LIKE '%{}%'", symbol))
			.limit(1)
			.execute()
			.await?;

		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				let converter = BatchConverter::new(self.code_vector_dim);
				let code_blocks = converter.batch_to_code_blocks(&batch, None)?;
				return Ok(code_blocks.into_iter().next());
			}
		}

		Ok(None)
	}

	pub async fn get_code_block_by_hash(&self, hash: &str) -> Result<CodeBlock> {
		let table_ops = TableOperations::new(&self.db);
		if !table_ops.table_exists("code_blocks").await? {
			return Err(anyhow::anyhow!("Code blocks table does not exist"));
		}

		let table = self.db.open_table("code_blocks").execute().await?;
		let mut results = table
			.query()
			.only_if(format!("hash = '{}'", hash))
			.limit(1)
			.execute()
			.await?;

		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				let converter = BatchConverter::new(self.code_vector_dim);
				let code_blocks = converter.batch_to_code_blocks(&batch, None)?;
				return code_blocks
					.into_iter()
					.next()
					.ok_or_else(|| anyhow::anyhow!("Failed to convert result to CodeBlock"));
			}
		}

		Err(anyhow::anyhow!("Code block with hash {} not found", hash))
	}

	pub async fn tables_exist(&self, table_names: &[&str]) -> Result<bool> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.tables_exist(table_names).await
	}

	// Add missing methods for backward compatibility
	pub async fn get_file_blocks_metadata(
		&self,
		file_path: &str,
		table_name: &str,
	) -> Result<Vec<String>> {
		let table_ops = TableOperations::new(&self.db);
		table_ops
			.get_file_blocks_metadata(file_path, table_name)
			.await
	}

	pub async fn remove_blocks_by_hashes(&self, hashes: &[String], table_name: &str) -> Result<()> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.remove_blocks_by_hashes(hashes, table_name).await
	}
}
