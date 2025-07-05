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

// GraphRAG AI-powered enhancements

use crate::config::Config;
use crate::indexer::graphrag::types::{CodeNode, CodeRelationship};
use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;

pub struct AIEnhancements {
	config: Config,
	client: Client,
	quiet: bool,
}

// Structure for collecting files that need AI descriptions
#[derive(Debug, Clone)]
pub struct FileForAI {
	pub file_id: String,
	pub file_path: String,
	pub language: String,
	pub symbols: Vec<String>,
	pub content_sample: String,
	pub function_count: usize,
	pub class_count: usize,
}

// Response structure for batch AI descriptions
#[derive(Debug, Deserialize)]
struct BatchDescriptionResponse {
	descriptions: Vec<FileDescription>,
}

#[derive(Debug, Deserialize)]
struct FileDescription {
	file_id: String,
	description: String,
}

impl AIEnhancements {
	pub fn new(config: Config, client: Client, quiet: bool) -> Self {
		Self {
			config,
			client,
			quiet,
		}
	}

	// Check if LLM enhancements are enabled
	pub fn llm_enabled(&self) -> bool {
		self.config.graphrag.use_llm
	}

	// Enhanced relationship discovery with optional AI for complex cases
	pub async fn discover_relationships_with_ai_enhancement(
		&self,
		new_files: &[CodeNode],
		all_nodes: &[CodeNode],
	) -> Result<Vec<CodeRelationship>> {
		// Start with rule-based relationships (fast and reliable)
		let mut relationships = crate::indexer::graphrag::relationships::RelationshipDiscovery::discover_relationships_efficiently(new_files, all_nodes).await?;

		// Add AI-enhanced relationship discovery for complex architectural patterns
		let ai_relationships = self
			.discover_complex_relationships_with_ai(new_files, all_nodes)
			.await?;
		relationships.extend(ai_relationships);

		// Deduplicate
		relationships.sort_by(|a, b| {
			(a.source.clone(), a.target.clone(), a.relation_type.clone()).cmp(&(
				b.source.clone(),
				b.target.clone(),
				b.relation_type.clone(),
			))
		});
		relationships.dedup_by(|a, b| {
			a.source == b.source && a.target == b.target && a.relation_type == b.relation_type
		});

		Ok(relationships)
	}

	// Use AI to discover complex architectural relationships
	async fn discover_complex_relationships_with_ai(
		&self,
		new_files: &[CodeNode],
		all_nodes: &[CodeNode],
	) -> Result<Vec<CodeRelationship>> {
		let mut ai_relationships = Vec::new();

		// Only use AI for files that are likely to have complex architectural relationships
		let complex_files: Vec<&CodeNode> = new_files
			.iter()
			.filter(|node| self.should_use_ai_for_relationships(node))
			.collect();

		if complex_files.is_empty() {
			if !self.quiet {
				eprintln!("Debug: No files qualified for AI relationship analysis in this batch");
			}
			return Ok(ai_relationships);
		}

		if !self.quiet {
			eprintln!(
				"Info: AI analyzing {} files for architectural relationships",
				complex_files.len()
			);
		}

		// Process in small batches to avoid overwhelming the AI
		let ai_batch_size = self.config.graphrag.llm.ai_batch_size;
		for batch in complex_files.chunks(ai_batch_size) {
			if let Ok(batch_relationships) = self
				.analyze_architectural_relationships_batch(batch, all_nodes)
				.await
			{
				ai_relationships.extend(batch_relationships);
			}
		}

		Ok(ai_relationships)
	}

	// Determine if a file is complex enough to benefit from AI relationship analysis
	fn should_use_ai_for_relationships(&self, node: &CodeNode) -> bool {
		// Use actual code substance, not directory guessing
		let has_meaningful_exports = node.exports.len() >= 2;
		let is_substantial_file = node.size_lines >= 50;
		let has_multiple_symbols = node.symbols.len() >= 3;

		// If file has substance, analyze it
		has_meaningful_exports || is_substantial_file || has_multiple_symbols
	}

	// Analyze architectural relationships using AI in small batches
	async fn analyze_architectural_relationships_batch(
		&self,
		source_nodes: &[&CodeNode],
		all_nodes: &[CodeNode],
	) -> Result<Vec<CodeRelationship>> {
		let system_prompt = String::from(
			"You are an expert software architect. Analyze these code files and identify ARCHITECTURAL relationships.\n\
				Focus on design patterns, dependency injection, factory patterns, observer patterns, etc.\n\
				Look for relationships that go beyond simple imports - identify architectural significance.\n\n\
				Respond with a JSON array of relationships. For each relationship, include:\n\
				- source_path: relative path of source file\n\
				- target_path: relative path of target file\n\
				- relation_type: one of 'implements_pattern', 'dependency_injection', 'factory_creates', 'observer_pattern', 'strategy_pattern', 'adapter_pattern', 'decorator_pattern', 'architectural_dependency'\n\
				- description: brief explanation of the architectural relationship\n\
				- confidence: 0.0-1.0 confidence score\n\n"
		);
		let mut batch_prompt = String::from("");

		// Add source nodes context
		batch_prompt.push_str("SOURCE FILES TO ANALYZE:\n");
		for node in source_nodes {
			batch_prompt.push_str(&format!(
				"File: {}\nLanguage: {}\nKey symbols: {}\nExports: {}\n\n",
				node.path,
				node.language,
				node.symbols
					.iter()
					.take(8)
					.cloned()
					.collect::<Vec<_>>()
					.join(", "),
				node.exports
					.iter()
					.take(5)
					.cloned()
					.collect::<Vec<_>>()
					.join(", ")
			));
		}

		// Add relevant target nodes (potential relationship targets)
		batch_prompt.push_str("POTENTIAL RELATIONSHIP TARGETS:\n");
		let relevant_targets: Vec<&CodeNode> = all_nodes
			.iter()
			.filter(|n| source_nodes.iter().all(|s| s.id != n.id)) // Not source files
			.filter(|n| !n.exports.is_empty() || n.size_lines > 100) // Has exports or is substantial
			.take(10) // Limit context size
			.collect();

		for node in &relevant_targets {
			batch_prompt.push_str(&format!(
				"File: {}\nLanguage: {}\nExports: {}\n\n",
				node.path,
				node.language,
				node.exports
					.iter()
					.take(3)
					.cloned()
					.collect::<Vec<_>>()
					.join(", ")
			));
		}

		batch_prompt.push_str("JSON Response:");

		// Call AI with architectural analysis
		match self
			.call_llm(
				&self.config.graphrag.llm.relationship_model,
				system_prompt,
				batch_prompt,
				None,
			)
			.await
		{
			Ok(response) => {
				// Parse AI response
				if let Ok(ai_relationships) = self.parse_ai_architectural_relationships(&response) {
					// Filter and validate relationships
					let valid_relationships: Vec<CodeRelationship> = ai_relationships
						.into_iter()
						.filter(|rel| {
							rel.confidence > self.config.graphrag.llm.confidence_threshold
						}) // Only high-confidence architectural relationships
						.filter(|rel| all_nodes.iter().any(|n| n.path == rel.target)) // Ensure target exists
						.map(|mut rel| {
							rel.weight = self.config.graphrag.llm.architectural_weight; // High weight for architectural relationships
							rel
						})
						.collect();

					Ok(valid_relationships)
				} else {
					Ok(Vec::new())
				}
			}
			Err(e) => {
				eprintln!("Warning: AI architectural analysis failed: {}", e);
				Ok(Vec::new())
			}
		}
	}

	// Parse AI response for architectural relationships
	fn parse_ai_architectural_relationships(
		&self,
		response: &str,
	) -> Result<Vec<CodeRelationship>> {
		#[derive(Deserialize)]
		struct AiRelationship {
			source_path: String,
			target_path: String,
			relation_type: String,
			description: String,
			confidence: f32,
		}

		// Try to parse as JSON array
		if let Ok(ai_rels) = serde_json::from_str::<Vec<AiRelationship>>(response) {
			let relationships = ai_rels
				.into_iter()
				.map(|ai_rel| CodeRelationship {
					source: ai_rel.source_path,
					target: ai_rel.target_path,
					relation_type: ai_rel.relation_type,
					description: ai_rel.description,
					confidence: ai_rel.confidence,
					weight: 0.9, // High weight for AI-discovered architectural patterns
				})
				.collect();
			return Ok(relationships);
		}

		// If JSON parsing fails, return empty (AI might have responded in wrong format)
		Ok(Vec::new())
	}

	// Determine if a file is complex enough to benefit from AI analysis
	pub fn should_use_ai_for_description(
		&self,
		symbols: &[String],
		lines: u32,
		language: &str,
	) -> bool {
		// FIXED: Count actual symbols, not prefixed ones
		let function_count = symbols.len(); // All extracted symbols are meaningful
		let has_substantial_content = lines > 20; // Lower threshold for testing
											// FIXED: Use dynamic language detection instead of hardcoded list
		let is_important_language = crate::indexer::languages::get_language(language).is_some();

		// For debugging: always use AI for substantial files in important languages
		let should_use = has_substantial_content && is_important_language && function_count > 0;

		if !self.quiet {
			eprintln!(
				"🤖 AI Decision: file={} lines, symbols={}, language={}, use_ai={}",
				lines,
				symbols.len(),
				language,
				should_use
			);
			if should_use && !symbols.is_empty() {
				eprintln!("🔍 Symbols found: {:?}", &symbols[..symbols.len().min(5)]);
			}
		}

		should_use
	}

	// Build a meaningful content sample for AI analysis (not full file content)
	pub fn build_content_sample_for_ai(&self, file_blocks: &[&crate::store::CodeBlock]) -> String {
		let mut sample = String::new();
		let mut total_tokens = 0;
		let max_tokens = self.config.graphrag.llm.max_sample_tokens;

		// Prioritize blocks with more symbols (more important code)
		let mut sorted_blocks: Vec<&crate::store::CodeBlock> = file_blocks.to_vec();
		sorted_blocks.sort_by(|a, b| b.symbols.len().cmp(&a.symbols.len()));

		for block in sorted_blocks {
			let block_tokens = crate::embedding::count_tokens(&block.content);
			if total_tokens + block_tokens >= max_tokens {
				break;
			}

			// Add block content with some context
			let block_content = if block.content.len() > 300 {
				// For large blocks, take beginning and end with proper UTF-8 handling
				let start_chars: String = block.content.chars().take(150).collect();
				let end_chars: String = block
					.content
					.chars()
					.rev()
					.take(150)
					.collect::<String>()
					.chars()
					.rev()
					.collect();
				format!("{}\n...\n{}", start_chars, end_chars)
			} else {
				block.content.clone()
			};

			sample.push_str(&format!(
				"// Block: {} symbols\n{}\n\n",
				block.symbols.len(),
				block_content
			));
			total_tokens += block_tokens + 50; // +50 for formatting tokens
		}

		sample
	}

	// Extract AI-powered descriptions for multiple files in a single batch call
	pub async fn extract_ai_descriptions_batch(
		&self,
		files: &[FileForAI],
	) -> Result<HashMap<String, String>> {
		if files.is_empty() {
			return Ok(HashMap::new());
		}

		// Build batch user message with all files
		let user_message = self.build_batch_user_message(files);

		// Create JSON schema for structured response
		let json_schema = self.create_batch_response_schema();

		// Single API call for multiple files
		match self
			.call_llm(
				&self.config.graphrag.llm.description_model,
				self.config.graphrag.llm.description_system_prompt.clone(),
				user_message,
				Some(json_schema),
			)
			.await
		{
			Ok(response) => self.parse_batch_response(&response, files),
			Err(e) => {
				if !self.quiet {
					eprintln!(
						"⚠️  Batch AI description failed for {} files: {}",
						files.len(),
						e
					);
				}

				// Fallback to individual calls if batch fails
				if self.config.graphrag.llm.ai_batch_size > 1 {
					if !self.quiet {
						eprintln!("🔄 Falling back to individual AI calls...");
					}
					self.fallback_to_individual_calls(files).await
				} else {
					Err(e)
				}
			}
		}
	}

	// Build user message for batch processing
	fn build_batch_user_message(&self, files: &[FileForAI]) -> String {
		let mut message = format!(
			"Analyze the following {} files and provide architectural descriptions:\n\n",
			files.len()
		);

		for (index, file) in files.iter().enumerate() {
			message.push_str(&format!("=== FILE {} ===\n", index + 1));
			message.push_str(&format!("ID: {}\n", file.file_id));
			message.push_str(&format!("Language: {}\n", file.language));
			message.push_str(&format!(
				"Stats: {} functions, {} classes/structs\n",
				file.function_count, file.class_count
			));
			message.push_str(&format!(
				"Key symbols: {}\n",
				file.symbols
					.iter()
					.take(5)
					.cloned()
					.collect::<Vec<_>>()
					.join(", ")
			));
			message.push_str(&format!("Code sample:\n{}\n\n", file.content_sample));
		}

		message.push_str("Provide a JSON response with descriptions for each file.");
		message
	}

	// Create JSON schema for batch response
	fn create_batch_response_schema(&self) -> serde_json::Value {
		json!({
			"type": "object",
			"properties": {
				"descriptions": {
					"type": "array",
					"items": {
						"type": "object",
						"properties": {
							"file_id": {
								"type": "string",
								"description": "The file ID exactly as provided in the request"
							},
							"description": {
								"type": "string",
								"description": "Architectural description of the file (max 300 chars)"
							}
						},
						"required": ["file_id", "description"],
						"additionalProperties": false
					}
				}
			},
			"required": ["descriptions"],
			"additionalProperties": false
		})
	}

	// Parse batch response back to individual file descriptions
	fn parse_batch_response(
		&self,
		response: &str,
		files: &[FileForAI],
	) -> Result<HashMap<String, String>> {
		let parsed: BatchDescriptionResponse = serde_json::from_str(response)
			.map_err(|e| anyhow::anyhow!("Failed to parse batch response: {}", e))?;

		let mut results = HashMap::new();

		for desc in parsed.descriptions {
			// Validate that file_id exists in our request
			if files.iter().any(|f| f.file_id == desc.file_id) {
				let cleaned_desc = if desc.description.len() > 300 {
					format!("{}...", &desc.description[0..297])
				} else {
					desc.description
				};
				results.insert(desc.file_id, cleaned_desc);
			} else if !self.quiet {
				eprintln!(
					"⚠️  Received description for unknown file: {}",
					desc.file_id
				);
			}
		}

		// Check if we got descriptions for all files
		let missing_files: Vec<&str> = files
			.iter()
			.filter(|f| !results.contains_key(&f.file_id))
			.map(|f| f.file_id.as_str())
			.collect();

		if !missing_files.is_empty() && !self.quiet {
			eprintln!(
				"⚠️  Missing descriptions for {} files: {:?}",
				missing_files.len(),
				missing_files
			);
		}

		Ok(results)
	}

	// Fallback to individual calls when batch fails
	async fn fallback_to_individual_calls(
		&self,
		files: &[FileForAI],
	) -> Result<HashMap<String, String>> {
		let mut results = HashMap::new();

		for file in files {
			match self
				.extract_ai_description(
					&file.content_sample,
					&file.file_path,
					&file.language,
					&file.symbols,
				)
				.await
			{
				Ok(description) => {
					results.insert(file.file_id.clone(), description);
				}
				Err(e) => {
					if !self.quiet {
						eprintln!(
							"⚠️  Individual AI description failed for {}: {}",
							file.file_id, e
						);
					}
					// Continue with other files even if one fails
				}
			}
		}

		Ok(results)
	}

	// Call LLM API
	async fn call_llm(
		&self,
		model_name: &str,
		system: String,
		prompt: String,
		json_schema: Option<serde_json::Value>,
	) -> Result<String> {
		// Check if we have an API key configured
		let api_key = match &self.config.openrouter.api_key {
			Some(key) => key.clone(),
			None => return Err(anyhow::anyhow!("OpenRouter API key not configured")),
		};

		// Prepare request body
		let mut request_body = json!({
			"model": model_name,
		"messages": [{
			"role": "system",
			"content": system
		}, {
			"role": "user",
			"content": prompt
		}],
			// "max_tokens": 200
		});

		// Only add response_format if schema is provided
		if let Some(schema_value) = json_schema {
			request_body["response_format"] = json!({
				"type": "json_schema",
				"json_schema": {
					"name": "relationship",
					"strict": true,
					"schema": schema_value
				}
			});
		}

		// Call OpenRouter API
		let response = self
			.client
			.post("https://openrouter.ai/api/v1/chat/completions")
			.header("Authorization", format!("Bearer {}", api_key))
			.header("HTTP-Referer", "https://github.com/muvon/octocode")
			.header("X-Title", "Octocode")
			.json(&request_body)
			.send()
			.await?;

		// Check if the API call was successful
		if !response.status().is_success() {
			let status = response.status();
			let error_text = response
				.text()
				.await
				.unwrap_or_else(|_| "Unable to read error response".to_string());
			return Err(anyhow::anyhow!("API error: {} - {}", status, error_text));
		}

		// Parse the response
		let response_json = response.json::<serde_json::Value>().await?;

		// Extract the response text
		if let Some(content) = response_json["choices"][0]["message"]["content"].as_str() {
			Ok(content.to_string())
		} else {
			// Provide more detailed error information
			Err(anyhow::anyhow!(
				"Failed to get response content: {:?}",
				response_json
			))
		}
	}

	// Extract AI-powered description for complex files (legacy single-file method)
	pub async fn extract_ai_description(
		&self,
		content_sample: &str,
		file_path: &str,
		language: &str,
		symbols: &[String],
	) -> Result<String> {
		let function_count = symbols
			.iter()
			.filter(|s| s.contains("function_") || s.contains("method_"))
			.count();
		let class_count = symbols
			.iter()
			.filter(|s| s.contains("class_") || s.contains("struct_"))
			.count();

		// Separate system prompt from file data for better LLM handling
		let user_message = format!(
			"File: {}\nLanguage: {}\nStats: {} functions, {} classes/structs\nKey symbols: {}\n\nCode sample:\n{}",
			std::path::Path::new(file_path).file_name().and_then(|s| s.to_str()).unwrap_or("unknown"),
			language,
			function_count,
			class_count,
			symbols.iter().take(5).cloned().collect::<Vec<_>>().join(", "),
			content_sample
		);

		match self
			.call_llm(
				&self.config.graphrag.llm.description_model,
				self.config.graphrag.llm.description_system_prompt.clone(),
				user_message,
				None,
			)
			.await
		{
			Ok(description) => {
				let cleaned = description.trim();
				if cleaned.len() > 300 {
					Ok(format!("{}...", &cleaned[0..297]))
				} else {
					Ok(cleaned.to_string())
				}
			}
			Err(e) => {
				if !self.quiet {
					eprintln!("Warning: AI description failed for {}: {}", file_path, e);
				}
				Err(e)
			}
		}
	}
}
