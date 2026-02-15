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

//! File watcher for MCP server
//! Monitors file system changes and triggers reindexing

use anyhow::Result;
use notify_debouncer_mini::notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEvent};
use tokio::sync::mpsc;
use tokio::time::Duration;
use tracing::{debug, trace, warn};

use crate::mcp::logging::{log_critical_anyhow_error, log_critical_error, log_watcher_event};
use crate::watcher_config::{IgnorePatterns, MIN_DEBOUNCE_MS};

const MAX_WATCHER_ERRORS: u32 = 5;

/// Run file watcher with debouncing and error recovery
pub async fn run_watcher(
	tx: mpsc::Sender<()>,
	working_dir: std::path::PathBuf,
	debug: bool,
	max_pending_events: usize,
) -> Result<()> {
	let (debouncer_tx, mut debouncer_rx) = mpsc::channel(max_pending_events);

	// Create ignore patterns manager with error handling
	let ignore_patterns = IgnorePatterns::new(working_dir.clone());

	// Use minimal debounce for the file watcher itself - we handle the real debouncing in the event handler
	let mut debouncer = new_debouncer(
		Duration::from_millis(MIN_DEBOUNCE_MS),
		move |res: Result<Vec<DebouncedEvent>, notify_debouncer_mini::notify::Error>| match res {
			Ok(events) => {
				// Filter out events from irrelevant paths using ignore patterns
				let relevant_events: Vec<_> = events
					.iter()
					.filter(|event| !ignore_patterns.should_ignore_path(&event.path))
					.collect();

				if !relevant_events.is_empty() {
					// Log file watcher events using our structured logging
					log_watcher_event("file_change_batch", None, relevant_events.len());

					if debug && crate::mcp::server::MCP_ENABLE_VERBOSE_EVENTS {
						trace!(
							event_count = relevant_events.len(),
							"File watcher detected relevant events"
						);
						for event in &relevant_events {
							trace!(
								event_kind = ?event.kind,
								event_path = ?event.path,
								"File watcher event detail"
							);
						}
					}

					// Send notification for each relevant event batch with error handling
					if let Err(e) = debouncer_tx.try_send(()) {
						warn!(
							error = ?e,
							"Failed to send file event - channel may be full"
						);
						// Don't panic on channel send failure - just log and continue
					}
				}
			}
			Err(e) => {
				log_critical_error("File watcher error", &e);
				// Continue running even on watcher errors
			}
		},
	)
	.map_err(|e| anyhow::anyhow!("Failed to create file watcher: {}", e))?;

	// Watch directory with error handling
	if let Err(e) = debouncer
		.watcher()
		.watch(&working_dir, RecursiveMode::Recursive)
	{
		log_critical_error("Failed to start watching directory", &e);
		return Err(anyhow::anyhow!("Failed to watch directory: {}", e));
	}

	debug!(
		working_dir = %working_dir.display(),
		"File watcher started with ignore patterns loaded"
	);

	// Forward events from debouncer to the main event handler with error recovery
	let mut consecutive_errors = 0u32;

	while let Some(()) = debouncer_rx.recv().await {
		match tx.send(()).await {
			Ok(()) => {
				consecutive_errors = 0; // Reset on successful send
			}
			Err(e) => {
				consecutive_errors += 1;
				log_critical_error("Event channel send failed", &e);

				debug!(
					consecutive_errors = consecutive_errors,
					"Event channel closed or failed"
				);

				if consecutive_errors >= MAX_WATCHER_ERRORS {
					log_critical_anyhow_error(
						"Too many watcher errors",
						&anyhow::anyhow!(
							"Stopping file watcher after {} consecutive errors",
							consecutive_errors
						),
					);
					break;
				}

				// Brief delay before retrying
				tokio::time::sleep(Duration::from_millis(100)).await;
			}
		}
	}

	debug!("File watcher stopped");

	Ok(())
}
