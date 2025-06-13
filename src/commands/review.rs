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

use octocode::config::Config;

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

	// Check if we're in a git repository
	if !current_dir.join(".git").exists() {
		return Err(anyhow::anyhow!("❌ Not in a git repository!"));
	}

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

	// Perform the code review
	println!("\n🤖 Analyzing changes for best practices and potential issues...");
	let review_result = perform_code_review(&current_dir, config, args).await?;

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

async fn perform_code_review(
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

	// Prepare the enhanced prompt for code review
	let prompt = format!(
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
		analyze_file_types(&changed_files),
		if file_stats.trim().is_empty() { "No stats available" } else { &file_stats },
		// Truncate diff if it's too long (keep first 8000 chars for thorough analysis)
		if diff.chars().count() > 8000 {
			let truncated: String = diff.chars().take(8000).collect();
			format!("{}...\n[diff truncated for brevity]", truncated)
		} else {
			diff
		},
		focus_context
	);

	// Call the LLM for code review
	match call_llm_for_review(&prompt, config).await {
		Ok(response) => {
			// Parse the JSON response (should be valid due to structured output)
			match serde_json::from_str::<ReviewResult>(&response) {
				Ok(review_result) => Ok(review_result),
				Err(e) => {
					eprintln!(
						"Warning: Failed to parse LLM response as JSON ({}), creating fallback",
						e
					);
					eprintln!("Raw response: {}", response);
					create_fallback_review(file_count, &changed_files, &response)
				}
			}
		}
		Err(e) => {
			eprintln!("Warning: LLM call failed ({}), creating basic review", e);
			create_fallback_review(file_count, &changed_files, "LLM analysis failed")
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
		"max_tokens": 2000,
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
