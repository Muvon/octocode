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
use octocode::constants::MAX_QUERIES;
use octocode::indexer;

use octocode::storage;
use octocode::store::Store;

use crate::commands::OutputFormat;

fn validate_detail_level(s: &str) -> Result<String, String> {
	match s {
		"signatures" | "partial" | "full" => Ok(s.to_string()),
		_ => Err(format!(
			"Invalid detail level '{}'. Use 'signatures', 'partial', or 'full'.",
			s
		)),
	}
}

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
		let query = query.trim();
		if query.len() < 3 {
			return Err(anyhow::anyhow!(
				"Query {} must be at least 3 characters long",
				i + 1
			));
		}
		if query.len() > 500 {
			return Err(anyhow::anyhow!(
				"Query {} must be no more than 500 characters long",
				i + 1
			));
		}
	}

	Ok(())
}

#[derive(Debug, Args)]
pub struct SearchArgs {
	/// The search queries
	#[arg(required = true)]
	pub queries: Vec<String>,

	/// Search mode: 'all' (default), 'code', 'docs', or 'text'
	#[arg(short, long, default_value = "all")]
	pub mode: String,

	/// Output format: 'cli', 'json', 'md', or 'text'
	#[arg(short, long, default_value = "cli")]
	pub format: OutputFormat,

	/// Similarity threshold (0.0-1.0). Higher values = more similar results only. Defaults to config.search.similarity_threshold
	#[arg(short, long)]
	pub threshold: Option<f32>,

	/// Expand symbols (show full function/class definitions)
	#[arg(short, long)]
	pub expand: bool,

	/// Detail level for output: 'signatures', 'partial', or 'full' (default: partial for cli/text formats)
	#[arg(short = 'd', long, value_parser = validate_detail_level)]
	pub detail_level: Option<String>,

	/// Filter by programming language (only affects code blocks)
	#[arg(short = 'l', long)]
	pub language: Option<String>,
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

	// Use config default threshold if not provided via CLI
	let threshold = args.threshold.unwrap_or(config.search.similarity_threshold);

	// Validate similarity threshold
	if !(0.0..=1.0).contains(&threshold) {
		return Err(anyhow::anyhow!(
			"Similarity threshold must be between 0.0 and 1.0, got: {}",
			threshold
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

	// Validate language filter if provided
	if let Some(ref language) = args.language {
		use octocode::indexer::languages;
		if languages::get_language(language).is_none() {
			return Err(anyhow::anyhow!(
				"Invalid language '{}'. Supported languages: rust, javascript, typescript, python, go, cpp, php, bash, ruby, json, svelte, css",
				language
			));
		}
	}

	// Validate detail_level is only used with compatible formats
	if args.detail_level.is_some() {
		if args.format.is_json() {
			return Err(anyhow::anyhow!(
				"--detail-level is not supported with JSON format. Use --format=cli or --format=text instead."
			));
		}
		if args.format.is_md() {
			return Err(anyhow::anyhow!(
				"--detail-level is not supported with Markdown format. Use --format=cli or --format=text instead."
			));
		}
	}

	// Convert similarity threshold to distance threshold
	let distance_threshold = 1.0 - threshold;

	// Get effective detail level (default to "partial" for cli/text formats)
	let effective_detail_level = args.detail_level.as_deref().unwrap_or("partial");

	// Generate batch embeddings for all queries
	let embeddings =
		indexer::search::generate_batch_embeddings_for_queries(&args.queries, search_mode, config)
			.await?;

	// Zip queries with embeddings
	let query_embeddings: Vec<_> = args
		.queries
		.iter()
		.cloned()
		.zip(embeddings.into_iter())
		.collect();

	// Execute parallel searches - FIXED: Use distance_threshold instead of args.threshold
	let search_results = indexer::search::execute_parallel_searches(
		store,
		query_embeddings,
		search_mode,
		config.search.max_results,
		distance_threshold, // FIXED: Was args.threshold, now distance_threshold
		args.language.as_deref(),
	)
	.await?;

	// Deduplicate and merge with multi-query bonuses
	let (mut code_blocks, mut doc_blocks, mut text_blocks) =
		indexer::search::deduplicate_and_merge_results(
			search_results,
			&args.queries,
			distance_threshold,
		);

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
			if args.format.is_json() {
				indexer::render_results_json(&code_blocks)?
			} else if args.format.is_md() {
				let markdown = indexer::code_blocks_to_markdown_with_config(&code_blocks, config);
				println!("{}", markdown);
			} else if args.format.is_text() {
				// Use text formatting function for token efficiency
				let text_output = indexer::search::format_code_search_results_as_text(
					&code_blocks,
					effective_detail_level,
				);
				println!("{}", text_output);
			} else {
				indexer::render_code_blocks_with_config(
					&code_blocks,
					config,
					effective_detail_level,
				);
			}
		}
		"docs" => {
			if args.format.is_json() {
				let json = serde_json::to_string_pretty(&doc_blocks)?;
				println!("{}", json);
			} else if args.format.is_md() {
				let markdown =
					indexer::document_blocks_to_markdown_with_config(&doc_blocks, config);
				println!("{}", markdown);
			} else if args.format.is_text() {
				// Use text formatting function for token efficiency
				let text_output = indexer::search::format_doc_search_results_as_text(
					&doc_blocks,
					effective_detail_level,
				);
				println!("{}", text_output);
			} else {
				render_document_blocks_with_config(&doc_blocks, config, effective_detail_level);
			}
		}
		"text" => {
			if args.format.is_json() {
				let json = serde_json::to_string_pretty(&text_blocks)?;
				println!("{}", json);
			} else if args.format.is_md() {
				let markdown = indexer::text_blocks_to_markdown_with_config(&text_blocks, config);
				println!("{}", markdown);
			} else if args.format.is_text() {
				// Use text formatting function for token efficiency
				let text_output = indexer::search::format_text_search_results_as_text(
					&text_blocks,
					effective_detail_level,
				);
				println!("{}", text_output);
			} else {
				render_text_blocks_with_config(&text_blocks, config, effective_detail_level);
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

			if args.format.is_json() {
				let combined = serde_json::json!({
					"code_blocks": final_code_results,
					"document_blocks": doc_blocks,
					"text_blocks": text_blocks
				});
				println!("{}", serde_json::to_string_pretty(&combined)?);
			} else if args.format.is_md() {
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
			} else if args.format.is_text() {
				// Use text formatting function for token efficiency
				let text_output = indexer::search::format_combined_search_results_as_text(
					&final_code_results,
					&text_blocks,
					&doc_blocks,
					effective_detail_level,
				);
				println!("{}", text_output);
			} else {
				if !doc_blocks.is_empty() {
					println!("=== DOCUMENTATION RESULTS ===\n");
					render_document_blocks_with_config(&doc_blocks, config, effective_detail_level);
					println!("\n");
				}

				if !final_code_results.is_empty() {
					println!("=== CODE RESULTS ===\n");
					indexer::render_code_blocks_with_config(
						&final_code_results,
						config,
						effective_detail_level,
					);
					println!("\n");
				}

				if !text_blocks.is_empty() {
					println!("=== TEXT RESULTS ===\n");
					render_text_blocks_with_config(&text_blocks, config, effective_detail_level);
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

fn render_text_blocks_with_config(
	blocks: &[octocode::store::TextBlock],
	_config: &Config,
	detail_level: &str,
) {
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
			println!("║ Block {} of {}: Text Block", idx + 1, file_blocks.len());
			println!("║ Lines: {}-{}", block.start_line, block.end_line);

			// Show similarity score if available
			if let Some(distance) = block.distance {
				println!("║ Similarity: {:.4}", 1.0 - distance);
			}

			println!("║");

			// Add content based on detail level (consistent with MCP smart truncation)
			match detail_level {
				"signatures" => {
					// Show only first line for signatures mode
					if let Some(first_line) = block.content.lines().next() {
						println!("║ {:4} │ {}", block.start_line, first_line.trim());
					}
				}
				"partial" => {
					// Show smart truncated content (first 4 + last 3 lines with separator)
					let lines: Vec<&str> = block.content.lines().collect();
					if lines.len() <= 10 {
						// Show all lines if content is short
						for (i, line) in lines.iter().enumerate() {
							println!("║ {:4} │ {}", block.start_line + i + 1, line);
						}
					} else {
						// Smart truncation: first 4 lines
						for (i, line) in lines.iter().take(4).enumerate() {
							println!("║ {:4} │ {}", block.start_line + i + 1, line);
						}

						// Show separator with count
						let omitted_lines = lines.len() - 7; // 4 start + 3 end
						if omitted_lines > 0 {
							println!("║      │ ... ({} more lines)", omitted_lines);
						}

						// Last 3 lines
						let last_3_start = lines.len() - 3;
						for (i, line) in lines.iter().skip(last_3_start).enumerate() {
							println!("║ {:4} │ {}", block.start_line + last_3_start + i + 1, line);
						}
					}
				}
				"full" => {
					// Show full content with line numbers
					let lines: Vec<&str> = block.content.lines().collect();
					for (i, line) in lines.iter().enumerate() {
						println!("║ {:4} │ {}", block.start_line + i + 1, line);
					}
				}
				_ => {
					// Default to partial
					let lines: Vec<&str> = block.content.lines().collect();
					if lines.len() <= 10 {
						for (i, line) in lines.iter().enumerate() {
							println!("║ {:4} │ {}", block.start_line + i + 1, line);
						}
					} else {
						// Smart truncation: first 4 lines
						for (i, line) in lines.iter().take(4).enumerate() {
							println!("║ {:4} │ {}", block.start_line + i + 1, line);
						}

						let omitted_lines = lines.len() - 7;
						if omitted_lines > 0 {
							println!("║      │ ... ({} more lines)", omitted_lines);
						}

						// Last 3 lines
						let last_3_start = lines.len() - 3;
						for (i, line) in lines.iter().skip(last_3_start).enumerate() {
							println!("║ {:4} │ {}", block.start_line + last_3_start + i + 1, line);
						}
					}
				}
			}

			if idx < file_blocks.len() - 1 {
				println!("║");
				println!("╠══════════════════════════════════════════════");
			}
		}

		println!("╚══════════════════════════════════════════════\n");
	}
}

fn render_document_blocks_with_config(
	blocks: &[octocode::store::DocumentBlock],
	_config: &Config,
	detail_level: &str,
) {
	if blocks.is_empty() {
		println!("No document blocks found.");
		return;
	}

	println!("Found {} document blocks:\n", blocks.len());

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
				"║ Block {} of {}: Document Block",
				idx + 1,
				file_blocks.len()
			);
			println!("║ Lines: {}-{}", block.start_line, block.end_line);

			// Show similarity score if available
			if let Some(distance) = block.distance {
				println!("║ Similarity: {:.4}", 1.0 - distance);
			}

			println!("║");

			// Add content based on detail level (consistent with smart truncation)
			match detail_level {
				"signatures" => {
					// Show only title/first line for signatures mode
					if !block.title.is_empty() {
						println!("║ Title: {}", block.title);
					} else if let Some(first_line) = block.content.lines().next() {
						println!("║ {}: {}", block.start_line, first_line.trim());
					}
				}
				"partial" => {
					// Show title and smart truncated content (first 4 + last 3 lines with separator)
					if !block.title.is_empty() {
						println!("║ Title: {}", block.title);
					}
					let lines: Vec<&str> = block.content.lines().collect();
					if lines.len() <= 10 {
						// Show all lines if content is short
						for (i, line) in lines.iter().enumerate() {
							println!("║ {:4} │ {}", block.start_line + i + 1, line);
						}
					} else {
						// Smart truncation: first 4 lines
						for (i, line) in lines.iter().take(4).enumerate() {
							println!("║ {:4} │ {}", block.start_line + i + 1, line);
						}

						// Show separator with count
						let omitted_lines = lines.len() - 7; // 4 start + 3 end
						if omitted_lines > 0 {
							println!("║      │ ... ({} more lines)", omitted_lines);
						}

						// Last 3 lines
						let last_3_start = lines.len() - 3;
						for (i, line) in lines.iter().skip(last_3_start).enumerate() {
							println!("║ {:4} │ {}", block.start_line + last_3_start + i + 1, line);
						}
					}
				}
				"full" => {
					// Show full content with line numbers
					if !block.title.is_empty() {
						println!("║ Title: {}", block.title);
					}
					let lines: Vec<&str> = block.content.lines().collect();
					for (i, line) in lines.iter().enumerate() {
						println!("║ {:4} │ {}", block.start_line + i + 1, line);
					}
				}
				_ => {
					// Default to partial
					if !block.title.is_empty() {
						println!("║ Title: {}", block.title);
					}
					let lines: Vec<&str> = block.content.lines().collect();
					if lines.len() <= 10 {
						for (i, line) in lines.iter().enumerate() {
							println!("║ {:4} │ {}", block.start_line + i + 1, line);
						}
					} else {
						// Smart truncation: first 4 lines
						for (i, line) in lines.iter().take(4).enumerate() {
							println!("║ {:4} │ {}", block.start_line + i + 1, line);
						}

						let omitted_lines = lines.len() - 7;
						if omitted_lines > 0 {
							println!("║      │ ... ({} more lines)", omitted_lines);
						}

						// Last 3 lines
						let last_3_start = lines.len() - 3;
						for (i, line) in lines.iter().skip(last_3_start).enumerate() {
							println!("║ {:4} │ {}", block.start_line + last_3_start + i + 1, line);
						}
					}
				}
			}

			if idx < file_blocks.len() - 1 {
				println!("║");
				println!("╠══════════════════════════════════════════════");
			}
		}

		println!("╚══════════════════════════════════════════════\n");
	}
}
