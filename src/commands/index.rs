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
use parking_lot::RwLock;
use std::io::Write;
use std::sync::Arc;

use octocode::config::Config;
use octocode::indexer;
use octocode::lock::IndexLock;
use octocode::state;
use octocode::store::Store;

#[derive(Args, Debug)]
pub struct IndexArgs {
	/// Skip git repository requirement and git-based optimizations
	#[arg(long)]
	pub no_git: bool,

	/// List all files currently indexed in the database
	#[arg(long)]
	pub list_files: bool,

	/// Show all chunks for a specific file with metadata
	#[arg(long, value_name = "FILE_PATH")]
	pub show_file: Option<String>,

	/// Show GraphRAG connections for a specific file
	#[arg(long, value_name = "FILE_PATH")]
	pub graphrag: Option<String>,
}

pub async fn execute(
	store: &Store,
	config: &Config,
	args: &IndexArgs,
) -> Result<(), anyhow::Error> {
	// Handle list-files option first
	if args.list_files {
		println!("Listing all files currently indexed in the database...");
		store.list_indexed_files().await?;
		return Ok(());
	}

	// Handle show-file option
	if let Some(file_path) = &args.show_file {
		println!("Showing all chunks for file: {}", file_path);
		store.show_file_chunks(file_path).await?;
		return Ok(());
	}

	// Handle graphrag debug option
	if let Some(file_path) = &args.graphrag {
		println!("Showing GraphRAG connections for file: {}", file_path);
		show_graphrag_connections(store, file_path).await?;
		return Ok(());
	}

	let current_dir = std::env::current_dir()?;

	// Git repository validation and optimization
	let git_repo_root = if !args.no_git && config.index.require_git {
		// Check if we're in a git repository root
		if !indexer::git::is_git_repo_root(&current_dir) {
			return Err(anyhow::anyhow!(
				"❌ Error: Not in a git repository root!\n\n\
				This tool requires running from the root of a git repository.\n\
				Please:\n\
				1. Navigate to your git repository root (where .git/ folder exists)\n\
				2. Or use --no-git flag to skip git requirement\n\
				3. Or set index.require_git = false in your config"
			));
		}
		Some(current_dir.clone())
	} else if !args.no_git {
		// Try to find git root (for optimization even if not required)
		indexer::git::find_git_root(&current_dir)
	} else {
		None
	};

	if let Some(ref git_root) = git_repo_root {
		println!("✓ Git repository detected: {}", git_root.display());
	} else if args.no_git {
		println!("⚠️  Git integration disabled (--no-git flag)");
	} else {
		println!("⚠️  No git repository found, using file-based indexing");
	}

	let state = state::create_shared_state();
	state.write().current_directory = current_dir.clone();

	// Acquire indexing lock before starting
	let mut lock = IndexLock::new(&current_dir)?;
	lock.acquire()?;
	tracing::info!("Acquired indexing lock for {:?}", current_dir);

	// Spawn the progress display task
	let progress_handle = tokio::spawn(display_indexing_progress(state.clone()));

	// Start indexing with git optimization
	indexer::index_files(store, state.clone(), config, git_repo_root.as_deref()).await?;

	// Wait for the progress display to finish
	let _ = progress_handle.await;

	// Flush index to disk
	store.flush().await?;

	// Release the lock (also happens automatically on drop)
	lock.release()?;
	tracing::info!("Released indexing lock");

	Ok(())
}

pub async fn display_indexing_progress(state: Arc<RwLock<state::IndexState>>) {
	let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
	let mut spinner_idx = 0;
	let mut last_indexed = 0;
	let mut last_skipped = 0;
	let mut last_graphrag_blocks = 0;
	let mut last_status_message = String::new();
	let mut indexing_complete = false;

	while !indexing_complete {
		// Gather all necessary state in local variables before the await
		let current_indexed;
		let current_skipped;
		let total_files;
		let graphrag_blocks;
		let status_message;
		let graphrag_enabled;
		let counting_files;

		{
			let current_state = state.read();
			current_indexed = current_state.indexed_files;
			current_skipped = current_state.skipped_files;
			total_files = current_state.total_files;
			graphrag_blocks = current_state.graphrag_blocks;
			status_message = current_state.status_message.clone();
			graphrag_enabled = current_state.graphrag_enabled;
			counting_files = current_state.counting_files;
			indexing_complete = current_state.indexing_complete; // Update our loop control variable
			                                            // Lock is dropped here when we exit the scope
		}

		// Exit early if indexing is complete
		if indexing_complete {
			break;
		}

		// Only redraw if something changed or on spinner change
		if current_indexed != last_indexed
			|| current_skipped != last_skipped
			|| graphrag_blocks != last_graphrag_blocks
			|| status_message != last_status_message
		{
			// Clear the line and move cursor to beginning with \r
			print!("\r\x1b[K"); // \x1b[K clears the rest of the line

			// Build display string based on current phase
			if counting_files {
				print!("{} Counting files...", spinner_chars[spinner_idx]);
			} else if total_files > 0 {
				let processed_total = current_indexed + current_skipped;
				let percentage = (processed_total as f32 / total_files as f32 * 100.0) as u32;
				print!(
					"{} Indexing: {}/{} files ({}%)",
					spinner_chars[spinner_idx], processed_total, total_files, percentage
				);

				// Show breakdown if we have skipped files
				if current_skipped > 0 {
					print!(" [{} new, {} unchanged]", current_indexed, current_skipped);
				}

				// Add GraphRAG info if enabled and blocks exist
				if graphrag_enabled && graphrag_blocks > 0 {
					print!(", GraphRAG: {} blocks", graphrag_blocks);
				}
			} else {
				// Fallback for when total is not known yet
				print!(
					"{} Indexing: {} files",
					spinner_chars[spinner_idx], current_indexed
				);
			}

			// Add status message if there is one
			if !status_message.is_empty() {
				print!(" - {}", status_message);
			}

			std::io::stdout().flush().unwrap();
			last_indexed = current_indexed;
			last_skipped = current_skipped;
			last_graphrag_blocks = graphrag_blocks;
			last_status_message = status_message.clone();
		} else {
			// Just update the spinner
			print!("\r\x1b[K"); // Clear the line
			if counting_files {
				print!("{} Counting files...", spinner_chars[spinner_idx]);
			} else if total_files > 0 {
				let processed_total = current_indexed + current_skipped;
				let percentage = (processed_total as f32 / total_files as f32 * 100.0) as u32;
				print!(
					"{} Indexing: {}/{} files ({}%)",
					spinner_chars[spinner_idx], processed_total, total_files, percentage
				);

				// Show breakdown if we have skipped files
				if current_skipped > 0 {
					print!(" [{} new, {} unchanged]", current_indexed, current_skipped);
				}

				// Add GraphRAG info if enabled and blocks exist
				if graphrag_enabled && graphrag_blocks > 0 {
					print!(", GraphRAG: {} blocks", graphrag_blocks);
				}
			} else {
				print!(
					"{} Indexing: {} files",
					spinner_chars[spinner_idx], current_indexed
				);
			}

			// Add status message if there is one
			if !status_message.is_empty() {
				print!(" - {}", status_message);
			}
			std::io::stdout().flush().unwrap();
		}

		spinner_idx = (spinner_idx + 1) % spinner_chars.len();
		tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
	}

	// Final summary message
	let final_indexed;
	let final_skipped;
	let final_total;
	let final_graphrag_enabled;
	let final_graphrag_blocks;

	{
		let final_state = state.read();
		final_indexed = final_state.indexed_files;
		final_skipped = final_state.skipped_files;
		final_total = final_state.total_files;
		final_graphrag_enabled = final_state.graphrag_enabled;
		final_graphrag_blocks = final_state.graphrag_blocks;
	}

	print!("\r\x1b[K"); // Clear the line before final message
	if !final_graphrag_enabled {
		if final_skipped > 0 {
			println!(
				"✓ Indexing complete! {} of {} files processed ({} new, {} unchanged)",
				final_indexed + final_skipped,
				final_total,
				final_indexed,
				final_skipped
			);
		} else {
			println!(
				"✓ Indexing complete! {} of {} files processed",
				final_indexed, final_total
			);
		}
	} else if final_skipped > 0 {
		println!(
			"✓ Indexing complete! {} of {} files processed ({} new, {} unchanged), GraphRAG: {} blocks",
			final_indexed + final_skipped, final_total, final_indexed, final_skipped, final_graphrag_blocks
		);
	} else {
		println!(
			"✓ Indexing complete! {} of {} files processed, GraphRAG: {} blocks",
			final_indexed, final_total, final_graphrag_blocks
		);
	}
}

async fn show_graphrag_connections(store: &Store, file_path: &str) -> Result<(), anyhow::Error> {
	use arrow::array::StringArray;

	// Search for nodes related to this file using vector search
	// Use dummy embedding to get all nodes (we'll filter by file path)
	let query_embedding = vec![0.0; store.get_code_vector_dim()];

	// Search for nodes that might be related to this file
	let nodes_batch = store.search_graph_nodes(&query_embedding, 100).await?;

	if nodes_batch.num_rows() == 0 {
		println!("No GraphRAG nodes found in database");
		return Ok(());
	}

	// Filter nodes by file path
	let file_paths = nodes_batch
		.column_by_name("path") // Use "path" not "file_path" - matches actual schema
		.ok_or_else(|| anyhow::anyhow!("No path column in nodes"))?
		.as_any()
		.downcast_ref::<StringArray>()
		.ok_or_else(|| anyhow::anyhow!("path column is not a StringArray"))?;

	let node_ids = nodes_batch
		.column_by_name("id")
		.ok_or_else(|| anyhow::anyhow!("No id column in nodes"))?
		.as_any()
		.downcast_ref::<StringArray>()
		.ok_or_else(|| anyhow::anyhow!("id column is not a StringArray"))?;

	let node_types = nodes_batch
		.column_by_name("kind") // Use "kind" not "node_type" - matches actual schema
		.ok_or_else(|| anyhow::anyhow!("No kind column in nodes"))?
		.as_any()
		.downcast_ref::<StringArray>()
		.ok_or_else(|| anyhow::anyhow!("kind column is not a StringArray"))?;

	let descriptions = nodes_batch
		.column_by_name("description")
		.ok_or_else(|| anyhow::anyhow!("No description column in nodes"))?
		.as_any()
		.downcast_ref::<StringArray>()
		.ok_or_else(|| anyhow::anyhow!("description column is not a StringArray"))?;

	// Find nodes for this file
	let mut file_nodes = Vec::new();
	for i in 0..nodes_batch.num_rows() {
		let stored_path = file_paths.value(i);

		// Try multiple matching strategies:
		// 1. Exact match
		// 2. Match after stripping "./" prefix from stored path
		// 3. Match if user path starts with stored path
		// 4. Match if stored path ends with user path
		let matches = stored_path == file_path
			|| (stored_path.strip_prefix("./") == Some(file_path))
			|| stored_path.ends_with(file_path)
			|| file_path.ends_with(stored_path);

		if matches {
			file_nodes.push((
				node_ids.value(i),
				node_types.value(i),
				descriptions.value(i),
			));
		}
	}

	if file_nodes.is_empty() {
		println!("No GraphRAG nodes found for file: {}", file_path);
		return Ok(());
	}

	println!("GraphRAG connections for file: {}", file_path);
	println!("{}", "=".repeat(60));

	// Show nodes in this file
	println!("\n📁 Nodes in this file:");
	for (node_id, node_type, description) in &file_nodes {
		println!("  • {} ({})", node_id, node_type);
		if !description.is_empty() {
			println!("    Description: {}", description);
		}
	}

	// Get all relationships
	let relationships_batch = store.get_graph_relationships().await?;

	if relationships_batch.num_rows() == 0 {
		println!("\n🔗 No relationships found in database");
		return Ok(());
	}

	let rel_sources = relationships_batch
		.column_by_name("source")
		.ok_or_else(|| anyhow::anyhow!("No source column in relationships"))?
		.as_any()
		.downcast_ref::<StringArray>()
		.ok_or_else(|| anyhow::anyhow!("source column is not a StringArray"))?;

	let rel_targets = relationships_batch
		.column_by_name("target")
		.ok_or_else(|| anyhow::anyhow!("No target column in relationships"))?
		.as_any()
		.downcast_ref::<StringArray>()
		.ok_or_else(|| anyhow::anyhow!("target column is not a StringArray"))?;

	let rel_types = relationships_batch
		.column_by_name("relation_type")
		.ok_or_else(|| anyhow::anyhow!("No relation_type column in relationships"))?
		.as_any()
		.downcast_ref::<StringArray>()
		.ok_or_else(|| anyhow::anyhow!("relation_type column is not a StringArray"))?;

	let rel_descriptions = relationships_batch
		.column_by_name("description")
		.ok_or_else(|| anyhow::anyhow!("No description column in relationships"))?
		.as_any()
		.downcast_ref::<StringArray>()
		.ok_or_else(|| anyhow::anyhow!("description column is not a StringArray"))?;

	// Show outgoing relationships
	println!("\n🔗 Outgoing relationships:");
	let mut found_outgoing = false;
	for (node_id, _, _) in &file_nodes {
		for i in 0..relationships_batch.num_rows() {
			if rel_sources.value(i) == *node_id {
				found_outgoing = true;
				println!(
					"  {} → {} ({})",
					rel_sources.value(i),
					rel_targets.value(i),
					rel_types.value(i)
				);
				let desc = rel_descriptions.value(i);
				if !desc.is_empty() {
					println!("    Description: {}", desc);
				}
			}
		}
	}
	if !found_outgoing {
		println!("  (none)");
	}

	// Show incoming relationships
	println!("\n🔗 Incoming relationships:");
	let mut found_incoming = false;
	for (node_id, _, _) in &file_nodes {
		for i in 0..relationships_batch.num_rows() {
			if rel_targets.value(i) == *node_id {
				found_incoming = true;
				println!(
					"  {} → {} ({})",
					rel_sources.value(i),
					rel_targets.value(i),
					rel_types.value(i)
				);
				let desc = rel_descriptions.value(i);
				if !desc.is_empty() {
					println!("    Description: {}", desc);
				}
			}
		}
	}
	if !found_incoming {
		println!("  (none)");
	}

	Ok(())
}
