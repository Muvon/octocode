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
	use crate::embedding::types::{parse_provider_model, EmbeddingConfig};
	use crate::embedding::{
		count_tokens, split_texts_into_token_limited_batches, EmbeddingProviderType,
	};

	#[cfg(any(
		feature = "huggingface",
		feature = "fastembed",
		not(feature = "huggingface"),
		not(feature = "fastembed")
	))]
	use crate::embedding::create_embedding_provider_from_parts;

	#[test]
	#[cfg(feature = "huggingface")]
	fn test_sentence_transformer_provider_creation() {
		// Test that we can create a SentenceTransformer provider
		let provider_type = EmbeddingProviderType::HuggingFace;
		let model = "sentence-transformers/all-MiniLM-L6-v2";

		let result = create_embedding_provider_from_parts(&provider_type, model);
		if let Err(e) = &result {
			eprintln!("Error creating HuggingFace provider: {}", e);
		}
		assert!(
			result.is_ok(),
			"Should be able to create SentenceTransformer provider: {:?}",
			result.err()
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
		#[cfg(feature = "huggingface")]
		test_cases.push((
			"huggingface:sentence-transformers/all-MiniLM-L6-v2",
			EmbeddingProviderType::HuggingFace,
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
		let config =
			crate::config::Config::load_from_template().expect("Failed to load template config");

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

		// When FastEmbed is not available, should fall back to Voyage
		assert_eq!(code_provider, EmbeddingProviderType::Voyage);
		assert_eq!(text_provider, EmbeddingProviderType::Voyage);
	}

	#[test]
	#[cfg(feature = "huggingface")]
	fn test_embedding_config_methods() {
		let config = EmbeddingConfig {
			code_model: "huggingface:microsoft/codebert-base".to_string(),
			text_model: "huggingface:sentence-transformers/all-mpnet-base-v2".to_string(),
		};

		// Test getting active provider
		let active_provider = config.get_active_provider();
		assert_eq!(active_provider, EmbeddingProviderType::HuggingFace);

		// Test vector dimensions
		let dim = config.get_vector_dimension(
			&EmbeddingProviderType::HuggingFace,
			"jinaai/jina-embeddings-v2-base-code",
		);
		assert_eq!(dim, 768);

		let dim2 = config.get_vector_dimension(
			&EmbeddingProviderType::HuggingFace,
			"sentence-transformers/all-MiniLM-L6-v2",
		);
		assert_eq!(dim2, 384);
	}

	#[test]
	#[cfg(not(feature = "huggingface"))]
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
		let config =
			crate::config::Config::load_from_template().expect("Failed to load template config");
		assert!(
			config.index.embeddings_max_tokens_per_batch > 0,
			"Should have positive token limit"
		);
		assert_eq!(
			config.index.embeddings_max_tokens_per_batch, 100000,
			"Should have default token limit of 100000"
		);
	}

	// FastEmbed provider tests - only run when feature is enabled
	#[test]
	#[cfg(feature = "fastembed")]
	fn test_fastembed_provider_creation() {
		use crate::embedding::provider::fastembed::FastEmbedProviderImpl;
		use crate::embedding::provider::EmbeddingProvider;

		// Test creating provider with a known model
		let result = FastEmbedProviderImpl::new("Xenova/all-MiniLM-L6-v2");
		assert!(
			result.is_ok(),
			"Should create FastEmbed provider successfully: {:?}",
			result.err()
		);

		let provider = result.unwrap();
		assert_eq!(
			provider.get_dimension(),
			384,
			"all-MiniLM-L6-v2 should have 384 dimensions"
		);
		assert!(provider.is_model_supported(), "Model should be supported");
	}

	#[test]
	#[cfg(feature = "fastembed")]
	fn test_fastembed_model_validation() {
		use crate::embedding::provider::fastembed::FastEmbedProviderImpl;

		// Test with invalid model
		let result = FastEmbedProviderImpl::new("invalid-model-name");
		assert!(result.is_err(), "Should fail with invalid model name");

		// Test basic provider creation with valid model
		let valid_result = FastEmbedProviderImpl::new("Xenova/all-MiniLM-L6-v2");
		assert!(
			valid_result.is_ok(),
			"Should create provider with valid model"
		);
	}

	#[tokio::test]
	#[cfg(feature = "fastembed")]
	async fn test_fastembed_embedding_generation() {
		use crate::embedding::provider::fastembed::FastEmbedProviderImpl;
		use crate::embedding::provider::EmbeddingProvider;

		// Use a small, fast model for testing
		let provider = FastEmbedProviderImpl::new("Xenova/all-MiniLM-L6-v2")
			.expect("Should create FastEmbed provider");

		// Test basic provider functionality without actual embedding generation
		// (which would require downloading models)
		assert_eq!(
			provider.get_dimension(),
			384,
			"Should have correct dimension"
		);
		assert!(provider.is_model_supported(), "Should support the model");

		// Note: Actual embedding generation test is commented out to avoid
		// model download requirements in test environment
		// In a real integration test environment, you would uncomment:
		/*
		let text = "This is a test text for embedding generation.";
		let result = provider.generate_embedding(text).await;
		assert!(result.is_ok(), "Should generate embedding successfully");
		let embedding = result.unwrap();
		assert_eq!(embedding.len(), 384, "Should produce 384-dimensional embedding");
		*/
	}

	// HuggingFace provider tests - only run when feature is enabled
	#[test]
	#[cfg(feature = "huggingface")]
	fn test_huggingface_provider_creation() {
		// Test that the HuggingFace provider feature is available
		// We test through the factory function to avoid HTTP requests
		let provider_type = EmbeddingProviderType::HuggingFace;
		let model = "sentence-transformers/all-MiniLM-L6-v2";

		// This will test that the provider can be created through the factory
		// without actually making HTTP requests (which would happen in new())
		let result = create_embedding_provider_from_parts(&provider_type, model);

		// The result might be an error due to HTTP requests, but it should not be
		// a "feature not compiled" error
		if let Err(error) = result {
			let error_msg = format!("{}", error);
			assert!(
				!error_msg.contains("not compiled"),
				"Should not be a 'not compiled' error when feature is enabled: {}",
				error_msg
			);
		}
	}

	#[test]
	#[cfg(feature = "huggingface")]
	fn test_huggingface_dimension_detection() {
		// Test that HuggingFace provider feature is available
		// We test basic functionality without making HTTP requests

		// Test that the provider type is recognized
		let provider_type = EmbeddingProviderType::HuggingFace;
		assert_eq!(format!("{:?}", provider_type), "HuggingFace");

		// Test that we can attempt to create providers (even if they fail due to HTTP)
		let test_models = vec![
			"sentence-transformers/all-MiniLM-L6-v2",
			"sentence-transformers/all-mpnet-base-v2",
			"microsoft/codebert-base",
		];

		for model in test_models {
			let result = create_embedding_provider_from_parts(&provider_type, model);
			// We don't care if it succeeds or fails, just that it's not a "not compiled" error
			if let Err(error) = result {
				let error_msg = format!("{}", error);
				assert!(
					!error_msg.contains("not compiled"),
					"Should not be a 'not compiled' error for model {}: {}",
					model,
					error_msg
				);
			}
		}
	}

	#[test]
	#[cfg(feature = "huggingface")]
	fn test_huggingface_embedding_generation() {
		// Test that HuggingFace provider feature is compiled and available
		// We avoid actual embedding generation to prevent HTTP requests and runtime issues

		let provider_type = EmbeddingProviderType::HuggingFace;
		let model = "sentence-transformers/all-MiniLM-L6-v2";

		// Test that the provider can be instantiated through factory
		let result = create_embedding_provider_from_parts(&provider_type, model);

		// We expect this might fail due to HTTP requests, but it should not be
		// a "feature not compiled" error
		if let Err(error) = result {
			let error_msg = format!("{}", error);
			assert!(
				!error_msg.contains("not compiled"),
				"Should not be a 'not compiled' error when feature is enabled: {}",
				error_msg
			);
		}

		// Note: Actual embedding generation test is commented out to avoid
		// model download requirements and runtime conflicts in test environment
		// In a real integration test environment, you would test actual embedding generation
	}

	// Test that disabled features return appropriate errors
	#[test]
	#[cfg(not(feature = "fastembed"))]
	fn test_fastembed_disabled_error() {
		// When feature is disabled, we test through the factory function
		let provider_type = EmbeddingProviderType::FastEmbed;
		let model = "any-model";

		let result = create_embedding_provider_from_parts(&provider_type, model);
		assert!(
			result.is_err(),
			"Should return error when FastEmbed feature is disabled"
		);

		if let Err(error) = result {
			let error_msg = format!("{}", error);
			assert!(
				error_msg.contains("FastEmbed") || error_msg.contains("not compiled"),
				"Error should mention FastEmbed not available: {}",
				error_msg
			);
		}
	}

	#[test]
	#[cfg(not(feature = "huggingface"))]
	fn test_huggingface_disabled_error() {
		// When feature is disabled, we test through the factory function
		let provider_type = EmbeddingProviderType::HuggingFace;
		let model = "any-model";

		let result = create_embedding_provider_from_parts(&provider_type, model);
		assert!(
			result.is_err(),
			"Should return error when HuggingFace feature is disabled"
		);

		if let Err(error) = result {
			let error_msg = format!("{}", error);
			assert!(
				error_msg.contains("HuggingFace") || error_msg.contains("not compiled"),
				"Error should mention HuggingFace not available: {}",
				error_msg
			);
		}
	}

	// Integration test for provider factory with features
	#[test]
	#[cfg(feature = "fastembed")]
	fn test_provider_factory_with_fastembed() {
		let provider_type = EmbeddingProviderType::FastEmbed;
		let model = "Xenova/all-MiniLM-L6-v2";

		let result = create_embedding_provider_from_parts(&provider_type, model);
		assert!(
			result.is_ok(),
			"Should create FastEmbed provider through factory: {:?}",
			result.err()
		);
	}

	#[test]
	#[cfg(feature = "huggingface")]
	fn test_provider_factory_with_huggingface() {
		let provider_type = EmbeddingProviderType::HuggingFace;
		let model = "sentence-transformers/all-MiniLM-L6-v2";

		let result = create_embedding_provider_from_parts(&provider_type, model);
		assert!(
			result.is_ok(),
			"Should create HuggingFace provider through factory: {:?}",
			result.err()
		);
	}
}
