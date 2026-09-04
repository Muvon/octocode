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

#[cfg(test)]
mod tests {
	use crate::watcher_config::*;
	use std::fs;
	use tempfile::TempDir;

	fn workspace(gitignore: Option<&str>, noindex: Option<&str>) -> (TempDir, IgnorePatterns) {
		let dir = TempDir::new().expect("tempdir");
		if let Some(content) = gitignore {
			fs::write(dir.path().join(".gitignore"), content).unwrap();
		}
		if let Some(content) = noindex {
			fs::write(dir.path().join(".noindex"), content).unwrap();
		}
		let patterns = IgnorePatterns::new(dir.path().to_path_buf());
		(dir, patterns)
	}

	#[test]
	fn debounce_bounds_are_ordered() {
		// Compile-time assertions: the ordering is a property of the constants,
		// so checking it at runtime would be dead weight.
		const { assert!(MIN_DEBOUNCE_MS < MCP_DEFAULT_DEBOUNCE_MS) };
		const { assert!(MCP_DEFAULT_DEBOUNCE_MS < MAX_DEBOUNCE_MS) };
		const { assert!(DEFAULT_ADDITIONAL_DELAY_MS < MAX_ADDITIONAL_DELAY_MS) };
		const { assert!(WATCH_MIN_DEBOUNCE_SECS < WATCH_DEFAULT_DEBOUNCE_SECS) };
		const { assert!(WATCH_DEFAULT_DEBOUNCE_SECS < WATCH_MAX_DEBOUNCE_SECS) };
	}

	#[test]
	fn git_internals_are_always_ignored() {
		let (dir, patterns) = workspace(None, None);
		assert!(patterns.should_ignore_path(&dir.path().join(".git/index")));
		assert!(patterns.should_ignore_path(&dir.path().join(".git")));
	}

	#[test]
	fn missing_ignore_files_ignore_nothing_else() {
		let (dir, patterns) = workspace(None, None);
		assert!(!patterns.should_ignore_path(&dir.path().join("src/main.rs")));
	}

	#[test]
	fn slash_only_line_does_not_swallow_every_path() {
		// "/" trims to the empty string; an empty pattern would match every path
		// through `contains("")` and silence the whole watcher.
		let (dir, patterns) = workspace(Some("/\n"), Some("/\n"));
		assert!(!patterns.should_ignore_path(&dir.path().join("src/main.rs")));
	}

	#[test]
	fn comments_and_blank_lines_are_skipped() {
		let (dir, patterns) = workspace(Some("# comment\n\n   \ntarget\n"), None);
		assert!(patterns.should_ignore_path(&dir.path().join("target/debug/app")));
		assert!(!patterns.should_ignore_path(&dir.path().join("comment/file.rs")));
	}

	#[test]
	fn noindex_patterns_apply_alongside_gitignore() {
		let (dir, patterns) = workspace(Some("target\n"), Some("vendor\n"));
		assert!(patterns.should_ignore_path(&dir.path().join("target/x")));
		assert!(patterns.should_ignore_path(&dir.path().join("vendor/y")));
		assert!(!patterns.should_ignore_path(&dir.path().join("src/y")));
	}

	#[test]
	fn wildcard_patterns_match_prefix_and_suffix() {
		let (dir, patterns) = workspace(Some("*.log\nbuild*\n"), None);
		assert!(patterns.should_ignore_path(&dir.path().join("server.log")));
		assert!(patterns.should_ignore_path(&dir.path().join("build-output")));
		assert!(!patterns.should_ignore_path(&dir.path().join("src/app.rs")));
	}

	#[test]
	fn bare_star_matches_everything() {
		let (dir, patterns) = workspace(Some("*\n"), None);
		assert!(patterns.should_ignore_path(&dir.path().join("anything.rs")));
	}

	#[test]
	fn paths_outside_the_working_directory_are_matched_absolutely() {
		let (_dir, patterns) = workspace(Some("secrets\n"), None);
		assert!(patterns.should_ignore_path(std::path::Path::new("/elsewhere/secrets/key")));
		assert!(!patterns.should_ignore_path(std::path::Path::new("/elsewhere/public/key")));
	}

	#[test]
	fn reload_picks_up_edited_ignore_files() {
		let dir = TempDir::new().unwrap();
		let mut patterns = IgnorePatterns::new(dir.path().to_path_buf());
		assert!(!patterns.should_ignore_path(&dir.path().join("dist/app.js")));

		fs::write(dir.path().join(".gitignore"), "dist\n").unwrap();
		patterns.reload();
		assert!(patterns.should_ignore_path(&dir.path().join("dist/app.js")));

		fs::write(dir.path().join(".gitignore"), "# nothing\n").unwrap();
		patterns.reload();
		assert!(!patterns.should_ignore_path(&dir.path().join("dist/app.js")));
	}
}
