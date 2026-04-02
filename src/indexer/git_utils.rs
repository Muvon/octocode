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

use anyhow::Result;
use std::path::Path;
use std::process::Command;

/// Git utilities for repository management
pub struct GitUtils;

impl GitUtils {
	/// Check if current directory is a git repository root
	pub fn is_git_repo_root(path: &Path) -> bool {
		path.join(".git").exists()
	}

	/// Find git repository root from current path
	pub fn find_git_root(start_path: &Path) -> Option<std::path::PathBuf> {
		let mut current = start_path;
		loop {
			if Self::is_git_repo_root(current) {
				return Some(current.to_path_buf());
			}
			match current.parent() {
				Some(parent) => current = parent,
				None => break,
			}
		}
		None
	}

	/// Get current git commit hash
	pub fn get_current_commit_hash(repo_path: &Path) -> Result<String> {
		let output = Command::new("git")
			.arg("rev-parse")
			.arg("HEAD")
			.current_dir(repo_path)
			.output()?;

		if !output.status.success() {
			return Err(anyhow::anyhow!("Failed to get git commit hash"));
		}

		Ok(String::from_utf8(output.stdout)?.trim().to_string())
	}

	/// Get files changed between two commits (committed changes only, no unstaged)
	pub fn get_changed_files_since_commit(
		repo_path: &Path,
		since_commit: &str,
	) -> Result<Vec<String>> {
		let mut changed_files = std::collections::HashSet::new();

		// Get files changed between commits (committed changes only)
		let output = Command::new("git")
			.args(["diff", "--name-only", since_commit, "HEAD"])
			.current_dir(repo_path)
			.output()?;

		if output.status.success() {
			let stdout = String::from_utf8(output.stdout)?;
			for line in stdout.lines() {
				if !line.trim().is_empty() {
					changed_files.insert(line.trim().to_string());
				}
			}
		}

		Ok(changed_files.into_iter().collect())
	}

	/// Get only staged files (files in git index)
	pub fn get_staged_files(repo_path: &Path) -> Result<Vec<String>> {
		let mut staged_files = Vec::new();

		// Get staged files
		let output = Command::new("git")
			.args(["diff", "--cached", "--name-only"])
			.current_dir(repo_path)
			.output()?;

		if output.status.success() {
			let stdout = String::from_utf8(output.stdout)?;
			for line in stdout.lines() {
				if !line.trim().is_empty() {
					staged_files.push(line.trim().to_string());
				}
			}
		}

		Ok(staged_files)
	}

	/// Note: This is used for non-git optimization scenarios only
	pub fn get_all_changed_files(repo_path: &Path) -> Result<Vec<String>> {
		let mut changed_files = std::collections::HashSet::new();

		// Get staged files
		let output = Command::new("git")
			.args(["diff", "--cached", "--name-only"])
			.current_dir(repo_path)
			.output()?;

		if output.status.success() {
			let stdout = String::from_utf8(output.stdout)?;
			for line in stdout.lines() {
				if !line.trim().is_empty() {
					changed_files.insert(line.trim().to_string());
				}
			}
		}

		// Get unstaged files
		let output = Command::new("git")
			.args(["diff", "--name-only"])
			.current_dir(repo_path)
			.output()?;

		if output.status.success() {
			let stdout = String::from_utf8(output.stdout)?;
			for line in stdout.lines() {
				if !line.trim().is_empty() {
					changed_files.insert(line.trim().to_string());
				}
			}
		}

		// Get untracked files
		let output = Command::new("git")
			.args(["ls-files", "--others", "--exclude-standard"])
			.current_dir(repo_path)
			.output()?;

		if output.status.success() {
			let stdout = String::from_utf8(output.stdout)?;
			for line in stdout.lines() {
				if !line.trim().is_empty() {
					changed_files.insert(line.trim().to_string());
				}
			}
		}

		Ok(changed_files.into_iter().collect())
	}

	/// Detect the default branch name
	pub fn get_default_branch(repo_path: &Path) -> Result<String> {
		// Try remote HEAD first
		let output = Command::new("git")
			.args(["symbolic-ref", "refs/remotes/origin/HEAD"])
			.current_dir(repo_path)
			.output()?;

		if output.status.success() {
			let refname = String::from_utf8(output.stdout)?.trim().to_string();
			if let Some(branch) = refname.strip_prefix("refs/remotes/origin/") {
				return Ok(branch.to_string());
			}
		}

		// Fallback: check if main or master exists
		for branch in &["main", "master"] {
			let output = Command::new("git")
				.args(["rev-parse", "--verify", branch])
				.current_dir(repo_path)
				.output()?;
			if output.status.success() {
				return Ok(branch.to_string());
			}
		}

		// Last resort: current branch
		let output = Command::new("git")
			.args(["rev-parse", "--abbrev-ref", "HEAD"])
			.current_dir(repo_path)
			.output()?;

		if output.status.success() {
			return Ok(String::from_utf8(output.stdout)?.trim().to_string());
		}

		Err(anyhow::anyhow!("Could not determine default branch"))
	}

	/// Get commit log entries. If `since_commit` is Some, only returns commits after that hash.
	pub fn get_commit_log(
		repo_path: &Path,
		branch: &str,
		since_commit: Option<&str>,
	) -> Result<Vec<CommitEntry>> {
		let range = match since_commit {
			Some(hash) => format!("{}..{}", hash, branch),
			None => branch.to_string(),
		};

		let output = Command::new("git")
			.args(["log", "--format=%H|%an|%at|%B%x00", "--reverse", &range])
			.current_dir(repo_path)
			.output()?;

		if !output.status.success() {
			return Err(anyhow::anyhow!("Failed to get commit log"));
		}

		let stdout = String::from_utf8(output.stdout)?;
		let mut entries = Vec::new();

		// Records are separated by null bytes (%x00).
		// Each record: HASH|AUTHOR|TIMESTAMP|FULL_MESSAGE (may contain newlines)
		for record in stdout.split('\0') {
			let record = record.trim();
			if record.is_empty() {
				continue;
			}

			let parts: Vec<&str> = record.splitn(4, '|').collect();
			if parts.len() < 4 {
				continue;
			}

			entries.push(CommitEntry {
				hash: parts[0].to_string(),
				author: parts[1].to_string(),
				date: parts[2].parse::<i64>().unwrap_or(0),
				message: parts[3].trim().to_string(),
			});
		}

		Ok(entries)
	}

	/// Get changed file paths for a specific commit
	pub fn get_changed_files_for_commit(repo_path: &Path, hash: &str) -> Result<Vec<String>> {
		let output = Command::new("git")
			.args(["diff-tree", "--no-commit-id", "--name-only", "-r", hash])
			.current_dir(repo_path)
			.output()?;

		if !output.status.success() {
			return Ok(vec![]);
		}

		let stdout = String::from_utf8(output.stdout)?;
		Ok(stdout
			.lines()
			.filter(|l| !l.trim().is_empty())
			.map(|l| l.trim().to_string())
			.collect())
	}

	/// Get diff for a specific commit (truncated to max_chars)
	pub fn get_commit_diff(repo_path: &Path, hash: &str, max_chars: usize) -> Result<String> {
		// Check if this is the root commit (no parent)
		let parent_check = Command::new("git")
			.args(["rev-parse", &format!("{}^", hash)])
			.current_dir(repo_path)
			.output()?;

		let output = if parent_check.status.success() {
			Command::new("git")
				.args(["diff", &format!("{}^..{}", hash, hash), "--stat", "-p"])
				.current_dir(repo_path)
				.output()?
		} else {
			// Root commit
			Command::new("git")
				.args(["diff", "--root", hash, "--stat", "-p"])
				.current_dir(repo_path)
				.output()?
		};

		if !output.status.success() {
			return Ok(String::new());
		}

		let diff = String::from_utf8_lossy(&output.stdout).to_string();
		if diff.len() > max_chars {
			Ok(diff[..max_chars].to_string())
		} else {
			Ok(diff)
		}
	}
}

/// Parsed commit entry from git log
#[derive(Debug, Clone)]
pub struct CommitEntry {
	pub hash: String,
	pub author: String,
	pub date: i64,
	pub message: String,
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::Path;

	#[test]
	fn test_is_git_repo_root() {
		// Current project should be a git repo
		let path = Path::new(env!("CARGO_MANIFEST_DIR"));
		assert!(GitUtils::is_git_repo_root(path));
	}

	#[test]
	fn test_find_git_root_from_subdir() {
		let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
		let src = manifest.join("src");
		let result = GitUtils::find_git_root(&src);
		assert!(result.is_some());
		assert_eq!(result.unwrap(), manifest);
	}

	#[test]
	fn test_get_default_branch() {
		let path = Path::new(env!("CARGO_MANIFEST_DIR"));
		let result = GitUtils::get_default_branch(path);
		// Should succeed — returns some branch name
		assert!(result.is_ok());
		let branch = result.unwrap();
		assert!(!branch.is_empty());
	}

	#[test]
	fn test_get_commit_log() {
		let path = Path::new(env!("CARGO_MANIFEST_DIR"));
		let branch = GitUtils::get_default_branch(path).unwrap();
		let commits = GitUtils::get_commit_log(path, &branch, None).unwrap();
		// The project has commits
		assert!(!commits.is_empty());
		// Each entry should have non-empty fields
		let first = &commits[0];
		assert!(!first.hash.is_empty());
		assert!(!first.author.is_empty());
		assert!(first.date > 0);
		assert!(!first.message.is_empty());
	}

	#[test]
	fn test_get_changed_files_for_commit() {
		let path = Path::new(env!("CARGO_MANIFEST_DIR"));
		let hash = GitUtils::get_current_commit_hash(path).unwrap();
		let files = GitUtils::get_changed_files_for_commit(path, &hash).unwrap();
		// Current commit should have changed some files
		assert!(!files.is_empty());
	}
}
