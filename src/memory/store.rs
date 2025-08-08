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
use chrono::Utc;
use std::sync::Arc;

// Arrow imports
use arrow::array::{Array, FixedSizeListArray, Float32Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;

// LanceDB imports
use futures::TryStreamExt;
use lancedb::{
	connect,
	index::Index,
	query::{ExecutableQuery, QueryBase},
	Connection, DistanceType,
};

use super::types::{Memory, MemoryConfig, MemoryQuery, MemoryRelationship, MemorySearchResult};

/// LanceDB-based storage for memories with vector search capabilities
pub struct MemoryStore {
	db: Connection,
	embedding_provider: Box<dyn crate::embedding::provider::EmbeddingProvider>,
	config: MemoryConfig,
	main_config: crate::config::Config,
	vector_dim: usize,
}

impl MemoryStore {
	/// Create a new memory store
	pub async fn new(
		db_path: &str,
		embedding_provider: Box<dyn crate::embedding::provider::EmbeddingProvider>,
		config: MemoryConfig,
		main_config: crate::config::Config,
	) -> Result<Self> {
		// Connect to LanceDB
		let db = connect(db_path).execute().await?;

		// Get vector dimension from the embedding provider by testing with a short text
		let test_embedding = embedding_provider.generate_embedding("test").await?;
		let vector_dim = test_embedding.len();

		let store = Self {
			db,
			embedding_provider,
			config,
			main_config,
			vector_dim,
		};

		// Initialize tables
		store.initialize_tables().await?;

		// Ensure optimal vector index (only during initialization, not on every store)
		store.ensure_optimal_index().await?;

		Ok(store)
	}

	/// Initialize memory and relationship tables
	async fn initialize_tables(&self) -> Result<()> {
		let table_names = self.db.table_names().execute().await?;

		// Create memories table if it doesn't exist
		if !table_names.contains(&"memories".to_string()) {
			let schema = Arc::new(Schema::new(vec![
				Field::new("id", DataType::Utf8, false),
				Field::new("memory_type", DataType::Utf8, false),
				Field::new("title", DataType::Utf8, false),
				Field::new("content", DataType::Utf8, false),
				Field::new("created_at", DataType::Utf8, false),
				Field::new("updated_at", DataType::Utf8, false),
				Field::new("importance", DataType::Float32, false),
				Field::new("confidence", DataType::Float32, false),
				Field::new("tags", DataType::Utf8, true), // JSON serialized
				Field::new("related_files", DataType::Utf8, true), // JSON serialized
				Field::new("git_commit", DataType::Utf8, true),
				Field::new(
					"embedding",
					DataType::FixedSizeList(
						Arc::new(Field::new("item", DataType::Float32, true)),
						self.vector_dim as i32,
					),
					true,
				),
			]));

			self.db
				.create_empty_table("memories", schema)
				.execute()
				.await?;
		}

		// Create relationships table if it doesn't exist
		if !table_names.contains(&"memory_relationships".to_string()) {
			let schema = Arc::new(Schema::new(vec![
				Field::new("id", DataType::Utf8, false),
				Field::new("source_id", DataType::Utf8, false),
				Field::new("target_id", DataType::Utf8, false),
				Field::new("relationship_type", DataType::Utf8, false),
				Field::new("strength", DataType::Float32, false),
				Field::new("description", DataType::Utf8, false),
				Field::new("created_at", DataType::Utf8, false),
			]));

			self.db
				.create_empty_table("memory_relationships", schema)
				.execute()
				.await?;
		}

		Ok(())
	}

	/// Store a memory
	pub async fn store_memory(&mut self, memory: &Memory) -> Result<()> {
		// Generate embedding using the optimized single embedding function for better performance
		let embedding = crate::embedding::generate_embeddings(
			&memory.get_searchable_text(),
			false,
			&self.main_config,
		)
		.await?;

		self.store_memory_with_embedding(memory, embedding).await
	}

	/// Store a memory with a pre-computed embedding (for batch operations)
	async fn store_memory_with_embedding(
		&mut self,
		memory: &Memory,
		embedding: Vec<f32>,
	) -> Result<()> {
		// Create record batch
		let schema = Arc::new(Schema::new(vec![
			Field::new("id", DataType::Utf8, false),
			Field::new("memory_type", DataType::Utf8, false),
			Field::new("title", DataType::Utf8, false),
			Field::new("content", DataType::Utf8, false),
			Field::new("created_at", DataType::Utf8, false),
			Field::new("updated_at", DataType::Utf8, false),
			Field::new("importance", DataType::Float32, false),
			Field::new("confidence", DataType::Float32, false),
			Field::new("tags", DataType::Utf8, true),
			Field::new("related_files", DataType::Utf8, true),
			Field::new("git_commit", DataType::Utf8, true),
			Field::new(
				"embedding",
				DataType::FixedSizeList(
					Arc::new(Field::new("item", DataType::Float32, true)),
					self.vector_dim as i32,
				),
				true,
			),
		]));

		// Prepare data
		let tags_json = serde_json::to_string(&memory.metadata.tags)?;
		let files_json = serde_json::to_string(&memory.metadata.related_files)?;

		// Create embedding array
		let embedding_values = Float32Array::from(embedding);
		let embedding_array = FixedSizeListArray::new(
			Arc::new(Field::new("item", DataType::Float32, true)),
			self.vector_dim as i32,
			Arc::new(embedding_values),
			None,
		);

		let batch = RecordBatch::try_new(
			schema.clone(),
			vec![
				Arc::new(StringArray::from(vec![memory.id.clone()])),
				Arc::new(StringArray::from(vec![memory.memory_type.to_string()])),
				Arc::new(StringArray::from(vec![memory.title.clone()])),
				Arc::new(StringArray::from(vec![memory.content.clone()])),
				Arc::new(StringArray::from(vec![memory.created_at.to_rfc3339()])),
				Arc::new(StringArray::from(vec![memory.updated_at.to_rfc3339()])),
				Arc::new(Float32Array::from(vec![memory.metadata.importance])),
				Arc::new(Float32Array::from(vec![memory.metadata.confidence])),
				Arc::new(StringArray::from(vec![tags_json])),
				Arc::new(StringArray::from(vec![files_json])),
				Arc::new(StringArray::from(vec![memory.metadata.git_commit.clone()])),
				Arc::new(embedding_array),
			],
		)?;

		// Open table and add the batch
		let table = self.db.open_table("memories").execute().await?;

		// Delete existing memory with same ID if it exists
		table.delete(&format!("id = '{}'", memory.id)).await.ok();

		// Add new memory
		use std::iter::once;
		let batches = once(Ok(batch));
		let batch_reader = arrow::record_batch::RecordBatchIterator::new(batches, schema);
		table.add(batch_reader).execute().await?;

		// Index management moved to separate method for performance

		Ok(())
	}

	/// Store multiple memories in batch with optimized embedding generation
	pub async fn store_memories(&mut self, memories: &[Memory]) -> Result<()> {
		if memories.is_empty() {
			return Ok(());
		}

		// Collect all searchable texts for batch embedding generation
		let texts: Vec<String> = memories
			.iter()
			.map(|memory| memory.get_searchable_text())
			.collect();

		// Generate ALL embeddings in ONE API request using the same high-level function as indexer
		// This includes token-aware batching and respects config limits
		let embeddings = crate::embedding::generate_embeddings_batch(
			texts,
			false,
			&self.main_config,
			crate::embedding::types::InputType::Query,
		)
		.await?;

		if embeddings.len() != memories.len() {
			return Err(anyhow::anyhow!(
				"Embedding count mismatch: expected {}, got {}",
				memories.len(),
				embeddings.len()
			));
		}

		// Store all memories with their pre-computed embeddings
		for (memory, embedding) in memories.iter().zip(embeddings.into_iter()) {
			self.store_memory_with_embedding(memory, embedding).await?;
		}

		Ok(())
	}

	/// Update an existing memory
	pub async fn update_memory(&mut self, memory: &Memory) -> Result<()> {
		// Just use store_memory as it handles updates by deleting and re-inserting
		self.store_memory(memory).await
	}

	/// Delete a memory by ID
	pub async fn delete_memory(&mut self, memory_id: &str) -> Result<()> {
		let table = self.db.open_table("memories").execute().await?;
		table.delete(&format!("id = '{}'", memory_id)).await?;

		// Also delete any relationships involving this memory
		let rel_table = self.db.open_table("memory_relationships").execute().await?;
		rel_table
			.delete(&format!(
				"source_id = '{}' OR target_id = '{}'",
				memory_id, memory_id
			))
			.await
			.ok();

		Ok(())
	}

	/// Ensure optimal vector index for memories table (call periodically, not on every store)
	pub async fn ensure_optimal_index(&self) -> Result<()> {
		let table = self.db.open_table("memories").execute().await?;

		// Get current dataset statistics
		let row_count = table.count_rows(None).await?;
		let has_index = table
			.list_indices()
			.await?
			.iter()
			.any(|idx| idx.columns == vec!["embedding"]);

		if !has_index {
			// Use intelligent optimizer to determine optimal index parameters
			let index_params =
				crate::store::vector_optimizer::VectorOptimizer::calculate_index_params(
					row_count,
					self.vector_dim,
				);

			if index_params.should_create_index {
				tracing::info!(
					"Creating optimized vector index for memories table: {} rows, {} partitions, {} sub-vectors",
					row_count, index_params.num_partitions, index_params.num_sub_vectors
				);

				table
					.create_index(
						&["embedding"],
						Index::IvfPq(
							lancedb::index::vector::IvfPqIndexBuilder::default()
								.distance_type(index_params.distance_type)
								.num_partitions(index_params.num_partitions)
								.num_sub_vectors(index_params.num_sub_vectors)
								.num_bits(index_params.num_bits as u32),
						),
					)
					.execute()
					.await?;
			} else {
				tracing::debug!(
					"Skipping index creation for memories table with {} rows - brute force will be faster",
					row_count
				);
			}
		} else {
			// Check if we should optimize existing index due to growth
			if crate::store::vector_optimizer::VectorOptimizer::should_optimize_for_growth(
				row_count,
				self.vector_dim,
				true,
			) {
				tracing::info!("Dataset growth detected, optimizing memories index");

				// Recreate index with optimal parameters
				let index_params =
					crate::store::vector_optimizer::VectorOptimizer::calculate_index_params(
						row_count,
						self.vector_dim,
					);

				if index_params.should_create_index {
					table
						.create_index(
							&["embedding"],
							Index::IvfPq(
								lancedb::index::vector::IvfPqIndexBuilder::default()
									.distance_type(index_params.distance_type)
									.num_partitions(index_params.num_partitions)
									.num_sub_vectors(index_params.num_sub_vectors)
									.num_bits(index_params.num_bits as u32),
							),
						)
						.execute()
						.await?;
				}
			}
		}

		Ok(())
	}

	/// Get a memory by ID
	pub async fn get_memory(&self, memory_id: &str) -> Result<Option<Memory>> {
		let table = self.db.open_table("memories").execute().await?;

		let mut results = table
			.query()
			.only_if(format!("id = '{}'", memory_id))
			.limit(1)
			.execute()
			.await?;

		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				let memories = self.batch_to_memories(&batch)?;
				return Ok(memories.into_iter().next());
			}
		}

		Ok(None)
	}

	/// Search memories using vector similarity and optional filters
	pub async fn search_memories(&self, query: &MemoryQuery) -> Result<Vec<MemorySearchResult>> {
		let table = self.db.open_table("memories").execute().await?;

		let limit = query
			.limit
			.unwrap_or(self.config.max_search_results)
			.min(self.config.max_search_results);
		let min_relevance = query.min_relevance.unwrap_or(0.0);

		let mut results = Vec::new();

		// If we have a text query, use semantic search
		if let Some(ref query_text) = query.query_text {
			let query_embedding = self
				.embedding_provider
				.generate_embedding(query_text)
				.await?;

			// Start with optimized vector search
			let mut db_query = table
				.vector_search(query_embedding.as_slice())?
				.distance_type(DistanceType::Cosine)
				.limit(limit * 2); // Get more results to filter

			// Apply intelligent search optimization
			db_query = crate::store::vector_optimizer::VectorOptimizer::optimize_query(
				db_query, &table, "memories",
			)
			.await
			.map_err(|e| anyhow::anyhow!("Failed to optimize query: {}", e))?;

			let mut db_results = db_query.execute().await?;

			while let Some(batch) = db_results.try_next().await? {
				if batch.num_rows() == 0 {
					continue;
				}

				// Extract distance column
				let distance_array = batch
					.column_by_name("_distance")
					.and_then(|col| col.as_any().downcast_ref::<Float32Array>())
					.map(|arr| (0..arr.len()).map(|i| arr.value(i)).collect::<Vec<f32>>())
					.unwrap_or_default();

				let memories = self.batch_to_memories(&batch)?;

				for (memory, distance) in memories.into_iter().zip(distance_array.into_iter()) {
					// Apply filters
					if !self.matches_filters(&memory, query) {
						continue;
					}

					// Convert distance to similarity (cosine distance is 1 - similarity)
					let similarity = 1.0 - distance;
					if similarity >= min_relevance {
						results.push(MemorySearchResult {
							memory,
							relevance_score: similarity,
							selection_reason: self.generate_selection_reason(query, similarity),
						});
					}
				}
			}
		} else {
			// No text query, just apply filters
			let mut db_results = table.query().execute().await?;

			while let Some(batch) = db_results.try_next().await? {
				if batch.num_rows() == 0 {
					continue;
				}

				let memories = self.batch_to_memories(&batch)?;

				for memory in memories {
					if self.matches_filters(&memory, query) {
						let relevance_score = memory.metadata.importance;
						if relevance_score >= min_relevance {
							results.push(MemorySearchResult {
								memory,
								relevance_score,
								selection_reason: self
									.generate_selection_reason(query, relevance_score),
							});
						}
					}
				}
			}
		}

		// Apply sorting based on query parameters
		if let Some(sort_by) = &query.sort_by {
			let sort_order = query
				.sort_order
				.as_ref()
				.unwrap_or(&super::types::SortOrder::Descending);

			results.sort_by(|a, b| {
				let ordering = match sort_by {
					super::types::MemorySortBy::CreatedAt => {
						a.memory.created_at.cmp(&b.memory.created_at)
					}
					super::types::MemorySortBy::UpdatedAt => {
						a.memory.updated_at.cmp(&b.memory.updated_at)
					}
					super::types::MemorySortBy::Importance => a
						.memory
						.metadata
						.importance
						.partial_cmp(&b.memory.metadata.importance)
						.unwrap_or(std::cmp::Ordering::Equal),
					super::types::MemorySortBy::Confidence => a
						.memory
						.metadata
						.confidence
						.partial_cmp(&b.memory.metadata.confidence)
						.unwrap_or(std::cmp::Ordering::Equal),
					super::types::MemorySortBy::Relevance => a
						.relevance_score
						.partial_cmp(&b.relevance_score)
						.unwrap_or(std::cmp::Ordering::Equal),
				};

				match sort_order {
					super::types::SortOrder::Ascending => ordering,
					super::types::SortOrder::Descending => ordering.reverse(),
				}
			});
		} else {
			// Default: Sort by relevance score (highest first)
			results.sort_by(|a, b| {
				b.relevance_score
					.partial_cmp(&a.relevance_score)
					.unwrap_or(std::cmp::Ordering::Equal)
			});
		}

		// Apply final limit
		results.truncate(limit);

		Ok(results)
	}

	/// Get all memories (paginated)
	pub async fn get_all_memories(&self, offset: usize, limit: usize) -> Result<Vec<Memory>> {
		let table = self.db.open_table("memories").execute().await?;

		let mut results = table
			.query()
			.limit(offset + limit) // LanceDB doesn't have native offset, so we limit and skip
			.execute()
			.await?;

		let mut all_memories = Vec::new();

		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() == 0 {
				continue;
			}

			let mut batch_memories = self.batch_to_memories(&batch)?;
			all_memories.append(&mut batch_memories);
		}

		// Sort by creation date (most recent first)
		all_memories.sort_by(|a, b| b.created_at.cmp(&a.created_at));

		// Apply pagination manually
		let start = offset.min(all_memories.len());
		let end = (offset + limit).min(all_memories.len());

		Ok(all_memories[start..end].to_vec())
	}

	/// Store a memory relationship
	pub async fn store_relationship(&mut self, relationship: &MemoryRelationship) -> Result<()> {
		let table = self.db.open_table("memory_relationships").execute().await?;

		let schema = Arc::new(Schema::new(vec![
			Field::new("id", DataType::Utf8, false),
			Field::new("source_id", DataType::Utf8, false),
			Field::new("target_id", DataType::Utf8, false),
			Field::new("relationship_type", DataType::Utf8, false),
			Field::new("strength", DataType::Float32, false),
			Field::new("description", DataType::Utf8, false),
			Field::new("created_at", DataType::Utf8, false),
		]));

		let batch = RecordBatch::try_new(
			schema.clone(),
			vec![
				Arc::new(StringArray::from(vec![relationship.id.clone()])),
				Arc::new(StringArray::from(vec![relationship.source_id.clone()])),
				Arc::new(StringArray::from(vec![relationship.target_id.clone()])),
				Arc::new(StringArray::from(vec![relationship
					.relationship_type
					.to_string()])),
				Arc::new(Float32Array::from(vec![relationship.strength])),
				Arc::new(StringArray::from(vec![relationship.description.clone()])),
				Arc::new(StringArray::from(vec![relationship
					.created_at
					.to_rfc3339()])),
			],
		)?;

		// Delete existing relationship with same ID if it exists
		table
			.delete(&format!("id = '{}'", relationship.id))
			.await
			.ok();

		// Add new relationship
		use std::iter::once;
		let batches = once(Ok(batch));
		let batch_reader = arrow::record_batch::RecordBatchIterator::new(batches, schema);
		table.add(batch_reader).execute().await?;

		Ok(())
	}

	/// Get relationships for a memory
	pub async fn get_memory_relationships(
		&self,
		memory_id: &str,
	) -> Result<Vec<MemoryRelationship>> {
		let table = self.db.open_table("memory_relationships").execute().await?;

		let mut results = table
			.query()
			.only_if(format!(
				"source_id = '{}' OR target_id = '{}'",
				memory_id, memory_id
			))
			.execute()
			.await?;

		let mut relationships = Vec::new();

		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() == 0 {
				continue;
			}

			let mut batch_relationships = self.batch_to_relationships(&batch)?;
			relationships.append(&mut batch_relationships);
		}

		Ok(relationships)
	}

	/// Get total count of memories
	pub async fn get_memory_count(&self) -> Result<usize> {
		let table = self.db.open_table("memories").execute().await?;
		Ok(table.count_rows(None).await?)
	}

	/// Clean up old memories based on configuration
	pub async fn cleanup_old_memories(&mut self) -> Result<usize> {
		if let Some(cleanup_days) = self.config.auto_cleanup_days {
			let cutoff_date = Utc::now() - chrono::Duration::days(cleanup_days as i64);
			let cutoff_str = cutoff_date.to_rfc3339();

			let table = self.db.open_table("memories").execute().await?;

			// Count memories to be deleted
			let mut count_results = table
				.query()
				.only_if(format!(
					"created_at < '{}' AND importance < {}",
					cutoff_str, self.config.cleanup_min_importance
				))
				.execute()
				.await?;

			let mut count = 0;
			while let Some(batch) = count_results.try_next().await? {
				count += batch.num_rows();
			}

			// Delete old memories
			table
				.delete(&format!(
					"created_at < '{}' AND importance < {}",
					cutoff_str, self.config.cleanup_min_importance
				))
				.await?;

			Ok(count)
		} else {
			Ok(0)
		}
	}

	/// Convert RecordBatch to Vec<Memory>
	fn batch_to_memories(&self, batch: &RecordBatch) -> Result<Vec<Memory>> {
		use chrono::DateTime;

		let num_rows = batch.num_rows();
		let mut memories = Vec::with_capacity(num_rows);

		// Extract all columns
		let id_array = batch
			.column_by_name("id")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("id column not found or wrong type"))?;

		let memory_type_array = batch
			.column_by_name("memory_type")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("memory_type column not found or wrong type"))?;

		let title_array = batch
			.column_by_name("title")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("title column not found or wrong type"))?;

		let content_array = batch
			.column_by_name("content")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("content column not found or wrong type"))?;

		let created_at_array = batch
			.column_by_name("created_at")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("created_at column not found or wrong type"))?;

		let updated_at_array = batch
			.column_by_name("updated_at")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("updated_at column not found or wrong type"))?;

		let importance_array = batch
			.column_by_name("importance")
			.and_then(|col| col.as_any().downcast_ref::<Float32Array>())
			.ok_or_else(|| anyhow::anyhow!("importance column not found or wrong type"))?;

		let confidence_array = batch
			.column_by_name("confidence")
			.and_then(|col| col.as_any().downcast_ref::<Float32Array>())
			.ok_or_else(|| anyhow::anyhow!("confidence column not found or wrong type"))?;

		let tags_array = batch
			.column_by_name("tags")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("tags column not found or wrong type"))?;

		let files_array = batch
			.column_by_name("related_files")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("related_files column not found or wrong type"))?;

		let git_array = batch
			.column_by_name("git_commit")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("git_commit column not found or wrong type"))?;

		for i in 0..num_rows {
			let memory_type =
				super::types::MemoryType::from(memory_type_array.value(i).to_string());

			let tags: Vec<String> = if tags_array.is_null(i) {
				Vec::new()
			} else {
				serde_json::from_str(tags_array.value(i)).unwrap_or_default()
			};

			let related_files: Vec<String> = if files_array.is_null(i) {
				Vec::new()
			} else {
				serde_json::from_str(files_array.value(i)).unwrap_or_default()
			};

			let git_commit = if git_array.is_null(i) {
				None
			} else {
				Some(git_array.value(i).to_string())
			};

			let metadata = super::types::MemoryMetadata {
				git_commit,
				importance: importance_array.value(i),
				confidence: confidence_array.value(i),
				tags,
				related_files,
				..Default::default()
			};

			let memory = Memory {
				id: id_array.value(i).to_string(),
				memory_type,
				title: title_array.value(i).to_string(),
				content: content_array.value(i).to_string(),
				created_at: DateTime::parse_from_rfc3339(created_at_array.value(i))?
					.with_timezone(&Utc),
				updated_at: DateTime::parse_from_rfc3339(updated_at_array.value(i))?
					.with_timezone(&Utc),
				metadata,
				relevance_score: None,
			};

			memories.push(memory);
		}

		Ok(memories)
	}

	/// Convert RecordBatch to Vec<MemoryRelationship>
	fn batch_to_relationships(&self, batch: &RecordBatch) -> Result<Vec<MemoryRelationship>> {
		use chrono::DateTime;

		let num_rows = batch.num_rows();
		let mut relationships = Vec::with_capacity(num_rows);

		// Extract all columns
		let id_array = batch
			.column_by_name("id")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("id column not found or wrong type"))?;

		let source_array = batch
			.column_by_name("source_id")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("source_id column not found or wrong type"))?;

		let target_array = batch
			.column_by_name("target_id")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("target_id column not found or wrong type"))?;

		let type_array = batch
			.column_by_name("relationship_type")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("relationship_type column not found or wrong type"))?;

		let strength_array = batch
			.column_by_name("strength")
			.and_then(|col| col.as_any().downcast_ref::<Float32Array>())
			.ok_or_else(|| anyhow::anyhow!("strength column not found or wrong type"))?;

		let desc_array = batch
			.column_by_name("description")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("description column not found or wrong type"))?;

		let created_array = batch
			.column_by_name("created_at")
			.and_then(|col| col.as_any().downcast_ref::<StringArray>())
			.ok_or_else(|| anyhow::anyhow!("created_at column not found or wrong type"))?;

		for i in 0..num_rows {
			let relationship_type = match type_array.value(i) {
				"RelatedTo" => super::types::RelationshipType::RelatedTo,
				"DependsOn" => super::types::RelationshipType::DependsOn,
				"Supersedes" => super::types::RelationshipType::Supersedes,
				"Similar" => super::types::RelationshipType::Similar,
				"Conflicts" => super::types::RelationshipType::Conflicts,
				"Implements" => super::types::RelationshipType::Implements,
				"Extends" => super::types::RelationshipType::Extends,
				other => super::types::RelationshipType::Custom(other.to_string()),
			};

			let relationship = MemoryRelationship {
				id: id_array.value(i).to_string(),
				source_id: source_array.value(i).to_string(),
				target_id: target_array.value(i).to_string(),
				relationship_type,
				strength: strength_array.value(i),
				description: desc_array.value(i).to_string(),
				created_at: DateTime::parse_from_rfc3339(created_array.value(i))?
					.with_timezone(&Utc),
			};

			relationships.push(relationship);
		}

		Ok(relationships)
	}

	/// Check if memory matches the query filters
	fn matches_filters(&self, memory: &Memory, query: &MemoryQuery) -> bool {
		// Filter by memory types
		if let Some(ref memory_types) = query.memory_types {
			if !memory_types.contains(&memory.memory_type) {
				return false;
			}
		}

		// Filter by tags (any of these tags)
		if let Some(ref tags) = query.tags {
			if !tags.iter().any(|tag| memory.metadata.tags.contains(tag)) {
				return false;
			}
		}

		// Filter by related files
		if let Some(ref files) = query.related_files {
			if !files
				.iter()
				.any(|file| memory.metadata.related_files.contains(file))
			{
				return false;
			}
		}

		// Filter by git commit
		if let Some(ref git_commit) = query.git_commit {
			if memory.metadata.git_commit.as_ref() != Some(git_commit) {
				return false;
			}
		}

		// Filter by minimum importance
		if let Some(min_importance) = query.min_importance {
			if memory.metadata.importance < min_importance {
				return false;
			}
		}

		// Filter by minimum confidence
		if let Some(min_confidence) = query.min_confidence {
			if memory.metadata.confidence < min_confidence {
				return false;
			}
		}

		// Filter by creation date range
		if let Some(created_after) = query.created_after {
			if memory.created_at < created_after {
				return false;
			}
		}

		if let Some(created_before) = query.created_before {
			if memory.created_at > created_before {
				return false;
			}
		}

		true
	}

	/// Clear all memory data (memories and relationships)
	pub async fn clear_all_memory_data(&mut self) -> Result<usize> {
		// Get current counts before deletion
		let memory_count = self.get_memory_count().await.unwrap_or(0);

		// Count relationships
		let rel_table = self.db.open_table("memory_relationships").execute().await?;
		let relationship_count = rel_table.count_rows(None).await.unwrap_or(0);

		let total_deleted = memory_count + relationship_count;

		// Drop and recreate memories table
		if self
			.db
			.table_names()
			.execute()
			.await?
			.contains(&"memories".to_string())
		{
			self.db.drop_table("memories").await?;
		}

		// Drop and recreate relationships table
		if self
			.db
			.table_names()
			.execute()
			.await?
			.contains(&"memory_relationships".to_string())
		{
			self.db.drop_table("memory_relationships").await?;
		}

		// Recreate tables
		self.initialize_tables().await?;

		Ok(total_deleted)
	}

	/// Generate selection reason for search results
	fn generate_selection_reason(&self, query: &MemoryQuery, relevance_score: f32) -> String {
		let mut reasons = Vec::new();

		if query.query_text.is_some() {
			reasons.push(format!("Semantic similarity: {:.2}", relevance_score));
		}

		if query.memory_types.is_some() {
			reasons.push("Matches memory type filter".to_string());
		}

		if query.tags.is_some() {
			reasons.push("Contains matching tags".to_string());
		}

		if query.related_files.is_some() {
			reasons.push("Related to specified files".to_string());
		}

		if query.git_commit.is_some() {
			reasons.push("Matches Git commit filter".to_string());
		}

		if reasons.is_empty() {
			"Matches search criteria".to_string()
		} else {
			reasons.join(", ")
		}
	}
}
