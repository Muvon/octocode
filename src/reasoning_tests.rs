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
	use crate::config::Config;
	use crate::reasoning::reason_rank_code_blocks;
	use crate::store::CodeBlock;

	fn blocks(n: usize) -> Vec<CodeBlock> {
		(0..n)
			.map(|i| CodeBlock {
				path: format!("src/f{i}.rs"),
				language: "rust".to_string(),
				content: format!("fn f{i}() {{}}"),
				symbols: vec![format!("f{i}")],
				start_line: 1,
				end_line: 2,
				hash: format!("h{i}"),
				distance: Some(0.1 * i as f32),
			})
			.collect()
	}

	#[tokio::test]
	async fn a_disabled_reasoner_returns_the_input_untouched() {
		let mut config = Config::default();
		config.search.reasoning.enabled = false;

		let input = blocks(3);
		let out = reason_rank_code_blocks("query", input.clone(), &config)
			.await
			.unwrap();
		assert_eq!(out.len(), 3);
		assert_eq!(out[0].path, input[0].path);
		assert_eq!(out[0].distance, input[0].distance);
	}

	#[tokio::test]
	async fn an_empty_candidate_list_short_circuits() {
		let mut config = Config::default();
		config.search.reasoning.enabled = true;
		assert!(reason_rank_code_blocks("query", vec![], &config)
			.await
			.unwrap()
			.is_empty());
	}

	#[tokio::test]
	async fn an_unbuildable_client_keeps_the_truncated_input_order() {
		// A model string with no provider prefix cannot resolve, so the reasoner
		// must fall back to the hybrid order instead of failing the search.
		let mut config = Config::default();
		config.search.reasoning.enabled = true;
		config.search.reasoning.model = "no-provider-prefix".to_string();
		config.search.reasoning.max_candidates = 2;

		let out = reason_rank_code_blocks("query", blocks(5), &config)
			.await
			.unwrap();
		assert_eq!(
			out.len(),
			2,
			"the candidate pool is truncated before the call"
		);
		assert_eq!(out[0].path, "src/f0.rs");
		assert_eq!(out[1].path, "src/f1.rs");
	}

	#[tokio::test]
	async fn a_zero_candidate_budget_still_keeps_one_candidate() {
		let mut config = Config::default();
		config.search.reasoning.enabled = true;
		config.search.reasoning.model = "no-provider-prefix".to_string();
		config.search.reasoning.max_candidates = 0;

		let out = reason_rank_code_blocks("query", blocks(3), &config)
			.await
			.unwrap();
		assert_eq!(out.len(), 1);
	}

	#[tokio::test]
	async fn every_context_level_builds_a_prompt_without_panicking() {
		let mut config = Config::default();
		config.search.reasoning.enabled = true;
		config.search.reasoning.model = "no-provider-prefix".to_string();

		for level in ["signatures", "snippets", "full", "unrecognised"] {
			config.search.reasoning.context_level = level.to_string();
			let out = reason_rank_code_blocks("query", blocks(2), &config)
				.await
				.unwrap_or_else(|e| panic!("level {level} failed: {e}"));
			assert_eq!(out.len(), 2);
		}
	}

	#[tokio::test]
	async fn a_candidate_without_symbols_is_still_described() {
		let mut config = Config::default();
		config.search.reasoning.enabled = true;
		config.search.reasoning.model = "no-provider-prefix".to_string();

		let mut input = blocks(1);
		input[0].symbols.clear();
		let out = reason_rank_code_blocks("query", input, &config)
			.await
			.unwrap();
		assert_eq!(out.len(), 1);
	}
}
