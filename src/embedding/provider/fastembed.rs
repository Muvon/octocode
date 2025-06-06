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

/*!
* FastEmbed Provider Implementation
*
* This module provides embedding generation using the FastEmbed library.
* FastEmbed offers fast, local embedding generation with automatic model downloading and caching.
*
* Key features:
* - Automatic model downloading and caching
* - Local CPU-based inference with optimized performance
* - Thread-safe model instances
* - Support for various embedding models
* - No API keys required
*
* Usage:
* - Set provider: `octocode config --embedding-provider fastembed`
* - Set models: `octocode config --code-embedding-model "fastembed:jinaai/jina-embeddings-v2-base-code"`
* - Popular models: sentence-transformers/all-MiniLM-L6-v2, BAAI/bge-base-en-v1.5, jinaai/jina-embeddings-v2-base-code
*
* Models are automatically downloaded to the system cache directory and reused across sessions.
*/

#[cfg(feature = "fastembed")]
use anyhow::{Context, Result};
#[cfg(feature = "fastembed")]
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
#[cfg(feature = "fastembed")]
use std::sync::Arc;

#[cfg(feature = "fastembed")]
use super::super::EmbeddingProvider;

#[cfg(feature = "fastembed")]
/// FastEmbed provider implementation for trait
pub struct FastEmbedProviderImpl {
	model: Arc<TextEmbedding>,
}

#[cfg(feature = "fastembed")]
impl FastEmbedProviderImpl {
	pub fn new(model_name: &str) -> Result<Self> {
		let model_enum = FastEmbedProvider::map_model_to_fastembed(model_name);

		// Use system-wide cache for FastEmbed models
		let cache_dir = crate::storage::get_fastembed_cache_dir()
			.context("Failed to get FastEmbed cache directory")?;

		let model = TextEmbedding::try_new(
			InitOptions::new(model_enum)
				.with_show_download_progress(true)
				.with_cache_dir(cache_dir),
		)
		.context("Failed to initialize FastEmbed model")?;

		Ok(Self {
			model: Arc::new(model),
		})
	}
}

#[cfg(feature = "fastembed")]
#[async_trait::async_trait]
impl EmbeddingProvider for FastEmbedProviderImpl {
	async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
		let text = text.to_string();
		let model = self.model.clone();

		let embedding = tokio::task::spawn_blocking(move || -> Result<Vec<f32>> {
			let embedding = model.embed(vec![text], None)?;

			if embedding.is_empty() {
				return Err(anyhow::anyhow!("No embeddings were generated"));
			}

			Ok(embedding[0].clone())
		})
		.await??;

		Ok(embedding)
	}

	async fn generate_embeddings_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
		let model = self.model.clone();

		let embeddings = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<f32>>> {
			let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
			let embeddings = model.embed(text_refs, None)?;

			Ok(embeddings)
		})
		.await??;

		Ok(embeddings)
	}
}

#[cfg(feature = "fastembed")]
/// FastEmbed provider implementation
pub struct FastEmbedProvider;

#[cfg(feature = "fastembed")]
impl FastEmbedProvider {
	/// Map model name to FastEmbed model enum
	pub fn map_model_to_fastembed(model: &str) -> EmbeddingModel {
		match model {
			"sentence-transformers/all-MiniLM-L6-v2" => EmbeddingModel::AllMiniLML6V2,
			"sentence-transformers/all-MiniLM-L6-v2-quantized" => EmbeddingModel::AllMiniLML6V2Q,
			"sentence-transformers/all-MiniLM-L12-v2" | "all-MiniLM-L12-v2" => {
				EmbeddingModel::AllMiniLML12V2
			}
			"sentence-transformers/all-MiniLM-L12-v2-quantized" => EmbeddingModel::AllMiniLML12V2Q,
			"BAAI/bge-base-en-v1.5" => EmbeddingModel::BGEBaseENV15,
			"BAAI/bge-base-en-v1.5-quantized" => EmbeddingModel::BGEBaseENV15Q,
			"BAAI/bge-large-en-v1.5" => EmbeddingModel::BGELargeENV15,
			"BAAI/bge-large-en-v1.5-quantized" => EmbeddingModel::BGELargeENV15Q,
			"BAAI/bge-small-en-v1.5" => EmbeddingModel::BGESmallENV15,
			"BAAI/bge-small-en-v1.5-quantized" => EmbeddingModel::BGESmallENV15Q,
			"nomic-ai/nomic-embed-text-v1" => EmbeddingModel::NomicEmbedTextV1,
			"nomic-ai/nomic-embed-text-v1.5" => EmbeddingModel::NomicEmbedTextV15,
			"nomic-ai/nomic-embed-text-v1.5-quantized" => EmbeddingModel::NomicEmbedTextV15Q,
			"sentence-transformers/paraphrase-MiniLM-L6-v2" => {
				EmbeddingModel::ParaphraseMLMiniLML12V2
			}
			"sentence-transformers/paraphrase-MiniLM-L6-v2-quantized" => {
				EmbeddingModel::ParaphraseMLMiniLML12V2Q
			}
			"sentence-transformers/paraphrase-mpnet-base-v2" => {
				EmbeddingModel::ParaphraseMLMpnetBaseV2
			}
			"BAAI/bge-small-zh-v1.5" => EmbeddingModel::BGESmallZHV15,
			"BAAI/bge-large-zh-v1.5" => EmbeddingModel::BGELargeZHV15,
			"lightonai/modernbert-embed-large" => EmbeddingModel::ModernBertEmbedLarge,
			"intfloat/multilingual-e5-small" | "multilingual-e5-small" => {
				EmbeddingModel::MultilingualE5Small
			}
			"intfloat/multilingual-e5-base" | "multilingual-e5-base" => {
				EmbeddingModel::MultilingualE5Base
			}
			"intfloat/multilingual-e5-large" | "multilingual-e5-large" => {
				EmbeddingModel::MultilingualE5Large
			}
			"mixedbread-ai/mxbai-embed-large-v1" => EmbeddingModel::MxbaiEmbedLargeV1,
			"mixedbread-ai/mxbai-embed-large-v1-quantized" => EmbeddingModel::MxbaiEmbedLargeV1Q,
			"Alibaba-NLP/gte-base-en-v1.5" => EmbeddingModel::GTEBaseENV15,
			"Alibaba-NLP/gte-base-en-v1.5-quantized" => EmbeddingModel::GTEBaseENV15Q,
			"Alibaba-NLP/gte-large-en-v1.5" => EmbeddingModel::GTELargeENV15,
			"Alibaba-NLP/gte-large-en-v1.5-quantized" => EmbeddingModel::GTELargeENV15Q,
			"Qdrant/clip-ViT-B-32-text" => EmbeddingModel::ClipVitB32,
			"jinaai/jina-embeddings-v2-base-code" => EmbeddingModel::JinaEmbeddingsV2BaseCode,
			_ => panic!("Unsupported embedding model: {}", model),
		}
	}
}

// Stubs for when fastembed feature is disabled
#[cfg(not(feature = "fastembed"))]
use anyhow::Result;

#[cfg(not(feature = "fastembed"))]
pub struct FastEmbedProviderImpl;

#[cfg(not(feature = "fastembed"))]
impl FastEmbedProviderImpl {
	pub fn new(_model_name: &str) -> Result<Self> {
		Err(anyhow::anyhow!(
			"FastEmbed support is not compiled in. Please rebuild with --features fastembed"
		))
	}
}

#[cfg(not(feature = "fastembed"))]
#[async_trait::async_trait]
impl super::super::EmbeddingProvider for FastEmbedProviderImpl {
	async fn generate_embedding(&self, _text: &str) -> Result<Vec<f32>> {
		Err(anyhow::anyhow!(
			"FastEmbed support is not compiled in. Please rebuild with --features fastembed"
		))
	}

	async fn generate_embeddings_batch(&self, _texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
		Err(anyhow::anyhow!(
			"FastEmbed support is not compiled in. Please rebuild with --features fastembed"
		))
	}
}

#[cfg(not(feature = "fastembed"))]
pub struct FastEmbedProvider;

#[cfg(not(feature = "fastembed"))]
impl FastEmbedProvider {
	pub fn map_model_to_fastembed(_model: &str) {
		// Return unit type when feature is disabled
	}
}
