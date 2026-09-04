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
}
