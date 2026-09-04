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
	use crate::state::create_shared_state;
	use crate::store::mod_tests::{embedding, test_store, TEXT_DIM};
	use std::path::Path;

	fn state(force_reindex: bool) -> crate::state::SharedState {
		let state = create_shared_state();
		state.write().force_reindex = force_reindex;
		state
	}

	#[test]
	fn markdown_files_are_recognised_by_extension() {
		assert!(is_markdown_file(Path::new("README.md")));
		assert!(is_markdown_file(Path::new("book.MARKDOWN")));
		assert!(!is_markdown_file(Path::new("notes.txt")));
		assert!(!is_markdown_file(Path::new("noext")));
	}

	#[test]
	fn allowed_text_extensions_pass_through_to_the_shared_list() {
		assert!(is_allowed_text_extension(Path::new("notes.txt")));
		assert!(is_allowed_text_extension(Path::new("config.toml")));
		assert!(!is_allowed_text_extension(Path::new("logo.png")));
	}

	#[test]
	fn readable_text_is_distinguished_from_binary_noise() {
		assert!(is_text_file("plain readable text\n"));
		assert!(!is_text_file("\u{0}\u{1}\u{2}binary\u{0}"));
	}

	#[test]
	fn chunking_delegates_to_the_shared_text_processor() {
		let content = (1..=30)
			.map(|i| format!("line {i}"))
			.collect::<Vec<_>>()
			.join("\n");
		let chunks = chunk_text(&content, 40, 0);
		assert!(chunks.len() > 1);
		assert_eq!(chunks[0].start_line, 1);
	}

	#[tokio::test]
	async fn a_text_file_becomes_one_block_per_chunk() {
		let (_dir, store) = test_store().await;
		let mut config = Config::default();
		config.index.chunk_size = 40;
		config.index.chunk_overlap = 0;

		let content = (1..=30)
			.map(|i| format!("note line {i}"))
			.collect::<Vec<_>>()
			.join("\n");

		let mut batch = Vec::new();
		process_text_file(
			&store,
			&content,
			"notes.txt",
			&mut batch,
			&config,
			state(false),
		)
		.await
		.unwrap();

		assert!(batch.len() > 1);
		assert!(batch.iter().all(|b| b.path == "notes.txt"));
		assert!(batch.iter().all(|b| b.language == "text"));
	}

	#[tokio::test]
	async fn a_text_chunk_already_in_the_store_is_skipped_unless_forced() {
		let (_dir, store) = test_store().await;
		let config = Config::default();

		let mut first = Vec::new();
		process_text_file(
			&store,
			"a short note",
			"notes.txt",
			&mut first,
			&config,
			state(false),
		)
		.await
		.unwrap();
		assert_eq!(first.len(), 1);

		let embeddings = vec![embedding(TEXT_DIM, 0)];
		store.store_text_blocks(&first, &embeddings).await.unwrap();

		let mut second = Vec::new();
		process_text_file(
			&store,
			"a short note",
			"notes.txt",
			&mut second,
			&config,
			state(false),
		)
		.await
		.unwrap();
		assert!(second.is_empty());

		let mut forced = Vec::new();
		process_text_file(
			&store,
			"a short note",
			"notes.txt",
			&mut forced,
			&config,
			state(true),
		)
		.await
		.unwrap();
		assert_eq!(forced.len(), 1);
	}

	#[tokio::test]
	async fn a_markdown_file_becomes_document_blocks() {
		let (_dir, store) = test_store().await;
		let config = Config::default();
		let contents = "# Title\n\nIntro paragraph.\n\n## Install\n\nRun the installer.\n";

		let mut batch = Vec::new();
		process_markdown_file(
			&store,
			contents,
			"README.md",
			&mut batch,
			&config,
			state(false),
		)
		.await
		.unwrap();
		assert!(!batch.is_empty());
		assert!(batch.iter().all(|b| b.path == "README.md"));
	}

	#[tokio::test]
	async fn markdown_blocks_already_stored_are_skipped_unless_forced() {
		let (_dir, store) = test_store().await;
		let config = Config::default();
		let contents = "# Title\n\nIntro paragraph that is long enough to survive chunking.\n";

		let mut first = Vec::new();
		process_markdown_file(
			&store,
			contents,
			"README.md",
			&mut first,
			&config,
			state(false),
		)
		.await
		.unwrap();
		let embeddings: Vec<_> = (0..first.len()).map(|i| embedding(TEXT_DIM, i)).collect();
		store
			.store_document_blocks(&first, &embeddings)
			.await
			.unwrap();

		let mut second = Vec::new();
		process_markdown_file(
			&store,
			contents,
			"README.md",
			&mut second,
			&config,
			state(false),
		)
		.await
		.unwrap();
		assert!(second.is_empty());

		let mut forced = Vec::new();
		process_markdown_file(
			&store,
			contents,
			"README.md",
			&mut forced,
			&config,
			state(true),
		)
		.await
		.unwrap();
		assert_eq!(forced.len(), first.len());
	}
}
