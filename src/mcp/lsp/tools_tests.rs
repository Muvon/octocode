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
	use crate::mcp::lsp::provider::LspProvider;
	use lsp_types::*;
	use serde_json::json;
	use std::path::PathBuf;
	use std::str::FromStr;

	/// A workspace root that is a genuine absolute path on the host. Windows has
	/// no rootless absolute path, so `file:///repo/...` is not a file URI there
	/// and `Url::to_file_path` rejects it — the formatters would then fall back
	/// to the raw URI and these assertions would be describing that fallback
	/// rather than the relative-path rendering they are about.
	const ROOT: &str = if cfg!(windows) { "C:/repo" } else { "/repo" };
	const ROOT_URI: &str = if cfg!(windows) {
		"file:///C:/repo"
	} else {
		"file:///repo"
	};

	/// The response helpers only read `working_directory`, so a provider that
	/// never starts a server is enough to exercise all of them.
	fn provider() -> LspProvider {
		LspProvider::new(PathBuf::from(ROOT), "unused-command".to_string())
	}

	fn uri(text: &str) -> Uri {
		Uri::from_str(text).unwrap()
	}

	fn range(start_line: u32, start_char: u32, end_line: u32, end_char: u32) -> Range {
		Range {
			start: Position {
				line: start_line,
				character: start_char,
			},
			end: Position {
				line: end_line,
				character: end_char,
			},
		}
	}

	fn location(path: &str, line: u32, character: u32) -> Location {
		Location {
			uri: uri(path),
			range: range(line, character, line, character + 4),
		}
	}

	#[test]
	fn a_path_inside_the_workspace_is_reported_relative() {
		let provider = provider();
		assert_eq!(
			provider.make_path_relative(std::path::Path::new(&format!("{ROOT}/src/lib.rs"))),
			"src/lib.rs"
		);
		// Anything outside the workspace keeps its absolute form.
		assert_eq!(
			provider.make_path_relative(std::path::Path::new("/elsewhere/lib.rs")),
			"/elsewhere/lib.rs"
		);
	}

	#[test]
	fn goto_definition_formatting_uses_the_first_location_in_one_based_coordinates() {
		let provider = provider();
		assert_eq!(
			provider.format_goto_definition_response(&[]),
			"No definition found"
		);

		let out = provider.format_goto_definition_response(&[
			location(&format!("{ROOT_URI}/src/lib.rs"), 0, 7),
			location(&format!("{ROOT_URI}/src/other.rs"), 9, 1),
		]);
		assert_eq!(out, "Definition found at src/lib.rs:1:8");
	}

	#[test]
	fn goto_definition_formatting_falls_back_to_the_raw_uri_for_non_file_schemes() {
		let provider = provider();
		let out = provider.format_goto_definition_response(&[Location {
			uri: uri("untitled:Untitled-1"),
			range: range(2, 0, 2, 1),
		}]);
		assert_eq!(out, "Definition found at untitled:Untitled-1:3:1");
	}

	#[test]
	fn hover_formatting_strips_markdown_and_reports_a_range_when_present() {
		let provider = provider();

		let with_range = provider.format_hover_response(&Hover {
			contents: HoverContents::Scalar(MarkedString::String(
				"```rust\n**fn** helper() -> u32\n```".to_string(),
			)),
			range: Some(range(0, 6, 0, 12)),
		});
		assert_eq!(with_range, "Hover info (1:7-1:13):\nfn helper() -> u32");

		let without_range = provider.format_hover_response(&Hover {
			contents: HoverContents::Scalar(MarkedString::String("plain text".to_string())),
			range: None,
		});
		assert_eq!(without_range, "Hover info:\nplain text");
	}

	#[test]
	fn every_hover_content_shape_is_extracted() {
		let provider = provider();

		assert_eq!(
			provider.extract_hover_contents(&HoverContents::Scalar(MarkedString::String(
				"scalar".to_string()
			))),
			"scalar"
		);
		assert_eq!(
			provider.extract_hover_contents(&HoverContents::Scalar(MarkedString::LanguageString(
				LanguageString {
					language: "rust".to_string(),
					value: "fn helper()".to_string(),
				}
			))),
			"fn helper()"
		);
		assert_eq!(
			provider.extract_hover_contents(&HoverContents::Array(vec![
				MarkedString::String("first".to_string()),
				MarkedString::LanguageString(LanguageString {
					language: "rust".to_string(),
					value: "second".to_string(),
				}),
			])),
			"first\n\nsecond"
		);
		assert_eq!(
			provider.extract_hover_contents(&HoverContents::Markup(MarkupContent {
				kind: MarkupKind::Markdown,
				value: "markup".to_string(),
			})),
			"markup"
		);
	}

	#[test]
	fn references_are_numbered_in_one_based_coordinates() {
		let provider = provider();
		assert_eq!(
			provider.format_references_response(&[]),
			"No references found"
		);

		let out = provider.format_references_response(&[
			location(&format!("{ROOT_URI}/src/lib.rs"), 0, 7),
			location(&format!("{ROOT_URI}/src/lib.rs"), 4, 1),
		]);
		assert_eq!(
			out,
			"Found 2 reference(s):\n1. src/lib.rs:1:8\n2. src/lib.rs:5:2"
		);
	}

	#[test]
	fn document_symbols_are_rendered_with_a_lowercase_kind() {
		let provider = provider();
		assert_eq!(
			provider.format_document_symbols_response(&[]),
			"No symbols found in document"
		);

		let out = provider.format_document_symbols_response(&[
			json!({"name": "helper", "kind": "SymbolKind::FUNCTION", "line": 1, "character": 8}),
			json!({"missing": "everything"}),
		]);
		assert_eq!(
			out,
			"Found 2 symbol(s):\n1. helper (function) at 1:8\n2. unknown (unknown) at 0:0"
		);
	}

	#[test]
	fn workspace_symbols_are_rendered_with_their_file() {
		let provider = provider();
		assert_eq!(
			provider.format_workspace_symbols_response(&[]),
			"No symbols found in workspace"
		);

		let out = provider.format_workspace_symbols_response(&[json!({
			"name": "helper",
			"kind": "SymbolKind::FUNCTION",
			"file_path": "src/lib.rs",
			"line": 1
		})]);
		assert_eq!(
			out,
			"Found 1 symbol(s) in workspace:\n1. helper (function) in src/lib.rs:1"
		);
	}

	#[test]
	fn completions_are_capped_at_ten_with_a_remainder_line() {
		let provider = provider();
		assert_eq!(
			provider.format_completion_response(&[]),
			"No completions available"
		);

		let items: Vec<serde_json::Value> = (0..12)
			.map(|i| json!({"label": format!("item{}", i)}))
			.collect();
		let out = provider.format_completion_response(&items);
		assert!(out.starts_with("Found 12 completion(s):\n"), "{out}");
		assert!(out.contains("\n10. item9\n"), "{out}");
		assert!(!out.contains("item10"), "{out}");
		assert!(out.ends_with("... and 2 more completions"), "{out}");
	}

	#[test]
	fn a_completion_detail_is_shown_only_when_it_is_short() {
		let provider = provider();
		let out = provider.format_completion_response(&[
			json!({"label": "helper", "kind": "CompletionItemKind::FUNCTION", "detail": "fn() -> u32"}),
			json!({"label": "verbose", "detail": "x".repeat(50)}),
		]);
		assert_eq!(
			out,
			"Found 2 completion(s):\n1. helper (function) - fn() -> u32\n2. verbose"
		);
	}

	#[test]
	fn goto_definition_parsing_accepts_every_documented_response_shape() {
		let provider = provider();

		assert!(provider
			.parse_goto_definition_response(json!(null))
			.unwrap()
			.is_empty());

		let single = provider
			.parse_goto_definition_response(json!({
				"uri": "file:///repo/src/lib.rs",
				"range": {"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 13}}
			}))
			.unwrap();
		assert_eq!(single.len(), 1);
		assert_eq!(single[0].range.start.character, 7);

		let many = provider
			.parse_goto_definition_response(json!([
				{"uri": "file:///repo/a.rs", "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 2}}},
				{"uri": "file:///repo/b.rs", "range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 2}}}
			]))
			.unwrap();
		assert_eq!(many.len(), 2);
		assert_eq!(many[1].range.start.line, 2);
	}

	#[test]
	fn a_location_link_collapses_to_its_target_selection_range() {
		let provider = provider();
		let links = provider
			.parse_goto_definition_response(json!([{
				"originSelectionRange": {"start": {"line": 5, "character": 1}, "end": {"line": 5, "character": 7}},
				"targetUri": "file:///repo/src/lib.rs",
				"targetRange": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 1}},
				"targetSelectionRange": {"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 13}}
			}]))
			.unwrap();
		assert_eq!(links.len(), 1);
		// The narrow selection range wins over the full target range.
		assert_eq!(links[0].range.start.line, 0);
		assert_eq!(links[0].range.start.character, 7);
		assert_eq!(links[0].range.end.character, 13);
		assert_eq!(links[0].uri.to_string(), "file:///repo/src/lib.rs");
	}

	#[test]
	fn an_unrecognised_goto_definition_payload_yields_no_locations() {
		let provider = provider();
		let out = provider
			.parse_goto_definition_response(json!({"unexpected": true}))
			.unwrap();
		assert!(out.is_empty());
	}

	#[test]
	fn document_symbol_parsing_accepts_both_lsp_shapes() {
		let provider = provider();

		assert!(provider
			.parse_document_symbols_response(json!(null))
			.unwrap()
			.is_empty());

		let nested = provider
			.parse_document_symbols_response(json!([{
				"name": "helper",
				"detail": "fn() -> u32",
				"kind": 12,
				"range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 1}},
				"selectionRange": {"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 13}}
			}]))
			.unwrap();
		assert_eq!(nested.len(), 1);
		assert_eq!(nested[0]["name"], "helper");
		// 0-based LSP ranges are published as 1-based positions.
		assert_eq!(nested[0]["line"], 1);
		assert_eq!(nested[0]["character"], 1);
		assert_eq!(nested[0]["end_line"], 3);
		assert_eq!(nested[0]["detail"], "fn() -> u32");

		let flat = provider
			.parse_document_symbols_response(json!([{
				"name": "helper",
				"kind": 12,
				"containerName": "lib",
				"location": {
					"uri": "file:///repo/src/lib.rs",
					"range": {"start": {"line": 3, "character": 4}, "end": {"line": 3, "character": 10}}
				}
			}]))
			.unwrap();
		assert_eq!(flat.len(), 1);
		assert_eq!(flat[0]["line"], 4);
		assert_eq!(flat[0]["character"], 5);
		assert_eq!(flat[0]["container_name"], "lib");
	}

	#[test]
	fn an_unrecognised_document_symbol_payload_yields_no_symbols() {
		let provider = provider();
		assert!(provider
			.parse_document_symbols_response(json!({"unexpected": true}))
			.unwrap()
			.is_empty());
	}

	#[test]
	fn completion_parsing_accepts_both_a_list_and_a_bare_array() {
		let provider = provider();

		assert!(provider
			.parse_completion_response(json!(null))
			.unwrap()
			.is_empty());

		let list = provider
			.parse_completion_response(json!({
				"isIncomplete": false,
				"items": [{"label": "helper", "kind": 3, "detail": "fn() -> u32", "insertText": "helper()"}]
			}))
			.unwrap();
		assert_eq!(list.len(), 1);
		assert_eq!(list[0]["label"], "helper");
		assert_eq!(list[0]["detail"], "fn() -> u32");
		assert_eq!(list[0]["insert_text"], "helper()");

		let array = provider
			.parse_completion_response(json!([
				{"label": "one", "documentation": "plain docs"},
				{"label": "two", "documentation": {"kind": "markdown", "value": "rich docs"}}
			]))
			.unwrap();
		assert_eq!(array.len(), 2);
		assert_eq!(array[0]["documentation"], "plain docs");
		// Markup documentation is flattened to its value.
		assert_eq!(array[1]["documentation"], "rich docs");
		assert_eq!(array[1]["kind"], serde_json::Value::Null);
	}

	#[test]
	fn an_unrecognised_completion_payload_yields_no_items() {
		let provider = provider();
		assert!(provider
			.parse_completion_response(json!({"unexpected": true}))
			.unwrap()
			.is_empty());
	}
}
