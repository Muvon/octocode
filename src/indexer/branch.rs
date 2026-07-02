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
///
/// Version history:
/// - v1: changed_paths/deleted_paths + base_commit (default-branch tip)
/// - v2: adds fork_point, base_db_commit, remote_base_observed. The branch
///   delta is now diffed against `fork_point` (the merge-base) instead of the
///   default-branch tip, so commits the branch is "missing" no longer leak
///   into the delta. `base_db_commit` records which commit the main index was
///   at when we built this branch — search refuses to load the branch DB on
///   top of a main DB that has since moved, because the override semantics
///   would be incoherent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchManifest {
	/// Schema version for forward compatibility.
	pub version: u32,
	/// Original git branch name (unsanitized).
	pub branch_name: String,
	/// Default branch name at time of indexing (e.g. "main").
	pub base_branch: String,
	/// Commit hash of the default branch HEAD when delta was computed.
	/// Kept for backwards compatibility with v1 manifests; new code should
	/// prefer `fork_point` for the diff base and `base_db_commit` for the
	/// search-time coherence check.
	pub base_commit: String,
	/// Branch HEAD commit when last indexed.
	pub branch_commit: String,
	/// Files modified or added relative to the default branch.
	pub changed_paths: Vec<String>,
	/// Files deleted relative to the default branch.
	pub deleted_paths: Vec<String>,
	/// Unix timestamp of when the index was last updated.
	pub indexed_at: i64,
	/// Merge-base between the default branch and this branch's HEAD — the
	/// real fork point. Delta is diffed against this commit so the branch
	/// DB only contains files that actually changed on the branch.
	#[serde(default)]
	pub fork_point: String,
	/// The commit hash the *main index* was at when this branch delta was
	/// built. Search rejects this branch DB if the main DB has since moved
	/// to a different commit — the override mapping would be wrong.
	#[serde(default)]
	pub base_db_commit: String,
	/// `origin/<default_branch>` commit observed at index time, if a remote
	/// existed. Empty when there's no remote or the fetch ref was missing.
	/// Diagnostic only — does not gate anything.
	#[serde(default)]
	pub remote_base_observed: String,
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

/// Snapshot of "where the master/main branch is" across the three places that
/// can disagree: the index database, the local git ref, and the remote ref.
/// Produced by [`reconcile_master_state`] and passed into branch indexing so
/// the delta gets computed against the right commit and the manifest captures
/// the state we relied on.
#[derive(Debug, Clone)]
pub struct MasterState {
	/// Default branch name as detected from `origin/HEAD` (or fallback).
	pub branch_name: String,
	/// Last commit the main index was updated to. `None` if the main DB has
	/// never been indexed (fresh setup).
	pub db_commit: Option<String>,
	/// Current commit of the local `<default_branch>` ref.
	pub local_ref_commit: String,
	/// Current commit of `origin/<default_branch>`, if a remote ref exists.
	pub remote_ref_commit: Option<String>,
	/// Merge-base between `<default_branch>` (local ref) and `HEAD` — the
	/// commit where the current branch actually forked off main.
	pub fork_point: String,
	/// True when the local default-branch ref is behind the remote one. The
	/// caller is expected to log a prominent warning; we keep indexing
	/// against the local ref because pulling is a workspace action that
	/// belongs to the user.
	pub local_behind_remote: bool,
	/// `Some(n)` if the main DB was re-indexed by reconcile because the DB
	/// commit didn't match the local ref. `None` when no master reindex was
	/// triggered (DB already current, or no main DB exists yet).
	pub db_resynced_to: Option<String>,
}

/// Compare and (when needed) converge the three views of the main branch
/// before branch-delta indexing runs. Steps:
///
/// 1. Read the default branch name, local ref, and (best-effort) remote ref.
/// 2. Compute the fork-point between local default branch and `HEAD`.
/// 3. Read the main DB's last indexed commit.
/// 4. If DB commit != local ref commit: re-index the main store so the branch
///    DB lays on top of a coherent main snapshot. This is the "always index up
///    to master before branching" guarantee — silent skip is what got the
///    `conversation_compression/` files lost in the first place.
/// 5. Surface (don't auto-fix) `local < remote` divergence. Pulling is a
///    user workspace action; we won't run it implicitly.
///
/// Pass `main_store` so we can resynchronize when needed.
pub async fn reconcile_master_state(
	main_store: &crate::store::Store,
	state: crate::state::SharedState,
	config: &crate::config::Config,
	git_repo_root: &Path,
	quiet: bool,
) -> Result<MasterState> {
	let branch_name = GitUtils::get_default_branch(git_repo_root)?;

	let local_ref_commit = GitUtils::resolve_ref(git_repo_root, &branch_name).ok_or_else(|| {
		anyhow::anyhow!(
			"Default branch '{}' could not be resolved locally. Check `git rev-parse {}`.",
			branch_name,
			branch_name
		)
	})?;

	let remote_ref_name = format!("origin/{}", branch_name);
	let remote_ref_commit = GitUtils::resolve_ref(git_repo_root, &remote_ref_name);

	let fork_point = GitUtils::merge_base(git_repo_root, &branch_name, "HEAD").map_err(|e| {
		anyhow::anyhow!(
			"Failed to compute fork-point between '{}' and HEAD: {}. \
			 If the branches have no common ancestor (orphan branch), branch-delta \
			 indexing cannot proceed — checkout the default branch and re-run.",
			branch_name,
			e
		)
	})?;

	let local_behind_remote = match &remote_ref_commit {
		Some(remote) if remote != &local_ref_commit => {
			// `local..remote` counts commits the remote has that local doesn't.
			match GitUtils::commits_ahead(git_repo_root, &branch_name, &remote_ref_name) {
				Ok(n) if n > 0 => {
					if !quiet {
						eprintln!(
							"⚠️  Local '{}' is behind '{}' by {} commit{} — branch delta \
							 will be computed against the local (stale) base. Run `git pull` \
							 on '{}' and re-index to use the latest remote tip.",
							branch_name,
							remote_ref_name,
							n,
							if n == 1 { "" } else { "s" },
							branch_name,
						);
					}
					true
				}
				_ => false,
			}
		}
		_ => false,
	};

	// Propagate a real DB error instead of collapsing it into "no commit
	// recorded yet", which would silently trigger a full reindex/resync.
	let db_commit = main_store.get_last_commit_hash().await?;

	let db_resynced_to = match &db_commit {
		Some(db) if db == &local_ref_commit => None,
		_ => {
			if !quiet {
				match &db_commit {
					Some(prev) => println!(
						"🔄 Main index at {} but local '{}' is at {} — resyncing main \
						 index before computing branch delta.",
						&prev[..prev.len().min(8)],
						branch_name,
						&local_ref_commit[..local_ref_commit.len().min(8)],
					),
					None => println!(
						"🔄 Main index empty — running a full main index before computing \
						 branch delta."
					),
				}
			}
			super::index_files_with_quiet(
				main_store,
				state.clone(),
				config,
				Some(git_repo_root),
				quiet,
			)
			.await
			.map_err(|e| {
				anyhow::anyhow!(
					"Main index resync failed while preparing branch delta for '{}': {}. \
						 Branch indexing aborted — main DB is left in its previous state.",
					branch_name,
					e
				)
			})?;
			main_store.flush().await?;
			Some(local_ref_commit.clone())
		}
	};

	Ok(MasterState {
		branch_name,
		db_commit,
		local_ref_commit,
		remote_ref_commit,
		fork_point,
		local_behind_remote,
		db_resynced_to,
	})
}

/// Compute the delta between the current branch and a specific base commit
/// (typically the fork-point returned by [`reconcile_master_state`]).
/// Returns `(changed_files, deleted_files)` as relative paths.
pub fn compute_branch_delta(
	repo_path: &Path,
	base_ref: &str,
) -> Result<(Vec<String>, Vec<String>)> {
	// Get committed changes: files that differ between base ref and HEAD
	let changed = get_diff_files(repo_path, base_ref, None)?;
	let deleted = get_diff_files(repo_path, base_ref, Some("D"))?;

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

/// Decide whether a branch manifest is still safe to overlay onto the given
/// `main_commit`. v2 manifests must match exactly; v1 manifests (no
/// `base_db_commit`) are grandfathered so existing indexes keep working until
/// the next re-index upgrades them. Returns `Ok(())` when overlay is safe.
pub fn manifest_is_coherent_with(manifest: &BranchManifest, main_commit: Option<&str>) -> bool {
	match (main_commit, manifest.base_db_commit.as_str()) {
		(Some(main), recorded) if !recorded.is_empty() && main == recorded => true,
		(_, "") => true, // legacy v1 manifest, grandfathered
		_ => false,
	}
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
			version: 2,
			branch_name: "test-branch".to_string(),
			base_branch: "main".to_string(),
			base_commit: "abc123".to_string(),
			branch_commit: "def456".to_string(),
			changed_paths: changed.into_iter().map(|s| s.to_string()).collect(),
			deleted_paths: deleted.into_iter().map(|s| s.to_string()).collect(),
			indexed_at: 1000,
			fork_point: "abc123".to_string(),
			base_db_commit: "abc123".to_string(),
			remote_base_observed: String::new(),
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
	fn test_coherent_matching_commit() {
		let m = make_manifest(vec!["a.rs"], vec![]);
		// base_db_commit == "abc123" per make_manifest
		assert!(manifest_is_coherent_with(&m, Some("abc123")));
	}

	#[test]
	fn test_incoherent_when_main_moved() {
		let m = make_manifest(vec!["a.rs"], vec![]);
		assert!(!manifest_is_coherent_with(&m, Some("abc999")));
	}

	#[test]
	fn test_incoherent_when_main_missing() {
		let m = make_manifest(vec!["a.rs"], vec![]);
		assert!(!manifest_is_coherent_with(&m, None));
	}

	#[test]
	fn test_legacy_v1_manifest_grandfathered() {
		// Manifests written before v2 have empty base_db_commit; coherence
		// check must trust them rather than break existing indexes.
		let mut m = make_manifest(vec!["a.rs"], vec![]);
		m.base_db_commit = String::new();
		assert!(manifest_is_coherent_with(&m, Some("abc999")));
		assert!(manifest_is_coherent_with(&m, None));
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
		assert_eq!(loaded.version, 2);
		assert_eq!(loaded.indexed_at, 1000);
		assert_eq!(loaded.fork_point, "abc123");
		assert_eq!(loaded.base_db_commit, "abc123");
		assert_eq!(loaded.remote_base_observed, "");

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
