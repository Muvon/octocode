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
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// Arrow imports
use arrow_array::RecordBatch;
use arrow_schema::{DataType, Field, Schema};

// LanceDB imports
use futures::TryStreamExt;
use lancedb::{
	connect,
	index::scalar::FullTextSearchQuery,
	query::{ExecutableQuery, QueryBase},
	Connection, DistanceType, Table,
};
use tokio::sync::RwLock;

// Import modular components
use self::{
	batch_converter::BatchConverter, block_trait::BlockType, debug::DebugOperations,
	graphrag::GraphRagOperations, metadata::MetadataOperations, table_ops::TableOperations,
	vector_optimizer::VectorOptimizer,
};

pub mod batch_converter;
pub mod block_trait;
pub mod debug;
pub mod graphrag;
#[cfg(test)]
mod hybrid_tests;
pub mod metadata;
pub mod table_ops;
pub mod vector_optimizer;

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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommitBlock {
	pub hash: String, // commit SHA (dedup key)
	pub author: String,
	pub date: i64,           // unix timestamp
	pub message: String,     // original commit message
	pub content: String,     // composite: message + files + AI desc (FTS + embedding source)
	pub files: String,       // JSON array of changed paths
	pub description: String, // AI-generated description (empty if no LLM)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub distance: Option<f32>,
}
/// Hybrid search query combining vector and keyword signals
#[derive(Debug, Clone)]
pub struct HybridSearchQuery {
	/// Vector semantic search query (embedding)
	pub vector_query: Option<Vec<f32>>,
	/// Raw query string for full-text search (BM25 via LanceDB FTS index)
	pub keywords: Option<String>,
	/// Weight for vector similarity signal (0.0-1.0)
	pub vector_weight: f32,
	/// Weight for keyword matching signal (0.0-1.0)
	pub keyword_weight: f32,
	/// Maximum number of results to return
	pub limit: usize,
	/// Minimum relevance threshold (similarity, 0.0-1.0)
	pub min_relevance: Option<f32>,
	/// Language filter (for code/text blocks)
	pub language_filter: Option<String>,
}

impl HybridSearchQuery {
	/// Validate that weights are in valid ranges
	pub fn validate(&self) -> Result<(), String> {
		if self.vector_weight < 0.0 || self.vector_weight > 1.0 {
			return Err(format!(
				"vector_weight must be in [0.0, 1.0], got {}",
				self.vector_weight
			));
		}
		if self.keyword_weight < 0.0 || self.keyword_weight > 1.0 {
			return Err(format!(
				"keyword_weight must be in [0.0, 1.0], got {}",
				self.keyword_weight
			));
		}
		if self.vector_query.is_none() && self.keywords.is_none() {
			return Err("At least one of vector_query or keywords must be provided".to_string());
		}
		Ok(())
	}
}

pub struct Store {
	db: Connection,
	code_vector_dim: usize, // Size of code embedding vectors
	text_vector_dim: usize, // Size of text embedding vectors
	// Cache for table instances to avoid repeated opening overhead
	table_cache: Arc<RwLock<HashMap<String, Arc<Table>>>>,
	// Cache for "vector index exists" flag per table. Populated lazily on first
	// successful index check; subsequent batch inserts skip the open+list_indices
	// round-trip. Cleared when a new index is created.
	vector_index_present: Arc<RwLock<HashMap<String, bool>>>,
	// Cache for "FTS index exists" flag per table. Same rationale.
	fts_index_present: Arc<RwLock<HashMap<String, bool>>>,
	// Cached `[index].quantization` setting; avoids re-reading TOML on every batch.
	quantization: bool,
	// Cached `[search.hybrid].enabled` setting; avoids re-reading TOML on every batch.
	hybrid_enabled: bool,
}

impl Store {
	/// Create a new Store for the current project (default branch database).
	pub async fn new() -> Result<Self> {
		let current_dir = std::env::current_dir()?;
		let index_path = crate::storage::get_project_database_path(&current_dir)?;
		crate::storage::ensure_project_storage_exists(&current_dir)?;
		Self::new_with_path(index_path).await
	}

	/// Create a new Store for a specific branch's delta database.
	pub async fn new_for_branch(branch_name: &str) -> Result<Self> {
		let current_dir = std::env::current_dir()?;
		let index_path = crate::storage::get_branch_database_path(&current_dir, branch_name)?;
		Self::new_with_path(index_path).await
	}

	/// Create a new Store backed by a LanceDB database at the given path.
	pub async fn new_with_path(index_path: std::path::PathBuf) -> Result<Self> {
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
			crate::embedding::parse_provider_model(&config.embedding.code_model)
				.map_err(|e| anyhow::anyhow!("Failed to parse code model: {}", e))?;
		let code_vector_dim = config
			.embedding
			.get_vector_dimension(&code_provider, &code_model)
			.await?;

		let (text_provider, text_model) =
			crate::embedding::parse_provider_model(&config.embedding.text_model)
				.map_err(|e| anyhow::anyhow!("Failed to parse text model: {}", e))?;
		let text_vector_dim = config
			.embedding
			.get_vector_dimension(&text_provider, &text_model)
			.await?;

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
									if let Err(e) = db.drop_table(table_name, &[]).await {
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
			table_cache: Arc::new(RwLock::new(HashMap::new())),
			vector_index_present: Arc::new(RwLock::new(HashMap::new())),
			fts_index_present: Arc::new(RwLock::new(HashMap::new())),
			quantization: config.index.quantization,
			hybrid_enabled: config.search.hybrid.enabled,
		})
	}

	/// Get or open a table with caching to avoid repeated open overhead
	/// This is critical for performance when running multiple queries
	async fn get_table(&self, table_name: &str) -> Result<Arc<Table>> {
		// Check cache first (read lock)
		{
			let cache = self.table_cache.read().await;
			if let Some(table) = cache.get(table_name) {
				return Ok(Arc::clone(table));
			}
		}

		// Not in cache, open it (write lock)
		let mut cache = self.table_cache.write().await;

		// Double-check after acquiring write lock (another task may have opened it)
		if let Some(table) = cache.get(table_name) {
			return Ok(Arc::clone(table));
		}

		// Open table and cache it
		let table = self.db.open_table(table_name).execute().await?;
		let table = Arc::new(table);
		cache.insert(table_name.to_string(), Arc::clone(&table));

		Ok(table)
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
		// Use cached table handle — `content_exists` is called per-block during the
		// differential pass; opening the table each time costs a manifest read.
		if !TableOperations::new(&self.db)
			.table_exists(collection)
			.await?
		{
			return Ok(false);
		}
		let table = self.get_table(collection).await?;
		let mut results = table
			.query()
			.only_if(format!("hash = '{}'", hash))
			.limit(1)
			.select(lancedb::query::Select::Columns(vec!["hash".to_string()]))
			.execute()
			.await?;
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				return Ok(true);
			}
		}
		Ok(false)
	}

	pub async fn store_code_blocks(
		&self,
		blocks: &[CodeBlock],
		embeddings: &[Vec<f32>],
	) -> Result<()> {
		self.store_blocks(blocks, embeddings, self.code_vector_dim)
			.await
	}

	pub async fn store_text_blocks(
		&self,
		blocks: &[TextBlock],
		embeddings: &[Vec<f32>],
	) -> Result<()> {
		self.store_blocks(blocks, embeddings, self.text_vector_dim)
			.await
	}

	pub async fn store_document_blocks(
		&self,
		blocks: &[DocumentBlock],
		embeddings: &[Vec<f32>],
	) -> Result<()> {
		self.store_blocks(blocks, embeddings, self.text_vector_dim)
			.await
	}

	pub async fn store_commit_blocks(
		&self,
		blocks: &[CommitBlock],
		embeddings: &[Vec<f32>],
	) -> Result<()> {
		self.store_blocks(blocks, embeddings, self.text_vector_dim)
			.await
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
		self.get_code_blocks_with_language_filter(embedding, limit, distance_threshold, None)
			.await
	}

	pub async fn get_code_blocks_with_language_filter(
		&self,
		embedding: Vec<f32>,
		limit: Option<usize>,
		distance_threshold: Option<f32>,
		language_filter: Option<&str>,
	) -> Result<Vec<CodeBlock>> {
		self.get_blocks_with_config(
			embedding,
			limit,
			distance_threshold,
			language_filter,
			self.code_vector_dim,
		)
		.await
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
		self.get_blocks_with_config(
			embedding,
			limit,
			distance_threshold,
			None,
			self.text_vector_dim,
		)
		.await
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
		self.get_blocks_with_config(
			embedding,
			limit,
			distance_threshold,
			None,
			self.text_vector_dim,
		)
		.await
	}

	pub async fn get_commit_blocks_with_config(
		&self,
		embedding: Vec<f32>,
		limit: Option<usize>,
		distance_threshold: Option<f32>,
	) -> Result<Vec<CommitBlock>> {
		self.get_blocks_with_config(
			embedding,
			limit,
			distance_threshold,
			None,
			self.text_vector_dim,
		)
		.await
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
		// Clean up GraphRAG data for the file
		table_ops
			.remove_blocks_by_path(file_path, "graphrag_nodes")
			.await?;
		// Use specific GraphRAG operation for relationships (they don't have a 'path' field)
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops
			.remove_graph_relationships_by_path(file_path)
			.await?;
		// Also remove file metadata to prevent stale mtime from causing skip-on-reindex
		table_ops
			.remove_blocks_by_path(file_path, "file_metadata")
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

	/// Run LanceDB's full table optimization on all block + graphrag tables.
	///
	/// This is the maintenance routine LanceDB recommends after incremental writes:
	///   1. Compact small fragments into larger ones (fewer files, faster reads).
	///   2. Extend the vector & FTS indexes to cover newly-added rows
	///      (otherwise every batch insert lands in an unindexed tail that the
	///      query path must brute-force scan — search latency grows with the tail).
	///   3. Prune dataset versions older than 7 days (default retention).
	///
	/// Skips tables that don't exist. Logged at info level. Errors are logged
	/// and swallowed per-table so one bad table doesn't abort the whole sweep.
	pub async fn optimize_tables(&self) -> Result<()> {
		use lancedb::table::OptimizeAction;

		let candidates = [
			"code_blocks",
			"text_blocks",
			"document_blocks",
			"commit_blocks",
			"graphrag_nodes",
			"graphrag_relationships",
			"file_metadata",
		];

		let existing = self.db.table_names().execute().await?;
		for name in candidates {
			if !existing.contains(&name.to_string()) {
				continue;
			}
			let table = match self.db.open_table(name).execute().await {
				Ok(t) => t,
				Err(e) => {
					tracing::warn!("optimize: failed to open '{}': {}", name, e);
					continue;
				}
			};
			let start = std::time::Instant::now();
			match table.optimize(OptimizeAction::All).await {
				Ok(stats) => {
					tracing::info!(
						"optimize '{}': compaction={:?} prune={:?} in {:.2}s",
						name,
						stats.compaction.is_some(),
						stats.prune.is_some(),
						start.elapsed().as_secs_f64()
					);
					// Cached Table snapshot is stale after compaction — invalidate.
					self.table_cache.write().await.remove(name);
				}
				Err(e) => {
					tracing::warn!("optimize '{}' failed: {}", name, e);
				}
			}
		}
		Ok(())
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

	// ============================================================================
	// GENERIC BLOCK OPERATIONS - Eliminates duplication across block types
	// ============================================================================

	/// Generic method to store blocks with embeddings
	/// This replaces the duplicated store_code_blocks, store_text_blocks, store_document_blocks
	pub async fn store_blocks<B: BlockType>(
		&self,
		blocks: &[B],
		embeddings: &[Vec<f32>],
		vector_dim: usize,
	) -> Result<()> {
		let batch = B::to_batch(blocks, embeddings, vector_dim)?;
		let table_ops = TableOperations::new(&self.db);
		table_ops.store_batch(B::TABLE_NAME, batch).await?;

		// Vector index: only check/create if not cached as present. Avoids
		// open_table + list_indices on every batch flush.
		let needs_vector_check = {
			let cache = self.vector_index_present.read().await;
			!cache.get(B::TABLE_NAME).copied().unwrap_or(false)
		};

		if needs_vector_check {
			if let Ok(table) = self.db.open_table(B::TABLE_NAME).execute().await {
				let indices = table.list_indices().await?;
				let has_vector_index = indices.iter().any(|idx| idx.columns == vec!["embedding"]);

				if has_vector_index {
					self.vector_index_present
						.write()
						.await
						.insert(B::TABLE_NAME.to_string(), true);
				} else if let Err(e) = table_ops
					.create_vector_index_optimized(
						B::TABLE_NAME,
						"embedding",
						vector_dim,
						self.quantization,
					)
					.await
				{
					tracing::warn!("Failed to create optimized vector index: {}", e);
				} else {
					self.vector_index_present
						.write()
						.await
						.insert(B::TABLE_NAME.to_string(), true);
					// Invalidate cached Table handle so subsequent searches see the
					// fresh index. A stale dataset snapshot would otherwise miss it.
					self.table_cache.write().await.remove(B::TABLE_NAME);
				}
			}
		}

		// FTS index: only build when hybrid search is configured, and only check
		// when not cached as present.
		if self.hybrid_enabled {
			let needs_fts_check = {
				let cache = self.fts_index_present.read().await;
				!cache.get(B::TABLE_NAME).copied().unwrap_or(false)
			};
			if needs_fts_check {
				if let Err(e) = table_ops.create_fts_index(B::TABLE_NAME).await {
					tracing::warn!("Failed to create FTS index for '{}': {}", B::TABLE_NAME, e);
				} else {
					self.fts_index_present
						.write()
						.await
						.insert(B::TABLE_NAME.to_string(), true);
				}
			}
		}

		Ok(())
	}

	/// Generic method to retrieve blocks with optional filtering
	/// This replaces the duplicated get_*_blocks_with_config methods
	pub async fn get_blocks_with_config<B: BlockType>(
		&self,
		embedding: Vec<f32>,
		limit: Option<usize>,
		distance_threshold: Option<f32>,
		language_filter: Option<&str>,
		_vector_dim: usize,
	) -> Result<Vec<B>> {
		let table_ops = TableOperations::new(&self.db);
		if !table_ops.table_exists(B::TABLE_NAME).await? {
			return Ok(Vec::new());
		}

		let table = self.get_table(B::TABLE_NAME).await?;

		let mut query = table
			.vector_search(embedding)?
			.distance_type(DistanceType::Cosine) // Always use Cosine for consistency
			.limit(limit.unwrap_or(10));

		// Apply language filter if specified (only for code/text blocks)
		if let Some(language) = language_filter {
			query = query.only_if(format!("language = '{}'", language));
		}

		// Apply intelligent search optimization
		query = VectorOptimizer::optimize_query(query, &table, B::TABLE_NAME)
			.await
			.map_err(|e| anyhow::anyhow!("Failed to optimize query: {}", e))?;

		let mut results = query.execute().await?;
		let mut all_blocks = Vec::new();

		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				let mut blocks = B::from_batch(&batch)?;

				// Apply distance threshold if specified
				if let Some(distance_threshold_value) = distance_threshold {
					blocks.retain(|block| {
						block
							.distance()
							.is_none_or(|d| d <= distance_threshold_value)
					});
				}

				all_blocks.append(&mut blocks);
			}
		}

		// Sort results by distance (ascending - lower distance = higher similarity)
		all_blocks.sort_by(|a, b| match (a.distance(), b.distance()) {
			(Some(dist_a), Some(dist_b)) => dist_a
				.partial_cmp(&dist_b)
				.unwrap_or(std::cmp::Ordering::Equal),
			(Some(_), None) => std::cmp::Ordering::Less, // Results with distance come first
			(None, Some(_)) => std::cmp::Ordering::Greater,
			(None, None) => std::cmp::Ordering::Equal,
		});

		Ok(all_blocks)
	}

	pub async fn clear_docs_table(&self) -> Result<()> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.clear_table("document_blocks").await
	}

	pub async fn clear_text_table(&self) -> Result<()> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.clear_table("text_blocks").await
	}

	pub async fn clear_commits_table(&self) -> Result<()> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.clear_table("commit_blocks").await
	}

	pub async fn clear_commits_git_metadata(&self) -> Result<()> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.clear_table("commits_git_metadata").await
	}

	pub async fn clear_graphrag_git_metadata(&self) -> Result<()> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.clear_table("graphrag_git_metadata").await
	}

	pub fn get_code_vector_dim(&self) -> usize {
		self.code_vector_dim
	}

	/// Get row count for a table. Returns 0 if table doesn't exist.
	pub async fn get_table_row_count(&self, table_name: &str) -> Result<usize> {
		let table_ops = TableOperations::new(&self.db);
		if !table_ops.table_exists(table_name).await? {
			return Ok(0);
		}
		let table = self.db.open_table(table_name).execute().await?;
		Ok(table.count_rows(None).await?)
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

	pub async fn get_graphrag_last_commit_hash(&self) -> Result<Option<String>> {
		let metadata_ops = MetadataOperations::new(&self.db);
		metadata_ops.get_graphrag_last_commit_hash().await
	}

	pub async fn store_graphrag_commit_hash(&self, commit_hash: &str) -> Result<()> {
		let metadata_ops = MetadataOperations::new(&self.db);
		metadata_ops.store_graphrag_commit_hash(commit_hash).await
	}

	pub async fn get_commits_last_commit_hash(&self) -> Result<Option<String>> {
		let metadata_ops = MetadataOperations::new(&self.db);
		metadata_ops.get_commits_last_commit_hash().await
	}

	pub async fn store_commits_last_commit_hash(&self, commit_hash: &str) -> Result<()> {
		let metadata_ops = MetadataOperations::new(&self.db);
		metadata_ops
			.store_commits_last_commit_hash(commit_hash)
			.await
	}

	// GraphRAG operations
	pub async fn graphrag_needs_indexing(&self) -> Result<bool> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops.graphrag_needs_indexing().await
	}

	pub async fn get_all_code_blocks_for_graphrag(&self) -> Result<Vec<CodeBlock>> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops.get_all_code_blocks_for_graphrag().await
	}

	pub async fn store_graph_nodes(&self, node_batch: RecordBatch) -> Result<()> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops.store_graph_nodes(node_batch).await
	}

	pub async fn store_graph_relationships(&self, rel_batch: RecordBatch) -> Result<()> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops.store_graph_relationships(rel_batch).await
	}

	pub async fn clear_graph_nodes(&self) -> Result<()> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops.clear_graph_nodes().await
	}

	pub async fn clear_graph_relationships(&self) -> Result<()> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops.clear_graph_relationships().await
	}

	pub async fn remove_graph_nodes_by_path(&self, file_path: &str) -> Result<usize> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops.remove_graph_nodes_by_path(file_path).await
	}

	pub async fn remove_graph_relationships_by_path(&self, file_path: &str) -> Result<usize> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops
			.remove_graph_relationships_by_path(file_path)
			.await
	}

	pub async fn get_all_graph_nodes(&self) -> Result<RecordBatch> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops.get_all_graph_nodes().await
	}

	pub async fn search_graph_nodes(&self, embedding: &[f32], limit: usize) -> Result<RecordBatch> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops.search_graph_nodes(embedding, limit).await
	}

	pub async fn get_graph_relationships(&self) -> Result<RecordBatch> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops.get_graph_relationships().await
	}

	/// Get relationships for a specific node with direction filtering (NEW - Phase 1.2)
	pub async fn get_node_relationships(
		&self,
		node_id: &str,
		direction: crate::indexer::graphrag::types::RelationshipDirection,
	) -> Result<Vec<crate::indexer::graphrag::types::CodeRelationship>> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops
			.get_node_relationships(node_id, direction)
			.await
	}

	/// Get relationships filtered by type (NEW - Phase 1.2)
	pub async fn get_relationships_by_type(
		&self,
		relation_type: &crate::indexer::graphrag::types::RelationType,
	) -> Result<Vec<crate::indexer::graphrag::types::CodeRelationship>> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops.get_relationships_by_type(relation_type).await
	}

	/// Get all nodes with pagination (NEW - Phase 1.2)
	pub async fn get_all_nodes_paginated(
		&self,
		offset: usize,
		limit: usize,
	) -> Result<Vec<crate::indexer::graphrag::types::CodeNode>> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops.get_all_nodes_paginated(offset, limit).await
	}

	/// Get all relationships efficiently with streaming (NEW - Phase 1.2)
	pub async fn get_all_relationships_efficient(
		&self,
	) -> Result<Vec<crate::indexer::graphrag::types::CodeRelationship>> {
		let graphrag_ops = GraphRagOperations::new(
			&self.db,
			self.code_vector_dim,
			Arc::clone(&self.table_cache),
		);
		graphrag_ops.get_all_relationships_efficient().await
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

		let table = self.get_table("code_blocks").await?;
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

		let table = self.get_table("code_blocks").await?;
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
	// ===== Hybrid Search =====

	/// Perform hybrid search combining vector similarity and full-text search (BM25).
	///
	/// Uses LanceDB's native hybrid execution: both vector ANN and FTS run in parallel,
	/// results are normalized and fused via Reciprocal Rank Fusion (RRF) internally.
	/// Requires an FTS index on the `content` column (created by `ensure_fts_index`).
	///
	/// Falls back to vector-only search if no FTS index exists or keywords are absent.
	pub async fn hybrid_search<B: BlockType>(&self, query: &HybridSearchQuery) -> Result<Vec<B>> {
		query
			.validate()
			.map_err(|e| anyhow::anyhow!("Invalid hybrid query: {}", e))?;

		let table_ops = TableOperations::new(&self.db);
		if !table_ops.table_exists(B::TABLE_NAME).await? {
			return Ok(Vec::new());
		}

		let mut table = self.get_table(B::TABLE_NAME).await?;
		let distance_threshold = query.min_relevance.map(|sim| 1.0 - sim);
		let limit = query.limit;

		// Empty table → nothing to search. Without this guard, an FTS query against an
		// indexless empty table fails with "Cannot perform full text search unless an
		// INVERTED index has been created". `create_fts_index` also short-circuits on
		// zero rows, so we must avoid issuing the FTS query at all.
		if table.count_rows(None).await? == 0 {
			return Ok(Vec::new());
		}

		// When both signals are present, use LanceDB native hybrid (vector + FTS with RRF).
		// When only one signal is present, use that signal alone.
		match (&query.vector_query, &query.keywords) {
			(Some(embedding), Some(kw_query)) => {
				// Check FTS index, create if missing (lazy)
				let indices = table.list_indices().await?;
				let has_fts = indices
					.iter()
					.any(|idx| idx.index_type == lancedb::index::IndexType::FTS);

				if !has_fts {
					table_ops.create_fts_index(B::TABLE_NAME).await?;
					// Cached Table holds a stale dataset snapshot that doesn't see the
					// freshly created FTS index. Drop it so the next get_table reopens.
					self.table_cache.write().await.remove(B::TABLE_NAME);
					table = self.get_table(B::TABLE_NAME).await?;

					// Verify creation actually succeeded. It can no-op (e.g. empty table
					// raced to zero, or index build failed silently); issuing an FTS
					// query without the index raises an opaque Lance error. Fall back
					// to vector-only search when the index is still missing.
					let indices = table.list_indices().await?;
					let has_fts_now = indices
						.iter()
						.any(|idx| idx.index_type == lancedb::index::IndexType::FTS);
					if !has_fts_now {
						return self
							.get_blocks_with_config::<B>(
								embedding.clone(),
								Some(limit),
								distance_threshold,
								query.language_filter.as_deref(),
								0,
							)
							.await;
					}
				}

				// Native hybrid: LanceDB runs vector + FTS in parallel, fuses with RRF
				let mut vq = table
					.vector_search(embedding.clone())?
					.distance_type(DistanceType::Cosine)
					.limit(limit)
					.full_text_search(FullTextSearchQuery::new(kw_query.clone()));

				if let Some(lang) = query.language_filter.as_deref() {
					vq = vq.only_if(format!("language = '{}'", lang));
				}

				vq = VectorOptimizer::optimize_query(vq, &table, B::TABLE_NAME)
					.await
					.map_err(|e| anyhow::anyhow!("Failed to optimize query: {}", e))?;

				let mut stream = vq.execute().await?;
				let mut blocks = Vec::new();
				while let Some(batch) = stream.try_next().await? {
					if batch.num_rows() > 0 {
						let mut batch_blocks = B::from_batch(&batch)?;
						if let Some(thresh) = distance_threshold {
							batch_blocks.retain(|b| b.distance().is_none_or(|d| d <= thresh));
						}
						blocks.append(&mut batch_blocks);
					}
				}
				blocks.sort_by(|a, b| {
					a.distance()
						.partial_cmp(&b.distance())
						.unwrap_or(std::cmp::Ordering::Equal)
				});
				blocks.truncate(limit);
				Ok(blocks)
			}
			(Some(embedding), None) => {
				// Vector-only
				self.get_blocks_with_config::<B>(
					embedding.clone(),
					Some(limit),
					distance_threshold,
					query.language_filter.as_deref(),
					0,
				)
				.await
			}
			(None, Some(kw_query)) => {
				// FTS-only: plain query with full-text search
				let indices = table.list_indices().await?;
				let has_fts = indices
					.iter()
					.any(|idx| idx.index_type == lancedb::index::IndexType::FTS);

				if !has_fts {
					// Auto-create FTS index (same lazy pattern as the hybrid arm)
					table_ops.create_fts_index(B::TABLE_NAME).await?;
					// Reopen the cached handle so the new index becomes visible.
					self.table_cache.write().await.remove(B::TABLE_NAME);
					table = self.get_table(B::TABLE_NAME).await?;

					// If the index still isn't there (creation no-op), we have no
					// vector fallback in this arm — return empty results rather than
					// raising the cryptic Lance INVERTED-index error.
					let indices = table.list_indices().await?;
					let has_fts_now = indices
						.iter()
						.any(|idx| idx.index_type == lancedb::index::IndexType::FTS);
					if !has_fts_now {
						return Ok(Vec::new());
					}
				}

				let mut q = table
					.query()
					.full_text_search(FullTextSearchQuery::new(kw_query.clone()))
					.limit(limit);

				if let Some(lang) = query.language_filter.as_deref() {
					q = q.only_if(format!("language = '{}'", lang));
				}

				let mut stream = q.execute().await?;
				let mut blocks = Vec::new();
				while let Some(batch) = stream.try_next().await? {
					if batch.num_rows() > 0 {
						blocks.append(&mut B::from_batch(&batch)?);
					}
				}
				blocks.truncate(limit);
				Ok(blocks)
			}
			(None, None) => unreachable!("validate() ensures at least one signal"),
		}
	}

	/// Ensure an FTS index exists on the `content` column of the given table.
	/// Called after data is stored so the index covers all current rows.
	/// Safe to call repeatedly — skips creation if index already exists.
	pub async fn ensure_fts_index(&self, table_name: &str) -> Result<()> {
		let table_ops = TableOperations::new(&self.db);
		table_ops.create_fts_index(table_name).await
	}
}
