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

#[cfg(test)]
mod tests {
	use crate::mcp::logging::*;
	use crate::storage::get_project_storage_path;
	use serde_json::json;
	use std::fs;
	use tempfile::TempDir;

	#[derive(Debug)]
	struct TestError;

	impl std::fmt::Display for TestError {
		fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
			write!(f, "test failure")
		}
	}

	impl std::error::Error for TestError {}

	#[test]
	fn initialising_logging_creates_the_project_log_directory() {
		// `MCP_LOG_DIR` is a process-wide OnceLock, so first-init behaviour can
		// only be asserted from a single test — the repeat call is checked here
		// too rather than in a second test that might win the race.
		let dir = TempDir::new().unwrap();
		init_mcp_logging(dir.path().to_path_buf(), true).expect("init must never fail startup");

		let storage = get_project_storage_path(dir.path()).unwrap();
		assert!(storage.join("logs").is_dir());
		assert_eq!(
			fs::read_to_string(storage.join("latest_log.txt")).unwrap(),
			storage.join("logs").to_string_lossy()
		);
		assert_eq!(get_log_directory().as_ref(), Some(&storage.join("logs")));

		// A repeat init cannot set the global again; it must be swallowed rather
		// than break MCP startup.
		let second = TempDir::new().unwrap();
		init_mcp_logging(second.path().to_path_buf(), false).unwrap();
		assert_eq!(get_log_directory().as_ref(), Some(&storage.join("logs")));
	}

	#[test]
	fn log_directories_are_empty_until_the_project_has_logged() {
		let dir = TempDir::new().unwrap();
		assert!(get_all_log_directories(dir.path()).unwrap().is_empty());

		let logs = get_project_storage_path(dir.path()).unwrap().join("logs");
		fs::create_dir_all(&logs).unwrap();
		assert_eq!(get_all_log_directories(dir.path()).unwrap(), vec![logs]);
	}

	#[test]
	fn printing_log_directories_handles_missing_empty_and_populated_dirs() {
		let dir = TempDir::new().unwrap();
		print_log_directories(dir.path()).unwrap();

		let logs = get_project_storage_path(dir.path()).unwrap().join("logs");
		fs::create_dir_all(&logs).unwrap();
		print_log_directories(dir.path()).unwrap();

		fs::write(logs.join("mcp_server.log"), "line\n").unwrap();
		fs::write(logs.join("notes.txt"), "ignored\n").unwrap();
		print_log_directories(dir.path()).unwrap();
	}

	#[test]
	fn request_and_response_logging_covers_every_method_shape() {
		let id = json!(7);
		log_mcp_request(
			"tools/call",
			Some(&json!({"name": "semantic_search"})),
			Some(&id),
		);
		log_mcp_request("tools/call", Some(&json!({})), None);
		log_mcp_request("initialize", None, Some(&id));
		log_mcp_request("tools/list", None, None);
		log_mcp_request("resources/read", Some(&json!({"uri": "x"})), Some(&id));

		log_mcp_response("tools/call", true, Some(&id), Some(12));
		log_mcp_response("tools/call", false, None, None);
	}

	#[test]
	fn error_logging_accepts_both_error_flavours() {
		log_critical_error("startup", &TestError);
		log_critical_anyhow_error("startup", &anyhow::anyhow!("boom"));
		log_file_processing_error("src/a.rs", "parse", &TestError);
	}

	#[test]
	fn watcher_event_logging_covers_every_branch() {
		let path = std::path::Path::new("src/a.rs");
		log_watcher_event("file_change_batch", Some(path), 6);
		log_watcher_event("file_change_batch", Some(path), 2);
		log_watcher_event("file_change", None, 10);
		log_watcher_event("file_change", None, 11);
		log_watcher_event("debounce_trigger", None, 1);
		log_watcher_event("unknown", Some(path), 0);
	}

	#[test]
	fn indexing_operation_logging_covers_every_performance_tier() {
		for duration in [None, Some(0), Some(1_001), Some(5_001), Some(20_000)] {
			log_indexing_operation("index", Some(3), duration, true);
			log_indexing_operation("index", None, duration, false);
		}
	}

	#[test]
	fn indexing_progress_logging_covers_every_phase() {
		log_indexing_progress("file_processing", 50, 100, Some("a.rs"), 2);
		log_indexing_progress("file_processing", 100, 100, None, 2);
		log_indexing_progress("file_processing", 7, 100, None, 0);
		log_indexing_progress("cleanup", 0, 0, None, 0);
		log_indexing_progress("git_optimization", 0, 0, Some("a.rs"), 0);
		log_indexing_progress("graphrag_build", 1, 2, None, 0);
		log_indexing_progress("other", 1, 0, None, 0);
	}

	#[test]
	fn metric_and_git_logging_handle_zero_values() {
		log_performance_metrics("embed", 0, 10, None);
		log_performance_metrics("embed", 2_000, 10, Some(64.5));
		log_git_operation("status", "/repo", Some(2), true);
		log_git_operation("status", "/repo", None, false);
	}
}
