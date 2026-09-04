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

	// ---- content hashes ----

	#[test]
	fn a_content_hash_is_the_plain_sha256_of_the_bytes() {
		assert_eq!(
			calculate_content_hash("hello"),
			"2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
		);
		assert_eq!(
			calculate_content_hash("fn main() {}"),
			"ef32637cb9c3ec2e3968c9cbdf26a5e9c172be94f88af533e14bd43f892d5297"
		);
	}

	#[test]
	fn a_unique_content_hash_digests_the_content_followed_by_the_path() {
		assert_eq!(
			calculate_unique_content_hash("fn main() {}", "src/main.rs"),
			"f29b1f6ca7a88a7b06d73aacadcccd05b1c4e99152b9737a61785bd965727188"
		);
		// Which is exactly the digest of the two strings concatenated.
		assert_eq!(
			calculate_unique_content_hash("fn main() {}", "src/main.rs"),
			calculate_content_hash("fn main() {}src/main.rs")
		);
	}

	#[test]
	fn the_same_content_at_two_paths_hashes_differently() {
		let a = calculate_unique_content_hash("body", "a.rs");
		let b = calculate_unique_content_hash("body", "b.rs");
		assert_ne!(a, b);
		assert_ne!(a, calculate_content_hash("body"));
	}

	#[test]
	fn a_line_aware_hash_digests_content_path_and_both_line_numbers() {
		assert_eq!(
			calculate_content_hash_with_lines("fn main() {}", "src/main.rs", 1, 10),
			"8eedf763cbb5ae912357c9515a42197910eda682d9d565d9e657e1d059b0e212"
		);
		assert_eq!(
			calculate_content_hash_with_lines("fn main() {}", "src/main.rs", 1, 10),
			calculate_content_hash("fn main() {}src/main.rs110")
		);
	}

	#[test]
	fn moving_a_block_within_a_file_changes_its_line_aware_hash() {
		// This is the whole point of the line-aware variant: identical content
		// at a different position must be treated as a changed block.
		let at_top = calculate_content_hash_with_lines("body", "a.rs", 1, 5);
		let moved = calculate_content_hash_with_lines("body", "a.rs", 20, 24);
		assert_ne!(at_top, moved);
		assert_ne!(
			at_top,
			calculate_content_hash_with_lines("body", "a.rs", 1, 6)
		);
		assert_ne!(at_top, calculate_unique_content_hash("body", "a.rs"));
	}

	#[test]
	fn every_hash_is_a_64_character_lowercase_hex_digest() {
		for hash in [
			calculate_content_hash(""),
			calculate_unique_content_hash("", ""),
			calculate_content_hash_with_lines("", "", 0, 0),
		] {
			assert_eq!(hash.len(), 64, "{hash}");
			assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "{hash}");
			assert_eq!(hash, hash.to_lowercase());
		}
	}

	// ---- EmbeddingGenerationConfig ----

	#[test]
	fn the_default_generation_config_pins_the_voyage_models_and_batch_budget() {
		let config = EmbeddingGenerationConfig::default();
		assert_eq!(config.code_model, "voyage:voyage-code-3");
		assert_eq!(config.text_model, "voyage:voyage-3.5-lite");
		assert_eq!(config.batch_size, 16);
		assert_eq!(config.max_tokens_per_batch, 100_000);
	}

	#[test]
	fn a_generation_config_is_built_from_the_embedding_and_index_sections() {
		let mut config = Config::default();
		config.embedding.code_model = "openai:text-embedding-3-large".to_string();
		config.embedding.text_model = "openai:text-embedding-3-small".to_string();
		config.index.embeddings_batch_size = 7;
		config.index.embeddings_max_tokens_per_batch = 1234;

		let generation = EmbeddingGenerationConfig::from(&config);
		assert_eq!(generation.code_model, "openai:text-embedding-3-large");
		assert_eq!(generation.text_model, "openai:text-embedding-3-small");
		assert_eq!(generation.batch_size, 7);
		assert_eq!(generation.max_tokens_per_batch, 1234);
	}

	// ---- model-string validation (runs before any provider call) ----

	fn config_with_models(code: &str, text: &str) -> Config {
		let mut config = Config::default();
		config.embedding.code_model = code.to_string();
		config.embedding.text_model = text.to_string();
		config
	}

	#[tokio::test]
	async fn a_code_model_without_a_provider_prefix_is_rejected_before_any_request() {
		let config = config_with_models("voyage-code-3", "voyage:voyage-3.5-lite");
		let err = generate_embeddings("fn main() {}", true, &config)
			.await
			.expect_err("a model string without ':' must not reach the provider");
		assert_eq!(err.to_string(), "Invalid model format: voyage-code-3");
	}

	#[tokio::test]
	async fn a_text_model_without_a_provider_prefix_is_rejected_before_any_request() {
		let config = config_with_models("voyage:voyage-code-3", "plain-text-model");
		let err = generate_embeddings("some prose", false, &config)
			.await
			.expect_err("a model string without ':' must not reach the provider");
		assert_eq!(err.to_string(), "Invalid model format: plain-text-model");
	}

	#[tokio::test]
	async fn the_is_code_flag_selects_which_model_string_is_validated() {
		// Only the code model is malformed, so the text path must not complain
		// about it — it fails later, on the provider, which we never reach here.
		let config = config_with_models("broken", "also-broken");
		assert_eq!(
			generate_embeddings("x", true, &config)
				.await
				.unwrap_err()
				.to_string(),
			"Invalid model format: broken"
		);
		assert_eq!(
			generate_embeddings("x", false, &config)
				.await
				.unwrap_err()
				.to_string(),
			"Invalid model format: also-broken"
		);
	}

	#[tokio::test]
	async fn the_batch_entry_point_applies_the_same_model_validation() {
		let config = config_with_models("no-colon", "still-no-colon");
		let texts = vec!["a".to_string(), "b".to_string()];
		assert_eq!(
			generate_embeddings_batch(&texts, true, &config, InputType::Document)
				.await
				.unwrap_err()
				.to_string(),
			"Invalid model format: no-colon"
		);
		assert_eq!(
			generate_embeddings_batch(&texts, false, &config, InputType::Query)
				.await
				.unwrap_err()
				.to_string(),
			"Invalid model format: still-no-colon"
		);
	}

	#[tokio::test]
	async fn an_empty_model_string_is_rejected_as_malformed() {
		let config = config_with_models("", "");
		assert_eq!(
			generate_embeddings("x", true, &config)
				.await
				.unwrap_err()
				.to_string(),
			"Invalid model format: "
		);
	}
}
