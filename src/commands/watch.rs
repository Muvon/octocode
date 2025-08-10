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
use octocode::indexer;
use octocode::lock::IndexLock;
use octocode::state;
use octocode::store::Store;
use octocode::watcher_config::{
	IgnorePatterns, DEFAULT_ADDITIONAL_DELAY_MS, MAX_ADDITIONAL_DELAY_MS,
	WATCH_DEFAULT_DEBOUNCE_SECS, WATCH_MAX_DEBOUNCE_SECS, WATCH_MIN_DEBOUNCE_SECS,
};

use super::index::IndexArgs;

#[derive(Args, Debug)]
pub struct WatchArgs {
	/// Run in quiet mode with less output
	#[arg(long, short)]
	pub quiet: bool,

	/// Change debounce time in seconds (min: 1, max: 30, default: 2)
	#[arg(long, short)]
	pub debounce: Option<u64>,

	/// Additional delay after debounce in milliseconds (default: 1000, max: 5000)
	#[arg(long)]
	pub additional_delay: Option<u64>,

	/// Skip git repository requirement and git-based optimizations
	#[arg(long)]
	pub no_git: bool,
}

pub async fn execute(
	store: &Store,
	config: &Config,
	args: &WatchArgs,
) -> Result<(), anyhow::Error> {
	let current_dir = std::env::current_dir()?;

	// Get the debounce time from args or use default, with bounds checking
	let debounce_secs = args
		.debounce
		.unwrap_or(WATCH_DEFAULT_DEBOUNCE_SECS)
		.clamp(WATCH_MIN_DEBOUNCE_SECS, WATCH_MAX_DEBOUNCE_SECS);
	let additional_delay_ms = args
		.additional_delay
		.unwrap_or(DEFAULT_ADDITIONAL_DELAY_MS)
		.clamp(0, MAX_ADDITIONAL_DELAY_MS);

	// Only show verbose output if not in quiet mode
	if !args.quiet {
		println!(
			"Starting watch mode for current directory: {}",
			current_dir.display()
		);
		println!(
			"Configuration: debounce={}s, additional_delay={}ms",
			debounce_secs, additional_delay_ms
		);
		println!("Initial indexing...");
	}

	// Do initial indexing
	if !args.quiet {
		// If not in quiet mode, use the regular indexing with progress display
		super::index::execute(
			store,
			config,
			&IndexArgs {
				no_git: args.no_git,
				list_files: false,
				show_file: None,
				graphrag: None,
			},
		)
		.await?
	} else {
		// In quiet mode, just do the indexing without progress display
		let state = state::create_shared_state();
		state.write().current_directory = current_dir.clone();

		// Get git root for optimization
		let git_repo_root = if !args.no_git {
			indexer::git::find_git_root(&current_dir)
		} else {
			None
		};

		// Acquire lock for initial indexing
		let mut lock = IndexLock::new(&current_dir)?;
		lock.acquire()?;
		tracing::debug!("Watch mode: acquired indexing lock for initial scan");

		indexer::index_files(store, state.clone(), config, git_repo_root.as_deref()).await?;

		// Release lock after initial indexing
		lock.release()?;
		tracing::debug!("Watch mode: released indexing lock after initial scan");
	}

	if !args.quiet {
		println!("Loaded ignore patterns from .gitignore and .noindex files");
		println!("Watching for changes (press Ctrl+C to stop)...");
	}

	// Setup the file watcher with debouncer
	use notify_debouncer_mini::notify::RecursiveMode;
	use notify_debouncer_mini::{new_debouncer, DebouncedEvent};
	use std::sync::mpsc::channel;
	use std::time::Duration;

	let (tx, rx) = channel();

	// Copy quiet flag to capture in closure
	let quiet_mode = args.quiet;

	// Create ignore patterns manager
	let ignore_patterns = IgnorePatterns::new(current_dir.clone());

	// Create a debounced watcher to call our tx sender when files change
	let mut debouncer = new_debouncer(
		Duration::from_secs(debounce_secs),
		move |res: Result<Vec<DebouncedEvent>, notify_debouncer_mini::notify::Error>| {
			match res {
				Ok(events) => {
					// Filter out events from irrelevant paths using ignore patterns
					let relevant_events = events
						.iter()
						.filter(|event| !ignore_patterns.should_ignore_path(&event.path))
						.count();

					if relevant_events > 0 {
						let _ = tx.send(());
					}
				}
				Err(e) => {
					if !quiet_mode {
						eprintln!("Error in file watcher: {:?}", e);
					}
				}
			}
		},
	)?;

	// Add the current directory to the watcher
	debouncer
		.watcher()
		.watch(&current_dir, RecursiveMode::Recursive)?;

	// Create shared state for reindexing
	let state = state::create_shared_state();
	state.write().current_directory = current_dir;

	// Keep a copy of the config for reindexing
	let config = config.clone();

	loop {
		// Wait for changes
		match rx.recv() {
			Ok(()) => {
				if !args.quiet {
					println!("\nDetected file changes, reindexing...");
				}

				// Reset the indexing state
				{
					let mut state_guard = state.write();
					state_guard.indexed_files = 0;
					state_guard.indexing_complete = false;
				}

				// Additional delay to ensure all file operations are complete
				if additional_delay_ms > 0 {
					tokio::time::sleep(tokio::time::Duration::from_millis(additional_delay_ms))
						.await;
				}

				if !args.quiet {
					// Use regular indexing with progress in non-quiet mode
					super::index::execute(
						store,
						&config,
						&IndexArgs {
							no_git: args.no_git,
							list_files: false,
							show_file: None,
							graphrag: None,
						},
					)
					.await?
				} else {
					// In quiet mode, just do the indexing without progress display
					let current_dir = state.read().current_directory.clone();
					let git_repo_root = if !args.no_git {
						indexer::git::find_git_root(&current_dir)
					} else {
						None
					};

					// Acquire lock for re-indexing
					let mut lock = IndexLock::new(&current_dir)?;
					lock.acquire()?;
					tracing::debug!("Watch mode: acquired indexing lock for re-indexing");

					indexer::index_files(store, state.clone(), &config, git_repo_root.as_deref())
						.await?;

					// Release lock after re-indexing
					lock.release()?;
					tracing::debug!("Watch mode: released indexing lock after re-indexing");
				}
			}
			Err(e) => {
				if !args.quiet {
					eprintln!("Watch error: {:?}", e);
				}
				break;
			}
		}
	}

	Ok(())
}
