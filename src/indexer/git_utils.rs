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
}
