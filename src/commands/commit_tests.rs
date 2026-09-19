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

	fn context(files_section: &str) -> CommitPromptContext {
		CommitPromptContext {
			file_count: 2,
			additions: 12,
			deletions: 3,
			guidance_section: "Guidance line.\n".to_string(),
			docs_restriction: "Docs only.\n".to_string(),
			files_section: files_section.to_string(),
		}
	}

	#[test]
	fn each_git_status_marker_is_reported() {
		let diff = "\
diff --git a/src/added.rs b/src/added.rs
new file mode 100644
--- /dev/null
+++ b/src/added.rs
diff --git a/src/gone.rs b/src/gone.rs
deleted file mode 100644
diff --git a/src/old.rs b/src/new.rs
rename from src/old.rs
rename to src/new.rs
diff --git a/src/edited.rs b/src/edited.rs
index 111..222 100644
";
		let section = file_status_section(diff);
		assert!(section.starts_with("File status (A=added"), "{section}");
		assert!(section.contains("A src/added.rs"), "{section}");
		assert!(section.contains("D src/gone.rs"), "{section}");
		assert!(section.contains("R src/new.rs"), "{section}");
		assert!(section.contains("M src/edited.rs"), "{section}");
	}

	#[test]
	fn a_diff_with_no_file_headers_yields_an_empty_section() {
		assert_eq!(file_status_section(""), "");
		assert_eq!(file_status_section("just some text\n"), "");
	}

	#[test]
	fn the_commit_prompt_carries_the_diff_and_its_context() {
		let prompt = create_commit_prompt(
			"diff --git a/src/a.rs b/src/a.rs",
			&context("M src/a.rs\n"),
			"M src/a.rs\n",
			DIFF_SOURCE_LABEL,
		);
		assert!(prompt.contains("diff --git a/src/a.rs"), "{prompt}");
		assert!(prompt.contains("M src/a.rs"), "{prompt}");
		assert!(prompt.contains("Guidance line."), "{prompt}");
		assert!(prompt.contains("Docs only."), "{prompt}");
	}

	#[test]
	fn a_chunk_specific_file_section_overrides_the_shared_one() {
		let prompt = create_commit_prompt(
			"diff",
			&context("M src/a.rs\n"),
			"chunk 2 of 3\nM src/b.rs\n",
			DIFF_SOURCE_LABEL,
		);
		assert!(prompt.contains("chunk 2 of 3"), "{prompt}");
		assert!(!prompt.contains("M src/a.rs"), "{prompt}");
	}

	#[test]
	fn a_precommit_config_is_detected_under_either_extension() {
		let dir = TempDir::new().unwrap();
		assert!(!has_precommit_config(dir.path()));

		std::fs::write(dir.path().join(".pre-commit-config.yaml"), "repos: []\n").unwrap();
		assert!(has_precommit_config(dir.path()));

		let other = TempDir::new().unwrap();
		std::fs::write(other.path().join(".pre-commit-config.yml"), "repos: []\n").unwrap();
		assert!(has_precommit_config(other.path()));
	}

	#[test]
	fn precommit_availability_is_probed_without_failing() {
		// The answer depends on the machine; the call must simply not panic.
		let _ = is_precommit_available();
	}

	fn draft(
		kind: &str,
		scope: &str,
		subject: &str,
		effect: &str,
		changes: &[&str],
		breaking: &str,
	) -> CommitDraft {
		CommitDraft {
			kind: kind.to_string(),
			scope: scope.to_string(),
			subject: subject.to_string(),
			effect: effect.to_string(),
			changes: changes.iter().map(|c| c.to_string()).collect(),
			breaking: breaking.to_string(),
		}
	}

	#[test]
	fn a_single_purpose_draft_renders_as_a_bare_subject() {
		let message = render_commit_message(&draft(
			"Fix",
			"index",
			"Checkpoint commit indexing after each batch.",
			"",
			&[],
			"",
		))
		.unwrap();
		assert_eq!(
			message,
			"fix(index): checkpoint commit indexing after each batch"
		);
	}

	#[test]
	fn an_acronym_at_the_start_of_the_subject_keeps_its_case() {
		let message =
			render_commit_message(&draft("feat", "", "ONNX providers are listed", "", &[], ""))
				.unwrap();
		assert_eq!(message, "feat: ONNX providers are listed");
	}

	#[test]
	fn a_multi_part_draft_renders_effect_bullets_and_breaking_footer() {
		let message = render_commit_message(&draft(
			"feat",
			"",
			"add OAuth2 login",
			"Password login is being retired.",
			&["- Add token refresh", "**Store** sessions in the DB", ""],
			"Login now requires a token.",
		))
		.unwrap();
		assert_eq!(
			message,
			"feat!: add OAuth2 login\n\n\
			 Password login is being retired.\n\n\
			 - Add token refresh\n\
			 - Store sessions in the DB\n\n\
			 BREAKING CHANGE: Login now requires a token."
		);
	}

	#[test]
	fn an_unknown_type_an_empty_subject_and_an_overlong_subject_are_rejected() {
		assert!(render_commit_message(&draft("feature", "", "add x", "", &[], "")).is_err());
		assert!(render_commit_message(&draft("feat", "", "```", "", &[], "")).is_err());
		let long = "a".repeat(80);
		assert!(render_commit_message(&draft("feat", "", &long, "", &[], "")).is_err());
	}

	#[test]
	fn body_text_wraps_at_72_with_a_bullet_continuation_indent() {
		let words = vec!["word"; 20].join(" ");
		let wrapped = wrap_text(&format!("- {}", words), 72, "  ");
		let lines: Vec<&str> = wrapped.lines().collect();
		assert_eq!(lines.len(), 2, "{wrapped}");
		assert!(
			lines[0].len() <= 72 && lines[0].starts_with("- word"),
			"{wrapped}"
		);
		assert!(lines[1].starts_with("  word"), "{wrapped}");
		assert_eq!(wrap_text("short", 72, "  "), "short");
	}

	#[test]
	fn the_prompt_context_is_derived_from_the_diff_headers() {
		let diff = "diff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n+line\n\
		            diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n-old\n+new\n";
		let ctx = build_prompt_context(diff, Some("hint"));
		assert_eq!(ctx.file_count, 2);
		assert_eq!((ctx.additions, ctx.deletions), (2, 1));
		assert!(
			ctx.docs_restriction.contains("primary change"),
			"{}",
			ctx.docs_restriction
		);
		assert!(ctx.guidance_section.contains("hint"));
		assert!(ctx.files_section.contains("M src/a.rs"));

		let code_only = build_prompt_context("diff --git a/src/a.rs b/src/a.rs\n+x\n", None);
		assert!(code_only.docs_restriction.contains("must not be docs"));
		assert!(code_only.guidance_section.is_empty());
	}
}
