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
	use crate::indexer::code_region_extractor::{extract_meaningful_regions, CodeRegion};
	use crate::indexer::languages::python::Python;
	use crate::indexer::languages::resolution_utils::FileRegistry;
	use crate::indexer::languages::{Language, TypeRelationKind};
	use tree_sitter::{Node, Parser, Tree};

	fn parse(source: &str) -> Tree {
		let mut parser = Parser::new();
		parser
			.set_language(&Python {}.get_ts_language())
			.expect("python grammar");
		parser.parse(source, None).expect("parse")
	}

	fn regions(source: &str) -> Vec<CodeRegion> {
		let tree = parse(source);
		let lang = Python {};
		let mut out = Vec::new();
		extract_meaningful_regions(tree.root_node(), source, &lang, &mut out);
		out
	}

	/// Depth-first walk collecting every node of `kind`.
	fn nodes_of_kind<'a>(node: Node<'a>, kind: &str, out: &mut Vec<Node<'a>>) {
		if node.kind() == kind {
			out.push(node);
		}
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			nodes_of_kind(child, kind, out);
		}
	}

	fn first_node<'a>(tree: &'a Tree, kind: &str) -> Node<'a> {
		let mut found = Vec::new();
		nodes_of_kind(tree.root_node(), kind, &mut found);
		*found
			.first()
			.unwrap_or_else(|| panic!("no {kind} node in source"))
	}

	fn registry(files: &[&str]) -> FileRegistry {
		let owned: Vec<String> = files.iter().map(|f| f.to_string()).collect();
		FileRegistry::new(&owned)
	}

	#[test]
	fn language_metadata_is_wired_up() {
		let lang = Python {};
		assert_eq!(lang.name(), "python");
		assert_eq!(lang.get_file_extensions(), vec!["py"]);
		assert!(lang.get_meaningful_kinds().contains(&"function_definition"));
		// Classes are deliberately excluded from chunking so a big class does not
		// collapse into one region; methods are chunked individually instead.
		assert!(!lang.get_meaningful_kinds().contains(&"class_definition"));
		assert!(lang.get_symbol_kinds().contains(&"class_definition"));
	}

	#[test]
	fn node_type_descriptions_cover_the_known_kinds() {
		let lang = Python {};
		assert_eq!(
			lang.get_node_type_description("function_definition"),
			"function declarations"
		);
		assert_eq!(
			lang.get_node_type_description("class_definition"),
			"class declarations"
		);
		assert_eq!(
			lang.get_node_type_description("import_from_statement"),
			"import statements"
		);
		assert_eq!(lang.get_node_type_description("whatever"), "declarations");
	}

	#[test]
	fn import_statement_kinds_are_semantically_equivalent() {
		let lang = Python {};
		assert!(lang.are_node_types_equivalent("import_statement", "import_from_statement"));
		assert!(lang.are_node_types_equivalent("function_definition", "function_definition"));
		assert!(!lang.are_node_types_equivalent("function_definition", "class_definition"));
	}

	#[test]
	fn methods_carry_their_owning_class_name() {
		let source = "class Service:\n    def handle(self, req):\n        pass\n";
		let tree = parse(source);
		let symbols = Python {}.extract_symbols(first_node(&tree, "function_definition"), source);
		assert!(symbols.contains(&"handle".to_string()));
		assert!(
			symbols.contains(&"Service".to_string()),
			"method should carry its class so Service.handle queries hit: {symbols:?}"
		);
	}

	#[test]
	fn function_symbols_include_local_assignments_but_skip_private_names() {
		let source = "def run():\n    total = 1\n    _hidden = 2\n    total += 3\n";
		let tree = parse(source);
		let symbols = Python {}.extract_symbols(first_node(&tree, "function_definition"), source);
		assert!(symbols.contains(&"run".to_string()));
		assert!(symbols.contains(&"total".to_string()));
		assert!(!symbols.contains(&"_hidden".to_string()));
	}

	#[test]
	fn assignments_inside_nested_clauses_are_still_collected() {
		let source = "\
def run(flag):
    if flag:
        first = 1
    elif flag is None:
        second = 2
    else:
        third = 3
    try:
        fourth = 4
    except ValueError:
        fifth = 5
    finally:
        sixth = 6
    for i in []:
        seventh = 7
    while flag:
        eighth = 8
    with open('f') as fh:
        ninth = 9
";
		let tree = parse(source);
		let symbols = Python {}.extract_symbols(first_node(&tree, "function_definition"), source);
		for name in [
			"first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
		] {
			assert!(
				symbols.contains(&name.to_string()),
				"{name} missing from {symbols:?}"
			);
		}
	}

	#[test]
	fn symbols_are_deduplicated() {
		let source = "def run():\n    total = 1\n    total = 2\n";
		let tree = parse(source);
		let symbols = Python {}.extract_symbols(first_node(&tree, "function_definition"), source);
		assert_eq!(symbols.iter().filter(|s| *s == "total").count(), 1);
	}

	#[test]
	fn non_function_nodes_fall_back_to_public_identifier_extraction() {
		let source = "value = helper(_private)\n";
		let tree = parse(source);
		let symbols = Python {}.extract_symbols(first_node(&tree, "expression_statement"), source);
		assert!(symbols.contains(&"helper".to_string()));
		assert!(!symbols.iter().any(|s| s.starts_with('_')));
	}

	#[test]
	fn plain_imports_keep_the_full_dotted_path() {
		let source = "import os.path as p, json\n";
		let tree = parse(source);
		let (imports, exports) =
			Python {}.extract_imports_exports(first_node(&tree, "import_statement"), source);
		assert_eq!(imports, vec!["os.path".to_string(), "json".to_string()]);
		assert!(exports.is_empty());
	}

	#[test]
	fn from_imports_record_the_module_not_the_items() {
		let source = "from pkg.sub import alpha, beta\n";
		let tree = parse(source);
		let (imports, _) =
			Python {}.extract_imports_exports(first_node(&tree, "import_from_statement"), source);
		assert_eq!(imports, vec!["pkg.sub".to_string()]);
	}

	#[test]
	fn relative_from_imports_keep_their_leading_dots() {
		let source = "from ..pkg import thing\n";
		let tree = parse(source);
		let (imports, _) =
			Python {}.extract_imports_exports(first_node(&tree, "import_from_statement"), source);
		assert_eq!(imports, vec!["..pkg".to_string()]);
	}

	#[test]
	fn module_level_definitions_are_exported() {
		let source = "def top():\n    pass\n\nclass Top:\n    def inner(self):\n        pass\n";
		let tree = parse(source);
		let lang = Python {};

		let mut functions = Vec::new();
		nodes_of_kind(tree.root_node(), "function_definition", &mut functions);
		let (_, top_exports) = lang.extract_imports_exports(functions[0], source);
		assert_eq!(top_exports, vec!["top".to_string()]);

		// A method is not module level, so it must not be exported.
		let (_, inner_exports) = lang.extract_imports_exports(functions[1], source);
		assert!(inner_exports.is_empty());

		let (_, class_exports) =
			lang.extract_imports_exports(first_node(&tree, "class_definition"), source);
		assert_eq!(class_exports, vec!["Top".to_string()]);
	}

	#[test]
	fn decorated_module_level_functions_are_still_exported() {
		let source = "@app.route('/')\ndef handler():\n    pass\n";
		let tree = parse(source);
		let (_, exports) =
			Python {}.extract_imports_exports(first_node(&tree, "function_definition"), source);
		assert_eq!(
			exports,
			vec!["handler".to_string()],
			"a decorator wrapper must not hide a module-level export"
		);
	}

	#[test]
	fn calls_resolve_to_a_name_and_qualifier() {
		let source = "service.run(1)\nplain()\n";
		let tree = parse(source);
		let lang = Python {};
		let mut calls = Vec::new();
		nodes_of_kind(tree.root_node(), "call", &mut calls);

		let qualified = lang.extract_function_calls(calls[0], source);
		assert_eq!(qualified.len(), 1);
		assert_eq!(qualified[0].name, "run");
		assert_eq!(qualified[0].qualifier.as_deref(), Some("service"));

		let bare = lang.extract_function_calls(calls[1], source);
		assert_eq!(bare.len(), 1);
		assert_eq!(bare[0].name, "plain");
		assert!(bare[0].qualifier.is_none());

		// Nodes that are not calls yield nothing.
		assert!(lang
			.extract_function_calls(tree.root_node(), source)
			.is_empty());
	}

	#[test]
	fn class_bases_are_all_extends_never_implements() {
		let source = "class Child(Base, mixins.Loggable):\n    pass\n";
		let tree = parse(source);
		let relations =
			Python {}.extract_type_relations(first_node(&tree, "class_definition"), source);
		let names: Vec<_> = relations
			.iter()
			.map(|(kind, name)| {
				assert_eq!(*kind, TypeRelationKind::Extends);
				name.as_str()
			})
			.collect();
		assert!(names.contains(&"Base"), "got {names:?}");
		assert!(names.contains(&"Loggable"), "got {names:?}");
	}

	#[test]
	fn a_class_without_bases_has_no_type_relations() {
		let source = "class Solo:\n    pass\n";
		let tree = parse(source);
		assert!(Python {}
			.extract_type_relations(first_node(&tree, "class_definition"), source)
			.is_empty());
	}

	#[test]
	fn single_dot_relative_imports_resolve_within_the_package() {
		let resolved = Python {}.resolve_import(
			".helper",
			"pkg/app.py",
			&registry(&["pkg/app.py", "pkg/helper.py"]),
		);
		assert_eq!(resolved.as_deref(), Some("pkg/helper.py"));
	}

	#[test]
	fn double_dot_relative_imports_climb_one_package() {
		let resolved = Python {}.resolve_import(
			"..shared",
			"pkg/sub/app.py",
			&registry(&["pkg/sub/app.py", "pkg/shared.py"]),
		);
		assert_eq!(resolved.as_deref(), Some("pkg/shared.py"));
	}

	#[test]
	fn a_package_directory_resolves_through_its_init_file() {
		let resolved = Python {}.resolve_import(
			".sub",
			"pkg/app.py",
			&registry(&["pkg/app.py", "pkg/sub/__init__.py"]),
		);
		assert_eq!(resolved.as_deref(), Some("pkg/sub/__init__.py"));
	}

	#[test]
	fn absolute_imports_resolve_against_the_sibling_package() {
		let resolved = Python {}.resolve_import(
			"sub.deep",
			"pkg/app.py",
			&registry(&["pkg/app.py", "pkg/sub/deep.py"]),
		);
		assert_eq!(resolved.as_deref(), Some("pkg/sub/deep.py"));
	}

	#[test]
	fn third_party_imports_do_not_resolve_to_a_local_file() {
		let resolved = Python {}.resolve_import(
			"requests",
			"pkg/app.py",
			&registry(&["pkg/app.py", "pkg/helper.py"]),
		);
		assert_eq!(resolved, None);
	}

	#[test]
	fn each_function_becomes_its_own_region_and_the_class_does_not_collapse() {
		// Bodies are deliberately long so the single-line merge pass leaves the
		// two methods as separate regions.
		let source = "\
class Service:
    def alpha(self):
        first = 1
        second = first + 1
        third = second + 1
        return third

    def beta(self):
        first = 2
        second = first + 2
        third = second + 2
        return third
";
		let found = regions(source);
		let functions: Vec<_> = found
			.iter()
			.filter(|r| r.node_kind == "function_definition")
			.collect();
		assert_eq!(
			functions.len(),
			2,
			"expected one region per method, got {:?}",
			found.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
		assert!(
			!found.iter().any(|r| r.node_kind == "class_definition"),
			"a class must never surface as one blob region"
		);
		// Each method region carries its own name and its own locals.
		assert!(functions[0].symbols.contains(&"alpha".to_string()));
		assert!(functions[1].symbols.contains(&"beta".to_string()));
	}

	#[test]
	fn consecutive_single_line_imports_merge_into_one_region() {
		let found = regions("import os\nfrom sys import argv\n");
		assert_eq!(found.len(), 1, "expected one merged import region");
		assert!(found[0].content.contains("// Merged import statements"));
		assert!(found[0].content.contains("import os"));
		assert!(found[0].content.contains("from sys import argv"));
	}

	#[test]
	fn a_lone_import_stays_its_own_region() {
		let found = regions("import os\n");
		assert_eq!(found.len(), 1);
		assert_eq!(found[0].node_kind, "import_statement");
		assert!(!found[0].content.contains("// Merged"));
	}
}
