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
use std::sync::Arc;

// Arrow imports
use arrow_array::{Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};

// LanceDB imports
use futures::TryStreamExt;
use lancedb::{
	query::{ExecutableQuery, QueryBase, Select},
	Connection,
};

use crate::store::sql::escape_single_quotes;
use crate::store::table_ops::TableOperations;
use crate::store::tables;

/// Handles git and file metadata operations
pub struct MetadataOperations<'a> {
	pub db: &'a Connection,
	pub table_ops: TableOperations<'a>,
}

impl<'a> MetadataOperations<'a> {
	pub fn new(db: &'a Connection) -> Self {
		Self {
			db,
			table_ops: TableOperations::new(db),
		}
	}

	// ========================================================================
	// Commit-marker tables
	//
	// `git_metadata`, `graphrag_git_metadata` and `commits_git_metadata` are
	// structurally identical: each holds a single row tracking the last commit
	// hash a given pipeline indexed, plus a timestamp. They differ only by which
	// pipeline owns them, so the schema and the get/store logic are shared here
	// and the per-pipeline methods are thin delegates.
	// ========================================================================

	/// Schema shared by every commit-marker table.
	fn commit_marker_schema() -> Arc<Schema> {
		Arc::new(Schema::new(vec![
			Field::new("commit_hash", DataType::Utf8, false),
			Field::new("indexed_at", DataType::Int64, false),
		]))
	}

	/// Read the stored commit hash from a commit-marker table, if present.
	async fn get_commit_marker(&self, table_name: &str) -> Result<Option<String>> {
		if !self.table_ops.table_exists(table_name).await? {
			return Ok(None);
		}

		let table = self.db.open_table(table_name).execute().await?;
		let mut results = table
			.query()
			.select(Select::Columns(vec!["commit_hash".to_string()]))
			.limit(1)
			.execute()
			.await?;

		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				if let Some(column) = batch.column_by_name("commit_hash") {
					if let Some(hash_array) = column.as_any().downcast_ref::<StringArray>() {
						if let Some(hash) = hash_array.iter().next() {
							return Ok(hash.map(|s| s.to_string()));
						}
					}
				}
			}
		}

		Ok(None)
	}

	/// Overwrite a commit-marker table with `commit_hash` and the current time.
	/// No-op when the stored hash already matches. The table holds exactly one
	/// row, so we clear and rewrite rather than update in place. `store_batch`
	/// creates the table on first write, so no explicit create step is needed.
	async fn store_commit_marker(&self, table_name: &str, commit_hash: &str) -> Result<()> {
		if let Ok(Some(existing_hash)) = self.get_commit_marker(table_name).await {
			if existing_hash == commit_hash {
				return Ok(());
			}
		}

		let batch = RecordBatch::try_new(
			Self::commit_marker_schema(),
			vec![
				Arc::new(StringArray::from(vec![commit_hash])),
				Arc::new(Int64Array::from(vec![chrono::Utc::now().timestamp()])),
			],
		)?;

		self.table_ops.clear_table(table_name).await?;
		self.table_ops.store_batch(table_name, batch).await?;

		Ok(())
	}

	// ----- git_metadata (main code index) -----

	/// Store git metadata (commit hash, etc.)
	pub async fn store_git_metadata(&self, commit_hash: &str) -> Result<()> {
		self.store_commit_marker(tables::GIT_METADATA, commit_hash)
			.await
	}

	/// Get last indexed git commit hash
	pub async fn get_last_commit_hash(&self) -> Result<Option<String>> {
		self.get_commit_marker(tables::GIT_METADATA).await
	}

	/// Clear git metadata table to force full re-scan
	pub async fn clear_git_metadata(&self) -> Result<()> {
		self.table_ops.clear_table(tables::GIT_METADATA).await
	}

	// ----- graphrag_git_metadata -----

	/// Get the last GraphRAG commit hash
	pub async fn get_graphrag_last_commit_hash(&self) -> Result<Option<String>> {
		self.get_commit_marker(tables::GRAPHRAG_GIT_METADATA).await
	}

	/// Store GraphRAG git metadata (commit hash and timestamp)
	pub async fn store_graphrag_commit_hash(&self, commit_hash: &str) -> Result<()> {
		self.store_commit_marker(tables::GRAPHRAG_GIT_METADATA, commit_hash)
			.await
	}

	// ----- commits_git_metadata (commit history index) -----

	/// Get the last commits commit hash
	pub async fn get_commits_last_commit_hash(&self) -> Result<Option<String>> {
		self.get_commit_marker(tables::COMMITS_GIT_METADATA).await
	}

	/// Store commits git metadata (commit hash and timestamp)
	pub async fn store_commits_last_commit_hash(&self, commit_hash: &str) -> Result<()> {
		self.store_commit_marker(tables::COMMITS_GIT_METADATA, commit_hash)
			.await
	}

	// ========================================================================
	// File metadata (per-file modification time, for incremental reindex)
	// ========================================================================

	/// Schema for the `file_metadata` table.
	fn file_metadata_schema() -> Arc<Schema> {
		Arc::new(Schema::new(vec![
			Field::new("path", DataType::Utf8, false),
			Field::new("mtime", DataType::Int64, false),
			Field::new("indexed_at", DataType::Int64, false),
		]))
	}

	/// Create the file metadata table.
	async fn create_file_metadata_table(&self) -> Result<()> {
		self.table_ops
			.create_table_with_schema(tables::FILE_METADATA, Self::file_metadata_schema())
			.await
	}

	/// Store file metadata (modification time, etc.)
	pub async fn store_file_metadata(&self, file_path: &str, mtime: u64) -> Result<()> {
		// Check if table exists, create if not
		if !self.table_ops.table_exists(tables::FILE_METADATA).await? {
			self.create_file_metadata_table().await?;
		}

		let table = self.db.open_table(tables::FILE_METADATA).execute().await?;
		let path_predicate = format!("path = '{}'", escape_single_quotes(file_path));

		// Check if file already exists in metadata
		let mut existing_results = table
			.query()
			.only_if(path_predicate.clone())
			.limit(1)
			.execute()
			.await?;

		let mut file_exists = false;
		while let Some(batch) = existing_results.try_next().await? {
			if batch.num_rows() > 0 {
				file_exists = true;
				break;
			}
		}

		if file_exists {
			// Update existing record using correct LanceDB UpdateBuilder API
			table
				.update()
				.only_if(path_predicate)
				.column("mtime", (mtime as i64).to_string())
				.column("indexed_at", chrono::Utc::now().timestamp().to_string())
				.execute()
				.await?;
		} else {
			// Insert new record
			let batch = RecordBatch::try_new(
				Self::file_metadata_schema(),
				vec![
					Arc::new(StringArray::from(vec![file_path])),
					Arc::new(Int64Array::from(vec![mtime as i64])),
					Arc::new(Int64Array::from(vec![chrono::Utc::now().timestamp()])),
				],
			)?;
			self.table_ops
				.store_batch(tables::FILE_METADATA, batch)
				.await?;
		}

		Ok(())
	}

	/// Get file modification time from metadata
	pub async fn get_file_mtime(&self, file_path: &str) -> Result<Option<u64>> {
		if !self.table_ops.table_exists(tables::FILE_METADATA).await? {
			return Ok(None);
		}

		let table = self.db.open_table(tables::FILE_METADATA).execute().await?;

		// Query for the specific file
		let mut results = table
			.query()
			.only_if(format!("path = '{}'", escape_single_quotes(file_path)))
			.select(Select::Columns(vec!["mtime".to_string()]))
			.limit(1)
			.execute()
			.await?;

		// Process results
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				if let Some(column) = batch.column_by_name("mtime") {
					if let Some(mtime_array) = column.as_any().downcast_ref::<Int64Array>() {
						if let Some(mtime) = mtime_array.iter().next() {
							return Ok(mtime.map(|t| t as u64));
						}
					}
				}
			}
		}

		Ok(None)
	}

	/// Get all file metadata for efficient batch processing
	/// This eliminates the need for individual database queries per file
	pub async fn get_all_file_metadata(&self) -> Result<std::collections::HashMap<String, u64>> {
		let mut metadata_map = std::collections::HashMap::new();

		if !self.table_ops.table_exists(tables::FILE_METADATA).await? {
			return Ok(metadata_map);
		}

		let table = self.db.open_table(tables::FILE_METADATA).execute().await?;

		// Query for all file metadata
		let mut results = table
			.query()
			.select(Select::Columns(vec![
				"path".to_string(),
				"mtime".to_string(),
			]))
			.execute()
			.await?;

		// Process all result batches
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				if let (Some(path_column), Some(mtime_column)) =
					(batch.column_by_name("path"), batch.column_by_name("mtime"))
				{
					if let (Some(path_array), Some(mtime_array)) = (
						path_column.as_any().downcast_ref::<StringArray>(),
						mtime_column.as_any().downcast_ref::<Int64Array>(),
					) {
						for i in 0..path_array.len() {
							// Index directly with `value(i)` — `iter().nth(i)` re-walks the
							// array from the start on every iteration, making this loop
							// O(n^2) over what is meant to be an efficient bulk load.
							if path_array.is_null(i) || mtime_array.is_null(i) {
								continue;
							}
							metadata_map.insert(
								path_array.value(i).to_string(),
								mtime_array.value(i) as u64,
							);
						}
					}
				}
			}
		}

		Ok(metadata_map)
	}
}
