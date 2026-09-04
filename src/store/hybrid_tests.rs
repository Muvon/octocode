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
	use crate::store::HybridSearchQuery;

	#[test]
	fn test_hybrid_search_query_validation_vector_and_keywords() {
		let valid_query = HybridSearchQuery {
			vector_query: Some(vec![0.1, 0.2, 0.3]),
			keywords: Some("test query".to_string()),
			vector_weight: 0.7,
			keyword_weight: 0.3,
			limit: 10,
			min_relevance: Some(0.5),
			language_filter: None,
		};
		assert!(valid_query.validate().is_ok());
	}

	#[test]
	fn test_hybrid_search_query_validation_vector_only() {
		let valid_query = HybridSearchQuery {
			vector_query: Some(vec![0.1, 0.2, 0.3]),
			keywords: None,
			vector_weight: 0.7,
			keyword_weight: 0.3,
			limit: 10,
			min_relevance: Some(0.5),
			language_filter: None,
		};
		assert!(valid_query.validate().is_ok());
	}

	#[test]
	fn test_hybrid_search_query_validation_keywords_only() {
		let valid_query = HybridSearchQuery {
			vector_query: None,
			keywords: Some("test query".to_string()),
			vector_weight: 0.7,
			keyword_weight: 0.3,
			limit: 10,
			min_relevance: Some(0.5),
			language_filter: None,
		};
		assert!(valid_query.validate().is_ok());
	}

	#[test]
	fn test_hybrid_search_query_validation_invalid_weights() {
		let invalid_weights = HybridSearchQuery {
			vector_query: Some(vec![0.1, 0.2, 0.3]),
			keywords: None,
			vector_weight: 1.5, // Invalid: > 1.0
			keyword_weight: 0.3,
			limit: 10,
			min_relevance: Some(0.5),
			language_filter: None,
		};
		assert!(invalid_weights.validate().is_err());
	}

	#[test]
	fn test_hybrid_search_query_validation_no_signals() {
		let no_signals = HybridSearchQuery {
			vector_query: None,
			keywords: None,
			vector_weight: 0.7,
			keyword_weight: 0.3,
			limit: 10,
			min_relevance: Some(0.5),
			language_filter: None,
		};
		assert!(no_signals.validate().is_err());
	}

	#[test]
	fn test_hybrid_search_query_validation_negative_weights() {
		let negative_weight = HybridSearchQuery {
			vector_query: Some(vec![0.1, 0.2, 0.3]),
			keywords: None,
			vector_weight: -0.5, // Invalid: < 0.0
			keyword_weight: 0.3,
			limit: 10,
			min_relevance: Some(0.5),
			language_filter: None,
		};
		assert!(negative_weight.validate().is_err());
	}

	// --- cosine_similarity -------------------------------------------------

	#[test]
	fn cosine_similarity_scores_identical_orthogonal_and_opposite_vectors() {
		use crate::store::cosine_similarity;
		assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]), 1.0);
		assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
		assert_eq!(cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]), -1.0);
		// Magnitude does not matter, only direction.
		assert_eq!(cosine_similarity(&[3.0, 0.0], &[9.0, 0.0]), 1.0);
	}

	#[test]
	fn cosine_similarity_is_zero_for_mismatched_lengths_or_a_zero_vector() {
		use crate::store::cosine_similarity;
		assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
		assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
		assert_eq!(cosine_similarity(&[], &[]), 0.0);
	}

	// --- hybrid_search -----------------------------------------------------

	mod search {
		use crate::store::mod_tests::{
			code_block, document_block, embedding, test_store, text_block, CODE_DIM, TEXT_DIM,
		};
		use crate::store::{CodeBlock, DocumentBlock, HybridSearchQuery, Store, TextBlock};

		fn query(vector: Option<Vec<f32>>, keywords: Option<&str>) -> HybridSearchQuery {
			HybridSearchQuery {
				vector_query: vector,
				keywords: keywords.map(str::to_string),
				vector_weight: 0.5,
				keyword_weight: 0.5,
				limit: 10,
				min_relevance: None,
				language_filter: None,
			}
		}

		/// Three rust code blocks whose embeddings are the unit vectors at
		/// indices 1, 2 and 3, so a query vector picks a known winner.
		async fn store_with_code() -> (tempfile::TempDir, Store) {
			let (dir, store) = test_store().await;
			let blocks: Vec<CodeBlock> = (1..=3)
				.map(|i| {
					let mut block = code_block(&format!("src/f{i}.rs"), &format!("h{i}"));
					block.content = format!("fn alpha_{i}() {{ beta_{i}() }}");
					block
				})
				.collect();
			let embeddings: Vec<Vec<f32>> =
				(1..=3).map(|i| embedding(CODE_DIM, i as usize)).collect();
			store
				.store_code_blocks(&blocks, &embeddings)
				.await
				.expect("store code blocks");
			(dir, store)
		}

		#[tokio::test]
		async fn a_missing_table_yields_no_results_rather_than_an_error() {
			let (_dir, store) = test_store().await;
			let out: Vec<CodeBlock> = store
				.hybrid_search(&query(Some(embedding(CODE_DIM, 1)), None))
				.await
				.expect("search");
			assert!(out.is_empty());
		}

		#[tokio::test]
		async fn an_invalid_query_is_rejected_before_any_table_is_touched() {
			let (_dir, store) = test_store().await;
			let err = store
				.hybrid_search::<CodeBlock>(&query(None, None))
				.await
				.expect_err("no signal must be rejected");
			assert!(err.to_string().contains("Invalid hybrid query"), "{err}");
		}

		#[tokio::test]
		async fn a_vector_only_query_ranks_the_nearest_block_first() {
			let (_dir, store) = store_with_code().await;
			let out: Vec<CodeBlock> = store
				.hybrid_search(&query(Some(embedding(CODE_DIM, 2)), None))
				.await
				.expect("search");
			assert_eq!(out.len(), 3);
			assert_eq!(out[0].path, "src/f2.rs");
			// The exact-match row sits at cosine distance 0.
			assert!(out[0].distance.expect("distance") < 1e-5);
		}

		#[tokio::test]
		async fn a_keyword_only_query_finds_the_block_whose_content_matches() {
			let (_dir, store) = store_with_code().await;
			let out: Vec<CodeBlock> = store
				.hybrid_search(&query(None, Some("alpha_3")))
				.await
				.expect("search");
			// The tokenizer splits `alpha_3` into `alpha` and `3`, so every block
			// sharing the `alpha` token is a candidate; BM25 ranking is what
			// separates them, and the exact match has to come first.
			assert_eq!(out.first().expect("a hit").path, "src/f3.rs");
		}

		#[tokio::test]
		async fn a_hybrid_query_returns_the_recomputed_cosine_distance_not_an_rrf_score() {
			let (_dir, store) = store_with_code().await;
			let out: Vec<CodeBlock> = store
				.hybrid_search(&query(Some(embedding(CODE_DIM, 1)), Some("alpha_1")))
				.await
				.expect("search");
			assert!(!out.is_empty());
			let exact = out
				.iter()
				.find(|b| b.path == "src/f1.rs")
				.expect("the matching block");
			// A recomputed cosine distance, not the RRF relevance score.
			assert!(exact.distance.expect("distance") < 1e-5);
		}

		#[tokio::test]
		async fn the_limit_caps_the_result_count_on_every_arm() {
			let (_dir, store) = store_with_code().await;
			let mut q = query(Some(embedding(CODE_DIM, 1)), None);
			q.limit = 2;
			let vector_only: Vec<CodeBlock> = store.hybrid_search(&q).await.expect("vector");
			assert_eq!(vector_only.len(), 2);

			let mut q = query(None, Some("beta_1 beta_2 beta_3"));
			q.limit = 1;
			let keyword_only: Vec<CodeBlock> = store.hybrid_search(&q).await.expect("keyword");
			assert_eq!(keyword_only.len(), 1);

			let mut q = query(Some(embedding(CODE_DIM, 1)), Some("beta_1 beta_2 beta_3"));
			q.limit = 2;
			let hybrid: Vec<CodeBlock> = store.hybrid_search(&q).await.expect("hybrid");
			assert!(hybrid.len() <= 2);
		}

		#[tokio::test]
		async fn a_language_filter_excludes_rows_of_every_other_language() {
			let (_dir, store) = store_with_code().await;
			let mut q = query(Some(embedding(CODE_DIM, 1)), None);
			q.language_filter = Some("python".to_string());
			let none: Vec<CodeBlock> = store.hybrid_search(&q).await.expect("search");
			assert!(none.is_empty());

			q.language_filter = Some("rust".to_string());
			let all: Vec<CodeBlock> = store.hybrid_search(&q).await.expect("search");
			assert_eq!(all.len(), 3);
		}

		#[tokio::test]
		async fn min_relevance_drops_rows_beyond_the_matching_distance() {
			let (_dir, store) = store_with_code().await;
			// The query vector is orthogonal to every stored row except f1, so
			// demanding near-perfect relevance leaves only f1.
			let mut q = query(Some(embedding(CODE_DIM, 1)), None);
			q.min_relevance = Some(0.99);
			let out: Vec<CodeBlock> = store.hybrid_search(&q).await.expect("search");
			assert_eq!(
				out.iter().map(|b| b.path.as_str()).collect::<Vec<_>>(),
				["src/f1.rs"]
			);
		}

		#[tokio::test]
		async fn text_and_document_blocks_search_through_the_same_generic_path() {
			let (_dir, store) = test_store().await;
			store
				.store_text_blocks(
					&[text_block("notes.txt", "alpha")],
					&[embedding(TEXT_DIM, 4)],
				)
				.await
				.expect("store text");
			store
				.store_document_blocks(
					&[document_block("README.md", "alpha")],
					&[embedding(TEXT_DIM, 5)],
				)
				.await
				.expect("store docs");

			let texts: Vec<TextBlock> = store
				.hybrid_search(&query(Some(embedding(TEXT_DIM, 4)), None))
				.await
				.expect("text search");
			assert_eq!(
				texts.iter().map(|b| b.path.as_str()).collect::<Vec<_>>(),
				["notes.txt"]
			);

			let docs: Vec<DocumentBlock> = store
				.hybrid_search(&query(None, Some("alpha")))
				.await
				.expect("doc search");
			assert_eq!(
				docs.iter().map(|b| b.path.as_str()).collect::<Vec<_>>(),
				["README.md"]
			);
		}

		#[tokio::test]
		async fn blocks_are_readable_by_exact_path_up_to_a_limit() {
			let (_dir, store) = store_with_code().await;
			let hits = store
				.get_code_blocks_by_path("src/f2.rs", 10)
				.await
				.expect("by path");
			assert_eq!(hits.len(), 1);
			assert_eq!(hits[0].hash, "h2");

			assert!(store
				.get_code_blocks_by_path("src/missing.rs", 10)
				.await
				.expect("by path")
				.is_empty());
		}

		#[tokio::test]
		async fn reading_by_path_from_a_store_with_no_table_is_empty() {
			let (_dir, store) = test_store().await;
			assert!(store
				.get_code_blocks_by_path("src/f1.rs", 10)
				.await
				.expect("by path")
				.is_empty());
		}
	}
}
