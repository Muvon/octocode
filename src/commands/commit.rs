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

	/// Rewrite the message of an existing commit by its hash.
	/// Uses that commit's diff to generate a new message, then replaces it in-place.
	/// HEAD commits are amended directly; older commits are rewritten via git rebase.
	#[arg(short, long, value_name = "HASH")]
	pub commit: Option<String>,
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

	// -c/--commit mode: rewrite an existing commit's message
	if let Some(ref hash) = args.commit {
		return rewrite_commit_message(
			&current_dir,
			config,
			hash,
			args.message.as_deref(),
			args.yes,
		)
		.await;
	}

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

/// Rewrite the commit message of an existing commit identified by `hash`.
///
/// - If the hash resolves to HEAD, uses `git commit --amend` (fast, no rebase needed).
/// - Otherwise, uses a non-interactive `git rebase -i` with a custom sequence editor
///   that marks only the target commit as `reword`, leaving all others as `pick`.
///   The new message is injected via `GIT_EDITOR` pointing to a temp script.
async fn rewrite_commit_message(
	repo_path: &std::path::Path,
	config: &Config,
	hash: &str,
	extra_context: Option<&str>,
	skip_confirm: bool,
) -> Result<()> {
	// Resolve the hash to a full SHA so we can compare with HEAD
	let resolved = Command::new("git")
		.args(["rev-parse", hash])
		.current_dir(repo_path)
		.output()?;
	if !resolved.status.success() {
		return Err(anyhow::anyhow!(
			"❌ Cannot resolve commit '{}': {}",
			hash,
			String::from_utf8_lossy(&resolved.stderr).trim()
		));
	}
	let full_hash = String::from_utf8(resolved.stdout)?.trim().to_string();

	// Resolve HEAD for comparison
	let head = Command::new("git")
		.args(["rev-parse", "HEAD"])
		.current_dir(repo_path)
		.output()?;
	let head_hash = if head.status.success() {
		String::from_utf8(head.stdout)?.trim().to_string()
	} else {
		String::new()
	};

	let is_head = full_hash == head_hash;

	// Get the diff of the target commit to generate a message from
	println!("🔍 Analysing commit {}...", &full_hash[..8]);
	let diff_output = Command::new("git")
		.args(["show", &full_hash, "--format=", "-p"])
		.current_dir(repo_path)
		.output()?;
	if !diff_output.status.success() {
		return Err(anyhow::anyhow!(
			"❌ Failed to get diff for commit '{}': {}",
			hash,
			String::from_utf8_lossy(&diff_output.stderr).trim()
		));
	}
	let diff = String::from_utf8(diff_output.stdout)?;
	if diff.trim().is_empty() {
		return Err(anyhow::anyhow!(
			"❌ Commit '{}' has no diff (merge commit or empty commit).",
			hash
		));
	}

	// Show files touched by this commit
	let files_output = Command::new("git")
		.args(["show", &full_hash, "--name-only", "--format="])
		.current_dir(repo_path)
		.output()?;
	if files_output.status.success() {
		let files = String::from_utf8_lossy(&files_output.stdout);
		println!("📋 Files in commit:");
		for f in files.lines().filter(|l| !l.is_empty()) {
			println!("  • {}", f);
		}
	}

	// Generate new commit message from the commit's own diff
	println!("\n🤖 Generating commit message...");
	let commit_message = generate_commit_message_from_diff(&diff, config, extra_context).await?;

	println!("\n📝 Generated commit message:");
	println!("═══════════════════════════════════");
	println!("{}", commit_message);
	println!("═══════════════════════════════════");

	if !skip_confirm {
		print!("\nReplace message of commit {}? [y/N] ", &full_hash[..8]);
		io::stdout().flush()?;
		let mut input = String::new();
		io::stdin().read_line(&mut input)?;
		if !input.trim().to_lowercase().starts_with('y') {
			println!("❌ Cancelled.");
			return Ok(());
		}
	}

	if is_head {
		// Fast path: just amend HEAD
		println!("✏️  Amending HEAD commit message...");
		let output = Command::new("git")
			.args(["commit", "--amend", "-m", &commit_message, "--no-edit"])
			.current_dir(repo_path)
			.output()?;
		if !output.status.success() {
			return Err(anyhow::anyhow!(
				"Failed to amend commit: {}",
				String::from_utf8_lossy(&output.stderr).trim()
			));
		}
	} else {
		// Non-HEAD: rewrite via rebase using a temp editor script that injects the message
		println!("✏️  Rewriting commit {} via rebase...", &full_hash[..8]);

		// Write the new message to a temp file so the editor script can read it
		let msg_file = repo_path.join(".git").join("OCTOCODE_REWORD_MSG");
		std::fs::write(&msg_file, &commit_message)?;

		// Editor script: replaces the COMMIT_EDITMSG content with our message
		// git rebase -i calls $GIT_EDITOR with the path to the commit message file
		let editor_script = format!("#!/bin/sh\ncp '{}' \"$1\"", msg_file.display());
		let editor_file = repo_path.join(".git").join("octocode_reword_editor.sh");
		std::fs::write(&editor_file, &editor_script)?;
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			std::fs::set_permissions(&editor_file, std::fs::Permissions::from_mode(0o755))?;
		}

		// Sequence editor: marks only our target commit as 'reword', rest stay 'pick'
		let seq_script = format!(
			"#!/bin/sh\nsed -i.bak 's/^pick {}/reword {}/' \"$1\"",
			&full_hash[..7],
			&full_hash[..7]
		);
		let seq_file = repo_path.join(".git").join("octocode_seq_editor.sh");
		std::fs::write(&seq_file, &seq_script)?;
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			std::fs::set_permissions(&seq_file, std::fs::Permissions::from_mode(0o755))?;
		}

		let output = Command::new("git")
			.args(["rebase", "-i", &format!("{}^", full_hash)])
			.env("GIT_SEQUENCE_EDITOR", &seq_file)
			.env("GIT_EDITOR", &editor_file)
			.current_dir(repo_path)
			.output()?;

		// Clean up temp files regardless of outcome
		let _ = std::fs::remove_file(&msg_file);
		let _ = std::fs::remove_file(&editor_file);
		let _ = std::fs::remove_file(&seq_file);
		let _ = std::fs::remove_file(repo_path.join(".git").join("octocode_reword_editor.sh.bak"));
		let _ = std::fs::remove_file(repo_path.join(".git").join("octocode_seq_editor.sh.bak"));

		if !output.status.success() {
			return Err(anyhow::anyhow!(
				"Failed to rebase: {}",
				String::from_utf8_lossy(&output.stderr).trim()
			));
		}
	}

	println!("✅ Commit message replaced successfully!");

	// Show updated commit info (after rebase the old hash is gone, show latest)
	let log = Command::new("git")
		.args(["log", "--oneline", "-1"])
		.current_dir(repo_path)
		.output()?;
	if log.status.success() {
		println!("📄 Commit: {}", String::from_utf8_lossy(&log.stdout).trim());
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

/// Generate a commit message from a pre-fetched diff string (used by -c/--commit rewrite mode).
///
/// Shares all prompt-building and chunking logic with `generate_commit_message_chunked`
/// but accepts the diff directly instead of reading staged changes.
async fn generate_commit_message_from_diff(
	diff: &str,
	config: &Config,
	extra_context: Option<&str>,
) -> Result<String> {
	// Derive file list from the diff headers (no git staging involved)
	let changed_files: String = diff
		.lines()
		.filter(|l| l.starts_with("diff --git "))
		.filter_map(|l| l.split(" b/").nth(1))
		.collect::<Vec<_>>()
		.join("\n");

	let has_markdown_files = changed_files
		.lines()
		.any(|f| f.ends_with(".md") || f.ends_with(".markdown") || f.ends_with(".rst"));

	let has_non_markdown_files = changed_files.lines().any(|f| {
		!f.ends_with(".md")
			&& !f.ends_with(".markdown")
			&& !f.ends_with(".rst")
			&& !f.trim().is_empty()
	});

	let file_count = diff.matches("diff --git").count();
	let additions = diff
		.matches("\n+")
		.count()
		.saturating_sub(diff.matches("\n+++").count());
	let deletions = diff
		.matches("\n-")
		.count()
		.saturating_sub(diff.matches("\n---").count());

	let guidance_section = extra_context
		.map(|c| format!("\n\nUser guidance for commit intent:\n{}", c))
		.unwrap_or_default();

	let docs_restriction = if has_non_markdown_files && !has_markdown_files {
		"\n\nCRITICAL - DOCS TYPE RESTRICTION:\n\
		- NEVER use 'docs(...)' when only non-markdown files are changed\n\
		- Current changes include ONLY non-markdown files (.rs, .js, .py, .toml, etc.)\n\
		- Use 'fix', 'feat', 'refactor', 'chore', etc. instead of 'docs'\n\
		- 'docs' is ONLY for .md, .markdown, .rst files or documentation-only changes"
	} else if has_non_markdown_files && has_markdown_files {
		"\n\nDOCS TYPE GUIDANCE:\n\
		- Use 'docs(...)' ONLY if the primary change is documentation\n\
		- If code changes are the main focus, use appropriate code type (fix, feat, refactor)\n\
		- Mixed changes: prioritize the most significant change type"
	} else {
		""
	};

	let chunks = diff_chunker::chunk_diff(diff);

	if chunks.len() == 1 {
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

	let combined = diff_chunker::combine_commit_messages(responses);

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
		"STRICT FORMAT: Plain text commit message, NO markdown, NO backticks, NO code blocks.
type(scope): description under 50 chars
Types: feat, fix, docs, style, refactor, test, chore, perf, ci, build
Use imperative mood (add not added, fix not fixed)
Avoid generic words: update, change, modify, various, several
Focus on WHAT functionality changed, not implementation details{}
---
COMMIT TYPE GUIDE:
feat: NEW functionality being added
fix: CORRECTING bugs/errors/broken functionality
refactor: IMPROVING code without changing functionality
perf: OPTIMIZING performance
docs: .md/.markdown/.rst files ONLY
test: ADDING or fixing tests
style: formatting/whitespace (no logic changes)
chore: maintenance (dependencies, build, tooling)
ci: workflows/pipelines
build: Cargo.toml, package.json, Makefile{}
---
FEATURE vs FIX: Working code with bugs = fix, completely new = feat
Examples: fix(auth): resolve token validation error, feat(auth): add OAuth2 support
When in doubt: choose fix if addressing problems, feat if adding new
---
BREAKING CHANGES: Function/API signature changes, removed public methods, interface/trait changes
Library code: mark any public interface changes as breaking
Application code: internal changes dont need marker unless affects config/user-facing
Use type! format and add BREAKING CHANGE footer if detected
---
BODY NEEDED if: 4+ files OR 25+ lines OR multiple change types OR complex refactoring OR breaking changes
Body format (plain text):
- Blank line after subject
- Each point starts with dash space: -
- Focus on key changes and purpose
- Keep bullets concise (1 line max)
- For breaking: add BREAKING CHANGE: description
---
OUTPUT FORMAT: Plain text only
Subject: type(scope): description
If body: blank line then dash bullets
If breaking: BREAKING CHANGE: line
NO code blocks, NO backticks, NO markdown
---
Changes: {} files (+{} -{} lines)

Git diff:
{}

Generate commit message:",
		guidance_section,
		docs_restriction,
		file_count,
		additions,
		deletions,
		diff_content
	)
}
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
	use octocode::llm::{LlmClient, Message};

	// Create LLM client from config
	let client = LlmClient::from_config(config)?;

	// Build messages
	let messages = vec![Message::user(prompt)];

	// Call LLM with low temperature for consistent commit messages
	let response = client
		.chat_completion_with_temperature(messages, 0.1)
		.await?;

	Ok(response)
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
		"SYNTHESISE COMMIT MESSAGE - CRITICAL: Output must be PLAIN TEXT ONLY, NO MARKDOWN, NO backticks, NO code blocks.

The diff was too large to process at once, so it was split into chunks and each chunk was summarised separately.
Below are the per-chunk summaries. Your job is to synthesise them into ONE accurate commit message that covers ALL changes across ALL chunks.

PER-CHUNK SUMMARIES:
{}

SYNTHESIS REQUIREMENTS:
1. Read ALL chunk summaries before writing anything
2. Identify every distinct change across all chunks
3. Choose a single conventional commit type that best represents the overall change set
4. Subject line: type(scope): description — max 50 chars, imperative mood
5. Body: list every meaningful change as a dash-space bullet, one per line, max 72 chars each
6. Group related changes together; remove duplication
7. Do NOT omit changes just because they appear in a later chunk
8. Types: feat, fix, docs, style, refactor, test, chore, perf, ci, build

OUTPUT FORMAT (PLAIN TEXT ONLY):
feat(agents): add multi-jurisdiction lawyer specialists

- Add Australian, Canadian, French, German, Indian, Singaporean, UK, US specialists
- Include federal Acts knowledge base per jurisdiction
- Enable legal query handling for each region

Return ONLY the synthesised commit message as plain text, nothing else.",
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
