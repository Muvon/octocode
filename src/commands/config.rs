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
use clap::Args;
use octocode::config::Config;
use octocode::embedding::types::{parse_provider_model, EmbeddingProviderType};

#[derive(Args)]
pub struct ConfigArgs {
	/// Set the model to use (e.g., "openai/gpt-4.1-mini", "anthropic/claude-3.5-sonnet")
	#[arg(long)]
	pub model: Option<String>,

	/// Set the code embedding model (use provider:model format, e.g. "huggingface:microsoft/codebert-base")
	#[arg(long)]
	pub code_embedding_model: Option<String>,

	/// Set the text embedding model (use provider:model format, e.g. "huggingface:sentence-transformers/all-mpnet-base-v2")
	#[arg(long)]
	pub text_embedding_model: Option<String>,

	/// Set the chunk size for text processing
	#[arg(long)]
	pub chunk_size: Option<usize>,

	/// Set the chunk overlap for text processing
	#[arg(long)]
	pub chunk_overlap: Option<usize>,

	/// Set the maximum number of search results
	#[arg(long)]
	pub max_results: Option<usize>,

	/// Set the similarity threshold for search
	#[arg(long)]
	pub similarity_threshold: Option<f32>,

	/// Enable or disable GraphRAG
	#[arg(long)]
	pub graphrag_enabled: Option<bool>,

	/// Show current configuration
	#[arg(long)]
	pub show: bool,

	/// Reset configuration to defaults
	#[arg(long)]
	pub reset: bool,
}

pub fn execute(args: &ConfigArgs, mut config: Config) -> Result<()> {
	if args.reset {
		config = Config::default();
		config.save()?;
		println!("Configuration reset to defaults");
		return Ok(());
	}

	if args.show {
		println!("=== Octocode Configuration ===");
		println!();

		// Show configuration file location
		if let Ok(config_path) = Config::get_system_config_path() {
			println!("📄 Configuration file: {}", config_path.display());
			if config_path.exists() {
				println!("   Status: ✅ Found");
			} else {
				println!("   Status: ⚠️  Not found (using defaults)");
			}
		}
		println!();

		// LLM Configuration
		println!("🤖 LLM Configuration:");
		println!("   Model: {}", config.llm.model);
		println!("   Temperature: {}", config.llm.temperature);
		println!("   Max Tokens: {}", config.llm.max_tokens);
		println!("   Timeout: {}s", config.llm.timeout);

		// Embedding Configuration
		println!("🔍 Embedding Configuration:");
		let active_provider = config.embedding.get_active_provider();
		println!("   Active provider: {:?} (auto-detected)", active_provider);
		println!("   Code model: {}", config.embedding.code_model);
		println!("   Text model: {}", config.embedding.text_model);

		// Show API key status for providers that need them
		match active_provider {
			Ok(EmbeddingProviderType::Jina) => {
				let api_key_status = if config
					.embedding
					.get_api_key(&EmbeddingProviderType::Jina)
					.is_some()
				{
					"✅ Set"
				} else {
					"❌ Not set"
				};
				println!("   Jina API key: {}", api_key_status);
			}
			Ok(EmbeddingProviderType::Voyage) => {
				let api_key_status = if config
					.embedding
					.get_api_key(&EmbeddingProviderType::Voyage)
					.is_some()
				{
					"✅ Set"
				} else {
					"❌ Not set"
				};
				println!("   Voyage API key: {}", api_key_status);
			}
			Ok(EmbeddingProviderType::Google) => {
				let api_key_status = if config
					.embedding
					.get_api_key(&EmbeddingProviderType::Google)
					.is_some()
				{
					"✅ Set"
				} else {
					"❌ Not set"
				};
				println!("   Google API key: {}", api_key_status);
			}
			_ => {
				// FastEmbed and SentenceTransformer don't need API keys
				println!("   API key: Not required");
			}
		}
		println!();

		// Indexing Configuration
		println!("📚 Indexing Configuration:");
		println!("   Chunk size: {} characters", config.index.chunk_size);
		println!(
			"   Chunk overlap: {} characters",
			config.index.chunk_overlap
		);
		println!(
			"   Batch size: {} texts",
			config.index.embeddings_batch_size
		);
		println!(
			"   GraphRAG: {}",
			if config.graphrag.enabled {
				"✅ Enabled"
			} else {
				"❌ Disabled"
			}
		);
		println!(
			"   LLM processing: {}",
			if config.graphrag.use_llm {
				"✅ Enabled"
			} else {
				"❌ Disabled"
			}
		);
		println!();

		// Search Configuration
		println!("🔎 Search Configuration:");
		println!("   Max results: {}", config.search.max_results);
		println!(
			"   Similarity threshold: {:.2}",
			config.search.similarity_threshold
		);
		println!("   Output format: {}", config.search.output_format);
		println!("   Max files: {}", config.search.max_files);
		println!("   Context lines: {}", config.search.context_lines);
		println!(
			"   Block max chars: {}",
			config.search.search_block_max_characters
		);
		println!();

		// Storage Locations
		println!("💾 Storage Locations:");
		if let Ok(storage_dir) = octocode::storage::get_system_storage_dir() {
			println!("   System storage: {}", storage_dir.display());
			println!("   FastEmbed cache: {}/fastembed/", storage_dir.display());
			println!(
				"   SentenceTransformer cache: {}/sentencetransformer/",
				storage_dir.display()
			);
		}
		if let Ok(current_dir) = std::env::current_dir() {
			if let Ok(db_path) = octocode::storage::get_project_database_path(&current_dir) {
				println!("   Project database: {}", db_path.display());
				if db_path.exists() {
					println!("   Database status: ✅ Found");
				} else {
					println!("   Database status: ❌ Not indexed (run 'octocode index')");
				}
			}
		}

		// GraphRAG Configuration (if enabled)
		if config.graphrag.enabled {
			println!();
			println!("🕸️  GraphRAG Configuration:");
			println!(
				"   Description model: {}",
				config.graphrag.llm.description_model
			);
			println!(
				"   Relationship model: {}",
				config.graphrag.llm.relationship_model
			);
		}

		return Ok(());
	}

	let mut updated = false;

	if let Some(model) = &args.model {
		config.llm.model = model.clone();
		println!("Model set to: {}", model);
		updated = true;
	}

	if let Some(code_model) = &args.code_embedding_model {
		// Parse provider from the model string and set the code model
		let (provider, _) = parse_provider_model(code_model)?;
		config.embedding.code_model = code_model.clone();
		println!(
			"Code embedding model set to: {} (provider: {:?})",
			code_model, provider
		);
		updated = true;
	}

	if let Some(text_model) = &args.text_embedding_model {
		// Parse provider from the model string and set the text model
		let (provider, _) = parse_provider_model(text_model)?;
		config.embedding.text_model = text_model.clone();
		println!(
			"Text embedding model set to: {} (provider: {:?})",
			text_model, provider
		);
		updated = true;
	}

	if let Some(chunk_size) = args.chunk_size {
		config.index.chunk_size = chunk_size;
		println!("Chunk size set to: {}", chunk_size);
		updated = true;
	}

	if let Some(chunk_overlap) = args.chunk_overlap {
		config.index.chunk_overlap = chunk_overlap;
		println!("Chunk overlap set to: {}", chunk_overlap);
		updated = true;
	}

	if let Some(max_results) = args.max_results {
		config.search.max_results = max_results;
		println!("Max results set to: {}", max_results);
		updated = true;
	}

	if let Some(similarity_threshold) = args.similarity_threshold {
		config.search.similarity_threshold = similarity_threshold;
		println!("Similarity threshold set to: {}", similarity_threshold);
		updated = true;
	}

	if let Some(graphrag_enabled) = args.graphrag_enabled {
		config.graphrag.enabled = graphrag_enabled;
		println!(
			"GraphRAG {}",
			if graphrag_enabled {
				"enabled"
			} else {
				"disabled"
			}
		);
		updated = true;
	}

	if updated {
		config.save()?;
		println!("Configuration updated successfully!");
	} else {
		println!("No configuration changes made. Use --show to see current settings.");
		println!();
		println!("Example usage:");
		println!("  # Set SentenceTransformer models (provider is auto-detected):");
		println!("  octocode config --code-embedding-model 'huggingface:microsoft/codebert-base'");
		println!("  octocode config --text-embedding-model 'huggingface:sentence-transformers/all-mpnet-base-v2'");
		println!();
		println!("  # Use other providers:");
		println!("  octocode config --code-embedding-model 'fastembed:all-MiniLM-L6-v2'");
		println!("  octocode config --code-embedding-model 'jinaai:jina-embeddings-v2-base-code'");
		println!();
		println!("Popular SentenceTransformer models:");
		println!("  Code models: microsoft/codebert-base, microsoft/unixcoder-base");
		println!("  Text models: sentence-transformers/all-mpnet-base-v2, BAAI/bge-base-en-v1.5");
	}

	Ok(())
}
