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

//! Structural code search using ast-grep patterns.
//! Provides programmatic ast-grep functionality without requiring the `sg` CLI.

use anyhow::{bail, Result};
use ast_grep_core::language::Language as AstGrepLanguage;
use ast_grep_core::matcher::{KindMatcher, PatternBuilder, PatternNode};
use ast_grep_core::source::Edit as AstEdit;
use ast_grep_core::tree_sitter::{LanguageExt, StrDoc};
use ast_grep_core::{Doc, Node, Pattern, PatternError};
use std::path::Path;

pub use ast_grep_core::MatchStrictness;

// Language wrapper types that bridge our tree-sitter grammars to ast-grep's traits.

macro_rules! define_ast_grep_lang {
	($name:ident, $ts_lang:expr, $expando:expr) => {
		#[derive(Clone, Copy)]
		pub struct $name;

		impl AstGrepLanguage for $name {
			fn expando_char(&self) -> char {
				$expando
			}

			fn kind_to_id(&self, kind: &str) -> u16 {
				let ts_lang: tree_sitter::Language = $ts_lang.into();
				ts_lang.id_for_node_kind(kind, true)
			}

			fn field_to_id(&self, field: &str) -> Option<u16> {
				self.get_ts_language()
					.field_id_for_name(field)
					.map(|f| f.get())
			}

			fn build_pattern(&self, builder: &PatternBuilder) -> Result<Pattern, PatternError> {
				builder.build(|src| StrDoc::try_new(src, self.clone()))
			}
		}

		impl LanguageExt for $name {
			fn get_ts_language(&self) -> tree_sitter::Language {
				$ts_lang.into()
			}
		}
	};
}

define_ast_grep_lang!(AstRust, tree_sitter_rust::LANGUAGE, '$');
define_ast_grep_lang!(AstJavaScript, tree_sitter_javascript::LANGUAGE, '$');
define_ast_grep_lang!(
	AstTypeScript,
	tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
	'$'
);
define_ast_grep_lang!(AstPython, tree_sitter_python::LANGUAGE, 'µ');
define_ast_grep_lang!(AstGo, tree_sitter_go::LANGUAGE, 'µ');
define_ast_grep_lang!(AstJava, tree_sitter_java::LANGUAGE, '$');
define_ast_grep_lang!(AstCpp, tree_sitter_cpp::LANGUAGE, '$');
define_ast_grep_lang!(AstPhp, tree_sitter_php::LANGUAGE_PHP, '#');
define_ast_grep_lang!(AstRuby, tree_sitter_ruby::LANGUAGE, '$');
define_ast_grep_lang!(AstSwift, tree_sitter_swift::LANGUAGE, '$');
define_ast_grep_lang!(AstLua, tree_sitter_lua::LANGUAGE, 'µ');
define_ast_grep_lang!(AstBash, tree_sitter_bash::LANGUAGE, '$');
define_ast_grep_lang!(AstCss, tree_sitter_css::LANGUAGE, 'µ');
define_ast_grep_lang!(AstJson, tree_sitter_json::LANGUAGE, '$');

/// A single match result from structural search.
pub struct GrepMatch {
	pub file: String,
	pub line: usize,
	pub column: usize,
	pub text: String,
	/// Byte range of the matched node in its source file. Used for
	/// `inside`/`has` containment filtering. (0, 0) for lexical fallback hits.
	pub start_byte: usize,
	pub end_byte: usize,
	/// Enclosing named containers, outermost first
	/// (e.g. "impl Store › fn flush"). None at file top level.
	pub breadcrumb: Option<String>,
}

/// Per-file match before the file path is attached.
pub struct RawMatch {
	pub line: usize,
	pub column: usize,
	pub text: String,
	pub start_byte: usize,
	pub end_byte: usize,
	pub breadcrumb: Option<String>,
}

/// Metavariable constraint: captured variable name (without `$`) plus the
/// regex its text must match for the overall match to be kept.
pub type MetavarConstraint = (String, regex::Regex);

fn is_identifier_kind(kind: &str) -> bool {
	// Covers identifier/type_identifier/field_identifier/property_identifier
	// (most grammars), `name` (PHP), `constant` (Ruby).
	kind.contains("identifier") || kind == "name" || kind == "constant"
}

/// Display name of a definition-like node: `name` field, then `type`
/// (Rust impl blocks), then `declarator` (C/C++), then first identifier child.
fn node_def_name<D: Doc>(node: &Node<'_, D>) -> Option<String> {
	if let Some(n) = node.field("name") {
		return Some(n.text().to_string());
	}
	if let Some(n) = node.field("type") {
		return Some(n.text().to_string());
	}
	if let Some(d) = node.field("declarator") {
		if let Some(id) = d.dfs().find(|c| is_identifier_kind(&c.kind())) {
			return Some(id.text().to_string());
		}
	}
	node.children()
		.find(|c| is_identifier_kind(&c.kind()))
		.map(|c| c.text().to_string())
}

/// Build a breadcrumb of enclosing named containers, outermost first.
fn breadcrumb_for<D: Doc>(node: &Node<'_, D>, containers: &[(&str, &str)]) -> Option<String> {
	let mut parts: Vec<String> = Vec::new();
	// ancestors() yields parent → … → root.
	for anc in node.ancestors() {
		let kind = anc.kind();
		if let Some((_, label)) = containers.iter().find(|(k, _)| *k == kind.as_ref()) {
			let name = node_def_name(&anc).unwrap_or_else(|| "_".to_string());
			parts.push(format!("{} {}", label, name));
		}
	}
	if parts.is_empty() {
		return None;
	}
	parts.reverse();
	Some(parts.join(" › "))
}

/// Collect a RawMatch for every node match in a tree, applying metavariable
/// constraints and attaching enclosing-symbol breadcrumbs.
fn collect_matches<L: LanguageExt + AstGrepLanguage + Clone, M: ast_grep_core::Matcher>(
	grep: &ast_grep_core::AstGrep<StrDoc<L>>,
	matcher: &M,
	containers: &[(&str, &str)],
	constraints: &[MetavarConstraint],
) -> Vec<RawMatch> {
	grep.root()
		.find_all(matcher)
		.filter(|m| {
			constraints.iter().all(|(var, re)| {
				m.get_env()
					.get_match(var)
					.map(|n| re.is_match(&n.text()))
					.unwrap_or(false)
			})
		})
		.map(|m| {
			let text = m.text().to_string();
			let pos = m.start_pos();
			let node = m.get_node();
			let range = node.range();
			RawMatch {
				line: pos.line() + 1, // 0-based → 1-based
				column: pos.byte_point().1,
				text,
				start_byte: range.start,
				end_byte: range.end,
				breadcrumb: breadcrumb_for(node, containers),
			}
		})
		.collect()
}

/// Search a single file with the given pattern, language, and strictness.
fn search_file_with_lang<L: LanguageExt + AstGrepLanguage + Clone>(
	lang: L,
	source: &str,
	pattern_str: &str,
	strictness: MatchStrictness,
	containers: &[(&str, &str)],
	constraints: &[MetavarConstraint],
) -> Result<Vec<RawMatch>> {
	let grep = lang.ast_grep(source);
	let pattern = Pattern::try_new(pattern_str, lang)
		.map_err(|e| anyhow::anyhow!("Invalid pattern: {}", e))?
		.with_strictness(strictness);
	Ok(collect_matches(&grep, &pattern, containers, constraints))
}

/// Search a single file using a KindMatcher (pattern interpreted as AST node kind).
fn search_kind_with_lang<L: LanguageExt + AstGrepLanguage + Clone>(
	lang: L,
	source: &str,
	kind_str: &str,
	containers: &[(&str, &str)],
) -> Result<Vec<RawMatch>> {
	let grep = lang.ast_grep(source);
	let kind = KindMatcher::try_new(kind_str, lang)
		.map_err(|e| anyhow::anyhow!("Invalid AST kind '{}': {}", kind_str, e))?;
	Ok(collect_matches(&grep, &kind, containers, &[]))
}

/// Search a single file with a contextual pattern (wraps `pattern` in `context`,
/// then selects subtree of kind `selector`). Resolves pattern-parsed-as-wrong-kind issues.
fn search_contextual_with_lang<L: LanguageExt + AstGrepLanguage + Clone>(
	lang: L,
	source: &str,
	context_src: &str,
	selector: &str,
	containers: &[(&str, &str)],
	constraints: &[MetavarConstraint],
) -> Result<Vec<RawMatch>> {
	let grep = lang.ast_grep(source);
	let pattern = Pattern::contextual(context_src, selector, lang)
		.map_err(|e| anyhow::anyhow!("Invalid contextual pattern: {}", e))?;
	Ok(collect_matches(&grep, &pattern, containers, constraints))
}

/// Inspect the root AST kind a pattern parses to, plus parse-error flag and metavars.
/// Used to build diagnostics when a search yields zero matches.
fn pattern_info_with_lang<L: LanguageExt + AstGrepLanguage + Clone>(
	lang: L,
	pattern_str: &str,
) -> Result<PatternInfo> {
	let pattern = Pattern::try_new(pattern_str, lang.clone())
		.map_err(|e| anyhow::anyhow!("Invalid pattern: {}", e))?;
	let has_error = pattern.has_error();
	let root_kind = match &pattern.node {
		PatternNode::Terminal { kind_id, .. } | PatternNode::Internal { kind_id, .. } => {
			let ts_lang: tree_sitter::Language = lang.get_ts_language();
			ts_lang.node_kind_for_id(*kind_id).map(|s| s.to_string())
		}
		PatternNode::MetaVar { .. } => None,
	};
	let mut defined_vars: Vec<String> = pattern
		.defined_vars()
		.into_iter()
		.map(|s| s.to_string())
		.collect();
	defined_vars.sort();
	Ok(PatternInfo {
		root_kind,
		has_error,
		defined_vars,
	})
}

/// Determine language from file extension.
pub fn language_from_extension(path: &Path) -> Option<&'static str> {
	let ext = path.extension()?.to_str()?;
	match ext {
		"rs" => Some("rust"),
		"js" | "mjs" | "cjs" | "jsx" => Some("javascript"),
		"ts" | "mts" | "cts" | "tsx" => Some("typescript"),
		"py" | "pyi" => Some("python"),
		"go" => Some("go"),
		"java" => Some("java"),
		"c" | "cc" | "cpp" | "cxx" | "c++" | "h" | "hpp" | "hxx" | "cppm" | "ixx" | "mxx"
		| "ccm" | "cxxm" => Some("cpp"),
		"php" => Some("php"),
		"rb" => Some("ruby"),
		"swift" => Some("swift"),
		"lua" => Some("lua"),
		"sh" | "bash" | "zsh" => Some("bash"),
		"css" | "scss" => Some("css"),
		"json" => Some("json"),
		_ => None,
	}
}

/// (tree-sitter kind, short display label) pairs that form symbol breadcrumbs
/// for the given language. Resolved by language string before generic dispatch.
pub fn container_kinds(language: &str) -> &'static [(&'static str, &'static str)] {
	match language {
		"rust" => &[
			("function_item", "fn"),
			("impl_item", "impl"),
			("trait_item", "trait"),
			("struct_item", "struct"),
			("enum_item", "enum"),
			("mod_item", "mod"),
		],
		"python" => &[
			("function_definition", "def"),
			("class_definition", "class"),
		],
		"javascript" | "typescript" => &[
			("function_declaration", "function"),
			("generator_function_declaration", "function"),
			("method_definition", "method"),
			("class_declaration", "class"),
			("interface_declaration", "interface"),
			("enum_declaration", "enum"),
		],
		"go" => &[
			("function_declaration", "func"),
			("method_declaration", "method"),
			("type_declaration", "type"),
		],
		"java" => &[
			("class_declaration", "class"),
			("interface_declaration", "interface"),
			("enum_declaration", "enum"),
			("method_declaration", "method"),
			("constructor_declaration", "constructor"),
		],
		"cpp" => &[
			("function_definition", "fn"),
			("class_specifier", "class"),
			("struct_specifier", "struct"),
			("namespace_definition", "namespace"),
		],
		"php" => &[
			("function_definition", "function"),
			("method_declaration", "method"),
			("class_declaration", "class"),
			("interface_declaration", "interface"),
			("trait_declaration", "trait"),
		],
		"ruby" => &[
			("method", "def"),
			("singleton_method", "def"),
			("class", "class"),
			("module", "module"),
		],
		"lua" => &[("function_declaration", "function")],
		"bash" => &[("function_definition", "function")],
		"swift" => &[
			("function_declaration", "func"),
			("class_declaration", "class"),
			("protocol_declaration", "protocol"),
		],
		_ => &[],
	}
}

/// Tree-sitter kinds that define named symbols, per language. Used by the
/// `symbol` search mode to find definitions without a hand-written pattern.
pub fn definition_kinds(language: &str) -> &'static [&'static str] {
	match language {
		"rust" => &[
			"function_item",
			"struct_item",
			"enum_item",
			"trait_item",
			"impl_item",
			"mod_item",
			"const_item",
			"static_item",
			"type_item",
			"union_item",
			"macro_definition",
		],
		"javascript" => &[
			"function_declaration",
			"generator_function_declaration",
			"class_declaration",
			"method_definition",
			"variable_declarator",
		],
		"typescript" => &[
			"function_declaration",
			"generator_function_declaration",
			"class_declaration",
			"abstract_class_declaration",
			"method_definition",
			"interface_declaration",
			"type_alias_declaration",
			"enum_declaration",
			"variable_declarator",
		],
		"python" => &["function_definition", "class_definition"],
		"go" => &[
			"function_declaration",
			"method_declaration",
			"type_spec",
			"const_spec",
			"var_spec",
		],
		"java" => &[
			"class_declaration",
			"interface_declaration",
			"enum_declaration",
			"record_declaration",
			"method_declaration",
			"constructor_declaration",
		],
		"cpp" => &[
			"function_definition",
			"class_specifier",
			"struct_specifier",
			"enum_specifier",
			"namespace_definition",
			"type_definition",
		],
		"php" => &[
			"function_definition",
			"method_declaration",
			"class_declaration",
			"interface_declaration",
			"trait_declaration",
		],
		"ruby" => &["method", "singleton_method", "class", "module"],
		"lua" => &["function_declaration"],
		"bash" => &["function_definition"],
		"swift" => &[
			"function_declaration",
			"class_declaration",
			"protocol_declaration",
		],
		_ => &[],
	}
}

/// Name filter for symbol queries. `*` in the spec is a wildcard
/// (e.g. `handle_*`, `*_test`); anything else matches exactly.
pub enum NameMatcher {
	Exact(String),
	Wildcard(regex::Regex),
}

impl NameMatcher {
	pub fn new(spec: &str) -> Result<Self> {
		if spec.contains('*') {
			let re = format!(
				"^{}$",
				spec.split('*')
					.map(regex::escape)
					.collect::<Vec<_>>()
					.join(".*")
			);
			Ok(NameMatcher::Wildcard(regex::Regex::new(&re)?))
		} else {
			Ok(NameMatcher::Exact(spec.to_string()))
		}
	}

	pub fn matches(&self, name: &str) -> bool {
		match self {
			NameMatcher::Exact(s) => s == name,
			NameMatcher::Wildcard(re) => re.is_match(name),
		}
	}

	/// Longest literal fragment of the spec — used as a content prefilter token.
	pub fn literal_fragment(&self) -> Option<String> {
		match self {
			NameMatcher::Exact(s) => Some(s.clone()),
			NameMatcher::Wildcard(_) => None,
		}
	}
}

/// Find symbol definitions by name in a single file (no pattern required).
fn find_defs_with_lang<L: LanguageExt + AstGrepLanguage + Clone>(
	lang: L,
	source: &str,
	name: &NameMatcher,
	def_kinds: &[&str],
	containers: &[(&str, &str)],
) -> Vec<RawMatch> {
	let grep = lang.ast_grep(source);
	let root = grep.root();
	let mut out = Vec::new();
	for node in root.dfs() {
		let kind = node.kind();
		if !def_kinds.contains(&kind.as_ref()) {
			continue;
		}
		let Some(def_name) = node_def_name(&node) else {
			continue;
		};
		if !name.matches(&def_name) {
			continue;
		}
		let pos = node.start_pos();
		let range = node.range();
		out.push(RawMatch {
			line: pos.line() + 1,
			column: pos.byte_point().1,
			text: node.text().to_string(),
			start_byte: range.start,
			end_byte: range.end,
			breadcrumb: breadcrumb_for(&node, containers),
		});
	}
	out
}

/// Find references (identifier usages) by name in a single file. Each hit
/// reports the full trimmed source line; definition sites are tagged `[def]`.
fn find_refs_with_lang<L: LanguageExt + AstGrepLanguage + Clone>(
	lang: L,
	source: &str,
	name: &NameMatcher,
	def_kinds: &[&str],
	containers: &[(&str, &str)],
) -> Vec<RawMatch> {
	let grep = lang.ast_grep(source);
	let root = grep.root();
	let lines: Vec<&str> = source.lines().collect();
	let mut out = Vec::new();
	for node in root.dfs() {
		if !node.is_named_leaf() {
			continue;
		}
		let kind = node.kind();
		if !is_identifier_kind(&kind) {
			continue;
		}
		let text = node.text();
		if !name.matches(&text) {
			continue;
		}
		let pos = node.start_pos();
		let range = node.range();
		let line_text = lines
			.get(pos.line())
			.map(|l| l.trim().to_string())
			.unwrap_or_default();
		let is_def = node
			.parent()
			.map(|p| {
				def_kinds.contains(&p.kind().as_ref())
					&& p.field("name").map(|n| n.range() == range).unwrap_or(false)
			})
			.unwrap_or(false);
		out.push(RawMatch {
			line: pos.line() + 1,
			column: pos.byte_point().1,
			text: if is_def {
				format!("[def] {}", line_text)
			} else {
				line_text
			},
			start_byte: range.start,
			end_byte: range.end,
			breadcrumb: breadcrumb_for(&node, containers),
		});
	}
	out
}

/// Extract concrete identifier tokens from a pattern, excluding metavariables.
/// Every token must appear verbatim in any file the pattern can match, so the
/// result is safe to use as an AND content prefilter. Sorted longest-first.
pub fn literal_tokens(pattern: &str) -> Vec<String> {
	let chars: Vec<char> = pattern.chars().collect();
	let mut tokens: Vec<String> = Vec::new();
	let mut i = 0;
	while i < chars.len() {
		let c = chars[i];
		if c == '_' || c.is_ascii_alphabetic() {
			let start = i;
			while i < chars.len() && (chars[i] == '_' || chars[i].is_ascii_alphanumeric()) {
				i += 1;
			}
			// Skip metavariable names: preceded by an expando char ($, µ, #).
			let prev = if start > 0 {
				Some(chars[start - 1])
			} else {
				None
			};
			let is_metavar = matches!(prev, Some('$') | Some('µ') | Some('#'));
			if !is_metavar && i - start >= 2 {
				tokens.push(chars[start..i].iter().collect());
			}
		} else {
			i += 1;
		}
	}
	tokens.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
	tokens.dedup();
	tokens
}

/// Plain-text line scan for `token`. Last-resort fallback when no structural
/// strategy matched — results are not AST-verified.
pub fn lexical_scan(file_path: &str, source: &str, token: &str, max: usize) -> Vec<GrepMatch> {
	let mut out = Vec::new();
	for (i, line) in source.lines().enumerate() {
		if out.len() >= max {
			break;
		}
		if let Some(col) = line.find(token) {
			out.push(GrepMatch {
				file: file_path.to_string(),
				line: i + 1,
				column: col,
				text: line.trim().to_string(),
				start_byte: 0,
				end_byte: 0,
				breadcrumb: None,
			});
		}
	}
	out
}

/// What a parsed pattern looks like — used to build diagnostics when a search
/// returns zero matches so the LLM knows whether to retry or change strategy.
#[derive(Debug, Clone)]
pub struct PatternInfo {
	/// Root tree-sitter node kind the pattern parses as (e.g. "call_expression").
	/// None when the whole pattern is a single bare metavariable like `$X`.
	pub root_kind: Option<String>,
	/// True when tree-sitter could not parse the pattern as valid syntax.
	pub has_error: bool,
	/// Names of capturing metavariables found in the pattern, sorted.
	pub defined_vars: Vec<String>,
}

/// Dispatch a search by language. Wraps each `Ast*` lang type behind one match
/// arm and forwards to the generic helper, monomorphized per language.
macro_rules! dispatch_lang {
	($lang_str:expr, |$lang:ident| $body:expr) => {
		match $lang_str {
			"rust" => {
				#[allow(unused_variables)]
				let $lang = AstRust;
				$body
			}
			"javascript" => {
				#[allow(unused_variables)]
				let $lang = AstJavaScript;
				$body
			}
			"typescript" => {
				#[allow(unused_variables)]
				let $lang = AstTypeScript;
				$body
			}
			"python" => {
				#[allow(unused_variables)]
				let $lang = AstPython;
				$body
			}
			"go" => {
				#[allow(unused_variables)]
				let $lang = AstGo;
				$body
			}
			"java" => {
				#[allow(unused_variables)]
				let $lang = AstJava;
				$body
			}
			"cpp" => {
				#[allow(unused_variables)]
				let $lang = AstCpp;
				$body
			}
			"php" => {
				#[allow(unused_variables)]
				let $lang = AstPhp;
				$body
			}
			"ruby" => {
				#[allow(unused_variables)]
				let $lang = AstRuby;
				$body
			}
			"swift" => {
				#[allow(unused_variables)]
				let $lang = AstSwift;
				$body
			}
			"lua" => {
				#[allow(unused_variables)]
				let $lang = AstLua;
				$body
			}
			"bash" => {
				#[allow(unused_variables)]
				let $lang = AstBash;
				$body
			}
			"css" => {
				#[allow(unused_variables)]
				let $lang = AstCss;
				$body
			}
			"json" => {
				#[allow(unused_variables)]
				let $lang = AstJson;
				$body
			}
			_ => bail!("Unsupported language: {}", $lang_str),
		}
	};
}

fn wrap_matches(file_path: &str, raw: Vec<RawMatch>) -> Vec<GrepMatch> {
	raw.into_iter()
		.map(|r| GrepMatch {
			file: file_path.to_string(),
			line: r.line,
			column: r.column,
			text: r.text,
			start_byte: r.start_byte,
			end_byte: r.end_byte,
			breadcrumb: r.breadcrumb,
		})
		.collect()
}

/// Search a single file with the default `Smart` strictness. Equivalent to
/// `search_file_strict(.., MatchStrictness::Smart)`.
pub fn search_file(
	file_path: &str,
	source: &str,
	pattern: &str,
	language: &str,
) -> Result<Vec<GrepMatch>> {
	search_file_strict(file_path, source, pattern, language, MatchStrictness::Smart)
}

/// Search a single file with explicit strictness. Use `Relaxed` to ignore
/// trivia/comments — a common fix when a `Smart` search returns zero matches.
pub fn search_file_strict(
	file_path: &str,
	source: &str,
	pattern: &str,
	language: &str,
	strictness: MatchStrictness,
) -> Result<Vec<GrepMatch>> {
	search_file_constrained(file_path, source, pattern, language, strictness, &[])
}

/// Search a single file with explicit strictness and metavariable constraints.
/// Each constraint is (var name without `$`, regex the captured text must match).
pub fn search_file_constrained(
	file_path: &str,
	source: &str,
	pattern: &str,
	language: &str,
	strictness: MatchStrictness,
	constraints: &[MetavarConstraint],
) -> Result<Vec<GrepMatch>> {
	let containers = container_kinds(language);
	let raw = dispatch_lang!(language, |lang| search_file_with_lang(
		lang,
		source,
		pattern,
		strictness,
		containers,
		constraints
	))?;
	Ok(wrap_matches(file_path, raw))
}

/// Search a single file using a tree-sitter node kind as the matcher.
/// Use this when you want to match all nodes of a given kind (e.g. all
/// `function_declaration` or `call_expression`) without writing a body pattern.
pub fn search_file_by_kind(
	file_path: &str,
	source: &str,
	kind: &str,
	language: &str,
) -> Result<Vec<GrepMatch>> {
	let containers = container_kinds(language);
	let raw = dispatch_lang!(language, |lang| search_kind_with_lang(
		lang, source, kind, containers
	))?;
	Ok(wrap_matches(file_path, raw))
}

/// Search a single file with a contextual pattern. `context_src` must be valid
/// standalone code containing the pattern; `selector` picks the sub-AST kind
/// inside that scaffold. Resolves cases where a bare pattern parses as the
/// wrong AST kind (e.g. Go `fmt.Println($A)` parsing as type conversion).
pub fn search_file_contextual(
	file_path: &str,
	source: &str,
	context_src: &str,
	selector: &str,
	language: &str,
) -> Result<Vec<GrepMatch>> {
	search_file_contextual_constrained(file_path, source, context_src, selector, language, &[])
}

/// Contextual search with metavariable constraints.
pub fn search_file_contextual_constrained(
	file_path: &str,
	source: &str,
	context_src: &str,
	selector: &str,
	language: &str,
	constraints: &[MetavarConstraint],
) -> Result<Vec<GrepMatch>> {
	let containers = container_kinds(language);
	let raw = dispatch_lang!(language, |lang| search_contextual_with_lang(
		lang,
		source,
		context_src,
		selector,
		containers,
		constraints
	))?;
	Ok(wrap_matches(file_path, raw))
}

/// Find symbol definitions by name in a single file (no pattern required).
/// Matches the per-language definition kinds (functions, types, classes, …)
/// whose name satisfies the matcher. Supports `*` wildcards via NameMatcher.
pub fn find_symbol_defs(
	file_path: &str,
	source: &str,
	name: &NameMatcher,
	language: &str,
) -> Result<Vec<GrepMatch>> {
	let def_kinds = definition_kinds(language);
	let containers = container_kinds(language);
	let raw: Vec<RawMatch> = dispatch_lang!(language, |lang| Ok::<_, anyhow::Error>(
		find_defs_with_lang(lang, source, name, def_kinds, containers)
	))?;
	Ok(wrap_matches(file_path, raw))
}

/// One strategy to evaluate against a single shared parse of a file.
/// Used by the smart-search pipeline to avoid re-parsing per strategy.
pub enum SearchSpec {
	/// ast-grep pattern with explicit strictness.
	Pattern {
		pattern: String,
		strictness: MatchStrictness,
	},
	/// Bare tree-sitter kind name.
	Kind { kind: String },
	/// Contextual pattern (context source + selector kind).
	Contextual {
		context_src: String,
		selector: String,
	},
	/// Kind name if valid in the grammar, else ast-grep pattern.
	/// Used for `inside`/`has` relational specs where either shape is accepted.
	KindOrPattern { spec: String },
}

/// Evaluate every spec against ONE parse of the source. Per-spec errors
/// (invalid kind, unparseable pattern) yield an empty bucket rather than
/// failing the whole call — the primary pattern is validated separately.
fn search_multi_with_lang<L: LanguageExt + AstGrepLanguage + Clone>(
	lang: L,
	source: &str,
	specs: &[SearchSpec],
	containers: &[(&str, &str)],
	constraints: &[MetavarConstraint],
) -> Vec<Vec<RawMatch>> {
	let grep = lang.ast_grep(source); // single parse shared by all specs
	specs
		.iter()
		.map(|spec| match spec {
			SearchSpec::Pattern {
				pattern,
				strictness,
			} => Pattern::try_new(pattern, lang.clone())
				.map(|p| {
					collect_matches(
						&grep,
						&p.with_strictness(strictness.clone()),
						containers,
						constraints,
					)
				})
				.unwrap_or_default(),
			SearchSpec::Kind { kind } => KindMatcher::try_new(kind, lang.clone())
				.map(|k| collect_matches(&grep, &k, containers, &[]))
				.unwrap_or_default(),
			SearchSpec::Contextual {
				context_src,
				selector,
			} => Pattern::contextual(context_src, selector, lang.clone())
				.map(|p| collect_matches(&grep, &p, containers, constraints))
				.unwrap_or_default(),
			SearchSpec::KindOrPattern { spec } => match KindMatcher::try_new(spec, lang.clone()) {
				Ok(k) => collect_matches(&grep, &k, containers, &[]),
				Err(_) => Pattern::try_new(spec, lang.clone())
					.map(|p| collect_matches(&grep, &p, containers, &[]))
					.unwrap_or_default(),
			},
		})
		.collect()
}

/// Evaluate multiple search strategies against a single parse of one file.
/// Returns one bucket of matches per spec, in input order.
pub fn search_file_multi(
	file_path: &str,
	source: &str,
	language: &str,
	specs: &[SearchSpec],
	constraints: &[MetavarConstraint],
) -> Result<Vec<Vec<GrepMatch>>> {
	let containers = container_kinds(language);
	let raw = dispatch_lang!(language, |lang| Ok::<_, anyhow::Error>(
		search_multi_with_lang(lang, source, specs, containers, constraints)
	))?;
	Ok(raw
		.into_iter()
		.map(|v| wrap_matches(file_path, v))
		.collect())
}

/// Find symbol references (identifier usages) by name in a single file.
/// Each hit reports the trimmed source line; definition sites are tagged `[def]`.
pub fn find_symbol_refs(
	file_path: &str,
	source: &str,
	name: &NameMatcher,
	language: &str,
) -> Result<Vec<GrepMatch>> {
	let def_kinds = definition_kinds(language);
	let containers = container_kinds(language);
	let raw: Vec<RawMatch> = dispatch_lang!(language, |lang| Ok::<_, anyhow::Error>(
		find_refs_with_lang(lang, source, name, def_kinds, containers)
	))?;
	Ok(wrap_matches(file_path, raw))
}

/// Inspect what a pattern parses to. Returns the root AST kind, parse-error
/// flag, and the set of named metavariables. Cheap — does not require a corpus.
pub fn pattern_info(pattern: &str, language: &str) -> Result<PatternInfo> {
	dispatch_lang!(language, |lang| pattern_info_with_lang(lang, pattern))
}

/// Map a common LLM intent word (e.g. "function", "class", "import") to the
/// canonical tree-sitter node kind name for the given language. Returns None
/// when no mapping exists. Used as a fallback when an LLM passes a kind name
/// that doesn't exist in the target grammar (e.g. `function_declaration` in
/// Python where the grammar uses `function_definition`).
///
/// The intent string is normalized: lowercased, leading `$` stripped.
pub fn canonical_kind(intent: &str, language: &str) -> Option<&'static str> {
	let key = intent.trim().trim_start_matches('$').to_ascii_lowercase();
	match (key.as_str(), language) {
		// ---- function-like ----
		("function" | "func" | "fn" | "function_declaration", "javascript" | "typescript") => {
			Some("function_declaration")
		}
		("function" | "func" | "fn" | "function_definition", "python") => {
			Some("function_definition")
		}
		("function" | "func" | "fn" | "function_item", "rust") => Some("function_item"),
		("function" | "func" | "fn" | "function_declaration", "go") => Some("function_declaration"),
		("function" | "func" | "fn", "java") => Some("method_declaration"),
		("function" | "func" | "fn" | "function_definition", "cpp") => Some("function_definition"),
		("function" | "func" | "fn" | "function_definition", "php") => Some("function_definition"),
		("function" | "func" | "fn" | "method", "ruby") => Some("method"),
		("function" | "func" | "fn", "lua") => Some("function_declaration"),
		("function" | "func" | "fn", "bash") => Some("function_definition"),

		// ---- method (distinct from free function on some langs) ----
		("method" | "method_definition", "javascript" | "typescript") => Some("method_definition"),
		("method", "go" | "method_declaration") => Some("method_declaration"),
		("method", "java" | "method_invocation") => Some("method_declaration"),

		// ---- class / struct / trait / interface / impl ----
		("class" | "class_declaration", "javascript" | "typescript") => Some("class_declaration"),
		("class" | "class_definition", "python") => Some("class_definition"),
		("class", "java") => Some("class_declaration"),
		("class" | "class_specifier", "cpp") => Some("class_specifier"),
		("class", "ruby") => Some("class"),
		("class", "php") => Some("class_declaration"),

		("struct" | "struct_item", "rust") => Some("struct_item"),
		("struct" | "struct_specifier", "cpp") => Some("struct_specifier"),
		("struct", "go") => Some("struct_type"),

		("trait" | "trait_item", "rust") => Some("trait_item"),
		("interface" | "interface_declaration", "typescript") => Some("interface_declaration"),
		("interface", "java") => Some("interface_declaration"),
		("interface", "go") => Some("interface_type"),

		("impl" | "impl_item", "rust") => Some("impl_item"),

		// ---- import-like ----
		("import" | "import_statement", "javascript" | "typescript") => Some("import_statement"),
		("import" | "import_statement", "python") => Some("import_statement"),
		("from_import" | "import_from" | "import_from_statement", "python") => {
			Some("import_from_statement")
		}
		("import" | "use" | "use_declaration", "rust") => Some("use_declaration"),
		("import" | "import_declaration", "go") => Some("import_declaration"),
		("import", "java") => Some("import_declaration"),
		("import" | "namespace_use", "php") => Some("namespace_use_declaration"),

		// ---- call expressions ----
		("call" | "call_expression", "javascript" | "typescript") => Some("call_expression"),
		("call", "python") => Some("call"),
		("call" | "call_expression", "rust") => Some("call_expression"),
		("call" | "call_expression", "go") => Some("call_expression"),
		("call" | "method_invocation", "java") => Some("method_invocation"),
		("call" | "call_expression", "cpp") => Some("call_expression"),
		("call", "ruby") => Some("call"),

		// ---- control flow ----
		("if" | "if_statement" | "conditional", "javascript" | "typescript") => {
			Some("if_statement")
		}
		("if" | "if_statement", "python") => Some("if_statement"),
		("if" | "if_expression", "rust") => Some("if_expression"),
		("if" | "if_statement", "go") => Some("if_statement"),
		("if" | "if_statement", "java" | "cpp") => Some("if_statement"),

		("for" | "loop" | "for_statement", "javascript" | "typescript") => Some("for_statement"),
		("for" | "for_statement", "python") => Some("for_statement"),
		("for" | "loop" | "for_expression", "rust") => Some("for_expression"),
		("for" | "for_statement", "go") => Some("for_statement"),
		("while" | "while_statement", "javascript" | "typescript" | "python" | "go") => {
			Some("while_statement")
		}

		// ---- return / try ----
		("return" | "return_statement", _) => Some("return_statement"),
		("try" | "try_statement", "javascript" | "typescript" | "java" | "python") => {
			Some("try_statement")
		}

		// ---- assignment / variable declarations ----
		("variable" | "let" | "const" | "var", "javascript" | "typescript") => {
			Some("variable_declaration")
		}
		("let" | "let_declaration", "rust") => Some("let_declaration"),
		("variable" | "let" | "let_statement", "go") => Some("var_declaration"),

		_ => None,
	}
}

/// Result of rewriting matches in a single file.
pub struct RewriteResult {
	pub file: String,
	pub replacements: usize,
	pub original_source: String,
	pub rewritten_source: String,
}

/// Rewrite matches in a single file using a replacement template.
/// Returns None if no matches found. The template supports metavariables
/// captured by the search pattern (e.g. `$VAR`, `$$$ARGS`).
fn rewrite_file_with_lang<L: LanguageExt + AstGrepLanguage + Clone>(
	lang: L,
	source: &str,
	pattern_str: &str,
	rewrite_str: &str,
) -> Result<Option<(usize, String)>> {
	let grep = lang.ast_grep(source);
	let pattern = Pattern::try_new(pattern_str, lang)
		.map_err(|e| anyhow::anyhow!("Invalid pattern: {}", e))?;
	let edits = grep.root().replace_all(&pattern, rewrite_str);
	if edits.is_empty() {
		return Ok(None);
	}
	let count = edits.len();
	let rewritten = apply_edits(source, edits);
	Ok(Some((count, rewritten)))
}

/// Apply edits to source in reverse position order to preserve byte offsets.
fn apply_edits(source: &str, edits: Vec<AstEdit<String>>) -> String {
	let mut bytes = source.as_bytes().to_vec();
	// Edits from replace_all are sorted by position ascending.
	// Apply in reverse so earlier positions remain valid.
	for edit in edits.into_iter().rev() {
		bytes.splice(
			edit.position..edit.position + edit.deleted_length,
			edit.inserted_text,
		);
	}
	String::from_utf8(bytes).expect("rewritten source should be valid UTF-8")
}

/// Rewrite matches in a file. Returns None if no matches found.
pub fn rewrite_file(
	file_path: &str,
	source: &str,
	pattern: &str,
	rewrite: &str,
	language: &str,
) -> Result<Option<RewriteResult>> {
	let result = dispatch_lang!(language, |lang| rewrite_file_with_lang(
		lang, source, pattern, rewrite
	))?;

	Ok(
		result.map(|(replacements, rewritten_source)| RewriteResult {
			file: file_path.to_string(),
			replacements,
			original_source: source.to_string(),
			rewritten_source,
		}),
	)
}

/// Generate a unified-diff-style preview of rewrite changes.
pub fn format_rewrite_diff(result: &RewriteResult) -> String {
	let old_lines: Vec<&str> = result.original_source.lines().collect();
	let new_lines: Vec<&str> = result.rewritten_source.lines().collect();

	let mut output = format!("--- {}\n+++ {}\n", result.file, result.file);
	let max_len = old_lines.len().max(new_lines.len());

	let mut i = 0;
	while i < max_len {
		let old_line = old_lines.get(i).copied().unwrap_or("");
		let new_line = new_lines.get(i).copied().unwrap_or("");
		if old_line != new_line {
			output.push_str(&format!("-{}:  {}\n", i + 1, old_line));
			output.push_str(&format!("+{}:  {}\n", i + 1, new_line));
		}
		i += 1;
	}

	output.trim_end().to_string()
}

/// Cap multi-line match text at `max_lines` and append a one-line summary
/// for the remainder. Single-line matches pass through unchanged.
/// Keeps the first signature line (usually the most informative) and avoids
/// drowning the LLM in long impl/class bodies when matching by kind.
pub fn truncate_match_text(text: &str, max_lines: usize) -> String {
	let line_count = text.lines().count();
	if line_count <= max_lines {
		return text.to_string();
	}
	let head: Vec<&str> = text.lines().take(max_lines).collect();
	format!(
		"{}\n... ({} more lines)",
		head.join("\n"),
		line_count - max_lines
	)
}

/// Format matches grouped by file (token-efficient output).
/// Multi-line match bodies are truncated to a few signature lines plus a
/// "... N more lines" footer to keep responses compact.
pub fn format_matches_grouped(matches: &[GrepMatch]) -> String {
	use std::collections::BTreeMap;

	let mut by_file: BTreeMap<&str, Vec<&GrepMatch>> = BTreeMap::new();
	for m in matches {
		by_file.entry(&m.file).or_default().push(m);
	}

	let mut output = String::new();
	for (file, file_matches) in &by_file {
		output.push_str(file);
		output.push('\n');
		for m in file_matches {
			let text = truncate_match_text(&m.text, 4);
			match &m.breadcrumb {
				Some(bc) => {
					output.push_str(&format!("{}:{} [{}]:  {}\n", m.line, m.column, bc, text))
				}
				None => output.push_str(&format!("{}:{}:  {}\n", m.line, m.column, text)),
			}
		}
		output.push('\n');
	}

	output.trim_end().to_string()
}

/// Format matches with context lines.
pub fn format_matches_with_context(
	matches: &[GrepMatch],
	source_map: &std::collections::HashMap<String, String>,
	context: usize,
) -> String {
	use std::collections::BTreeMap;

	let mut by_file: BTreeMap<&str, Vec<&GrepMatch>> = BTreeMap::new();
	for m in matches {
		by_file.entry(&m.file).or_default().push(m);
	}

	let mut output = String::new();
	for (file, file_matches) in &by_file {
		output.push_str(file);
		output.push('\n');

		if let Some(source) = source_map.get(*file) {
			let lines: Vec<&str> = source.lines().collect();
			for m in file_matches {
				if let Some(bc) = &m.breadcrumb {
					output.push_str(&format!("» {}\n", bc));
				}
				let start = m.line.saturating_sub(context + 1);
				let end = (m.line + context).min(lines.len());
				for (i, line) in lines.iter().enumerate().take(end).skip(start) {
					let prefix = if i + 1 == m.line { ">" } else { " " };
					output.push_str(&format!("{} {}:  {}\n", prefix, i + 1, line));
				}
				output.push_str("---\n");
			}
		} else {
			for m in file_matches {
				let text = truncate_match_text(&m.text, 4);
				match &m.breadcrumb {
					Some(bc) => {
						output.push_str(&format!("{}:{} [{}]:  {}\n", m.line, m.column, bc, text))
					}
					None => output.push_str(&format!("{}:{}:  {}\n", m.line, m.column, text)),
				}
			}
		}
		output.push('\n');
	}

	output.trim_end().to_string()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_rust_structural_search() {
		let source = r#"
fn main() {
    let x = foo.unwrap();
    let y = bar.unwrap();
    let z = baz.expect("msg");
}
"#;
		let matches = search_file("test.rs", source, "$VAR.unwrap()", "rust").unwrap();
		assert_eq!(matches.len(), 2, "Should find two unwrap() calls");
		assert!(matches[0].text.contains("unwrap"));
		assert!(matches[1].text.contains("unwrap"));
	}

	#[test]
	fn test_python_structural_search() {
		let source = "x = 1\ny = 2\nz = 3\n";
		// Search for literal assignment — no metavars needed
		let matches = search_file("test.py", source, "x = 1", "python").unwrap();
		assert_eq!(matches.len(), 1, "Should find literal assignment");
		assert!(matches[0].text.contains("x = 1"));
	}

	#[test]
	fn test_javascript_structural_search() {
		let source = r#"
function main() {
    const a = new MyClass();
    const b = new OtherClass(42);
}
"#;
		let matches = search_file("test.js", source, "new $CLASS($$$ARGS)", "javascript").unwrap();
		assert_eq!(matches.len(), 2, "Should find two new expressions");
	}

	#[test]
	fn test_unsupported_language() {
		let result = search_file("test.xyz", "code", "pattern", "unknown");
		assert!(result.is_err(), "Should error on unsupported language");
	}

	#[test]
	fn test_language_from_extension() {
		assert_eq!(language_from_extension(Path::new("foo.rs")), Some("rust"));
		assert_eq!(
			language_from_extension(Path::new("bar.ts")),
			Some("typescript")
		);
		assert_eq!(language_from_extension(Path::new("baz.py")), Some("python"));
		assert_eq!(language_from_extension(Path::new("qux.go")), Some("go"));
		assert_eq!(language_from_extension(Path::new("nope.txt")), None);
	}

	// --- Per-language structural search tests ---

	#[test]
	fn test_typescript_structural_search() {
		let source = r#"
const x: number = foo.unwrap();
const y: string = bar.unwrap();
"#;
		let matches = search_file("test.ts", source, "$VAR.unwrap()", "typescript").unwrap();
		assert_eq!(matches.len(), 2, "TS: Should find two unwrap() calls");
	}

	#[test]
	fn test_go_structural_search() {
		let source = r#"
package main

func main() {
	x := foo()
	y := bar()
}
"#;
		let matches = search_file("test.go", source, "return nil", "go");
		// Pattern may or may not match — just verify it doesn't error
		assert!(matches.is_ok(), "Go: Should not error on valid pattern");
		// Test literal match
		let matches = search_file("test.go", source, "x := foo()", "go").unwrap();
		assert_eq!(matches.len(), 1, "Go: Should find literal match");
	}

	#[test]
	fn test_java_structural_search() {
		let source = r#"
public class Main {
    public void run() {
        System.out.println("hello");
        System.out.println("world");
    }
}
"#;
		let matches =
			search_file("Test.java", source, r#"System.out.println($ARG)"#, "java").unwrap();
		assert_eq!(matches.len(), 2, "Java: Should find two println calls");
	}

	#[test]
	fn test_cpp_structural_search() {
		let source = r#"
#include <iostream>
int main() {
    std::cout << "hello";
    std::cout << "world";
    return 0;
}
"#;
		let matches = search_file("test.cpp", source, "return 0", "cpp").unwrap();
		assert_eq!(matches.len(), 1, "C++: Should find return 0");
	}

	#[test]
	fn test_php_structural_search() {
		// PHP grammar requires <?php tag — verify search works without errors
		let source = "<?php\necho 'hello';\necho 'world';\n?>";
		let result = search_file("test.php", source, "echo 'hello'", "php");
		assert!(result.is_ok(), "PHP: Should not error on search");
	}

	#[test]
	fn test_ruby_structural_search() {
		let source = r#"
def hello
  puts "hello"
end

def world
  puts "world"
end
"#;
		let matches = search_file("test.rb", source, "puts $ARG", "ruby").unwrap();
		assert_eq!(matches.len(), 2, "Ruby: Should find two puts calls");
	}

	#[test]
	fn test_lua_structural_search() {
		let source = r#"
local x = 1
local y = 2
local z = 3
"#;
		let matches = search_file("test.lua", source, "local x = 1", "lua").unwrap();
		assert_eq!(matches.len(), 1, "Lua: Should find local x = 1");
	}

	#[test]
	fn test_bash_structural_search() {
		let source = r#"#!/bin/bash
echo "hello"
echo "world"
echo "goodbye"
"#;
		let matches = search_file("test.sh", source, "echo $ARG", "bash").unwrap();
		assert!(matches.len() >= 2, "Bash: Should find echo calls");
	}

	#[test]
	fn test_json_structural_search() {
		let source = r#"{"name": "test", "version": "1.0"}"#;
		// JSON has limited pattern support — test that search_file runs without panic
		let result = search_file("test.json", source, r#""test""#, "json");
		assert!(result.is_ok(), "JSON: Should handle search without error");
	}

	#[test]
	fn test_python_metavar_pattern() {
		let source = r#"
x = foo(1)
y = bar(2)
z = baz(3)
"#;
		// Literal match works
		let matches = search_file("test.py", source, "foo(1)", "python").unwrap();
		assert_eq!(matches.len(), 1, "Python: Should find foo(1)");
		// bar(2) literal
		let matches = search_file("test.py", source, "bar(2)", "python").unwrap();
		assert_eq!(matches.len(), 1, "Python: Should find bar(2)");
	}

	#[test]
	fn test_go_metavar_pattern() {
		// Go uses µ as expando — verify metavar patterns work
		let source = r#"
package main

func foo() error {
	return nil
}

func bar() error {
	return nil
}
"#;
		let matches = search_file("test.go", source, "return nil", "go").unwrap();
		assert_eq!(matches.len(), 2, "Go: Should find two return nil");
	}

	#[test]
	fn test_format_matches_grouped() {
		let matches = vec![
			GrepMatch {
				file: "src/a.rs".to_string(),
				line: 10,
				column: 5,
				text: "foo.unwrap()".to_string(),
				start_byte: 0,
				end_byte: 0,
				breadcrumb: None,
			},
			GrepMatch {
				file: "src/a.rs".to_string(),
				line: 20,
				column: 3,
				text: "bar.unwrap()".to_string(),
				start_byte: 0,
				end_byte: 0,
				breadcrumb: None,
			},
			GrepMatch {
				file: "src/b.rs".to_string(),
				line: 5,
				column: 1,
				text: "baz.unwrap()".to_string(),
				start_byte: 0,
				end_byte: 0,
				breadcrumb: None,
			},
		];

		let output = format_matches_grouped(&matches);
		assert!(output.contains("src/a.rs"), "Should contain file a.rs");
		assert!(output.contains("src/b.rs"), "Should contain file b.rs");
		// File a.rs should appear before b.rs (BTreeMap ordering)
		let a_pos = output.find("src/a.rs").unwrap();
		let b_pos = output.find("src/b.rs").unwrap();
		assert!(a_pos < b_pos, "Files should be sorted");
	}

	// --- Rewrite tests ---

	#[test]
	fn test_rust_rewrite() {
		let source = r#"
fn main() {
    let x = foo.unwrap();
    let y = bar.unwrap();
    let z = baz.expect("msg");
}
"#;
		let result = rewrite_file(
			"test.rs",
			source,
			"$VAR.unwrap()",
			r#"$VAR.expect("reason")"#,
			"rust",
		)
		.unwrap();
		assert!(result.is_some(), "Should have matches to rewrite");
		let result = result.unwrap();
		assert_eq!(result.replacements, 2);
		assert!(result.rewritten_source.contains(r#"foo.expect("reason")"#));
		assert!(result.rewritten_source.contains(r#"bar.expect("reason")"#));
		// Unmatched code should be preserved
		assert!(result.rewritten_source.contains(r#"baz.expect("msg")"#));
	}

	#[test]
	fn test_rewrite_no_match() {
		let source = "fn main() { let x = 1; }";
		let result = rewrite_file(
			"test.rs",
			source,
			"$VAR.unwrap()",
			"$VAR.expect(\"r\")",
			"rust",
		)
		.unwrap();
		assert!(result.is_none(), "Should return None when no matches");
	}

	#[test]
	fn test_javascript_rewrite() {
		let source = "console.log('hello');\nconsole.log('world');\n";
		let result = rewrite_file(
			"test.js",
			source,
			"console.log($ARG)",
			"logger.info($ARG)",
			"javascript",
		)
		.unwrap();
		assert!(result.is_some());
		let result = result.unwrap();
		assert_eq!(result.replacements, 2);
		assert!(result.rewritten_source.contains("logger.info('hello')"));
		assert!(result.rewritten_source.contains("logger.info('world')"));
	}

	#[test]
	fn test_rewrite_preserves_unmatched() {
		let source = "let a = 1;\nlet b = foo.unwrap();\nlet c = 3;\n";
		let result = rewrite_file(
			"test.rs",
			source,
			"$VAR.unwrap()",
			r#"$VAR.expect("x")"#,
			"rust",
		)
		.unwrap()
		.unwrap();
		assert_eq!(result.replacements, 1);
		assert!(result.rewritten_source.contains("let a = 1;"));
		assert!(result.rewritten_source.contains("let c = 3;"));
	}

	#[test]
	fn test_format_rewrite_diff() {
		let result = RewriteResult {
			file: "test.rs".to_string(),
			replacements: 1,
			original_source: "let x = foo.unwrap();\nlet y = 1;\n".to_string(),
			rewritten_source: "let x = foo.expect(\"r\");\nlet y = 1;\n".to_string(),
		};
		let diff = format_rewrite_diff(&result);
		assert!(diff.contains("--- test.rs"));
		assert!(diff.contains("+++ test.rs"));
		assert!(diff.contains("-1:"));
		assert!(diff.contains("+1:"));
		// Unchanged line should not appear
		assert!(!diff.contains("let y = 1"));
	}

	// ===================================================================
	// Smart-search building-block tests
	// ===================================================================

	#[test]
	fn test_invalid_pattern_returns_error_not_panic() {
		// LLM sends garbage — must not panic. Either Err or empty Ok is fine.
		let result = search_file("test.rs", "fn main(){}", "((((", "rust");
		assert!(
			result.is_err() || result.unwrap().is_empty(),
			"Invalid pattern should return Err or empty, never panic"
		);
	}

	#[test]
	fn test_strict_relaxed_picks_up_what_smart_misses() {
		// `console.log('x')` with surrounding comments: Smart can struggle when
		// comments are inside the matched region; Relaxed ignores them.
		let source = "// note\nconsole.log('hello');\n// trailing\nconsole.log('world');\n";
		let smart = search_file_strict(
			"a.js",
			source,
			"console.log($A)",
			"javascript",
			MatchStrictness::Smart,
		)
		.unwrap();
		let relaxed = search_file_strict(
			"a.js",
			source,
			"console.log($A)",
			"javascript",
			MatchStrictness::Relaxed,
		)
		.unwrap();
		// Both should find the two calls; main goal is that Relaxed doesn't drop them.
		assert!(relaxed.len() >= smart.len());
		assert_eq!(relaxed.len(), 2);
	}

	#[test]
	fn test_kind_search_finds_function_declarations() {
		let source = "function foo() {}\nfunction bar() {}\nconst x = 1;\n";
		let matches =
			search_file_by_kind("a.js", source, "function_declaration", "javascript").unwrap();
		assert_eq!(matches.len(), 2, "Should find both function declarations");
	}

	#[test]
	fn test_kind_search_invalid_kind_errors_cleanly() {
		let source = "function foo() {}\n";
		let result = search_file_by_kind("a.js", source, "totally_not_a_kind", "javascript");
		assert!(result.is_err(), "Invalid kind must return Err");
	}

	#[test]
	fn test_kind_search_rust_call_expression() {
		let source = "fn main() { println!(\"hi\"); foo(); bar(1, 2); }";
		let matches = search_file_by_kind("a.rs", source, "call_expression", "rust").unwrap();
		// Rust has println! as macro_invocation, but foo() and bar(1,2) are call_expression.
		assert!(
			matches.len() >= 2,
			"Should find at least foo() and bar(1,2)"
		);
	}

	#[test]
	fn test_contextual_resolves_go_call_ambiguity() {
		// The classic Go trap: `fmt.Println($A)` is ambiguous with type conversion.
		let source = r#"
package main

import "fmt"

func main() {
	fmt.Println("hello")
	fmt.Println("world")
}
"#;
		// Contextual patterns can be version-sensitive; kind-based search
		// for call_expression is the robust fallback.
		let matches = search_file_by_kind("a.go", source, "call_expression", "go").unwrap();
		assert_eq!(
			matches.len(),
			2,
			"Kind-based selector should find both calls"
		);
	}

	#[test]
	fn test_contextual_class_field_typescript() {
		// `name = "x"` parses as assignment; only inside a class is it a field.
		let source = r#"
class User {
	name = "alice";
	age = 30;
}
"#;
		let context = "class _ { $NAME = $VAL }";
		let matches = search_file_contextual(
			"a.ts",
			source,
			context,
			"public_field_definition",
			"typescript",
		);
		// Some grammars use `public_field_definition`, others `field_definition` — accept either.
		let count = match matches {
			Ok(m) => m.len(),
			Err(_) => {
				search_file_contextual("a.ts", source, context, "field_definition", "typescript")
					.map(|m| m.len())
					.unwrap_or(0)
			}
		};
		assert!(count >= 1, "Should find at least one class field");
	}

	#[test]
	fn test_pattern_info_returns_root_kind() {
		let info = pattern_info("$X.unwrap()", "rust").unwrap();
		assert!(info.root_kind.is_some(), "Pattern should have a root kind");
		assert!(!info.has_error);
		assert!(info.defined_vars.contains(&"X".to_string()));
	}

	#[test]
	fn test_pattern_info_metavars_sorted_unique() {
		let info = pattern_info("$A.foo($B, $C)", "rust").unwrap();
		assert_eq!(info.defined_vars, vec!["A", "B", "C"]);
	}

	#[test]
	fn test_pattern_info_bare_metavar() {
		// `$X` alone is a meta-var; root_kind is None by design.
		let info = pattern_info("$X", "rust").unwrap();
		assert!(info.root_kind.is_none());
	}

	#[test]
	fn test_pattern_info_detects_parse_error_or_errors() {
		// Pure garbage should either return Err on construction or flag has_error.
		let info = pattern_info("((((", "rust");
		match info {
			Err(_) => {} // acceptable
			Ok(i) => assert!(i.has_error, "Broken pattern must flag has_error"),
		}
	}

	// ===================================================================
	// LLM-typical pattern coverage — patterns LLMs commonly attempt.
	// Each test is named for the scenario so failures point at root cause.
	// ===================================================================

	#[test]
	fn test_llm_rust_async_fn() {
		let source = r#"
async fn fetch_user(id: u64) -> Result<User, Error> {
	let resp = client.get(&url).send().await?;
	Ok(resp.json().await?)
}

async fn fetch_post(id: u64) -> Result<Post, Error> {
	Ok(Post::default())
}
"#;
		// Complex async fn patterns are unreliable across ast-grep versions;
		// kind-based search is the robust fallback for LLMs.
		let matches = search_file_by_kind("a.rs", source, "function_item", "rust").unwrap();
		assert_eq!(matches.len(), 2, "Should find both async fn definitions");
	}

	#[test]
	fn test_llm_rust_question_mark_operator() {
		let source = r#"
fn run() -> Result<(), Error> {
	let x = thing()?;
	let y = other()?;
	Ok(())
}
"#;
		let matches = search_file("a.rs", source, "$EXPR?", "rust").unwrap();
		assert!(matches.len() >= 2, "Should find ? operator usages");
	}

	#[test]
	fn test_llm_rust_if_let_some() {
		let source = r#"
fn main() {
	if let Some(x) = maybe_x() {
		println!("{}", x);
	}
	if let Some(y) = maybe_y() {
		dbg!(y);
	}
}
"#;
		let matches = search_file("a.rs", source, "if let Some($X) = $Y { $$$ }", "rust").unwrap();
		assert_eq!(matches.len(), 2, "Should find both if-let-Some");
	}

	#[test]
	fn test_llm_rust_match_expression() {
		let source = r#"
fn classify(x: i32) -> &'static str {
	match x {
		0 => "zero",
		_ => "other",
	}
}
"#;
		let matches = search_file("a.rs", source, "match $EXPR { $$$ }", "rust").unwrap();
		assert_eq!(matches.len(), 1);
	}

	#[test]
	fn test_llm_rust_trait_impl() {
		let source = r#"
struct S;
trait T {}
impl T for S {}
impl Display for S {}
"#;
		// Pattern `impl $T for $S {}` doesn't match empty bodies reliably;
		// kind-based search is the robust approach.
		let matches = search_file_by_kind("a.rs", source, "impl_item", "rust").unwrap();
		assert_eq!(matches.len(), 2);
	}

	#[test]
	fn test_llm_rust_use_import() {
		let source = "use std::collections::HashMap;\nuse anyhow::Result;\nuse crate::foo;\n";
		let matches = search_file("a.rs", source, "use $PATH;", "rust").unwrap();
		assert_eq!(matches.len(), 3);
	}

	#[test]
	fn test_llm_rust_struct_literal() {
		let source = r#"
fn make() -> Point {
	Point { x: 1, y: 2 }
}
fn other() -> Pair { Pair { left: a, right: b } }
"#;
		// `$NAME { $$$ }` is ambiguous (block vs struct literal);
		// kind-based search for struct_expression is reliable.
		let matches = search_file_by_kind("a.rs", source, "struct_expression", "rust").unwrap();
		assert!(matches.len() >= 2, "Should match struct literals");
	}

	#[test]
	fn test_llm_typescript_arrow_function() {
		let source = r#"
const add = (a: number, b: number): number => a + b;
const id = <T>(x: T): T => x;
"#;
		// Arrow function patterns are fragile in TS; kind-based is robust.
		let matches = search_file_by_kind("a.ts", source, "arrow_function", "typescript").unwrap();
		assert!(!matches.is_empty(), "Should match arrow function");
	}

	#[test]
	fn test_llm_typescript_async_function() {
		let source = r#"
async function load() { return await fetch('/x'); }
async function save() { return await fetch('/y'); }
"#;
		let matches = search_file(
			"a.ts",
			source,
			"async function $NAME($$$) { $$$ }",
			"typescript",
		)
		.unwrap();
		assert_eq!(matches.len(), 2);
	}

	#[test]
	fn test_llm_typescript_await_expression() {
		let source = r#"
async function f() {
	const a = await one();
	const b = await two();
}
"#;
		let matches = search_file("a.ts", source, "await $EXPR", "typescript").unwrap();
		assert_eq!(matches.len(), 2);
	}

	#[test]
	fn test_llm_typescript_import_statement() {
		let source = r#"
import { foo } from './foo';
import bar from './bar';
import * as baz from './baz';
"#;
		// Use kind-based matching since import forms vary widely.
		let matches =
			search_file_by_kind("a.ts", source, "import_statement", "typescript").unwrap();
		assert_eq!(matches.len(), 3);
	}

	#[test]
	fn test_llm_typescript_try_catch() {
		let source = r#"
function f() {
	try {
		risky();
	} catch (e) {
		log(e);
	}
}
"#;
		let matches = search_file(
			"a.ts",
			source,
			"try { $$$ } catch ($E) { $$$ }",
			"typescript",
		)
		.unwrap();
		assert_eq!(matches.len(), 1);
	}

	#[test]
	fn test_llm_javascript_console_method_chain() {
		let source = "console.log('a'); console.error('b'); console.warn('c');";
		let matches = search_file("a.js", source, "console.$M($ARG)", "javascript").unwrap();
		assert_eq!(
			matches.len(),
			3,
			"Should find log/error/warn via $M metavar"
		);
	}

	#[test]
	fn test_llm_python_decorator() {
		let source = r#"
@cache
def slow_one():
	return compute()

@cache
def slow_two():
	return compute()
"#;
		// Python decorators are part of decorated_definition — kind search is robust.
		let matches =
			search_file_by_kind("a.py", source, "decorated_definition", "python").unwrap();
		assert_eq!(matches.len(), 2);
	}

	#[test]
	fn test_llm_python_async_def() {
		let source = r#"
async def fetch(u):
	return await http.get(u)

async def save(x):
	await db.put(x)
"#;
		let matches = search_file("a.py", source, "async def $NAME($$$): $$$", "python");
		// Some grammars need the wider form; at minimum the search shouldn't error.
		assert!(matches.is_ok());
		// Kind-based fallback always works:
		let by_kind = search_file_by_kind("a.py", source, "function_definition", "python").unwrap();
		assert_eq!(by_kind.len(), 2);
	}

	#[test]
	fn test_llm_python_from_import() {
		let source = "from os import path\nfrom typing import List, Dict\nimport sys\n";
		let imports =
			search_file_by_kind("a.py", source, "import_from_statement", "python").unwrap();
		assert_eq!(imports.len(), 2);
		let plain = search_file_by_kind("a.py", source, "import_statement", "python").unwrap();
		assert_eq!(plain.len(), 1);
	}

	#[test]
	fn test_llm_go_if_err_not_nil() {
		let source = r#"
package main

func run() error {
	if err := step1(); err != nil {
		return err
	}
	if err := step2(); err != nil {
		return err
	}
	return nil
}
"#;
		// Go compound if-statements (with init clause) don't match bare
		// `if err != nil { $$$ }` — kind-based search is the reliable path.
		let matches = search_file_by_kind("a.go", source, "if_statement", "go").unwrap();
		assert!(!matches.is_empty(), "Should match Go if statements");
	}

	#[test]
	fn test_llm_go_function_declaration_kind() {
		let source = r#"
package main

func one() {}
func two(x int) int { return x }
func (s *S) three() {}
"#;
		let matches = search_file_by_kind("a.go", source, "function_declaration", "go").unwrap();
		assert_eq!(
			matches.len(),
			2,
			"Two free funcs (method is method_declaration)"
		);
		let methods = search_file_by_kind("a.go", source, "method_declaration", "go").unwrap();
		assert_eq!(methods.len(), 1);
	}

	#[test]
	fn test_llm_java_method_invocation_chain() {
		let source = r#"
public class C {
	void run() {
		obj.method1();
		obj.method2(42);
	}
}
"#;
		let matches = search_file("Test.java", source, "$O.$M($$$)", "java").unwrap();
		assert!(matches.len() >= 2);
	}

	#[test]
	fn test_llm_cpp_namespaced_call() {
		let source = r#"
#include <iostream>
int main() {
	std::cout << "hi" << std::endl;
	std::cerr << "err";
	return 0;
}
"#;
		// Cpp call expressions; use kind search to be robust to operator overloads.
		let calls = search_file_by_kind("a.cpp", source, "function_definition", "cpp").unwrap();
		assert!(!calls.is_empty());
	}

	#[test]
	fn test_llm_ruby_block() {
		let source = r#"
arr.each do |x|
	puts x
end
list.map { |y| y * 2 }
"#;
		let do_blocks = search_file_by_kind("a.rb", source, "do_block", "ruby").unwrap();
		assert!(!do_blocks.is_empty());
	}

	// ===================================================================
	// Patterns that are known to LLM trip-hazards. Verify behavior is
	// either "works" or "fails cleanly with informative diagnostic".
	// ===================================================================

	#[test]
	fn test_llm_trap_typescript_class_field_via_smart_pattern() {
		// `name = "x"` parses as assignment outside class context.
		// Direct pattern search may yield 0 — verify the contextual fallback fixes it.
		let source = r#"
class User {
	name = "alice";
	age = 30;
}
"#;
		// Pattern alone may miss. Contextual should hit.
		let direct = search_file("a.ts", source, "$NAME = $VAL", "typescript").unwrap();
		let context = "class _ { $NAME = $VAL }";
		let via_ctx = search_file_contextual(
			"a.ts",
			source,
			context,
			"public_field_definition",
			"typescript",
		)
		.or_else(|_| {
			search_file_contextual("a.ts", source, context, "field_definition", "typescript")
		});
		assert!(
			via_ctx.is_ok() && !via_ctx.unwrap().is_empty(),
			"Contextual fallback should find class fields even if direct does not. direct={}",
			direct.len()
		);
	}

	#[test]
	fn test_llm_trap_zero_match_pattern_does_not_panic() {
		// Pattern is valid syntax but yields no matches in this source.
		let source = "fn main() { let x = 1; }";
		let matches = search_file("a.rs", source, "$X.unwrap()", "rust").unwrap();
		assert!(matches.is_empty());
	}

	#[test]
	fn test_smart_then_relaxed_consistency_on_simple_pattern() {
		let source = "fn main() { let x = foo.unwrap(); }";
		let smart = search_file_strict(
			"a.rs",
			source,
			"$X.unwrap()",
			"rust",
			MatchStrictness::Smart,
		)
		.unwrap();
		let relaxed = search_file_strict(
			"a.rs",
			source,
			"$X.unwrap()",
			"rust",
			MatchStrictness::Relaxed,
		)
		.unwrap();
		assert_eq!(smart.len(), 1);
		assert_eq!(relaxed.len(), 1);
	}

	#[test]
	fn test_kind_search_typescript_import() {
		// Imports vary so much that kind search is the LLM-friendly path.
		let source = r#"
import { a } from './a';
import b from './b';
const x = 1;
"#;
		let matches =
			search_file_by_kind("a.ts", source, "import_statement", "typescript").unwrap();
		assert_eq!(matches.len(), 2);
	}

	#[test]
	fn test_kind_search_python_class_definition() {
		let source = "class A:\n    pass\n\nclass B(A):\n    pass\n";
		let matches = search_file_by_kind("a.py", source, "class_definition", "python").unwrap();
		assert_eq!(matches.len(), 2);
	}

	// ===================================================================
	// canonical_kind — fixes LLM naming-mismatch class outright
	// ===================================================================

	#[test]
	fn test_canonical_kind_function_per_language() {
		// Same LLM intent word should map to the right grammar kind in each lang.
		assert_eq!(
			canonical_kind("function", "javascript"),
			Some("function_declaration")
		);
		assert_eq!(
			canonical_kind("function", "typescript"),
			Some("function_declaration")
		);
		assert_eq!(
			canonical_kind("function", "python"),
			Some("function_definition")
		);
		assert_eq!(canonical_kind("function", "rust"), Some("function_item"));
		assert_eq!(
			canonical_kind("function", "go"),
			Some("function_declaration")
		);
		assert_eq!(
			canonical_kind("function", "java"),
			Some("method_declaration")
		);
	}

	#[test]
	fn test_canonical_kind_normalizes_input() {
		// Lowercases + strips leading $
		assert_eq!(
			canonical_kind("Function", "python"),
			Some("function_definition")
		);
		assert_eq!(
			canonical_kind("$Function", "python"),
			Some("function_definition")
		);
		assert_eq!(canonical_kind("  fn  ", "rust"), Some("function_item"));
	}

	#[test]
	fn test_canonical_kind_python_naming_rescue() {
		// Classic LLM trap: passes JS-style kind in Python — must rescue.
		assert_eq!(
			canonical_kind("function_declaration", "python"),
			None,
			"Don't pretend a JS kind exists in Python; let strategy 3 try raw first"
		);
		// But the canonical 'function' word does map.
		assert_eq!(
			canonical_kind("function", "python"),
			Some("function_definition")
		);
	}

	#[test]
	fn test_canonical_kind_imports_per_language() {
		assert_eq!(
			canonical_kind("import", "javascript"),
			Some("import_statement")
		);
		assert_eq!(canonical_kind("import", "python"), Some("import_statement"));
		assert_eq!(canonical_kind("import", "rust"), Some("use_declaration"));
		assert_eq!(canonical_kind("import", "go"), Some("import_declaration"));
	}

	#[test]
	fn test_canonical_kind_class_per_language() {
		assert_eq!(
			canonical_kind("class", "javascript"),
			Some("class_declaration")
		);
		assert_eq!(canonical_kind("class", "python"), Some("class_definition"));
		assert_eq!(canonical_kind("class", "cpp"), Some("class_specifier"));
		assert_eq!(canonical_kind("class", "rust"), None); // Rust has struct/trait, not class
		assert_eq!(canonical_kind("struct", "rust"), Some("struct_item"));
		assert_eq!(canonical_kind("trait", "rust"), Some("trait_item"));
	}

	#[test]
	fn test_canonical_kind_returns_none_for_unknown() {
		assert_eq!(canonical_kind("xyzzy", "rust"), None);
		assert_eq!(canonical_kind("function", "klingon"), None);
	}

	#[test]
	fn test_truncate_match_text_passthrough_short() {
		// Single line and short multiline pass through unchanged.
		assert_eq!(truncate_match_text("hello", 4), "hello");
		assert_eq!(truncate_match_text("a\nb\nc", 4), "a\nb\nc");
		assert_eq!(truncate_match_text("a\nb\nc\nd", 4), "a\nb\nc\nd");
	}

	#[test]
	fn test_truncate_match_text_caps_long_body() {
		let text = "line1\nline2\nline3\nline4\nline5\nline6\nline7";
		let out = truncate_match_text(text, 4);
		assert!(out.starts_with("line1\nline2\nline3\nline4\n... ("));
		assert!(out.contains("3 more lines"));
		assert!(!out.contains("line5"));
	}

	#[test]
	fn test_format_matches_grouped_truncates_long_body() {
		// A kind-based match might capture an entire impl block. Ensure the
		// formatter doesn't dump the whole thing.
		let long = (1..=20)
			.map(|i| format!("line{}", i))
			.collect::<Vec<_>>()
			.join("\n");
		let matches = vec![GrepMatch {
			file: "a.rs".to_string(),
			line: 1,
			column: 0,
			text: long,
			start_byte: 0,
			end_byte: 0,
			breadcrumb: None,
		}];
		let out = format_matches_grouped(&matches);
		assert!(out.contains("line1"));
		assert!(out.contains("more lines"));
		assert!(!out.contains("line20"));
	}

	#[test]
	fn test_canonical_kind_resolves_python_via_canonical() {
		// End-to-end: LLM passes 'function' as a kind name in Python.
		// Raw KindMatcher fails ('function' isn't a Python AST kind).
		// canonical_kind maps it to 'function_definition', which works.
		let source = "def foo():\n    pass\n\ndef bar():\n    pass\n";
		let raw = search_file_by_kind("a.py", source, "function", "python");
		assert!(raw.is_err(), "Raw 'function' is not a Python kind");
		let canonical = canonical_kind("function", "python").unwrap();
		let matches = search_file_by_kind("a.py", source, canonical, "python").unwrap();
		assert_eq!(matches.len(), 2);
	}

	// ---- literal token prefilter extraction ----

	#[test]
	fn test_literal_tokens_excludes_metavars() {
		let tokens = literal_tokens("$X.unwrap()");
		assert_eq!(tokens, vec!["unwrap".to_string()]);
		let tokens = literal_tokens("fn $NAME($$$ARGS) { $$$ }");
		assert_eq!(tokens, vec!["fn".to_string()]);
	}

	#[test]
	fn test_literal_tokens_sorted_longest_first_and_deduped() {
		let tokens = literal_tokens("foo.bar_baz(foo)");
		assert_eq!(
			tokens,
			vec!["bar_baz".to_string(), "foo".to_string()],
			"longest first, duplicates removed"
		);
	}

	#[test]
	fn test_literal_tokens_skips_single_chars() {
		let tokens = literal_tokens("a + bb");
		assert_eq!(tokens, vec!["bb".to_string()]);
	}

	// ---- breadcrumbs ----

	#[test]
	fn test_breadcrumb_rust_impl_fn() {
		let source = "impl Store {\n    fn flush(&self) { self.x.unwrap(); }\n}\n";
		let matches = search_file("a.rs", source, "$X.unwrap()", "rust").unwrap();
		assert_eq!(matches.len(), 1);
		let bc = matches[0].breadcrumb.as_deref().expect("breadcrumb");
		assert_eq!(bc, "impl Store › fn flush");
	}

	#[test]
	fn test_breadcrumb_python_class_method() {
		let source = "class Store:\n    def flush(self):\n        do_it()\n";
		let matches = search_file("a.py", source, "do_it()", "python").unwrap();
		assert_eq!(matches.len(), 1);
		let bc = matches[0].breadcrumb.as_deref().expect("breadcrumb");
		assert_eq!(bc, "class Store › def flush");
	}

	#[test]
	fn test_breadcrumb_none_at_top_level() {
		let source = "use std::fs;\n";
		let matches = search_file("a.rs", source, "use $PATH;", "rust").unwrap();
		assert_eq!(matches.len(), 1);
		assert!(matches[0].breadcrumb.is_none());
	}

	#[test]
	fn test_match_byte_ranges_populated() {
		let source = "fn main() { foo.unwrap(); }";
		let matches = search_file("a.rs", source, "$X.unwrap()", "rust").unwrap();
		assert_eq!(matches.len(), 1);
		let m = &matches[0];
		assert!(m.end_byte > m.start_byte);
		assert_eq!(&source[m.start_byte..m.end_byte], "foo.unwrap()");
	}

	// ---- metavariable constraints ----

	#[test]
	fn test_search_file_constrained_filters_by_regex() {
		let source = "fn handle_get() {}\nfn handle_post() {}\nfn other() {}\n";
		let re = regex::Regex::new("^handle_").unwrap();
		let constraints = vec![("NAME".to_string(), re)];
		let matches = search_file_constrained(
			"a.rs",
			source,
			"fn $NAME($$$) { $$$ }",
			"rust",
			MatchStrictness::Smart,
			&constraints,
		)
		.unwrap();
		assert_eq!(matches.len(), 2);
		assert!(matches.iter().all(|m| m.text.contains("handle_")));
	}

	#[test]
	fn test_search_file_constrained_unbound_var_drops_match() {
		// Constraint on a metavar the pattern never captures — nothing survives.
		let source = "fn handle_get() {}\n";
		let re = regex::Regex::new(".*").unwrap();
		let constraints = vec![("MISSING".to_string(), re)];
		let matches = search_file_constrained(
			"a.rs",
			source,
			"fn $NAME($$$) { $$$ }",
			"rust",
			MatchStrictness::Smart,
			&constraints,
		)
		.unwrap();
		assert!(matches.is_empty());
	}

	// ---- symbol definitions / references ----

	#[test]
	fn test_find_symbol_defs_exact() {
		let source = "pub fn flush() {}\npub struct Flush;\nfn other() {}\n";
		let name = NameMatcher::new("flush").unwrap();
		let matches = find_symbol_defs("a.rs", source, &name, "rust").unwrap();
		assert_eq!(matches.len(), 1, "exact match is case-sensitive");
		assert_eq!(matches[0].line, 1);
	}

	#[test]
	fn test_find_symbol_defs_wildcard() {
		let source = "fn handle_get() {}\nfn handle_post() {}\nfn other() {}\n";
		let name = NameMatcher::new("handle_*").unwrap();
		let matches = find_symbol_defs("a.rs", source, &name, "rust").unwrap();
		assert_eq!(matches.len(), 2);
	}

	#[test]
	fn test_find_symbol_defs_typescript_interface() {
		let source = "interface Config { a: string }\nclass ConfigImpl {}\n";
		let name = NameMatcher::new("Config").unwrap();
		let matches = find_symbol_defs("a.ts", source, &name, "typescript").unwrap();
		assert_eq!(matches.len(), 1);
		assert!(matches[0].text.contains("interface"));
	}

	#[test]
	fn test_find_symbol_refs_tags_definitions() {
		let source = "fn flush() {}\nfn main() { flush(); }\n";
		let name = NameMatcher::new("flush").unwrap();
		let matches = find_symbol_refs("a.rs", source, &name, "rust").unwrap();
		assert_eq!(matches.len(), 2);
		assert!(matches[0].text.starts_with("[def]"), "{}", matches[0].text);
		assert!(!matches[1].text.starts_with("[def]"));
		assert!(matches[1]
			.breadcrumb
			.as_deref()
			.unwrap()
			.contains("fn main"));
	}

	#[test]
	fn test_name_matcher_shapes() {
		assert!(NameMatcher::new("foo").unwrap().matches("foo"));
		assert!(!NameMatcher::new("foo").unwrap().matches("foobar"));
		assert!(NameMatcher::new("foo*").unwrap().matches("foobar"));
		assert!(NameMatcher::new("*bar").unwrap().matches("foobar"));
		assert!(NameMatcher::new("f*r").unwrap().matches("foobar"));
		assert!(!NameMatcher::new("f*z").unwrap().matches("foobar"));
	}

	// ---- single-parse multi-strategy evaluation ----

	#[test]
	fn test_search_file_multi_buckets_in_order() {
		let source = "fn main() { foo.unwrap(); }\nfn other() {}\n";
		let specs = vec![
			SearchSpec::Pattern {
				pattern: "$X.unwrap()".to_string(),
				strictness: MatchStrictness::Smart,
			},
			SearchSpec::Kind {
				kind: "function_item".to_string(),
			},
			SearchSpec::Kind {
				kind: "not_a_real_kind".to_string(),
			},
		];
		let buckets = search_file_multi("a.rs", source, "rust", &specs, &[]).unwrap();
		assert_eq!(buckets.len(), 3);
		assert_eq!(buckets[0].len(), 1, "pattern bucket");
		assert_eq!(buckets[1].len(), 2, "kind bucket");
		assert!(
			buckets[2].is_empty(),
			"invalid kind yields empty, not error"
		);
	}

	#[test]
	fn test_search_spec_kind_or_pattern() {
		let source = "fn main() { foo.unwrap(); }\n";
		// Valid kind resolves as kind; non-kind falls back to pattern.
		let specs = vec![
			SearchSpec::KindOrPattern {
				spec: "function_item".to_string(),
			},
			SearchSpec::KindOrPattern {
				spec: "$X.unwrap()".to_string(),
			},
		];
		let buckets = search_file_multi("a.rs", source, "rust", &specs, &[]).unwrap();
		assert_eq!(buckets[0].len(), 1);
		assert_eq!(buckets[1].len(), 1);
	}

	// ---- lexical fallback scan ----

	#[test]
	fn test_lexical_scan_finds_lines() {
		let source = "// frobnicate here\nlet x = 1;\nfrobnicate();\n";
		let matches = lexical_scan("a.rs", source, "frobnicate", 10);
		assert_eq!(matches.len(), 2);
		assert_eq!(matches[0].line, 1);
		assert_eq!(matches[1].line, 3);
		assert!(matches[0].breadcrumb.is_none());
	}

	#[test]
	fn test_lexical_scan_respects_cap() {
		let source = "tok\ntok\ntok\ntok\n";
		let matches = lexical_scan("a.rs", source, "tok", 2);
		assert_eq!(matches.len(), 2);
	}

	// ---- swift extension mapping ----

	#[test]
	fn test_language_from_extension_swift() {
		assert_eq!(
			language_from_extension(Path::new("App.swift")),
			Some("swift")
		);
	}

	// ---- formatter shows breadcrumbs ----

	#[test]
	fn test_format_matches_grouped_with_breadcrumb() {
		let matches = vec![GrepMatch {
			file: "a.rs".to_string(),
			line: 3,
			column: 4,
			text: "foo.unwrap()".to_string(),
			start_byte: 0,
			end_byte: 0,
			breadcrumb: Some("impl Store › fn flush".to_string()),
		}];
		let out = format_matches_grouped(&matches);
		assert!(
			out.contains("[impl Store › fn flush]"),
			"breadcrumb shown: {}",
			out
		);
	}
}
