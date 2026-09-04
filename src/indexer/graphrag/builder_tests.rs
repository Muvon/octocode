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
	use crate::config::Config;
	use crate::indexer::graphrag::types::RelationType;
	use crate::state::create_shared_state;
	use crate::store::mod_tests::{graph_node, graph_relationship, use_offline_test_config};
	use std::collections::HashSet;
	use tempfile::TempDir;

	/// A project whose persisted graph holds the chain a -> b -> c.
	async fn project() -> (TempDir, GraphBuilder) {
		use_offline_test_config();
		let dir = TempDir::new().unwrap();
		let index_path = crate::storage::get_project_database_path(dir.path()).unwrap();
		crate::storage::ensure_project_storage_exists(dir.path()).unwrap();

		let store = Store::new_with_path(index_path).await.unwrap();
		store.initialize_collections().await.unwrap();
		DatabaseOperations::new(&store)
			.save_graph_incremental(
				&[
					graph_node("src/a.rs", "file", 0),
					graph_node("src/b.rs", "file", 1),
					graph_node("src/c.rs", "file", 2),
				],
				&[
					graph_relationship("src/a.rs", "src/b.rs", RelationType::Imports),
					graph_relationship("src/b.rs", "src/c.rs", RelationType::Imports),
				],
			)
			.await
			.unwrap();
		drop(store);

		let mut config = Config::default();
		config.graphrag.use_llm = false;
		let builder = GraphBuilder::new_with_quiet(config, dir.path(), true)
			.await
			.expect("builder must open the project store");
		(dir, builder)
	}

	#[tokio::test]
	async fn a_builder_loads_the_persisted_graph_on_construction() {
		let (_dir, builder) = project().await;
		let graph = builder.get_graph().await.unwrap();
		assert_eq!(graph.nodes.len(), 3);
		assert_eq!(graph.relationships.len(), 2);
	}

	#[tokio::test]
	async fn a_project_without_a_graph_loads_an_empty_one() {
		use_offline_test_config();
		let dir = TempDir::new().unwrap();
		let mut config = Config::default();
		config.graphrag.use_llm = false;

		let builder = GraphBuilder::new_with_quiet(config, dir.path(), true)
			.await
			.unwrap();
		assert!(builder.get_graph().await.unwrap().nodes.is_empty());
	}

	#[tokio::test]
	async fn an_unparseable_embedding_model_is_rejected() {
		use_offline_test_config();
		let dir = TempDir::new().unwrap();
		let mut config = Config::default();
		config.embedding.text_model = "no-provider-prefix".to_string();

		// `GraphBuilder` is not `Debug`, so unwrap_err() is unavailable here.
		let err = match GraphBuilder::new_with_quiet(config, dir.path(), true).await {
			Ok(_) => panic!("an unparseable embedding model must not build a graph builder"),
			Err(e) => e,
		};
		assert!(
			err.to_string().contains("Failed to parse provider model"),
			"{err}"
		);
	}

	#[tokio::test]
	async fn paths_are_reported_relative_to_the_project_root() {
		let (dir, builder) = project().await;
		let absolute = dir.path().join("src/a.rs");
		let relative = builder
			.to_relative_path(&absolute.to_string_lossy())
			.unwrap();
		assert!(relative.ends_with("src/a.rs"), "{relative}");
	}

	#[tokio::test]
	async fn paths_between_nodes_follow_the_relationship_chain() {
		let (_dir, builder) = project().await;

		let paths = builder.find_paths("src/a.rs", "src/c.rs", 4).await.unwrap();
		assert_eq!(paths.len(), 1, "{paths:?}");
		assert_eq!(
			paths[0],
			vec![
				"src/a.rs".to_string(),
				"src/b.rs".to_string(),
				"src/c.rs".to_string()
			]
		);

		// A depth budget shorter than the chain finds nothing.
		assert!(builder
			.find_paths("src/a.rs", "src/c.rs", 1)
			.await
			.unwrap()
			.is_empty());
		// Unknown endpoints yield no paths rather than an error.
		assert!(builder
			.find_paths("src/nope.rs", "src/c.rs", 4)
			.await
			.unwrap()
			.is_empty());
	}

	#[tokio::test]
	async fn the_branch_filter_drops_overridden_nodes_and_their_edges() {
		let (_dir, builder) = project().await;

		// An empty override set leaves the graph alone.
		builder.apply_branch_filter(&HashSet::new()).await;
		assert_eq!(builder.get_graph().await.unwrap().nodes.len(), 3);

		let mut overridden = HashSet::new();
		overridden.insert("src/b.rs");
		builder.apply_branch_filter(&overridden).await;

		let graph = builder.get_graph().await.unwrap();
		assert_eq!(graph.nodes.len(), 2);
		assert!(!graph.nodes.contains_key("src/b.rs"));
		assert!(
			graph.relationships.is_empty(),
			"edges touching a dropped node must go too: {:?}",
			graph.relationships
		);
	}

	#[tokio::test]
	async fn a_branch_graph_is_merged_on_top_of_the_main_one() {
		let (dir, builder) = project().await;

		// Build a branch delta store holding one extra node.
		let branch_path = crate::storage::get_branch_database_path(dir.path(), "feature").unwrap();
		let branch_store = Store::new_with_path(branch_path).await.unwrap();
		branch_store.initialize_collections().await.unwrap();
		DatabaseOperations::new(&branch_store)
			.save_graph_incremental(
				&[graph_node("src/branch_only.rs", "file", 5)],
				&[graph_relationship(
					"src/branch_only.rs",
					"src/a.rs",
					RelationType::Imports,
				)],
			)
			.await
			.unwrap();

		builder.merge_branch_graph(&branch_store).await.unwrap();

		let graph = builder.get_graph().await.unwrap();
		assert!(graph.nodes.contains_key("src/branch_only.rs"));
		assert_eq!(graph.nodes.len(), 4);
		assert_eq!(graph.relationships.len(), 3);
	}

	#[tokio::test]
	async fn merging_an_empty_branch_graph_changes_nothing() {
		let (dir, builder) = project().await;
		let branch_path = crate::storage::get_branch_database_path(dir.path(), "empty").unwrap();
		let branch_store = Store::new_with_path(branch_path).await.unwrap();
		branch_store.initialize_collections().await.unwrap();

		builder.merge_branch_graph(&branch_store).await.unwrap();
		assert_eq!(builder.get_graph().await.unwrap().nodes.len(), 3);
	}

	#[tokio::test]
	async fn rebuilding_from_an_index_with_no_code_blocks_clears_the_graph() {
		let (_dir, builder) = project().await;
		let state = create_shared_state();

		builder
			.build_from_existing_database(Some(state.clone()))
			.await
			.expect("an empty code index must not be an error");

		assert!(builder.get_graph().await.unwrap().nodes.is_empty());
		assert_eq!(
			state.read().status_message,
			"No code blocks found in database for GraphRAG"
		);
	}

	#[tokio::test]
	async fn rebuilding_without_a_state_handle_also_succeeds() {
		let (_dir, builder) = project().await;
		builder.build_from_existing_database(None).await.unwrap();
	}

	#[tokio::test]
	async fn the_batch_trigger_follows_the_configured_size_and_token_budget() {
		let (_dir, builder) = project().await;
		let batch_size = builder.config.index.embeddings_batch_size;
		let max_tokens = builder.config.index.embeddings_max_tokens_per_batch;

		assert!(!builder.should_process_batch(&[]));

		let by_count: Vec<String> = (0..batch_size).map(|i| format!("chunk {i}")).collect();
		assert!(builder.should_process_batch(&by_count));

		// One oversized chunk trips the token estimate (len / 4) on its own.
		let by_tokens = vec!["x".repeat(max_tokens * 4)];
		assert!(builder.should_process_batch(&by_tokens));
	}

	#[tokio::test]
	async fn llm_enhancement_is_off_unless_configured() {
		let (_dir, builder) = project().await;
		assert!(!builder.llm_enabled());
	}
}
