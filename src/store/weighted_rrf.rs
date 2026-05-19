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

//! Weighted Reciprocal Rank Fusion reranker.
//!
//! LanceDB ships a vanilla `RRFReranker` that treats vector and FTS rankings
//! equally. For code retrieval we often want to tilt the fusion: identifier
//! matches via BM25 carry strong signal for "find me the `parse_remote`
//! function" queries, while dense vectors carry the load for paraphrased
//! intent queries. This reranker exposes per-source weights so the operator
//! can sweep the vector/keyword balance without modifying LanceDB.
//!
//! The fused score for a row that appears at vector-rank `v_i` and FTS-rank
//! `f_i` is `vector_weight / (k + v_i) + keyword_weight / (k + f_i)`. Either
//! term drops when the row is absent from that side. With weights `(1.0, 1.0)`
//! this collapses to the standard RRF defined in Cormack et al. (SIGIR 2009).

use std::collections::BTreeMap;
use std::sync::Arc;

use arrow::array::downcast_array;
use arrow::compute::{sort_to_indices, take};
use arrow_array::{Float32Array, RecordBatch, UInt64Array};
use arrow_schema::{DataType, Field, Schema, SortOptions};
use async_trait::async_trait;
use lance::dataset::ROW_ID;
use lancedb::rerankers::Reranker;

const RELEVANCE_SCORE: &str = "_relevance_score";

/// Reranker that fuses vector and FTS rankings via weighted Reciprocal Rank
/// Fusion. `vector_weight` and `keyword_weight` multiply each source's
/// `1 / (k + rank)` contribution before they are summed.
#[derive(Debug)]
pub struct WeightedRRFReranker {
	/// The standard RRF dampening constant (default 60).
	k: f32,
	/// Weight applied to the vector search contribution.
	vector_weight: f32,
	/// Weight applied to the FTS / BM25 contribution.
	keyword_weight: f32,
}

impl WeightedRRFReranker {
	pub fn new(k: f32, vector_weight: f32, keyword_weight: f32) -> Self {
		Self {
			k: k.max(1.0),
			vector_weight: vector_weight.max(0.0),
			keyword_weight: keyword_weight.max(0.0),
		}
	}
}

#[async_trait]
impl Reranker for WeightedRRFReranker {
	async fn rerank_hybrid(
		&self,
		_query: &str,
		vector_results: RecordBatch,
		fts_results: RecordBatch,
	) -> lancedb::error::Result<RecordBatch> {
		// Compute weighted RRF scores keyed by row id.
		let mut rrf_scores: BTreeMap<u64, f32> = BTreeMap::new();

		if vector_results.num_rows() > 0 {
			let vec_ids = vector_results.column_by_name(ROW_ID).ok_or_else(|| {
				lancedb::error::Error::InvalidInput {
					message: format!("missing {} column on vector results", ROW_ID),
				}
			})?;
			let vec_ids: UInt64Array = downcast_array(&vec_ids);
			for (i, id) in vec_ids.values().iter().enumerate() {
				let score = self.vector_weight / (i as f32 + self.k);
				rrf_scores
					.entry(*id)
					.and_modify(|e| *e += score)
					.or_insert(score);
			}
		}

		if fts_results.num_rows() > 0 {
			let fts_ids = fts_results.column_by_name(ROW_ID).ok_or_else(|| {
				lancedb::error::Error::InvalidInput {
					message: format!("missing {} column on fts results", ROW_ID),
				}
			})?;
			let fts_ids: UInt64Array = downcast_array(&fts_ids);
			for (i, id) in fts_ids.values().iter().enumerate() {
				let score = self.keyword_weight / (i as f32 + self.k);
				rrf_scores
					.entry(*id)
					.and_modify(|e| *e += score)
					.or_insert(score);
			}
		}

		// Merge the two RecordBatches, deduplicating on ROW_ID (same logic the
		// trait's default `merge_results` uses; reimplement locally so we don't
		// pull in lancedb's private API).
		let combined = merge_dedup(vector_results, fts_results)?;

		// Pull row ids in combined order and look up their fused scores.
		let combined_row_ids =
			combined
				.column_by_name(ROW_ID)
				.ok_or_else(|| lancedb::error::Error::InvalidInput {
					message: format!("missing {} on combined batch", ROW_ID),
				})?;
		let combined_row_ids: UInt64Array = downcast_array(combined_row_ids);
		let relevance_scores = Float32Array::from_iter_values(
			combined_row_ids
				.values()
				.iter()
				.map(|id| *rrf_scores.get(id).unwrap_or(&0.0)),
		);

		// Sort by score descending.
		let sort_indices = sort_to_indices(
			&relevance_scores,
			Some(SortOptions {
				descending: true,
				..Default::default()
			}),
			None,
		)
		.map_err(|e| lancedb::error::Error::InvalidInput {
			message: format!("sort failed: {e}"),
		})?;

		let mut columns = combined.columns().to_vec();
		columns.push(Arc::new(relevance_scores));
		let columns: Vec<_> = columns
			.iter()
			.map(|c| {
				take(c, &sort_indices, None).map_err(|e| lancedb::error::Error::InvalidInput {
					message: format!("take failed: {e}"),
				})
			})
			.collect::<lancedb::error::Result<Vec<_>>>()?;

		let mut fields = combined.schema().fields().to_vec();
		fields.push(Arc::new(Field::new(
			RELEVANCE_SCORE,
			DataType::Float32,
			false,
		)));
		let schema = Schema::new(fields);

		RecordBatch::try_new(Arc::new(schema), columns).map_err(|e| {
			lancedb::error::Error::InvalidInput {
				message: format!("rebuild record batch failed: {e}"),
			}
		})
	}
}

fn merge_dedup(
	vector_results: RecordBatch,
	fts_results: RecordBatch,
) -> lancedb::error::Result<RecordBatch> {
	use arrow::array::BooleanArray;
	use arrow::compute::{concat_batches, filter_record_batch};
	use std::collections::BTreeSet;

	let combined = concat_batches(&fts_results.schema(), [vector_results, fts_results].iter())
		.map_err(|e| lancedb::error::Error::InvalidInput {
			message: format!("concat failed: {e}"),
		})?;

	let mut mask = BooleanArray::builder(combined.num_rows());
	let mut seen: BTreeSet<u64> = BTreeSet::new();
	let row_ids =
		combined
			.column_by_name(ROW_ID)
			.ok_or_else(|| lancedb::error::Error::InvalidInput {
				message: format!("missing {} on combined batch", ROW_ID),
			})?;
	let row_ids: UInt64Array = downcast_array(row_ids);
	for id in row_ids.values().iter() {
		mask.append_value(seen.insert(*id));
	}
	filter_record_batch(&combined, &mask.finish()).map_err(|e| {
		lancedb::error::Error::InvalidInput {
			message: format!("filter failed: {e}"),
		}
	})
}
