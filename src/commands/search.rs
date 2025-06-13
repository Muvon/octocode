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

use clap::Args;

use octocode::config::Config;
use octocode::embedding;
use octocode::indexer;
use octocode::reranker::Reranker;
use octocode::storage;
use octocode::store::Store;
use std::cmp::Ordering;
use std::collections::HashMap;

const MAX_QUERIES: usize = 3;

fn validate_queries(queries: &[String]) -> Result<(), anyhow::Error> {
	if queries.is_empty() {
		return Err(anyhow::anyhow!("At least one query is required"));
	}
	if queries.len() > MAX_QUERIES {
		return Err(anyhow::anyhow!(
			"Maximum {} queries allowed, got {}. Use fewer, more specific terms.",
			MAX_QUERIES,
			queries.len()
		));
	}
	for (i, query) in queries.iter().enumerate() {
		if query.trim().is_empty() {
			return Err(anyhow::anyhow!("Query {} is empty", i + 1));
		}
		if query.len() > 500 {
			return Err(anyhow::anyhow!("Query {} too long (max 500 chars)", i + 1));
		}
	}
	Ok(())
}

#[derive(Args, Debug)]
pub struct SearchArgs {
	/// Search queries (maximum 3 queries supported for optimal performance)
	pub queries: Vec<String>,

	/// Expand all symbols in matching code blocks
	#[arg(long, short)]
	pub expand: bool,

	/// Output in JSON format
	#[arg(long)]
	pub json: bool,

	/// Output in Markdown format
	#[arg(long)]
	pub md: bool,

	/// Output in text format (token-efficient)
	#[arg(long)]
	pub text: bool,

	/// Search mode: all (default), code, docs, or text
	#[arg(long, default_value = "all")]
	pub mode: String,

	/// Similarity threshold (0.0-1.0). Higher values = more strict matching. Lower values = more results.
	/// Examples: 0.3 (broad results), 0.5 (balanced), 0.7 (high quality), 0.8 (very strict)
	#[arg(long, short = 't', default_value = "0.5")]
	pub threshold: f32,
}

#[derive(Debug)]
struct QuerySearchResult {
	query_index: usize,
	code_blocks: Vec<octocode::store::CodeBlock>,
	doc_blocks: Vec<octocode::store::DocumentBlock>,
	text_blocks: Vec<octocode::store::TextBlock>,
}

async fn generate_batch_embeddings_for_queries(
	queries: &[String],
	mode: &str,
	config: &Config,
) -> Result<Vec<embedding::SearchModeEmbeddings>, anyhow::Error> {
	match mode {
		"code" => {
			// Batch generate code embeddings for all queries
			let code_embeddings =
				embedding::generate_embeddings_batch(queries.to_vec(), true, config).await?;
			Ok(code_embeddings
				.into_iter()
				.map(|emb| embedding::SearchModeEmbeddings {
					code_embeddings: Some(emb),
					text_embeddings: None,
				})
				.collect())
		}
		"docs" | "text" => {
			// Batch generate text embeddings for all queries
			let text_embeddings =
				embedding::generate_embeddings_batch(queries.to_vec(), false, config).await?;
			Ok(text_embeddings
				.into_iter()
				.map(|emb| embedding::SearchModeEmbeddings {
					code_embeddings: None,
					text_embeddings: Some(emb),
				})
				.collect())
		}
		"all" => {
			let code_model = &config.embedding.code_model;
			let text_model = &config.embedding.text_model;

			if code_model == text_model {
				// Same model - generate once and reuse (efficient!)
				let embeddings =
					embedding::generate_embeddings_batch(queries.to_vec(), true, config).await?;
				Ok(embeddings
					.into_iter()
					.map(|emb| embedding::SearchModeEmbeddings {
						code_embeddings: Some(emb.clone()),
						text_embeddings: Some(emb),
					})
					.collect())
			} else {
				// Different models - generate both types in parallel
				let (code_embeddings, text_embeddings) = tokio::try_join!(
					embedding::generate_embeddings_batch(queries.to_vec(), true, config),
					embedding::generate_embeddings_batch(queries.to_vec(), false, config)
				)?;

				Ok(code_embeddings
					.into_iter()
					.zip(text_embeddings.into_iter())
					.map(|(code_emb, text_emb)| embedding::SearchModeEmbeddings {
						code_embeddings: Some(code_emb),
						text_embeddings: Some(text_emb),
					})
					.collect())
			}
		}
		_ => Err(anyhow::anyhow!("Invalid search mode: {}", mode)),
	}
}

async fn execute_single_search_with_embeddings(
	store: &Store,
	query: &str,
	embeddings: embedding::SearchModeEmbeddings,
	mode: &str,
	limit: usize,
	query_index: usize,
) -> Result<QuerySearchResult, anyhow::Error> {
	let (code_blocks, doc_blocks, text_blocks) = match mode {
		"code" => {
			let code_embeddings = embeddings
				.code_embeddings
				.ok_or_else(|| anyhow::anyhow!("No code embeddings for code search"))?;
			let mut blocks = store
				.get_code_blocks_with_config(code_embeddings, Some(limit), Some(1.01))
				.await?;
			blocks = Reranker::rerank_code_blocks(blocks, query);
			Reranker::tf_idf_boost(&mut blocks, query);
			(blocks, vec![], vec![])
		}
		"docs" => {
			let text_embeddings = embeddings
				.text_embeddings
				.ok_or_else(|| anyhow::anyhow!("No text embeddings for docs search"))?;
			let mut blocks = store
				.get_document_blocks_with_config(text_embeddings, Some(limit), Some(1.01))
				.await?;
			blocks = Reranker::rerank_document_blocks(blocks, query);
			(vec![], blocks, vec![])
		}
		"text" => {
			let text_embeddings = embeddings
				.text_embeddings
				.ok_or_else(|| anyhow::anyhow!("No text embeddings for text search"))?;
			let mut blocks = store
				.get_text_blocks_with_config(text_embeddings, Some(limit), Some(1.01))
				.await?;
			blocks = Reranker::rerank_text_blocks(blocks, query);
			(vec![], vec![], blocks)
		}
		"all" => {
			// Execute all three searches in parallel
			let code_embeddings = embeddings
				.code_embeddings
				.ok_or_else(|| anyhow::anyhow!("No code embeddings for all search"))?;
			let text_embeddings = embeddings
				.text_embeddings
				.ok_or_else(|| anyhow::anyhow!("No text embeddings for all search"))?;

			let (mut code_blocks, mut doc_blocks, mut text_blocks) = tokio::try_join!(
				store.get_code_blocks_with_config(code_embeddings, Some(limit), Some(1.01)),
				store.get_document_blocks_with_config(
					text_embeddings.clone(),
					Some(limit),
					Some(1.01)
				),
				store.get_text_blocks_with_config(text_embeddings, Some(limit), Some(1.01))
			)?;

			// Apply reranking
			code_blocks = Reranker::rerank_code_blocks(code_blocks, query);
			doc_blocks = Reranker::rerank_document_blocks(doc_blocks, query);
			text_blocks = Reranker::rerank_text_blocks(text_blocks, query);

			Reranker::tf_idf_boost(&mut code_blocks, query);

			(code_blocks, doc_blocks, text_blocks)
		}
		_ => unreachable!(),
	};

	Ok(QuerySearchResult {
		query_index,
		code_blocks,
		doc_blocks,
		text_blocks,
	})
}

async fn execute_parallel_searches(
	store: &Store,
	query_embeddings: Vec<(String, embedding::SearchModeEmbeddings)>,
	mode: &str,
	config: &Config,
) -> Result<Vec<QuerySearchResult>, anyhow::Error> {
	let per_query_limit = (config.search.max_results * 2) / query_embeddings.len().max(1);

	let search_futures: Vec<_> = query_embeddings
		.into_iter()
		.enumerate()
		.map(|(index, (query, embeddings))| async move {
			execute_single_search_with_embeddings(
				store,
				&query,
				embeddings,
				mode,
				per_query_limit,
				index,
			)
			.await
		})
		.collect();

	// Execute all searches concurrently
	futures::future::try_join_all(search_futures).await
}

fn apply_multi_query_bonus_code(
	block: &mut octocode::store::CodeBlock,
	query_indices: &[usize],
	total_queries: usize,
) {
	if query_indices.len() > 1 && total_queries > 1 {
		let coverage_ratio = query_indices.len() as f32 / total_queries as f32;
		let bonus_factor = 1.0 - (coverage_ratio * 0.1).min(0.2); // Up to 20% bonus

		if let Some(distance) = block.distance {
			block.distance = Some(distance * bonus_factor);
		}
	}
}

fn apply_multi_query_bonus_doc(
	block: &mut octocode::store::DocumentBlock,
	query_indices: &[usize],
	total_queries: usize,
) {
	if query_indices.len() > 1 && total_queries > 1 {
		let coverage_ratio = query_indices.len() as f32 / total_queries as f32;
		let bonus_factor = 1.0 - (coverage_ratio * 0.1).min(0.2); // Up to 20% bonus

		if let Some(distance) = block.distance {
			block.distance = Some(distance * bonus_factor);
		}
	}
}

fn apply_multi_query_bonus_text(
	block: &mut octocode::store::TextBlock,
	query_indices: &[usize],
	total_queries: usize,
) {
	if query_indices.len() > 1 && total_queries > 1 {
		let coverage_ratio = query_indices.len() as f32 / total_queries as f32;
		let bonus_factor = 1.0 - (coverage_ratio * 0.1).min(0.2); // Up to 20% bonus

		if let Some(distance) = block.distance {
			block.distance = Some(distance * bonus_factor);
		}
	}
}

fn deduplicate_and_merge_results(
	search_results: Vec<QuerySearchResult>,
	queries: &[String],
	threshold: f32,
) -> (
	Vec<octocode::store::CodeBlock>,
	Vec<octocode::store::DocumentBlock>,
	Vec<octocode::store::TextBlock>,
) {
	// Deduplicate code blocks
	let mut code_map: HashMap<String, (octocode::store::CodeBlock, Vec<usize>)> = HashMap::new();

	for result in &search_results {
		for block in &result.code_blocks {
			match code_map.entry(block.hash.clone()) {
				std::collections::hash_map::Entry::Vacant(e) => {
					e.insert((block.clone(), vec![result.query_index]));
				}
				std::collections::hash_map::Entry::Occupied(mut e) => {
					let (existing_block, query_indices) = e.get_mut();
					query_indices.push(result.query_index);
					// Keep block with better score (lower distance)
					if block.distance < existing_block.distance {
						*existing_block = block.clone();
					}
				}
			}
		}
	}

	// Deduplicate document blocks
	let mut doc_map: HashMap<String, (octocode::store::DocumentBlock, Vec<usize>)> = HashMap::new();

	for result in &search_results {
		for block in &result.doc_blocks {
			match doc_map.entry(block.hash.clone()) {
				std::collections::hash_map::Entry::Vacant(e) => {
					e.insert((block.clone(), vec![result.query_index]));
				}
				std::collections::hash_map::Entry::Occupied(mut e) => {
					let (existing_block, query_indices) = e.get_mut();
					query_indices.push(result.query_index);
					if block.distance < existing_block.distance {
						*existing_block = block.clone();
					}
				}
			}
		}
	}

	// Deduplicate text blocks
	let mut text_map: HashMap<String, (octocode::store::TextBlock, Vec<usize>)> = HashMap::new();

	for result in &search_results {
		for block in &result.text_blocks {
			match text_map.entry(block.hash.clone()) {
				std::collections::hash_map::Entry::Vacant(e) => {
					e.insert((block.clone(), vec![result.query_index]));
				}
				std::collections::hash_map::Entry::Occupied(mut e) => {
					let (existing_block, query_indices) = e.get_mut();
					query_indices.push(result.query_index);
					if block.distance < existing_block.distance {
						*existing_block = block.clone();
					}
				}
			}
		}
	}

	// Apply multi-query bonuses and filter
	let mut final_code_blocks: Vec<octocode::store::CodeBlock> = code_map
		.into_values()
		.map(|(mut block, query_indices)| {
			apply_multi_query_bonus_code(&mut block, &query_indices, queries.len());
			block
		})
		.filter(|block| {
			if let Some(distance) = block.distance {
				distance <= threshold
			} else {
				true
			}
		})
		.collect();

	let mut final_doc_blocks: Vec<octocode::store::DocumentBlock> = doc_map
		.into_values()
		.map(|(mut block, query_indices)| {
			apply_multi_query_bonus_doc(&mut block, &query_indices, queries.len());
			block
		})
		.filter(|block| {
			if let Some(distance) = block.distance {
				distance <= threshold
			} else {
				true
			}
		})
		.collect();

	let mut final_text_blocks: Vec<octocode::store::TextBlock> = text_map
		.into_values()
		.map(|(mut block, query_indices)| {
			apply_multi_query_bonus_text(&mut block, &query_indices, queries.len());
			block
		})
		.filter(|block| {
			if let Some(distance) = block.distance {
				distance <= threshold
			} else {
				true
			}
		})
		.collect();

	// Sort by relevance
	final_code_blocks.sort_by(|a, b| match (a.distance, b.distance) {
		(Some(dist_a), Some(dist_b)) => dist_a.partial_cmp(&dist_b).unwrap_or(Ordering::Equal),
		(Some(_), None) => Ordering::Less,
		(None, Some(_)) => Ordering::Greater,
		(None, None) => Ordering::Equal,
	});

	final_doc_blocks.sort_by(|a, b| match (a.distance, b.distance) {
		(Some(dist_a), Some(dist_b)) => dist_a.partial_cmp(&dist_b).unwrap_or(Ordering::Equal),
		(Some(_), None) => Ordering::Less,
		(None, Some(_)) => Ordering::Greater,
		(None, None) => Ordering::Equal,
	});

	final_text_blocks.sort_by(|a, b| match (a.distance, b.distance) {
		(Some(dist_a), Some(dist_b)) => dist_a.partial_cmp(&dist_b).unwrap_or(Ordering::Equal),
		(Some(_), None) => Ordering::Less,
		(None, Some(_)) => Ordering::Greater,
		(None, None) => Ordering::Equal,
	});

	(final_code_blocks, final_doc_blocks, final_text_blocks)
}

pub async fn execute(
	store: &Store,
	args: &SearchArgs,
	config: &Config,
) -> Result<(), anyhow::Error> {
	let current_dir = std::env::current_dir()?;

	// Use the new storage system to check for index
	let index_path = storage::get_project_database_path(&current_dir)?;

	// Check if we have an index already; if not, inform the user but don't auto-index
	if !index_path.exists() {
		return Err(anyhow::anyhow!(
			"No index found. Please run 'octocode index' first to create an index."
		));
	}

	// Validate queries
	validate_queries(&args.queries)?;

	// Validate similarity threshold
	if args.threshold < 0.0 || args.threshold > 1.0 {
		return Err(anyhow::anyhow!(
			"Similarity threshold must be between 0.0 and 1.0, got: {}",
			args.threshold
		));
	}

	// Validate search mode
	let search_mode = match args.mode.as_str() {
		"all" | "code" | "docs" | "text" => args.mode.as_str(),
		_ => {
			return Err(anyhow::anyhow!(
				"Invalid search mode '{}'. Use 'all', 'code', 'docs', or 'text'.",
				args.mode
			));
		}
	};

	// Convert similarity threshold to distance threshold
	let distance_threshold = 1.0 - args.threshold;

	// Generate batch embeddings for all queries
	let embeddings =
		generate_batch_embeddings_for_queries(&args.queries, search_mode, config).await?;

	// Zip queries with embeddings
	let query_embeddings: Vec<_> = args
		.queries
		.iter()
		.cloned()
		.zip(embeddings.into_iter())
		.collect();

	// Execute parallel searches
	let search_results =
		execute_parallel_searches(store, query_embeddings, search_mode, config).await?;

	// Deduplicate and merge with multi-query bonuses
	let (mut code_blocks, mut doc_blocks, mut text_blocks) =
		deduplicate_and_merge_results(search_results, &args.queries, distance_threshold);

	// Apply global result limits
	code_blocks.truncate(config.search.max_results);
	doc_blocks.truncate(config.search.max_results);
	text_blocks.truncate(config.search.max_results);

	// Symbol expansion if requested
	if args.expand && !code_blocks.is_empty() {
		println!("Expanding symbols...");
		code_blocks = indexer::expand_symbols(store, code_blocks).await?;
	}

	// Use EXISTING output formatting with added text support
	match search_mode {
		"code" => {
			if args.json {
				indexer::render_results_json(&code_blocks)?
			} else if args.md {
				let markdown = indexer::code_blocks_to_markdown_with_config(&code_blocks, config);
				println!("{}", markdown);
			} else if args.text {
				// Use text formatting function for token efficiency
				let text_output =
					indexer::search::format_code_search_results_as_text(&code_blocks, "partial");
				println!("{}", text_output);
			} else {
				indexer::render_code_blocks_with_config(&code_blocks, config);
			}
		}
		"docs" => {
			if args.json {
				let json = serde_json::to_string_pretty(&doc_blocks)?;
				println!("{}", json);
			} else if args.md {
				let markdown =
					indexer::document_blocks_to_markdown_with_config(&doc_blocks, config);
				println!("{}", markdown);
			} else if args.text {
				// Use text formatting function for token efficiency
				let text_output = indexer::search::format_doc_search_results_as_text(&doc_blocks);
				println!("{}", text_output);
			} else {
				render_document_blocks_with_config(&doc_blocks, config);
			}
		}
		"text" => {
			if args.json {
				let json = serde_json::to_string_pretty(&text_blocks)?;
				println!("{}", json);
			} else if args.md {
				let markdown = indexer::text_blocks_to_markdown_with_config(&text_blocks, config);
				println!("{}", markdown);
			} else if args.text {
				// Use text formatting function for token efficiency
				let text_output = indexer::search::format_text_search_results_as_text(&text_blocks);
				println!("{}", text_output);
			} else {
				render_text_blocks_with_config(&text_blocks, config);
			}
		}
		"all" => {
			// Filter final results by threshold again
			code_blocks.retain(|block| {
				if let Some(distance) = block.distance {
					distance <= distance_threshold
				} else {
					true
				}
			});
			doc_blocks.retain(|block| {
				if let Some(distance) = block.distance {
					distance <= distance_threshold
				} else {
					true
				}
			});
			text_blocks.retain(|block| {
				if let Some(distance) = block.distance {
					distance <= distance_threshold
				} else {
					true
				}
			});

			let mut final_code_results = code_blocks;
			if args.expand {
				println!("Expanding symbols...");
				final_code_results = indexer::expand_symbols(store, final_code_results).await?;
			}

			if args.json {
				let combined = serde_json::json!({
					"code_blocks": final_code_results,
					"document_blocks": doc_blocks,
					"text_blocks": text_blocks
				});
				println!("{}", serde_json::to_string_pretty(&combined)?);
			} else if args.md {
				let mut combined_markdown = String::new();

				if !doc_blocks.is_empty() {
					combined_markdown.push_str("# Documentation Results\n\n");
					combined_markdown.push_str(&indexer::document_blocks_to_markdown_with_config(
						&doc_blocks,
						config,
					));
					combined_markdown.push('\n');
				}

				if !final_code_results.is_empty() {
					combined_markdown.push_str("# Code Results\n\n");
					combined_markdown.push_str(&indexer::code_blocks_to_markdown_with_config(
						&final_code_results,
						config,
					));
					combined_markdown.push('\n');
				}

				if !text_blocks.is_empty() {
					combined_markdown.push_str("# Text Results\n\n");
					combined_markdown.push_str(&indexer::text_blocks_to_markdown_with_config(
						&text_blocks,
						config,
					));
				}

				if combined_markdown.is_empty() {
					combined_markdown.push_str("No results found for the query.");
				}

				println!("{}", combined_markdown);
			} else if args.text {
				// Use text formatting function for token efficiency
				let text_output = indexer::search::format_combined_search_results_as_text(
					&final_code_results,
					&text_blocks,
					&doc_blocks,
					"partial",
				);
				println!("{}", text_output);
			} else {
				if !doc_blocks.is_empty() {
					println!("=== DOCUMENTATION RESULTS ===\n");
					render_document_blocks_with_config(&doc_blocks, config);
					println!("\n");
				}

				if !final_code_results.is_empty() {
					println!("=== CODE RESULTS ===\n");
					indexer::render_code_blocks_with_config(&final_code_results, config);
					println!("\n");
				}

				if !text_blocks.is_empty() {
					println!("=== TEXT RESULTS ===\n");
					render_text_blocks_with_config(&text_blocks, config);
				}

				if doc_blocks.is_empty() && final_code_results.is_empty() && text_blocks.is_empty()
				{
					println!("No results found for the query.");
				}
			}
		}
		_ => unreachable!(),
	}

	Ok(())
}

fn render_text_blocks_with_config(blocks: &[octocode::store::TextBlock], config: &Config) {
	if blocks.is_empty() {
		println!("No text blocks found.");
		return;
	}

	println!("Found {} text blocks:\n", blocks.len());

	// Group blocks by file path for better organization
	let mut blocks_by_file: std::collections::HashMap<String, Vec<&octocode::store::TextBlock>> =
		std::collections::HashMap::new();

	for block in blocks {
		blocks_by_file
			.entry(block.path.clone())
			.or_default()
			.push(block);
	}

	// Print results organized by file
	for (file_path, file_blocks) in blocks_by_file.iter() {
		println!("╔══════════════════ File: {} ══════════════════", file_path);

		for (idx, block) in file_blocks.iter().enumerate() {
			println!("║");
			println!(
				"║ Block {} of {}: {}",
				idx + 1,
				file_blocks.len(),
				block.language
			);
			println!("║ Lines: {}-{}", block.start_line, block.end_line);

			// Show similarity score if available
			if let Some(distance) = block.distance {
				println!("║ Similarity: {:.4}", 1.0 - distance);
			}

			println!("║");
			println!("║ Content:");

			// Use smart truncation based on configuration
			let max_chars = config.search.search_block_max_characters;
			let (content, was_truncated) =
				indexer::truncate_content_smartly(&block.content, max_chars);

			// Display content with proper indentation
			for line in content.lines() {
				println!("║   {}", line);
			}

			// Add note if content was truncated
			if was_truncated {
				println!("║   [Content truncated - limit: {} chars]", max_chars);
			}
		}

		println!("╚════════════════════════════════════════\n");
	}
}

fn render_document_blocks_with_config(blocks: &[octocode::store::DocumentBlock], config: &Config) {
	if blocks.is_empty() {
		println!("No documentation blocks found.");
		return;
	}

	println!("Found {} documentation sections:\n", blocks.len());

	// Group blocks by file path for better organization
	let mut blocks_by_file: std::collections::HashMap<
		String,
		Vec<&octocode::store::DocumentBlock>,
	> = std::collections::HashMap::new();

	for block in blocks {
		blocks_by_file
			.entry(block.path.clone())
			.or_default()
			.push(block);
	}

	// Print results organized by file
	for (file_path, file_blocks) in blocks_by_file.iter() {
		println!("╔══════════════════ File: {} ══════════════════", file_path);

		for (idx, block) in file_blocks.iter().enumerate() {
			println!("║");
			println!(
				"║ Section {} of {}: {}",
				idx + 1,
				file_blocks.len(),
				block.title
			);
			println!(
				"║ Level: {}  Lines: {}-{}",
				block.level, block.start_line, block.end_line
			);

			// Show similarity score if available
			if let Some(distance) = block.distance {
				println!("║ Similarity: {:.4}", 1.0 - distance);
			}

			println!("║");
			println!("║ Content:");

			// Use smart truncation based on configuration
			let max_chars = config.search.search_block_max_characters;
			let (content, was_truncated) =
				indexer::truncate_content_smartly(&block.content, max_chars);

			// Display content with proper indentation
			for line in content.lines() {
				println!("║   {}", line);
			}

			// Add note if content was truncated
			if was_truncated {
				println!("║   [Content truncated - limit: {} chars]", max_chars);
			}
		}

		println!("╚════════════════════════════════════════\n");
	}
}
