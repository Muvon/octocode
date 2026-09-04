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
}
