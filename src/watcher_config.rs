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

//! Shared configuration for file watching and debouncing across MCP and Watch commands

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Default debounce time in milliseconds for MCP server
pub const MCP_DEFAULT_DEBOUNCE_MS: u64 = 2000; // 2 seconds

/// Maximum debounce time in milliseconds
pub const MAX_DEBOUNCE_MS: u64 = 10000; // 10 seconds

/// Minimum debounce time in milliseconds for file watcher
pub const MIN_DEBOUNCE_MS: u64 = 500; // 500ms

/// Additional delay after debounce in milliseconds
pub const DEFAULT_ADDITIONAL_DELAY_MS: u64 = 1000; // 1 second

/// Maximum additional delay in milliseconds
pub const MAX_ADDITIONAL_DELAY_MS: u64 = 5000; // 5 seconds

/// Default debounce time in seconds for watch command
pub const WATCH_DEFAULT_DEBOUNCE_SECS: u64 = 2;

/// Maximum debounce time in seconds for watch command
pub const WATCH_MAX_DEBOUNCE_SECS: u64 = 30;

/// Minimum debounce time in seconds for watch command
pub const WATCH_MIN_DEBOUNCE_SECS: u64 = 1;

/// Ignore patterns manager for file watching
pub struct IgnorePatterns {
	gitignore_patterns: HashSet<String>,
	noindex_patterns: HashSet<String>,
	working_directory: PathBuf,
}

impl IgnorePatterns {
	/// Create a new IgnorePatterns instance by reading .gitignore and .noindex files
	pub fn new(working_directory: PathBuf) -> Self {
		let mut ignore_patterns = Self {
			gitignore_patterns: HashSet::new(),
			noindex_patterns: HashSet::new(),
			working_directory,
		};

		ignore_patterns.load_gitignore();
		ignore_patterns.load_noindex();
		ignore_patterns
	}

	/// Load patterns from .gitignore file
	fn load_gitignore(&mut self) {
		let gitignore_path = self.working_directory.join(".gitignore");
		if let Ok(content) = std::fs::read_to_string(&gitignore_path) {
			for line in content.lines() {
				let line = line.trim();
				if !line.is_empty() && !line.starts_with('#') {
					// Convert gitignore patterns to simple contains checks for now
					// This is a simplified implementation - a full implementation would use glob patterns
					let pattern = line.trim_start_matches('/').trim_end_matches('/');
					// A slash-only line (e.g. "/") trims to empty; an empty pattern would
					// match every path via `contains("")`, silently ignoring all changes.
					if !pattern.is_empty() {
						self.gitignore_patterns.insert(pattern.to_string());
					}
				}
			}
		}
	}

	/// Load patterns from .noindex file
	fn load_noindex(&mut self) {
		let noindex_path = self.working_directory.join(".noindex");
		if let Ok(content) = std::fs::read_to_string(&noindex_path) {
			for line in content.lines() {
				let line = line.trim();
				if !line.is_empty() && !line.starts_with('#') {
					let pattern = line.trim_start_matches('/').trim_end_matches('/');
					// Skip empty patterns (e.g. a slash-only line) — they would match
					// every path via `contains("")`, silently ignoring all changes.
					if !pattern.is_empty() {
						self.noindex_patterns.insert(pattern.to_string());
					}
				}
			}
		}
	}

	/// Check if a path should be ignored during file watching
	pub fn should_ignore_path(&self, path: &Path) -> bool {
		let path_str = path.to_string_lossy();

		// Get relative path from working directory
		let relative_path = if let Ok(rel_path) = path.strip_prefix(&self.working_directory) {
			rel_path.to_string_lossy().to_string()
		} else {
			path_str.to_string()
		};

		// Check gitignore patterns
		if self.matches_patterns(&relative_path, &self.gitignore_patterns) {
			return true;
		}

		// Check noindex patterns
		if self.matches_patterns(&relative_path, &self.noindex_patterns) {
			return true;
		}

		false
	}

	/// Check if a path matches any of the given patterns
	fn matches_patterns(&self, path: &str, patterns: &HashSet<String>) -> bool {
		for pattern in patterns {
			if self.matches_pattern(path, pattern) {
				return true;
			}
		}
		false
	}

	/// Simple pattern matching - supports basic wildcards and directory patterns
	fn matches_pattern(&self, path: &str, pattern: &str) -> bool {
		// Handle exact matches
		if path == pattern {
			return true;
		}

		// Handle directory patterns (pattern ends with /)
		if pattern.ends_with('/') {
			let dir_pattern = pattern.trim_end_matches('/');
			if path.starts_with(&format!("{}/", dir_pattern)) || path == dir_pattern {
				return true;
			}
		}

		// Handle patterns that should match anywhere in the path
		if path.contains(pattern) {
			return true;
		}

		// Handle simple wildcard patterns
		if pattern.contains('*') {
			return self.matches_wildcard(path, pattern);
		}

		// Handle patterns that match file extensions
		if let Some(ext) = pattern.strip_prefix("*.") {
			if path.ends_with(&format!(".{}", ext)) {
				return true;
			}
		}

		false
	}

	/// Simple wildcard matching for * patterns
	fn matches_wildcard(&self, path: &str, pattern: &str) -> bool {
		// Very basic wildcard implementation
		// For a full implementation, consider using the `glob` crate
		if pattern == "*" {
			return true;
		}

		if let Some(star_pos) = pattern.find('*') {
			let before = &pattern[..star_pos];
			let after = &pattern[star_pos + 1..];

			if path.starts_with(before) && path.ends_with(after) {
				return true;
			}
		}

		false
	}

	/// Reload ignore patterns (useful when files change)
	pub fn reload(&mut self) {
		self.gitignore_patterns.clear();
		self.noindex_patterns.clear();
		self.load_gitignore();
		self.load_noindex();
	}
}
