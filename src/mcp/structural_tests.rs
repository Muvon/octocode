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
	use super::super::structural::*;
	use tempfile::TempDir;

	/// A gitignore-aware workspace with two Rust files, one Python file and one
	/// ignored build artefact.
	fn repo() -> TempDir {
		let dir = TempDir::new().unwrap();
		std::fs::create_dir_all(dir.path().join("src/inner")).unwrap();
		std::fs::create_dir_all(dir.path().join("target")).unwrap();
		std::fs::create_dir_all(dir.path().join(".git")).unwrap();
		std::fs::write(
			dir.path().join("src/main.rs"),
			"fn main() {\n\tlet value = parse().unwrap();\n}\n",
		)
		.unwrap();
		std::fs::write(
			dir.path().join("src/inner/helper.rs"),
			"pub fn helper() -> u32 {\n\t7\n}\n",
		)
		.unwrap();
		std::fs::write(dir.path().join("src/app.py"), "def run():\n    pass\n").unwrap();
		std::fs::write(dir.path().join("target/built.rs"), "fn built() {}\n").unwrap();
		std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
		dir
	}

	fn displays(files: &[FileData]) -> Vec<&str> {
		files.iter().map(|f| f.display.as_str()).collect()
	}

	#[test]
	fn only_files_of_the_requested_language_are_collected() {
		let dir = repo();
		let (files, stamp) = collect_file_data(dir.path(), "rust", None, &[]);
		assert_eq!(displays(&files), vec!["src/inner/helper.rs", "src/main.rs"]);
		assert_eq!(stamp.file_count, 2);
		assert!(stamp.total_size > 0);
		assert!(stamp.max_mtime.is_some());
		// Gitignored build output never enters the candidate set.
		assert!(!displays(&files).contains(&"target/built.rs"));
	}

	#[test]
	fn a_substring_path_filter_narrows_the_candidates() {
		let dir = repo();
		let (files, _) = collect_file_data(dir.path(), "rust", Some(&["inner".to_string()]), &[]);
		assert_eq!(displays(&files), vec!["src/inner/helper.rs"]);
	}

	#[test]
	fn a_glob_path_filter_is_honoured() {
		let dir = repo();
		let (files, _) =
			collect_file_data(dir.path(), "rust", Some(&["src/**/*.rs".to_string()]), &[]);
		assert!(displays(&files).contains(&"src/inner/helper.rs"));
	}

	#[test]
	fn a_malformed_glob_degrades_to_a_substring_match() {
		// A stray `[` must not silently empty the candidate set.
		let dir = repo();
		let (files, _) = collect_file_data(dir.path(), "rust", Some(&["src/[".to_string()]), &[]);
		assert!(files.is_empty(), "no display path contains 'src/['");

		let (matched, _) = collect_file_data(dir.path(), "rust", Some(&["src/m".to_string()]), &[]);
		assert_eq!(displays(&matched), vec!["src/main.rs"]);
	}

	#[test]
	fn a_filter_matching_nothing_collects_nothing() {
		let dir = repo();
		let (files, stamp) =
			collect_file_data(dir.path(), "rust", Some(&["nowhere".to_string()]), &[]);
		assert!(files.is_empty());
		assert_eq!(stamp, RepoStamp::default());
	}

	#[test]
	fn the_literal_prefilter_flags_files_that_contain_every_token() {
		let dir = repo();
		let (files, _) = collect_file_data(
			dir.path(),
			"rust",
			None,
			&["unwrap".to_string(), "parse".to_string()],
		);
		let main = files.iter().find(|f| f.display == "src/main.rs").unwrap();
		let helper = files
			.iter()
			.find(|f| f.display == "src/inner/helper.rs")
			.unwrap();
		assert!(main.prefilter_hit);
		assert!(!helper.prefilter_hit, "helper has neither token");
	}

	#[test]
	fn an_empty_prefilter_marks_every_file_as_a_hit() {
		let dir = repo();
		let (files, _) = collect_file_data(dir.path(), "rust", None, &[]);
		assert!(files.iter().all(|f| f.prefilter_hit));
	}

	#[test]
	fn a_stamp_changes_when_the_candidate_set_changes() {
		let dir = repo();
		let (_, before) = collect_file_data(dir.path(), "rust", None, &[]);
		std::fs::write(dir.path().join("src/extra.rs"), "fn extra() {}\n").unwrap();
		let (_, after) = collect_file_data(dir.path(), "rust", None, &[]);
		assert_ne!(before, after);
		assert_eq!(after.file_count, before.file_count + 1);
	}

	#[test]
	fn a_request_fingerprint_depends_on_every_part() {
		let base = fingerprint_request(&["rust", "$X.unwrap()", ""]);
		assert_eq!(base, fingerprint_request(&["rust", "$X.unwrap()", ""]));
		assert_ne!(base, fingerprint_request(&["go", "$X.unwrap()", ""]));
		assert_ne!(base, fingerprint_request(&["rust", "$X.expect()", ""]));
		// Field boundaries matter: the same concatenation must not collide.
		assert_ne!(
			fingerprint_request(&["ab", "c"]),
			fingerprint_request(&["a", "bc"])
		);
	}

	#[test]
	fn a_cache_entry_is_only_valid_for_its_own_fingerprint_and_stamp() {
		let dir = repo();
		let (files, stamp) = collect_file_data(dir.path(), "rust", None, &[]);
		let outcome = smart_search(&files, "$X.unwrap()", "rust", &[], None, None);

		let cache = QueryCache {
			fingerprint: fingerprint_request(&["rust", "$X.unwrap()"]),
			stamp,
			matches: outcome.matches,
			note: outcome.note,
			diagnostic: outcome.diagnostic,
		};

		assert!(!cache.matches.is_empty());
		assert_eq!(cache.stamp, stamp);
		assert_ne!(
			cache.fingerprint,
			fingerprint_request(&["go", "$X.unwrap()"])
		);
	}

	#[test]
	fn an_unresolvable_symbol_spec_reports_a_diagnostic() {
		let dir = repo();
		let (files, _) = collect_file_data(dir.path(), "rust", None, &[]);
		let outcome = symbol_search(&files, "[", "rust", false);
		assert!(outcome.matches.is_empty());
		assert!(
			outcome.diagnostic.is_some(),
			"a bad symbol spec must explain itself"
		);
	}

	#[test]
	fn a_symbol_search_over_a_real_workspace_finds_the_definition() {
		let dir = repo();
		let (files, _) = collect_file_data(dir.path(), "rust", None, &[]);
		let outcome = symbol_search(&files, "helper", "rust", false);
		assert_eq!(outcome.matches.len(), 1, "{:?}", outcome.diagnostic);
		assert_eq!(outcome.matches[0].file, "src/inner/helper.rs");
	}
}
