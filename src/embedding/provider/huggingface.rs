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
 * HuggingFace Provider Implementation
 *
 * This module provides local embedding generation using HuggingFace models via the Candle library.
 * It supports multiple model architectures with safetensors format from the HuggingFace Hub.
 *
 * Key features:
 * - Automatic model downloading and caching
 * - Local CPU-based inference (GPU support can be added)
 * - Thread-safe model cache for efficient reuse
 * - Mean pooling and L2 normalization for sentence embeddings
 * - Full compatibility with provider:model syntax
 * - Dynamic model architecture detection
 *
 * Usage:
 * - Set provider: `octocode config --embedding-provider huggingface`
 * - Set models: `octocode config --code-embedding-model "huggingface:jinaai/jina-embeddings-v2-base-code"`
 * - Popular models: jinaai/jina-embeddings-v2-base-code, sentence-transformers/all-mpnet-base-v2
 *
 * Models are automatically downloaded to the system cache directory and reused across sessions.
 */

// When huggingface feature is enabled
#[cfg(feature = "huggingface")]
use anyhow::{Context, Result};
#[cfg(feature = "huggingface")]
use candle_core::{DType, Device, Tensor};
#[cfg(feature = "huggingface")]
use candle_nn::VarBuilder;
#[cfg(feature = "huggingface")]
use candle_transformers::models::bert::{BertModel, Config as BertConfig};
use candle_transformers::models::jina_bert::Config as JinaBertConfig;
#[cfg(feature = "huggingface")]
use hf_hub::{api::tokio::Api, Repo, RepoType};
#[cfg(feature = "huggingface")]
use std::collections::HashMap;
#[cfg(feature = "huggingface")]
use std::sync::Arc;
#[cfg(feature = "huggingface")]
use tokenizers::Tokenizer;
#[cfg(feature = "huggingface")]
use tokio::sync::RwLock;

#[cfg(feature = "huggingface")]
/// HuggingFace model instance
pub struct HuggingFaceModel {
	model: BertModel,
	tokenizer: Tokenizer,
	device: Device,
}

#[cfg(feature = "huggingface")]
impl HuggingFaceModel {
	/// Load a SentenceTransformer model from HuggingFace Hub
	pub async fn load(model_name: &str) -> Result<Self> {
		let device = Device::Cpu; // Use CPU for now, can be extended to support GPU

		// Use our custom cache directory for consistency with FastEmbed
		// Set HF_HOME environment variable to control where models are downloaded
		let cache_dir = crate::storage::get_huggingface_cache_dir()
			.context("Failed to get HuggingFace cache directory")?;

		// Set the HuggingFace cache directory via environment variable
		std::env::set_var("HF_HOME", &cache_dir);

		// Download model files from HuggingFace Hub with proper error handling
		let api = Api::new().context("Failed to initialize HuggingFace API")?;
		let repo = api.repo(Repo::new(model_name.to_string(), RepoType::Model));

		// Download required files with enhanced error handling
		let config_path = repo
			.get("config.json")
			.await
			.with_context(|| format!("Failed to download config.json for model: {}", model_name))?;

		// Load tokenizer - try different formats
		let tokenizer = if let Ok(tokenizer_json_path) = repo.get("tokenizer.json").await {
			// Direct tokenizer.json file (most models)
			Tokenizer::from_file(tokenizer_json_path)
				.map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?
		} else {
			// Try to build tokenizer from components (for models like microsoft/codebert-base)
			// Check for RoBERTa-style tokenizer (vocab.json + merges.txt)
			if let (Ok(vocab_path), Ok(merges_path)) =
				(repo.get("vocab.json").await, repo.get("merges.txt").await)
			{
				// Build RoBERTa/GPT2-style BPE tokenizer using BPE::from_file
				use tokenizers::{
					models::bpe::BPE, normalizers, pre_tokenizers::byte_level::ByteLevel,
					processors::roberta::RobertaProcessing,
				};

				// Use BPE::from_file which handles the vocab and merges loading
				let bpe = BPE::from_file(
					vocab_path
						.to_str()
						.ok_or_else(|| anyhow::anyhow!("Invalid vocab path"))?,
					merges_path
						.to_str()
						.ok_or_else(|| anyhow::anyhow!("Invalid merges path"))?,
				)
				.unk_token("<unk>".to_string())
				.build()
				.map_err(|e| anyhow::anyhow!("Failed to build BPE tokenizer: {:?}", e))?;

				let mut tokenizer = Tokenizer::new(bpe);

				// Add ByteLevel pre-tokenizer (for RoBERTa)
				tokenizer.with_pre_tokenizer(Some(ByteLevel::default()));

				// Add RoBERTa post-processing
				let post_processor = RobertaProcessing::new(
					("</s>".to_string(), 2), // SEP token
					("<s>".to_string(), 0),  // CLS token
				)
				.trim_offsets(false)
				.add_prefix_space(true);
				tokenizer.with_post_processor(Some(post_processor));

				// Add normalizer
				let normalizer =
					normalizers::Sequence::new(vec![normalizers::Strip::new(true, true).into()]);
				tokenizer.with_normalizer(Some(normalizer));

				tokenizer
			} else {
				return Err(anyhow::anyhow!(
					"Could not find tokenizer files for model: {}. \
					Expected either tokenizer.json or (vocab.json + merges.txt). \
					This model may not be compatible.",
					model_name
				));
			}
		};

		// Try different weight file formats
		let weights_path = if let Ok(path) = repo.get("model.safetensors").await {
			path
		} else if let Ok(path) = repo.get("pytorch_model.bin").await {
			path
		} else {
			return Err(anyhow::anyhow!(
				"Could not find model weights in safetensors or pytorch format"
			));
		};

		// Load configuration
		let config_content = std::fs::read_to_string(config_path)?;
		let config: BertConfig = serde_json::from_str(&config_content)?;

		// Load model weights - only support safetensors for now
		let weights = if weights_path.to_string_lossy().ends_with(".safetensors") {
			candle_core::safetensors::load(&weights_path, &device)?
		} else {
			return Err(anyhow::anyhow!("PyTorch .bin format not supported in this implementation. Please use a model with safetensors format."));
		};

		let var_builder = VarBuilder::from_tensors(weights, DType::F32, &device);

		// Create model
		let model = BertModel::load(var_builder, &config)?;

		Ok(Self {
			model,
			tokenizer,
			device,
		})
	}

	/// Generate embeddings for a single text
	pub fn encode(&self, text: &str) -> Result<Vec<f32>> {
		self.encode_batch(&[text.to_string()])
			.map(|embeddings| embeddings.into_iter().next().unwrap_or_default())
	}

	/// Generate embeddings for multiple texts
	pub fn encode_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
		let mut all_embeddings = Vec::new();

		for text in texts {
			// Tokenize input - convert String to &str
			let encoding = self
				.tokenizer
				.encode(text.as_str(), true)
				.map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?;

			let tokens = encoding.get_ids();
			let token_ids = Tensor::new(tokens, &self.device)?.unsqueeze(0)?; // Add batch dimension

			// Create attention mask (all 1s for valid tokens)
			let attention_mask = Tensor::ones((1, tokens.len()), DType::U8, &self.device)?;

			// Run through model - BertModel.forward takes 3 arguments: input_ids, attention_mask, token_type_ids
			let output = self.model.forward(&token_ids, &attention_mask, None)?;

			// Apply mean pooling to get sentence embedding
			let embeddings = self.mean_pooling(&output, &attention_mask)?;

			// Normalize embeddings
			let normalized = self.normalize(&embeddings)?;

			// Convert to Vec<f32>
			let embedding_vec = normalized.to_vec1::<f32>()?;
			all_embeddings.push(embedding_vec);
		}

		Ok(all_embeddings)
	}

	/// Mean pooling operation
	fn mean_pooling(&self, hidden_states: &Tensor, attention_mask: &Tensor) -> Result<Tensor> {
		// Convert attention mask to f32 and expand dimensions
		let attention_mask = attention_mask.to_dtype(DType::F32)?;
		let attention_mask = attention_mask.unsqueeze(2)?; // (batch_size, seq_len, 1)

		// Apply attention mask to hidden states
		let masked_hidden_states = hidden_states.mul(&attention_mask)?;

		// Sum along sequence dimension
		let sum_hidden_states = masked_hidden_states.sum(1)?; // (batch_size, hidden_size)

		// Sum attention mask to get actual sequence lengths
		let sum_mask = attention_mask.sum(1)?; // (batch_size, 1)

		// Compute mean
		let mean_pooled = sum_hidden_states.div(&sum_mask)?;

		Ok(mean_pooled)
	}

	/// Normalize embeddings to unit vectors
	fn normalize(&self, embeddings: &Tensor) -> Result<Tensor> {
		let norm = embeddings.sqr()?.sum_keepdim(1)?.sqrt()?;
		Ok(embeddings.div(&norm)?)
	}
}

#[cfg(feature = "huggingface")]
// Global cache for loaded models using async-compatible RwLock
lazy_static::lazy_static! {
	static ref MODEL_CACHE: Arc<RwLock<HashMap<String, Arc<HuggingFaceModel>>>> =
		Arc::new(RwLock::new(HashMap::new()));
}

#[cfg(feature = "huggingface")]
/// HuggingFace provider implementation
pub struct HuggingFaceProvider;

#[cfg(feature = "huggingface")]
impl HuggingFaceProvider {
	/// Get or load a model from cache
	async fn get_model(model_name: &str) -> Result<Arc<HuggingFaceModel>> {
		{
			let cache = MODEL_CACHE.read().await;
			if let Some(model) = cache.get(model_name) {
				return Ok(model.clone());
			}
		}

		// Model not in cache, load it
		let model = HuggingFaceModel::load(model_name)
			.await
			.with_context(|| format!("Failed to load HuggingFace model: {}", model_name))?;

		let model_arc = Arc::new(model);

		// Add to cache
		{
			let mut cache = MODEL_CACHE.write().await;
			cache.insert(model_name.to_string(), model_arc.clone());
		}

		Ok(model_arc)
	}

	/// Generate embeddings for a single text
	pub async fn generate_embeddings(contents: &str, model: &str) -> Result<Vec<f32>> {
		let model_instance = Self::get_model(model).await?;

		// Run encoding in a blocking task to avoid blocking async runtime
		let contents = contents.to_string();
		let result =
			tokio::task::spawn_blocking(move || model_instance.encode(&contents)).await??;

		Ok(result)
	}

	/// Generate batch embeddings for multiple texts
	pub async fn generate_embeddings_batch(
		texts: Vec<String>,
		model: &str,
	) -> Result<Vec<Vec<f32>>> {
		let model_instance = Self::get_model(model).await?;

		// Run encoding in a blocking task to avoid blocking async runtime
		let result =
			tokio::task::spawn_blocking(move || model_instance.encode_batch(&texts)).await??;

		Ok(result)
	}
}

// Stubs for when huggingface feature is disabled
#[cfg(not(feature = "huggingface"))]
use anyhow::Result;

#[cfg(not(feature = "huggingface"))]
pub struct HuggingFaceProvider;

#[cfg(not(feature = "huggingface"))]
impl HuggingFaceProvider {
	pub async fn generate_embeddings(_contents: &str, _model: &str) -> Result<Vec<f32>> {
		Err(anyhow::anyhow!(
			"HuggingFace support is not compiled in. Please rebuild with --features huggingface"
		))
	}

	pub async fn generate_embeddings_batch(
		_texts: Vec<String>,
		_model: &str,
	) -> Result<Vec<Vec<f32>>> {
		Err(anyhow::anyhow!(
			"HuggingFace support is not compiled in. Please rebuild with --features huggingface"
		))
	}
}
use super::super::types::InputType;
use super::EmbeddingProvider;

/// HuggingFace provider implementation for trait
#[cfg(feature = "huggingface")]
pub struct HuggingFaceProviderImpl {
	model_name: String,
	dimension: usize,
}

#[cfg(feature = "huggingface")]
impl HuggingFaceProviderImpl {
	pub async fn new(model: &str) -> Result<Self> {
		#[cfg(not(feature = "huggingface"))]
		{
			Err(anyhow::anyhow!("HuggingFace provider requires 'huggingface' feature to be enabled. Cannot validate model '{}' without Hub API access.", model))
		}

		#[cfg(feature = "huggingface")]
		{
			let dimension = Self::get_model_dimension(model).await?;
			Ok(Self {
				model_name: model.to_string(),
				dimension,
			})
		}
	}

	#[cfg(feature = "huggingface")]
	async fn get_model_dimension(model: &str) -> Result<usize> {
		Self::get_dimension_from_config(model).await
	}

	/// Get model dimension using Candle config structs (like examples)
	#[cfg(feature = "huggingface")]
	async fn get_dimension_from_config(model_name: &str) -> Result<usize> {
		// Download config.json
		let config_json = Self::download_config_direct(model_name).await?;

		// Try different Candle config types - JinaBert first, then standard Bert
		if let Ok(config) = Self::parse_as_jina_bert_config(&config_json) {
			return Ok(config.hidden_size);
		}

		if let Ok(config) = Self::parse_as_bert_config(&config_json) {
			return Ok(config.hidden_size);
		}

		// Fallback to JSON parsing
		Self::parse_hidden_size_from_json(&config_json, model_name)
	}

	/// Try to parse config as JinaBert config (for Jina models)
	#[cfg(feature = "huggingface")]
	fn parse_as_jina_bert_config(config_json: &str) -> Result<JinaBertConfig> {
		serde_json::from_str::<JinaBertConfig>(config_json)
			.map_err(|e| anyhow::anyhow!("Failed to parse as JinaBertConfig: {}", e))
	}

	/// Try to parse config as standard Candle BertConfig
	#[cfg(feature = "huggingface")]
	fn parse_as_bert_config(
		config_json: &str,
	) -> Result<candle_transformers::models::bert::Config> {
		use candle_transformers::models::bert::Config as BertConfig;
		serde_json::from_str::<BertConfig>(config_json)
			.map_err(|e| anyhow::anyhow!("Failed to parse as BertConfig: {}", e))
	}

	/// Parse hidden_size from JSON config flexibly
	#[cfg(feature = "huggingface")]
	fn parse_hidden_size_from_json(config_json: &str, model_name: &str) -> Result<usize> {
		use serde_json::Value;

		let config: Value = serde_json::from_str(config_json).with_context(|| {
			format!(
				"Failed to parse config.json as JSON for model: {}",
				model_name
			)
		})?;

		// Try different field names that contain embedding dimensions
		let dimension_fields = ["hidden_size", "d_model", "embedding_size", "dim"];

		for field in &dimension_fields {
			if let Some(dim) = config.get(field).and_then(|v| v.as_u64()) {
				tracing::debug!(
					"Found dimension {} for model {} from config.json field '{}'",
					dim,
					model_name,
					field
				);
				return Ok(dim as usize);
			}
		}

		Err(anyhow::anyhow!(
			"No dimension field found in config.json for model '{}'. \
			Searched for fields: {:?}. Available fields: {:?}",
			model_name,
			dimension_fields,
			config
				.as_object()
				.map(|obj| obj.keys().collect::<Vec<_>>())
				.unwrap_or_default()
		))
	}

	/// Download config.json directly from HuggingFace Hub using HTTP
	#[cfg(feature = "huggingface")]
	async fn download_config_direct(model_name: &str) -> Result<String> {
		use reqwest;

		// Construct direct URL to config.json
		let config_url = format!("https://huggingface.co/{}/raw/main/config.json", model_name);

		tracing::debug!("Downloading config from: {}", config_url);

		// Use reqwest for direct HTTP download
		let client = reqwest::Client::new();
		let response = client
			.get(&config_url)
			.header("User-Agent", "octocode/0.7.1")
			.send()
			.await
			.with_context(|| format!("Failed to download config.json from {}", config_url))?;

		if !response.status().is_success() {
			return Err(anyhow::anyhow!(
				"Failed to download config.json for model '{}'. HTTP status: {}. \
				This could be due to:\n\
				1. Model doesn't exist on HuggingFace Hub\n\
				2. Network connectivity issues\n\
				3. Model is private and requires authentication\n\
				4. Model doesn't have a config.json file",
				model_name,
				response.status()
			));
		}

		let config_text = response.text().await.with_context(|| {
			format!(
				"Failed to read config.json response for model: {}",
				model_name
			)
		})?;

		Ok(config_text)
	}
}

#[cfg(feature = "huggingface")]
#[async_trait::async_trait]
impl EmbeddingProvider for HuggingFaceProviderImpl {
	async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
		HuggingFaceProvider::generate_embeddings(text, &self.model_name).await
	}

	async fn generate_embeddings_batch(
		&self,
		texts: Vec<String>,
		input_type: InputType,
	) -> Result<Vec<Vec<f32>>> {
		// Apply prefix manually for HuggingFace (doesn't support input_type API)
		let processed_texts: Vec<String> = texts
			.into_iter()
			.map(|text| input_type.apply_prefix(&text))
			.collect();
		HuggingFaceProvider::generate_embeddings_batch(processed_texts, &self.model_name).await
	}

	fn get_dimension(&self) -> usize {
		self.dimension
	}

	fn is_model_supported(&self) -> bool {
		// For HuggingFace, we support many models, so return true for most cases
		// The actual validation happens when trying to load the model
		true
	}
}

#[cfg(all(test, feature = "huggingface"))]
mod tests {
	#[test]
	fn test_roberta_tokenizer_building() {
		// Test that we can build a RoBERTa-style tokenizer using BPE::from_file approach
		use tokenizers::{
			models::bpe::BPE, pre_tokenizers::byte_level::ByteLevel,
			processors::roberta::RobertaProcessing, Tokenizer,
		};

		// Create temporary files for testing
		let vocab_file = std::env::temp_dir().join("test_vocab.json");
		let merges_file = std::env::temp_dir().join("test_merges.txt");

		// Write test vocab - must include all tokens used in merges
		let vocab_content = r#"{"<s>":0,"<pad>":1,"</s>":2,"<unk>":3,"h":4,"e":5,"l":6,"o":7,"r":8,"he":9,"ll":10,"or":11,"hello":12,"world":13}"#;
		std::fs::write(&vocab_file, vocab_content).expect("Failed to write vocab");

		// Write test merges
		let merges_content = "#version: 0.2\nh e\nl l\no r";
		std::fs::write(&merges_file, merges_content).expect("Failed to write merges");

		// Build BPE model using from_file
		let bpe = BPE::from_file(vocab_file.to_str().unwrap(), merges_file.to_str().unwrap())
			.unk_token("<unk>".to_string())
			.build()
			.expect("Failed to build BPE tokenizer");

		let mut tokenizer = Tokenizer::new(bpe);

		// Add ByteLevel pre-tokenizer (for RoBERTa)
		tokenizer.with_pre_tokenizer(Some(ByteLevel::default()));

		// Add RoBERTa post-processing
		let post_processor = RobertaProcessing::new(
			("</s>".to_string(), 2), // SEP token
			("<s>".to_string(), 0),  // CLS token
		)
		.trim_offsets(false)
		.add_prefix_space(true);
		tokenizer.with_post_processor(Some(post_processor));

		// Test that tokenizer works
		let test_text = "hello world";
		let encoding = tokenizer
			.encode(test_text, false)
			.expect("Failed to encode");

		assert!(
			!encoding.get_ids().is_empty(),
			"Encoding should produce tokens"
		);
		println!("✓ RoBERTa-style tokenizer built successfully using BPE::from_file");

		// Clean up
		let _ = std::fs::remove_file(vocab_file);
		let _ = std::fs::remove_file(merges_file);
	}

	#[test]
	fn test_merges_parsing() {
		// Test that we correctly parse merges.txt format
		let merges_content = r#"#version: 0.2
Ġ t
Ġ a
h e
Ġt he
i n"#;

		let merges: Vec<(String, String)> = merges_content
			.lines()
			.skip(1) // Skip header line
			.filter_map(|line| {
				let parts: Vec<&str> = line.split_whitespace().collect();
				if parts.len() == 2 {
					Some((parts[0].to_string(), parts[1].to_string()))
				} else {
					None
				}
			})
			.collect();

		assert_eq!(merges.len(), 5);
		assert_eq!(merges[0], ("Ġ".to_string(), "t".to_string()));
		println!("✓ Merges parsing works correctly");
	}
}
