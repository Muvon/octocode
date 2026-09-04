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
	use crate::state::create_shared_state;
	use crate::store::mod_tests::test_store;
	use crate::store::tables;
	use std::process::Command;
	use tempfile::TempDir;

	fn git(repo: &Path, args: &[&str]) -> String {
		let out = Command::new("git")
			.args(args)
			.current_dir(repo)
			.output()
			.unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
		assert!(
			out.status.success(),
			"git {args:?} failed: {}",
			String::from_utf8_lossy(&out.stderr)
		);
		String::from_utf8_lossy(&out.stdout).trim().to_string()
	}

	fn repo_with_one_commit() -> TempDir {
		let dir = TempDir::new().unwrap();
		git(dir.path(), &["init", "-q", "-b", "main"]);
		git(dir.path(), &["config", "user.email", "test@example.com"]);
		git(dir.path(), &["config", "user.name", "Test"]);
		std::fs::write(dir.path().join("keep.txt"), "keep\n").unwrap();
		git(dir.path(), &["add", "."]);
		git(dir.path(), &["commit", "-q", "-m", "feat: add keep.txt"]);
		dir
	}

	#[tokio::test]
	async fn a_directory_without_a_repository_indexes_nothing() {
		let (_db, store) = test_store().await;
		let dir = TempDir::new().unwrap();

		index_commits(
			&Config::default(),
			&store,
			dir.path(),
			create_shared_state(),
			true,
		)
		.await
		.expect("a missing default branch is not fatal");

		assert_eq!(
			store
				.get_table_row_count(tables::COMMIT_BLOCKS)
				.await
				.unwrap(),
			0
		);
		assert_eq!(store.get_commits_last_commit_hash().await.unwrap(), None);
	}

	#[tokio::test]
	async fn commits_already_recorded_are_not_indexed_again() {
		let (_db, store) = test_store().await;
		let dir = repo_with_one_commit();
		let head = git(dir.path(), &["rev-parse", "HEAD"]);
		store.store_commits_last_commit_hash(&head).await.unwrap();

		let state = create_shared_state();
		index_commits(&Config::default(), &store, dir.path(), state.clone(), true)
			.await
			.unwrap();

		assert_eq!(
			store
				.get_table_row_count(tables::COMMIT_BLOCKS)
				.await
				.unwrap(),
			0,
			"nothing new to index means no embedding work"
		);
		assert_eq!(
			store.get_commits_last_commit_hash().await.unwrap(),
			Some(head)
		);
		// The early return happens before any progress message is set.
		assert!(state.read().status_message.is_empty());
	}

	#[tokio::test]
	async fn descriptions_are_empty_when_the_configured_llm_cannot_be_built() {
		let dir = repo_with_one_commit();
		let mut config = Config::default();
		config.llm.model = "no-such-provider:some-model".to_string();

		let commits = vec![CommitEntry {
			hash: "abc12345deadbeef".to_string(),
			author: "Alice".to_string(),
			date: 1_700_000_000,
			message: "feat: add retry logic".to_string(),
		}];

		let descriptions = generate_descriptions(&config, dir.path(), &commits, true)
			.await
			.expect("an unavailable LLM downgrades to no descriptions");
		assert!(descriptions.is_empty());
	}
}
