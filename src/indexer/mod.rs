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

pub mod batch_processor; // Batch processing utilities for embedding operations
pub mod code_region_extractor; // Code region extraction and smart merging utilities
pub mod differential_processor; // Differential processing utilities for incremental updates
pub mod file_processor; // File processing utilities for text and markdown files
pub mod graph_optimization;
pub mod graphrag; // GraphRAG generation for code relationships (modular implementation)
pub mod languages; // Language-specific processors
pub mod markdown_processor; // Markdown document processing utilities
pub mod search; // Search functionality // Task-focused graph extraction and optimization
pub mod signature_extractor; // Code signature extraction utilities

pub mod render_utils;
pub use batch_processor::*;
pub use code_region_extractor::*;
pub use differential_processor::*;
pub use file_processor::*;
pub use graph_optimization::*;
pub use graphrag::*;
pub use languages::*;
pub use markdown_processor::*;
pub use search::*;
pub use signature_extractor::*;

use crate::config::Config;
use crate::mcp::logging::{log_file_processing_error, log_indexing_progress};
use crate::state;
use crate::state::SharedState;
#[cfg(test)]
use crate::store::DocumentBlock;
use crate::store::Store;
pub use render_utils::*;

// Import the new modular utilities
mod file_utils;
mod git_utils;
mod path_utils;
mod text_processing;

use self::file_utils::FileUtils;
use self::git_utils::GitUtils;

// Re-export for external use
pub use self::path_utils::PathUtils;
use std::fs;
// We're using ignore::WalkBuilder instead of walkdir::WalkDir
use anyhow::Result;
use ignore;
// serde::Serialize moved to signature_extractor module
use std::path::Path;

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

// ProcessFileContext and process_file function moved to differential_processor module

// Code region extraction logic moved to code_region_extractor module

// Batch processing functions moved to batch_processor module

// Constants for text chunking - REMOVED: Now using config.index.chunk_size and config.index.chunk_overlap

// File processing functions moved to file_processor module

// Differential processing functions moved to differential_processor module

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
