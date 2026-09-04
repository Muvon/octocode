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
	use crate::config::Config;

	#[test]
	fn a_client_is_built_from_the_configured_model() {
		let config = Config::default();
		let client = LlmClient::from_config(&config).expect("the shipped default must resolve");
		assert!(!client.model().is_empty());
		// The model name loses its provider prefix once the provider is resolved.
		assert!(
			config.llm.model.ends_with(client.model()),
			"{}",
			client.model()
		);
	}

	#[test]
	fn an_explicit_model_overrides_the_configured_one() {
		let config = Config::default();
		let client = LlmClient::with_model(&config, "openai:gpt-4o-mini")
			.expect("a provider-prefixed model must resolve");
		assert_eq!(client.model(), "gpt-4o-mini");
	}

	#[test]
	fn a_model_without_a_provider_is_rejected() {
		let config = Config::default();
		assert!(LlmClient::with_model(&config, "gpt-4o-mini").is_err());
		// A slash is not the separator — only `provider:model` is accepted.
		assert!(LlmClient::with_model(&config, "openai/gpt-4o-mini").is_err());
		assert!(LlmClient::with_model(&config, "").is_err());
	}

	#[test]
	fn structured_output_support_is_reported_per_provider() {
		let config = Config::default();
		let client = LlmClient::with_model(&config, "openai:gpt-4o-mini").unwrap();
		// Only asserts the call is wired through to the provider; the answer is
		// the provider's to decide.
		let _ = client.supports_structured_output();
	}

	#[test]
	fn json_is_recovered_from_a_language_tagged_fence() {
		let content = "Here:\n```json\n{\"ok\": true, \"n\": 2}\n```";
		assert_eq!(
			LlmClient::strip_json_from_markdown(content),
			serde_json::json!({"ok": true, "n": 2})
		);
	}

	#[test]
	fn content_that_is_not_json_is_reported_with_the_raw_text() {
		// Only fenced blocks and whole-string JSON are recognised; a JSON object
		// embedded in prose is not extracted.
		for content in ["no json here", "prose {\"outer\": 1} trailing"] {
			let value = LlmClient::strip_json_from_markdown(content);
			assert_eq!(
				value["error"],
				serde_json::json!("Failed to parse JSON from response"),
				"for {content:?}"
			);
			assert_eq!(value["raw_content"], serde_json::json!(content));
		}
	}

	#[test]
	fn message_constructors_tag_the_role() {
		let system = Message::system("be terse");
		let user = Message::user("hello");
		assert_ne!(
			serde_json::to_string(&system).unwrap(),
			serde_json::to_string(&user).unwrap()
		);
	}

	#[test]
	fn a_configured_model_without_a_provider_prefix_fails_to_build_a_client() {
		let mut config = Config::default();
		config.llm.model = "gpt-4o-mini".to_string();
		assert!(LlmClient::from_config(&config).is_err());
	}

	#[test]
	fn a_whole_string_of_json_is_parsed_without_a_fence() {
		// Both object and array roots, with surrounding whitespace trimmed.
		assert_eq!(
			LlmClient::strip_json_from_markdown("  \n {\"a\": [1, 2]} \n "),
			serde_json::json!({"a": [1, 2]})
		);
		assert_eq!(
			LlmClient::strip_json_from_markdown("[1, 2, 3]"),
			serde_json::json!([1, 2, 3])
		);
		assert_eq!(
			LlmClient::strip_json_from_markdown("null"),
			serde_json::Value::Null
		);
	}

	#[test]
	fn a_trailing_array_is_recovered_from_surrounding_prose() {
		// The last-resort scan slices from the first `[` to the end, so it only
		// works when the array runs to the end of the response.
		assert_eq!(
			LlmClient::strip_json_from_markdown("Here you go: [4, 5]"),
			serde_json::json!([4, 5])
		);
		assert_eq!(
			LlmClient::strip_json_from_markdown("Here you go: [4, 5] — hope that helps")["error"],
			serde_json::json!("Failed to parse JSON from response")
		);
	}

	#[test]
	fn a_fence_with_a_non_json_language_tag_is_still_unwrapped() {
		// `find("```json")` is case-sensitive, so an upper-cased tag misses the
		// dedicated branch and has to be rescued by the generic-fence branch.
		assert_eq!(
			LlmClient::strip_json_from_markdown("```JSON\n{\"a\": 1}\n```"),
			serde_json::json!({"a": 1})
		);
		assert_eq!(
			LlmClient::strip_json_from_markdown("```python\n{\"a\": 1}\n```"),
			serde_json::json!({"a": 1})
		);
	}

	#[test]
	fn a_fence_preceded_by_multibyte_text_is_sliced_on_char_boundaries() {
		let content = "Résumé ✅ — voilà:\n```json\n{\"ok\": true}\n```";
		assert_eq!(
			LlmClient::strip_json_from_markdown(content),
			serde_json::json!({"ok": true})
		);
	}

	#[test]
	fn a_tilde_fence_is_not_recognised_as_a_code_block() {
		let content = "~~~json\n{\"a\": 1}\n~~~";
		let value = LlmClient::strip_json_from_markdown(content);
		assert_eq!(
			value["error"],
			serde_json::json!("Failed to parse JSON from response")
		);
		assert_eq!(value["raw_content"], serde_json::json!(content));
	}

	#[test]
	fn a_fence_holding_non_json_reports_the_whole_response_as_raw_content() {
		let content = "```json\nnot actually json\n```";
		let value = LlmClient::strip_json_from_markdown(content);
		assert_eq!(
			value["error"],
			serde_json::json!("Failed to parse JSON from response")
		);
		assert_eq!(value["raw_content"], serde_json::json!(content));
	}

	#[test]
	fn an_empty_response_is_reported_as_unparseable() {
		let value = LlmClient::strip_json_from_markdown("");
		assert_eq!(
			value["error"],
			serde_json::json!("Failed to parse JSON from response")
		);
		assert_eq!(value["raw_content"], serde_json::json!(""));
	}
}
