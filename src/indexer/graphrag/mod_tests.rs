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
	use crate::indexer::graphrag::types::RelationType;
	use crate::indexer::graphrag::GraphRAG;
	use crate::store::mod_tests::{
		graph_node, graph_relationship, offline_config, use_offline_test_config,
	};
	use crate::store::Store;
	use tempfile::TempDir;

	/// A project whose persisted graph holds the chain a -> b -> c plus an
	/// unconnected node. GraphRAG stays disabled so nothing reaches an LLM.
	async fn project() -> (TempDir, GraphRAG) {
		use_offline_test_config();
		let dir = TempDir::new().unwrap();
		let working = dir.path().to_path_buf();
		let index_path = crate::storage::get_project_database_path(dir.path()).unwrap();
		crate::storage::ensure_project_storage_exists(dir.path()).unwrap();

		let store = Store::new_with_path(index_path).await.unwrap();
		store.initialize_collections().await.unwrap();
		DatabaseOperations::new(&store)
			.save_graph_incremental(
				&[
					graph_node("src/a.rs", "file", 0),
					graph_node("src/b.rs", "file", 1),
					graph_node("src/c.rs", "module", 2),
					graph_node("src/lonely.rs", "file", 3),
				],
				&[
					graph_relationship("src/a.rs", "src/b.rs", RelationType::Imports),
					graph_relationship("src/b.rs", "src/c.rs", RelationType::Calls),
				],
			)
			.await
			.unwrap();
		drop(store);

		let mut config = offline_config();
		config.graphrag.use_llm = false;
		(dir, GraphRAG::new(config, working))
	}

	#[tokio::test]
	async fn a_node_is_rendered_with_its_metadata() {
		let (_dir, graphrag) = project().await;
		let out = graphrag.get_node("src/a.rs").await.unwrap();
		assert!(out.contains("Node: a.rs"));
		assert!(out.contains("ID: src/a.rs"));
		assert!(out.contains("Kind: file"));
		assert!(out.contains("Symbols: sym_0"));
	}

	#[tokio::test]
	async fn an_unknown_node_is_an_error() {
		let (_dir, graphrag) = project().await;
		let err = graphrag.get_node("src/nope.rs").await.unwrap_err();
		assert!(err.to_string().contains("Node not found"), "{err}");
	}

	#[tokio::test]
	async fn relationships_are_listed_by_direction() {
		let (_dir, graphrag) = project().await;
		let out = graphrag.get_relationships("src/b.rs").await.unwrap();
		assert!(
			out.contains("Relationships for src/b.rs (2 total)"),
			"{out}"
		);
		assert!(out.contains("Outgoing:"), "{out}");
		assert!(out.contains("Incoming:"), "{out}");
		assert!(out.contains("c.rs (src/c.rs)"), "{out}");
	}

	#[tokio::test]
	async fn an_unconnected_node_reports_no_relationships() {
		let (_dir, graphrag) = project().await;
		let out = graphrag.get_relationships("src/lonely.rs").await.unwrap();
		assert_eq!(out, "No relationships found for node: src/lonely.rs");
	}

	#[tokio::test]
	async fn relationships_for_an_unknown_node_are_an_error() {
		let (_dir, graphrag) = project().await;
		assert!(graphrag.get_relationships("src/nope.rs").await.is_err());
	}

	#[tokio::test]
	async fn a_path_is_rendered_with_the_edge_types() {
		let (_dir, graphrag) = project().await;
		let out = graphrag.find_path("src/a.rs", "src/c.rs", 4).await.unwrap();
		assert!(out.contains("Paths from src/a.rs to src/c.rs"), "{out}");
		assert!(out.contains("--imports->"), "{out}");
		assert!(out.contains("--calls->"), "{out}");
	}

	#[tokio::test]
	async fn an_unreachable_target_reports_no_path() {
		let (_dir, graphrag) = project().await;
		let out = graphrag
			.find_path("src/a.rs", "src/lonely.rs", 4)
			.await
			.unwrap();
		assert!(out.starts_with("No paths found"), "{out}");
	}

	#[tokio::test]
	async fn a_path_endpoint_that_does_not_exist_is_an_error() {
		let (_dir, graphrag) = project().await;
		let source = graphrag.find_path("src/nope.rs", "src/a.rs", 3).await;
		assert!(source
			.unwrap_err()
			.to_string()
			.contains("Source node not found"));

		let target = graphrag.find_path("src/a.rs", "src/nope.rs", 3).await;
		assert!(target
			.unwrap_err()
			.to_string()
			.contains("Target node not found"));
	}

	#[tokio::test]
	async fn the_overview_counts_nodes_and_relationships_by_kind() {
		let (_dir, graphrag) = project().await;
		let out = graphrag.overview().await.unwrap();
		assert!(
			out.starts_with("GraphRAG Overview: 4 nodes, 2 relationships"),
			"{out}"
		);
		assert!(out.contains("  file: 3"), "{out}");
		assert!(out.contains("  module: 1"), "{out}");
		assert!(out.contains("  imports: 1"), "{out}");
		assert!(out.contains("  calls: 1"), "{out}");
	}

	#[tokio::test]
	async fn the_config_is_available_to_callers() {
		let (_dir, graphrag) = project().await;
		assert!(!graphrag.config().graphrag.use_llm);
	}
}
