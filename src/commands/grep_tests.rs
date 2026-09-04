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
	use super::super::*;
	use tempfile::TempDir;

	const SOURCE: &str = "\
fn main() {
	let a = parse().unwrap();
	let b = other().unwrap();
	println!(\"{a}{b}\");
}
";

	/// A workspace with two Rust files, one Python file and one gitignored file.
	fn workspace() -> TempDir {
		let dir = TempDir::new().unwrap();
		std::fs::create_dir_all(dir.path().join("src")).unwrap();
		std::fs::create_dir_all(dir.path().join("target")).unwrap();
		std::fs::write(dir.path().join("src/main.rs"), SOURCE).unwrap();
		std::fs::write(dir.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
		std::fs::write(dir.path().join("src/app.py"), "def run():\n    pass\n").unwrap();
		std::fs::write(dir.path().join("target/built.rs"), SOURCE).unwrap();
		std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
		std::fs::create_dir_all(dir.path().join(".git")).unwrap();
		dir
	}

	fn args(pattern: &str) -> GrepArgs {
		GrepArgs {
			pattern: pattern.to_string(),
			lang: None,
			paths: vec![],
			context: 0,
			rewrite: None,
			update_all: false,
			json: false,
		}
	}

	fn names(files: &[String]) -> Vec<String> {
		let mut out: Vec<String> = files
			.iter()
			.map(|f| {
				std::path::Path::new(f)
					.file_name()
					.unwrap()
					.to_string_lossy()
					.to_string()
			})
			.collect();
		out.sort();
		out
	}

	#[test]
	fn walking_without_patterns_collects_every_recognised_source_file() {
		let dir = workspace();
		let files = collect_files(dir.path(), &[], None).unwrap();
		let found = names(&files);
		assert!(found.contains(&"main.rs".to_string()), "{found:?}");
		assert!(found.contains(&"app.py".to_string()), "{found:?}");
		// Gitignored output is skipped.
		assert!(!found.contains(&"built.rs".to_string()), "{found:?}");
	}

	#[test]
	fn a_language_filter_narrows_the_walk() {
		let dir = workspace();
		let files = collect_files(dir.path(), &[], Some("python")).unwrap();
		assert_eq!(names(&files), vec!["app.py".to_string()]);
	}

	#[test]
	fn a_glob_pattern_selects_matching_files_only() {
		let dir = workspace();
		let files = collect_files(dir.path(), &["src/*.rs".to_string()], None).unwrap();
		assert_eq!(
			names(&files),
			vec!["lib.rs".to_string(), "main.rs".to_string()]
		);

		let filtered = collect_files(dir.path(), &["src/*".to_string()], Some("python")).unwrap();
		assert_eq!(names(&filtered), vec!["app.py".to_string()]);
	}

	#[test]
	fn an_unparseable_glob_is_rejected() {
		let dir = workspace();
		let err = collect_files(dir.path(), &["src/[unclosed".to_string()], None).unwrap_err();
		assert!(err.to_string().contains("Invalid glob pattern"), "{err}");
	}

	#[test]
	fn a_pattern_matching_nothing_collects_nothing() {
		let dir = workspace();
		assert!(collect_files(dir.path(), &["**/*.zig".to_string()], None)
			.unwrap()
			.is_empty());
	}

	#[test]
	fn a_dry_run_rewrite_reports_replacements_without_touching_the_file() {
		let dir = workspace();
		let file = dir.path().join("src/main.rs");
		let mut a = args("$X.unwrap()");
		a.lang = Some("rust".to_string());

		execute_rewrite(
			&a,
			dir.path(),
			&[file.to_string_lossy().to_string()],
			"$X.expect(\"reason\")",
		)
		.expect("a dry run must succeed");

		assert_eq!(std::fs::read_to_string(&file).unwrap(), SOURCE);
	}

	#[test]
	fn a_json_dry_run_renders_the_diff() {
		let dir = workspace();
		let file = dir.path().join("src/main.rs");
		let mut a = args("$X.unwrap()");
		a.lang = Some("rust".to_string());
		a.json = true;

		execute_rewrite(
			&a,
			dir.path(),
			&[file.to_string_lossy().to_string()],
			"$X.expect(\"reason\")",
		)
		.unwrap();
		assert_eq!(std::fs::read_to_string(&file).unwrap(), SOURCE);
	}

	#[test]
	fn update_all_writes_the_rewritten_source_back() {
		let dir = workspace();
		let file = dir.path().join("src/main.rs");
		let mut a = args("$X.unwrap()");
		a.lang = Some("rust".to_string());
		a.update_all = true;

		execute_rewrite(
			&a,
			dir.path(),
			&[file.to_string_lossy().to_string()],
			"$X.expect(\"reason\")",
		)
		.unwrap();

		let rewritten = std::fs::read_to_string(&file).unwrap();
		assert!(rewritten.contains("expect(\"reason\")"), "{rewritten}");
		assert!(!rewritten.contains("unwrap()"), "{rewritten}");
	}

	#[test]
	fn a_rewrite_that_matches_nothing_leaves_the_file_alone() {
		let dir = workspace();
		let file = dir.path().join("src/lib.rs");
		let before = std::fs::read_to_string(&file).unwrap();
		let mut a = args("$X.unwrap()");
		a.lang = Some("rust".to_string());
		a.update_all = true;

		execute_rewrite(
			&a,
			dir.path(),
			&[file.to_string_lossy().to_string()],
			"$X.expect(\"reason\")",
		)
		.unwrap();
		assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
	}

	#[test]
	fn files_with_no_detectable_language_or_no_content_are_skipped() {
		let dir = workspace();
		let a = args("$X.unwrap()");
		execute_rewrite(
			&a,
			dir.path(),
			&[
				dir.path().join("notes.bin").to_string_lossy().to_string(),
				dir.path()
					.join("src/missing.rs")
					.to_string_lossy()
					.to_string(),
			],
			"$X.expect(\"reason\")",
		)
		.expect("unreadable and unknown files must be skipped, not fatal");
	}
}
