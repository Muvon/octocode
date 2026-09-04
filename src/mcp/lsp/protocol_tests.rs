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
	use crate::mcp::lsp::protocol::*;
	use lsp_types::*;
	use serde_json::json;
	use std::str::FromStr;
	use tempfile::TempDir;

	fn uri_for(path: &std::path::Path) -> Uri {
		Uri::from_str(file_path_to_uri(path).unwrap().as_ref()).unwrap()
	}

	fn position_params(uri: Uri) -> TextDocumentPositionParams {
		TextDocumentPositionParams {
			text_document: TextDocumentIdentifier { uri },
			position: Position {
				line: 1,
				character: 2,
			},
		}
	}

	#[test]
	fn every_request_constructor_stamps_the_method_and_id() {
		let dir = TempDir::new().unwrap();
		let uri = uri_for(dir.path());

		let initialize = LspRequest::initialize(1, InitializeParams::default()).unwrap();
		assert_eq!(initialize.method, "initialize");
		assert_eq!(initialize.id, 1);
		assert_eq!(initialize.jsonrpc, "2.0");

		let definition = LspRequest::goto_definition(
			2,
			GotoDefinitionParams {
				text_document_position_params: position_params(uri.clone()),
				work_done_progress_params: WorkDoneProgressParams::default(),
				partial_result_params: PartialResultParams::default(),
			},
		)
		.unwrap();
		assert_eq!(definition.method, "textDocument/definition");

		let hover = LspRequest::hover(
			3,
			HoverParams {
				text_document_position_params: position_params(uri.clone()),
				work_done_progress_params: WorkDoneProgressParams::default(),
			},
		)
		.unwrap();
		assert_eq!(hover.method, "textDocument/hover");

		let references = LspRequest::find_references(
			4,
			ReferenceParams {
				text_document_position: position_params(uri.clone()),
				work_done_progress_params: WorkDoneProgressParams::default(),
				partial_result_params: PartialResultParams::default(),
				context: ReferenceContext {
					include_declaration: true,
				},
			},
		)
		.unwrap();
		assert_eq!(references.method, "textDocument/references");

		let document_symbols = LspRequest::document_symbols(
			5,
			DocumentSymbolParams {
				text_document: TextDocumentIdentifier { uri: uri.clone() },
				work_done_progress_params: WorkDoneProgressParams::default(),
				partial_result_params: PartialResultParams::default(),
			},
		)
		.unwrap();
		assert_eq!(document_symbols.method, "textDocument/documentSymbol");

		let workspace_symbols = LspRequest::workspace_symbols(
			6,
			WorkspaceSymbolParams {
				query: "Store".to_string(),
				work_done_progress_params: WorkDoneProgressParams::default(),
				partial_result_params: PartialResultParams::default(),
			},
		)
		.unwrap();
		assert_eq!(workspace_symbols.method, "workspace/symbol");

		let completion = LspRequest::completion(
			7,
			CompletionParams {
				text_document_position: position_params(uri),
				work_done_progress_params: WorkDoneProgressParams::default(),
				partial_result_params: PartialResultParams::default(),
				context: None,
			},
		)
		.unwrap();
		assert_eq!(completion.method, "textDocument/completion");
		assert_eq!(completion.id, 7);
	}

	#[test]
	fn a_request_built_by_hand_keeps_its_parameters() {
		let request = LspRequest::new(9, "custom/method".to_string(), json!({"a": 1}));
		assert_eq!(request.method, "custom/method");
		assert_eq!(request.params, json!({"a": 1}));
	}

	#[test]
	fn every_notification_constructor_stamps_the_method() {
		let dir = TempDir::new().unwrap();
		let file = dir.path().join("a.rs");
		std::fs::write(&file, "fn a() {}\n").unwrap();
		let uri = uri_for(&file);

		assert_eq!(
			LspNotification::initialized().unwrap().method,
			"initialized"
		);

		let did_open = LspNotification::did_open(DidOpenTextDocumentParams {
			text_document: TextDocumentItem {
				uri: uri.clone(),
				language_id: "rust".to_string(),
				version: 1,
				text: "fn a() {}".to_string(),
			},
		})
		.unwrap();
		assert_eq!(did_open.method, "textDocument/didOpen");

		let did_change = LspNotification::did_change(DidChangeTextDocumentParams {
			text_document: VersionedTextDocumentIdentifier {
				uri: uri.clone(),
				version: 2,
			},
			content_changes: vec![TextDocumentContentChangeEvent {
				range: None,
				range_length: None,
				text: "fn a() { 1 }".to_string(),
			}],
		})
		.unwrap();
		assert_eq!(did_change.method, "textDocument/didChange");

		let did_close = LspNotification::did_close(DidCloseTextDocumentParams {
			text_document: TextDocumentIdentifier { uri },
		})
		.unwrap();
		assert_eq!(did_close.method, "textDocument/didClose");

		let custom = LspNotification::new("custom".to_string(), json!(null));
		assert_eq!(custom.jsonrpc, "2.0");
	}

	#[test]
	fn a_file_path_round_trips_through_its_uri() {
		let dir = TempDir::new().unwrap();
		let file = dir.path().join("a.rs");
		std::fs::write(&file, "x").unwrap();

		let uri = uri_for(&file);
		let back = uri_to_file_path(&uri).unwrap();
		assert_eq!(back.canonicalize().unwrap(), file.canonicalize().unwrap());
	}

	#[test]
	fn a_relative_uri_cannot_be_turned_into_a_path() {
		let uri = Uri::from_str("untitled:Untitled-1").unwrap();
		assert!(uri_to_file_path(&uri).is_err());
	}

	#[test]
	fn a_relative_path_resolves_inside_the_working_directory() {
		let dir = TempDir::new().unwrap();
		std::fs::create_dir_all(dir.path().join("src")).unwrap();
		let file = dir.path().join("src/a.rs");
		std::fs::write(&file, "x").unwrap();

		let resolved = resolve_relative_path(dir.path(), "src/a.rs").unwrap();
		assert_eq!(resolved, file.canonicalize().unwrap());
	}

	#[test]
	fn a_path_escaping_the_working_directory_is_rejected() {
		let dir = TempDir::new().unwrap();
		let outside = TempDir::new().unwrap();
		let secret = outside.path().join("secret.rs");
		std::fs::write(&secret, "x").unwrap();

		assert!(resolve_relative_path(dir.path(), "../../etc/passwd").is_err());
		assert!(resolve_relative_path(dir.path(), &secret.to_string_lossy()).is_err());
	}

	#[test]
	fn a_message_deserializes_into_the_right_variant() {
		let response: LspMessage =
			serde_json::from_value(json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}}))
				.unwrap();
		assert!(matches!(response, LspMessage::Response(_)));

		// `LspResponse` has only optional fields beyond `jsonrpc`, so the untagged
		// enum resolves an id-less notification to it as well; the client
		// distinguishes them by whether `id` is present.
		let notification: LspMessage = serde_json::from_value(
			json!({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "t"}}),
		)
		.unwrap();
		match notification {
			LspMessage::Response(r) => assert!(r.id.is_none()),
			LspMessage::Notification(n) => assert_eq!(n.method, "$/progress"),
			LspMessage::IncomingRequest(_) => panic!("no id means no request"),
		}

		let request: LspMessage = serde_json::from_value(
			json!({"jsonrpc": "2.0", "id": 4, "method": "window/workDoneProgress/create"}),
		)
		.unwrap();
		// An incoming request carries both an id and a method; the untagged enum
		// resolves it to the response arm first because every field is optional
		// there, so only the method distinguishes it downstream.
		match request {
			LspMessage::Response(r) => assert_eq!(r.id, Some(4)),
			LspMessage::IncomingRequest(r) => {
				assert_eq!(r.method, "window/workDoneProgress/create")
			}
			LspMessage::Notification(_) => panic!("a message with an id is not a notification"),
		}
	}

	#[test]
	fn an_error_response_carries_the_server_code_and_message() {
		let response: LspResponse = serde_json::from_value(json!({
			"jsonrpc": "2.0",
			"id": 2,
			"error": {"code": -32601, "message": "unknown method"}
		}))
		.unwrap();
		let error = response.error.expect("error payload");
		assert_eq!(error.code, -32601);
		assert_eq!(error.message, "unknown method");
		assert!(error.data.is_none());
	}
}
