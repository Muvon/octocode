// Copyright 2025 Muvon Un Limited
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

//! Java language implementation for the indexer

use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct Java {}

impl Language for Java {
	fn name(&self) -> &'static str {
		"java"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_java::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec![
			// Individual method-level constructs (following pattern from other languages)
			"method_declaration",
			"constructor_declaration",
			// Removed: "class_declaration" - too large, not semantic
			// Removed: "interface_declaration" - too large, not semantic
			// Removed: "enum_declaration" - too large, not semantic
			// Individual methods inside classes/interfaces/enums will be captured separately
			"annotation_type_declaration", // Keep for small annotation definitions
			"record_declaration",          // Java 14+ - usually small and semantic
			// Single-line statements that will be merged by smart merging
			"import_declaration",
			"package_declaration",
			"field_declaration",
			// Lambda expressions and method references for modern Java
			"lambda_expression",
			"method_reference",
		]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"class_declaration"
			| "interface_declaration"
			| "enum_declaration"
			| "annotation_type_declaration"
			| "record_declaration" => {
				// Extract class/interface/enum/annotation/record name
				if let Some(name) = super::extract_symbol_by_kind(node, contents, "identifier") {
					symbols.push(name);
				}
			}
			"method_declaration" | "constructor_declaration" => {
				// Extract method or constructor name
				if let Some(name) = super::extract_symbol_by_kind(node, contents, "identifier") {
					symbols.push(name);
				}
			}
			"lambda_expression" => {
				// For lambda expressions, mark as lambda
				symbols.push("<lambda>".to_string());
			}
			"method_reference" => {
				// For method references, extract the referenced method if possible
				if let Ok(method_ref) = node.utf8_text(contents.as_bytes()) {
					symbols.push(method_ref.to_string());
				}
			}
			_ => {
				// For other nodes, don't recurse to avoid infinite loops
				// Just try to extract direct identifiers if this is an identifier node
				if node.kind() == "identifier" {
					if let Ok(name) = node.utf8_text(contents.as_bytes()) {
						symbols.push(name.to_string());
					}
				}
			}
		}

		super::deduplicate_symbols(&mut symbols);
		symbols
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		let mut imports = Vec::new();
		let mut exports = Vec::new();

		match node.kind() {
			"import_declaration" => {
				if let Ok(import_text) = node.utf8_text(contents.as_bytes()) {
					// Clean up import statement
					let import_path = import_text
						.trim()
						.strip_prefix("import")
						.unwrap_or(import_text)
						.strip_prefix("static")
						.unwrap_or(import_text.strip_prefix("import").unwrap_or(import_text))
						.trim()
						.trim_end_matches(';')
						.trim();
					if !import_path.is_empty() {
						imports.push(import_path.to_string());
					}
				}
			}
			"package_declaration" => {
				// Package declaration defines the current module's namespace
				for child in node.children(&mut node.walk()) {
					if child.kind() == "scoped_identifier" || child.kind() == "identifier" {
						if let Ok(package_name) = child.utf8_text(contents.as_bytes()) {
							exports.push(format!("package:{}", package_name));
							break;
						}
					}
				}
			}
			"class_declaration"
			| "interface_declaration"
			| "enum_declaration"
			| "annotation_type_declaration"
			| "record_declaration" => {
				// Check if this is a public declaration (exported)
				let mut is_public = false;
				let mut type_name = String::new();

				for child in node.children(&mut node.walk()) {
					if child.kind() == "modifiers" {
						if let Ok(modifiers_text) = child.utf8_text(contents.as_bytes()) {
							if modifiers_text.contains("public") {
								is_public = true;
							}
						}
					} else if child.kind() == "identifier" {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							type_name = name.to_string();
						}
					}
				}

				if is_public && !type_name.is_empty() {
					let type_kind = match node.kind() {
						"class_declaration" => "class",
						"interface_declaration" => "interface",
						"enum_declaration" => "enum",
						"annotation_type_declaration" => "annotation",
						"record_declaration" => "record",
						_ => "type",
					};
					exports.push(format!("{}:{}", type_kind, type_name));
				}
			}
			"method_declaration" => {
				// Check if this is a public method (exported)
				let mut is_public = false;
				let mut method_name = String::new();

				for child in node.children(&mut node.walk()) {
					if child.kind() == "modifiers" {
						if let Ok(modifiers_text) = child.utf8_text(contents.as_bytes()) {
							if modifiers_text.contains("public") {
								is_public = true;
							}
						}
					} else if child.kind() == "identifier" {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							method_name = name.to_string();
						}
					}
				}

				if is_public && !method_name.is_empty() {
					exports.push(format!("method:{}", method_name));
				}
			}
			_ => {}
		}

		(imports, exports)
	}

	fn are_node_types_equivalent(&self, type1: &str, type2: &str) -> bool {
		// Java-specific equivalences for better merging
		// Keep this simple to avoid infinite recursion
		match (type1, type2) {
			// Single-line statements that should be merged together
			("import_declaration", "package_declaration")
			| ("package_declaration", "import_declaration") => true,
			("field_declaration", "import_declaration")
			| ("import_declaration", "field_declaration") => true,
			("field_declaration", "package_declaration")
			| ("package_declaration", "field_declaration") => true,
			// Same types are equivalent
			("field_declaration", "field_declaration") => true,
			("import_declaration", "import_declaration") => true,
			("package_declaration", "package_declaration") => true,
			// Methods and constructors should NOT be merged - keep them separate
			// annotation_type_declaration and record_declaration should NOT be merged
			// Default: exact match only
			_ => type1 == type2,
		}
	}

	fn get_node_type_description(&self, node_type: &str) -> &'static str {
		match node_type {
			"class_declaration" => "Java class definition",
			"interface_declaration" => "Java interface definition",
			"enum_declaration" => "Java enum definition",
			"method_declaration" => "Java method definition",
			"constructor_declaration" => "Java constructor definition",
			"field_declaration" => "Java field declaration",
			"annotation_type_declaration" => "Java annotation type definition",
			"record_declaration" => "Java record definition (Java 14+)",
			"import_declaration" => "Java import statement",
			"package_declaration" => "Java package declaration",
			"lambda_expression" => "Java lambda expression",
			"method_reference" => "Java method reference",
			_ => "Java code element",
		}
	}

	fn extract_identifiers(&self, node: Node, contents: &str, java_files: &mut Vec<String>) {
		super::extract_identifiers_default(node, contents, java_files, |kind, text| {
			// Include identifiers with length > 1 (avoid single-char variables)
			kind == "identifier" && text.len() > 1
		});
	}

	fn resolve_import(
		&self,
		import_path: &str,
		_current_file: &str,
		_java_files: &[String],
	) -> Option<String> {
		// Java import resolution - convert import statements to file paths
		// For example: java.util.List -> java/util/List.java
		if import_path.contains('.') && !import_path.ends_with('*') {
			Some(format!("{}.java", import_path.replace('.', "/")))
		} else {
			None
		}
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["java"]
	}
}
