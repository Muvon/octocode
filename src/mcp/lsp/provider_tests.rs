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
	use std::path::Path;
	use tempfile::TempDir;

	/// A minimal LSP server that speaks the Content-Length framing and answers
	/// every request this client makes. Using a stub instead of a real language
	/// server keeps the tests hermetic and fast.
	const STUB_SERVER: &str = r#"
import json, sys

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
        result = {"contents": {"kind": "markdown", "value": "```rust\nfn helper() -> u32\n```"}}
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
            ],
        }
    elif method == "shutdown":
        result = None
    else:
        write_message({"jsonrpc": "2.0", "id": request_id, "error": {"code": -32601, "message": "unknown method"}})
        continue
    write_message({"jsonrpc": "2.0", "id": request_id, "result": result})
"#;

	/// A workspace containing one Rust file plus the stub server script.
	fn workspace() -> (TempDir, String) {
		let dir = TempDir::new().unwrap();
		std::fs::create_dir_all(dir.path().join("src")).unwrap();
		std::fs::write(
			dir.path().join("src/lib.rs"),
			"pub fn helper() -> u32 {\n\t7\n}\n\npub fn caller() -> u32 {\n\thelper()\n}\n",
		)
		.unwrap();

		let script = dir.path().join("stub_lsp.py");
		std::fs::write(&script, STUB_SERVER).unwrap();
		let command = format!("python3 {}", script.display());
		(dir, command)
	}

	async fn ready_provider(dir: &Path, command: String) -> LspProvider {
		let mut provider = LspProvider::new(dir.to_path_buf(), command);
		provider
			.start_initialization()
			.await
			.expect("the stub server must complete the handshake");
		provider
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
	async fn updating_or_closing_an_unknown_file_is_an_error_or_no_op() {
		let (dir, command) = workspace();
		let provider = ready_provider(dir.path(), command).await;
		let _ = provider.update_file("src/nope.rs").await;
		let _ = provider.close_file("src/nope.rs").await;
	}
}
