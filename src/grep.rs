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
use ast_grep_core::matcher::PatternBuilder;
use ast_grep_core::tree_sitter::{LanguageExt, StrDoc};
use ast_grep_core::{Pattern, PatternError};
use std::path::Path;

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
define_ast_grep_lang!(AstPython, tree_sitter_python::LANGUAGE, '$');
define_ast_grep_lang!(AstGo, tree_sitter_go::LANGUAGE, '$');
define_ast_grep_lang!(AstJava, tree_sitter_java::LANGUAGE, '$');
define_ast_grep_lang!(AstCpp, tree_sitter_cpp::LANGUAGE, '$');
define_ast_grep_lang!(AstPhp, tree_sitter_php::LANGUAGE_PHP, '#');
define_ast_grep_lang!(AstRuby, tree_sitter_ruby::LANGUAGE, '$');
define_ast_grep_lang!(AstLua, tree_sitter_lua::LANGUAGE, '$');
define_ast_grep_lang!(AstBash, tree_sitter_bash::LANGUAGE, '$');
define_ast_grep_lang!(AstCss, tree_sitter_css::LANGUAGE, '$');
define_ast_grep_lang!(AstJson, tree_sitter_json::LANGUAGE, '$');

/// A single match result from structural search.
pub struct GrepMatch {
	pub file: String,
	pub line: usize,
	pub column: usize,
	pub text: String,
}

/// Search a single file with the given pattern and language.
fn search_file_with_lang<L: LanguageExt + AstGrepLanguage>(
	lang: L,
	source: &str,
	pattern_str: &str,
) -> Result<Vec<(usize, usize, String)>> {
	let grep = lang.ast_grep(source);
	let pattern = Pattern::new(pattern_str, lang);
	let mut results = Vec::new();
	for m in grep.root().find_all(&pattern) {
		let text = m.text().to_string();
		let pos = m.start_pos();
		let line = pos.line() + 1; // 0-based → 1-based
		let col = pos.byte_point().1;
		results.push((line, col, text));
	}
	Ok(results)
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
		"c" | "cc" | "cpp" | "cxx" | "h" | "hpp" | "hxx" => Some("cpp"),
		"php" => Some("php"),
		"rb" => Some("ruby"),
		"lua" => Some("lua"),
		"sh" | "bash" | "zsh" => Some("bash"),
		"css" | "scss" => Some("css"),
		"json" => Some("json"),
		_ => None,
	}
}

/// Search a single file for the given pattern. Returns matches.
pub fn search_file(
	file_path: &str,
	source: &str,
	pattern: &str,
	language: &str,
) -> Result<Vec<GrepMatch>> {
	let raw_matches = match language {
		"rust" => search_file_with_lang(AstRust, source, pattern),
		"javascript" => search_file_with_lang(AstJavaScript, source, pattern),
		"typescript" => search_file_with_lang(AstTypeScript, source, pattern),
		"python" => search_file_with_lang(AstPython, source, pattern),
		"go" => search_file_with_lang(AstGo, source, pattern),
		"java" => search_file_with_lang(AstJava, source, pattern),
		"cpp" => search_file_with_lang(AstCpp, source, pattern),
		"php" => search_file_with_lang(AstPhp, source, pattern),
		"ruby" => search_file_with_lang(AstRuby, source, pattern),
		"lua" => search_file_with_lang(AstLua, source, pattern),
		"bash" => search_file_with_lang(AstBash, source, pattern),
		"css" => search_file_with_lang(AstCss, source, pattern),
		"json" => search_file_with_lang(AstJson, source, pattern),
		_ => bail!("Unsupported language: {}", language),
	}?;

	Ok(raw_matches
		.into_iter()
		.map(|(line, column, text)| GrepMatch {
			file: file_path.to_string(),
			line,
			column,
			text,
		})
		.collect())
}

/// Format matches grouped by file (token-efficient output).
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
			output.push_str(&format!("{}:{}:  {}\n", m.line, m.column, m.text));
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
				let start = m.line.saturating_sub(context + 1);
				let end = (m.line + context).min(lines.len());
				for i in start..end {
					let prefix = if i + 1 == m.line { ">" } else { " " };
					output.push_str(&format!("{} {}:  {}\n", prefix, i + 1, lines[i]));
				}
				output.push_str("---\n");
			}
		} else {
			for m in file_matches {
				output.push_str(&format!("{}:{}:  {}\n", m.line, m.column, m.text));
			}
		}
		output.push('\n');
	}

	output.trim_end().to_string()
}
