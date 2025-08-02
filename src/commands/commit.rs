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
use tokio::task::JoinSet;

use octocode::config::Config;
use octocode::indexer::git_utils::GitUtils;
use octocode::utils::diff_chunker;

/// Retry configuration for failed chunk processing
const MAX_RETRIES: usize = 2;
const RETRY_DELAY_MS: u64 = 1000;

/// Retry wrapper for LLM calls with exponential backoff
async fn call_llm_with_retry<F, Fut>(operation: F, context: &str) -> Result<String>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Result<String>>,
{
	let mut last_error = None;

	for attempt in 1..=MAX_RETRIES + 1 {
		match operation().await {
			Ok(response) => return Ok(response),
			Err(e) => {
				last_error = Some(e);
				if attempt <= MAX_RETRIES {
					eprintln!(
						"Warning: {} attempt {} failed, retrying in {}ms...",
						context, attempt, RETRY_DELAY_MS
					);
					tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS)).await;
				}
			}
		}
	}

	// All retries failed
	if let Some(e) = last_error {
		Err(anyhow::anyhow!(
			"{} failed after {} attempts: {}",
			context,
			MAX_RETRIES + 1,
			e
		))
	} else {
		Err(anyhow::anyhow!("{} failed with unknown error", context))
	}
}

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

	// Generate commit message using AI with intelligent chunking for large diffs
	println!("\n🤖 Generating commit message...");
	let commit_message =
		generate_commit_message_chunked(&current_dir, config, args.message.as_deref()).await?;

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

async fn generate_commit_message_chunked(
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

	// Check if we need to chunk the diff
	let chunks = diff_chunker::chunk_diff(&diff);

	if chunks.len() == 1 {
		// Single chunk - use existing logic
		let prompt = create_commit_prompt(
			&chunks[0].content,
			file_count,
			additions,
			deletions,
			&guidance_section,
			docs_restriction,
		);
		return call_llm_with_retry(
			|| call_llm_for_commit_message(&prompt, config),
			"Single chunk commit message",
		)
		.await;
	}

	// Multiple chunks - process in parallel for better performance
	println!(
		"📝 Processing large diff in {} chunks in parallel...",
		chunks.len()
	);

	let responses = process_commit_chunks_parallel(
		&chunks,
		file_count,
		additions,
		deletions,
		&guidance_section,
		docs_restriction,
		config,
	)
	.await;

	if responses.is_empty() {
		return Ok("chore: update files".to_string());
	}

	// Combine responses into final commit message
	let combined = diff_chunker::combine_commit_messages(responses);

	// Always apply AI refinement for chunked messages
	println!("🎯 Refining commit message with AI...");
	match refine_commit_message_with_ai(&combined, config).await {
		Ok(refined) => Ok(refined),
		Err(e) => {
			eprintln!(
				"Warning: AI refinement failed ({}), using combined message",
				e
			);
			Ok(combined)
		}
	}
}

/// Create a standardized commit prompt for LLM processing
fn create_commit_prompt(
	diff_content: &str,
	file_count: usize,
	additions: usize,
	deletions: usize,
	guidance_section: &str,
	docs_restriction: &str,
) -> String {
	format!(
		"Analyze this Git diff and create an appropriate commit message. Be specific and concise.\\n\\n\\\
		STRICT FORMATTING RULES:\\n\\\
		- Format: type(scope): description (under 50 chars)\\n\\\
		- Types: feat, fix, docs, style, refactor, test, chore, perf, ci, build\\n\\\
		- Add '!' after type for breaking changes: feat!: or fix!:\\n\\\
		- Be specific, avoid generic words like \\\"update\\\", \\\"change\\\", \\\"modify\\\", \\\"various\\\", \\\"several\\\"\\n\\\
		- Use imperative mood: \\\"add\\\" not \\\"added\\\", \\\"fix\\\" not \\\"fixed\\\"\\n\\\
		- Focus on WHAT functionality changed, not implementation details\\n\\\
		- If user guidance provided, use it to understand the INTENT but create your own message{}\\n\\n\\\
		COMMIT TYPE SELECTION (READ CAREFULLY):\\n\\\
		- feat: NEW functionality being added (new features, capabilities, commands)\\n\\\
		- fix: CORRECTING bugs, errors, or broken functionality (including fixes to existing features)\\n\\\
		- refactor: IMPROVING existing code without changing functionality (code restructuring)\\n\\\
		- perf: OPTIMIZING performance without adding features\\n\\\
		- docs: DOCUMENTATION changes ONLY (.md, .markdown, .rst files)\\n\\\
		- test: ADDING or fixing tests\\n\\\
		- style: CODE formatting, whitespace, missing semicolons (no logic changes)\\n\\\
		- chore: MAINTENANCE tasks (dependencies, build, tooling, config)\\n\\\
		- ci: CONTINUOUS integration changes (workflows, pipelines)\\n\\\
		- build: BUILD system changes (Cargo.toml, package.json, Makefile){}\\n\\n\\\
		FEATURE vs FIX DECISION GUIDE:\\n\\\
		- If code was working but had bugs/errors → use 'fix' (even for new features with bugs)\\n\\\
		- If adding completely new functionality that didn't exist → use 'feat'\\n\\\
		- If improving existing working code structure → use 'refactor' or 'perf'\\n\\\
		- Examples: 'fix(auth): resolve token validation error', 'feat(auth): add OAuth2 support'\\n\\\
		- When fixing issues in recently added features → use 'fix(scope): correct feature-name issue'\\n\\\
		- When in doubt between feat/fix: choose 'fix' if addressing problems, 'feat' if adding completely new\\n\\n\\\
		BREAKING CHANGE DETECTION:\\n\\\
		- Look for function signature changes, API modifications, removed public methods\\n\\\
		- Check for interface/trait changes, configuration schema changes\\n\\\
		- Identify database migrations, dependency version bumps with breaking changes\\n\\\
		- If breaking changes detected, use type! format and add BREAKING CHANGE footer\\n\\n\\\
		BODY RULES (add body with bullet points if ANY of these apply):\\n\\\
		- 4+ files changed OR 25+ lines changed\\n\\\
		- Multiple different types of changes (feat+fix, refactor+feat, etc.)\\n\\\
		- Complex refactoring or architectural changes\\n\\\
		- Breaking changes or major feature additions\\n\\\
		- Changes affect multiple modules/components\\n\\n\\\
		Body format when needed:\\n\\\
		- Blank line after subject\\n\\\
		- Start each point with \\\"- \\\"\\n\\\
		- Focus on key changes and their purpose\\n\\\
		- Explain WHY if not obvious from subject\\n\\\
		- Keep each bullet concise (1 line max)\\n\\\
		- For breaking changes, add footer: \\\"BREAKING CHANGE: description\\\"\\n\\n\\\
		Changes: {} files (+{} -{} lines)\\n\\n\\\
		Git diff:\\n\\\
		```\\n{}\\n```\\n\\n\\\
		Generate commit message:",
		guidance_section,
		docs_restriction,
		file_count,
		additions,
		deletions,
		diff_content
	)
}

/// Collect responses from parallel tasks maintaining original order
///
/// Waits for all parallel tasks to complete and collects successful responses.
/// Maintains the original chunk order for coherent result combination.
/// Handles task failures gracefully with appropriate logging.
///
/// # Arguments
/// * `join_set` - Set of spawned async tasks
/// * `expected_count` - Number of responses expected
///
/// # Returns
/// Vector of successful responses in original order
async fn collect_ordered_responses(
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

/// Process chunks in parallel with proper error handling and resource limits
///
/// Processes multiple diff chunks concurrently using tokio tasks for improved performance.
/// Implements resource limits to prevent system overload and maintains result ordering.
///
/// # Arguments
/// * `chunks` - Array of diff chunks to process
/// * `file_count` - Total number of files changed (for context)
/// * `additions` - Total lines added (for context)
/// * `deletions` - Total lines deleted (for context)
/// * `guidance_section` - User-provided guidance for commit message
/// * `docs_restriction` - Documentation type restrictions
/// * `config` - Application configuration
///
/// # Returns
/// Vector of successful LLM responses in original chunk order
async fn process_commit_chunks_parallel(
	chunks: &[diff_chunker::DiffChunk],
	file_count: usize,
	additions: usize,
	deletions: usize,
	guidance_section: &str,
	docs_restriction: &str,
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
		let guidance_section = guidance_section.to_string();
		let docs_restriction = docs_restriction.to_string();

		join_set.spawn(async move {
			println!(
				"  Processing chunk {}/{}: {}",
				i + 1,
				chunk_limit,
				chunk_summary
			);

			let chunk_prompt = create_commit_prompt(
				&chunk_content,
				file_count,
				additions,
				deletions,
				&guidance_section,
				&docs_restriction,
			);

			match call_llm_for_commit_message(&chunk_prompt, &config).await {
				Ok(response) => Ok((i, response)),
				Err(e) => {
					eprintln!("Warning: Chunk {} failed ({})", i + 1, e);
					Err(e)
				}
			}
		});
	}

	// Collect results maintaining order
	collect_ordered_responses(join_set, chunk_limit).await
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
		"max_tokens": 300
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

/// Refine a verbose commit message using AI to create a concise, deduplicated version
///
/// Takes a combined commit message from multiple chunks and uses AI to:
/// - Remove duplication and redundancy
/// - Create concise bullet points
/// - Maintain proper conventional commit format
/// - Preserve important technical details
///
/// # Arguments
/// * `verbose_message` - The combined verbose commit message from chunks
/// * `config` - Application configuration
///
/// # Returns
/// A refined, concise commit message
async fn refine_commit_message_with_ai(verbose_message: &str, config: &Config) -> Result<String> {
	let refinement_prompt = format!(
		r#"You are an expert at creating concise, professional commit messages.

I have a verbose commit message that was generated by combining multiple chunks of a large diff. Your task is to refine it into a clean, concise commit message that follows conventional commit format.

ORIGINAL VERBOSE MESSAGE:
{}

REFINEMENT REQUIREMENTS:
1. Keep the conventional commit format: type(scope): description
2. Choose the MOST APPROPRIATE single type (feat, fix, refactor, chore, docs, etc.)
3. Remove ALL duplication and redundancy
4. Create concise bullet points for the body (if needed)
5. Focus on WHAT changed, not implementation details
6. Maximum 50 characters for subject line
7. Maximum 72 characters per body line
8. Group related changes together
9. Remove verbose explanations - keep it factual and brief

EXAMPLE OUTPUT FORMAT:
refactor(diff_chunker): improve chunking with limits and robustness

- Add resource limits to prevent exhaustion
- Enhance filename extraction accuracy
- Improve error handling and logging
- Add comprehensive test coverage

Return ONLY the refined commit message, nothing else."#,
		verbose_message
	);

	call_llm_with_retry(
		|| call_llm_for_commit_message(&refinement_prompt, config),
		"AI commit message refinement",
	)
	.await
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
