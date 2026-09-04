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
	use crate::indexer::graphrag::types::{FunctionInfo, RelationType};
	use crate::store::mod_tests::{
		embedding, graph_node, graph_relationship, test_store, CODE_DIM,
	};
	use std::path::Path;

	#[tokio::test]
	async fn an_unindexed_project_loads_an_empty_graph() {
		let (_dir, store) = test_store().await;
		let graph = DatabaseOperations::new(&store)
			.load_graph(Path::new("/repo"), true)
			.await
			.unwrap();
		assert!(graph.nodes.is_empty());
		assert!(graph.relationships.is_empty());
	}

	#[tokio::test]
	async fn saved_nodes_and_relationships_load_back_intact() {
		let (_dir, store) = test_store().await;
		let ops = DatabaseOperations::new(&store);

		let mut node_a = graph_node("src/a.rs", "file", 0);
		node_a.functions = vec![FunctionInfo {
			name: "run".to_string(),
			signature: "run()".to_string(),
			start_line: 1,
			end_line: 5,
			calls: vec![],
			called_by: vec![],
			parameters: vec![],
			return_type: None,
			extends: vec![],
			implements: vec![],
		}];
		let node_b = graph_node("src/b.rs", "file", 1);

		ops.save_graph_incremental(
			&[node_a.clone(), node_b],
			&[graph_relationship(
				"src/a.rs",
				"src/b.rs",
				RelationType::Imports,
			)],
		)
		.await
		.unwrap();

		let graph = ops.load_graph(Path::new("/repo"), true).await.unwrap();
		assert_eq!(graph.nodes.len(), 2);
		assert_eq!(graph.relationships.len(), 1);

		let loaded = graph.nodes.get("src/a.rs").expect("node a");
		assert_eq!(loaded.name, "a.rs");
		assert_eq!(loaded.kind, "file");
		assert_eq!(loaded.language, "rust");
		assert_eq!(loaded.size_lines, 42);
		assert_eq!(loaded.symbols, vec!["sym_0".to_string()]);
		assert_eq!(loaded.imports, vec!["dep".to_string()]);
		assert_eq!(loaded.exports, vec!["api".to_string()]);
		assert_eq!(loaded.functions.len(), 1);
		assert_eq!(loaded.functions[0].name, "run");
		assert_eq!(loaded.embedding.len(), CODE_DIM);

		let rel = &graph.relationships[0];
		assert_eq!(rel.source, "src/a.rs");
		assert_eq!(rel.target, "src/b.rs");
		assert_eq!(rel.relation_type, RelationType::Imports);
		assert!((rel.confidence - 0.9).abs() < 1e-6);
	}

	#[tokio::test]
	async fn saving_nothing_leaves_the_graph_untouched() {
		let (_dir, store) = test_store().await;
		let ops = DatabaseOperations::new(&store);
		ops.save_graph_incremental(&[], &[]).await.unwrap();
		let graph = ops.load_graph(Path::new("/repo"), true).await.unwrap();
		assert!(graph.nodes.is_empty());
	}

	#[tokio::test]
	async fn nodes_can_be_saved_without_any_relationships() {
		let (_dir, store) = test_store().await;
		let ops = DatabaseOperations::new(&store);
		ops.save_graph_incremental(&[graph_node("src/a.rs", "file", 0)], &[])
			.await
			.unwrap();

		// `load_graph` needs both tables; with only nodes written it reports empty
		// rather than failing.
		let graph = ops.load_graph(Path::new("/repo"), true).await.unwrap();
		assert!(graph.nodes.is_empty());

		// The nodes really are stored, though.
		assert_eq!(store.get_all_graph_nodes().await.unwrap().num_rows(), 1);
	}

	#[tokio::test]
	async fn a_node_embedding_of_the_wrong_width_is_rejected() {
		let (_dir, store) = test_store().await;
		let mut node = graph_node("src/a.rs", "file", 0);
		node.embedding = vec![0.0; 3];

		let err = DatabaseOperations::new(&store)
			.save_graph_incremental(&[node], &[])
			.await
			.unwrap_err()
			.to_string();
		assert!(err.contains("dimension 3 but expected"), "{err}");
	}

	#[tokio::test]
	async fn searching_an_empty_database_returns_nothing() {
		let (_dir, store) = test_store().await;
		let hits = DatabaseOperations::new(&store)
			.search_nodes_in_database(&embedding(CODE_DIM, 0), "anything")
			.await
			.unwrap();
		assert!(hits.is_empty());
	}

	#[tokio::test]
	async fn search_returns_the_nearest_node_first() {
		let (_dir, store) = test_store().await;
		let ops = DatabaseOperations::new(&store);
		ops.save_graph_incremental(
			&[
				graph_node("src/a.rs", "file", 0),
				graph_node("src/b.rs", "file", 1),
			],
			&[],
		)
		.await
		.unwrap();

		let hits = ops
			.search_nodes_in_database(&embedding(CODE_DIM, 1), "b")
			.await
			.unwrap();
		assert!(!hits.is_empty());
		assert_eq!(hits[0].id, "src/b.rs");
	}

	#[tokio::test]
	async fn a_verbose_load_prints_progress_without_changing_the_result() {
		let (_dir, store) = test_store().await;
		let ops = DatabaseOperations::new(&store);
		ops.save_graph_incremental(
			&[graph_node("src/a.rs", "file", 0)],
			&[graph_relationship(
				"src/a.rs",
				"src/a.rs",
				RelationType::Contains,
			)],
		)
		.await
		.unwrap();

		let quiet = ops.load_graph(Path::new("/repo"), true).await.unwrap();
		let loud = ops.load_graph(Path::new("/repo"), false).await.unwrap();
		assert_eq!(quiet.nodes.len(), loud.nodes.len());
		assert_eq!(quiet.relationships.len(), loud.relationships.len());
	}
}
