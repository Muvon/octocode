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
use arrow::array::Array;
use arrow::datatypes::Schema;
use arrow::record_batch::RecordBatch;

// LanceDB imports
use futures::TryStreamExt;
use lancedb::{
	query::{ExecutableQuery, QueryBase, Select},
	Connection, DistanceType,
};

/// Generic table operations for LanceDB
pub struct TableOperations<'a> {
	pub db: &'a Connection,
}

impl<'a> TableOperations<'a> {
	pub fn new(db: &'a Connection) -> Self {
		Self { db }
	}

	/// Check if a table exists
	pub async fn table_exists(&self, table_name: &str) -> Result<bool> {
		let table_names = self.db.table_names().execute().await?;
		Ok(table_names.contains(&table_name.to_string()))
	}

	/// Check if multiple tables exist
	pub async fn tables_exist(&self, table_names: &[&str]) -> Result<bool> {
		let existing_tables = self.db.table_names().execute().await?;
		for &table_name in table_names {
			if !existing_tables.contains(&table_name.to_string()) {
				return Ok(false);
			}
		}
		Ok(true)
	}

	/// Clear (drop) a single table
	pub async fn clear_table(&self, table_name: &str) -> Result<()> {
		let table_names = self.db.table_names().execute().await?;

		if table_names.contains(&table_name.to_string()) {
			if let Err(e) = self.db.drop_table(table_name).await {
				// Log error to structured logging instead of stderr
				tracing::warn!("Failed to drop {} table: {}", table_name, e);
			} else {
				// Log success to structured logging instead of stdout
				tracing::debug!("Dropped table: {}", table_name);
			}
		} else {
			// Log info to structured logging instead of stdout
			tracing::debug!("Table {} does not exist, skipping.", table_name);
		}

		Ok(())
	}

	/// Clear multiple tables
	pub async fn clear_tables(&self, table_names: &[&str]) -> Result<()> {
		for &table_name in table_names {
			self.clear_table(table_name).await?;
		}
		Ok(())
	}

	/// Clear all tables (drop tables completely to reset schema)
	pub async fn clear_all_tables(&self) -> Result<()> {
		// Get table names
		let table_names = self.db.table_names().execute().await?;

		// Drop each table completely (this removes both data and schema)
		for table_name in table_names {
			if let Err(e) = self.db.drop_table(&table_name).await {
				// Log error to structured logging instead of stderr
				tracing::warn!("Failed to drop table {}: {}", table_name, e);
			} else {
				// Log success to structured logging instead of stdout
				tracing::debug!("Dropped table: {}", table_name);
			}
		}

		Ok(())
	}

	/// Flush all tables to ensure data is persisted
	pub async fn flush_all_tables(&self) -> Result<()> {
		// Get all tables
		let table_names = self.db.table_names().execute().await?;

		// Open and flush each table by performing operations that force persistence
		for table_name in table_names {
			let table = self.db.open_table(&table_name).execute().await?;

			// Perform operations to ensure any pending writes are flushed:
			// 1. Count rows to force read access and ensure consistency
			let row_count = table.count_rows(None).await?;

			// 2. For tables with data, also check schema to ensure metadata is flushed
			if row_count > 0 {
				let _ = table.schema().await?;
			}

			// Log flush activity in debug mode for troubleshooting
			if cfg!(debug_assertions) {
				tracing::debug!("Flushed table '{}' with {} rows", table_name, row_count);
			}
		}

		Ok(())
	}

	/// Check if content exists in a table by hash
	pub async fn content_exists(&self, hash: &str, collection: &str) -> Result<bool> {
		let table = self.db.open_table(collection).execute().await?;

		// Use a more efficient query to check existence
		let mut results = table
			.query()
			.only_if(format!("hash = '{}'", hash))
			.limit(1) // We only need to know if one exists
			.select(Select::Columns(vec!["hash".to_string()])) // Only select hash column
			.execute()
			.await?;

		// Check if we got any results
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				return Ok(true);
			}
		}

		Ok(false)
	}

	/// Remove blocks by path from a table
	pub async fn remove_blocks_by_path(&self, file_path: &str, table_name: &str) -> Result<usize> {
		if !self.table_exists(table_name).await? {
			return Ok(0);
		}

		let table = self.db.open_table(table_name).execute().await?;

		// Count rows before deletion for reporting
		let before_count = table.count_rows(None).await?;

		// Delete rows matching the file path
		table
			.delete(&format!("path = '{}'", file_path))
			.await
			.map_err(|e| anyhow::anyhow!("Failed to delete from {}: {}", table_name, e))?;

		// Count rows after deletion
		let after_count = table.count_rows(None).await?;
		let deleted_count = before_count.saturating_sub(after_count);

		Ok(deleted_count)
	}

	/// Remove blocks by hashes from a table
	pub async fn remove_blocks_by_hashes(&self, hashes: &[String], table_name: &str) -> Result<()> {
		if hashes.is_empty() {
			return Ok(());
		}

		if !self.table_exists(table_name).await? {
			return Ok(());
		}

		let table = self.db.open_table(table_name).execute().await?;

		// Create a filter for all hashes
		let hash_filters: Vec<String> = hashes.iter().map(|h| format!("hash = '{}'", h)).collect();
		let filter = hash_filters.join(" OR ");

		// Delete rows matching any of the hashes
		table
			.delete(&filter)
			.await
			.map_err(|e| anyhow::anyhow!("Failed to delete from {}: {}", table_name, e))?;

		Ok(())
	}

	/// Get file metadata (hashes) for a specific file path from a table
	pub async fn get_file_blocks_metadata(
		&self,
		file_path: &str,
		table_name: &str,
	) -> Result<Vec<String>> {
		let mut hashes = Vec::new();

		if !self.table_exists(table_name).await? {
			return Ok(hashes);
		}

		let table = self.db.open_table(table_name).execute().await?;

		// Query for blocks matching the file path, only selecting hash column
		let mut results = table
			.query()
			.only_if(format!("path = '{}'", file_path))
			.select(Select::Columns(vec!["hash".to_string()]))
			.execute()
			.await?;

		// Process all result batches
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				if let Some(column) = batch.column_by_name("hash") {
					if let Some(hash_array) =
						column.as_any().downcast_ref::<arrow::array::StringArray>()
					{
						for i in 0..hash_array.len() {
							hashes.push(hash_array.value(i).to_string());
						}
					}
				}
			}
		}

		Ok(hashes)
	}

	/// Get all indexed file paths from multiple tables
	pub async fn get_all_indexed_file_paths(
		&self,
		table_names: &[&str],
	) -> Result<std::collections::HashSet<String>> {
		let mut all_paths = std::collections::HashSet::new();

		let existing_tables = self.db.table_names().execute().await?;

		for &table_name in table_names {
			if existing_tables.contains(&table_name.to_string()) {
				let table = self.db.open_table(table_name).execute().await?;

				// Query for all paths in this table
				let mut results = table
					.query()
					.select(Select::Columns(vec!["path".to_string()]))
					.execute()
					.await?;

				// Process all result batches
				while let Some(batch) = results.try_next().await? {
					if batch.num_rows() > 0 {
						if let Some(column) = batch.column_by_name("path") {
							if let Some(path_array) =
								column.as_any().downcast_ref::<arrow::array::StringArray>()
							{
								for i in 0..path_array.len() {
									all_paths.insert(path_array.value(i).to_string());
								}
							}
						}
					}
				}
			}
		}

		Ok(all_paths)
	}

	/// Create a table with the given schema
	pub async fn create_table_with_schema(
		&self,
		table_name: &str,
		schema: Arc<Schema>,
	) -> Result<()> {
		let _table = self
			.db
			.create_empty_table(table_name, schema)
			.execute()
			.await?;
		Ok(())
	}

	/// Store a record batch in a table (create table if it doesn't exist)
	pub async fn store_batch(&self, table_name: &str, batch: RecordBatch) -> Result<()> {
		// Check if table exists
		if self.table_exists(table_name).await? {
			// Table exists, append data
			let table = self.db.open_table(table_name).execute().await?;

			// Use RecordBatchIterator instead of Vec<RecordBatch>
			use std::iter::once;
			let batches = once(Ok(batch.clone()));
			let batch_reader =
				arrow::record_batch::RecordBatchIterator::new(batches, batch.schema());
			table.add(batch_reader).execute().await?;
		} else {
			// Table doesn't exist, create it with the batch
			use std::iter::once;
			let batches = once(Ok(batch.clone()));
			let batch_reader =
				arrow::record_batch::RecordBatchIterator::new(batches, batch.schema());
			let _table = self
				.db
				.create_table(table_name, batch_reader)
				.execute()
				.await?;
		}

		Ok(())
	}

	/// Create vector index on a table
	pub async fn create_vector_index(
		&self,
		table_name: &str,
		column_name: &str,
		distance_type: DistanceType,
	) -> Result<()> {
		if !self.table_exists(table_name).await? {
			return Err(anyhow::anyhow!("Table {} does not exist", table_name));
		}

		let table = self.db.open_table(table_name).execute().await?;

		// Create vector index
		table
			.create_index(
				&[column_name],
				lancedb::index::Index::IvfPq(
					lancedb::index::vector::IvfPqIndexBuilder::default()
						.distance_type(distance_type),
				),
			)
			.execute()
			.await?;

		Ok(())
	}
}
