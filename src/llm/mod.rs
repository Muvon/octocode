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
				usage.prompt_tokens,
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
				usage.prompt_tokens,
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
				usage.prompt_tokens,
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
}
