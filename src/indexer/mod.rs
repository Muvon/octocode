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

// Indexer module for Octocode
// Handles code indexing, embedding, and search functionality

pub mod graph_optimization;
pub mod graphrag; // GraphRAG generation for code relationships (modular implementation)
pub mod languages; // Language-specific processors
pub mod markdown_processor; // Markdown document processing utilities
pub mod search; // Search functionality // Task-focused graph extraction and optimization
pub mod signature_extractor; // Code signature extraction utilities

pub mod render_utils;
pub use graph_optimization::*;
pub use graphrag::*;
pub use languages::*;
pub use markdown_processor::*;
pub use search::*;
pub use signature_extractor::*;

use crate::config::Config;
use crate::embedding::{
	calculate_content_hash_with_lines, calculate_unique_content_hash, count_tokens,
};
use crate::mcp::logging::{
	log_file_processing_error, log_indexing_progress, log_performance_metrics,
};
use crate::state;
use crate::state::SharedState;
use crate::store::{CodeBlock, DocumentBlock, Store, TextBlock};
pub use render_utils::*;

// Import the new modular utilities
mod file_utils;
mod git_utils;
mod path_utils;
mod text_processing;

use self::file_utils::FileUtils;
use self::git_utils::GitUtils;
use self::text_processing::{TextChunkWithLines, TextProcessor};

// Re-export for external use
pub use self::path_utils::PathUtils;
use std::fs;
// We're using ignore::WalkBuilder instead of walkdir::WalkDir
use anyhow::Result;
use ignore;
// serde::Serialize moved to signature_extractor module
use std::path::Path;
use tree_sitter::{Node, Parser};

// Signature extraction types moved to signature_extractor module

/// Utility to create an ignore Walker that respects both .gitignore and .noindex files
pub struct NoindexWalker;

impl NoindexWalker {
	/// Creates a WalkBuilder that respects .gitignore and .noindex files
	/// FIXED: Properly handles .noindex patterns using custom filter
	pub fn create_walker(current_dir: &Path) -> ignore::WalkBuilder {
		let mut builder = ignore::WalkBuilder::new(current_dir);

		// Standard git ignore settings
		builder
			.hidden(true) // Ignore hidden files (like .git/, .vscode/, etc.)
			.git_ignore(true) // Respect .gitignore files
			.git_global(true) // Respect global git ignore files
			.git_exclude(true); // Respect .git/info/exclude files

		// FIXED: Use add_custom_ignore_filename to properly handle .noindex
		// This method actually works unlike add_ignore()
		builder.add_custom_ignore_filename(".noindex");

		builder
	}

	/// Creates a GitignoreBuilder for checking individual files against both .gitignore and .noindex
	/// ENHANCED: Better error handling and debugging
	pub fn create_matcher(current_dir: &Path, quiet: bool) -> Result<ignore::gitignore::Gitignore> {
		let mut builder = ignore::gitignore::GitignoreBuilder::new(current_dir);

		// Add .gitignore files
		let gitignore_path = current_dir.join(".gitignore");
		if gitignore_path.exists() {
			if let Some(e) = builder.add(&gitignore_path) {
				if !quiet {
					eprintln!("Warning: Failed to load .gitignore file: {}", e);
				}
			} // Successfully loaded
		}

		// Add .noindex file if it exists
		let noindex_path = current_dir.join(".noindex");
		if noindex_path.exists() {
			if let Some(e) = builder.add(&noindex_path) {
				if !quiet {
					eprintln!("Warning: Failed to load .noindex file for matcher: {}", e);
				}
			} // Successfully loaded
		}

		Ok(builder.build()?)
	}
}

/// Git utilities for repository management
pub mod git {
	use super::GitUtils;
	use anyhow::Result;
	use std::path::Path;

	/// Check if current directory is a git repository root
	pub fn is_git_repo_root(path: &Path) -> bool {
		GitUtils::is_git_repo_root(path)
	}

	/// Find git repository root from current path
	pub fn find_git_root(start_path: &Path) -> Option<std::path::PathBuf> {
		GitUtils::find_git_root(start_path)
	}

	/// Get current git commit hash
	pub fn get_current_commit_hash(repo_path: &Path) -> Result<String> {
		GitUtils::get_current_commit_hash(repo_path)
	}

	/// Get files changed between two commits (committed changes only, no unstaged)
	pub fn get_changed_files_since_commit(
		repo_path: &Path,
		since_commit: &str,
	) -> Result<Vec<String>> {
		GitUtils::get_changed_files_since_commit(repo_path, since_commit)
	}

	/// Get all working directory changes (staged + unstaged + untracked)
	/// Note: This is used for non-git optimization scenarios only
	pub fn get_all_changed_files(repo_path: &Path) -> Result<Vec<String>> {
		GitUtils::get_all_changed_files(repo_path)
	}
}

/// Get file modification time as seconds since Unix epoch
pub fn get_file_mtime(file_path: &std::path::Path) -> Result<u64> {
	FileUtils::get_file_mtime(file_path)
}

// Detect language based on file extension
pub fn detect_language(path: &std::path::Path) -> Option<&str> {
	FileUtils::detect_language(path)
}

// Signature extraction functions moved to signature_extractor module

// Signature extraction helper functions moved to signature_extractor module

// Signature extraction utility functions moved to signature_extractor module

// Markdown processing types and implementation moved to markdown_processor module

// All DocumentHierarchy implementation moved to markdown_processor module
// All DocumentHierarchy implementation and markdown functions moved to markdown_processor module

/// Optimized cleanup function that only processes files that actually need cleanup
async fn cleanup_deleted_files_optimized(
	store: &Store,
	current_dir: &std::path::Path,
	quiet: bool,
) -> Result<()> {
	// Get all indexed file paths from the database
	let indexed_files = store.get_all_indexed_file_paths().await?;

	// Early exit if no files to check
	if indexed_files.is_empty() {
		return Ok(());
	}

	// Create ignore matcher to check against .noindex and .gitignore patterns
	let ignore_matcher = NoindexWalker::create_matcher(current_dir, quiet)?;

	// Use parallel processing for file existence checks
	let mut files_to_remove = Vec::new();

	// Convert HashSet to Vec for chunking
	let indexed_files_vec: Vec<String> = indexed_files.into_iter().collect();

	// Process files in chunks to avoid overwhelming the file system
	const CHUNK_SIZE: usize = 100;
	for chunk in indexed_files_vec.chunks(CHUNK_SIZE) {
		for indexed_file in chunk {
			// Always treat indexed paths as relative to current directory
			let absolute_path = current_dir.join(indexed_file);

			// Check if file was deleted
			if !absolute_path.exists() {
				files_to_remove.push(indexed_file.clone());
			} else {
				// Check if file is now ignored by .noindex or .gitignore patterns
				let is_ignored = ignore_matcher
					.matched(&absolute_path, absolute_path.is_dir())
					.is_ignore();
				if is_ignored {
					files_to_remove.push(indexed_file.clone());
				}
			}
		}

		// Process removals in batches to avoid overwhelming the database
		if files_to_remove.len() >= CHUNK_SIZE {
			for file_to_remove in &files_to_remove {
				if let Err(e) = store.remove_blocks_by_path(file_to_remove).await {
					eprintln!(
						"Warning: Failed to remove blocks for {}: {}",
						file_to_remove, e
					);
				}
			}
			files_to_remove.clear();

			// Flush after each chunk to maintain data consistency
			store.flush().await?;
		}
	}

	// Remove any remaining files
	if !files_to_remove.is_empty() {
		for file_to_remove in &files_to_remove {
			if let Err(e) = store.remove_blocks_by_path(file_to_remove).await {
				if !quiet {
					eprintln!(
						"Warning: Failed to remove blocks for {}: {}",
						file_to_remove, e
					);
				}
			}
		}
		// Final flush
		store.flush().await?;
	}

	Ok(())
}

/// Helper function to perform intelligent flushing based on configuration
/// Returns true if a flush was performed
async fn flush_if_needed(
	store: &Store,
	batches_processed: &mut usize,
	config: &Config,
	force: bool,
) -> Result<bool> {
	if force || *batches_processed >= config.index.flush_frequency {
		store.flush().await?;
		*batches_processed = 0; // Reset counter
		Ok(true)
	} else {
		Ok(false)
	}
}

/// Render signatures and search results as markdown output (more efficient for AI tools)
// Rendering functions have been moved to src/indexer/render_utils.rs
// Main function to index files with optional git optimization
pub async fn index_files(
	store: &Store,
	state: SharedState,
	config: &Config,
	git_repo_root: Option<&Path>,
) -> Result<()> {
	index_files_with_quiet(store, state, config, git_repo_root, false).await
}

pub async fn index_files_with_quiet(
	store: &Store,
	state: SharedState,
	config: &Config,
	git_repo_root: Option<&Path>,
	quiet: bool,
) -> Result<()> {
	let current_dir = state.read().current_directory.clone();
	let mut code_blocks_batch = Vec::new();
	let mut text_blocks_batch = Vec::new();
	let mut document_blocks_batch = Vec::new();
	let mut all_code_blocks = Vec::new(); // Store all code blocks for GraphRAG

	let mut embedding_calls = 0;
	let mut batches_processed = 0; // Track batches for intelligent flushing

	// Log indexing start
	log_indexing_progress(
		"indexing_start",
		0,
		0,
		Some(&current_dir.display().to_string()),
		0,
	);

	// Initialize GraphRAG state if enabled
	{
		let mut state_guard = state.write();
		state_guard.graphrag_enabled = config.graphrag.enabled;
		state_guard.graphrag_blocks = 0;
		state_guard.counting_files = true;
		state_guard.status_message = "Counting files...".to_string();
		state_guard.quiet_mode = quiet;
	}

	// Get force_reindex flag from state
	let force_reindex = state.read().force_reindex;

	// Git-based optimization: Get changed files if we have a git repository
	let git_changed_files = if let Some(git_root) = git_repo_root {
		if !force_reindex {
			// Try to get the last indexed commit
			if let Ok(Some(last_commit)) = store.get_last_commit_hash().await {
				// Get current commit
				if let Ok(current_commit) = git::get_current_commit_hash(git_root) {
					if last_commit != current_commit {
						// Commit hash changed - get files changed since last indexed commit
						match git::get_changed_files_since_commit(git_root, &last_commit) {
							Ok(changed_files) => {
								if !quiet {
									println!(
										"🚀 Git optimization: Commit changed, found {} files to reindex",
										changed_files.len()
									);
								}
								Some(
									changed_files
										.into_iter()
										.collect::<std::collections::HashSet<_>>(),
								)
							}
							Err(e) => {
								if !quiet {
									eprintln!(
										"Warning: Could not get git changes, indexing all files: {}",
										e
									);
								}
								None
							}
						}
					} else {
						// Same commit hash - skip indexing entirely (ignore unstaged changes)
						if !quiet {
							println!("✅ No commit changes since last index, skipping reindex");
						}

						// Check if GraphRAG needs to be built from existing database even when no files changed
						if config.graphrag.enabled {
							let needs_graphrag_from_existing =
								store.graphrag_needs_indexing().await.unwrap_or(false);
							if needs_graphrag_from_existing {
								if !quiet {
									println!("🔗 Building GraphRAG from existing database...");
								}
								log_indexing_progress("graphrag_build", 0, 0, None, 0);
								let graph_builder =
									graphrag::GraphBuilder::new_with_quiet(config.clone(), quiet)
										.await?;
								graph_builder
									.build_from_existing_database(Some(state.clone()))
									.await?;
							}
						}

						{
							let mut state_guard = state.write();
							state_guard.indexing_complete = true;
						}
						return Ok(());
					}
				} else {
					// Could not get current commit, fall back to full indexing
					if !quiet {
						println!("⚠️  Could not get current commit hash, indexing all files");
					}
					None
				}
			} else {
				// No previous commit stored, need to index all files for baseline
				if !quiet {
					println!("📋 First-time git indexing: indexing all files");
				}
				None
			}
		} else {
			// Force reindex, ignore git optimization
			None
		}
	} else {
		// No git repository, use file-based optimization
		None
	};

	// Optimized cleanup: Only do cleanup if we have existing data and it's not a force reindex
	let should_cleanup_deleted_files = {
		let force_reindex = state.read().force_reindex;
		!force_reindex // Only cleanup if not force reindexing
	};

	if should_cleanup_deleted_files {
		{
			let mut state_guard = state.write();
			state_guard.status_message = "Checking for deleted files...".to_string();
		}

		// Log cleanup phase start
		log_indexing_progress("cleanup", 0, 0, None, 0);

		// Optimized cleanup: Get indexed files and check them efficiently
		if let Err(e) = cleanup_deleted_files_optimized(store, &current_dir, quiet).await {
			if !quiet {
				eprintln!("Warning: Cleanup failed: {}", e);
			}
		}

		{
			let mut state_guard = state.write();
			state_guard.status_message = "".to_string();
		}
	}

	// PERFORMANCE OPTIMIZATION: Load all file metadata in one batch query
	// This eliminates individual database queries for each file during traversal
	{
		let mut state_guard = state.write();
		state_guard.status_message = "Loading file metadata...".to_string();
	}

	let file_metadata_map = store.get_all_file_metadata().await?;
	if !quiet {
		println!(
			"📊 Loaded metadata for {} files from database",
			file_metadata_map.len()
		);
	}

	// Progressive processing: Skip separate counting phase and count during processing
	{
		let mut state_guard = state.write();
		state_guard.total_files = 0; // Will be updated progressively
		state_guard.counting_files = true;
		state_guard.status_message = "Starting indexing...".to_string();
	}

	// Single pass: progressive counting + processing combined
	// Use NoindexWalker to respect both .gitignore and .noindex files
	let walker = NoindexWalker::create_walker(&current_dir).build();

	// Progressive counting variables
	let mut total_files_found = 0;
	let mut files_processed = 0;

	// Log file processing phase start
	log_indexing_progress("file_processing", 0, 0, None, 0);

	for result in walker {
		let entry = match result {
			Ok(entry) => entry,
			Err(_) => continue,
		};

		// Skip directories, only process files
		if !entry.file_type().is_some_and(|ft| ft.is_file()) {
			continue;
		}

		// Create relative path from the current directory using our utility
		let file_path = path_utils::PathUtils::to_relative_string(entry.path(), &current_dir);

		// Check if this file would be indexed (for progressive counting)
		let is_indexable = if let Some(ref changed_files) = git_changed_files {
			// Git optimization: only count changed files that are indexable
			changed_files.contains(&file_path)
				&& (detect_language(entry.path()).is_some()
					|| is_allowed_text_extension(entry.path()))
		} else {
			// Normal mode: count all indexable files
			detect_language(entry.path()).is_some() || is_allowed_text_extension(entry.path())
		};

		if is_indexable {
			total_files_found += 1;

			// Update total count progressively every 10 files to avoid too frequent updates
			if total_files_found % 10 == 0 {
				let mut state_guard = state.write();
				state_guard.total_files = total_files_found;
				if total_files_found <= 50 {
					// Still in early discovery phase
					state_guard.status_message = format!("Found {} files...", total_files_found);
				}
			}
		}

		// GIT OPTIMIZATION: Skip files not in the changed set (if git optimization is active)
		if let Some(ref changed_files) = git_changed_files {
			if !changed_files.contains(&file_path) {
				// File not in git changes, skip processing entirely
				continue;
			}
		}

		// PERFORMANCE OPTIMIZATION: Fast file modification time check using preloaded metadata
		// This replaces individual database queries with HashMap lookup
		let force_reindex = state.read().force_reindex;
		if !force_reindex {
			if let Ok(actual_mtime) = get_file_mtime(entry.path()) {
				// Fast HashMap lookup instead of database query
				if let Some(stored_mtime) = file_metadata_map.get(&file_path) {
					if actual_mtime <= *stored_mtime {
						// File hasn't changed, skip processing entirely but count as skipped
						{
							let mut state_guard = state.write();
							state_guard.skipped_files += 1;
						}
						continue;
					}
				}
			}
		}

		if let Some(language) = detect_language(entry.path()) {
			match fs::read_to_string(entry.path()) {
				Ok(contents) => {
					// Store the file modification time after successful processing
					let file_processed;

					if language == "markdown" {
						// Handle markdown files specially - index as document blocks
						process_markdown_file_differential(
							store,
							&contents,
							&file_path,
							&mut document_blocks_batch,
							config,
							state.clone(),
						)
						.await?;
						file_processed = true;
					} else {
						// Handle code files - index as semantic code blocks only
						let ctx = ProcessFileContext {
							store,
							config,
							state: state.clone(),
						};
						process_file_differential(
							&ctx,
							&contents,
							&file_path,
							language,
							&mut code_blocks_batch,
							&mut text_blocks_batch, // Will remain empty for code files
							&mut all_code_blocks,
						)
						.await?;
						file_processed = true;
					}

					// Store file modification time after successful processing
					if file_processed {
						if let Ok(actual_mtime) = get_file_mtime(entry.path()) {
							let _ = store.store_file_metadata(&file_path, actual_mtime).await;
						}
					}

					files_processed += 1;
					state.write().indexed_files = files_processed;

					// Update counting phase status
					{
						let mut state_guard = state.write();
						if state_guard.counting_files && total_files_found > 50 {
							// Switch from counting to processing mode
							state_guard.counting_files = false;
							state_guard.total_files = total_files_found;
							state_guard.status_message = "".to_string();
						}
					}

					// Log progress periodically for code files
					if files_processed % 50 == 0 {
						let current_total = state.read().total_files;
						log_indexing_progress(
							"file_processing",
							files_processed,
							current_total,
							Some(&file_path),
							embedding_calls,
						);
					}

					// Process batches when they reach the batch size or token limit
					if should_process_batch(&code_blocks_batch, |b| &b.content, config) {
						embedding_calls += code_blocks_batch.len();
						process_code_blocks_batch(store, &code_blocks_batch, config).await?;
						code_blocks_batch.clear();
						batches_processed += 1;
						// Intelligent flush based on configuration
						flush_if_needed(store, &mut batches_processed, config, false).await?;
					}
					// Only process text_blocks_batch if we have any (from unsupported files)
					if should_process_batch(&text_blocks_batch, |b| &b.content, config) {
						embedding_calls += text_blocks_batch.len();
						process_text_blocks_batch(store, &text_blocks_batch, config).await?;
						text_blocks_batch.clear();
						batches_processed += 1;
						// Intelligent flush based on configuration
						flush_if_needed(store, &mut batches_processed, config, false).await?;
					}
					if should_process_batch(&document_blocks_batch, |b| &b.content, config) {
						embedding_calls += document_blocks_batch.len();
						process_document_blocks_batch(store, &document_blocks_batch, config)
							.await?;
						document_blocks_batch.clear();
						batches_processed += 1;
						// Intelligent flush based on configuration
						flush_if_needed(store, &mut batches_processed, config, false).await?;
					}
				}
				Err(e) => {
					// Log file reading error
					log_file_processing_error(&file_path, "read_file", &e);
				}
			}
		} else {
			// Handle unsupported file types as chunked text
			// First check if the file extension is in our whitelist
			// BUT exclude markdown files since they're already processed as documents
			if is_allowed_text_extension(entry.path()) && !is_markdown_file(entry.path()) {
				if let Ok(contents) = fs::read_to_string(entry.path()) {
					// Only process files that are likely to contain readable text
					if is_text_file(&contents) {
						process_text_file_differential(
							store,
							&contents,
							&file_path,
							&mut text_blocks_batch,
							config,
							state.clone(),
						)
						.await?;

						// Store file modification time after successful processing
						if let Ok(actual_mtime) = get_file_mtime(entry.path()) {
							let _ = store.store_file_metadata(&file_path, actual_mtime).await;
						}

						files_processed += 1;
						state.write().indexed_files = files_processed;

						// Update counting phase status
						{
							let mut state_guard = state.write();
							if state_guard.counting_files && total_files_found > 50 {
								// Switch from counting to processing mode
								state_guard.counting_files = false;
								state_guard.total_files = total_files_found;
								state_guard.status_message = "".to_string();
							}
						}

						// Log progress periodically for text files
						if files_processed % 50 == 0 {
							let current_total = state.read().total_files;
							log_indexing_progress(
								"file_processing",
								files_processed,
								current_total,
								Some(&file_path),
								embedding_calls,
							);
						}

						// Process batch when it reaches the batch size or token limit
						if should_process_batch(&text_blocks_batch, |b| &b.content, config) {
							embedding_calls += text_blocks_batch.len();
							process_text_blocks_batch(store, &text_blocks_batch, config).await?;
							text_blocks_batch.clear();
							batches_processed += 1;
							// Intelligent flush based on configuration
							flush_if_needed(store, &mut batches_processed, config, false).await?;
						}
					}
				}
			}
		}
	}

	// Process remaining batches
	if !code_blocks_batch.is_empty() {
		process_code_blocks_batch(store, &code_blocks_batch, config).await?;
		embedding_calls += code_blocks_batch.len();
		batches_processed += 1;
	}
	// Only process text_blocks_batch if we have any (from unsupported files)
	if !text_blocks_batch.is_empty() {
		process_text_blocks_batch(store, &text_blocks_batch, config).await?;
		embedding_calls += text_blocks_batch.len();
		batches_processed += 1;
	}
	if !document_blocks_batch.is_empty() {
		process_document_blocks_batch(store, &document_blocks_batch, config).await?;
		embedding_calls += document_blocks_batch.len();
		batches_processed += 1;
	}

	// Force flush any remaining data after processing all batches
	flush_if_needed(store, &mut batches_processed, config, true).await?;

	// Build GraphRAG if enabled
	if config.graphrag.enabled {
		// Check if we have new blocks from this indexing run OR if GraphRAG needs initial indexing
		let needs_graphrag_from_existing = if all_code_blocks.is_empty() {
			// No new blocks, check if GraphRAG needs indexing from existing database
			store.graphrag_needs_indexing().await.unwrap_or(false)
		} else {
			false // We have new blocks, process them normally
		};

		if !all_code_blocks.is_empty() || needs_graphrag_from_existing {
			{
				let mut state_guard = state.write();
				if needs_graphrag_from_existing {
					state_guard.status_message =
						"Building GraphRAG from existing database...".to_string();
				} else {
					state_guard.status_message = "Building GraphRAG knowledge graph...".to_string();
				}
			}

			// Log GraphRAG phase start
			log_indexing_progress(
				"graphrag_build",
				state.read().indexed_files,
				state.read().total_files,
				None,
				embedding_calls,
			);

			// Initialize GraphBuilder
			let graph_builder =
				graphrag::GraphBuilder::new_with_quiet(config.clone(), quiet).await?;

			if needs_graphrag_from_existing {
				// Build GraphRAG from existing database (critical fix for the reported issue)
				graph_builder
					.build_from_existing_database(Some(state.clone()))
					.await?;
			} else {
				// Process new code blocks to build/update the graph
				graph_builder
					.process_code_blocks(&all_code_blocks, Some(state.clone()))
					.await?;
			}

			// Update final state
			{
				let mut state_guard = state.write();
				state_guard.status_message = "".to_string();
			}
		}
	}

	{
		let mut state_guard = state.write();
		state_guard.indexing_complete = true;
		state_guard.embedding_calls = embedding_calls;
	}

	// Finalize counting if still in progress
	{
		let mut state_guard = state.write();
		if state_guard.counting_files {
			state_guard.counting_files = false;
			state_guard.total_files = total_files_found;
		}
	}

	// Log indexing completion
	let final_files = state.read().indexed_files;
	let final_total = state.read().total_files;
	log_indexing_progress(
		"indexing_complete",
		final_files,
		final_total,
		None,
		embedding_calls,
	);

	// Store current git commit hash for future optimization
	if let Some(git_root) = git_repo_root {
		if let Ok(current_commit) = git::get_current_commit_hash(git_root) {
			if let Err(e) = store.store_git_metadata(&current_commit).await {
				if !quiet {
					eprintln!("Warning: Could not store git metadata: {}", e);
				}
			}
		}
	}

	// Flush the store to ensure all data is persisted
	store.flush().await?;

	Ok(())
}

// Function to handle file changes (for watch mode)
pub async fn handle_file_change(store: &Store, file_path: &str, config: &Config) -> Result<()> {
	// Create a state for tracking changes
	let state = state::create_shared_state();
	{
		let mut state_guard = state.write();
		state_guard.graphrag_enabled = config.graphrag.enabled;
		state_guard.graphrag_blocks = 0;
	}

	// First, let's remove any existing code blocks for this file path
	store.remove_blocks_by_path(file_path).await?;

	// Now, if the file still exists, check if it should be indexed based on ignore rules
	let path = std::path::Path::new(file_path);
	if path.exists() {
		// Get the current directory for proper relative path handling
		let current_dir = std::env::current_dir()?;

		// Convert relative path to absolute for ignore checking
		let absolute_path = if path.is_absolute() {
			path.to_path_buf()
		} else {
			current_dir.join(path)
		};

		// Create a matcher that respects both .gitignore and .noindex rules
		if let Ok(matcher) = NoindexWalker::create_matcher(&current_dir, true) {
			// Use quiet=true for watcher
			// Check if the file should be ignored
			if matcher
				.matched(&absolute_path, absolute_path.is_dir())
				.is_ignore()
			{
				// File is in ignore patterns, so don't index it
				return Ok(());
			}
		}

		// File is not ignored, so proceed with indexing
		if let Some(language) = detect_language(&absolute_path) {
			if let Ok(contents) = fs::read_to_string(&absolute_path) {
				// Ensure we use relative path for storage
				let relative_file_path =
					path_utils::PathUtils::to_relative_string(&absolute_path, &current_dir);

				if language == "markdown" {
					// Handle markdown files specially
					let mut document_blocks_batch = Vec::new();
					process_markdown_file(
						store,
						&contents,
						&relative_file_path,
						&mut document_blocks_batch,
						config,
						state.clone(),
					)
					.await?;

					if !document_blocks_batch.is_empty() {
						process_document_blocks_batch(store, &document_blocks_batch, config)
							.await?;
					}
				} else {
					// Handle code files
					let mut code_blocks_batch = Vec::new();
					let mut text_blocks_batch = Vec::new(); // Will remain empty for code files
					let mut all_code_blocks = Vec::new(); // For GraphRAG

					let ctx = ProcessFileContext {
						store,
						config,
						state: state.clone(),
					};
					process_file(
						&ctx,
						&contents,
						&relative_file_path,
						language,
						&mut code_blocks_batch,
						&mut text_blocks_batch,
						&mut all_code_blocks,
					)
					.await?;

					if !code_blocks_batch.is_empty() {
						process_code_blocks_batch(store, &code_blocks_batch, config).await?;
					}
					// No need to process text_blocks_batch since it will be empty for code files

					// Update GraphRAG if enabled and we have new blocks
					if config.graphrag.enabled && !all_code_blocks.is_empty() {
						let graph_builder = graphrag::GraphBuilder::new(config.clone()).await?;
						graph_builder
							.process_code_blocks(&all_code_blocks, Some(state.clone()))
							.await?;
					}
				}

				// Explicitly flush to ensure all data is persisted
				store.flush().await?;
			}
		} else {
			// Handle unsupported file types as chunked text
			// First check if the file extension is in our whitelist
			if is_allowed_text_extension(&absolute_path) {
				if let Ok(contents) = fs::read_to_string(&absolute_path) {
					if is_text_file(&contents) {
						// Ensure we use relative path for storage
						let relative_file_path =
							path_utils::PathUtils::to_relative_string(&absolute_path, &current_dir);

						let mut text_blocks_batch = Vec::new();
						process_text_file(
							store,
							&contents,
							&relative_file_path,
							&mut text_blocks_batch,
							config,
							state.clone(),
						)
						.await?;

						if !text_blocks_batch.is_empty() {
							process_text_blocks_batch(store, &text_blocks_batch, config).await?;
						}

						// Explicitly flush to ensure all data is persisted
						store.flush().await?;
					}
				}
			}
		}
	}

	Ok(())
}

// Context for file processing to reduce the number of function arguments
struct ProcessFileContext<'a> {
	store: &'a Store,
	config: &'a Config,
	state: SharedState,
}

// Processes a single file, extracting code blocks and adding them to the batch
async fn process_file(
	ctx: &ProcessFileContext<'_>,
	contents: &str,
	file_path: &str,
	language: &str,
	code_blocks_batch: &mut Vec<CodeBlock>,
	_text_blocks_batch: &mut [TextBlock], // Unused for code files - only used for unsupported files
	all_code_blocks: &mut Vec<CodeBlock>,
) -> Result<()> {
	let mut parser = Parser::new();

	// Get force_reindex flag from state
	let force_reindex = ctx.state.read().force_reindex;

	// Get the language implementation
	let lang_impl = match languages::get_language(language) {
		Some(impl_) => impl_,
		None => return Ok(()), // Skip unsupported languages
	};

	// Set the parser language
	parser.set_language(&lang_impl.get_ts_language())?;

	let tree = parser
		.parse(contents, None)
		.unwrap_or_else(|| parser.parse("", None).unwrap());
	let mut code_regions = Vec::new();

	extract_meaningful_regions(
		tree.root_node(),
		contents,
		lang_impl.as_ref(),
		&mut code_regions,
	);

	// Track the number of blocks we added to all_code_blocks for GraphRAG
	let mut graphrag_blocks_added = 0;

	for region in code_regions {
		// Use a hash that includes content, path, and line ranges
		let content_hash = calculate_content_hash_with_lines(
			&region.content,
			file_path,
			region.start_line,
			region.end_line,
		);

		// Skip the check if force_reindex is true
		let exists = !force_reindex
			&& ctx
				.store
				.content_exists(&content_hash, "code_blocks")
				.await?;
		if !exists {
			let code_block = CodeBlock {
				path: file_path.to_string(),
				hash: content_hash,
				language: lang_impl.name().to_string(),
				content: region.content.clone(),
				symbols: region.symbols.clone(),
				start_line: region.start_line,
				end_line: region.end_line,
				distance: None, // No relevance score when indexing
			};

			// Add to batch for embedding
			code_blocks_batch.push(code_block.clone());

			// Add to all code blocks for GraphRAG
			if ctx.config.graphrag.enabled {
				all_code_blocks.push(code_block);
				graphrag_blocks_added += 1;
			}
		} else if ctx.config.graphrag.enabled {
			// If skipping because block exists, but we need for GraphRAG, fetch from store
			if let Ok(existing_block) = ctx.store.get_code_block_by_hash(&content_hash).await {
				// Add the existing block to the GraphRAG collection
				all_code_blocks.push(existing_block);
				graphrag_blocks_added += 1;
			}
		}
	}

	// Update GraphRAG state if enabled and blocks were added
	if ctx.config.graphrag.enabled && graphrag_blocks_added > 0 {
		let mut state_guard = ctx.state.write();
		state_guard.graphrag_blocks += graphrag_blocks_added;
	}

	// Note: We DON'T create text blocks for code files - only for unsupported file types
	// Code files are already indexed as semantic code blocks above

	Ok(())
}

/// Represents a meaningful code block/region with tree-sitter node information.
#[derive(Clone)]
struct CodeRegion {
	content: String,
	symbols: Vec<String>,
	start_line: usize,
	end_line: usize,
	node_kind: String, // Store the original tree-sitter node kind
	node_id: usize,    // Store a unique node identifier for grouping
}

// Configuration for single-line block merging
const MAX_LINES_PER_BLOCK: usize = 15; // Maximum lines in a merged block
const MIN_LINES_TO_MERGE: usize = 2; // Minimum consecutive single-lines to merge

/// Recursively extracts meaningful regions based on node kinds.
/// Includes smart merging of single-line declarations.
fn extract_meaningful_regions(
	node: Node,
	contents: &str,
	lang_impl: &dyn languages::Language,
	regions: &mut Vec<CodeRegion>,
) {
	let meaningful_kinds = lang_impl.get_meaningful_kinds();
	let mut candidate_regions = Vec::new();

	// First pass: collect all meaningful regions without merging
	collect_meaningful_regions_recursive(
		node,
		contents,
		lang_impl,
		&meaningful_kinds,
		&mut candidate_regions,
	);

	// Second pass: apply smart merging logic
	apply_smart_merging(candidate_regions, regions, lang_impl);
}

/// Recursively collects meaningful regions without merging
fn collect_meaningful_regions_recursive(
	node: Node,
	contents: &str,
	lang_impl: &dyn languages::Language,
	meaningful_kinds: &[&str],
	regions: &mut Vec<CodeRegion>,
) {
	let node_kind = node.kind();

	if meaningful_kinds.contains(&node_kind) {
		let (combined_content, start_line) = combine_with_preceding_comments(node, contents);
		let end_line = node.end_position().row;
		let symbols = lang_impl.extract_symbols(node, contents);

		// Only create a region if we have meaningful content
		if !combined_content.trim().is_empty() {
			// Ensure we have at least one symbol by using the node kind if necessary
			let mut final_symbols = symbols;
			if final_symbols.is_empty() {
				// Create a default symbol from the node kind
				final_symbols.push(format!("{}_{}", node_kind, start_line));
			}

			regions.push(CodeRegion {
				content: combined_content,
				symbols: final_symbols,
				start_line,
				end_line,
				node_kind: node_kind.to_string(),
				node_id: node.id(),
			});
		}
		return;
	}

	let mut cursor = node.walk();
	if cursor.goto_first_child() {
		loop {
			collect_meaningful_regions_recursive(
				cursor.node(),
				contents,
				lang_impl,
				meaningful_kinds,
				regions,
			);
			if !cursor.goto_next_sibling() {
				break;
			}
		}
	}
}

/// Applies smart merging logic to consolidate single-line declarations
fn apply_smart_merging(
	candidate_regions: Vec<CodeRegion>,
	final_regions: &mut Vec<CodeRegion>,
	lang_impl: &dyn languages::Language,
) {
	if candidate_regions.is_empty() {
		return;
	}

	let mut i = 0;
	while i < candidate_regions.len() {
		let current = &candidate_regions[i];

		// Check if this is a single-line block
		if is_single_line_declaration(current) {
			// Look ahead to find consecutive single-line blocks
			let mut consecutive_single_lines = vec![current];
			let mut j = i + 1;

			while j < candidate_regions.len() {
				let next = &candidate_regions[j];
				if is_single_line_declaration(next)
					&& are_consecutive_or_related(
						consecutive_single_lines.last().unwrap(),
						next,
						lang_impl,
					) {
					consecutive_single_lines.push(next);
					j += 1;
				} else {
					break;
				}
			}

			// If we have enough single-line blocks to merge
			if consecutive_single_lines.len() >= MIN_LINES_TO_MERGE {
				merge_single_line_blocks(consecutive_single_lines, final_regions, lang_impl);
				i = j; // Skip the processed blocks
			} else {
				// Not enough to merge, add as-is
				final_regions.push(current.clone());
				i += 1;
			}
		} else {
			// Multi-line block, add as-is
			final_regions.push(current.clone());
			i += 1;
		}
	}
}

/// Checks if a code region is a single-line declaration
fn is_single_line_declaration(region: &CodeRegion) -> bool {
	// Consider it single-line if it's 1 line or very short content
	let line_count = region.end_line - region.start_line + 1;
	let is_short = region.content.trim().lines().count() <= 1 || region.content.len() < 80;

	line_count <= 1 || (line_count <= 2 && is_short)
}

/// Checks if two single-line regions are consecutive or thematically related
fn are_consecutive_or_related(
	first: &CodeRegion,
	second: &CodeRegion,
	lang_impl: &dyn languages::Language,
) -> bool {
	// Consider consecutive if they're within a few lines of each other
	let line_gap = second.start_line.saturating_sub(first.end_line);

	// Allow small gaps (for comments or empty lines)
	if line_gap <= 3 {
		return true;
	}

	// Check if they're thematically related (same type of declaration)
	if are_thematically_related(first, second, lang_impl) {
		return line_gap <= 10; // Allow larger gaps for related content
	}

	false
}

/// Checks if two regions contain thematically related content using language-specific logic
fn are_thematically_related(
	first: &CodeRegion,
	second: &CodeRegion,
	lang_impl: &dyn languages::Language,
) -> bool {
	// Direct node kind match
	if first.node_kind == second.node_kind {
		return true;
	}

	// Use language-specific semantic equivalence
	lang_impl.are_node_types_equivalent(&first.node_kind, &second.node_kind)
}

/// Merges consecutive single-line blocks into optimally-sized chunks
fn merge_single_line_blocks(
	single_lines: Vec<&CodeRegion>,
	final_regions: &mut Vec<CodeRegion>,
	lang_impl: &dyn languages::Language,
) {
	if single_lines.is_empty() {
		return;
	}

	// Group by theme first
	let mut themed_groups: Vec<Vec<&CodeRegion>> = Vec::new();
	let mut current_group = vec![single_lines[0]];

	for region in single_lines.iter().skip(1) {
		if are_thematically_related(current_group[0], region, lang_impl) {
			current_group.push(region);
		} else {
			themed_groups.push(current_group);
			current_group = vec![region];
		}
	}
	themed_groups.push(current_group);

	// Process each themed group
	for group in themed_groups {
		create_merged_blocks_from_group(group, final_regions, lang_impl);
	}
}

/// Creates merged blocks from a thematically grouped set of single-line regions
fn create_merged_blocks_from_group(
	group: Vec<&CodeRegion>,
	final_regions: &mut Vec<CodeRegion>,
	lang_impl: &dyn languages::Language,
) {
	if group.is_empty() {
		return;
	}

	// Split into chunks if the group is too large
	let chunks: Vec<&[&CodeRegion]> = group.chunks(MAX_LINES_PER_BLOCK).collect();

	for chunk in chunks {
		if chunk.is_empty() {
			continue;
		}

		let start_line = chunk[0].start_line;
		let end_line = chunk.last().unwrap().end_line;

		// Combine content from all regions in the chunk
		let mut combined_content = String::new();
		let mut all_symbols = Vec::new();

		for (i, region) in chunk.iter().enumerate() {
			if i > 0 {
				combined_content.push('\n');
			}
			combined_content.push_str(&region.content);
			all_symbols.extend(region.symbols.clone());
		}

		// Deduplicate symbols
		all_symbols.sort();
		all_symbols.dedup();

		// Add a descriptive comment to the merged block using language-specific description
		let block_type = determine_block_type(chunk[0], lang_impl);
		let block_description =
			format!("// Merged {} ({} declarations)\n", block_type, chunk.len());
		let final_content = block_description + &combined_content;

		final_regions.push(CodeRegion {
			content: final_content,
			symbols: all_symbols,
			start_line,
			end_line,
			node_kind: chunk[0].node_kind.clone(),
			node_id: chunk[0].node_id, // Use the first region's node_id as representative
		});
	}
}

/// Determines the type of block for descriptive purposes using language-specific logic
fn determine_block_type(
	sample_region: &CodeRegion,
	lang_impl: &dyn languages::Language,
) -> &'static str {
	lang_impl.get_node_type_description(&sample_region.node_kind)
}

/// Combines preceding comment or attribute nodes with a declaration node.
fn combine_with_preceding_comments(node: Node, contents: &str) -> (String, usize) {
	let mut combined_start = node.start_position().row;
	let mut snippet = String::new();
	if let Some(parent) = node.parent() {
		let mut cursor = parent.walk();
		let mut preceding = Vec::new();
		for child in parent.children(&mut cursor) {
			if child.id() == node.id() {
				break;
			} else {
				preceding.push(child);
			}
		}
		if let Some(last) = preceding.last() {
			let kind = last.kind();
			if kind.contains("comment") || kind.contains("attribute") {
				combined_start = last.start_position().row;
				snippet.push_str(&contents[last.start_byte()..last.end_byte()]);
				snippet.push('\n');
			}
		}
	}
	snippet.push_str(&contents[node.start_byte()..node.end_byte()]);
	(snippet, combined_start)
}

async fn process_code_blocks_batch(
	store: &Store,
	blocks: &[CodeBlock],
	config: &Config,
) -> Result<()> {
	let start_time = std::time::Instant::now();
	let contents: Vec<String> = blocks.iter().map(|b| b.content.clone()).collect();
	let embeddings = crate::embedding::generate_embeddings_batch(contents, true, config).await?;
	store.store_code_blocks(blocks, &embeddings).await?;

	let duration_ms = start_time.elapsed().as_millis() as u64;
	log_performance_metrics("code_blocks_batch", duration_ms, blocks.len(), None);

	Ok(())
}

async fn process_text_blocks_batch(
	store: &Store,
	blocks: &[TextBlock],
	config: &Config,
) -> Result<()> {
	let start_time = std::time::Instant::now();
	let contents: Vec<String> = blocks.iter().map(|b| b.content.clone()).collect();
	let embeddings = crate::embedding::generate_embeddings_batch(contents, false, config).await?;
	store.store_text_blocks(blocks, &embeddings).await?;

	let duration_ms = start_time.elapsed().as_millis() as u64;
	log_performance_metrics("text_blocks_batch", duration_ms, blocks.len(), None);

	Ok(())
}

async fn process_document_blocks_batch(
	store: &Store,
	blocks: &[DocumentBlock],
	config: &Config,
) -> Result<()> {
	let start_time = std::time::Instant::now();
	let contents: Vec<String> = blocks
		.iter()
		.map(|b| {
			if !b.context.is_empty() {
				format!("{}\n\n{}", b.context.join("\n"), b.content)
			} else {
				b.content.clone()
			}
		})
		.collect();
	let embeddings = crate::embedding::generate_embeddings_batch(contents, false, config).await?;
	store.store_document_blocks(blocks, &embeddings).await?;

	let duration_ms = start_time.elapsed().as_millis() as u64;
	log_performance_metrics("document_blocks_batch", duration_ms, blocks.len(), None);

	Ok(())
}

/// Check if a batch should be processed based on size and token limits
fn should_process_batch<T>(batch: &[T], get_content: impl Fn(&T) -> &str, config: &Config) -> bool {
	if batch.is_empty() {
		return false;
	}

	// Check size limit
	if batch.len() >= config.index.embeddings_batch_size {
		return true;
	}

	// Check token limit
	let total_tokens: usize = batch
		.iter()
		.map(|item| count_tokens(get_content(item)))
		.sum();

	total_tokens >= config.index.embeddings_max_tokens_per_batch
}

// Constants for text chunking - REMOVED: Now using config.index.chunk_size and config.index.chunk_overlap

/// Check if a file extension is allowed for text indexing
fn is_allowed_text_extension(path: &std::path::Path) -> bool {
	FileUtils::is_allowed_text_extension(path)
}

/// Check if a file is a markdown file
fn is_markdown_file(path: &std::path::Path) -> bool {
	if let Some(extension) = path.extension() {
		if let Some(ext_str) = extension.to_str() {
			let ext_lower = ext_str.to_lowercase();
			return ext_lower == "md" || ext_lower == "markdown";
		}
	}
	false
}

/// Check if a file contains readable text
fn is_text_file(contents: &str) -> bool {
	FileUtils::is_text_file(contents)
}

fn chunk_text(content: &str, chunk_size: usize, overlap: usize) -> Vec<TextChunkWithLines> {
	TextProcessor::chunk_text(content, chunk_size, overlap)
}

/// Process an unsupported file as chunked text blocks
/// Only processes files with whitelisted extensions to avoid indexing
/// binary files, lock files, and other non-useful content.
/// Supported extensions: txt, log, xml, html, css, sql, csv, yaml, toml, ini, conf, etc.
/// Chunk size: 2000 characters with 200 character overlap.
/// FIXED: Now stores simple path without chunk numbers for better display
async fn process_text_file(
	store: &Store,
	contents: &str,
	file_path: &str,
	text_blocks_batch: &mut Vec<TextBlock>,
	config: &Config,
	state: SharedState,
) -> Result<()> {
	let force_reindex = state.read().force_reindex;

	// Split content into chunks using configuration values
	let chunks = chunk_text(
		contents,
		config.index.chunk_size,
		config.index.chunk_overlap,
	);

	for (chunk_idx, chunk_with_lines) in chunks.iter().enumerate() {
		// Use chunk index in hash for uniqueness but keep path clean
		let chunk_hash = calculate_unique_content_hash(
			&chunk_with_lines.content,
			&format!("{}#{}", file_path, chunk_idx),
		);

		// Skip the check if force_reindex is true
		let exists = !force_reindex && store.content_exists(&chunk_hash, "text_blocks").await?;
		if !exists {
			text_blocks_batch.push(TextBlock {
				path: file_path.to_string(),
				language: "text".to_string(),
				content: chunk_with_lines.content.clone(),
				start_line: chunk_with_lines.start_line, // Use directly from TextChunkWithLines
				end_line: chunk_with_lines.end_line,     // Use directly from TextChunkWithLines
				hash: chunk_hash,
				distance: None,
			});
		}
	}

	Ok(())
}

// Process a markdown file, extracting document blocks
async fn process_markdown_file(
	store: &Store,
	contents: &str,
	file_path: &str,
	document_blocks_batch: &mut Vec<DocumentBlock>,
	config: &Config,
	state: SharedState,
) -> Result<()> {
	// Get force_reindex flag from state
	let force_reindex = state.read().force_reindex;

	// Parse markdown content into document blocks using context-aware chunking
	let document_blocks = parse_markdown_content(contents, file_path, config);

	for doc_block in document_blocks {
		// Check if this document block already exists (unless force reindex)
		let exists = !force_reindex
			&& store
				.content_exists(&doc_block.hash, "document_blocks")
				.await?;
		if !exists {
			document_blocks_batch.push(doc_block);
		}
	}

	Ok(())
}

// NEW DIFFERENTIAL PROCESSING FUNCTIONS

// Differential processing for code files - only updates changed blocks
async fn process_file_differential(
	ctx: &ProcessFileContext<'_>,
	contents: &str,
	file_path: &str,
	language: &str,
	code_blocks_batch: &mut Vec<CodeBlock>,
	_text_blocks_batch: &mut [TextBlock], // Unused for code files
	all_code_blocks: &mut Vec<CodeBlock>,
) -> Result<()> {
	let mut parser = Parser::new();

	// Get force_reindex flag from state
	let force_reindex = ctx.state.read().force_reindex;

	// Get the language implementation
	let lang_impl = match languages::get_language(language) {
		Some(impl_) => impl_,
		None => return Ok(()), // Skip unsupported languages
	};

	// Set the parser language
	parser.set_language(&lang_impl.get_ts_language())?;

	let tree = parser
		.parse(contents, None)
		.unwrap_or_else(|| parser.parse("", None).unwrap());
	let mut code_regions = Vec::new();

	extract_meaningful_regions(
		tree.root_node(),
		contents,
		lang_impl.as_ref(),
		&mut code_regions,
	);

	// If not force reindexing, get existing hashes for this file to compare
	let existing_hashes = if force_reindex {
		Vec::new()
	} else {
		ctx.store
			.get_file_blocks_metadata(file_path, "code_blocks")
			.await?
	};

	// Create set of new hashes for this file
	let mut new_hashes = std::collections::HashSet::new();
	let mut graphrag_blocks_added = 0;

	for region in code_regions {
		// Use a hash that includes content, path, and line ranges
		let content_hash = calculate_content_hash_with_lines(
			&region.content,
			file_path,
			region.start_line,
			region.end_line,
		);
		new_hashes.insert(content_hash.clone());

		// Skip the check if force_reindex is true
		let exists = !force_reindex
			&& ctx
				.store
				.content_exists(&content_hash, "code_blocks")
				.await?;
		if !exists {
			let code_block = CodeBlock {
				path: file_path.to_string(),
				hash: content_hash.clone(),
				language: lang_impl.name().to_string(),
				content: region.content.clone(),
				symbols: region.symbols.clone(),
				start_line: region.start_line,
				end_line: region.end_line,
				distance: None, // No relevance score when indexing
			};

			// Add to batch for embedding
			code_blocks_batch.push(code_block.clone());

			// Add to all code blocks for GraphRAG
			if ctx.config.graphrag.enabled {
				all_code_blocks.push(code_block);
				graphrag_blocks_added += 1;
			}
		} else if ctx.config.graphrag.enabled {
			// If skipping because block exists, but we need for GraphRAG, fetch from store
			if let Ok(existing_block) = ctx.store.get_code_block_by_hash(&content_hash).await {
				// Add the existing block to the GraphRAG collection
				all_code_blocks.push(existing_block);
				graphrag_blocks_added += 1;
			}
		}
	}

	// Remove blocks that no longer exist (only if not force reindexing)
	if !force_reindex && !existing_hashes.is_empty() {
		let hashes_to_remove: Vec<String> = existing_hashes
			.into_iter()
			.filter(|hash| !new_hashes.contains(hash))
			.collect();

		if !hashes_to_remove.is_empty() {
			ctx.store
				.remove_blocks_by_hashes(&hashes_to_remove, "code_blocks")
				.await?;
		}
	}

	// Update GraphRAG state if enabled and blocks were added
	if ctx.config.graphrag.enabled && graphrag_blocks_added > 0 {
		let mut state_guard = ctx.state.write();
		state_guard.graphrag_blocks += graphrag_blocks_added;
	}

	Ok(())
}

// Differential processing for text files - only updates changed blocks
// FIXED: Now stores simple path without chunk numbers
async fn process_text_file_differential(
	store: &Store,
	contents: &str,
	file_path: &str,
	text_blocks_batch: &mut Vec<TextBlock>,
	config: &Config,
	state: SharedState,
) -> Result<()> {
	let force_reindex = state.read().force_reindex;

	// Get existing text block hashes for this file (including chunked versions)
	let existing_hashes = if force_reindex {
		Vec::new()
	} else {
		// Get blocks for this file path (the chunks will have same path now)
		store
			.get_file_blocks_metadata(file_path, "text_blocks")
			.await?
	};

	// Split content into chunks using configuration values
	let chunks = chunk_text(
		contents,
		config.index.chunk_size,
		config.index.chunk_overlap,
	);
	let mut new_hashes = std::collections::HashSet::new();

	for (chunk_idx, chunk_with_lines) in chunks.iter().enumerate() {
		// Use chunk index in hash for uniqueness but keep path clean
		let chunk_hash = calculate_unique_content_hash(
			&chunk_with_lines.content,
			&format!("{}#{}", file_path, chunk_idx),
		);
		new_hashes.insert(chunk_hash.clone());

		// Skip the check if force_reindex is true
		let exists = !force_reindex && store.content_exists(&chunk_hash, "text_blocks").await?;
		if !exists {
			text_blocks_batch.push(TextBlock {
				path: file_path.to_string(),
				language: "text".to_string(),
				content: chunk_with_lines.content.clone(),
				start_line: chunk_with_lines.start_line, // Use directly from TextChunkWithLines
				end_line: chunk_with_lines.end_line,     // Use directly from TextChunkWithLines
				hash: chunk_hash,
				distance: None,
			});
		}
	}

	// Remove blocks that no longer exist (only if not force reindexing)
	if !force_reindex && !existing_hashes.is_empty() {
		let hashes_to_remove: Vec<String> = existing_hashes
			.into_iter()
			.filter(|hash| !new_hashes.contains(hash))
			.collect();

		if !hashes_to_remove.is_empty() {
			store
				.remove_blocks_by_hashes(&hashes_to_remove, "text_blocks")
				.await?;
		}
	}

	Ok(())
}

// Differential processing for markdown files - only updates changed blocks
async fn process_markdown_file_differential(
	store: &Store,
	contents: &str,
	file_path: &str,
	document_blocks_batch: &mut Vec<DocumentBlock>,
	config: &Config,
	state: SharedState,
) -> Result<()> {
	// Get force_reindex flag from state
	let force_reindex = state.read().force_reindex;

	// Get existing document block hashes for this file
	let existing_hashes = if force_reindex {
		Vec::new()
	} else {
		store
			.get_file_blocks_metadata(file_path, "document_blocks")
			.await?
	};

	// Parse markdown content into document blocks using context-aware chunking
	let document_blocks = parse_markdown_content(contents, file_path, config);
	let mut new_hashes = std::collections::HashSet::new();

	for doc_block in document_blocks {
		new_hashes.insert(doc_block.hash.clone());

		// Check if this document block already exists (unless force reindex)
		let exists = !force_reindex
			&& store
				.content_exists(&doc_block.hash, "document_blocks")
				.await?;
		if !exists {
			document_blocks_batch.push(doc_block);
		}
	}

	// Remove blocks that no longer exist (only if not force reindexing)
	if !force_reindex && !existing_hashes.is_empty() {
		let hashes_to_remove: Vec<String> = existing_hashes
			.into_iter()
			.filter(|hash| !new_hashes.contains(hash))
			.collect();

		if !hashes_to_remove.is_empty() {
			store
				.remove_blocks_by_hashes(&hashes_to_remove, "document_blocks")
				.await?;
		}
	}

	Ok(())
}

#[cfg(test)]
mod context_optimization_tests {
	use super::*;

	#[test]
	fn test_context_optimization() {
		// Create a DocumentBlock with context
		let doc_block = DocumentBlock {
			path: "test.md".to_string(),
			title: "Test Section".to_string(),
			content: "This is the actual content.".to_string(),
			context: vec![
				"# Main Document".to_string(),
				"## Authentication".to_string(),
				"### JWT Implementation".to_string(),
			],
			level: 3,
			start_line: 10,
			end_line: 15,
			hash: "test_hash".to_string(),
			distance: None,
		};

		// Test context merging for embedding
		let embedding_text = if !doc_block.context.is_empty() {
			format!("{}\n\n{}", doc_block.context.join("\n"), doc_block.content)
		} else {
			doc_block.content.clone()
		};

		// Verify the embedding text contains context
		assert!(embedding_text.contains("# Main Document"));
		assert!(embedding_text.contains("## Authentication"));
		assert!(embedding_text.contains("### JWT Implementation"));
		assert!(embedding_text.contains("This is the actual content."));

		// Verify memory efficiency
		let storage_size = doc_block.content.len();
		let context_size: usize = doc_block.context.iter().map(|s| s.len()).sum();
		let total_size = storage_size + context_size;
		let old_approach_size = embedding_text.len() + doc_block.content.len();

		// New approach should be more efficient
		assert!(total_size < old_approach_size);

		println!("New approach size: {} bytes", total_size);
		println!("Old approach size: {} bytes", old_approach_size);
		println!(
			"Memory savings: {}%",
			((old_approach_size - total_size) as f64 / old_approach_size as f64 * 100.0) as i32
		);
	}

	#[test]
	fn test_empty_context() {
		let doc_block = DocumentBlock {
			path: "test.md".to_string(),
			title: "Test Section".to_string(),
			content: "Content without context.".to_string(),
			context: Vec::new(), // Empty context
			level: 1,
			start_line: 0,
			end_line: 5,
			hash: "test_hash".to_string(),
			distance: None,
		};

		// Test context merging with empty context
		let embedding_text = if !doc_block.context.is_empty() {
			format!("{}\n\n{}", doc_block.context.join("\n"), doc_block.content)
		} else {
			doc_block.content.clone()
		};

		// Should just be the content
		assert_eq!(embedding_text, doc_block.content);
	}

	#[test]
	fn test_smart_chunking_eliminates_tiny_chunks() {
		// Test markdown content that would create tiny chunks
		let test_content = r#"# Main Document

## Section A
Some content here.

### Tiny Subsection
Only 33 symbols here - very small!

### Another Tiny
Also small content.

## Section B
This has more substantial content that should be fine on its own.
It has multiple lines and provides good context for understanding.

### Small Child
Brief content.
"#;

		let hierarchy = parse_document_hierarchy(test_content);
		let chunks = hierarchy.bottom_up_chunking(2000); // 2000 char target

		// Verify no chunks are extremely tiny (less than 100 chars as reasonable minimum)
		let tiny_chunks: Vec<&ChunkResult> = chunks
			.iter()
			.filter(|chunk| chunk.storage_content.len() < 100)
			.collect();

		println!("Generated {} chunks total", chunks.len());
		for (i, chunk) in chunks.iter().enumerate() {
			println!(
				"Chunk {}: {} chars - '{}'",
				i + 1,
				chunk.storage_content.len(),
				chunk.title
			);
		}

		if !tiny_chunks.is_empty() {
			println!("Found {} tiny chunks:", tiny_chunks.len());
			for chunk in &tiny_chunks {
				println!("- '{}': {} chars", chunk.title, chunk.storage_content.len());
			}
		}

		// The smart chunking should eliminate most tiny chunks through merging
		assert!(
			tiny_chunks.len() <= 1,
			"Should have at most 1 tiny chunk after smart merging"
		);
	}
}
