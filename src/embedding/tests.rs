// Copyright 2025 Muvon Un Limited
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

// Example test to verify SentenceTransformer integration
// This would typically go in tests/ directory or within the module

#[cfg(test)]
mod embedding_tests {
	use crate::config::Config;
	use crate::embedding::types::{parse_provider_model, EmbeddingConfig};
	use crate::embedding::{
		count_tokens, split_texts_into_token_limited_batches, EmbeddingProviderType,
	};

	#[cfg(any(feature = "sentence-transformer", feature = "fastembed"))]
	use crate::embedding::create_embedding_provider_from_parts;

	#[test]
	#[cfg(feature = "sentence-transformer")]
	fn test_sentence_transformer_provider_creation() {
		// Test that we can create a SentenceTransformer provider
		let provider_type = EmbeddingProviderType::SentenceTransformer;
		let model = "sentence-transformers/all-MiniLM-L6-v2";

		let result = create_embedding_provider_from_parts(&provider_type, model);
		assert!(
			result.is_ok(),
			"Should be able to create SentenceTransformer provider"
		);
	}

	#[test]
	fn test_provider_model_parsing() {
		// Test the new provider:model syntax parsing
		let mut test_cases = vec![(
			"jinaai:jina-embeddings-v3",
			EmbeddingProviderType::Jina,
			"jina-embeddings-v3",
		)];

		// Add SentenceTransformer test case only if feature is enabled
		#[cfg(feature = "sentence-transformer")]
		test_cases.push((
			"sentencetransformer:sentence-transformers/all-MiniLM-L6-v2",
			EmbeddingProviderType::SentenceTransformer,
			"sentence-transformers/all-MiniLM-L6-v2",
		));

		// Add FastEmbed test cases only if feature is enabled
		#[cfg(feature = "fastembed")]
		{
			test_cases.push((
				"fastembed:all-MiniLM-L6-v2",
				EmbeddingProviderType::FastEmbed,
				"all-MiniLM-L6-v2",
			));
			test_cases.push((
				"all-MiniLM-L6-v2", // Legacy format without provider
				EmbeddingProviderType::FastEmbed,
				"all-MiniLM-L6-v2",
			));
		}

		// Add Voyage test case (always available)
		test_cases.push((
			"voyage:voyage-code-3",
			EmbeddingProviderType::Voyage,
			"voyage-code-3",
		));

		for (input, expected_provider, expected_model) in test_cases {
			let (provider, model) = parse_provider_model(input);
			assert_eq!(
				provider, expected_provider,
				"Provider should match for input: {}",
				input
			);
			assert_eq!(
				model, expected_model,
				"Model should match for input: {}",
				input
			);
		}
	}

	#[test]
	fn test_default_config_format() {
		// Test that default config uses new provider:model format
		let config = Config::default();

		// Check that default models use provider:model format
		assert!(
			config.embedding.code_model.contains(':'),
			"Code model should use provider:model format"
		);
		assert!(
			config.embedding.text_model.contains(':'),
			"Text model should use provider:model format"
		);

		// Test parsing the default models
		let (code_provider, _) = parse_provider_model(&config.embedding.code_model);
		let (text_provider, _) = parse_provider_model(&config.embedding.text_model);

		// Default provider depends on available features
		#[cfg(feature = "fastembed")]
		{
			assert_eq!(code_provider, EmbeddingProviderType::FastEmbed);
			assert_eq!(text_provider, EmbeddingProviderType::FastEmbed);
		}
		#[cfg(not(feature = "fastembed"))]
		{
			// When FastEmbed is not available, should fall back to Voyage
			assert_eq!(code_provider, EmbeddingProviderType::Voyage);
			assert_eq!(text_provider, EmbeddingProviderType::Voyage);
		}
	}

	#[test]
	#[cfg(feature = "sentence-transformer")]
	fn test_embedding_config_methods() {
		let config = EmbeddingConfig {
			code_model: "sentencetransformer:microsoft/codebert-base".to_string(),
			text_model: "sentencetransformer:sentence-transformers/all-mpnet-base-v2".to_string(),
		};

		// Test getting active provider
		let active_provider = config.get_active_provider();
		assert_eq!(active_provider, EmbeddingProviderType::SentenceTransformer);

		// Test vector dimensions
		let dim = config.get_vector_dimension(
			&EmbeddingProviderType::SentenceTransformer,
			"microsoft/codebert-base",
		);
		assert_eq!(dim, 768);

		let dim2 = config.get_vector_dimension(
			&EmbeddingProviderType::SentenceTransformer,
			"sentence-transformers/all-MiniLM-L6-v2",
		);
		assert_eq!(dim2, 384);
	}

	#[test]
	#[cfg(not(feature = "sentence-transformer"))]
	fn test_embedding_config_methods_without_sentence_transformer() {
		let config = EmbeddingConfig {
			code_model: "voyage:voyage-code-3".to_string(),
			text_model: "voyage:voyage-3.5-lite".to_string(),
		};

		// Test getting active provider
		let active_provider = config.get_active_provider();
		assert_eq!(active_provider, EmbeddingProviderType::Voyage);

		// Test vector dimensions for Voyage models
		let dim = config.get_vector_dimension(&EmbeddingProviderType::Voyage, "voyage-code-3");
		assert_eq!(dim, 1024);

		let dim2 = config.get_vector_dimension(&EmbeddingProviderType::Voyage, "voyage-3.5-lite");
		assert_eq!(dim2, 1024);
	}

	#[test]
	fn test_token_counting() {
		// Test basic token counting
		let text = "Hello world!";
		let token_count = count_tokens(text);
		assert!(token_count > 0, "Should count tokens for basic text");

		// Test empty string
		let empty_count = count_tokens("");
		assert_eq!(empty_count, 0, "Empty string should have 0 tokens");

		// Test longer text
		let long_text = "This is a longer text that should have more tokens than the simple hello world example.";
		let long_count = count_tokens(long_text);
		assert!(
			long_count > token_count,
			"Longer text should have more tokens"
		);
	}

	#[test]
	fn test_token_limited_batching() {
		let texts = vec![
			"Short text".to_string(),
			"This is a medium length text that has more tokens".to_string(),
			"Another short one".to_string(),
			"This is a very long text that contains many words and should definitely exceed any reasonable token limit for a single batch when combined with other texts".to_string(),
			"Final text".to_string(),
		];

		// Test with small token limit to force splitting
		let batches = split_texts_into_token_limited_batches(texts.clone(), 10, 20);

		// Should create multiple batches due to token limit
		assert!(
			batches.len() > 1,
			"Should create multiple batches with small token limit"
		);

		// Verify all texts are included
		let total_texts: usize = batches.iter().map(|b| b.len()).sum();
		assert_eq!(
			total_texts,
			texts.len(),
			"All texts should be included in batches"
		);

		// Test with large limits (should create single batch)
		let single_batch = split_texts_into_token_limited_batches(texts.clone(), 100, 10000);
		assert_eq!(
			single_batch.len(),
			1,
			"Should create single batch with large limits"
		);
		assert_eq!(
			single_batch[0].len(),
			texts.len(),
			"Single batch should contain all texts"
		);
	}

	#[test]
	fn test_config_has_token_limit() {
		let config = Config::default();
		assert!(
			config.index.embeddings_max_tokens_per_batch > 0,
			"Should have positive token limit"
		);
		assert_eq!(
			config.index.embeddings_max_tokens_per_batch, 100000,
			"Should have default token limit of 100000"
		);
	}

	// Note: This test would require network access and is more of an integration test
	// #[tokio::test]
	// async fn test_sentence_transformer_embedding_generation() {
	//     let mut config = Config::default();
	//     config.embedding.provider = EmbeddingProviderType::SentenceTransformer;
	//     config.embedding.sentencetransformer.text_model =
	//         "sentencetransformer:sentence-transformers/all-MiniLM-L6-v2".to_string();
	//
	//     let text = "This is a test text for embedding generation.";
	//     let result = generate_embeddings(text, false, &config).await;
	//
	//     assert!(result.is_ok(), "Should generate embeddings successfully");
	//     let embeddings = result.unwrap();
	//     assert_eq!(embeddings.len(), 384, "all-MiniLM-L6-v2 should produce 384-dimensional embeddings");
	//
	//     // Verify embeddings are normalized (L2 norm should be approximately 1.0)
	//     let norm: f32 = embeddings.iter().map(|x| x * x).sum::<f32>().sqrt();
	//     assert!((norm - 1.0).abs() < 0.01, "Embeddings should be normalized");
	// }
}
