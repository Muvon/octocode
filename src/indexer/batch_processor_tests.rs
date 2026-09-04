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
	use super::super::*;
	use crate::config::Config;
	use crate::store::mod_tests::test_store;

	#[test]
	fn a_fresh_metadata_batch_is_empty() {
		let batch = FileMetadataBatch::new();
		assert!(batch.is_empty());
		assert!(FileMetadataBatch::default().is_empty());
	}

	#[tokio::test]
	async fn adding_the_same_path_twice_keeps_the_latest_mtime() {
		let (_dir, store) = test_store().await;
		let mut batch = FileMetadataBatch::new();
		batch.add("src/a.rs", 1);
		batch.add("src/a.rs", 2);
		assert!(!batch.is_empty());

		batch.persist(&store).await.unwrap();
		assert_eq!(store.get_file_mtime("src/a.rs").await.unwrap(), Some(2));
	}

	#[tokio::test]
	async fn clearing_drops_every_pending_entry() {
		let (_dir, store) = test_store().await;
		let mut batch = FileMetadataBatch::new();
		batch.add("src/a.rs", 1);
		batch.clear();
		assert!(batch.is_empty());

		batch.persist(&store).await.unwrap();
		assert!(store.get_all_file_metadata().await.unwrap().is_empty());
	}

	#[tokio::test]
	async fn extending_merges_the_other_batch_and_wins_on_conflicts() {
		let (_dir, store) = test_store().await;
		let mut left = FileMetadataBatch::new();
		left.add("src/a.rs", 1);
		left.add("src/shared.rs", 1);
		let mut right = FileMetadataBatch::new();
		right.add("src/b.rs", 2);
		right.add("src/shared.rs", 9);

		left.extend(&right);
		// The source batch is untouched.
		assert!(!right.is_empty());

		left.persist(&store).await.unwrap();
		assert_eq!(store.get_file_mtime("src/a.rs").await.unwrap(), Some(1));
		assert_eq!(store.get_file_mtime("src/b.rs").await.unwrap(), Some(2));
		assert_eq!(
			store.get_file_mtime("src/shared.rs").await.unwrap(),
			Some(9),
			"the merged-in batch overrides a shared path"
		);
	}

	#[tokio::test]
	async fn persisting_writes_every_pending_mtime() {
		let (_dir, store) = test_store().await;
		let mut batch = FileMetadataBatch::new();
		batch.add("src/a.rs", 11);
		batch.add("src/b.rs", 22);
		batch.persist(&store).await.unwrap();

		assert_eq!(store.get_file_mtime("src/a.rs").await.unwrap(), Some(11));
		assert_eq!(store.get_file_mtime("src/b.rs").await.unwrap(), Some(22));
	}

	#[tokio::test]
	async fn persisting_an_empty_batch_is_a_no_op() {
		let (_dir, store) = test_store().await;
		FileMetadataBatch::new().persist(&store).await.unwrap();
		assert!(store.get_all_file_metadata().await.unwrap().is_empty());
	}

	#[test]
	fn an_empty_batch_never_triggers_processing() {
		let config = Config::default();
		let empty: Vec<String> = Vec::new();
		assert!(!should_process_batch(&empty, |s| s.as_str(), &config));
	}

	#[test]
	fn reaching_the_configured_batch_size_triggers_processing() {
		let config = Config::default();
		let batch: Vec<String> = (0..config.index.embeddings_batch_size)
			.map(|i| format!("item {i}"))
			.collect();
		assert!(should_process_batch(&batch, |s| s.as_str(), &config));
	}

	#[test]
	fn one_item_short_of_the_batch_size_keeps_accumulating() {
		let mut config = Config::default();
		// Keep the token budget out of the way so only the size limit can fire.
		config.index.embeddings_max_tokens_per_batch = usize::MAX;
		let batch: Vec<String> = (0..config.index.embeddings_batch_size - 1)
			.map(|i| format!("item {i}"))
			.collect();
		assert!(!should_process_batch(&batch, |s| s.as_str(), &config));
	}

	#[test]
	fn a_single_oversized_item_triggers_the_token_budget() {
		let mut config = Config::default();
		// Keep the size limit out of the way so only the token budget can fire.
		config.index.embeddings_batch_size = 10_000;
		config.index.embeddings_max_tokens_per_batch = 10;

		let small = vec!["hi".to_string()];
		assert!(!should_process_batch(&small, |s| s.as_str(), &config));

		let large = vec!["word ".repeat(500)];
		assert!(should_process_batch(&large, |s| s.as_str(), &config));
	}

	#[test]
	fn the_token_budget_sums_across_the_whole_batch() {
		let mut config = Config::default();
		config.index.embeddings_batch_size = 10_000;
		config.index.embeddings_max_tokens_per_batch = 10;

		// No single item is over budget, but together they are.
		let batch: Vec<String> = (0..20).map(|_| "word".to_string()).collect();
		assert!(should_process_batch(&batch, |s| s.as_str(), &config));
	}
}
