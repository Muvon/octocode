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

//! LLM client wrapper for octolib integration
//!
//! This module provides a clean wrapper around octolib's LLM functionality
//! with octocode-specific helpers and configuration integration.

use crate::config::Config;
use anyhow::Result;
use serde::de::DeserializeOwned;

// Re-export octolib types for convenience
pub use octolib::llm::{
	AiProvider, ChatCompletionParams, Message, MessageBuilder, ProviderFactory, ProviderResponse,
	StructuredOutputRequest, TokenUsage,
};

/// LLM client wrapper that integrates octolib with octocode configuration
/// LLM client wrapper that integrates octolib with octocode configuration
pub struct LlmClient {
	provider: Box<dyn AiProvider>,
	model: String,
	temperature: f32,
	max_tokens: usize,
}

impl LlmClient {
	/// Create LlmClient from octocode Config
	pub fn from_config(config: &Config) -> Result<Self> {
		let (provider, model) = ProviderFactory::get_provider_for_model(&config.llm.model)?;

		Ok(Self {
			provider,
			model,
			temperature: config.llm.temperature,
			max_tokens: config.llm.max_tokens,
		})
	}

	/// Create LlmClient with custom model (overrides config)
	pub fn with_model(config: &Config, model_str: &str) -> Result<Self> {
		let (provider, model) = ProviderFactory::get_provider_for_model(model_str)?;

		Ok(Self {
			provider,
			model,
			temperature: config.llm.temperature,
			max_tokens: config.llm.max_tokens,
		})
	}

	/// Simple chat completion returning text response
	pub async fn chat_completion(&self, messages: Vec<Message>) -> Result<String> {
		let params = ChatCompletionParams::new(
			&messages,
			&self.model,
			self.temperature,
			1.0,                    // top_p
			50,                     // min_tokens
			self.max_tokens as u32, // max_tokens (convert usize to u32)
		);

		let response = self.provider.chat_completion(params).await?;

		// Log token usage if available from exchange
		if let Some(usage) = &response.exchange.usage {
			tracing::debug!(
				"LLM tokens: input={}, output={}, total={}",
				usage.input_tokens,
				usage.output_tokens,
				usage.total_tokens
			);

			if let Some(cost) = usage.cost {
				tracing::debug!("LLM cost: ${:.6}", cost);
			}
		}

		Ok(response.content)
	}

	/// Chat completion with structured JSON output
	pub async fn chat_completion_structured<T: DeserializeOwned>(
		&self,
		messages: Vec<Message>,
	) -> Result<T> {
		// Check if provider supports structured output
		if !self.provider.supports_structured_output(&self.model) {
			return Err(anyhow::anyhow!(
				"Provider does not support structured output for model: {}",
				self.model
			));
		}

		let structured_request = StructuredOutputRequest::json();
		let params = ChatCompletionParams::new(
			&messages,
			&self.model,
			self.temperature,
			1.0,                    // top_p
			50,                     // min_tokens
			self.max_tokens as u32, // max_tokens (convert usize to u32)
		)
		.with_structured_output(structured_request);

		let response = self.provider.chat_completion(params).await?;

		// Log token usage
		if let Some(usage) = &response.exchange.usage {
			tracing::debug!(
				"LLM tokens (structured): input={}, output={}, total={}",
				usage.input_tokens,
				usage.output_tokens,
				usage.total_tokens
			);

			if let Some(cost) = usage.cost {
				tracing::debug!("LLM cost: ${:.6}", cost);
			}
		}

		// Parse structured output
		if let Some(structured) = response.structured_output {
			let result: T = serde_json::from_value(structured)?;
			Ok(result)
		} else {
			// Fallback: try to parse content as JSON
			let result: T = serde_json::from_str(&response.content)?;
			Ok(result)
		}
	}

	/// Chat completion with custom temperature
	pub async fn chat_completion_with_temperature(
		&self,
		messages: Vec<Message>,
		temperature: f32,
	) -> Result<String> {
		let params = ChatCompletionParams::new(
			&messages,
			&self.model,
			temperature,
			1.0,                    // top_p
			50,                     // min_tokens
			self.max_tokens as u32, // max_tokens (convert usize to u32)
		);

		let response = self.provider.chat_completion(params).await?;

		// Log token usage
		if let Some(usage) = &response.exchange.usage {
			tracing::debug!(
				"LLM tokens: input={}, output={}, total={}",
				usage.input_tokens,
				usage.output_tokens,
				usage.total_tokens
			);

			if let Some(cost) = usage.cost {
				tracing::debug!("LLM cost: ${:.6}", cost);
			}
		}

		Ok(response.content)
	}

	/// Get the model name
	pub fn model(&self) -> &str {
		&self.model
	}

	/// Check if provider supports structured output
	pub fn supports_structured_output(&self) -> bool {
		self.provider.supports_structured_output(&self.model)
	}

	/// Chat completion with JSON output (tries structured output, falls back to markdown stripping)
	///
	/// This method first attempts to use structured output if the provider supports it.
	/// If structured output is not available, it uses regular completion and strips
	/// markdown code blocks to extract raw JSON.
	///
	/// # Returns
	/// Parsed JSON value or an error
	pub async fn chat_completion_json(&self, messages: Vec<Message>) -> Result<serde_json::Value> {
		let supports_structured = self.provider.supports_structured_output(&self.model);
		tracing::debug!(
			"Provider {} supports structured output for model {}: {}",
			self.provider.name(),
			self.model,
			supports_structured
		);

		if supports_structured {
			// Try structured output first
			let structured_request = StructuredOutputRequest::json();
			let params = ChatCompletionParams::new(
				&messages,
				&self.model,
				self.temperature,
				1.0,
				50,
				self.max_tokens as u32,
			)
			.with_structured_output(structured_request);

			let response = self.provider.chat_completion(params).await?;

			// Log token usage
			if let Some(usage) = &response.exchange.usage {
				tracing::debug!(
					"LLM tokens (structured): input={}, output={}, total={}",
					usage.input_tokens,
					usage.output_tokens,
					usage.total_tokens
				);

				if let Some(cost) = usage.cost {
					tracing::debug!("LLM cost: ${:.6}", cost);
				}
			}

			tracing::debug!(
				"Response has structured_output: {}",
				response.structured_output.is_some()
			);
			tracing::debug!("Response content length: {}", response.content.len());
			tracing::debug!(
				"Response content preview: {}",
				response.content.chars().take(200).collect::<String>()
			);

			// Return structured output if available
			if let Some(structured) = response.structured_output {
				tracing::debug!("Using structured output from provider");
				return Ok(structured);
			}

			// Fall through to try parsing content
			tracing::debug!("No structured output, falling back to content parsing");
		} else {
			tracing::debug!("Provider does not support structured output, using markdown fallback");
		}

		// Fallback: use regular completion and strip markdown
		let content = self.chat_completion(messages).await?;
		tracing::debug!("Raw content length: {}", content.len());
		tracing::debug!(
			"Raw content preview: {}",
			content.chars().take(200).collect::<String>()
		);

		let json = Self::strip_json_from_markdown(&content);
		tracing::debug!(
			"Parsed JSON has error field: {}",
			json.get("error").is_some()
		);

		Ok(json)
	}

	/// Strip markdown code blocks from JSON content and parse it
	///
	/// LLMs often return JSON wrapped in markdown code blocks like:
	/// ```json
	/// { "key": "value" }
	/// ```
	///
	/// This method extracts the raw JSON and parses it.
	fn strip_json_from_markdown(content: &str) -> serde_json::Value {
		// Try to parse as-is first (in case it's already raw JSON)
		if let Ok(parsed) = serde_json::from_str(content.trim()) {
			return parsed;
		}

		// Look for JSON code block
		let marker = "```json";
		let end_marker = "```";

		if let Some(start) = content.find(marker) {
			let after_marker = &content[start + marker.len()..];
			if let Some(end) = after_marker.find(end_marker) {
				let json_content = &after_marker[..end];
				if let Ok(parsed) = serde_json::from_str(json_content.trim()) {
					return parsed;
				}
			}
		}

		// Look for any code block and try to parse its content
		let mut in_code_block = false;
		let mut code_start = 0;

		for (line_num, line) in content.lines().enumerate() {
			let trimmed = line.trim();
			if trimmed.starts_with("```") {
				if !in_code_block {
					// Found code block start - set position to after this line
					in_code_block = true;
					// Calculate position after this line (line start + line length + newline)
					code_start = content
						.lines()
						.take(line_num + 1)
						.map(|l| l.len() + 1)
						.sum();
				} else {
					// Found code block end - extract content from code_start to current position
					let line_start = content.lines().take(line_num).map(|l| l.len() + 1).sum();
					let code_content = &content[code_start..line_start];
					if let Ok(parsed) = serde_json::from_str(code_content.trim()) {
						return parsed;
					}
					break;
				}
			}
		}

		// Last resort: try to extract JSON by looking for { or [
		if let Some(start) = content.find('{') {
			if let Ok(parsed) = serde_json::from_str(&content[start..]) {
				return parsed;
			}
		}
		if let Some(start) = content.find('[') {
			if let Ok(parsed) = serde_json::from_str(&content[start..]) {
				return parsed;
			}
		}

		// Return error as JSON
		serde_json::json!({
			"error": "Failed to parse JSON from response",
			"raw_content": content
		})
	}
}
