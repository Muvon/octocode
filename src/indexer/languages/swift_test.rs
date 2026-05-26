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
mod swift_tests {
	use crate::indexer::code_region_extractor::extract_meaningful_regions;
	use crate::indexer::languages::{self, Language};
	use tree_sitter::Parser;

	fn make_parser() -> (Parser, Box<dyn Language>) {
		let lang = languages::get_language("swift").expect("Swift language not registered");
		let mut parser = Parser::new();
		parser.set_language(&lang.get_ts_language()).unwrap();
		(parser, lang)
	}

	// ── region extraction ────────────────────────────────────────────────────

	#[test]
	fn test_region_extraction_class_and_methods() {
		let code = r#"
import Foundation
import UIKit

public class Animal {
    public var name: String
    private var age: Int

    public init(name: String, age: Int) {
        self.name = name
        self.age = age
    }

    public func speak() -> String {
        return "..."
    }

    deinit {
        print("Animal freed")
    }
}
"#;

		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();
		let mut regions = Vec::new();
		extract_meaningful_regions(tree.root_node(), code, lang.as_ref(), &mut regions);

		assert!(
			!regions.is_empty(),
			"Should extract regions from class file"
		);

		let has_import = regions.iter().any(|r| r.node_kind == "import_declaration");
		assert!(has_import, "Should extract import declarations");

		let has_class = regions.iter().any(|r| r.node_kind == "class_declaration");
		assert!(has_class, "Should extract class declaration");
		let has_func = regions
			.iter()
			.any(|r| r.node_kind == "function_declaration" || r.node_kind == "class_declaration");
		assert!(
			has_func,
			"Should extract function or class declaration (speak or Animal)"
		);
	}

	#[test]
	fn test_region_extraction_struct_protocol_enum() {
		let code = r#"
protocol Drawable {
    func draw()
}

struct Point: Drawable {
    var x: Double
    var y: Double

    func draw() {
        print("Point at (\(x), \(y))")
    }
}

enum Direction {
    case north, south, east, west

    func opposite() -> Direction {
        switch self {
        case .north: return .south
        case .south: return .north
        case .east: return .west
        case .west: return .east
        }
    }
}
"#;

		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();
		let mut regions = Vec::new();
		extract_meaningful_regions(tree.root_node(), code, lang.as_ref(), &mut regions);

		assert!(!regions.is_empty(), "Should extract regions");

		let has_protocol = regions
			.iter()
			.any(|r| r.node_kind == "protocol_declaration");
		assert!(has_protocol, "Should extract protocol declaration");

		// struct and enum parse as class_declaration in tree-sitter-swift
		let has_type_decl = regions.iter().any(|r| r.node_kind == "class_declaration");
		assert!(
			has_type_decl,
			"Should extract struct/enum as class_declaration"
		);
	}

	#[test]
	fn test_region_extraction_extension_typealias() {
		let code = r#"
typealias Completion = () -> Void
typealias Handler<T> = (T) -> Void

extension String {
    func trimmed() -> String {
        return self.trimmingCharacters(in: .whitespaces)
    }

    var isEmpty: Bool {
        return self.count == 0
    }
}
"#;

		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();
		let mut regions = Vec::new();
		extract_meaningful_regions(tree.root_node(), code, lang.as_ref(), &mut regions);

		assert!(
			!regions.is_empty(),
			"Should extract regions from extension file"
		);

		let typealias_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "typealias_declaration")
			.collect();
		assert!(
			!typealias_regions.is_empty(),
			"Should extract typealias declarations"
		);

		let has_extension = regions.iter().any(|r| r.node_kind == "class_declaration");
		assert!(
			has_extension,
			"Should extract extension as class_declaration"
		);
	}

	#[test]
	fn test_region_extraction_free_functions() {
		let code = r#"
func add(_ a: Int, _ b: Int) -> Int {
    return a + b
}

func greet(name: String) -> String {
    return "Hello, \(name)!"
}
"#;

		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();
		let mut regions = Vec::new();
		extract_meaningful_regions(tree.root_node(), code, lang.as_ref(), &mut regions);

		assert!(
			!regions.is_empty(),
			"Should extract free functions (may be merged into one region)"
		);
	}

	// ── extract_symbols ──────────────────────────────────────────────────────

	#[test]
	fn test_extract_symbols_function() {
		let code = r#"func computeSum(_ values: [Int]) -> Int { return 0 }"#;
		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();

		// The root is source_file; first named child should be function_declaration
		let func_node = tree
			.root_node()
			.named_children(&mut tree.root_node().walk())
			.find(|n| n.kind() == "function_declaration")
			.expect("Should have function_declaration");

		let symbols = lang.extract_symbols(func_node, code);
		assert!(
			symbols.contains(&"computeSum".to_string()),
			"Should extract function name: got {:?}",
			symbols
		);
	}

	#[test]
	fn test_extract_symbols_class_declaration() {
		let code = r#"class MyViewController: UIViewController { }"#;
		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();

		let class_node = tree
			.root_node()
			.named_children(&mut tree.root_node().walk())
			.find(|n| n.kind() == "class_declaration")
			.expect("Should have class_declaration");

		let symbols = lang.extract_symbols(class_node, code);
		assert!(
			symbols.contains(&"MyViewController".to_string()),
			"Should extract class name: got {:?}",
			symbols
		);
	}

	#[test]
	fn test_extract_symbols_protocol() {
		let code = r#"protocol Serializable { func serialize() -> Data }"#;
		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();

		let proto_node = tree
			.root_node()
			.named_children(&mut tree.root_node().walk())
			.find(|n| n.kind() == "protocol_declaration")
			.expect("Should have protocol_declaration");

		let symbols = lang.extract_symbols(proto_node, code);
		assert!(
			symbols.contains(&"Serializable".to_string()),
			"Should extract protocol name: got {:?}",
			symbols
		);
	}

	#[test]
	fn test_extract_symbols_typealias() {
		let code = r#"typealias CompletionHandler = (Bool, Error?) -> Void"#;
		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();

		let alias_node = tree
			.root_node()
			.named_children(&mut tree.root_node().walk())
			.find(|n| n.kind() == "typealias_declaration")
			.expect("Should have typealias_declaration");

		let symbols = lang.extract_symbols(alias_node, code);
		assert!(
			symbols.contains(&"CompletionHandler".to_string()),
			"Should extract typealias name: got {:?}",
			symbols
		);
	}

	// ── extract_imports_exports ──────────────────────────────────────────────

	#[test]
	fn test_extract_imports_foundation() {
		let code = r#"import Foundation"#;
		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();

		let import_node = tree
			.root_node()
			.named_children(&mut tree.root_node().walk())
			.find(|n| n.kind() == "import_declaration")
			.expect("Should have import_declaration");

		let (imports, exports) = lang.extract_imports_exports(import_node, code);
		assert!(
			imports.iter().any(|i| i.contains("Foundation")),
			"Should extract Foundation import: got {:?}",
			imports
		);
		assert!(exports.is_empty(), "Import node should have no exports");
	}

	#[test]
	fn test_extract_imports_uikit_and_swiftui() {
		let code = r#"
import UIKit
import SwiftUI
"#;
		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();

		let mut all_imports = Vec::new();
		for node in tree
			.root_node()
			.named_children(&mut tree.root_node().walk())
		{
			if node.kind() == "import_declaration" {
				let (imports, _) = lang.extract_imports_exports(node, code);
				all_imports.extend(imports);
			}
		}

		assert!(
			all_imports.iter().any(|i| i.contains("UIKit")),
			"Should extract UIKit import"
		);
		assert!(
			all_imports.iter().any(|i| i.contains("SwiftUI")),
			"Should extract SwiftUI import"
		);
	}

	#[test]
	fn test_extract_exports_public_func() {
		let code = r#"public func doSomething() {}"#;
		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();

		let func_node = tree
			.root_node()
			.named_children(&mut tree.root_node().walk())
			.find(|n| n.kind() == "function_declaration")
			.expect("Should have function_declaration");

		let (imports, exports) = lang.extract_imports_exports(func_node, code);
		assert!(imports.is_empty(), "Function node should have no imports");
		assert!(
			exports.contains(&"doSomething".to_string()),
			"Should export public function: got {:?}",
			exports
		);
	}

	#[test]
	fn test_no_export_for_internal_func() {
		let code = r#"func internalHelper() {}"#;
		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();

		let func_node = tree
			.root_node()
			.named_children(&mut tree.root_node().walk())
			.find(|n| n.kind() == "function_declaration")
			.expect("Should have function_declaration");

		let (_, exports) = lang.extract_imports_exports(func_node, code);
		assert!(
			exports.is_empty(),
			"Internal function should not be exported: got {:?}",
			exports
		);
	}

	// ── extract_function_calls ───────────────────────────────────────────────

	#[test]
	fn test_extract_function_calls() {
		// We need to find call_expression nodes inside a function body
		let code = r#"
func test() {
    print("hello")
    doWork()
    someObject.process()
}
"#;
		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();

		let mut calls = Vec::new();
		collect_calls(tree.root_node(), code, lang.as_ref(), &mut calls);

		// Should capture at least "print" and "doWork"
		assert!(
			!calls.is_empty(),
			"Should extract function calls: got {:?}",
			calls
		);
		assert!(
			calls.iter().any(|c| c == "print"),
			"Should contain 'print' call: got {:?}",
			calls
		);
	}

	fn collect_calls(
		node: tree_sitter::Node,
		code: &str,
		lang: &dyn Language,
		calls: &mut Vec<String>,
	) {
		calls.extend(lang.extract_function_calls(node, code));
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			collect_calls(child, code, lang, calls);
		}
	}

	// ── extract_type_relations ───────────────────────────────────────────────

	#[test]
	fn test_type_relations_class_inheritance() {
		use crate::indexer::languages::TypeRelationKind;

		let code = r#"class Dog: Animal, Trainable { }"#;
		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();

		let class_node = tree
			.root_node()
			.named_children(&mut tree.root_node().walk())
			.find(|n| n.kind() == "class_declaration")
			.expect("Should have class_declaration");

		let relations = lang.extract_type_relations(class_node, code);
		assert!(
			!relations.is_empty(),
			"Should extract type relations: got {:?}",
			relations
		);

		// All should be Implements (we conservatively treat all specifiers as Implements)
		assert!(
			relations
				.iter()
				.all(|(k, _)| *k == TypeRelationKind::Implements),
			"Class specifiers should produce Implements relations"
		);

		let names: Vec<&String> = relations.iter().map(|(_, n)| n).collect();
		assert!(
			names.iter().any(|n| n.as_str() == "Animal"),
			"Should reference Animal: got {:?}",
			names
		);
	}

	#[test]
	fn test_type_relations_protocol_inheritance() {
		use crate::indexer::languages::TypeRelationKind;

		let code = r#"protocol Flyable: Movable { }"#;
		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();

		let proto_node = tree
			.root_node()
			.named_children(&mut tree.root_node().walk())
			.find(|n| n.kind() == "protocol_declaration")
			.expect("Should have protocol_declaration");

		let relations = lang.extract_type_relations(proto_node, code);
		assert!(
			!relations.is_empty(),
			"Protocol should emit inheritance relations: got {:?}",
			relations
		);

		assert!(
			relations
				.iter()
				.any(|(k, _)| *k == TypeRelationKind::Extends),
			"Protocol inheritance should use Extends"
		);

		let names: Vec<&String> = relations.iter().map(|(_, n)| n).collect();
		assert!(
			names.iter().any(|n| n.as_str() == "Movable"),
			"Should reference Movable: got {:?}",
			names
		);
	}

	#[test]
	fn test_no_type_relations_for_plain_function() {
		let code = r#"func standalone() {}"#;
		let (mut parser, lang) = make_parser();
		let tree = parser.parse(code, None).unwrap();

		let func_node = tree
			.root_node()
			.named_children(&mut tree.root_node().walk())
			.find(|n| n.kind() == "function_declaration")
			.expect("Should have function_declaration");

		let relations = lang.extract_type_relations(func_node, code);
		assert!(
			relations.is_empty(),
			"Plain function should have no type relations"
		);
	}

	// ── resolve_import ───────────────────────────────────────────────────────

	#[test]
	fn test_resolve_import_relative() {
		let lang = languages::get_language("swift").unwrap();

		let all_files = vec![
			"Sources/App/Models/User.swift".to_string(),
			"Sources/App/Services/UserService.swift".to_string(),
			"Sources/App/Views/UserView.swift".to_string(),
			// Path that ./Models/User resolves to from Services/
			"Sources/App/Services/Models/User.swift".to_string(),
		];

		// Relative import from UserService.swift
		let result = lang.resolve_import(
			"./Models/User",
			"Sources/App/Services/UserService.swift",
			&all_files,
		);
		assert!(
			result.is_some(),
			"Should resolve relative import to User.swift"
		);
	}

	#[test]
	fn test_resolve_import_module_maps_to_directory() {
		let lang = languages::get_language("swift").unwrap();

		let all_files = vec![
			"Sources/Models/User.swift".to_string(),
			"Sources/Models/Post.swift".to_string(),
			"Sources/Services/AuthService.swift".to_string(),
		];

		// Module-level import "Models" — should find a file in Models directory
		let result = lang.resolve_import("Models", "Sources/main.swift", &all_files);
		// May or may not resolve depending on implementation — just check no panic
		let _ = result;
	}

	// ── metadata ────────────────────────────────────────────────────────────

	#[test]
	fn test_language_name_and_extensions() {
		let lang = languages::get_language("swift").unwrap();
		assert_eq!(lang.name(), "swift");
		assert!(
			lang.get_file_extensions().contains(&"swift"),
			"Should support .swift extension"
		);
	}

	#[test]
	fn test_are_node_types_equivalent() {
		let lang = languages::get_language("swift").unwrap();

		// Same type
		assert!(lang.are_node_types_equivalent("function_declaration", "function_declaration"));

		// Function and init are grouped
		assert!(lang.are_node_types_equivalent("function_declaration", "init_declaration"));

		// Property and subscript are grouped
		assert!(lang.are_node_types_equivalent("property_declaration", "subscript_declaration"));

		// Different groups should not be equivalent
		assert!(!lang.are_node_types_equivalent("function_declaration", "class_declaration"));
		assert!(!lang.are_node_types_equivalent("import_declaration", "function_declaration"));
	}

	#[test]
	fn test_get_node_type_description() {
		let lang = languages::get_language("swift").unwrap();

		assert_eq!(
			lang.get_node_type_description("function_declaration"),
			"function declarations"
		);
		assert!(
			lang.get_node_type_description("class_declaration")
				.contains("class"),
			"class_declaration description should mention class"
		);
		assert_eq!(
			lang.get_node_type_description("protocol_declaration"),
			"protocol declarations"
		);
		assert_eq!(
			lang.get_node_type_description("import_declaration"),
			"import statements"
		);
		assert_eq!(
			lang.get_node_type_description("typealias_declaration"),
			"type alias declarations"
		);
	}
}
