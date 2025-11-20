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

//! Re-export embedding functionality from octolib and add octocode-specific logic

use crate::config::Config;
use anyhow::Result;

// Re-export core functionality from octolib::embedding
pub use octolib::embedding::{
	count_tokens, create_embedding_provider_from_parts, split_texts_into_token_limited_batches,
	truncate_output, EmbeddingProvider, InputType,
};

// Re-export types for backward compatibility
pub use octolib::embedding::types::{parse_provider_model, EmbeddingProviderType};

// Create a types module for backward compatibility
pub mod types {
	pub use octolib::embedding::types::*;
}

// Create a provider module for backward compatibility
pub mod provider {
	pub use octolib::embedding::provider::*;
}

/// Configuration for embedding generation (octocode-specific)
#[derive(Debug, Clone)]
pub struct EmbeddingGenerationConfig {
	/// Code embedding model (format: "provider:model")
	pub code_model: String,
	/// Text embedding model (format: "provider:model")
	pub text_model: String,
	/// Batch size for embedding generation
	pub batch_size: usize,
	/// Maximum tokens per batch
	pub max_tokens_per_batch: usize,
}

impl Default for EmbeddingGenerationConfig {
	fn default() -> Self {
		Self {
			code_model: "voyage:voyage-code-3".to_string(),
			text_model: "voyage:voyage-3.5-lite".to_string(),
			batch_size: 16,
			max_tokens_per_batch: 100_000,
		}
	}
}

/// Convert octocode Config to octocode EmbeddingGenerationConfig
impl From<&Config> for EmbeddingGenerationConfig {
	fn from(config: &Config) -> Self {
		Self {
			code_model: config.embedding.code_model.clone(),
			text_model: config.embedding.text_model.clone(),
			batch_size: config.index.embeddings_batch_size,
			max_tokens_per_batch: config.index.embeddings_max_tokens_per_batch,
		}
	}
}

/// Generate embeddings based on configured provider (supports provider:model format)
/// Compatibility wrapper for octocode Config
pub async fn generate_embeddings(
	contents: &str,
	is_code: bool,
	config: &Config,
) -> Result<Vec<f32>> {
	let embedding_config = EmbeddingGenerationConfig::from(config);

	// Get the model string from config
	let model_string = if is_code {
		&embedding_config.code_model
	} else {
		&embedding_config.text_model
	};

	// Parse provider and model from the string
	let (provider, model) = if let Some((p, m)) = model_string.split_once(':') {
		(p, m)
	} else {
		return Err(anyhow::anyhow!("Invalid model format: {}", model_string));
	};

	octolib::embedding::generate_embeddings(contents, provider, model).await
}

/// Generate batch embeddings based on configured provider (supports provider:model format)
/// Compatibility wrapper for octocode Config
pub async fn generate_embeddings_batch(
	texts: Vec<String>,
	is_code: bool,
	config: &Config,
	input_type: InputType,
) -> Result<Vec<Vec<f32>>> {
	let embedding_config = EmbeddingGenerationConfig::from(config);

	// Get the model string from config
	let model_string = if is_code {
		&embedding_config.code_model
	} else {
		&embedding_config.text_model
	};

	// Parse provider and model from the string
	let (provider, model) = if let Some((p, m)) = model_string.split_once(':') {
		(p, m)
	} else {
		return Err(anyhow::anyhow!("Invalid model format: {}", model_string));
	};

	octolib::embedding::generate_embeddings_batch(
		texts,
		provider,
		model,
		input_type,
		embedding_config.batch_size,
		embedding_config.max_tokens_per_batch,
	)
	.await
}

/// Search mode embeddings result (octocode-specific)
#[derive(Debug, Clone)]
pub struct SearchModeEmbeddings {
	pub code_embeddings: Option<Vec<f32>>,
	pub text_embeddings: Option<Vec<f32>>,
}

/// Generate embeddings for search based on mode - centralized logic to avoid duplication
/// Compatibility wrapper for octocode Config (octocode-specific)
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
			let embedding_config = EmbeddingGenerationConfig::from(config);
			let code_model = &embedding_config.code_model;
			let text_model = &embedding_config.text_model;

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

/// Calculate a unique hash for content including file path (octocode-specific)
pub fn calculate_unique_content_hash(contents: &str, file_path: &str) -> String {
	use sha2::{Digest, Sha256};
	let mut hasher = Sha256::new();
	hasher.update(contents.as_bytes());
	hasher.update(file_path.as_bytes());
	format!("{:x}", hasher.finalize())
}

/// Calculate a unique hash for content including file path and line ranges (octocode-specific)
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

/// Calculate content hash without file path (octocode-specific)
pub fn calculate_content_hash(contents: &str) -> String {
	use sha2::{Digest, Sha256};
	let mut hasher = Sha256::new();
	hasher.update(contents.as_bytes());
	format!("{:x}", hasher.finalize())
}
