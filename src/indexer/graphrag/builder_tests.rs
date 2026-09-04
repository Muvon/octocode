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
	use crate::indexer::graphrag::types::RelationType;
	use crate::state::create_shared_state;
	use crate::store::mod_tests::{
		code_block, graph_node, graph_relationship, offline_config, use_offline_test_config,
	};
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

		let mut config = offline_config();
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
		let mut config = offline_config();
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
		let mut config = offline_config();
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
		assert!(builder.ai_enhancements.is_none());
	}

	/// A builder with GraphRAG LLM support switched on. The provider is resolved
	/// from the model string alone, so construction stays offline.
	async fn llm_project() -> (TempDir, GraphBuilder) {
		use_offline_test_config();
		let dir = TempDir::new().unwrap();
		let mut config = offline_config();
		config.graphrag.use_llm = true;
		let builder = GraphBuilder::new_with_quiet(config, dir.path(), true)
			.await
			.expect("the shipped default LLM model must resolve without a key");
		(dir, builder)
	}

	#[tokio::test]
	async fn enabling_the_llm_attaches_the_ai_enhancements() {
		let (_dir, builder) = llm_project().await;
		assert!(builder.llm_enabled());
		assert!(builder.ai_enhancements.is_some());
	}

	#[tokio::test]
	async fn the_ai_helpers_are_inert_while_the_llm_is_off() {
		let (_dir, builder) = project().await;
		let block = code_block("src/x.rs", "h1");
		assert!(!builder.should_use_ai_for_description(&["main".to_string()], 10, "rust"));
		assert_eq!(builder.build_content_sample_for_ai(&[&block]), "");
	}

	#[tokio::test]
	async fn the_ai_helpers_delegate_once_the_llm_is_on() {
		let (_dir, builder) = llm_project().await;
		let block = code_block("src/x.rs", "h1");
		assert!(builder.should_use_ai_for_description(&["main".to_string()], 10, "rust"));
		// An unsupported language still short-circuits.
		assert!(!builder.should_use_ai_for_description(&["main".to_string()], 10, "klingon"));
		assert_eq!(
			builder.build_content_sample_for_ai(&[&block]),
			"// Block: 1 symbols\nfn from_h1() {}\n\n"
		);
	}

	#[tokio::test]
	async fn a_path_outside_the_project_root_cannot_be_relativized() {
		use_offline_test_config();
		// The marker file pins the detected project root to this directory, so a
		// sibling temp dir is unambiguously outside it.
		let dir = TempDir::new().unwrap();
		std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
		let mut config = offline_config();
		config.graphrag.use_llm = false;
		let builder = GraphBuilder::new_with_quiet(config, dir.path(), true)
			.await
			.unwrap();

		std::fs::write(dir.path().join("inside.rs"), "").unwrap();
		assert_eq!(
			builder
				.to_relative_path(&dir.path().join("inside.rs").to_string_lossy())
				.unwrap(),
			"inside.rs"
		);

		let elsewhere = TempDir::new().unwrap();
		let stranger = elsewhere.path().join("outside.rs");
		std::fs::write(&stranger, "").unwrap();
		let err = builder
			.to_relative_path(&stranger.to_string_lossy())
			.unwrap_err();
		assert!(
			err.to_string().contains("is not within project root"),
			"{err}"
		);
	}

	#[tokio::test]
	async fn every_acyclic_route_within_the_depth_budget_is_returned() {
		let (_dir, builder) = project().await;
		{
			let mut graph = builder.graph.write().await;
			graph
				.nodes
				.insert("src/d.rs".to_string(), graph_node("src/d.rs", "file", 3));
			// Diamond: a -> b -> d and a -> c -> d on top of the fixture's a -> b -> c.
			graph.relationships = vec![
				graph_relationship("src/a.rs", "src/b.rs", RelationType::Imports),
				graph_relationship("src/a.rs", "src/c.rs", RelationType::Imports),
				graph_relationship("src/b.rs", "src/d.rs", RelationType::Imports),
				graph_relationship("src/c.rs", "src/d.rs", RelationType::Imports),
			];
		}

		let mut paths = builder.find_paths("src/a.rs", "src/d.rs", 3).await.unwrap();
		paths.sort();
		assert_eq!(
			paths,
			vec![
				vec![
					"src/a.rs".to_string(),
					"src/b.rs".to_string(),
					"src/d.rs".to_string()
				],
				vec![
					"src/a.rs".to_string(),
					"src/c.rs".to_string(),
					"src/d.rs".to_string()
				],
			]
		);
	}

	#[tokio::test]
	async fn a_node_is_trivially_reachable_from_itself() {
		let (_dir, builder) = project().await;
		assert_eq!(
			builder.find_paths("src/a.rs", "src/a.rs", 2).await.unwrap(),
			vec![vec!["src/a.rs".to_string()]]
		);
	}

	#[tokio::test]
	async fn an_emptied_in_memory_graph_is_reloaded_from_the_database() {
		let (_dir, builder) = project().await;
		{
			let mut graph = builder.graph.write().await;
			graph.nodes.clear();
			graph.relationships.clear();
		}

		let graph = builder.get_graph().await.unwrap();
		assert_eq!(graph.nodes.len(), 3);
		assert_eq!(graph.relationships.len(), 2);
		// The reload is cached back into memory.
		assert_eq!(builder.graph.read().await.nodes.len(), 3);
	}

	#[tokio::test]
	async fn a_branch_node_replaces_the_main_node_with_the_same_id() {
		let (dir, builder) = project().await;
		let branch_path = crate::storage::get_branch_database_path(dir.path(), "swap").unwrap();
		let branch_store = Store::new_with_path(branch_path).await.unwrap();
		branch_store.initialize_collections().await.unwrap();
		DatabaseOperations::new(&branch_store)
			.save_graph_incremental(
				&[graph_node("src/a.rs", "module", 9)],
				// `load_graph` only reads a store once BOTH graphrag tables exist,
				// and they are created lazily on first write — so the branch delta
				// needs a relationship for its nodes to be readable at all.
				&[graph_relationship(
					"src/a.rs",
					"src/c.rs",
					RelationType::Calls,
				)],
			)
			.await
			.unwrap();

		assert_eq!(
			builder.get_graph().await.unwrap().nodes["src/a.rs"].kind,
			"file"
		);
		builder.merge_branch_graph(&branch_store).await.unwrap();

		let graph = builder.get_graph().await.unwrap();
		assert_eq!(graph.nodes.len(), 3, "the id is replaced, not added");
		assert_eq!(graph.nodes["src/a.rs"].kind, "module");
		assert_eq!(graph.relationships.len(), 3, "branch edges are appended");
	}

	#[tokio::test]
	async fn blocks_for_files_that_no_longer_exist_are_skipped() {
		let (_dir, builder) = project().await;
		let state = create_shared_state();

		builder
			.process_files_from_codeblocks(
				&[code_block("/definitely/not/here/gone.rs", "h1")],
				Some(state.clone()),
			)
			.await
			.unwrap();

		assert_eq!(
			state.read().status_message,
			"GraphRAG processing complete: 0 files processed (1 skipped)"
		);
		assert_eq!(builder.get_graph().await.unwrap().nodes.len(), 3);
	}

	#[tokio::test]
	async fn a_file_whose_content_hash_is_unchanged_is_skipped() {
		let (dir, builder) = project().await;
		std::fs::create_dir_all(dir.path().join("src")).unwrap();
		let file = dir.path().join("src/unchanged.rs");
		let block = code_block(&file.to_string_lossy(), "h1");
		std::fs::write(&file, &block.content).unwrap();

		// Seed the graph with a node carrying exactly the hash the builder will
		// compute for this file's blocks.
		let relative = builder.to_relative_path(&block.path).unwrap();
		let hash = calculate_unique_content_hash(&block.content, &block.path);
		{
			let mut graph = builder.graph.write().await;
			let mut node = graph_node("src/a.rs", "file", 0);
			node.id = relative.clone();
			node.path = relative.clone();
			node.hash = hash;
			graph.nodes.insert(relative, node);
		}

		let state = create_shared_state();
		builder
			.process_files_from_codeblocks(&[block], Some(state.clone()))
			.await
			.unwrap();

		assert_eq!(
			state.read().status_message,
			"GraphRAG processing complete: 0 files processed (1 skipped)"
		);
	}

	#[tokio::test]
	async fn processing_no_blocks_leaves_the_graph_untouched() {
		let (_dir, builder) = project().await;
		builder.process_code_blocks(&[], None).await.unwrap();
		assert_eq!(builder.get_graph().await.unwrap().nodes.len(), 3);
	}

	#[tokio::test]
	async fn the_store_is_flushed_only_once_the_batch_frequency_is_reached() {
		let (_dir, builder) = project().await;
		let frequency = builder.config.index.flush_frequency;

		let mut batches = frequency - 1;
		builder.flush_if_needed(&mut batches).await.unwrap();
		assert_eq!(
			batches,
			frequency - 1,
			"below the frequency nothing flushes"
		);

		let mut batches = frequency;
		builder.flush_if_needed(&mut batches).await.unwrap();
		assert_eq!(batches, 0, "reaching the frequency flushes and resets");
	}
}
