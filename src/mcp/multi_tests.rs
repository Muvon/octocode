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
	use rmcp::model::Tool;
	use serde_json::{json, Value};
	use tempfile::TempDir;

	/// A root with two git repos, one plain directory and one hidden directory.
	fn root() -> TempDir {
		let dir = TempDir::new().unwrap();
		for name in ["alpha", "beta"] {
			std::fs::create_dir_all(dir.path().join(name).join(".git")).unwrap();
		}
		std::fs::create_dir_all(dir.path().join("plain")).unwrap();
		std::fs::create_dir_all(dir.path().join(".hidden").join(".git")).unwrap();
		std::fs::write(dir.path().join("loose.txt"), "x").unwrap();
		dir
	}

	fn tool(name: &str, schema: Value) -> Tool {
		let object = schema
			.as_object()
			.expect("schema must be an object")
			.clone();
		Tool::new(
			name.to_string(),
			"base description".to_string(),
			std::sync::Arc::new(object),
		)
	}

	#[test]
	fn only_git_repositories_are_discovered_by_default() {
		let dir = root();
		let repos = discover_repos(dir.path(), false).unwrap();
		let mut names: Vec<_> = repos.keys().cloned().collect();
		names.sort();
		assert_eq!(names, vec!["alpha", "beta"]);
		assert_eq!(repos["alpha"], dir.path().join("alpha"));
	}

	#[test]
	fn no_git_mode_accepts_any_visible_subdirectory() {
		let dir = root();
		let repos = discover_repos(dir.path(), true).unwrap();
		let mut names: Vec<_> = repos.keys().cloned().collect();
		names.sort();
		assert_eq!(names, vec!["alpha", "beta", "plain"]);
	}

	#[test]
	fn hidden_directories_and_loose_files_are_never_projects() {
		let dir = root();
		for no_git in [false, true] {
			let repos = discover_repos(dir.path(), no_git).unwrap();
			assert!(!repos.contains_key(".hidden"));
			assert!(!repos.contains_key("loose.txt"));
		}
	}

	#[test]
	fn an_unreadable_root_is_an_error() {
		assert!(discover_repos(std::path::Path::new("/no/such/root"), false).is_err());
		assert!(!has_child_repos(
			std::path::Path::new("/no/such/root"),
			false
		));
	}

	#[test]
	fn child_repo_detection_matches_discovery() {
		let dir = root();
		assert!(has_child_repos(dir.path(), false));

		let empty = TempDir::new().unwrap();
		assert!(!has_child_repos(empty.path(), false));

		// A lone non-git directory only counts in --no-git mode.
		std::fs::create_dir_all(empty.path().join("plain")).unwrap();
		assert!(!has_child_repos(empty.path(), false));
		assert!(has_child_repos(empty.path(), true));
	}

	#[test]
	fn the_project_argument_is_added_to_an_existing_schema() {
		let tools = inject_project_arg(
			vec![tool(
				"semantic_search",
				json!({
					"type": "object",
					"properties": {"query": {"type": "string"}},
					"required": ["query"]
				}),
			)],
			&["alpha".to_string(), "beta".to_string()],
		);

		let schema = &tools[0].input_schema;
		let properties = schema["properties"].as_object().unwrap();
		assert!(
			properties.contains_key("query"),
			"existing properties must survive"
		);
		assert_eq!(properties["project"]["type"], json!("string"));
		assert_eq!(properties["project"]["enum"], json!(["alpha", "beta"]));

		let required = schema["required"].as_array().unwrap();
		assert!(required.contains(&json!("query")));
		assert!(required.contains(&json!("project")));

		let description = tools[0].description.as_deref().unwrap();
		assert!(description.starts_with("base description"));
		assert!(description.contains("Available: alpha, beta."));
	}

	#[test]
	fn a_schema_without_properties_or_required_gets_both() {
		let tools = inject_project_arg(
			vec![tool("noop", json!({"type": "object"}))],
			&["alpha".to_string()],
		);
		let schema = &tools[0].input_schema;
		assert!(schema["properties"]["project"].is_object());
		assert_eq!(schema["required"], json!(["project"]));
	}

	#[test]
	fn injecting_twice_does_not_duplicate_the_requirement() {
		let once = inject_project_arg(
			vec![tool(
				"noop",
				json!({"type": "object", "required": ["project"]}),
			)],
			&["alpha".to_string()],
		);
		assert_eq!(once[0].input_schema["required"], json!(["project"]));
	}

	#[test]
	fn with_no_discovered_projects_the_argument_is_unconstrained() {
		let tools = inject_project_arg(vec![tool("noop", json!({"type": "object"}))], &[]);
		let project = &tools[0].input_schema["properties"]["project"];
		assert!(
			project.get("enum").is_none(),
			"an empty enum would reject every call"
		);
		assert!(tools[0]
			.description
			.as_deref()
			.unwrap()
			.contains("(none discovered)"));
	}

	#[test]
	fn injecting_into_no_tools_yields_no_tools() {
		assert!(inject_project_arg(vec![], &["alpha".to_string()]).is_empty());
	}
}
