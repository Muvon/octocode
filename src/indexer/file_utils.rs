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

/// File system utilities for indexing operations
pub struct FileUtils;

impl FileUtils {
	/// Get file modification time as seconds since Unix epoch
	pub fn get_file_mtime(file_path: &Path) -> Result<u64> {
		let metadata = std::fs::metadata(file_path)?;
		let mtime = metadata
			.modified()?
			.duration_since(std::time::UNIX_EPOCH)?
			.as_secs();
		Ok(mtime)
	}

	/// Check if a file contains readable text
	pub fn is_text_file(contents: &str) -> bool {
		if contents.is_empty() {
			return false;
		}

		// Quick check for binary content
		if contents.contains('\0') {
			return false;
		}

		// Check for reasonable ratio of printable characters
		let total_chars = contents.len();
		let printable_chars = contents
			.chars()
			.filter(|c| c.is_ascii_graphic() || c.is_ascii_whitespace())
			.count();

		// Allow files with at least 80% printable characters
		let printable_ratio = printable_chars as f64 / total_chars as f64;
		printable_ratio > 0.8
	}

	/// Check if a file extension is allowed for text indexing
	pub fn is_allowed_text_extension(path: &Path) -> bool {
		const ALLOWED_TEXT_EXTENSIONS: &[&str] = &[
			"txt",
			"md",
			"markdown",
			"rst",
			"org",
			"adoc",
			"asciidoc",
			"readme",
			"changelog",
			"license",
			"contributors",
		];

		if let Some(extension) = path.extension() {
			if let Some(ext_str) = extension.to_str() {
				return ALLOWED_TEXT_EXTENSIONS.contains(&ext_str.to_lowercase().as_str());
			}
		}

		// Check if filename matches common text file patterns (no extension)
		if let Some(filename) = path.file_name() {
			if let Some(name_str) = filename.to_str() {
				let name_lower = name_str.to_lowercase();
				return ALLOWED_TEXT_EXTENSIONS
					.iter()
					.any(|&ext| name_lower == ext || name_lower.starts_with(&format!("{}.", ext)));
			}
		}

		false
	}

	/// Detect language based on file extension
	pub fn detect_language(path: &Path) -> Option<&'static str> {
		match path.extension()?.to_str()? {
			"rs" => Some("rust"),
			"py" => Some("python"),
			"js" | "mjs" => Some("javascript"),
			"ts" => Some("typescript"),
			"go" => Some("go"),
			"c" => Some("c"),
			"cpp" | "cc" | "cxx" => Some("cpp"),
			"h" | "hpp" => Some("c"),
			"java" => Some("java"),
			"kt" => Some("kotlin"),
			"swift" => Some("swift"),
			"rb" => Some("ruby"),
			"php" => Some("php"),
			"cs" => Some("c_sharp"),
			"scala" => Some("scala"),
			"sh" | "bash" => Some("bash"),
			_ => None,
		}
	}
}
