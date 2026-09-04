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
	use crate::indexer::git_utils::GitUtils;
	use std::path::Path;
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

	/// `main` with two commits and a `feature` branch one commit ahead.
	fn repo() -> TempDir {
		let dir = TempDir::new().unwrap();
		let path = dir.path();
		git(path, &["init", "-q", "-b", "main"]);
		git(path, &["config", "user.email", "test@example.com"]);
		git(path, &["config", "user.name", "Test"]);

		std::fs::write(path.join("first.txt"), "one\n").unwrap();
		git(path, &["add", "."]);
		git(path, &["commit", "-q", "-m", "first commit"]);

		std::fs::write(path.join("second.txt"), "two\n").unwrap();
		git(path, &["add", "."]);
		git(path, &["commit", "-q", "-m", "second commit"]);

		git(path, &["checkout", "-q", "-b", "feature"]);
		std::fs::write(path.join("third.txt"), "three\n").unwrap();
		git(path, &["add", "."]);
		git(path, &["commit", "-q", "-m", "third commit"]);

		dir
	}

	#[test]
	fn a_plain_directory_is_not_a_repository() {
		let dir = TempDir::new().unwrap();
		assert!(!GitUtils::is_git_repo_root(dir.path()));
		assert_eq!(GitUtils::find_git_root(dir.path()), None);
		assert!(GitUtils::get_current_commit_hash(dir.path()).is_err());
	}

	#[test]
	fn the_repository_root_is_found_from_a_nested_directory() {
		let dir = repo();
		let nested = dir.path().join("a/b");
		std::fs::create_dir_all(&nested).unwrap();
		assert!(GitUtils::is_git_repo_root(dir.path()));
		assert_eq!(
			GitUtils::find_git_root(&nested).as_deref(),
			Some(dir.path())
		);
	}

	#[test]
	fn the_head_commit_hash_is_a_full_sha() {
		let dir = repo();
		let hash = GitUtils::get_current_commit_hash(dir.path()).unwrap();
		assert_eq!(hash.len(), 40);
		assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
	}

	#[test]
	fn files_changed_since_a_commit_are_listed() {
		let dir = repo();
		let root = git(dir.path(), &["rev-list", "--max-parents=0", "HEAD"]);
		let changed = GitUtils::get_changed_files_since_commit(dir.path(), &root).unwrap();
		assert!(changed.contains(&"second.txt".to_string()), "{changed:?}");
		assert!(changed.contains(&"third.txt".to_string()), "{changed:?}");
		assert!(!changed.contains(&"first.txt".to_string()));
	}

	#[test]
	fn staged_and_working_tree_changes_are_reported_separately() {
		let dir = repo();
		assert!(GitUtils::get_staged_files(dir.path()).unwrap().is_empty());

		std::fs::write(dir.path().join("untracked.txt"), "new\n").unwrap();
		std::fs::write(dir.path().join("first.txt"), "edited\n").unwrap();

		let all = GitUtils::get_all_changed_files(dir.path()).unwrap();
		assert!(all.contains(&"untracked.txt".to_string()), "{all:?}");
		assert!(all.contains(&"first.txt".to_string()), "{all:?}");
		assert!(GitUtils::get_staged_files(dir.path()).unwrap().is_empty());

		git(dir.path(), &["add", "first.txt"]);
		let staged = GitUtils::get_staged_files(dir.path()).unwrap();
		assert_eq!(staged, vec!["first.txt".to_string()]);
	}

	#[test]
	fn refs_resolve_to_a_hash_or_nothing() {
		let dir = repo();
		let head = GitUtils::resolve_ref(dir.path(), "HEAD").expect("HEAD resolves");
		assert_eq!(head.len(), 40);
		assert_eq!(
			GitUtils::resolve_ref(dir.path(), "main").map(|h| h.len()),
			Some(40)
		);
		assert_eq!(GitUtils::resolve_ref(dir.path(), "no-such-ref"), None);
	}

	#[test]
	fn the_merge_base_of_a_branch_and_its_parent_is_the_fork_point() {
		let dir = repo();
		let base = GitUtils::merge_base(dir.path(), "main", "feature").unwrap();
		let main_head = GitUtils::resolve_ref(dir.path(), "main").unwrap();
		assert_eq!(base, main_head, "feature branched straight off main's tip");
		assert!(GitUtils::merge_base(dir.path(), "main", "no-such-ref").is_err());
	}

	#[test]
	fn commits_ahead_counts_only_the_branch_side() {
		let dir = repo();
		assert_eq!(
			GitUtils::commits_ahead(dir.path(), "main", "feature").unwrap(),
			1
		);
		assert_eq!(
			GitUtils::commits_ahead(dir.path(), "feature", "main").unwrap(),
			0
		);
	}

	#[test]
	fn the_default_branch_falls_back_to_the_local_head_without_a_remote() {
		let dir = repo();
		let default = GitUtils::get_default_branch(dir.path()).unwrap();
		assert!(!default.is_empty());
	}

	#[test]
	fn the_commit_log_is_returned_oldest_first() {
		let dir = repo();
		let all = GitUtils::get_commit_log(dir.path(), "feature", None).unwrap();
		assert_eq!(all.len(), 3);
		assert_eq!(all[0].message, "first commit");
		assert_eq!(all[2].message, "third commit");
		assert!(!all[0].hash.is_empty());
		assert_eq!(all[0].author, "Test");
		assert!(all[0].date > 0);
	}

	#[test]
	fn a_since_commit_excludes_everything_up_to_and_including_it() {
		let dir = repo();
		let root = git(dir.path(), &["rev-list", "--max-parents=0", "HEAD"]);
		let since = GitUtils::get_commit_log(dir.path(), "feature", Some(&root)).unwrap();
		assert_eq!(since.len(), 2);
		assert_eq!(since[0].message, "second commit");

		assert!(GitUtils::get_commit_log(dir.path(), "no-such-branch", None).is_err());
	}

	#[test]
	fn a_commit_diff_is_returned_and_capped_at_the_character_budget() {
		let dir = repo();
		let head = GitUtils::get_current_commit_hash(dir.path()).unwrap();

		let full = GitUtils::get_commit_diff(dir.path(), &head, 10_000).unwrap();
		assert!(full.contains("third.txt"), "{full}");

		let capped = GitUtils::get_commit_diff(dir.path(), &head, 20).unwrap();
		assert!(capped.len() <= full.len());
	}

	#[test]
	fn the_root_commit_reports_its_files() {
		// `git diff-tree` without --root returns nothing for the first commit.
		let dir = repo();
		let root = git(dir.path(), &["rev-list", "--max-parents=0", "HEAD"]);
		let files = GitUtils::get_changed_files_for_commit(dir.path(), &root).unwrap();
		assert_eq!(files, vec!["first.txt".to_string()]);
	}

	#[test]
	fn a_diff_against_an_unreachable_commit_fails_instead_of_reporting_no_changes() {
		// A last-indexed commit can stop being reachable after a rebase, squash,
		// force-push or `git gc`. Answering "nothing changed" there makes the
		// caller index zero files and still stamp the new commit, leaving the
		// index permanently stale.
		let dir = repo();
		let missing = "0000000000000000000000000000000000000000";
		let err = GitUtils::get_changed_files_since_commit(dir.path(), missing)
			.expect_err("an unreachable commit must be an error");
		assert!(err.to_string().contains("git diff"), "{err}");
	}
}
