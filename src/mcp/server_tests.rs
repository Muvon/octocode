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
	use crate::grep::GrepMatch;
	use crate::mcp::structural::FileData;
	use serde_json::json;
	use tempfile::TempDir;

	fn grep_match(file: &str, line: usize, text: &str) -> GrepMatch {
		GrepMatch {
			file: file.to_string(),
			line,
			column: 1,
			text: text.to_string(),
			start_byte: 0,
			end_byte: text.len(),
			breadcrumb: None,
		}
	}

	fn file_data(display: &str, content: &str) -> FileData {
		FileData {
			path: std::path::PathBuf::from(display),
			display: display.to_string(),
			content: content.to_string(),
			prefilter_hit: true,
		}
	}

	#[test]
	fn an_empty_match_set_reports_the_diagnostic_or_a_default() {
		assert_eq!(
			format_structural_response(&[], None, None, 0, 50, 0, &[]),
			"No matches found."
		);
		assert_eq!(
			format_structural_response(&[], None, Some("bad pattern"), 0, 50, 0, &[]),
			"bad pattern"
		);
	}

	#[test]
	fn a_single_page_result_is_summarised_by_totals() {
		let matches = vec![
			grep_match("src/a.rs", 1, "fn a() {}"),
			grep_match("src/b.rs", 2, "fn b() {}"),
		];
		let out = format_structural_response(&matches, None, None, 0, 50, 0, &[]);
		assert!(out.ends_with("2 matches in 2 files."), "{out}");
		assert!(out.contains("src/a.rs"));
	}

	#[test]
	fn a_note_is_prepended_and_a_diagnostic_appended() {
		let matches = vec![grep_match("src/a.rs", 1, "fn a() {}")];
		let out = format_structural_response(
			&matches,
			Some("note line"),
			Some("diagnostic line"),
			0,
			50,
			0,
			&[],
		);
		assert!(out.starts_with("note line\n"), "{out}");
		assert!(out.ends_with("diagnostic line"), "{out}");
	}

	#[test]
	fn a_truncated_page_points_at_the_next_offset() {
		let matches: Vec<_> = (1..=10).map(|i| grep_match("src/a.rs", i, "hit")).collect();
		let out = format_structural_response(&matches, None, None, 0, 3, 0, &[]);
		assert!(
			out.contains("Showing 1–3 of 10 matches across 1 files."),
			"{out}"
		);
		assert!(out.contains("Next page: offset=3."), "{out}");
	}

	#[test]
	fn the_last_page_reports_the_range_without_a_next_offset() {
		let matches: Vec<_> = (1..=10).map(|i| grep_match("src/a.rs", i, "hit")).collect();
		let out = format_structural_response(&matches, None, None, 8, 5, 0, &[]);
		assert!(out.contains("Showing 9–10 of 10"), "{out}");
		assert!(!out.contains("Next page"), "{out}");
	}

	#[test]
	fn an_offset_past_the_end_says_so_instead_of_panicking() {
		let matches = vec![grep_match("src/a.rs", 1, "hit")];
		assert_eq!(
			format_structural_response(&matches, None, None, 5, 50, 0, &[]),
			"Offset 5 is beyond the result set (1 total matches)."
		);
	}

	#[test]
	fn requesting_context_pulls_the_surrounding_source() {
		let content = "line one\nline two\nline three\nline four\n";
		let matches = vec![grep_match("src/a.rs", 2, "line two")];
		let out = format_structural_response(
			&matches,
			None,
			None,
			0,
			50,
			1,
			&[
				file_data("src/a.rs", content),
				file_data("src/other.rs", "unused"),
			],
		);
		assert!(out.contains("line one"), "{out}");
		assert!(out.contains("line three"), "{out}");
	}

	#[test]
	fn a_nullable_type_array_collapses_to_the_concrete_type() {
		let mut schema = json!({
			"properties": {
				"max_results": {"type": ["integer", "null"], "description": "count"}
			}
		});
		strip_null_variants(&mut schema);
		assert_eq!(
			schema["properties"]["max_results"]["type"],
			json!("integer")
		);
		assert_eq!(
			schema["properties"]["max_results"]["description"],
			json!("count")
		);
	}

	#[test]
	fn a_nullable_any_of_is_merged_into_the_parent() {
		let mut schema = json!({
			"anyOf": [{"type": "string", "minLength": 1}, {"type": "null"}],
			"description": "field level"
		});
		strip_null_variants(&mut schema);
		assert!(schema.get("anyOf").is_none());
		assert_eq!(schema["type"], json!("string"));
		assert_eq!(schema["minLength"], json!(1));
		// A sibling key already present wins over the merged variant's.
		assert_eq!(schema["description"], json!("field level"));
	}

	#[test]
	fn one_of_is_collapsed_the_same_way_and_nesting_is_walked() {
		let mut schema = json!({
			"properties": {
				"nested": {
					"items": [{"oneOf": [{"type": "number"}, {"type": "null"}]}]
				}
			}
		});
		strip_null_variants(&mut schema);
		assert_eq!(
			schema["properties"]["nested"]["items"][0]["type"],
			json!("number")
		);
	}

	#[test]
	fn a_multi_variant_union_is_left_alone() {
		let mut schema = json!({
			"anyOf": [{"type": "string"}, {"type": "array"}, {"type": "null"}]
		});
		strip_null_variants(&mut schema);
		// Two real branches survive, so there is nothing to collapse into.
		assert_eq!(schema["anyOf"].as_array().unwrap().len(), 2);
	}

	#[test]
	fn scalars_pass_through_unchanged() {
		let mut value = json!("plain");
		strip_null_variants(&mut value);
		assert_eq!(value, json!("plain"));
	}

	#[test]
	fn view_signatures_accepts_a_bare_pattern_or_an_array() {
		let single: ViewSignaturesParams =
			serde_json::from_value(json!({"files": "src/*.rs"})).unwrap();
		assert_eq!(single.files, vec!["src/*.rs".to_string()]);

		let many: ViewSignaturesParams =
			serde_json::from_value(json!({"files": ["a.rs", "b.rs"]})).unwrap();
		assert_eq!(many.files, vec!["a.rs".to_string(), "b.rs".to_string()]);

		assert!(serde_json::from_value::<ViewSignaturesParams>(json!({"files": 7})).is_err());
	}

	#[test]
	fn semantic_search_params_default_the_optional_fields() {
		let params: SemanticSearchParams =
			serde_json::from_value(json!({"query": "how does indexing work"})).unwrap();
		assert_eq!(params.query, json!("how does indexing work"));
		assert!(params.max_results.is_none());
		assert!(params.detail_level.is_none());
		assert!(params.language.is_none());
		assert!(params.mode.is_none());
		assert!(params.threshold.is_none());
	}

	#[test]
	fn find_references_includes_the_declaration_unless_told_otherwise() {
		let default: LspFindReferencesParams =
			serde_json::from_value(json!({"file_path": "a.rs", "line": 3, "symbol": "run"}))
				.unwrap();
		assert!(default.include_declaration);

		let explicit: LspFindReferencesParams = serde_json::from_value(json!({
			"file_path": "a.rs", "line": 3, "symbol": "run", "include_declaration": false
		}))
		.unwrap();
		assert!(!explicit.include_declaration);
	}

	#[test]
	fn the_remaining_lsp_parameter_shapes_deserialize() {
		let position: LspPositionParams =
			serde_json::from_value(json!({"file_path": "a.rs", "line": 1, "symbol": "x"})).unwrap();
		assert_eq!(position.line, 1);

		let document: LspDocumentSymbolsParams =
			serde_json::from_value(json!({"file_path": "a.rs"})).unwrap();
		assert_eq!(document.file_path, "a.rs");

		let workspace: LspWorkspaceSymbolsParams =
			serde_json::from_value(json!({"query": "Store"})).unwrap();
		assert_eq!(workspace.query, "Store");
	}

	#[test]
	fn graphrag_parameters_require_only_the_operation() {
		let params: GraphRagParams =
			serde_json::from_value(json!({"operation": "overview"})).unwrap();
		assert_eq!(params.operation, "overview");
	}

	#[test]
	fn a_non_string_path_filter_is_rejected() {
		assert!(serde_json::from_value::<StructuralSearchParams>(
			json!({"language": "rust", "symbol": "x", "paths": 7})
		)
		.is_err());
	}

	#[test]
	fn a_type_union_without_a_null_branch_is_left_alone() {
		let mut schema = json!({"type": ["integer", "string"]});
		strip_null_variants(&mut schema);
		assert_eq!(schema["type"], json!(["integer", "string"]));
	}

	#[test]
	fn a_result_set_at_the_hard_cap_is_marked_as_truncated() {
		let matches: Vec<_> = (1..=crate::mcp::structural::MAX_TOTAL_MATCHES)
			.map(|i| grep_match("src/a.rs", i, "hit"))
			.collect();
		let out = format_structural_response(&matches, None, None, 0, 5, 0, &[]);
		let footer = out.lines().next_back().unwrap_or_default();
		assert_eq!(
			footer,
			"Showing 1–5 of 10000+ matches across 1 files. Next page: offset=5. \
			 Narrow with `paths`, `inside`, `has`, or metavariable `constraints`."
		);
	}

	// -----------------------------------------------------------------------
	// Per-repo handler (`new_repo_core`): providers and tool router only — no
	// store, no LSP, no background threads.
	// -----------------------------------------------------------------------

	/// A repo directory containing `files`, plus a handler serving it.
	fn repo_server(files: &[(&str, &str)]) -> (TempDir, McpServer) {
		let dir = TempDir::new().unwrap();
		for (name, content) in files {
			std::fs::write(dir.path().join(name), content).unwrap();
		}
		let mut config = Config::default();
		config.index.mcp_index = false;
		let server = McpServer::new_repo_core(config, dir.path().to_path_buf());
		(dir, server)
	}

	fn structural(arguments: serde_json::Value) -> Parameters<StructuralSearchParams> {
		Parameters(serde_json::from_value(arguments).expect("valid structural_search arguments"))
	}

	/// Assert that no nullable form schemars emits for `Option<T>` survived.
	fn assert_no_null_variants(value: &serde_json::Value, path: &str) {
		match value {
			serde_json::Value::Object(obj) => {
				if let Some(types) = obj.get("type").and_then(|t| t.as_array()) {
					assert!(
						!types.iter().any(|t| t.as_str() == Some("null")),
						"{path} still declares a nullable type array"
					);
				}
				for key in ["anyOf", "oneOf"] {
					if let Some(variants) = obj.get(key).and_then(|v| v.as_array()) {
						assert!(
							!variants
								.iter()
								.any(|v| v.get("type").and_then(|t| t.as_str()) == Some("null")),
							"{path}.{key} still has a null branch"
						);
					}
				}
				for (key, nested) in obj {
					assert_no_null_variants(nested, &format!("{path}.{key}"));
				}
			}
			serde_json::Value::Array(items) => {
				for (i, item) in items.iter().enumerate() {
					assert_no_null_variants(item, &format!("{path}[{i}]"));
				}
			}
			_ => {}
		}
	}

	#[test]
	fn a_repo_core_handler_exposes_every_tool_except_lsp() {
		let (_dir, server) = repo_server(&[]);
		let mut names: Vec<String> = server
			.list_tool_defs()
			.iter()
			.map(|t| t.name.to_string())
			.collect();
		names.sort();
		assert_eq!(
			names,
			vec![
				"graphrag",
				"semantic_search",
				"structural_search",
				"view_signatures"
			]
		);
	}

	#[test]
	fn published_tool_schemas_carry_no_nullable_variants() {
		let (_dir, server) = repo_server(&[]);
		let tools = server.list_tool_defs();
		assert!(!tools.is_empty());

		for tool in &tools {
			let schema = serde_json::Value::Object((*tool.input_schema).clone());
			assert_no_null_variants(&schema, tool.name.as_ref());
		}

		// The hand-written union on `query` has no null branch, so it must
		// survive stripping intact.
		let semantic = tools
			.iter()
			.find(|t| t.name.as_ref() == "semantic_search")
			.expect("semantic_search is published");
		let branches = semantic.input_schema["properties"]["query"]["anyOf"]
			.as_array()
			.expect("query keeps its anyOf union");
		assert_eq!(branches.len(), 2);
	}

	#[test]
	fn the_server_info_warns_when_in_process_indexing_is_disabled() {
		let (_dir, server) = repo_server(&[]);
		let info = server.get_info();

		let instructions = info.instructions.as_deref().unwrap();
		assert!(
			instructions.starts_with("NOTE: in-process indexing is disabled"),
			"{instructions}"
		);
		assert_eq!(info.server_info.name, "octocode-mcp");
		assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
		assert_eq!(info.protocol_version, ProtocolVersion::V_2026_07_28);
		assert!(
			info.capabilities.tools.is_some(),
			"the tool capability must be advertised"
		);
	}

	#[test]
	fn the_server_info_describes_the_live_graph_when_indexing_is_enabled() {
		let dir = TempDir::new().unwrap();
		let mut config = Config::default();
		config.index.mcp_index = true;
		let server = McpServer::new_repo_core(config, dir.path().to_path_buf());

		let instructions = server.get_info().instructions.unwrap();
		assert!(
			instructions.starts_with("This server provides semantic search"),
			"{instructions}"
		);
	}

	// -----------------------------------------------------------------------
	// structural_search argument validation and dispatch
	// -----------------------------------------------------------------------

	#[tokio::test]
	async fn structural_search_requires_exactly_one_query_mode() {
		let (_dir, server) = repo_server(&[("a.rs", "fn a() {}\n")]);

		let err = server
			.structural_search(structural(json!({"language": "rust"})))
			.await
			.unwrap_err();
		assert!(err.contains("(got: none)"), "{err}");

		let err = server
			.structural_search(structural(json!({
				"language": "rust", "pattern": "$X.unwrap()", "symbol": "a"
			})))
			.await
			.unwrap_err();
		assert!(err.contains("(got: pattern, symbol)"), "{err}");
	}

	#[tokio::test]
	async fn a_rewrite_without_a_pattern_is_rejected() {
		let (_dir, server) = repo_server(&[("a.rs", "fn a() {}\n")]);
		let err = server
			.structural_search(structural(json!({
				"language": "rust", "symbol": "a", "rewrite": "$X"
			})))
			.await
			.unwrap_err();
		assert_eq!(err, "`rewrite` requires `pattern`.");
	}

	#[tokio::test]
	async fn an_unparseable_metavariable_constraint_names_the_variable() {
		let (_dir, server) = repo_server(&[("a.rs", "fn a() {}\n")]);
		let err = server
			.structural_search(structural(json!({
				"language": "rust", "symbol": "a", "constraints": {"NAME": "["}
			})))
			.await
			.unwrap_err();
		assert!(err.starts_with("Invalid regex for $NAME:"), "{err}");
	}

	#[tokio::test]
	async fn no_candidate_files_explains_which_knob_to_turn() {
		let (_dir, server) = repo_server(&[("a.rs", "fn a() {}\n")]);

		let out = server
			.structural_search(structural(json!({"language": "go", "symbol": "a"})))
			.await
			.unwrap();
		assert!(
			out.starts_with("No go files found. Check that `language`"),
			"{out}"
		);

		let out = server
			.structural_search(structural(json!({
				"language": "rust", "symbol": "a", "paths": "does/not/exist"
			})))
			.await
			.unwrap();
		assert!(
			out.starts_with("No rust files found matching paths [\"does/not/exist\"]."),
			"{out}"
		);
	}

	#[tokio::test]
	async fn a_symbol_wildcard_finds_every_definition_and_counts_the_files() {
		let (_dir, server) = repo_server(&[
			("a.rs", "fn handle_alpha() {}\n"),
			("b.rs", "fn handle_beta() {}\n"),
			("c.rs", "fn unrelated() {}\n"),
		]);

		let out = server
			.structural_search(structural(
				json!({"language": "rust", "symbol": "handle_*"}),
			))
			.await
			.unwrap();
		assert!(out.contains("a.rs"), "{out}");
		assert!(out.contains("b.rs"), "{out}");
		assert!(!out.contains("c.rs"), "{out}");
		assert!(out.ends_with("2 matches in 2 files."), "{out}");
	}

	#[tokio::test]
	async fn an_unchanged_repeat_query_is_served_from_the_cache() {
		let (_dir, server) = repo_server(&[("a.rs", "fn handle_alpha() {}\n")]);
		let arguments = json!({"language": "rust", "symbol": "handle_alpha"});

		let first = server
			.structural_search(structural(arguments.clone()))
			.await
			.unwrap();
		assert!(first.contains("a.rs"), "{first}");

		// Swap the cached matches while leaving the fingerprint and repo stamp
		// intact: an identical follow-up must replay the cache, not search again.
		{
			let mut cache = server.structural_cache.write();
			let entry = cache.as_mut().expect("the first call fills the cache");
			assert_eq!(entry.matches.len(), 1);
			entry.matches = vec![grep_match("cached.rs", 42, "fn from_the_cache() {}")];
			entry.note = Some("replayed".to_string());
		}

		let second = server
			.structural_search(structural(arguments))
			.await
			.unwrap();
		assert!(second.starts_with("replayed\n"), "{second}");
		assert!(second.contains("cached.rs"), "{second}");
	}

	// -----------------------------------------------------------------------
	// structural_search rewrite mode
	// -----------------------------------------------------------------------

	const REWRITE_SOURCE: &str = "fn main() {\n\tlet v = load().unwrap();\n}\n";

	#[tokio::test]
	async fn a_rewrite_previews_the_diff_without_touching_the_file() {
		let (dir, server) = repo_server(&[("a.rs", REWRITE_SOURCE)]);

		let out = server
			.structural_search(structural(json!({
				"language": "rust",
				"pattern": "$X.unwrap()",
				"rewrite": "$X.expect(\"boom\")"
			})))
			.await
			.unwrap();

		assert!(
			out.ends_with("1 replacements across 1 files (preview, set update_all=true to apply)."),
			"{out}"
		);
		assert_eq!(
			std::fs::read_to_string(dir.path().join("a.rs")).unwrap(),
			REWRITE_SOURCE,
			"a preview must not write to disk"
		);
	}

	#[tokio::test]
	async fn applying_a_rewrite_updates_the_file_on_disk() {
		let (dir, server) = repo_server(&[("a.rs", REWRITE_SOURCE)]);

		let out = server
			.structural_search(structural(json!({
				"language": "rust",
				"pattern": "$X.unwrap()",
				"rewrite": "$X.expect(\"boom\")",
				"update_all": true
			})))
			.await
			.unwrap();

		assert_eq!(out, "Applied 1 replacements across 1 files.");
		let on_disk = std::fs::read_to_string(dir.path().join("a.rs")).unwrap();
		assert!(on_disk.contains("load().expect(\"boom\")"), "{on_disk}");
	}

	#[tokio::test]
	async fn a_rewrite_with_nothing_to_match_reports_no_matches() {
		let (dir, server) = repo_server(&[("a.rs", REWRITE_SOURCE)]);

		let out = server
			.structural_search(structural(json!({
				"language": "rust",
				"pattern": "$X.no_such_method()",
				"rewrite": "$X.expect(\"boom\")",
				"update_all": true
			})))
			.await
			.unwrap();

		assert_eq!(out, "No matches found.");
		assert_eq!(
			std::fs::read_to_string(dir.path().join("a.rs")).unwrap(),
			REWRITE_SOURCE
		);
	}

	// -----------------------------------------------------------------------
	// Background service lifecycle
	// -----------------------------------------------------------------------

	/// Four tasks that would each report in after 500ms. Aborting them drops
	/// their sender, so the channel closes without ever delivering a message.
	fn spawned_services() -> (BackgroundServices, mpsc::Receiver<()>) {
		let (tx, rx) = mpsc::channel(4);
		let mut handles: Vec<tokio::task::JoinHandle<()>> = (0..4)
			.map(|_| {
				let tx = tx.clone();
				tokio::spawn(async move {
					sleep(Duration::from_millis(500)).await;
					let _ = tx.send(()).await;
					std::future::pending::<()>().await;
				})
			})
			.collect();
		drop(tx);

		let bg = BackgroundServices {
			watcher_handle: handles.pop(),
			index_handle: handles.pop(),
			indexing_handle: handles.pop(),
			lsp_init_handle: handles.pop(),
		};
		(bg, rx)
	}

	#[test]
	fn an_inert_bundle_owns_no_tasks() {
		let bg = BackgroundServices::none();
		assert!(bg.watcher_handle.is_none());
		assert!(bg.index_handle.is_none());
		assert!(bg.indexing_handle.is_none());
		assert!(bg.lsp_init_handle.is_none());
	}

	#[tokio::test]
	async fn starting_background_services_owns_the_watcher_and_indexing_tasks() {
		crate::store::mod_tests::use_offline_test_config();

		// The database lives outside the watched directory: LanceDB's own writes
		// would otherwise look like source changes to the watcher.
		let db = TempDir::new().unwrap();
		let repo = TempDir::new().unwrap();
		let store = Arc::new(Store::new_with_path(db.path().join("db")).await.unwrap());

		let bg = start_background_services(
			Config::default(),
			store,
			repo.path().to_path_buf(),
			true,
			false,
			None,
			crate::indexer::graphrag::runtime::RuntimeGraphCache::default(),
		)
		.await
		.expect("background services must start on an empty repository");

		assert!(bg.watcher_handle.is_some());
		assert!(bg.index_handle.is_some());
		assert!(bg.indexing_handle.is_some());
		assert!(
			bg.lsp_init_handle.is_none(),
			"the LSP initializer is attached by the caller, not here"
		);
		// Dropping the bundle here aborts all three tasks.
	}

	#[tokio::test]
	async fn shutting_down_aborts_every_background_task() {
		let (bg, mut rx) = spawned_services();
		bg.shutdown().await;

		let outcome = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;
		assert_eq!(
			outcome.expect("the channel must close once every task is aborted"),
			None
		);
	}

	#[tokio::test]
	async fn dropping_the_bundle_aborts_every_background_task() {
		let (bg, mut rx) = spawned_services();
		drop(bg);

		let outcome = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;
		assert_eq!(
			outcome.expect("the channel must close once every task is aborted"),
			None
		);
	}
}
