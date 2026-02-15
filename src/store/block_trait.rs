// Copyright 2025 Muvon Un Limited
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

//! Generic block type trait for reducing code duplication in store operations

use anyhow::Result;
use arrow_array::RecordBatch;

use super::{CodeBlock, DocumentBlock, TextBlock};

/// Trait for block types to enable generic store operations
pub trait BlockType: Clone + Send + Sync + 'static {
	/// The table name for this block type
	const TABLE_NAME: &'static str;

	/// Convert blocks to Arrow RecordBatch
	fn to_batch(blocks: &[Self], embeddings: &[Vec<f32>], vector_dim: usize)
		-> Result<RecordBatch>;

	/// Convert Arrow RecordBatch to blocks
	fn from_batch(batch: &RecordBatch) -> Result<Vec<Self>>;

	/// Get the distance field from a block (for sorting)
	fn distance(&self) -> Option<f32>;

	/// Set the distance field on a block
	fn set_distance(&mut self, distance: f32);
}

impl BlockType for CodeBlock {
	const TABLE_NAME: &'static str = "code_blocks";

	fn to_batch(
		blocks: &[Self],
		embeddings: &[Vec<f32>],
		vector_dim: usize,
	) -> Result<RecordBatch> {
		super::batch_converter::BatchConverter::new(vector_dim)
			.code_block_to_batch(blocks, embeddings)
	}

	fn from_batch(batch: &RecordBatch) -> Result<Vec<Self>> {
		super::batch_converter::BatchConverter::new(0).batch_to_code_blocks(batch, None)
	}

	fn distance(&self) -> Option<f32> {
		self.distance
	}

	fn set_distance(&mut self, distance: f32) {
		self.distance = Some(distance);
	}
}

impl BlockType for TextBlock {
	const TABLE_NAME: &'static str = "text_blocks";

	fn to_batch(
		blocks: &[Self],
		embeddings: &[Vec<f32>],
		vector_dim: usize,
	) -> Result<RecordBatch> {
		super::batch_converter::BatchConverter::new(vector_dim)
			.text_block_to_batch(blocks, embeddings)
	}

	fn from_batch(batch: &RecordBatch) -> Result<Vec<Self>> {
		super::batch_converter::BatchConverter::new(0).batch_to_text_blocks(batch, None)
	}

	fn distance(&self) -> Option<f32> {
		self.distance
	}

	fn set_distance(&mut self, distance: f32) {
		self.distance = Some(distance);
	}
}

impl BlockType for DocumentBlock {
	const TABLE_NAME: &'static str = "document_blocks";

	fn to_batch(
		blocks: &[Self],
		embeddings: &[Vec<f32>],
		vector_dim: usize,
	) -> Result<RecordBatch> {
		super::batch_converter::BatchConverter::new(vector_dim)
			.document_block_to_batch(blocks, embeddings)
	}

	fn from_batch(batch: &RecordBatch) -> Result<Vec<Self>> {
		super::batch_converter::BatchConverter::new(0).batch_to_document_blocks(batch, None)
	}

	fn distance(&self) -> Option<f32> {
		self.distance
	}

	fn set_distance(&mut self, distance: f32) {
		self.distance = Some(distance);
	}
}
