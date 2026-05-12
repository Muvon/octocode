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
	handler::server::{router::tool::ToolRouter, wrapper::Parameters},
	model::{Implementation, ServerCapabilities, ServerInfo},
	schemars, tool, tool_handler, tool_router, ServerHandler, ServiceExt,
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
	/// String or array of strings describing functionality to find. Array preferred for comprehensive results.
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
	#[schemars(length(min = 1, max = 100))]
	pub files: Vec<String>,
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
	/// AST pattern using ast-grep syntax (e.g. '$FUNC.unwrap()', 'if err != nil { $$$ }')
	pub pattern: String,
	/// Language to search (required: rust, javascript, typescript, python, go, java, cpp, php, ruby, lua, bash, css, json)
	pub language: String,
	/// File paths or glob patterns to search (default: current directory)
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub paths: Option<Vec<String>>,
	/// Number of context lines around matches
	#[serde(default, skip_serializing_if = "Option::is_none")]
	#[schemars(range(min = 0, max = 10))]
	pub context: Option<usize>,
	/// Max results to return (default: 50)
	#[serde(default, skip_serializing_if = "Option::is_none")]
	#[schemars(range(min = 1, max = 200), extend("default" = 50))]
	pub max_results: Option<usize>,
	/// Rewrite template with metavariable substitution (e.g. '$VAR.expect("reason")'). When provided, matched code is rewritten.
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
	semantic_code: SemanticCodeProvider,
	graphrag: Option<GraphRagProvider>,
	lsp: Option<Arc<Mutex<crate::mcp::lsp::LspProvider>>>,
	indexer_enabled: bool,
	tool_router: ToolRouter<Self>,
}

/// Tool execution error -> CallToolResult with is_error=true (MCP spec).
#[tool_router]
impl McpServer {
	#[tool(
		description = "Search codebase by meaning. Finds code by what it does, not exact symbol names. Prefer an array of related terms over a single query for broader coverage."
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
		description = "Extract function signatures, class definitions, and declarations from files without implementation bodies. Supports Rust, JS/TS, Python, Go, C++, PHP, Ruby, Bash, JSON, CSS, Svelte, Markdown."
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
		description = "Search or rewrite code by AST structure using ast-grep pattern syntax. Finds code matching structural patterns like '$FUNC.unwrap()', 'if let Some($X) = $Y { $$$ }'. The pattern argument accepts THREE shapes — the tool tries them in order automatically so an LLM can pass whichever it knows: (1) ast-grep pattern with metavariables ($X, $$$ARGS); (2) bare tree-sitter node kind name like 'function_declaration' or 'call_expression' to match all nodes of that kind; (3) an exact code snippet to match literally. When a pattern returns zero matches the tool automatically retries with relaxed strictness and language-aware context wrapping (resolves Go type-conversion ambiguity, class fields parsing as assignments, JSON key/value fragments), and emits a diagnostic showing how the pattern was parsed and what to try next. Optionally rewrite matches using a template with metavariable substitution. Complements semantic_search: use this for structural/syntactic patterns, semantic_search for meaning-based queries."
	)]
	async fn structural_search(
		&self,
		Parameters(params): Parameters<StructuralSearchParams>,
	) -> Result<String, String> {
		debug!(
			"Executing structural_search with pattern: {} lang: {}",
			params.pattern, params.language
		);

		let current_dir = std::env::current_dir().map_err(|e| e.to_string())?;
		let max_results = params.max_results.unwrap_or(50);
		let context = params.context.unwrap_or(0);

		// Collect matching files
		let walker = ignore::WalkBuilder::new(&current_dir)
			.git_ignore(true)
			.git_global(true)
			.git_exclude(true)
			.hidden(true)
			.build();

		let mut files = Vec::new();
		for entry in walker.flatten() {
			if !entry.file_type().is_some_and(|ft| ft.is_file()) {
				continue;
			}
			let path = entry.path();
			if crate::grep::language_from_extension(path) != Some(params.language.as_str()) {
				continue;
			}
			if let Some(ref filter_paths) = params.paths {
				let rel = path
					.strip_prefix(&current_dir)
					.unwrap_or(path)
					.to_string_lossy();
				if !filter_paths.iter().any(|p| rel.contains(p)) {
					continue;
				}
			}
			files.push(path.to_path_buf());
		}

		// Branch: rewrite mode vs search mode
		if let Some(ref rewrite_template) = params.rewrite {
			return self.structural_rewrite(
				&files,
				&current_dir,
				&params.pattern,
				rewrite_template,
				&params.language,
				params.update_all.unwrap_or(false),
			);
		}

		let outcome = smart_search_all_files(
			&files,
			&current_dir,
			&params.pattern,
			&params.language,
			max_results,
			context > 0,
		);

		if outcome.matches.is_empty() {
			return Ok(outcome
				.diagnostic
				.unwrap_or_else(|| "No matches found.".to_string()));
		}

		let output = if context > 0 {
			crate::grep::format_matches_with_context(&outcome.matches, &outcome.source_map, context)
		} else {
			crate::grep::format_matches_grouped(&outcome.matches)
		};

		let header = outcome
			.note
			.as_deref()
			.map(|n| format!("[fallback: {}]\n", n))
			.unwrap_or_default();

		Ok(format!(
			"{}{}\n\n{} matches found.",
			header,
			output,
			outcome.matches.len()
		))
	}
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
		files: &[std::path::PathBuf],
		current_dir: &std::path::Path,
		pattern: &str,
		rewrite_template: &str,
		language: &str,
		update_all: bool,
	) -> Result<String, String> {
		let mut results = Vec::new();
		let mut total_replacements = 0;

		for path in files {
			let source = match std::fs::read_to_string(path) {
				Ok(s) => s,
				Err(_) => continue,
			};
			let display_path = path
				.strip_prefix(current_dir)
				.unwrap_or(path)
				.to_string_lossy()
				.to_string();

			match crate::grep::rewrite_file(
				&display_path,
				&source,
				pattern,
				rewrite_template,
				language,
			) {
				Ok(Some(result)) => {
					total_replacements += result.replacements;
					results.push((path.clone(), result));
				}
				Ok(None) => {}
				Err(e) => {
					debug!("Error rewriting {}: {}", display_path, e);
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

	/// Lightweight constructor for proxy mode.
	///
	/// Creates only the providers and tool router — no store, no LSP, no
	/// background services, no `set_current_dir`.  The proxy manages its own
	/// lifecycle (indexing, cleanup, logging).
	pub fn new_for_proxy(config: Config, working_directory: std::path::PathBuf) -> Result<Self> {
		let semantic_code = SemanticCodeProvider::new(config.clone(), working_directory.clone());
		let graphrag = GraphRagProvider::new(config, working_directory);

		let mut tool_router = Self::tool_router();

		// Proxy never has LSP — remove LSP tools
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

		Ok(Self {
			semantic_code,
			graphrag,
			lsp: None,
			indexer_enabled: false,
			tool_router,
		})
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
			semantic_code,
			graphrag,
			lsp,
			indexer_enabled: should_start_indexer,
			tool_router,
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
// Smart structural-search orchestration
// ---------------------------------------------------------------------------

/// Result of a multi-strategy structural search across a file list.
struct SmartSearchOutcome {
	matches: Vec<crate::grep::GrepMatch>,
	source_map: std::collections::HashMap<String, String>,
	/// When a non-default strategy yielded the matches, describes which one.
	note: Option<String>,
	/// Human-readable explanation of why zero matches were found. Only set when
	/// `matches` is empty; intended to guide an LLM toward a better next attempt.
	diagnostic: Option<String>,
}

/// Run the user pattern against `files` using a chain of strategies. Each step
/// fires only when the previous one produced zero matches; the first step that
/// yields anything wins. On total failure the result carries a diagnostic
/// describing how the pattern was parsed and what alternative shapes to try.
///
/// Strategy order (designed for the way LLMs commonly mis-pattern):
///   1. Smart-strictness ast-grep pattern (the default)
///   2. Relaxed-strictness ast-grep pattern (ignores trivia/comments)
///   3. KindMatcher — treat the pattern as a tree-sitter node kind
///   4. Language-aware contextual fallback for known parsing traps
fn smart_search_all_files(
	files: &[std::path::PathBuf],
	current_dir: &std::path::Path,
	pattern: &str,
	language: &str,
	max_results: usize,
	want_source: bool,
) -> SmartSearchOutcome {
	// Capture metavariable names once for downstream relevance reranking
	// (e.g. pattern `$User.save()` boosts files whose path contains "user").
	let metavars: Vec<String> = crate::grep::pattern_info(pattern, language)
		.map(|info| info.defined_vars)
		.unwrap_or_default();

	let finalize = |mut out: StrategyOutput, note: Option<String>| -> SmartSearchOutcome {
		rerank_matches(&mut out.0, &metavars);
		out.0.truncate(max_results);
		SmartSearchOutcome {
			matches: out.0,
			source_map: out.1,
			note,
			diagnostic: None,
		}
	};

	// Strategy 1 — default pattern + Smart strictness
	let r1 = run_strategy_pattern(
		files,
		current_dir,
		pattern,
		language,
		crate::grep::MatchStrictness::Smart,
		max_results,
		want_source,
	);
	if !r1.0.is_empty() {
		return finalize(r1, None);
	}

	// Strategy 2 — same pattern, Relaxed strictness (skips trivia + comments)
	let r2 = run_strategy_pattern(
		files,
		current_dir,
		pattern,
		language,
		crate::grep::MatchStrictness::Relaxed,
		max_results,
		want_source,
	);
	if !r2.0.is_empty() {
		return finalize(
			r2,
			Some("relaxed strictness (default Smart matched nothing)".to_string()),
		);
	}

	// Strategy 3 — KindMatcher with the raw pattern
	let r3 = run_strategy_kind(
		files,
		current_dir,
		pattern,
		language,
		max_results,
		want_source,
	);
	if !r3.0.is_empty() {
		return finalize(
			r3,
			Some(format!("interpreted '{}' as AST node kind", pattern)),
		);
	}

	// Strategy 3b — canonical kind mapping (rescues LLM naming mismatches
	// like `function_declaration` in Python where grammar uses `function_definition`).
	if let Some(canonical) = crate::grep::canonical_kind(pattern, language) {
		if canonical != pattern {
			let r3b = run_strategy_kind(
				files,
				current_dir,
				canonical,
				language,
				max_results,
				want_source,
			);
			if !r3b.0.is_empty() {
				return finalize(
					r3b,
					Some(format!(
						"mapped '{}' to canonical {} kind '{}'",
						pattern, language, canonical
					)),
				);
			}
		}
	}

	// Strategy 4 — language-aware contextual fallback
	for (desc, selector, context_src) in auto_contexts(language, pattern) {
		let r4 = run_strategy_contextual(
			files,
			current_dir,
			&context_src,
			selector,
			language,
			max_results,
			want_source,
		);
		if !r4.0.is_empty() {
			return finalize(r4, Some(format!("auto-context wrapping ({})", desc)));
		}
	}

	// All strategies exhausted — build diagnostic
	SmartSearchOutcome {
		matches: Vec::new(),
		source_map: std::collections::HashMap::new(),
		note: None,
		diagnostic: Some(build_diagnostic(pattern, language)),
	}
}

/// Light reranking: hoist matches whose file path mentions any metavariable
/// name (case-insensitive). Stable within the priority class so per-file
/// match order is preserved.
fn rerank_matches(matches: &mut [crate::grep::GrepMatch], metavars: &[String]) {
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

type StrategyOutput = (
	Vec<crate::grep::GrepMatch>,
	std::collections::HashMap<String, String>,
);

fn run_strategy_pattern(
	files: &[std::path::PathBuf],
	current_dir: &std::path::Path,
	pattern: &str,
	language: &str,
	strictness: crate::grep::MatchStrictness,
	max_results: usize,
	want_source: bool,
) -> StrategyOutput {
	let mut matches = Vec::new();
	let mut source_map = std::collections::HashMap::new();
	for path in files {
		if matches.len() >= max_results {
			break;
		}
		let source = match std::fs::read_to_string(path) {
			Ok(s) => s,
			Err(_) => continue,
		};
		let display_path = path
			.strip_prefix(current_dir)
			.unwrap_or(path)
			.to_string_lossy()
			.to_string();
		if let Ok(found) = crate::grep::search_file_strict(
			&display_path,
			&source,
			pattern,
			language,
			strictness.clone(),
		) {
			if !found.is_empty() {
				if want_source {
					source_map.insert(display_path.clone(), source);
				}
				matches.extend(found);
			}
		}
	}
	matches.truncate(max_results);
	(matches, source_map)
}

fn run_strategy_kind(
	files: &[std::path::PathBuf],
	current_dir: &std::path::Path,
	kind: &str,
	language: &str,
	max_results: usize,
	want_source: bool,
) -> StrategyOutput {
	let mut matches = Vec::new();
	let mut source_map = std::collections::HashMap::new();
	for path in files {
		if matches.len() >= max_results {
			break;
		}
		let source = match std::fs::read_to_string(path) {
			Ok(s) => s,
			Err(_) => continue,
		};
		let display_path = path
			.strip_prefix(current_dir)
			.unwrap_or(path)
			.to_string_lossy()
			.to_string();
		if let Ok(found) = crate::grep::search_file_by_kind(&display_path, &source, kind, language)
		{
			if !found.is_empty() {
				if want_source {
					source_map.insert(display_path.clone(), source);
				}
				matches.extend(found);
			}
		}
	}
	matches.truncate(max_results);
	(matches, source_map)
}

fn run_strategy_contextual(
	files: &[std::path::PathBuf],
	current_dir: &std::path::Path,
	context_src: &str,
	selector: &str,
	language: &str,
	max_results: usize,
	want_source: bool,
) -> StrategyOutput {
	let mut matches = Vec::new();
	let mut source_map = std::collections::HashMap::new();
	for path in files {
		if matches.len() >= max_results {
			break;
		}
		let source = match std::fs::read_to_string(path) {
			Ok(s) => s,
			Err(_) => continue,
		};
		let display_path = path
			.strip_prefix(current_dir)
			.unwrap_or(path)
			.to_string_lossy()
			.to_string();
		if let Ok(found) = crate::grep::search_file_contextual(
			&display_path,
			&source,
			context_src,
			selector,
			language,
		) {
			if !found.is_empty() {
				if want_source {
					source_map.insert(display_path.clone(), source);
				}
				matches.extend(found);
			}
		}
	}
	matches.truncate(max_results);
	(matches, source_map)
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

/// Produce a structured diagnostic when no strategy matched. The message tells
/// the LLM how the pattern was parsed, what was tried, and what to try next.
fn build_diagnostic(pattern: &str, language: &str) -> String {
	let mut out = String::new();
	out.push_str(&format!(
		"No matches for pattern: {}\nLanguage: {}\n",
		pattern, language
	));

	match crate::grep::pattern_info(pattern, language) {
		Ok(info) => {
			out.push_str("\nPattern parsed as:\n");
			out.push_str(&format!(
				"  root_kind = {}\n",
				info.root_kind.as_deref().unwrap_or("(bare metavariable)")
			));
			out.push_str(&format!("  has_parse_error = {}\n", info.has_error));
			if info.defined_vars.is_empty() {
				out.push_str("  metavariables = (none)\n");
			} else {
				out.push_str(&format!(
					"  metavariables = {}\n",
					info.defined_vars
						.iter()
						.map(|v| format!("${}", v))
						.collect::<Vec<_>>()
						.join(", ")
				));
			}
		}
		Err(e) => {
			out.push_str(&format!("\nPattern does not parse: {}\n", e));
		}
	}

	out.push_str("\nStrategies tried (all returned 0 matches):\n");
	out.push_str("  - ast-grep pattern with Smart strictness\n");
	out.push_str("  - ast-grep pattern with Relaxed strictness\n");
	out.push_str("  - KindMatcher (pattern as AST node kind name)\n");
	if let Some(canonical) = crate::grep::canonical_kind(pattern, language) {
		if canonical != pattern {
			out.push_str(&format!(
				"  - KindMatcher with canonical kind '{}'\n",
				canonical
			));
		}
	}
	if !auto_contexts(language, pattern).is_empty() {
		out.push_str("  - language-aware context wrapping\n");
	}

	// Worked examples: the strongest single LLM-correction signal in the
	// literature (Structural-Code-Search paper, +12 F1 on F1=33→45).
	let examples = worked_examples(language);
	if !examples.is_empty() {
		out.push_str(&format!(
			"\nKnown-good {} pattern shapes (copy + adapt):\n",
			language
		));
		for (pat, desc) in examples {
			out.push_str(&format!("  - {}\n    {}\n", pat, desc));
		}
	}

	out.push_str("\nSuggestions for the next attempt:\n");
	if let Some(canonical) = crate::grep::canonical_kind(pattern, language) {
		if canonical != pattern {
			out.push_str(&format!(
				"  • In {}, the canonical kind for that intent is '{}' — \
				   try it directly.\n",
				language, canonical
			));
		}
	}
	out.push_str(
		"  • If matching code shape, write the pattern as valid standalone code\n  \
		   in this language (wrap statements in a function body if needed).\n",
	);
	out.push_str(
		"  • If matching by node type, pass a tree-sitter kind name (above examples\n  \
		   show typical shapes).\n",
	);
	out.push_str(
		"  • For meaning-based lookups (\"find error handling\", \"find auth checks\")\n  \
		   prefer the semantic_search tool over structural patterns.\n",
	);
	out
}

/// A small catalog of high-coverage example patterns per language. Inlined
/// into the zero-match diagnostic so the LLM can rewrite its next attempt
/// against a known-parsing shape rather than guess. Validated against the
/// grammars we link in `define_ast_grep_lang!`.
fn worked_examples(language: &str) -> Vec<(&'static str, &'static str)> {
	match language {
		"rust" => vec![
			("$X.unwrap()", "any unwrap() call on any expression"),
			("fn $NAME($$$ARGS) { $$$ }", "any function declaration"),
			("if let Some($X) = $Y { $$$ }", "if-let-Some binding"),
			("impl $T for $S { $$$ }", "trait implementations"),
			("use $PATH;", "use-imports (kind: use_declaration)"),
		],
		"javascript" => vec![
			("console.$M($$$)", "any console method call"),
			("function $NAME($$$) { $$$ }", "function declarations"),
			("($$$PARAMS) => $BODY", "arrow functions"),
			("import { $$$ } from $SRC", "named ES imports"),
			("try { $$$ } catch ($E) { $$$ }", "try/catch blocks"),
		],
		"typescript" => vec![
			("async function $N($$$) { $$$ }", "async functions"),
			("interface $NAME { $$$ }", "interface declarations"),
			("import { $$$ } from $SRC", "named ES imports"),
			(
				"class $C { $NAME: $TYPE }",
				"class members (use selector 'public_field_definition' if direct pattern misses)",
			),
		],
		"python" => vec![
			(
				"def $NAME($$$): $$$",
				"function definitions (kind: function_definition)",
			),
			(
				"class $NAME: $$$",
				"class definitions (kind: class_definition)",
			),
			("from $MOD import $$$", "from-imports"),
			("@$DECO", "decorators (kind: decorator)"),
		],
		"go" => vec![
			("if err != nil { $$$ }", "the err-check idiom"),
			(
				"func $N($$$) $RET { $$$ }",
				"function declarations (kind: function_declaration)",
			),
			(
				"$X.$M($$$)",
				"method-style calls (Go grammar prefers contextual selector 'call_expression')",
			),
		],
		"java" => vec![
			(
				"public $RET $NAME($$$) { $$$ }",
				"public method declarations",
			),
			("$O.$M($$$)", "method invocations (kind: method_invocation)"),
			("import $PKG;", "imports (kind: import_declaration)"),
		],
		"cpp" => vec![
			(
				"$RET $NAME($$$) { $$$ }",
				"function definitions (kind: function_definition)",
			),
			("class $NAME { $$$ }", "class specifiers"),
		],
		"ruby" => vec![
			("def $NAME($$$); $$$; end", "method definitions"),
			(
				"$X.each do |$P| $$$ end",
				"each-with-block (kind: do_block)",
			),
		],
		"php" => vec![
			("function $NAME($$$) { $$$ }", "function definitions"),
			("use $NS;", "namespace uses"),
		],
		"json" => vec![(
			"\"$KEY\": $VAL",
			"object pairs (use selector 'pair' when direct pattern fails)",
		)],
		_ => vec![],
	}
}

// ---------------------------------------------------------------------------
// Background services setup (preserves all watcher + indexing logic)
// ---------------------------------------------------------------------------

async fn start_background_services(
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
