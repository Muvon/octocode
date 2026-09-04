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
	use crate::config::Config;
	use crate::mcp::graphrag::{GraphRagProvider, OutputFormat};
	use serde_json::{json, Value};
	use tempfile::TempDir;

	/// A small Rust project whose live Tree-sitter graph has both a file node and
	/// a call edge, so every operation has something real to resolve against.
	/// GraphRAG persistence stays off, so nothing touches LanceDB or embeddings.
	fn project() -> (TempDir, GraphRagProvider) {
		let dir = TempDir::new().unwrap();
		std::fs::create_dir_all(dir.path().join("src")).unwrap();
		std::fs::write(
			dir.path().join("src/helper.rs"),
			"pub fn helper() -> u32 {\n\t7\n}\n",
		)
		.unwrap();
		std::fs::write(
			dir.path().join("src/main.rs"),
			"mod helper;\n\nfn main() {\n\tlet value = helper::helper();\n\tprintln!(\"{value}\");\n}\n",
		)
		.unwrap();

		let mut config = Config::default();
		config.graphrag.enabled = false;
		let provider = GraphRagProvider::new(config, dir.path().to_path_buf());
		(dir, provider)
	}

	#[tokio::test]
	async fn a_missing_operation_is_rejected() {
		let (_dir, provider) = project();
		let err = provider.execute(&json!({})).await.unwrap_err();
		assert_eq!(err.code, -32602);
		assert!(err
			.message
			.contains("Missing required parameter 'operation'"));
	}

	#[tokio::test]
	async fn an_unknown_operation_is_rejected() {
		let (_dir, provider) = project();
		let err = provider
			.execute(&json!({"operation": "explode"}))
			.await
			.unwrap_err();
		assert!(err.message.contains("Invalid operation 'explode'"), "{err}");
	}

	#[tokio::test]
	async fn search_requires_a_non_empty_bounded_query() {
		let (_dir, provider) = project();

		let missing = provider
			.execute(&json!({"operation": "search"}))
			.await
			.unwrap_err();
		assert!(missing
			.message
			.contains("Missing required parameter 'query'"));

		let empty = provider
			.execute(&json!({"operation": "search", "query": "   "}))
			.await
			.unwrap_err();
		assert!(empty.message.contains("must not be empty"));

		let too_long = provider
			.execute(&json!({"operation": "search", "query": "x".repeat(1001)}))
			.await
			.unwrap_err();
		assert!(too_long.message.contains("no more than 1000 characters"));
	}

	#[tokio::test]
	async fn node_operations_require_a_node_id() {
		let (_dir, provider) = project();
		for operation in ["get-node", "get-relationships"] {
			let err = provider
				.execute(&json!({"operation": operation}))
				.await
				.unwrap_err();
			assert!(
				err.message.contains("Missing required parameter 'node_id'"),
				"{err}"
			);
		}
	}

	#[tokio::test]
	async fn find_path_requires_both_endpoints() {
		let (_dir, provider) = project();

		let no_source = provider
			.execute(&json!({"operation": "find-path"}))
			.await
			.unwrap_err();
		assert!(no_source.message.contains("'source_id'"), "{no_source}");

		let no_target = provider
			.execute(&json!({"operation": "find-path", "source_id": "src/main.rs"}))
			.await
			.unwrap_err();
		assert!(no_target.message.contains("'target_id'"), "{no_target}");
	}

	#[tokio::test]
	async fn max_depth_is_bounded() {
		let (_dir, provider) = project();
		for depth in [0, 11] {
			let err = provider
				.execute(&json!({"operation": "overview", "max_depth": depth}))
				.await
				.unwrap_err();
			assert!(err.message.contains("max_depth"), "{err}");
		}
	}

	#[tokio::test]
	async fn an_unknown_output_format_is_rejected() {
		let (_dir, provider) = project();
		let err = provider
			.execute(&json!({"operation": "overview", "format": "yaml"}))
			.await
			.unwrap_err();
		assert!(err.message.contains("Invalid format 'yaml'"), "{err}");
	}

	#[tokio::test]
	async fn an_overview_describes_the_live_graph() {
		let (_dir, provider) = project();
		let out = provider
			.execute(&json!({"operation": "overview"}))
			.await
			.unwrap();
		assert!(!out.is_empty());
	}

	#[tokio::test]
	async fn get_node_resolves_a_file_and_renders_every_format() {
		let (_dir, provider) = project();
		for format in ["text", "json", "markdown"] {
			let out = provider
				.execute(&json!({
					"operation": "get-node",
					"node_id": "src/helper.rs",
					"format": format
				}))
				.await
				.unwrap_or_else(|e| panic!("get-node failed for {format}: {e}"));
			assert!(out.contains("helper"), "{format}: {out}");
		}
	}

	#[tokio::test]
	async fn get_node_reports_an_unknown_id() {
		let (_dir, provider) = project();
		let err = provider
			.execute(&json!({"operation": "get-node", "node_id": "src/nope.rs"}))
			.await
			.unwrap_err();
		assert_eq!(err.code, -32603);
		assert!(err.message.contains("Node not found"), "{err}");
	}

	#[tokio::test]
	async fn get_relationships_renders_every_format() {
		let (_dir, provider) = project();
		for format in ["text", "json", "markdown"] {
			let out = provider
				.execute(&json!({
					"operation": "get-relationships",
					"node_id": "src/main.rs",
					"format": format
				}))
				.await
				.unwrap_or_else(|e| panic!("get-relationships failed for {format}: {e}"));
			assert!(!out.is_empty());
		}
	}

	#[tokio::test]
	async fn get_relationships_reports_an_unknown_id() {
		let (_dir, provider) = project();
		let err = provider
			.execute(&json!({"operation": "get-relationships", "node_id": "src/nope.rs"}))
			.await
			.unwrap_err();
		assert!(err.message.contains("Node not found"), "{err}");
	}

	#[tokio::test]
	async fn find_path_reports_unknown_endpoints() {
		let (_dir, provider) = project();
		let err = provider
			.execute(&json!({
				"operation": "find-path",
				"source_id": "src/nope.rs",
				"target_id": "src/helper.rs"
			}))
			.await
			.unwrap_err();
		assert!(err.message.contains("Source node not found"), "{err}");

		let err = provider
			.execute(&json!({
				"operation": "find-path",
				"source_id": "src/main.rs",
				"target_id": "src/nope.rs"
			}))
			.await
			.unwrap_err();
		assert!(err.message.contains("Target node not found"), "{err}");
	}

	#[tokio::test]
	async fn find_path_between_known_nodes_returns_output() {
		let (_dir, provider) = project();
		let out = provider
			.execute(&json!({
				"operation": "find-path",
				"source_id": "src/main.rs",
				"target_id": "src/helper.rs",
				"max_depth": 4
			}))
			.await
			.unwrap();
		assert!(!out.is_empty());
	}

	#[tokio::test]
	async fn search_renders_every_format() {
		let (_dir, provider) = project();
		for format in ["text", "json", "markdown"] {
			let out = provider
				.execute(&json!({
					"operation": "search",
					"query": "helper",
					"format": format
				}))
				.await
				.unwrap_or_else(|e| panic!("search failed for {format}: {e}"));
			assert!(!out.is_empty());
		}
	}

	#[tokio::test]
	async fn a_project_without_supported_sources_reports_an_empty_graph() {
		let dir = TempDir::new().unwrap();
		std::fs::write(dir.path().join("notes.bin"), [0u8, 1, 2, 3]).unwrap();

		let mut config = Config::default();
		config.graphrag.enabled = false;
		let provider = GraphRagProvider::new(config, dir.path().to_path_buf());

		let err = provider
			.execute(&json!({"operation": "overview"}))
			.await
			.unwrap_err();
		assert!(err.message.contains("No supported source symbols"), "{err}");
	}

	#[tokio::test]
	async fn the_runtime_cache_handle_is_shareable() {
		let (dir, provider) = project();
		let cache = provider.runtime_cache();
		assert!(cache.graph(dir.path()).await.is_ok());
	}

	#[test]
	fn each_output_format_answers_only_to_its_own_predicate() {
		let formats = [
			OutputFormat::Text,
			OutputFormat::Json,
			OutputFormat::Md,
			OutputFormat::Cli,
		];
		let expected = [
			// (is_text, is_json, is_md, is_cli)
			(true, false, false, false),
			(false, true, false, false),
			(false, false, true, false),
			(false, false, false, true),
		];

		for (format, (text, json, md, cli)) in formats.iter().zip(expected) {
			assert_eq!(format.is_text(), text, "{format:?}");
			assert_eq!(format.is_json(), json, "{format:?}");
			assert_eq!(format.is_md(), md, "{format:?}");
			assert_eq!(format.is_cli(), cli, "{format:?}");
		}
	}

	#[tokio::test]
	async fn the_depth_bounds_are_inclusive_and_the_default_is_used_for_junk() {
		let (_dir, provider) = project();
		for depth in [1, 10] {
			provider
				.execute(&json!({"operation": "overview", "max_depth": depth}))
				.await
				.unwrap_or_else(|e| panic!("depth {depth} should be accepted: {e}"));
		}

		// A non-numeric depth is not an error: the documented default applies.
		provider
			.execute(&json!({"operation": "overview", "max_depth": "3"}))
			.await
			.expect("a non-numeric depth falls back to the default");
	}

	#[tokio::test]
	async fn an_overview_reports_the_graph_mode_and_its_counts() {
		let (_dir, provider) = project();

		let text = provider
			.execute(&json!({"operation": "overview"}))
			.await
			.unwrap();
		// GraphRAG persistence is off, so the answer comes from the live graph.
		assert!(
			text.starts_with("Code Graph Overview (runtime_structural): "),
			"{text}"
		);
		assert!(text.contains("Node Types:\n"), "{text}");
		assert!(text.contains("Relationship Types:\n"), "{text}");

		let markdown = provider
			.execute(&json!({"operation": "overview", "format": "markdown"}))
			.await
			.unwrap();
		assert!(
			markdown.starts_with("# Code Graph Overview\n\nMode: `runtime_structural`\n"),
			"{markdown}"
		);
		assert!(markdown.contains("## Node Types\n"), "{markdown}");

		let raw = provider
			.execute(&json!({"operation": "overview", "format": "json"}))
			.await
			.unwrap();
		let parsed: Value = serde_json::from_str(&raw).expect("json format must parse");
		assert_eq!(parsed["mode"], "runtime_structural");
		assert!(
			parsed["node_count"].as_u64().unwrap() >= 2,
			"both source files are nodes: {raw}"
		);
		assert!(parsed["node_types"].is_object(), "{raw}");
		assert!(parsed["relationship_types"].is_object(), "{raw}");
	}

	#[tokio::test]
	async fn a_node_is_rendered_with_every_field_in_each_format() {
		let (_dir, provider) = project();

		let text = provider
			.execute(&json!({"operation": "get-node", "node_id": "src/helper.rs"}))
			.await
			.unwrap();
		assert!(text.starts_with("Node: "), "{text}");
		for label in [
			"\nID: ",
			"\nKind: ",
			"\nPath: ",
			"\nDescription: ",
			"\nSymbols: ",
		] {
			assert!(text.contains(label), "missing {label:?} in: {text}");
		}

		let markdown = provider
			.execute(
				&json!({"operation": "get-node", "node_id": "src/helper.rs", "format": "markdown"}),
			)
			.await
			.unwrap();
		assert!(markdown.starts_with("# Node: "), "{markdown}");
		assert!(markdown.contains("**Kind:**"), "{markdown}");

		let raw = provider
			.execute(
				&json!({"operation": "get-node", "node_id": "src/helper.rs", "format": "json"}),
			)
			.await
			.unwrap();
		let parsed: Value = serde_json::from_str(&raw).expect("json format must parse");
		assert!(parsed["id"].is_string(), "{raw}");
		assert!(parsed["name"].is_string(), "{raw}");
		assert!(parsed["path"].is_string(), "{raw}");
	}

	#[tokio::test]
	async fn relationships_are_returned_as_structured_edges_in_json() {
		let (_dir, provider) = project();
		let raw = provider
			.execute(
				&json!({"operation": "get-relationships", "node_id": "src/main.rs", "format": "json"}),
			)
			.await
			.unwrap();

		let parsed: Value = serde_json::from_str(&raw).expect("json format must parse");
		let edges = parsed.as_array().expect("an array of relationships");
		assert!(!edges.is_empty(), "main.rs calls helper: {raw}");
		for edge in edges {
			assert!(edge["source"].is_string(), "{edge}");
			assert!(edge["target"].is_string(), "{edge}");
			assert!(edge["relation_type"].is_string(), "{edge}");
		}
	}

	#[tokio::test]
	async fn a_search_answers_with_the_nodes_it_matched() {
		let (_dir, provider) = project();

		let raw = provider
			.execute(&json!({"operation": "search", "query": "helper", "format": "json"}))
			.await
			.unwrap();
		let parsed: Value = serde_json::from_str(&raw).expect("json format must parse");
		let nodes = parsed.as_array().expect("an array of nodes");
		assert!(!nodes.is_empty(), "`helper` exists in the corpus: {raw}");
		assert!(
			nodes
				.iter()
				.any(|node| node["path"].as_str() == Some("src/helper.rs")),
			"{raw}"
		);
		// The search is capped, so it can never flood the model's context.
		assert!(nodes.len() <= 50, "{raw}");
	}
}
