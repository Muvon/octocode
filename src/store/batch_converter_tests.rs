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
	use crate::store::batch_converter::{extract_embeddings_from_batch, BatchConverter};
	use crate::store::mod_tests::{code_block, commit_block, document_block, text_block};
	use arrow::array::{Array, ArrayRef, Float32Array, RecordBatch, StringArray};
	use arrow_schema::{DataType, Field, Schema};
	use std::sync::Arc;

	const DIM: usize = 4;

	fn converter() -> BatchConverter {
		BatchConverter::new(DIM)
	}

	fn vector(seed: f32) -> Vec<f32> {
		vec![seed, seed + 1.0, seed + 2.0, seed + 3.0]
	}

	/// Attach a `_distance` column so the decoders take the vector-search path.
	fn with_distance(batch: RecordBatch, distances: &[f32]) -> RecordBatch {
		let mut fields = batch.schema().fields().to_vec();
		fields.push(Arc::new(Field::new("_distance", DataType::Float32, false)));
		let mut columns = batch.columns().to_vec();
		columns.push(Arc::new(Float32Array::from(distances.to_vec())) as ArrayRef);
		RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap()
	}

	#[test]
	fn code_blocks_round_trip_through_a_record_batch() {
		let blocks = vec![code_block("src/a.rs", "h1"), code_block("src/b.rs", "h2")];
		let batch = converter()
			.code_block_to_batch(&blocks, &[vector(0.0), vector(1.0)])
			.unwrap();
		assert_eq!(batch.num_rows(), 2);

		let decoded = converter().batch_to_code_blocks(&batch, None).unwrap();
		assert_eq!(decoded.len(), 2);
		assert_eq!(decoded[0].path, "src/a.rs");
		assert_eq!(decoded[0].language, "rust");
		assert_eq!(decoded[0].symbols, vec!["from_h1".to_string()]);
		assert_eq!(decoded[0].hash, "h1");
		// Line numbers are stored as the indexer's 0-based rows and handed back
		// 1-based for display, so the decoded value is deliberately one higher.
		assert_eq!(decoded[0].start_line, blocks[0].start_line + 1);
		assert_eq!(decoded[0].end_line, blocks[0].end_line + 1);
		assert!(decoded[0].distance.is_none());
	}

	#[test]
	fn a_distance_column_is_carried_onto_each_decoded_block() {
		let batch = converter()
			.code_block_to_batch(&[code_block("src/a.rs", "h1")], &[vector(0.0)])
			.unwrap();
		let decoded = converter()
			.batch_to_code_blocks(&with_distance(batch, &[0.25]), None)
			.unwrap();
		assert_eq!(decoded[0].distance, Some(0.25));
	}

	#[test]
	fn text_blocks_round_trip_through_a_record_batch() {
		let batch = converter()
			.text_block_to_batch(&[text_block("notes.txt", "t1")], &[vector(0.0)])
			.unwrap();
		let decoded = converter()
			.batch_to_text_blocks(&with_distance(batch, &[0.5]), None)
			.unwrap();
		assert_eq!(decoded.len(), 1);
		assert_eq!(decoded[0].path, "notes.txt");
		assert_eq!(decoded[0].content, "note t1");
		assert_eq!(decoded[0].distance, Some(0.5));
	}

	#[test]
	fn document_blocks_round_trip_including_their_heading_context() {
		let batch = converter()
			.document_block_to_batch(&[document_block("README.md", "d1")], &[vector(0.0)])
			.unwrap();
		let decoded = converter().batch_to_document_blocks(&batch, None).unwrap();
		assert_eq!(decoded.len(), 1);
		assert_eq!(decoded[0].title, "Title d1");
		assert_eq!(decoded[0].context, vec!["Root".to_string()]);
		assert_eq!(decoded[0].level, 2);
	}

	#[test]
	fn a_document_with_no_heading_context_decodes_to_an_empty_list() {
		let mut block = document_block("README.md", "d1");
		block.context = Vec::new();
		let batch = converter()
			.document_block_to_batch(&[block], &[vector(0.0)])
			.unwrap();
		let decoded = converter().batch_to_document_blocks(&batch, None).unwrap();
		assert!(decoded[0].context.is_empty());
	}

	#[test]
	fn commit_blocks_round_trip_through_a_record_batch() {
		let batch = converter()
			.commit_block_to_batch(&[commit_block("c1")], &[vector(0.0)])
			.unwrap();
		let decoded = converter().batch_to_commit_blocks(&batch).unwrap();
		assert_eq!(decoded.len(), 1);
		assert_eq!(decoded[0].hash, "c1");
		assert_eq!(decoded[0].author, "dev");
		assert_eq!(decoded[0].date, 1_700_000_000);
		assert_eq!(decoded[0].files, "[\"src/a.rs\"]");
	}

	#[test]
	fn every_encoder_rejects_an_empty_input() {
		let c = converter();
		assert!(c.code_block_to_batch(&[], &[]).is_err());
		assert!(c.text_block_to_batch(&[], &[]).is_err());
		assert!(c.document_block_to_batch(&[], &[]).is_err());
		assert!(c.commit_block_to_batch(&[], &[]).is_err());
	}

	#[test]
	fn every_encoder_rejects_a_block_embedding_count_mismatch() {
		let c = converter();
		let err = c
			.code_block_to_batch(&[code_block("a", "h")], &[])
			.unwrap_err()
			.to_string();
		assert!(err.contains("must match"), "{err}");

		assert!(c.text_block_to_batch(&[text_block("a", "h")], &[]).is_err());
		assert!(c
			.document_block_to_batch(&[document_block("a", "h")], &[])
			.is_err());
		assert!(c.commit_block_to_batch(&[commit_block("h")], &[]).is_err());
	}

	#[test]
	fn every_encoder_rejects_a_wrong_width_embedding() {
		let c = converter();
		let wrong = vec![vec![0.0f32; DIM + 1]];
		let err = c
			.code_block_to_batch(&[code_block("a", "h")], &wrong)
			.unwrap_err()
			.to_string();
		assert!(err.contains("dimension 5 but expected 4"), "{err}");

		assert!(c
			.text_block_to_batch(&[text_block("a", "h")], &wrong)
			.is_err());
		assert!(c
			.document_block_to_batch(&[document_block("a", "h")], &wrong)
			.is_err());
		assert!(c
			.commit_block_to_batch(&[commit_block("h")], &wrong)
			.is_err());
	}

	#[test]
	fn decoding_a_batch_that_is_missing_a_column_is_an_error() {
		let schema = Arc::new(Schema::new(vec![Field::new("path", DataType::Utf8, false)]));
		let batch = RecordBatch::try_new(
			schema,
			vec![Arc::new(StringArray::from(vec!["src/a.rs"])) as ArrayRef],
		)
		.unwrap();

		let c = converter();
		assert!(c.batch_to_code_blocks(&batch, None).is_err());
		assert!(c.batch_to_text_blocks(&batch, None).is_err());
		assert!(c.batch_to_document_blocks(&batch, None).is_err());
		assert!(c.batch_to_commit_blocks(&batch).is_err());
	}

	#[test]
	fn embeddings_are_recoverable_from_an_encoded_batch() {
		let batch = converter()
			.code_block_to_batch(
				&[code_block("src/a.rs", "h1"), code_block("src/b.rs", "h2")],
				&[vector(0.0), vector(10.0)],
			)
			.unwrap();
		let recovered = extract_embeddings_from_batch(&batch).expect("embedding column");
		assert_eq!(recovered, vec![vector(0.0), vector(10.0)]);
	}

	#[test]
	fn a_batch_without_an_embedding_column_yields_no_embeddings() {
		let schema = Arc::new(Schema::new(vec![Field::new("path", DataType::Utf8, false)]));
		let batch = RecordBatch::try_new(
			schema,
			vec![Arc::new(StringArray::from(vec!["src/a.rs"])) as ArrayRef],
		)
		.unwrap();
		assert!(extract_embeddings_from_batch(&batch).is_none());
	}

	#[test]
	fn a_mistyped_embedding_column_yields_no_embeddings() {
		let schema = Arc::new(Schema::new(vec![Field::new(
			"embedding",
			DataType::Utf8,
			false,
		)]));
		let batch = RecordBatch::try_new(
			schema,
			vec![Arc::new(StringArray::from(vec!["not a vector"])) as ArrayRef],
		)
		.unwrap();
		assert!(extract_embeddings_from_batch(&batch).is_none());
	}

	#[test]
	fn symbols_that_are_not_valid_json_decode_to_an_empty_list() {
		// The symbols column stores serialized JSON; a corrupted value must not
		// take down the whole batch.
		let batch = converter()
			.code_block_to_batch(&[code_block("src/a.rs", "h1")], &[vector(0.0)])
			.unwrap();
		let mut columns = batch.columns().to_vec();
		let symbols_index = batch.schema().index_of("symbols").unwrap();
		columns[symbols_index] = Arc::new(StringArray::from(vec!["{not json"])) as ArrayRef;
		let corrupted = RecordBatch::try_new(batch.schema(), columns).unwrap();

		let decoded = converter().batch_to_code_blocks(&corrupted, None).unwrap();
		assert!(decoded[0].symbols.is_empty());
	}

	#[test]
	fn a_block_with_no_symbols_decodes_to_an_empty_list() {
		let mut block = code_block("src/a.rs", "h1");
		block.symbols = Vec::new();
		let batch = converter()
			.code_block_to_batch(&[block], &[vector(0.0)])
			.unwrap();
		let decoded = converter().batch_to_code_blocks(&batch, None).unwrap();
		assert!(decoded[0].symbols.is_empty());
	}

	#[test]
	fn each_encoded_row_gets_its_own_generated_id() {
		let batch = converter()
			.code_block_to_batch(
				&[code_block("src/a.rs", "h1"), code_block("src/a.rs", "h1")],
				&[vector(0.0), vector(0.0)],
			)
			.unwrap();
		let ids = batch
			.column_by_name("id")
			.unwrap()
			.as_any()
			.downcast_ref::<StringArray>()
			.unwrap();
		assert_ne!(ids.value(0), ids.value(1));
		assert!(!ids.value(0).is_empty());
	}
}
