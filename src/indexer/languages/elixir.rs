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

//! Elixir language implementation for indexing and live GraphRAG extraction.
//!
//! The Elixir grammar represents declarations (`defmodule`, `def`, and so on)
//! as ordinary `call` nodes. This implementation classifies those calls by
//! their target instead of treating every invocation as a declaration.

use crate::indexer::code_region_extractor::{extract_meaningful_regions, CodeRegion};
use crate::indexer::languages::{CallTarget, Language, TypeRelationKind};
use tree_sitter::Node;

const FUNCTION_DEFINITION_MACROS: &[&str] = &[
	"def",
	"defp",
	"defdelegate",
	"defguard",
	"defguardp",
	"defmacro",
	"defmacrop",
	"defn",
	"defnp",
	"defcallback",
	"defmacrocallback",
];

const CONTAINER_DEFINITION_MACROS: &[&str] = &["defmodule", "defprotocol", "defimpl"];
const DATA_DEFINITION_MACROS: &[&str] = &["defstruct", "defexception"];
const TEST_DEFINITION_MACROS: &[&str] = &["test", "property"];

const NON_CALL_MACROS: &[&str] = &[
	"after",
	"alias",
	"case",
	"catch",
	"cond",
	"else",
	"for",
	"if",
	"import",
	"quote",
	"raise",
	"receive",
	"require",
	"rescue",
	"reraise",
	"super",
	"throw",
	"try",
	"unless",
	"unquote",
	"unquote_splicing",
	"use",
	"with",
];

pub struct Elixir {}

impl Language for Elixir {
	fn name(&self) -> &'static str {
		"elixir"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_elixir::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec!["call"]
	}

	fn is_meaningful_node(&self, node: Node, contents: &str) -> bool {
		call_macro_name(node, contents).is_some_and(|name| {
			FUNCTION_DEFINITION_MACROS.contains(&name.as_str())
				|| DATA_DEFINITION_MACROS.contains(&name.as_str())
				|| (!CONTAINER_DEFINITION_MACROS.contains(&name.as_str())
					&& !NON_CALL_MACROS.contains(&name.as_str())
					&& has_do_block(node))
		})
	}

	fn is_signature_node(&self, node: Node, contents: &str) -> bool {
		call_macro_name(node, contents).is_some_and(|name| is_definition_macro(&name))
	}

	fn expand_meaningful_node(&self, node: Node, contents: &str) -> Option<Vec<CodeRegion>> {
		// A meaningful call-with-do-block (e.g. ExUnit `describe "x" do ...
		// end` or a Phoenix `scope "/api", W do ... end`) can wrap other
		// meaningful calls (`test`, `get`, `post`, ...). Recurse into its
		// children looking for those first; is_meaningful_node already
		// excludes ordinary calls (`Repo.get`, `assert`, ...), so this never
		// turns plain expressions into their own regions. Returning None
		// when nothing nested qualifies falls back to the normal path, which
		// keeps the whole call as one region exactly as before (e.g. a `def`
		// with an ordinary body, or a `test` with no nested block).
		if node.kind() != "call" || !self.is_meaningful_node(node, contents) {
			return None;
		}
		let mut sub_regions = Vec::new();
		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			extract_meaningful_regions(child, contents, self, &mut sub_regions);
		}
		(!sub_regions.is_empty()).then_some(sub_regions)
	}

	fn get_symbol_kinds(&self) -> Vec<&'static str> {
		vec!["call"]
	}

	fn extract_declaration_name(&self, node: Node, contents: &str) -> Option<String> {
		let macro_name = call_macro_name(node, contents)?;
		if matches!(macro_name.as_str(), "defmodule" | "defprotocol") {
			return first_argument(node)
				.and_then(|argument| node_text(argument, contents))
				.map(normalize_module_name);
		}
		if macro_name == "defimpl" {
			let protocol = first_argument(node)
				.and_then(|argument| node_text(argument, contents))
				.map(normalize_module_name)?;
			return implementation_type(node, contents)
				.map(|target| format!("{protocol} for {target}"))
				.or(Some(protocol));
		}
		if FUNCTION_DEFINITION_MACROS.contains(&macro_name.as_str()) {
			return first_argument(node)
				.and_then(|argument| definition_head_name(argument, contents));
		}
		if TEST_DEFINITION_MACROS.contains(&macro_name.as_str()) {
			return first_argument(node)
				.and_then(|argument| node_text(argument, contents))
				.map(|name| name.trim().trim_matches(['"', '\'']).to_string())
				.filter(|name| !name.is_empty());
		}
		None
	}

	fn extract_declaration_kind(&self, node: Node, contents: &str) -> Option<&'static str> {
		match call_macro_name(node, contents)?.as_str() {
			"defmodule" => Some("module"),
			"defprotocol" => Some("interface"),
			"defimpl" => Some("implementation"),
			"defstruct" => Some("struct"),
			"defexception" => Some("class"),
			name if TEST_DEFINITION_MACROS.contains(&name) => Some("function"),
			"defmacro" | "defmacrop" | "defmacrocallback" => Some("macro"),
			name if FUNCTION_DEFINITION_MACROS.contains(&name) => Some("function"),
			_ => None,
		}
	}

	fn extract_signature_name(&self, node: Node, contents: &str) -> Option<String> {
		let macro_name = call_macro_name(node, contents)?;
		if DATA_DEFINITION_MACROS.contains(&macro_name.as_str()) {
			return self.extract_symbol_owner(node, contents);
		}
		self.extract_declaration_name(node, contents)
	}

	fn extract_symbol_owner(&self, node: Node, contents: &str) -> Option<String> {
		let current_macro = call_macro_name(node, contents)?;
		if CONTAINER_DEFINITION_MACROS.contains(&current_macro.as_str()) {
			return None;
		}

		let mut parent = node.parent();
		while let Some(candidate) = parent {
			if candidate.kind() == "call" {
				if let Some(macro_name) = call_macro_name(candidate, contents) {
					if CONTAINER_DEFINITION_MACROS.contains(&macro_name.as_str()) {
						if macro_name == "defimpl" {
							return implementation_type(candidate, contents)
								.or_else(|| self.extract_declaration_name(candidate, contents));
						}
						return self.extract_declaration_name(candidate, contents);
					}
				}
			}
			parent = candidate.parent();
		}
		None
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();
		if let Some(name) = self.extract_declaration_name(node, contents) {
			symbols.push(name);
		}
		if let Some(owner) = self.extract_symbol_owner(node, contents) {
			symbols.push(owner);
		}
		self.extract_identifiers(node, contents, &mut symbols);
		super::deduplicate_symbols(&mut symbols);
		symbols
	}

	fn extract_identifiers(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		if matches!(node.kind(), "identifier" | "operator_identifier" | "alias") {
			if let Some(text) = node_text(node, contents) {
				let text = text.trim();
				if !text.is_empty()
					&& !is_definition_macro(text)
					&& !NON_CALL_MACROS.contains(&text)
					&& !symbols.iter().any(|symbol| symbol == text)
				{
					symbols.push(text.to_string());
				}
			}
		}

		let mut cursor = node.walk();
		for child in node.children(&mut cursor) {
			self.extract_identifiers(child, contents, symbols);
		}
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		let Some(macro_name) = call_macro_name(node, contents) else {
			return (Vec::new(), Vec::new());
		};

		let imports = if matches!(macro_name.as_str(), "alias" | "import" | "require" | "use") {
			first_argument(node)
				.and_then(|argument| node_text(argument, contents))
				.map(expand_module_aliases)
				.unwrap_or_default()
		} else {
			Vec::new()
		};

		let exports = if matches!(macro_name.as_str(), "defmodule" | "defprotocol")
			|| is_public_definition_macro(&macro_name)
		{
			self.extract_declaration_name(node, contents)
				.into_iter()
				.collect()
		} else {
			Vec::new()
		};

		(imports, exports)
	}

	fn extract_function_calls(&self, node: Node, contents: &str) -> Vec<CallTarget> {
		if node.kind() == "binary_operator" {
			let operator = node
				.child_by_field_name("operator")
				.and_then(|operator| node_text(operator, contents));
			if operator.as_deref() == Some("|>") {
				if let Some(right) = node.child_by_field_name("right") {
					if matches!(right.kind(), "identifier" | "dot") {
						return node_text(right, contents)
							.and_then(|target| super::extract_call_target(&target))
							.into_iter()
							.collect();
					}
				}
			}
			return Vec::new();
		}

		if node.kind() != "call" || is_definition_head_call(node, contents) {
			return Vec::new();
		}
		let Some(target) = node.child_by_field_name("target") else {
			return Vec::new();
		};
		let Some(target_text) = node_text(target, contents) else {
			return Vec::new();
		};
		if target.kind() == "identifier"
			&& (is_definition_macro(&target_text)
				|| NON_CALL_MACROS.contains(&target_text.as_str()))
		{
			return Vec::new();
		}
		super::extract_call_target(&target_text)
			.into_iter()
			.collect()
	}

	fn extract_type_relations(
		&self,
		node: Node,
		contents: &str,
	) -> Vec<(TypeRelationKind, String)> {
		if call_macro_name(node, contents).as_deref() != Some("defimpl") {
			return Vec::new();
		}
		first_argument(node)
			.and_then(|argument| node_text(argument, contents))
			.map(normalize_module_name)
			.map(|protocol| vec![(TypeRelationKind::Implements, protocol)])
			.unwrap_or_default()
	}

	fn extract_type_relation_source(&self, node: Node, contents: &str) -> Option<String> {
		implementation_type(node, contents)
	}

	fn get_node_type_description(&self, node_type: &str) -> &'static str {
		if node_type == "call" {
			"module, function, macro, protocol, and data declarations"
		} else {
			"declarations"
		}
	}

	fn resolve_import(
		&self,
		import_path: &str,
		source_file: &str,
		all_files: &super::resolution_utils::FileRegistry,
	) -> Option<String> {
		use super::resolution_utils::resolve_relative_path;

		let registry = all_files;
		if import_path.starts_with("./") || import_path.starts_with("../") {
			let path = resolve_relative_path(source_file, import_path)?;
			return registry.find_file_with_extensions(&path, &self.get_file_extensions());
		}

		let module_path = module_to_file_path(import_path)?;
		let files = registry.get_files_with_extensions(&self.get_file_extensions());
		let suffixes = [format!("/{module_path}.ex"), format!("/{module_path}.exs")];
		let exact_names = [format!("{module_path}.ex"), format!("{module_path}.exs")];
		let mut matches: Vec<String> = files
			.into_iter()
			.filter(|file| {
				let normalized = file.replace('\\', "/");
				exact_names.contains(&normalized)
					|| suffixes.iter().any(|suffix| normalized.ends_with(suffix))
			})
			.collect();
		matches.sort_by_key(|file| {
			let normalized = file.replace('\\', "/");
			(
				!normalized.ends_with(&format!("/lib/{module_path}.ex")),
				normalized.len(),
			)
		});
		matches.into_iter().next()
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["ex", "exs"]
	}
}

fn is_definition_macro(name: &str) -> bool {
	FUNCTION_DEFINITION_MACROS.contains(&name)
		|| CONTAINER_DEFINITION_MACROS.contains(&name)
		|| DATA_DEFINITION_MACROS.contains(&name)
		|| TEST_DEFINITION_MACROS.contains(&name)
}

fn is_public_definition_macro(name: &str) -> bool {
	matches!(
		name,
		"def"
			| "defdelegate"
			| "defguard"
			| "defmacro"
			| "defn" | "defcallback"
			| "defmacrocallback"
	)
}

fn node_text(node: Node, contents: &str) -> Option<String> {
	node.utf8_text(contents.as_bytes()).ok().map(str::to_string)
}

fn call_macro_name(node: Node, contents: &str) -> Option<String> {
	if node.kind() != "call" {
		return None;
	}
	let target = node.child_by_field_name("target")?;
	(target.kind() == "identifier")
		.then(|| node_text(target, contents))
		.flatten()
}

fn first_argument(node: Node) -> Option<Node> {
	let mut cursor = node.walk();
	let arguments = node
		.children(&mut cursor)
		.find(|child| child.kind() == "arguments")?;
	arguments.named_child(0)
}

fn has_do_block(node: Node) -> bool {
	let mut cursor = node.walk();
	let found = node
		.children(&mut cursor)
		.any(|child| child.kind() == "do_block");
	found
}

fn definition_head_name(node: Node, contents: &str) -> Option<String> {
	match node.kind() {
		"identifier" | "operator_identifier" => node_text(node, contents),
		"call" => node
			.child_by_field_name("target")
			.and_then(|target| node_text(target, contents))
			.and_then(|target| super::extract_call_target(&target).map(|call| call.name)),
		"binary_operator" => node
			.child_by_field_name("left")
			.and_then(|left| definition_head_name(left, contents)),
		_ => {
			let mut cursor = node.walk();
			let name = node
				.children(&mut cursor)
				.find_map(|child| definition_head_name(child, contents));
			name
		}
	}
}

fn normalize_module_name(name: String) -> String {
	name.trim()
		.trim_start_matches(':')
		.trim_start_matches("Elixir.")
		.to_string()
}

fn expand_module_aliases(text: String) -> Vec<String> {
	let normalized = normalize_module_name(text);
	if let Some((prefix, tail)) = normalized.split_once(".{") {
		if let Some(names) = tail.strip_suffix('}') {
			return names
				.split(',')
				.map(str::trim)
				.filter(|name| !name.is_empty())
				.map(|name| format!("{prefix}.{name}"))
				.collect();
		}
	}
	(!normalized.is_empty())
		.then_some(normalized)
		.into_iter()
		.collect()
}

fn implementation_type(node: Node, contents: &str) -> Option<String> {
	if call_macro_name(node, contents).as_deref() != Some("defimpl") {
		return None;
	}
	if let Some(text) = node_text(node, contents) {
		if let Some((_, tail)) = text.split_once("for:") {
			let target = tail
				.trim_start()
				.split(|character: char| {
					character.is_whitespace() || matches!(character, ',' | ')' | ']')
				})
				.find(|part| !part.is_empty())?;
			if !target.starts_with('[') {
				return Some(normalize_module_name(target.to_string()));
			}
		}
	}
	let mut cursor = node.walk();
	for descendant in node.children(&mut cursor) {
		if let Some(found) = find_for_option(descendant, contents) {
			return Some(found);
		}
	}
	None
}

fn find_for_option(node: Node, contents: &str) -> Option<String> {
	if node.kind() == "pair" {
		let key = node
			.child_by_field_name("key")
			.and_then(|child| node_text(child, contents));
		if key
			.as_deref()
			.is_some_and(|key| key.trim_end_matches(':') == "for")
		{
			return node
				.child_by_field_name("value")
				.and_then(|child| node_text(child, contents))
				.map(normalize_module_name);
		}
	}
	let mut cursor = node.walk();
	let option = node
		.children(&mut cursor)
		.find_map(|child| find_for_option(child, contents));
	option
}

fn is_definition_head_call(node: Node, contents: &str) -> bool {
	let start = node.start_byte();
	let end = node.end_byte();
	let mut ancestor = node.parent();
	while let Some(candidate) = ancestor {
		if candidate.kind() == "call" {
			if call_macro_name(candidate, contents)
				.is_some_and(|name| FUNCTION_DEFINITION_MACROS.contains(&name.as_str()))
			{
				return first_argument(candidate)
					.is_some_and(|head| start >= head.start_byte() && end <= head.end_byte());
			}
			return false;
		}
		ancestor = candidate.parent();
	}
	false
}

fn module_to_file_path(module: &str) -> Option<String> {
	let module = module
		.trim()
		.trim_start_matches(':')
		.trim_start_matches("Elixir.");
	if module.is_empty()
		|| !module
			.chars()
			.all(|character| character.is_alphanumeric() || matches!(character, '.' | '_'))
	{
		return None;
	}
	Some(
		module
			.split('.')
			.map(camel_to_snake)
			.collect::<Vec<_>>()
			.join("/"),
	)
}

fn camel_to_snake(segment: &str) -> String {
	let characters: Vec<char> = segment.chars().collect();
	let mut output = String::with_capacity(segment.len() + 4);
	for (index, character) in characters.iter().copied().enumerate() {
		let previous = index
			.checked_sub(1)
			.and_then(|i| characters.get(i))
			.copied();
		let next = characters.get(index + 1).copied();
		if character.is_uppercase()
			&& index > 0
			&& (previous.is_some_and(|previous| previous.is_lowercase() || previous.is_numeric())
				|| (previous.is_some_and(char::is_uppercase)
					&& next.is_some_and(char::is_lowercase)))
		{
			output.push('_');
		}
		output.extend(character.to_lowercase());
	}
	output
}

#[cfg(test)]
mod helper_tests {
	use super::*;

	#[test]
	fn converts_module_names_to_mix_paths() {
		assert_eq!(
			module_to_file_path("MyApp.HTTPClient").as_deref(),
			Some("my_app/http_client")
		);
		assert_eq!(
			module_to_file_path("Elixir.OAuth2.Client").as_deref(),
			Some("o_auth2/client")
		);
		assert_eq!(module_to_file_path("dynamic(module)"), None);
	}

	#[test]
	fn expands_grouped_aliases() {
		assert_eq!(
			expand_module_aliases("MyApp.{Accounts, Repo}".to_string()),
			vec!["MyApp.Accounts", "MyApp.Repo"]
		);
	}
}
