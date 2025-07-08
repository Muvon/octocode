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

//! Cross-platform path normalization utilities
//!
//! This module provides consistent path handling across different operating systems,
//! ensuring that path comparisons and operations work reliably on Windows, macOS, and Linux.

use std::path::{Path, PathBuf};

/// Cross-platform path normalization utilities
pub struct PathNormalizer;

impl PathNormalizer {
	/// Normalize path separators for cross-platform string comparison
	///
	/// Converts all backslashes to forward slashes for consistent comparison.
	/// This is the primary function for normalizing paths in string form.
	///
	/// # Examples
	/// ```
	/// use octocode::utils::path::PathNormalizer;
	///
	/// assert_eq!(PathNormalizer::normalize_separators("src\\main.rs"), "src/main.rs");
	/// assert_eq!(PathNormalizer::normalize_separators("src/main.rs"), "src/main.rs");
	/// assert_eq!(PathNormalizer::normalize_separators("src\\utils/helper.rs"), "src/utils/helper.rs");
	/// ```
	pub fn normalize_separators(path: &str) -> String {
		path.replace('\\', "/")
	}

	/// Compare two paths for equality with cross-platform normalization
	///
	/// This is a convenience function that normalizes both paths before comparison.
	/// Use this when you need to check if two path strings refer to the same location
	/// regardless of the separator style used.
	///
	/// # Examples
	/// ```
	/// use octocode::utils::path::PathNormalizer;
	///
	/// assert!(PathNormalizer::paths_equal("src\\main.rs", "src/main.rs"));
	/// assert!(PathNormalizer::paths_equal("src/utils/helper.rs", "src\\utils\\helper.rs"));
	/// assert!(!PathNormalizer::paths_equal("src/main.rs", "lib/main.rs"));
	/// ```
	pub fn paths_equal(path1: &str, path2: &str) -> bool {
		Self::normalize_separators(path1) == Self::normalize_separators(path2)
	}

	/// Find a path in a collection using cross-platform comparison
	///
	/// Searches for a target path in a collection of paths, using normalized
	/// comparison to handle different separator styles.
	///
	/// # Examples
	/// ```
	/// use octocode::utils::path::PathNormalizer;
	///
	/// let files = vec!["src/main.rs", "src\\utils\\helper.rs", "lib/config.rs"];
	/// assert_eq!(
	///     PathNormalizer::find_path_in_collection("src\\main.rs", &files),
	///     Some("src/main.rs")
	/// );
	/// ```
	pub fn find_path_in_collection<'a>(
		target: &str,
		paths: &'a [impl AsRef<str>],
	) -> Option<&'a str> {
		let normalized_target = Self::normalize_separators(target);

		for path in paths {
			let path_str = path.as_ref();
			if Self::normalize_separators(path_str) == normalized_target {
				return Some(path_str);
			}
		}

		None
	}

	/// Normalize a path using std::path operations when possible
	///
	/// This function attempts to use the standard library's path operations
	/// for normalization (resolving .. and . components), falling back to
	/// string-based normalization if canonicalization fails.
	///
	/// # Examples
	/// ```
	/// use octocode::utils::path::PathNormalizer;
	///
	/// // For existing files, this will resolve .. components
	/// let normalized = PathNormalizer::normalize_path("src/../lib/main.rs");
	/// // For non-existent files, this will still normalize separators
	/// assert_eq!(PathNormalizer::normalize_path("src\\main.rs"), "src/main.rs");
	/// ```
	pub fn normalize_path(path: &str) -> String {
		let path_buf = Path::new(path);

		// Try to canonicalize first (resolves .. and . components)
		if let Ok(canonical) = path_buf.canonicalize() {
			// If we can canonicalize, try to make it relative to current dir
			if let Ok(current_dir) = std::env::current_dir() {
				if let Ok(relative) = canonical.strip_prefix(&current_dir) {
					return Self::normalize_separators(&relative.to_string_lossy());
				}
			}
			return Self::normalize_separators(&canonical.to_string_lossy());
		}

		// Fallback: manually resolve .. components and normalize separators
		let mut components = Vec::new();
		for component in path_buf.components() {
			match component {
				std::path::Component::ParentDir => {
					components.pop();
				}
				std::path::Component::CurDir => {
					// Skip current directory references
				}
				_ => {
					components.push(component.as_os_str().to_string_lossy().to_string());
				}
			}
		}

		let normalized: PathBuf = components.into_iter().collect();
		Self::normalize_separators(&normalized.to_string_lossy())
	}

	/// Convert a path to a relative string for display purposes
	///
	/// This function ensures paths are displayed consistently and never shows
	/// absolute paths to users when possible.
	pub fn to_relative_string(path: &Path, current_dir: &Path) -> String {
		let relative = path.strip_prefix(current_dir).unwrap_or(path);
		Self::normalize_separators(&relative.to_string_lossy())
	}

	/// Create a path suitable for display, ensuring it never shows absolute paths
	pub fn for_display(path: &Path, current_dir: &Path) -> String {
		let relative = Self::to_relative_string(path, current_dir);

		// Ensure we never display absolute paths to users
		if relative.starts_with('/') || relative.contains(":\\") {
			// If somehow we still have an absolute path, extract just the filename
			path.file_name()
				.and_then(|name| name.to_str())
				.unwrap_or("unknown")
				.to_string()
		} else {
			relative
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_normalize_separators() {
		// Test Windows to Unix normalization
		assert_eq!(
			PathNormalizer::normalize_separators("src\\main.rs"),
			"src/main.rs"
		);
		assert_eq!(
			PathNormalizer::normalize_separators("src\\utils\\helper.rs"),
			"src/utils/helper.rs"
		);

		// Test Unix paths remain unchanged
		assert_eq!(
			PathNormalizer::normalize_separators("src/main.rs"),
			"src/main.rs"
		);
		assert_eq!(
			PathNormalizer::normalize_separators("src/utils/helper.rs"),
			"src/utils/helper.rs"
		);

		// Test mixed separators
		assert_eq!(
			PathNormalizer::normalize_separators("src\\utils/helper.rs"),
			"src/utils/helper.rs"
		);
		assert_eq!(
			PathNormalizer::normalize_separators("src/utils\\helper.rs"),
			"src/utils/helper.rs"
		);

		// Test edge cases
		assert_eq!(PathNormalizer::normalize_separators(""), "");
		assert_eq!(PathNormalizer::normalize_separators("\\"), "/");
		assert_eq!(PathNormalizer::normalize_separators("/"), "/");
	}

	#[test]
	fn test_paths_equal() {
		// Test cross-platform equality
		assert!(PathNormalizer::paths_equal("src\\main.rs", "src/main.rs"));
		assert!(PathNormalizer::paths_equal(
			"src/utils/helper.rs",
			"src\\utils\\helper.rs"
		));
		assert!(PathNormalizer::paths_equal("src/main.rs", "src/main.rs"));

		// Test inequality
		assert!(!PathNormalizer::paths_equal("src/main.rs", "lib/main.rs"));
		assert!(!PathNormalizer::paths_equal("src\\main.rs", "src\\lib.rs"));
	}

	#[test]
	fn test_find_path_in_collection() {
		let files = vec!["src/main.rs", "src\\utils\\helper.rs", "lib/config.rs"];

		// Test finding with different separator styles
		assert_eq!(
			PathNormalizer::find_path_in_collection("src\\main.rs", &files),
			Some("src/main.rs")
		);
		assert_eq!(
			PathNormalizer::find_path_in_collection("src/utils/helper.rs", &files),
			Some("src\\utils\\helper.rs")
		);

		// Test not found
		assert_eq!(
			PathNormalizer::find_path_in_collection("nonexistent.rs", &files),
			None
		);
	}

	#[test]
	fn test_normalize_path() {
		// Test basic separator normalization
		assert_eq!(
			PathNormalizer::normalize_path("src\\main.rs"),
			"src/main.rs"
		);

		// Test that it handles various path formats
		let result = PathNormalizer::normalize_path("src/utils/../main.rs");
		// The exact result depends on whether the path exists, but it should be normalized
		assert!(!result.contains("\\"));
	}
}
