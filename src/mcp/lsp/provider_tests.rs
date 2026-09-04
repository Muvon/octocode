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
	use crate::mcp::lsp::provider::LspProvider;
	use serde_json::json;
	use std::path::Path;
	use std::time::Duration;
	use tempfile::TempDir;

	/// A stub handshake takes milliseconds; anything longer is a hang, and a
	/// failed test beats a stuck suite.
	const HANDSHAKE_LIMIT: Duration = Duration::from_secs(20);

	/// A minimal LSP server that speaks the Content-Length framing and answers
	/// every request this client makes. Using a stub instead of a real language
	/// server keeps the tests hermetic and fast.
	/// `sys.argv[1]` selects how the stub answers everything after the
	/// handshake: "normal", "error" (JSON-RPC error), "null" (empty result) or
	/// "hangup" (exit without answering).
	const STUB_SERVER: &str = r#"
import json, sys

MODE = sys.argv[1] if len(sys.argv) > 1 else "normal"

def read_message(stream):
    length = None
    while True:
        line = stream.readline()
        if not line:
            return None
        line = line.decode("utf-8")
        if line in ("\r\n", "\n"):
            break
        if line.lower().startswith("content-length:"):
            length = int(line.split(":", 1)[1].strip())
    if length is None:
        return None
    return json.loads(stream.read(length).decode("utf-8"))

def write_message(payload):
    body = json.dumps(payload).encode("utf-8")
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()

DOC_URI = None

def location(uri, line, character):
    return {
        "uri": uri,
        "range": {
            "start": {"line": line, "character": character},
            "end": {"line": line, "character": character + 4},
        },
    }

while True:
    message = read_message(sys.stdin.buffer)
    if message is None:
        break
    method = message.get("method")
    if method == "textDocument/didOpen":
        DOC_URI = message["params"]["textDocument"]["uri"]
        continue
    if "id" not in message:
        if method == "exit":
            break
        continue

    request_id = message["id"]
    uri = DOC_URI or "file:///stub.rs"
    if method == "initialize" and MODE in ("initerror", "initgarbage"):
        if MODE == "initerror":
            write_message({"jsonrpc": "2.0", "id": request_id, "error": {"code": -32603, "message": "server exploded"}})
        else:
            write_message({"jsonrpc": "2.0", "id": request_id, "result": {"capabilities": "not-an-object"}})
        continue
    if method != "initialize" and MODE != "normal":
        if MODE == "hangup":
            break
        if MODE == "error":
            write_message({"jsonrpc": "2.0", "id": request_id, "error": {"code": -32603, "message": "server exploded"}})
            continue
        if MODE == "null":
            write_message({"jsonrpc": "2.0", "id": request_id, "result": None})
            continue
    position = (message.get("params") or {}).get("position") or {"line": -1, "character": -1}
    if method == "initialize":
        result = {
            "capabilities": {
                "textDocumentSync": 1,
                "definitionProvider": True,
                "hoverProvider": True,
                "referencesProvider": True,
                "documentSymbolProvider": True,
                "workspaceSymbolProvider": True,
                "completionProvider": {"triggerCharacters": ["."]},
            },
            "serverInfo": {"name": "stub-lsp", "version": "1"},
        }
    elif method == "textDocument/definition":
        result = location(uri, 0, 7)
    elif method == "textDocument/hover":
        # Echo the requested position so tests can observe the character the
        # provider resolved from a symbol name.
        result = {"contents": {"kind": "markdown", "value": "```rust\nfn helper() -> u32\n```\nposition %d:%d" % (position["line"], position["character"])}}
    elif method == "textDocument/references":
        result = [location(uri, 0, 7), location(uri, 4, 1)]
    elif method == "textDocument/documentSymbol":
        result = [
            {
                "name": "helper",
                "kind": 12,
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 1}},
                "selectionRange": {"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 13}},
            }
        ]
    elif method == "workspace/symbol":
        result = [{"name": "helper", "kind": 12, "location": location(uri, 0, 7)}]
    elif method == "textDocument/completion":
        result = {
            "isIncomplete": False,
            "items": [
                {"label": "helper", "kind": 3, "detail": "fn() -> u32"},
                {"label": "helper_two", "kind": 3},
                {"label": "pos_%d_%d" % (position["line"], position["character"]), "kind": 3},
            ],
        }
    elif method == "shutdown":
        result = None
    else:
        write_message({"jsonrpc": "2.0", "id": request_id, "error": {"code": -32601, "message": "unknown method"}})
        continue
    write_message({"jsonrpc": "2.0", "id": request_id, "result": result})
"#;

	/// The interpreter that runs the stub servers below. GitHub's Windows
	/// runners expose it as `python`; `python3` there is usually an App Execute
	/// alias that is not a real interpreter.
	const PYTHON: &str = if cfg!(windows) { "python" } else { "python3" };

	/// A workspace containing one Rust file plus the stub server script.
	fn workspace() -> (TempDir, String) {
		workspace_mode("normal")
	}

	fn workspace_mode(mode: &str) -> (TempDir, String) {
		let dir = TempDir::new().unwrap();
		std::fs::create_dir_all(dir.path().join("src")).unwrap();
		std::fs::write(
			dir.path().join("src/lib.rs"),
			"pub fn helper() -> u32 {\n\t7\n}\n\npub fn caller() -> u32 {\n\thelper()\n}\n",
		)
		.unwrap();

		let script = dir.path().join("stub_lsp.py");
		std::fs::write(&script, STUB_SERVER).unwrap();
		let command = format!("{PYTHON} {} {}", script.display(), mode);
		(dir, command)
	}

	async fn ready_provider(dir: &Path, command: String) -> LspProvider {
		let mut provider = LspProvider::new(dir.to_path_buf(), command);
		tokio::time::timeout(HANDSHAKE_LIMIT, provider.start_initialization())
			.await
			.expect("the handshake must not hang")
			.expect("the stub server must complete the handshake");
		provider
	}

	/// A ready provider talking to a stub in the given answer mode.
	async fn provider_in_mode(mode: &str) -> (TempDir, LspProvider) {
		let (dir, command) = workspace_mode(mode);
		let provider = ready_provider(dir.path(), command).await;
		(dir, provider)
	}

	#[tokio::test]
	async fn an_unstarted_provider_is_not_ready() {
		let (dir, command) = workspace();
		let provider = LspProvider::new(dir.path().to_path_buf(), command);
		assert!(!provider.is_initialized());
		assert!(!provider.is_ready());
		assert!(provider.capabilities().is_none());

		// Every tool refuses to run until the handshake completes.
		assert!(provider.hover("src/lib.rs", 1, 8).await.is_err());
		assert!(provider.goto_definition("src/lib.rs", 6, 2).await.is_err());
		assert!(provider
			.find_references("src/lib.rs", 1, 8, true)
			.await
			.is_err());
		assert!(provider.document_symbols("src/lib.rs").await.is_err());
		assert!(provider.workspace_symbols("helper").await.is_err());
		assert!(provider.completion("src/lib.rs", 6, 2).await.is_err());
	}

	#[tokio::test]
	async fn starting_a_missing_server_binary_reports_an_error() {
		let dir = TempDir::new().unwrap();
		let mut provider = LspProvider::new(
			dir.path().to_path_buf(),
			"octocode-no-such-language-server".to_string(),
		);
		assert!(provider.start_initialization().await.is_err());
		assert!(!provider.is_ready());
	}

	#[tokio::test]
	async fn an_empty_command_is_rejected() {
		let dir = TempDir::new().unwrap();
		let mut provider = LspProvider::new(dir.path().to_path_buf(), String::new());
		assert!(provider.start_initialization().await.is_err());
	}

	#[tokio::test]
	async fn the_handshake_records_the_server_capabilities() {
		let (dir, command) = workspace();
		let mut provider = ready_provider(dir.path(), command).await;

		assert!(provider.is_initialized());
		assert!(provider.is_ready());
		let capabilities = provider.capabilities().expect("capabilities");
		assert!(capabilities.hover_provider.is_some());
		assert!(capabilities.definition_provider.is_some());

		// Re-initializing is a no-op rather than a second handshake.
		provider.start_initialization().await.unwrap();
		assert!(provider.is_ready());
	}

	#[tokio::test]
	async fn hover_returns_the_cleaned_up_server_markdown() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;
		let out = provider.hover("src/lib.rs", 1, 8).await.unwrap();
		assert!(out.contains("fn helper() -> u32"), "{out}");
		// Markdown fences are stripped for readability.
		assert!(!out.contains("```"), "{out}");
	}

	#[tokio::test]
	async fn goto_definition_reports_a_one_based_position() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;
		let out = provider.goto_definition("src/lib.rs", 6, 2).await.unwrap();
		assert!(out.starts_with("Definition found at"), "{out}");
		// The stub answers with 0-based line 0, character 7.
		assert!(out.ends_with(":1:8"), "{out}");
	}

	#[tokio::test]
	async fn find_references_lists_every_hit() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;
		let out = provider
			.find_references("src/lib.rs", 1, 8, true)
			.await
			.unwrap();
		assert!(out.contains("2"), "{out}");
		assert!(out.contains("lib.rs"), "{out}");
	}

	#[tokio::test]
	async fn document_and_workspace_symbols_are_rendered() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;

		let document = provider.document_symbols("src/lib.rs").await.unwrap();
		assert!(document.contains("helper"), "{document}");

		let workspace = provider.workspace_symbols("helper").await.unwrap();
		assert!(workspace.contains("helper"), "{workspace}");
	}

	#[tokio::test]
	async fn completions_are_listed_with_their_detail() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;
		// `completion` does not open the document itself.
		provider.ensure_file_opened("src/lib.rs").await.unwrap();
		let out = provider.completion("src/lib.rs", 6, 2).await.unwrap();
		assert!(out.contains("helper"), "{out}");
		assert!(out.contains("helper_two"), "{out}");
	}

	#[tokio::test]
	async fn a_request_for_a_missing_file_is_an_error() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;
		assert!(provider.hover("src/nope.rs", 1, 1).await.is_err());
	}

	#[tokio::test]
	async fn opening_updating_and_closing_a_document_tracks_its_state() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;

		provider.ensure_file_opened("src/lib.rs").await.unwrap();
		// A second open is a no-op.
		provider.ensure_file_opened("src/lib.rs").await.unwrap();

		std::fs::write(
			dir.path().join("src/lib.rs"),
			"pub fn helper() -> u32 {\n\t8\n}\n",
		)
		.unwrap();
		provider.update_file("src/lib.rs").await.unwrap();
		// Updating with unchanged content must not fail either.
		provider.update_file("src/lib.rs").await.unwrap();

		provider.close_file("src/lib.rs").await.unwrap();
		// Closing twice is harmless.
		provider.close_file("src/lib.rs").await.unwrap();
	}

	#[tokio::test]
	async fn updating_a_missing_file_fails_while_closing_it_is_a_no_op() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;
		// An unopened path is opened on demand, so a path that does not exist
		// fails when its contents are read.
		assert!(provider.update_file("src/nope.rs").await.is_err());
		// Closing something that was never opened has nothing to undo.
		assert!(provider.close_file("src/nope.rs").await.is_ok());
	}

	// --- Handshake failures ---

	#[tokio::test]
	async fn a_server_that_rejects_initialize_leaves_the_provider_unusable() {
		let (dir, command) = workspace_mode("initerror");
		let mut provider = LspProvider::new(dir.path().to_path_buf(), command);
		let error = tokio::time::timeout(HANDSHAKE_LIMIT, provider.start_initialization())
			.await
			.expect("a rejected handshake must not hang")
			.expect_err("the server refused to initialize");
		assert!(error.to_string().contains("server exploded"), "{error}");
		assert!(!provider.is_initialized());
		assert!(!provider.is_ready());
	}

	#[tokio::test]
	async fn an_unparseable_initialize_result_fails_the_handshake() {
		let (dir, command) = workspace_mode("initgarbage");
		let mut provider = LspProvider::new(dir.path().to_path_buf(), command);
		let error = tokio::time::timeout(HANDSHAKE_LIMIT, provider.start_initialization())
			.await
			.expect("a malformed handshake must not hang")
			.expect_err("the capabilities payload is not an object");
		assert!(
			error.to_string().contains("Failed to parse initialize"),
			"{error}"
		);
		assert!(provider.capabilities().is_none());
	}

	// --- MCP tool entry points ---

	#[tokio::test]
	async fn every_tool_entry_point_refuses_to_run_before_the_handshake() {
		let (dir, command) = workspace();
		let mut provider = LspProvider::new(dir.path().to_path_buf(), command);
		let args = json!({
			"file_path": "src/lib.rs",
			"line": 1,
			"symbol": "helper",
			"query": "helper"
		});

		let mut errors = Vec::new();
		errors.push(provider.execute_goto_definition(&args).await.unwrap_err());
		errors.push(provider.execute_hover(&args).await.unwrap_err());
		errors.push(provider.execute_find_references(&args).await.unwrap_err());
		errors.push(provider.execute_document_symbols(&args).await.unwrap_err());
		errors.push(provider.execute_workspace_symbols(&args).await.unwrap_err());
		errors.push(provider.execute_completion(&args).await.unwrap_err());

		for error in errors {
			assert_eq!(error.code, -32601, "{error}");
			assert!(error.message.contains("not initialized"), "{error}");
		}
	}

	#[tokio::test]
	async fn each_tool_names_the_parameter_it_is_missing() {
		let (dir, command) = workspace();
		let mut provider = ready_provider(dir.path(), command).await;

		let no_path = provider
			.execute_hover(&json!({"line": 1, "symbol": "helper"}))
			.await
			.unwrap_err();
		assert_eq!(no_path.code, -32602);
		assert!(no_path.message.contains("file_path"), "{no_path}");

		let no_line = provider
			.execute_hover(&json!({"file_path": "src/lib.rs", "symbol": "helper"}))
			.await
			.unwrap_err();
		assert!(no_line.message.contains("line"), "{no_line}");

		let no_symbol = provider
			.execute_hover(&json!({"file_path": "src/lib.rs", "line": 1}))
			.await
			.unwrap_err();
		assert!(no_symbol.message.contains("symbol"), "{no_symbol}");

		let no_definition_path = provider
			.execute_goto_definition(&json!({"line": 1, "symbol": "helper"}))
			.await
			.unwrap_err();
		assert_eq!(no_definition_path.code, -32602);
		assert!(
			no_definition_path.message.contains("file_path"),
			"{no_definition_path}"
		);

		let no_references_line = provider
			.execute_find_references(&json!({"file_path": "src/lib.rs"}))
			.await
			.unwrap_err();
		assert!(
			no_references_line.message.contains("line"),
			"{no_references_line}"
		);

		let no_symbols_path = provider
			.execute_document_symbols(&json!({}))
			.await
			.unwrap_err();
		assert!(
			no_symbols_path.message.contains("file_path"),
			"{no_symbols_path}"
		);

		let no_query = provider
			.execute_workspace_symbols(&json!({}))
			.await
			.unwrap_err();
		assert!(no_query.message.contains("query"), "{no_query}");

		let no_completion_symbol = provider
			.execute_completion(&json!({"file_path": "src/lib.rs", "line": 1}))
			.await
			.unwrap_err();
		assert!(
			no_completion_symbol.message.contains("symbol"),
			"{no_completion_symbol}"
		);
	}

	#[tokio::test]
	async fn the_tool_entry_points_render_the_server_answers() {
		let (dir, command) = workspace();
		// The server answers with absolute URIs that are rendered relative to
		// the workspace root, so the root must already be canonical for the
		// comparison to be exact on platforms where /tmp is a symlink.
		let root = dir.path().canonicalize().unwrap();
		let mut provider = ready_provider(&root, command).await;

		let definition = provider
			.execute_goto_definition(&json!({
				"file_path": "src/lib.rs", "line": 6, "symbol": "helper"
			}))
			.await
			.unwrap();
		assert_eq!(definition, "Definition found at src/lib.rs:1:8");

		let references = provider
			.execute_find_references(&json!({
				"file_path": "src/lib.rs", "line": 1, "symbol": "helper"
			}))
			.await
			.unwrap();
		assert_eq!(
			references,
			"Found 2 reference(s):\n1. src/lib.rs:1:8\n2. src/lib.rs:5:2"
		);

		let symbols = provider
			.execute_document_symbols(&json!({"file_path": "src/lib.rs"}))
			.await
			.unwrap();
		assert!(symbols.starts_with("Found 1 symbol(s):"), "{symbols}");
		assert!(symbols.contains("1. helper ("), "{symbols}");
		assert!(symbols.ends_with(") at 1:1"), "{symbols}");

		let workspace_symbols = provider
			.execute_workspace_symbols(&json!({"query": "helper"}))
			.await
			.unwrap();
		assert!(
			workspace_symbols.starts_with("Found 1 symbol(s) in workspace:"),
			"{workspace_symbols}"
		);
		assert!(
			workspace_symbols.ends_with("in src/lib.rs:1"),
			"{workspace_symbols}"
		);
	}

	#[tokio::test]
	async fn a_bracketed_file_path_is_unwrapped_before_the_file_is_opened() {
		let (dir, command) = workspace();
		let mut provider = ready_provider(dir.path(), command).await;
		let out = provider
			.execute_document_symbols(&json!({"file_path": "[Rust file: src/lib.rs]"}))
			.await
			.unwrap();
		assert!(out.contains("helper"), "{out}");
		assert!(
			provider
				.opened_documents
				.lock()
				.unwrap()
				.contains("src/lib.rs"),
			"the cleaned path is what gets opened"
		);
	}

	// --- Symbol position resolution ---

	#[tokio::test]
	async fn a_symbol_is_located_by_word_boundary_then_by_looser_strategies() {
		let (dir, command) = workspace();
		let mut provider = ready_provider(dir.path(), command).await;

		// Line 1 is `pub fn helper() -> u32 {`, so `helper` sits at 0-based
		// column 7 and the stub echoes back the position it was asked about.
		for (symbol, expected) in [
			("helper", "position 0:7"),
			// A substring with no word boundary on either side.
			("elpe", "position 0:8"),
			// Case-insensitive match.
			("HELPER", "position 0:7"),
			// A qualified name falls back to its last segment.
			("crate::helper", "position 0:7"),
			// Nothing matches: the first identifier on the line is used.
			("zzz", "position 0:0"),
		] {
			let out = provider
				.execute_hover(&json!({
					"file_path": "src/lib.rs", "line": 1, "symbol": symbol
				}))
				.await
				.unwrap_or_else(|e| panic!("hover for {symbol} failed: {e}"));
			assert!(out.contains(expected), "symbol {symbol}: {out}");
		}
	}

	#[tokio::test]
	async fn a_line_without_any_identifier_has_no_fallback_position() {
		let (dir, command) = workspace();
		let mut provider = ready_provider(dir.path(), command).await;
		// Line 2 is a tab followed by `7` — no identifier to fall back to.
		let error = provider
			.execute_hover(&json!({"file_path": "src/lib.rs", "line": 2, "symbol": "zzz"}))
			.await
			.unwrap_err();
		assert!(
			error.message.contains("no fallback position"),
			"{}",
			error.message
		);
	}

	#[tokio::test]
	async fn a_line_outside_the_document_is_rejected() {
		let (dir, command) = workspace();
		let mut provider = ready_provider(dir.path(), command).await;
		for line in [0, 99] {
			let error = provider
				.execute_hover(&json!({
					"file_path": "src/lib.rs", "line": line, "symbol": "helper"
				}))
				.await
				.unwrap_err();
			assert!(
				error.message.contains("out of bounds"),
				"line {line}: {}",
				error.message
			);
		}
	}

	#[tokio::test]
	async fn completion_is_requested_at_the_end_of_the_symbol() {
		let (dir, command) = workspace();
		let mut provider = ready_provider(dir.path(), command).await;
		let out = provider
			.execute_completion(&json!({
				"file_path": "src/lib.rs", "line": 1, "symbol": "helper"
			}))
			.await
			.unwrap();
		// `helper` starts at 1-based column 8, so completion asks at 8 + 6 = 14,
		// which the server sees as 0-based column 13.
		assert!(out.contains("pos_0_13"), "{out}");
	}

	// --- Position plumbing ---

	#[tokio::test]
	async fn positions_are_validated_against_the_open_document() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;
		provider.ensure_file_opened("src/lib.rs").await.unwrap();

		let position = provider.text_document_position("src/lib.rs", 1, 8).unwrap();
		assert_eq!(position.position.line, 0);
		assert_eq!(position.position.character, 7);

		// Column 0 clamps instead of underflowing.
		let clamped = provider.text_document_position("src/lib.rs", 1, 0).unwrap();
		assert_eq!(clamped.position.character, 0);

		// Line 1 has 24 UTF-16 units, so column 25 is the insertion point past
		// its end and 26 is out of bounds.
		assert!(provider.text_document_position("src/lib.rs", 1, 25).is_ok());
		let past_column = provider
			.text_document_position("src/lib.rs", 1, 26)
			.unwrap_err();
		assert!(
			past_column.to_string().contains("Character 26"),
			"{past_column}"
		);

		assert!(provider.text_document_position("src/lib.rs", 0, 1).is_err());
		assert!(provider
			.text_document_position("src/lib.rs", 99, 1)
			.is_err());

		let unopened = provider
			.text_document_position("src/caller.rs", 1, 1)
			.unwrap_err();
		assert!(unopened.to_string().contains("not opened"), "{unopened}");
	}

	#[tokio::test]
	async fn a_file_uri_resolves_the_same_way_from_a_relative_and_an_absolute_path() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;

		let relative = provider.resolve_file_uri("src/lib.rs").unwrap().to_string();
		assert!(relative.starts_with("file://"), "{relative}");
		assert!(relative.ends_with("/src/lib.rs"), "{relative}");

		let absolute = provider
			.resolve_file_uri(&dir.path().join("src/lib.rs").to_string_lossy())
			.unwrap()
			.to_string();
		assert_eq!(relative, absolute);

		let identifier = provider.text_document_identifier("src/lib.rs").unwrap();
		assert_eq!(identifier.uri.to_string(), relative);
	}

	#[tokio::test]
	async fn a_path_escaping_the_workspace_is_refused() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;

		let outside = TempDir::new().unwrap();
		let secret = outside.path().join("secret.rs");
		std::fs::write(&secret, "fn secret() {}\n").unwrap();

		let error = provider
			.ensure_file_opened(&secret.to_string_lossy())
			.await
			.unwrap_err();
		assert!(
			error.to_string().contains("outside the working directory"),
			"{error}"
		);
	}

	// --- Document synchronisation bookkeeping ---

	#[tokio::test]
	async fn document_versions_only_advance_when_the_content_really_changed() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;
		let file = dir.path().join("src/lib.rs");
		let version = |provider: &LspProvider| {
			*provider
				.document_versions
				.lock()
				.unwrap()
				.get("src/lib.rs")
				.expect("the document is tracked")
		};

		// An unopened path is opened on demand and starts at version 1.
		provider.update_file("src/lib.rs").await.unwrap();
		assert_eq!(version(&provider), 1);

		std::fs::write(&file, "pub fn helper() -> u32 {\n\t8\n}\n").unwrap();
		provider.update_file("src/lib.rs").await.unwrap();
		assert_eq!(version(&provider), 2);

		// Re-reading identical content must not push another didChange.
		provider.update_file("src/lib.rs").await.unwrap();
		assert_eq!(version(&provider), 2);
	}

	#[tokio::test]
	async fn reopening_a_file_edited_on_disk_pushes_the_new_content() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;
		let file = dir.path().join("src/lib.rs");

		provider.ensure_file_opened("src/lib.rs").await.unwrap();
		assert_eq!(
			*provider
				.document_versions
				.lock()
				.unwrap()
				.get("src/lib.rs")
				.unwrap(),
			1
		);

		std::fs::write(&file, "pub fn helper() -> u32 {\n\t9\n}\n").unwrap();
		provider.ensure_file_opened("src/lib.rs").await.unwrap();
		assert_eq!(
			*provider
				.document_versions
				.lock()
				.unwrap()
				.get("src/lib.rs")
				.unwrap(),
			2
		);
		assert!(provider
			.document_contents
			.lock()
			.unwrap()
			.get("src/lib.rs")
			.unwrap()
			.contains('9'));

		// An unchanged file is not resent.
		provider.ensure_file_opened("src/lib.rs").await.unwrap();
		assert_eq!(
			*provider
				.document_versions
				.lock()
				.unwrap()
				.get("src/lib.rs")
				.unwrap(),
			2
		);
	}

	#[tokio::test]
	async fn closing_a_file_forgets_everything_tracked_about_it() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;

		provider.ensure_file_opened("src/lib.rs").await.unwrap();
		provider.close_file("src/lib.rs").await.unwrap();

		assert!(provider.opened_documents.lock().unwrap().is_empty());
		assert!(provider.document_versions.lock().unwrap().is_empty());
		assert!(provider.document_contents.lock().unwrap().is_empty());
	}

	#[tokio::test]
	async fn document_traffic_is_skipped_until_the_server_is_initialized() {
		let (dir, command) = workspace();
		let provider = LspProvider::new(dir.path().to_path_buf(), command);

		// The file watcher can fire before the handshake finishes; both calls
		// must no-op rather than fail or spawn a server.
		provider.update_file("src/lib.rs").await.unwrap();
		provider.close_file("src/lib.rs").await.unwrap();
		assert!(provider.opened_documents.lock().unwrap().is_empty());
	}

	// --- Server-side failures ---

	#[tokio::test]
	async fn a_server_error_response_is_surfaced_with_its_code_and_method() {
		let (_dir, provider) = provider_in_mode("error").await;
		let error = provider.hover("src/lib.rs", 1, 8).await.unwrap_err();
		let text = error.to_string();
		assert!(text.contains("LSP error -32603"), "{text}");
		assert!(text.contains("server exploded"), "{text}");
		assert!(text.contains("textDocument/hover"), "{text}");
	}

	#[tokio::test]
	async fn an_empty_server_result_reads_as_nothing_found_for_every_tool() {
		let (_dir, provider) = provider_in_mode("null").await;
		provider.ensure_file_opened("src/lib.rs").await.unwrap();

		assert_eq!(
			provider.goto_definition("src/lib.rs", 1, 8).await.unwrap(),
			"No definition found"
		);
		assert_eq!(
			provider.hover("src/lib.rs", 1, 8).await.unwrap(),
			"No hover information available"
		);
		assert_eq!(
			provider
				.find_references("src/lib.rs", 1, 8, false)
				.await
				.unwrap(),
			"No references found"
		);
		assert_eq!(
			provider.document_symbols("src/lib.rs").await.unwrap(),
			"No symbols found in document"
		);
		assert_eq!(
			provider.workspace_symbols("helper").await.unwrap(),
			"No symbols found in workspace"
		);
		assert_eq!(
			provider.completion("src/lib.rs", 1, 8).await.unwrap(),
			"No completions available"
		);
	}

	#[tokio::test]
	async fn a_server_that_hangs_up_fails_the_request_instead_of_waiting_it_out() {
		let (_dir, provider) = provider_in_mode("hangup").await;

		// The request timeout is 30s; a dropped connection must be noticed by
		// the pending-request channel closing, not by that timer.
		let error = tokio::time::timeout(HANDSHAKE_LIMIT, provider.hover("src/lib.rs", 1, 8))
			.await
			.expect("a dropped connection must fail fast")
			.expect_err("the server exited without answering");
		assert!(error.to_string().contains("channel closed"), "{error}");

		tokio::time::timeout(HANDSHAKE_LIMIT, async {
			while provider.is_ready() {
				tokio::task::yield_now().await;
			}
		})
		.await
		.expect("the client should observe the closed connection");

		// Still initialized, but no longer usable.
		assert!(provider.is_initialized());
		assert!(provider.hover("src/lib.rs", 1, 8).await.is_err());
	}
}
