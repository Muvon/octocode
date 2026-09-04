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
	use crate::indexer::graphrag::types::{Provenance, RelationType};
	use crate::store::CodeBlock;
	use serde_json::json;

	/// AI enhancements with no LLM client — every pure helper still works.
	fn offline() -> AIEnhancements {
		let mut config = Config::default();
		config.graphrag.use_llm = false;
		AIEnhancements::new(config, true).expect("no LLM is required when use_llm is off")
	}

	/// AI enhancements holding a resolved LLM client. `ProviderFactory` resolves
	/// `openrouter:...` from the model string alone, so no key or network is used.
	fn with_llm() -> AIEnhancements {
		let mut config = Config::default();
		config.graphrag.use_llm = true;
		AIEnhancements::new(config, true).expect("the shipped default model must resolve")
	}

	fn block(content: &str, symbols: &[&str]) -> CodeBlock {
		CodeBlock {
			path: "src/lib.rs".to_string(),
			language: "rust".to_string(),
			content: content.to_string(),
			symbols: symbols.iter().map(|s| s.to_string()).collect(),
			start_line: 1,
			end_line: 2,
			hash: "h".to_string(),
			distance: None,
		}
	}

	fn node(id: &str, exports: &[&str], symbols: &[&str], size_lines: u32) -> CodeNode {
		CodeNode {
			id: id.to_string(),
			name: id.rsplit('/').next().unwrap_or(id).to_string(),
			kind: "file".to_string(),
			path: id.to_string(),
			description: String::new(),
			symbols: symbols.iter().map(|s| s.to_string()).collect(),
			hash: String::new(),
			embedding: Vec::new(),
			imports: Vec::new(),
			exports: exports.iter().map(|s| s.to_string()).collect(),
			functions: Vec::new(),
			size_lines,
			language: "text".to_string(),
		}
	}

	fn file_for_ai(file_id: &str, symbols: &[&str]) -> FileForAI {
		FileForAI {
			file_id: file_id.to_string(),
			file_path: format!("/abs/{file_id}"),
			language: "rust".to_string(),
			symbols: symbols.iter().map(|s| s.to_string()).collect(),
			content_sample: "fn sample() {}".to_string(),
			function_count: 2,
			class_count: 1,
		}
	}

	#[test]
	fn a_disabled_llm_leaves_the_client_unset() {
		let ai = offline();
		assert!(!ai.llm_enabled());
		assert!(ai.llm_client.is_none());
		// `LlmClient` is not `Debug`, so unwrap_err() is unavailable here.
		let err = match ai.create_llm_client("openai:gpt-4o-mini") {
			Ok(_) => panic!("a client must not be built while the LLM is disabled"),
			Err(e) => e,
		};
		assert_eq!(err.to_string(), "LLM client not initialized");
	}

	#[test]
	fn an_enabled_llm_builds_a_client_from_the_configured_model() {
		let ai = with_llm();
		assert!(ai.llm_enabled());
		assert!(ai.llm_client.is_some());
	}

	#[test]
	fn an_unresolvable_model_fails_construction_instead_of_disabling_the_llm() {
		let mut config = Config::default();
		config.graphrag.use_llm = true;
		config.llm.model = "no-provider-prefix".to_string();

		let err = AIEnhancements::new(config, true)
			.err()
			.expect("an unresolvable model must abort construction");
		assert!(
			err.to_string()
				.starts_with("LLM required for GraphRAG but unavailable:"),
			"{err}"
		);
		assert!(
			err.to_string().contains("Disable graphrag.use_llm"),
			"{err}"
		);
	}

	#[test]
	fn create_llm_client_overrides_the_configured_model() {
		let ai = with_llm();
		let client = ai
			.create_llm_client("openai:gpt-4o-mini")
			.expect("a provider-prefixed model must resolve");
		assert_eq!(client.model(), "gpt-4o-mini");
		// The provider prefix is mandatory even once a client exists.
		assert!(ai.create_llm_client("gpt-4o-mini").is_err());
	}

	#[test]
	fn ai_relationship_analysis_needs_exports_size_or_symbols() {
		let ai = offline();

		assert!(!ai.should_use_ai_for_relationships(&node("src/empty.rs", &[], &[], 0)));
		assert!(!ai.should_use_ai_for_relationships(&node(
			"src/thin.rs",
			&["one"],
			&["a", "b"],
			49
		)));

		// Any single qualifying signal is enough.
		assert!(ai.should_use_ai_for_relationships(&node("src/e.rs", &["a", "b"], &[], 0)));
		assert!(ai.should_use_ai_for_relationships(&node("src/l.rs", &[], &[], 50)));
		assert!(ai.should_use_ai_for_relationships(&node("src/s.rs", &[], &["a", "b", "c"], 0)));
	}

	#[test]
	fn ai_descriptions_need_symbols_and_a_supported_language() {
		let ai = offline();

		assert!(ai.should_use_ai_for_description(&["main".to_string()], 10, "rust"));
		// No symbols — nothing worth describing.
		assert!(!ai.should_use_ai_for_description(&[], 10, "rust"));
		// Unsupported language — no parser backs the sample.
		assert!(!ai.should_use_ai_for_description(&["main".to_string()], 10, "cobol"));
		// Line count is deliberately ignored.
		assert!(ai.should_use_ai_for_description(&["main".to_string()], 0, "rust"));
	}

	#[test]
	fn the_content_sample_orders_blocks_by_symbol_count() {
		let ai = offline();
		let few = block("fn few() {}", &["few"]);
		let many = block("fn many() {}", &["a", "b", "c"]);

		let sample = ai.build_content_sample_for_ai(&[&few, &many]);
		assert_eq!(
			sample,
			"// Block: 3 symbols\nfn many() {}\n\n// Block: 1 symbols\nfn few() {}\n\n"
		);
	}

	#[test]
	fn the_content_sample_elides_the_middle_of_a_large_block() {
		let ai = offline();
		let head = "A".repeat(150);
		let middle = "M".repeat(100);
		let tail = "Z".repeat(150);
		let big = block(&format!("{head}{middle}{tail}"), &["big"]);

		let sample = ai.build_content_sample_for_ai(&[&big]);
		assert_eq!(
			sample,
			format!("// Block: 1 symbols\n{head}\n...\n{tail}\n\n")
		);
		assert!(!sample.contains('M'));
	}

	#[test]
	fn a_block_of_exactly_300_bytes_is_kept_whole() {
		let ai = offline();
		let content = "x".repeat(300);
		let sample = ai.build_content_sample_for_ai(&[&block(&content, &["s"])]);
		assert_eq!(sample, format!("// Block: 1 symbols\n{content}\n\n"));
	}

	#[test]
	fn the_content_sample_stops_at_the_token_budget() {
		let first = block("fn first() {}", &["first"]);
		let second = block("fn second() {}", &["second"]);
		let first_tokens = crate::embedding::count_tokens(&first.content);

		// One token of headroom above the first block: it fits, and the +50
		// formatting charge then puts the second block over budget.
		let mut config = Config::default();
		config.graphrag.use_llm = false;
		config.graphrag.llm.max_sample_tokens = first_tokens + 1;
		let ai = AIEnhancements::new(config, true).unwrap();
		assert_eq!(
			ai.build_content_sample_for_ai(&[&first, &second]),
			"// Block: 1 symbols\nfn first() {}\n\n"
		);

		// A budget the very first block cannot meet yields nothing at all.
		let mut config = Config::default();
		config.graphrag.use_llm = false;
		config.graphrag.llm.max_sample_tokens = first_tokens;
		let ai = AIEnhancements::new(config, true).unwrap();
		assert_eq!(ai.build_content_sample_for_ai(&[&first, &second]), "");
	}

	#[test]
	fn the_batch_user_message_numbers_files_and_caps_symbols_at_five() {
		let ai = offline();
		let message = ai.build_batch_user_message(&[
			file_for_ai("src/a.rs", &["s1", "s2", "s3", "s4", "s5", "s6"]),
			file_for_ai("src/b.rs", &["only"]),
		]);

		assert!(
			message.starts_with(
				"Analyze the following 2 files and provide architectural descriptions:\n\n"
			),
			"{message}"
		);
		assert!(message.contains("=== FILE 1 ===\nID: src/a.rs\nLanguage: rust\nStats: 2 functions, 1 classes/structs\nKey symbols: s1, s2, s3, s4, s5\nCode sample:\nfn sample() {}\n\n"), "{message}");
		assert!(
			message.contains("=== FILE 2 ===\nID: src/b.rs\n"),
			"{message}"
		);
		// The sixth symbol is dropped by the take(5) cap.
		assert!(!message.contains("s6"), "{message}");
		assert!(
			message.ends_with("Include one entry per file using the exact ID provided."),
			"{message}"
		);
	}

	#[test]
	fn the_batch_schema_requires_a_file_id_and_description_per_entry() {
		let ai = offline();
		assert_eq!(
			ai.create_batch_response_schema(),
			json!({
				"type": "object",
				"properties": {
					"descriptions": {
						"type": "array",
						"items": {
							"type": "object",
							"properties": {
								"file_id": {"type": "string"},
								"description": {"type": "string"}
							},
							"required": ["file_id", "description"]
						}
					}
				},
				"required": ["descriptions"]
			})
		);
	}

	#[test]
	fn the_batch_response_keeps_known_ids_and_drops_unknown_ones() {
		let ai = offline();
		let files = [
			file_for_ai("src/a.rs", &["a"]),
			file_for_ai("src/b.rs", &["b"]),
		];
		let response = json!({
			"descriptions": [
				{"file_id": "src/a.rs", "description": "The A layer."},
				{"file_id": "src/ghost.rs", "description": "Never requested."}
			]
		});

		let parsed = ai.parse_batch_response(&response, &files).unwrap();
		assert_eq!(parsed.len(), 1);
		assert_eq!(
			parsed.get("src/a.rs").map(String::as_str),
			Some("The A layer.")
		);
		assert!(!parsed.contains_key("src/ghost.rs"));
		// A file the model skipped is simply absent — not an error.
		assert!(!parsed.contains_key("src/b.rs"));
	}

	#[test]
	fn a_batch_description_longer_than_300_bytes_is_truncated_with_an_ellipsis() {
		let ai = offline();
		let files = [file_for_ai("src/a.rs", &["a"])];

		let long = "a".repeat(400);
		let parsed = ai
			.parse_batch_response(
				&json!({"descriptions": [{"file_id": "src/a.rs", "description": long}]}),
				&files,
			)
			.unwrap();
		let description = &parsed["src/a.rs"];
		assert_eq!(description.len(), 300);
		assert_eq!(*description, format!("{}...", "a".repeat(297)));

		// Exactly 300 bytes is left alone.
		let exact = "b".repeat(300);
		let parsed = ai
			.parse_batch_response(
				&json!({"descriptions": [{"file_id": "src/a.rs", "description": exact}]}),
				&files,
			)
			.unwrap();
		assert_eq!(parsed["src/a.rs"], exact);
	}

	#[test]
	fn a_batch_response_of_the_wrong_shape_is_an_error() {
		let ai = offline();
		let files = [file_for_ai("src/a.rs", &["a"])];

		for response in [
			json!({"summaries": []}),
			json!({"descriptions": [{"file_id": "src/a.rs"}]}),
			json!([]),
		] {
			let err = ai
				.parse_batch_response(&response, &files)
				.err()
				.unwrap_or_else(|| panic!("{response} must not parse"));
			assert!(
				err.to_string()
					.starts_with("Failed to parse batch response:"),
				"{err}"
			);
		}
	}

	#[test]
	fn architectural_relationships_are_read_from_the_relationships_key() {
		let ai = offline();
		let parsed = ai
			.parse_ai_architectural_relationships(&json!({
				"relationships": [{
					"source_path": "src/app.rs",
					"target_path": "src/db.rs",
					"relation_type": "configures",
					"description": "Wires the pool",
					"confidence": 0.75
				}]
			}))
			.unwrap();

		assert_eq!(parsed.len(), 1);
		let relationship = &parsed[0];
		assert_eq!(relationship.source, "src/app.rs");
		assert_eq!(relationship.target, "src/db.rs");
		assert_eq!(relationship.relation_type, RelationType::Configures);
		assert_eq!(relationship.description, "Wires the pool");
		assert_eq!(relationship.confidence, 0.75);
		assert_eq!(relationship.weight, 0.9);
		assert_eq!(relationship.provenance, Provenance::Inferred);
	}

	#[test]
	fn architectural_relationships_also_parse_from_a_bare_array() {
		let ai = offline();
		let parsed = ai
			.parse_ai_architectural_relationships(&json!([{
				"source_path": "a",
				"target_path": "b",
				"relation_type": "factory_creates",
				"description": "d",
				"confidence": 0.5
			}]))
			.unwrap();
		assert_eq!(parsed.len(), 1);
		assert_eq!(parsed[0].relation_type, RelationType::FactoryCreates);
	}

	#[test]
	fn an_unknown_relation_type_from_the_model_falls_back_to_imports() {
		let ai = offline();
		let parsed = ai
			.parse_ai_architectural_relationships(&json!({
				"relationships": [{
					"source_path": "a",
					"target_path": "b",
					"relation_type": "teleports_into",
					"description": "d",
					"confidence": 0.9
				}]
			}))
			.unwrap();
		assert_eq!(parsed[0].relation_type, RelationType::Imports);
	}

	#[test]
	fn an_unparseable_architectural_response_yields_no_relationships() {
		let ai = offline();
		for response in [
			json!({"relationships": "not an array"}),
			json!({"relationships": [{"source_path": "a"}]}),
			json!("plain text"),
		] {
			assert!(
				ai.parse_ai_architectural_relationships(&response)
					.unwrap()
					.is_empty(),
				"{response}"
			);
		}
	}

	#[tokio::test]
	async fn an_empty_batch_request_never_reaches_the_llm() {
		let ai = with_llm();
		assert!(ai
			.extract_ai_descriptions_batch(&[])
			.await
			.unwrap()
			.is_empty());
	}

	#[tokio::test]
	async fn files_without_substance_skip_the_llm_and_keep_the_rule_based_edges() {
		let ai = with_llm();
		let parent = node("src", &[], &[], 0);
		let child = node("src/a.txt", &[], &[], 0);
		let nodes = vec![parent, child];

		// No node qualifies for AI analysis, so no LLM call is attempted and only
		// the rule-based hierarchy edge survives deduplication.
		let relationships = ai
			.discover_relationships_with_ai_enhancement(&nodes, &nodes)
			.await
			.expect("rule-based discovery must not need the network");

		assert_eq!(relationships.len(), 1, "{relationships:?}");
		assert_eq!(relationships[0].source, "src");
		assert_eq!(relationships[0].target, "src/a.txt");
		assert_eq!(relationships[0].relation_type, RelationType::ParentModule);
	}
}
