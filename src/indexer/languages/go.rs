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

//! Go language implementation for the indexer

use crate::indexer::code_region_extractor::{combine_with_preceding_comments, CodeRegion};
use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct Go {}

impl Language for Go {
	fn name(&self) -> &'static str {
		"go"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_go::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec![
			"function_declaration",
			"method_declaration",
			"type_declaration",
			"const_declaration",
			"var_declaration",
			"import_declaration",
		]
	}

	fn expand_meaningful_node(&self, node: Node, contents: &str) -> Option<Vec<CodeRegion>> {
		// The grouped form (`const ( A = 1\nB = 2\n)`) wraps an unbounded
		// repeat() of spec children in ONE node — a real generated file can
		// have thousands in one node. The ungrouped single-declaration form
		// (`const X = 1`) produces exactly one spec child in the SAME node
		// kind, so splitting unconditionally would strip the `const`/`var`/
		// `type` keyword prefix from the overwhelmingly common case. Only
		// split when there is more than one spec child.
		let spec_kind = match node.kind() {
			"const_declaration" => "const_spec",
			"var_declaration" => "var_spec",
			"type_declaration" => "type_spec",
			_ => return None,
		};
		// Grouped `var (...)` wraps its specs one level deeper in a
		// var_spec_list node; const_spec/type_spec are direct children either way.
		let spec_container = node
			.children(&mut node.walk())
			.find(|c| c.kind() == "var_spec_list")
			.unwrap_or(node);
		let specs: Vec<Node> = spec_container
			.children(&mut spec_container.walk())
			.filter(|c| c.kind() == spec_kind)
			.collect();
		if specs.len() <= 1 {
			// Ungrouped single-declaration form: keep today's behavior of one
			// region including the const/var/type keyword.
			return None;
		}
		let mut sub_regions = Vec::new();
		for spec in specs {
			let (content, start_line) = combine_with_preceding_comments(spec, contents, None);
			if content.trim().is_empty() {
				continue;
			}
			let mut symbols = self.extract_symbols(spec, contents);
			if symbols.is_empty() {
				symbols.push(format!("{}_{}", spec.kind(), start_line));
			}
			sub_regions.push(CodeRegion {
				content,
				symbols,
				start_line,
				end_line: spec.end_position().row,
				node_kind: spec.kind().to_string(),
				node_id: spec.id(),
			});
		}
		(!sub_regions.is_empty()).then_some(sub_regions)
	}

	fn get_symbol_kinds(&self) -> Vec<&'static str> {
		// `type_spec` instead of `type_declaration`: the name lives two levels
		// deep (type_declaration > type_spec > identifier), so per-spec nodes
		// are the only way the default name scan resolves them — and grouped
		// declarations (`type ( A ...; B ... )`) yield one symbol per spec.
		// const/var/import declarations declare no single named symbol.
		vec!["function_declaration", "method_declaration", "type_spec"]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"function_declaration" | "method_declaration" => {
				// Extract function or method name
				if let Some(name) = super::extract_symbol_by_kinds(
					node,
					contents,
					&["identifier", "field_identifier"],
				) {
					symbols.push(name);
				}

				// Extract variables declared in function body
				for child in node.children(&mut node.walk()) {
					if child.kind() == "block" {
						self.extract_go_variables(child, contents, &mut symbols);
						break;
					}
				}
			}
			"type_declaration" => {
				// Extract type name. A Go type name is a `type_identifier`, so an
				// exact match on "identifier" never fires.
				for child in node.children(&mut node.walk()) {
					if child.kind() == "type_spec" {
						if let Some(name) =
							super::extract_symbol_by_kind(child, contents, "type_identifier")
						{
							symbols.push(name);
						}
						break;
					}
				}
			}
			"struct_type" | "interface_type" => {
				// Extract field names within structs or interfaces
				self.extract_struct_interface_fields(node, contents, &mut symbols);
			}
			"const_spec" | "var_spec" => {
				// A spec can declare several comma-separated names
				// (`a, b = 1, 2`); each appears as its own direct "identifier"
				// child rather than wrapped in a list node.
				for child in node.children(&mut node.walk()) {
					if child.kind() == "identifier" {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							symbols.push(name.to_string());
						}
					}
				}
			}
			"type_spec" => {
				if let Some(name) = super::extract_symbol_by_kind(node, contents, "type_identifier")
				{
					symbols.push(name);
				}
			}
			_ => self.extract_identifiers(node, contents, &mut symbols),
		}

		super::deduplicate_symbols(&mut symbols);
		symbols
	}

	fn extract_identifiers(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		super::extract_identifiers_default(node, contents, symbols, |kind, _text| {
			// Include identifiers and field identifiers
			kind == "identifier" || kind == "field_identifier"
		});
	}

	fn are_node_types_equivalent(&self, type1: &str, type2: &str) -> bool {
		// Go-specific semantic groups
		let semantic_groups = [
			// Functions and methods
			&["function_declaration", "method_declaration"] as &[&str],
			// Type definitions
			&["type_declaration", "struct_type", "interface_type"],
			// Variable and constant declarations
			&[
				"var_declaration",
				"const_declaration",
				"short_var_declaration",
			],
			// Import statements
			&["import_declaration"],
		];

		super::check_semantic_groups(type1, type2, &semantic_groups)
	}

	fn get_node_type_description(&self, node_type: &str) -> &'static str {
		match node_type {
			"function_declaration" | "method_declaration" => "function declarations",
			"type_declaration" => "type declarations",
			"struct_type" => "struct definitions",
			"interface_type" => "interface definitions",
			"var_declaration" | "const_declaration" | "short_var_declaration" => {
				"variable declarations"
			}
			"import_declaration" => "import statements",
			_ => "declarations",
		}
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		let mut imports = Vec::new();
		let mut exports = Vec::new();

		match node.kind() {
			"import_declaration" => {
				// Handle: import "package"
				// Handle: import alias "package"
				// Handle: import ( "package1"; "package2" )
				if let Ok(import_text) = node.utf8_text(contents.as_bytes()) {
					if let Some(imported_items) = parse_go_import_statement(import_text) {
						imports.extend(imported_items);
					}
				}
			}
			"function_declaration"
			| "method_declaration"
			| "type_declaration"
			| "const_declaration"
			| "var_declaration" => {
				// In Go, exported items start with an uppercase letter. Func and
				// method names are direct children, but a type/var/const name sits
				// one level down inside its spec node, so both levels are scanned.
				let mut push_if_exported = |node: Node| {
					for child in node.children(&mut node.walk()) {
						if matches!(
							child.kind(),
							"identifier" | "field_identifier" | "type_identifier"
						) {
							if let Ok(name) = child.utf8_text(contents.as_bytes()) {
								if name.chars().next().is_some_and(|c| c.is_uppercase()) {
									exports.push(name.to_string());
								}
								break;
							}
						}
					}
				};

				push_if_exported(node);
				for child in node.children(&mut node.walk()) {
					if matches!(child.kind(), "type_spec" | "var_spec" | "const_spec") {
						push_if_exported(child);
					}
				}
			}
			_ => {}
		}

		(imports, exports)
	}

	fn extract_function_calls(&self, node: Node, contents: &str) -> Vec<super::CallTarget> {
		if node.kind() == "call_expression" {
			if let Some(func_node) = node.child(0) {
				if let Ok(text) = func_node.utf8_text(contents.as_bytes()) {
					return super::extract_call_target(text).into_iter().collect();
				}
			}
		}
		Vec::new()
	}

	fn extract_symbol_owner(&self, node: Node, contents: &str) -> Option<String> {
		if node.kind() != "method_declaration" {
			return super::find_graph_symbol_owner(node, contents);
		}
		let receiver = node.child_by_field_name("receiver")?;
		for parameter in receiver.children(&mut receiver.walk()) {
			if let Some(receiver_type) = parameter.child_by_field_name("type") {
				if let Ok(text) = receiver_type.utf8_text(contents.as_bytes()) {
					return super::simple_type_name(text);
				}
			}
		}
		None
	}

	fn extract_type_relations(
		&self,
		node: Node,
		contents: &str,
	) -> Vec<(super::TypeRelationKind, String)> {
		// Go has no `extends`/`implements`, but struct embedding —
		// `type Foo struct { Bar }` — is the idiomatic equivalent of
		// inheritance/composition and is the most useful edge to capture.
		// Interface implementation is structural (no syntactic hook); not emitted.
		let mut out = Vec::new();
		if node.kind() == "field_declaration" {
			let has_named_field = node.child_by_field_name("name").is_some();
			if !has_named_field {
				if let Some(type_node) = node.child_by_field_name("type") {
					if let Ok(text) = type_node.utf8_text(contents.as_bytes()) {
						if let Some(name) = super::simple_type_name(text) {
							out.push((super::TypeRelationKind::Extends, name));
						}
					}
				}
			}
		}
		out
	}

	fn resolve_import(
		&self,
		import_path: &str,
		source_file: &str,
		all_files: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		let registry = all_files;
		let go_files = registry.get_files_with_extensions(&self.get_file_extensions());

		if import_path.starts_with("./") || import_path.starts_with("../") {
			// Relative import
			self.resolve_relative_import(import_path, source_file, &go_files)
		} else {
			// Absolute import - look for package directory
			self.resolve_package_import(import_path, &go_files)
		}
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["go"]
	}
}

impl Go {
	/// Extract variable declarations in Go blocks
	#[allow(clippy::only_used_in_recursion)]
	fn extract_go_variables(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		// Traverse the block looking for variable declarations
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				let child = cursor.node();

				match child.kind() {
					"short_var_declaration" => {
						// Handle short variables like x := 10
						for var_child in child.children(&mut child.walk()) {
							if var_child.kind() == "expression_list" {
								for expr in var_child.children(&mut var_child.walk()) {
									if expr.kind() == "identifier" {
										if let Ok(name) = expr.utf8_text(contents.as_bytes()) {
											if !symbols.iter().any(|s| s.as_str() == name) {
												symbols.push(name.to_string());
											}
										}
									}
								}
								break; // Only process the left side of :=
							}
						}
					}
					"var_declaration" => {
						// Handle var x = 10 or var x int = 10
						for spec in child.children(&mut child.walk()) {
							if spec.kind() == "var_spec" {
								for spec_child in spec.children(&mut spec.walk()) {
									if spec_child.kind() == "identifier" {
										if let Ok(name) = spec_child.utf8_text(contents.as_bytes())
										{
											if !symbols.iter().any(|s| s.as_str() == name) {
												symbols.push(name.to_string());
											}
										}
									}
								}
							}
						}
					}
					"const_declaration" => {
						// Handle const declarations
						for spec in child.children(&mut child.walk()) {
							if spec.kind() == "const_spec" {
								for spec_child in spec.children(&mut spec.walk()) {
									if spec_child.kind() == "identifier" {
										if let Ok(name) = spec_child.utf8_text(contents.as_bytes())
										{
											if !symbols.iter().any(|s| s.as_str() == name) {
												symbols.push(name.to_string());
											}
										}
									}
								}
							}
						}
					}
					"block" | "statement_list" => {
						// A block's statements are wrapped in a `statement_list`, so
						// without descending through it no body declaration is ever
						// reached.
						self.extract_go_variables(child, contents, symbols);
					}
					"if_statement" | "for_statement" => {
						// Process blocks inside control structures.
						for stmt_child in child.children(&mut child.walk()) {
							if stmt_child.kind() == "block" {
								self.extract_go_variables(stmt_child, contents, symbols);
							}
						}
					}
					"labeled_statement" => {
						self.extract_go_variables(child, contents, symbols);
					}
					"expression_switch_statement" | "type_switch_statement" => {
						// Switch statements hold case clauses (and an optional init
						// statement) directly — there is no nested "block" — so
						// recurse into the switch node itself; the case arms below
						// then handle each case body.
						self.extract_go_variables(child, contents, symbols);
					}
					"expression_case" | "default_case" | "type_case" => {
						// Switch case bodies hold statements directly (not wrapped in
						// a nested "block"), so recurse straight into the case node.
						self.extract_go_variables(child, contents, symbols);
					}
					_ => {}
				}

				if !cursor.goto_next_sibling() {
					break;
				}
			}
		}
	}

	/// Extract field names from struct or interface types
	#[allow(clippy::only_used_in_recursion)]
	fn extract_struct_interface_fields(
		&self,
		node: Node,
		contents: &str,
		symbols: &mut Vec<String>,
	) {
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				let child = cursor.node();

				// Fields and method specs hang off the body list, never directly
				// off the struct/interface node itself.
				if matches!(child.kind(), "field_declaration_list" | "interface_type") {
					self.extract_struct_interface_fields(child, contents, symbols);
				}

				if child.kind() == "field_declaration" {
					for field_child in child.children(&mut child.walk()) {
						if field_child.kind() == "field_identifier" {
							if let Ok(name) = field_child.utf8_text(contents.as_bytes()) {
								if !symbols.iter().any(|s| s.as_str() == name) {
									symbols.push(name.to_string());
								}
							}
						}
					}
				} else if matches!(child.kind(), "method_spec" | "method_elem") {
					// For interface methods; the grammar names them `method_elem`.
					for method_child in child.children(&mut child.walk()) {
						if method_child.kind() == "field_identifier" {
							if let Ok(name) = method_child.utf8_text(contents.as_bytes()) {
								if !symbols.iter().any(|s| s.as_str() == name) {
									symbols.push(name.to_string());
								}
							}
						}
					}
				}

				if !cursor.goto_next_sibling() {
					break;
				}
			}
		}
	}
}
// Helper function for parsing Go import statements.
// Returns the full import path (e.g. "github.com/user/repo/pkg") so that
// resolve_package_import can match it against directory path suffixes.
// For aliased imports (e.g. `alias "pkg/path"`) we still return the full path
// because the resolver needs the path, not the alias.
fn parse_go_import_statement(import_text: &str) -> Option<Vec<String>> {
	let mut imports = Vec::new();
	let cleaned = import_text.trim();

	// Handle single import: import "package" or import alias "package"
	if cleaned.starts_with("import ") && !cleaned.contains('(') {
		let rest = cleaned[7..].trim(); // Skip "import "
								  // Strip a trailing "// comment" so it doesn't get counted as extra tokens
		let rest = rest.split("//").next().unwrap_or(rest).trim();

		let parts: Vec<&str> = rest.split_whitespace().collect();
		let raw_path = if parts.len() == 2 {
			// import alias "path" — take the quoted path (parts[1])
			parts[1].trim_matches('"')
		} else if parts.len() == 1 {
			parts[0].trim_matches('"')
		} else {
			return None;
		};
		if !raw_path.is_empty() {
			imports.push(raw_path.to_string());
		}
		return Some(imports);
	}

	// Handle grouped imports: import ( ... )
	if cleaned.contains('(') && cleaned.contains(')') {
		if let Some(start) = cleaned.find('(') {
			if let Some(end) = cleaned.rfind(')') {
				let imports_block = &cleaned[start + 1..end];
				for line in imports_block.lines() {
					let line = line.trim();
					if line.is_empty() || line.starts_with("//") {
						continue;
					}
					// Strip a trailing "// comment" (e.g. blank imports: `_ "pq" // driver`)
					let line = line.split("//").next().unwrap_or(line).trim();

					// Handle: alias "package" or "package"
					let parts: Vec<&str> = line.split_whitespace().collect();
					let raw_path = if parts.len() == 2 {
						// alias "path" — take the quoted path
						parts[1].trim_matches('"')
					} else if parts.len() == 1 {
						parts[0].trim_matches('"')
					} else {
						continue;
					};
					if !raw_path.is_empty() {
						imports.push(raw_path.to_string());
					}
				}
				return Some(imports);
			}
		}
	}

	None
}

impl Go {
	/// Resolve relative imports in Go
	fn resolve_relative_import(
		&self,
		import_path: &str,
		source_file: &str,
		go_files: &[String],
	) -> Option<String> {
		use super::resolution_utils::resolve_relative_path;

		let relative_path = resolve_relative_path(source_file, import_path)?;

		// Look for any .go file in the target directory
		for go_file in go_files {
			let file_path = std::path::Path::new(go_file);
			if let Some(file_dir) = file_path.parent() {
				if file_dir == relative_path {
					return Some(go_file.clone());
				}
			}
		}

		None
	}

	/// Resolve package imports by matching the full import path against directory path suffixes.
	/// e.g. "github.com/user/repo/pkg" matches any go file whose parent dir ends with "user/repo/pkg".
	fn resolve_package_import(&self, import_path: &str, go_files: &[String]) -> Option<String> {
		// Normalize separators for cross-platform comparison
		let normalized_import = import_path.replace('\\', "/");
		for go_file in go_files {
			let file_path = std::path::Path::new(go_file);
			if let Some(file_dir) = file_path.parent() {
				let dir_str = file_dir.to_string_lossy().replace('\\', "/");
				// Match if the directory path ends with the import path (or its last segment)
				if dir_str.ends_with(&normalized_import)
					|| dir_str.ends_with(
						normalized_import
							.split('/')
							.next_back()
							.unwrap_or(&normalized_import),
					) {
					return Some(go_file.clone());
				}
			}
		}

		None
	}
}
