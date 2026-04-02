//! Generic block type trait for reducing code duplication in store operations

use anyhow::Result;
use arrow_array::RecordBatch;

use super::{CodeBlock, CommitBlock, DocumentBlock, TextBlock};

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

	/// Get unique hash for the block (used for deduplication in hybrid search)
	fn get_hash(&self) -> String;
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

	fn get_hash(&self) -> String {
		self.hash.clone()
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

	fn get_hash(&self) -> String {
		self.hash.clone()
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

	fn get_hash(&self) -> String {
		self.hash.clone()
	}
}

impl BlockType for CommitBlock {
	const TABLE_NAME: &'static str = "commit_blocks";

	fn to_batch(
		blocks: &[Self],
		embeddings: &[Vec<f32>],
		vector_dim: usize,
	) -> Result<RecordBatch> {
		super::batch_converter::BatchConverter::new(vector_dim)
			.commit_block_to_batch(blocks, embeddings)
	}

	fn from_batch(batch: &RecordBatch) -> Result<Vec<Self>> {
		super::batch_converter::BatchConverter::new(0).batch_to_commit_blocks(batch)
	}

	fn distance(&self) -> Option<f32> {
		self.distance
	}

	fn set_distance(&mut self, distance: f32) {
		self.distance = Some(distance);
	}

	fn get_hash(&self) -> String {
		self.hash.clone()
	}
}
