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

//! Branch-aware delta indexing.
//!
//! Manages per-branch delta databases containing only files that differ from
//! the default branch, enabling branch-specific search without duplicating
//! the entire index.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::git_utils::GitUtils;

/// Metadata describing a branch's delta index state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchManifest {
	/// Schema version for forward compatibility.
	pub version: u32,
	/// Original git branch name (unsanitized).
	pub branch_name: String,
	/// Default branch name at time of indexing (e.g. "main").
	pub base_branch: String,
	/// Commit hash of the default branch HEAD when delta was computed.
	pub base_commit: String,
	/// Branch HEAD commit when last indexed.
	pub branch_commit: String,
	/// Files modified or added relative to the default branch.
	pub changed_paths: Vec<String>,
	/// Files deleted relative to the default branch.
	pub deleted_paths: Vec<String>,
	/// Unix timestamp of when the index was last updated.
	pub indexed_at: i64,
}

impl BranchManifest {
	/// Set of all paths overridden by this branch (changed + deleted).
	/// Used during search to filter out main results for these paths.
	pub fn overridden_paths(&self) -> HashSet<&str> {
		self.changed_paths
			.iter()
			.chain(self.deleted_paths.iter())
			.map(|s| s.as_str())
			.collect()
	}
}

/// Sanitize a branch name for use as a filesystem directory name.
/// Replaces `/` with `--` (e.g. `feature/foo` → `feature--foo`).
pub fn sanitize_branch_name(name: &str) -> String {
	name.replace('/', "--")
}

/// Reverse a sanitized branch name back to the original.
/// Only used as a hint — the canonical name is stored in the manifest.
pub fn desanitize_branch_name(sanitized: &str) -> String {
	sanitized.replace("--", "/")
}

/// Detect the current git branch.
/// Returns `None` if in detached HEAD state or not in a git repo.
pub fn get_current_branch(repo_path: &Path) -> Option<String> {
	let output = std::process::Command::new("git")
		.args(["rev-parse", "--abbrev-ref", "HEAD"])
		.current_dir(repo_path)
		.output()
		.ok()?;

	if !output.status.success() {
		return None;
	}

	let branch = String::from_utf8(output.stdout).ok()?.trim().to_string();
	if branch == "HEAD" {
		// Detached HEAD — not a branch
		return None;
	}
	Some(branch)
}

/// Determine whether the current checkout is on a non-default branch.
/// Returns `Some(branch_name)` if on a feature branch, `None` if on default or detached.
pub fn detect_branch_context(repo_path: &Path) -> Option<String> {
	let current = get_current_branch(repo_path)?;
	let default = GitUtils::get_default_branch(repo_path).ok()?;
	if current == default {
		None
	} else {
		Some(current)
	}
}

/// Get the commit hash for a given branch ref.
pub fn get_branch_commit(repo_path: &Path, branch: &str) -> Result<String> {
	let output = std::process::Command::new("git")
		.args(["rev-parse", branch])
		.current_dir(repo_path)
		.output()?;

	if !output.status.success() {
		return Err(anyhow::anyhow!(
			"Failed to resolve commit for branch '{}'",
			branch
		));
	}

	Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

/// Compute the delta between the current branch and the default branch.
/// Returns `(changed_files, deleted_files)` as relative paths.
pub fn compute_branch_delta(
	repo_path: &Path,
	default_branch: &str,
) -> Result<(Vec<String>, Vec<String>)> {
	// Get committed changes: files that differ between default branch and HEAD
	let changed = get_diff_files(repo_path, default_branch, None)?;
	let deleted = get_diff_files(repo_path, default_branch, Some("D"))?;

	// Also include uncommitted changes (staged + unstaged + untracked)
	let working_changes = get_working_tree_changes(repo_path)?;

	// Union committed delta with working tree changes
	let mut all_changed: HashSet<String> = changed.into_iter().collect();
	for path in working_changes {
		// Only add working tree changes if they're not already in the deleted set
		if !deleted.contains(&path) {
			all_changed.insert(path);
		}
	}

	// Remove deleted files from the changed set (they're tracked separately)
	let deleted_set: HashSet<&str> = deleted.iter().map(|s| s.as_str()).collect();
	let final_changed: Vec<String> = all_changed
		.into_iter()
		.filter(|p| !deleted_set.contains(p.as_str()))
		.collect();

	Ok((final_changed, deleted))
}

/// Get files that differ between default_branch and HEAD.
/// If `diff_filter` is Some, apply --diff-filter (e.g. "D" for deleted only).
fn get_diff_files(
	repo_path: &Path,
	default_branch: &str,
	diff_filter: Option<&str>,
) -> Result<Vec<String>> {
	let range = format!("{}...HEAD", default_branch);
	let mut args = vec!["diff", "--name-only"];
	if let Some(filter) = diff_filter {
		args.push("--diff-filter");
		args.push(filter);
	}
	args.push(&range);

	let output = std::process::Command::new("git")
		.args(&args)
		.current_dir(repo_path)
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);
		return Err(anyhow::anyhow!(
			"git diff failed for range '{}': {}",
			range,
			stderr
		));
	}

	let stdout = String::from_utf8(output.stdout)?;
	Ok(stdout
		.lines()
		.filter(|l| !l.trim().is_empty())
		.map(|l| l.trim().to_string())
		.collect())
}

/// Get all working tree changes (staged + unstaged + untracked).
fn get_working_tree_changes(repo_path: &Path) -> Result<Vec<String>> {
	let mut files = HashSet::new();

	// Staged changes
	let output = std::process::Command::new("git")
		.args(["diff", "--name-only", "--cached"])
		.current_dir(repo_path)
		.output()?;
	if output.status.success() {
		for line in String::from_utf8(output.stdout)?.lines() {
			if !line.trim().is_empty() {
				files.insert(line.trim().to_string());
			}
		}
	}

	// Unstaged changes
	let output = std::process::Command::new("git")
		.args(["diff", "--name-only"])
		.current_dir(repo_path)
		.output()?;
	if output.status.success() {
		for line in String::from_utf8(output.stdout)?.lines() {
			if !line.trim().is_empty() {
				files.insert(line.trim().to_string());
			}
		}
	}

	// Untracked files
	let output = std::process::Command::new("git")
		.args(["ls-files", "--others", "--exclude-standard"])
		.current_dir(repo_path)
		.output()?;
	if output.status.success() {
		for line in String::from_utf8(output.stdout)?.lines() {
			if !line.trim().is_empty() {
				files.insert(line.trim().to_string());
			}
		}
	}

	Ok(files.into_iter().collect())
}

/// Load a branch manifest from disk.
pub fn load_manifest(branch_dir: &Path) -> Result<Option<BranchManifest>> {
	let manifest_path = branch_dir.join("manifest.json");
	if !manifest_path.exists() {
		return Ok(None);
	}
	let content = std::fs::read_to_string(&manifest_path)?;
	let manifest: BranchManifest = serde_json::from_str(&content)?;
	Ok(Some(manifest))
}

/// Save a branch manifest to disk.
pub fn save_manifest(branch_dir: &Path, manifest: &BranchManifest) -> Result<()> {
	std::fs::create_dir_all(branch_dir)?;
	let manifest_path = branch_dir.join("manifest.json");
	let content = serde_json::to_string_pretty(manifest)?;
	std::fs::write(&manifest_path, content)?;
	Ok(())
}

/// List all indexed branches by reading branch directories and their manifests.
pub fn list_indexed_branches(project_path: &Path) -> Result<Vec<BranchManifest>> {
	let branches_dir = crate::storage::get_branches_dir(project_path)?;
	if !branches_dir.exists() {
		return Ok(Vec::new());
	}

	let mut manifests = Vec::new();
	for entry in std::fs::read_dir(&branches_dir)? {
		let entry = entry?;
		if entry.file_type()?.is_dir() {
			if let Ok(Some(manifest)) = load_manifest(&entry.path()) {
				manifests.push(manifest);
			}
		}
	}
	manifests.sort_by(|a, b| a.branch_name.cmp(&b.branch_name));
	Ok(manifests)
}

/// Check if a git branch exists locally.
pub fn branch_exists_in_git(repo_path: &Path, branch_name: &str) -> bool {
	let output = std::process::Command::new("git")
		.args(["rev-parse", "--verify", branch_name])
		.current_dir(repo_path)
		.output();

	matches!(output, Ok(o) if o.status.success())
}

/// Get branches that have been merged into the default branch.
pub fn get_merged_branches(repo_path: &Path, default_branch: &str) -> Result<Vec<String>> {
	let output = std::process::Command::new("git")
		.args(["branch", "--merged", default_branch])
		.current_dir(repo_path)
		.output()?;

	if !output.status.success() {
		return Ok(Vec::new());
	}

	let stdout = String::from_utf8(output.stdout)?;
	Ok(stdout
		.lines()
		.map(|l| l.trim().trim_start_matches("* ").to_string())
		.filter(|name| !name.is_empty() && name != default_branch)
		.collect())
}

/// Prune branch indexes for branches that no longer exist in git or are merged.
/// Returns list of pruned branch names.
pub fn prune_branches(project_path: &Path, repo_path: &Path, dry_run: bool) -> Result<Vec<String>> {
	let manifests = list_indexed_branches(project_path)?;
	let default_branch = GitUtils::get_default_branch(repo_path)?;
	let merged = get_merged_branches(repo_path, &default_branch)?;
	let merged_set: HashSet<&str> = merged.iter().map(|s| s.as_str()).collect();

	let mut pruned = Vec::new();

	for manifest in &manifests {
		let should_prune = !branch_exists_in_git(repo_path, &manifest.branch_name)
			|| merged_set.contains(manifest.branch_name.as_str());

		if should_prune {
			pruned.push(manifest.branch_name.clone());
			if !dry_run {
				delete_branch_index(project_path, &manifest.branch_name)?;
			}
		}
	}

	Ok(pruned)
}

/// Delete a branch's delta index entirely.
pub fn delete_branch_index(project_path: &Path, branch_name: &str) -> Result<()> {
	let branch_dir = crate::storage::get_branch_dir(project_path, branch_name)?;
	if branch_dir.exists() {
		std::fs::remove_dir_all(&branch_dir)?;
	}
	Ok(())
}

/// Resolve the branch directory path, loading manifest if available.
/// Returns `(branch_dir, Option<manifest>)`.
pub fn resolve_branch_state(
	project_path: &Path,
	branch_name: &str,
) -> Result<(PathBuf, Option<BranchManifest>)> {
	let branch_dir = crate::storage::get_branch_dir(project_path, branch_name)?;
	let manifest = if branch_dir.exists() {
		load_manifest(&branch_dir)?
	} else {
		None
	};
	Ok((branch_dir, manifest))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_sanitize_branch_name() {
		assert_eq!(sanitize_branch_name("feature/foo"), "feature--foo");
		assert_eq!(sanitize_branch_name("main"), "main");
		assert_eq!(sanitize_branch_name("fix/deep/nested"), "fix--deep--nested");
		assert_eq!(sanitize_branch_name("no-slash"), "no-slash");
	}

	#[test]
	fn test_desanitize_branch_name() {
		assert_eq!(desanitize_branch_name("feature--foo"), "feature/foo");
		assert_eq!(desanitize_branch_name("main"), "main");
		assert_eq!(
			desanitize_branch_name("fix--deep--nested"),
			"fix/deep/nested"
		);
	}

	#[test]
	fn test_sanitize_desanitize_roundtrip() {
		let names = ["feature/foo", "main", "fix/a/b/c", "simple"];
		for name in names {
			assert_eq!(desanitize_branch_name(&sanitize_branch_name(name)), name);
		}
	}

	fn make_manifest(changed: Vec<&str>, deleted: Vec<&str>) -> BranchManifest {
		BranchManifest {
			version: 1,
			branch_name: "test-branch".to_string(),
			base_branch: "main".to_string(),
			base_commit: "abc123".to_string(),
			branch_commit: "def456".to_string(),
			changed_paths: changed.into_iter().map(|s| s.to_string()).collect(),
			deleted_paths: deleted.into_iter().map(|s| s.to_string()).collect(),
			indexed_at: 1000,
		}
	}

	#[test]
	fn test_overridden_paths_combines_changed_and_deleted() {
		let manifest = make_manifest(vec!["src/a.rs", "src/b.rs"], vec!["src/old.rs"]);
		let overridden = manifest.overridden_paths();
		assert_eq!(overridden.len(), 3);
		assert!(overridden.contains("src/a.rs"));
		assert!(overridden.contains("src/b.rs"));
		assert!(overridden.contains("src/old.rs"));
	}

	#[test]
	fn test_overridden_paths_empty() {
		let manifest = make_manifest(vec![], vec![]);
		assert!(manifest.overridden_paths().is_empty());
	}

	#[test]
	fn test_manifest_save_load_roundtrip() {
		let dir = std::env::temp_dir().join("octocode_test_manifest_roundtrip");
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();

		let manifest = make_manifest(vec!["src/foo.rs"], vec!["src/bar.rs"]);
		save_manifest(&dir, &manifest).unwrap();

		let loaded = load_manifest(&dir).unwrap().unwrap();
		assert_eq!(loaded.branch_name, "test-branch");
		assert_eq!(loaded.base_branch, "main");
		assert_eq!(loaded.base_commit, "abc123");
		assert_eq!(loaded.branch_commit, "def456");
		assert_eq!(loaded.changed_paths, vec!["src/foo.rs"]);
		assert_eq!(loaded.deleted_paths, vec!["src/bar.rs"]);
		assert_eq!(loaded.version, 1);
		assert_eq!(loaded.indexed_at, 1000);

		let _ = std::fs::remove_dir_all(&dir);
	}

	#[test]
	fn test_load_manifest_missing_dir() {
		let dir = std::env::temp_dir().join("octocode_test_manifest_missing");
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();

		let loaded = load_manifest(&dir).unwrap();
		assert!(loaded.is_none());

		let _ = std::fs::remove_dir_all(&dir);
	}
}
