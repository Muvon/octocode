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

	fn analysis(
		breaking: &[&str],
		features: &[&str],
		fixes: &[&str],
		other: &[&str],
	) -> CommitAnalysis {
		CommitAnalysis {
			commits: vec![],
			breaking_changes: breaking.iter().map(|s| s.to_string()).collect(),
			features: features.iter().map(|s| s.to_string()).collect(),
			fixes: fixes.iter().map(|s| s.to_string()).collect(),
			other_changes: other.iter().map(|s| s.to_string()).collect(),
		}
	}

	fn commit(message: &str, scope: Option<&str>, description: &str) -> CommitInfo {
		CommitInfo {
			hash: "0123456789abcdef".to_string(),
			message: message.to_string(),
			author: "dev".to_string(),
			date: "2026-01-01".to_string(),
			commit_type: "feat".to_string(),
			scope: scope.map(String::from),
			description: description.to_string(),
			breaking: false,
		}
	}

	#[test]
	fn project_type_is_detected_from_the_manifest_that_is_present() {
		let dir = TempDir::new().unwrap();
		assert!(matches!(
			detect_project_type(dir.path()).unwrap(),
			ProjectType::Unknown
		));

		for (file, expect) in [
			("pyproject.toml", "Python (pyproject.toml)"),
			("go.mod", "Go (go.mod)"),
			("composer.json", "PHP (composer.json)"),
			("package.json", "Node.js (package.json)"),
			("Cargo.toml", "Rust (Cargo.toml)"),
		] {
			std::fs::write(dir.path().join(file), "").unwrap();
			let detected = detect_project_type(dir.path()).unwrap();
			assert_eq!(format_project_type(&detected), expect);
		}
	}

	#[test]
	fn format_project_type_covers_the_unknown_case() {
		assert_eq!(
			format_project_type(&ProjectType::Unknown),
			"Unknown (no project file detected)"
		);
	}

	#[test]
	fn a_quoted_version_is_extracted_from_a_manifest_line() {
		assert_eq!(
			extract_version_from_line("version = \"1.2.3\"").as_deref(),
			Some("1.2.3")
		);
		assert_eq!(
			extract_version_from_line("version = '4.5.6'").as_deref(),
			Some("4.5.6")
		);
		assert_eq!(extract_version_from_line("version = 7"), None);
	}

	#[test]
	fn semver_validation_accepts_the_spec_and_rejects_junk() {
		for good in [
			"0.0.0",
			"1.2.3",
			"1.2.3-beta.1",
			"1.2.3+build.5",
			"1.2.3-rc-1+sha.abc",
		] {
			assert!(is_valid_semver(good), "{good} should be valid");
		}
		for bad in [
			"",
			"1.2",
			"1.2.3.4",
			"1.2.x",
			"01.2.3-",
			"1.2.3-",
			"1.2.3-a..b",
			"1.2.3+",
			"v1.2.3",
		] {
			assert!(!is_valid_semver(bad), "{bad} should be invalid");
		}
	}

	#[test]
	fn conventional_commits_are_split_into_type_scope_and_description() {
		let (kind, scope, description, breaking) =
			parse_conventional_commit("feat(api): add endpoint");
		assert_eq!(kind, "feat");
		assert_eq!(scope.as_deref(), Some("api"));
		assert_eq!(description, "add endpoint");
		assert!(!breaking);

		let (kind, scope, _, _) = parse_conventional_commit("fix: correct rounding");
		assert_eq!(kind, "fix");
		assert!(scope.is_none());
	}

	#[test]
	fn only_a_bang_before_the_colon_marks_a_breaking_change() {
		let (_, _, _, breaking) = parse_conventional_commit("feat(api)!: drop v1");
		assert!(breaking);

		let (_, _, _, breaking) = parse_conventional_commit("feat!: drop v1");
		assert!(breaking);

		// A `!` inside the description is not a breaking marker.
		let (_, _, _, breaking) = parse_conventional_commit("fix: prevent panic!() in parser");
		assert!(
			!breaking,
			"an exclamation in prose must not signal breaking"
		);

		let (_, _, _, breaking) = parse_conventional_commit("chore: tidy\n\nBREAKING CHANGE: api");
		assert!(breaking);
	}

	#[test]
	fn a_non_conventional_message_falls_back_to_a_prefix_guess() {
		for (message, expected) in [
			("Feature something", "feat"),
			("fixed the thing", "fix"),
			("docs update", "docs"),
			("style tweak", "style"),
			("refactor loop", "refactor"),
			("tests added", "test"),
			("random words", "chore"),
		] {
			let (kind, scope, description, _) = parse_conventional_commit(message);
			assert_eq!(kind, expected, "for {message}");
			assert!(scope.is_none());
			assert_eq!(description, message);
		}
	}

	#[test]
	fn the_fallback_bump_follows_semver_precedence() {
		let major =
			calculate_version_fallback("1.2.3", &analysis(&["b"], &["f"], &["x"], &[])).unwrap();
		assert_eq!(major.new_version, "2.0.0");
		assert_eq!(major.version_type, "major");

		let minor =
			calculate_version_fallback("1.2.3", &analysis(&[], &["f"], &["x"], &[])).unwrap();
		assert_eq!(minor.new_version, "1.3.0");
		assert_eq!(minor.version_type, "minor");

		let patch = calculate_version_fallback("1.2.3", &analysis(&[], &[], &["x"], &[])).unwrap();
		assert_eq!(patch.new_version, "1.2.4");
		assert_eq!(patch.version_type, "patch");

		let other = calculate_version_fallback("1.2.3", &analysis(&[], &[], &[], &["x"])).unwrap();
		assert_eq!(other.new_version, "1.2.4");

		let nothing = calculate_version_fallback("1.2.3", &analysis(&[], &[], &[], &[])).unwrap();
		assert_eq!(nothing.new_version, "1.2.4");
		assert_eq!(nothing.reasoning, "Miscellaneous changes");
	}

	#[test]
	fn the_fallback_bumps_the_core_of_a_prerelease_version() {
		let bumped =
			calculate_version_fallback("1.2.3-beta.1", &analysis(&[], &["f"], &[], &[])).unwrap();
		assert_eq!(bumped.new_version, "1.3.0");
		assert_eq!(bumped.current_version, "1.2.3-beta.1");
	}

	#[test]
	fn the_fallback_rejects_a_malformed_current_version() {
		assert!(calculate_version_fallback("1.2", &analysis(&[], &[], &[], &[])).is_err());
		assert!(calculate_version_fallback("a.b.c", &analysis(&[], &[], &[], &[])).is_err());
	}

	#[test]
	fn a_changelog_entry_prefers_the_parsed_description_and_shows_the_short_hash() {
		let scoped = format_enhanced_commit_entry(&commit("feat(api): add", Some("api"), "add"));
		assert_eq!(scoped, "- **api**: add `01234567`\n");

		let unscoped = format_enhanced_commit_entry(&commit("feat: add", None, "add"));
		assert_eq!(unscoped, "- add `01234567`\n");

		// With no separate description the raw message is used.
		let raw = format_enhanced_commit_entry(&commit("raw message", None, ""));
		assert_eq!(raw, "- raw message `01234567`\n");
	}

	#[test]
	fn a_toml_field_is_extracted_by_name() {
		let content = "[package]\nname = \"octocode\"\nversion = \"1.0.0\"\n";
		assert_eq!(
			extract_field_from_toml(content, "name").as_deref(),
			Some("octocode")
		);
		assert_eq!(extract_field_from_toml(content, "missing"), None);
	}

	#[test]
	fn a_pyproject_version_is_read_from_project_or_poetry() {
		let project = "[project]\nname = \"x\"\nversion = \"1.0.0\"\n";
		assert_eq!(extract_pyproject_version(project).as_deref(), Some("1.0.0"));

		let poetry = "[tool.poetry]\nversion = '2.0.0'\n";
		assert_eq!(extract_pyproject_version(poetry).as_deref(), Some("2.0.0"));

		// A version outside those sections is not the project version.
		let other = "[tool.black]\nversion = \"9.9.9\"\n";
		assert_eq!(extract_pyproject_version(other), None);
	}

	#[test]
	fn updating_a_pyproject_version_preserves_the_quote_style() {
		let content =
			"[project]\nname = \"x\"\nversion = \"1.0.0\"\n\n[tool.black]\nversion = \"9.9.9\"\n";
		let updated = update_pyproject_version(content, "1.1.0").unwrap();
		assert!(updated.contains("version = \"1.1.0\""));
		// The unrelated section keeps its own value.
		assert!(updated.contains("version = \"9.9.9\""));

		let single =
			update_pyproject_version("[tool.poetry]\nversion = '1.0.0'\n", "1.1.0").unwrap();
		assert!(single.contains("version = '1.1.0'"), "{single}");
	}

	#[test]
	fn updating_a_cargo_version_only_touches_the_package_section() {
		let content = "[package]\nname = \"x\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = { version = \"1.0\" }\n";
		let updated = update_cargo_version(content, "0.2.0").unwrap();
		assert!(updated.contains("version = \"0.2.0\""));
		assert!(updated.contains("serde = { version = \"1.0\" }"));
	}

	#[test]
	fn a_workspace_inherited_version_key_is_left_alone() {
		// `version.workspace = true` starts with "version" but carries no literal
		// to bump; rewriting it would break the manifest.
		let content = "[package]\nname = \"x\"\nversion.workspace = true\n";
		let updated = update_cargo_version(content, "0.2.0").unwrap();
		assert_eq!(updated, content);
	}

	#[test]
	fn json_version_fields_are_replaced_in_place() {
		let content = "{\n  \"name\": \"x\",\n  \"version\": \"1.0.0\"\n}\n";
		let updated = update_json_version(content, "1.1.0", "version").unwrap();
		assert!(updated.contains("\"version\": \"1.1.0\""));
		assert!(updated.contains("\"name\": \"x\""));

		// A field that isn't present leaves the document untouched.
		assert_eq!(
			update_json_version(content, "1.1.0", "nope").unwrap(),
			content
		);
	}

	#[test]
	fn info_plists_are_found_in_the_root_and_immediate_subdirectories() {
		let dir = TempDir::new().unwrap();
		std::fs::write(dir.path().join("Info.plist"), "<plist/>").unwrap();
		for sub in ["App", "target", "node_modules", ".hidden", "Pods"] {
			std::fs::create_dir_all(dir.path().join(sub)).unwrap();
			std::fs::write(dir.path().join(sub).join("Info.plist"), "<plist/>").unwrap();
		}

		let found = find_info_plists(dir.path());
		assert_eq!(found.len(), 2, "got {found:?}");
		assert!(found.iter().any(|p| p.ends_with("App/Info.plist")));
		assert!(!found.iter().any(|p| p.to_string_lossy().contains("target")));
	}

	#[test]
	fn no_plists_are_reported_for_a_bare_directory() {
		let dir = TempDir::new().unwrap();
		assert!(find_info_plists(dir.path()).is_empty());
	}

	#[test]
	fn a_plist_short_version_string_is_rewritten() {
		let content =
			"<dict>\n\t<key>CFBundleShortVersionString</key>\n\t<string>1.0.0</string>\n</dict>";
		let updated = update_plist_version(content, "1.1.0").unwrap();
		assert!(updated.contains("<string>1.1.0</string>"));
	}

	#[test]
	fn a_plist_without_the_version_key_is_an_error() {
		assert!(update_plist_version("<dict/>", "1.1.0").is_err());
		assert!(update_plist_version("<key>CFBundleShortVersionString</key>", "1.1.0").is_err());
	}
}
