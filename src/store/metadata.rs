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
use arrow::array::{Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;

// LanceDB imports
use futures::TryStreamExt;
use lancedb::{
	query::{ExecutableQuery, QueryBase, Select},
	Connection,
};

use crate::store::table_ops::TableOperations;

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

	/// Store git metadata (commit hash, etc.)
	pub async fn store_git_metadata(&self, commit_hash: &str) -> Result<()> {
		// Check if table exists, create if not
		if !self.table_ops.table_exists("git_metadata").await? {
			self.create_git_metadata_table().await?;
		}

		// Check if the commit hash is already stored
		if let Ok(Some(existing_hash)) = self.get_last_commit_hash().await {
			if existing_hash == commit_hash {
				// Same commit hash, no need to update
				return Ok(());
			}
		}

		// Create a record with the current timestamp
		let schema = Arc::new(Schema::new(vec![
			Field::new("commit_hash", DataType::Utf8, false),
			Field::new("indexed_at", DataType::Int64, false),
		]));

		let commit_hashes = vec![commit_hash];
		let timestamps = vec![chrono::Utc::now().timestamp()];

		let batch = RecordBatch::try_new(
			schema,
			vec![
				Arc::new(StringArray::from(commit_hashes)),
				Arc::new(Int64Array::from(timestamps)),
			],
		)?;

		// Only clear and store if we have a different commit hash
		self.table_ops.clear_table("git_metadata").await?;
		self.table_ops.store_batch("git_metadata", batch).await?;

		Ok(())
	}

	/// Get last indexed git commit hash
	pub async fn get_last_commit_hash(&self) -> Result<Option<String>> {
		if !self.table_ops.table_exists("git_metadata").await? {
			return Ok(None);
		}

		let table = self.db.open_table("git_metadata").execute().await?;

		// Get the most recent commit hash
		let mut results = table
			.query()
			.select(Select::Columns(vec!["commit_hash".to_string()]))
			.limit(1)
			.execute()
			.await?;

		// Process results
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

	/// Store file metadata (modification time, etc.)
	pub async fn store_file_metadata(&self, file_path: &str, mtime: u64) -> Result<()> {
		// Check if table exists, create if not
		if !self.table_ops.table_exists("file_metadata").await? {
			self.create_file_metadata_table().await?;
		}

		let table = self.db.open_table("file_metadata").execute().await?;

		// Check if file already exists in metadata
		let mut existing_results = table
			.query()
			.only_if(format!("path = '{}'", file_path))
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
			// TODO: Fix LanceDB update API - current API doesn't support .set() method
			// For now, we'll skip updating existing records and just insert new ones
			// This is a temporary workaround until we figure out the correct LanceDB update syntax
			/*
			// Update existing record
			table
				.update()
				.only_if(format!("path = '{}'", file_path))
				.set("mtime", mtime as i64)
				.set("indexed_at", chrono::Utc::now().timestamp())
				.execute()
				.await?;
			*/
		} else {
			// Insert new record
			let schema = Arc::new(Schema::new(vec![
				Field::new("path", DataType::Utf8, false),
				Field::new("mtime", DataType::Int64, false),
				Field::new("indexed_at", DataType::Int64, false),
			]));

			let paths = vec![file_path];
			let mtimes = vec![mtime as i64];
			let timestamps = vec![chrono::Utc::now().timestamp()];

			let batch = RecordBatch::try_new(
				schema,
				vec![
					Arc::new(StringArray::from(paths)),
					Arc::new(Int64Array::from(mtimes)),
					Arc::new(Int64Array::from(timestamps)),
				],
			)?;

			// Use RecordBatchIterator instead of Vec<RecordBatch>
			use std::iter::once;
			let batches = once(Ok(batch.clone()));
			let batch_reader =
				arrow::record_batch::RecordBatchIterator::new(batches, batch.schema());
			table.add(batch_reader).execute().await?;
		}

		Ok(())
	}

	/// Get file modification time from metadata
	pub async fn get_file_mtime(&self, file_path: &str) -> Result<Option<u64>> {
		if !self.table_ops.table_exists("file_metadata").await? {
			return Ok(None);
		}

		let table = self.db.open_table("file_metadata").execute().await?;

		// Query for the specific file
		let mut results = table
			.query()
			.only_if(format!("path = '{}'", file_path))
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

		if !self.table_ops.table_exists("file_metadata").await? {
			return Ok(metadata_map);
		}

		let table = self.db.open_table("file_metadata").execute().await?;

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
							if let (Some(path), Some(mtime)) = (
								path_array.iter().nth(i).flatten(),
								mtime_array.iter().nth(i).flatten(),
							) {
								metadata_map.insert(path.to_string(), mtime as u64);
							}
						}
					}
				}
			}
		}

		Ok(metadata_map)
	}

	/// Clear git metadata table to force full re-scan
	pub async fn clear_git_metadata(&self) -> Result<()> {
		self.table_ops.clear_table("git_metadata").await
	}

	/// Create git metadata table
	async fn create_git_metadata_table(&self) -> Result<()> {
		let schema = Arc::new(Schema::new(vec![
			Field::new("commit_hash", DataType::Utf8, false),
			Field::new("indexed_at", DataType::Int64, false),
		]));

		self.table_ops
			.create_table_with_schema("git_metadata", schema)
			.await
	}

	/// Create file metadata table
	async fn create_file_metadata_table(&self) -> Result<()> {
		let schema = Arc::new(Schema::new(vec![
			Field::new("path", DataType::Utf8, false),
			Field::new("mtime", DataType::Int64, false),
			Field::new("indexed_at", DataType::Int64, false),
		]));

		self.table_ops
			.create_table_with_schema("file_metadata", schema)
			.await
	}
}
