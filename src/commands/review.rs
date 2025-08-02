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

use anyhow::Result;
use clap::Args;
use std::collections::HashMap;
use std::process::Command;
use tokio::task::JoinSet;

use octocode::config::Config;
use octocode::indexer::git_utils::GitUtils;
use octocode::utils::diff_chunker;

#[derive(Args, Debug)]
pub struct ReviewArgs {
	/// Add all changes before reviewing
	#[arg(short, long)]
	pub all: bool,

	/// Focus on specific areas (security, performance, maintainability, style)
	#[arg(long)]
	pub focus: Option<String>,

	/// Output in JSON format for integration with other tools
	#[arg(long)]
	pub json: bool,

	/// Severity level filter: all, critical, high, medium, low
	#[arg(long, default_value = "medium")]
	pub severity: String,
}

pub async fn execute(config: &Config, args: &ReviewArgs) -> Result<()> {
	let current_dir = std::env::current_dir()?;

	// Find git repository root
	let git_root = GitUtils::find_git_root(&current_dir)
		.ok_or_else(|| anyhow::anyhow!("❌ Not in a git repository!"))?;

	// Use git root as working directory for all operations
	let current_dir = git_root;

	// Add all files if requested
	if args.all {
		println!("📂 Adding all changes for review...");
		let output = Command::new("git")
			.args(["add", "."])
			.current_dir(&current_dir)
			.output()?;

		if !output.status.success() {
			return Err(anyhow::anyhow!(
				"Failed to add files: {}",
				String::from_utf8_lossy(&output.stderr)
			));
		}
	}

	// Check if there are staged changes
	let output = Command::new("git")
		.args(["diff", "--cached", "--name-only"])
		.current_dir(&current_dir)
		.output()?;

	if !output.status.success() {
		return Err(anyhow::anyhow!(
			"Failed to check staged changes: {}",
			String::from_utf8_lossy(&output.stderr)
		));
	}

	let staged_files = String::from_utf8(output.stdout)?;
	if staged_files.trim().is_empty() {
		return Err(anyhow::anyhow!(
			"❌ No staged changes to review. Use 'git add' or --all flag."
		));
	}

	println!("🔍 Reviewing staged files:");
	for file in staged_files.lines() {
		println!("  • {}", file);
	}

	// Perform the code review with intelligent chunking for large diffs
	println!("\n🤖 Analyzing changes for best practices and potential issues...");
	let review_result = perform_code_review_chunked(&current_dir, config, args).await?;

	// Output the results
	if args.json {
		println!("{}", serde_json::to_string_pretty(&review_result)?);
	} else {
		display_review_results(&review_result, &args.severity);
	}

	Ok(())
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ReviewResult {
	summary: ReviewSummary,
	issues: Vec<ReviewIssue>,
	recommendations: Vec<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ReviewSummary {
	total_files: usize,
	total_issues: usize,
	overall_score: u8, // 0-100
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ReviewIssue {
	severity: String,
	category: String,
	title: String,
	description: String,
}

async fn perform_code_review_chunked(
	repo_path: &std::path::Path,
	config: &Config,
	args: &ReviewArgs,
) -> Result<ReviewResult> {
	// Get the diff of staged changes
	let output = Command::new("git")
		.args(["diff", "--cached"])
		.current_dir(repo_path)
		.output()?;

	if !output.status.success() {
		return Err(anyhow::anyhow!(
			"Failed to get diff: {}",
			String::from_utf8_lossy(&output.stderr)
		));
	}

	let diff = String::from_utf8(output.stdout)?;

	if diff.trim().is_empty() {
		return Err(anyhow::anyhow!("No staged changes found"));
	}

	// Get file statistics
	let stats_output = Command::new("git")
		.args(["diff", "--cached", "--stat"])
		.current_dir(repo_path)
		.output()?;

	let file_stats = if stats_output.status.success() {
		String::from_utf8_lossy(&stats_output.stdout).to_string()
	} else {
		String::new()
	};

	// Get list of changed files
	let files_output = Command::new("git")
		.args(["diff", "--cached", "--name-only"])
		.current_dir(repo_path)
		.output()?;

	let changed_files: Vec<String> = if files_output.status.success() {
		String::from_utf8_lossy(&files_output.stdout)
			.lines()
			.map(|s| s.to_string())
			.collect()
	} else {
		vec![]
	};

	// Analyze file types and count
	let file_count = changed_files.len();
	let additions = diff
		.matches("\n+")
		.count()
		.saturating_sub(diff.matches("\n+++").count());
	let deletions = diff
		.matches("\n-")
		.count()
		.saturating_sub(diff.matches("\n---").count());

	// Build focus area context
	let focus_context = if let Some(focus) = &args.focus {
		format!("\n\nFocus areas requested: {}", focus)
	} else {
		String::new()
	};

	// Check if we need to chunk the diff
	let chunks = diff_chunker::chunk_diff(&diff);

	if chunks.len() == 1 {
		// Single chunk - use existing logic
		let prompt = create_review_prompt(
			&chunks[0].content,
			file_count,
			additions,
			deletions,
			&analyze_file_types(&changed_files),
			&file_stats,
			&focus_context,
		);
		let response = call_llm_for_review(&prompt, config).await?;
		return parse_review_response(&response, file_count, &changed_files);
	}

	// Multiple chunks - process in parallel for comprehensive review
	println!(
		"🔍 Processing large diff in {} chunks in parallel for comprehensive review...",
		chunks.len()
	);

	let responses = process_review_chunks_parallel(
		&chunks,
		file_count,
		additions,
		deletions,
		&changed_files,
		&file_stats,
		&focus_context,
		config,
	)
	.await;

	/// Process review chunks in parallel with comprehensive error handling
	#[allow(clippy::too_many_arguments)]
	async fn process_review_chunks_parallel(
		chunks: &[diff_chunker::DiffChunk],
		file_count: usize,
		additions: usize,
		deletions: usize,
		changed_files: &[String],
		file_stats: &str,
		focus_context: &str,
		config: &Config,
	) -> Vec<String> {
		// Limit parallel processing to prevent resource exhaustion
		let chunk_limit = std::cmp::min(chunks.len(), diff_chunker::MAX_PARALLEL_CHUNKS);
		let mut join_set = JoinSet::new();

		// Process chunks in parallel batches
		for (i, chunk) in chunks.iter().take(chunk_limit).enumerate() {
			let chunk_content = chunk.content.clone();
			let chunk_summary = chunk.file_summary.clone();
			let config = config.clone();
			let file_types = analyze_file_types(changed_files);
			let file_stats = file_stats.to_string();
			let focus_context = focus_context.to_string();

			join_set.spawn(async move {
				println!(
					"  Analyzing chunk {}/{}: {}",
					i + 1,
					chunk_limit,
					chunk_summary
				);

				let chunk_prompt = create_review_prompt(
					&chunk_content,
					file_count,
					additions,
					deletions,
					&file_types,
					&file_stats,
					&focus_context,
				);

				match call_llm_for_review(&chunk_prompt, &config).await {
					Ok(response) => Ok((i, response)),
					Err(e) => {
						eprintln!("Warning: Chunk {} failed ({})", i + 1, e);
						Err(e)
					}
				}
			});
		}

		// Collect results maintaining order
		collect_review_responses(join_set, chunk_limit).await
	}

	/// Collect review responses from parallel tasks maintaining original order
	async fn collect_review_responses(
		mut join_set: JoinSet<Result<(usize, String)>>,
		expected_count: usize,
	) -> Vec<String> {
		let mut ordered_responses = vec![None; expected_count];

		while let Some(result) = join_set.join_next().await {
			match result {
				Ok(Ok((index, response))) => {
					ordered_responses[index] = Some(response);
				}
				Ok(Err(_)) => {
					// Error already logged in spawn
				}
				Err(e) => {
					eprintln!("Warning: Task join error: {}", e);
				}
			}
		}

		// Extract successful responses
		ordered_responses.into_iter().flatten().collect()
	}

	if responses.is_empty() {
		return create_fallback_review(file_count, &changed_files, "All chunks failed");
	}

	// Combine responses into final review result
	let combined_json = diff_chunker::combine_review_results(responses)?;
	parse_review_response(&combined_json, file_count, &changed_files)
}

fn create_review_prompt(
	diff_content: &str,
	file_count: usize,
	additions: usize,
	deletions: usize,
	file_types: &str,
	file_stats: &str,
	focus_context: &str,
) -> String {
	format!(
		"You are an expert code reviewer. Analyze the following git diff and provide a comprehensive code review focusing on best practices, potential issues, and maintainability.\n\n\
		ANALYSIS SCOPE:\n\
		- Files changed: {}\n\
		- Lines added: {}\n\
		- Lines deleted: {}\n\
		- File types: {}\n\n\
		REVIEW CRITERIA:\n\
		1. **Security Issues**: SQL injection, XSS, hardcoded secrets, insecure patterns\n\
		2. **Performance**: Inefficient algorithms, memory leaks, unnecessary computations\n\
		3. **Code Quality**: Complexity, readability, maintainability, DRY principle\n\
		4. **Best Practices**: Language-specific conventions, design patterns, error handling\n\
		5. **Testing**: Missing tests, test coverage, test quality\n\
		6. **Documentation**: Missing comments, unclear naming, API documentation\n\
		7. **Architecture**: Coupling, separation of concerns, SOLID principles\n\n\
		SEVERITY LEVELS:\n\
		- CRITICAL: Security vulnerabilities, data corruption risks, breaking changes\n\
		- HIGH: Performance issues, major bugs, significant technical debt\n\
		- MEDIUM: Code quality issues, minor bugs, style violations\n\
		- LOW: Suggestions, optimizations, documentation improvements\n\n\
		COMPLEXITY LEVELS:\n\
		- low: Simple, straightforward code\n\
		- medium: Moderate complexity with some logic\n\
		- high: Complex logic, multiple responsibilities\n\
		- very_high: Highly complex, difficult to understand\n\n\
		MAINTAINABILITY LEVELS:\n\
		- poor: Difficult to modify, lacks structure\n\
		- fair: Some issues but manageable\n\
		- good: Well-structured, easy to understand\n\
		- excellent: Exemplary code quality\n\n\
		File Statistics:\n\
		{}\n\n\
		Git Diff:\n\
		```\n{}\n```{}\n\n\
		Provide a structured analysis. Focus on actionable feedback and be specific about issues. Provide clear suggestions for improvements. Be thorough but concise.",
		file_count,
		additions,
		deletions,
		file_types,
		if file_stats.trim().is_empty() { "No stats available" } else { file_stats },
		diff_content,
		focus_context
	)
}

fn parse_review_response(
	response: &str,
	file_count: usize,
	files: &[String],
) -> Result<ReviewResult> {
	// Try to parse as JSON first
	match serde_json::from_str::<ReviewResult>(response) {
		Ok(review_result) => Ok(review_result),
		Err(e) => {
			eprintln!(
				"Warning: Failed to parse LLM response as JSON ({}), creating fallback",
				e
			);
			eprintln!("Raw response: {}", response);
			create_fallback_review(file_count, files, response)
		}
	}
}

fn analyze_file_types(files: &[String]) -> String {
	let mut type_counts: HashMap<String, usize> = HashMap::new();

	for file in files {
		if let Some(ext) = std::path::Path::new(file).extension() {
			if let Some(ext_str) = ext.to_str() {
				*type_counts.entry(ext_str.to_string()).or_insert(0) += 1;
			}
		}
	}

	type_counts
		.iter()
		.map(|(ext, count)| format!("{}: {}", ext, count))
		.collect::<Vec<_>>()
		.join(", ")
}

fn create_fallback_review(
	file_count: usize,
	_files: &[String],
	_llm_response: &str,
) -> Result<ReviewResult> {
	Ok(ReviewResult {
		summary: ReviewSummary {
			total_files: file_count,
			total_issues: 1,
			overall_score: 75,
		},
		issues: vec![ReviewIssue {
			severity: "MEDIUM".to_string(),
			category: "System".to_string(),
			title: "Review Analysis Incomplete".to_string(),
			description:
				"The automated review could not complete fully. Manual review recommended."
					.to_string(),
		}],
		recommendations: vec![
			"Consider running the review again".to_string(),
			"Perform manual code review for complex changes".to_string(),
		],
	})
}

fn display_review_results(review: &ReviewResult, severity_filter: &str) {
	println!("\n📊 Code Review Summary");
	println!("═══════════════════════════════════════════════");
	println!("📁 Files reviewed: {}", review.summary.total_files);
	println!("🔍 Total issues found: {}", review.summary.total_issues);
	println!("📈 Overall Score: {}/100", review.summary.overall_score);

	// Filter issues by severity
	let filtered_issues: Vec<&ReviewIssue> = review
		.issues
		.iter()
		.filter(|issue| should_show_issue(&issue.severity, severity_filter))
		.collect();

	if !filtered_issues.is_empty() {
		println!("\n🚨 Issues Found");
		println!("═══════════════════════════════════════════════");

		for issue in filtered_issues {
			let severity_emoji = match issue.severity.as_str() {
				"CRITICAL" => "🔥",
				"HIGH" => "⚠️",
				"MEDIUM" => "📝",
				"LOW" => "💡",
				_ => "❓",
			};

			println!("\n{} {} [{}]", severity_emoji, issue.title, issue.severity);
			println!("   Category: {}", issue.category);
			println!("   Description: {}", issue.description);
		}
	}

	if !review.recommendations.is_empty() {
		println!("\n💡 General Recommendations");
		println!("═══════════════════════════════════════════════");
		for (i, rec) in review.recommendations.iter().enumerate() {
			println!("{}. {}", i + 1, rec);
		}
	}

	// Score interpretation
	println!("\n📈 Score Interpretation");
	println!("═══════════════════════════════════════════════");
	match review.summary.overall_score {
		90..=100 => println!("🌟 Excellent - High quality code with minimal issues"),
		80..=89 => println!("✅ Good - Well-written code with minor improvements needed"),
		70..=79 => println!("⚠️  Fair - Some issues present, review recommended"),
		60..=69 => println!("❌ Poor - Multiple issues found, refactoring suggested"),
		_ => println!("🚨 Critical - Significant issues found, immediate attention required"),
	}
}

fn should_show_issue(issue_severity: &str, filter: &str) -> bool {
	let severity_levels = ["CRITICAL", "HIGH", "MEDIUM", "LOW"];
	let filter_index = severity_levels
		.iter()
		.position(|&x| x.to_lowercase() == filter.to_lowercase());
	let issue_index = severity_levels.iter().position(|&x| x == issue_severity);

	match (filter_index, issue_index) {
		(Some(filter_idx), Some(issue_idx)) => issue_idx <= filter_idx,
		_ => true, // Show all if unclear
	}
}

async fn call_llm_for_review(prompt: &str, config: &Config) -> Result<String> {
	use reqwest::Client;
	use serde_json::{json, Value};

	let client = Client::new();

	// Get API key
	let api_key = if let Some(key) = &config.openrouter.api_key {
		key.clone()
	} else if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
		key
	} else {
		return Err(anyhow::anyhow!("No OpenRouter API key found"));
	};

	// Prepare the request with structured output
	let payload = json!({
		"model": config.openrouter.model,
		"messages": [
			{
				"role": "user",
				"content": prompt
			}
		],
		"temperature": 0.2,
		"max_tokens": 4000,
		"response_format": {
			"type": "json_schema",
			"json_schema": {
				"name": "code_review_result",
				"strict": true,
				"schema": {
					"type": "object",
					"properties": {
						"summary": {
							"type": "object",
							"properties": {
								"total_files": {"type": "integer"},
								"total_issues": {"type": "integer"},
								"overall_score": {"type": "integer"}
							},
							"required": ["total_files", "total_issues", "overall_score"],
							"additionalProperties": false
						},
						"issues": {
							"type": "array",
							"items": {
								"type": "object",
								"properties": {
									"severity": {"type": "string"},
									"category": {"type": "string"},
									"title": {"type": "string"},
									"description": {"type": "string"}
								},
								"required": ["severity", "category", "title", "description"],
								"additionalProperties": false
							}
						},
						"recommendations": {
							"type": "array",
							"items": {"type": "string"}
						}
					},
					"required": ["summary", "issues", "recommendations"],
					"additionalProperties": false
				}
			}
		}
	});

	let response = client
		.post(format!(
			"{}/chat/completions",
			config.openrouter.base_url.trim_end_matches('/')
		))
		.header("Authorization", format!("Bearer {}", api_key))
		.header("HTTP-Referer", "https://github.com/muvon/octocode")
		.header("X-Title", "Octocode")
		.header("Content-Type", "application/json")
		.json(&payload)
		.timeout(std::time::Duration::from_secs(config.openrouter.timeout))
		.send()
		.await?;

	if !response.status().is_success() {
		let error_text = response.text().await?;
		return Err(anyhow::anyhow!("LLM API error: {}", error_text));
	}

	let response_json: Value = response.json().await?;

	let message = response_json
		.get("choices")
		.and_then(|choices| choices.get(0))
		.and_then(|choice| choice.get("message"))
		.and_then(|message| message.get("content"))
		.and_then(|content| content.as_str())
		.ok_or_else(|| anyhow::anyhow!("Invalid response format from LLM"))?;

	Ok(message.to_string())
}
