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

//! Smart structural-search pipeline for the MCP `structural_search` tool.
//!
//! Execution model (zero standing index):
//!   1. A parallel gitignore-aware walk collects candidate files for the
//!      target language, reading contents and evaluating a literal-token
//!      prefilter inline (a pattern's concrete tokens must appear verbatim
//!      in any file it can match, so failing files are never parsed).
//!   2. Pass A parses ONLY prefilter-surviving files — once per file — and
//!      runs the pattern strategies (Smart, Relaxed) plus the `inside`/`has`
//!      relational specs against that single shared parse.
//!   3. Pass B (only when Pass A found nothing) parses every candidate file
//!      once and runs the kind/contextual fallback strategies the same way.
//!   4. When everything misses, a labeled lexical line scan answers instead
//!      of returning empty-handed, alongside the structural diagnostic.
//!
//! A single-entry query cache (request fingerprint + repo stamp) makes
//! repeat and pagination calls skip parsing entirely.

use crate::grep::{self, GrepMatch, MetavarConstraint};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

/// Hard cap on matches kept per file (protects against e.g. kind=identifier).
const MAX_PER_FILE_MATCHES: usize = 500;
/// Hard cap on total matches kept per query. The steering footer reports
/// when this cap was hit so the LLM knows the count is a floor.
pub const MAX_TOTAL_MATCHES: usize = 10_000;
/// Caps for the lexical fallback scan.
const LEXICAL_PER_FILE: usize = 20;
const LEXICAL_TOTAL: usize = 200;
/// Files larger than this are skipped (generated bundles, vendored blobs).
const MAX_FILE_SIZE: u64 = 5_000_000;

/// A candidate file loaded into memory by the parallel walk.
pub struct FileData {
	pub path: PathBuf,
	pub display: String,
	pub content: String,
	/// True when the content contains every literal prefilter token.
	pub prefilter_hit: bool,
}

/// Stamp of the candidate file set, used for cache staleness checks.
#[derive(PartialEq, Eq, Clone, Copy, Debug, Default)]
pub struct RepoStamp {
	pub file_count: usize,
	pub total_size: u64,
	pub max_mtime: Option<SystemTime>,
}

/// Single-entry cache of the last fully evaluated query. Serving repeat or
/// paginated requests from it skips all parsing and strategy evaluation.
pub struct QueryCache {
	pub fingerprint: u64,
	pub stamp: RepoStamp,
	pub matches: Vec<GrepMatch>,
	pub note: Option<String>,
	pub diagnostic: Option<String>,
}

/// Result of a multi-strategy structural search across a file list.
pub struct SmartSearchOutcome {
	pub matches: Vec<GrepMatch>,
	/// When a non-default strategy yielded the matches, describes which one.
	pub note: Option<String>,
	/// Explanation of why structural matching failed. Set alongside lexical
	/// fallback matches, or alone when nothing at all was found.
	pub diagnostic: Option<String>,
}

/// Hash the request-defining fields into a cache fingerprint.
pub fn fingerprint_request(parts: &[&str]) -> u64 {
	use std::hash::{Hash, Hasher};
	let mut h = std::collections::hash_map::DefaultHasher::new();
	for p in parts {
		p.hash(&mut h);
	}
	h.finish()
}

/// Collect candidate files via a parallel gitignore-aware walk. Reads file
/// contents (needed by every later stage) and evaluates the literal-token
/// prefilter inline. Returns files sorted by display path plus the repo stamp.
pub fn collect_file_data(
	root: &Path,
	language: &str,
	paths_filter: Option<&[String]>,
	prefilter_tokens: &[String],
) -> (Vec<FileData>, RepoStamp) {
	let acc: parking_lot::Mutex<(Vec<FileData>, RepoStamp)> =
		parking_lot::Mutex::new((Vec::new(), RepoStamp::default()));

	let walker = ignore::WalkBuilder::new(root)
		.git_ignore(true)
		.git_global(true)
		.git_exclude(true)
		.hidden(true)
		.build_parallel();

	let acc_ref = &acc;
	walker.run(|| {
		Box::new(move |result| {
			let entry = match result {
				Ok(e) => e,
				Err(_) => return ignore::WalkState::Continue,
			};
			if !entry.file_type().is_some_and(|ft| ft.is_file()) {
				return ignore::WalkState::Continue;
			}
			let path = entry.path();
			if grep::language_from_extension(path) != Some(language) {
				return ignore::WalkState::Continue;
			}
			let display = path
				.strip_prefix(root)
				.unwrap_or(path)
				.to_string_lossy()
				.to_string();
			if let Some(filter) = paths_filter {
				if !filter.iter().any(|p| display.contains(p)) {
					return ignore::WalkState::Continue;
				}
			}
			let (size, mtime) = entry
				.metadata()
				.map(|m| (m.len(), m.modified().ok()))
				.unwrap_or((0, None));
			if size > MAX_FILE_SIZE {
				return ignore::WalkState::Continue;
			}
			let content = match std::fs::read_to_string(path) {
				Ok(c) => c,
				Err(_) => return ignore::WalkState::Continue,
			};
			let prefilter_hit = prefilter_tokens.is_empty()
				|| prefilter_tokens
					.iter()
					.all(|t| content.contains(t.as_str()));

			let mut guard = acc_ref.lock();
			guard.1.file_count += 1;
			guard.1.total_size += size;
			if let Some(mt) = mtime {
				if guard.1.max_mtime.map(|cur| mt > cur).unwrap_or(true) {
					guard.1.max_mtime = Some(mt);
				}
			}
			guard.0.push(FileData {
				path: path.to_path_buf(),
				display,
				content,
				prefilter_hit,
			});
			ignore::WalkState::Continue
		})
	});

	let (mut files, stamp) = acc.into_inner();
	files.sort_by(|a, b| a.display.cmp(&b.display));
	(files, stamp)
}

/// Map `work` over (a filtered subset of) files on all cores, preserving
/// input file order in the returned vec.
fn parallel_file_map<T: Send>(
	files: &[FileData],
	only_prefilter_hits: bool,
	work: impl Fn(&FileData) -> Option<T> + Sync,
) -> Vec<T> {
	let cursor = AtomicUsize::new(0);
	let collected: parking_lot::Mutex<Vec<(usize, T)>> = parking_lot::Mutex::new(Vec::new());
	let workers = std::thread::available_parallelism()
		.map(|n| n.get())
		.unwrap_or(4)
		.min(16)
		.min(files.len().max(1));

	std::thread::scope(|s| {
		for _ in 0..workers {
			s.spawn(|| {
				let mut local: Vec<(usize, T)> = Vec::new();
				loop {
					let i = cursor.fetch_add(1, Ordering::Relaxed);
					if i >= files.len() {
						break;
					}
					let fd = &files[i];
					if only_prefilter_hits && !fd.prefilter_hit {
						continue;
					}
					if let Some(v) = work(fd) {
						local.push((i, v));
					}
				}
				collected.lock().extend(local);
			});
		}
	});

	let mut v = collected.into_inner();
	v.sort_by_key(|(i, _)| *i);
	v.into_iter().map(|(_, t)| t).collect()
}

/// A prepared set of specs to evaluate per file: `n_strategies` strategy
/// buckets (with display notes) plus optional `inside`/`has` relational specs.
struct SpecSet {
	specs: Vec<grep::SearchSpec>,
	notes: Vec<Option<String>>,
	n_strategies: usize,
	inside_idx: Option<usize>,
	has_idx: Option<usize>,
}

fn build_spec_set(
	strategies: Vec<(Option<String>, grep::SearchSpec)>,
	inside: Option<&str>,
	has: Option<&str>,
) -> SpecSet {
	let n_strategies = strategies.len();
	let mut specs = Vec::with_capacity(n_strategies + 2);
	let mut notes = Vec::with_capacity(n_strategies);
	for (note, spec) in strategies {
		notes.push(note);
		specs.push(spec);
	}
	let inside_idx = inside.map(|s| {
		specs.push(grep::SearchSpec::KindOrPattern {
			spec: s.to_string(),
		});
		specs.len() - 1
	});
	let has_idx = has.map(|s| {
		specs.push(grep::SearchSpec::KindOrPattern {
			spec: s.to_string(),
		});
		specs.len() - 1
	});
	SpecSet {
		specs,
		notes,
		n_strategies,
		inside_idx,
		has_idx,
	}
}

/// Keep a match only when the relational constraints hold:
/// `inside` — some inside-match range strictly contains the match;
/// `has` — the match strictly contains some has-match range.
fn filter_by_containment(
	matches: Vec<GrepMatch>,
	inside_ranges: Option<&[(usize, usize)]>,
	has_ranges: Option<&[(usize, usize)]>,
) -> Vec<GrepMatch> {
	matches
		.into_iter()
		.filter(|m| {
			let mr = (m.start_byte, m.end_byte);
			if let Some(ranges) = inside_ranges {
				if !ranges
					.iter()
					.any(|r| r.0 <= mr.0 && mr.1 <= r.1 && *r != mr)
				{
					return false;
				}
			}
			if let Some(ranges) = has_ranges {
				if !ranges
					.iter()
					.any(|r| mr.0 <= r.0 && r.1 <= mr.1 && *r != mr)
				{
					return false;
				}
			}
			true
		})
		.collect()
}

/// Evaluate a spec set across files (single parse per file), apply the
/// relational filters, and merge into one global bucket per strategy.
fn run_specs_over_files(
	files: &[FileData],
	only_prefilter_hits: bool,
	language: &str,
	set: &SpecSet,
	constraints: &[MetavarConstraint],
) -> Vec<Vec<GrepMatch>> {
	let per_file = parallel_file_map(files, only_prefilter_hits, |fd| {
		let buckets =
			grep::search_file_multi(&fd.display, &fd.content, language, &set.specs, constraints)
				.ok()?;
		if buckets.iter().take(set.n_strategies).all(|b| b.is_empty()) {
			return None;
		}
		let inside_ranges: Option<Vec<(usize, usize)>> = set.inside_idx.map(|i| {
			buckets[i]
				.iter()
				.map(|m| (m.start_byte, m.end_byte))
				.collect()
		});
		let has_ranges: Option<Vec<(usize, usize)>> = set.has_idx.map(|i| {
			buckets[i]
				.iter()
				.map(|m| (m.start_byte, m.end_byte))
				.collect()
		});
		let mut out: Vec<Vec<GrepMatch>> = Vec::with_capacity(set.n_strategies);
		for bucket in buckets.into_iter().take(set.n_strategies) {
			let mut filtered =
				filter_by_containment(bucket, inside_ranges.as_deref(), has_ranges.as_deref());
			filtered.truncate(MAX_PER_FILE_MATCHES);
			out.push(filtered);
		}
		Some(out)
	});

	let mut merged: Vec<Vec<GrepMatch>> = (0..set.n_strategies).map(|_| Vec::new()).collect();
	for file_buckets in per_file {
		for (i, bucket) in file_buckets.into_iter().enumerate() {
			if merged[i].len() < MAX_TOTAL_MATCHES {
				merged[i].extend(bucket);
			}
		}
	}
	for bucket in &mut merged {
		bucket.truncate(MAX_TOTAL_MATCHES);
	}
	merged
}

/// Deterministic base order regardless of parallel completion order.
fn sort_matches(matches: &mut [GrepMatch]) {
	matches.sort_by(|a, b| {
		a.file
			.cmp(&b.file)
			.then(a.line.cmp(&b.line))
			.then(a.column.cmp(&b.column))
	});
}

/// Boost matches whose file path mentions one of the pattern's metavariable
/// names (weak relevance signal an LLM tends to encode in variable naming).
/// Stable sort: within boost/non-boost groups the base order is preserved.
fn rerank_matches(matches: &mut [GrepMatch], metavars: &[String]) {
	if metavars.is_empty() {
		return;
	}
	let needles: Vec<String> = metavars.iter().map(|n| n.to_ascii_lowercase()).collect();
	matches.sort_by_key(|m| {
		let path_lower = m.file.to_ascii_lowercase();
		let hit = needles
			.iter()
			.any(|n| !n.is_empty() && path_lower.contains(n));
		// Lower key sorts first — boost (0) goes ahead of non-boost (1).
		(if hit { 0u8 } else { 1u8 }, m.file.clone(), m.line)
	});
}

fn finalize(
	mut matches: Vec<GrepMatch>,
	note: Option<String>,
	metavars: &[String],
) -> SmartSearchOutcome {
	sort_matches(&mut matches);
	rerank_matches(&mut matches, metavars);
	SmartSearchOutcome {
		matches,
		note,
		diagnostic: None,
	}
}

/// Run the pattern against `files` using a chain of strategies, two passes:
///
/// Pass A (prefilter-surviving files only, one parse each):
///   1. Smart-strictness ast-grep pattern (the default)
///   2. Relaxed-strictness pattern — only when root_kind is None or pattern
///      has no item-keyword (avoids false positives like `pub mod` matching
///      `pub struct $N { $$$ }`)
///
/// Pass B (all candidate files, one parse each; only when Pass A is empty):
///   3. Item-keyword broadening — `pub struct …` → `struct_item` kind, etc.
///   4. KindMatcher with raw pattern
///   5. Canonical kind mapping (LLM intent dictionary)
///   6. Language-aware contextual fallback (Rust types, Go calls, TS fields,
///      JSON pairs)
///
/// Fallback: labeled lexical line scan, so the tool never answers empty-handed
/// when the pattern's tokens exist somewhere in the corpus.
pub fn smart_search(
	files: &[FileData],
	pattern: &str,
	language: &str,
	constraints: &[MetavarConstraint],
	inside: Option<&str>,
	has: Option<&str>,
) -> SmartSearchOutcome {
	// Parse the pattern once — needed for diagnostics and to gate Relaxed.
	let info = grep::pattern_info(pattern, language).ok();
	let metavars: Vec<String> = info
		.as_ref()
		.map(|i| i.defined_vars.clone())
		.unwrap_or_default();

	// --- Pass A: pattern strategies on prefiltered files ---
	let relaxed_safe = info
		.as_ref()
		.map(|i| i.root_kind.is_none() && !i.has_error)
		.unwrap_or(false);

	let mut strategies: Vec<(Option<String>, grep::SearchSpec)> = vec![(
		None,
		grep::SearchSpec::Pattern {
			pattern: pattern.to_string(),
			strictness: grep::MatchStrictness::Smart,
		},
	)];
	if relaxed_safe {
		strategies.push((
			Some("[strictness: relaxed]".to_string()),
			grep::SearchSpec::Pattern {
				pattern: pattern.to_string(),
				strictness: grep::MatchStrictness::Relaxed,
			},
		));
	}
	let set = build_spec_set(strategies, inside, has);
	let buckets = run_specs_over_files(files, true, language, &set, constraints);
	for (i, bucket) in buckets.into_iter().enumerate() {
		if !bucket.is_empty() {
			return finalize(bucket, set.notes[i].clone(), &metavars);
		}
	}

	// --- Pass B: kind/contextual fallbacks on all candidate files ---
	let mut strategies: Vec<(Option<String>, grep::SearchSpec)> = Vec::new();
	if let Some((kind, desc)) = item_keyword_kind(pattern, language) {
		strategies.push((
			Some(format!(
				"[kind: {}] broadened from `{}` (returns ALL {} — your pattern likely missed due to decorators or signature shape)",
				kind,
				pattern.trim(),
				desc
			)),
			grep::SearchSpec::Kind {
				kind: kind.to_string(),
			},
		));
	}
	strategies.push((
		Some(format!("[kind: {}]", pattern)),
		grep::SearchSpec::Kind {
			kind: pattern.to_string(),
		},
	));
	if let Some(canonical) = grep::canonical_kind(pattern, language) {
		if canonical != pattern {
			strategies.push((
				Some(format!(
					"[kind: {}] (canonical {} kind for `{}`)",
					canonical, language, pattern
				)),
				grep::SearchSpec::Kind {
					kind: canonical.to_string(),
				},
			));
		}
	}
	for (desc, selector, context_src) in auto_contexts(language, pattern) {
		strategies.push((
			Some(format!("[context wrap: {}]", desc)),
			grep::SearchSpec::Contextual {
				context_src,
				selector: selector.to_string(),
			},
		));
	}
	let set = build_spec_set(strategies, inside, has);
	let buckets = run_specs_over_files(files, false, language, &set, constraints);
	for (i, bucket) in buckets.into_iter().enumerate() {
		if !bucket.is_empty() {
			return finalize(bucket, set.notes[i].clone(), &metavars);
		}
	}

	// --- Lexical fallback: never answer empty-handed when tokens exist ---
	let tokens = grep::literal_tokens(pattern);
	if let Some(token) = tokens.first() {
		let mut lex: Vec<GrepMatch> = Vec::new();
		for fd in files {
			if lex.len() >= LEXICAL_TOTAL {
				break;
			}
			lex.extend(grep::lexical_scan(
				&fd.display,
				&fd.content,
				token,
				LEXICAL_PER_FILE,
			));
		}
		lex.truncate(LEXICAL_TOTAL);
		if !lex.is_empty() {
			sort_matches(&mut lex);
			return SmartSearchOutcome {
				matches: lex,
				note: Some(format!(
					"[lexical fallback] No structural matches; showing plain-text lines containing \"{}\" (not AST-verified).",
					token
				)),
				diagnostic: Some(build_diagnostic(pattern, language)),
			};
		}
	}

	// All strategies exhausted — diagnostic only.
	SmartSearchOutcome {
		matches: Vec::new(),
		note: None,
		diagnostic: Some(build_diagnostic(pattern, language)),
	}
}

/// Symbol-mode search: find definitions of (or references to) a named symbol
/// without writing an AST pattern. Supports `*` wildcards in the name.
pub fn symbol_search(
	files: &[FileData],
	symbol: &str,
	language: &str,
	references: bool,
) -> SmartSearchOutcome {
	let name = match grep::NameMatcher::new(symbol) {
		Ok(n) => n,
		Err(e) => {
			return SmartSearchOutcome {
				matches: Vec::new(),
				note: None,
				diagnostic: Some(format!("Invalid symbol spec `{}`: {}", symbol, e)),
			}
		}
	};

	let per_file = parallel_file_map(files, true, |fd| {
		let found = if references {
			grep::find_symbol_refs(&fd.display, &fd.content, &name, language)
		} else {
			grep::find_symbol_defs(&fd.display, &fd.content, &name, language)
		};
		match found {
			Ok(v) if !v.is_empty() => {
				let mut v = v;
				v.truncate(MAX_PER_FILE_MATCHES);
				Some(v)
			}
			_ => None,
		}
	});

	let mut matches: Vec<GrepMatch> = Vec::new();
	for v in per_file {
		if matches.len() >= MAX_TOTAL_MATCHES {
			break;
		}
		matches.extend(v);
	}
	matches.truncate(MAX_TOTAL_MATCHES);

	if !matches.is_empty() {
		sort_matches(&mut matches);
		let mode = if references {
			"references"
		} else {
			"definitions"
		};
		return SmartSearchOutcome {
			matches,
			note: Some(format!("[symbol {}: {}]", mode, symbol)),
			diagnostic: None,
		};
	}

	// Lexical fallback on the literal fragment of the name.
	let token = symbol.replace('*', "");
	if token.len() >= 2 {
		let mut lex: Vec<GrepMatch> = Vec::new();
		for fd in files {
			if lex.len() >= LEXICAL_TOTAL {
				break;
			}
			lex.extend(grep::lexical_scan(
				&fd.display,
				&fd.content,
				&token,
				LEXICAL_PER_FILE,
			));
		}
		lex.truncate(LEXICAL_TOTAL);
		if !lex.is_empty() {
			sort_matches(&mut lex);
			return SmartSearchOutcome {
				matches: lex,
				note: Some(format!(
					"[lexical fallback] No {} found for `{}`; showing plain-text lines containing \"{}\" (not AST-verified).",
					if references { "references" } else { "definitions" },
					symbol,
					token
				)),
				diagnostic: None,
			};
		}
	}

	SmartSearchOutcome {
		matches: Vec::new(),
		note: None,
		diagnostic: Some(format!(
			"No symbol {} for `{}` ({} files scanned, language: {}). \
			 Hints: wildcards widen the match (`*{}*`); use `references` to find usages \
			 or `symbol` for definitions; verify the `language` matches the file extensions.",
			if references {
				"references"
			} else {
				"definitions"
			},
			symbol,
			files.len(),
			language,
			symbol.trim_matches('*')
		)),
	}
}

/// Per-language scaffolds for the contextual fallback. Each entry returns
/// (description, selector, full-context-source) where `pattern` is embedded
/// at the cursor position so the parser sees it in an unambiguous context.
///
/// These cover the documented parsing traps:
///   • Go — `fmt.Println($A)` parses as type_conversion unless wrapped in a func body
///   • TS/JS — `name = value` parses as assignment unless wrapped in a class body
///   • JSON — `"key": $V` doesn't parse standalone; needs an object wrapper
fn auto_contexts(language: &str, pattern: &str) -> Vec<(&'static str, &'static str, String)> {
	match language {
		"rust" => {
			// Rust type-expressions like `Arc<Mutex<$T>>` don't parse standalone
			// (top-level isn't a type). Wrap as a type alias to disambiguate.
			let looks_like_type = pattern.contains('<')
				&& pattern.contains('>')
				&& !pattern.contains('{')
				&& !pattern.contains(';')
				&& !pattern.contains(" fn ")
				&& !pattern.contains("fn ");
			if looks_like_type {
				vec![(
					"Rust type alias context, generic_type selector",
					"generic_type",
					format!("type _ = {};", pattern),
				)]
			} else {
				Vec::new()
			}
		}
		"go" => vec![(
			"Go func body, call_expression selector",
			"call_expression",
			format!("package _\nfunc _() {{ {} }}", pattern),
		)],
		"typescript" | "javascript" => vec![
			(
				"class body, field_definition selector",
				"field_definition",
				format!("class _ {{ {} }}", pattern),
			),
			(
				"class body, method_definition selector",
				"method_definition",
				format!("class _ {{ {} }}", pattern),
			),
		],
		"json" => vec![(
			"object body, pair selector",
			"pair",
			format!("{{ {} }}", pattern),
		)],
		_ => Vec::new(),
	}
}

/// If `pattern` starts with a recognized item-level keyword in `language`,
/// return the corresponding tree-sitter kind plus a human label. Used to
/// broaden a failed structural pattern to "find all items of this kind",
/// rescuing the common case where the LLM omitted attributes / return types
/// / signature shape that the real source has.
fn item_keyword_kind(pattern: &str, language: &str) -> Option<(&'static str, &'static str)> {
	let trimmed = pattern.trim_start();
	let starts = |kw: &str| {
		trimmed.starts_with(kw)
			&& trimmed[kw.len()..]
				.chars()
				.next()
				.map(|c| c.is_whitespace())
				.unwrap_or(false)
	};
	match language {
		"rust" => {
			if starts("pub struct") || starts("struct") {
				Some(("struct_item", "struct definitions"))
			} else if starts("pub enum") || starts("enum") {
				Some(("enum_item", "enum definitions"))
			} else if starts("pub trait") || starts("trait") {
				Some(("trait_item", "trait definitions"))
			} else if starts("impl") {
				Some(("impl_item", "impl blocks"))
			} else if starts("pub fn")
				|| starts("pub async fn")
				|| starts("async fn")
				|| starts("fn")
				|| starts("pub const fn")
				|| starts("const fn")
				|| starts("pub unsafe fn")
				|| starts("unsafe fn")
			{
				Some(("function_item", "function definitions"))
			} else if starts("pub use") || starts("use") {
				Some(("use_declaration", "use imports"))
			} else if starts("pub mod") || starts("mod") {
				Some(("mod_item", "module declarations"))
			} else if starts("pub type") || starts("type") {
				Some(("type_item", "type aliases"))
			} else {
				None
			}
		}
		"javascript" | "typescript" => {
			if starts("class") || starts("export class") {
				Some(("class_declaration", "class declarations"))
			} else if starts("function") || starts("async function") || starts("export function") {
				Some(("function_declaration", "function declarations"))
			} else if starts("interface") || starts("export interface") {
				Some(("interface_declaration", "interface declarations"))
			} else if starts("import") {
				Some(("import_statement", "import statements"))
			} else {
				None
			}
		}
		"python" => {
			if starts("def") || starts("async def") {
				Some(("function_definition", "function definitions"))
			} else if starts("class") {
				Some(("class_definition", "class definitions"))
			} else if starts("from") {
				Some(("import_from_statement", "from-imports"))
			} else if starts("import") {
				Some(("import_statement", "imports"))
			} else if starts("@") {
				Some(("decorated_definition", "decorated definitions"))
			} else {
				None
			}
		}
		"go" => {
			if starts("func") {
				Some(("function_declaration", "function declarations"))
			} else if starts("type") {
				Some(("type_declaration", "type declarations"))
			} else if starts("import") {
				Some(("import_declaration", "import declarations"))
			} else {
				None
			}
		}
		_ => None,
	}
}

/// Short diagnostic for when no strategy matched. Just the signals the LLM
/// needs to retry — parsed kind, parse-error flag, metavars, and one or two
/// targeted hints. Worked examples and language tips live in the tool
/// description (read once), not duplicated per failure.
fn build_diagnostic(pattern: &str, language: &str) -> String {
	let mut out = String::new();
	out.push_str(&format!("No matches: {}\n", pattern));

	match grep::pattern_info(pattern, language) {
		Ok(info) => {
			let kind = info.root_kind.as_deref().unwrap_or("(bare $metavar)");
			let vars = if info.defined_vars.is_empty() {
				"none".to_string()
			} else {
				info.defined_vars
					.iter()
					.map(|v| format!("${}", v))
					.collect::<Vec<_>>()
					.join(",")
			};
			out.push_str(&format!(
				"Parsed: kind={}, parse_error={}, metavars={}\n",
				kind, info.has_error, vars
			));

			// One-shot retargeted hint, chosen by what we observed.
			if info.has_error {
				out.push_str(&format!(
					"Hint: pattern doesn't parse as standalone {}. \
					   For type expressions try kind 'generic_type'; for code fragments \
					   wrap in a valid context. See tool description for shapes.\n",
					language
				));
			} else if let Some((kind_name, label)) = item_keyword_kind(pattern, language) {
				out.push_str(&format!(
					"Hint: for finding all {} regardless of decorators/signature, \
					   pass `{}` as the pattern (a tree-sitter kind name).\n",
					label, kind_name
				));
			} else if let Some(canonical) = grep::canonical_kind(pattern, language) {
				if canonical != pattern {
					out.push_str(&format!(
						"Hint: canonical {} kind for this intent is '{}'.\n",
						language, canonical
					));
				}
			} else {
				out.push_str(
					"Hint: see tool description for valid pattern shapes per language. \
					   For meaning-based lookups, use semantic_search.\n",
				);
			}
		}
		Err(e) => {
			out.push_str(&format!("Parse error: {}\n", e));
			out.push_str(
				"Hint: pattern must be syntactically valid in the target language. \
				   See tool description for valid shapes.\n",
			);
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	fn fd(display: &str, content: &str, prefilter_hit: bool) -> FileData {
		FileData {
			path: PathBuf::from(display),
			display: display.to_string(),
			content: content.to_string(),
			prefilter_hit,
		}
	}

	#[test]
	fn test_smart_search_basic_pattern() {
		let files = vec![
			fd("a.rs", "fn main() { let x = foo.unwrap(); }", true),
			fd("b.rs", "fn run() { let y = bar.unwrap(); }", true),
			fd("c.rs", "fn other() { let z = 1; }", false),
		];
		let out = smart_search(&files, "$X.unwrap()", "rust", &[], None, None);
		assert_eq!(out.matches.len(), 2);
		assert!(out.note.is_none());
		assert!(out.diagnostic.is_none());
		// Deterministic order: a.rs before b.rs.
		assert_eq!(out.matches[0].file, "a.rs");
	}

	#[test]
	fn test_smart_search_prefilter_skips_files() {
		// File contains a match but is marked prefilter-missed — Pass A must
		// skip it; Pass B (kind strategies) doesn't apply to this pattern, so
		// the result falls through to lexical/diagnostic territory.
		let files = vec![fd("a.rs", "fn main() { foo.unwrap(); }", false)];
		let out = smart_search(&files, "$X.definitely_missing()", "rust", &[], None, None);
		assert!(out.matches.is_empty());
		assert!(out.diagnostic.is_some());
	}

	#[test]
	fn test_smart_search_kind_broadening_pass_b() {
		// `pub struct $N { $$$ }` misses decorated structs; Pass B broadens
		// to struct_item and must find it with the explanatory note.
		let files = vec![fd(
			"s.rs",
			"#[derive(Debug)]\npub struct Config {\n    pub key: String,\n}\n",
			true,
		)];
		let out = smart_search(&files, "pub struct $N { $$$ }", "rust", &[], None, None);
		assert_eq!(out.matches.len(), 1);
		let note = out.note.expect("broadening note expected");
		assert!(note.contains("[kind: struct_item]"), "note: {}", note);
	}

	#[test]
	fn test_smart_search_inside_filter() {
		let files = vec![fd(
			"a.rs",
			"const A: i32 = X.unwrap();\nfn main() { let x = foo.unwrap(); }\n",
			true,
		)];
		// Without inside: both unwraps.
		let all = smart_search(&files, "$X.unwrap()", "rust", &[], None, None);
		assert_eq!(all.matches.len(), 2);
		// With inside=function_item: only the one inside fn main.
		let inside = smart_search(
			&files,
			"$X.unwrap()",
			"rust",
			&[],
			Some("function_item"),
			None,
		);
		assert_eq!(inside.matches.len(), 1);
		assert_eq!(inside.matches[0].line, 2);
	}

	#[test]
	fn test_smart_search_has_filter() {
		let files = vec![fd(
			"a.rs",
			"fn with() { foo.unwrap(); }\nfn without() { let x = 1; }\n",
			true,
		)];
		// Kind search for functions, keeping only those containing unwrap.
		let out = smart_search(
			&files,
			"function_item",
			"rust",
			&[],
			None,
			Some("$X.unwrap()"),
		);
		assert_eq!(out.matches.len(), 1);
		assert!(out.matches[0].text.contains("with"));
	}

	#[test]
	fn test_smart_search_metavar_constraints() {
		let files = vec![fd(
			"a.rs",
			"fn handle_request() { a(); }\nfn other_thing() { b(); }\n",
			true,
		)];
		let re = regex::Regex::new("^handle_").unwrap();
		let constraints = vec![("NAME".to_string(), re)];
		let out = smart_search(
			&files,
			"fn $NAME($$$) { $$$ }",
			"rust",
			&constraints,
			None,
			None,
		);
		assert_eq!(out.matches.len(), 1);
		assert!(out.matches[0].text.contains("handle_request"));
	}

	#[test]
	fn test_smart_search_lexical_fallback() {
		// Structurally absent (no such call), but the token appears in a
		// comment — lexical fallback must surface it, labeled, with diagnostic.
		let files = vec![fd(
			"a.rs",
			"// TODO: call frobnicate_widget here\nfn main() {}\n",
			true,
		)];
		let out = smart_search(&files, "frobnicate_widget($$$)", "rust", &[], None, None);
		assert_eq!(out.matches.len(), 1);
		let note = out.note.expect("lexical fallback note expected");
		assert!(note.contains("lexical fallback"), "note: {}", note);
		assert!(out.diagnostic.is_some());
	}

	#[test]
	fn test_smart_search_breadcrumbs_attached() {
		let files = vec![fd(
			"a.rs",
			"impl Store {\n    fn flush(&self) { self.inner.unwrap(); }\n}\n",
			true,
		)];
		let out = smart_search(&files, "$X.unwrap()", "rust", &[], None, None);
		assert_eq!(out.matches.len(), 1);
		let bc = out.matches[0].breadcrumb.as_deref().expect("breadcrumb");
		assert!(bc.contains("impl Store"), "breadcrumb: {}", bc);
		assert!(bc.contains("fn flush"), "breadcrumb: {}", bc);
	}

	#[test]
	fn test_symbol_search_definitions() {
		let files = vec![
			fd("a.rs", "pub fn flush_cache() {}\npub fn other() {}\n", true),
			fd("b.rs", "pub struct FlushConfig { pub a: u8 }\n", true),
		];
		let out = symbol_search(&files, "flush_cache", "rust", false);
		assert_eq!(out.matches.len(), 1);
		assert_eq!(out.matches[0].file, "a.rs");
		assert!(out.note.unwrap().contains("definitions"));
	}

	#[test]
	fn test_symbol_search_wildcard() {
		let files = vec![fd(
			"a.rs",
			"fn handle_get() {}\nfn handle_post() {}\nfn other() {}\n",
			true,
		)];
		let out = symbol_search(&files, "handle_*", "rust", false);
		assert_eq!(out.matches.len(), 2);
	}

	#[test]
	fn test_symbol_search_references() {
		let files = vec![fd(
			"a.rs",
			"fn flush() {}\nfn main() { flush(); flush(); }\n",
			true,
		)];
		let out = symbol_search(&files, "flush", "rust", true);
		// 1 definition site + 2 call sites.
		assert_eq!(out.matches.len(), 3);
		let defs: Vec<_> = out
			.matches
			.iter()
			.filter(|m| m.text.starts_with("[def]"))
			.collect();
		assert_eq!(defs.len(), 1);
		assert_eq!(defs[0].line, 1);
	}

	#[test]
	fn test_symbol_search_no_match_diagnostic() {
		let files = vec![fd("a.rs", "fn main() {}\n", true)];
		let out = symbol_search(&files, "zzz_missing", "rust", false);
		assert!(out.matches.is_empty());
		assert!(out.diagnostic.unwrap().contains("zzz_missing"));
	}

	#[test]
	fn test_fingerprint_request_deterministic() {
		let a = fingerprint_request(&["$X.unwrap()", "rust", "", ""]);
		let b = fingerprint_request(&["$X.unwrap()", "rust", "", ""]);
		let c = fingerprint_request(&["$X.unwrap()", "go", "", ""]);
		assert_eq!(a, b);
		assert_ne!(a, c);
	}

	#[test]
	fn test_repo_stamp_equality() {
		let now = SystemTime::now();
		let s1 = RepoStamp {
			file_count: 2,
			total_size: 10,
			max_mtime: Some(now),
		};
		let s2 = RepoStamp {
			file_count: 2,
			total_size: 10,
			max_mtime: Some(now),
		};
		let s3 = RepoStamp {
			file_count: 3,
			total_size: 10,
			max_mtime: Some(now),
		};
		assert_eq!(s1, s2);
		assert_ne!(s1, s3);
	}

	#[test]
	fn test_filter_by_containment() {
		let m = |s: usize, e: usize| GrepMatch {
			file: "a.rs".into(),
			line: 1,
			column: 0,
			text: "x".into(),
			start_byte: s,
			end_byte: e,
			breadcrumb: None,
		};
		// inside: match (5,10) is inside (0,20) but (30,40) is not.
		let kept = filter_by_containment(vec![m(5, 10), m(30, 40)], Some(&[(0, 20)]), None);
		assert_eq!(kept.len(), 1);
		assert_eq!(kept[0].start_byte, 5);
		// has: match (0,20) contains (5,10); match (30,40) contains nothing.
		let kept = filter_by_containment(vec![m(0, 20), m(30, 40)], None, Some(&[(5, 10)]));
		assert_eq!(kept.len(), 1);
		assert_eq!(kept[0].start_byte, 0);
	}
}
