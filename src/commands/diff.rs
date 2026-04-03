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

use clap::Args;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

use octocode::config::Config;
use octocode::indexer;
use octocode::llm::{LlmClient, Message};
use octocode::store::Store;
use octocode::utils::diff_chunker;

use crate::commands::OutputFormat;

#[derive(Debug, Args)]
pub struct DiffArgs {
	/// Target: commit hash, commit range (a..b), branch name, or omit for working changes
	pub target: Option<String>,

	/// Only analyze staged changes
	#[arg(long)]
	pub staged: bool,

	/// Output format: 'cli', 'json', 'text', or 'md'
	#[arg(short, long, default_value = "cli")]
	pub format: OutputFormat,
}

#[derive(Debug, Serialize, Deserialize)]
struct DiffAnalysis {
	summary: String,
	risk: String,
	changes: Vec<ChangeCard>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChangeCard {
	title: String,
	risk: String,
	what_changed: Vec<String>,
	impact: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	uncertain: Option<String>,
}

const DIFF_SYSTEM_PROMPT: &str = "\
You are a senior engineer reviewing code changes.
Produce a behavioral summary — what changed and why it matters.

RULES:
- Describe BEHAVIORS, not files or lines. Bad: 'Modified line 47 in api.rs'. Good: 'API calls now timeout after 5s instead of hanging'
- Flag real risks only — not theoretical edge cases
- If uncertain about impact, include an 'uncertain' field explaining what you couldn't verify — NEVER assert unverified claims
- Group related changes into logical units (max 5 cards)
- Be concise: 1-2 sentences per bullet point
- risk must be one of: low, medium, high

Respond with ONLY a JSON object (no markdown, no code fences):
{
  \"summary\": \"one-line overall description\",
  \"risk\": \"low|medium|high\",
  \"changes\": [
    {
      \"title\": \"behavioral change description\",
      \"risk\": \"low|medium|high\",
      \"what_changed\": [\"bullet points of behaviors\"],
      \"impact\": \"what else is affected or 'Isolated change'\",
      \"uncertain\": \"what couldn't be verified (omit if confident)\"
    }
  ]
}";

pub async fn execute(store: &Store, args: &DiffArgs, config: &Config) -> Result<(), anyhow::Error> {
	let current_dir = std::env::current_dir()?;

	// Step 1: Get the diff
	let (diff, changed_files, diff_label) = get_diff(&current_dir, args)?;

	if diff.trim().is_empty() {
		println!("No changes to analyze.");
		return Ok(());
	}

	// Step 2: Gather context from index (optional — works without index)
	let context = gather_diff_context(store, &changed_files, config).await;

	// Step 3: Build LLM prompt with chunking for large diffs
	let analysis = analyze_diff(&diff, &changed_files, &context, &diff_label, config).await?;

	// Step 4: Output
	match () {
		_ if args.format.is_json() => {
			println!("{}", serde_json::to_string_pretty(&analysis)?);
		}
		_ if args.format.is_md() => {
			print_markdown(&analysis, &diff_label);
		}
		_ => {
			print_cli(&analysis, &diff_label);
		}
	}

	Ok(())
}

/// Get diff content and changed file list based on target
fn get_diff(
	repo_path: &Path,
	args: &DiffArgs,
) -> Result<(String, Vec<String>, String), anyhow::Error> {
	match &args.target {
		None => {
			// Working directory changes
			let git_args = if args.staged {
				vec!["diff", "--cached"]
			} else {
				vec!["diff", "HEAD"]
			};

			let diff = run_git(repo_path, &git_args)?;

			let name_args = if args.staged {
				vec!["diff", "--cached", "--name-only"]
			} else {
				vec!["diff", "HEAD", "--name-only"]
			};
			let files = run_git(repo_path, &name_args)?
				.lines()
				.filter(|l| !l.trim().is_empty())
				.map(|l| l.to_string())
				.collect();

			let label = if args.staged {
				"Staged changes".to_string()
			} else {
				"Working changes".to_string()
			};

			Ok((diff, files, label))
		}
		Some(target) => {
			if target.contains("..") {
				// Commit range: a..b
				let diff = run_git(repo_path, &["diff", target])?;
				let files = run_git(repo_path, &["diff", "--name-only", target])?
					.lines()
					.filter(|l| !l.trim().is_empty())
					.map(|l| l.to_string())
					.collect();
				Ok((diff, files, format!("Range: {}", target)))
			} else {
				// Try as commit hash first
				let diff_result =
					run_git(repo_path, &["diff", &format!("{}^..{}", target, target)]);
				if let Ok(diff) = diff_result {
					let files = run_git(
						repo_path,
						&["diff", "--name-only", &format!("{}^..{}", target, target)],
					)?
					.lines()
					.filter(|l| !l.trim().is_empty())
					.map(|l| l.to_string())
					.collect();

					// Get commit message
					let msg = run_git(repo_path, &["log", "-1", "--format=%s", target])
						.unwrap_or_default();
					let label =
						format!("Commit: {} {}", &target[..target.len().min(7)], msg.trim());
					return Ok((diff, files, label));
				}

				// Try as branch name — diff against default branch
				let default_branch =
					octocode::indexer::git_utils::GitUtils::get_default_branch(repo_path)?;
				let range = format!("{}...{}", default_branch, target);
				let diff = run_git(repo_path, &["diff", &range])?;
				let files = run_git(repo_path, &["diff", "--name-only", &range])?
					.lines()
					.filter(|l| !l.trim().is_empty())
					.map(|l| l.to_string())
					.collect();

				Ok((
					diff,
					files,
					format!("Branch: {} vs {}", target, default_branch),
				))
			}
		}
	}
}

fn run_git(repo_path: &Path, args: &[&str]) -> Result<String, anyhow::Error> {
	let output = Command::new("git")
		.args(args)
		.current_dir(repo_path)
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);
		return Err(anyhow::anyhow!("git {} failed: {}", args.join(" "), stderr));
	}

	Ok(String::from_utf8(output.stdout)?)
}

/// Gather codebase context for changed files from the index
async fn gather_diff_context(store: &Store, changed_files: &[String], config: &Config) -> String {
	let mut context = String::new();

	// For each changed file, find consumers/dependents via search
	for file in changed_files.iter().take(5) {
		// Try GraphRAG relationships
		if let Ok(rels) = store
			.get_node_relationships(
				file,
				octocode::indexer::graphrag::types::RelationshipDirection::Incoming,
			)
			.await
		{
			if !rels.is_empty() {
				context.push_str(&format!("{} is used by: ", file));
				let consumers: Vec<&str> = rels.iter().take(5).map(|r| r.source.as_str()).collect();
				context.push_str(&consumers.join(", "));
				context.push('\n');
			}
		}
	}

	// Try semantic search for broader context
	if !changed_files.is_empty() {
		let query = changed_files
			.iter()
			.take(3)
			.cloned()
			.collect::<Vec<_>>()
			.join(" ");
		if let Ok(related) = indexer::search::search_codebase_with_details_text(
			&query,
			"code",
			"signatures",
			5,
			0.3,
			None,
			config,
		)
		.await
		{
			if !related.trim().is_empty() && !related.contains("No results") {
				context.push_str("\nRelated code signatures:\n");
				if related.len() > 1500 {
					context.push_str(&related[..1500]);
				} else {
					context.push_str(&related);
				}
			}
		}
	}

	context
}

/// Analyze diff using LLM, with chunking for large diffs
async fn analyze_diff(
	diff: &str,
	changed_files: &[String],
	context: &str,
	_label: &str,
	config: &Config,
) -> Result<DiffAnalysis, anyhow::Error> {
	let client = LlmClient::from_config(config)?;

	// Check if diff needs chunking
	let max_diff_chars = 12000;
	if diff.len() <= max_diff_chars {
		return analyze_single_diff(&client, diff, changed_files, context).await;
	}

	// Large diff: chunk and analyze each, then combine
	let chunks = diff_chunker::chunk_diff(diff);
	if chunks.len() <= 1 {
		let truncated = &diff[..max_diff_chars];
		return analyze_single_diff(&client, truncated, changed_files, context).await;
	}

	let mut all_changes = Vec::new();
	let mut overall_risk = "low";

	for chunk in &chunks {
		let chunk_diff = chunk.content.as_str();
		if chunk_diff.trim().is_empty() {
			continue;
		}
		match analyze_single_diff(&client, chunk_diff, changed_files, context).await {
			Ok(analysis) => {
				if analysis.risk == "high" {
					overall_risk = "high";
				} else if analysis.risk == "medium" && overall_risk != "high" {
					overall_risk = "medium";
				}
				all_changes.extend(analysis.changes);
			}
			Err(e) => {
				all_changes.push(ChangeCard {
					title: format!("Chunk analysis failed: {}", chunk.file_summary),
					risk: "medium".to_string(),
					what_changed: vec!["Could not analyze this portion of the diff".to_string()],
					impact: "Unknown".to_string(),
					uncertain: Some(format!("Analysis error: {}", e)),
				});
			}
		}
	}

	// Truncate to max 5 cards
	all_changes.truncate(5);

	Ok(DiffAnalysis {
		summary: format!(
			"{} logical change(s) across {} files",
			all_changes.len(),
			changed_files.len()
		),
		risk: overall_risk.to_string(),
		changes: all_changes,
	})
}

async fn analyze_single_diff(
	client: &LlmClient,
	diff: &str,
	changed_files: &[String],
	context: &str,
) -> Result<DiffAnalysis, anyhow::Error> {
	let mut user_prompt = String::new();
	user_prompt.push_str("DIFF:\n```\n");
	user_prompt.push_str(diff);
	user_prompt.push_str("\n```\n\n");

	user_prompt.push_str("CHANGED FILES: ");
	user_prompt.push_str(&changed_files.join(", "));
	user_prompt.push('\n');

	if !context.is_empty() {
		user_prompt.push_str("\nCODEBASE CONTEXT (from index):\n");
		user_prompt.push_str(context);
		user_prompt.push('\n');
	}

	let messages = vec![
		Message::system(DIFF_SYSTEM_PROMPT),
		Message::user(&user_prompt),
	];

	let json = client.chat_completion_json(messages).await?;

	let analysis: DiffAnalysis = serde_json::from_value(json)
		.map_err(|e| anyhow::anyhow!("Failed to parse LLM response as DiffAnalysis: {}", e))?;

	Ok(analysis)
}

fn print_cli(analysis: &DiffAnalysis, label: &str) {
	let risk_icon = match analysis.risk.as_str() {
		"high" => "HIGH",
		"medium" => "MEDIUM",
		_ => "LOW",
	};

	println!("Diff: {}", label);
	println!(
		"Risk: {} | Changes: {}\n",
		risk_icon,
		analysis.changes.len()
	);
	println!("{}\n", analysis.summary);

	// Triage table
	println!("{:<4} {:<50} Risk", "#", "Change");
	println!("{}", "-".repeat(65));
	for (i, card) in analysis.changes.iter().enumerate() {
		let risk = match card.risk.as_str() {
			"high" => "HIGH",
			"medium" => "MED",
			_ => "LOW",
		};
		println!(
			"{:<4} {:<50} {}",
			i + 1,
			truncate_str(&card.title, 48),
			risk
		);
	}

	println!();

	// Cards
	for (i, card) in analysis.changes.iter().enumerate() {
		println!(
			"--- Card {}/{}: {} ---",
			i + 1,
			analysis.changes.len(),
			card.title
		);

		println!("  What changed:");
		for bullet in &card.what_changed {
			println!("    - {}", bullet);
		}

		println!("  Impact: {}", card.impact);

		if let Some(ref uncertain) = card.uncertain {
			println!("  Uncertain: {}", uncertain);
		}

		println!();
	}
}

fn print_markdown(analysis: &DiffAnalysis, label: &str) {
	let risk_icon = match analysis.risk.as_str() {
		"high" => "HIGH",
		"medium" => "MEDIUM",
		_ => "LOW",
	};

	println!("## Diff: {}", label);
	println!(
		"**Risk:** {} | **Changes:** {}\n",
		risk_icon,
		analysis.changes.len()
	);
	println!("{}\n", analysis.summary);

	println!("| # | Change | Risk |");
	println!("|---|--------|------|");
	for (i, card) in analysis.changes.iter().enumerate() {
		println!("| {} | {} | {} |", i + 1, card.title, card.risk);
	}
	println!();

	for (i, card) in analysis.changes.iter().enumerate() {
		println!(
			"### Card {}/{}: {}\n",
			i + 1,
			analysis.changes.len(),
			card.title
		);

		println!("**What changed:**");
		for bullet in &card.what_changed {
			println!("- {}", bullet);
		}

		println!("\n**Impact:** {}", card.impact);

		if let Some(ref uncertain) = card.uncertain {
			println!("\n**Uncertain:** {}", uncertain);
		}

		println!();
	}
}

fn truncate_str(s: &str, max: usize) -> String {
	if s.len() <= max {
		s.to_string()
	} else {
		format!("{}...", &s[..max - 3])
	}
}
