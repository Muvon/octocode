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

use crate::store::{CodeBlock, DocumentBlock, TextBlock};
use std::collections::HashMap;

/// Reranking strategies for improving search results
pub struct Reranker;

impl Reranker {
	/// Combined reranking using multiple signals
	pub fn rerank_code_blocks(mut blocks: Vec<CodeBlock>, query: &str) -> Vec<CodeBlock> {
		if blocks.is_empty() {
			return blocks;
		}

		// Apply multiple reranking strategies
		let query_lower = query.to_lowercase();

		for block in &mut blocks {
			let mut score = block.distance.unwrap_or(1.0);

			// 1. Exact text matches (most important)
			score *= Self::text_match_factor(&block.content, &query_lower);

			// 2. Symbol matches
			score *= Self::symbol_match_factor(&block.symbols, &query_lower);

			// 3. Path relevance
			score *= Self::path_relevance_factor(&block.path, &query_lower);

			// 4. Content length factor (prefer reasonable sized blocks)
			score *= Self::content_length_factor(&block.content);

			// Update the distance with the reranked score
			block.distance = Some(score);
		}

		// Sort by the new reranked scores
		blocks.sort_by(|a, b| {
			let score_a = a.distance.unwrap_or(1.0);
			let score_b = b.distance.unwrap_or(1.0);
			score_a
				.partial_cmp(&score_b)
				.unwrap_or(std::cmp::Ordering::Equal)
		});

		blocks
	}

	/// Rerank document blocks
	pub fn rerank_document_blocks(
		mut blocks: Vec<DocumentBlock>,
		query: &str,
	) -> Vec<DocumentBlock> {
		if blocks.is_empty() {
			return blocks;
		}

		let query_lower = query.to_lowercase();

		for block in &mut blocks {
			let mut score = block.distance.unwrap_or(1.0);

			// 1. Title matches (very important for docs)
			score *= Self::title_match_factor(&block.title, &query_lower);

			// 2. Content matches
			score *= Self::text_match_factor(&block.content, &query_lower);

			// 3. Path relevance
			score *= Self::path_relevance_factor(&block.path, &query_lower);

			// 4. Header level factor (prefer higher level headers)
			score *= Self::header_level_factor(block.level);

			block.distance = Some(score);
		}

		blocks.sort_by(|a, b| {
			let score_a = a.distance.unwrap_or(1.0);
			let score_b = b.distance.unwrap_or(1.0);
			score_a
				.partial_cmp(&score_b)
				.unwrap_or(std::cmp::Ordering::Equal)
		});

		blocks
	}

	/// Rerank text blocks
	pub fn rerank_text_blocks(mut blocks: Vec<TextBlock>, query: &str) -> Vec<TextBlock> {
		if blocks.is_empty() {
			return blocks;
		}

		let query_lower = query.to_lowercase();

		for block in &mut blocks {
			let mut score = block.distance.unwrap_or(1.0);

			// 1. Text matches
			score *= Self::text_match_factor(&block.content, &query_lower);

			// 2. Path relevance
			score *= Self::path_relevance_factor(&block.path, &query_lower);

			// 3. Content length factor
			score *= Self::content_length_factor(&block.content);

			block.distance = Some(score);
		}

		blocks.sort_by(|a, b| {
			let score_a = a.distance.unwrap_or(1.0);
			let score_b = b.distance.unwrap_or(1.0);
			score_a
				.partial_cmp(&score_b)
				.unwrap_or(std::cmp::Ordering::Equal)
		});

		blocks
	}

	/// Calculate text match factor - exact matches get strong boost
	fn text_match_factor(content: &str, query: &str) -> f32 {
		let content_lower = content.to_lowercase();

		// Exact phrase match
		if content_lower.contains(query) {
			let word_count = query.split_whitespace().count();
			return match word_count {
				1 => 0.7,     // Single word exact match
				2..=3 => 0.5, // 2-3 word phrase match
				_ => 0.6,     // Longer phrase match
			};
		}

		// Individual word matches
		let query_words: Vec<&str> = query.split_whitespace().collect();
		let content_words: Vec<&str> = content_lower.split_whitespace().collect();

		let mut matches = 0;
		for query_word in &query_words {
			if content_words
				.iter()
				.any(|&word| word.contains(query_word) || query_word.contains(word))
			{
				matches += 1;
			}
		}

		if matches > 0 {
			let match_ratio = matches as f32 / query_words.len() as f32;
			return 0.8 + (match_ratio * 0.15); // 0.8 to 0.95 range
		}

		1.0 // No change if no matches
	}

	/// Calculate title match factor - titles are very important for relevance
	fn title_match_factor(title: &str, query: &str) -> f32 {
		let title_lower = title.to_lowercase();

		// Exact title match
		if title_lower == query {
			return 0.4; // Very strong boost
		}

		// Title contains query
		if title_lower.contains(query) {
			return 0.5;
		}

		// Query contains title (for short titles)
		if query.contains(&title_lower) && title_lower.len() > 2 {
			return 0.6;
		}

		// Individual word matches in title
		let query_words: Vec<&str> = query.split_whitespace().collect();
		let title_words: Vec<&str> = title_lower.split_whitespace().collect();

		let mut matches = 0;
		for query_word in &query_words {
			if title_words
				.iter()
				.any(|&word| word.contains(query_word) || query_word.contains(word))
			{
				matches += 1;
			}
		}

		if matches > 0 {
			let match_ratio = matches as f32 / query_words.len() as f32;
			return 0.6 + (match_ratio * 0.2); // 0.6 to 0.8 range
		}

		1.0 // No change if no matches
	}

	/// Calculate symbol match factor
	fn symbol_match_factor(symbols: &[String], query: &str) -> f32 {
		for symbol in symbols {
			let symbol_lower = symbol.to_lowercase();
			if symbol_lower.contains(&query.to_lowercase())
				|| query.to_lowercase().contains(&symbol_lower)
			{
				return 0.6; // Strong boost for symbol matches
			}
		}
		1.0
	}

	/// Calculate path relevance factor
	fn path_relevance_factor(path: &str, query: &str) -> f32 {
		let path_lower = path.to_lowercase();
		let query_lower = query.to_lowercase();

		// Check filename
		if let Some(filename) = path_lower.split('/').next_back() {
			if filename.contains(&query_lower) {
				return 0.75;
			}
		}

		// Check directory names
		if path_lower.contains(&query_lower) {
			return 0.85;
		}

		1.0
	}

	/// Content length factor - prefer reasonably sized blocks
	fn content_length_factor(content: &str) -> f32 {
		let length = content.len();

		match length {
			0..=50 => 0.9,       // Very short - might lack context
			51..=500 => 0.95,    // Good size - easy to read
			501..=2000 => 1.0,   // Ideal size - good context
			2001..=5000 => 0.98, // Long but manageable
			_ => 0.95,           // Very long - harder to scan
		}
	}

	/// Header level factor - prefer higher level headers in docs
	fn header_level_factor(level: usize) -> f32 {
		match level {
			1 => 0.9,  // H1 - main topics
			2 => 0.85, // H2 - sections
			3 => 0.9,  // H3 - subsections
			4 => 0.95, // H4 - details
			_ => 1.0,  // H5+ or other
		}
	}

	/// Calculate tf-idf style boosting based on term frequency
	pub fn tf_idf_boost(blocks: &mut [CodeBlock], query: &str) {
		let query_lower = query.to_lowercase();
		let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

		// Calculate document frequency for each term
		let mut doc_freq: HashMap<String, usize> = HashMap::new();
		let total_docs = blocks.len();

		for block in blocks.iter() {
			let content_lower = block.content.to_lowercase();
			let mut seen_terms = std::collections::HashSet::new();

			for term in &query_terms {
				if content_lower.contains(term) && !seen_terms.contains(term) {
					*doc_freq.entry(term.to_string()).or_insert(0) += 1;
					seen_terms.insert(term);
				}
			}
		}

		// Apply tf-idf scoring
		for block in blocks.iter_mut() {
			let content_lower = block.content.to_lowercase();
			let mut tf_idf_score = 0.0;

			for term in &query_terms {
				// Term frequency in this document
				let tf = content_lower.matches(term).count() as f32;

				// Inverse document frequency
				let df = doc_freq.get(*term).unwrap_or(&1);
				let idf = (total_docs as f32 / *df as f32).ln();

				tf_idf_score += tf * idf;
			}

			if tf_idf_score > 0.0 {
				// Apply tf-idf boost (reduce distance for higher tf-idf)
				let boost_factor = (1.0 - (tf_idf_score / 10.0).min(0.3)).max(0.5);
				if let Some(distance) = block.distance {
					block.distance = Some(distance * boost_factor);
				}
			}
		}
	}
}
