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

//! Shared utilities for import resolution across all languages
//!
//! This module provides common file-finding and path resolution utilities
//! that can be used by language-specific import resolvers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::utils::path::PathNormalizer;

/// File registry for efficient file lookup by extension and pattern
pub struct FileRegistry {
	/// Files grouped by extension for quick lookup
	files_by_extension: HashMap<String, Vec<String>>,
	/// All files for general searches
	all_files: Vec<String>,
}

impl FileRegistry {
	/// Create a new file registry from a list of file paths
	pub fn new(all_files: &[String]) -> Self {
		let mut files_by_extension = HashMap::new();

		for file_path in all_files {
			if let Some(extension) = Path::new(file_path).extension() {
				if let Some(ext_str) = extension.to_str() {
					files_by_extension
						.entry(ext_str.to_lowercase())
						.or_insert_with(Vec::new)
						.push(file_path.clone());
				}
			}
		}

		Self {
			files_by_extension,
			all_files: all_files.to_vec(),
		}
	}

	/// Get all files with specific extensions
	pub fn get_files_with_extensions(&self, extensions: &[&str]) -> Vec<String> {
		let mut result = Vec::new();
		for ext in extensions {
			if let Some(files) = self.files_by_extension.get(&ext.to_lowercase()) {
				result.extend(files.clone());
			}
		}
		result
	}

	/// Find a file with multiple possible extensions
	pub fn find_file_with_extensions(
		&self,
		base_path: &Path,
		extensions: &[&str],
	) -> Option<String> {
		for ext in extensions {
			let file_path = if ext.is_empty() {
				base_path.to_path_buf()
			} else {
				PathBuf::from(format!("{}.{}", base_path.to_string_lossy(), ext))
			};

			if let Some(found) = self.find_exact_file(&file_path.to_string_lossy()) {
				return Some(found);
			}
		}
		None
	}

	/// Find exact file match with cross-platform path comparison
	pub fn find_exact_file(&self, target_path: &str) -> Option<String> {
		// Try direct canonicalize first (most accurate for real files)
		if let Ok(canonical_target) = std::path::Path::new(target_path).canonicalize() {
			let canonical_str = canonical_target.to_string_lossy().to_string();
			for file_path in &self.all_files {
				if let Ok(canonical_file) = std::path::Path::new(file_path).canonicalize() {
					let canonical_file_str = canonical_file.to_string_lossy().to_string();
					if canonical_str == canonical_file_str {
						return Some(file_path.clone());
					}
				}
			}
		}

		// Fallback: normalize separators for cross-platform string comparison
		// Fallback: normalize separators for cross-platform string comparison
		if let Some(found) = PathNormalizer::find_path_in_collection(target_path, &self.all_files) {
			return Some(found.to_string());
		}

		None
	}

	/// Find files matching a pattern
	pub fn find_files_by_pattern(&self, pattern: &str) -> Vec<String> {
		self.all_files
			.iter()
			.filter(|file| file.contains(pattern))
			.cloned()
			.collect()
	}

	/// Get all files
	pub fn get_all_files(&self) -> &[String] {
		&self.all_files
	}
}

/// Find project root by looking for common project indicators
pub fn find_project_root(source_file: &str) -> Option<String> {
	let source_path = Path::new(source_file);
	let mut current_dir = source_path.parent()?;

	loop {
		// Look for common project root indicators
		let indicators = [
			"Cargo.toml",
			"package.json",
			"setup.py",
			"go.mod",
			"composer.json",
			"pyproject.toml",
			"pom.xml",
			"build.gradle",
			".git",
		];

		for indicator in &indicators {
			let indicator_path = current_dir.join(indicator);
			if indicator_path.exists() {
				return Some(current_dir.to_string_lossy().to_string());
			}
		}

		// Move up one directory
		if let Some(parent) = current_dir.parent() {
			current_dir = parent;
		} else {
			break;
		}
	}

	None
}

/// Normalize a file path for consistent comparison
pub fn normalize_path(path: &str) -> String {
	let path_buf = Path::new(path);

	// Try to canonicalize first (resolves .. and . components)
	if let Ok(canonical) = path_buf.canonicalize() {
		// If we can canonicalize, try to make it relative to current dir
		if let Ok(current_dir) = std::env::current_dir() {
			if let Ok(relative) = canonical.strip_prefix(&current_dir) {
				return relative.to_string_lossy().to_string();
			}
		}
		return canonical.to_string_lossy().to_string();
	}

	// If canonicalize fails (file doesn't exist), manually resolve .. components
	let mut components = Vec::new();
	for component in path_buf.components() {
		match component {
			std::path::Component::ParentDir => {
				// Pop the last component if possible
				if !components.is_empty() {
					components.pop();
				}
			}
			std::path::Component::CurDir => {
				// Skip current directory components
			}
			_ => {
				components.push(component);
			}
		}
	}

	// Rebuild the path
	let normalized: PathBuf = components.into_iter().collect();
	normalized.to_string_lossy().to_string()
}

/// Detect language from file path extension
pub fn detect_language_from_path(file_path: &str) -> Option<String> {
	let path = Path::new(file_path);
	let extension = path.extension()?.to_str()?;

	match extension {
		"rs" => Some("rust".to_string()),
		"js" | "mjs" => Some("javascript".to_string()),
		"ts" | "tsx" => Some("typescript".to_string()),
		"py" => Some("python".to_string()),
		"go" => Some("go".to_string()),
		"php" => Some("php".to_string()),
		"cpp" | "cc" | "cxx" | "c++" => Some("cpp".to_string()),
		"c" | "h" => Some("c".to_string()),
		"rb" => Some("ruby".to_string()),
		"sh" | "bash" => Some("bash".to_string()),
		"json" => Some("json".to_string()),
		"css" | "scss" | "sass" => Some("css".to_string()),
		"md" | "markdown" => Some("markdown".to_string()),
		"svelte" => Some("svelte".to_string()),
		_ => None,
	}
}

/// Helper to resolve relative paths from a source directory
pub fn resolve_relative_path(source_file: &str, relative_path: &str) -> Option<PathBuf> {
	let source_path = Path::new(source_file);
	let source_dir = source_path.parent()?;
	let resolved = source_dir.join(relative_path);

	// Normalize the path to resolve ".." components
	// This converts "src/../lib.rs" to "lib.rs"
	let normalized_str = normalize_path(&resolved.to_string_lossy());
	Some(PathBuf::from(normalized_str))
}

/// Helper to find files in a specific directory
pub fn find_files_in_directory(
	directory: &Path,
	registry: &FileRegistry,
	extensions: &[&str],
) -> Vec<String> {
	let dir_str = directory.to_string_lossy();
	registry
		.get_files_with_extensions(extensions)
		.into_iter()
		.filter(|file| file.starts_with(&*dir_str))
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_file_registry_creation() {
		let files = vec![
			"src/main.rs".to_string(),
			"src/lib.rs".to_string(),
			"package.json".to_string(),
			"index.js".to_string(),
		];

		let registry = FileRegistry::new(&files);
		let rust_files = registry.get_files_with_extensions(&["rs"]);
		assert_eq!(rust_files.len(), 2);
		assert!(rust_files.contains(&"src/main.rs".to_string()));
		assert!(rust_files.contains(&"src/lib.rs".to_string()));
	}

	#[test]
	fn test_find_file_with_extensions() {
		let files = vec!["src/utils.rs".to_string(), "src/utils.js".to_string()];

		let registry = FileRegistry::new(&files);
		let result = registry.find_file_with_extensions(Path::new("src/utils"), &["rs", "js"]);
		assert!(result.is_some());
		let result_path = result.unwrap();
		assert!(result_path.ends_with(".rs") || result_path.ends_with(".js"));
	}

	#[test]
	fn test_detect_language_from_path() {
		assert_eq!(
			detect_language_from_path("main.rs"),
			Some("rust".to_string())
		);
		assert_eq!(
			detect_language_from_path("index.js"),
			Some("javascript".to_string())
		);
		assert_eq!(
			detect_language_from_path("app.py"),
			Some("python".to_string())
		);
		assert_eq!(detect_language_from_path("unknown.xyz"), None);
	}

	#[test]
	fn test_resolve_relative_path() {
		let result = resolve_relative_path("src/main.rs", "../lib.rs");
		assert!(result.is_some());
		assert_eq!(result.unwrap().to_string_lossy(), "lib.rs");
	}

	#[test]
	fn test_cross_platform_path_comparison() {
		let files = vec![
			"src/main.rs".to_string(),
			"src\\utils\\helper.rs".to_string(), // Windows-style path
			"lib/config.rs".to_string(),
		];

		let registry = FileRegistry::new(&files);

		// Test finding Windows-style path with Unix-style query
		let result = registry.find_exact_file("src/utils/helper.rs");
		assert!(result.is_some(), "Should find Windows path with Unix query");

		// Test finding Unix-style path with Windows-style query
		let result = registry.find_exact_file("lib\\config.rs");
		assert!(result.is_some(), "Should find Unix path with Windows query");
	}

	#[test]
	fn test_normalize_path_separators() {
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

		// Test empty and single character
		assert_eq!(PathNormalizer::normalize_separators(""), "");
		assert_eq!(PathNormalizer::normalize_separators("\\"), "/");
		assert_eq!(PathNormalizer::normalize_separators("/"), "/");
	}
}
