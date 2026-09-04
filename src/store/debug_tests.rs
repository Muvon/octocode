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
	use crate::store::mod_tests::{
		code_block, document_block, embedding, test_store, text_block, CODE_DIM, TEXT_DIM,
	};

	#[tokio::test]
	async fn listing_an_empty_index_reports_no_files() {
		let (_dir, store) = test_store().await;
		store.list_indexed_files().await.unwrap();
	}

	#[tokio::test]
	async fn listing_reports_files_from_every_populated_table() {
		let (_dir, store) = test_store().await;
		store
			.store_code_blocks(
				&[code_block("src/a.rs", "h1"), code_block("src/a.rs", "h2")],
				&[embedding(CODE_DIM, 0), embedding(CODE_DIM, 1)],
			)
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

		store.list_indexed_files().await.unwrap();
	}

	#[tokio::test]
	async fn showing_chunks_for_an_unindexed_file_is_not_an_error() {
		let (_dir, store) = test_store().await;
		store.show_file_chunks("src/missing.rs").await.unwrap();
	}

	#[tokio::test]
	async fn showing_chunks_prints_every_block_for_the_file() {
		let (_dir, store) = test_store().await;
		store
			.store_code_blocks(
				&[code_block("src/a.rs", "h1"), code_block("src/b.rs", "h2")],
				&[embedding(CODE_DIM, 0), embedding(CODE_DIM, 1)],
			)
			.await
			.unwrap();
		store.show_file_chunks("src/a.rs").await.unwrap();
	}

	#[tokio::test]
	async fn a_path_containing_a_quote_cannot_break_the_predicate() {
		let (_dir, store) = test_store().await;
		let path = "src/it's.rs";
		store
			.store_code_blocks(&[code_block(path, "h1")], &[embedding(CODE_DIM, 0)])
			.await
			.unwrap();
		store.show_file_chunks(path).await.unwrap();
	}
}
