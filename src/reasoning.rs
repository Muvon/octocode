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

//! PageIndex-style reasoning retrieval.
//!
//! After the deterministic retrievers (hybrid vector+BM25, optional GraphRAG
//! expansion, optional exact `structural_search` hits) gather a recall-rich
//! candidate pool, an LLM reasons over each candidate's *structure* — file path,
//! symbol names, and a code snippet — and re-ranks by whether the code actually
//! ANSWERS the query, not merely by embedding similarity. This is PageIndex's
//! "similarity ≠ relevance" idea applied to code, using the LLM we already
//! configure. Gated (`search.reasoning.enabled`); any failure degrades to the
//! input order so search never breaks.

use crate::config::Config;
use crate::llm::{LlmClient, Message};
use crate::store::CodeBlock;
use anyhow::Result;

const REASONING_SYSTEM_PROMPT: &str = "You are an expert code-retrieval judge. Given a \
developer's search query and a numbered list of candidate code locations (file path, symbol \
names, and a code snippet), decide which candidates actually ANSWER the query. Reason about \
intent and behaviour, not just keyword overlap: a snippet that implements what the query asks \
for outranks one that merely mentions the words. Return the candidate numbers ordered from most \
to least relevant, INCLUDING ONLY those that are genuinely relevant.";

/// Re-rank code candidates by LLM reasoning over their structure. Returns at
/// most `final_top_k` blocks, ordered most-relevant-first, with `distance` set
/// from the reasoning rank so downstream sorting/formatting stays consistent.
/// Irrelevant candidates the model omits are dropped (precision). On any error
/// the original order is returned unchanged.
pub async fn reason_rank_code_blocks(
	query: &str,
	blocks: Vec<CodeBlock>,
	config: &Config,
) -> Result<Vec<CodeBlock>> {
	let rc = &config.search.reasoning;
	if !rc.enabled || blocks.is_empty() {
		return Ok(blocks);
	}

	let mut pool = blocks;
	pool.truncate(rc.max_candidates.max(1));

	let client = match LlmClient::with_model(config, &rc.model) {
		Ok(c) => c.with_reasoning_effort(&rc.reasoning_effort),
		Err(e) => {
			tracing::warn!("reasoning: cannot build LLM client ({e}); keeping input order");
			return Ok(pool);
		}
	};

	let snippet_len = match rc.context_level.as_str() {
		"signatures" => 200,
		"full" => 4000,
		_ => 600, // "snippets"
	};

	let mut prompt = String::new();
	prompt.push_str(&format!("Query: {}\n\nCandidates:\n", query));
	for (i, b) in pool.iter().enumerate() {
		let syms = if b.symbols.is_empty() {
			"(none)".to_string()
		} else {
			b.symbols.join(", ")
		};
		let snippet = crate::indexer::contextual::strip_enriched_preamble(&b.content);
		let snippet = crate::utils::truncate_at_char_boundary(snippet, snippet_len);
		prompt.push_str(&format!(
			"[{}] {} (lines {}-{})\nSymbols: {}\n<code>\n{}\n</code>\n\n",
			i + 1,
			b.path,
			b.start_line,
			b.end_line,
			syms,
			snippet
		));
	}
	prompt.push_str(&format!(
		"Return a JSON object {{\"ranked\": [numbers]}} listing candidate numbers (1-{}) that \
		 answer the query, most relevant first, omitting irrelevant ones. Output ONLY the JSON.",
		pool.len()
	));

	let schema = serde_json::json!({
		"type": "object",
		"properties": { "ranked": { "type": "array", "items": { "type": "integer" } } },
		"required": ["ranked"]
	});

	let messages = vec![
		Message::system(REASONING_SYSTEM_PROMPT),
		Message::user(&prompt),
	];
	let json = match client.chat_completion_json(messages, Some(schema)).await {
		Ok(v) => v,
		Err(e) => {
			tracing::warn!("reasoning: LLM call failed ({e}); keeping input order");
			return Ok(pool);
		}
	};

	// Accept {"ranked":[...]} or a bare array [...].
	let ranked: Vec<usize> = json
		.get("ranked")
		.and_then(|v| v.as_array())
		.or_else(|| json.as_array())
		.map(|a| {
			a.iter()
				.filter_map(|x| x.as_u64())
				.map(|n| n as usize)
				.collect()
		})
		.unwrap_or_default();

	if ranked.is_empty() {
		tracing::warn!("reasoning: empty/invalid ranking; keeping input order");
		return Ok(pool);
	}

	let n = pool.len();
	// Reciprocal Rank Fusion of the reasoning ranking with the original hybrid
	// ranking. The hybrid rank (pool position) always contributes, acting as a
	// recall floor so an LLM-omitted or LLM-demoted true hit is not lost; the
	// reasoning rank contributes `reasoning_weight`x on top, so the model's
	// judgement drives the head of the list. Keeps the MRR/NDCG gains of pure
	// reasoning while recovering Recall@k.
	const RRF_K: f32 = 60.0;
	let rw = rc.reasoning_weight.max(0.0);
	let mut score = vec![0.0f32; n];
	for (i, s) in score.iter_mut().enumerate() {
		*s += 1.0 / (RRF_K + i as f32); // hybrid contribution (always present)
	}
	for (rrank, &num) in ranked.iter().enumerate() {
		if num >= 1 && num <= n {
			score[num - 1] += rw / (RRF_K + rrank as f32); // reasoning contribution
		}
	}
	let mut order: Vec<usize> = (0..n).collect();
	order.sort_by(|&a, &b| {
		score[b]
			.partial_cmp(&score[a])
			.unwrap_or(std::cmp::Ordering::Equal)
	});
	let mut out: Vec<CodeBlock> = order
		.iter()
		.enumerate()
		.map(|(pos, &i)| {
			let mut b = pool[i].clone();
			// Distance from the fused rank so the surfaced similarity is consistent.
			b.distance = Some((pos as f32 + 1.0) / (n as f32 + 1.0));
			b
		})
		.collect();

	out.truncate(rc.final_top_k.max(1));
	Ok(out)
}
