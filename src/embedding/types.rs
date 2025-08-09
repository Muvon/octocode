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

/// Input type for embedding generation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputType {
	/// Default - no input_type (existing behavior)
	None,
	/// For search operations
	Query,
	/// For indexing operations
	Document,
}

impl Default for InputType {
	fn default() -> Self {
		Self::None
	}
}

impl InputType {
	/// Convert to API string for providers that support it (like Voyage)
	pub fn as_api_str(&self) -> Option<&'static str> {
		match self {
			InputType::None => None,
			InputType::Query => Some("query"),
			InputType::Document => Some("document"),
		}
	}

	/// Get prefix for manual injection (for providers that don't support input_type API)
	pub fn get_prefix(&self) -> Option<&'static str> {
		match self {
			InputType::None => None,
			InputType::Query => Some(crate::constants::QUERY_PREFIX),
			InputType::Document => Some(crate::constants::DOCUMENT_PREFIX),
		}
	}

	/// Apply prefix to text for manual injection
	pub fn apply_prefix(&self, text: &str) -> String {
		match self.get_prefix() {
			Some(prefix) => format!("{}{}", prefix, text),
			None => text.to_string(),
		}
	}
}

/// Supported embedding providers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingProviderType {
	FastEmbed,
	Jina,
	Voyage,
	Google,
	HuggingFace,
	OpenAI,
}

impl Default for EmbeddingProviderType {
	fn default() -> Self {
		#[cfg(feature = "fastembed")]
		{
			Self::FastEmbed
		}
		#[cfg(not(feature = "fastembed"))]
		{
			Self::Voyage
		}
	}
}

/// Configuration for embedding models (simplified)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
	/// Code embedding model (format: "provider:model")
	pub code_model: String,

	/// Text embedding model (format: "provider:model")
	pub text_model: String,
}

impl Default for EmbeddingConfig {
	fn default() -> Self {
		// Use FastEmbed models if available, otherwise fall back to Voyage
		#[cfg(feature = "fastembed")]
		{
			Self {
				code_model: "fastembed:jinaai/jina-embeddings-v2-base-code".to_string(),
				text_model: "fastembed:sentence-transformers/all-MiniLM-L6-v2-quantized"
					.to_string(),
			}
		}
		#[cfg(not(feature = "fastembed"))]
		{
			Self {
				code_model: "voyage:voyage-code-3".to_string(),
				text_model: "voyage:voyage-3.5-lite".to_string(),
			}
		}
	}
}

/// Parse provider and model from a string in format "provider:model"
pub fn parse_provider_model(input: &str) -> (EmbeddingProviderType, String) {
	if let Some((provider_str, model)) = input.split_once(':') {
		let provider = match provider_str.to_lowercase().as_str() {
			"fastembed" => EmbeddingProviderType::FastEmbed,
			"jinaai" | "jina" => EmbeddingProviderType::Jina,
			"voyageai" | "voyage" => EmbeddingProviderType::Voyage,
			"google" => EmbeddingProviderType::Google,
			"huggingface" | "hf" => EmbeddingProviderType::HuggingFace,
			"openai" => EmbeddingProviderType::OpenAI,
			_ => {
				// Default fallback - use FastEmbed if available, otherwise Voyage
				#[cfg(feature = "fastembed")]
				{
					EmbeddingProviderType::FastEmbed
				}
				#[cfg(not(feature = "fastembed"))]
				{
					EmbeddingProviderType::Voyage
				}
			}
		};
		(provider, model.to_string())
	} else {
		// Legacy format - assume FastEmbed if available, otherwise Voyage
		#[cfg(feature = "fastembed")]
		{
			(EmbeddingProviderType::FastEmbed, input.to_string())
		}
		#[cfg(not(feature = "fastembed"))]
		{
			(EmbeddingProviderType::Voyage, input.to_string())
		}
	}
}

impl EmbeddingConfig {
	/// Get the currently active provider based on the code model
	pub fn get_active_provider(&self) -> EmbeddingProviderType {
		let (provider, _) = parse_provider_model(&self.code_model);
		provider
	}

	/// Get API key for a specific provider (from environment variables only)
	pub fn get_api_key(&self, provider: &EmbeddingProviderType) -> Option<String> {
		match provider {
			EmbeddingProviderType::Jina => std::env::var("JINA_API_KEY").ok(),
			EmbeddingProviderType::Voyage => std::env::var("VOYAGE_API_KEY").ok(),
			EmbeddingProviderType::Google => std::env::var("GOOGLE_API_KEY").ok(),
			_ => None, // FastEmbed and SentenceTransformer don't need API keys
		}
	}

	/// Get vector dimension by creating a provider instance
	pub async fn get_vector_dimension(
		&self,
		provider: &EmbeddingProviderType,
		model: &str,
	) -> usize {
		// Try to create provider and get dimension
		match crate::embedding::provider::create_embedding_provider_from_parts(provider, model)
			.await
		{
			Ok(provider_impl) => provider_impl.get_dimension(),
			Err(e) => {
				panic!(
					"Failed to create provider for {:?}:{}: {}. Using fallback dimension.",
					provider, model, e
				);
			}
		}
	}

	/// Validate model by trying to create provider
	pub async fn validate_model(
		&self,
		provider: &EmbeddingProviderType,
		model: &str,
	) -> Result<()> {
		let provider_impl =
			crate::embedding::provider::create_embedding_provider_from_parts(provider, model)
				.await?;
		if !provider_impl.is_model_supported() {
			return Err(anyhow::anyhow!(
				"Model {} is not supported by provider {:?}",
				model,
				provider
			));
		}
		Ok(())
	}
}
