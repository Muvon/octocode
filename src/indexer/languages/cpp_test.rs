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
mod cpp_tests {
	use crate::indexer::file_utils::FileUtils;
	use crate::indexer::languages::cpp::Cpp;
	use crate::indexer::languages::Language;
	use crate::indexer::languages::{self, resolution_utils};
	use std::path::Path;
	use tree_sitter::{Node, Tree};

	const CPP_MODULE_EXTENSIONS: [&str; 5] = ["cppm", "ixx", "mxx", "ccm", "cxxm"];

	#[test]
	fn test_cpp_module_extensions_are_registered_on_language() {
		let lang = languages::get_language("cpp").expect("C++ language should be registered");
		let extensions = lang.get_file_extensions();

		for extension in CPP_MODULE_EXTENSIONS {
			assert!(
				extensions.contains(&extension),
				"C++ language should support .{extension} module files"
			);
		}
	}

	#[test]
	fn test_cpp_module_extensions_detect_as_cpp() {
		for extension in CPP_MODULE_EXTENSIONS {
			let file_name = format!("math.{extension}");

			assert_eq!(
				FileUtils::detect_language(Path::new(&file_name)),
				Some("cpp"),
				"FileUtils should detect .{extension} files as C++"
			);
			assert_eq!(
				resolution_utils::detect_language_from_path(&file_name).as_deref(),
				Some("cpp"),
				"resolution_utils should detect .{extension} files as C++"
			);
		}
	}

	#[test]
	fn test_namespace_splits_into_individual_functions() {
		use crate::indexer::code_region_extractor::extract_meaningful_regions;
		use tree_sitter::Parser;

		// Non-trivial content so the smart single-line merge pass doesn't recombine them.
		let code = r#"
namespace app {
	void foo() {
		int x = 1;
		int y = 2;
		int z = x + y;
		return;
	}
	void bar() {
		int a = 10;
		int b = 20;
		int c = a * b;
		return;
	}
}
"#;
		let lang = languages::get_language("cpp").unwrap();
		let mut parser = Parser::new();
		parser.set_language(&lang.get_ts_language()).unwrap();
		let tree = parser.parse(code, None).unwrap();
		let mut regions = Vec::new();
		extract_meaningful_regions(tree.root_node(), code, lang.as_ref(), &mut regions);

		let namespace_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "namespace_definition")
			.collect();
		assert_eq!(
			namespace_regions.len(),
			0,
			"namespace with functions inside should not collapse into one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		let function_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "function_definition")
			.collect();
		assert_eq!(
			function_regions.len(),
			2,
			"expected a region per function inside namespace"
		);
	}

	#[test]
	fn test_empty_namespace_falls_back_to_single_region() {
		use crate::indexer::code_region_extractor::extract_meaningful_regions;
		use tree_sitter::Parser;

		let code = r#"
namespace empty_ns {
	// nothing meaningful in here
}
"#;
		let lang = languages::get_language("cpp").unwrap();
		let mut parser = Parser::new();
		parser.set_language(&lang.get_ts_language()).unwrap();
		let tree = parser.parse(code, None).unwrap();
		let mut regions = Vec::new();
		extract_meaningful_regions(tree.root_node(), code, lang.as_ref(), &mut regions);

		let namespace_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "namespace_definition")
			.collect();
		assert_eq!(
			namespace_regions.len(),
			1,
			"empty namespace should fall back to its own single region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}

	// ── harness ──────────────────────────────────────────────────────────────

	fn parse_cpp(source: &str) -> Tree {
		let mut parser = tree_sitter::Parser::new();
		parser
			.set_language(&Cpp {}.get_ts_language())
			.expect("C++ grammar should load");
		parser.parse(source, None).expect("C++ source should parse")
	}

	fn collect_nodes<'tree>(node: Node<'tree>, kind: &str, out: &mut Vec<Node<'tree>>) {
		if node.kind() == kind {
			out.push(node);
		}
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			collect_nodes(child, kind, out);
		}
	}

	fn nodes_of_kind<'tree>(tree: &'tree Tree, kind: &str) -> Vec<Node<'tree>> {
		let mut found = Vec::new();
		collect_nodes(tree.root_node(), kind, &mut found);
		found
	}

	fn first_node<'tree>(tree: &'tree Tree, kind: &str) -> Node<'tree> {
		*nodes_of_kind(tree, kind)
			.first()
			.unwrap_or_else(|| panic!("parsed source has no `{kind}` node"))
	}

	fn symbols_of(source: &str, kind: &str) -> Vec<String> {
		let tree = parse_cpp(source);
		Cpp {}.extract_symbols(first_node(&tree, kind), source)
	}

	fn imports_exports_of(source: &str, kind: &str) -> (Vec<String>, Vec<String>) {
		let tree = parse_cpp(source);
		Cpp {}.extract_imports_exports(first_node(&tree, kind), source)
	}

	fn calls_of(source: &str) -> Vec<(String, Option<String>)> {
		let tree = parse_cpp(source);
		nodes_of_kind(&tree, "call_expression")
			.into_iter()
			.flat_map(|node| Cpp {}.extract_function_calls(node, source))
			.map(|target| (target.name, target.qualifier))
			.collect()
	}

	fn relations_of(source: &str, kind: &str) -> Vec<(languages::TypeRelationKind, String)> {
		let tree = parse_cpp(source);
		Cpp {}.extract_type_relations(first_node(&tree, kind), source)
	}

	fn registry_of(files: &[&str]) -> resolution_utils::FileRegistry {
		let owned: Vec<String> = files.iter().map(|file| (*file).to_string()).collect();
		resolution_utils::FileRegistry::new(&owned)
	}

	fn cpp_regions(source: &str) -> Vec<crate::indexer::code_region_extractor::CodeRegion> {
		let tree = parse_cpp(source);
		let mut regions = Vec::new();
		crate::indexer::code_region_extractor::extract_meaningful_regions(
			tree.root_node(),
			source,
			&Cpp {},
			&mut regions,
		);
		regions
	}

	// ── language metadata ────────────────────────────────────────────────────

	#[test]
	fn language_reports_its_name_and_every_supported_extension() {
		let cpp = Cpp {};
		assert_eq!(cpp.name(), "cpp");
		assert_eq!(
			cpp.get_file_extensions(),
			vec![
				"cpp", "cc", "cxx", "c++", "c", "h", "hpp", "hxx", "cppm", "ixx", "mxx", "ccm",
				"cxxm"
			]
		);
	}

	#[test]
	fn chunking_kinds_exclude_containers_that_the_symbol_tier_restores() {
		let cpp = Cpp {};
		assert_eq!(
			cpp.get_meaningful_kinds(),
			vec![
				"function_definition",
				"declaration",
				"enum_specifier",
				"namespace_definition",
				"preproc_include",
			]
		);
		assert_eq!(
			cpp.get_symbol_kinds(),
			vec![
				"function_definition",
				"class_specifier",
				"struct_specifier",
				"enum_specifier",
				"namespace_definition",
			]
		);
		assert_eq!(cpp.descend_first_kinds(), vec!["namespace_definition"]);
	}

	#[test]
	fn node_type_descriptions_cover_every_mapped_kind_and_the_fallback() {
		let cpp = Cpp {};
		for (node_type, expected) in [
			("function_definition", "function declarations"),
			("class_specifier", "class declarations"),
			("struct_specifier", "struct declarations"),
			("enum_specifier", "enum declarations"),
			("namespace_definition", "namespace declarations"),
			("template_declaration", "template declarations"),
			("declaration", "variable declarations"),
			("preproc_include", "preprocessor directives"),
			("preproc_def", "preprocessor directives"),
			("preproc_function_def", "preprocessor directives"),
			("preproc_ifdef", "preprocessor directives"),
			("compound_statement", "declarations"),
		] {
			assert_eq!(
				cpp.get_node_type_description(node_type),
				expected,
				"unexpected description for {node_type}"
			);
		}
	}

	#[test]
	fn node_types_are_equivalent_only_within_the_same_semantic_group() {
		let cpp = Cpp {};
		for (left, right) in [
			("function_definition", "function_definition"),
			("class_specifier", "struct_specifier"),
			("struct_specifier", "enum_specifier"),
			("preproc_include", "preproc_ifdef"),
			("unknown_kind", "unknown_kind"),
		] {
			assert!(
				cpp.are_node_types_equivalent(left, right),
				"{left} and {right} should be equivalent"
			);
		}

		for (left, right) in [
			("function_definition", "class_specifier"),
			("declaration", "function_definition"),
			("preproc_include", "namespace_definition"),
			("template_declaration", "class_specifier"),
			("unknown_kind", "other_kind"),
		] {
			assert!(
				!cpp.are_node_types_equivalent(left, right),
				"{left} and {right} should not be equivalent"
			);
		}
	}

	// ── symbol extraction ────────────────────────────────────────────────────

	#[test]
	fn function_definition_symbols_include_name_owner_and_body_locals() {
		let code = r#"
struct Point {
	double x;
	void move() {
		int tmp = 1;
		double scale = 2.0;
		if (tmp > 0) {
			int inner = 3;
		}
	}
};
"#;
		assert_eq!(
			symbols_of(code, "function_definition"),
			vec!["Point", "inner", "move", "scale", "tmp"]
		);
	}

	#[test]
	fn out_of_line_member_definition_keeps_its_name_in_extract_symbols() {
		let code = r#"
void Widget::resize(int w) {
	int local_a = 1;
	return;
}
"#;
		// The declarator of an out-of-line definition is a `qualified_identifier`;
		// the name is read through its `name` field, so the method name lands in
		// the symbol list alongside the body locals.
		assert_eq!(
			symbols_of(code, "function_definition"),
			vec!["local_a", "resize"]
		);

		let tree = parse_cpp(code);
		let node = first_node(&tree, "function_definition");
		assert_eq!(
			Cpp {}.extract_declaration_name(node, code).as_deref(),
			Some("resize")
		);
		assert_eq!(
			Cpp {}.extract_symbol_owner(node, code).as_deref(),
			Some("Widget")
		);
	}

	#[test]
	fn declaration_symbols_unwrap_pointer_array_and_comma_separated_declarators() {
		for (code, expected) in [
			("void free_fn(int a);\n", vec!["free_fn"]),
			("int x = 1;\n", vec!["x"]),
			("int* ptr_var;\n", vec!["ptr_var"]),
			("int arr_var[10];\n", vec!["arr_var"]),
			("int m1, m2 = 3;\n", vec!["m1", "m2"]),
		] {
			assert_eq!(symbols_of(code, "declaration"), expected, "for {code:?}");
		}
	}

	#[test]
	fn class_and_enum_symbols_include_their_members() {
		let class_code = r#"
class Widget : public Base {
public:
	int width;
	void resize(int w);
};
"#;
		// Members hang off a `field_declaration_list`, so they are only reached
		// once `extract_cpp_members` descends into it.
		// `extract_symbols` ends in `deduplicate_symbols`, which sorts.
		assert_eq!(
			symbols_of(class_code, "class_specifier"),
			vec!["Widget", "resize", "width"]
		);
		assert_eq!(
			symbols_of("enum class Status { OK, FAIL };\n", "enum_specifier"),
			vec!["FAIL", "OK", "Status"]
		);
	}

	#[test]
	fn namespace_symbols_are_the_namespace_name() {
		assert_eq!(
			symbols_of("namespace app { }\n", "namespace_definition"),
			vec!["app"]
		);
	}

	#[test]
	fn unmapped_node_kinds_fall_back_to_sorted_identifiers() {
		let code = "void run() {\n\thelper(value);\n}\n";
		assert_eq!(
			symbols_of(code, "expression_statement"),
			vec!["helper", "value"]
		);
	}

	#[test]
	fn extract_identifiers_keeps_first_occurrence_order_and_drops_duplicates() {
		let code = "void f(Widget w) {\n\tw.width = w.width;\n}\n";
		let tree = parse_cpp(code);
		let mut symbols = Vec::new();
		Cpp {}.extract_identifiers(first_node(&tree, "function_definition"), code, &mut symbols);
		assert_eq!(symbols, vec!["f", "Widget", "w", "width"]);
	}

	// ── imports / exports ────────────────────────────────────────────────────

	#[test]
	fn angle_bracket_and_quoted_includes_both_become_imports() {
		let code = "#include <vector>\n#include \"local/header.h\"\n#include HEADER_MACRO\n";
		let tree = parse_cpp(code);
		let includes = nodes_of_kind(&tree, "preproc_include");
		assert_eq!(includes.len(), 3);

		let parsed: Vec<(Vec<String>, Vec<String>)> = includes
			.into_iter()
			.map(|node| Cpp {}.extract_imports_exports(node, code))
			.collect();
		assert_eq!(parsed[0].0, vec!["vector"]);
		assert_eq!(parsed[1].0, vec!["local/header.h"]);
		// A macro-expanded include has no quotes or angle brackets to strip, so
		// `parse_cpp_include` yields nothing.
		assert!(parsed[2].0.is_empty());
		assert!(parsed.iter().all(|(_, exports)| exports.is_empty()));
	}

	#[test]
	fn function_and_variable_declarations_are_exported_by_name() {
		for (code, kind, expected) in [
			(
				"int main() { return 0; }\n",
				"function_definition",
				vec!["main"],
			),
			("void free_fn(int a);\n", "declaration", vec!["free_fn"]),
			("int counter = 0;\n", "declaration", vec!["counter"]),
			("int p1, p2;\n", "declaration", vec!["p1", "p2"]),
		] {
			let (imports, exports) = imports_exports_of(code, kind);
			assert!(imports.is_empty());
			assert_eq!(exports, expected, "for {code:?}");
		}
	}

	#[test]
	fn out_of_line_member_definition_exports_its_method_name() {
		let (_, exports) =
			imports_exports_of("void Widget::resize(int w) { }\n", "function_definition");
		assert_eq!(exports, vec!["resize"]);
	}

	#[test]
	fn type_namespace_template_and_typedef_declarations_are_exported() {
		for (code, kind, expected) in [
			("class Widget { };\n", "class_specifier", "Widget"),
			("struct Point { };\n", "struct_specifier", "Point"),
			("enum class Status { OK };\n", "enum_specifier", "Status"),
			("namespace app { }\n", "namespace_definition", "app"),
			("typedef int MyInt;\n", "type_definition", "MyInt"),
		] {
			let (_, exports) = imports_exports_of(code, kind);
			assert_eq!(exports, vec![expected], "for {code:?}");
		}

		let template_class = "template<typename T>\nclass Holder {\n\tT value;\n};\n";
		let (_, exports) = imports_exports_of(template_class, "template_declaration");
		assert_eq!(exports, vec!["template<Holder>"]);

		let template_fn = "template<typename T>\nT identity(T value) { return value; }\n";
		let (_, exports) = imports_exports_of(template_fn, "template_declaration");
		assert_eq!(exports, vec!["template<identity>"]);
	}

	#[test]
	fn define_directives_export_their_macro_name() {
		// The grammar emits `preproc_def` / `preproc_function_def`; there is no
		// `preproc_define` kind.
		let (imports, exports) = imports_exports_of("#define MAX_SIZE 10\n", "preproc_def");
		assert!(imports.is_empty());
		assert_eq!(exports, vec!["MAX_SIZE"]);
	}

	// ── calls and type relations ─────────────────────────────────────────────

	#[test]
	fn call_extraction_keeps_the_qualifier_for_dot_arrow_and_scope_operators() {
		let code = r#"
void run() {
	helper();
	obj.method();
	ptr->call();
	app::Widget::create();
}
"#;
		let extracted = calls_of(code);
		let calls: Vec<(&str, Option<&str>)> = extracted
			.iter()
			.map(|(name, qualifier)| (name.as_str(), qualifier.as_deref()))
			.collect();
		assert_eq!(
			calls,
			vec![
				("helper", None),
				("method", Some("obj")),
				("call", Some("ptr")),
				("create", Some("app::Widget")),
			]
		);
	}

	#[test]
	fn non_call_nodes_produce_no_call_targets() {
		let code = "void run() {\n\thelper();\n}\n";
		let tree = parse_cpp(code);
		assert!(Cpp {}
			.extract_function_calls(first_node(&tree, "function_definition"), code)
			.is_empty());
	}

	#[test]
	fn base_classes_are_reported_as_extends_regardless_of_access_specifier() {
		let extracted = relations_of(
			"class Derived : public ns::Base<int>, private Other { };\n",
			"class_specifier",
		);
		let relations: Vec<(languages::TypeRelationKind, &str)> = extracted
			.iter()
			.map(|(kind, name)| (*kind, name.as_str()))
			.collect();
		assert_eq!(
			relations,
			vec![
				(languages::TypeRelationKind::Extends, "Base"),
				(languages::TypeRelationKind::Extends, "Other"),
			]
		);

		let extracted_struct = relations_of("struct Point : Base { };\n", "struct_specifier");
		let struct_relations: Vec<(languages::TypeRelationKind, &str)> = extracted_struct
			.iter()
			.map(|(kind, name)| (*kind, name.as_str()))
			.collect();
		assert_eq!(
			struct_relations,
			vec![(languages::TypeRelationKind::Extends, "Base")]
		);
	}

	#[test]
	fn types_without_a_base_clause_and_non_type_nodes_have_no_relations() {
		assert!(relations_of("class Plain { };\n", "class_specifier").is_empty());
		assert!(relations_of("int main() { return 0; }\n", "function_definition").is_empty());
	}

	// ── declaration names and owners ─────────────────────────────────────────

	#[test]
	fn declaration_names_come_from_the_definition_not_from_type_references() {
		for (code, kind, expected) in [
			(
				"int main() { return 0; }\n",
				"function_definition",
				Some("main"),
			),
			(
				"class Widget { int w; };\n",
				"class_specifier",
				Some("Widget"),
			),
			("namespace app { }\n", "namespace_definition", Some("app")),
			// Bodyless specifiers are forward declarations or type references.
			("class Fwd;\n", "class_specifier", None),
			("enum Plain;\n", "enum_specifier", None),
			("struct stat st;\n", "struct_specifier", None),
		] {
			let tree = parse_cpp(code);
			let name = Cpp {}.extract_declaration_name(first_node(&tree, kind), code);
			assert_eq!(name.as_deref(), expected, "for {code:?}");
		}
	}

	#[test]
	fn symbol_owner_falls_back_to_the_enclosing_container_for_inline_methods() {
		let code = "struct Point {\n\tvoid move() { }\n};\n";
		let tree = parse_cpp(code);
		assert_eq!(
			Cpp {}
				.extract_symbol_owner(first_node(&tree, "function_definition"), code)
				.as_deref(),
			Some("Point")
		);

		let free_code = "void free_fn() { }\n";
		let free_tree = parse_cpp(free_code);
		assert_eq!(
			Cpp {}.extract_symbol_owner(first_node(&free_tree, "function_definition"), free_code),
			None
		);
	}

	// ── import resolution ────────────────────────────────────────────────────

	#[test]
	fn quoted_includes_resolve_relative_to_the_including_file() {
		let cpp = Cpp {};
		let registry = registry_of(&["src/gfx/widget.h", "src/common/defs.h"]);
		assert_eq!(
			cpp.resolve_import("\"widget.h\"", "src/gfx/main.cpp", &registry),
			Some("src/gfx/widget.h".to_string())
		);
		assert_eq!(
			cpp.resolve_import("\"../common/defs.h\"", "src/gfx/main.cpp", &registry),
			Some("src/common/defs.h".to_string())
		);
	}

	#[test]
	fn angle_bracket_includes_only_resolve_when_the_header_is_in_the_project() {
		let cpp = Cpp {};
		let registry = registry_of(&["gfx/widget.h", "src/main.cpp"]);
		assert_eq!(
			cpp.resolve_import("<gfx/widget.h>", "src/main.cpp", &registry),
			Some("gfx/widget.h".to_string())
		);
		// A real system header has no project file to point at.
		assert_eq!(
			cpp.resolve_import("<vector>", "src/main.cpp", &registry),
			None
		);
	}

	#[test]
	fn unquoted_headers_try_the_source_directory_then_exact_then_include_dirs() {
		let cpp = Cpp {};

		let sibling = registry_of(&["src/gfx/widget.h"]);
		assert_eq!(
			cpp.resolve_import("widget.h", "src/gfx/main.cpp", &sibling),
			Some("src/gfx/widget.h".to_string())
		);

		let exact = registry_of(&["gfx/widget.h"]);
		assert_eq!(
			cpp.resolve_import("gfx/widget.h", "app/main.cpp", &exact),
			Some("gfx/widget.h".to_string())
		);

		let include_dir = registry_of(&["include/widget.h"]);
		assert_eq!(
			cpp.resolve_import("widget.h", "app/main.cpp", &include_dir),
			Some("include/widget.h".to_string())
		);
	}

	#[test]
	fn third_party_headers_outside_the_project_do_not_resolve() {
		let registry = registry_of(&["src/main.cpp", "src/util.h"]);
		assert_eq!(
			Cpp {}.resolve_import("absl/strings/str_cat.h", "src/main.cpp", &registry),
			None
		);
	}

	// ── region extraction ────────────────────────────────────────────────────

	#[test]
	fn consecutive_includes_merge_into_one_preprocessor_block() {
		let code = "#include <vector>\n#include <string>\n#include \"local.h\"\n";
		let regions = cpp_regions(code);
		assert_eq!(regions.len(), 1, "expected one merged include block");
		assert_eq!(regions[0].node_kind, "preproc_include");
		assert!(regions[0]
			.content
			.starts_with("// Merged preprocessor directives (3 declarations)\n"));
		assert!(regions[0].content.contains("#include \"local.h\""));
	}

	#[test]
	fn consecutive_globals_merge_into_one_variable_declaration_block() {
		let code = "int width = 100;\nint height = 200;\nint depth = 300;\n";
		let regions = cpp_regions(code);
		assert_eq!(regions.len(), 1);
		assert_eq!(regions[0].node_kind, "declaration");
		assert!(regions[0]
			.content
			.starts_with("// Merged variable declarations (3 declarations)\n"));
		assert_eq!(regions[0].symbols, vec!["depth", "height", "width"]);
	}

	#[test]
	fn class_methods_inside_a_namespace_are_chunked_individually() {
		let code = r#"
namespace app {
class Widget {
public:
	void resize(int w) {
		int a = 1;
		int b = 2;
		int c = a + b;
		return;
	}
	void reset() {
		int x = 0;
		int y = 0;
		int z = x + y;
		return;
	}
};
}
"#;
		let regions = cpp_regions(code);
		let kinds: Vec<&str> = regions.iter().map(|r| r.node_kind.as_str()).collect();
		assert_eq!(
			kinds,
			vec!["function_definition", "function_definition"],
			"neither the namespace nor the class should become a region"
		);
	}

	#[test]
	fn an_enum_nested_in_a_class_contributes_its_constants() {
		// A top-level enum reaches its constants through the enumerator_list
		// descent; a nested one arrives at the enum_specifier arm instead, so
		// both routes need their own case.
		let symbols = symbols_of(
			"class Palette {\npublic:\n\tenum Shade { Light, Dark };\n\tint width;\n};\n",
			"class_specifier",
		);
		assert!(symbols.contains(&"Light".to_string()), "{symbols:?}");
		assert!(symbols.contains(&"Dark".to_string()), "{symbols:?}");
		assert!(symbols.contains(&"width".to_string()), "{symbols:?}");
	}

	#[test]
	fn a_repeated_enum_constant_name_is_only_recorded_once() {
		let symbols = symbols_of(
			"class Dup {\n\tenum A { Same };\n\tenum B { Same };\n};\n",
			"class_specifier",
		);
		assert_eq!(symbols.iter().filter(|s| s.as_str() == "Same").count(), 1);
	}
}
