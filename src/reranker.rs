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

use crate::config::RerankerConfig;
use crate::store::{CodeBlock, DocumentBlock, TextBlock};
use anyhow::Result;

/// Rerank code blocks using octolib's cross-encoder reranker.
/// Fetches `top_k_candidates` from the input, reranks them, and returns `final_top_k`.
/// The reranker score (higher = more relevant) is converted to a distance-like value
/// (lower = better) so the rest of the pipeline stays consistent.
pub async fn rerank_code_blocks_with_octolib(
	query: &str,
	mut blocks: Vec<CodeBlock>,
	config: &RerankerConfig,
) -> Result<Vec<CodeBlock>> {
	if !config.enabled || blocks.is_empty() {
		return Ok(blocks);
	}

	// Take only the candidates we'll rerank
	blocks.truncate(config.top_k_candidates);

	let (provider, model) = parse_reranker_model(&config.model)?;
	let documents: Vec<String> = blocks.iter().map(|b| b.content.clone()).collect();

	let response = octolib::reranker::rerank(
		query,
		documents,
		&provider,
		&model,
		Some(config.final_top_k),
	)
	.await?;

	let mut reranked: Vec<CodeBlock> = response
		.results
		.into_iter()
		.filter_map(|r| {
			blocks.get(r.index).map(|b| {
				let mut block = b.clone();
				// Convert relevance score (higher=better) to distance (lower=better)
				block.distance = Some(1.0 - r.relevance_score as f32);
				block
			})
		})
		.collect();

	reranked.sort_by(|a, b| {
		a.distance
			.unwrap_or(1.0)
			.partial_cmp(&b.distance.unwrap_or(1.0))
			.unwrap_or(std::cmp::Ordering::Equal)
	});

	Ok(reranked)
}

/// Rerank document blocks using octolib's cross-encoder reranker.
pub async fn rerank_doc_blocks_with_octolib(
	query: &str,
	mut blocks: Vec<DocumentBlock>,
	config: &RerankerConfig,
) -> Result<Vec<DocumentBlock>> {
	if !config.enabled || blocks.is_empty() {
		return Ok(blocks);
	}

	blocks.truncate(config.top_k_candidates);

	let (provider, model) = parse_reranker_model(&config.model)?;
	// Use title + content for better reranking signal
	let documents: Vec<String> = blocks
		.iter()
		.map(|b| format!("{}\n{}", b.title, b.content))
		.collect();

	let response = octolib::reranker::rerank(
		query,
		documents,
		&provider,
		&model,
		Some(config.final_top_k),
	)
	.await?;

	let mut reranked: Vec<DocumentBlock> = response
		.results
		.into_iter()
		.filter_map(|r| {
			blocks.get(r.index).map(|b| {
				let mut block = b.clone();
				block.distance = Some(1.0 - r.relevance_score as f32);
				block
			})
		})
		.collect();

	reranked.sort_by(|a, b| {
		a.distance
			.unwrap_or(1.0)
			.partial_cmp(&b.distance.unwrap_or(1.0))
			.unwrap_or(std::cmp::Ordering::Equal)
	});

	Ok(reranked)
}

/// Rerank text blocks using octolib's cross-encoder reranker.
pub async fn rerank_text_blocks_with_octolib(
	query: &str,
	mut blocks: Vec<TextBlock>,
	config: &RerankerConfig,
) -> Result<Vec<TextBlock>> {
	if !config.enabled || blocks.is_empty() {
		return Ok(blocks);
	}

	blocks.truncate(config.top_k_candidates);

	let (provider, model) = parse_reranker_model(&config.model)?;
	let documents: Vec<String> = blocks.iter().map(|b| b.content.clone()).collect();

	let response = octolib::reranker::rerank(
		query,
		documents,
		&provider,
		&model,
		Some(config.final_top_k),
	)
	.await?;

	let mut reranked: Vec<TextBlock> = response
		.results
		.into_iter()
		.filter_map(|r| {
			blocks.get(r.index).map(|b| {
				let mut block = b.clone();
				block.distance = Some(1.0 - r.relevance_score as f32);
				block
			})
		})
		.collect();

	reranked.sort_by(|a, b| {
		a.distance
			.unwrap_or(1.0)
			.partial_cmp(&b.distance.unwrap_or(1.0))
			.unwrap_or(std::cmp::Ordering::Equal)
	});

	Ok(reranked)
}

/// Parse "provider:model" string into (provider, model) parts.
fn parse_reranker_model(model_str: &str) -> Result<(String, String)> {
	match model_str.split_once(':') {
		Some((provider, model)) => Ok((provider.to_string(), model.to_string())),
		None => Err(anyhow::anyhow!(
			"Invalid reranker model format: '{}'. Expected 'provider:model' (e.g., 'voyage:rerank-2.5')",
			model_str
		)),
	}
}
