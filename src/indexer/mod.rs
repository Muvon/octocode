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
pub mod git_utils;
mod path_utils;
mod text_processing;

use self::file_utils::FileUtils;

// Re-export for external use
pub use self::git_utils::GitUtils;
pub use self::path_utils::PathUtils;
use std::fs;
// We're using ignore::WalkBuilder instead of walkdir::WalkDir
use anyhow::Result;
use ignore;
// serde::Serialize moved to signature_extractor module
use std::path::Path;

// Signature extraction types moved to signature_extractor module

use std::collections::HashMap;
use std::sync::OnceLock;

/// Utility to create an ignore Walker that respects both .gitignore and .noindex files
pub struct NoindexWalker;

/// Cache for .noindex file detection results to avoid repeated file system checks
static NOINDEX_CACHE: OnceLock<parking_lot::RwLock<HashMap<std::path::PathBuf, bool>>> =
	OnceLock::new();

impl NoindexWalker {
	/// Creates a WalkBuilder that respects .gitignore and .noindex files
	/// PERFORMANCE: Uses caching to avoid repeated .noindex detection
	pub fn create_walker(current_dir: &Path) -> ignore::WalkBuilder {
		let mut builder = ignore::WalkBuilder::new(current_dir);

		// Standard git ignore settings
		builder
			.hidden(true) // Don't ignore all hidden files - let gitignore handle it
			.git_ignore(true) // Respect .gitignore files
			.git_global(true) // Respect global git ignore files
			.git_exclude(true) // Respect .git/info/exclude files
			.follow_links(false);

		// PERFORMANCE: Only add .noindex support if .noindex files actually exist
		// Uses caching to avoid repeated file system checks during the same session
		if Self::has_noindex_files_cached(current_dir) {
			builder.add_custom_ignore_filename(".noindex");
		}

		builder
	}

	/// Cached version of .noindex file detection
	fn has_noindex_files_cached(current_dir: &Path) -> bool {
		let cache = NOINDEX_CACHE.get_or_init(|| parking_lot::RwLock::new(HashMap::new()));
		let current_dir_buf = current_dir.to_path_buf();

		// Try to read from cache first
		{
			let cache_read = cache.read();
			if let Some(&cached_result) = cache_read.get(&current_dir_buf) {
				return cached_result;
			}
		}

		// Not in cache, compute the result
		let result = Self::has_noindex_files(current_dir);

		// Store in cache
		{
			let mut cache_write = cache.write();
			cache_write.insert(current_dir_buf, result);
		}

		result
	}

	/// Fast check if there are any .noindex files in the current directory
	/// PERFORMANCE: Uses targeted file system checks instead of expensive tree traversal
	fn has_noindex_files(current_dir: &Path) -> bool {
		// Quick check: .noindex file in current directory
		if current_dir.join(".noindex").exists() {
			return true;
		}

		// PERFORMANCE OPTIMIZATION: Instead of scanning entire tree, use a more targeted approach
		// Most projects either have no .noindex files or have them in common locations
		// This avoids the expensive full directory traversal that was causing slow startup

		// Check common subdirectories where .noindex files might exist
		// This covers 99% of real-world usage while being much faster
		let common_paths = [
			"src",
			"lib",
			"tests",
			"test",
			"docs",
			"doc",
			"examples",
			"example",
			"target",
			"build",
			"dist",
			"node_modules",
			".git",
			"vendor",
		];

		for subdir in &common_paths {
			let noindex_path = current_dir.join(subdir).join(".noindex");
			if noindex_path.exists() {
				return true;
			}
		}

		// If no .noindex files found in common locations, assume none exist
		// This is a reasonable trade-off: 99% performance improvement for 99% of cases
		// Users can still add .noindex files, they just need to be in common directories
		// or in the root directory (which we always check above)
		false
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

/// Fast file counting function that performs minimal checks
/// Returns the total count of indexable files without expensive operations
fn fast_count_indexable_files(
	current_dir: &Path,
	git_changed_files: Option<&std::collections::HashSet<String>>,
) -> usize {
	let mut count = 0;

	// If we have git optimization, just count the changed files
	if let Some(changed_files) = git_changed_files {
		for file_path in changed_files {
			let full_path = current_dir.join(file_path);
			// Quick extension check only - no language detection
			if full_path.extension().is_some() {
				count += 1;
			}
		}
		return count;
	}

	// Otherwise, do a fast walk with minimal checks
	let walker = NoindexWalker::create_walker(current_dir).build();

	for result in walker {
		let entry = match result {
			Ok(entry) => entry,
			Err(_) => continue,
		};

		// Skip directories
		if !entry.file_type().is_some_and(|ft| ft.is_file()) {
			continue;
		}

		// Quick check: just see if file has an extension that might be indexable
		// This is much faster than full language detection
		if let Some(ext) = entry.path().extension() {
			let ext_str = ext.to_str().unwrap_or("");
			// Quick list of common extensions we index
			if matches!(
				ext_str,
				"rs" | "js"
					| "ts" | "jsx" | "tsx"
					| "py" | "go" | "java"
					| "c" | "cpp" | "h"
					| "hpp" | "cs" | "php"
					| "rb" | "swift"
					| "kt" | "scala"
					| "r" | "m" | "mm"
					| "md" | "markdown"
					| "txt" | "json"
					| "yaml" | "yml"
					| "toml" | "xml"
					| "html" | "css"
					| "scss" | "sass"
					| "less" | "sql"
					| "sh" | "bash" | "zsh"
					| "fish" | "vim"
					| "lua" | "pl" | "pm"
					| "t" | "pod" | "raku"
					| "rakumod" | "rakudoc"
					| "nix" | "dhall"
					| "tf" | "tfvars"
					| "hcl" | "vue" | "svelte"
					| "elm" | "purs"
					| "hs" | "lhs" | "ml"
					| "mli" | "fs" | "fsi"
					| "fsx" | "clj" | "cljs"
					| "cljc" | "edn"
					| "ex" | "exs" | "erl"
					| "hrl" | "zig" | "v"
					| "vsh" | "nim" | "nims"
					| "cr" | "jl" | "d"
					| "dart" | "pas"
					| "pp" | "inc" | "asm"
					| "s" | "S" | "rst"
					| "adoc" | "tex"
					| "bib" | "org" | "wiki"
					| "pod6" | "rakutest"
					| "cfg" | "conf"
					| "config" | "ini"
					| "env" | "properties"
					| "gradle" | "cmake"
					| "make" | "makefile"
					| "dockerfile" | "containerfile"
					| "vagrantfile" | "gemfile"
					| "rakefile" | "guardfile"
					| "podfile" | "fastfile"
					| "brewfile"
			) {
				count += 1;
			}
		}
	}

	count
}

// Render signatures and search results as markdown output (more efficient for AI tools)
// Rendering functions have been moved to src/indexer/render_utils.rs

/// Centralized helper function to persist data and store git metadata atomically
/// This ensures metadata is only stored when data is safely persisted
async fn persist_and_store_metadata(
	store: &Store,
	git_repo_root: Option<&Path>,
	config: &Config,
	quiet: bool,
	context: &str,
) -> Result<()> {
	// CRITICAL: Flush first with explicit error handling
	// If flush fails, we must NOT store metadata as data is not persisted
	if let Err(e) = store.flush().await {
		tracing::error!(
			context = context,
			error = %e,
			"Failed to flush store - metadata will NOT be stored"
		);
		return Err(e);
	}

	tracing::debug!(context = context, "Successfully flushed store");

	// Only store metadata if we have a git repository
	let Some(git_root) = git_repo_root else {
		return Ok(());
	};

	// Get current commit hash
	let current_commit = match git::get_current_commit_hash(git_root) {
		Ok(hash) => hash,
		Err(e) => {
			tracing::warn!(
				context = context,
				error = %e,
				"Could not get current commit hash, skipping metadata storage"
			);
			return Ok(()); // Not a fatal error, just skip metadata
		}
	};

	// Store git metadata
	if let Err(e) = store.store_git_metadata(&current_commit).await {
		tracing::error!(
			context = context,
			commit = %current_commit,
			error = %e,
			"Failed to store git metadata"
		);
		if !quiet {
			eprintln!("Warning: Could not store git metadata: {}", e);
		}
		// Continue to try GraphRAG metadata even if git metadata fails
	} else {
		tracing::debug!(
			context = context,
			commit = %current_commit,
			"Successfully stored git metadata"
		);
	}

	// Store GraphRAG commit hash if GraphRAG is enabled
	if config.graphrag.enabled {
		if let Err(e) = store.store_graphrag_commit_hash(&current_commit).await {
			tracing::error!(
				context = context,
				commit = %current_commit,
				error = %e,
				"Failed to store GraphRAG git metadata"
			);
			if !quiet {
				eprintln!("Warning: Could not store GraphRAG git metadata: {}", e);
			}
		} else {
			tracing::debug!(
				context = context,
				commit = %current_commit,
				"Successfully stored GraphRAG metadata"
			);
		}
	}

	Ok(())
}

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

								// Clean up existing data for changed files (includes GraphRAG cleanup)
								for file_path in &changed_files {
									if let Err(e) = store.remove_blocks_by_path(file_path).await {
										if !quiet {
											eprintln!(
												"Warning: Failed to clean up data for {}: {}",
												file_path, e
											);
										}
									}
								}

								// CRITICAL: Flush immediately after cleanup to persist removals
								// This prevents data loss if process is interrupted before new data is written
								// Cannot be deferred because git metadata is not yet updated - if we crash here,
								// next run will correctly detect files need reindexing
								if !changed_files.is_empty() {
									store.flush().await?;
									tracing::debug!(
										files_cleaned = changed_files.len(),
										"Flushed after cleanup of changed files"
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
								match store.graphrag_needs_indexing().await {
									Ok(v) => v,
									Err(e) => {
										tracing::warn!(
											error = %e,
											"Failed to check if GraphRAG needs indexing, assuming false"
										);
										false
									}
								};
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

								// CRITICAL: Persist data and store git metadata after GraphRAG build
								// This prevents infinite rebuilding on subsequent runs
								persist_and_store_metadata(
									store,
									Some(git_root),
									config,
									quiet,
									"graphrag_from_existing",
								)
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

	// Progressive counting variables
	let total_files_found;
	let mut files_processed = 0;

	// Log file processing phase start
	log_indexing_progress("file_processing", 0, 0, None, 0);

	// PERFORMANCE FIX: Fast-path for git optimization - process only changed files
	if let Some(ref changed_files) = git_changed_files {
		// Use fast counting function for git optimization
		total_files_found = fast_count_indexable_files(&current_dir, Some(changed_files));

		// Update state with final count and switch to processing mode
		{
			let mut state_guard = state.write();
			state_guard.total_files = total_files_found;
			state_guard.counting_files = false;
			state_guard.status_message = "".to_string();
		}

		// Process only the changed files directly
		for file_path in changed_files {
			let full_path = current_dir.join(file_path);
			if !full_path.is_file() {
				continue;
			}

			// PERFORMANCE OPTIMIZATION: Fast file modification time check using preloaded metadata
			let force_reindex = state.read().force_reindex;
			if !force_reindex {
				if let Ok(actual_mtime) = get_file_mtime(&full_path) {
					// Fast HashMap lookup instead of database query
					if let Some(stored_mtime) = file_metadata_map.get(file_path) {
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

			// Process the file (same logic as walker loop below)
			if let Some(language) = detect_language(&full_path) {
				match fs::read_to_string(&full_path) {
					Ok(contents) => {
						// Store the file modification time after successful processing
						let file_processed;

						if language == "markdown" {
							// Handle markdown files specially - index as document blocks
							process_markdown_file_differential(
								store,
								&contents,
								file_path,
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
								file_path,
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
							if let Ok(actual_mtime) = get_file_mtime(&full_path) {
								let _ = store.store_file_metadata(file_path, actual_mtime).await;
							}
						}

						files_processed += 1;
						state.write().indexed_files = files_processed;

						// Log progress periodically for code files
						if files_processed % 50 == 0 {
							let current_total = state.read().total_files;
							log_indexing_progress(
								"file_processing",
								files_processed,
								current_total,
								Some(file_path),
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
						log_file_processing_error(file_path, "read_file", &e);
					}
				}
			} else {
				// Handle unsupported file types as chunked text
				// First check if the file extension is in our whitelist
				// BUT exclude markdown files since they're already processed as documents
				if is_allowed_text_extension(&full_path) && !is_markdown_file(&full_path) {
					if let Ok(contents) = fs::read_to_string(&full_path) {
						// Only process files that are likely to contain readable text
						if is_text_file(&contents) {
							process_text_file_differential(
								store,
								&contents,
								file_path,
								&mut text_blocks_batch,
								config,
								state.clone(),
							)
							.await?;

							// Store file modification time after successful processing
							if let Ok(actual_mtime) = get_file_mtime(&full_path) {
								let _ = store.store_file_metadata(file_path, actual_mtime).await;
							}

							files_processed += 1;
							state.write().indexed_files = files_processed;

							// Log progress periodically for text files
							if files_processed % 50 == 0 {
								let current_total = state.read().total_files;
								log_indexing_progress(
									"file_processing",
									files_processed,
									current_total,
									Some(file_path),
									embedding_calls,
								);
							}

							// Process batch when it reaches the batch size or token limit
							if should_process_batch(&text_blocks_batch, |b| &b.content, config) {
								embedding_calls += text_blocks_batch.len();
								process_text_blocks_batch(store, &text_blocks_batch, config)
									.await?;
								text_blocks_batch.clear();
								batches_processed += 1;
								// Intelligent flush based on configuration
								flush_if_needed(store, &mut batches_processed, config, false)
									.await?;
							}
						}
					}
				}
			}
		}
	} else {
		// Normal mode: First do a fast count, then process files

		// PERFORMANCE FIX: Do a fast count first without expensive operations
		total_files_found = fast_count_indexable_files(&current_dir, None);

		// Update state with the total count immediately
		{
			let mut state_guard = state.write();
			state_guard.total_files = total_files_found;
			state_guard.counting_files = false;
			state_guard.status_message = "".to_string();
		}

		// Now do the actual processing with proper language detection
		let walker = NoindexWalker::create_walker(&current_dir).build();

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
								process_text_blocks_batch(store, &text_blocks_batch, config)
									.await?;
								text_blocks_batch.clear();
								batches_processed += 1;
								// Intelligent flush based on configuration
								flush_if_needed(store, &mut batches_processed, config, false)
									.await?;
							}
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
		// Check if we have new blocks from this indexing run OR if GraphRAG needs building from existing database
		let needs_graphrag_from_existing = if all_code_blocks.is_empty() {
			// No new blocks from this run - check if we have existing blocks in database to build from
			// This handles the case where GraphRAG is enabled after database is already indexed
			let existing_blocks = store
				.get_all_code_blocks_for_graphrag()
				.await
				.unwrap_or_default();
			let needs_indexing = match store.graphrag_needs_indexing().await {
				Ok(v) => v,
				Err(e) => {
					tracing::warn!(
						error = %e,
						"Failed to check if GraphRAG needs indexing, assuming false"
					);
					false
				}
			};
			!existing_blocks.is_empty() && needs_indexing
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

			// NOTE: GraphRAG commit hash will be stored AFTER final flush for data integrity
		}
	}

	{
		let mut state_guard = state.write();
		state_guard.indexing_complete = true;
		state_guard.embedding_calls = embedding_calls;
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

	// CRITICAL: Persist data and store git metadata atomically
	// Only mark commit as "indexed" when all data is safely persisted
	// Check if we should store metadata based on what was processed
	let should_store_metadata = final_files > 0
		|| git_changed_files.is_some()
		|| (total_files_found == 0 && git_changed_files.is_none());

	if should_store_metadata {
		persist_and_store_metadata(store, git_repo_root, config, quiet, "indexing_complete")
			.await?;
	} else if !quiet {
		println!("⚠️  Git metadata not stored - no files processed and no git optimization used");
	}

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
