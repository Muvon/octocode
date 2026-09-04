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
	use octocode::state;

	fn state_with(
		indexed: usize,
		skipped: usize,
		total: usize,
		graphrag: bool,
		blocks: usize,
	) -> Arc<RwLock<state::IndexState>> {
		let shared = state::create_shared_state();
		{
			let mut guard = shared.write();
			guard.indexed_files = indexed;
			guard.skipped_files = skipped;
			guard.total_files = total;
			guard.graphrag_enabled = graphrag;
			guard.graphrag_blocks = blocks;
		}
		shared
	}

	#[test]
	fn the_summary_covers_every_combination_of_skips_and_graphrag() {
		print_indexing_summary(&state_with(10, 0, 10, false, 0));
		print_indexing_summary(&state_with(4, 6, 10, false, 0));
		print_indexing_summary(&state_with(10, 0, 10, true, 42));
		print_indexing_summary(&state_with(4, 6, 10, true, 42));
	}

	#[test]
	fn a_path_matches_only_on_a_segment_boundary() {
		assert!(ends_with_path_boundary("index.rs", "index.rs"));
		assert!(ends_with_path_boundary("src/commands/index.rs", "index.rs"));
		assert!(ends_with_path_boundary("a/b/c.rs", "b/c.rs"));

		// A shared suffix that is not a whole segment must not match.
		assert!(!ends_with_path_boundary("src/reindex.rs", "index.rs"));
		assert!(!ends_with_path_boundary("index.rs", "src/index.rs"));
		assert!(!ends_with_path_boundary("other.rs", "index.rs"));
	}
}
