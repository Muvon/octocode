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

use clap::Args;
use std::process::Command;
use std::io::{self, Write};
use anyhow::Result;

use octocode::config::Config;

#[derive(Args, Debug)]
pub struct CommitArgs {
	/// Add all changes before committing
	#[arg(short, long)]
	pub all: bool,

	/// Additional context to help AI generate better commit message
	#[arg(short, long)]
	pub message: Option<String>,

	/// Skip confirmation prompt
	#[arg(short, long)]
	pub yes: bool,
}

pub async fn execute(config: &Config, args: &CommitArgs) -> Result<()> {
	let current_dir = std::env::current_dir()?;

	// Check if we're in a git repository
	if !current_dir.join(".git").exists() {
		return Err(anyhow::anyhow!("❌ Not in a git repository!"));
	}

	// Add all files if requested
	if args.all {
		println!("📂 Adding all changes...");
		let output = Command::new("git")
			.args(&["add", "."])
			.current_dir(&current_dir)
			.output()?;

		if !output.status.success() {
			return Err(anyhow::anyhow!("Failed to add files: {}",
				String::from_utf8_lossy(&output.stderr)));
		}
	}

	// Check if there are staged changes
	let output = Command::new("git")
		.args(&["diff", "--cached", "--name-only"])
		.current_dir(&current_dir)
		.output()?;

	if !output.status.success() {
		return Err(anyhow::anyhow!("Failed to check staged changes: {}",
			String::from_utf8_lossy(&output.stderr)));
	}

	let staged_files = String::from_utf8(output.stdout)?;
	if staged_files.trim().is_empty() {
		return Err(anyhow::anyhow!("❌ No staged changes to commit. Use 'git add' or --all flag."));
	}

	println!("📋 Staged files:");
	for file in staged_files.lines() {
		println!("  • {}", file);
	}

	// Generate commit message using AI (always, but with optional context)
	println!("\n🤖 Generating commit message...");
	let commit_message = generate_commit_message(&current_dir, config, args.message.as_deref()).await?;

	println!("\n📝 Generated commit message:");
	println!("═══════════════════════════════════");
	println!("{}", commit_message);
	println!("═══════════════════════════════════");

	// Confirm with user (unless --yes flag is used)
	if !args.yes {
		print!("\nProceed with this commit? [y/N] ");
		io::stdout().flush()?;

		let mut input = String::new();
		io::stdin().read_line(&mut input)?;

		if !input.trim().to_lowercase().starts_with('y') {
			println!("❌ Commit cancelled.");
			return Ok(());
		}
	}

	// Perform the commit
	println!("💾 Committing changes...");
	let output = Command::new("git")
		.args(&["commit", "-m", &commit_message])
		.current_dir(&current_dir)
		.output()?;

	if !output.status.success() {
		return Err(anyhow::anyhow!("Failed to commit: {}",
			String::from_utf8_lossy(&output.stderr)));
	}

	println!("✅ Successfully committed changes!");

	// Show commit info
	let output = Command::new("git")
		.args(&["log", "--oneline", "-1"])
		.current_dir(&current_dir)
		.output()?;

	if output.status.success() {
		let commit_info = String::from_utf8_lossy(&output.stdout);
		println!("📄 Commit: {}", commit_info.trim());
	}

	Ok(())
}

async fn generate_commit_message(repo_path: &std::path::Path, config: &Config, extra_context: Option<&str>) -> Result<String> {
	// Get the diff of staged changes
	let output = Command::new("git")
		.args(&["diff", "--cached"])
		.current_dir(repo_path)
		.output()?;

	if !output.status.success() {
		return Err(anyhow::anyhow!("Failed to get diff: {}",
			String::from_utf8_lossy(&output.stderr)));
	}

	let diff = String::from_utf8(output.stdout)?;

	if diff.trim().is_empty() {
		return Err(anyhow::anyhow!("No staged changes found"));
	}

	// Count files and changes
	let file_count = diff.matches("diff --git").count();
	let additions = diff.matches("\n+").count().saturating_sub(diff.matches("\n+++").count());
	let deletions = diff.matches("\n-").count().saturating_sub(diff.matches("\n---").count());

	// Build the context section
	let mut context_section = String::new();
	if let Some(context) = extra_context {
		context_section = format!("\n\nUser commit message:\n{}", context);
	}

	// Prepare the enhanced prompt for the LLM
	let prompt = format!(
		"Create a Git commit message from this diff. Be specific and concise.\n\n\
		STRICT RULES:\n\
		- Format: type(scope): description (under 50 chars)\n\
		- Types: feat, fix, docs, style, refactor, test, chore, perf, ci, build\n\
		- Be specific, avoid generic words like \"update\", \"change\", \"modify\", \"various\", \"several\"\n\
		- Use imperative mood: \"add\" not \"added\"\n\
		- Focus on WHAT functionality changed, not implementation details\n\
		- If user message provided, align with that intent{}\n\n\
		BODY RULES (add body with bullet points if ANY of these apply):\n\
		- 4+ files changed OR 25+ lines changed\n\
		- Multiple different types of changes (feat+fix, refactor+feat, etc.)\n\
		- Complex refactoring or architectural changes\n\
		- Breaking changes or major feature additions\n\
		- Changes affect multiple modules/components\n\n\
		Body format when needed:\n\
		- Blank line after subject\n\
		- Start each point with \"- \"\n\
		- Focus on key changes and their purpose\n\
		- Explain WHY if not obvious from subject\n\
		- Keep each bullet concise (1 line max)\n\n\
		Changes: {} files (+{} -{} lines)\n\n\
		Git diff:\n\
		```\n{}\n```\n\n\
		Generate commit message:",
		context_section,
		file_count,
		additions,
		deletions,
		// Truncate diff if it's too long (keep first 4000 chars for better analysis)
		if diff.len() > 4000 {
			format!("{}...\n[diff truncated for brevity]", &diff[..4000])
		} else {
			diff
		}
	);

	// Call the LLM using existing infrastructure
	match call_llm_for_commit_message(&prompt, config).await {
		Ok(message) => {
			// Clean up the response but preserve multi-line structure
			let cleaned = message
				.trim()
				.trim_matches('"') // Remove quotes if present
				.trim();

			// Validate the message
			if cleaned.is_empty() {
				Ok("chore: update files".to_string())
			} else {
				// Split into lines and validate subject line length
				let lines: Vec<&str> = cleaned.lines().collect();
				if let Some(subject) = lines.first() {
					let subject = subject.trim();
					if subject.len() > 72 {
						// Truncate subject if too long but keep body if present
						let truncated_subject = format!("{}...", &subject[..69]);
						if lines.len() > 1 {
							let body = lines[1..].join("\n");
							Ok(format!("{}\n{}", truncated_subject, body))
						} else {
							Ok(truncated_subject)
						}
					} else {
						Ok(cleaned.to_string())
					}
				} else {
					Ok("chore: update files".to_string())
				}
			}
		},
		Err(e) => {
			eprintln!("Warning: LLM call failed ({}), using fallback", e);
			Ok("chore: update files".to_string())
		}
	}
}

async fn call_llm_for_commit_message(prompt: &str, config: &Config) -> Result<String> {
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

	// Prepare the request
	let payload = json!({
		"model": config.openrouter.model,
		"messages": [
			{
				"role": "user",
				"content": prompt
			}
		],
		"temperature": 0.1,
		"max_tokens": 180
	});

	let response = client
		.post(&format!("{}/chat/completions", config.openrouter.base_url.trim_end_matches('/')))
		.header("Authorization", format!("Bearer {}", api_key))
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
