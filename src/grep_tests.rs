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
	use std::collections::HashMap;

	const SOURCE: &str = "line one\nline two\nline three\nline four\nline five\n";

	fn hit(file: &str, line: usize, text: &str) -> GrepMatch {
		GrepMatch {
			file: file.to_string(),
			line,
			column: 0,
			text: text.to_string(),
			start_byte: 0,
			end_byte: 0,
			breadcrumb: None,
		}
	}

	fn sources(pairs: &[(&str, &str)]) -> HashMap<String, String> {
		pairs
			.iter()
			.map(|(f, s)| (f.to_string(), s.to_string()))
			.collect()
	}

	// ---- format_matches_with_context ----

	#[test]
	fn context_lines_surround_the_match_and_only_it_is_marked() {
		let out = format_matches_with_context(
			&[hit("a.rs", 3, "line three")],
			&sources(&[("a.rs", SOURCE)]),
			1,
		);
		assert_eq!(
			out,
			"a.rs\n  2:  line two\n> 3:  line three\n  4:  line four\n---"
		);
	}

	#[test]
	fn a_zero_context_window_shows_only_the_matched_line() {
		let out = format_matches_with_context(
			&[hit("a.rs", 3, "line three")],
			&sources(&[("a.rs", SOURCE)]),
			0,
		);
		assert_eq!(out, "a.rs\n> 3:  line three\n---");
	}

	#[test]
	fn a_match_on_the_first_line_clamps_the_leading_context() {
		let out = format_matches_with_context(
			&[hit("a.rs", 1, "line one")],
			&sources(&[("a.rs", SOURCE)]),
			2,
		);
		assert_eq!(
			out,
			"a.rs\n> 1:  line one\n  2:  line two\n  3:  line three\n---"
		);
	}

	#[test]
	fn a_context_window_past_the_end_of_the_file_is_clamped() {
		let out = format_matches_with_context(
			&[hit("a.rs", 5, "line five")],
			&sources(&[("a.rs", SOURCE)]),
			10,
		);
		assert_eq!(
			out,
			"a.rs\n  1:  line one\n  2:  line two\n  3:  line three\n  4:  line four\n> 5:  line five\n---"
		);
	}

	#[test]
	fn a_breadcrumb_is_printed_above_the_context_block() {
		let mut m = hit("a.rs", 2, "line two");
		m.breadcrumb = Some("impl Store › fn flush".to_string());
		let out = format_matches_with_context(&[m], &sources(&[("a.rs", SOURCE)]), 0);
		assert_eq!(out, "a.rs\n» impl Store › fn flush\n> 2:  line two\n---");
	}

	#[test]
	fn a_file_missing_from_the_source_map_falls_back_to_grouped_output() {
		let mut m = hit("a.rs", 3, "line three");
		m.column = 7;
		let out = format_matches_with_context(&[m], &HashMap::new(), 2);
		assert_eq!(out, "a.rs\n3:7:  line three");
	}

	#[test]
	fn the_fallback_path_truncates_a_long_match_body() {
		let body = "l1\nl2\nl3\nl4\nl5\nl6";
		let out = format_matches_with_context(&[hit("a.rs", 1, body)], &HashMap::new(), 1);
		assert_eq!(out, "a.rs\n1:0:  l1\nl2\nl3\nl4\n... (2 more lines)");
	}

	#[test]
	fn files_are_emitted_in_sorted_order_with_a_blank_line_between_them() {
		let out = format_matches_with_context(
			&[hit("z.rs", 1, "line one"), hit("a.rs", 1, "line one")],
			&sources(&[("a.rs", SOURCE), ("z.rs", SOURCE)]),
			0,
		);
		assert_eq!(
			out,
			"a.rs\n> 1:  line one\n---\n\nz.rs\n> 1:  line one\n---"
		);
	}

	#[test]
	fn several_matches_in_one_file_each_get_their_own_context_block() {
		let out = format_matches_with_context(
			&[hit("a.rs", 1, "line one"), hit("a.rs", 4, "line four")],
			&sources(&[("a.rs", SOURCE)]),
			0,
		);
		assert_eq!(out, "a.rs\n> 1:  line one\n---\n> 4:  line four\n---");
	}

	#[test]
	fn formatting_no_matches_produces_no_output() {
		assert_eq!(format_matches_with_context(&[], &HashMap::new(), 3), "");
		assert_eq!(format_matches_grouped(&[]), "");
	}

	// ---- canonical_kind: arms not exercised elsewhere ----

	#[test]
	fn canonical_kind_separates_methods_from_free_functions() {
		assert_eq!(
			canonical_kind("method", "typescript"),
			Some("method_definition")
		);
		assert_eq!(canonical_kind("method", "go"), Some("method_declaration"));
		assert_eq!(canonical_kind("method", "java"), Some("method_declaration"));
		assert_eq!(canonical_kind("method", "ruby"), Some("method"));
	}

	#[test]
	fn canonical_kind_maps_type_declarations_per_language() {
		assert_eq!(canonical_kind("struct", "rust"), Some("struct_item"));
		assert_eq!(canonical_kind("struct", "go"), Some("struct_type"));
		assert_eq!(canonical_kind("struct", "cpp"), Some("struct_specifier"));
		assert_eq!(canonical_kind("trait", "rust"), Some("trait_item"));
		assert_eq!(canonical_kind("impl", "rust"), Some("impl_item"));
		assert_eq!(
			canonical_kind("interface", "typescript"),
			Some("interface_declaration")
		);
		assert_eq!(canonical_kind("interface", "go"), Some("interface_type"));
		assert_eq!(
			canonical_kind("interface", "java"),
			Some("interface_declaration")
		);
		// Go has no `trait`, Rust has no `interface`.
		assert_eq!(canonical_kind("trait", "go"), None);
		assert_eq!(canonical_kind("interface", "rust"), None);
	}

	#[test]
	fn canonical_kind_maps_call_expressions_per_language() {
		assert_eq!(canonical_kind("call", "rust"), Some("call_expression"));
		assert_eq!(canonical_kind("call", "python"), Some("call"));
		assert_eq!(canonical_kind("call", "ruby"), Some("call"));
		// Java's call node is `method_invocation`, not a declaration kind.
		assert_eq!(canonical_kind("call", "java"), Some("method_invocation"));
		assert_eq!(
			canonical_kind("method_invocation", "java"),
			Some("method_invocation")
		);
	}

	#[test]
	fn canonical_kind_respects_rusts_expression_oriented_control_flow() {
		assert_eq!(canonical_kind("if", "rust"), Some("if_expression"));
		assert_eq!(canonical_kind("for", "rust"), Some("for_expression"));
		assert_eq!(canonical_kind("loop", "rust"), Some("for_expression"));
		assert_eq!(canonical_kind("if", "python"), Some("if_statement"));
		assert_eq!(
			canonical_kind("conditional", "typescript"),
			Some("if_statement")
		);
		assert_eq!(canonical_kind("while", "go"), Some("while_statement"));
		// Rust's loop kinds are named differently, so `while` has no mapping.
		assert_eq!(canonical_kind("while", "rust"), None);
	}

	#[test]
	fn canonical_kind_maps_variable_declarations_per_language() {
		assert_eq!(
			canonical_kind("const", "typescript"),
			Some("variable_declaration")
		);
		assert_eq!(
			canonical_kind("var", "javascript"),
			Some("variable_declaration")
		);
		assert_eq!(canonical_kind("let", "rust"), Some("let_declaration"));
		assert_eq!(canonical_kind("let", "go"), Some("var_declaration"));
	}

	#[test]
	fn canonical_kind_only_maps_try_for_the_languages_that_have_it() {
		assert_eq!(canonical_kind("try", "java"), Some("try_statement"));
		assert_eq!(canonical_kind("try", "python"), Some("try_statement"));
		assert_eq!(canonical_kind("try", "rust"), None);
		assert_eq!(canonical_kind("try", "go"), None);
	}

	#[test]
	fn canonical_kind_maps_return_to_the_kind_each_grammar_actually_defines() {
		// Regression: the generic arm used to hand `return_statement` to every
		// language, including the two grammars that have no such node.
		assert_eq!(canonical_kind("return", "rust"), Some("return_expression"));
		assert_eq!(canonical_kind("return", "ruby"), Some("return"));
		assert_eq!(canonical_kind("return", "python"), Some("return_statement"));
		assert_eq!(canonical_kind("return", "go"), Some("return_statement"));
		assert_eq!(
			canonical_kind("return", "typescript"),
			Some("return_statement")
		);
	}

	#[test]
	fn the_rust_return_kind_is_accepted_by_the_grammar() {
		let source = "fn f(x: i32) -> i32 {\n    if x > 0 {\n        return 1;\n    }\n    0\n}\n";
		let kind = canonical_kind("return", "rust").expect("rust `return` must map to a kind");
		let matches = search_file_by_kind("f.rs", source, kind, "rust")
			.expect("the mapped kind must exist in the rust grammar");
		assert_eq!(matches.len(), 1);
		assert_eq!(matches[0].line, 3);
		assert!(
			matches[0].text.starts_with("return 1"),
			"{}",
			matches[0].text
		);
	}

	#[test]
	fn the_ruby_return_kind_is_accepted_by_the_grammar() {
		let source = "def f(x)\n  return 1\nend\n";
		let kind = canonical_kind("return", "ruby").expect("ruby `return` must map to a kind");
		let matches = search_file_by_kind("f.rb", source, kind, "ruby")
			.expect("the mapped kind must exist in the ruby grammar");
		assert_eq!(matches.len(), 1);
		assert_eq!(matches[0].line, 2);
	}

	#[test]
	fn an_unmapped_intent_or_language_yields_no_kind() {
		assert_eq!(canonical_kind("function", "swift"), None);
		assert_eq!(canonical_kind("nonsense", "rust"), None);
		assert_eq!(canonical_kind("class", "rust"), None);
	}

	// ---- small pure helpers ----

	#[test]
	fn identifier_kinds_cover_the_per_grammar_spellings() {
		for kind in [
			"identifier",
			"type_identifier",
			"field_identifier",
			"property_identifier",
			"name",
			"constant",
		] {
			assert!(is_identifier_kind(kind), "{kind} should be an identifier");
		}
		for kind in ["function_item", "call_expression", "string_literal"] {
			assert!(!is_identifier_kind(kind), "{kind} should not be");
		}
	}

	#[test]
	fn only_an_exact_name_matcher_offers_a_literal_prefilter_token() {
		let exact = NameMatcher::new("flush_all").unwrap();
		assert_eq!(exact.literal_fragment(), Some("flush_all".to_string()));
		assert!(exact.matches("flush_all"));
		assert!(!exact.matches("flush_all_now"));

		let wildcard = NameMatcher::new("flush_*").unwrap();
		assert_eq!(wildcard.literal_fragment(), None);
		assert!(wildcard.matches("flush_all"));
		assert!(!wildcard.matches("drain_all"));
	}

	#[test]
	fn a_wildcard_matcher_anchors_both_ends_and_escapes_regex_syntax() {
		let matcher = NameMatcher::new("a.b*").unwrap();
		assert!(matcher.matches("a.bcd"));
		// The dot is escaped, so it is not a single-character wildcard.
		assert!(!matcher.matches("axbcd"));
		// The pattern is anchored, so a leading prefix does not match.
		assert!(!matcher.matches("za.bcd"));
	}

	#[test]
	fn a_lexical_scan_reports_one_untrimmed_column_per_line() {
		let source = "let foo = 1;\n    foo += 1;\nlet bar = 2;\n";
		let matches = lexical_scan("a.rs", source, "foo", 10);
		assert_eq!(matches.len(), 2);
		assert_eq!((matches[0].line, matches[0].column), (1, 4));
		assert_eq!(matches[0].text, "let foo = 1;");
		// Column is the byte offset in the raw line, but the text is trimmed.
		assert_eq!((matches[1].line, matches[1].column), (2, 4));
		assert_eq!(matches[1].text, "foo += 1;");
		// Lexical hits are not AST-verified, so they carry no byte range.
		assert!(matches.iter().all(|m| m.start_byte == 0 && m.end_byte == 0));
	}

	#[test]
	fn a_lexical_scan_of_an_absent_token_finds_nothing() {
		assert!(lexical_scan("a.rs", "let x = 1;\n", "missing", 10).is_empty());
	}

	#[test]
	fn literal_tokens_skips_every_expando_prefixed_name() {
		// `$`, `µ` and `#` all introduce metavariables.
		assert_eq!(
			literal_tokens("$VAR + µOTHER + #THIRD"),
			Vec::<String>::new()
		);
		// A single-character run is too short to be a useful prefilter token.
		assert_eq!(literal_tokens("a + bb"), vec!["bb".to_string()]);
	}

	#[test]
	fn an_unknown_language_has_neither_containers_nor_definition_kinds() {
		assert!(container_kinds("cobol").is_empty());
		assert!(definition_kinds("cobol").is_empty());
	}

	#[test]
	fn elixir_has_definition_kinds_but_no_breadcrumb_containers() {
		// Every Elixir declaration is a generic `call`, so using it as a
		// container would label every nested invocation as an enclosing symbol.
		assert_eq!(definition_kinds("elixir"), &["call"]);
		assert!(container_kinds("elixir").is_empty());
	}

	#[test]
	fn a_match_body_of_exactly_the_cap_is_not_truncated() {
		let text = "a\nb\nc";
		assert_eq!(truncate_match_text(text, 3), text);
		assert_eq!(truncate_match_text(text, 2), "a\nb\n... (1 more lines)");
	}

	#[test]
	fn a_rewrite_diff_reports_lines_the_rewrite_appended() {
		let result = RewriteResult {
			file: "a.rs".to_string(),
			replacements: 1,
			original_source: "one\ntwo\n".to_string(),
			rewritten_source: "one\ntwo\nthree\n".to_string(),
		};
		assert_eq!(
			format_rewrite_diff(&result),
			"--- a.rs\n+++ a.rs\n-3:  \n+3:  three"
		);
	}
}
