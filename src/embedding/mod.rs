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

//! Re-export embedding functionality from octolib

use crate::config::Config;
use anyhow::Result;

// Re-export everything from octolib::embedding
pub use octolib::embedding::*;

/// Convert octocode Config to octolib EmbeddingGenerationConfig
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
	octolib::embedding::generate_embeddings(contents, is_code, &embedding_config).await
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
	octolib::embedding::generate_embeddings_batch(texts, is_code, &embedding_config, input_type)
		.await
}

/// Generate embeddings for search based on mode - centralized logic to avoid duplication
/// Compatibility wrapper for octocode Config
pub async fn generate_search_embeddings(
	query: &str,
	mode: &str,
	config: &Config,
) -> Result<SearchModeEmbeddings> {
	let embedding_config = EmbeddingGenerationConfig::from(config);
	octolib::embedding::generate_search_embeddings(query, mode, &embedding_config).await
}
