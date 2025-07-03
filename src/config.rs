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
use std::fs;
use std::path::PathBuf;

use crate::embedding::types::EmbeddingConfig;
use crate::storage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
	pub description_model: String,
	pub relationship_model: String,
	pub ai_batch_size: usize,
	pub max_batch_tokens: usize,
	pub batch_timeout_seconds: u64,
	pub fallback_to_individual: bool,
	pub max_sample_tokens: usize,
	pub confidence_threshold: f32,
	pub architectural_weight: f32,
	pub relationship_system_prompt: String,
	pub description_system_prompt: String,
}

// NOTE: This Default implementation should NEVER be used in practice
// All LLM values must come from the config template file
// This exists only to satisfy serde's requirements for deserialization
impl Default for LLMConfig {
	fn default() -> Self {
		panic!("LLM config must be loaded from template file - defaults not allowed")
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRAGConfig {
	pub enabled: bool,
	pub use_llm: bool,
	pub llm: LLMConfig,
}

// NOTE: This Default implementation should NEVER be used in practice
// All GraphRAG values must come from the config template file
// This exists only to satisfy serde's requirements for deserialization
impl Default for GraphRAGConfig {
	fn default() -> Self {
		panic!("GraphRAG config must be loaded from template file - defaults not allowed")
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRouterConfig {
	pub model: String,
	pub base_url: String,
	pub timeout: u64,
	pub api_key: Option<String>,
}

impl Default for OpenRouterConfig {
	fn default() -> Self {
		Self {
			model: "openai/gpt-4.1-mini".to_string(),
			base_url: "https://openrouter.ai/api/v1".to_string(),
			timeout: 120,
			api_key: None,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
	pub chunk_size: usize,
	pub chunk_overlap: usize,
	pub embeddings_batch_size: usize,

	/// Maximum tokens per batch for embeddings generation (global limit).
	/// This prevents API errors like "max allowed tokens per submitted batch is 120000".
	/// Uses tiktoken cl100k_base tokenizer for counting. Default: 100000
	pub embeddings_max_tokens_per_batch: usize,

	/// How often to flush data to storage during indexing (in batches).
	/// 1 = flush after every batch (safest, slower)
	/// 5 = flush every 5 batches (faster, less safe)
	/// Default: 1 for maximum data safety
	pub flush_frequency: usize,

	/// Require git repository for indexing (default: true)
	pub require_git: bool,
}

impl Default for IndexConfig {
	fn default() -> Self {
		Self {
			chunk_size: 2000,
			chunk_overlap: 100,
			embeddings_batch_size: 16,
			embeddings_max_tokens_per_batch: 100000,
			flush_frequency: 2,
			require_git: true,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
	pub max_results: usize,
	pub similarity_threshold: f32,
	pub top_k: usize,
	pub output_format: String,
	pub max_files: usize,
	pub context_lines: usize,

	/// Maximum characters to display per code/text/doc block in search results.
	/// If 0, displays full content. Default: 1000
	pub search_block_max_characters: usize,
}

impl Default for SearchConfig {
	fn default() -> Self {
		Self {
			max_results: 50,
			similarity_threshold: 0.6,
			top_k: 20,
			output_format: "markdown".to_string(),
			max_files: 20,
			context_lines: 3,
			search_block_max_characters: 1000,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
	/// Configuration version for future migrations
	#[serde(default = "default_version")]
	pub version: u32,

	#[serde(default)]
	pub openrouter: OpenRouterConfig,

	#[serde(default)]
	pub index: IndexConfig,

	#[serde(default)]
	pub search: SearchConfig,

	#[serde(default)]
	pub embedding: EmbeddingConfig,

	#[serde(default)]
	pub graphrag: GraphRAGConfig,
}

fn default_version() -> u32 {
	1
}

impl Default for Config {
	fn default() -> Self {
		Self {
			version: default_version(),
			openrouter: OpenRouterConfig::default(),
			index: IndexConfig::default(),
			search: SearchConfig::default(),
			embedding: EmbeddingConfig::default(),
			// This should never be reached - template loading should provide GraphRAG config
			graphrag: GraphRAGConfig::default(),
		}
	}
}

impl Config {
	pub fn load() -> Result<Self> {
		let config_path = Self::get_system_config_path()?;

		let mut config = if config_path.exists() {
			let content = fs::read_to_string(&config_path)?;
			toml::from_str(&content)?
		} else {
			// Load from template first, then save to system config
			let template_config = Self::load_from_template()?;

			// Ensure the parent directory exists
			if let Some(parent) = config_path.parent() {
				if !parent.exists() {
					fs::create_dir_all(parent)?;
				}
			}

			// Save template as the new config
			let toml_content = toml::to_string_pretty(&template_config)?;
			fs::write(&config_path, toml_content)?;
			template_config
		};

		// Environment variables take precedence over config file values
		if let Ok(api_key) = std::env::var("OPENROUTER_API_KEY") {
			config.openrouter.api_key = Some(api_key);
		}

		Ok(config)
	}

	/// Load configuration from the default template
	pub fn load_from_template() -> Result<Self> {
		// Try to load from embedded template first
		let template_content = Self::get_default_template_content()?;
		let config: Config = toml::from_str(&template_content)?;
		Ok(config)
	}

	/// Get the default template content
	fn get_default_template_content() -> Result<String> {
		// First try to read from config-templates/default.toml in the current directory
		let template_path = std::path::Path::new("config-templates/default.toml");
		if template_path.exists() {
			return Ok(fs::read_to_string(template_path)?);
		}

		// If not found, use embedded template
		Ok(include_str!("../config-templates/default.toml").to_string())
	}

	pub fn save(&self) -> Result<()> {
		let config_path = Self::get_system_config_path()?;

		// Ensure the parent directory exists
		if let Some(parent) = config_path.parent() {
			if !parent.exists() {
				fs::create_dir_all(parent)?;
			}
		}

		let toml_content = toml::to_string_pretty(self)?;
		fs::write(config_path, toml_content)?;
		Ok(())
	}

	/// Get the system-wide config file path
	/// Stored at ~/.local/share/octocode/config.toml (same level as fastembed cache)
	pub fn get_system_config_path() -> Result<PathBuf> {
		let system_storage = storage::get_system_storage_dir()?;
		Ok(system_storage.join("config.toml"))
	}

	pub fn get_model(&self) -> &str {
		&self.openrouter.model
	}

	pub fn get_base_url(&self) -> &str {
		&self.openrouter.base_url
	}

	pub fn get_timeout(&self) -> u64 {
		self.openrouter.timeout
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_default_config() {
		// Use template loading instead of Config::default() to avoid GraphRAG panic
		let config = Config::load_from_template().expect("Failed to load template config");
		assert_eq!(config.version, 1);
		assert_eq!(config.openrouter.model, "openai/gpt-4.1-mini");
		assert_eq!(config.index.chunk_size, 2000);
		assert_eq!(config.search.max_results, 20);

		assert_eq!(
			config.embedding.get_active_provider(),
			crate::embedding::types::EmbeddingProviderType::Voyage
		);
		// Test new GraphRAG configuration structure
		assert!(!config.graphrag.enabled);
		assert!(!config.graphrag.use_llm);
		assert_eq!(config.graphrag.llm.description_model, "openai/gpt-4.1-mini");
		assert_eq!(
			config.graphrag.llm.relationship_model,
			"openai/gpt-4.1-mini"
		);
		assert_eq!(config.graphrag.llm.ai_batch_size, 8);
		assert_eq!(config.graphrag.llm.max_batch_tokens, 16384);
		assert_eq!(config.graphrag.llm.batch_timeout_seconds, 60);
		assert!(config.graphrag.llm.fallback_to_individual);
		assert_eq!(config.graphrag.llm.max_sample_tokens, 1500);
		assert_eq!(config.graphrag.llm.confidence_threshold, 0.8);
		assert_eq!(config.graphrag.llm.architectural_weight, 0.9);
		assert!(config
			.graphrag
			.llm
			.relationship_system_prompt
			.contains("expert software architect"));
		assert!(config
			.graphrag
			.llm
			.description_system_prompt
			.contains("ROLE and PURPOSE"));
	}

	#[test]
	fn test_template_loading() {
		let result = Config::load_from_template();
		assert!(result.is_ok(), "Should be able to load from template");

		let config = result.unwrap();
		assert_eq!(config.version, 1);
		assert_eq!(config.openrouter.model, "openai/gpt-4.1-mini");
		assert_eq!(config.index.chunk_size, 2000);
		assert_eq!(config.search.max_results, 20);
		assert_eq!(config.embedding.code_model, "voyage:voyage-code-3");
		assert_eq!(config.embedding.text_model, "voyage:voyage-3.5-lite");
		// Test new GraphRAG configuration structure from template
		assert!(!config.graphrag.enabled);
		assert!(!config.graphrag.use_llm);
		assert_eq!(config.graphrag.llm.description_model, "openai/gpt-4.1-mini");
		assert_eq!(
			config.graphrag.llm.relationship_model,
			"openai/gpt-4.1-mini"
		);
		assert_eq!(config.graphrag.llm.ai_batch_size, 8);
		assert_eq!(config.graphrag.llm.max_batch_tokens, 16384);
		assert_eq!(config.graphrag.llm.batch_timeout_seconds, 60);
		assert!(config.graphrag.llm.fallback_to_individual);
		assert_eq!(config.graphrag.llm.max_sample_tokens, 1500);
		assert_eq!(config.graphrag.llm.confidence_threshold, 0.8);
		assert_eq!(config.graphrag.llm.architectural_weight, 0.9);
		assert!(config
			.graphrag
			.llm
			.relationship_system_prompt
			.contains("expert software architect"));
		assert!(config
			.graphrag
			.llm
			.description_system_prompt
			.contains("ROLE and PURPOSE"));
	}

	#[test]
	#[should_panic(expected = "GraphRAG config must be loaded from template file")]
	fn test_graphrag_default_panics() {
		// Verify that GraphRAGConfig::default() panics to enforce strict config loading
		let _ = GraphRAGConfig::default();
	}
}
