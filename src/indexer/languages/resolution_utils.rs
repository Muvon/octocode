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
	/// Lazily-populated cache of each file's canonicalized path (or `None` if
	/// canonicalize failed), keyed by the original path. Populated on first
	/// use of `find_exact_file`'s canonicalize fallback rather than eagerly
	/// at construction, since most lookups hit the fast path in
	/// `PathNormalizer::find_path_in_collection` and never need it — but
	/// `FileRegistry` is built once per indexing pass and reused across many
	/// `resolve_import` calls, so a fallback that does hit this path would
	/// otherwise re-run a `Path::canonicalize()` syscall per file on every call.
	canonical_cache: std::sync::OnceLock<HashMap<String, Option<String>>>,
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
			canonical_cache: std::sync::OnceLock::new(),
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
		// Use cross-platform path comparison first (most reliable for tests)
		if let Some(found) = PathNormalizer::find_path_in_collection(target_path, &self.all_files) {
			return Some(found.to_string());
		}

		// Try direct canonicalize only as fallback (for real files)
		if let Ok(canonical_target) = std::path::Path::new(target_path).canonicalize() {
			let normalized_canonical = Self::normalize_canonical_path(&canonical_target);

			for (file_path, normalized_file) in self.canonical_files() {
				if normalized_file.as_deref() == Some(normalized_canonical.as_str()) {
					return Some(file_path.clone());
				}
			}
		}

		None
	}

	/// Each registry file's canonicalized-and-normalized path (or `None` if
	/// canonicalize failed), computed once on first call and cached for the
	/// lifetime of this `FileRegistry`. `find_exact_file`'s fallback runs this
	/// per `resolve_import` call in the worst case, so without caching every
	/// fallback lookup would re-run one `Path::canonicalize()` syscall per
	/// registry file.
	fn canonical_files(&self) -> &HashMap<String, Option<String>> {
		self.canonical_cache.get_or_init(|| {
			self.all_files
				.iter()
				.map(|file_path| {
					let normalized = std::path::Path::new(file_path)
						.canonicalize()
						.ok()
						.map(|canonical| Self::normalize_canonical_path(&canonical));
					(file_path.clone(), normalized)
				})
				.collect()
		})
	}

	/// Normalize a canonicalized path for cross-platform comparison, handling
	/// Windows UNC paths (`//?/C:/...`) the same way as a plain path.
	fn normalize_canonical_path(canonical: &Path) -> String {
		let canonical_str = canonical.to_string_lossy();
		if let Some(drive_pos) = canonical_str
			.starts_with("//?/")
			.then(|| canonical_str.find(":/"))
			.flatten()
		{
			if let Some(relative_part) = canonical_str.get(drive_pos + 2..) {
				return PathNormalizer::normalize_separators(relative_part);
			}
		}
		PathNormalizer::normalize_separators(&canonical_str)
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

	// Manually resolve .. components to avoid Windows canonicalization issues
	let mut components = Vec::new();
	for component in path_buf.components() {
		match component {
			std::path::Component::ParentDir => {
				// Pop the last component if possible. If there's nothing left to
				// pop (or what's left is itself an unresolved ".."), keep the ".."
				// so the result correctly reflects an escape above the known root
				// instead of silently resolving to a shorter, wrong path.
				match components.last().map(String::as_str) {
					Some("..") | None => components.push("..".to_string()),
					_ => {
						components.pop();
					}
				}
			}
			std::path::Component::CurDir => {
				// Skip current directory components
			}
			std::path::Component::Prefix(_) => {
				// Skip Windows drive prefixes for relative path normalization
			}
			std::path::Component::RootDir => {
				// Skip root directory for relative path normalization
			}
			std::path::Component::Normal(name) => {
				components.push(name.to_string_lossy().to_string());
			}
		}
	}

	// If we have components, build relative path with normalized separators
	if !components.is_empty() {
		let normalized: PathBuf = components.into_iter().collect();
		return PathNormalizer::normalize_separators(&normalized.to_string_lossy());
	}

	// Try canonicalization only as fallback and make relative
	if let Ok(canonical) = path_buf.canonicalize() {
		if let Ok(current_dir) = std::env::current_dir() {
			if let Ok(relative) = canonical.strip_prefix(&current_dir) {
				return PathNormalizer::normalize_separators(&relative.to_string_lossy());
			}
		}
		// Handle Windows UNC paths like //?/D:/path/to/file
		let canonical_str = canonical.to_string_lossy();
		if canonical_str.starts_with("//?/") {
			if let Some(drive_pos) = canonical_str.find(":/") {
				if let Some(relative_part) = canonical_str.get(drive_pos + 2..) {
					return PathNormalizer::normalize_separators(relative_part);
				}
			}
		}
		return PathNormalizer::normalize_separators(&canonical_str);
	}

	// Final fallback: just normalize separators
	PathNormalizer::normalize_separators(path)
}

/// Detect language from file path extension
pub fn detect_language_from_path(file_path: &str) -> Option<String> {
	let path = Path::new(file_path);
	if let Some(language) = crate::language::associated_language(path) {
		return Some(language.to_string());
	}

	let extension = path.extension()?.to_str()?;

	match extension {
		"rs" => Some("rust".to_string()),
		"js" | "mjs" => Some("javascript".to_string()),
		"ts" | "tsx" => Some("typescript".to_string()),
		"py" => Some("python".to_string()),
		"go" => Some("go".to_string()),
		"php" => Some("php".to_string()),
		"cpp" | "cc" | "cxx" | "c++" | "c" | "h" | "hpp" | "hxx" | "cppm" | "ixx" | "mxx"
		| "ccm" | "cxxm" => Some("cpp".to_string()),
		"rb" => Some("ruby".to_string()),
		"sh" | "bash" => Some("bash".to_string()),
		"json" => Some("json".to_string()),
		"css" | "scss" | "sass" => Some("css".to_string()),
		"ex" | "exs" => Some("elixir".to_string()),
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

	#[test]
	fn test_find_exact_file_canonicalize_fallback_cache_is_reused() {
		let unique = format!(
			"octocode_test_{}_{}",
			std::process::id(),
			std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_nanos()
		);
		let dir = std::env::temp_dir().join(unique).join("sub");
		std::fs::create_dir_all(&dir).expect("failed to create temp dir");
		let file_path = dir.join("file.txt");
		std::fs::write(&file_path, b"content").expect("failed to write temp file");

		let canonical = file_path.canonicalize().expect("failed to canonicalize");
		let registered_path = canonical.to_string_lossy().to_string();

		let registry = FileRegistry::new(std::slice::from_ref(&registered_path));

		// Query with a textually different but equivalent path (redundant
		// `..` component) so the fast string-match path misses and the
		// canonicalize fallback is actually exercised.
		let query_path = dir.join("..").join("sub").join("file.txt");
		let query = query_path.to_string_lossy().to_string();

		let first = registry.find_exact_file(&query);
		assert_eq!(first, Some(registered_path.clone()));

		// Second fallback lookup against the same registry instance must
		// still succeed and agree, proving the lazily-built cache is
		// populated once and reused correctly rather than going stale.
		let second = registry.find_exact_file(&query);
		assert_eq!(second, Some(registered_path));

		let _ = std::fs::remove_dir_all(dir.parent().unwrap());
	}
}
