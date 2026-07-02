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

impl Default for LLMConfig {
	fn default() -> Self {
		Self {
			description_model: "openrouter:openai/gpt-4o-mini".to_string(),
			relationship_model: "openrouter:openai/gpt-4o-mini".to_string(),
			ai_batch_size: 8,
			max_batch_tokens: 16384,
			batch_timeout_seconds: 60,
			fallback_to_individual: true,
			max_sample_tokens: 1500,
			confidence_threshold: 0.6,
			architectural_weight: 0.9,
			relationship_system_prompt: DEFAULT_RELATIONSHIP_SYSTEM_PROMPT.to_string(),
			description_system_prompt: DEFAULT_DESCRIPTION_SYSTEM_PROMPT.to_string(),
		}
	}
}

const DEFAULT_RELATIONSHIP_SYSTEM_PROMPT: &str = "You are an expert software architect specializing in code analysis. Analyze the provided code files and identify meaningful ARCHITECTURAL relationships that go beyond simple imports.

Focus on these relationship types:
- 'imports': Module/package imports and dependencies
- 'implements': Interface implementation, trait implementation
- 'extends': Class inheritance, module extension
- 'calls': Function/method calls between modules
- 'uses': Utility usage, service consumption
- 'configures': Configuration setup, dependency injection
- 'factory_creates': Factory pattern instantiation
- 'observer_pattern': Event listening, callback registration
- 'strategy_pattern': Algorithm selection, behavior delegation
- 'adapter_pattern': Interface adaptation, wrapper usage
- 'architectural_dependency': High-level system dependencies

Respond with a JSON array of relationships. Each relationship must include:
- source_path: relative path of source file
- target_path: relative path of target file
- relation_type: one of the types listed above
- description: specific explanation of HOW the relationship works
- confidence: 0.0-1.0 confidence score (use 0.8+ for clear relationships)

Only include relationships with clear architectural significance. Avoid trivial imports.";

const DEFAULT_DESCRIPTION_SYSTEM_PROMPT: &str = "You are a senior software engineer analyzing code architecture. Provide a concise 2-3 sentence description of the file's ROLE and PURPOSE in the system.

Focus on:
- What architectural layer this file belongs to (API, business logic, data access, utilities, etc.)
- Its primary responsibility and how it contributes to the system
- Key patterns or architectural decisions it implements

Avoid listing specific functions/classes. Instead, describe the file's architectural significance and how it fits into the larger system design.";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphRAGConfig {
	pub enabled: bool,
	pub use_llm: bool,
	pub llm: LLMConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
	pub model: String,
	pub timeout: u64,
	pub temperature: f32,
	pub max_tokens: usize,
}

impl Default for LlmConfig {
	fn default() -> Self {
		Self {
			model: "openrouter:openai/gpt-4o-mini".to_string(),
			timeout: 120,
			temperature: 0.7,
			max_tokens: 4000,
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

	/// Enable RaBitQ quantization for vector indexes (default: true)
	/// When enabled, uses IVF_RQ (32x compression) instead of IVF_HNSW_SQ (4x compression)
	/// RaBitQ provides better storage efficiency while maintaining good recall
	pub quantization: bool,

	/// Enable LLM-generated contextual descriptions for code chunks (default: false)
	/// When enabled, uses the configured LLM to generate natural language descriptions
	/// of code chunks at indexing time. These descriptions are prepended to chunk content
	/// before embedding (not stored), improving search recall by ~35-67%.
	/// Structural context (file path, language, symbols) is ALWAYS prepended regardless.
	#[serde(default)]
	pub contextual_descriptions: bool,

	/// Model for contextual description generation in provider:model format
	pub contextual_model: String,

	/// Number of code chunks per LLM description batch (default: 10)
	#[serde(default = "default_contextual_batch_size")]
	pub contextual_batch_size: usize,

	/// When `true`, the MCP server runs background indexing + a file watcher to keep
	/// the index fresh while serving. When `false` (default), MCP serves search,
	/// `view_signatures`, and `structural_search` over the EXISTING index in read-only
	/// mode and never (re)indexes in-process. The `index` CLI command is unaffected —
	/// this gates only the in-process MCP indexer.
	pub mcp_index: bool,
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
			quantization: true,
			contextual_descriptions: false,
			contextual_model: "openrouter:openai/gpt-4o-mini".to_string(),
			contextual_batch_size: 10,
			mcp_index: false,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankerConfig {
	/// Enable reranking for search results
	pub enabled: bool,
	/// Reranker model in provider:model format (e.g., "voyage:rerank-2.5")
	pub model: String,
	/// Number of candidates to retrieve before reranking
	pub top_k_candidates: usize,
	/// Number of results to return after reranking
	pub final_top_k: usize,
}

/// Hybrid search configuration for combining vector and keyword search.
///
/// FTS is BM25 over the `content` column only — there is no multi-field
/// keyword scoring, so per-field weights (path/symbols/title) would have no
/// effect. Only the two RRF fusion weights below are wired into the search
/// pipeline (`WeightedRRFReranker` in `store::weighted_rrf`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchConfig {
	/// Enable hybrid search (vector + keyword)
	pub enabled: bool,
	/// Default weight for vector similarity signal in RRF fusion
	pub default_vector_weight: f32,
	/// Default weight for keyword (BM25) signal in RRF fusion
	pub default_keyword_weight: f32,
}

impl Default for HybridSearchConfig {
	fn default() -> Self {
		Self {
			enabled: true,
			default_vector_weight: 0.6,
			default_keyword_weight: 0.4,
		}
	}
}

impl Default for RerankerConfig {
	fn default() -> Self {
		Self {
			enabled: true,
			model: "fastembed:jina-reranker-v2-base-multilingual".to_string(),
			top_k_candidates: 50,
			final_top_k: 10,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
	pub max_results: usize,
	pub similarity_threshold: f32,
	pub output_format: String,
	pub max_files: usize,
	pub context_lines: usize,

	/// Maximum characters to display per code/text/doc block in search results.
	/// If 0, displays full content. Default: 400
	pub search_block_max_characters: usize,

	/// Reranker configuration for improving search result accuracy
	pub reranker: RerankerConfig,

	/// Hybrid search configuration for combining vector and keyword search
	pub hybrid: HybridSearchConfig,

	/// Expand code-search candidates with structurally-related files via the
	/// GraphRAG graph (imports/calls/extends) before reranking. File-level,
	/// best-effort, requires `graphrag.enabled`. Set false to disable — A/B on
	/// your eval before enabling, since naive expansion can dilute precision.
	pub graph_expansion: bool,
}

impl Default for SearchConfig {
	fn default() -> Self {
		Self {
			max_results: 20,
			similarity_threshold: 0.65,
			output_format: "markdown".to_string(),
			max_files: 10,
			context_lines: 3,
			search_block_max_characters: 400,
			reranker: RerankerConfig::default(),
			hybrid: HybridSearchConfig::default(),
			graph_expansion: false,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommitsConfig {
	/// Use LLM to generate rich descriptions of commit diffs
	pub use_llm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
	/// Configuration version for future migrations
	#[serde(default = "default_version")]
	pub version: u32,

	#[serde(default)]
	pub llm: LlmConfig,

	#[serde(default)]
	pub index: IndexConfig,

	#[serde(default)]
	pub search: SearchConfig,

	#[serde(default)]
	pub embedding: EmbeddingConfig,

	#[serde(default)]
	pub graphrag: GraphRAGConfig,

	#[serde(default)]
	pub commits: CommitsConfig,
}

fn default_version() -> u32 {
	1
}

fn default_contextual_batch_size() -> usize {
	10
}

impl Default for Config {
	fn default() -> Self {
		Self {
			version: default_version(),
			llm: LlmConfig::default(),
			index: IndexConfig::default(),
			search: SearchConfig::default(),
			embedding: EmbeddingConfig::default(),
			graphrag: GraphRAGConfig::default(),
			commits: CommitsConfig::default(),
		}
	}
}

impl Config {
	pub fn load() -> Result<Self> {
		let config_path = Self::get_config_path()?;

		let config = if config_path.exists() {
			let content = fs::read_to_string(&config_path)?;
			toml::from_str(&content)?
		} else {
			// Load from template first, then save to config path
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

		// Environment variables are handled by octolib providers automatically
		// No need to set API keys in config

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
		let config_path = Self::get_config_path()?;

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

	/// Get the active config file path.
	/// Checks `OCTOCODE_CONFIG_PATH` environment variable first;
	/// falls back to the system-wide config path.
	pub fn get_config_path() -> Result<PathBuf> {
		if let Ok(env_path) = std::env::var("OCTOCODE_CONFIG_PATH") {
			return Ok(PathBuf::from(env_path));
		}
		Self::get_system_config_path()
	}

	/// Get the system-wide config file path
	/// Stored at ~/.local/share/octocode/config.toml (same level as fastembed cache)
	pub fn get_system_config_path() -> Result<PathBuf> {
		let system_storage = storage::get_system_storage_dir()?;
		Ok(system_storage.join("config.toml"))
	}

	pub fn get_model(&self) -> &str {
		&self.llm.model
	}

	pub fn get_timeout(&self) -> u64 {
		self.llm.timeout
	}

	pub fn get_temperature(&self) -> f32 {
		self.llm.temperature
	}

	pub fn get_max_tokens(&self) -> usize {
		self.llm.max_tokens
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_default_config() {
		let config = Config::load_from_template().expect("Failed to load template config");
		assert_eq!(config.version, 1);
		assert_eq!(config.llm.model, "openrouter:openai/gpt-4o-mini");
		assert_eq!(config.index.chunk_size, 2000);
		assert_eq!(config.search.max_results, 20);

		assert_eq!(
			config
				.embedding
				.get_active_provider()
				.expect("embedding provider should be set"),
			crate::embedding::types::EmbeddingProviderType::FastEmbed
		);
		// Test new GraphRAG configuration structure
		assert!(!config.graphrag.enabled);
		assert!(!config.graphrag.use_llm);
		assert_eq!(
			config.graphrag.llm.description_model,
			"openrouter:openai/gpt-4o-mini"
		);
		assert_eq!(
			config.graphrag.llm.relationship_model,
			"openrouter:openai/gpt-4o-mini"
		);
		assert_eq!(config.graphrag.llm.ai_batch_size, 8);
		assert_eq!(config.graphrag.llm.max_batch_tokens, 16384);
		assert_eq!(config.graphrag.llm.batch_timeout_seconds, 60);
		assert!(config.graphrag.llm.fallback_to_individual);
		assert_eq!(config.graphrag.llm.max_sample_tokens, 1500);
		assert_eq!(config.graphrag.llm.confidence_threshold, 0.6);
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

		let config = result.expect("Template config should load successfully");
		assert_eq!(config.version, 1);
		assert_eq!(config.llm.model, "openrouter:openai/gpt-4o-mini");
		assert_eq!(config.index.chunk_size, 2000);
		assert_eq!(config.search.max_results, 20);
		assert_eq!(
			config.embedding.code_model,
			"fastembed:jinaai/jina-embeddings-v2-base-code"
		);
		assert_eq!(
			config.embedding.text_model,
			"fastembed:nomic-ai/nomic-embed-text-v1.5"
		);
		// Test new GraphRAG configuration structure from template
		assert!(!config.graphrag.enabled);
		assert!(!config.graphrag.use_llm);
		assert_eq!(
			config.graphrag.llm.description_model,
			"openrouter:openai/gpt-4o-mini"
		);
		assert_eq!(
			config.graphrag.llm.relationship_model,
			"openrouter:openai/gpt-4o-mini"
		);
		assert_eq!(config.graphrag.llm.ai_batch_size, 8);
		assert_eq!(config.graphrag.llm.max_batch_tokens, 16384);
		assert_eq!(config.graphrag.llm.batch_timeout_seconds, 60);
		assert!(config.graphrag.llm.fallback_to_individual);
		assert_eq!(config.graphrag.llm.max_sample_tokens, 1500);
		assert_eq!(config.graphrag.llm.confidence_threshold, 0.6);
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
	fn test_config_default_matches_template() {
		// `Config::default()` (used by e.g. `octocode config --reset`) must never panic,
		// and should match the values in config-templates/default.toml.
		let config = Config::default();
		assert_eq!(config.search.max_results, 20);
		assert!(!config.graphrag.enabled);
		assert_eq!(config.graphrag.llm.ai_batch_size, 8);
		assert_eq!(
			config.search.reranker.model,
			"fastembed:jina-reranker-v2-base-multilingual"
		);
		assert_eq!(config.search.hybrid.default_vector_weight, 0.6);
	}

	#[test]
	fn test_toml_missing_optional_sections_uses_defaults() {
		// A config.toml that omits whole tables (legal TOML) must fall back to
		// sane defaults instead of panicking via serde's #[serde(default)].
		let minimal = "version = 1\n";
		let config: Config = toml::from_str(minimal).expect("should deserialize with defaults");
		assert_eq!(config.search.max_results, 20);
		assert!(!config.graphrag.enabled);
	}
}
