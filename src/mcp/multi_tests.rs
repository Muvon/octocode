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

	/// A `MultiServer` over `keys`, wired like `MultiServer::new` but without
	/// initializing logging. `mcp_index` is pinned off so `get_server` builds a
	/// handler without a store or background threads.
	fn multi_server(root: &TempDir, keys: &[&str]) -> MultiServer {
		let repos: HashMap<String, PathBuf> = keys
			.iter()
			.map(|k| {
				let path = root.path().join(k);
				std::fs::create_dir_all(&path).unwrap();
				(k.to_string(), path)
			})
			.collect();

		let mut sorted: Vec<String> = repos.keys().cloned().collect();
		sorted.sort();
		let tools = inject_project_arg(
			vec![tool("semantic_search", json!({"type": "object"}))],
			&sorted,
		);

		let mut config = Config::default();
		config.index.mcp_index = false;

		MultiServer {
			config,
			no_git: true,
			debug: false,
			repos: Arc::new(repos),
			instances: Arc::new(Mutex::new(HashMap::new())),
			tools: Arc::new(tools),
		}
	}

	#[test]
	fn the_project_list_is_sorted_and_comma_separated() {
		let dir = TempDir::new().unwrap();
		let server = multi_server(&dir, &["zeta", "alpha", "mid"]);
		assert_eq!(server.project_list(), "alpha, mid, zeta");
	}

	#[test]
	fn a_root_without_projects_reports_a_placeholder_list() {
		let dir = TempDir::new().unwrap();
		let server = multi_server(&dir, &[]);
		assert_eq!(server.project_list(), "(none discovered)");
	}

	#[test]
	fn the_server_info_advertises_the_repositories_and_the_project_argument() {
		let dir = TempDir::new().unwrap();
		let server = multi_server(&dir, &["beta", "alpha"]);
		let info = server.get_info();

		let instructions = info.instructions.as_deref().unwrap();
		assert!(
			instructions.contains("2 repositories are available (alpha, beta)"),
			"{instructions}"
		);
		assert!(
			instructions.contains("every tool requires a `project` argument"),
			"{instructions}"
		);
		assert_eq!(info.server_info.name, "octocode-mcp");
		assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
		assert_eq!(info.protocol_version, ProtocolVersion::V_2026_07_28);
		assert!(
			info.capabilities.tools.is_some(),
			"multi mode serves tools, so the capability must be advertised"
		);
	}

	#[test]
	fn the_server_info_still_renders_with_no_repositories() {
		let dir = TempDir::new().unwrap();
		let server = multi_server(&dir, &[]);
		let instructions = server.get_info().instructions.unwrap();
		assert!(
			instructions.contains("0 repositories are available ((none discovered))"),
			"{instructions}"
		);
	}

	#[test]
	fn a_tool_is_looked_up_by_name_with_the_project_argument_attached() {
		let dir = TempDir::new().unwrap();
		let server = multi_server(&dir, &["alpha"]);

		let found = server.get_tool("semantic_search").expect("injected tool");
		assert_eq!(found.name.as_ref(), "semantic_search");
		assert_eq!(found.input_schema["required"], json!(["project"]));
		assert_eq!(
			found.input_schema["properties"]["project"]["enum"],
			json!(["alpha"])
		);

		assert!(server.get_tool("semantic_searc").is_none());
	}

	#[tokio::test]
	async fn an_unknown_project_is_rejected_with_the_available_list() {
		let dir = TempDir::new().unwrap();
		let server = multi_server(&dir, &["alpha", "beta"]);

		let Err(err) = server.get_server("gamma").await else {
			panic!("an unknown project must not resolve to a handler");
		};
		assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
		assert!(
			err.message.contains("Unknown project 'gamma'"),
			"{}",
			err.message
		);
		assert!(
			err.message.contains("Available repositories: alpha, beta"),
			"{}",
			err.message
		);
		assert!(
			server.instances.lock().await.is_empty(),
			"a failed lookup must not create an instance"
		);
	}

	#[tokio::test]
	async fn a_repo_handler_is_built_once_per_project_and_reused() {
		let dir = TempDir::new().unwrap();
		let server = multi_server(&dir, &["alpha", "beta"]);

		server.get_server("alpha").await.unwrap();
		let first_seen = server.instances.lock().await["alpha"].last_accessed;

		// Instant has nanosecond resolution; a real pause makes the refreshed
		// timestamp strictly greater.
		tokio::time::sleep(Duration::from_millis(5)).await;
		server.get_server("alpha").await.unwrap();
		server.get_server("beta").await.unwrap();

		let guard = server.instances.lock().await;
		let mut keys: Vec<&str> = guard.keys().map(String::as_str).collect();
		keys.sort();
		assert_eq!(keys, vec!["alpha", "beta"]);
		assert!(
			guard["alpha"].last_accessed > first_seen,
			"serving a cached instance must refresh its idle timer"
		);
	}
}
