// Copyright 2026 Muvon Un Limited
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

use anyhow::{Context, Result};
use clap::Args;
use serde::Deserialize;
use std::io::{self, Write};
use std::process::Command;
use tokio::task::JoinSet;

use octocode::config::Config;
use octocode::indexer::git_utils::GitUtils;
use octocode::llm::{LlmClient, Message};
use octocode::utils::{diff_chunker, plain_line};

/// Retry configuration for failed chunk processing
const MAX_RETRIES: usize = 2;
const RETRY_DELAY_MS: u64 = 1000;

#[cfg(test)]
#[path = "commit_tests.rs"]
mod commit_tests;

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

		// Sequence editor: marks only our target commit as 'reword', rest stay 'pick'.
		// The rebase-todo list abbreviates hashes to whatever length `core.abbrev`
		// resolves to (default "auto", which grows with repo size) — a hardcoded
		// 7-char match can silently fail to match on repos where it's longer, and
		// `sed` still exits 0 with nothing changed. Match any abbreviation length
		// of the target hash, and fail loudly if the substitution didn't land.
		// The trailing-boundary group is required: without it, `^pick abcd` also
		// matches OTHER commits whose abbreviated hash merely starts with those 4
		// chars, flipping them to reword and corrupting the rebase.
		let hash_alternatives: Vec<&str> =
			(4..=full_hash.len()).map(|len| &full_hash[..len]).collect();
		let hash_pattern = hash_alternatives.join("|");
		let seq_script = format!(
			"#!/bin/sh\nsed -E -i.bak 's/^pick ({})([[:space:]]|$)/reword \\1\\2/' \"$1\"\nif ! grep -q '^reword ' \"$1\"; then\n  echo \"octocode: failed to mark {} for reword\" >&2\n  exit 1\nfi",
			hash_pattern, full_hash
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

/// Sampling temperature for commit drafting; low keeps output factual and stable.
const LLM_TEMPERATURE: f32 = 0.1;
/// Hard ceiling for the subject line. The prompt asks for 50; git convention tolerates 72.
const SUBJECT_MAX_CHARS: usize = 72;
/// Body lines are wrapped at the git convention width.
const BODY_WRAP_WIDTH: usize = 72;
const COMMIT_TYPES: [&str; 10] = [
	"feat", "fix", "docs", "style", "refactor", "test", "chore", "perf", "ci", "build",
];

const COMMIT_SYSTEM_PROMPT: &str = "You write git commit messages in the Conventional Commits format. Respond with a single JSON object and nothing else:
{\"type\": string, \"scope\": string, \"subject\": string, \"effect\": string, \"changes\": [string], \"breaking\": string}

type: one of feat, fix, docs, style, refactor, test, chore, perf, ci, build.
  feat = a NEW user-visible capability. fix = corrects broken behavior. refactor = rework with no user-visible change. perf = speed or memory. docs = documentation files only. test = tests only. style = formatting only. chore = maintenance, dependencies, tooling. ci = pipelines. build = build system and manifests.
  Prefer fix or refactor over feat when unsure; feat only when the capability did not exist before.
scope: the module or area touched, short and lowercase, no spaces; empty string when none fits.
subject: imperative mood, lowercase first word, no trailing period, at most 50 characters. Name the behavior that changed; never a vague phrase such as \"update files\".
effect: what now happens differently for a user or caller, stated only from the before/after code in the diff or from the author's description. At most two plain sentences. Empty string when the subject already says it or the diff does not show it. Never state motivation, goals or benefits the diff does not show.
changes: one item per distinct change, only when the diff contains two or more distinct changes. Each item states a behavior or code change in at most 72 characters; never a file name, a line count, or a paraphrase of the subject. Empty array for a single-purpose change.
breaking: one sentence describing an incompatible change to a public API, CLI, config format or data layout; empty string when none.

Accuracy:
- Describe only what the diff shows. Do not infer intent beyond the changed lines.
- Use \"add\", \"introduce\" or \"implement\" only for files with status A or for entirely new functions and types. Edits inside existing code are modifications: extend, rework, fix, update.
- A change to existing functionality is never presented as adding that functionality.
- Plain text in every field: no markdown, no code fences, no headings, no emoji.";

const CHUNK_SYSTEM_PROMPT: &str = "You summarise one part of a git diff that was too large to read whole. Respond with plain text only: one line per distinct change, each starting with \"- \", stating what behavior or code changed and in which file. Use \"add\" only for files with status A or for entirely new functions and types; edits inside existing code are modifications. No commit message, no headings, no markdown, no commentary.";

/// Rounds of draft, audit, redraft-with-feedback before the command gives up.
const MAX_AUDIT_ROUNDS: usize = 2;

const COMMIT_AUDIT_RULES: &str = "The source is a git diff (or a change list compiled from one) with a per-file status list and, optionally, the author's own description. A claim is unsupported when it:
- describes behavior, a feature or a fix that no changed line shows;
- says something was added, introduced or implemented although its file has status M and the diff only edits existing code;
- names a component, option, file or symbol that appears nowhere in the diff;
- states a motivation, benefit or effect that neither the diff nor the author's description gives.";

const DIFF_SOURCE_LABEL: &str = "Git diff:";
const CHANGE_LIST_SOURCE_LABEL: &str = "Change list compiled from every part of a diff too large to show whole. Each part was summarised separately; merge them into one message, do not concatenate:";

async fn generate_commit_message_chunked(
	repo_path: &std::path::Path,
	config: &Config,
	extra_context: Option<&str>,
) -> Result<String> {
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

	generate_commit_message_from_diff(&diff, config, extra_context).await
}

/// Generate a commit message from a diff string. Serves both staged changes and
/// -c/--commit rewrites, which pass the target commit's own diff.
async fn generate_commit_message_from_diff(
	diff: &str,
	config: &Config,
	extra_context: Option<&str>,
) -> Result<String> {
	let ctx = build_prompt_context(diff, extra_context);
	let chunks = diff_chunker::chunk_diff(diff);

	if chunks.len() == 1 {
		let prompt = create_commit_prompt(
			&chunks[0].content,
			&ctx,
			&ctx.files_section,
			DIFF_SOURCE_LABEL,
		);
		return draft_commit_message(&prompt, config).await;
	}

	println!(
		"📝 Processing large diff in {} chunks in parallel...",
		chunks.len()
	);
	let notes = process_commit_chunks_parallel(&chunks, &ctx, config).await;
	if notes.len() != chunks.len() {
		return Err(anyhow::anyhow!(
			"{} of {} diff chunks could not be summarised; refusing to write a commit message that omits part of the change",
			chunks.len() - notes.len(),
			chunks.len()
		));
	}

	println!(
		"🎯 Synthesising commit message from {} chunk summaries...",
		notes.len()
	);
	let change_list = diff_chunker::combine_commit_messages(notes);
	let prompt = create_commit_prompt(
		&change_list,
		&ctx,
		&ctx.files_section,
		CHANGE_LIST_SOURCE_LABEL,
	);
	draft_commit_message(&prompt, config).await
}

/// Derive the diff-wide prompt context (totals, docs-type rule, author guidance,
/// file statuses) from the diff headers alone, so staged and rewrite paths agree.
fn build_prompt_context(diff: &str, extra_context: Option<&str>) -> CommitPromptContext {
	let is_doc = |f: &str| f.ends_with(".md") || f.ends_with(".markdown") || f.ends_with(".rst");
	let files: Vec<&str> = diff
		.lines()
		.filter(|l| l.starts_with("diff --git "))
		.filter_map(|l| l.split(" b/").nth(1))
		.collect();
	let has_docs = files.iter().any(|f| is_doc(f));
	let has_code = files.iter().any(|f| !is_doc(f));

	let additions = diff
		.matches("\n+")
		.count()
		.saturating_sub(diff.matches("\n+++").count());
	let deletions = diff
		.matches("\n-")
		.count()
		.saturating_sub(diff.matches("\n---").count());

	let guidance_section = extra_context
		.map(|c| {
			format!(
				"Author's description of the change (guidance to verify against the diff, not text to copy):\n{}\n\n",
				c
			)
		})
		.unwrap_or_default();

	let docs_restriction = if has_code && !has_docs {
		"Type rule for this diff: no documentation files changed, so the type must not be docs.\n\n"
	} else if has_code && has_docs {
		"Type rule for this diff: use docs only if documentation is the primary change; otherwise use the type of the code change.\n\n"
	} else {
		""
	}
	.to_string();

	CommitPromptContext {
		file_count: files.len(),
		additions,
		deletions,
		guidance_section,
		docs_restriction,
		files_section: file_status_section(diff),
	}
}

/// Build a "STATUS path" line per file from the diff headers so the model can
/// distinguish new files from modified ones (the accuracy rules depend on it).
fn file_status_section(diff: &str) -> String {
	let mut lines = Vec::new();
	let mut current: Option<(String, char)> = None;

	for line in diff.lines() {
		if let Some(rest) = line.strip_prefix("diff --git ") {
			if let Some((path, status)) = current.take() {
				lines.push(format!("{} {}", status, path));
			}
			let path = rest.split(" b/").nth(1).unwrap_or(rest).to_string();
			current = Some((path, 'M'));
		} else if let Some((_, status)) = current.as_mut() {
			if line.starts_with("new file mode") {
				*status = 'A';
			} else if line.starts_with("deleted file mode") {
				*status = 'D';
			} else if line.starts_with("rename from") {
				*status = 'R';
			}
		}
	}
	if let Some((path, status)) = current {
		lines.push(format!("{} {}", status, path));
	}

	if lines.is_empty() {
		String::new()
	} else {
		format!(
			"File status (A=added, M=modified, D=deleted, R=renamed):\n{}\n\n",
			lines.join("\n")
		)
	}
}

/// Diff-wide context shared by every chunk prompt: change totals plus the
/// pre-built guidance, docs-restriction and file-status sections.
#[derive(Clone)]
struct CommitPromptContext {
	file_count: usize,
	additions: usize,
	deletions: usize,
	guidance_section: String,
	docs_restriction: String,
	files_section: String,
}

/// Build the user message for one LLM call. The rules live in the system prompt;
/// this carries only the facts: guidance, type rule, file statuses, totals, content.
///
/// `files_section` stays a separate parameter because chunked processing
/// substitutes it with a chunk-note-wrapped variant of `ctx.files_section`.
/// `source_label` names what `content` is: a git diff or a merged change list.
fn create_commit_prompt(
	content: &str,
	ctx: &CommitPromptContext,
	files_section: &str,
	source_label: &str,
) -> String {
	format!(
		"{}{}{}Changes: {} files (+{} -{} lines)\n\n{}\n{}",
		ctx.guidance_section,
		ctx.docs_restriction,
		files_section,
		ctx.file_count,
		ctx.additions,
		ctx.deletions,
		source_label,
		content
	)
}

/// Structured commit drafted by the LLM; `render_commit_message` turns it into text.
/// Empty strings mean "none": the JSON schema keeps every field required so strict
/// providers accept it, and `default` tolerates lenient providers that omit fields.
#[derive(Debug, Deserialize)]
struct CommitDraft {
	#[serde(rename = "type")]
	kind: String,
	#[serde(default)]
	scope: String,
	subject: String,
	#[serde(default)]
	effect: String,
	#[serde(default)]
	changes: Vec<String>,
	#[serde(default)]
	breaking: String,
}

fn commit_draft_schema() -> serde_json::Value {
	serde_json::json!({
		"type": "object",
		"properties": {
			"type": {"type": "string"},
			"scope": {"type": "string"},
			"subject": {"type": "string"},
			"effect": {"type": "string"},
			"changes": {"type": "array", "items": {"type": "string"}},
			"breaking": {"type": "string"}
		},
		"required": ["type", "scope", "subject", "effect", "changes", "breaking"]
	})
}

/// Render a draft as a conventional commit message. Format is enforced here, not
/// by the model: type whitelist, subject length, wrapping, plain text.
fn render_commit_message(draft: &CommitDraft) -> Result<String> {
	let kind = draft.kind.trim().to_lowercase();
	if !COMMIT_TYPES.contains(&kind.as_str()) {
		return Err(anyhow::anyhow!(
			"LLM returned unknown commit type '{}'",
			draft.kind
		));
	}

	let mut subject = plain_line(&draft.subject).trim_end_matches('.').to_string();
	if subject.is_empty() {
		return Err(anyhow::anyhow!("LLM returned an empty commit subject"));
	}
	// Lowercase a capitalised first word but leave acronyms (ONNX, MCP) alone.
	let mut chars = subject.chars();
	if let (Some(first), Some(second)) = (chars.next(), chars.next()) {
		if first.is_ascii_uppercase() && second.is_lowercase() {
			subject.replace_range(..1, &first.to_ascii_lowercase().to_string());
		}
	}

	let scope = plain_line(&draft.scope);
	let breaking = plain_line(&draft.breaking);

	let mut message = kind;
	if !scope.is_empty() {
		message.push_str(&format!("({})", scope));
	}
	if !breaking.is_empty() {
		message.push('!');
	}
	message.push_str(": ");
	message.push_str(&subject);
	if message.chars().count() > SUBJECT_MAX_CHARS {
		return Err(anyhow::anyhow!(
			"Commit subject exceeds {} characters: {}",
			SUBJECT_MAX_CHARS,
			message
		));
	}

	let effect = plain_line(&draft.effect);
	if !effect.is_empty() {
		message.push_str("\n\n");
		message.push_str(&wrap_text(&effect, BODY_WRAP_WIDTH, ""));
	}

	let changes: Vec<String> = draft
		.changes
		.iter()
		.map(|c| plain_line(c))
		.filter(|c| !c.is_empty())
		.collect();
	if !changes.is_empty() {
		message.push_str("\n\n");
		let bullets: Vec<String> = changes
			.iter()
			.map(|c| wrap_text(&format!("- {}", c), BODY_WRAP_WIDTH, "  "))
			.collect();
		message.push_str(&bullets.join("\n"));
	}

	if !breaking.is_empty() {
		message.push_str("\n\n");
		message.push_str(&wrap_text(
			&format!("BREAKING CHANGE: {}", breaking),
			BODY_WRAP_WIDTH,
			"",
		));
	}

	Ok(message)
}

/// Greedy word wrap at `width`; continuation lines are prefixed with `indent`.
fn wrap_text(text: &str, width: usize, indent: &str) -> String {
	let mut lines: Vec<String> = Vec::new();
	let mut current = String::new();
	for word in text.split_whitespace() {
		if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > width {
			lines.push(std::mem::take(&mut current));
		}
		if current.is_empty() {
			if !lines.is_empty() {
				current.push_str(indent);
			}
		} else {
			current.push(' ');
		}
		current.push_str(word);
	}
	if !current.is_empty() {
		lines.push(current);
	}
	lines.join("\n")
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

/// Summarise every chunk of a large diff concurrently, preserving chunk order.
///
/// Each chunk yields a plain change list, not a commit message; the caller
/// merges the lists and drafts the commit from them in one final call.
/// The semaphore only bounds concurrency; every chunk is processed.
async fn process_commit_chunks_parallel(
	chunks: &[diff_chunker::DiffChunk],
	ctx: &CommitPromptContext,
	config: &Config,
) -> Vec<String> {
	let total_chunks = chunks.len();
	let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(
		diff_chunker::MAX_PARALLEL_CHUNKS,
	));
	let mut join_set = JoinSet::new();

	for (i, chunk) in chunks.iter().enumerate() {
		let chunk_content = chunk.content.clone();
		let chunk_summary = chunk.file_summary.clone();
		let config = config.clone();
		let ctx = ctx.clone();
		let semaphore = semaphore.clone();

		join_set.spawn(async move {
			let _permit = semaphore
				.acquire_owned()
				.await
				.expect("semaphore never closed");

			println!(
				"  Processing chunk {}/{}: {}",
				i + 1,
				total_chunks,
				chunk_summary
			);

			let chunk_note = format!(
				"NOTE: This is chunk {}/{} of a larger diff and shows only PART of the changes.\n\
The file status list and totals below cover the WHOLE commit, not just this chunk.\n\
Added lines inside this chunk may belong to files marked M (modified); do not assume they are new functionality.\n\n{}",
				i + 1,
				total_chunks,
				ctx.files_section
			);

			let chunk_prompt =
				create_commit_prompt(&chunk_content, &ctx, &chunk_note, DIFF_SOURCE_LABEL);
			let label = format!("Chunk {}", i + 1);

			match call_llm_with_retry(|| summarise_chunk(&chunk_prompt, &config), &label).await {
				Ok(response) => Ok((i, response)),
				Err(e) => {
					eprintln!("Warning: {}", e);
					Err(e)
				}
			}
		});
	}

	collect_ordered_responses(join_set, total_chunks).await
}

/// Draft, audit the draft against the source, and redraft with the rejected
/// claims as feedback. The auditor always sees the original prompt (diff,
/// statuses, author guidance) and never a rejected draft, so it cannot be led.
/// Transport retries live inside `chat_completion_json`.
async fn draft_commit_message(prompt: &str, config: &Config) -> Result<String> {
	let client = LlmClient::from_config(config)?.with_temperature(LLM_TEMPERATURE);
	let mut draft_prompt = prompt.to_string();
	let mut rejected = Vec::new();
	for _ in 0..MAX_AUDIT_ROUNDS {
		let message = draft_once(&client, &draft_prompt).await?;
		rejected = client
			.unsupported_claims(COMMIT_AUDIT_RULES, prompt, &message)
			.await?;
		if rejected.is_empty() {
			return Ok(message);
		}
		println!("🔍 Audit rejected the draft; redrafting without these claims:");
		for claim in &rejected {
			println!("  • {}", claim);
		}
		draft_prompt.push_str(&format!(
			"\n\nA previous draft was rejected because the diff does not support these claims. Do not repeat them; describe only what the diff shows:\n- {}\n\nRejected draft:\n{}",
			rejected.join("\n- "),
			message
		));
	}
	Err(anyhow::anyhow!(
		"Commit message still claims more than the diff shows after {} drafts: {}",
		MAX_AUDIT_ROUNDS,
		rejected.join("; ")
	))
}

/// One LLM call that returns a validated, rendered commit message.
async fn draft_once(client: &LlmClient, prompt: &str) -> Result<String> {
	let messages = vec![Message::system(COMMIT_SYSTEM_PROMPT), Message::user(prompt)];
	let value = client
		.chat_completion_json(messages, Some(commit_draft_schema()))
		.await?;
	let draft: CommitDraft =
		serde_json::from_value(value).context("LLM returned malformed commit JSON")?;
	render_commit_message(&draft)
}

/// One LLM call that turns a diff chunk into a plain change list.
async fn summarise_chunk(prompt: &str, config: &Config) -> Result<String> {
	let client = LlmClient::from_config(config)?;
	let messages = vec![Message::system(CHUNK_SYSTEM_PROMPT), Message::user(prompt)];
	client
		.chat_completion_with_temperature(messages, LLM_TEMPERATURE)
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
							.args(["add", "--", file.trim()])
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_file_status_section() {
		let diff = "diff --git a/src/existing.rs b/src/existing.rs\n\
index 123..456 100644\n\
--- a/src/existing.rs\n\
+++ b/src/existing.rs\n\
@@ -1,2 +1,3 @@\n\
+new line\n\
diff --git a/src/brand_new.rs b/src/brand_new.rs\n\
new file mode 100644\n\
--- /dev/null\n\
+++ b/src/brand_new.rs\n\
diff --git a/src/gone.rs b/src/gone.rs\n\
deleted file mode 100644\n";

		let section = file_status_section(diff);
		assert!(section.contains("M src/existing.rs"));
		assert!(section.contains("A src/brand_new.rs"));
		assert!(section.contains("D src/gone.rs"));
		assert!(file_status_section("").is_empty());
	}
}
