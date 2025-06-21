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

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

// Arrow imports
use arrow::array::{Array, FixedSizeListArray, Float32Array, ListArray, StringArray, UInt32Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;

use crate::store::{CodeBlock, DocumentBlock, TextBlock};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BatchConverter {
	vector_dim: usize,
}

impl BatchConverter {
	pub fn new(vector_dim: usize) -> Self {
		Self { vector_dim }
	}

	// Convert a CodeBlock to a RecordBatch
	pub fn code_block_to_batch(
		&self,
		blocks: &[CodeBlock],
		embeddings: &[Vec<f32>],
	) -> Result<RecordBatch> {
		// Ensure we have the same number of blocks and embeddings
		if blocks.len() != embeddings.len() {
			return Err(anyhow::anyhow!(
				"Number of blocks and embeddings must match"
			));
		}

		if blocks.is_empty() {
			return Err(anyhow::anyhow!("Empty blocks array"));
		}

		// Check if all embedding vectors have the expected dimension
		for (i, embedding) in embeddings.iter().enumerate() {
			if embedding.len() != self.vector_dim {
				return Err(anyhow::anyhow!(
					"Embedding at index {} has dimension {} but expected {}",
					i,
					embedding.len(),
					self.vector_dim
				));
			}
		}

		// Create schema
		let schema = Arc::new(Schema::new(vec![
			Field::new("id", DataType::Utf8, false),
			Field::new("path", DataType::Utf8, false),
			Field::new("language", DataType::Utf8, false),
			Field::new("content", DataType::Utf8, false),
			Field::new("symbols", DataType::Utf8, true), // Storing serialized JSON of symbols
			Field::new("start_line", DataType::UInt32, false),
			Field::new("end_line", DataType::UInt32, false),
			Field::new("hash", DataType::Utf8, false),
			Field::new(
				"embedding",
				DataType::FixedSizeList(
					Arc::new(Field::new("item", DataType::Float32, true)),
					self.vector_dim as i32,
				),
				true,
			),
		]));

		// Create arrays
		let ids: Vec<String> = blocks.iter().map(|_| Uuid::new_v4().to_string()).collect();
		let paths: Vec<&str> = blocks.iter().map(|b| b.path.as_str()).collect();
		let languages: Vec<&str> = blocks.iter().map(|b| b.language.as_str()).collect();
		let contents: Vec<&str> = blocks.iter().map(|b| b.content.as_str()).collect();
		let symbols: Vec<String> = blocks
			.iter()
			.map(|b| serde_json::to_string(&b.symbols).unwrap_or_default())
			.collect();
		let start_lines: Vec<u32> = blocks.iter().map(|b| b.start_line as u32).collect();
		let end_lines: Vec<u32> = blocks.iter().map(|b| b.end_line as u32).collect();
		let hashes: Vec<&str> = blocks.iter().map(|b| b.hash.as_str()).collect();

		// Create the embedding fixed size list array
		let mut flattened_embeddings = Vec::with_capacity(blocks.len() * self.vector_dim);
		for embedding in embeddings {
			flattened_embeddings.extend_from_slice(embedding);
		}
		let values = Float32Array::from(flattened_embeddings);

		// Create the fixed size list array
		let embedding_array = FixedSizeListArray::new(
			Arc::new(Field::new("item", DataType::Float32, true)),
			self.vector_dim as i32,
			Arc::new(values),
			None, // No validity buffer - all values are valid
		);

		// Verify all arrays have the same length
		let expected_len = blocks.len();
		assert_eq!(ids.len(), expected_len, "ids array length mismatch");
		assert_eq!(paths.len(), expected_len, "paths array length mismatch");
		assert_eq!(
			languages.len(),
			expected_len,
			"languages array length mismatch"
		);
		assert_eq!(
			contents.len(),
			expected_len,
			"contents array length mismatch"
		);
		assert_eq!(symbols.len(), expected_len, "symbols array length mismatch");
		assert_eq!(
			start_lines.len(),
			expected_len,
			"start_lines array length mismatch"
		);
		assert_eq!(
			end_lines.len(),
			expected_len,
			"end_lines array length mismatch"
		);
		assert_eq!(hashes.len(), expected_len, "hashes array length mismatch");
		assert_eq!(
			embedding_array.len(),
			expected_len,
			"embedding array length mismatch"
		);

		// Create batch
		let batch = RecordBatch::try_new(
			schema,
			vec![
				Arc::new(StringArray::from(ids)),
				Arc::new(StringArray::from(paths)),
				Arc::new(StringArray::from(languages)),
				Arc::new(StringArray::from(contents)),
				Arc::new(StringArray::from(symbols)),
				Arc::new(UInt32Array::from(start_lines)),
				Arc::new(UInt32Array::from(end_lines)),
				Arc::new(StringArray::from(hashes)),
				Arc::new(embedding_array),
			],
		)?;

		Ok(batch)
	}

	// Convert a TextBlock to a RecordBatch
	pub fn text_block_to_batch(
		&self,
		blocks: &[TextBlock],
		embeddings: &[Vec<f32>],
	) -> Result<RecordBatch> {
		// Ensure we have the same number of blocks and embeddings
		if blocks.len() != embeddings.len() {
			return Err(anyhow::anyhow!(
				"Number of blocks and embeddings must match"
			));
		}

		if blocks.is_empty() {
			return Err(anyhow::anyhow!("Empty blocks array"));
		}

		// Check if all embedding vectors have the expected dimension
		for (i, embedding) in embeddings.iter().enumerate() {
			if embedding.len() != self.vector_dim {
				return Err(anyhow::anyhow!(
					"Embedding at index {} has dimension {} but expected {}",
					i,
					embedding.len(),
					self.vector_dim
				));
			}
		}

		// Create schema matching the actual TextBlock structure
		let schema = Arc::new(Schema::new(vec![
			Field::new("id", DataType::Utf8, false),
			Field::new("path", DataType::Utf8, false),
			Field::new("language", DataType::Utf8, false),
			Field::new("content", DataType::Utf8, false),
			Field::new("start_line", DataType::UInt32, false),
			Field::new("end_line", DataType::UInt32, false),
			Field::new("hash", DataType::Utf8, false),
			Field::new(
				"embedding",
				DataType::FixedSizeList(
					Arc::new(Field::new("item", DataType::Float32, true)),
					self.vector_dim as i32,
				),
				true,
			),
		]));

		// Create arrays
		let ids: Vec<String> = blocks.iter().map(|_| Uuid::new_v4().to_string()).collect();
		let paths: Vec<&str> = blocks.iter().map(|b| b.path.as_str()).collect();
		let languages: Vec<&str> = blocks.iter().map(|b| b.language.as_str()).collect();
		let contents: Vec<&str> = blocks.iter().map(|b| b.content.as_str()).collect();
		let start_lines: Vec<u32> = blocks.iter().map(|b| b.start_line as u32).collect();
		let end_lines: Vec<u32> = blocks.iter().map(|b| b.end_line as u32).collect();
		let hashes: Vec<&str> = blocks.iter().map(|b| b.hash.as_str()).collect();

		// Create the embedding fixed size list array
		let mut flattened_embeddings = Vec::with_capacity(blocks.len() * self.vector_dim);
		for embedding in embeddings {
			flattened_embeddings.extend_from_slice(embedding);
		}
		let values = Float32Array::from(flattened_embeddings);

		// Create the fixed size list array
		let embedding_array = FixedSizeListArray::new(
			Arc::new(Field::new("item", DataType::Float32, true)),
			self.vector_dim as i32,
			Arc::new(values),
			None, // No validity buffer - all values are valid
		);

		// Verify all arrays have the same length
		let expected_len = blocks.len();
		assert_eq!(ids.len(), expected_len, "ids array length mismatch");
		assert_eq!(paths.len(), expected_len, "paths array length mismatch");
		assert_eq!(
			languages.len(),
			expected_len,
			"languages array length mismatch"
		);
		assert_eq!(
			contents.len(),
			expected_len,
			"contents array length mismatch"
		);
		assert_eq!(
			start_lines.len(),
			expected_len,
			"start_lines array length mismatch"
		);
		assert_eq!(
			end_lines.len(),
			expected_len,
			"end_lines array length mismatch"
		);
		assert_eq!(hashes.len(), expected_len, "hashes array length mismatch");
		assert_eq!(
			embedding_array.len(),
			expected_len,
			"embedding array length mismatch"
		);

		// Create batch
		let batch = RecordBatch::try_new(
			schema,
			vec![
				Arc::new(StringArray::from(ids)),
				Arc::new(StringArray::from(paths)),
				Arc::new(StringArray::from(languages)),
				Arc::new(StringArray::from(contents)),
				Arc::new(UInt32Array::from(start_lines)),
				Arc::new(UInt32Array::from(end_lines)),
				Arc::new(StringArray::from(hashes)),
				Arc::new(embedding_array),
			],
		)?;

		Ok(batch)
	}

	// Convert a DocumentBlock to a RecordBatch
	pub fn document_block_to_batch(
		&self,
		blocks: &[DocumentBlock],
		embeddings: &[Vec<f32>],
	) -> Result<RecordBatch> {
		// Ensure we have the same number of blocks and embeddings
		if blocks.len() != embeddings.len() {
			return Err(anyhow::anyhow!(
				"Number of blocks and embeddings must match"
			));
		}

		if blocks.is_empty() {
			return Err(anyhow::anyhow!("Empty blocks array"));
		}

		// Check if all embedding vectors have the expected dimension
		for (i, embedding) in embeddings.iter().enumerate() {
			if embedding.len() != self.vector_dim {
				return Err(anyhow::anyhow!(
					"Embedding at index {} has dimension {} but expected {}",
					i,
					embedding.len(),
					self.vector_dim
				));
			}
		}

		// Create schema matching the actual DocumentBlock structure
		let schema = Arc::new(Schema::new(vec![
			Field::new("id", DataType::Utf8, false),
			Field::new("path", DataType::Utf8, false),
			Field::new("title", DataType::Utf8, false),
			Field::new("content", DataType::Utf8, false),
			Field::new(
				"context",
				DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
				true,
			),
			Field::new("level", DataType::UInt32, false),
			Field::new("start_line", DataType::UInt32, false),
			Field::new("end_line", DataType::UInt32, false),
			Field::new("hash", DataType::Utf8, false),
			Field::new(
				"embedding",
				DataType::FixedSizeList(
					Arc::new(Field::new("item", DataType::Float32, true)),
					self.vector_dim as i32,
				),
				true,
			),
		]));

		// Create arrays
		let ids: Vec<String> = blocks.iter().map(|_| Uuid::new_v4().to_string()).collect();
		let paths: Vec<&str> = blocks.iter().map(|b| b.path.as_str()).collect();
		let titles: Vec<&str> = blocks.iter().map(|b| b.title.as_str()).collect();
		let contents: Vec<&str> = blocks.iter().map(|b| b.content.as_str()).collect();

		// Create context list array
		let mut context_values = Vec::new();
		let mut context_offsets = vec![0i32];
		for block in blocks {
			for context_item in &block.context {
				context_values.push(context_item.as_str());
			}
			context_offsets.push(context_values.len() as i32);
		}
		let context_array = ListArray::new(
			Arc::new(Field::new("item", DataType::Utf8, true)),
			arrow::buffer::OffsetBuffer::new(context_offsets.into()),
			Arc::new(StringArray::from(context_values)),
			None,
		);

		let levels: Vec<u32> = blocks.iter().map(|b| b.level as u32).collect();
		let start_lines: Vec<u32> = blocks.iter().map(|b| b.start_line as u32).collect();
		let end_lines: Vec<u32> = blocks.iter().map(|b| b.end_line as u32).collect();
		let hashes: Vec<&str> = blocks.iter().map(|b| b.hash.as_str()).collect();

		// Create the embedding fixed size list array
		let mut flattened_embeddings = Vec::with_capacity(blocks.len() * self.vector_dim);
		for embedding in embeddings {
			flattened_embeddings.extend_from_slice(embedding);
		}
		let values = Float32Array::from(flattened_embeddings);

		// Create the fixed size list array
		let embedding_array = FixedSizeListArray::new(
			Arc::new(Field::new("item", DataType::Float32, true)),
			self.vector_dim as i32,
			Arc::new(values),
			None, // No validity buffer - all values are valid
		);

		// Verify all arrays have the same length
		let expected_len = blocks.len();
		assert_eq!(ids.len(), expected_len, "ids array length mismatch");
		assert_eq!(paths.len(), expected_len, "paths array length mismatch");
		assert_eq!(titles.len(), expected_len, "titles array length mismatch");
		assert_eq!(
			contents.len(),
			expected_len,
			"contents array length mismatch"
		);
		assert_eq!(levels.len(), expected_len, "levels array length mismatch");
		assert_eq!(
			start_lines.len(),
			expected_len,
			"start_lines array length mismatch"
		);
		assert_eq!(
			end_lines.len(),
			expected_len,
			"end_lines array length mismatch"
		);
		assert_eq!(hashes.len(), expected_len, "hashes array length mismatch");
		assert_eq!(
			embedding_array.len(),
			expected_len,
			"embedding array length mismatch"
		);
		assert_eq!(
			embedding_array.len(),
			expected_len,
			"embedding array length mismatch"
		);

		// Create batch
		let batch = RecordBatch::try_new(
			schema,
			vec![
				Arc::new(StringArray::from(ids)),
				Arc::new(StringArray::from(paths)),
				Arc::new(StringArray::from(titles)),
				Arc::new(StringArray::from(contents)),
				Arc::new(context_array),
				Arc::new(UInt32Array::from(levels)),
				Arc::new(UInt32Array::from(start_lines)),
				Arc::new(UInt32Array::from(end_lines)),
				Arc::new(StringArray::from(hashes)),
				Arc::new(embedding_array),
			],
		)?;

		Ok(batch)
	}

	// Convert a RecordBatch to a Vec of CodeBlocks
	pub fn batch_to_code_blocks(
		&self,
		batch: &RecordBatch,
		_embeddings: Option<&[Vec<f32>]>,
	) -> Result<Vec<CodeBlock>> {
		let mut code_blocks = Vec::new();

		// Extract columns from the batch
		let _id_array = batch
			.column_by_name("id")
			.ok_or_else(|| anyhow::anyhow!("id column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("id column is not a StringArray"))?;

		let path_array = batch
			.column_by_name("path")
			.ok_or_else(|| anyhow::anyhow!("path column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("path column is not a StringArray"))?;

		let language_array = batch
			.column_by_name("language")
			.ok_or_else(|| anyhow::anyhow!("language column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("language column is not a StringArray"))?;

		let content_array = batch
			.column_by_name("content")
			.ok_or_else(|| anyhow::anyhow!("content column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("content column is not a StringArray"))?;

		let symbols_array = batch
			.column_by_name("symbols")
			.ok_or_else(|| anyhow::anyhow!("symbols column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("symbols column is not a StringArray"))?;

		let start_line_array = batch
			.column_by_name("start_line")
			.ok_or_else(|| anyhow::anyhow!("start_line column not found"))?
			.as_any()
			.downcast_ref::<UInt32Array>()
			.ok_or_else(|| anyhow::anyhow!("start_line column is not a UInt32Array"))?;

		let end_line_array = batch
			.column_by_name("end_line")
			.ok_or_else(|| anyhow::anyhow!("end_line column not found"))?
			.as_any()
			.downcast_ref::<UInt32Array>()
			.ok_or_else(|| anyhow::anyhow!("end_line column is not a UInt32Array"))?;

		let hash_array = batch
			.column_by_name("hash")
			.ok_or_else(|| anyhow::anyhow!("hash column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("hash column is not a StringArray"))?;

		// Process each row
		for i in 0..batch.num_rows() {
			// Parse symbols JSON
			let symbols_json = symbols_array.value(i);
			let symbols: Vec<String> = if symbols_json.is_empty() {
				Vec::new()
			} else {
				serde_json::from_str(symbols_json).unwrap_or_default()
			};

			let code_block = CodeBlock {
				path: path_array.value(i).to_string(),
				language: language_array.value(i).to_string(),
				content: content_array.value(i).to_string(),
				symbols,
				start_line: start_line_array.value(i) as usize,
				end_line: end_line_array.value(i) as usize,
				hash: hash_array.value(i).to_string(),
				distance: None,
			};

			code_blocks.push(code_block);
		}

		Ok(code_blocks)
	}

	// Convert a RecordBatch to a Vec of TextBlocks
	pub fn batch_to_text_blocks(
		&self,
		batch: &RecordBatch,
		_embeddings: Option<&[Vec<f32>]>,
	) -> Result<Vec<TextBlock>> {
		let mut text_blocks = Vec::new();

		// Extract columns from the batch
		let path_array = batch
			.column_by_name("path")
			.ok_or_else(|| anyhow::anyhow!("path column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("path column is not a StringArray"))?;

		let language_array = batch
			.column_by_name("language")
			.ok_or_else(|| anyhow::anyhow!("language column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("language column is not a StringArray"))?;

		let content_array = batch
			.column_by_name("content")
			.ok_or_else(|| anyhow::anyhow!("content column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("content column is not a StringArray"))?;

		let start_line_array = batch
			.column_by_name("start_line")
			.ok_or_else(|| anyhow::anyhow!("start_line column not found"))?
			.as_any()
			.downcast_ref::<UInt32Array>()
			.ok_or_else(|| anyhow::anyhow!("start_line column is not a UInt32Array"))?;

		let end_line_array = batch
			.column_by_name("end_line")
			.ok_or_else(|| anyhow::anyhow!("end_line column not found"))?
			.as_any()
			.downcast_ref::<UInt32Array>()
			.ok_or_else(|| anyhow::anyhow!("end_line column is not a UInt32Array"))?;

		let hash_array = batch
			.column_by_name("hash")
			.ok_or_else(|| anyhow::anyhow!("hash column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("hash column is not a StringArray"))?;

		// Process each row
		for i in 0..batch.num_rows() {
			let text_block = TextBlock {
				path: path_array.value(i).to_string(),
				language: language_array.value(i).to_string(),
				content: content_array.value(i).to_string(),
				start_line: start_line_array.value(i) as usize,
				end_line: end_line_array.value(i) as usize,
				hash: hash_array.value(i).to_string(),
				distance: None,
			};

			text_blocks.push(text_block);
		}

		Ok(text_blocks)
	}

	// Convert a RecordBatch to a Vec of DocumentBlocks
	pub fn batch_to_document_blocks(
		&self,
		batch: &RecordBatch,
		_embeddings: Option<&[Vec<f32>]>,
	) -> Result<Vec<DocumentBlock>> {
		let mut document_blocks = Vec::new();

		// Extract columns from the batch
		let path_array = batch
			.column_by_name("path")
			.ok_or_else(|| anyhow::anyhow!("path column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("path column is not a StringArray"))?;

		let title_array = batch
			.column_by_name("title")
			.ok_or_else(|| anyhow::anyhow!("title column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("title column is not a StringArray"))?;

		let content_array = batch
			.column_by_name("content")
			.ok_or_else(|| anyhow::anyhow!("content column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("content column is not a StringArray"))?;

		let context_array = batch
			.column_by_name("context")
			.ok_or_else(|| anyhow::anyhow!("context column not found"))?
			.as_any()
			.downcast_ref::<ListArray>()
			.ok_or_else(|| anyhow::anyhow!("context column is not a ListArray"))?;

		let start_line_array = batch
			.column_by_name("start_line")
			.ok_or_else(|| anyhow::anyhow!("start_line column not found"))?
			.as_any()
			.downcast_ref::<UInt32Array>()
			.ok_or_else(|| anyhow::anyhow!("start_line column is not a UInt32Array"))?;

		let end_line_array = batch
			.column_by_name("end_line")
			.ok_or_else(|| anyhow::anyhow!("end_line column not found"))?
			.as_any()
			.downcast_ref::<UInt32Array>()
			.ok_or_else(|| anyhow::anyhow!("end_line column is not a UInt32Array"))?;

		let hash_array = batch
			.column_by_name("hash")
			.ok_or_else(|| anyhow::anyhow!("hash column not found"))?
			.as_any()
			.downcast_ref::<StringArray>()
			.ok_or_else(|| anyhow::anyhow!("hash column is not a StringArray"))?;

		// Process each row
		for i in 0..batch.num_rows() {
			let context = if context_array.is_null(i) {
				Vec::new()
			} else {
				// Extract the list of strings from the ListArray
				let list_values = context_array.value(i);
				let string_array = list_values
					.as_any()
					.downcast_ref::<StringArray>()
					.ok_or_else(|| anyhow::anyhow!("Context list items are not strings"))?;

				let mut context_vec = Vec::new();
				for j in 0..string_array.len() {
					if !string_array.is_null(j) {
						context_vec.push(string_array.value(j).to_string());
					}
				}
				context_vec
			};

			// Check if level column exists
			let level = if let Some(level_col) = batch.column_by_name("level") {
				if let Some(level_array) = level_col.as_any().downcast_ref::<UInt32Array>() {
					level_array.value(i) as usize
				} else {
					0 // Default level
				}
			} else {
				0 // Default level if column doesn't exist
			};

			let document_block = DocumentBlock {
				path: path_array.value(i).to_string(),
				title: title_array.value(i).to_string(),
				content: content_array.value(i).to_string(),
				context,
				level,
				start_line: start_line_array.value(i) as usize,
				end_line: end_line_array.value(i) as usize,
				hash: hash_array.value(i).to_string(),
				distance: None, // Set by search functions when needed
			};

			document_blocks.push(document_block);
		}

		Ok(document_blocks)
	}
}
