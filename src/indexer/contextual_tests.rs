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

	fn block(path: &str, symbols: &[&str], content: &str) -> CodeBlock {
		CodeBlock {
			path: path.to_string(),
			language: "rust".to_string(),
			content: content.to_string(),
			symbols: symbols.iter().map(|s| s.to_string()).collect(),
			start_line: 1,
			end_line: 1,
			hash: "h".to_string(),
			distance: None,
		}
	}

	#[test]
	fn structural_context_lists_path_language_and_symbols() {
		let b = block("src/auth/middleware.rs", &["verify_token"], "fn f() {}");
		assert_eq!(
			build_structural_context(&b),
			"# File: src/auth/middleware.rs\n# Language: rust\n# Defines: verify_token"
		);
	}

	#[test]
	fn structural_context_omits_the_defines_line_without_symbols() {
		let b = block("src/utils.rs", &[], "const V: u8 = 1;");
		assert_eq!(
			build_structural_context(&b),
			"# File: src/utils.rs\n# Language: rust"
		);
	}

	#[test]
	fn several_symbols_are_joined_on_one_defines_line() {
		let b = block("src/a.rs", &["alpha", "beta", "gamma"], "");
		assert!(build_structural_context(&b).ends_with("# Defines: alpha, beta, gamma"));
	}

	#[test]
	fn a_description_is_prepended_before_the_structural_context() {
		let b = block("src/auth/jwt.rs", &["decode_token"], "fn decode_token() {}");
		let result = build_enriched_embedding_input(&b, Some("Decodes and validates a JWT token"));
		assert_eq!(
			result,
			"Decodes and validates a JWT token\n\n\
			 # File: src/auth/jwt.rs\n# Language: rust\n# Defines: decode_token\n\n\
			 fn decode_token() {}"
		);
	}

	#[test]
	fn without_a_description_the_input_starts_at_the_structural_context() {
		let b = block("src/main.rs", &["main"], "fn main() {}");
		assert_eq!(
			build_enriched_embedding_input(&b, None),
			"# File: src/main.rs\n# Language: rust\n# Defines: main\n\nfn main() {}"
		);
	}

	#[test]
	fn an_empty_description_is_not_prepended() {
		let b = block("src/main.rs", &["main"], "fn main() {}");
		assert_eq!(
			build_enriched_embedding_input(&b, Some("")),
			build_enriched_embedding_input(&b, None),
		);
	}

	#[test]
	fn stripping_returns_the_code_from_a_described_block() {
		let b = block("src/auth.rs", &["verify"], "fn verify() {}");
		let enriched = build_enriched_embedding_input(&b, Some("Verifies a JWT token"));
		assert_eq!(strip_enriched_preamble(&enriched), "fn verify() {}");
	}

	#[test]
	fn stripping_returns_the_code_from_an_undescribed_block() {
		let b = block(
			"src/main.rs",
			&["main"],
			"fn main() {\n    println!(\"hi\");\n}",
		);
		let enriched = build_enriched_embedding_input(&b, None);
		assert_eq!(
			strip_enriched_preamble(&enriched),
			"fn main() {\n    println!(\"hi\");\n}"
		);
	}

	#[test]
	fn stripping_handles_the_text_block_preamble() {
		// Matches the format process_text_blocks_batch writes.
		let enriched = "# File: README.md\n\nThis is content.\nSecond line.";
		assert_eq!(
			strip_enriched_preamble(enriched),
			"This is content.\nSecond line."
		);
	}

	#[test]
	fn stripping_handles_the_document_block_preamble_with_section_context() {
		// Matches process_document_blocks_batch when context is non-empty.
		let enriched = "# File: spec.md\nIntroduction > Overview\n\n# Heading\n\nBody.";
		assert_eq!(strip_enriched_preamble(enriched), "# Heading\n\nBody.");
	}

	#[test]
	fn raw_content_without_a_preamble_round_trips_unchanged() {
		let raw = "fn foo() {}\nfn bar() {}";
		assert_eq!(strip_enriched_preamble(raw), raw);
	}

	#[test]
	fn stripping_keys_off_the_file_marker_not_any_hash_comment() {
		// First substantive line of stored code can start with `#` (Python comment).
		let enriched =
			"# File: app.py\n# Language: python\n\n# TODO: refactor\ndef main():\n    pass";
		assert_eq!(
			strip_enriched_preamble(enriched),
			"# TODO: refactor\ndef main():\n    pass"
		);
	}

	#[test]
	fn a_preamble_with_nothing_after_it_strips_to_an_empty_string() {
		assert_eq!(strip_enriched_preamble("# File: empty.rs\n\n"), "");
	}

	#[test]
	fn a_preamble_without_a_blank_separator_is_left_intact() {
		// No `\n\n` means we cannot tell where the preamble ends, so the row is
		// returned verbatim rather than guessed at.
		let malformed = "# File: a.rs\n# Language: rust";
		assert_eq!(strip_enriched_preamble(malformed), malformed);
	}

	#[test]
	fn siblings_are_collected_per_file() {
		let blocks = vec![
			block("src/auth.rs", &["verify", "decode"], ""),
			block("src/auth.rs", &["refresh"], ""),
			block("src/db.rs", &["connect"], ""),
		];

		let siblings = build_siblings_map(&blocks);
		assert_eq!(
			siblings.get("src/auth.rs").unwrap(),
			&vec![
				"verify".to_string(),
				"decode".to_string(),
				"refresh".to_string()
			]
		);
		assert_eq!(
			siblings.get("src/db.rs").unwrap(),
			&vec!["connect".to_string()]
		);
	}

	#[test]
	fn a_symbol_repeated_across_blocks_is_recorded_once() {
		let blocks = vec![
			block("src/a.rs", &["shared"], ""),
			block("src/a.rs", &["shared", "other"], ""),
		];
		assert_eq!(
			build_siblings_map(&blocks).get("src/a.rs").unwrap(),
			&vec!["shared".to_string(), "other".to_string()]
		);
	}

	#[tokio::test]
	async fn an_unusable_contextual_model_aborts_instead_of_falling_back() {
		let mut config = Config::default();
		config.index.contextual_model = "no-such-provider:some-model".to_string();

		// An empty batch means no LLM request can be issued: the only thing that
		// can fail here is building the client, which is exactly the guarantee
		// under test (contextual descriptions never degrade silently).
		let err = generate_contextual_descriptions(&[], &config, &FileContextMap::new())
			.await
			.expect_err("an unusable model must be fatal");
		let message = err.to_string();
		assert!(
			message.contains("LLM required for contextual descriptions"),
			"{message}"
		);
		assert!(message.contains("no-such-provider:some-model"), "{message}");
	}
}
