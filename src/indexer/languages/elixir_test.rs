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

use super::{get_language, resolution_utils, CallTarget, TypeRelationKind};
use crate::grep::language_from_extension;
use crate::indexer::code_region_extractor::{extract_meaningful_regions, CodeRegion};
use crate::indexer::file_utils::FileUtils;
use crate::indexer::signature_extractor::extract_signatures;
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

const SOURCE: &str = r#"
defmodule MyApp.Accounts do
  alias MyApp.{Repo, User}
  import MyApp.Validation
  require Logger
  use MyApp.Telemetry

  defstruct [:name, :email]

  def fetch_user(id) when is_integer(id) do
    Repo.get(User, id)
  end

  defp validate(user), do: MyApp.Validation.call(user)

  defmacro active(query) do
    quote(do: where(unquote(query), [u], u.active))
  end
end

defprotocol MyApp.Renderable do
  def render(value)
end

defimpl MyApp.Renderable, for: MyApp.User do
  def render(user), do: user.name
end
"#;

fn parse(source: &str) -> Tree {
	let language = get_language("elixir").expect("Elixir language should be registered");
	let mut parser = Parser::new();
	parser
		.set_language(&language.get_ts_language())
		.expect("Elixir grammar should load");
	parser
		.parse(source, None)
		.expect("Elixir source should parse")
}

fn walk_calls<F>(node: Node, visit: &mut F)
where
	F: FnMut(Node),
{
	if node.kind() == "call" || node.kind() == "binary_operator" {
		visit(node);
	}
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		walk_calls(child, visit);
	}
}

#[test]
fn registers_and_detects_elixir_extensions() {
	let language = get_language("elixir").expect("Elixir language should be available");
	assert_eq!(language.name(), "elixir");
	assert_eq!(language.get_file_extensions(), vec!["ex", "exs"]);
	for file in ["lib/accounts.ex", "mix.exs", "test/accounts_test.exs"] {
		assert_eq!(FileUtils::detect_language(Path::new(file)), Some("elixir"));
		assert_eq!(
			resolution_utils::detect_language_from_path(file).as_deref(),
			Some("elixir")
		);
		assert_eq!(language_from_extension(Path::new(file)), Some("elixir"));
	}
}

#[test]
fn parses_realistic_modules_without_errors() {
	let tree = parse(SOURCE);
	assert!(!tree.root_node().has_error(), "{:#?}", tree.root_node());
}

#[test]
fn chunks_declarations_without_chunking_ordinary_calls_or_whole_modules() {
	let language = get_language("elixir").unwrap();
	let tree = parse(SOURCE);
	let mut regions: Vec<CodeRegion> = Vec::new();
	extract_meaningful_regions(tree.root_node(), SOURCE, language.as_ref(), &mut regions);

	assert_eq!(
		regions.len(),
		3,
		"regions: {:?}",
		regions.iter().map(|r| &r.content).collect::<Vec<_>>()
	);
	assert!(regions
		.iter()
		.any(|region| region.content.contains("def fetch_user")));
	assert!(regions
		.iter()
		.any(|region| region.content.starts_with("defmacro active")));
	assert!(regions
		.iter()
		.any(|region| region.content.contains("defstruct")));
	assert!(!regions
		.iter()
		.any(|region| region.content.starts_with("defmodule")));
	assert!(!regions
		.iter()
		.any(|region| region.content.starts_with("Repo.get")));
}

#[test]
fn extracts_semantic_signatures_and_declaration_kinds() {
	let language = get_language("elixir").unwrap();
	let tree = parse(SOURCE);
	let signatures = extract_signatures(tree.root_node(), SOURCE, language.as_ref());
	let summary: Vec<_> = signatures
		.iter()
		.map(|signature| (signature.kind.as_str(), signature.name.as_str()))
		.collect();

	assert!(summary.contains(&("module", "MyApp.Accounts")));
	assert!(summary.contains(&("function", "fetch_user")));
	assert!(summary.contains(&("function", "validate")));
	assert!(summary.contains(&("macro", "active")));
	assert!(summary.contains(&("interface", "MyApp.Renderable")));
	assert!(summary.contains(&("implementation", "MyApp.Renderable for MyApp.User")));
	assert_eq!(
		summary.iter().filter(|(_, name)| *name == "render").count(),
		2
	);
}

#[test]
fn extracts_imports_exports_calls_owners_and_protocol_relations() {
	let language = get_language("elixir").unwrap();
	let tree = parse(SOURCE);
	let mut imports = Vec::new();
	let mut exports = Vec::new();
	let mut calls: Vec<CallTarget> = Vec::new();
	let mut declarations = Vec::new();
	let mut relations = Vec::new();

	walk_calls(tree.root_node(), &mut |node| {
		let (node_imports, node_exports) = language.extract_imports_exports(node, SOURCE);
		imports.extend(node_imports);
		exports.extend(node_exports);
		calls.extend(language.extract_function_calls(node, SOURCE));
		if let Some(name) = language.extract_declaration_name(node, SOURCE) {
			declarations.push((name, language.extract_symbol_owner(node, SOURCE)));
		}
		for (kind, target) in language.extract_type_relations(node, SOURCE) {
			relations.push((
				kind,
				target,
				language.extract_type_relation_source(node, SOURCE),
			));
		}
	});

	imports.sort();
	imports.dedup();
	assert_eq!(
		imports,
		vec![
			"Logger",
			"MyApp.Repo",
			"MyApp.Telemetry",
			"MyApp.User",
			"MyApp.Validation"
		]
	);
	assert!(exports.contains(&"MyApp.Accounts".to_string()));
	assert!(exports.contains(&"fetch_user".to_string()));
	assert!(!exports.contains(&"validate".to_string()));
	assert!(calls.contains(&CallTarget {
		name: "get".to_string(),
		qualifier: Some("Repo".to_string())
	}));
	assert!(calls.contains(&CallTarget {
		name: "call".to_string(),
		qualifier: Some("MyApp::Validation".to_string())
	}));
	assert!(!calls
		.iter()
		.any(|call| call.name == "fetch_user" || call.name == "def"));
	assert!(declarations.contains(&("fetch_user".to_string(), Some("MyApp.Accounts".to_string()))));
	assert_eq!(
		relations,
		vec![(
			TypeRelationKind::Implements,
			"MyApp.Renderable".to_string(),
			Some("MyApp.User".to_string())
		)]
	);
}

#[test]
fn resolves_mix_module_paths() {
	let language = get_language("elixir").unwrap();
	let files = vec![
		"lib/my_app/accounts.ex".to_string(),
		"lib/my_app/repo.ex".to_string(),
		"lib/my_app/user.ex".to_string(),
		"test/support/my_app/user.exs".to_string(),
	];

	assert_eq!(
		language.resolve_import("MyApp.Repo", "lib/my_app/accounts.ex", &files),
		Some("lib/my_app/repo.ex".to_string())
	);
	assert_eq!(
		language.resolve_import("MyApp.User", "lib/my_app/accounts.ex", &files),
		Some("lib/my_app/user.ex".to_string())
	);
	assert_eq!(
		language.resolve_import("External.Package", "lib/my_app/accounts.ex", &files),
		None
	);
}
