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

use anyhow::{Context, Result};
use octolib::utils::config_file;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::embedding::types::EmbeddingConfig;
use crate::storage;

mod migrations;

const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("../config-templates/default.toml");

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

	/// Project-specific final-extension to language associations.
	#[serde(default)]
	pub file_associations: HashMap<String, String>,

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
			file_associations: HashMap::new(),
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
	/// RRF dampening constant k (Cormack et al. 2009). Lower k lets the very top
	/// ranks dominate the fusion — helps exact-identifier code queries.
	pub rrf_k: f32,
	/// When true, tilt the vector/keyword weights per query by a deterministic
	/// (no-LLM) shape heuristic: identifier/symbol lookups lean keyword, natural
	/// language leans vector.
	pub auto_weight: bool,
}

impl Default for HybridSearchConfig {
	fn default() -> Self {
		Self {
			enabled: true,
			default_vector_weight: 0.6,
			default_keyword_weight: 0.4,
			rrf_k: 60.0,
			auto_weight: false,
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

	/// LLM reasoning-based selection over retrieved candidates (PageIndex-style:
	/// reason over code structure instead of trusting similarity rank).
	pub reasoning: ReasoningConfig,
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
			reasoning: ReasoningConfig::default(),
		}
	}
}

/// PageIndex-style reasoning retrieval. After hybrid retrieval gathers a
/// candidate pool, an LLM reasons over each candidate's code (path, symbols,
/// body) and re-ranks by true relevance, fused with the hybrid rank via RRF.
/// Runs only in the semantic search path; `structural_search` stays a
/// deterministic grep. Off by default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningConfig {
	/// Enable the reasoning selection step.
	pub enabled: bool,
	/// Model for reasoning, "provider:model" (e.g. "deepseek:...", "openrouter:...").
	pub model: String,
	/// How many top candidates to reason over.
	pub max_candidates: usize,
	/// How many results to keep after reasoning.
	pub final_top_k: usize,
	/// Per-candidate context fed to the LLM: "signatures" | "snippets" | "full".
	pub context_level: String,
	/// Weight of the reasoning rank relative to the hybrid rank when fusing (RRF).
	/// >1 leans on the LLM ordering; the hybrid rank always contributes as a
	/// > recall floor so LLM-omitted true hits aren't lost.
	pub reasoning_weight: f32,
	/// Thinking budget for the ranking call: "low" | "medium" | "high" | "xhigh" | "max",
	/// or "default" to send nothing and let the provider decide.
	///
	/// Ranking is a selection task, not a deliberation task, so this defaults to
	/// "low". Left at the provider default, a thinking model spends most of
	/// `llm.max_tokens` on chain-of-thought before emitting the JSON — measured at
	/// 3776 of 4000 tokens for a 25-candidate pool — and once the thinking crosses
	/// that ceiling the JSON is truncated, unparseable, and retried to no effect.
	#[serde(default = "default_reasoning_effort")]
	pub reasoning_effort: String,
}

fn default_reasoning_effort() -> String {
	"low".to_string()
}

impl Default for ReasoningConfig {
	fn default() -> Self {
		Self {
			enabled: false,
			model: "deepseek:deepseek-v4-flash".to_string(),
			max_candidates: 25,
			final_top_k: 10,
			context_level: "full".to_string(),
			reasoning_weight: 2.0,
			reasoning_effort: default_reasoning_effort(),
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
	/// Configuration schema version
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

fn default_contextual_batch_size() -> usize {
	10
}

impl Default for Config {
	fn default() -> Self {
		Self::load_from_template().expect("embedded default configuration must be valid")
	}
}

impl Config {
	pub fn load() -> Result<Self> {
		let config_path = Self::get_config_path()?;
		let config = Self::load_from_path(&config_path)?;
		crate::language::configure_file_associations(&config.index.file_associations)?;
		Ok(config)
	}

	fn load_from_path(config_path: &Path) -> Result<Self> {
		// Current configs are read-only. Acquire the write lock only when the file
		// is missing or an actual migration is required.
		if config_path.exists() {
			let content = fs::read_to_string(config_path).with_context(|| {
				format!("failed to read configuration at {}", config_path.display())
			})?;
			if migrations::migrate(&content, DEFAULT_CONFIG_TEMPLATE)?.is_none() {
				return toml::from_str(&content).with_context(|| {
					format!("failed to load configuration at {}", config_path.display())
				});
			}
		}

		config_file::with_lock(config_path, || Self::load_from_path_locked(config_path))
	}

	fn load_from_path_locked(config_path: &Path) -> Result<Self> {
		if !config_path.exists() {
			config_file::atomic_write(config_path, DEFAULT_CONFIG_TEMPLATE.as_bytes(), None)?;
			return toml::from_str(DEFAULT_CONFIG_TEMPLATE)
				.context("failed to parse embedded default configuration");
		}

		let original = fs::read_to_string(config_path).with_context(|| {
			format!("failed to read configuration at {}", config_path.display())
		})?;
		let migration = migrations::migrate(&original, DEFAULT_CONFIG_TEMPLATE)?;
		let content = migration
			.as_ref()
			.map_or(original.as_str(), |migration| migration.content.as_str());

		// Validate the fully migrated document before changing the user's file.
		let config: Config = toml::from_str(content).with_context(|| {
			format!(
				"failed to load configuration at {} after migration",
				config_path.display()
			)
		})?;

		if let Some(migration) = migration {
			config_file::apply_migration(config_path, original.as_bytes(), &migration)?;
			debug_assert_eq!(config.version, migration.to_version);
		}

		Ok(config)
	}

	/// Load configuration from the default template
	pub fn load_from_template() -> Result<Self> {
		let config: Config = toml::from_str(DEFAULT_CONFIG_TEMPLATE)?;
		Ok(config)
	}

	pub fn save(&self) -> Result<()> {
		let config_path = Self::get_config_path()?;
		let toml_content = toml::to_string_pretty(self)?;
		config_file::with_lock(&config_path, || {
			let permissions = fs::metadata(&config_path)
				.ok()
				.map(|metadata| metadata.permissions());
			config_file::atomic_write(&config_path, toml_content.as_bytes(), permissions)
		})
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

// File primitives (locking, atomic replace, versioned backup) live in
// octolib::utils::config_file — shared verbatim with the other Octo products.

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::Arc;

	struct TempConfigDir {
		path: PathBuf,
	}

	impl TempConfigDir {
		fn new() -> Self {
			let path =
				std::env::temp_dir().join(format!("octocode-config-test-{}", uuid::Uuid::new_v4()));
			fs::create_dir_all(&path).expect("temporary config directory should be created");
			Self { path }
		}

		fn config_path(&self) -> PathBuf {
			self.path.join("config.toml")
		}
	}

	impl Drop for TempConfigDir {
		fn drop(&mut self) {
			let _ = fs::remove_dir_all(&self.path);
		}
	}

	/// Files octolib left next to `config_path`, by extension. The names are
	/// octolib's to choose, so tests here only ask whether a backup or a lock
	/// was made, never what it's called.
	fn siblings(config_path: &Path, extension: &str) -> Vec<PathBuf> {
		let parent = config_path
			.parent()
			.expect("config path must have a parent");
		fs::read_dir(parent)
			.expect("config directory must be readable")
			.map(|entry| entry.expect("directory entry must be readable").path())
			.filter(|path| path.extension().is_some_and(|found| found == extension))
			.collect()
	}

	fn released_v1_config() -> String {
		let mut document = DEFAULT_CONFIG_TEMPLATE
			.parse::<toml_edit::DocumentMut>()
			.expect("default template should be editable");
		document["version"] = toml_edit::value(1);
		let search = document["search"]
			.as_table_mut()
			.expect("template search config should be a table");
		search.remove("reasoning");
		let hybrid = search["hybrid"]
			.as_table_mut()
			.expect("template hybrid config should be a table");
		hybrid.remove("rrf_k");
		hybrid.remove("auto_weight");
		hybrid["default_vector_weight"] = toml_edit::value(0.73);
		document.to_string()
	}

	#[test]
	fn test_default_config() {
		let config = Config::load_from_template().expect("Failed to load template config");
		assert_eq!(config.version, 2);
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
		assert_eq!(config.version, 2);
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
		assert!(config.index.file_associations.is_empty());
	}

	#[test]
	fn parses_project_file_associations() {
		let configured = DEFAULT_CONFIG_TEMPLATE.replace("# inc = \"php\"", "inc = \"php\"");
		let config: Config = toml::from_str(&configured).expect("association should parse");

		assert_eq!(
			config
				.index
				.file_associations
				.get("inc")
				.map(String::as_str),
			Some("php")
		);
	}

	#[test]
	fn test_toml_missing_optional_sections_uses_defaults() {
		// A config.toml that omits whole tables (legal TOML) must fall back to
		// sane defaults instead of panicking via serde's #[serde(default)].
		let minimal = "version = 2\n";
		let config: Config = toml::from_str(minimal).expect("should deserialize with defaults");
		assert_eq!(config.search.max_results, 20);
		assert!(!config.graphrag.enabled);
	}

	#[test]
	fn missing_config_is_exact_copy_of_current_template() {
		let temp = TempConfigDir::new();
		let config_path = temp.config_path();

		let config = Config::load_from_path(&config_path).expect("config should be created");

		assert_eq!(config.version, 2);
		assert_eq!(
			fs::read_to_string(&config_path).unwrap(),
			DEFAULT_CONFIG_TEMPLATE
		);
		assert!(siblings(&config_path, "bak").is_empty());
	}

	#[test]
	fn current_config_load_does_not_create_write_lock() {
		let temp = TempConfigDir::new();
		let config_path = temp.config_path();
		fs::write(&config_path, DEFAULT_CONFIG_TEMPLATE).unwrap();

		let config = Config::load_from_path(&config_path).expect("current config should load");

		assert_eq!(config.version, 2);
		assert!(siblings(&config_path, "lock").is_empty());
	}

	#[test]
	fn load_migrates_v1_once_and_keeps_backup() {
		let temp = TempConfigDir::new();
		let config_path = temp.config_path();
		let original = released_v1_config();
		fs::write(&config_path, &original).unwrap();

		let config = Config::load_from_path(&config_path).expect("v1 config should migrate");
		let migrated = fs::read_to_string(&config_path).unwrap();

		assert_eq!(config.version, 2);
		assert_eq!(config.search.hybrid.default_vector_weight, 0.73);
		assert_eq!(config.search.hybrid.rrf_k, 60.0);
		assert!(!config.search.reasoning.enabled);
		let backups = siblings(&config_path, "bak");
		let [backup] = backups.as_slice() else {
			panic!("migration should leave exactly one backup");
		};
		assert_eq!(fs::read_to_string(backup).unwrap(), original);

		Config::load_from_path(&config_path).expect("v2 config should load unchanged");
		assert_eq!(fs::read_to_string(&config_path).unwrap(), migrated);
	}

	#[test]
	fn future_version_is_rejected_without_modifying_file() {
		let temp = TempConfigDir::new();
		let config_path = temp.config_path();
		let future = DEFAULT_CONFIG_TEMPLATE.replacen("version = 2", "version = 3", 1);
		fs::write(&config_path, &future).unwrap();

		let error = Config::load_from_path(&config_path).expect_err("future config should fail");

		assert!(error
			.to_string()
			.contains("newer than this octocode binary"));
		assert_eq!(fs::read_to_string(&config_path).unwrap(), future);
		assert!(siblings(&config_path, "bak").is_empty());
	}

	#[test]
	fn invalid_config_is_not_modified() {
		let temp = TempConfigDir::new();
		let config_path = temp.config_path();
		let invalid = "version = 1\n[search\n";
		fs::write(&config_path, invalid).unwrap();

		assert!(Config::load_from_path(&config_path).is_err());
		assert_eq!(fs::read_to_string(&config_path).unwrap(), invalid);
		assert!(siblings(&config_path, "bak").is_empty());
	}

	#[test]
	fn migration_keeps_an_older_backup_intact() {
		let temp = TempConfigDir::new();
		let config_path = temp.config_path();
		let stale_backup = config_path.with_file_name("config.toml.v1.bak");
		let original = released_v1_config();
		fs::write(&config_path, &original).unwrap();
		fs::write(&stale_backup, "backup of an earlier config").unwrap();

		let config = Config::load_from_path(&config_path).expect("v1 config should migrate");

		assert_eq!(config.version, 2);
		assert_eq!(
			fs::read_to_string(&stale_backup).unwrap(),
			"backup of an earlier config"
		);
		let fresh = siblings(&config_path, "bak")
			.into_iter()
			.find(|path| *path != stale_backup)
			.expect("migration should add its own backup");
		assert_eq!(fs::read_to_string(fresh).unwrap(), original);
	}

	#[test]
	fn concurrent_loaders_produce_one_valid_migration() {
		let temp = TempConfigDir::new();
		let config_path = Arc::new(temp.config_path());
		fs::write(config_path.as_ref(), released_v1_config()).unwrap();

		let threads: Vec<_> = (0..4)
			.map(|_| {
				let config_path = Arc::clone(&config_path);
				std::thread::spawn(move || Config::load_from_path(&config_path))
			})
			.collect();

		for thread in threads {
			assert_eq!(
				thread
					.join()
					.expect("loader should not panic")
					.unwrap()
					.version,
				2
			);
		}

		let migrated = fs::read_to_string(config_path.as_ref()).unwrap();
		let config: Config = toml::from_str(&migrated).expect("final config should be valid");
		assert_eq!(config.version, 2);
	}
}
