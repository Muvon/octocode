// Copyright 2025 Muvon Un Limited
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

//! LSP protocol message types and utilities

use lsp_types::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

/// LSP request message wrapper
#[derive(Debug, Clone, Serialize)]
pub struct LspRequest {
	pub jsonrpc: String,
	pub id: u32,
	pub method: String,
	pub params: Value,
}

impl LspRequest {
	pub fn new(id: u32, method: String, params: Value) -> Self {
		Self {
			jsonrpc: "2.0".to_string(),
			id,
			method,
			params,
		}
	}

	pub fn initialize(id: u32, params: InitializeParams) -> Result<Self, serde_json::Error> {
		Ok(Self::new(
			id,
			"initialize".to_string(),
			serde_json::to_value(params)?,
		))
	}

	pub fn goto_definition(
		id: u32,
		params: GotoDefinitionParams,
	) -> Result<Self, serde_json::Error> {
		Ok(Self::new(
			id,
			"textDocument/definition".to_string(),
			serde_json::to_value(params)?,
		))
	}

	pub fn hover(id: u32, params: HoverParams) -> Result<Self, serde_json::Error> {
		Ok(Self::new(
			id,
			"textDocument/hover".to_string(),
			serde_json::to_value(params)?,
		))
	}

	pub fn find_references(id: u32, params: ReferenceParams) -> Result<Self, serde_json::Error> {
		Ok(Self::new(
			id,
			"textDocument/references".to_string(),
			serde_json::to_value(params)?,
		))
	}

	pub fn document_symbols(
		id: u32,
		params: DocumentSymbolParams,
	) -> Result<Self, serde_json::Error> {
		Ok(Self::new(
			id,
			"textDocument/documentSymbol".to_string(),
			serde_json::to_value(params)?,
		))
	}

	pub fn workspace_symbols(
		id: u32,
		params: WorkspaceSymbolParams,
	) -> Result<Self, serde_json::Error> {
		Ok(Self::new(
			id,
			"workspace/symbol".to_string(),
			serde_json::to_value(params)?,
		))
	}

	pub fn completion(id: u32, params: CompletionParams) -> Result<Self, serde_json::Error> {
		Ok(Self::new(
			id,
			"textDocument/completion".to_string(),
			serde_json::to_value(params)?,
		))
	}
}

/// LSP notification message wrapper
#[derive(Debug, Clone, Serialize)]
pub struct LspNotification {
	pub jsonrpc: String,
	pub method: String,
	pub params: Value,
}

impl LspNotification {
	pub fn new(method: String, params: Value) -> Self {
		Self {
			jsonrpc: "2.0".to_string(),
			method,
			params,
		}
	}

	pub fn initialized() -> Result<Self, serde_json::Error> {
		Ok(Self::new(
			"initialized".to_string(),
			serde_json::to_value(InitializedParams {})?,
		))
	}

	pub fn did_open(params: DidOpenTextDocumentParams) -> Result<Self, serde_json::Error> {
		Ok(Self::new(
			"textDocument/didOpen".to_string(),
			serde_json::to_value(params)?,
		))
	}

	pub fn did_change(params: DidChangeTextDocumentParams) -> Result<Self, serde_json::Error> {
		Ok(Self::new(
			"textDocument/didChange".to_string(),
			serde_json::to_value(params)?,
		))
	}

	pub fn did_close(params: DidCloseTextDocumentParams) -> Result<Self, serde_json::Error> {
		Ok(Self::new(
			"textDocument/didClose".to_string(),
			serde_json::to_value(params)?,
		))
	}
}

/// LSP response message wrapper
#[derive(Debug, Clone, Deserialize)]
pub struct LspResponse {
	pub jsonrpc: String,
	pub id: Option<u32>,
	pub result: Option<Value>,
	pub error: Option<LspError>,
}

/// LSP incoming notification wrapper
#[derive(Debug, Clone, Deserialize)]
pub struct LspIncomingNotification {
	pub jsonrpc: String,
	pub method: String,
	pub params: Option<Value>,
}

/// Generic LSP message that can be either response or notification
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum LspMessage {
	Response(LspResponse),
	Notification(LspIncomingNotification),
}

/// LSP error type
#[derive(Debug, Clone, Deserialize)]
pub struct LspError {
	pub code: i32,
	pub message: String,
	pub data: Option<Value>,
}

/// Utility functions for path handling
pub fn file_path_to_uri(path: &std::path::Path) -> anyhow::Result<Url> {
	Url::from_file_path(path)
		.map_err(|_| anyhow::anyhow!("Failed to convert path to URI: {}", path.display()))
}

pub fn uri_to_file_path(uri: &Uri) -> anyhow::Result<std::path::PathBuf> {
	// Convert LSP Uri to url::Url first
	let url_str = uri.to_string();
	let url = Url::parse(&url_str)?;
	url.to_file_path()
		.map_err(|_| anyhow::anyhow!("Failed to convert URI to file path: {}", url_str))
}

/// Convert relative path to absolute path based on working directory
pub fn resolve_relative_path(
	working_dir: &std::path::Path,
	relative_path: &str,
) -> std::path::PathBuf {
	if std::path::Path::new(relative_path).is_absolute() {
		std::path::PathBuf::from(relative_path)
	} else {
		working_dir.join(relative_path)
	}
}
