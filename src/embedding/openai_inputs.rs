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

//! Token-safe OpenAI inputs and one-vector-per-source aggregation.
//!
//! This is input preparation, not a provider implementation. Provider I/O
//! remains in octolib. Splitting happens after structural/LLM enrichment so
//! every byte of the final input is represented without changing block IDs.

use super::{EmbeddingProvider, InputType};
use anyhow::{ensure, Result};
use std::sync::LazyLock;
use tiktoken_rs::{cl100k_base, CoreBPE};

// https://developers.openai.com/api/reference/resources/embeddings/methods/create
const MAX_INPUT_TOKENS: usize = 8192;
static TOKENIZER: LazyLock<CoreBPE> = LazyLock::new(|| cl100k_base().expect("cl100k_base tokenizer must load"));

struct InputPart {
	text: String,
	source: usize,
	weight: usize,
	tokens: usize,
}

// Source code can contain literal special-token markers. Encode them as
// ordinary text, exactly as the embeddings API does for string inputs.
fn tokens(text: &str) -> usize {
	TOKENIZER.encode_ordinary(text).len()
}

fn split_input(text: &str, source: usize, input_type: &InputType, limit: usize) -> Result<Vec<InputPart>> {
	let mut pending = vec![text];
	let mut parts = Vec::new();
	while let Some(text) = pending.pop() {
		let prefixed = input_type.apply_prefix(text);
		let count = tokens(&prefixed);
		if count <= limit {
			parts.push(InputPart {
				text: prefixed,
				source,
				weight: tokens(text).max(1),
				tokens: count,
			});
			continue;
		}

		// Split the source on a UTF-8 boundary, then measure both halves
		// again. Token counts need not be additive across a string boundary.
		let mut midpoint = text.len() / 2;
		while midpoint > 0 && !text.is_char_boundary(midpoint) {
			midpoint -= 1;
		}
		if midpoint == 0 {
			midpoint = text.chars().next().map_or(0, char::len_utf8);
		}
		ensure!(
			midpoint > 0 && midpoint < text.len(),
			"embedding token budget cannot fit a character with its input prefix"
		);
		let (left, right) = text.split_at(midpoint);
		pending.push(right);
		pending.push(left);
	}
	Ok(parts)
}

pub(super) async fn embed(
	provider: &dyn EmbeddingProvider,
	texts: Vec<String>,
	input_type: InputType,
	batch_size: usize,
	max_tokens_per_batch: usize,
) -> Result<Vec<Vec<f32>>> {
	ensure!(batch_size > 0, "embedding batch size must be positive");
	ensure!(
		max_tokens_per_batch > 0,
		"embedding batch token budget must be positive"
	);
	let limit = MAX_INPUT_TOKENS.min(max_tokens_per_batch);
	let mut parts = Vec::new();
	for (source, text) in texts.iter().enumerate() {
		parts.extend(split_input(text, source, &input_type, limit)?);
	}

	let mut vectors: Vec<Vec<(Vec<f32>, usize)>> = (0..texts.len()).map(|_| Vec::new()).collect();
	let mut start = 0;
	while start < parts.len() {
		let mut end = start;
		let mut batch_tokens = 0;
		while end < parts.len() && end - start < batch_size && parts[end].tokens <= max_tokens_per_batch - batch_tokens
		{
			batch_tokens += parts[end].tokens;
			end += 1;
		}
		let batch = &mut parts[start..end];
		let inputs = batch.iter_mut().map(|part| std::mem::take(&mut part.text)).collect();
		// Prefixes were included before counting; do not apply them twice.
		let (embeddings, _) = provider.generate_embeddings_batch(inputs, InputType::None).await?;
		ensure!(
			embeddings.len() == batch.len(),
			"embedding provider returned an unexpected vector count"
		);
		for (part, vector) in batch.iter().zip(embeddings) {
			ensure!(
				!vector.is_empty() && vector.len() == provider.get_dimension() && vector.iter().all(|v| v.is_finite()),
				"embedding provider returned an invalid vector"
			);
			vectors[part.source].push((vector, part.weight));
		}
		start = end;
	}
	vectors
		.into_iter()
		.map(|vectors| {
			if vectors.len() == 1 {
				// Preserve existing vectors byte-for-byte for ordinary inputs.
				Ok(vectors.into_iter().next().expect("one vector per input part").0)
			} else {
				pool(vectors)
			}
		})
		.collect()
}

// Token-weighted averaging followed by L2 normalization, as in OpenAI's
// long-input embedding recipe. f64 accumulation avoids intermediate overflow.
fn pool(vectors: Vec<(Vec<f32>, usize)>) -> Result<Vec<f32>> {
	let mut sum = vec![0.0_f64; vectors[0].0.len()];
	for (vector, weight) in vectors {
		for (total, value) in sum.iter_mut().zip(vector) {
			*total += f64::from(value) * weight as f64;
		}
	}
	let norm = sum.iter().map(|v| v * v).sum::<f64>().sqrt();
	ensure!(
		norm.is_finite() && norm > 0.0,
		"split embedding has zero or non-finite norm"
	);
	Ok(sum.into_iter().map(|value| (value / norm) as f32).collect())
}

#[cfg(test)]
#[path = "openai_inputs_tests.rs"]
mod tests;
