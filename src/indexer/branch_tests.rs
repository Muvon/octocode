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

	/// `main` with two files, plus a `feature` branch that edits one, adds one
	/// and deletes another.
	fn repo() -> TempDir {
		let dir = TempDir::new().unwrap();
		let path = dir.path();
		git(path, &["init", "-q", "-b", "main"]);
		git(path, &["config", "user.email", "test@example.com"]);
		git(path, &["config", "user.name", "Test"]);

		std::fs::write(path.join("keep.txt"), "keep\n").unwrap();
		std::fs::write(path.join("gone.txt"), "gone\n").unwrap();
		git(path, &["add", "."]);
		git(path, &["commit", "-q", "-m", "base"]);

		git(path, &["checkout", "-q", "-b", "feature"]);
		std::fs::write(path.join("keep.txt"), "changed\n").unwrap();
		std::fs::write(path.join("added.txt"), "added\n").unwrap();
		std::fs::remove_file(path.join("gone.txt")).unwrap();
		git(path, &["add", "-A"]);
		git(path, &["commit", "-q", "-m", "feature work"]);

		dir
	}

	fn manifest(changed: &[&str], deleted: &[&str], base_db_commit: &str) -> BranchManifest {
		BranchManifest {
			version: 2,
			branch_name: "feature".to_string(),
			base_branch: "main".to_string(),
			base_commit: "aaa".to_string(),
			branch_commit: "bbb".to_string(),
			changed_paths: changed.iter().map(|s| s.to_string()).collect(),
			deleted_paths: deleted.iter().map(|s| s.to_string()).collect(),
			indexed_at: 1_700_000_000,
			fork_point: "ccc".to_string(),
			base_db_commit: base_db_commit.to_string(),
			remote_base_observed: String::new(),
		}
	}

	#[test]
	fn a_slash_in_a_branch_name_becomes_a_double_dash() {
		assert_eq!(sanitize_branch_name("feature/foo"), "feature--foo");
		assert_eq!(sanitize_branch_name("fix/deep/nested"), "fix--deep--nested");
		assert_eq!(sanitize_branch_name("no-slash"), "no-slash");
	}

	#[test]
	fn sanitizing_round_trips_for_ordinary_branch_names() {
		for name in ["feature/foo", "main", "fix/a/b/c", "simple"] {
			assert_eq!(desanitize_branch_name(&sanitize_branch_name(name)), name);
		}
	}

	#[test]
	fn desanitizing_is_lossy_for_a_name_that_already_contains_a_double_dash() {
		// Documented limitation: desanitize is only a display hint. The canonical
		// branch name lives in the manifest, which is what callers must trust.
		assert_eq!(sanitize_branch_name("wip--x"), "wip--x");
		assert_eq!(desanitize_branch_name("wip--x"), "wip/x");
	}

	#[test]
	fn overridden_paths_covers_changed_and_deleted_files() {
		let manifest = manifest(&["a.rs", "b.rs"], &["c.rs"], "");
		let overridden = manifest.overridden_paths();
		assert_eq!(overridden.len(), 3);
		assert!(overridden.contains("a.rs"));
		assert!(overridden.contains("c.rs"));
	}

	#[test]
	fn a_branch_that_changed_nothing_overrides_nothing() {
		assert!(manifest(&[], &[], "").overridden_paths().is_empty());
	}

	#[test]
	fn a_v2_manifest_only_overlays_onto_the_commit_it_was_built_against() {
		let m = manifest(&[], &[], "abc123");
		assert!(manifest_is_coherent_with(&m, Some("abc123")));
		assert!(!manifest_is_coherent_with(&m, Some("def456")));
		assert!(!manifest_is_coherent_with(&m, None));
	}

	#[test]
	fn a_legacy_manifest_without_a_recorded_commit_is_grandfathered() {
		let m = manifest(&[], &[], "");
		assert!(manifest_is_coherent_with(&m, Some("anything")));
		assert!(manifest_is_coherent_with(&m, None));
	}

	#[test]
	fn a_manifest_round_trips_through_disk() {
		let dir = TempDir::new().unwrap();
		let branch_dir = dir.path().join("branches/feature");

		assert!(load_manifest(&branch_dir).unwrap().is_none());

		let original = manifest(&["a.rs"], &["b.rs"], "abc123");
		save_manifest(&branch_dir, &original).unwrap();

		let loaded = load_manifest(&branch_dir).unwrap().expect("manifest");
		assert_eq!(loaded.version, 2);
		assert_eq!(loaded.branch_name, "feature");
		assert_eq!(loaded.base_branch, "main");
		assert_eq!(loaded.base_commit, "aaa");
		assert_eq!(loaded.branch_commit, "bbb");
		assert_eq!(loaded.changed_paths, vec!["a.rs".to_string()]);
		assert_eq!(loaded.deleted_paths, vec!["b.rs".to_string()]);
		assert_eq!(loaded.indexed_at, 1_700_000_000);
		assert_eq!(loaded.fork_point, "ccc");
		assert_eq!(loaded.base_db_commit, "abc123");
		assert_eq!(loaded.remote_base_observed, "");
	}

	#[test]
	fn a_corrupt_manifest_is_reported_as_an_error() {
		let dir = TempDir::new().unwrap();
		std::fs::create_dir_all(dir.path()).unwrap();
		std::fs::write(dir.path().join("manifest.json"), "{not json").unwrap();
		assert!(load_manifest(dir.path()).is_err());
	}

	#[test]
	fn the_current_branch_is_read_from_git() {
		let dir = repo();
		assert_eq!(get_current_branch(dir.path()).as_deref(), Some("feature"));

		// A detached HEAD is not a branch.
		let head = git(dir.path(), &["rev-parse", "HEAD"]);
		git(dir.path(), &["checkout", "-q", &head]);
		assert_eq!(get_current_branch(dir.path()), None);
	}

	#[test]
	fn a_non_repository_has_no_current_branch() {
		let dir = TempDir::new().unwrap();
		assert_eq!(get_current_branch(dir.path()), None);
	}

	#[test]
	fn branch_context_is_empty_on_the_default_branch() {
		let dir = repo();
		assert_eq!(
			detect_branch_context(dir.path()).as_deref(),
			Some("feature")
		);

		git(dir.path(), &["checkout", "-q", "main"]);
		assert_eq!(detect_branch_context(dir.path()), None);
	}

	#[test]
	fn branch_commits_resolve_and_unknown_refs_error() {
		let dir = repo();
		let head = get_branch_commit(dir.path(), "feature").unwrap();
		assert_eq!(head.len(), 40);
		assert!(get_branch_commit(dir.path(), "no-such-branch").is_err());
	}

	#[test]
	fn branch_existence_is_checked_against_git() {
		let dir = repo();
		assert!(branch_exists_in_git(dir.path(), "main"));
		assert!(branch_exists_in_git(dir.path(), "feature"));
		assert!(!branch_exists_in_git(dir.path(), "nope"));
	}

	#[test]
	fn nothing_exists_in_a_directory_that_is_not_a_repository() {
		let dir = TempDir::new().unwrap();
		assert!(!branch_exists_in_git(dir.path(), "main"));
	}

	#[test]
	fn the_committed_diff_lists_every_touched_path() {
		let dir = repo();
		let mut all = get_diff_files(dir.path(), "main", None).unwrap();
		all.sort();
		assert_eq!(all, vec!["added.txt", "gone.txt", "keep.txt"]);
	}

	#[test]
	fn the_delete_filter_narrows_the_diff_to_removed_paths() {
		let dir = repo();
		assert_eq!(
			get_diff_files(dir.path(), "main", Some("D")).unwrap(),
			vec!["gone.txt".to_string()]
		);
	}

	#[test]
	fn a_diff_against_an_unresolvable_ref_reports_the_range() {
		let dir = repo();
		let err = get_diff_files(dir.path(), "no-such-branch", None)
			.expect_err("unknown ref must fail")
			.to_string();
		assert!(err.contains("no-such-branch...HEAD"), "{err}");
	}

	#[test]
	fn a_clean_checkout_has_no_working_tree_changes() {
		let dir = repo();
		assert!(get_working_tree_changes(dir.path()).unwrap().is_empty());
	}

	#[test]
	fn staged_unstaged_and_untracked_files_are_all_reported() {
		let dir = repo();
		std::fs::write(dir.path().join("keep.txt"), "edited again\n").unwrap();
		std::fs::write(dir.path().join("staged.txt"), "staged\n").unwrap();
		git(dir.path(), &["add", "staged.txt"]);
		std::fs::write(dir.path().join("untracked.txt"), "new\n").unwrap();

		let mut changes = get_working_tree_changes(dir.path()).unwrap();
		changes.sort();
		assert_eq!(changes, vec!["keep.txt", "staged.txt", "untracked.txt"]);
	}

	#[test]
	fn the_delta_separates_changed_from_deleted_files() {
		let dir = repo();
		let (changed, deleted) = compute_branch_delta(dir.path(), "main").unwrap();

		assert!(changed.contains(&"keep.txt".to_string()), "{changed:?}");
		assert!(changed.contains(&"added.txt".to_string()), "{changed:?}");
		assert_eq!(deleted, vec!["gone.txt".to_string()]);
		// A deleted file must never also appear as changed.
		assert!(!changed.contains(&"gone.txt".to_string()));
	}

	#[test]
	fn uncommitted_work_is_included_in_the_delta() {
		let dir = repo();
		std::fs::write(dir.path().join("untracked.txt"), "new\n").unwrap();
		std::fs::write(dir.path().join("keep.txt"), "changed again\n").unwrap();

		let (changed, _) = compute_branch_delta(dir.path(), "main").unwrap();
		assert!(
			changed.contains(&"untracked.txt".to_string()),
			"{changed:?}"
		);
	}

	#[test]
	fn the_default_branch_has_an_empty_delta_against_itself() {
		let dir = repo();
		git(dir.path(), &["checkout", "-q", "main"]);
		let (changed, deleted) = compute_branch_delta(dir.path(), "main").unwrap();
		assert!(changed.is_empty(), "{changed:?}");
		assert!(deleted.is_empty(), "{deleted:?}");
	}

	#[test]
	fn a_delta_against_an_unknown_base_is_an_error() {
		let dir = repo();
		assert!(compute_branch_delta(dir.path(), "no-such-branch").is_err());
	}

	#[test]
	fn merged_branches_are_listed_without_the_default() {
		let dir = repo();
		git(dir.path(), &["checkout", "-q", "main"]);
		git(
			dir.path(),
			&["merge", "-q", "--no-ff", "-m", "merge", "feature"],
		);

		let merged = get_merged_branches(dir.path(), "main").unwrap();
		assert!(merged.contains(&"feature".to_string()), "{merged:?}");
		assert!(!merged.contains(&"main".to_string()));
	}

	#[test]
	fn an_unmerged_branch_is_not_listed_as_merged() {
		let dir = repo();
		assert_eq!(
			get_merged_branches(dir.path(), "main").unwrap(),
			Vec::<String>::new()
		);
	}

	#[test]
	fn a_failing_branch_listing_yields_no_merged_branches() {
		// Not a repository: git exits non-zero, which is treated as "nothing
		// merged" rather than an error so pruning stays a no-op.
		let dir = TempDir::new().unwrap();
		assert!(get_merged_branches(dir.path(), "main").unwrap().is_empty());
	}

	#[test]
	fn indexed_branches_are_listed_from_their_manifests() {
		let project = TempDir::new().unwrap();
		assert!(list_indexed_branches(project.path()).unwrap().is_empty());

		let branches_dir = crate::storage::get_branches_dir(project.path()).unwrap();
		for name in ["zeta", "alpha"] {
			let mut m = manifest(&[], &[], "");
			m.branch_name = name.to_string();
			save_manifest(&branches_dir.join(name), &m).unwrap();
		}
		// A directory without a manifest is skipped rather than failing the listing.
		std::fs::create_dir_all(branches_dir.join("no-manifest")).unwrap();

		let listed = list_indexed_branches(project.path()).unwrap();
		let names: Vec<_> = listed.iter().map(|m| m.branch_name.as_str()).collect();
		assert_eq!(names, vec!["alpha", "zeta"], "listing must be sorted");

		std::fs::remove_dir_all(&branches_dir).unwrap();
	}

	#[test]
	fn resolving_branch_state_reports_a_missing_index() {
		let project = TempDir::new().unwrap();
		let (branch_dir, manifest) = resolve_branch_state(project.path(), "feature").unwrap();
		assert!(branch_dir.to_string_lossy().contains("feature"));
		assert!(manifest.is_none());
	}

	#[test]
	fn resolving_branch_state_returns_the_manifest_once_indexed() {
		let project = TempDir::new().unwrap();
		let branch_dir = crate::storage::get_branch_dir(project.path(), "feature/x").unwrap();
		save_manifest(&branch_dir, &manifest(&["a.rs"], &[], "abc123")).unwrap();

		let (resolved_dir, loaded) = resolve_branch_state(project.path(), "feature/x").unwrap();
		assert_eq!(resolved_dir, branch_dir);
		let loaded = loaded.expect("manifest must be loaded");
		assert_eq!(loaded.changed_paths, vec!["a.rs".to_string()]);
		assert_eq!(loaded.base_db_commit, "abc123");

		delete_branch_index(project.path(), "feature/x").unwrap();
	}

	#[test]
	fn deleting_a_branch_index_is_idempotent() {
		let project = TempDir::new().unwrap();
		let branch_dir = crate::storage::get_branch_dir(project.path(), "feature").unwrap();
		save_manifest(&branch_dir, &manifest(&[], &[], "")).unwrap();
		assert!(branch_dir.exists());

		delete_branch_index(project.path(), "feature").unwrap();
		assert!(!branch_dir.exists());
		// Deleting again must not fail.
		delete_branch_index(project.path(), "feature").unwrap();
	}

	#[test]
	fn pruning_reports_merged_and_vanished_branches() {
		let dir = repo();
		git(dir.path(), &["checkout", "-q", "main"]);
		git(
			dir.path(),
			&["merge", "-q", "--no-ff", "-m", "merge", "feature"],
		);

		let project = TempDir::new().unwrap();
		let branches_dir = crate::storage::get_branches_dir(project.path()).unwrap();
		for name in ["feature", "vanished"] {
			let mut m = manifest(&[], &[], "");
			m.branch_name = name.to_string();
			save_manifest(&branches_dir.join(name), &m).unwrap();
		}

		let dry = prune_branches(project.path(), dir.path(), true).unwrap();
		assert!(dry.contains(&"feature".to_string()), "{dry:?}");
		assert!(dry.contains(&"vanished".to_string()), "{dry:?}");
		// A dry run leaves the indexes alone.
		assert!(branches_dir.join("feature").exists());

		let real = prune_branches(project.path(), dir.path(), false).unwrap();
		assert_eq!(real.len(), 2);
		assert!(list_indexed_branches(project.path()).unwrap().is_empty());

		std::fs::remove_dir_all(&branches_dir).ok();
	}

	#[test]
	fn pruning_keeps_a_branch_that_still_exists_and_is_unmerged() {
		let dir = repo();
		let project = TempDir::new().unwrap();
		let branches_dir = crate::storage::get_branches_dir(project.path()).unwrap();
		let mut m = manifest(&[], &[], "");
		m.branch_name = "feature".to_string();
		save_manifest(&branches_dir.join("feature"), &m).unwrap();

		assert!(prune_branches(project.path(), dir.path(), false)
			.unwrap()
			.is_empty());
		assert_eq!(list_indexed_branches(project.path()).unwrap().len(), 1);

		std::fs::remove_dir_all(&branches_dir).ok();
	}

	mod store_backed {
		use super::super::super::*;
		use super::{git, repo};
		use crate::config::Config;
		use crate::state::create_shared_state;
		use crate::store::mod_tests::test_store;
		use std::path::Path;
		use tempfile::TempDir;

		/// A repository holding nothing the indexer can index, so a baseline
		/// index walks the tree and produces zero embedding calls.
		fn repo_without_indexable_files() -> TempDir {
			let dir = TempDir::new().unwrap();
			let path = dir.path();
			git(path, &["init", "-q", "-b", "main"]);
			git(path, &["config", "user.email", "test@example.com"]);
			git(path, &["config", "user.name", "Test"]);
			std::fs::write(path.join("blob.bin"), "not indexable\n").unwrap();
			git(path, &["add", "."]);
			git(path, &["commit", "-q", "-m", "base"]);
			dir
		}

		fn state_at(dir: &Path) -> crate::state::SharedState {
			let state = create_shared_state();
			state.write().current_directory = dir.to_path_buf();
			state
		}

		#[tokio::test]
		async fn reconciling_fails_without_a_resolvable_default_branch() {
			let (_db, store) = test_store().await;
			let dir = TempDir::new().unwrap();
			git(dir.path(), &["init", "-q", "-b", "main"]);

			// A repository with no commits has no branch to diff against.
			assert!(reconcile_master_state(
				&store,
				state_at(dir.path()),
				&Config::default(),
				dir.path(),
				true,
			)
			.await
			.is_err());
		}

		#[tokio::test]
		async fn an_already_indexed_main_is_never_re_indexed() {
			let (_db, store) = test_store().await;
			let dir = repo();
			git(dir.path(), &["checkout", "-q", "main"]);
			let head = git(dir.path(), &["rev-parse", "HEAD"]);
			store.store_git_metadata(&head).await.unwrap();

			let master = reconcile_master_state(
				&store,
				state_at(dir.path()),
				&Config::default(),
				dir.path(),
				true,
			)
			.await
			.unwrap();

			assert_eq!(master.branch_name, "main");
			assert_eq!(master.db_commit.as_deref(), Some(head.as_str()));
			assert_eq!(master.local_ref_commit, head);
			// On the default branch the fork-point is HEAD itself.
			assert_eq!(master.fork_point, head);
			assert_eq!(master.remote_ref_commit, None);
			assert!(!master.local_behind_remote);
			assert!(
				master.db_resynced_to.is_none(),
				"a populated main index must not be rebuilt"
			);
		}

		#[tokio::test]
		async fn an_empty_main_index_is_baselined_and_stamped_with_head() {
			let (_db, store) = test_store().await;
			let dir = repo_without_indexable_files();
			let head = git(dir.path(), &["rev-parse", "HEAD"]);
			// Commit indexing needs embeddings; mark it up to date so the
			// baseline stays offline.
			store.store_commits_last_commit_hash(&head).await.unwrap();

			let master = reconcile_master_state(
				&store,
				state_at(dir.path()),
				&Config::default(),
				dir.path(),
				true,
			)
			.await
			.unwrap();

			assert_eq!(master.db_commit, None, "the DB was empty before the run");
			assert_eq!(
				master.db_resynced_to.as_deref(),
				Some(head.as_str()),
				"the baseline stamps the working tree HEAD"
			);
			assert_eq!(store.get_last_commit_hash().await.unwrap(), Some(head));
		}

		#[tokio::test]
		async fn a_local_default_branch_behind_its_remote_is_flagged() {
			let (_db, store) = test_store().await;
			let origin = repo();
			git(origin.path(), &["checkout", "-q", "main"]);

			let workspace = TempDir::new().unwrap();
			let clone = workspace.path().join("clone");
			git(
				origin.path(),
				&[
					"clone",
					"-q",
					origin.path().to_str().unwrap(),
					clone.to_str().unwrap(),
				],
			);
			git(&clone, &["config", "user.email", "test@example.com"]);
			git(&clone, &["config", "user.name", "Test"]);
			let cloned_head = git(&clone, &["rev-parse", "HEAD"]);
			store.store_git_metadata(&cloned_head).await.unwrap();

			// Move origin/main forward, then fetch so the clone can see it.
			std::fs::write(origin.path().join("keep.txt"), "remote work\n").unwrap();
			git(origin.path(), &["commit", "-qam", "remote work"]);
			let origin_head = git(origin.path(), &["rev-parse", "HEAD"]);
			git(&clone, &["fetch", "-q", "origin"]);

			let master =
				reconcile_master_state(&store, state_at(&clone), &Config::default(), &clone, true)
					.await
					.unwrap();

			assert_eq!(master.branch_name, "main");
			assert_eq!(master.local_ref_commit, cloned_head);
			assert_eq!(
				master.remote_ref_commit.as_deref(),
				Some(origin_head.as_str())
			);
			assert!(
				master.local_behind_remote,
				"a stale local default branch must be surfaced"
			);
			// We warn but never pull, so the delta base stays the local ref.
			assert_eq!(master.fork_point, cloned_head);
		}
	}
}
