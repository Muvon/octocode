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

pub mod provider;
#[cfg(test)]
mod tests;
pub mod types;

use crate::config::Config;
use anyhow::Result;
use tiktoken_rs::cl100k_base;

pub use provider::{create_embedding_provider_from_parts, EmbeddingProvider};
pub use types::*;

/// Generate embeddings based on configured provider (supports provider:model format)
pub async fn generate_embeddings(
	contents: &str,
	is_code: bool,
	config: &Config,
) -> Result<Vec<f32>> {
	// Get the model string from config
	let model_string = if is_code {
		&config.embedding.code_model
	} else {
		&config.embedding.text_model
	};

	// Parse provider and model from the string
	let (provider, model) = parse_provider_model(model_string);

	let provider_impl = create_embedding_provider_from_parts(&provider, &model)?;
	provider_impl.generate_embedding(contents).await
}

/// Count tokens in a text using tiktoken (cl100k_base tokenizer)
pub fn count_tokens(text: &str) -> usize {
	let bpe = cl100k_base().expect("Failed to load cl100k_base tokenizer");
	bpe.encode_with_special_tokens(text).len()
}

/// Split texts into batches respecting both count and token limits
pub fn split_texts_into_token_limited_batches(
	texts: Vec<String>,
	max_batch_size: usize,
	max_tokens_per_batch: usize,
) -> Vec<Vec<String>> {
	let mut batches = Vec::new();
	let mut current_batch = Vec::new();
	let mut current_token_count = 0;

	for text in texts {
		let text_tokens = count_tokens(&text);

		// If adding this text would exceed either limit, start a new batch
		if !current_batch.is_empty()
			&& (current_batch.len() >= max_batch_size
				|| current_token_count + text_tokens > max_tokens_per_batch)
		{
			batches.push(current_batch);
			current_batch = Vec::new();
			current_token_count = 0;
		}

		current_batch.push(text);
		current_token_count += text_tokens;
	}

	// Add the last batch if it's not empty
	if !current_batch.is_empty() {
		batches.push(current_batch);
	}

	batches
}

/// Generate batch embeddings based on configured provider (supports provider:model format)
/// Now includes token-aware batching
pub async fn generate_embeddings_batch(
	texts: Vec<String>,
	is_code: bool,
	config: &Config,
) -> Result<Vec<Vec<f32>>> {
	// Get the model string from config
	let model_string = if is_code {
		&config.embedding.code_model
	} else {
		&config.embedding.text_model
	};

	// Parse provider and model from the string
	let (provider, model) = parse_provider_model(model_string);

	let provider_impl = create_embedding_provider_from_parts(&provider, &model)?;

	// Split texts into token-limited batches
	let batches = split_texts_into_token_limited_batches(
		texts,
		config.index.embeddings_batch_size,
		config.index.embeddings_max_tokens_per_batch,
	);

	let mut all_embeddings = Vec::new();

	// Process each batch
	for batch in batches {
		let batch_embeddings = provider_impl.generate_embeddings_batch(batch).await?;
		all_embeddings.extend(batch_embeddings);
	}

	Ok(all_embeddings)
}

/// Calculate a unique hash for content including file path
pub fn calculate_unique_content_hash(contents: &str, file_path: &str) -> String {
	use sha2::{Digest, Sha256};
	let mut hasher = Sha256::new();
	hasher.update(contents.as_bytes());
	hasher.update(file_path.as_bytes());
	format!("{:x}", hasher.finalize())
}

/// Calculate a unique hash for content including file path and line ranges
/// This ensures blocks are reindexed when their position changes in the file
pub fn calculate_content_hash_with_lines(
	contents: &str,
	file_path: &str,
	start_line: usize,
	end_line: usize,
) -> String {
	use sha2::{Digest, Sha256};
	let mut hasher = Sha256::new();
	hasher.update(contents.as_bytes());
	hasher.update(file_path.as_bytes());
	hasher.update(start_line.to_string().as_bytes());
	hasher.update(end_line.to_string().as_bytes());
	format!("{:x}", hasher.finalize())
}

/// Calculate content hash without file path
pub fn calculate_content_hash(contents: &str) -> String {
	use sha2::{Digest, Sha256};
	let mut hasher = Sha256::new();
	hasher.update(contents.as_bytes());
	format!("{:x}", hasher.finalize())
}

/// Search mode embeddings result
#[derive(Debug, Clone)]
pub struct SearchModeEmbeddings {
	pub code_embeddings: Option<Vec<f32>>,
	pub text_embeddings: Option<Vec<f32>>,
}

/// Generate embeddings for search based on mode - centralized logic to avoid duplication
/// This ensures consistent behavior across CLI and MCP interfaces
pub async fn generate_search_embeddings(
	query: &str,
	mode: &str,
	config: &Config,
) -> Result<SearchModeEmbeddings> {
	match mode {
		"code" => {
			// Use code model for code searches only
			let embeddings = generate_embeddings(query, true, config).await?;
			Ok(SearchModeEmbeddings {
				code_embeddings: Some(embeddings),
				text_embeddings: None,
			})
		}
		"docs" | "text" => {
			// Use text model for documents and text searches only
			let embeddings = generate_embeddings(query, false, config).await?;
			Ok(SearchModeEmbeddings {
				code_embeddings: None,
				text_embeddings: Some(embeddings),
			})
		}
		"all" => {
			// For "all" mode, check if code and text models are different
			// If different, generate separate embeddings; if same, use one set
			let code_model = &config.embedding.code_model;
			let text_model = &config.embedding.text_model;

			if code_model == text_model {
				// Same model for both - generate once and reuse
				let embeddings = generate_embeddings(query, true, config).await?;
				Ok(SearchModeEmbeddings {
					code_embeddings: Some(embeddings.clone()),
					text_embeddings: Some(embeddings),
				})
			} else {
				// Different models - generate separate embeddings
				let code_embeddings = generate_embeddings(query, true, config).await?;
				let text_embeddings = generate_embeddings(query, false, config).await?;
				Ok(SearchModeEmbeddings {
					code_embeddings: Some(code_embeddings),
					text_embeddings: Some(text_embeddings),
				})
			}
		}
		_ => Err(anyhow::anyhow!(
			"Invalid search mode '{}'. Use 'all', 'code', 'docs', or 'text'.",
			mode
		)),
	}
}
