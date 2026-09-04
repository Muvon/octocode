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
	use super::super::*;
	use tempfile::TempDir;

	/// Keep rewriting `path` so the debouncer keeps seeing change batches for as
	/// long as the test needs them. Aborted by the caller once it has observed
	/// what it is waiting for.
	fn touch_repeatedly(path: std::path::PathBuf) -> tokio::task::JoinHandle<()> {
		tokio::spawn(async move {
			let mut i = 0u64;
			loop {
				let _ = std::fs::write(&path, format!("fn v{i}() {{}}\n"));
				tokio::time::sleep(Duration::from_millis(50)).await;
				i += 1;
			}
		})
	}

	/// Consume queued notifications until the channel has been quiet for a
	/// second, so a later "nothing arrives" assertion cannot see a stale event.
	async fn drain(rx: &mut mpsc::Receiver<()>) {
		while let Ok(Some(())) = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {}
	}

	#[tokio::test]
	async fn watching_a_missing_directory_is_reported_as_an_error() {
		let (tx, _rx) = mpsc::channel(4);
		let missing = std::env::temp_dir().join("octocode-watcher-no-such-directory");
		let _ = std::fs::remove_dir_all(&missing);

		let err = run_watcher(tx, missing, false, 4)
			.await
			.expect_err("watching a path that does not exist must fail");
		assert!(
			err.to_string().starts_with("Failed to watch directory"),
			"{err}"
		);
	}

	#[tokio::test]
	async fn a_source_change_is_forwarded_to_the_index_channel() {
		let dir = TempDir::new().unwrap();
		let (tx, mut rx) = mpsc::channel(8);
		let watcher = tokio::spawn(run_watcher(tx, dir.path().to_path_buf(), false, 8));

		let writer = touch_repeatedly(dir.path().join("a.rs"));
		let seen = tokio::time::timeout(Duration::from_secs(30), rx.recv()).await;
		writer.abort();
		watcher.abort();

		assert_eq!(
			seen.expect("a file change must reach the index channel"),
			Some(())
		);
	}

	#[tokio::test]
	async fn changes_under_dot_git_are_filtered_out() {
		let dir = TempDir::new().unwrap();
		std::fs::create_dir_all(dir.path().join(".git")).unwrap();
		let (tx, mut rx) = mpsc::channel(8);
		let watcher = tokio::spawn(run_watcher(tx, dir.path().to_path_buf(), false, 8));

		// Prove the watcher is live before asserting on silence.
		let writer = touch_repeatedly(dir.path().join("a.rs"));
		let seen = tokio::time::timeout(Duration::from_secs(30), rx.recv()).await;
		writer.abort();
		assert_eq!(seen.expect("watcher must be running"), Some(()));
		drain(&mut rx).await;

		let git_writer = touch_repeatedly(dir.path().join(".git").join("index"));
		let quiet = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await;
		git_writer.abort();
		watcher.abort();

		assert!(
			quiet.is_err(),
			"git metadata churn must not trigger a reindex, got {quiet:?}"
		);
	}

	#[tokio::test]
	async fn the_watcher_stops_after_repeated_send_failures() {
		let dir = TempDir::new().unwrap();
		let (tx, rx) = mpsc::channel(8);
		// A closed receiver fails every forward; the watcher gives up after
		// MAX_WATCHER_ERRORS consecutive failures instead of spinning forever.
		drop(rx);

		let writer = touch_repeatedly(dir.path().join("a.rs"));
		let stopped = tokio::time::timeout(
			Duration::from_secs(60),
			run_watcher(tx, dir.path().to_path_buf(), false, 8),
		)
		.await;
		writer.abort();

		let result = stopped.expect("the watcher must stop once the channel is gone");
		assert!(
			result.is_ok(),
			"giving up is a clean stop, not an error: {result:?}"
		);
	}
}
