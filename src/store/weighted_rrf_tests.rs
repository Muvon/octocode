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
	use crate::store::weighted_rrf::WeightedRRFReranker;
	use arrow::array::downcast_array;
	use arrow_array::{Float32Array, RecordBatch, StringArray, UInt64Array};
	use arrow_schema::{DataType, Field, Schema};
	use lance::dataset::ROW_ID;
	use lancedb::rerankers::Reranker;
	use std::sync::Arc;

	fn schema() -> Arc<Schema> {
		Arc::new(Schema::new(vec![
			Field::new(ROW_ID, DataType::UInt64, false),
			Field::new("text", DataType::Utf8, false),
		]))
	}

	fn batch(ids: &[u64]) -> RecordBatch {
		let texts: Vec<String> = ids.iter().map(|id| format!("row-{id}")).collect();
		RecordBatch::try_new(
			schema(),
			vec![
				Arc::new(UInt64Array::from(ids.to_vec())),
				Arc::new(StringArray::from(texts)),
			],
		)
		.unwrap()
	}

	fn row_ids(batch: &RecordBatch) -> Vec<u64> {
		let column = batch.column_by_name(ROW_ID).expect("row id column");
		let ids: UInt64Array = downcast_array(column);
		ids.values().to_vec()
	}

	fn scores(batch: &RecordBatch) -> Vec<f32> {
		let column = batch
			.column_by_name("_relevance_score")
			.expect("relevance column");
		let values: Float32Array = downcast_array(column);
		values.values().to_vec()
	}

	#[tokio::test]
	async fn rows_present_in_both_rankings_outrank_single_source_hits() {
		let reranker = WeightedRRFReranker::new(60.0, 2.0, 1.0);
		let fused = reranker
			.rerank_hybrid("q", batch(&[1, 2, 3]), batch(&[3, 4]))
			.await
			.unwrap();

		// Row 3 is the only one scoring from both sides, so it wins outright.
		assert_eq!(row_ids(&fused), vec![3, 1, 2, 4]);
		// Every input row survives exactly once.
		assert_eq!(fused.num_rows(), 4);

		let fused_scores = scores(&fused);
		assert!(fused_scores.windows(2).all(|w| w[0] >= w[1]));
		let expected_top = 2.0 / 62.0 + 1.0 / 60.0;
		assert!((fused_scores[0] - expected_top).abs() < 1e-6);
	}

	#[tokio::test]
	async fn a_zero_keyword_weight_ignores_the_fts_ranking_entirely() {
		let reranker = WeightedRRFReranker::new(60.0, 1.0, 0.0);
		let fused = reranker
			.rerank_hybrid("q", batch(&[1, 2]), batch(&[9]))
			.await
			.unwrap();

		// The FTS-only row is still carried through, but scores nothing.
		assert_eq!(row_ids(&fused), vec![1, 2, 9]);
		assert_eq!(scores(&fused)[2], 0.0);
	}

	#[tokio::test]
	async fn negative_weights_and_a_tiny_k_are_clamped() {
		// k below 1 would blow up the leading term; negative weights would invert
		// the ranking. Both are clamped at construction.
		let reranker = WeightedRRFReranker::new(0.0, -5.0, -5.0);
		let fused = reranker
			.rerank_hybrid("q", batch(&[1]), batch(&[2]))
			.await
			.unwrap();
		assert!(scores(&fused).iter().all(|s| *s == 0.0));
	}

	#[tokio::test]
	async fn an_empty_side_still_returns_the_other_sides_rows() {
		let reranker = WeightedRRFReranker::new(60.0, 1.0, 1.0);

		let vector_only = reranker
			.rerank_hybrid("q", batch(&[5, 6]), batch(&[]))
			.await
			.unwrap();
		assert_eq!(row_ids(&vector_only), vec![5, 6]);

		let fts_only = reranker
			.rerank_hybrid("q", batch(&[]), batch(&[7, 8]))
			.await
			.unwrap();
		assert_eq!(row_ids(&fts_only), vec![7, 8]);
	}

	#[tokio::test]
	async fn two_empty_sides_produce_an_empty_result() {
		let fused = WeightedRRFReranker::new(60.0, 1.0, 1.0)
			.rerank_hybrid("q", batch(&[]), batch(&[]))
			.await
			.unwrap();
		assert_eq!(fused.num_rows(), 0);
		assert!(fused.column_by_name("_relevance_score").is_some());
	}

	#[tokio::test]
	async fn a_batch_without_a_row_id_column_is_rejected() {
		let no_id_schema = Arc::new(Schema::new(vec![Field::new("text", DataType::Utf8, false)]));
		let no_id = RecordBatch::try_new(
			no_id_schema,
			vec![Arc::new(StringArray::from(vec!["a".to_string()]))],
		)
		.unwrap();

		let reranker = WeightedRRFReranker::new(60.0, 1.0, 1.0);
		let err = reranker
			.rerank_hybrid("q", no_id, batch(&[1]))
			.await
			.expect_err("a batch without row ids cannot be fused");
		assert!(err.to_string().contains(ROW_ID), "{err}");
	}
}
