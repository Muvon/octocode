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

use std::path::Path;

use crate::utils::path::PathNormalizer;

/// Path utilities for consistent path handling across the application
pub struct PathUtils;

impl PathUtils {
	/// Returns the relative path as a String, suitable for storage and display
	///
	/// Separators are always forward slashes. The result is the key every index
	/// row, GraphRAG node id and MCP answer is built from, and callers match it
	/// against paths that arrived as text (a glob, an `import` specifier, a
	/// `file_path` argument), which use `/` on every platform. Keeping Windows'
	/// native `\` here would make each of those lookups miss.
	pub fn to_relative_string(path: &Path, current_dir: &Path) -> String {
		let relative = path.strip_prefix(current_dir).unwrap_or(path);
		PathNormalizer::normalize_separators(&relative.to_string_lossy())
	}

	/// Creates a relative path for display purposes, ensuring it never shows absolute paths
	pub fn for_display(path: &Path, current_dir: &Path) -> String {
		let relative = Self::to_relative_string(path, current_dir);

		// Ensure we never display absolute paths to users. `to_relative_string`
		// has already rewritten the separators, so a Windows absolute path
		// reaches this point as `C:/...`, never `C:\...`.
		if relative.starts_with('/') || relative.get(1..3) == Some(":/") {
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
