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
	use crate::indexer::graph_optimization::*;
	use crate::indexer::graphrag::types::{
		CodeGraph, CodeNode, CodeRelationship, Provenance, RelationType,
	};
	use crate::store::CodeBlock;
	use std::collections::HashMap;

	fn node(id: &str, kind: &str, embedding: Vec<f32>) -> CodeNode {
		CodeNode {
			id: id.to_string(),
			name: id.rsplit('/').next().unwrap_or(id).to_string(),
			kind: kind.to_string(),
			path: id.to_string(),
			description: "handles requestTimeout and retry_policy wiring".to_string(),
			symbols: vec!["handler".to_string()],
			hash: format!("hash-{id}"),
			embedding,
			imports: vec![],
			exports: vec![],
			functions: vec![],
			size_lines: 10,
			language: "rust".to_string(),
		}
	}

	fn relationship(source: &str, target: &str, kind: RelationType) -> CodeRelationship {
		CodeRelationship {
			source: source.to_string(),
			target: target.to_string(),
			relation_type: kind,
			description: "rel".to_string(),
			confidence: 0.9,
			weight: 1.0,
			provenance: Provenance::Extracted,
		}
	}

	fn code_block(path: &str, content: &str, symbols: Vec<&str>) -> CodeBlock {
		CodeBlock {
			path: path.to_string(),
			language: "rust".to_string(),
			content: content.to_string(),
			symbols: symbols.into_iter().map(String::from).collect(),
			start_line: 1,
			end_line: 5,
			hash: format!("h-{path}"),
			distance: None,
		}
	}

	fn graph(nodes: Vec<CodeNode>, relationships: Vec<CodeRelationship>) -> CodeGraph {
		let mut map = HashMap::new();
		for n in nodes {
			map.insert(n.id.clone(), n);
		}
		CodeGraph {
			nodes: map,
			relationships,
		}
	}

	#[test]
	fn empty_subgraph_costs_no_tokens_and_reports_zero_counts() {
		let subgraph = TaskFocusedSubgraph::default();
		assert_eq!(subgraph.estimated_token_count(), 0);
		let md = subgraph.to_markdown();
		assert!(md.starts_with("# Code Knowledge Graph: 0 nodes, 0 relationships"));
		assert!(!md.contains("## Key Concepts"));
		assert!(!md.contains("## Relevant Files"));
	}

	#[test]
	fn token_estimate_scales_with_nodes_and_relationships() {
		let mut subgraph = TaskFocusedSubgraph::new();
		subgraph.add_node(node("a.rs", "file", vec![1.0]));
		subgraph.add_node(node("b.rs", "file", vec![1.0]));
		subgraph.add_relationship(relationship("a.rs", "b.rs", RelationType::Imports));
		assert_eq!(subgraph.estimated_token_count(), 2 * 100 + 50);
	}

	#[test]
	fn adding_a_node_twice_keeps_one_copy_but_still_tracks_the_file() {
		let mut subgraph = TaskFocusedSubgraph::new();
		subgraph.add_node(node("a.rs", "file", vec![1.0]));
		subgraph.add_node(node("a.rs", "file", vec![1.0]));
		assert_eq!(subgraph.nodes.len(), 1);
		assert_eq!(subgraph.relevant_files.len(), 1);
	}

	#[test]
	fn duplicate_relationships_are_collapsed_but_a_new_type_is_kept() {
		let mut subgraph = TaskFocusedSubgraph::new();
		subgraph.add_relationship(relationship("a.rs", "b.rs", RelationType::Imports));
		subgraph.add_relationship(relationship("a.rs", "b.rs", RelationType::Imports));
		assert_eq!(subgraph.relationships.len(), 1);
		subgraph.add_relationship(relationship("a.rs", "b.rs", RelationType::Calls));
		assert_eq!(subgraph.relationships.len(), 2);
	}

	#[test]
	fn markdown_sorts_concepts_by_relevance_and_caps_the_list() {
		let mut subgraph = TaskFocusedSubgraph::new();
		for i in 0..15 {
			subgraph.add_key_concept(format!("concept{i}"), i as f32 / 100.0);
		}
		let md = subgraph.to_markdown();
		assert!(md.contains("## Key Concepts"));
		assert!(md.contains("- **concept14** (relevance: 0.14)"));
		// Only the ten highest-scoring concepts survive the token budget.
		assert!(!md.contains("concept4 "));
		assert_eq!(md.matches("(relevance:").count(), 10);
	}

	#[test]
	fn markdown_caps_the_file_list_and_reports_the_remainder() {
		let mut subgraph = TaskFocusedSubgraph::new();
		for i in 0..18 {
			subgraph.add_node(node(&format!("src/f{i:02}.rs"), "file", vec![1.0]));
		}
		let md = subgraph.to_markdown();
		assert!(md.contains("- `src/f00.rs`"));
		assert!(md.contains("- *(and 3 more files)*"));
	}

	#[test]
	fn markdown_groups_components_by_kind_and_notes_overflow() {
		let mut subgraph = TaskFocusedSubgraph::new();
		for i in 0..7 {
			subgraph.add_node(node(&format!("src/fn{i}.rs"), "function", vec![1.0]));
		}
		let md = subgraph.to_markdown();
		assert!(md.contains("## Key Components"));
		assert!(md.contains("### FUNCTIONs"));
		assert!(md.contains("- *(and 2 more functions)*"));
	}

	#[test]
	fn markdown_orders_relationship_types_by_frequency_and_trims_names() {
		let mut subgraph = TaskFocusedSubgraph::new();
		for i in 0..5 {
			subgraph.add_relationship(relationship(
				&format!("src/a{i}.rs"),
				&format!("src/b{i}.rs"),
				RelationType::Imports,
			));
		}
		subgraph.add_relationship(relationship("src/x.rs", "src/y.rs", RelationType::Calls));
		let md = subgraph.to_markdown();
		// The most frequent type is rendered first.
		assert!(
			md.find("### imports relationships") < md.find("### calls relationships"),
			"{md}"
		);
		// Only the leaf name is rendered, not the whole path.
		assert!(md.contains("- `a0.rs` → `b0.rs`"), "{md}");
		assert!(
			md.contains("- *(and 2 more imports relationships)*"),
			"{md}"
		);
	}

	#[tokio::test]
	async fn subgraph_extraction_pulls_in_neighbours_of_relevant_nodes() {
		let full = graph(
			vec![
				node("src/hit.rs", "file", vec![1.0, 0.0]),
				node("src/miss.rs", "file", vec![0.0, 1.0]),
				node("src/neighbour.rs", "file", vec![0.0, 1.0]),
			],
			vec![relationship(
				"src/hit.rs",
				"src/neighbour.rs",
				RelationType::Imports,
			)],
		);
		let optimizer = GraphOptimizer::new(100_000);
		let subgraph = optimizer
			.extract_task_subgraph("task", &[1.0, 0.0], &full)
			.await
			.unwrap();
		let ids: Vec<_> = subgraph.nodes.iter().map(|n| n.id.as_str()).collect();
		assert!(ids.contains(&"src/hit.rs"));
		assert!(ids.contains(&"src/neighbour.rs"));
		assert_eq!(subgraph.relationships.len(), 1);
	}

	#[tokio::test]
	async fn a_zero_token_budget_stops_after_the_first_node() {
		let full = graph(
			(0..10)
				.map(|i| node(&format!("src/n{i}.rs"), "file", vec![1.0, 0.0]))
				.collect(),
			vec![],
		);
		let optimizer = GraphOptimizer::new(0);
		let subgraph = optimizer
			.extract_task_subgraph("task", &[1.0, 0.0], &full)
			.await
			.unwrap();
		assert_eq!(subgraph.nodes.len(), 1);
	}

	#[tokio::test]
	async fn task_focused_view_embeds_the_graph_and_matching_snippets() {
		let full = graph(vec![node("src/hit.rs", "file", vec![1.0, 0.0])], vec![]);
		let blocks = vec![
			code_block("src/hit.rs", "fn hit() {}", vec!["handler"]),
			code_block("src/unrelated.rs", "fn other() {}", vec![]),
		];
		let optimizer = GraphOptimizer::new(100_000);
		let view = optimizer
			.generate_task_focused_view("wire the handler", &[1.0, 0.0], &full, &blocks)
			.await
			.unwrap();
		assert!(view.contains("# Task-Focused Code Overview"));
		assert!(view.contains("**Task:** wire the handler"));
		assert!(view.contains("## Knowledge Graph Summary"));
		// Blocks outside the subgraph's files are never considered.
		assert!(!view.contains("src/unrelated.rs"));
	}

	#[tokio::test]
	async fn long_snippets_are_elided_in_the_middle() {
		let full = graph(vec![node("src/hit.rs", "file", vec![1.0, 0.0])], vec![]);
		let content: String = (1..=40)
			.map(|i| format!("line_{i}();"))
			.collect::<Vec<_>>()
			.join("\n");
		let blocks = vec![code_block("src/hit.rs", &content, vec!["handler"])];
		let view = GraphOptimizer::new(100_000)
			.generate_task_focused_view("task", &[1.0, 0.0], &full, &blocks)
			.await
			.unwrap();
		if view.contains("## Relevant Code Snippets") {
			assert!(view.contains("lines omitted"));
			assert!(view.contains("line_1();"));
			assert!(view.contains("line_40();"));
		}
	}

	#[tokio::test]
	async fn camel_case_terms_shorter_than_the_length_cutoff_still_count() {
		// The mixed-case test used to run against an already-lowercased copy, so it
		// could never fire and short camelCase identifiers were silently dropped.
		let mut n = node("src/a.rs", "file", vec![1.0, 0.0]);
		n.description = "calls getKey then their value".to_string();
		let subgraph = GraphOptimizer::new(100_000)
			.extract_task_subgraph("task", &[1.0, 0.0], &graph(vec![n], vec![]))
			.await
			.unwrap();
		assert!(subgraph.key_concepts.contains_key("getKey"));
		// "their" is a stop word and must not become a concept.
		assert!(!subgraph.key_concepts.contains_key("their"));
	}

	#[tokio::test]
	async fn snake_case_and_long_words_are_treated_as_technical_terms() {
		let mut n = node("src/a.rs", "file", vec![1.0, 0.0]);
		n.description = "retry_policy governs reconnection about".to_string();
		let subgraph = GraphOptimizer::new(100_000)
			.extract_task_subgraph("task", &[1.0, 0.0], &graph(vec![n], vec![]))
			.await
			.unwrap();
		assert!(subgraph.key_concepts.contains_key("retry_policy"));
		assert!(subgraph.key_concepts.contains_key("reconnection"));
		assert!(!subgraph.key_concepts.contains_key("about"));
	}

	#[tokio::test]
	async fn node_name_and_kind_always_become_concepts() {
		let subgraph = GraphOptimizer::new(100_000)
			.extract_task_subgraph(
				"task",
				&[1.0, 0.0],
				&graph(vec![node("src/a.rs", "file", vec![1.0, 0.0])], vec![]),
			)
			.await
			.unwrap();
		assert_eq!(subgraph.key_concepts.get("a.rs"), Some(&1.0));
		assert_eq!(subgraph.key_concepts.get("file"), Some(&0.8));
	}
}
