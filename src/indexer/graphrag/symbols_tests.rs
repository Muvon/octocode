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
	use tempfile::TempDir;

	fn declaration(name: &str, kind: &str, start_line: u32, end_line: u32) -> SymbolDecl {
		SymbolDecl {
			name: name.to_string(),
			kind: kind.to_string(),
			owner: None,
			start_line,
			end_line,
		}
	}

	fn owned(owner: &str, name: &str, start_line: u32, end_line: u32) -> SymbolDecl {
		SymbolDecl {
			owner: Some(owner.to_string()),
			..declaration(name, "function", start_line, end_line)
		}
	}

	fn call(name: &str, qualifier: Option<&str>) -> CallTarget {
		CallTarget {
			name: name.to_string(),
			qualifier: qualifier.map(str::to_string),
		}
	}

	fn file(path: &str, language: &str) -> SymbolFileData {
		SymbolFileData {
			path: path.to_string(),
			language: language.to_string(),
			imports: Vec::new(),
			import_bindings: Vec::new(),
			calls: Vec::new(),
			type_relations: Vec::new(),
			symbols: Vec::new(),
		}
	}

	/// Write `source` into a temp dir under `name` and run the single-pass
	/// extractor over it as `language`.
	fn extract(name: &str, language: &str, source: &str) -> (TempDir, FileAstData) {
		let dir = TempDir::new().unwrap();
		let path = dir.path().join(name);
		std::fs::write(&path, source).unwrap();
		let data = extract_symbols_from_file(path.to_str().unwrap(), language)
			.unwrap_or_else(|e| panic!("{language} extraction failed: {e}"));
		(dir, data)
	}

	fn binding<'a>(data: &'a FileAstData, local_name: &str) -> &'a ImportBinding {
		data.import_bindings
			.iter()
			.find(|b| b.local_name == local_name)
			.unwrap_or_else(|| {
				panic!(
					"no binding named {local_name} in {:?}",
					data.import_bindings
				)
			})
	}

	#[test]
	fn a_python_import_alias_binds_the_module_namespace() {
		let (_dir, data) = extract("mod.py", "python", "import os.path as p\n");
		let alias = binding(&data, "p");
		assert_eq!(alias.imported_name, None, "a module alias names no symbol");
		assert_eq!(alias.import_path, "os.path");
	}

	#[test]
	fn a_python_from_import_binds_each_name_with_its_alias() {
		let (_dir, data) = extract(
			"mod.py",
			"python",
			"from package.helpers import helper as h, other\n",
		);
		assert_eq!(data.import_bindings.len(), 2, "{:?}", data.import_bindings);

		let aliased = binding(&data, "h");
		assert_eq!(aliased.imported_name.as_deref(), Some("helper"));
		assert_eq!(aliased.import_path, "package.helpers");

		let plain = binding(&data, "other");
		assert_eq!(plain.imported_name.as_deref(), Some("other"));
		assert_eq!(plain.import_path, "package.helpers");
	}

	#[test]
	fn a_javascript_namespace_import_binds_the_whole_module() {
		let (_dir, data) = extract(
			"app.js",
			"javascript",
			"import * as utils from './utils';\n",
		);
		let namespace = binding(&data, "utils");
		assert_eq!(namespace.imported_name, None);
		assert_eq!(namespace.import_path, "./utils");
	}

	#[test]
	fn javascript_named_imports_bind_each_specifier() {
		let (_dir, data) = extract(
			"app.js",
			"javascript",
			"import { helper as h, other } from './utils';\n",
		);
		assert_eq!(data.import_bindings.len(), 2, "{:?}", data.import_bindings);
		assert_eq!(binding(&data, "h").imported_name.as_deref(), Some("helper"));
		assert_eq!(
			binding(&data, "other").imported_name.as_deref(),
			Some("other")
		);
		assert_eq!(binding(&data, "h").import_path, "./utils");
	}

	#[test]
	fn a_rust_use_alias_binds_the_original_item_name() {
		let (_dir, data) = extract(
			"lib.rs",
			"rust",
			"use crate::helpers::helper as h;\n\nfn main() {}\n",
		);
		let alias = binding(&data, "h");
		assert_eq!(alias.imported_name.as_deref(), Some("helper"));
		assert_eq!(alias.import_path, "crate::helpers::helper");
	}

	#[test]
	fn a_go_named_import_binds_the_package_alias() {
		let (_dir, data) = extract(
			"main.go",
			"go",
			"package main\n\nimport (\n\tpkg \"example.com/lib/pkg\"\n)\n",
		);
		let alias = binding(&data, "pkg");
		assert_eq!(alias.imported_name, None);
		assert_eq!(alias.import_path, "example.com/lib/pkg");
	}

	#[test]
	fn an_import_without_an_alias_produces_no_binding() {
		// The Rust arm only fires on `use ... as ...`; a plain import is left to
		// path-based resolution.
		let (_dir, data) = extract("lib.rs", "rust", "use std::fmt;\n\nfn main() {}\n");
		assert!(
			data.imports.contains(&"std::fmt".to_string()),
			"{:?}",
			data.imports
		);
		assert!(
			data.import_bindings.is_empty(),
			"{:?}",
			data.import_bindings
		);
	}

	#[test]
	fn extraction_reports_an_unknown_language_and_an_unreadable_file() {
		let dir = TempDir::new().unwrap();
		let path = dir.path().join("a.rs");
		std::fs::write(&path, "fn main() {}").unwrap();

		let err = extract_symbols_from_file(path.to_str().unwrap(), "klingon").unwrap_err();
		assert_eq!(
			err.to_string(),
			"Failed to get language implementation for: klingon"
		);

		let missing = dir.path().join("gone.rs");
		let err = extract_symbols_from_file(missing.to_str().unwrap(), "rust").unwrap_err();
		assert!(
			err.to_string()
				.starts_with("Failed to read file for AST extraction:"),
			"{err}"
		);
	}

	#[test]
	fn node_kinds_map_to_coarse_symbol_kinds() {
		assert_eq!(
			symbol_kind_from_node_kind("procedure_declaration"),
			"function"
		);
		assert_eq!(
			symbol_kind_from_node_kind("constructor_declaration"),
			"function"
		);
		assert_eq!(symbol_kind_from_node_kind("init_declaration"), "function");
		assert_eq!(symbol_kind_from_node_kind("namespace_definition"), "module");
		assert_eq!(symbol_kind_from_node_kind("package_clause"), "module");
		assert_eq!(symbol_kind_from_node_kind("variable_declarator"), "const");
		// `starts_with("init")`, not `contains`: "definition" contains "init" and
		// must not be classified as a function.
		assert_eq!(symbol_kind_from_node_kind("definition"), "symbol");
		// "interface" is tested before "class", so a hybrid kind reads as an interface.
		assert_eq!(
			symbol_kind_from_node_kind("interface_class_body"),
			"interface"
		);
	}

	#[test]
	fn symbol_ids_qualify_methods_by_owner_and_disambiguate_only_real_collisions() {
		let symbols = vec![
			owned("Service", "run", 0, 5),
			owned("Worker", "run", 10, 15),
			owned("Service", "run", 20, 25),
			declaration("free_fn", "function", 30, 31),
		];
		assert_eq!(
			symbol_node_ids("src/s.rs", &symbols),
			vec![
				"src/s.rs::Service::run@1",
				"src/s.rs::Worker::run",
				"src/s.rs::Service::run@21",
				"src/s.rs::free_fn",
			]
		);
	}

	#[test]
	fn from_ast_carries_every_relationship_input_for_one_file() {
		let ast = FileAstData {
			imports: vec!["./dep".to_string()],
			import_bindings: vec![ImportBinding {
				local_name: "d".to_string(),
				imported_name: Some("dep".to_string()),
				import_path: "./dep".to_string(),
			}],
			exports: vec!["main".to_string()],
			calls: vec![(3, call("dep", None))],
			type_relations: vec![TypeRelationDecl {
				line: 1,
				source_name: Some("App".to_string()),
				kind: TypeRelationKind::Implements,
				target_name: "Runnable".to_string(),
			}],
			symbols: vec![declaration("main", "function", 0, 5)],
		};

		let data =
			SymbolFileData::from_ast("src/app.js".to_string(), "javascript".to_string(), &ast);
		assert_eq!(data.path, "src/app.js");
		assert_eq!(data.language, "javascript");
		assert_eq!(data.imports, ast.imports);
		assert_eq!(data.import_bindings.len(), 1);
		assert_eq!(data.import_bindings[0].local_name, "d");
		assert_eq!(data.calls.len(), 1);
		assert_eq!(data.calls[0].1.name, "dep");
		assert_eq!(data.type_relations.len(), 1);
		assert_eq!(data.type_relations[0].target_name, "Runnable");
		assert_eq!(data.symbols.len(), 1);
		assert_eq!(data.symbols[0].name, "main");
	}

	#[test]
	fn a_function_calling_itself_gets_a_self_edge() {
		let mut source = file("src/r.rs", "rust");
		source.symbols = vec![declaration("recurse", "function", 0, 5)];
		source.calls = vec![(2, call("recurse", None))];

		let edges = discover_symbol_relationships(&[source]);
		assert_eq!(edges.len(), 1, "{edges:?}");
		assert_eq!(edges[0].source, "src/r.rs::recurse");
		assert_eq!(edges[0].target, "src/r.rs::recurse");
		assert_eq!(edges[0].relation_type, RelationType::Calls);
		assert_eq!(edges[0].description, "recurse calls itself");
		assert_eq!(edges[0].confidence, 1.0);
		assert_eq!(edges[0].provenance, Provenance::Extracted);
	}

	#[test]
	fn markdown_and_unknown_languages_are_skipped_entirely() {
		let mut markdown = file("docs/readme.md", "markdown");
		markdown.symbols = vec![declaration("Heading", "symbol", 0, 2)];
		markdown.calls = vec![(1, call("Heading", None))];

		let mut alien = file("src/a.klingon", "klingon");
		alien.symbols = vec![declaration("nuqneH", "function", 0, 2)];
		alien.calls = vec![(1, call("nuqneH", None))];

		assert!(discover_symbol_relationships(&[markdown, alien]).is_empty());
	}

	#[test]
	fn a_call_resolves_to_the_imported_file_rather_than_an_ambiguous_global() {
		// `helper` is declared twice project-wide, so only the import scope can
		// disambiguate it.
		let mut main = file("src/main.js", "javascript");
		main.imports = vec!["./utils".to_string()];
		main.calls = vec![(2, call("helper", None))];
		main.symbols = vec![declaration("main", "function", 0, 5)];

		let mut utils = file("src/utils.js", "javascript");
		utils.symbols = vec![declaration("helper", "function", 0, 1)];

		let mut other = file("src/other.js", "javascript");
		other.symbols = vec![declaration("helper", "function", 0, 1)];

		let edges = discover_symbol_relationships(&[main, utils, other]);
		let resolved: Vec<_> = edges
			.iter()
			.filter(|edge| edge.source == "src/main.js::main")
			.collect();
		assert_eq!(resolved.len(), 1, "{resolved:?}");
		assert_eq!(resolved[0].target, "src/utils.js::helper");
		assert_eq!(resolved[0].provenance, Provenance::Extracted);
		assert_eq!(resolved[0].confidence, 0.85);
	}

	#[test]
	fn an_ambiguous_same_file_call_produces_no_edge() {
		// Two overloads of `target` in the same file: the caller cannot be
		// attributed to either, and calls never fan out to both.
		let mut source = file("src/dup.rs", "rust");
		source.symbols = vec![
			declaration("caller", "function", 0, 5),
			declaration("target", "function", 10, 12),
			declaration("target", "function", 20, 22),
		];
		source.calls = vec![(2, call("target", None))];

		assert!(discover_symbol_relationships(&[source]).is_empty());
	}
}
