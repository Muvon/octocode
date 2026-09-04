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

//! Shared LanceDB fixtures for the `store` tests, plus the `Store` lifecycle
//! tests themselves.

use crate::config::Config;
use crate::store::{CodeBlock, CommitBlock, DocumentBlock, Store, TextBlock};
use std::path::PathBuf;
use std::sync::OnceLock;
use tempfile::TempDir;

/// Vector width of `voyage:voyage-code-3` / `voyage:voyage-3.5-lite`, the models
/// the test config pins so store construction never touches the network.
pub(crate) const CODE_DIM: usize = 1024;
pub(crate) const TEXT_DIM: usize = 1024;

static TEST_CONFIG: OnceLock<PathBuf> = OnceLock::new();

/// Point `Config::load()` at a throwaway config pinned to API-backed embedding
/// models. `Store::new_with_path` only asks the provider for its vector width,
/// and the Voyage provider answers that offline — whereas the default
/// `fastembed` models would download an ONNX model on first construction.
pub(crate) fn use_offline_test_config() {
	let path = TEST_CONFIG.get_or_init(|| {
		let dir = std::env::temp_dir().join("octocode-store-tests");
		std::fs::create_dir_all(&dir).expect("config dir");
		let path = dir.join("config.toml");

		let mut config = Config::default();
		config.embedding.code_model = "voyage:voyage-code-3".to_string();
		config.embedding.text_model = "voyage:voyage-3.5-lite".to_string();
		std::fs::write(
			&path,
			toml::to_string_pretty(&config).expect("serialize config"),
		)
		.expect("write config");
		path
	});
	std::env::set_var("OCTOCODE_CONFIG_PATH", path);
}

/// A `Store` backed by a fresh LanceDB database in a temp directory, with all
/// block tables created.
pub(crate) async fn test_store() -> (TempDir, Store) {
	use_offline_test_config();
	let dir = TempDir::new().expect("tempdir");
	let store = Store::new_with_path(dir.path().join("db"))
		.await
		.expect("open store");
	store
		.initialize_collections()
		.await
		.expect("create collections");
	(dir, store)
}

/// A raw LanceDB connection with no tables, for the operations-layer tests.
pub(crate) async fn test_connection() -> (TempDir, lancedb::Connection) {
	let dir = TempDir::new().expect("tempdir");
	let db = lancedb::connect(dir.path().join("db").to_str().unwrap())
		.execute()
		.await
		.expect("connect");
	(dir, db)
}

/// A deterministic unit vector whose direction is driven by `seed`, so tests can
/// control which block a nearest-neighbour query returns.
pub(crate) fn embedding(dim: usize, seed: usize) -> Vec<f32> {
	let mut v = vec![0.0f32; dim];
	v[seed % dim] = 1.0;
	v
}

pub(crate) fn code_block(path: &str, hash: &str) -> CodeBlock {
	CodeBlock {
		path: path.to_string(),
		language: "rust".to_string(),
		content: format!("fn from_{hash}() {{}}"),
		symbols: vec![format!("from_{hash}")],
		start_line: 1,
		end_line: 3,
		hash: hash.to_string(),
		distance: None,
	}
}

pub(crate) fn text_block(path: &str, hash: &str) -> TextBlock {
	TextBlock {
		path: path.to_string(),
		language: "text".to_string(),
		content: format!("note {hash}"),
		start_line: 1,
		end_line: 2,
		hash: hash.to_string(),
		distance: None,
	}
}

pub(crate) fn document_block(path: &str, hash: &str) -> DocumentBlock {
	DocumentBlock {
		path: path.to_string(),
		title: format!("Title {hash}"),
		content: format!("body {hash}"),
		context: vec!["Root".to_string()],
		level: 2,
		start_line: 1,
		end_line: 4,
		hash: hash.to_string(),
		distance: None,
	}
}

pub(crate) fn graph_node(
	id: &str,
	kind: &str,
	seed: usize,
) -> crate::indexer::graphrag::types::CodeNode {
	crate::indexer::graphrag::types::CodeNode {
		id: id.to_string(),
		name: id.rsplit('/').next().unwrap_or(id).to_string(),
		kind: kind.to_string(),
		path: id.to_string(),
		description: format!("node {id}"),
		symbols: vec![format!("sym_{seed}")],
		hash: format!("nh-{id}"),
		embedding: embedding(CODE_DIM, seed),
		imports: vec!["dep".to_string()],
		exports: vec!["api".to_string()],
		functions: vec![],
		size_lines: 42,
		language: "rust".to_string(),
	}
}

pub(crate) fn graph_relationship(
	source: &str,
	target: &str,
	relation_type: crate::indexer::graphrag::types::RelationType,
) -> crate::indexer::graphrag::types::CodeRelationship {
	crate::indexer::graphrag::types::CodeRelationship {
		source: source.to_string(),
		target: target.to_string(),
		relation_type,
		description: format!("{source} -> {target}"),
		confidence: 0.9,
		weight: 0.7,
		provenance: crate::indexer::graphrag::types::Provenance::Extracted,
	}
}

pub(crate) fn commit_block(hash: &str) -> CommitBlock {
	CommitBlock {
		hash: hash.to_string(),
		author: "dev".to_string(),
		date: 1_700_000_000,
		message: format!("message {hash}"),
		content: format!("message {hash} body"),
		files: "[\"src/a.rs\"]".to_string(),
		description: String::new(),
		distance: None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::store::tables;
	use crate::store::HybridSearchQuery;

	#[tokio::test]
	async fn a_fresh_store_reports_the_configured_vector_width() {
		let (_dir, store) = test_store().await;
		assert_eq!(store.get_code_vector_dim(), CODE_DIM);
	}

	#[tokio::test]
	async fn initialising_collections_twice_is_idempotent() {
		let (_dir, store) = test_store().await;
		store.initialize_collections().await.unwrap();
		for table in [
			tables::CODE_BLOCKS,
			tables::TEXT_BLOCKS,
			tables::DOCUMENT_BLOCKS,
		] {
			assert_eq!(store.get_table_row_count(table).await.unwrap(), 0);
		}
	}

	#[tokio::test]
	async fn stored_code_blocks_come_back_from_a_vector_query() {
		let (_dir, store) = test_store().await;
		let blocks = vec![code_block("src/a.rs", "h1"), code_block("src/b.rs", "h2")];
		let embeddings = vec![embedding(CODE_DIM, 0), embedding(CODE_DIM, 1)];
		store.store_code_blocks(&blocks, &embeddings).await.unwrap();
		store.flush().await.unwrap();

		assert_eq!(
			store
				.get_table_row_count(tables::CODE_BLOCKS)
				.await
				.unwrap(),
			2
		);

		let hits = store
			.get_code_blocks_with_config(embedding(CODE_DIM, 0), Some(1), None)
			.await
			.unwrap();
		assert_eq!(hits.len(), 1);
		assert_eq!(hits[0].path, "src/a.rs");
		assert_eq!(hits[0].symbols, vec!["from_h1".to_string()]);
		assert!(hits[0].distance.is_some());
	}

	#[tokio::test]
	async fn a_language_filter_narrows_the_result_set() {
		let (_dir, store) = test_store().await;
		let mut python = code_block("src/a.py", "hp");
		python.language = "python".to_string();
		let blocks = vec![code_block("src/a.rs", "hr"), python];
		let embeddings = vec![embedding(CODE_DIM, 0), embedding(CODE_DIM, 0)];
		store.store_code_blocks(&blocks, &embeddings).await.unwrap();

		let hits = store
			.get_code_blocks_with_language_filter(
				embedding(CODE_DIM, 0),
				Some(10),
				None,
				Some("python"),
			)
			.await
			.unwrap();
		assert_eq!(hits.len(), 1);
		assert_eq!(hits[0].path, "src/a.py");
	}

	#[tokio::test]
	async fn a_distance_threshold_drops_dissimilar_blocks() {
		let (_dir, store) = test_store().await;
		store
			.store_code_blocks(&[code_block("src/a.rs", "h1")], &[embedding(CODE_DIM, 0)])
			.await
			.unwrap();

		// An orthogonal query vector is cosine distance 1.0 away.
		let hits = store
			.get_code_blocks_with_config(embedding(CODE_DIM, 5), Some(10), Some(0.1))
			.await
			.unwrap();
		assert!(hits.is_empty(), "got {hits:?}");
	}

	#[tokio::test]
	async fn text_document_and_commit_blocks_round_trip() {
		let (_dir, store) = test_store().await;

		store
			.store_text_blocks(&[text_block("notes.txt", "t1")], &[embedding(TEXT_DIM, 0)])
			.await
			.unwrap();
		store
			.store_document_blocks(
				&[document_block("README.md", "d1")],
				&[embedding(TEXT_DIM, 0)],
			)
			.await
			.unwrap();
		store
			.store_commit_blocks(&[commit_block("c1")], &[embedding(TEXT_DIM, 0)])
			.await
			.unwrap();

		let texts = store
			.get_text_blocks_with_config(embedding(TEXT_DIM, 0), Some(5), None)
			.await
			.unwrap();
		assert_eq!(texts.len(), 1);
		assert_eq!(texts[0].content, "note t1");

		let docs = store
			.get_document_blocks_with_config(embedding(TEXT_DIM, 0), Some(5), None)
			.await
			.unwrap();
		assert_eq!(docs.len(), 1);
		assert_eq!(docs[0].title, "Title d1");
		assert_eq!(docs[0].context, vec!["Root".to_string()]);
		assert_eq!(docs[0].level, 2);

		let commits = store
			.get_commit_blocks_with_config(embedding(TEXT_DIM, 0), Some(5), None)
			.await
			.unwrap();
		assert_eq!(commits.len(), 1);
		assert_eq!(commits[0].hash, "c1");
		assert_eq!(commits[0].author, "dev");
	}

	#[tokio::test]
	async fn content_existence_is_reported_per_table() {
		let (_dir, store) = test_store().await;
		assert!(!store
			.content_exists("h1", tables::CODE_BLOCKS)
			.await
			.unwrap());
		// A table that was never created answers false rather than erroring.
		assert!(!store
			.content_exists("h1", tables::COMMIT_BLOCKS)
			.await
			.unwrap());

		store
			.store_code_blocks(&[code_block("src/a.rs", "h1")], &[embedding(CODE_DIM, 0)])
			.await
			.unwrap();
		assert!(store
			.content_exists("h1", tables::CODE_BLOCKS)
			.await
			.unwrap());
		assert!(!store
			.content_exists("nope", tables::CODE_BLOCKS)
			.await
			.unwrap());
	}

	#[tokio::test]
	async fn a_hash_containing_a_quote_cannot_break_the_predicate() {
		let (_dir, store) = test_store().await;
		let hash = "it's-a-hash";
		store
			.store_code_blocks(&[code_block("src/a.rs", hash)], &[embedding(CODE_DIM, 0)])
			.await
			.unwrap();
		assert!(store
			.content_exists(hash, tables::CODE_BLOCKS)
			.await
			.unwrap());
	}

	#[tokio::test]
	async fn indexed_paths_are_collected_across_tables() {
		let (_dir, store) = test_store().await;
		store
			.store_code_blocks(&[code_block("src/a.rs", "h1")], &[embedding(CODE_DIM, 0)])
			.await
			.unwrap();
		store
			.store_text_blocks(&[text_block("notes.txt", "t1")], &[embedding(TEXT_DIM, 0)])
			.await
			.unwrap();

		let paths = store.get_all_indexed_file_paths().await.unwrap();
		assert!(paths.contains("src/a.rs"));
		assert!(paths.contains("notes.txt"));
	}

	#[tokio::test]
	async fn removing_by_path_only_drops_that_files_blocks() {
		let (_dir, store) = test_store().await;
		store
			.store_code_blocks(
				&[code_block("src/a.rs", "h1"), code_block("src/b.rs", "h2")],
				&[embedding(CODE_DIM, 0), embedding(CODE_DIM, 1)],
			)
			.await
			.unwrap();

		store.remove_blocks_by_path("src/a.rs").await.unwrap();
		let paths = store.get_all_indexed_file_paths().await.unwrap();
		assert!(!paths.contains("src/a.rs"));
		assert!(paths.contains("src/b.rs"));

		store
			.remove_blocks_by_paths(&["src/b.rs".to_string()])
			.await
			.unwrap();
		assert!(store.get_all_indexed_file_paths().await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn a_search_after_removal_does_not_serve_the_deleted_blocks() {
		// The first search populates the Store's table-handle cache; the removal
		// deletes through a separately opened handle. Without invalidation the
		// cached handle stays pinned to the pre-delete dataset version and keeps
		// returning the removed block.
		let (_dir, store) = test_store().await;
		store
			.store_code_blocks(
				&[code_block("src/a.rs", "h1"), code_block("src/b.rs", "h2")],
				&[embedding(CODE_DIM, 0), embedding(CODE_DIM, 1)],
			)
			.await
			.unwrap();
		assert_eq!(
			store
				.get_code_blocks_with_config(embedding(CODE_DIM, 0), Some(10), None)
				.await
				.unwrap()
				.len(),
			2
		);

		store.remove_blocks_by_path("src/a.rs").await.unwrap();

		let after = store
			.get_code_blocks_with_config(embedding(CODE_DIM, 0), Some(10), None)
			.await
			.unwrap();
		assert_eq!(after.len(), 1, "got {after:?}");
		assert_eq!(after[0].path, "src/b.rs");
	}

	#[tokio::test]
	async fn a_search_after_clearing_a_table_returns_nothing() {
		let (_dir, store) = test_store().await;
		store
			.store_code_blocks(&[code_block("src/a.rs", "h1")], &[embedding(CODE_DIM, 0)])
			.await
			.unwrap();
		assert_eq!(
			store
				.get_code_blocks_with_config(embedding(CODE_DIM, 0), Some(10), None)
				.await
				.unwrap()
				.len(),
			1
		);

		// `clear_code_table` drops the table outright, so a surviving cached
		// handle would point at a table that no longer exists.
		store.clear_code_table().await.unwrap();
		assert!(store
			.get_code_blocks_with_config(embedding(CODE_DIM, 0), Some(10), None)
			.await
			.unwrap()
			.is_empty());
	}

	#[tokio::test]
	async fn removing_no_paths_is_a_no_op() {
		let (_dir, store) = test_store().await;
		store
			.store_code_blocks(&[code_block("src/a.rs", "h1")], &[embedding(CODE_DIM, 0)])
			.await
			.unwrap();
		store.remove_blocks_by_paths(&[]).await.unwrap();
		assert_eq!(
			store
				.get_table_row_count(tables::CODE_BLOCKS)
				.await
				.unwrap(),
			1
		);
	}

	#[tokio::test]
	async fn clearing_targets_one_table_at_a_time() {
		let (_dir, store) = test_store().await;
		store
			.store_code_blocks(&[code_block("src/a.rs", "h1")], &[embedding(CODE_DIM, 0)])
			.await
			.unwrap();
		store
			.store_text_blocks(&[text_block("notes.txt", "t1")], &[embedding(TEXT_DIM, 0)])
			.await
			.unwrap();
		store
			.store_document_blocks(
				&[document_block("README.md", "d1")],
				&[embedding(TEXT_DIM, 0)],
			)
			.await
			.unwrap();
		store
			.store_commit_blocks(&[commit_block("c1")], &[embedding(TEXT_DIM, 0)])
			.await
			.unwrap();

		store.clear_code_table().await.unwrap();
		assert_eq!(
			store
				.get_table_row_count(tables::CODE_BLOCKS)
				.await
				.unwrap(),
			0
		);
		assert_eq!(
			store
				.get_table_row_count(tables::TEXT_BLOCKS)
				.await
				.unwrap(),
			1
		);

		store.clear_text_table().await.unwrap();
		store.clear_docs_table().await.unwrap();
		store.clear_commits_table().await.unwrap();
		for table in [
			tables::TEXT_BLOCKS,
			tables::DOCUMENT_BLOCKS,
			tables::COMMIT_BLOCKS,
		] {
			assert_eq!(store.get_table_row_count(table).await.unwrap(), 0);
		}
	}

	#[tokio::test]
	async fn clearing_all_tables_empties_everything() {
		let (_dir, store) = test_store().await;
		store
			.store_code_blocks(&[code_block("src/a.rs", "h1")], &[embedding(CODE_DIM, 0)])
			.await
			.unwrap();
		store.store_git_metadata("abc123").await.unwrap();

		store.clear_all_tables().await.unwrap();
		assert_eq!(
			store
				.get_table_row_count(tables::CODE_BLOCKS)
				.await
				.unwrap(),
			0
		);
	}

	#[tokio::test]
	async fn git_and_graphrag_commit_markers_round_trip() {
		let (_dir, store) = test_store().await;
		assert_eq!(store.get_last_commit_hash().await.unwrap(), None);

		store.store_git_metadata("abc123").await.unwrap();
		assert_eq!(
			store.get_last_commit_hash().await.unwrap().as_deref(),
			Some("abc123")
		);

		store.store_graphrag_commit_hash("def456").await.unwrap();
		assert_eq!(
			store
				.get_graphrag_last_commit_hash()
				.await
				.unwrap()
				.as_deref(),
			Some("def456")
		);

		store
			.store_commits_last_commit_hash("ghi789")
			.await
			.unwrap();
		assert_eq!(
			store
				.get_commits_last_commit_hash()
				.await
				.unwrap()
				.as_deref(),
			Some("ghi789")
		);

		store.clear_git_metadata().await.unwrap();
		assert_eq!(store.get_last_commit_hash().await.unwrap(), None);
		store.clear_graphrag_git_metadata().await.unwrap();
		assert_eq!(store.get_graphrag_last_commit_hash().await.unwrap(), None);
		store.clear_commits_git_metadata().await.unwrap();
		assert_eq!(store.get_commits_last_commit_hash().await.unwrap(), None);
	}

	#[tokio::test]
	async fn file_mtimes_round_trip_individually_and_in_bulk() {
		let (_dir, store) = test_store().await;
		assert_eq!(store.get_file_mtime("src/a.rs").await.unwrap(), None);

		store.store_file_metadata("src/a.rs", 111).await.unwrap();
		assert_eq!(store.get_file_mtime("src/a.rs").await.unwrap(), Some(111));

		store
			.store_file_metadata_batch(&[
				("src/b.rs".to_string(), 222),
				("src/c.rs".to_string(), 333),
			])
			.await
			.unwrap();
		let all = store.get_all_file_metadata().await.unwrap();
		assert_eq!(all.get("src/a.rs"), Some(&111));
		assert_eq!(all.get("src/b.rs"), Some(&222));
		assert_eq!(all.get("src/c.rs"), Some(&333));
	}

	#[tokio::test]
	async fn storing_a_file_mtime_twice_keeps_the_latest_value() {
		let (_dir, store) = test_store().await;
		store.store_file_metadata("src/a.rs", 111).await.unwrap();
		store.store_file_metadata("src/a.rs", 999).await.unwrap();
		assert_eq!(store.get_file_mtime("src/a.rs").await.unwrap(), Some(999));
	}

	#[tokio::test]
	async fn storing_an_empty_block_slice_fails_fast() {
		let (_dir, store) = test_store().await;
		// An empty write is a caller bug, not something to swallow silently.
		let err = store.store_code_blocks(&[], &[]).await.unwrap_err();
		assert!(err.to_string().contains("Empty blocks array"), "{err}");

		// Empty mtime batches, by contrast, are a normal incremental-run outcome.
		store.store_file_metadata_batch(&[]).await.unwrap();
		assert_eq!(
			store
				.get_table_row_count(tables::CODE_BLOCKS)
				.await
				.unwrap(),
			0
		);
	}

	#[tokio::test]
	async fn optimize_and_flush_run_cleanly_on_a_small_index() {
		let (_dir, store) = test_store().await;
		store
			.store_code_blocks(&[code_block("src/a.rs", "h1")], &[embedding(CODE_DIM, 0)])
			.await
			.unwrap();
		store.flush().await.unwrap();
		store.optimize_tables().await.unwrap();
		store.close().await.unwrap();
	}

	#[tokio::test]
	async fn a_row_count_for_a_missing_table_is_zero() {
		let (_dir, store) = test_store().await;
		assert_eq!(store.get_table_row_count("no_such_table").await.unwrap(), 0);
	}

	#[tokio::test]
	async fn a_reopened_store_sees_the_previously_written_blocks() {
		use_offline_test_config();
		let dir = TempDir::new().unwrap();
		let path = dir.path().join("db");
		{
			let store = Store::new_with_path(path.clone()).await.unwrap();
			store.initialize_collections().await.unwrap();
			store
				.store_code_blocks(&[code_block("src/a.rs", "h1")], &[embedding(CODE_DIM, 0)])
				.await
				.unwrap();
			store.flush().await.unwrap();
		}
		let reopened = Store::new_with_path(path).await.unwrap();
		assert_eq!(
			reopened
				.get_table_row_count(tables::CODE_BLOCKS)
				.await
				.unwrap(),
			1
		);
	}

	#[test]
	fn a_hybrid_query_needs_at_least_one_signal() {
		let empty = HybridSearchQuery {
			vector_query: None,
			keywords: None,
			vector_weight: 0.5,
			keyword_weight: 0.5,
			limit: 10,
			min_relevance: None,
			language_filter: None,
		};
		assert!(empty.validate().is_err());
	}
}
