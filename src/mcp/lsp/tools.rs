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

//! LSP MCP tools implementation

use anyhow::Result;
use lsp_types::*;
use serde_json::{json, Value};
use tracing::{debug, warn};

use super::protocol::{uri_to_file_path, LspRequest};
use super::provider::LspProvider;

impl LspProvider {
	/// LSP goto definition tool
	pub async fn goto_definition(
		&self,
		file_path: &str,
		line: u32,
		character: u32,
	) -> Result<Value> {
		if !self.is_ready() {
			return Err(Self::lsp_not_ready_error());
		}

		debug!("LSP goto definition: {}:{}:{}", file_path, line, character);

		// Ensure file is opened in LSP server before making position-based requests
		self.ensure_file_opened(file_path).await?;

		// Add a small delay to ensure the file is fully processed by rust-analyzer
		tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

		let params = GotoDefinitionParams {
			text_document_position_params: self
				.text_document_position(file_path, line, character)?,
			work_done_progress_params: WorkDoneProgressParams::default(),
			partial_result_params: PartialResultParams::default(),
		};

		let request = LspRequest::goto_definition(self.next_request_id(), params)?;
		let response = self.client.send_request(request).await?;
		debug!("Goto definition response: {:?}", response);

		if let Some(result) = response.result {
			// Handle different response types (Location, Vec<Location>, LocationLink, etc.)
			let locations = self.parse_goto_definition_response(result)?;

			if let Some(location) = locations.first() {
				let file_path = uri_to_file_path(&location.uri)?;
				let relative_path = self.make_path_relative(&file_path);

				Ok(json!({
					"file_path": relative_path,
					"line": location.range.start.line + 1,
					"character": location.range.start.character + 1,
					"end_line": location.range.end.line + 1,
					"end_character": location.range.end.character + 1
				}))
			} else {
				Err(anyhow::anyhow!("No definition found"))
			}
		} else {
			Err(anyhow::anyhow!("No definition found"))
		}
	}

	/// LSP hover tool
	pub async fn hover(&self, file_path: &str, line: u32, character: u32) -> Result<Value> {
		if !self.is_ready() {
			return Err(Self::lsp_not_ready_error());
		}

		debug!("LSP hover: {}:{}:{}", file_path, line, character);

		// Ensure file is opened in LSP server before making position-based requests
		self.ensure_file_opened(file_path).await?;

		// Add a small delay to ensure the file is fully processed by rust-analyzer
		tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

		let params = HoverParams {
			text_document_position_params: match self
				.text_document_position(file_path, line, character)
			{
				Ok(pos) => {
					debug!(
						"Created text document position: uri={:?}, line={}, character={}",
						pos.text_document.uri, pos.position.line, pos.position.character
					);
					pos
				}
				Err(e) => {
					warn!(
						"Failed to create text document position for {}:{}:{}: {}",
						file_path, line, character, e
					);
					return Err(e);
				}
			},
			work_done_progress_params: WorkDoneProgressParams::default(),
		};

		let request = match LspRequest::hover(self.next_request_id(), params) {
			Ok(req) => {
				debug!("Created hover request successfully");
				req
			}
			Err(e) => {
				warn!("Failed to create hover request: {}", e);
				return Err(anyhow::anyhow!("Failed to create hover request: {}", e));
			}
		};
		debug!(
			"Sending hover request: {}",
			serde_json::to_string(&request)
				.unwrap_or_else(|_| "<serialization failed>".to_string())
		);
		let response = self.client.send_request(request).await?;

		if let Some(result) = response.result {
			let hover: Option<Hover> = serde_json::from_value(result)?;

			if let Some(hover) = hover {
				let contents = self.extract_hover_contents(&hover.contents);

				let mut result = json!({
					"contents": contents
				});

				if let Some(range) = hover.range {
					result["range"] = json!({
						"start": {
							"line": range.start.line + 1,
							"character": range.start.character + 1
						},
						"end": {
							"line": range.end.line + 1,
							"character": range.end.character + 1
						}
					});
				}

				Ok(result)
			} else {
				Err(anyhow::anyhow!("No hover information available"))
			}
		} else {
			Err(anyhow::anyhow!("No hover information available"))
		}
	}

	/// LSP find references tool
	pub async fn find_references(
		&self,
		file_path: &str,
		line: u32,
		character: u32,
		include_declaration: bool,
	) -> Result<Value> {
		if !self.is_ready() {
			return Err(Self::lsp_not_ready_error());
		}

		debug!(
			"LSP find references: {}:{}:{} (include_declaration: {})",
			file_path, line, character, include_declaration
		);

		// Ensure file is opened in LSP server before making position-based requests
		self.ensure_file_opened(file_path).await?;

		// Add a small delay to ensure the file is fully processed by rust-analyzer
		tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

		let params = ReferenceParams {
			text_document_position: self.text_document_position(file_path, line, character)?,
			work_done_progress_params: WorkDoneProgressParams::default(),
			partial_result_params: PartialResultParams::default(),
			context: ReferenceContext {
				include_declaration,
			},
		};

		let request = LspRequest::find_references(self.next_request_id(), params)?;
		let response = self.client.send_request(request).await?;

		if let Some(result) = response.result {
			let locations: Option<Vec<Location>> = serde_json::from_value(result)?;

			if let Some(locations) = locations {
				let mut references = Vec::new();

				for location in locations {
					let file_path = uri_to_file_path(&location.uri)?;
					let relative_path = self.make_path_relative(&file_path);

					references.push(json!({
						"file_path": relative_path,
						"line": location.range.start.line + 1,
						"character": location.range.start.character + 1,
						"end_line": location.range.end.line + 1,
						"end_character": location.range.end.character + 1
					}));
				}

				Ok(json!({
					"references": references,
					"count": references.len()
				}))
			} else {
				Ok(json!({
					"references": [],
					"count": 0
				}))
			}
		} else {
			Ok(json!({
				"references": [],
				"count": 0
			}))
		}
	}

	/// LSP document symbols tool
	pub async fn document_symbols(&self, file_path: &str) -> Result<Value> {
		if !self.is_ready() {
			return Err(Self::lsp_not_ready_error());
		}

		debug!("LSP document symbols: {}", file_path);

		let params = DocumentSymbolParams {
			text_document: self.text_document_identifier(file_path)?,
			work_done_progress_params: WorkDoneProgressParams::default(),
			partial_result_params: PartialResultParams::default(),
		};

		let request = LspRequest::document_symbols(self.next_request_id(), params)?;
		let response = self.client.send_request(request).await?;

		if let Some(result) = response.result {
			let symbols = self.parse_document_symbols_response(result)?;
			Ok(json!({
				"symbols": symbols,
				"count": symbols.len()
			}))
		} else {
			Ok(json!({
				"symbols": [],
				"count": 0
			}))
		}
	}

	/// LSP workspace symbols tool
	pub async fn workspace_symbols(&self, query: &str) -> Result<Value> {
		if !self.is_ready() {
			return Err(Self::lsp_not_ready_error());
		}

		debug!("LSP workspace symbols: {}", query);

		let params = WorkspaceSymbolParams {
			query: query.to_string(),
			work_done_progress_params: WorkDoneProgressParams::default(),
			partial_result_params: PartialResultParams::default(),
		};

		let request = LspRequest::workspace_symbols(self.next_request_id(), params)?;
		let response = self.client.send_request(request).await?;

		if let Some(result) = response.result {
			let symbols: Option<Vec<SymbolInformation>> = serde_json::from_value(result)?;

			if let Some(symbols) = symbols {
				let mut workspace_symbols = Vec::new();

				for symbol in symbols {
					let file_path = uri_to_file_path(&symbol.location.uri)?;
					let relative_path = self.make_path_relative(&file_path);

					workspace_symbols.push(json!({
						"name": symbol.name,
						"kind": format!("{:?}", symbol.kind),
						"file_path": relative_path,
						"line": symbol.location.range.start.line + 1,
						"character": symbol.location.range.start.character + 1,
						"container_name": symbol.container_name
					}));
				}

				Ok(json!({
					"symbols": workspace_symbols,
					"count": workspace_symbols.len()
				}))
			} else {
				Ok(json!({
					"symbols": [],
					"count": 0
				}))
			}
		} else {
			Ok(json!({
				"symbols": [],
				"count": 0
			}))
		}
	}

	/// LSP completion tool
	pub async fn completion(&self, file_path: &str, line: u32, character: u32) -> Result<Value> {
		if !self.is_ready() {
			return Err(Self::lsp_not_ready_error());
		}

		debug!("LSP completion: {}:{}:{}", file_path, line, character);

		let params = CompletionParams {
			text_document_position: self.text_document_position(file_path, line, character)?,
			work_done_progress_params: WorkDoneProgressParams::default(),
			partial_result_params: PartialResultParams::default(),
			context: None,
		};

		let request = LspRequest::completion(self.next_request_id(), params)?;
		let response = self.client.send_request(request).await?;

		if let Some(result) = response.result {
			let completion_response = self.parse_completion_response(result)?;
			Ok(json!({
				"completions": completion_response,
				"count": completion_response.len()
			}))
		} else {
			Ok(json!({
				"completions": [],
				"count": 0
			}))
		}
	}

	// Helper methods

	fn next_request_id(&self) -> u32 {
		// Simple counter for request IDs
		static REQUEST_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
		REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
	}

	fn make_path_relative(&self, absolute_path: &std::path::Path) -> String {
		if let Ok(relative) = absolute_path.strip_prefix(&self.working_directory) {
			relative.to_string_lossy().to_string()
		} else {
			absolute_path.to_string_lossy().to_string()
		}
	}

	fn parse_goto_definition_response(&self, result: Value) -> Result<Vec<Location>> {
		// LSP can return Location, Vec<Location>, or LocationLink
		if result.is_null() {
			return Ok(vec![]);
		}

		// Try as single Location
		if let Ok(location) = serde_json::from_value::<Location>(result.clone()) {
			return Ok(vec![location]);
		}

		// Try as Vec<Location>
		if let Ok(locations) = serde_json::from_value::<Vec<Location>>(result.clone()) {
			return Ok(locations);
		}

		// Try as LocationLink (convert to Location)
		if let Ok(links) = serde_json::from_value::<Vec<LocationLink>>(result) {
			let locations = links
				.into_iter()
				.map(|link| Location {
					uri: link.target_uri,
					range: link.target_selection_range,
				})
				.collect();
			return Ok(locations);
		}

		warn!("Unknown goto definition response format");
		Ok(vec![])
	}

	fn extract_hover_contents(&self, contents: &HoverContents) -> String {
		match contents {
			HoverContents::Scalar(markup) => match markup {
				MarkedString::String(s) => s.clone(),
				MarkedString::LanguageString(ls) => ls.value.clone(),
			},
			HoverContents::Array(markups) => markups
				.iter()
				.map(|m| match m {
					MarkedString::String(s) => s.clone(),
					MarkedString::LanguageString(ls) => ls.value.clone(),
				})
				.collect::<Vec<_>>()
				.join("\n\n"),
			HoverContents::Markup(markup) => markup.value.clone(),
		}
	}

	fn parse_document_symbols_response(&self, result: Value) -> Result<Vec<Value>> {
		// LSP can return DocumentSymbol[] or SymbolInformation[]
		if result.is_null() {
			return Ok(vec![]);
		}

		// Try as DocumentSymbol[]
		if let Ok(doc_symbols) = serde_json::from_value::<Vec<DocumentSymbol>>(result.clone()) {
			let symbols = doc_symbols
				.into_iter()
				.map(|symbol| {
					json!({
						"name": symbol.name,
						"kind": format!("{:?}", symbol.kind),
						"line": symbol.range.start.line + 1,
						"character": symbol.range.start.character + 1,
						"end_line": symbol.range.end.line + 1,
						"end_character": symbol.range.end.character + 1,
						"detail": symbol.detail
					})
				})
				.collect();
			return Ok(symbols);
		}

		// Try as SymbolInformation[]
		if let Ok(symbol_infos) = serde_json::from_value::<Vec<SymbolInformation>>(result) {
			let symbols = symbol_infos
				.into_iter()
				.map(|symbol| {
					json!({
						"name": symbol.name,
						"kind": format!("{:?}", symbol.kind),
						"line": symbol.location.range.start.line + 1,
						"character": symbol.location.range.start.character + 1,
						"end_line": symbol.location.range.end.line + 1,
						"end_character": symbol.location.range.end.character + 1,
						"container_name": symbol.container_name
					})
				})
				.collect();
			return Ok(symbols);
		}

		warn!("Unknown document symbols response format");
		Ok(vec![])
	}

	fn parse_completion_response(&self, result: Value) -> Result<Vec<Value>> {
		// LSP can return CompletionItem[] or CompletionList
		if result.is_null() {
			return Ok(vec![]);
		}

		// Try as CompletionList
		if let Ok(completion_list) = serde_json::from_value::<CompletionList>(result.clone()) {
			let items = completion_list
				.items
				.into_iter()
				.map(|item| {
					json!({
						"label": item.label,
						"kind": item.kind.map(|k| format!("{:?}", k)),
						"detail": item.detail,
						"documentation": item.documentation.map(|doc| match doc {
							Documentation::String(s) => s,
							Documentation::MarkupContent(markup) => markup.value,
						}),
						"insert_text": item.insert_text
					})
				})
				.collect();
			return Ok(items);
		}

		// Try as CompletionItem[]
		if let Ok(completion_items) = serde_json::from_value::<Vec<CompletionItem>>(result) {
			let items = completion_items
				.into_iter()
				.map(|item| {
					json!({
						"label": item.label,
						"kind": item.kind.map(|k| format!("{:?}", k)),
						"detail": item.detail,
						"documentation": item.documentation.map(|doc| match doc {
							Documentation::String(s) => s,
							Documentation::MarkupContent(markup) => markup.value,
						}),
						"insert_text": item.insert_text
					})
				})
				.collect();
			return Ok(items);
		}

		warn!("Unknown completion response format");
		Ok(vec![])
	}
}
