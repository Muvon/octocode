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
mod go_tests {
	use crate::indexer::code_region_extractor::{extract_meaningful_regions, CodeRegion};
	use crate::indexer::languages;
	use crate::indexer::languages::go::Go;
	use crate::indexer::languages::resolution_utils::FileRegistry;
	use crate::indexer::languages::{CallTarget, Language, TypeRelationKind};
	use tree_sitter::{Node, Parser, Tree};

	fn parse_regions(source: &str) -> Vec<CodeRegion> {
		let lang = languages::get_language("go").expect("Go language not registered");
		let mut parser = Parser::new();
		parser.set_language(&lang.get_ts_language()).unwrap();
		let tree = parser.parse(source, None).unwrap();
		let mut regions = Vec::new();
		extract_meaningful_regions(tree.root_node(), source, lang.as_ref(), &mut regions);
		regions
	}

	fn parse(source: &str) -> Tree {
		let mut parser = Parser::new();
		parser.set_language(&Go {}.get_ts_language()).unwrap();
		parser.parse(source, None).unwrap()
	}

	fn nodes_of_kind<'a>(node: Node<'a>, kind: &str, out: &mut Vec<Node<'a>>) {
		if node.kind() == kind {
			out.push(node);
		}
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			nodes_of_kind(child, kind, out);
		}
	}

	/// The `index`-th node of `kind` in document order.
	fn nth_node<'a>(tree: &'a Tree, kind: &str, index: usize) -> Node<'a> {
		let mut found = Vec::new();
		nodes_of_kind(tree.root_node(), kind, &mut found);
		*found
			.get(index)
			.unwrap_or_else(|| panic!("no {kind} node at index {index}"))
	}

	fn first_node<'a>(tree: &'a Tree, kind: &str) -> Node<'a> {
		nth_node(tree, kind, 0)
	}

	fn registry(files: &[&str]) -> FileRegistry {
		let owned: Vec<String> = files.iter().map(|f| f.to_string()).collect();
		FileRegistry::new(&owned)
	}

	#[test]
	fn test_grouped_const_does_not_collapse_into_one_declaration_blob() {
		let source = r#"package main

const (
	A = 1
	B = 2
)
"#;
		let regions = parse_regions(source);
		let const_decl_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "const_declaration")
			.collect();
		assert_eq!(
			const_decl_regions.len(),
			0,
			"grouped const block should never surface as a single const_declaration region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		let const_spec_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "const_spec")
			.collect();
		assert!(
			!const_spec_regions.is_empty(),
			"grouped const block should surface as const_spec region(s), got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}

	#[test]
	fn test_large_grouped_const_splits_into_multiple_bounded_regions() {
		let mut source = String::from("package main\n\nconst (\n");
		for i in 0..40 {
			source.push_str(&format!("\tC{i} = {i}\n"));
		}
		source.push_str(")\n");

		let regions = parse_regions(&source);
		let const_spec_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "const_spec")
			.collect();
		assert!(
			const_spec_regions.len() > 1,
			"a 40-spec grouped const block must not become a single oversized region, got {} const_spec region(s)",
			const_spec_regions.len()
		);
	}

	#[test]
	fn test_single_const_stays_one_region_with_keyword() {
		let source = r#"package main

const X = 1
"#;
		let regions = parse_regions(source);
		let const_decl_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "const_declaration")
			.collect();
		assert_eq!(
			const_decl_regions.len(),
			1,
			"single const declaration should stay as one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
		assert!(
			const_decl_regions[0].content.contains("const"),
			"single const region should still include the 'const' keyword: {:?}",
			const_decl_regions[0].content
		);
	}

	#[test]
	fn test_grouped_var_does_not_collapse_into_one_declaration_blob() {
		let source = r#"package main

var (
	A = 1
	B = 2
)
"#;
		let regions = parse_regions(source);
		let var_decl_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "var_declaration")
			.collect();
		assert_eq!(
			var_decl_regions.len(),
			0,
			"grouped var block should never surface as a single var_declaration region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		let var_spec_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "var_spec")
			.collect();
		assert!(
			!var_spec_regions.is_empty(),
			"grouped var block should surface as var_spec region(s), got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}

	#[test]
	fn test_single_var_stays_one_region_with_keyword() {
		let source = r#"package main

var X = 1
"#;
		let regions = parse_regions(source);
		let var_decl_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "var_declaration")
			.collect();
		assert_eq!(
			var_decl_regions.len(),
			1,
			"single var declaration should stay as one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
		assert!(
			var_decl_regions[0].content.contains("var"),
			"single var region should still include the 'var' keyword: {:?}",
			var_decl_regions[0].content
		);
	}

	#[test]
	fn test_grouped_type_splits_per_spec() {
		// Non-trivial content so the smart single-line merge pass doesn't recombine them.
		let source = r#"package main

type (
	A struct {
		f int
		g string
		h bool
		k float64
	}
	B interface {
		M()
		N()
		O()
		P()
	}
)
"#;
		let regions = parse_regions(source);
		let type_decl_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "type_declaration")
			.collect();
		assert_eq!(
			type_decl_regions.len(),
			0,
			"grouped type block should never surface as a single type_declaration region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		let type_spec_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "type_spec")
			.collect();
		assert_eq!(
			type_spec_regions.len(),
			2,
			"grouped type block should produce one region per spec, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}

	#[test]
	fn test_single_type_stays_one_region_with_keyword() {
		let source = r#"package main

type Foo struct {
	F int
}
"#;
		let regions = parse_regions(source);
		let type_decl_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "type_declaration")
			.collect();
		assert_eq!(
			type_decl_regions.len(),
			1,
			"single type declaration should stay as one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
		assert!(
			type_decl_regions[0].content.contains("type"),
			"single type region should still include the 'type' keyword: {:?}",
			type_decl_regions[0].content
		);
	}

	#[test]
	fn the_language_is_named_go_and_owns_the_go_extension() {
		let go = Go {};
		assert_eq!(go.name(), "go");
		assert_eq!(go.get_file_extensions(), vec!["go"]);
	}

	#[test]
	fn chunking_kinds_and_symbol_kinds_deliberately_differ() {
		let go = Go {};
		assert_eq!(
			go.get_meaningful_kinds(),
			vec![
				"function_declaration",
				"method_declaration",
				"type_declaration",
				"const_declaration",
				"var_declaration",
				"import_declaration",
			]
		);
		// Symbols use `type_spec` (one per grouped spec) and drop const/var/import
		// declarations, which introduce no single named symbol.
		assert_eq!(
			go.get_symbol_kinds(),
			vec!["function_declaration", "method_declaration", "type_spec"]
		);
	}

	#[test]
	fn every_node_type_description_arm_is_reachable() {
		let go = Go {};
		assert_eq!(
			go.get_node_type_description("function_declaration"),
			"function declarations"
		);
		assert_eq!(
			go.get_node_type_description("method_declaration"),
			"function declarations"
		);
		assert_eq!(
			go.get_node_type_description("type_declaration"),
			"type declarations"
		);
		assert_eq!(
			go.get_node_type_description("struct_type"),
			"struct definitions"
		);
		assert_eq!(
			go.get_node_type_description("interface_type"),
			"interface definitions"
		);
		assert_eq!(
			go.get_node_type_description("var_declaration"),
			"variable declarations"
		);
		assert_eq!(
			go.get_node_type_description("const_declaration"),
			"variable declarations"
		);
		assert_eq!(
			go.get_node_type_description("short_var_declaration"),
			"variable declarations"
		);
		assert_eq!(
			go.get_node_type_description("import_declaration"),
			"import statements"
		);
		assert_eq!(go.get_node_type_description("comment"), "declarations");
	}

	#[test]
	fn semantic_groups_join_functions_with_methods_but_not_with_types() {
		let go = Go {};
		assert!(go.are_node_types_equivalent("function_declaration", "method_declaration"));
		assert!(go.are_node_types_equivalent("type_declaration", "interface_type"));
		assert!(go.are_node_types_equivalent("struct_type", "type_declaration"));
		assert!(go.are_node_types_equivalent("var_declaration", "short_var_declaration"));
		assert!(go.are_node_types_equivalent("const_declaration", "var_declaration"));
		assert!(go.are_node_types_equivalent("import_declaration", "import_declaration"));

		assert!(!go.are_node_types_equivalent("function_declaration", "type_declaration"));
		assert!(!go.are_node_types_equivalent("import_declaration", "var_declaration"));
		assert!(!go.are_node_types_equivalent("comment", "block"));
	}

	#[test]
	fn a_function_symbol_list_holds_the_name_and_its_body_declarations() {
		// A block's statements sit inside a `statement_list`, so the collector
		// only reaches them once it descends through that wrapper.
		let source = r#"package main

func Run() {
	total := 1
	var count int
	const limit = 2
	if total > 0 {
		inner := 3
		_ = inner
	}
	_ = count
	_ = limit
}
"#;
		let tree = parse(source);
		let node = first_node(&tree, "function_declaration");
		let symbols = Go {}.extract_symbols(node, source);
		for expected in ["Run", "total", "count", "limit", "inner"] {
			assert!(
				symbols.contains(&expected.to_string()),
				"{expected} missing from {symbols:?}"
			);
		}
	}

	#[test]
	fn a_method_symbol_list_holds_the_method_name_and_its_body_declarations() {
		let source = r#"package main

func (s *Server) Handle() {
	req := 1
	_ = req
}
"#;
		let tree = parse(source);
		let node = first_node(&tree, "method_declaration");
		assert_eq!(
			Go {}.extract_symbols(node, source),
			vec!["Handle".to_string(), "req".to_string()]
		);
	}

	#[test]
	fn a_const_spec_yields_every_comma_separated_name() {
		let source = "package main\n\nconst (\n\tA, B = 1, 2\n\tC = 3\n)\n";
		let tree = parse(source);
		let go = Go {};
		assert_eq!(
			go.extract_symbols(nth_node(&tree, "const_spec", 0), source),
			vec!["A".to_string(), "B".to_string()]
		);
		assert_eq!(
			go.extract_symbols(nth_node(&tree, "const_spec", 1), source),
			vec!["C".to_string()]
		);
	}

	#[test]
	fn a_var_spec_yields_every_comma_separated_name() {
		let source = "package main\n\nvar (\n\tX, Y int\n)\n";
		let tree = parse(source);
		let node = first_node(&tree, "var_spec");
		assert_eq!(
			Go {}.extract_symbols(node, source),
			vec!["X".to_string(), "Y".to_string()]
		);
	}

	#[test]
	fn type_declarations_and_type_specs_yield_the_type_name() {
		// A Go type name is a `type_identifier`, not an `identifier`.
		let source = r#"package main

type Server struct {
	Name string
}
"#;
		let tree = parse(source);
		let go = Go {};
		assert_eq!(
			go.extract_symbols(first_node(&tree, "type_declaration"), source),
			vec!["Server".to_string()]
		);
		assert_eq!(
			go.extract_symbols(first_node(&tree, "type_spec"), source),
			vec!["Server".to_string()]
		);
	}

	#[test]
	fn struct_and_interface_types_yield_their_member_names() {
		// Struct fields nest inside a `field_declaration_list`, and interface
		// members are `method_elem` nodes.
		let source = r#"package main

type Server struct {
	Name string
}

type Handler interface {
	Serve() error
}
"#;
		let tree = parse(source);
		let go = Go {};
		assert_eq!(
			go.extract_symbols(first_node(&tree, "struct_type"), source),
			vec!["Name".to_string()]
		);
		assert_eq!(
			go.extract_symbols(first_node(&tree, "interface_type"), source),
			vec!["Serve".to_string()]
		);
	}

	#[test]
	fn an_unhandled_node_kind_falls_back_to_identifier_extraction() {
		let source = "package main\n\nfunc (s *Server) Handle(ctx int) {}\n";
		let tree = parse(source);
		let method = first_node(&tree, "method_declaration");
		let receiver = method.child_by_field_name("receiver").unwrap();
		// A parameter_list is not a handled kind, so the `_` arm collects the
		// identifiers below it — the receiver binding, not its type.
		assert_eq!(
			Go {}.extract_symbols(receiver, source),
			vec!["s".to_string()]
		);
	}

	#[test]
	fn identifier_extraction_keeps_identifiers_and_field_identifiers_in_tree_order() {
		let source = "package main\n\nfunc (s *Server) Handle(ctx int) {}\n";
		let tree = parse(source);
		let method = first_node(&tree, "method_declaration");
		let mut symbols = Vec::new();
		Go {}.extract_identifiers(method, source, &mut symbols);
		// `Server` and `int` are `type_identifier` nodes and are excluded.
		assert_eq!(
			symbols,
			vec!["s".to_string(), "Handle".to_string(), "ctx".to_string()]
		);
	}

	#[test]
	fn a_method_receiver_becomes_the_symbol_owner() {
		let go = Go {};
		for source in [
			"package main\n\nfunc (s *Server) Handle() {}\n",
			"package main\n\nfunc (s Server) Handle() {}\n",
			"package main\n\nfunc (s *pkg.Server) Handle() {}\n",
		] {
			let tree = parse(source);
			let method = first_node(&tree, "method_declaration");
			assert_eq!(
				go.extract_symbol_owner(method, source),
				Some("Server".to_string()),
				"receiver in {source:?} should resolve to Server"
			);
		}
	}

	#[test]
	fn a_generic_receiver_reports_the_bare_type_as_the_owner() {
		// `simple_type_name` cuts at the opening bracket; trimming only the ends
		// would leave `Store[T`.
		let source = "package main\n\nfunc (s *Store[T]) Handle() {}\n";
		let tree = parse(source);
		let method = first_node(&tree, "method_declaration");
		assert_eq!(
			Go {}.extract_symbol_owner(method, source),
			Some("Store".to_string())
		);
	}

	#[test]
	fn a_package_level_function_has_no_symbol_owner() {
		let source = "package main\n\nfunc Run() {}\n";
		let tree = parse(source);
		let function = first_node(&tree, "function_declaration");
		assert_eq!(Go {}.extract_symbol_owner(function, source), None);
	}

	#[test]
	fn a_single_import_yields_its_quoted_path_with_or_without_an_alias() {
		let go = Go {};
		for (source, expected) in [
			("package main\n\nimport \"fmt\"\n", "fmt"),
			(
				"package main\n\nimport alias \"example.com/pkg/thing\"\n",
				"example.com/pkg/thing",
			),
		] {
			let tree = parse(source);
			let node = first_node(&tree, "import_declaration");
			let (imports, exports) = go.extract_imports_exports(node, source);
			assert_eq!(imports, vec![expected.to_string()]);
			assert!(exports.is_empty());
		}
	}

	#[test]
	fn a_grouped_import_block_yields_every_path_and_skips_comments() {
		let source = r#"package main

import (
	"os"
	str "strings"
	// a standalone comment line
	_ "github.com/lib/pq" // driver comment
)
"#;
		let tree = parse(source);
		let node = first_node(&tree, "import_declaration");
		let (imports, _) = Go {}.extract_imports_exports(node, source);
		assert_eq!(
			imports,
			vec![
				"os".to_string(),
				"strings".to_string(),
				"github.com/lib/pq".to_string(),
			]
		);
	}

	#[test]
	fn only_uppercase_function_and_method_names_are_exported() {
		let go = Go {};
		let source = "package main\n\nfunc Exported() {}\n\nfunc unexported() {}\n";
		let tree = parse(source);
		let (_, exported) =
			go.extract_imports_exports(nth_node(&tree, "function_declaration", 0), source);
		assert_eq!(exported, vec!["Exported".to_string()]);
		let (_, unexported) =
			go.extract_imports_exports(nth_node(&tree, "function_declaration", 1), source);
		assert!(unexported.is_empty());

		let method_source = "package main\n\nfunc (s *Server) Handle() {}\n";
		let method_tree = parse(method_source);
		let (_, method_exports) = go.extract_imports_exports(
			first_node(&method_tree, "method_declaration"),
			method_source,
		);
		assert_eq!(method_exports, vec!["Handle".to_string()]);
	}

	#[test]
	fn exported_type_var_and_const_names_are_reported() {
		// A type/var/const name lives one level down inside its spec node, so
		// scanning only direct children found nothing at all.
		let go = Go {};
		for (source, kind, expected) in [
			(
				"package main\n\ntype Server struct{}\n",
				"type_declaration",
				"Server",
			),
			(
				"package main\n\nvar Public = 1\n",
				"var_declaration",
				"Public",
			),
			(
				"package main\n\nconst Public = 1\n",
				"const_declaration",
				"Public",
			),
		] {
			let tree = parse(source);
			let (_, exports) = go.extract_imports_exports(first_node(&tree, kind), source);
			assert_eq!(exports, vec![expected], "exports for {kind}");
		}

		// Lower-case names stay unexported.
		let tree = parse("package main\n\ntype server struct{}\n");
		let (_, exports) = go.extract_imports_exports(
			first_node(&tree, "type_declaration"),
			"package main\n\ntype server struct{}\n",
		);
		assert!(exports.is_empty(), "{exports:?}");
	}

	#[test]
	fn calls_keep_their_receiver_as_the_qualifier() {
		let source =
			"package main\n\nfunc main() {\n\thelper()\n\tfmt.Println(1)\n\tpkg.Sub.Deep()\n}\n";
		let tree = parse(source);
		let go = Go {};
		assert_eq!(
			go.extract_function_calls(nth_node(&tree, "call_expression", 0), source),
			vec![CallTarget {
				name: "helper".to_string(),
				qualifier: None,
			}]
		);
		assert_eq!(
			go.extract_function_calls(nth_node(&tree, "call_expression", 1), source),
			vec![CallTarget {
				name: "Println".to_string(),
				qualifier: Some("fmt".to_string()),
			}]
		);
		assert_eq!(
			go.extract_function_calls(nth_node(&tree, "call_expression", 2), source),
			vec![CallTarget {
				name: "Deep".to_string(),
				qualifier: Some("pkg::Sub".to_string()),
			}]
		);
	}

	#[test]
	fn a_node_that_is_not_a_call_expression_yields_no_call_targets() {
		let source = "package main\n\nfunc main() {\n\tx := 1\n\t_ = x\n}\n";
		let tree = parse(source);
		let node = first_node(&tree, "short_var_declaration");
		assert!(Go {}.extract_function_calls(node, source).is_empty());
	}

	#[test]
	fn an_embedded_struct_field_becomes_an_extends_relation() {
		let source =
			"package main\n\ntype Server struct {\n\tLogger\n\tmu sync.Mutex\n\tName string\n}\n";
		let tree = parse(source);
		let go = Go {};
		assert_eq!(
			go.extract_type_relations(nth_node(&tree, "field_declaration", 0), source),
			vec![(TypeRelationKind::Extends, "Logger".to_string())]
		);
		// Named fields are ordinary composition, not embedding.
		assert!(go
			.extract_type_relations(nth_node(&tree, "field_declaration", 1), source)
			.is_empty());
		assert!(go
			.extract_type_relations(nth_node(&tree, "field_declaration", 2), source)
			.is_empty());
		// Go has no syntactic `implements`, so the type node itself has no relations.
		assert!(go
			.extract_type_relations(first_node(&tree, "type_spec"), source)
			.is_empty());
	}

	#[test]
	fn only_multi_spec_declarations_are_expanded_into_sub_regions() {
		let go = Go {};
		let single = "package main\n\nconst X = 1\n";
		let single_tree = parse(single);
		assert!(go
			.expand_meaningful_node(first_node(&single_tree, "const_declaration"), single)
			.is_none());

		let grouped = "package main\n\nvar (\n\tA = 1\n\tB = 2\n)\n";
		let grouped_tree = parse(grouped);
		let expanded = go
			.expand_meaningful_node(first_node(&grouped_tree, "var_declaration"), grouped)
			.expect("grouped var should expand");
		assert_eq!(expanded.len(), 2);
		assert_eq!(expanded[0].symbols, vec!["A".to_string()]);
		assert_eq!(expanded[1].symbols, vec!["B".to_string()]);
		assert_eq!(expanded[0].node_kind, "var_spec");

		// Kinds without specs are never expanded.
		let function = "package main\n\nfunc Run() {}\n";
		let function_tree = parse(function);
		assert!(go
			.expand_meaningful_node(first_node(&function_tree, "function_declaration"), function)
			.is_none());
	}

	#[test]
	fn a_relative_import_resolves_to_a_file_in_the_target_directory() {
		let files = registry(&[
			"pkg/main.go",
			"pkg/util/helper.go",
			"internal/models/user.go",
			"README.md",
		]);
		let go = Go {};
		assert_eq!(
			go.resolve_import("./util", "pkg/main.go", &files),
			Some("pkg/util/helper.go".to_string())
		);
		assert_eq!(
			go.resolve_import("../internal/models", "pkg/main.go", &files),
			Some("internal/models/user.go".to_string())
		);
	}

	#[test]
	fn a_package_import_matches_a_directory_path_suffix() {
		let files = registry(&["pkg/main.go", "vendor/github.com/lib/pq/pq.go"]);
		assert_eq!(
			Go {}.resolve_import("github.com/lib/pq", "pkg/main.go", &files),
			Some("vendor/github.com/lib/pq/pq.go".to_string())
		);
	}

	#[test]
	fn a_package_import_also_matches_on_the_last_path_segment_alone() {
		// The resolver falls back to matching only the final segment, so an
		// unrelated module path resolves to a local directory of the same name.
		let files = registry(&["pkg/main.go", "internal/models/user.go"]);
		assert_eq!(
			Go {}.resolve_import("example.com/other/models", "pkg/main.go", &files),
			Some("internal/models/user.go".to_string())
		);
	}

	#[test]
	fn an_unknown_third_party_import_resolves_to_nothing() {
		let files = registry(&["pkg/main.go", "internal/models/user.go"]);
		assert_eq!(
			Go {}.resolve_import("example.com/nope/missing", "pkg/main.go", &files),
			None
		);
	}

	#[test]
	fn a_preceding_doc_comment_is_folded_into_the_function_region() {
		let source = r#"package main

// Greet says hello to the world.
func Greet(name string) string {
	prefix := "hi"
	message := prefix + name
	return message
}
"#;
		let regions = parse_regions(source);
		let function = regions
			.iter()
			.find(|r| r.node_kind == "function_declaration")
			.expect("expected a function region");
		assert!(
			function
				.content
				.starts_with("// Greet says hello to the world.\nfunc Greet"),
			"doc comment should lead the region content: {:?}",
			function.content
		);
		assert_eq!(
			function.start_line, 2,
			"region should start at the comment line, not the func line"
		);
		assert_eq!(
			function.symbols,
			vec![
				"Greet".to_string(),
				"message".to_string(),
				"prefix".to_string()
			]
		);
	}

	#[test]
	fn consecutive_single_line_functions_merge_into_one_described_block() {
		let source = "package main\n\nfunc a() {}\nfunc b() {}\n";
		let regions = parse_regions(source);
		assert_eq!(regions.len(), 1, "expected a single merged region");
		assert!(
			regions[0]
				.content
				.starts_with("// Merged function declarations (2 declarations)\n"),
			"merged block should carry the Go description: {:?}",
			regions[0].content
		);
		assert_eq!(regions[0].symbols, vec!["a".to_string(), "b".to_string()]);
	}
}
