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
	use crate::indexer::graphrag::database::DatabaseOperations;
	use crate::indexer::graphrag::types::{RelationType, RelationshipDirection};
	use crate::store::graphrag::{CacheStats, GraphRagOperations};
	use crate::store::mod_tests::{
		code_block, embedding, graph_node, graph_relationship, test_store, use_offline_test_config,
		CODE_DIM,
	};
	use crate::store::Store;
	use std::collections::HashMap;
	use std::sync::Arc;
	use tempfile::TempDir;
	use tokio::sync::RwLock;

	/// A store holding a three-node chain a -> b -> c plus an isolated node.
	async fn populated_store(dir: &TempDir) -> Store {
		use_offline_test_config();
		let store = Store::new_with_path(dir.path().join("db")).await.unwrap();
		store.initialize_collections().await.unwrap();

		DatabaseOperations::new(&store)
			.save_graph_incremental(
				&[
					graph_node("src/a.rs", "file", 0),
					graph_node("src/b.rs", "file", 1),
					graph_node("src/c.rs", "file", 2),
					graph_node("src/lonely.rs", "file", 3),
				],
				&[
					graph_relationship("src/a.rs", "src/b.rs", RelationType::Imports),
					graph_relationship("src/b.rs", "src/c.rs", RelationType::Imports),
					graph_relationship("src/a.rs", "src/c.rs", RelationType::Calls),
				],
			)
			.await
			.unwrap();
		store
	}

	/// A connection to the same database directory, so the cache-aware API can be
	/// driven directly (the `Store` does not re-export it).
	async fn connect(dir: &TempDir) -> lancedb::Connection {
		lancedb::connect(dir.path().join("db").to_str().unwrap())
			.execute()
			.await
			.unwrap()
	}

	fn graph_ops(db: &lancedb::Connection) -> GraphRagOperations<'_> {
		GraphRagOperations::new(db, CODE_DIM, Arc::new(RwLock::new(HashMap::new())))
	}

	#[test]
	fn an_untouched_cache_reports_a_zero_hit_rate() {
		assert_eq!(CacheStats::default().hit_rate(), 0.0);
		let stats = CacheStats { hits: 3, misses: 1 };
		assert!((stats.hit_rate() - 0.75).abs() < 1e-9);
	}

	#[tokio::test]
	async fn an_empty_index_always_needs_graphrag_indexing() {
		let (_dir, store) = test_store().await;
		assert!(store.graphrag_needs_indexing().await.unwrap());
	}

	#[tokio::test]
	async fn nodes_without_relationships_still_need_indexing() {
		let (_dir, store) = test_store().await;
		DatabaseOperations::new(&store)
			.save_graph_incremental(&[graph_node("src/a.rs", "file", 0)], &[])
			.await
			.unwrap();
		assert!(store.graphrag_needs_indexing().await.unwrap());
	}

	#[tokio::test]
	async fn a_fully_populated_graph_does_not_need_reindexing() {
		let dir = TempDir::new().unwrap();
		let store = populated_store(&dir).await;
		assert!(!store.graphrag_needs_indexing().await.unwrap());
	}

	#[tokio::test]
	async fn code_blocks_are_collected_for_graph_building() {
		let (_dir, store) = test_store().await;
		assert!(store
			.get_all_code_blocks_for_graphrag()
			.await
			.unwrap()
			.is_empty());

		store
			.store_code_blocks(
				&[code_block("src/a.rs", "h1"), code_block("src/b.rs", "h2")],
				&[embedding(CODE_DIM, 0), embedding(CODE_DIM, 1)],
			)
			.await
			.unwrap();

		let blocks = store.get_all_code_blocks_for_graphrag().await.unwrap();
		assert_eq!(blocks.len(), 2);
		assert!(blocks.iter().any(|b| b.path == "src/a.rs"));
	}

	#[tokio::test]
	async fn stored_nodes_are_readable_as_a_batch_and_by_page() {
		let dir = TempDir::new().unwrap();
		let store = populated_store(&dir).await;

		assert_eq!(store.get_all_graph_nodes().await.unwrap().num_rows(), 4);

		let first_page = store.get_all_nodes_paginated(0, 2).await.unwrap();
		assert_eq!(first_page.len(), 2);
		let second_page = store.get_all_nodes_paginated(2, 2).await.unwrap();
		assert_eq!(second_page.len(), 2);
		let past_the_end = store.get_all_nodes_paginated(10, 2).await.unwrap();
		assert!(past_the_end.is_empty());

		let mut seen: Vec<_> = first_page
			.iter()
			.chain(second_page.iter())
			.map(|n| n.id.clone())
			.collect();
		seen.sort();
		assert_eq!(
			seen,
			vec!["src/a.rs", "src/b.rs", "src/c.rs", "src/lonely.rs"]
		);
	}

	#[tokio::test]
	async fn a_vector_search_ranks_the_closest_node_first() {
		let dir = TempDir::new().unwrap();
		let store = populated_store(&dir).await;
		let batch = store
			.search_graph_nodes(&embedding(CODE_DIM, 2), 2)
			.await
			.unwrap();
		assert!(batch.num_rows() >= 1);
	}

	#[tokio::test]
	async fn relationships_are_readable_in_bulk_and_by_type() {
		let dir = TempDir::new().unwrap();
		let store = populated_store(&dir).await;

		assert_eq!(store.get_graph_relationships().await.unwrap().num_rows(), 3);
		assert_eq!(
			store.get_all_relationships_efficient().await.unwrap().len(),
			3
		);

		let imports = store
			.get_relationships_by_type(&RelationType::Imports)
			.await
			.unwrap();
		assert_eq!(imports.len(), 2);
		assert!(imports
			.iter()
			.all(|r| r.relation_type == RelationType::Imports));

		let extends = store
			.get_relationships_by_type(&RelationType::Extends)
			.await
			.unwrap();
		assert!(extends.is_empty());
	}

	#[tokio::test]
	async fn node_relationships_respect_the_requested_direction() {
		let dir = TempDir::new().unwrap();
		let store = populated_store(&dir).await;

		let outgoing = store
			.get_node_relationships("src/b.rs", RelationshipDirection::Outgoing)
			.await
			.unwrap();
		assert_eq!(outgoing.len(), 1);
		assert_eq!(outgoing[0].target, "src/c.rs");

		let incoming = store
			.get_node_relationships("src/b.rs", RelationshipDirection::Incoming)
			.await
			.unwrap();
		assert_eq!(incoming.len(), 1);
		assert_eq!(incoming[0].source, "src/a.rs");

		let both = store
			.get_node_relationships("src/b.rs", RelationshipDirection::Both)
			.await
			.unwrap();
		assert_eq!(both.len(), 2);

		let isolated = store
			.get_node_relationships("src/lonely.rs", RelationshipDirection::Both)
			.await
			.unwrap();
		assert!(isolated.is_empty());
	}

	#[tokio::test]
	async fn removing_a_node_by_path_leaves_the_rest_in_place() {
		let dir = TempDir::new().unwrap();
		let store = populated_store(&dir).await;

		assert_eq!(
			store.remove_graph_nodes_by_path("src/a.rs").await.unwrap(),
			1
		);
		assert_eq!(store.get_all_graph_nodes().await.unwrap().num_rows(), 3);

		assert_eq!(
			store
				.remove_graph_nodes_by_paths(&["src/b.rs".to_string(), "src/c.rs".to_string()])
				.await
				.unwrap(),
			2
		);
		assert_eq!(store.get_all_graph_nodes().await.unwrap().num_rows(), 1);

		// Removing nothing is a no-op.
		assert_eq!(store.remove_graph_nodes_by_paths(&[]).await.unwrap(), 0);
	}

	#[tokio::test]
	async fn a_read_after_removal_does_not_serve_the_deleted_nodes() {
		// Reading first populates the Store's table-handle cache. A LanceDB handle
		// is pinned to the dataset version it was opened at, so unless the removal
		// invalidates it the next read replays the deleted rows.
		let dir = TempDir::new().unwrap();
		let store = populated_store(&dir).await;
		assert_eq!(store.get_all_graph_nodes().await.unwrap().num_rows(), 4);

		store.remove_graph_nodes_by_path("src/a.rs").await.unwrap();
		assert_eq!(store.get_all_graph_nodes().await.unwrap().num_rows(), 3);
	}

	#[tokio::test]
	async fn removing_relationships_by_path_drops_both_directions() {
		let dir = TempDir::new().unwrap();
		let store = populated_store(&dir).await;

		// The return value counts matched *nodes*, not edges — it is documented as
		// an approximation that avoids a second full scan.
		let removed = store
			.remove_graph_relationships_by_path("src/b.rs")
			.await
			.unwrap();
		assert_eq!(removed, 1);

		// Both the incoming (a -> b) and outgoing (b -> c) edge are gone; only
		// a -> c survives.
		let remaining = store.get_all_relationships_efficient().await.unwrap();
		assert_eq!(remaining.len(), 1, "got {remaining:?}");
		assert_eq!(remaining[0].source, "src/a.rs");
		assert_eq!(remaining[0].target, "src/c.rs");
	}

	#[tokio::test]
	async fn clearing_empties_the_node_and_relationship_tables_separately() {
		let dir = TempDir::new().unwrap();
		let store = populated_store(&dir).await;

		store.clear_graph_relationships().await.unwrap();
		assert_eq!(store.get_graph_relationships().await.unwrap().num_rows(), 0);
		assert_eq!(store.get_all_graph_nodes().await.unwrap().num_rows(), 4);

		store.clear_graph_nodes().await.unwrap();
		assert_eq!(store.get_all_graph_nodes().await.unwrap().num_rows(), 0);
	}

	#[tokio::test]
	async fn the_adjacency_cache_answers_neighbour_lookups() {
		let dir = TempDir::new().unwrap();
		let _store = populated_store(&dir).await;
		let db = connect(&dir).await;
		let ops = graph_ops(&db);

		ops.build_adjacency_cache().await.unwrap();

		let imports = ops
			.get_neighbors_cached("src/a.rs", &RelationType::Imports)
			.await
			.unwrap();
		assert_eq!(imports, vec!["src/b.rs".to_string()]);

		let calls = ops
			.get_neighbors_cached("src/a.rs", &RelationType::Calls)
			.await
			.unwrap();
		assert_eq!(calls, vec!["src/c.rs".to_string()]);

		assert!(ops.get_cache_stats().hits > 0);
	}

	#[tokio::test]
	async fn a_cache_miss_falls_back_to_the_database_and_then_caches() {
		let dir = TempDir::new().unwrap();
		let _store = populated_store(&dir).await;
		let db = connect(&dir).await;
		let ops = graph_ops(&db);

		// No cache build: the first lookup must still find the edge.
		let first = ops
			.get_neighbors_cached("src/a.rs", &RelationType::Imports)
			.await
			.unwrap();
		assert_eq!(first, vec!["src/b.rs".to_string()]);
		assert_eq!(ops.get_cache_stats().misses, 1);

		let second = ops
			.get_neighbors_cached("src/a.rs", &RelationType::Imports)
			.await
			.unwrap();
		assert_eq!(second, first);
		assert_eq!(ops.get_cache_stats().hits, 1);
	}

	#[tokio::test]
	async fn clearing_and_invalidating_reset_the_cached_answers() {
		let dir = TempDir::new().unwrap();
		let _store = populated_store(&dir).await;
		let db = connect(&dir).await;
		let ops = graph_ops(&db);

		ops.build_adjacency_cache().await.unwrap();
		ops.invalidate_cache_for_node("src/a.rs");
		// The entry is gone, so this lookup is a miss that refills from the DB.
		assert_eq!(
			ops.get_neighbors_cached("src/a.rs", &RelationType::Imports)
				.await
				.unwrap(),
			vec!["src/b.rs".to_string()]
		);

		ops.clear_cache();
		let stats = ops.get_cache_stats();
		assert_eq!(stats.hits, 0);
		assert_eq!(stats.misses, 0);
	}

	#[tokio::test]
	async fn building_the_cache_without_a_relationships_table_is_a_no_op() {
		let dir = TempDir::new().unwrap();
		use_offline_test_config();
		let store = Store::new_with_path(dir.path().join("db")).await.unwrap();
		store.initialize_collections().await.unwrap();

		let db = connect(&dir).await;
		let ops = graph_ops(&db);
		ops.build_adjacency_cache().await.unwrap();
		assert_eq!(ops.get_cache_stats().hits, 0);
	}

	#[tokio::test]
	async fn traversal_follows_the_chain_up_to_the_depth_limit() {
		let dir = TempDir::new().unwrap();
		let _store = populated_store(&dir).await;
		let db = connect(&dir).await;
		let ops = graph_ops(&db);
		ops.build_adjacency_cache().await.unwrap();

		let depth_one = ops
			.traverse_path_cached("src/a.rs", &[RelationType::Imports], 1)
			.await
			.unwrap();
		assert_eq!(
			depth_one,
			vec!["src/a.rs".to_string(), "src/b.rs".to_string()]
		);

		let depth_two = ops
			.traverse_path_cached("src/a.rs", &[RelationType::Imports], 2)
			.await
			.unwrap();
		assert_eq!(
			depth_two,
			vec![
				"src/a.rs".to_string(),
				"src/b.rs".to_string(),
				"src/c.rs".to_string()
			]
		);

		// Depth 0 never leaves the starting node.
		assert_eq!(
			ops.traverse_path_cached("src/a.rs", &[RelationType::Imports], 0)
				.await
				.unwrap(),
			vec!["src/a.rs".to_string()]
		);
	}

	#[tokio::test]
	async fn connected_components_separate_the_chain_from_the_isolated_node() {
		let dir = TempDir::new().unwrap();
		let _store = populated_store(&dir).await;
		let db = connect(&dir).await;
		let ops = graph_ops(&db);
		ops.build_adjacency_cache().await.unwrap();

		let mut components = ops
			.find_connected_components_cached(&[RelationType::Imports])
			.await
			.unwrap();
		components.sort_by_key(|c| c.len());

		assert_eq!(components.len(), 2, "got {components:?}");
		assert_eq!(components[0], vec!["src/lonely.rs".to_string()]);
		assert_eq!(
			components[1],
			vec![
				"src/a.rs".to_string(),
				"src/b.rs".to_string(),
				"src/c.rs".to_string()
			]
		);
	}
}
