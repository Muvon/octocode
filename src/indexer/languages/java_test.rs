#[cfg(test)]
mod java_tests {
	use crate::indexer::languages::java::Java;
	use crate::indexer::languages::resolution_utils::FileRegistry;
	use crate::indexer::languages::{CallTarget, Language, TypeRelationKind};
	use crate::indexer::{code_region_extractor, languages};
	use tree_sitter::{Node, Parser, Tree};

	#[test]
	fn test_java_region_extraction() {
		let contents = r#"package com.example.test;

import java.util.List;
import java.util.ArrayList;

public class SimpleTest {
    private String name;

    public SimpleTest(String name) {
        this.name = name;
    }

    public String getName() {
        return name;
    }
}
"#;

		// Get Java language implementation
		let lang_impl = languages::get_language("java").unwrap();

		// Set up parser
		let mut parser = Parser::new();
		parser.set_language(&lang_impl.get_ts_language()).unwrap();

		// Parse the file
		let tree = parser.parse(contents, None).unwrap();

		// Extract regions
		let mut regions = Vec::new();
		code_region_extractor::extract_meaningful_regions(
			tree.root_node(),
			contents,
			lang_impl.as_ref(),
			&mut regions,
		);

		println!("Extracted {} regions:", regions.len());
		for (i, region) in regions.iter().enumerate() {
			println!("\n--- Region {} ---", i + 1);
			println!("Kind: {}", region.node_kind);
			println!("Lines: {}-{}", region.start_line, region.end_line);
			println!("Symbols: {:?}", region.symbols);
			println!("Content preview:");
			let preview = if region.content.len() > 200 {
				format!("{}...", &region.content[..200])
			} else {
				region.content.clone()
			};
			println!("{}", preview);
		}

		// Verify we have the expected regions
		assert!(!regions.is_empty(), "Should extract some regions");

		// class with a constructor and a method inside should not collapse into
		// one region, got {:?}
		let class_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "class_declaration")
			.collect();
		assert_eq!(
			class_regions.len(),
			0,
			"class with constructor/method inside should not collapse into one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		// Check that we have method and constructor declarations split out
		let constructor_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "constructor_declaration")
			.collect();
		assert!(
			!constructor_regions.is_empty(),
			"Should have constructor declaration"
		);

		let method_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "method_declaration")
			.collect();
		assert!(!method_regions.is_empty(), "Should have method declaration");
	}

	#[test]
	fn test_record_with_method_splits_into_method_region() {
		let contents = r#"public record Point(int x, int y) {
    int sum() {
        return x + y;
    }
}
"#;
		let lang_impl = languages::get_language("java").unwrap();
		let mut parser = Parser::new();
		parser.set_language(&lang_impl.get_ts_language()).unwrap();
		let tree = parser.parse(contents, None).unwrap();
		let mut regions = Vec::new();
		code_region_extractor::extract_meaningful_regions(
			tree.root_node(),
			contents,
			lang_impl.as_ref(),
			&mut regions,
		);

		let record_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "record_declaration")
			.collect();
		assert_eq!(
			record_regions.len(),
			0,
			"record with an explicit method should not collapse into one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		let method_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "method_declaration")
			.collect();
		assert_eq!(
			method_regions.len(),
			1,
			"expected a region for the record's explicit method"
		);
	}

	#[test]
	fn test_plain_data_record_stays_single_region() {
		let contents = r#"public record Point(int x, int y) {}
"#;
		let lang_impl = languages::get_language("java").unwrap();
		let mut parser = Parser::new();
		parser.set_language(&lang_impl.get_ts_language()).unwrap();
		let tree = parser.parse(contents, None).unwrap();
		let mut regions = Vec::new();
		code_region_extractor::extract_meaningful_regions(
			tree.root_node(),
			contents,
			lang_impl.as_ref(),
			&mut regions,
		);

		let record_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "record_declaration")
			.collect();
		assert_eq!(
			record_regions.len(),
			1,
			"plain data record with no explicit methods should remain its own single region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}

	fn parse(source: &str) -> Tree {
		let mut parser = Parser::new();
		parser
			.set_language(&Java {}.get_ts_language())
			.expect("java grammar");
		parser.parse(source, None).expect("parse")
	}

	fn regions(source: &str) -> Vec<code_region_extractor::CodeRegion> {
		let tree = parse(source);
		let lang = Java {};
		let mut out = Vec::new();
		code_region_extractor::extract_meaningful_regions(
			tree.root_node(),
			source,
			&lang,
			&mut out,
		);
		out
	}

	/// Depth-first walk collecting every node of `kind`, in document order.
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

	/// A source tree under `demo/`, which does not exist on disk, so resolution
	/// is decided purely by the registry contents.
	fn demo_project() -> FileRegistry {
		registry(&[
			"demo/src/main/java/com/example/app/Service.java",
			"demo/src/main/java/com/example/app/Widget.java",
		])
	}

	#[test]
	fn the_parser_is_named_java_and_claims_the_java_extension() {
		let lang = Java {};
		assert_eq!(lang.name(), "java");
		assert_eq!(lang.get_file_extensions(), vec!["java"]);
	}

	#[test]
	fn type_containers_are_excluded_from_chunking_but_restored_as_symbol_kinds() {
		let lang = Java {};
		let kinds = lang.get_meaningful_kinds();
		assert!(kinds.contains(&"method_declaration"));
		assert!(kinds.contains(&"field_declaration"));
		assert!(!kinds.contains(&"class_declaration"));
		assert!(!kinds.contains(&"interface_declaration"));

		let symbol_kinds = lang.get_symbol_kinds();
		assert!(symbol_kinds.contains(&"class_declaration"));
		assert!(symbol_kinds.contains(&"interface_declaration"));
		assert!(symbol_kinds.contains(&"enum_declaration"));
		// These chunk fine but declare no usable symbol name.
		assert!(!symbol_kinds.contains(&"field_declaration"));
		assert!(!symbol_kinds.contains(&"lambda_expression"));
		assert!(!symbol_kinds.contains(&"import_declaration"));

		assert_eq!(lang.descend_first_kinds(), vec!["record_declaration"]);
	}

	#[test]
	fn every_node_type_description_arm_names_a_java_construct() {
		let lang = Java {};
		for (kind, description) in [
			("class_declaration", "Java class definition"),
			("interface_declaration", "Java interface definition"),
			("enum_declaration", "Java enum definition"),
			("method_declaration", "Java method definition"),
			("constructor_declaration", "Java constructor definition"),
			("field_declaration", "Java field declaration"),
			(
				"annotation_type_declaration",
				"Java annotation type definition",
			),
			("record_declaration", "Java record definition (Java 14+)"),
			("import_declaration", "Java import statement"),
			("package_declaration", "Java package declaration"),
			("lambda_expression", "Java lambda expression"),
			("method_reference", "Java method reference"),
			("block", "Java code element"),
		] {
			assert_eq!(
				lang.get_node_type_description(kind),
				description,
				"description for {kind}"
			);
		}
	}

	#[test]
	fn single_line_declarations_are_equivalent_but_methods_never_merge() {
		let lang = Java {};
		for (first, second) in [
			("import_declaration", "package_declaration"),
			("package_declaration", "import_declaration"),
			("field_declaration", "import_declaration"),
			("import_declaration", "field_declaration"),
			("field_declaration", "package_declaration"),
			("package_declaration", "field_declaration"),
			("field_declaration", "field_declaration"),
			("import_declaration", "import_declaration"),
			("package_declaration", "package_declaration"),
			("method_declaration", "method_declaration"),
		] {
			assert!(
				lang.are_node_types_equivalent(first, second),
				"{first} vs {second}"
			);
		}
		for (first, second) in [
			("method_declaration", "constructor_declaration"),
			("annotation_type_declaration", "record_declaration"),
			("method_declaration", "field_declaration"),
		] {
			assert!(
				!lang.are_node_types_equivalent(first, second),
				"{first} vs {second}"
			);
		}
	}

	#[test]
	fn declaration_names_come_from_the_name_field_not_the_return_type() {
		let source = "class Service {\n\tpublic String getName() {\n\t\treturn null;\n\t}\n}\n";
		let tree = parse(source);
		let lang = Java {};
		assert_eq!(
			lang.extract_declaration_name(first_node(&tree, "class_declaration"), source),
			Some("Service".to_string())
		);
		assert_eq!(
			lang.extract_declaration_name(first_node(&tree, "method_declaration"), source),
			Some("getName".to_string())
		);
	}

	#[test]
	fn a_method_carries_its_enclosing_type_name_in_its_symbols() {
		let source = "class Service {\n\tpublic void run() {\n\t\tint x = 1;\n\t}\n}\n";
		let tree = parse(source);
		let node = first_node(&tree, "method_declaration");
		assert_eq!(
			Java {}.extract_symbols(node, source),
			vec!["Service", "run"]
		);
	}

	#[test]
	fn a_constructor_name_deduplicates_against_its_owning_class() {
		let source =
			"class Service {\n\tpublic Service(String name) {\n\t\tthis.name = name;\n\t}\n}\n";
		let tree = parse(source);
		let node = first_node(&tree, "constructor_declaration");
		assert_eq!(Java {}.extract_symbols(node, source), vec!["Service"]);
	}

	#[test]
	fn an_interface_method_carries_the_interface_name() {
		let source = "interface Shape {\n\tdouble area();\n}\n";
		let tree = parse(source);
		let node = first_node(&tree, "method_declaration");
		assert_eq!(Java {}.extract_symbols(node, source), vec!["Shape", "area"]);
	}

	#[test]
	fn type_declarations_yield_their_own_name_as_the_only_symbol() {
		for (source, kind, expected) in [
			("class Service {}\n", "class_declaration", "Service"),
			("interface Shape {}\n", "interface_declaration", "Shape"),
			("enum Status { OK }\n", "enum_declaration", "Status"),
			(
				"record Point(int x, int y) {}\n",
				"record_declaration",
				"Point",
			),
			(
				"@interface Marker {}\n",
				"annotation_type_declaration",
				"Marker",
			),
		] {
			let tree = parse(source);
			let node = first_node(&tree, kind);
			assert_eq!(
				Java {}.extract_symbols(node, source),
				vec![expected.to_string()],
				"symbols for {kind}"
			);
		}
	}

	#[test]
	fn a_lambda_is_a_placeholder_symbol_and_a_method_reference_is_its_own_text() {
		let source = "class Service {\n\tRunnable r = () -> run();\n\tSupplier<String> s = Service::create;\n}\n";
		let tree = parse(source);
		let lang = Java {};
		assert_eq!(
			lang.extract_symbols(first_node(&tree, "lambda_expression"), source),
			vec!["<lambda>"]
		);
		assert_eq!(
			lang.extract_symbols(first_node(&tree, "method_reference"), source),
			vec!["Service::create"]
		);
	}

	#[test]
	fn an_unhandled_node_only_yields_a_symbol_when_it_is_itself_an_identifier() {
		let source = "class Service {\n\tprivate String name;\n}\n";
		let tree = parse(source);
		let lang = Java {};
		// The fallback arm deliberately does not recurse, so a field declaration
		// contributes nothing even though it is a chunked kind.
		assert!(lang
			.extract_symbols(first_node(&tree, "field_declaration"), source)
			.is_empty());
		assert_eq!(
			lang.extract_symbols(first_node(&tree, "identifier"), source),
			vec!["Service"]
		);
	}

	#[test]
	fn identifier_extraction_drops_single_character_names() {
		let source = "class Service {\n\tvoid run(int x, int total) {\n\t\ttotal = x;\n\t}\n}\n";
		let tree = parse(source);
		let node = first_node(&tree, "method_declaration");
		let mut symbols = Vec::new();
		Java {}.extract_identifiers(node, source, &mut symbols);
		assert!(symbols.contains(&"run".to_string()), "{symbols:?}");
		assert!(symbols.contains(&"total".to_string()), "{symbols:?}");
		assert!(
			!symbols.contains(&"x".to_string()),
			"single-character identifiers are filtered out, got {symbols:?}"
		);
	}

	#[test]
	fn imports_are_stripped_down_to_their_dotted_path() {
		for (source, expected) in [
			("import java.util.List;\n", "java.util.List"),
			(
				"import static java.util.Arrays.asList;\n",
				"java.util.Arrays.asList",
			),
			("import java.util.*;\n", "java.util.*"),
		] {
			let tree = parse(source);
			let node = first_node(&tree, "import_declaration");
			let (imports, exports) = Java {}.extract_imports_exports(node, source);
			assert_eq!(imports, vec![expected], "imports for {source:?}");
			assert!(exports.is_empty(), "exports for {source:?}");
		}
	}

	#[test]
	fn a_package_declaration_is_exported_with_a_package_prefix() {
		let source = "package com.example.app;\n";
		let tree = parse(source);
		let node = first_node(&tree, "package_declaration");
		let (imports, exports) = Java {}.extract_imports_exports(node, source);
		assert!(imports.is_empty());
		assert_eq!(exports, vec!["package:com.example.app"]);
	}

	#[test]
	fn only_public_types_are_exported_and_the_kind_prefixes_the_name() {
		for (source, kind, expected) in [
			(
				"public class Service {}\n",
				"class_declaration",
				vec!["class:Service"],
			),
			(
				"public interface Shape {}\n",
				"interface_declaration",
				vec!["interface:Shape"],
			),
			(
				"public enum Status { OK }\n",
				"enum_declaration",
				vec!["enum:Status"],
			),
			(
				"public record Point(int x, int y) {}\n",
				"record_declaration",
				vec!["record:Point"],
			),
			(
				"public @interface Marker {}\n",
				"annotation_type_declaration",
				vec!["annotation:Marker"],
			),
			("class Service {}\n", "class_declaration", vec![]),
		] {
			let tree = parse(source);
			let node = first_node(&tree, kind);
			let (imports, exports) = Java {}.extract_imports_exports(node, source);
			assert!(imports.is_empty(), "imports for {source:?}");
			assert_eq!(exports, expected, "exports for {source:?}");
		}
	}

	#[test]
	fn only_public_methods_are_exported() {
		let source = "class Service {\n\tpublic void run() {}\n\tvoid hidden() {}\n}\n";
		let tree = parse(source);
		let mut methods = Vec::new();
		nodes_of_kind(tree.root_node(), "method_declaration", &mut methods);
		let lang = Java {};

		let (_, exports) = lang.extract_imports_exports(methods[0], source);
		assert_eq!(exports, vec!["method:run"]);

		let (_, exports) = lang.extract_imports_exports(methods[1], source);
		assert!(exports.is_empty());
	}

	#[test]
	fn method_invocations_keep_their_receiver_as_a_qualifier() {
		let source =
			"class Service {\n\tvoid run() {\n\t\thelper();\n\t\tthis.repo.save(name);\n\t}\n}\n";
		let tree = parse(source);
		let mut invocations = Vec::new();
		nodes_of_kind(tree.root_node(), "method_invocation", &mut invocations);
		let lang = Java {};
		let targets: Vec<CallTarget> = invocations
			.iter()
			.flat_map(|node| lang.extract_function_calls(*node, source))
			.collect();
		assert_eq!(
			targets,
			vec![
				CallTarget {
					name: "helper".to_string(),
					qualifier: None,
				},
				CallTarget {
					name: "save".to_string(),
					qualifier: Some("this::repo".to_string()),
				},
			]
		);
	}

	#[test]
	fn object_creation_reports_the_constructed_type_as_the_call_target() {
		let source = "class Service {\n\tvoid run() {\n\t\tWidget w = new Widget();\n\t}\n}\n";
		let tree = parse(source);
		let node = first_node(&tree, "object_creation_expression");
		assert_eq!(
			Java {}.extract_function_calls(node, source),
			vec![CallTarget {
				name: "Widget".to_string(),
				qualifier: None,
			}]
		);
	}

	#[test]
	fn a_method_reference_is_not_reported_as_a_call() {
		let source = "class Service {\n\tSupplier<String> s = Service::create;\n}\n";
		let tree = parse(source);
		let node = first_node(&tree, "method_reference");
		assert!(Java {}.extract_function_calls(node, source).is_empty());
	}

	#[test]
	fn a_class_reports_its_superclass_then_each_implemented_interface() {
		let source = "class Service extends Base implements Runnable, Closeable {}\n";
		let tree = parse(source);
		let node = first_node(&tree, "class_declaration");
		assert_eq!(
			Java {}.extract_type_relations(node, source),
			vec![
				(TypeRelationKind::Extends, "Base".to_string()),
				(TypeRelationKind::Implements, "Runnable".to_string()),
				(TypeRelationKind::Implements, "Closeable".to_string()),
			]
		);
	}

	#[test]
	fn interfaces_extend_while_enums_and_records_implement() {
		for (source, kind, expected) in [
			(
				"interface Shape extends Drawable, Sized {}\n",
				"interface_declaration",
				vec![
					(TypeRelationKind::Extends, "Drawable".to_string()),
					(TypeRelationKind::Extends, "Sized".to_string()),
				],
			),
			(
				"enum Status implements Serializable { OK }\n",
				"enum_declaration",
				vec![(TypeRelationKind::Implements, "Serializable".to_string())],
			),
			(
				// The generic argument is dropped by `simple_type_name`.
				"record Point(int x, int y) implements Comparable<Point> {}\n",
				"record_declaration",
				vec![(TypeRelationKind::Implements, "Comparable".to_string())],
			),
			("class Plain {}\n", "class_declaration", vec![]),
		] {
			let tree = parse(source);
			let node = first_node(&tree, kind);
			assert_eq!(
				Java {}.extract_type_relations(node, source),
				expected,
				"relations for {source:?}"
			);
		}
	}

	#[test]
	fn a_dotted_import_resolves_to_the_matching_source_file() {
		assert_eq!(
			Java {}.resolve_import(
				"com.example.app.Service",
				"demo/src/main/java/com/example/app/Widget.java",
				&demo_project()
			),
			Some("demo/src/main/java/com/example/app/Service.java".to_string())
		);
	}

	#[test]
	fn a_wildcard_import_never_resolves_to_a_single_file() {
		assert_eq!(
			Java {}.resolve_import(
				"com.example.app.*",
				"demo/src/main/java/com/example/app/Widget.java",
				&demo_project()
			),
			None
		);
	}

	#[test]
	fn imports_outside_the_project_and_unqualified_names_do_not_resolve() {
		let files = demo_project();
		let lang = Java {};
		let source = "demo/src/main/java/com/example/app/Widget.java";
		assert_eq!(lang.resolve_import("java.util.List", source, &files), None);
		// No dot at all means no package path to turn into a file path.
		assert_eq!(lang.resolve_import("Service", source, &files), None);
	}

	#[test]
	fn a_package_declaration_and_its_imports_merge_into_one_labelled_region() {
		let source = "package com.example.app;\n\nimport java.util.List;\nimport java.util.Map;\n\npublic class Service {\n\tpublic void run() {\n\t\tint x = 1;\n\t\tint y = 2;\n\t\tSystem.out.println(x + y);\n\t}\n}\n";
		let extracted = regions(source);

		let merged = extracted
			.iter()
			.find(|r| r.node_kind == "package_declaration")
			.expect("package/import block should survive merging");
		assert!(
			merged
				.content
				.starts_with("// Merged Java package declaration (3 declarations)\n"),
			"got: {:?}",
			merged.content
		);
		assert!(merged.content.contains("import java.util.List;"));
		assert!(merged.content.contains("import java.util.Map;"));

		// The multi-line method is never folded into the merged header block.
		assert_eq!(
			extracted
				.iter()
				.filter(|r| r.node_kind == "method_declaration")
				.count(),
			1
		);
	}

	#[test]
	fn a_field_declaration_region_falls_back_to_a_node_kind_symbol() {
		let source = "class Service {\n\tprivate String name;\n}\n";
		let extracted = regions(source);

		assert_eq!(
			extracted.len(),
			1,
			"got {:?}",
			extracted.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
		assert_eq!(extracted[0].node_kind, "field_declaration");
		// Java's extract_symbols has no field_declaration arm, so the region
		// extractor synthesises "<kind>_<start_line>" instead.
		assert_eq!(extracted[0].symbols, vec!["field_declaration_1"]);
	}
}
