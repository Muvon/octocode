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

//! Embedding providers module
//!
//! This module contains implementations for different embedding providers.
//! Each provider can be optionally compiled based on cargo features.

use anyhow::Result;
use reqwest::Client;
use std::sync::LazyLock;
use std::time::Duration;

use super::types::{EmbeddingProviderType, InputType};

// Shared HTTP client with connection pooling for optimal performance
static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(|| {
	Client::builder()
		.pool_max_idle_per_host(10)
		.pool_idle_timeout(Duration::from_secs(30))
		.timeout(Duration::from_secs(120)) // Increased from 60s to 120s for embedding APIs
		.connect_timeout(Duration::from_secs(10))
		.build()
		.expect("Failed to create HTTP client")
});

// Feature-specific provider modules
#[cfg(feature = "fastembed")]
pub mod fastembed;
#[cfg(feature = "huggingface")]
pub mod huggingface;

// Always available provider modules
pub mod google;
pub mod jina;
pub mod openai;
pub mod voyage;

// Re-export providers
#[cfg(feature = "fastembed")]
pub use fastembed::{FastEmbedProvider, FastEmbedProviderImpl};
#[cfg(feature = "huggingface")]
pub use huggingface::{HuggingFaceProvider, HuggingFaceProviderImpl};

// Always available provider re-exports
pub use google::{GoogleProvider, GoogleProviderImpl};
pub use jina::{JinaProvider, JinaProviderImpl};
pub use openai::{OpenAIProvider, OpenAIProviderImpl};
pub use voyage::{VoyageProvider, VoyageProviderImpl};

/// Trait for embedding providers
#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync {
	async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>>;
	async fn generate_embeddings_batch(
		&self,
		texts: Vec<String>,
		input_type: InputType,
	) -> Result<Vec<Vec<f32>>>;

	/// Get the vector dimension for this provider's model
	fn get_dimension(&self) -> usize;

	/// Validate if the model is supported (optional, defaults to true)
	fn is_model_supported(&self) -> bool {
		true
	}
}

/// Create an embedding provider from provider type and model
pub async fn create_embedding_provider_from_parts(
	provider: &EmbeddingProviderType,
	model: &str,
) -> Result<Box<dyn EmbeddingProvider>> {
	match provider {
		EmbeddingProviderType::FastEmbed => {
			#[cfg(feature = "fastembed")]
			{
				Ok(Box::new(FastEmbedProviderImpl::new(model)?))
			}
			#[cfg(not(feature = "fastembed"))]
			{
				Err(anyhow::anyhow!("FastEmbed support is not compiled in. Please rebuild with --features fastembed"))
			}
		}
		EmbeddingProviderType::Jina => Ok(Box::new(JinaProviderImpl::new(model)?)),
		EmbeddingProviderType::Voyage => Ok(Box::new(VoyageProviderImpl::new(model)?)),
		EmbeddingProviderType::Google => Ok(Box::new(GoogleProviderImpl::new(model)?)),
		EmbeddingProviderType::OpenAI => Ok(Box::new(OpenAIProviderImpl::new(model)?)),
		EmbeddingProviderType::HuggingFace => {
			#[cfg(feature = "huggingface")]
			{
				Ok(Box::new(HuggingFaceProviderImpl::new(model).await?))
			}
			#[cfg(not(feature = "huggingface"))]
			{
				Err(anyhow::anyhow!("HuggingFace support is not compiled in. Please rebuild with --features huggingface"))
			}
		}
	}
}
