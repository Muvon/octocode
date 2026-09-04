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
	use crate::indexer::graphrag::relationships::RelationshipDiscovery;
	use crate::indexer::graphrag::types::{CodeNode, RelationType};
	use crate::store::CodeBlock;

	fn node(path: &str, language: &str) -> CodeNode {
		let name = std::path::Path::new(path)
			.file_stem()
			.map(|s| s.to_string_lossy().to_string())
			.unwrap_or_default();
		CodeNode {
			id: path.to_string(),
			name,
			kind: "file".to_string(),
			path: path.to_string(),
			description: String::new(),
			symbols: vec![],
			hash: format!("h-{path}"),
			embedding: vec![],
			imports: vec![],
			exports: vec![],
			functions: vec![],
			size_lines: 10,
			language: language.to_string(),
		}
	}

	fn block(symbols: Vec<&str>) -> CodeBlock {
		CodeBlock {
			path: "src/a.rs".to_string(),
			language: "rust".to_string(),
			content: String::new(),
			symbols: symbols.into_iter().map(String::from).collect(),
			start_line: 12,
			end_line: 30,
			hash: "h".to_string(),
			distance: None,
		}
	}

	#[test]
	fn file_kind_classification_covers_every_branch() {
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("a/src/main.rs"),
			"source_file"
		);
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("a/lib/util.rb"),
			"source_file"
		);
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("pkg/tests/api.go"),
			"test_file"
		);
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("pkg/api_test.go"),
			"test_file"
		);
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("pkg/api.test.ts"),
			"test_file"
		);
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("README.md"),
			"document_file"
		);
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("book.markdown"),
			"document_file"
		);
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("notes.txt"),
			"documentation"
		);
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("index.rst"),
			"documentation"
		);
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("app/config/db.yml"),
			"config_file"
		);
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("app/.configrc"),
			"config_file"
		);
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("root/examples/demo.py"),
			"example_file"
		);
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("Makefile"),
			"file"
		);
	}

	#[test]
	fn source_file_wins_over_test_when_both_patterns_match() {
		// `/src/` is checked first, so a test living under src is still a source file.
		assert_eq!(
			RelationshipDiscovery::determine_file_kind("a/src/api_test.go"),
			"source_file"
		);
	}

	#[test]
	fn simple_description_counts_functions_and_classes() {
		let symbols = vec![
			"function_a".to_string(),
			"method_b".to_string(),
			"class_C".to_string(),
			"struct_D".to_string(),
		];
		assert_eq!(
			RelationshipDiscovery::generate_simple_description("a.rs", "rust", &symbols, 42),
			"a.rs rust file with 2 functions and 2 classes (42 lines)"
		);
	}

	#[test]
	fn simple_description_degrades_by_what_it_found() {
		let functions = vec!["function_a".to_string()];
		assert_eq!(
			RelationshipDiscovery::generate_simple_description("a.rs", "rust", &functions, 3),
			"a.rs rust file with 1 functions (3 lines)"
		);
		let classes = vec!["class_A".to_string()];
		assert_eq!(
			RelationshipDiscovery::generate_simple_description("a.rs", "rust", &classes, 3),
			"a.rs rust file with 1 classes (3 lines)"
		);
		assert_eq!(
			RelationshipDiscovery::generate_simple_description("a.rs", "rust", &[], 3),
			"a.rs rust file (3 lines)"
		);
	}

	#[test]
	fn functions_are_extracted_from_function_prefixed_symbols_only() {
		let functions =
			RelationshipDiscovery::extract_functions_from_block(&block(vec!["function_parse"]))
				.unwrap();
		assert_eq!(functions.len(), 1);
		assert_eq!(functions[0].name, "parse");
		assert_eq!(functions[0].signature, "parse(...)");
		assert_eq!(functions[0].start_line, 12);
		assert_eq!(functions[0].end_line, 30);
	}

	#[test]
	fn non_function_symbols_yield_nothing() {
		let functions =
			RelationshipDiscovery::extract_functions_from_block(&block(vec!["struct_Config"]))
				.unwrap();
		assert!(functions.is_empty());
	}

	#[test]
	fn imports_exports_helper_treats_symbols_as_exports() {
		let symbols = vec![
			"alpha".to_string(),
			String::new(),
			"IMPORT:std::fs".to_string(),
			"beta".to_string(),
		];
		let (imports, exports) =
			RelationshipDiscovery::extract_imports_exports_efficient(&symbols, "rust", "src/a.rs");
		assert!(imports.is_empty());
		assert_eq!(exports, vec!["alpha".to_string(), "beta".to_string()]);
	}

	#[test]
	fn a_resolved_rust_import_becomes_a_direct_import_edge() {
		let mut source = node("src/main.rs", "rust");
		source.imports = vec!["crate::helper".to_string()];
		let target = node("src/helper.rs", "rust");
		let all = vec![source.clone(), target];

		let mut relationships = Vec::new();
		RelationshipDiscovery::discover_import_relationships(&source, &all, &mut relationships);

		let direct: Vec<_> = relationships
			.iter()
			.filter(|r| r.relation_type == RelationType::Imports && r.source == "src/main.rs")
			.collect();
		assert_eq!(direct.len(), 1, "got {relationships:?}");
		assert_eq!(direct[0].target, "src/helper.rs");
		assert_eq!(direct[0].confidence, 0.95);
	}

	#[test]
	fn unresolvable_imports_produce_no_edges() {
		let mut source = node("src/main.rs", "rust");
		source.imports = vec!["serde::Deserialize".to_string()];
		let all = vec![source.clone()];

		let mut relationships = Vec::new();
		RelationshipDiscovery::discover_import_relationships(&source, &all, &mut relationships);
		assert!(relationships.is_empty());
	}

	#[test]
	fn markdown_links_are_directional_references_not_imports() {
		let mut source = node("docs/guide.md", "markdown");
		source.imports = vec!["./api.md".to_string()];
		let mut target = node("docs/api.md", "markdown");
		target.exports = vec!["*".to_string()];
		let all = vec![source.clone(), target];

		let mut relationships = Vec::new();
		RelationshipDiscovery::discover_import_relationships(&source, &all, &mut relationships);

		assert_eq!(relationships.len(), 1, "got {relationships:?}");
		assert_eq!(relationships[0].relation_type, RelationType::References);
		assert!(relationships[0].description.starts_with("References:"));
	}

	#[test]
	fn a_wildcard_export_adds_no_reverse_edge() {
		let mut source = node("src/main.rs", "rust");
		source.imports = vec!["crate::helper".to_string()];
		let mut target = node("src/helper.rs", "rust");
		target.exports = vec!["*".to_string()];
		let all = vec![source.clone(), target];

		let mut relationships = Vec::new();
		RelationshipDiscovery::discover_import_relationships(&source, &all, &mut relationships);

		// `helper.rs` exporting `*` does not make it an importer of `main.rs`.
		// Only the forward edge the import statement actually states is recorded.
		assert_eq!(relationships.len(), 1, "got {relationships:?}");
		assert_eq!(relationships[0].source, "src/main.rs");
		assert_eq!(relationships[0].target, "src/helper.rs");
		assert_eq!(relationships[0].relation_type, RelationType::Imports);
	}

	#[tokio::test]
	async fn parent_and_child_modules_are_linked_hierarchically() {
		let parent = node("src/mod.rs", "rust");
		let child = node("src/inner/leaf.rs", "rust");
		let all = vec![parent.clone(), child];

		let relationships =
			RelationshipDiscovery::discover_relationships_efficiently(&[parent], &all)
				.await
				.unwrap();
		assert!(relationships
			.iter()
			.any(|r| r.relation_type == RelationType::ParentModule));
	}

	#[tokio::test]
	async fn go_files_in_the_same_directory_are_siblings() {
		let a = node("pkg/store/a.go", "go");
		let b = node("pkg/store/b.go", "go");
		let far = node("pkg/other/c.go", "go");
		let all = vec![a.clone(), b, far];

		let relationships = RelationshipDiscovery::discover_relationships_efficiently(&[a], &all)
			.await
			.unwrap();
		let siblings: Vec<_> = relationships
			.iter()
			.filter(|r| r.relation_type == RelationType::SiblingModule)
			.collect();
		assert_eq!(siblings.len(), 1, "got {relationships:?}");
		assert_eq!(siblings[0].target, "pkg/store/b.go");
	}

	#[tokio::test]
	async fn php_files_sharing_a_namespace_are_siblings() {
		let a = node("app/Http/A.php", "php");
		let b = node("app/Http/B.php", "php");
		let far = node("app/Models/C.php", "php");
		let all = vec![a.clone(), b, far];

		let relationships = RelationshipDiscovery::discover_relationships_efficiently(&[a], &all)
			.await
			.unwrap();
		let siblings: Vec<_> = relationships
			.iter()
			.filter(|r| r.relation_type == RelationType::SiblingModule)
			.collect();
		assert_eq!(siblings.len(), 1, "got {relationships:?}");
		assert_eq!(siblings[0].target, "app/Http/B.php");
	}

	#[tokio::test]
	async fn python_package_init_parents_its_siblings() {
		let init = node("pkg/__init__.py", "python");
		let module = node("pkg/api.py", "python");
		let all = vec![init.clone(), module];

		let relationships =
			RelationshipDiscovery::discover_relationships_efficiently(&[init], &all)
				.await
				.unwrap();
		assert!(relationships
			.iter()
			.any(|r| r.target == "pkg/api.py" && r.relation_type == RelationType::ParentModule));
	}

	#[tokio::test]
	async fn javascript_index_parents_its_siblings() {
		let index = node("web/index.js", "javascript");
		let other = node("web/util.js", "javascript");
		let all = vec![index.clone(), other];

		let relationships =
			RelationshipDiscovery::discover_relationships_efficiently(&[index], &all)
				.await
				.unwrap();
		assert!(relationships
			.iter()
			.any(|r| r.target == "web/util.js" && r.relation_type == RelationType::ParentModule));
	}

	#[tokio::test]
	async fn identical_edges_are_deduplicated() {
		let root = node("src/lib.rs", "rust");
		let child = node("src/api.rs", "rust");
		let all = vec![root.clone(), child];

		let relationships =
			RelationshipDiscovery::discover_relationships_efficiently(&[root], &all)
				.await
				.unwrap();
		let mut keys: Vec<_> = relationships
			.iter()
			.map(|r| (&r.source, &r.target, &r.relation_type))
			.collect();
		let before = keys.len();
		keys.sort();
		keys.dedup();
		assert_eq!(
			before,
			keys.len(),
			"duplicate edges survived: {relationships:?}"
		);
	}

	#[tokio::test]
	async fn a_language_without_specific_patterns_yields_no_edges() {
		let a = node("src/a.lua", "lua");
		let b = node("src/b.lua", "lua");
		let all = vec![a.clone(), b];

		let relationships = RelationshipDiscovery::discover_relationships_efficiently(&[a], &all)
			.await
			.unwrap();
		assert!(relationships.is_empty(), "got {relationships:?}");
	}
}
