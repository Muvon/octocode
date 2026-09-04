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
	use crate::indexer::branch::*;
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
	fn overridden_paths_covers_changed_and_deleted_files() {
		let manifest = manifest(&["a.rs", "b.rs"], &["c.rs"], "");
		let overridden = manifest.overridden_paths();
		assert_eq!(overridden.len(), 3);
		assert!(overridden.contains("a.rs"));
		assert!(overridden.contains("c.rs"));
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
		assert_eq!(loaded.branch_name, "feature");
		assert_eq!(loaded.changed_paths, vec!["a.rs".to_string()]);
		assert_eq!(loaded.base_db_commit, "abc123");
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
}
