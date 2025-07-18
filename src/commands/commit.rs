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
use std::io::{self, Write};
use std::process::Command;

use octocode::config::Config;
use octocode::indexer::git_utils::GitUtils;

#[derive(Args, Debug)]
pub struct CommitArgs {
	/// Add all changes before committing
	#[arg(short, long)]
	pub all: bool,

	/// Additional context to help AI generate better commit message (guidance, not the base message)
	#[arg(short, long)]
	pub message: Option<String>,

	/// Skip confirmation prompt
	#[arg(short, long)]
	pub yes: bool,

	/// Skip pre-commit hooks and commit-msg hooks
	/// Note: Pre-commit hooks run automatically if pre-commit binary and config are detected
	#[arg(short, long)]
	pub no_verify: bool,
}

/// Execute the commit command with intelligent pre-commit hook integration.
///
/// Pre-commit hooks are automatically detected and run if:
/// - The `pre-commit` binary is available in PATH
/// - A `.pre-commit-config.yaml` or `.pre-commit-config.yml` file exists
/// - The `--no-verify` flag is not used
///
/// When `--all` is specified, pre-commit runs with `--all-files`.
/// Otherwise, it runs only on staged files (default behavior).
///
/// If pre-commit modifies files, they are automatically re-staged before
/// generating the commit message with AI.
pub async fn execute(config: &Config, args: &CommitArgs) -> Result<()> {
	let current_dir = std::env::current_dir()?;

	// Find git repository root
	let git_root = GitUtils::find_git_root(&current_dir)
		.ok_or_else(|| anyhow::anyhow!("❌ Not in a git repository!"))?;

	// Use git root as working directory for all operations
	let current_dir = git_root;

	// Add all files if requested
	if args.all {
		println!("📂 Adding all changes...");
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
			"❌ No staged changes to commit. Use 'git add' or --all flag."
		));
	}

	println!("📋 Staged files:");
	for file in staged_files.lines() {
		println!("  • {}", file);
	}

	// Run pre-commit hooks if available and not skipped
	if !args.no_verify {
		let originally_staged_files: Vec<String> =
			staged_files.lines().map(|s| s.to_string()).collect();
		run_precommit_hooks(&current_dir, args.all, &originally_staged_files).await?;
	}

	// Check staged changes again after pre-commit (files might have been modified)
	let output = Command::new("git")
		.args(["diff", "--cached", "--name-only"])
		.current_dir(&current_dir)
		.output()?;

	if !output.status.success() {
		return Err(anyhow::anyhow!(
			"Failed to check staged changes after pre-commit: {}",
			String::from_utf8_lossy(&output.stderr)
		));
	}

	let final_staged_files = String::from_utf8(output.stdout)?;
	if final_staged_files.trim().is_empty() {
		return Err(anyhow::anyhow!(
			"❌ No staged changes remaining after pre-commit hooks."
		));
	}

	// Show updated staged files if they changed
	if final_staged_files != staged_files {
		println!("\n📋 Updated staged files after pre-commit:");
		for file in final_staged_files.lines() {
			println!("  • {}", file);
		}
	}

	// Generate commit message using AI (always, but with optional context)
	println!("\n🤖 Generating commit message...");
	let commit_message =
		generate_commit_message(&current_dir, config, args.message.as_deref()).await?;

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
	let mut git_args = vec!["commit", "-m", &commit_message];
	if args.no_verify {
		git_args.push("--no-verify");
	}
	let output = Command::new("git")
		.args(&git_args)
		.current_dir(&current_dir)
		.output()?;

	if !output.status.success() {
		return Err(anyhow::anyhow!(
			"Failed to commit: {}",
			String::from_utf8_lossy(&output.stderr)
		));
	}

	println!("✅ Successfully committed changes!");

	// Show commit info
	let output = Command::new("git")
		.args(["log", "--oneline", "-1"])
		.current_dir(&current_dir)
		.output()?;

	if output.status.success() {
		let commit_info = String::from_utf8_lossy(&output.stdout);
		println!("📄 Commit: {}", commit_info.trim());
	}

	Ok(())
}

async fn generate_commit_message(
	repo_path: &std::path::Path,
	config: &Config,
	extra_context: Option<&str>,
) -> Result<String> {
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

	// Get list of staged files to analyze extensions
	let staged_files = GitUtils::get_staged_files(repo_path)?;
	let changed_files = staged_files.join("\n");

	// Analyze file extensions
	let has_markdown_files = changed_files
		.lines()
		.any(|file| file.ends_with(".md") || file.ends_with(".markdown") || file.ends_with(".rst"));

	let has_non_markdown_files = changed_files.lines().any(|file| {
		!file.ends_with(".md")
			&& !file.ends_with(".markdown")
			&& !file.ends_with(".rst")
			&& !file.trim().is_empty()
	});

	// Count files and changes
	let file_count = diff.matches("diff --git").count();
	let additions = diff
		.matches("\n+")
		.count()
		.saturating_sub(diff.matches("\n+++").count());
	let deletions = diff
		.matches("\n-")
		.count()
		.saturating_sub(diff.matches("\n---").count());

	// Build the guidance section
	let mut guidance_section = String::new();
	if let Some(context) = extra_context {
		guidance_section = format!("\n\nUser guidance for commit intent:\n{}", context);
	}

	// Build docs type restriction based on file analysis
	let docs_restriction = if has_non_markdown_files && !has_markdown_files {
		// Only non-markdown files changed - explicitly forbid docs
		"\n\nCRITICAL - DOCS TYPE RESTRICTION:\n\
		- NEVER use 'docs(...)' when only non-markdown files are changed\n\
		- Current changes include ONLY non-markdown files (.rs, .js, .py, .toml, etc.)\n\
		- Use 'fix', 'feat', 'refactor', 'chore', etc. instead of 'docs'\n\
		- 'docs' is ONLY for .md, .markdown, .rst files or documentation-only changes"
	} else if has_non_markdown_files && has_markdown_files {
		// Mixed files - provide guidance
		"\n\nDOCS TYPE GUIDANCE:\n\
		- Use 'docs(...)' ONLY if the primary change is documentation\n\
		- If code changes are the main focus, use appropriate code type (fix, feat, refactor)\n\
		- Mixed changes: prioritize the most significant change type"
	} else {
		// Only markdown files or no files detected - allow docs
		""
	};

	// Prepare the enhanced prompt for the LLM
	let prompt = format!(
		"Analyze this Git diff and create an appropriate commit message. Be specific and concise.\n\n\
		STRICT FORMATTING RULES:\n\
		- Format: type(scope): description (under 50 chars)\n\
		- Types: feat, fix, docs, style, refactor, test, chore, perf, ci, build\n\
		- Add '!' after type for breaking changes: feat!: or fix!:\n\
		- Be specific, avoid generic words like \"update\", \"change\", \"modify\", \"various\", \"several\"\n\
		- Use imperative mood: \"add\" not \"added\", \"fix\" not \"fixed\"\n\
		- Focus on WHAT functionality changed, not implementation details\n\
		- If user guidance provided, use it to understand the INTENT but create your own message{}\n\n\
		COMMIT TYPE SELECTION (READ CAREFULLY):\n\
		- feat: NEW functionality being added (new features, capabilities, commands)\n\
		- fix: CORRECTING bugs, errors, or broken functionality (including fixes to existing features)\n\
		- refactor: IMPROVING existing code without changing functionality (code restructuring)\n\
		- perf: OPTIMIZING performance without adding features\n\
		- docs: DOCUMENTATION changes ONLY (.md, .markdown, .rst files)\n\
		- test: ADDING or fixing tests\n\
		- style: CODE formatting, whitespace, missing semicolons (no logic changes)\n\
		- chore: MAINTENANCE tasks (dependencies, build, tooling, config)\n\
		- ci: CONTINUOUS integration changes (workflows, pipelines)\n\
		- build: BUILD system changes (Cargo.toml, package.json, Makefile){}\n\n\
		FEATURE vs FIX DECISION GUIDE:\n\
		- If code was working but had bugs/errors → use 'fix' (even for new features with bugs)\n\
		- If adding completely new functionality that didn't exist → use 'feat'\n\
		- If improving existing working code structure → use 'refactor' or 'perf'\n\
		- Examples: 'fix(auth): resolve token validation error', 'feat(auth): add OAuth2 support'\n\
		- When fixing issues in recently added features → use 'fix(scope): correct feature-name issue'\n\
		- When in doubt between feat/fix: choose 'fix' if addressing problems, 'feat' if adding completely new\n\n\
		BREAKING CHANGE DETECTION:\n\
		- Look for function signature changes, API modifications, removed public methods\n\
		- Check for interface/trait changes, configuration schema changes\n\
		- Identify database migrations, dependency version bumps with breaking changes\n\
		- If breaking changes detected, use type! format and add BREAKING CHANGE footer\n\n\
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
		- Keep each bullet concise (1 line max)\n\
		- For breaking changes, add footer: \"BREAKING CHANGE: description\"\n\n\
		Changes: {} files (+{} -{} lines)\n\n\
		Git diff:\n\
		```\n{}\n```\n\n\
		Generate commit message:",
		guidance_section,
		docs_restriction,
		file_count,
		additions,
		deletions,
		// Truncate diff if it's too long (keep first 4000 chars for better analysis)
		if diff.chars().count() > 4000 {
			let truncated: String = diff.chars().take(4000).collect();
			format!("{}...\n[diff truncated for brevity]", truncated)
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
						let truncated_subject = if subject.chars().count() > 69 {
							let truncated: String = subject.chars().take(69).collect();
							format!("{}...", truncated)
						} else {
							format!("{}...", subject)
						};
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
		}
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

/// Check if pre-commit binary is available in PATH
fn is_precommit_available() -> bool {
	Command::new("pre-commit")
		.arg("--version")
		.output()
		.map(|output| output.status.success())
		.unwrap_or(false)
}

/// Check if pre-commit is configured in the repository
fn has_precommit_config(repo_path: &std::path::Path) -> bool {
	repo_path.join(".pre-commit-config.yaml").exists()
		|| repo_path.join(".pre-commit-config.yml").exists()
}

/// Run pre-commit hooks intelligently based on the situation
async fn run_precommit_hooks(
	repo_path: &std::path::Path,
	run_all: bool,
	originally_staged_files: &[String],
) -> Result<()> {
	// Check if pre-commit is available and configured
	if !is_precommit_available() {
		// No pre-commit binary available, skip silently
		return Ok(());
	}

	if !has_precommit_config(repo_path) {
		// No pre-commit config found, skip silently
		return Ok(());
	}

	println!("🔧 Running pre-commit hooks...");

	// Determine which pre-commit command to run
	let pre_commit_args = if run_all {
		// When --all flag is used, run on all files
		vec!["run", "--all-files"]
	} else {
		// Run only on staged files (default pre-commit behavior)
		vec!["run"]
	};

	let output = Command::new("pre-commit")
		.args(&pre_commit_args)
		.current_dir(repo_path)
		.output()?;

	// Pre-commit can return non-zero exit codes for various reasons:
	// - Code 0: All hooks passed
	// - Code 1: Some hooks failed or made changes
	// - Code 3: No hooks to run
	match output.status.code() {
		Some(0) => {
			println!("✅ Pre-commit hooks passed successfully");
		}
		Some(1) => {
			// Hooks made changes or failed
			let stderr = String::from_utf8_lossy(&output.stderr);
			let stdout = String::from_utf8_lossy(&output.stdout);

			if !stdout.is_empty() {
				println!("📝 Pre-commit output:\n{}", stdout);
			}

			// Check if files were modified by pre-commit
			let modified_output = Command::new("git")
				.args(["diff", "--name-only"])
				.current_dir(repo_path)
				.output()?;

			if modified_output.status.success() {
				let all_modified_files = String::from_utf8_lossy(&modified_output.stdout);
				let all_modified_set: std::collections::HashSet<&str> =
					all_modified_files.lines().collect();

				// Only consider files that were originally staged AND were modified by pre-commit
				let staged_and_modified: Vec<&String> = originally_staged_files
					.iter()
					.filter(|file| all_modified_set.contains(file.as_str()))
					.collect();

				if !staged_and_modified.is_empty() {
					println!("🔄 Pre-commit hooks modified originally staged files:");
					for file in &staged_and_modified {
						println!("  • {}", file);
					}

					// Re-add modified files to staging area
					println!("📂 Re-staging modified files...");
					for file in &staged_and_modified {
						let add_output = Command::new("git")
							.args(["add", file.trim()])
							.current_dir(repo_path)
							.output()?;

						if !add_output.status.success() {
							eprintln!(
								"⚠️  Warning: Failed to re-stage {}: {}",
								file,
								String::from_utf8_lossy(&add_output.stderr)
							);
						}
					}
					println!("✅ Modified files re-staged successfully");
				}
			}

			// If there were actual failures (not just modifications), show them
			if !stderr.is_empty() && stderr.contains("FAILED") {
				println!("⚠️  Some pre-commit hooks failed:\n{}", stderr);
				// Don't fail the commit process, let user decide
			}
		}
		Some(3) => {
			println!("ℹ️  No pre-commit hooks configured to run");
		}
		Some(code) => {
			let stderr = String::from_utf8_lossy(&output.stderr);
			println!("⚠️  Pre-commit exited with code {}: {}", code, stderr);
			// Don't fail the commit process for other exit codes
		}
		None => {
			println!("⚠️  Pre-commit was terminated by signal");
		}
	}

	Ok(())
}
