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
	use crate::indexer::graphrag::types::{CodeGraph, CodeNode};
	use tempfile::TempDir;

	fn node(id: &str, kind: &str, name: &str, symbols: &[&str]) -> CodeNode {
		CodeNode {
			id: id.to_string(),
			name: name.to_string(),
			kind: kind.to_string(),
			path: id.split("::").next().unwrap_or(id).to_string(),
			description: format!("does {name}"),
			symbols: symbols.iter().map(|s| s.to_string()).collect(),
			hash: String::new(),
			embedding: Vec::new(),
			imports: Vec::new(),
			exports: Vec::new(),
			functions: Vec::new(),
			size_lines: 0,
			language: "rust".to_string(),
		}
	}

	fn graph_of(ids: &[&str]) -> CodeGraph {
		let mut graph = CodeGraph::default();
		for id in ids {
			let name = id.rsplit("::").next().unwrap_or(id);
			graph
				.nodes
				.insert(id.to_string(), node(id, "function", name, &[]));
		}
		graph
	}

	#[test]
	fn cosine_similarity_scores_direction() {
		let close = |actual: f32, expected: f32| {
			assert!(
				(actual - expected).abs() < 1e-6,
				"expected {expected}, got {actual}"
			);
		};

		close(cosine_similarity(&[1.0, 0.0, 0.0], &[1.0, 0.0, 0.0]), 1.0);
		close(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
		close(cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]), -1.0);
		// Magnitude is normalised away.
		close(cosine_similarity(&[3.0, 4.0], &[30.0, 40.0]), 1.0);
	}

	#[test]
	fn cosine_similarity_is_zero_when_it_cannot_be_defined() {
		// Different widths never compare.
		assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
		// A zero vector has no direction.
		assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
		assert_eq!(cosine_similarity(&[1.0, 1.0], &[0.0, 0.0]), 0.0);
		assert_eq!(cosine_similarity(&[], &[]), 0.0);
	}

	#[test]
	fn project_root_detection_stops_at_the_nearest_marker() {
		let dir = TempDir::new().unwrap();
		let root = dir.path();
		let nested = root.join("crates/app/src");
		std::fs::create_dir_all(&nested).unwrap();
		std::fs::write(root.join("Cargo.toml"), "[package]").unwrap();

		assert_eq!(detect_project_root_from(&nested).unwrap(), root);

		// A marker closer to the start directory wins.
		let inner = root.join("crates/app");
		std::fs::create_dir_all(inner.join(".git")).unwrap();
		assert_eq!(detect_project_root_from(&nested).unwrap(), inner);
	}

	#[test]
	fn a_path_under_the_project_root_is_reported_relative_to_it() {
		let dir = TempDir::new().unwrap();
		let root = dir.path().canonicalize().unwrap();
		std::fs::create_dir_all(root.join("src")).unwrap();
		let file = root.join("src/main.rs");
		std::fs::write(&file, "fn main() {}").unwrap();

		assert_eq!(
			to_relative_path(&file.to_string_lossy(), &root).unwrap(),
			"src/main.rs"
		);
		// A path that does not exist yet is resolved against the root instead of
		// the filesystem.
		assert_eq!(
			to_relative_path("src/not_created.rs", &root).unwrap(),
			"src/not_created.rs"
		);
	}

	#[test]
	fn a_path_outside_the_project_root_is_rejected() {
		let project = TempDir::new().unwrap();
		let elsewhere = TempDir::new().unwrap();
		let stranger = elsewhere.path().join("other.rs");
		std::fs::write(&stranger, "").unwrap();

		let err = to_relative_path(&stranger.to_string_lossy(), project.path()).unwrap_err();
		assert!(
			err.to_string().contains("is not within project root"),
			"{err}"
		);
	}

	#[test]
	fn node_ids_normalize_separators_and_edge_slashes() {
		assert_eq!(normalize_node_id("./src/a.rs"), "src/a.rs");
		assert_eq!(normalize_node_id("././src/a.rs"), "src/a.rs");
		assert_eq!(normalize_node_id("src\\utils\\a.rs"), "src/utils/a.rs");
		assert_eq!(normalize_node_id("src/utils/"), "src/utils");
		assert_eq!(normalize_node_id("src/utils///"), "src/utils");
		assert_eq!(normalize_node_id("./src\\utils/"), "src/utils");
		// Already-normal ids pass through untouched.
		assert_eq!(normalize_node_id("src/a.rs"), "src/a.rs");
	}

	#[test]
	fn symbols_match_after_stripping_import_and_export_prefixes() {
		assert!(symbols_match("handler", "handler"));
		assert!(symbols_match("import_handler", "export_handler"));
		assert!(symbols_match("use_handler", "pub_handler"));
		assert!(symbols_match("from_handler", "public_handler"));
		// Only the listed prefixes are stripped, so unrelated names stay apart.
		assert!(!symbols_match("handler", "handlers"));
		assert!(!symbols_match("get_handler", "handler"));
	}

	#[test]
	fn nodes_render_as_text_grouped_by_file_hiding_underscored_symbols() {
		let mut node = node("src/a.rs::alpha", "function", "alpha", &[]);
		// Duplicates collapse, output is sorted, and `_`-bearing internal symbol
		// names are not shown to users.
		node.symbols = vec![
			"gamma_x".to_string(),
			"beta".to_string(),
			"beta".to_string(),
		];

		assert_eq!(
			graphrag_nodes_to_text(&[node]),
			"GRAPHRAG NODES (1 found)\n\n\
			 FILE: src/a.rs\n\
			 \x20 function alpha\n\
			 \x20 ID: src/a.rs::alpha\n\
			 \x20 Description: does alpha\n\
			 \x20 Symbols:\n\
			 \x20   - beta\n\n\n"
		);
	}

	#[test]
	fn a_node_without_symbols_renders_no_symbol_section() {
		let text = graphrag_nodes_to_text(&[node("src/a.rs", "file", "a.rs", &[])]);
		assert!(!text.contains("Symbols"), "{text}");
		assert!(text.contains("FILE: src/a.rs\n"), "{text}");
	}

	#[test]
	fn nodes_render_as_markdown_with_a_section_per_file() {
		let node = node("src/a.rs::alpha", "function", "alpha", &["beta", "gamma_x"]);
		assert_eq!(
			graphrag_nodes_to_markdown(&[node]),
			"# Found 1 GraphRAG nodes\n\n\
			 ## File: src/a.rs\n\n\
			 ### function `alpha`\n\
			 **ID:** src/a.rs::alpha  \n\
			 **Description:** does alpha  \n\
			 **Symbols:**  \n\
			 - `beta`  \n\n\
			 ---\n\n"
		);
	}

	#[test]
	fn an_empty_node_list_reports_no_matches_in_both_renderers() {
		assert_eq!(graphrag_nodes_to_text(&[]), "No matching nodes found.");
		assert_eq!(graphrag_nodes_to_markdown(&[]), "No matching nodes found.");
	}

	#[test]
	fn node_lookup_accepts_unnormalized_and_differently_cased_ids() {
		let graph = graph_of(&["src/a.rs"]);
		assert_eq!(find_node_id(&graph, "src/a.rs"), Some("src/a.rs"));
		assert_eq!(find_node_id(&graph, "./src/a.rs"), Some("src/a.rs"));
		assert_eq!(find_node_id(&graph, "src\\a.rs"), Some("src/a.rs"));
		assert_eq!(find_node_id(&graph, "SRC/A.RS"), Some("src/a.rs"));
		assert_eq!(find_node_id(&graph, "src/missing.rs"), None);
	}

	#[test]
	fn a_file_suffix_resolves_to_the_shortest_matching_path() {
		let graph = graph_of(&["src/config.rs", "vendor/deep/nested/config.rs"]);
		assert_eq!(find_node_id(&graph, "config.rs"), Some("src/config.rs"));
	}

	#[test]
	fn an_owner_qualified_symbol_matching_several_files_does_not_resolve() {
		let graph = graph_of(&["src/a.rs::Service::run", "src/b.rs::Service::run"]);
		assert_eq!(find_node_id(&graph, "Service::run"), None);
	}

	#[test]
	fn rendering_nodes_as_json_serializes_the_whole_list() {
		// The renderer prints to stdout; assert the serialization it prints.
		let nodes = vec![node("src/a.rs", "file", "a.rs", &["beta"])];
		let json = serde_json::to_string_pretty(&nodes).unwrap();
		assert!(json.contains("\"id\": \"src/a.rs\""), "{json}");
		assert!(render_graphrag_nodes_json(&nodes).is_ok());
	}

	/// Reproduces the macOS CI failure on any unix: `/var` is a symlink to
	/// `/private/var` there, so a temp-dir project root canonicalizes into a
	/// different namespace than a path that cannot be canonicalized because it
	/// no longer exists on disk. A symlinked root reproduces the same shape.
	#[cfg(unix)]
	#[test]
	fn a_path_under_a_symlinked_root_is_relativized_even_when_it_is_gone_from_disk() {
		let outer = TempDir::new().expect("tempdir");
		let real = outer.path().join("real");
		std::fs::create_dir_all(real.join("src")).expect("mkdir");
		let link = outer.path().join("link");
		std::os::unix::fs::symlink(&real, &link).expect("symlink");

		// Present on disk: both sides canonicalize into the real directory.
		let existing = link.join("src/there.rs");
		std::fs::write(&existing, "fn there() {}\n").expect("write");
		assert_eq!(
			to_relative_path(&existing.to_string_lossy(), &link).unwrap(),
			"src/there.rs"
		);

		// Absent from disk — a file deleted since it was indexed. `canonicalize`
		// leaves it under the symlink while the root resolves through it, which
		// is exactly the state that reported the path as outside the project.
		let deleted = link.join("src/gone.rs");
		assert!(!deleted.exists());
		assert_eq!(
			to_relative_path(&deleted.to_string_lossy(), &link).unwrap(),
			"src/gone.rs"
		);
	}
}
