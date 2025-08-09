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

use super::git_utils::GitUtils;
use super::store::MemoryStore;
use super::types::{
	Memory, MemoryConfig, MemoryMetadata, MemoryQuery, MemoryRelationship, MemorySearchResult,
	MemoryType, RelationshipType,
};
use crate::config::Config;
use crate::embedding::{create_embedding_provider_from_parts, parse_provider_model};

/// High-level memory management interface
pub struct MemoryManager {
	store: MemoryStore,
	config: MemoryConfig,
}

impl MemoryManager {
	/// Create a new memory manager
	pub async fn new(config: &Config) -> Result<Self> {
		let memory_config = MemoryConfig::default();

		// Use the same storage system as the main application
		let current_dir = std::env::current_dir()?;
		let db_path = crate::storage::get_project_database_path(&current_dir)?;
		let db_path_str = db_path.to_string_lossy().to_string();

		// Create embedding provider using text model from config
		let model_string = &config.embedding.text_model;
		let (provider, model) = parse_provider_model(model_string);
		let embedding_provider = create_embedding_provider_from_parts(&provider, &model).await?;

		let store = MemoryStore::new(
			&db_path_str,
			embedding_provider,
			memory_config.clone(),
			config.clone(),
		)
		.await?;

		Ok(Self {
			store,
			config: memory_config,
		})
	}

	/// Create a new memory manager with custom config
	pub async fn with_config(config: &Config, memory_config: MemoryConfig) -> Result<Self> {
		// Use the same storage system as the main application
		let current_dir = std::env::current_dir()?;
		let db_path = crate::storage::get_project_database_path(&current_dir)?;
		let db_path_str = db_path.to_string_lossy().to_string();

		// Create embedding provider using text model from config
		let model_string = &config.embedding.text_model;
		let (provider, model) = parse_provider_model(model_string);
		let embedding_provider = create_embedding_provider_from_parts(&provider, &model).await?;

		let store = MemoryStore::new(
			&db_path_str,
			embedding_provider,
			memory_config.clone(),
			config.clone(),
		)
		.await?;

		Ok(Self {
			store,
			config: memory_config,
		})
	}

	/// Memorize new information with automatic Git context
	pub async fn memorize(
		&mut self,
		memory_type: MemoryType,
		title: String,
		content: String,
		importance: Option<f32>,
		tags: Option<Vec<String>>,
		related_files: Option<Vec<String>>,
	) -> Result<Memory> {
		// Initialize metadata with all values at once to satisfy clippy
		let mut metadata = MemoryMetadata {
			git_commit: GitUtils::get_current_commit(),
			importance: importance.unwrap_or(self.config.default_importance),
			tags: tags.unwrap_or_default(),
			related_files: Vec::new(), // Will be set below
			..Default::default()
		};

		// Add related files (convert to relative paths if possible)
		if let Some(files) = related_files {
			metadata.related_files = files
				.into_iter()
				.map(|file| GitUtils::get_relative_path(&file).unwrap_or(file))
				.collect();
		}

		// Auto-detect related files from Git changes if none provided
		if metadata.related_files.is_empty() {
			if let Ok(modified_files) = GitUtils::get_modified_files() {
				metadata.related_files = modified_files.into_iter().take(5).collect();
				// Limit to 5 files
			}
		}

		let memory = Memory::new(memory_type, title, content, Some(metadata));

		// Store the memory
		self.store.store_memory(&memory).await?;

		// Auto-create relationships if enabled
		if self.config.auto_relationships {
			self.create_automatic_relationships(&memory).await?;
		}

		Ok(memory)
	}

	/// Remember (search) memories based on query
	pub async fn remember(
		&self,
		query: &str,
		filters: Option<MemoryQuery>,
	) -> Result<Vec<MemorySearchResult>> {
		let mut search_query = filters.unwrap_or_default();
		search_query.query_text = Some(query.to_string());

		self.store.search_memories(&search_query).await
	}

	/// Remember (search) memories based on multiple queries with relevance-based merging
	pub async fn remember_multi(
		&self,
		queries: &[String],
		filters: Option<MemoryQuery>,
	) -> Result<Vec<MemorySearchResult>> {
		if queries.is_empty() {
			return Ok(Vec::new());
		}

		if queries.len() == 1 {
			// Single query - use existing method
			return self.remember(&queries[0], filters).await;
		}

		// Multiple queries - search each and merge results by relevance
		let base_filters = filters.unwrap_or_default();
		let mut all_results: std::collections::HashMap<String, MemorySearchResult> =
			std::collections::HashMap::new();
		let mut query_count: std::collections::HashMap<String, usize> =
			std::collections::HashMap::new();

		// Search with each query
		for query in queries {
			let mut search_query = base_filters.clone();
			search_query.query_text = Some(query.clone());

			let results = self.store.search_memories(&search_query).await?;

			for result in results {
				let memory_id = result.memory.id.clone();

				// Track how many queries matched this memory
				*query_count.entry(memory_id.clone()).or_insert(0) += 1;

				// Keep the result with highest relevance score
				match all_results.get(&memory_id) {
					Some(existing) if existing.relevance_score >= result.relevance_score => {
						// Keep existing with higher score
					}
					_ => {
						// Use this result (higher score or first occurrence)
						all_results.insert(memory_id, result);
					}
				}
			}
		}

		// Convert to vector and boost scores for memories that matched multiple queries
		let mut final_results: Vec<MemorySearchResult> = all_results
			.into_iter()
			.map(|(memory_id, mut result)| {
				let matches = query_count.get(&memory_id).unwrap_or(&1);

				// Boost relevance score for memories matching multiple queries
				if *matches > 1 {
					let boost_factor = 1.0 + ((*matches as f32 - 1.0) * 0.1); // 10% boost per additional match
					result.relevance_score = (result.relevance_score * boost_factor).min(1.0);

					// Update selection reason to indicate multi-query match
					result.selection_reason = format!(
						"Matched {} of {} queries: {}",
						matches,
						queries.len(),
						result.selection_reason
					);
				}

				result
			})
			.collect();

		// Sort by relevance score (highest first)
		final_results.sort_by(|a, b| {
			b.relevance_score
				.partial_cmp(&a.relevance_score)
				.unwrap_or(std::cmp::Ordering::Equal)
		});

		// Apply limit if specified in filters
		if let Some(limit) = base_filters.limit {
			final_results.truncate(limit);
		}

		Ok(final_results)
	}

	/// Remember memories with advanced filtering
	pub async fn remember_advanced(&self, query: MemoryQuery) -> Result<Vec<MemorySearchResult>> {
		self.store.search_memories(&query).await
	}

	/// Forget (delete) a memory by ID
	pub async fn forget(&mut self, memory_id: &str) -> Result<()> {
		self.store.delete_memory(memory_id).await
	}

	/// Forget memories matching criteria
	pub async fn forget_matching(&mut self, query: MemoryQuery) -> Result<usize> {
		let search_results = self.store.search_memories(&query).await?;
		let mut deleted_count = 0;

		for result in search_results {
			self.store.delete_memory(&result.memory.id).await?;
			deleted_count += 1;
		}

		Ok(deleted_count)
	}

	/// Update an existing memory
	pub async fn update_memory(
		&mut self,
		memory_id: &str,
		title: Option<String>,
		content: Option<String>,
		metadata_updates: Option<MemoryMetadata>,
	) -> Result<Option<Memory>> {
		if let Some(mut memory) = self.store.get_memory(memory_id).await? {
			// Update Git commit to current
			let current_commit = GitUtils::get_current_commit();
			if let Some(mut meta) = metadata_updates {
				meta.git_commit = current_commit.clone();
				memory.update(title, content, Some(meta));
			} else if let Some(commit) = current_commit {
				memory.metadata.git_commit = Some(commit);
				memory.update(title, content, None);
			} else {
				memory.update(title, content, None);
			}

			self.store.update_memory(&memory).await?;

			// Update relationships if auto-relationships is enabled
			if self.config.auto_relationships {
				self.update_automatic_relationships(&memory).await?;
			}

			Ok(Some(memory))
		} else {
			Ok(None)
		}
	}

	/// Get memory by ID
	pub async fn get_memory(&self, memory_id: &str) -> Result<Option<Memory>> {
		self.store.get_memory(memory_id).await
	}

	/// Get recent memories
	pub async fn get_recent_memories(&self, limit: usize) -> Result<Vec<Memory>> {
		let query = MemoryQuery {
			limit: Some(limit),
			sort_by: Some(super::types::MemorySortBy::CreatedAt),
			sort_order: Some(super::types::SortOrder::Descending),
			..Default::default()
		};

		let results = self.store.search_memories(&query).await?;
		Ok(results.into_iter().map(|r| r.memory).collect())
	}

	/// Get memories by type
	pub async fn get_memories_by_type(
		&self,
		memory_type: MemoryType,
		limit: Option<usize>,
	) -> Result<Vec<Memory>> {
		let query = MemoryQuery {
			memory_types: Some(vec![memory_type]),
			limit,
			sort_by: Some(super::types::MemorySortBy::CreatedAt),
			sort_order: Some(super::types::SortOrder::Descending),
			..Default::default()
		};

		let results = self.store.search_memories(&query).await?;
		Ok(results.into_iter().map(|r| r.memory).collect())
	}

	/// Get memories related to files
	pub async fn get_memories_for_files(
		&self,
		file_paths: Vec<String>,
	) -> Result<Vec<MemorySearchResult>> {
		// Convert to relative paths
		let relative_paths: Vec<String> = file_paths
			.into_iter()
			.map(|path| GitUtils::get_relative_path(&path).unwrap_or(path))
			.collect();

		let query = MemoryQuery {
			related_files: Some(relative_paths),
			sort_by: Some(super::types::MemorySortBy::Importance),
			sort_order: Some(super::types::SortOrder::Descending),
			..Default::default()
		};

		self.store.search_memories(&query).await
	}

	/// Get memories for current Git commit
	pub async fn get_memories_for_current_commit(&self) -> Result<Vec<Memory>> {
		if let Some(commit) = GitUtils::get_current_commit() {
			let query = MemoryQuery {
				git_commit: Some(commit),
				sort_by: Some(super::types::MemorySortBy::CreatedAt),
				sort_order: Some(super::types::SortOrder::Descending),
				..Default::default()
			};

			let results = self.store.search_memories(&query).await?;
			Ok(results.into_iter().map(|r| r.memory).collect())
		} else {
			Ok(Vec::new())
		}
	}

	/// Get memories with tags
	pub async fn get_memories_by_tags(&self, tags: Vec<String>) -> Result<Vec<MemorySearchResult>> {
		let query = MemoryQuery {
			tags: Some(tags),
			sort_by: Some(super::types::MemorySortBy::Importance),
			sort_order: Some(super::types::SortOrder::Descending),
			..Default::default()
		};

		self.store.search_memories(&query).await
	}

	/// Get memory statistics
	pub async fn get_memory_stats(&self) -> Result<MemoryStats> {
		let total_count = self.store.get_memory_count().await?;

		// Get count by type (simplified - would need custom queries for exact counts)
		let recent_memories = self.get_recent_memories(100).await?;
		let mut type_counts = std::collections::HashMap::new();

		for memory in &recent_memories {
			*type_counts
				.entry(memory.memory_type.to_string())
				.or_insert(0) += 1;
		}

		Ok(MemoryStats {
			total_memories: total_count,
			type_counts,
			recent_count: recent_memories.len().min(10),
			git_commit: GitUtils::get_current_commit(),
		})
	}

	/// Create a relationship between two memories
	pub async fn create_relationship(
		&mut self,
		source_id: String,
		target_id: String,
		relationship_type: RelationshipType,
		strength: f32,
		description: String,
	) -> Result<MemoryRelationship> {
		let relationship = MemoryRelationship {
			id: uuid::Uuid::new_v4().to_string(),
			source_id,
			target_id,
			relationship_type,
			strength,
			description,
			created_at: Utc::now(),
		};

		self.store.store_relationship(&relationship).await?;
		Ok(relationship)
	}

	/// Get relationships for a memory
	pub async fn get_relationships(&self, memory_id: &str) -> Result<Vec<MemoryRelationship>> {
		self.store.get_memory_relationships(memory_id).await
	}

	/// Get related memories through relationships
	pub async fn get_related_memories(&self, memory_id: &str) -> Result<Vec<Memory>> {
		let relationships = self.get_relationships(memory_id).await?;
		let mut related_memories = Vec::new();

		for rel in relationships {
			let related_id = if rel.source_id == memory_id {
				rel.target_id
			} else {
				rel.source_id
			};

			if let Some(memory) = self.store.get_memory(&related_id).await? {
				related_memories.push(memory);
			}
		}

		Ok(related_memories)
	}

	/// Clean up old memories
	pub async fn cleanup(&mut self) -> Result<usize> {
		self.store.cleanup_old_memories().await
	}

	/// Clear all memory data (DANGEROUS: deletes all memories and relationships)
	pub async fn clear_all(&mut self) -> Result<usize> {
		self.store.clear_all_memory_data().await
	}

	/// Auto-create relationships for a new memory
	async fn create_automatic_relationships(&mut self, memory: &Memory) -> Result<()> {
		// Find similar memories based on content similarity
		let similar_query = MemoryQuery {
			query_text: Some(memory.get_searchable_text()),
			memory_types: Some(vec![memory.memory_type.clone()]),
			limit: Some(5),
			min_relevance: Some(self.config.relationship_threshold),
			..Default::default()
		};

		let similar_memories = self.store.search_memories(&similar_query).await?;

		for result in similar_memories {
			if result.memory.id != memory.id
				&& result.relevance_score >= self.config.relationship_threshold
			{
				let relationship_type = if result.relevance_score > 0.9 {
					RelationshipType::Similar
				} else {
					RelationshipType::RelatedTo
				};

				let _ = self
					.create_relationship(
						memory.id.clone(),
						result.memory.id,
						relationship_type,
						result.relevance_score,
						format!(
							"Auto-detected relationship (similarity: {:.2})",
							result.relevance_score
						),
					)
					.await;
			}
		}

		// Create file-based relationships
		if !memory.metadata.related_files.is_empty() {
			let file_query = MemoryQuery {
				related_files: Some(memory.metadata.related_files.clone()),
				limit: Some(10),
				..Default::default()
			};

			let file_related = self.store.search_memories(&file_query).await?;
			for result in file_related {
				if result.memory.id != memory.id {
					let _ = self
						.create_relationship(
							memory.id.clone(),
							result.memory.id,
							RelationshipType::RelatedTo,
							0.7, // File relationship strength
							"Shares related files".to_string(),
						)
						.await;
				}
			}
		}

		Ok(())
	}

	/// Update automatic relationships for an updated memory
	async fn update_automatic_relationships(&mut self, memory: &Memory) -> Result<()> {
		// Remove existing auto-generated relationships
		let existing_relationships = self.get_relationships(&memory.id).await?;
		for rel in existing_relationships {
			if rel.description.contains("Auto-detected")
				|| rel.description.contains("Shares related files")
			{
				// Delete relationship - would need a delete method in store
				// For now, we'll skip deletion and just create new ones
			}
		}

		// Recreate relationships
		self.create_automatic_relationships(memory).await
	}

	/// Add tag to memory
	pub async fn add_tag(&mut self, memory_id: &str, tag: String) -> Result<bool> {
		if let Some(mut memory) = self.store.get_memory(memory_id).await? {
			memory.add_tag(tag);
			self.store.update_memory(&memory).await?;
			Ok(true)
		} else {
			Ok(false)
		}
	}

	/// Remove tag from memory
	pub async fn remove_tag(&mut self, memory_id: &str, tag: &str) -> Result<bool> {
		if let Some(mut memory) = self.store.get_memory(memory_id).await? {
			memory.remove_tag(tag);
			self.store.update_memory(&memory).await?;
			Ok(true)
		} else {
			Ok(false)
		}
	}

	/// Add related file to memory
	pub async fn add_related_file(&mut self, memory_id: &str, file_path: String) -> Result<bool> {
		if let Some(mut memory) = self.store.get_memory(memory_id).await? {
			let relative_path = GitUtils::get_relative_path(&file_path).unwrap_or(file_path);
			memory.add_related_file(relative_path);
			self.store.update_memory(&memory).await?;
			Ok(true)
		} else {
			Ok(false)
		}
	}

	/// Remove related file from memory
	pub async fn remove_related_file(&mut self, memory_id: &str, file_path: &str) -> Result<bool> {
		if let Some(mut memory) = self.store.get_memory(memory_id).await? {
			memory.remove_related_file(file_path);
			self.store.update_memory(&memory).await?;
			Ok(true)
		} else {
			Ok(false)
		}
	}
}

/// Memory statistics
#[derive(Debug, Clone)]
pub struct MemoryStats {
	pub total_memories: usize,
	pub type_counts: std::collections::HashMap<String, usize>,
	pub recent_count: usize,
	pub git_commit: Option<String>,
}

impl MemoryStats {
	/// Format stats as human-readable string
	pub fn format(&self) -> String {
		let mut output = "Memory Statistics:\n".to_string();
		output.push_str(&format!("  Total memories: {}\n", self.total_memories));
		output.push_str(&format!("  Recent memories: {}\n", self.recent_count));

		if let Some(ref commit) = self.git_commit {
			output.push_str(&format!("  Current commit: {}\n", commit));
		}

		if !self.type_counts.is_empty() {
			output.push_str("  Memory types:\n");
			for (memory_type, count) in &self.type_counts {
				output.push_str(&format!("    {}: {}\n", memory_type, count));
			}
		}

		output
	}
}
