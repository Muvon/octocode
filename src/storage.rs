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

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

/// Get the system-wide storage directory for Octocode
/// Following XDG Base Directory specification on Unix-like systems
/// and proper conventions on other systems
pub fn get_system_storage_dir() -> Result<PathBuf> {
	let base_dir = if cfg!(target_os = "macos") {
		// macOS: ~/.local/share/octocode
		dirs::home_dir()
			.ok_or_else(|| anyhow::anyhow!("Unable to determine home directory"))?
			.join(".local")
			.join("share")
			.join("octocode")
	} else if cfg!(target_os = "windows") {
		// Windows: %APPDATA%/octocode
		dirs::data_dir()
			.ok_or_else(|| anyhow::anyhow!("Unable to determine data directory"))?
			.join("octocode")
	} else {
		// Linux and other Unix-like: ~/.local/share/octocode or $XDG_DATA_HOME/octocode
		if let Ok(xdg_data_home) = std::env::var("XDG_DATA_HOME") {
			PathBuf::from(xdg_data_home).join("octocode")
		} else {
			dirs::home_dir()
				.ok_or_else(|| anyhow::anyhow!("Unable to determine home directory"))?
				.join(".local")
				.join("share")
				.join("octocode")
		}
	};

	// Create the directory if it doesn't exist
	if !base_dir.exists() {
		fs::create_dir_all(&base_dir)?;
	}

	Ok(base_dir)
}

/// Get the project identifier for a given directory.
/// Delegates to octolib::utils::path_to_id — single canonical implementation.
pub fn get_project_identifier(project_path: &Path) -> Result<String> {
	Ok(octolib::utils::path_to_id(project_path))
}
/// Get the storage path for a specific project
pub fn get_project_storage_path(project_path: &Path) -> Result<PathBuf> {
	let system_dir = get_system_storage_dir()?;
	let project_id = get_project_identifier(project_path)?;

	Ok(system_dir.join(project_id))
}

/// Get the database path for a specific project
pub fn get_project_database_path(project_path: &Path) -> Result<PathBuf> {
	let project_storage = get_project_storage_path(project_path)?;
	Ok(project_storage.join("storage"))
}

/// Get the directory containing all branch delta indexes for a project.
pub fn get_branches_dir(project_path: &Path) -> Result<PathBuf> {
	let project_storage = get_project_storage_path(project_path)?;
	Ok(project_storage.join("branches"))
}

/// Get the directory for a specific branch's delta index.
/// Branch name is sanitized for filesystem safety (`/` → `--`).
pub fn get_branch_dir(project_path: &Path, branch_name: &str) -> Result<PathBuf> {
	let branches_dir = get_branches_dir(project_path)?;
	let sanitized = crate::indexer::branch::sanitize_branch_name(branch_name);
	Ok(branches_dir.join(sanitized))
}

/// Get the database path for a specific branch's delta index.
pub fn get_branch_database_path(project_path: &Path, branch_name: &str) -> Result<PathBuf> {
	let branch_dir = get_branch_dir(project_path, branch_name)?;
	Ok(branch_dir.join("storage"))
}

/// Get the config path for a specific project (local to project)
/// Config remains local to projects for project-specific settings
pub fn get_project_config_path(project_path: &Path) -> Result<PathBuf> {
	Ok(project_path.join(".octocode"))
}

/// Get the system-wide FastEmbed cache directory
/// Stored directly under ~/.local/share/octocode/fastembed/ on all systems
pub fn get_fastembed_cache_dir() -> Result<PathBuf> {
	let cache_dir = get_system_storage_dir()?.join("fastembed");

	// Create the directory if it doesn't exist
	if !cache_dir.exists() {
		fs::create_dir_all(&cache_dir)?;
	}

	Ok(cache_dir)
}

/// Get the system-wide SentenceTransformer cache directory
/// Stored directly under ~/.local/share/octocode/sentencetransformer/ on all systems
pub fn get_huggingface_cache_dir() -> Result<PathBuf> {
	let cache_dir = get_system_storage_dir()?.join("sentencetransformer");

	// Create the directory if it doesn't exist
	if !cache_dir.exists() {
		fs::create_dir_all(&cache_dir)?;
	}

	Ok(cache_dir)
}

/// Ensure the project storage directory exists
pub fn ensure_project_storage_exists(project_path: &Path) -> Result<PathBuf> {
	let storage_path = get_project_storage_path(project_path)?;

	if !storage_path.exists() {
		fs::create_dir_all(&storage_path)?;
	}

	Ok(storage_path)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::env;

	#[test]
	fn test_project_identifier() {
		let temp_dir = env::temp_dir().join("test_octocode");
		let _ = fs::create_dir_all(&temp_dir);

		// Should not panic and should return a consistent hash
		let id1 = get_project_identifier(&temp_dir).expect("Should get project identifier");
		let id2 =
			get_project_identifier(&temp_dir).expect("Should get consistent project identifier");

		assert_eq!(id1, id2);
		assert_eq!(id1.len(), 16); // Should be 16 characters

		let _ = fs::remove_dir_all(&temp_dir);
	}

	#[test]
	fn test_system_storage_dir() {
		let storage_dir = get_system_storage_dir().expect("Should get system storage directory");

		// Should contain "octocode" in the path
		assert!(storage_dir.to_string_lossy().contains("octocode"));

		// Should be an absolute path
		assert!(storage_dir.is_absolute());
	}

	#[test]
	fn test_fastembed_cache_dir() {
		let fastembed_cache =
			get_fastembed_cache_dir().expect("Should get fastembed cache directory");

		// Should contain "octocode" and "fastembed" in the path
		assert!(fastembed_cache.to_string_lossy().contains("octocode"));
		assert!(fastembed_cache.to_string_lossy().contains("fastembed"));

		// Should be an absolute path
		assert!(fastembed_cache.is_absolute());

		// Should be a direct subdirectory of system storage directory
		let storage_dir = get_system_storage_dir().expect("Should get system storage directory");
		assert!(fastembed_cache.starts_with(&storage_dir));
		assert_eq!(fastembed_cache, storage_dir.join("fastembed"));
	}

	#[test]
	fn test_sentencetransformer_cache_dir() {
		let st_cache = get_huggingface_cache_dir().expect("Should get huggingface cache directory");

		// Should contain "octocode" and "sentencetransformer" in the path
		assert!(st_cache.to_string_lossy().contains("octocode"));
		assert!(st_cache.to_string_lossy().contains("sentencetransformer"));

		// Should be an absolute path
		assert!(st_cache.is_absolute());

		// Should be a direct subdirectory of system storage directory
		let storage_dir = get_system_storage_dir().expect("Should get system storage directory");
		assert!(st_cache.starts_with(&storage_dir));
		assert_eq!(st_cache, storage_dir.join("sentencetransformer"));
	}
}
