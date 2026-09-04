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
	use crate::commands::OutputFormat;
	use std::path::Path;
	use std::process::Command;
	use tempfile::TempDir;

	fn git(repo: &Path, args: &[&str]) -> String {
		let out = Command::new("git")
			.args(args)
			.current_dir(repo)
			.output()
			.unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
		assert!(
			out.status.success(),
			"git {args:?} failed: {}",
			String::from_utf8_lossy(&out.stderr)
		);
		String::from_utf8_lossy(&out.stdout).trim().to_string()
	}

	/// A repo with two commits on `main` and a `feature` branch adding a third.
	fn repo() -> TempDir {
		let dir = TempDir::new().unwrap();
		let path = dir.path();
		git(path, &["init", "-q", "-b", "main"]);
		git(path, &["config", "user.email", "test@example.com"]);
		git(path, &["config", "user.name", "Test"]);

		std::fs::write(path.join("first.txt"), "one\n").unwrap();
		git(path, &["add", "."]);
		git(path, &["commit", "-q", "-m", "root commit"]);

		std::fs::write(path.join("second.txt"), "two\n").unwrap();
		git(path, &["add", "."]);
		git(path, &["commit", "-q", "-m", "second commit"]);

		git(path, &["checkout", "-q", "-b", "feature"]);
		std::fs::write(path.join("third.txt"), "three\n").unwrap();
		git(path, &["add", "."]);
		git(path, &["commit", "-q", "-m", "feature commit"]);
		git(path, &["checkout", "-q", "main"]);

		dir
	}

	fn args(target: Option<&str>, staged: bool) -> DiffArgs {
		DiffArgs {
			target: target.map(String::from),
			staged,
			format: OutputFormat::Cli,
		}
	}

	#[test]
	fn a_clean_working_tree_produces_an_empty_diff() {
		let dir = repo();
		let (diff, files, label) = get_diff(dir.path(), &args(None, false)).unwrap();
		assert!(diff.trim().is_empty());
		assert!(files.is_empty());
		assert_eq!(label, "Working changes");
	}

	#[test]
	fn unstaged_edits_show_up_in_the_working_diff_but_not_the_staged_one() {
		let dir = repo();
		std::fs::write(dir.path().join("first.txt"), "one changed\n").unwrap();

		let (diff, files, label) = get_diff(dir.path(), &args(None, false)).unwrap();
		assert!(diff.contains("one changed"));
		assert_eq!(files, vec!["first.txt".to_string()]);
		assert_eq!(label, "Working changes");

		let (staged_diff, staged_files, staged_label) =
			get_diff(dir.path(), &args(None, true)).unwrap();
		assert!(staged_diff.trim().is_empty());
		assert!(staged_files.is_empty());
		assert_eq!(staged_label, "Staged changes");
	}

	#[test]
	fn staging_moves_the_change_into_the_staged_diff() {
		let dir = repo();
		std::fs::write(dir.path().join("first.txt"), "one changed\n").unwrap();
		git(dir.path(), &["add", "first.txt"]);

		let (diff, files, _) = get_diff(dir.path(), &args(None, true)).unwrap();
		assert!(diff.contains("one changed"));
		assert_eq!(files, vec!["first.txt".to_string()]);
	}

	#[test]
	fn a_commit_range_diffs_the_two_endpoints() {
		let dir = repo();
		let (diff, files, label) =
			get_diff(dir.path(), &args(Some("main..feature"), false)).unwrap();
		assert!(diff.contains("third.txt"));
		assert_eq!(files, vec!["third.txt".to_string()]);
		assert_eq!(label, "Range: main..feature");
	}

	#[test]
	fn a_commit_hash_diffs_that_commit_against_its_parent() {
		let dir = repo();
		let head = git(dir.path(), &["rev-parse", "HEAD"]);
		let (diff, files, label) = get_diff(dir.path(), &args(Some(&head), false)).unwrap();
		assert!(diff.contains("second.txt"));
		assert_eq!(files, vec!["second.txt".to_string()]);
		assert!(label.starts_with(&format!("Commit: {} ", &head[..7])));
		assert!(label.ends_with("second commit"));
	}

	#[test]
	fn the_root_commit_is_diffed_against_the_empty_tree() {
		// `root^` does not resolve, so without the empty-tree fallback the root
		// commit would silently fall through to a branch diff and come back empty.
		let dir = repo();
		let root = git(dir.path(), &["rev-list", "--max-parents=0", "HEAD"]);
		let (diff, files, label) = get_diff(dir.path(), &args(Some(&root), false)).unwrap();
		assert!(diff.contains("first.txt"), "root commit diff was empty");
		assert_eq!(files, vec!["first.txt".to_string()]);
		assert!(label.ends_with("root commit"));
	}

	#[test]
	fn a_branch_name_diffs_against_the_default_branch() {
		// A branch also resolves as `branch^`, so it must be recognised as a branch
		// first — otherwise only its tip commit would be reported.
		let dir = repo();
		let (diff, files, label) = get_diff(dir.path(), &args(Some("feature"), false)).unwrap();
		assert!(diff.contains("third.txt"));
		assert_eq!(files, vec!["third.txt".to_string()]);
		assert_eq!(label, "Branch: feature vs main");
	}

	#[test]
	fn an_unknown_target_falls_back_to_a_branch_diff_and_fails_loudly() {
		let dir = repo();
		let err = get_diff(dir.path(), &args(Some("no-such-ref"), false))
			.expect_err("an unresolvable target must not be reported as an empty diff");
		assert!(!err.to_string().is_empty());
	}

	#[test]
	fn run_git_surfaces_command_failures() {
		let dir = repo();
		assert!(run_git(dir.path(), &["rev-parse", "does-not-exist"]).is_err());
		// stdout is returned verbatim — callers split on lines or trim themselves.
		assert_eq!(
			run_git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap(),
			"main\n"
		);
	}

	#[test]
	fn titles_longer_than_the_column_are_truncated_on_a_char_boundary() {
		assert_eq!(truncate_str("short", 10), "short");
		assert_eq!(truncate_str("abcdefghij", 10), "abcdefghij");
		assert_eq!(truncate_str("abcdefghijk", 10), "abcdefg...");
		// Multi-byte input must never be sliced mid-character.
		let wide = "日本語のタイトル";
		let out = truncate_str(wide, 10);
		assert!(out.ends_with("..."));
		assert!(wide.starts_with(out.trim_end_matches("...")));
	}

	#[test]
	fn both_renderers_handle_every_risk_level_and_an_empty_card_list() {
		let analysis: DiffAnalysis = serde_json::from_value(serde_json::json!({
			"summary": "summary line",
			"risk": "high",
			"changes": [
				{"title": "a", "risk": "high", "what_changed": ["x"], "impact": "y"},
				{"title": "b", "risk": "medium", "what_changed": [], "impact": "y", "uncertain": "z"},
				{"title": "c", "risk": "low", "what_changed": ["x"], "impact": "y"}
			]
		}))
		.unwrap();
		print_cli(&analysis, "label");
		print_markdown(&analysis, "label");

		for risk in ["medium", "low", "unrecognised"] {
			let other: DiffAnalysis = serde_json::from_value(serde_json::json!({
				"summary": "s", "risk": risk, "changes": []
			}))
			.unwrap();
			print_cli(&other, "label");
			print_markdown(&other, "label");
		}
	}
}
