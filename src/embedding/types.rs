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

use serde::{Deserialize, Serialize};

/// Supported embedding providers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingProviderType {
	FastEmbed,
	Jina,
	Voyage,
	Google,
	SentenceTransformer,
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
			"sentencetransformer" | "st" | "huggingface" | "hf" => {
				EmbeddingProviderType::SentenceTransformer
			}
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

	/// Get the vector dimension for a specific provider and model
	pub fn get_vector_dimension(&self, provider: &EmbeddingProviderType, model: &str) -> usize {
		match provider {
			EmbeddingProviderType::FastEmbed => match model {
				"sentence-transformers/all-MiniLM-L6-v2" => 384,
				"sentence-transformers/all-MiniLM-L6-v2-quantized" => 384,
				"sentence-transformers/all-MiniLM-L12-v2" => 768,
				"sentence-transformers/all-MiniLM-L12-v2-quantized" => 768,
				"BAAI/bge-base-en-v1.5" => 768,
				"BAAI/bge-base-en-v1.5-quantized" => 768,
				"BAAI/bge-large-en-v1.5" => 1024,
				"BAAI/bge-large-en-v1.5-quantized" => 1024,
				"BAAI/bge-small-en-v1.5" => 384,
				"BAAI/bge-small-en-v1.5-quantized" => 384,
				"nomic-ai/nomic-embed-text-v1" => 768,
				"nomic-ai/nomic-embed-text-v1.5" => 768,
				"nomic-ai/nomic-embed-text-v1.5-quantized" => 768,
				"sentence-transformers/paraphrase-MiniLM-L6-v2" => 384,
				"sentence-transformers/paraphrase-MiniLM-L6-v2-quantized" => 384,
				"sentence-transformers/paraphrase-mpnet-base-v2" => 768,
				"BAAI/bge-small-zh-v1.5" => 512,
				"BAAI/bge-large-zh-v1.5" => 1024,
				"lightonai/modernbert-embed-large" => 1024,
				"intfloat/multilingual-e5-small" | "multilingual-e5-small" => 384,
				"intfloat/multilingual-e5-base" | "multilingual-e5-base" => 768,
				"intfloat/multilingual-e5-large" | "multilingual-e5-large" => 1024,
				"mixedbread-ai/mxbai-embed-large-v1" => 1024,
				"mixedbread-ai/mxbai-embed-large-v1-quantized" => 1024,
				"Alibaba-NLP/gte-base-en-v1.5" => 768,
				"Alibaba-NLP/gte-base-en-v1.5-quantized" => 768,
				"Alibaba-NLP/gte-large-en-v1.5" => 1024,
				"Alibaba-NLP/gte-large-en-v1.5-quantized" => 1024,
				"Qdrant/clip-ViT-B-32-text" => 512,
				"jinaai/jina-embeddings-v2-base-code" => 768,
				_ => panic!("Unsupported embedding model: {}", model),
			},
			EmbeddingProviderType::Jina => match model {
				"jina-embeddings-v3" => 1024,
				"jina-embeddings-v2-base-en" => 768,
				"jina-embeddings-v2-base-code" => 768,
				"jina-embeddings-v2-small-en" => 512,
				"jina-clip-v1" => 768,
				_ => panic!("Unsupported embedding model: {}", model),
			},
			EmbeddingProviderType::Voyage => match model {
				"voyage-3.5" => 1024,
				"voyage-3.5-lite" => 1024,
				"voyage-3-large" => 1024,
				"voyage-code-2" => 1536,
				"voyage-code-3" => 1024,
				"voyage-finance-2" => 1024,
				"voyage-law-2" => 1024,
				"voyage-2" => 1024,
				_ => panic!("Unsupported embedding model: {}", model),
			},
			EmbeddingProviderType::Google => match model {
				"text-embedding-004" => 768,
				"text-embedding-preview-0409" => 768,
				"text-multilingual-embedding-002" => 768,
				_ => panic!("Unsupported embedding model: {}", model),
			},
			EmbeddingProviderType::SentenceTransformer => {
				// Common SentenceTransformer model dimensions
				match model {
					"sentence-transformers/all-MiniLM-L6-v2" => 384,
					"sentence-transformers/all-MiniLM-L12-v2" => 384,
					"sentence-transformers/all-mpnet-base-v2" => 768,
					"sentence-transformers/all-roberta-large-v1" => 1024,
					"sentence-transformers/paraphrase-MiniLM-L6-v2" => 384,
					"sentence-transformers/paraphrase-mpnet-base-v2" => 768,
					"microsoft/codebert-base" => 768,
					"microsoft/unixcoder-base" => 768,
					"sentence-transformers/multi-qa-mpnet-base-dot-v1" => 768,
					"BAAI/bge-small-en-v1.5" => 384,
					"BAAI/bge-base-en-v1.5" => 768,
					"BAAI/bge-large-en-v1.5" => 1024,
					_ => panic!("Unsupported embedding model: {}", model),
				}
			}
		}
	}
}
