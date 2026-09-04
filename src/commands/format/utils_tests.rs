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
	use super::super::utils::*;
	use ec4rs::property::{IndentSize, IndentStyle, TabWidth};
	use ec4rs::Properties;
	use std::path::Path;

	fn properties(entries: &[(&str, &str)]) -> Properties {
		let mut props = Properties::new();
		for (key, value) in entries {
			props.insert_raw_for_key(key, value.to_string());
		}
		props
	}

	#[test]
	fn line_endings_are_detected_by_first_match() {
		assert_eq!(detect_line_ending("a\r\nb"), "\r\n");
		assert_eq!(detect_line_ending("a\rb"), "\r");
		assert_eq!(detect_line_ending("a\nb"), "\n");
		assert_eq!(detect_line_ending("no breaks"), "\n");
		// CRLF wins even when a bare CR appears later.
		assert_eq!(detect_line_ending("a\r\nb\rc"), "\r\n");
	}

	#[test]
	fn leading_whitespace_splits_cleanly() {
		assert_eq!(
			split_leading_whitespace("\t  code();"),
			("\t  ".to_string(), "code();")
		);
		assert_eq!(
			split_leading_whitespace("code();"),
			(String::new(), "code();")
		);
		assert_eq!(split_leading_whitespace("   "), ("   ".to_string(), ""));
	}

	#[test]
	fn indent_size_detection_prefers_the_largest_common_step() {
		assert_eq!(detect_space_indent_size(8, 4), 4);
		assert_eq!(detect_space_indent_size(4, 2), 4);
		assert_eq!(detect_space_indent_size(2, 4), 2);
		assert_eq!(detect_space_indent_size(3, 4), 1);
	}

	#[test]
	fn indentation_level_counts_tabs_one_per_level() {
		assert_eq!(determine_indentation_level("", 4), 0);
		assert_eq!(determine_indentation_level("\t\t", 4), 2);
		assert_eq!(determine_indentation_level("        ", 4), 2);
		// The source file's own step is inferred from the run length, not from
		// the target `indent_size`, so 4 spaces is one level whatever is asked.
		assert_eq!(determine_indentation_level("    ", 2), 1);
		assert_eq!(determine_indentation_level("  ", 2), 1);
		// Mixed indentation adds the tab levels to the space levels.
		assert_eq!(determine_indentation_level("\t    ", 4), 2);
	}

	#[test]
	fn indentation_level_stops_at_the_first_non_whitespace_char() {
		assert_eq!(determine_indentation_level("\tx\t", 4), 1);
	}

	#[test]
	fn spaces_convert_to_one_tab_per_logical_level() {
		assert_eq!(
			convert_indentation_smart("        ", IndentStyle::Tabs, 4),
			"\t\t"
		);
		assert_eq!(convert_indentation_smart("", IndentStyle::Tabs, 4), "");
	}

	#[test]
	fn tabs_convert_to_the_target_space_width() {
		assert_eq!(
			convert_indentation_smart("\t\t", IndentStyle::Spaces, 2),
			"    "
		);
	}

	#[test]
	fn spaces_already_matching_the_target_width_are_left_alone() {
		assert_eq!(
			convert_indentation_smart("    ", IndentStyle::Spaces, 4),
			"    "
		);
	}

	#[test]
	fn apply_indentation_rewrites_only_indented_lines() {
		let content = "fn main() {\n\t\tlet x = 1;\n\n\t}\n";
		let out = apply_indentation(content, IndentStyle::Spaces, 2, false).unwrap();
		assert_eq!(out, "fn main() {\n    let x = 1;\n\n  }\n");
	}

	#[test]
	fn apply_indentation_preserves_crlf_and_the_final_newline() {
		let content = "a\r\n\tb\r\n";
		let out = apply_indentation(content, IndentStyle::Spaces, 4, false).unwrap();
		assert_eq!(out, "a\r\n    b\r\n");

		let no_trailing = "a\n\tb";
		let out = apply_indentation(no_trailing, IndentStyle::Spaces, 4, false).unwrap();
		assert_eq!(out, "a\n    b");
	}

	#[test]
	fn trailing_whitespace_is_trimmed_per_line() {
		assert_eq!(trim_trailing_whitespace("a  \nb\t\n"), "a\nb\n");
		assert_eq!(trim_trailing_whitespace("a  \nb\t"), "a\nb");
		assert_eq!(trim_trailing_whitespace("a  \r\nb\t\r\n"), "a\r\nb\r\n");
		assert_eq!(trim_trailing_whitespace(""), "");
	}

	#[test]
	fn final_newline_is_inserted_only_when_missing() {
		assert_eq!(handle_final_newline("a", true), "a\n");
		assert_eq!(handle_final_newline("a\n", true), "a\n");
		assert_eq!(handle_final_newline("", true), "");
		assert_eq!(handle_final_newline("a\r\n", true), "a\r\n");
	}

	#[test]
	fn stripping_the_final_newline_removes_exactly_one_terminator() {
		// Intentional trailing blank lines must survive; only the last
		// terminator goes.
		assert_eq!(handle_final_newline("a\n\n\n", false), "a\n\n");
		assert_eq!(handle_final_newline("a\r\n", false), "a");
		assert_eq!(handle_final_newline("a\r", false), "a");
		assert_eq!(handle_final_newline("a", false), "a");
	}

	#[test]
	fn indent_size_falls_back_through_tab_width_to_two() {
		assert_eq!(get_effective_indent_size(&Properties::new()), 2);

		let explicit = properties(&[("indent_size", "8")]);
		assert!(matches!(
			explicit.get::<IndentSize>(),
			Ok(IndentSize::Value(8))
		));
		assert_eq!(get_effective_indent_size(&explicit), 8);

		let via_tab_width = properties(&[("indent_size", "tab"), ("tab_width", "6")]);
		assert!(matches!(
			via_tab_width.get::<TabWidth>(),
			Ok(TabWidth::Value(6))
		));
		assert_eq!(get_effective_indent_size(&via_tab_width), 6);

		// indent_size = tab with no tab_width has nothing to fall back on.
		assert_eq!(
			get_effective_indent_size(&properties(&[("indent_size", "tab")])),
			2
		);

		assert_eq!(
			get_effective_indent_size(&properties(&[("tab_width", "3")])),
			3
		);
	}

	#[test]
	fn source_extensions_and_bare_names_are_treated_as_text() {
		assert!(is_likely_text_file(Path::new("src/main.rs")));
		assert!(is_likely_text_file(Path::new("a/App.TSX")));
		assert!(is_likely_text_file(Path::new("Dockerfile")));
		assert!(is_likely_text_file(Path::new("docs/README")));
		assert!(is_likely_text_file(Path::new(".gitignore")));
	}

	#[test]
	fn binary_extensions_are_not_text() {
		assert!(!is_likely_text_file(Path::new("logo.png")));
		assert!(!is_likely_text_file(Path::new("bin/app.exe")));
		assert!(!is_likely_text_file(Path::new("archive.tar.gz")));
	}

	#[test]
	fn an_extensionless_file_with_a_shebang_counts_as_text() {
		let dir = tempfile::TempDir::new().unwrap();
		let script = dir.path().join("runme");
		std::fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();
		assert!(is_likely_text_file(&script));

		let opaque = dir.path().join("blob");
		std::fs::write(&opaque, [0u8, 159, 146, 150]).unwrap();
		assert!(!is_likely_text_file(&opaque));
	}

	#[test]
	fn long_line_check_is_silent_and_side_effect_free() {
		let content = format!("{}\nshort\n", "x".repeat(200));
		check_line_length(&content, 100, Path::new("a.rs"), false);
		check_line_length(&content, 100, Path::new("a.rs"), true);
		check_line_length("short\n", 100, Path::new("a.rs"), true);
	}

	#[test]
	fn git_root_lookup_finds_this_repository() {
		let root = find_git_root().expect("tests run inside the repo");
		assert!(root.join(".git").exists());
	}

	#[test]
	fn git_file_listing_returns_tracked_text_files() {
		let root = Path::new(env!("CARGO_MANIFEST_DIR"));
		let files = get_git_files(root).expect("git ls-files must succeed");
		assert!(!files.is_empty());
		assert!(files.iter().all(|f| f.is_absolute()));
		assert!(files.iter().any(|f| f.ends_with("Cargo.toml")));
	}

	#[test]
	fn text_detection_consults_git_attributes() {
		let root = Path::new(env!("CARGO_MANIFEST_DIR"));
		assert!(is_text_file(&root.join("Cargo.toml")).unwrap());
	}

	fn editorconfig_workspace(rules: &str) -> tempfile::TempDir {
		let dir = tempfile::TempDir::new().unwrap();
		std::fs::write(
			dir.path().join(".editorconfig"),
			format!("root = true\n\n[*]\n{rules}"),
		)
		.unwrap();
		dir
	}

	#[test]
	fn formatting_a_file_reports_and_optionally_applies_changes() {
		let dir = editorconfig_workspace(
			"indent_style = space\nindent_size = 4\ntrim_trailing_whitespace = true\ninsert_final_newline = true\nend_of_line = lf\nmax_line_length = 20\n",
		);
		let file = dir.path().join("sample.rs");
		let original = "fn main() {   \r\n\tlet x = 1;\r\n}";
		std::fs::write(&file, original).unwrap();

		let dry_run_changes = format_file(&file, false, false).unwrap();
		assert!(dry_run_changes > 0);
		assert_eq!(std::fs::read_to_string(&file).unwrap(), original);

		let applied = format_file(&file, true, true).unwrap();
		assert_eq!(applied, dry_run_changes);
		assert_eq!(
			std::fs::read_to_string(&file).unwrap(),
			"fn main() {\n    let x = 1;\n}\n"
		);
		// Re-running on the formatted file is a no-op.
		assert_eq!(format_file(&file, true, false).unwrap(), 0);
	}

	#[test]
	fn crlf_and_final_newline_removal_are_driven_by_editorconfig() {
		let dir = editorconfig_workspace("end_of_line = crlf\ninsert_final_newline = false\n");
		let file = dir.path().join("sample.txt");
		std::fs::write(&file, "a\nb\n").unwrap();

		assert!(format_file(&file, true, false).unwrap() > 0);
		assert_eq!(std::fs::read_to_string(&file).unwrap(), "a\r\nb");
	}

	#[test]
	fn a_file_without_editorconfig_rules_is_never_rewritten() {
		let dir = tempfile::TempDir::new().unwrap();
		std::fs::write(dir.path().join(".editorconfig"), "root = true\n").unwrap();
		let file = dir.path().join("untouched.rs");
		let original = "fn main() {   \n\tlet x = 1;\n}";
		std::fs::write(&file, original).unwrap();

		assert_eq!(format_file(&file, true, true).unwrap(), 0);
		assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
	}

	#[test]
	fn a_clean_file_reports_no_changes() {
		let dir = editorconfig_workspace(
			"indent_style = tab\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n",
		);
		let file = dir.path().join("clean.rs");
		std::fs::write(&file, "fn main() {}\n").unwrap();
		assert_eq!(format_file(&file, true, false).unwrap(), 0);
	}

	#[test]
	fn formatting_a_missing_file_is_an_error() {
		assert!(format_file(Path::new("/nonexistent/nope.rs"), false, false).is_err());
	}
}
