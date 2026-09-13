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

//! Re-export embedding functionality from octolib and add octocode-specific logic

use crate::config::Config;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

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

mod shared;

/// Loaded local model providers cached per model string. Local ONNX weights
/// are shared machine-wide via the `shared` embedding service.
static SHARED_PROVIDERS: LazyLock<Mutex<HashMap<String, Arc<dyn EmbeddingProvider>>>> =
	LazyLock::new(|| Mutex::new(HashMap::new()));

/// Create (or join) the shared embedding provider for a fully qualified
/// model string (`provider:model`).
///
/// Local model providers (fastembed, huggingface) are shared machine-wide:
/// one process loads the weights and serves inference; every other process
/// on this machine — including this process requesting a second model —
/// connects over loopback instead of loading its own copy. Falls back to a
/// private provider when the service cannot be joined. API-backed providers
/// are lightweight and constructed fresh.
pub async fn create_shared_provider(model_string: &str) -> Result<Arc<dyn EmbeddingProvider>> {
	let (provider, model) = parse_provider_model(model_string)?;

	let is_local = matches!(
		&provider,
		EmbeddingProviderType::FastEmbed | EmbeddingProviderType::HuggingFace
	);
	if !is_local {
		let boxed = create_embedding_provider_from_parts(&provider, &model).await?;
		return Ok(Arc::from(boxed));
	}

	if let Some(cached) = SHARED_PROVIDERS.lock().unwrap().get(model_string) {
		return Ok(cached.clone());
	}

	let built: Arc<dyn EmbeddingProvider> =
		match shared::join(model_string, provider.clone(), &model).await {
			Ok(shared) => Arc::new(shared),
			Err(e) => {
				tracing::warn!(
					"shared embedding service unavailable ({e:#}); loading a private model"
				);
				Arc::from(create_embedding_provider_from_parts(&provider, &model).await?)
			}
		};
	let mut cache = SHARED_PROVIDERS.lock().unwrap();
	Ok(cache
		.entry(model_string.to_string())
		.or_insert(built)
		.clone())
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

/// Maximum number of retries for embedding calls with exponential backoff
const MAX_EMBEDDING_RETRIES: u32 = 3;

/// Generate embeddings based on configured provider (supports provider:model format)
/// Uses Query input_type for search queries (asymmetric retrieval)
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

	let mut last_error = None;
	for attempt in 0..=MAX_EMBEDDING_RETRIES {
		if attempt > 0 {
			let delay = Duration::from_secs(5 * (1 << (attempt - 1)));
			tracing::warn!(
				"Embedding call failed (attempt {}/{}), retrying in {:?}...",
				attempt,
				MAX_EMBEDDING_RETRIES + 1,
				delay
			);
			tokio::time::sleep(delay).await;
		}

		match embed_batches(
			vec![contents.to_string()],
			model_string,
			InputType::Query,
			1,
			embedding_config.max_tokens_per_batch,
		)
		.await
		{
			Ok(results) => {
				return results
					.into_iter()
					.next()
					.ok_or_else(|| anyhow::anyhow!("No embeddings generated"));
			}
			Err(e) => last_error = Some(e),
		}
	}

	Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Embedding generation failed after retries")))
}

/// Generate batch embeddings based on configured provider (supports provider:model format)
/// Compatibility wrapper for octocode Config with retry and exponential backoff
pub async fn generate_embeddings_batch(
	texts: &[String],
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

	let mut last_error = None;
	for attempt in 0..=MAX_EMBEDDING_RETRIES {
		if attempt > 0 {
			let delay = Duration::from_secs(5 * (1 << (attempt - 1)));
			tracing::warn!(
				"Embedding batch failed (attempt {}/{}), retrying in {:?}...",
				attempt,
				MAX_EMBEDDING_RETRIES + 1,
				delay
			);
			tokio::time::sleep(delay).await;
		}

		match embed_batches(
			texts.to_vec(),
			model_string,
			input_type.clone(),
			embedding_config.batch_size,
			embedding_config.max_tokens_per_batch,
		)
		.await
		{
			Ok(result) => return Ok(result),
			Err(e) => last_error = Some(e),
		}
	}

	Err(last_error
		.unwrap_or_else(|| anyhow::anyhow!("Embedding batch generation failed after retries")))
}

/// One shared-provider attempt: token-limited batching over a provider that
/// outlives this call, so local models load once per machine instead of
/// once per invocation.
async fn embed_batches(
	texts: Vec<String>,
	model_string: &str,
	input_type: InputType,
	batch_size: usize,
	max_tokens_per_batch: usize,
) -> Result<Vec<Vec<f32>>> {
	let provider = create_shared_provider(model_string).await?;
	let batches = split_texts_into_token_limited_batches(texts, batch_size, max_tokens_per_batch);
	let mut all_embeddings = Vec::new();
	for batch in batches {
		let (batch_embeddings, _usage) = provider
			.generate_embeddings_batch(batch, input_type.clone())
			.await?;
		all_embeddings.extend(batch_embeddings);
	}
	Ok(all_embeddings)
}

/// Search mode embeddings result (octocode-specific)
#[derive(Debug, Clone)]
pub struct SearchModeEmbeddings {
	pub code_embeddings: Option<Vec<f32>>,
	pub text_embeddings: Option<Vec<f32>>,
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod embedding_mod_tests;
