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

//! MCP Server implementation using official rmcp SDK
//!
//! Supports both stdio and Streamable HTTP transports.
//! All core logic from the original server.rs is preserved:
//! - Semantic search, view signatures, GraphRAG tools
//! - LSP tools (goto_definition, hover, find_references, document_symbols, workspace_symbols, completion)
//! - File watcher with debounced background indexing
//! - Structured logging, graceful shutdown

use super::graphrag::GraphRagProvider;
use super::logging::{
	init_mcp_logging, log_critical_anyhow_error, log_indexing_operation, log_watcher_event,
};
use super::semantic_code::SemanticCodeProvider;
use super::watcher::run_watcher;
use crate::config::Config;
use crate::indexer;
use crate::lock::IndexLock;
use crate::state;
use crate::store::Store;
use crate::watcher_config::{DEFAULT_ADDITIONAL_DELAY_MS, MCP_DEFAULT_DEBOUNCE_MS};
use anyhow::Result;
use rmcp::{
	handler::server::{router::tool::ToolRouter, tool::ToolCallContext, wrapper::Parameters},
	model::{
		CallToolRequestParams, CallToolResult, Implementation, ServerCapabilities, ServerInfo, Tool,
	},
	schemars,
	service::RequestContext,
	tool, tool_handler, tool_router, ErrorData, RoleServer, ServerHandler, ServiceExt,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{sleep, Duration, Instant};
use tracing::{debug, info, warn};

// Configurable debounce settings
const MCP_DEBOUNCE_MS: u64 = MCP_DEFAULT_DEBOUNCE_MS; // 2000ms = 2 seconds
const MCP_MAX_PENDING_EVENTS: usize = 100;
const MCP_INDEX_TIMEOUT_MS: u64 = 300_000; // 5 minutes

// ---------------------------------------------------------------------------
// Parameter structs for rmcp tool schema generation
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SemanticSearchParams {
	/// Describe the code by what it does or the concept behind it, not its symbol name (e.g. 'retry failed network requests', not 'fn retry'). String or array of strings; an array of related phrasings widens recall.
	pub query: serde_json::Value,
	/// Max results to return (default: 3)
	#[serde(default, skip_serializing_if = "Option::is_none")]
	#[schemars(range(min = 1, max = 20), extend("default" = 3))]
	pub max_results: Option<usize>,
	/// Result verbosity: 'signatures' (declarations only), 'partial' (truncated, default), 'full' (complete bodies)
	#[serde(default, skip_serializing_if = "Option::is_none")]
	#[schemars(extend("enum" = ["signatures", "partial", "full"]), extend("default" = "partial"))]
	pub detail_level: Option<String>,
	/// Filter code results by language (rust, python, typescript, go, etc.)
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub language: Option<String>,
	/// Content type filter: 'code' (functions/classes), 'text' (plain text), 'docs' (markdown/README), 'commits' (git commit history), 'all' (default, excludes commits)
	#[serde(default, skip_serializing_if = "Option::is_none")]
	#[schemars(extend("enum" = ["code", "text", "docs", "commits", "all"]), extend("default" = "all"))]
	pub mode: Option<String>,
	/// Similarity cutoff 0.0-1.0 (higher = stricter match)
	#[serde(default, skip_serializing_if = "Option::is_none")]
	#[schemars(range(min = 0, max = 1))]
	pub threshold: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ViewSignaturesParams {
	/// File paths or glob patterns (e.g. 'src/main.rs', '**/*.py', 'src/**/*.ts')
	#[serde(deserialize_with = "string_or_vec")]
	#[schemars(length(min = 1, max = 100))]
	pub files: Vec<String>,
}

/// Accept either a single string or an array of strings, so a bare glob
/// pattern like 'tests/*.rs' deserializes the same as ['tests/*.rs'].
fn string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
	D: serde::Deserializer<'de>,
{
	use serde::de::{Error, SeqAccess, Visitor};
	use std::fmt;

	struct StringOrVec;

	impl<'de> Visitor<'de> for StringOrVec {
		type Value = Vec<String>;

		fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
			formatter.write_str("a string or an array of strings")
		}

		fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
			Ok(vec![value.to_string()])
		}

		fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
			let mut out = Vec::new();
			while let Some(item) = seq.next_element::<String>()? {
				out.push(item);
			}
			Ok(out)
		}
	}

	deserializer.deserialize_any(StringOrVec)
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GraphRagParams {
	/// 'search' (semantic node search), 'get-node' (node details), 'get-relationships' (node connections), 'find-path' (path between two nodes), 'overview' (graph stats)
	#[schemars(extend("enum" = ["search", "get-node", "get-relationships", "find-path", "overview"]))]
	pub operation: String,
	/// Search query for 'search' operation
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub query: Option<String>,
	/// Node ID for 'get-node'/'get-relationships' (format: 'path/to/file' or 'path/to/file/symbol')
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub node_id: Option<String>,
	/// Source node ID for 'find-path'
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source_id: Option<String>,
	/// Target node ID for 'find-path'
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub target_id: Option<String>,
	/// Max path depth for 'find-path' (default: 3)
	#[serde(default, skip_serializing_if = "Option::is_none")]
	#[schemars(range(min = 1, max = 10), extend("default" = 3))]
	pub max_depth: Option<usize>,
	/// Output format (default: 'text')
	#[serde(default, skip_serializing_if = "Option::is_none")]
	#[schemars(extend("enum" = ["text", "json", "markdown"]), extend("default" = "text"))]
	pub format: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StructuralSearchParams {
	/// AST pattern, kind name, or literal snippet. `$X` = single node, `$$$` = sequence.
	/// Examples: `$X.unwrap()`, `if err != nil { $$$ }`, `function_item`, `import_statement`.
	/// Pattern must be valid standalone code in the target language.
	/// Provide exactly one of `pattern`, `symbol`, or `references`.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub pattern: Option<String>,
	/// Find symbol DEFINITIONS by name — no pattern needed. Supports `*` wildcards
	/// (e.g. `flush_cache`, `handle_*`, `*Config`). Matches functions, types,
	/// classes, traits, etc. for the language.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub symbol: Option<String>,
	/// Find symbol REFERENCES (identifier usages) by name. Supports `*` wildcards.
	/// Each hit shows the source line and enclosing symbol; definition sites are tagged `[def]`.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub references: Option<String>,
	/// Language to search (required: rust, javascript, typescript, python, go, java, cpp, php, ruby, swift, lua, bash, css, json)
	pub language: String,
	/// File paths or glob patterns to search (default: current directory)
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub paths: Option<Vec<String>>,
	/// Only keep matches INSIDE a node matching this kind name or pattern
	/// (e.g. `function_item`, `class _ { $$$ }`). Applies per file.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub inside: Option<String>,
	/// Only keep matches that CONTAIN a node matching this kind name or pattern
	/// (e.g. find functions that contain `$X.unwrap()`).
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub has: Option<String>,
	/// Regex constraints on captured metavariables, e.g. {"NAME": "^handle_"}.
	/// Keys are metavariable names without `$`.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub constraints: Option<std::collections::HashMap<String, String>>,
	/// Number of context lines around matches
	#[serde(default, skip_serializing_if = "Option::is_none")]
	#[schemars(range(min = 0, max = 10))]
	pub context: Option<usize>,
	/// Max results to return (default: 50)
	#[serde(default, skip_serializing_if = "Option::is_none")]
	#[schemars(range(min = 1, max = 200), extend("default" = 50))]
	pub max_results: Option<usize>,
	/// Pagination offset into the full result set (default: 0). The response
	/// footer reports the total and the next offset when truncated.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub offset: Option<usize>,
	/// Rewrite template with metavariable substitution (e.g. '$VAR.expect("reason")'). When provided, matched code is rewritten. Requires `pattern`.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub rewrite: Option<String>,
	/// Apply rewrites to files in-place. When false or absent, returns a diff preview. Requires rewrite parameter.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub update_all: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LspPositionParams {
	/// Relative file path
	pub file_path: String,
	/// 1-indexed line number
	pub line: u32,
	/// Symbol name on that line
	pub symbol: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LspFindReferencesParams {
	/// Relative file path
	pub file_path: String,
	/// 1-indexed line number
	pub line: u32,
	/// Symbol name on that line
	pub symbol: String,
	/// Include the declaration site in results
	#[serde(default = "default_true")]
	pub include_declaration: bool,
}

fn default_true() -> bool {
	true
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LspDocumentSymbolsParams {
	/// Relative file path
	pub file_path: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LspWorkspaceSymbolsParams {
	/// Symbol name or prefix to search
	pub query: String,
}

// ---------------------------------------------------------------------------
// Background services: watcher + indexing (preserves all original logic)
// ---------------------------------------------------------------------------

/// Manages background tasks (file watcher, debouncer, indexing).
/// Abort-on-drop ensures clean shutdown.
pub struct BackgroundServices {
	watcher_handle: Option<tokio::task::JoinHandle<()>>,
	index_handle: Option<tokio::task::JoinHandle<()>>,
	indexing_handle: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for BackgroundServices {
	fn drop(&mut self) {
		if let Some(h) = self.watcher_handle.take() {
			h.abort();
		}
		if let Some(h) = self.index_handle.take() {
			h.abort();
		}
		if let Some(h) = self.indexing_handle.take() {
			h.abort();
		}
	}
}

// ---------------------------------------------------------------------------
// McpServer
// ---------------------------------------------------------------------------

/// MCP Server implementation using rmcp SDK.
#[derive(Clone)]
pub struct McpServer {
	/// Directory this server serves. Tools that scan the filesystem
	/// (e.g. structural_search) resolve paths against this, not the process CWD,
	/// so multiple per-repo servers can run concurrently under `--multi`.
	working_directory: std::path::PathBuf,
	semantic_code: SemanticCodeProvider,
	graphrag: Option<GraphRagProvider>,
	lsp: Option<Arc<Mutex<crate::mcp::lsp::LspProvider>>>,
	indexer_enabled: bool,
	tool_router: ToolRouter<Self>,
	/// Single-entry cache of the last structural_search evaluation. Serves
	/// repeat and paginated queries without re-parsing while the candidate
	/// file set is unchanged (fingerprint + repo stamp validated per call).
	structural_cache: Arc<parking_lot::RwLock<Option<crate::mcp::structural::QueryCache>>>,
}

/// Tool execution error -> CallToolResult with is_error=true (MCP spec).
#[tool_router]
impl McpServer {
	#[tool(
		description = "Recall-oriented search by concept or behavior — finds relevant code even when its names don't match your words. Use this when you DON'T know the symbol name and are searching by intent: e.g. \"where is authentication handled\", \"code that retries failed requests\", \"rate-limiting logic\". It favors recall over precision, so expect some loosely related hits. For a KNOWN symbol, exact string, or call sites, use structural_search instead — it is precise and cheaper. Prefer an array of related terms over a single query for broader coverage."
	)]
	async fn semantic_search(
		&self,
		Parameters(params): Parameters<SemanticSearchParams>,
	) -> Result<String, String> {
		debug!("Executing semantic_search with query: {:?}", params.query);

		let mut arguments = match serde_json::to_value(&params) {
			Ok(v) => v,
			Err(e) => return Err(format!("Failed to serialize params: {}", e)),
		};

		// Apply default for max_results if not provided
		if params.max_results.is_none() {
			arguments["max_results"] = serde_json::json!(3);
		}

		match self.semantic_code.execute_search(&arguments).await {
			Ok(result) => Ok(result),
			Err(e) => Err(e.to_string()),
		}
	}

	#[tool(
		description = "Map a file or area structurally: extract function signatures, class/type definitions, and declarations without implementation bodies — the cheapest way to see what code exposes. Reach for this FIRST to orient in unfamiliar code before reading full bodies. Accepts file paths or glob patterns. Supports Rust, JS/TS, Python, Go, C++, PHP, Ruby, Bash, JSON, CSS, Svelte, Swift, Markdown."
	)]
	async fn view_signatures(
		&self,
		Parameters(params): Parameters<ViewSignaturesParams>,
	) -> Result<String, String> {
		debug!("Executing view_signatures for {} files", params.files.len());

		let arguments = match serde_json::to_value(&params) {
			Ok(v) => v,
			Err(e) => return Err(format!("Failed to serialize params: {}", e)),
		};

		match self.semantic_code.execute_view_signatures(&arguments).await {
			Ok(result) => Ok(result),
			Err(e) => Err(e.to_string()),
		}
	}

	#[tool(
		description = "Knowledge graph operations over the indexed codebase. Use for architectural queries: component relationships, dependency chains, data flows. For simple code lookup use semantic_search instead."
	)]
	async fn graphrag(
		&self,
		Parameters(params): Parameters<GraphRagParams>,
	) -> Result<String, String> {
		debug!("Executing graphrag with operation: {}", params.operation);

		match &self.graphrag {
			Some(provider) => {
				let arguments = match serde_json::to_value(&params) {
					Ok(v) => v,
					Err(e) => return Err(format!("Failed to serialize params: {}", e)),
				};

				match provider.execute(&arguments).await {
					Ok(result) => Ok(result),
					Err(e) => Err(e.to_string()),
				}
			}
			None => Err(
				"GraphRAG is not enabled in the current configuration. Please enable GraphRAG in octocode.toml to use relationship-aware search.".to_string(),
			),
		}
	}

	// --- LSP tools ---

	#[tool(description = "Jump to the definition of a symbol via LSP.")]
	async fn lsp_goto_definition(
		&self,
		Parameters(params): Parameters<LspPositionParams>,
	) -> Result<String, String> {
		let Some(ref provider) = self.lsp else {
			return Err("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.".to_string());
		};
		let args = serde_json::to_value(&params).unwrap_or_default();
		provider
			.lock()
			.await
			.execute_goto_definition(&args)
			.await
			.map_err(|e| e.to_string())
	}

	#[tool(description = "Get type info and documentation for a symbol via LSP.")]
	async fn lsp_hover(
		&self,
		Parameters(params): Parameters<LspPositionParams>,
	) -> Result<String, String> {
		let Some(ref provider) = self.lsp else {
			return Err("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.".to_string());
		};
		let args = serde_json::to_value(&params).unwrap_or_default();
		provider
			.lock()
			.await
			.execute_hover(&args)
			.await
			.map_err(|e| e.to_string())
	}

	#[tool(description = "Find all usages of a symbol across the workspace via LSP.")]
	async fn lsp_find_references(
		&self,
		Parameters(params): Parameters<LspFindReferencesParams>,
	) -> Result<String, String> {
		let Some(ref provider) = self.lsp else {
			return Err("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.".to_string());
		};
		let args = serde_json::to_value(&params).unwrap_or_default();
		provider
			.lock()
			.await
			.execute_find_references(&args)
			.await
			.map_err(|e| e.to_string())
	}

	#[tool(
		description = "List all symbols (functions, types, variables) defined in a file via LSP."
	)]
	async fn lsp_document_symbols(
		&self,
		Parameters(params): Parameters<LspDocumentSymbolsParams>,
	) -> Result<String, String> {
		let Some(ref provider) = self.lsp else {
			return Err("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.".to_string());
		};
		let args = serde_json::to_value(&params).unwrap_or_default();
		provider
			.lock()
			.await
			.execute_document_symbols(&args)
			.await
			.map_err(|e| e.to_string())
	}

	#[tool(description = "Search for symbols by name across the entire workspace via LSP.")]
	async fn lsp_workspace_symbols(
		&self,
		Parameters(params): Parameters<LspWorkspaceSymbolsParams>,
	) -> Result<String, String> {
		let Some(ref provider) = self.lsp else {
			return Err("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.".to_string());
		};
		let args = serde_json::to_value(&params).unwrap_or_default();
		provider
			.lock()
			.await
			.execute_workspace_symbols(&args)
			.await
			.map_err(|e| e.to_string())
	}

	#[tool(description = "Get code completion suggestions at a symbol position via LSP.")]
	async fn lsp_completion(
		&self,
		Parameters(params): Parameters<LspPositionParams>,
	) -> Result<String, String> {
		let Some(ref provider) = self.lsp else {
			return Err("LSP server is not available. Start MCP server with --with-lsp=\"<command>\" to enable LSP features.".to_string());
		};
		let args = serde_json::to_value(&params).unwrap_or_default();
		provider
			.lock()
			.await
			.execute_completion(&args)
			.await
			.map_err(|e| e.to_string())
	}

	// --- Structural search tool ---

	#[tool(
		description = "Search or rewrite code by AST structure, or look up real symbols by name. Three query modes (provide exactly one): \
		`pattern` — ast-grep pattern (`$X` = one node, `$$$` = sequence), bare tree-sitter kind name (`function_item`), or literal snippet; \
		`symbol` — find symbol DEFINITIONS by name, `*` wildcards supported (e.g. `handle_*`) — use this to locate where a function/type/class is defined; \
		`references` — find all usages of a name, grouped by enclosing symbol, definition sites tagged `[def]`. \
		\n\nEvery match carries an enclosing-symbol breadcrumb (e.g. `[impl Store › fn flush]`). \
		Refine with: `inside` (only matches inside a kind/pattern), `has` (only matches containing a kind/pattern), \
		`constraints` (regex per metavariable, e.g. {\"NAME\": \"^handle_\"}), `offset` (pagination). \
		\n\nCommon LLM traps to avoid: \
		(a) Decorators/attributes are children of the item — `pub struct $N { $$$ }` will NOT match `#[derive(...)] pub struct ...`. Either include the decorator (`#[derive($$$)] pub struct $N { $$$ }`) or use the bare kind (`struct_item`). \
		(b) Method patterns need a receiver — write `$X.map_err($$$)`, not `.map_err($$$)`. \
		(c) Generic arity is exact — `Result<$T>` matches one-arg Results only; use `Result<$_, $_>` for two args. \
		(d) Pattern must be valid standalone code in the language — wrap type-expressions or fragments in code (e.g. for Rust types: `Arc<Mutex<$T>>` is invalid alone — use kind `generic_type` instead). \
		\n\nLanguage tips: \
		Rust kinds: function_item, struct_item, impl_item, enum_item, trait_item, use_declaration, call_expression, macro_invocation, match_expression, generic_type. \
		JS/TS kinds: function_declaration, class_declaration, method_definition, import_statement, call_expression. \
		Python kinds: function_definition, class_definition, import_statement, import_from_statement, decorated_definition. \
		Go kinds: function_declaration, method_declaration, type_declaration, call_expression, if_statement. \
		\n\nWhen a pattern returns zero matches the tool auto-retries with relaxed strictness, kind broadening, and language-aware context wrapping, then falls back to labeled plain-text hits plus a diagnostic — it never silently returns nothing relevant. Use `rewrite` to apply a template with metavariable substitution. \
		\n\nThis is the PRECISE, low-noise way to find code you can name or describe by shape — prefer it whenever you know the symbol, pattern, or usage you're after. Only when you DON'T know the name and must search by concept or behavior, reach for `semantic_search` instead."
	)]
	async fn structural_search(
		&self,
		Parameters(params): Parameters<StructuralSearchParams>,
	) -> Result<String, String> {
		// Exactly one query mode.
		let modes = [
			params.pattern.is_some(),
			params.symbol.is_some(),
			params.references.is_some(),
		]
		.iter()
		.filter(|m| **m)
		.count();
		if modes != 1 {
			return Err("Provide exactly one of `pattern`, `symbol`, or `references`.".to_string());
		}
		if params.rewrite.is_some() && params.pattern.is_none() {
			return Err("`rewrite` requires `pattern`.".to_string());
		}

		let language = params.language.clone();
		debug!(
			"Executing structural_search lang={} pattern={:?} symbol={:?} references={:?}",
			language, params.pattern, params.symbol, params.references
		);

		// Compile metavariable constraints up front (clear error on bad regex).
		let mut constraints: Vec<crate::grep::MetavarConstraint> = Vec::new();
		if let Some(map) = &params.constraints {
			for (var, re_src) in map {
				let re = regex::Regex::new(re_src)
					.map_err(|e| format!("Invalid regex for ${}: {}", var, e))?;
				constraints.push((var.trim_start_matches('$').to_string(), re));
			}
			constraints.sort_by(|a, b| a.0.cmp(&b.0));
		}

		// Literal prefilter tokens: files lacking them can never match the
		// pattern/symbol, so they get scanned but never parsed.
		let prefilter_tokens: Vec<String> = if let Some(p) = &params.pattern {
			crate::grep::literal_tokens(p)
		} else {
			let name = params
				.symbol
				.as_deref()
				.or(params.references.as_deref())
				.unwrap_or("");
			name.split('*')
				.filter(|t| t.len() >= 2)
				.max_by_key(|t| t.len())
				.map(|t| vec![t.to_string()])
				.unwrap_or_default()
		};

		let current_dir = self.working_directory.clone();
		let (files, stamp) = crate::mcp::structural::collect_file_data(
			&current_dir,
			&language,
			params.paths.as_deref(),
			&prefilter_tokens,
		);

		// Rewrite mode: separate path, no caching.
		if let Some(ref rewrite_template) = params.rewrite {
			return self.structural_rewrite(
				&files,
				params.pattern.as_deref().unwrap_or_default(),
				rewrite_template,
				&language,
				params.update_all.unwrap_or(false),
			);
		}

		let max_results = params.max_results.unwrap_or(50);
		let offset = params.offset.unwrap_or(0);
		let context = params.context.unwrap_or(0);

		// Cache key: everything that defines the result set (not pagination).
		let constraint_key = constraints
			.iter()
			.map(|(k, re)| format!("{}={}", k, re.as_str()))
			.collect::<Vec<_>>()
			.join(";");
		let paths_key = params.paths.clone().unwrap_or_default().join(";");
		let fp = crate::mcp::structural::fingerprint_request(&[
			params.pattern.as_deref().unwrap_or(""),
			params.symbol.as_deref().unwrap_or(""),
			params.references.as_deref().unwrap_or(""),
			&language,
			params.inside.as_deref().unwrap_or(""),
			params.has.as_deref().unwrap_or(""),
			&constraint_key,
			&paths_key,
		]);

		// Serve repeat/paginated queries from the cache — skips all parsing
		// and strategy evaluation while the candidate file set is unchanged.
		{
			let cache = self.structural_cache.read();
			if let Some(qc) = cache.as_ref() {
				if qc.fingerprint == fp && qc.stamp == stamp {
					return Ok(format_structural_response(
						&qc.matches,
						qc.note.as_deref(),
						qc.diagnostic.as_deref(),
						offset,
						max_results,
						context,
						&files,
					));
				}
			}
		}

		let outcome = if let Some(pattern) = &params.pattern {
			crate::mcp::structural::smart_search(
				&files,
				pattern,
				&language,
				&constraints,
				params.inside.as_deref(),
				params.has.as_deref(),
			)
		} else if let Some(symbol) = &params.symbol {
			crate::mcp::structural::symbol_search(&files, symbol, &language, false)
		} else {
			let name = params.references.as_deref().unwrap_or_default();
			crate::mcp::structural::symbol_search(&files, name, &language, true)
		};

		let response = format_structural_response(
			&outcome.matches,
			outcome.note.as_deref(),
			outcome.diagnostic.as_deref(),
			offset,
			max_results,
			context,
			&files,
		);

		*self.structural_cache.write() = Some(crate::mcp::structural::QueryCache {
			fingerprint: fp,
			stamp,
			matches: outcome.matches,
			note: outcome.note,
			diagnostic: outcome.diagnostic,
		});

		Ok(response)
	}
}

/// Render the final structural_search response: optional strategy note,
/// formatted match page, steering footer with totals and the next offset,
/// optional diagnostic. Truncation always tells the LLM how to continue.
fn format_structural_response(
	matches: &[crate::grep::GrepMatch],
	note: Option<&str>,
	diagnostic: Option<&str>,
	offset: usize,
	max_results: usize,
	context: usize,
	files: &[crate::mcp::structural::FileData],
) -> String {
	if matches.is_empty() {
		return diagnostic.unwrap_or("No matches found.").to_string();
	}
	let total = matches.len();
	if offset >= total {
		return format!(
			"Offset {} is beyond the result set ({} total matches).",
			offset, total
		);
	}
	let end = (offset + max_results).min(total);
	let page = &matches[offset..end];

	let body = if context > 0 {
		let mut source_map = std::collections::HashMap::new();
		for fd in files {
			if page.iter().any(|m| m.file == fd.display) {
				source_map.insert(fd.display.clone(), fd.content.clone());
			}
		}
		crate::grep::format_matches_with_context(page, &source_map, context)
	} else {
		crate::grep::format_matches_grouped(page)
	};

	let mut out = String::new();
	if let Some(n) = note {
		out.push_str(n);
		out.push('\n');
	}
	out.push_str(&body);
	if total > end || offset > 0 {
		let capped = if total >= crate::mcp::structural::MAX_TOTAL_MATCHES {
			"+"
		} else {
			""
		};
		out.push_str(&format!(
			"\n\nShowing {}–{} of {}{} matches.",
			offset + 1,
			end,
			total,
			capped
		));
		if end < total {
			out.push_str(&format!(
				" Next page: offset={}. Narrow with `paths`, `inside`, `has`, or metavariable `constraints`.",
				end
			));
		}
	} else {
		out.push_str(&format!("\n\n{} matches found.", total));
	}
	if let Some(d) = diagnostic {
		out.push_str("\n\n");
		out.push_str(d);
	}
	out
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for McpServer {
	fn get_info(&self) -> ServerInfo {
		let capabilities = ServerCapabilities::builder().enable_tools().build();

		let instructions = if self.indexer_enabled {
			"This server provides modular AI tools: semantic code search, view signatures, and GraphRAG (if available). Use 'semantic_search' for code/documentation searches and 'graphrag' (if enabled) for relationship queries."
		} else {
			"WARNING: Octocode indexer is disabled: not in a git repository root. Run with --no-git to enable indexing outside git repos. Tools available: semantic_search, view_signatures, graphrag (if enabled)."
		};

		ServerInfo::new(capabilities)
			.with_server_info(
				Implementation::new("octocode-mcp", env!("CARGO_PKG_VERSION"))
					.with_description("Semantic code search server with vector embeddings and optional GraphRAG support"),
			)
			.with_instructions(instructions)
	}
}

impl McpServer {
	fn structural_rewrite(
		&self,
		files: &[crate::mcp::structural::FileData],
		pattern: &str,
		rewrite_template: &str,
		language: &str,
		update_all: bool,
	) -> Result<String, String> {
		let mut results = Vec::new();
		let mut total_replacements = 0;

		// Prefilter-missed files cannot contain the pattern's literal tokens,
		// so they cannot match — skip without parsing.
		for fd in files.iter().filter(|f| f.prefilter_hit) {
			match crate::grep::rewrite_file(
				&fd.display,
				&fd.content,
				pattern,
				rewrite_template,
				language,
			) {
				Ok(Some(result)) => {
					total_replacements += result.replacements;
					results.push((fd.path.clone(), result));
				}
				Ok(None) => {}
				Err(e) => {
					debug!("Error rewriting {}: {}", fd.display, e);
				}
			}
		}

		if results.is_empty() {
			return Ok("No matches found.".to_string());
		}

		if update_all {
			for (path, result) in &results {
				if let Err(e) = std::fs::write(path, &result.rewritten_source) {
					return Err(format!("Failed to write {}: {}", result.file, e));
				}
			}
			Ok(format!(
				"Applied {} replacements across {} files.",
				total_replacements,
				results.len()
			))
		} else {
			let mut output = String::new();
			for (_, result) in &results {
				output.push_str(&crate::grep::format_rewrite_diff(result));
				output.push_str("\n\n");
			}
			output.push_str(&format!(
				"{} replacements across {} files (preview, set update_all=true to apply).",
				total_replacements,
				results.len()
			));
			Ok(output)
		}
	}

	/// Lightweight per-repo request handler for `--multi` mode.
	///
	/// Builds only the providers and tool router — no store, no LSP, no
	/// background services, no `set_current_dir`. The multi-server owns the
	/// repo lifecycle (indexing, watcher, cleanup, logging) and routes calls
	/// here by the injected `project` argument.
	pub(crate) fn new_repo_core(config: Config, working_directory: std::path::PathBuf) -> Self {
		let semantic_code = SemanticCodeProvider::new(config.clone(), working_directory.clone());
		let graphrag = GraphRagProvider::new(config, working_directory.clone());

		let mut tool_router = Self::tool_router();

		// Multi mode never has LSP — remove LSP tools
		for name in [
			"lsp_goto_definition",
			"lsp_hover",
			"lsp_find_references",
			"lsp_document_symbols",
			"lsp_workspace_symbols",
			"lsp_completion",
		] {
			tool_router.remove_route(name);
		}

		if graphrag.is_none() {
			tool_router.remove_route("graphrag");
		}

		Self {
			working_directory,
			semantic_code,
			graphrag,
			lsp: None,
			indexer_enabled: true,
			tool_router,
			structural_cache: Arc::new(parking_lot::RwLock::new(None)),
		}
	}

	/// Tool definitions exposed by this per-repo server. The multi-server clones
	/// these and injects a `project` argument before serving them to clients.
	pub(crate) fn list_tool_defs(&self) -> Vec<Tool> {
		self.tool_router.list_all()
	}

	/// Dispatch a tool call against this server's router. Used by the
	/// multi-server after it has resolved `project` and stripped it from args.
	pub(crate) async fn dispatch_tool(
		&self,
		request: CallToolRequestParams,
		context: RequestContext<RoleServer>,
	) -> Result<CallToolResult, ErrorData> {
		let tcc = ToolCallContext::new(self, request, context);
		self.tool_router.call(tcc).await
	}

	/// Create the per-repo store and start background watcher + indexing for a
	/// repository served under `--multi`. Returns the abort-on-drop handle
	/// bundle; dropping it tears down the repo's background threads.
	pub(crate) async fn start_repo_services(
		config: Config,
		working_directory: std::path::PathBuf,
		no_git: bool,
		debug: bool,
	) -> Result<BackgroundServices> {
		let db_path = crate::storage::get_project_database_path(&working_directory)?;
		crate::storage::ensure_project_storage_exists(&working_directory)?;
		let store = Arc::new(Store::new_with_path(db_path).await?);
		store.initialize_collections().await?;

		// One-shot initial index so a freshly-accessed repo is searchable without
		// waiting for a file change. Incremental and lock-guarded, so it's cheap
		// when already indexed and safe against the watcher-triggered reindex.
		{
			let store = store.clone();
			let config = config.clone();
			let working_directory = working_directory.clone();
			tokio::spawn(async move {
				if let Err(e) = perform_indexing(&store, &config, &working_directory, no_git).await
				{
					warn!(
						"Initial index failed for {}: {}",
						working_directory.display(),
						e
					);
				}
			});
		}

		start_background_services(config, store, working_directory, no_git, debug, None).await
	}

	/// Create a new MCP server instance.
	///
	/// Initialises the store, logging, providers, and optionally spawns LSP background init.
	pub async fn new(
		config: Config,
		debug_mode: bool,
		working_directory: std::path::PathBuf,
		no_git: bool,
		lsp_command: Option<String>,
	) -> Result<(Self, BackgroundServices)> {
		// Change to the working directory at server startup
		std::env::set_current_dir(&working_directory).map_err(|e| {
			anyhow::anyhow!(
				"Failed to change to working directory '{}': {}",
				working_directory.display(),
				e
			)
		})?;

		// Initialize the store
		let store = Store::new().await?;
		store.initialize_collections().await?;
		let store = Arc::new(store);

		// Initialize logging
		init_mcp_logging(working_directory.clone(), debug_mode)?;

		let semantic_code = SemanticCodeProvider::new(config.clone(), working_directory.clone());
		let graphrag = GraphRagProvider::new(config.clone(), working_directory.clone());

		// Initialize LSP provider if command is provided (lazy initialization)
		let lsp = if let Some(command) = lsp_command {
			info!(
				"LSP provider will be initialized lazily with command: {}",
				command
			);
			let provider = Arc::new(Mutex::new(crate::mcp::lsp::LspProvider::new(
				working_directory.clone(),
				command,
			)));

			// Start LSP initialization in background (non-blocking)
			let provider_clone = provider.clone();
			tokio::spawn(async move {
				let mut provider_guard = provider_clone.lock().await;
				if let Err(e) = provider_guard.start_initialization().await {
					warn!("LSP initialization failed: {}", e);
				}
			});

			Some(provider)
		} else {
			None
		};

		// Determine if indexer should start
		let should_start_indexer = if !no_git && config.index.require_git {
			indexer::git::is_git_repo_root(&working_directory)
		} else {
			true
		};

		if !should_start_indexer {
			warn!(
				"Indexer not started: Not in a git repository and --no-git flag not set. \
				 Use --no-git to enable indexing outside git repos."
			);
		}

		// Build tool router — LSP tools return helpful errors when LSP is not configured
		let mut tool_router = Self::tool_router();

		// Remove LSP tools from router if LSP is not configured (matching old server behaviour)
		if lsp.is_none() {
			for name in [
				"lsp_goto_definition",
				"lsp_hover",
				"lsp_find_references",
				"lsp_document_symbols",
				"lsp_workspace_symbols",
				"lsp_completion",
			] {
				tool_router.remove_route(name);
			}
		}

		// Remove GraphRAG tool if not configured
		if graphrag.is_none() {
			tool_router.remove_route("graphrag");
		}

		let server = Self {
			working_directory: working_directory.clone(),
			semantic_code,
			graphrag,
			lsp,
			indexer_enabled: should_start_indexer,
			tool_router,
			structural_cache: Arc::new(parking_lot::RwLock::new(None)),
		};

		// Start background services (watcher + indexing)
		let bg = if should_start_indexer {
			start_background_services(
				config,
				store,
				working_directory,
				no_git,
				debug_mode,
				server.lsp.clone(),
			)
			.await?
		} else {
			BackgroundServices {
				watcher_handle: None,
				index_handle: None,
				indexing_handle: None,
			}
		};

		info!(
			"MCP Server initialized (debug_mode={}, indexer={}, debounce={}ms, timeout={}ms, max_events={})",
			debug_mode, should_start_indexer, MCP_DEBOUNCE_MS, MCP_INDEX_TIMEOUT_MS, MCP_MAX_PENDING_EVENTS
		);

		Ok((server, bg))
	}

	/// Run the server using stdio transport (default MCP mode)
	pub async fn run_stdio(self, _bg: BackgroundServices) -> Result<()> {
		// Guard against panics in tool handlers crashing the whole server
		let original_hook = std::panic::take_hook();
		std::panic::set_hook(Box::new(move |info| {
			super::logging::log_critical_anyhow_error(
				"Panic in MCP server",
				&anyhow::anyhow!("{}", info),
			);
			original_hook(info);
		}));

		info!("Starting MCP server in stdio mode");

		let transport = rmcp::transport::io::stdio();
		let service = self.serve(transport).await?;

		// Wait for the service to complete (EOF / client disconnect)
		service.waiting().await?;

		// _bg is dropped here -> background tasks aborted
		Ok(())
	}

	/// Run the server using Streamable HTTP transport (MCP 2025-03-26 spec).
	///
	/// Uses rmcp's `StreamableHttpService` with `LocalSessionManager` for
	/// proper session management. Supports both SSE and plain JSON responses
	/// as required by the spec.
	pub async fn run_http(self, bind_addr: &str, _bg: BackgroundServices) -> Result<()> {
		use hyper_util::rt::TokioIo;
		use hyper_util::service::TowerToHyperService;
		use rmcp::transport::streamable_http_server::{
			session::local::LocalSessionManager, StreamableHttpService,
		};

		info!("Starting MCP server in HTTP mode on {}", bind_addr);

		let server = self.clone();

		// StreamableHttpService handles session lifecycle, SSE vs JSON content
		// negotiation, and MCP protocol compliance automatically.
		let service = StreamableHttpService::new(
			move || Ok(server.clone()),
			Arc::new(LocalSessionManager::default()),
			Default::default(),
		);

		let addr: std::net::SocketAddr = bind_addr
			.parse()
			.map_err(|e| anyhow::anyhow!("Invalid bind address '{}': {}", bind_addr, e))?;

		let listener = tokio::net::TcpListener::bind(addr).await?;
		info!("MCP HTTP server listening on {}", addr);

		loop {
			let (stream, remote_addr) = listener.accept().await?;
			debug!("Accepted connection from {}", remote_addr);

			let service = service.clone();
			tokio::spawn(async move {
				let io = TokioIo::new(stream);
				let hyper_service = TowerToHyperService::new(service);

				if let Err(e) = hyper::server::conn::http1::Builder::new()
					.serve_connection(io, hyper_service)
					.await
				{
					debug!("Connection error from {}: {}", remote_addr, e);
				}
			});
		}
	}
}

// ---------------------------------------------------------------------------
// Background services setup (preserves all watcher + indexing logic)
// ---------------------------------------------------------------------------

pub(crate) async fn start_background_services(
	config: Config,
	store: Arc<Store>,
	working_directory: std::path::PathBuf,
	no_git: bool,
	debug: bool,
	lsp: Option<Arc<Mutex<crate::mcp::lsp::LspProvider>>>,
) -> Result<BackgroundServices> {
	let (file_tx, file_rx) = mpsc::channel(MCP_MAX_PENDING_EVENTS);
	let (index_tx, index_rx) = mpsc::channel(10);

	// 1. File watcher
	let working_dir = working_directory.clone();
	let watcher_handle = tokio::spawn(async move {
		if let Err(e) = run_watcher(file_tx, working_dir, debug, MCP_MAX_PENDING_EVENTS).await {
			log_critical_anyhow_error("Watcher error", &e);
		}
	});

	// 2. Debouncer: accumulates file events, triggers indexing after quiet period
	let indexing_in_progress = Arc::new(AtomicBool::new(false));
	let indexing_flag = indexing_in_progress.clone();
	let debug_mode = debug;
	let index_handle = tokio::spawn(async move {
		let mut file_rx = file_rx;
		let mut last_event_time = None::<Instant>;
		let mut pending_events = 0u32;

		loop {
			let timeout_duration = Duration::from_millis(MCP_DEBOUNCE_MS);

			tokio::select! {
				event_result = file_rx.recv() => {
					match event_result {
						Some(_) => {
							pending_events += 1;
							last_event_time = Some(Instant::now());
							log_watcher_event("file_change", None, pending_events as usize);
						}
						None => {
							debug!("File watcher channel closed, stopping debouncer");
							break;
						}
					}
				}

				_ = sleep(timeout_duration), if last_event_time.is_some() => {
					if let Some(last_time) = last_event_time {
						if last_time.elapsed() >= timeout_duration && pending_events > 0 {
							if indexing_flag
								.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
								.is_ok()
							{
								if debug_mode {
									debug!(
										"Debounce period completed ({} pending events), requesting reindex",
										pending_events
									);
								}
								log_watcher_event("debounce_trigger", None, pending_events as usize);

								if (index_tx.send(()).await).is_err() {
									if debug_mode {
										debug!("Failed to send index request - server may be shutting down");
									}
									indexing_flag.store(false, Ordering::SeqCst);
									break;
								}

								pending_events = 0;
								last_event_time = None;
							} else if debug_mode {
								debug!("Indexing already in progress, will retry after current indexing completes");
							}
						}
					}
				}
			}
		}
	});

	// 3. Background indexing task
	let indexing_flag2 = indexing_in_progress;
	let indexing_handle = tokio::spawn(async move {
		let mut index_rx = index_rx;
		loop {
			match index_rx.recv().await {
				Some(_) => {
					debug!("Processing index request in background");

					// Additional delay to ensure all file operations are complete
					sleep(Duration::from_millis(DEFAULT_ADDITIONAL_DELAY_MS)).await;

					let indexing_result = tokio::time::timeout(
						Duration::from_millis(MCP_INDEX_TIMEOUT_MS),
						perform_indexing(&store, &config, &working_directory, no_git),
					)
					.await;

					match indexing_result {
						Ok(Ok(())) => {
							info!("Background reindex completed successfully");

							// Update LSP with changed files if LSP is enabled
							if let Some(ref lsp_provider) = lsp {
								let mut lsp_guard = lsp_provider.lock().await;
								if let Err(e) =
									update_lsp_after_indexing(&mut lsp_guard, &working_directory)
										.await
								{
									debug!("LSP update after indexing failed: {}", e);
								}
							}
						}
						Ok(Err(e)) => {
							log_critical_anyhow_error("Background reindex error", &e);
						}
						Err(_) => {
							log_critical_anyhow_error(
								"Background reindex timeout",
								&anyhow::anyhow!(
									"Background reindex timed out after {}ms",
									MCP_INDEX_TIMEOUT_MS
								),
							);
						}
					}

					// Always reset the indexing flag
					indexing_flag2.store(false, Ordering::SeqCst);
				}
				None => {
					debug!("Background indexing channel closed, stopping indexing task");
					break;
				}
			}
		}
	});

	Ok(BackgroundServices {
		watcher_handle: Some(watcher_handle),
		index_handle: Some(index_handle),
		indexing_handle: Some(indexing_handle),
	})
}

// ---------------------------------------------------------------------------
// Indexing helpers (preserved from original server.rs)
// ---------------------------------------------------------------------------

pub(crate) async fn perform_indexing(
	store: &Store,
	config: &Config,
	working_directory: &std::path::Path,
	no_git: bool,
) -> Result<()> {
	let start_time = std::time::Instant::now();
	log_indexing_operation("direct_reindex_start", None, None, true);

	let mut lock = IndexLock::new(working_directory)?;
	lock.acquire_async()
		.await
		.map_err(|e| anyhow::anyhow!("Failed to acquire index lock: {}", e))?;
	debug!("MCP server: acquired indexing lock");

	let state = state::create_shared_state();
	state.write().current_directory = working_directory.to_path_buf();

	let git_repo_root = if !no_git {
		indexer::git::find_git_root(working_directory)
	} else {
		None
	};

	let indexing_result = indexer::index_files_with_quiet(
		store,
		state.clone(),
		config,
		git_repo_root.as_deref(),
		true,
	)
	.await;

	lock.release()?;
	debug!("MCP server: released indexing lock");

	let duration_ms = start_time.elapsed().as_millis() as u64;

	match indexing_result {
		Ok(()) => {
			log_indexing_operation("direct_reindex_complete", None, Some(duration_ms), true);
			Ok(())
		}
		Err(e) => {
			log_indexing_operation("direct_reindex_complete", None, Some(duration_ms), false);
			log_critical_anyhow_error("Direct indexing", &e);
			Err(e)
		}
	}
}

/// Update LSP server with recently changed files after indexing
async fn update_lsp_after_indexing(
	lsp_provider: &mut crate::mcp::lsp::LspProvider,
	working_directory: &std::path::Path,
) -> Result<()> {
	use crate::indexer::{detect_language, NoindexWalker, PathUtils};

	debug!("Updating LSP server with changed files");

	let walker = NoindexWalker::create_walker(working_directory).build();
	let mut files_updated = 0;

	for result in walker {
		let entry = match result {
			Ok(entry) => entry,
			Err(_) => continue,
		};

		if !entry.file_type().is_some_and(|ft| ft.is_file()) {
			continue;
		}

		if detect_language(entry.path()).is_some() {
			let relative_path = PathUtils::to_relative_string(entry.path(), working_directory);

			if let Err(e) = lsp_provider.update_file(&relative_path).await {
				debug!("Failed to update file {} in LSP: {}", relative_path, e);
			} else {
				files_updated += 1;
			}
		}
	}

	debug!("LSP update completed: {} files updated", files_updated);
	Ok(())
}
