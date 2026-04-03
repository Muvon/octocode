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
	use crate::store::HybridSearchQuery;

	#[test]
	fn test_hybrid_search_query_validation_vector_and_keywords() {
		let valid_query = HybridSearchQuery {
			vector_query: Some(vec![0.1, 0.2, 0.3]),
			keywords: Some("test query".to_string()),
			vector_weight: 0.7,
			keyword_weight: 0.3,
			limit: 10,
			min_relevance: Some(0.5),
			language_filter: None,
		};
		assert!(valid_query.validate().is_ok());
	}

	#[test]
	fn test_hybrid_search_query_validation_vector_only() {
		let valid_query = HybridSearchQuery {
			vector_query: Some(vec![0.1, 0.2, 0.3]),
			keywords: None,
			vector_weight: 0.7,
			keyword_weight: 0.3,
			limit: 10,
			min_relevance: Some(0.5),
			language_filter: None,
		};
		assert!(valid_query.validate().is_ok());
	}

	#[test]
	fn test_hybrid_search_query_validation_keywords_only() {
		let valid_query = HybridSearchQuery {
			vector_query: None,
			keywords: Some("test query".to_string()),
			vector_weight: 0.7,
			keyword_weight: 0.3,
			limit: 10,
			min_relevance: Some(0.5),
			language_filter: None,
		};
		assert!(valid_query.validate().is_ok());
	}

	#[test]
	fn test_hybrid_search_query_validation_invalid_weights() {
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
	}

	#[test]
	fn test_hybrid_search_query_validation_no_signals() {
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
	fn test_hybrid_search_query_validation_negative_weights() {
		let negative_weight = HybridSearchQuery {
			vector_query: Some(vec![0.1, 0.2, 0.3]),
			keywords: None,
			vector_weight: -0.5, // Invalid: < 0.0
			keyword_weight: 0.3,
			limit: 10,
			min_relevance: Some(0.5),
			language_filter: None,
		};
		assert!(negative_weight.validate().is_err());
	}
}
