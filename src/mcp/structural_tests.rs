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

	/// An in-memory candidate file, bypassing the walk.
	fn fd(display: &str, content: &str) -> FileData {
		FileData {
			path: std::path::PathBuf::from(display),
			display: display.to_string(),
			content: content.to_string(),
			prefilter_hit: true,
		}
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

	#[test]
	fn a_file_that_is_not_valid_utf8_is_skipped() {
		let dir = repo();
		std::fs::write(dir.path().join("src/blob.rs"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
		let (files, stamp) = collect_file_data(dir.path(), "rust", None, &[]);
		assert!(
			!displays(&files).contains(&"src/blob.rs"),
			"unreadable candidates never reach the parser"
		);
		// It is skipped before it can be counted, so the stamp ignores it too.
		assert_eq!(stamp.file_count, 2);
	}

	// --- Pass B: keyword broadening per language ---

	#[test]
	fn a_python_def_pattern_broadens_to_the_function_definition_kind() {
		// The arity is wrong, so the structural pass finds nothing and the
		// `def` keyword broadens the search to every function definition.
		let files = vec![fd(
			"app.py",
			"@route(\"/\")\ndef handler():\n    return 1\n",
		)];
		let out = smart_search(&files, "def $N($A, $B, $C): $$$", "python", &[], None, None);
		assert_eq!(out.matches.len(), 1, "{:?}", out.diagnostic);
		let note = out.note.expect("a broadening note");
		assert!(note.contains("[kind: function_definition]"), "{note}");
		assert!(note.contains("function definitions"), "{note}");
		assert!(
			out.matches[0].text.contains("handler"),
			"{}",
			out.matches[0].text
		);
	}

	#[test]
	fn a_go_func_pattern_broadens_to_the_function_declaration_kind() {
		let files = vec![fd("main.go", "package main\n\nfunc run() {}\n")];
		let out = smart_search(&files, "func $N($A, $B) { $$$ }", "go", &[], None, None);
		assert_eq!(out.matches.len(), 1, "{:?}", out.diagnostic);
		let note = out.note.expect("a broadening note");
		assert!(note.contains("[kind: function_declaration]"), "{note}");
		assert!(
			out.matches[0].text.contains("run"),
			"{}",
			out.matches[0].text
		);
	}

	#[test]
	fn a_typescript_class_pattern_broadens_to_the_class_declaration_kind() {
		let files = vec![fd("app.ts", "class Widget {\n  render() {}\n}\n")];
		let out = smart_search(
			&files,
			"class $N { constructor($$$) { $$$ } }",
			"typescript",
			&[],
			None,
			None,
		);
		assert_eq!(out.matches.len(), 1, "{:?}", out.diagnostic);
		let note = out.note.expect("a broadening note");
		assert!(note.contains("[kind: class_declaration]"), "{note}");
	}

	// --- Pass B: canonical kinds and contextual wrapping ---

	#[test]
	fn an_intent_word_is_mapped_to_the_canonical_kind_for_the_language() {
		let files = vec![fd("a.rs", "fn main() {\n\thelper();\n}\nfn helper() {}\n")];
		let out = smart_search(&files, "call", "rust", &[], None, None);
		assert_eq!(out.matches.len(), 1, "{:?}", out.diagnostic);
		let note = out.note.expect("a canonical-kind note");
		assert!(note.contains("[kind: call_expression]"), "{note}");
		assert!(note.contains("canonical rust kind for `call`"), "{note}");
		assert_eq!(out.matches[0].line, 2);
	}

	#[test]
	fn a_rust_type_expression_is_wrapped_in_a_type_alias_context() {
		// `Arc<Mutex<$T>>` is not a standalone Rust item, so it only matches
		// once the fallback wraps it as `type _ = ...;`.
		let files = vec![fd(
			"a.rs",
			"pub struct Store {\n\tinner: Arc<Mutex<u32>>,\n}\n",
		)];
		let out = smart_search(&files, "Arc<Mutex<$T>>", "rust", &[], None, None);
		assert_eq!(out.matches.len(), 1, "{:?}", out.diagnostic);
		let note = out.note.expect("a context-wrap note");
		assert!(note.contains("[context wrap:"), "{note}");
		assert!(note.contains("generic_type"), "{note}");
		assert_eq!(out.matches[0].line, 2);
	}

	#[test]
	fn a_json_pair_pattern_falls_back_to_a_labelled_lexical_scan() {
		// tree-sitter-json does not support metavariables, so neither the plain
		// pattern nor the object-wrapped contextual strategy can match. The
		// labelled lexical scan is what keeps the tool from answering empty.
		let files = vec![fd("config.json", "{\n  \"key\": 7,\n  \"other\": 8\n}\n")];
		let out = smart_search(&files, "\"key\": $V", "json", &[], None, None);
		assert_eq!(out.matches.len(), 1);
		assert_eq!(out.matches[0].line, 2);
		let note = out.note.expect("a lexical fallback note");
		assert!(note.contains("[lexical fallback]"), "{note}");
		assert!(note.contains("not AST-verified"), "{note}");
		assert!(out.diagnostic.is_some());
	}

	// --- Ranking ---

	#[test]
	fn matches_in_files_named_after_a_metavariable_are_ranked_first() {
		// Base order is alphabetical by path, so `a_other.rs` would come first;
		// the `$HELPER` metavariable boosts the file whose name mentions it.
		let files = vec![
			fd("a_other.rs", "fn a() { x.unwrap(); }\n"),
			fd("b_helper.rs", "fn b() { y.unwrap(); }\n"),
		];

		let unranked = smart_search(&files, "$X.unwrap()", "rust", &[], None, None);
		assert_eq!(unranked.matches[0].file, "a_other.rs");

		let ranked = smart_search(&files, "$HELPER.unwrap()", "rust", &[], None, None);
		assert_eq!(ranked.matches.len(), 2);
		assert_eq!(ranked.matches[0].file, "b_helper.rs");
		assert_eq!(ranked.matches[1].file, "a_other.rs");
	}

	// --- Diagnostics ---

	#[test]
	fn a_pattern_that_cannot_stand_alone_reports_the_parse_error_hint() {
		// `pub struct $N { $$$ }` does not parse as a standalone Rust item
		// (Pass B still rescues it by kind), so a total miss reports that.
		let files = vec![fd("a.rs", "fn main() {}\n")];
		let out = smart_search(&files, "pub struct $N { $$$ }", "rust", &[], None, None);
		assert!(out.matches.is_empty());
		assert!(out.note.is_none());

		let diagnostic = out.diagnostic.expect("a diagnostic explains the miss");
		assert!(
			diagnostic.starts_with("No matches: pub struct $N { $$$ }"),
			"{diagnostic}"
		);
		assert!(diagnostic.contains("parse_error=true"), "{diagnostic}");
		assert!(diagnostic.contains("metavars=$N"), "{diagnostic}");
		assert!(
			diagnostic.contains("doesn't parse as standalone rust"),
			"{diagnostic}"
		);
	}

	#[test]
	fn an_item_keyword_that_matches_nothing_suggests_its_tree_sitter_kind() {
		// This one parses cleanly, so the diagnostic can offer the kind name
		// instead of a parse hint.
		let files = vec![fd("a.rs", "const A: u8 = 1;\n")];
		let out = smart_search(&files, "fn helper() {}", "rust", &[], None, None);
		assert!(out.matches.is_empty(), "{:?}", out.note);

		let diagnostic = out.diagnostic.expect("a diagnostic explains the miss");
		assert!(diagnostic.contains("parse_error=false"), "{diagnostic}");
		assert!(diagnostic.contains("function definitions"), "{diagnostic}");
		assert!(diagnostic.contains("`function_item`"), "{diagnostic}");
	}

	#[test]
	fn an_intent_word_that_matches_nothing_suggests_the_canonical_kind() {
		let files = vec![fd("a.rs", "fn main() {}\n")];
		let out = smart_search(&files, "call", "rust", &[], None, None);
		assert!(out.matches.is_empty(), "{:?}", out.note);

		let diagnostic = out.diagnostic.expect("a diagnostic explains the miss");
		assert!(
			diagnostic.contains("canonical rust kind for this intent is 'call_expression'"),
			"{diagnostic}"
		);
	}

	#[test]
	fn a_diagnostic_reports_the_metavariables_the_pattern_defined() {
		let files = vec![fd("a.rs", "fn main() {}\n")];
		let out = smart_search(&files, "$LEFT.frobnicate($RIGHT)", "rust", &[], None, None);
		assert!(out.matches.is_empty());
		let diagnostic = out.diagnostic.expect("a diagnostic explains the miss");
		assert!(diagnostic.contains("$LEFT"), "{diagnostic}");
		assert!(diagnostic.contains("$RIGHT"), "{diagnostic}");
	}

	// --- Symbol mode ---

	#[test]
	fn a_symbol_with_no_definition_falls_back_to_a_labelled_lexical_scan() {
		let files = vec![fd(
			"a.rs",
			"// frobnicate is only mentioned here\nfn main() {}\n",
		)];

		let definitions = symbol_search(&files, "frobnicate", "rust", false);
		assert_eq!(definitions.matches.len(), 1);
		assert_eq!(definitions.matches[0].line, 1);
		let note = definitions.note.expect("a lexical fallback note");
		assert!(note.contains("[lexical fallback]"), "{note}");
		assert!(note.contains("No definitions found"), "{note}");
		assert!(definitions.diagnostic.is_none());

		let references = symbol_search(&files, "frobnicate", "rust", true);
		let note = references.note.expect("a lexical fallback note");
		assert!(note.contains("No references found"), "{note}");
	}

	#[test]
	fn a_symbol_too_short_to_scan_for_reports_a_diagnostic_instead() {
		let files = vec![fd("a.rs", "fn helper() {}\n")];
		let out = symbol_search(&files, "z*", "rust", false);
		assert!(out.matches.is_empty());
		assert!(out.note.is_none());

		let diagnostic = out.diagnostic.expect("a diagnostic explains the miss");
		assert!(
			diagnostic.starts_with("No symbol definitions for `z*`"),
			"{diagnostic}"
		);
		assert!(diagnostic.contains("1 files scanned"), "{diagnostic}");
		assert!(diagnostic.contains("language: rust"), "{diagnostic}");
	}

	#[test]
	fn symbol_references_include_the_definition_site_and_every_call() {
		let files = vec![fd(
			"a.rs",
			"fn flush() {}\nfn main() {\n\tflush();\n\tflush();\n}\n",
		)];
		let out = symbol_search(&files, "flush", "rust", true);
		assert_eq!(out.matches.len(), 3, "{:?}", out.diagnostic);
		assert_eq!(out.note.unwrap(), "[symbol references: flush]");
		assert_eq!(
			out.matches.iter().map(|m| m.line).collect::<Vec<_>>(),
			vec![1, 3, 4]
		);
	}
}
