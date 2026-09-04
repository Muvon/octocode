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
	use crate::indexer::contextual::FileContextMap;
	use crate::state::create_shared_state;
	use crate::store::mod_tests::{embedding, test_store, CODE_DIM, TEXT_DIM};
	use crate::store::{CodeBlock, DocumentBlock, Store, TextBlock};

	const RUST_SOURCE: &str = "\
use std::fs;

pub fn alpha() -> u32 {
	let first = 1;
	let second = first + 1;
	first + second
}

pub fn beta() -> u32 {
	let first = 2;
	let second = first + 2;
	first + second
}
";

	fn state(force_reindex: bool) -> crate::state::SharedState {
		let state = create_shared_state();
		state.write().force_reindex = force_reindex;
		state
	}

	async fn process_rust(
		store: &Store,
		config: &Config,
		contents: &str,
		force_reindex: bool,
	) -> (Vec<CodeBlock>, FileContextMap) {
		let ctx = ProcessFileContext {
			store,
			config,
			state: state(force_reindex),
		};
		let mut batch = Vec::new();
		let mut all_blocks = Vec::new();
		let mut file_context = FileContextMap::new();
		process_file_differential(
			&ctx,
			contents,
			"src/lib.rs",
			"rust",
			&mut batch,
			&mut [],
			&mut all_blocks,
			&mut file_context,
		)
		.await
		.expect("processing must succeed");
		(batch, file_context)
	}

	#[test]
	fn duplicate_entries_are_collapsed_keeping_the_first_occurrence() {
		let mut values = vec![
			"b".to_string(),
			"a".to_string(),
			"b".to_string(),
			"c".to_string(),
			"a".to_string(),
		];
		dedup_in_place(&mut values);
		assert_eq!(values, vec!["b", "a", "c"]);
	}

	#[test]
	fn the_whole_tree_is_walked_for_imports_and_exports() {
		let lang = crate::indexer::languages::get_language("rust").unwrap();
		let mut parser = tree_sitter::Parser::new();
		parser.set_language(&lang.get_ts_language()).unwrap();
		let source = "use std::fs;\nuse std::fs;\npub fn alpha() {}\n";
		let tree = parser.parse(source, None).unwrap();

		let (imports, exports) = walk_for_imports_exports(tree.root_node(), source, lang.as_ref());
		// The root node is never an import itself, so this only passes if the walk
		// descends; the repeated `use` must appear once.
		assert_eq!(imports.iter().filter(|i| i.contains("fs")).count(), 1);
		assert!(exports.iter().any(|e| e == "alpha"), "got {exports:?}");
	}

	#[tokio::test]
	async fn a_new_code_file_produces_one_block_per_region() {
		let (_dir, store) = test_store().await;
		let (batch, _) = process_rust(&store, &Config::default(), RUST_SOURCE, false).await;
		assert!(batch.len() >= 2, "got {} blocks", batch.len());
		assert!(batch.iter().all(|b| b.path == "src/lib.rs"));
		assert!(batch.iter().all(|b| b.language == "rust"));
		assert!(batch.iter().all(|b| !b.hash.is_empty()));
	}

	#[tokio::test]
	async fn already_stored_blocks_are_not_re_emitted() {
		let (_dir, store) = test_store().await;
		let config = Config::default();

		let (first, _) = process_rust(&store, &config, RUST_SOURCE, false).await;
		let embeddings: Vec<_> = (0..first.len()).map(|i| embedding(CODE_DIM, i)).collect();
		store.store_code_blocks(&first, &embeddings).await.unwrap();

		let (second, _) = process_rust(&store, &config, RUST_SOURCE, false).await;
		assert!(second.is_empty(), "got {second:?}");
	}

	#[tokio::test]
	async fn a_forced_reindex_re_emits_everything() {
		let (_dir, store) = test_store().await;
		let config = Config::default();

		let (first, _) = process_rust(&store, &config, RUST_SOURCE, false).await;
		let embeddings: Vec<_> = (0..first.len()).map(|i| embedding(CODE_DIM, i)).collect();
		store.store_code_blocks(&first, &embeddings).await.unwrap();

		let (forced, _) = process_rust(&store, &config, RUST_SOURCE, true).await;
		assert_eq!(forced.len(), first.len());
	}

	#[tokio::test]
	async fn file_context_is_only_built_when_contextual_descriptions_are_enabled() {
		let (_dir, store) = test_store().await;

		let (_, off) = process_rust(&store, &Config::default(), RUST_SOURCE, false).await;
		assert!(
			off.is_empty(),
			"the second AST walk must stay off by default"
		);

		let mut config = Config::default();
		config.index.contextual_descriptions = true;
		let (_, on) = process_rust(&store, &config, RUST_SOURCE, false).await;
		let context = on.get("src/lib.rs").expect("context for the file");
		assert!(!context.exports.is_empty());
		assert!(!context.all_symbols.is_empty());
	}

	#[tokio::test]
	async fn an_unsupported_language_is_skipped_silently() {
		let (_dir, store) = test_store().await;
		let config = Config::default();
		let ctx = ProcessFileContext {
			store: &store,
			config: &config,
			state: state(false),
		};
		let mut batch = Vec::new();
		let mut all_blocks = Vec::new();
		let mut file_context = FileContextMap::new();
		process_file_differential(
			&ctx,
			"whatever",
			"a.zig",
			"zig",
			&mut batch,
			&mut [],
			&mut all_blocks,
			&mut file_context,
		)
		.await
		.expect("an unknown language must not be an error");
		assert!(batch.is_empty());
	}

	#[tokio::test]
	async fn text_files_are_chunked_into_blocks() {
		let (_dir, store) = test_store().await;
		let mut config = Config::default();
		config.index.chunk_size = 40;
		config.index.chunk_overlap = 0;

		let contents: String = (1..=30)
			.map(|i| format!("note line {i}"))
			.collect::<Vec<_>>()
			.join("\n");

		let mut batch: Vec<TextBlock> = Vec::new();
		process_text_file_differential(
			&store,
			&contents,
			"notes.txt",
			&mut batch,
			&config,
			state(false),
		)
		.await
		.unwrap();

		assert!(batch.len() > 1, "long text must be chunked");
		assert!(batch.iter().all(|b| b.path == "notes.txt"));
		assert!(batch.iter().all(|b| b.language == "text"));
		let hashes: std::collections::HashSet<_> = batch.iter().map(|b| &b.hash).collect();
		assert_eq!(hashes.len(), batch.len(), "chunk hashes must be unique");
	}

	#[tokio::test]
	async fn stale_text_chunks_are_removed_when_the_file_shrinks() {
		let (_dir, store) = test_store().await;
		let mut config = Config::default();
		config.index.chunk_size = 40;
		config.index.chunk_overlap = 0;

		let long: String = (1..=30)
			.map(|i| format!("note line {i}"))
			.collect::<Vec<_>>()
			.join("\n");
		let mut batch = Vec::new();
		process_text_file_differential(
			&store,
			&long,
			"notes.txt",
			&mut batch,
			&config,
			state(false),
		)
		.await
		.unwrap();
		let embeddings: Vec<_> = (0..batch.len()).map(|i| embedding(TEXT_DIM, i)).collect();
		store.store_text_blocks(&batch, &embeddings).await.unwrap();
		let stored = store
			.get_table_row_count(crate::store::tables::TEXT_BLOCKS)
			.await
			.unwrap();
		assert!(stored > 1);

		let mut batch = Vec::new();
		process_text_file_differential(
			&store,
			"only one line now",
			"notes.txt",
			&mut batch,
			&config,
			state(false),
		)
		.await
		.unwrap();

		let remaining = store
			.get_table_row_count(crate::store::tables::TEXT_BLOCKS)
			.await
			.unwrap();
		assert!(remaining < stored, "obsolete chunks must be deleted");
	}

	#[tokio::test]
	async fn a_forced_text_reindex_keeps_the_existing_rows() {
		let (_dir, store) = test_store().await;
		let config = Config::default();
		let mut batch = Vec::new();
		process_text_file_differential(
			&store,
			"a line",
			"notes.txt",
			&mut batch,
			&config,
			state(true),
		)
		.await
		.unwrap();
		assert_eq!(batch.len(), 1);
	}

	#[tokio::test]
	async fn markdown_files_become_document_blocks_per_section() {
		let (_dir, store) = test_store().await;
		let config = Config::default();
		let contents = "# Title\n\nIntro paragraph.\n\n## Install\n\nRun the installer.\n\n## Usage\n\nCall the binary.\n";

		let mut batch: Vec<DocumentBlock> = Vec::new();
		process_markdown_file_differential(
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
		assert!(batch.iter().any(|b| b.title.contains("Install")));
	}

	#[tokio::test]
	async fn markdown_blocks_already_stored_are_not_re_emitted() {
		let (_dir, store) = test_store().await;
		let config = Config::default();
		let contents = "# Title\n\nIntro paragraph.\n\n## Install\n\nRun the installer.\n";

		let mut first = Vec::new();
		process_markdown_file_differential(
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
		process_markdown_file_differential(
			&store,
			contents,
			"README.md",
			&mut second,
			&config,
			state(false),
		)
		.await
		.unwrap();
		assert!(second.is_empty(), "got {second:?}");
	}
}
