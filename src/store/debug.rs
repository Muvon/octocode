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

// Arrow imports
use arrow::array::{Array, StringArray};

// LanceDB imports
use futures::TryStreamExt;
use lancedb::{
	query::{ExecutableQuery, QueryBase, Select},
	Connection,
};

use crate::store::{
	batch_converter::BatchConverter, table_ops::TableOperations, CodeBlock, DocumentBlock,
	TextBlock,
};

/// Debug and inspection operations for the database
pub struct DebugOperations<'a> {
	pub db: &'a Connection,
	pub table_ops: TableOperations<'a>,
	pub code_vector_dim: usize,
}

impl<'a> DebugOperations<'a> {
	pub fn new(db: &'a Connection, code_vector_dim: usize) -> Self {
		Self {
			db,
			table_ops: TableOperations::new(db),
			code_vector_dim,
		}
	}

	/// List all indexed files (moved from debug command)
	pub async fn list_indexed_files(&self) -> Result<()> {
		let table_names = self.db.table_names().execute().await?;
		let mut total_files = 0;

		for table_name in &["code_blocks", "text_blocks", "document_blocks"] {
			if table_names.contains(&table_name.to_string()) {
				println!("\n📁 Files in {} table:", table_name);
				let table = self.db.open_table(*table_name).execute().await?;
				let mut results = table
					.query()
					.select(Select::Columns(vec!["path".to_string()]))
					.execute()
					.await?;

				let mut unique_paths = std::collections::HashSet::new();

				// Process all result batches
				while let Some(batch) = results.try_next().await? {
					if batch.num_rows() > 0 {
						if let Some(column) = batch.column_by_name("path") {
							if let Some(path_array) = column.as_any().downcast_ref::<StringArray>()
							{
								for i in 0..path_array.len() {
									let path = path_array.value(i).to_string();
									unique_paths.insert(path);
								}
							} else {
								return Err(anyhow::anyhow!("Path column is not a StringArray"));
							}
						} else {
							return Err(anyhow::anyhow!("Path column not found"));
						}
					}
				}

				if !unique_paths.is_empty() {
					let count = unique_paths.len();
					for path in unique_paths {
						println!("  📄 {}", path);
					}
					println!("   └─ {} unique files", count);
					total_files += count;
				} else {
					println!("   └─ (no files)");
				}
			} else {
				println!("\n❌ Table {} does not exist", table_name);
			}
		}

		println!("\n📊 Total indexed files: {}", total_files);
		Ok(())
	}

	/// Show all chunks for a specific file path across all tables
	pub async fn show_file_chunks(&self, file_path: &str) -> Result<()> {
		let table_names = self.db.table_names().execute().await?;
		let mut total_chunks = 0;
		let mut found_in_any_table = false;

		println!("🔍 Searching for chunks of file: {}", file_path);
		println!("{}", "=".repeat(80));

		// Check code_blocks table
		if table_names.contains(&"code_blocks".to_string()) {
			if let Ok(chunks) = self.get_file_code_blocks(file_path).await {
				if !chunks.is_empty() {
					found_in_any_table = true;
					println!("\n📦 CODE BLOCKS ({} chunks)", chunks.len());
					println!("{}", "-".repeat(40));

					for (i, chunk) in chunks.iter().enumerate() {
						println!("🔹 Chunk #{} (Code)", i + 1);
						println!("   📍 Lines: {}-{}", chunk.start_line, chunk.end_line);
						println!("   🏷️  Language: {}", chunk.language);
						println!("   🔑 Hash: {}", chunk.hash);
						println!("   📝 Symbols: {:?}", chunk.symbols);
						println!("   📄 Content preview:");
						self.print_content_preview(&chunk.content, 3);
						println!();
					}
					total_chunks += chunks.len();
				}
			}
		}

		// Check text_blocks table
		if table_names.contains(&"text_blocks".to_string()) {
			if let Ok(chunks) = self.get_file_text_blocks(file_path).await {
				if !chunks.is_empty() {
					found_in_any_table = true;
					println!("\n📄 TEXT BLOCKS ({} chunks)", chunks.len());
					println!("{}", "-".repeat(40));

					for (i, chunk) in chunks.iter().enumerate() {
						println!("🔹 Chunk #{} (Text)", i + 1);
						println!("   📍 Lines: {}-{}", chunk.start_line, chunk.end_line);
						println!("   🔑 Hash: {}", chunk.hash);
						println!("   📄 Content preview:");
						self.print_content_preview(&chunk.content, 3);
						println!();
					}
					total_chunks += chunks.len();
				}
			}
		}

		// Check document_blocks table
		if table_names.contains(&"document_blocks".to_string()) {
			if let Ok(chunks) = self.get_file_document_blocks(file_path).await {
				if !chunks.is_empty() {
					found_in_any_table = true;
					println!("\n📚 DOCUMENT BLOCKS ({} chunks)", chunks.len());
					println!("{}", "-".repeat(40));

					for (i, chunk) in chunks.iter().enumerate() {
						println!("🔹 Chunk #{} (Document)", i + 1);
						println!("   📍 Lines: {}-{}", chunk.start_line, chunk.end_line);
						println!("   🏷️  Title: {}", chunk.title);
						println!("   🔑 Hash: {}", chunk.hash);
						if !chunk.context.is_empty() {
							println!("   🔗 Context: {}", chunk.context.join(" > "));
						}
						println!("   📄 Content preview:");
						self.print_content_preview(&chunk.content, 3);
						println!();
					}
					total_chunks += chunks.len();
				}
			}
		}

		if !found_in_any_table {
			println!("\n❌ No chunks found for file: {}", file_path);
			println!("   This could mean:");
			println!("   • The file hasn't been indexed yet");
			println!("   • The file was excluded by .gitignore or .noindex");
			println!("   • The file doesn't contain indexable content");
		} else {
			println!("{}", "=".repeat(80));
			println!("📊 Total chunks found: {}", total_chunks);
		}

		Ok(())
	}

	/// Get all code blocks for a specific file
	async fn get_file_code_blocks(&self, file_path: &str) -> Result<Vec<CodeBlock>> {
		let table = self.db.open_table("code_blocks").execute().await?;

		let mut results = table
			.query()
			.only_if(format!("path = '{}'", file_path))
			.execute()
			.await?;

		let mut blocks = Vec::new();
		let converter = BatchConverter::new(self.code_vector_dim);

		// Process all result batches
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				let mut batch_blocks = converter.batch_to_code_blocks(&batch, None)?;
				blocks.append(&mut batch_blocks);
			}
		}

		Ok(blocks)
	}

	/// Get all text blocks for a specific file
	async fn get_file_text_blocks(&self, file_path: &str) -> Result<Vec<TextBlock>> {
		let table = self.db.open_table("text_blocks").execute().await?;

		let mut results = table
			.query()
			.only_if(format!("path = '{}'", file_path))
			.execute()
			.await?;

		let mut blocks = Vec::new();
		let converter = BatchConverter::new(self.code_vector_dim);

		// Process all result batches
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				let mut batch_blocks = converter.batch_to_text_blocks(&batch, None)?;
				blocks.append(&mut batch_blocks);
			}
		}

		Ok(blocks)
	}

	/// Get all document blocks for a specific file
	async fn get_file_document_blocks(&self, file_path: &str) -> Result<Vec<DocumentBlock>> {
		let table = self.db.open_table("document_blocks").execute().await?;

		let mut results = table
			.query()
			.only_if(format!("path = '{}'", file_path))
			.execute()
			.await?;

		let mut blocks = Vec::new();
		let converter = BatchConverter::new(self.code_vector_dim);

		// Process all result batches
		while let Some(batch) = results.try_next().await? {
			if batch.num_rows() > 0 {
				let mut batch_blocks = converter.batch_to_document_blocks(&batch, None)?;
				blocks.append(&mut batch_blocks);
			}
		}

		Ok(blocks)
	}

	/// Print a content preview with limited lines
	fn print_content_preview(&self, content: &str, max_lines: usize) {
		let lines: Vec<&str> = content.lines().collect();
		let preview_lines = if lines.len() > max_lines {
			&lines[..max_lines]
		} else {
			&lines
		};

		for line in preview_lines {
			println!("      {}", line);
		}

		if lines.len() > max_lines {
			println!("      ... ({} more lines)", lines.len() - max_lines);
		}
	}
}
