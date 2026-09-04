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

#[cfg(test)]
mod tests {
	use crate::store::mod_tests::test_connection;
	use crate::store::table_ops::TableOperations;
	use arrow::array::{ArrayRef, RecordBatch, StringArray};
	use arrow_schema::{DataType, Field, Schema};
	use std::sync::Arc;

	fn schema() -> Arc<Schema> {
		Arc::new(Schema::new(vec![
			Field::new("path", DataType::Utf8, false),
			Field::new("hash", DataType::Utf8, false),
		]))
	}

	fn batch(rows: &[(&str, &str)]) -> RecordBatch {
		let paths: Vec<&str> = rows.iter().map(|(p, _)| *p).collect();
		let hashes: Vec<&str> = rows.iter().map(|(_, h)| *h).collect();
		RecordBatch::try_new(
			schema(),
			vec![
				Arc::new(StringArray::from(paths)) as ArrayRef,
				Arc::new(StringArray::from(hashes)) as ArrayRef,
			],
		)
		.unwrap()
	}

	#[tokio::test]
	async fn table_existence_is_reported_for_one_and_many_tables() {
		let (_dir, db) = test_connection().await;
		let ops = TableOperations::new(&db);

		assert!(!ops.table_exists("blocks").await.unwrap());
		assert!(!ops.tables_exist(&["blocks", "other"]).await.unwrap());

		ops.create_table_with_schema("blocks", schema())
			.await
			.unwrap();
		assert!(ops.table_exists("blocks").await.unwrap());
		assert!(ops.tables_exist(&["blocks"]).await.unwrap());
		// One missing table is enough to answer false.
		assert!(!ops.tables_exist(&["blocks", "other"]).await.unwrap());
		// An empty list is trivially satisfied.
		assert!(ops.tables_exist(&[]).await.unwrap());
	}

	#[tokio::test]
	async fn storing_a_batch_creates_the_table_on_first_write() {
		let (_dir, db) = test_connection().await;
		let ops = TableOperations::new(&db);

		ops.store_batch("blocks", batch(&[("src/a.rs", "h1")]))
			.await
			.unwrap();
		assert!(ops.table_exists("blocks").await.unwrap());
		assert!(ops.content_exists("h1", "blocks").await.unwrap());
	}

	#[tokio::test]
	async fn content_existence_is_looked_up_by_hash() {
		let (_dir, db) = test_connection().await;
		let ops = TableOperations::new(&db);
		ops.store_batch("blocks", batch(&[("src/a.rs", "it's-h1")]))
			.await
			.unwrap();

		assert!(ops.content_exists("it's-h1", "blocks").await.unwrap());
		assert!(!ops.content_exists("missing", "blocks").await.unwrap());
	}

	#[tokio::test]
	async fn file_block_hashes_are_listed_per_path() {
		let (_dir, db) = test_connection().await;
		let ops = TableOperations::new(&db);

		// A table that was never created answers with an empty list.
		assert!(ops
			.get_file_blocks_metadata("src/a.rs", "blocks")
			.await
			.unwrap()
			.is_empty());

		ops.store_batch(
			"blocks",
			batch(&[("src/a.rs", "h1"), ("src/a.rs", "h2"), ("src/b.rs", "h3")]),
		)
		.await
		.unwrap();

		let mut hashes = ops
			.get_file_blocks_metadata("src/a.rs", "blocks")
			.await
			.unwrap();
		hashes.sort();
		assert_eq!(hashes, vec!["h1".to_string(), "h2".to_string()]);
	}

	#[tokio::test]
	async fn indexed_paths_are_unioned_across_the_named_tables() {
		let (_dir, db) = test_connection().await;
		let ops = TableOperations::new(&db);
		ops.store_batch("code", batch(&[("src/a.rs", "h1"), ("src/a.rs", "h2")]))
			.await
			.unwrap();
		ops.store_batch("text", batch(&[("notes.txt", "h3")]))
			.await
			.unwrap();

		let paths = ops
			.get_all_indexed_file_paths(&["code", "text", "missing"])
			.await
			.unwrap();
		assert_eq!(paths.len(), 2);
		assert!(paths.contains("src/a.rs"));
		assert!(paths.contains("notes.txt"));
	}

	#[tokio::test]
	async fn removing_by_path_and_by_hash_only_deletes_the_named_rows() {
		let (_dir, db) = test_connection().await;
		let ops = TableOperations::new(&db);
		ops.store_batch(
			"blocks",
			batch(&[("src/a.rs", "h1"), ("src/b.rs", "h2"), ("src/c.rs", "h3")]),
		)
		.await
		.unwrap();

		ops.remove_blocks_by_path("src/a.rs", "blocks")
			.await
			.unwrap();
		assert!(!ops.content_exists("h1", "blocks").await.unwrap());
		assert!(ops.content_exists("h2", "blocks").await.unwrap());

		ops.remove_blocks_by_hashes(&["h2".to_string()], "blocks")
			.await
			.unwrap();
		assert!(!ops.content_exists("h2", "blocks").await.unwrap());
		assert!(ops.content_exists("h3", "blocks").await.unwrap());
	}

	#[tokio::test]
	async fn removal_with_an_empty_argument_or_a_missing_table_is_a_no_op() {
		let (_dir, db) = test_connection().await;
		let ops = TableOperations::new(&db);

		ops.remove_blocks_by_paths(&[], "blocks").await.unwrap();
		ops.remove_blocks_by_hashes(&[], "blocks").await.unwrap();
		ops.remove_blocks_by_path("src/a.rs", "blocks")
			.await
			.unwrap();
		ops.remove_blocks_by_hashes(&["h1".to_string()], "blocks")
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn clearing_drops_the_named_tables_and_leaves_the_rest() {
		let (_dir, db) = test_connection().await;
		let ops = TableOperations::new(&db);
		ops.store_batch("code", batch(&[("src/a.rs", "h1")]))
			.await
			.unwrap();
		ops.store_batch("text", batch(&[("notes.txt", "h2")]))
			.await
			.unwrap();

		ops.clear_table("code").await.unwrap();
		assert!(!ops.table_exists("code").await.unwrap());
		assert!(ops.table_exists("text").await.unwrap());

		// Clearing something that was already dropped is fine.
		ops.clear_table("code").await.unwrap();

		ops.clear_tables(&["text"]).await.unwrap();
		assert!(!ops.table_exists("text").await.unwrap());
	}

	#[tokio::test]
	async fn clearing_everything_leaves_no_tables_behind() {
		let (_dir, db) = test_connection().await;
		let ops = TableOperations::new(&db);
		ops.store_batch("code", batch(&[("src/a.rs", "h1")]))
			.await
			.unwrap();
		ops.store_batch("text", batch(&[("notes.txt", "h2")]))
			.await
			.unwrap();

		ops.clear_all_tables().await.unwrap();
		assert!(db.table_names().execute().await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn flushing_is_a_no_op_that_still_succeeds() {
		let (_dir, db) = test_connection().await;
		TableOperations::new(&db).flush_all_tables().await.unwrap();
	}

	#[tokio::test]
	async fn a_full_text_index_can_be_created_over_the_content_column() {
		let (_dir, db) = test_connection().await;
		let ops = TableOperations::new(&db);

		let content_schema = Arc::new(Schema::new(vec![
			Field::new("path", DataType::Utf8, false),
			Field::new("content", DataType::Utf8, false),
		]));
		let rows = RecordBatch::try_new(
			content_schema,
			vec![
				Arc::new(StringArray::from(vec!["src/a.rs"])) as ArrayRef,
				Arc::new(StringArray::from(vec!["fn parse_remote() {}"])) as ArrayRef,
			],
		)
		.unwrap();
		ops.store_batch("blocks", rows).await.unwrap();

		ops.create_fts_index("blocks").await.unwrap();
		// A second call must not fail on the now-existing index.
		ops.create_fts_index("blocks").await.unwrap();
	}
}
