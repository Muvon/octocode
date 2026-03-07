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

#[cfg(test)]
mod tests {
	use crate::store::{HybridSearchQuery, Store};

	#[test]
	fn test_tokenize() {
		let text = "Hello World_test example";
		let tokens = Store::tokenize(text);
		assert_eq!(tokens, vec!["hello", "world_test", "example"]);
	}

	#[test]
	fn test_tokenize_with_punctuation() {
		let text = "function_name(arg1, arg2) { return value; }";
		let tokens = Store::tokenize(text);
		assert_eq!(
			tokens,
			vec!["function_name", "arg1", "arg2", "return", "value"]
		);
	}

	#[test]
	fn test_calculate_tf() {
		let text = "hello world hello test hello";
		let tf = Store::calculate_tf("hello", text);
		assert!((tf - 0.6).abs() < 0.01); // 3 occurrences out of 5 tokens
	}

	#[test]
	fn test_calculate_tf_no_match() {
		let text = "hello world test";
		let tf = Store::calculate_tf("missing", text);
		assert_eq!(tf, 0.0);
	}

	#[test]
	fn test_score_field() {
		let keywords = vec!["hello".to_string(), "world".to_string()];
		let text = "hello world hello test";
		let score = Store::score_field(&keywords, text, 1.0);
		assert!(score > 0.0);
	}

	#[test]
	fn test_score_field_with_weight() {
		let keywords = vec!["test".to_string()];
		let text = "test test test";
		let score = Store::score_field(&keywords, text, 2.0);
		assert!((score - 2.0).abs() < 0.01); // TF = 1.0, weight = 2.0
	}

	#[test]
	fn test_hybrid_search_query_validation() {
		let valid_query = HybridSearchQuery {
			vector_query: Some(vec![0.1, 0.2, 0.3]),
			keywords: Some(vec!["test".to_string()]),
			vector_weight: 0.7,
			keyword_weight: 0.3,
			limit: 10,
			min_relevance: Some(0.5),
			language_filter: None,
		};
		assert!(valid_query.validate().is_ok());

		let invalid_weights = HybridSearchQuery {
			vector_query: Some(vec![0.1, 0.2, 0.3]),
			keywords: None,
			vector_weight: 1.5, // Invalid: > 1.0
			keyword_weight: 0.3,
			limit: 10,
			min_relevance: Some(0.5),
			language_filter: None,
		};
		assert!(invalid_weights.validate().is_err());

		let no_signals = HybridSearchQuery {
			vector_query: None,
			keywords: None,
			vector_weight: 0.7,
			keyword_weight: 0.3,
			limit: 10,
			min_relevance: Some(0.5),
			language_filter: None,
		};
		assert!(no_signals.validate().is_err());
	}

	#[test]
	fn test_hybrid_search_query_default() {
		let query = HybridSearchQuery::default();
		assert_eq!(query.vector_weight, 0.7);
		assert_eq!(query.keyword_weight, 0.3);
		assert_eq!(query.limit, 10);
		assert!(query.vector_query.is_none());
		assert!(query.keywords.is_none());
	}
}