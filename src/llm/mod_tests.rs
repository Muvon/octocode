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
}
