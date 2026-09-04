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
	use crate::config::RerankerConfig;
	use crate::reranker::*;
	use crate::store::{CodeBlock, CommitBlock, DocumentBlock, TextBlock};

	fn config(enabled: bool, model: &str) -> RerankerConfig {
		RerankerConfig {
			enabled,
			model: model.to_string(),
			top_k_candidates: 20,
			final_top_k: 5,
		}
	}

	fn code_blocks(n: usize) -> Vec<CodeBlock> {
		(0..n)
			.map(|i| CodeBlock {
				path: format!("src/f{i}.rs"),
				language: "rust".to_string(),
				content: format!("fn f{i}() {{}}"),
				symbols: vec![],
				start_line: 1,
				end_line: 2,
				hash: format!("h{i}"),
				distance: None,
			})
			.collect()
	}

	fn text_blocks(n: usize) -> Vec<TextBlock> {
		(0..n)
			.map(|i| TextBlock {
				path: format!("notes{i}.txt"),
				language: "text".to_string(),
				content: format!("note {i}"),
				start_line: 1,
				end_line: 2,
				hash: format!("h{i}"),
				distance: None,
			})
			.collect()
	}

	fn document_blocks(n: usize) -> Vec<DocumentBlock> {
		(0..n)
			.map(|i| DocumentBlock {
				path: format!("doc{i}.md"),
				title: format!("Doc {i}"),
				content: format!("body {i}"),
				context: vec![],
				level: 1,
				start_line: 1,
				end_line: 2,
				hash: format!("h{i}"),
				distance: None,
			})
			.collect()
	}

	fn commit_blocks(n: usize) -> Vec<CommitBlock> {
		(0..n)
			.map(|i| CommitBlock {
				hash: format!("sha{i}"),
				author: "dev".to_string(),
				date: 0,
				message: format!("commit {i}"),
				content: format!("commit {i}"),
				files: "[]".to_string(),
				description: String::new(),
				distance: None,
			})
			.collect()
	}

	#[tokio::test]
	async fn a_disabled_reranker_returns_the_input_untouched() {
		let cfg = config(false, "voyage:rerank-2.5");
		let blocks = code_blocks(3);
		let out = rerank_code_blocks_with_octolib("q", blocks.clone(), &cfg)
			.await
			.unwrap();
		assert_eq!(out.len(), blocks.len());
		assert_eq!(out[0].path, blocks[0].path);
		assert!(out.iter().all(|b| b.distance.is_none()));
	}

	#[tokio::test]
	async fn every_block_type_short_circuits_when_disabled() {
		let cfg = config(false, "voyage:rerank-2.5");
		assert_eq!(
			rerank_text_blocks_with_octolib("q", text_blocks(2), &cfg)
				.await
				.unwrap()
				.len(),
			2
		);
		assert_eq!(
			rerank_doc_blocks_with_octolib("q", document_blocks(2), &cfg)
				.await
				.unwrap()
				.len(),
			2
		);
		assert_eq!(
			rerank_commit_blocks_with_octolib("q", commit_blocks(2), &cfg)
				.await
				.unwrap()
				.len(),
			2
		);
	}

	#[tokio::test]
	async fn an_empty_candidate_list_never_calls_the_provider() {
		// Enabled with a model that would fail to parse: reaching the provider
		// would surface as an error, so Ok(empty) proves the early return.
		let cfg = config(true, "no-colon-here");
		assert!(rerank_code_blocks_with_octolib("q", vec![], &cfg)
			.await
			.unwrap()
			.is_empty());
		assert!(rerank_text_blocks_with_octolib("q", vec![], &cfg)
			.await
			.unwrap()
			.is_empty());
		assert!(rerank_doc_blocks_with_octolib("q", vec![], &cfg)
			.await
			.unwrap()
			.is_empty());
		assert!(rerank_commit_blocks_with_octolib("q", vec![], &cfg)
			.await
			.unwrap()
			.is_empty());
	}

	#[tokio::test]
	async fn a_model_without_a_provider_prefix_is_rejected() {
		let cfg = config(true, "rerank-2.5");
		let err = rerank_code_blocks_with_octolib("q", code_blocks(1), &cfg)
			.await
			.expect_err("a model string without ':' must not reach the provider");
		let message = err.to_string();
		assert!(
			message.contains("Invalid reranker model format"),
			"{message}"
		);
		assert!(message.contains("provider:model"), "{message}");
	}

	#[tokio::test]
	async fn the_format_check_applies_to_every_block_type() {
		let cfg = config(true, "rerank-2.5");
		assert!(rerank_text_blocks_with_octolib("q", text_blocks(1), &cfg)
			.await
			.is_err());
		assert!(
			rerank_doc_blocks_with_octolib("q", document_blocks(1), &cfg)
				.await
				.is_err()
		);
		assert!(
			rerank_commit_blocks_with_octolib("q", commit_blocks(1), &cfg)
				.await
				.is_err()
		);
	}
}
