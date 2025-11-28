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

use anyhow::{Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use octocode::config::Config;
use octocode::indexer::git_utils::GitUtils;
use octocode::llm::LlmClient;

#[derive(Args, Debug)]
pub struct ReleaseArgs {
	/// Changelog file path (default: CHANGELOG.md)
	#[arg(short, long, default_value = "CHANGELOG.md")]
	pub changelog: String,

	/// Skip confirmation prompt
	#[arg(short, long)]
	pub yes: bool,

	/// Dry run - show what would be done without making changes
	#[arg(short, long)]
	pub dry_run: bool,

	/// Force a specific version instead of AI calculation
	#[arg(short, long)]
	pub force_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitAnalysis {
	pub commits: Vec<CommitInfo>,
	pub breaking_changes: Vec<String>,
	pub features: Vec<String>,
	pub fixes: Vec<String>,
	pub other_changes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
	pub hash: String,
	pub message: String,
	pub author: String,
	pub date: String,
	pub commit_type: String,
	pub scope: Option<String>,
	pub description: String,
	pub breaking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionCalculation {
	pub current_version: String,
	pub new_version: String,
	pub version_type: String, // major, minor, patch
	pub reasoning: String,
}

#[derive(Debug, Clone)]
pub enum ProjectType {
	Rust(PathBuf), // Cargo.toml
	Node(PathBuf), // package.json
	Php(PathBuf),  // composer.json
	Go(PathBuf),   // go.mod
	Unknown,
}

pub async fn execute(config: &Config, args: &ReleaseArgs) -> Result<()> {
	let current_dir = std::env::current_dir()?;

	// Find git repository root
	let git_root = GitUtils::find_git_root(&current_dir)
		.ok_or_else(|| anyhow::anyhow!("❌ Not in a git repository!"))?;

	// Use git root as working directory for all operations
	let current_dir = git_root;

	println!("🚀 Starting release process...\n");

	// Detect project type
	let project_type = detect_project_type(&current_dir)?;
	println!(
		"📦 Project type detected: {}",
		format_project_type(&project_type)
	);

	// Get current version from project files or git tags
	let current_version = get_current_version(&project_type).await?;
	println!("📌 Current version: {}", current_version);

	// Get latest tag to determine commit range
	let last_tag = get_latest_tag().await?;
	let commit_range = if let Some(ref tag) = last_tag {
		format!("{}..HEAD", tag)
	} else {
		"HEAD".to_string()
	};

	println!(
		"📋 Analyzing commits since: {}",
		last_tag.as_deref().unwrap_or("initial commit")
	);

	// Analyze commits since last tag
	let commit_analysis = analyze_commits(&commit_range).await?;

	if commit_analysis.commits.is_empty() {
		println!("✅ No new commits since last release. Nothing to release.");
		return Ok(());
	}

	println!(
		"📊 Found {} commits to analyze",
		commit_analysis.commits.len()
	);

	// Calculate new version
	let version_calculation = if let Some(forced_version) = &args.force_version {
		VersionCalculation {
			current_version: current_version.clone(),
			new_version: forced_version.clone(),
			version_type: "forced".to_string(),
			reasoning: "Version forced by user".to_string(),
		}
	} else if last_tag.is_none() {
		// First release: use current version without bumping
		VersionCalculation {
			current_version: current_version.clone(),
			new_version: current_version.clone(),
			version_type: "initial".to_string(),
			reasoning: "First release: using current version without bump".to_string(),
		}
	} else {
		calculate_version_with_ai(config, &current_version, &commit_analysis).await?
	};

	println!("\n🎯 Version calculation:");
	println!("   Current: {}", version_calculation.current_version);
	println!("   New:     {}", version_calculation.new_version);
	println!("   Type:    {}", version_calculation.version_type);
	println!("   Reason:  {}", version_calculation.reasoning);

	// Generate changelog content with AI enhancement
	let changelog_content = generate_enhanced_changelog_with_ai(
		config,
		&version_calculation,
		&commit_analysis,
		&project_type,
		&commit_range,
	)
	.await?;

	println!("\n📝 Generated changelog entry:");
	println!("═══════════════════════════════════");
	println!("{}", changelog_content);
	println!("═══════════════════════════════════");

	if args.dry_run {
		println!("\n🔍 DRY RUN - No changes would be made");
		return Ok(());
	}

	// Confirm with user (unless --yes flag is used)
	if !args.yes {
		print!(
			"\nProceed with release {}? [y/N] ",
			version_calculation.new_version
		);
		io::stdout().flush()?;

		let mut input = String::new();
		io::stdin().read_line(&mut input)?;

		if !input.trim().to_lowercase().starts_with('y') {
			println!("❌ Release cancelled.");
			return Ok(());
		}
	}

	println!("\n🔄 Creating release...");

	// Update project files with new version
	update_project_version(&project_type, &version_calculation.new_version).await?;
	println!("✅ Updated project files");

	// Update lock files after version change
	update_lock_files(&project_type).await?;
	println!("✅ Updated lock files");

	// Update changelog
	update_changelog(&args.changelog, &changelog_content).await?;
	println!("✅ Updated {}", args.changelog);

	// Stage changes
	stage_release_files(&args.changelog, &project_type).await?;
	println!("✅ Staged release files");

	// Create release commit
	let commit_message = format!("chore(release): {}", version_calculation.new_version);
	create_commit(&commit_message).await?;
	println!("✅ Created release commit");

	// Create git tag
	create_tag(&version_calculation.new_version, &changelog_content).await?;
	println!("✅ Created git tag: {}", version_calculation.new_version);

	println!(
		"\n🎉 Release {} created successfully!",
		version_calculation.new_version
	);
	println!("💡 Don't forget to push with: git push origin main --tags");

	Ok(())
}

fn detect_project_type(dir: &Path) -> Result<ProjectType> {
	if dir.join("Cargo.toml").exists() {
		Ok(ProjectType::Rust(dir.join("Cargo.toml")))
	} else if dir.join("package.json").exists() {
		Ok(ProjectType::Node(dir.join("package.json")))
	} else if dir.join("composer.json").exists() {
		Ok(ProjectType::Php(dir.join("composer.json")))
	} else if dir.join("go.mod").exists() {
		Ok(ProjectType::Go(dir.join("go.mod")))
	} else {
		Ok(ProjectType::Unknown)
	}
}

fn format_project_type(project_type: &ProjectType) -> String {
	match project_type {
		ProjectType::Rust(_) => "Rust (Cargo.toml)".to_string(),
		ProjectType::Node(_) => "Node.js (package.json)".to_string(),
		ProjectType::Php(_) => "PHP (composer.json)".to_string(),
		ProjectType::Go(_) => "Go (go.mod)".to_string(),
		ProjectType::Unknown => "Unknown (no project file detected)".to_string(),
	}
}

async fn get_current_version(project_type: &ProjectType) -> Result<String> {
	match project_type {
		ProjectType::Rust(cargo_path) => {
			let content = fs::read_to_string(cargo_path)?;
			if let Some(version_line) = content
				.lines()
				.find(|line| line.trim_start().starts_with("version"))
			{
				if let Some(version) = extract_version_from_line(version_line) {
					return Ok(version);
				}
			}
		}
		ProjectType::Node(package_path) => {
			let content = fs::read_to_string(package_path)?;
			let package: serde_json::Value = serde_json::from_str(&content)?;
			if let Some(version) = package.get("version").and_then(|v| v.as_str()) {
				return Ok(version.to_string());
			}
		}
		ProjectType::Php(composer_path) => {
			let content = fs::read_to_string(composer_path)?;
			let composer: serde_json::Value = serde_json::from_str(&content)?;
			if let Some(version) = composer.get("version").and_then(|v| v.as_str()) {
				return Ok(version.to_string());
			}
		}
		ProjectType::Go(go_mod_path) => {
			// Check for VERSION file in Go projects
			let version_file = go_mod_path.parent().unwrap().join("VERSION");
			if version_file.exists() {
				let content = fs::read_to_string(version_file)?;
				return Ok(content.trim().to_string());
			}
			// Fall back to git tags if no VERSION file
		}
		ProjectType::Unknown => {}
	}

	// Fallback to git tags
	if let Ok(Some(tag)) = get_latest_tag().await {
		// Remove 'v' prefix if present
		let version = tag.strip_prefix('v').unwrap_or(&tag);
		Ok(version.to_string())
	} else {
		Ok("0.1.0".to_string())
	}
}

fn extract_version_from_line(line: &str) -> Option<String> {
	// Extract version from line like: version = "1.0.0"
	if let Some(start) = line.find('"') {
		if let Some(end) = line[start + 1..].find('"') {
			return Some(line[start + 1..start + 1 + end].to_string());
		}
	}
	// Try single quotes: version = '1.0.0'
	if let Some(start) = line.find('\'') {
		if let Some(end) = line[start + 1..].find('\'') {
			return Some(line[start + 1..start + 1 + end].to_string());
		}
	}
	None
}

async fn get_latest_tag() -> Result<Option<String>> {
	let output = Command::new("git")
		.args(["describe", "--tags", "--abbrev=0"])
		.output()?;

	if output.status.success() {
		let tag = String::from_utf8(output.stdout)?;
		Ok(Some(tag.trim().to_string()))
	} else {
		Ok(None)
	}
}

async fn analyze_commits(commit_range: &str) -> Result<CommitAnalysis> {
	let output = Command::new("git")
		.args(["log", "--format=%H|%an|%ad|%s", "--date=iso", commit_range])
		.output()?;

	if !output.status.success() {
		return Err(anyhow::anyhow!(
			"Failed to get commit log: {}",
			String::from_utf8_lossy(&output.stderr)
		));
	}

	let log_output = String::from_utf8(output.stdout)?;
	let mut commits = Vec::new();
	let mut breaking_changes = Vec::new();
	let mut features = Vec::new();
	let mut fixes = Vec::new();
	let mut other_changes = Vec::new();

	for line in log_output.lines() {
		if line.trim().is_empty() {
			continue;
		}

		let parts: Vec<&str> = line.split('|').collect();
		if parts.len() < 4 {
			continue;
		}

		let hash = parts[0].to_string();
		let author = parts[1].to_string();
		let date = parts[2].to_string();
		let message = parts[3].to_string();

		let (commit_type, scope, description, breaking) = parse_conventional_commit(&message);

		let commit_info = CommitInfo {
			hash: hash.clone(),
			message: message.clone(),
			author,
			date,
			commit_type: commit_type.clone(),
			scope,
			description: description.clone(),
			breaking,
		};

		commits.push(commit_info);

		// Categorize changes
		if breaking {
			breaking_changes.push(format!("**{}**: {}", commit_type, description));
		} else {
			match commit_type.as_str() {
				"feat" => features.push(description),
				"fix" => fixes.push(description),
				_ => other_changes.push(format!("{}: {}", commit_type, description)),
			}
		}
	}

	Ok(CommitAnalysis {
		commits,
		breaking_changes,
		features,
		fixes,
		other_changes,
	})
}

fn parse_conventional_commit(message: &str) -> (String, Option<String>, String, bool) {
	let breaking = message.contains("BREAKING CHANGE") || message.contains('!');

	// Try to parse conventional commit format: type(scope): description
	if let Some(colon_pos) = message.find(':') {
		let prefix = &message[..colon_pos];
		let description = message[colon_pos + 1..].trim().to_string();

		if let Some(paren_start) = prefix.find('(') {
			if let Some(paren_end) = prefix.find(')') {
				let commit_type = prefix[..paren_start].trim().replace('!', "");
				let scope = Some(prefix[paren_start + 1..paren_end].to_string());
				return (commit_type, scope, description, breaking);
			}
		}

		let commit_type = prefix.trim().replace('!', "");
		return (commit_type, None, description, breaking);
	}

	// Fallback: try to detect type from message start
	let lower_message = message.to_lowercase();
	let commit_type = if lower_message.starts_with("feat") {
		"feat"
	} else if lower_message.starts_with("fix") {
		"fix"
	} else if lower_message.starts_with("docs") {
		"docs"
	} else if lower_message.starts_with("style") {
		"style"
	} else if lower_message.starts_with("refactor") {
		"refactor"
	} else if lower_message.starts_with("test") {
		"test"
	} else {
		"chore"
	};

	(commit_type.to_string(), None, message.to_string(), breaking)
}

async fn calculate_version_with_ai(
	config: &Config,
	current_version: &str,
	analysis: &CommitAnalysis,
) -> Result<VersionCalculation> {
	let analysis_json = serde_json::to_string_pretty(analysis)?;

	let prompt = format!(
		"Analyze git commits and determine the next semantic version.\n\n\
        CURRENT VERSION: {}\n\n\
        COMMIT ANALYSIS:\n{}\n\n\
        SEMANTIC VERSIONING RULES (STRICT):\n\
        - MAJOR (x.0.0): Breaking changes, BREAKING CHANGE keyword, or commits with '!'\n\
        - MINOR (0.x.0): New features (feat:) without breaking changes\n\
        - PATCH (0.0.x): Bug fixes (fix:), docs, chore, style, refactor, test, perf, ci, build\\n\
        - Follow semantic versioning 2.0.0 specification exactly\\n\\n\
        DECISION GUIDELINES:\\n\
        - If ANY commit has breaking changes → MAJOR version\\n\
        - If NO breaking changes but ANY new features → MINOR version\\n\
        - If ONLY fixes/improvements/docs/chores → PATCH version\\n\
        - Consider cumulative impact: multiple features may warrant MINOR even if individual commits seem small\\n\
        - When uncertain between MINOR/PATCH: choose PATCH for safety\\n\
        - When uncertain between MAJOR/MINOR: choose MAJOR for safety\\n\\n\
        IMPORTANT: Preserve all commit information exactly as provided. Do not modify or summarize commit messages.\n\n\
        Respond with valid JSON only (no markdown, no additional text):\n\
        {{\n\
        \"current_version\": \"{}\",\n\
        \"new_version\": \"X.Y.Z\",\n\
        \"version_type\": \"major|minor|patch\",\n\
        \"reasoning\": \"Clear explanation of version choice based on changes\"\n\
        }}",
		current_version, analysis_json, current_version
	);

	match call_llm_for_version_calculation(&prompt, config).await {
		Ok(response) => {
			// Try to parse JSON response
			if let Ok(calculation) = serde_json::from_str::<VersionCalculation>(&response) {
				Ok(calculation)
			} else {
				// Fallback to manual calculation
				calculate_version_fallback(current_version, analysis)
			}
		}
		Err(e) => {
			eprintln!(
				"Warning: LLM call failed ({}), using fallback calculation",
				e
			);
			calculate_version_fallback(current_version, analysis)
		}
	}
}

fn calculate_version_fallback(
	current_version: &str,
	analysis: &CommitAnalysis,
) -> Result<VersionCalculation> {
	let parts: Vec<&str> = current_version.split('.').collect();
	if parts.len() != 3 {
		return Err(anyhow::anyhow!(
			"Invalid version format: {}",
			current_version
		));
	}

	let major: u32 = parts[0].parse().context("Invalid major version")?;
	let minor: u32 = parts[1].parse().context("Invalid minor version")?;
	let patch: u32 = parts[2].parse().context("Invalid patch version")?;

	let (new_version, version_type, reasoning) = if !analysis.breaking_changes.is_empty() {
		(
			format!("{}.0.0", major + 1),
			"major",
			"Breaking changes detected",
		)
	} else if !analysis.features.is_empty() {
		(
			format!("{}.{}.0", major, minor + 1),
			"minor",
			"New features added",
		)
	} else if !analysis.fixes.is_empty() || !analysis.other_changes.is_empty() {
		(
			format!("{}.{}.{}", major, minor, patch + 1),
			"patch",
			"Bug fixes and improvements",
		)
	} else {
		(
			format!("{}.{}.{}", major, minor, patch + 1),
			"patch",
			"Miscellaneous changes",
		)
	};

	Ok(VersionCalculation {
		current_version: current_version.to_string(),
		new_version,
		version_type: version_type.to_string(),
		reasoning: reasoning.to_string(),
	})
}

async fn call_llm_for_version_calculation(prompt: &str, config: &Config) -> Result<String> {
	use octocode::llm::{LlmClient, Message};

	// Create LLM client from config
	let client = LlmClient::from_config(config)?;

	// Build messages
	let messages = vec![Message::user(prompt)];

	// Call LLM with low temperature for consistent version calculation
	let response = client
		.chat_completion_with_temperature(messages, 0.1)
		.await?;

	Ok(response)
}

async fn generate_changelog_content(
	version: &VersionCalculation,
	analysis: &CommitAnalysis,
) -> Result<String> {
	let mut content = String::new();
	let date = chrono::Utc::now().format("%Y-%m-%d").to_string();

	content.push_str(&format!("## [{}] - {}\n\n", version.new_version, date));

	// Enhanced categorization - group commits by impact and area
	let mut breaking_commits = Vec::new();
	let mut feature_commits = Vec::new();
	let mut improvement_commits = Vec::new();
	let mut fix_commits = Vec::new();
	let mut docs_commits = Vec::new();
	let mut other_commits = Vec::new();

	for commit in &analysis.commits {
		if commit.breaking {
			breaking_commits.push(commit);
		} else {
			match commit.commit_type.as_str() {
				"feat" => feature_commits.push(commit),
				"fix" => fix_commits.push(commit),
				"perf" | "refactor" | "style" => improvement_commits.push(commit),
				"docs" => docs_commits.push(commit),
				_ => other_commits.push(commit),
			}
		}
	}

	// Calculate counts
	let total_commits = analysis.commits.len();
	let breaking_count = breaking_commits.len();
	let feature_count = feature_commits.len();
	let improvement_count = improvement_commits.len();
	let fix_count = fix_commits.len();
	let docs_count = docs_commits.len();
	let other_count = other_commits.len();

	// Breaking Changes - Highest Priority
	if !breaking_commits.is_empty() {
		content.push_str("### 🚨 Breaking Changes\n\n");
		content.push_str("⚠️ **Important**: This release contains breaking changes that may require code updates.\n\n");
		for commit in &breaking_commits {
			content.push_str(&format_enhanced_commit_entry(commit));
		}
		content.push('\n');
	}

	// New Features & Enhancements
	if !feature_commits.is_empty() {
		content.push_str("### ✨ New Features & Enhancements\n\n");
		for commit in &feature_commits {
			content.push_str(&format_enhanced_commit_entry(commit));
		}
		content.push('\n');
	}

	// Improvements & Optimizations
	if !improvement_commits.is_empty() {
		content.push_str("### 🔧 Improvements & Optimizations\n\n");
		for commit in &improvement_commits {
			content.push_str(&format_enhanced_commit_entry(commit));
		}
		content.push('\n');
	}

	// Bug Fixes & Stability
	if !fix_commits.is_empty() {
		content.push_str("### 🐛 Bug Fixes & Stability\n\n");
		for commit in &fix_commits {
			content.push_str(&format_enhanced_commit_entry(commit));
		}
		content.push('\n');
	}

	// Documentation & Examples
	if !docs_commits.is_empty() {
		content.push_str("### 📚 Documentation & Examples\n\n");
		for commit in &docs_commits {
			content.push_str(&format_enhanced_commit_entry(commit));
		}
		content.push('\n');
	}

	// Other Changes
	if !other_commits.is_empty() {
		content.push_str("### 🔄 Other Changes\n\n");
		for commit in &other_commits {
			content.push_str(&format_enhanced_commit_entry(commit));
		}
		content.push('\n');
	}

	// Enhanced commit summary with better organization
	if !analysis.commits.is_empty() {
		content.push_str("### 📊 Release Summary\n\n");

		content.push_str(&format!(
			"**Total commits**: {} across {} categories\n\n",
			total_commits,
			[
				breaking_count > 0,
				feature_count > 0,
				improvement_count > 0,
				fix_count > 0,
				docs_count > 0,
				other_count > 0
			]
			.iter()
			.filter(|&&x| x)
			.count()
		));

		// Priority-based summary
		if breaking_count > 0 {
			content.push_str(&format!(
				"🚨 **{}** breaking change{} - *Review migration guide above*\n",
				breaking_count,
				if breaking_count == 1 { "" } else { "s" }
			));
		}
		if feature_count > 0 {
			content.push_str(&format!(
				"✨ **{}** new feature{} - *Enhanced functionality*\n",
				feature_count,
				if feature_count == 1 { "" } else { "s" }
			));
		}
		if improvement_count > 0 {
			content.push_str(&format!(
				"🔧 **{}** improvement{} - *Better performance & code quality*\n",
				improvement_count,
				if improvement_count == 1 { "" } else { "s" }
			));
		}
		if fix_count > 0 {
			content.push_str(&format!(
				"🐛 **{}** bug fix{} - *Improved stability*\n",
				fix_count,
				if fix_count == 1 { "" } else { "es" }
			));
		}
		if docs_count > 0 {
			content.push_str(&format!(
				"📚 **{}** documentation update{} - *Better developer experience*\n",
				docs_count,
				if docs_count == 1 { "" } else { "s" }
			));
		}
		if other_count > 0 {
			content.push_str(&format!(
				"🔄 **{}** other change{} - *Maintenance & tooling*\n",
				other_count,
				if other_count == 1 { "" } else { "s" }
			));
		}

		content.push('\n');
	}

	Ok(content)
}

fn format_enhanced_commit_entry(commit: &CommitInfo) -> String {
	let short_hash = &commit.hash[..8];
	let mut entry = String::new();

	// Use description if it's different from the full message, otherwise use the full message
	let display_text = if commit.description != commit.message && !commit.description.is_empty() {
		&commit.description
	} else {
		&commit.message
	};

	// Enhanced formatting with scope and better presentation
	if let Some(ref scope) = commit.scope {
		entry.push_str(&format!(
			"- **{}**: {} `{}`\n",
			scope, display_text, short_hash
		));
	} else {
		entry.push_str(&format!("- {} `{}`\n", display_text, short_hash));
	}

	entry
}

async fn generate_enhanced_changelog_with_ai(
	config: &Config,
	version: &VersionCalculation,
	analysis: &CommitAnalysis,
	project_type: &ProjectType,
	commit_range: &str,
) -> Result<String> {
	// First generate the standard changelog
	let standard_changelog = generate_changelog_content(version, analysis).await?;

	// Try to enhance with AI summary if LLM client can be created
	if let Ok(_client) = LlmClient::from_config(config) {
		match generate_ai_changelog_summary(config, analysis, project_type, commit_range).await {
			Ok(ai_summary) => {
				let mut enhanced = String::new();
				let date = chrono::Utc::now().format("%Y-%m-%d").to_string();

				enhanced.push_str(&format!("## [{}] - {}\n\n", version.new_version, date));

				if !ai_summary.trim().is_empty() {
					enhanced.push_str("### 📋 Release Summary\n\n");
					enhanced.push_str(&ai_summary);
					enhanced.push_str("\n\n");
				}

				// Add the detailed sections from standard changelog (skip the header)
				let lines: Vec<&str> = standard_changelog.lines().collect();
				let mut skip_header = true;
				for line in lines {
					if skip_header && line.starts_with("## [") {
						skip_header = false;
						continue;
					}
					if !skip_header && !line.trim().is_empty() {
						enhanced.push_str(line);
						enhanced.push('\n');
					} else if !skip_header {
						enhanced.push('\n');
					}
				}

				Ok(enhanced)
			}
			Err(_) => {
				// Fallback to standard changelog if AI enhancement fails
				Ok(standard_changelog)
			}
		}
	} else {
		Ok(standard_changelog)
	}
}

async fn gather_project_context(project_type: &ProjectType) -> Result<(String, String)> {
	let (name, description) = match project_type {
		ProjectType::Rust(cargo_path) => {
			let content = fs::read_to_string(cargo_path).unwrap_or_default();
			let name =
				extract_field_from_toml(&content, "name").unwrap_or("Unknown Project".to_string());
			let description = extract_field_from_toml(&content, "description")
				.unwrap_or("Rust project".to_string());
			(name, description)
		}
		ProjectType::Node(package_path) => {
			let content = fs::read_to_string(package_path).unwrap_or_default();
			if let Ok(package) = serde_json::from_str::<serde_json::Value>(&content) {
				let name = package
					.get("name")
					.and_then(|v| v.as_str())
					.unwrap_or("Unknown Project")
					.to_string();
				let description = package
					.get("description")
					.and_then(|v| v.as_str())
					.unwrap_or("Node.js project")
					.to_string();
				(name, description)
			} else {
				("Unknown Project".to_string(), "Node.js project".to_string())
			}
		}
		ProjectType::Php(composer_path) => {
			let content = fs::read_to_string(composer_path).unwrap_or_default();
			if let Ok(composer) = serde_json::from_str::<serde_json::Value>(&content) {
				let name = composer
					.get("name")
					.and_then(|v| v.as_str())
					.unwrap_or("Unknown Project")
					.to_string();
				let description = composer
					.get("description")
					.and_then(|v| v.as_str())
					.unwrap_or("PHP project")
					.to_string();
				(name, description)
			} else {
				("Unknown Project".to_string(), "PHP project".to_string())
			}
		}
		ProjectType::Go(_) => {
			// Try to get module name from go.mod
			let current_dir = std::env::current_dir().unwrap_or_default();
			let go_mod_path = current_dir.join("go.mod");
			if let Ok(content) = fs::read_to_string(&go_mod_path) {
				let name = content
					.lines()
					.find(|line| line.starts_with("module "))
					.and_then(|line| line.split_whitespace().nth(1))
					.unwrap_or("Unknown Project")
					.to_string();
				(name, "Go project".to_string())
			} else {
				("Unknown Project".to_string(), "Go project".to_string())
			}
		}
		ProjectType::Unknown => (
			"Unknown Project".to_string(),
			"Software project".to_string(),
		),
	};
	Ok((name, description))
}

fn extract_field_from_toml(content: &str, field: &str) -> Option<String> {
	for line in content.lines() {
		let trimmed = line.trim();
		if trimmed.starts_with(&format!("{} =", field)) {
			if let Some(start) = line.find('"') {
				if let Some(end) = line[start + 1..].find('"') {
					return Some(line[start + 1..start + 1 + end].to_string());
				}
			}
		}
	}
	None
}

async fn analyze_file_changes(commit_range: &str) -> Result<String> {
	let output = Command::new("git")
		.args(["diff", "--name-only", commit_range])
		.output()?;

	if !output.status.success() {
		return Ok("Unable to analyze file changes".to_string());
	}

	let files = String::from_utf8(output.stdout).unwrap_or_default();
	let file_list: Vec<&str> = files.lines().collect();

	if file_list.is_empty() {
		return Ok("No files changed".to_string());
	}

	// Categorize files by type/area
	let mut areas = Vec::new();
	let mut has_src = false;
	let mut has_docs = false;
	let mut has_config = false;
	let mut has_tests = false;

	for file in &file_list {
		if file.starts_with("src/")
			|| file.ends_with(".rs")
			|| file.ends_with(".js")
			|| file.ends_with(".ts")
			|| file.ends_with(".go")
			|| file.ends_with(".php")
		{
			has_src = true;
		} else if file.ends_with(".md") || file.starts_with("doc") {
			has_docs = true;
		} else if file.ends_with(".toml")
			|| file.ends_with(".json")
			|| file.ends_with(".yaml")
			|| file.ends_with(".yml")
		{
			has_config = true;
		} else if file.contains("test") || file.ends_with("_test.rs") || file.ends_with(".test.js")
		{
			has_tests = true;
		}
	}

	if has_src {
		areas.push("core functionality");
	}
	if has_docs {
		areas.push("documentation");
	}
	if has_config {
		areas.push("configuration");
	}
	if has_tests {
		areas.push("tests");
	}

	let area_summary = if areas.is_empty() {
		"miscellaneous files".to_string()
	} else {
		areas.join(", ")
	};

	Ok(format!(
		"{} files changed affecting: {}",
		file_list.len(),
		area_summary
	))
}

async fn generate_ai_changelog_summary(
	config: &Config,
	analysis: &CommitAnalysis,
	project_type: &ProjectType,
	commit_range: &str,
) -> Result<String> {
	// Gather enhanced context
	let (project_name, project_description) = gather_project_context(project_type).await?;
	let file_changes = analyze_file_changes(commit_range).await?;

	// Group commits by type for better summary context
	let mut breaking_msgs = Vec::new();
	let mut feature_msgs = Vec::new();
	let mut fix_msgs = Vec::new();
	let mut other_msgs = Vec::new();

	for commit in &analysis.commits {
		let msg = &commit.message;
		let short_hash = &commit.hash[..8];
		let msg_with_hash = format!("{} ({})", msg, short_hash);

		if commit.breaking {
			breaking_msgs.push(msg_with_hash);
		} else {
			match commit.commit_type.as_str() {
				"feat" => feature_msgs.push(msg_with_hash),
				"fix" => fix_msgs.push(msg_with_hash),
				_ => other_msgs.push(msg_with_hash),
			}
		}
	}

	let mut commits_context = String::new();

	if !breaking_msgs.is_empty() {
		commits_context.push_str("BREAKING CHANGES:\\n");
		for msg in &breaking_msgs {
			commits_context.push_str(&format!("- {}\\n", msg));
		}
		commits_context.push_str("\\n");
	}

	if !feature_msgs.is_empty() {
		commits_context.push_str("NEW FEATURES:\\n");
		for msg in &feature_msgs {
			commits_context.push_str(&format!("- {}\\n", msg));
		}
		commits_context.push_str("\\n");
	}

	if !fix_msgs.is_empty() {
		commits_context.push_str("BUG FIXES:\\n");
		for msg in &fix_msgs {
			commits_context.push_str(&format!("- {}\\n", msg));
		}
		commits_context.push_str("\\n");
	}

	if !other_msgs.is_empty() {
		commits_context.push_str("OTHER CHANGES:\\n");
		for msg in &other_msgs {
			commits_context.push_str(&format!("- {}\\n", msg));
		}
		commits_context.push_str("\\n");
	}

	let prompt = format!(
		"Generate a concise, professional release summary for {project_name}.\\n\\n\\
        PROJECT: {project_name} - {project_description}\\n\\
        SCOPE: {file_changes}\\n\\n\\
        COMMITS:\\n{commits_context}\\n\\
        REQUIREMENTS:\\n\\
        - Write 2-3 sentences maximum\\n\\
        - Focus on user-facing changes and improvements (not implementation details)\\n\\
        - Use professional, clear language suitable for end users\\n\\

        DEDUPLICATION & GROUPING RULES:\\n\\
        - NEVER repeat similar commits - group them together instead\\n\\
        - When multiple commits do similar things, combine them into ONE statement\\n\\
        - Reference multiple commits like: 'Enhanced search functionality (abc123f, def456g, hij789k)'\\n\\
        - Group by impact/feature, not by individual commit\\n\\
        - If commits are nearly identical, mention the improvement once with all commit references\\n\\

        MESSAGE REFINEMENT:\\n\\
        - You may IMPROVE and REFINE commit messages for clarity\\n\\
        - Don't preserve exact wording - make it user-friendly\\n\\
        - Focus on the RESULT/BENEFIT, not the technical implementation\\n\\
        - Combine multiple small changes into broader improvements\\n\\

        FORMATTING:\\n\\
        - Group similar changes together (e.g., 'Several bug fixes improve...')\\n\\
        - Prioritize: breaking changes → new features → improvements → bug fixes\\n\\
        - End with a period\\n\\
        - Create only a high-level summary for users, not developers\\n\\n\\

        Example: \\\"This release introduces multi-query search capabilities and enhanced memory management features (a1b2c3d, e4f5g6h). Performance improvements include optimized indexing with better batch processing and reduced memory usage (i7j8k9l, m0n1o2p). Several bug fixes improve search relevance, error handling, and system stability (q3r4s5t, u6v7w8x, y9z0a1b).\\\"\\n\\n\\

        Generate summary:",
		project_name = project_name,
		project_description = project_description,
		file_changes = file_changes,
		commits_context = commits_context
	);

	call_llm_for_version_calculation(&prompt, config).await
}

async fn update_project_version(project_type: &ProjectType, new_version: &str) -> Result<()> {
	match project_type {
		ProjectType::Rust(cargo_path) => {
			let content = fs::read_to_string(cargo_path)?;
			let updated_content = update_cargo_version(&content, new_version)?;
			fs::write(cargo_path, updated_content)?;
		}
		ProjectType::Node(package_path) => {
			let content = fs::read_to_string(package_path)?;
			let updated_content = update_json_version(&content, new_version, "version")?;
			fs::write(package_path, updated_content)?;
		}
		ProjectType::Php(composer_path) => {
			let content = fs::read_to_string(composer_path)?;
			let updated_content = update_json_version(&content, new_version, "version")?;
			fs::write(composer_path, updated_content)?;
		}
		ProjectType::Go(go_mod_path) => {
			// For Go projects, create/update a VERSION file in the same directory as go.mod
			let version_file = go_mod_path.parent().unwrap().join("VERSION");
			fs::write(version_file, new_version)?;
		}
		ProjectType::Unknown => {
			// No project file to update
		}
	}
	Ok(())
}

async fn update_lock_files(project_type: &ProjectType) -> Result<()> {
	match project_type {
		ProjectType::Rust(_) => {
			// Update Cargo.lock by running cargo check
			println!("🔄 Updating Cargo.lock...");
			let output = Command::new("cargo").args(["check", "--quiet"]).output()?;

			if !output.status.success() {
				return Err(anyhow::anyhow!(
					"Failed to update Cargo.lock: {}",
					String::from_utf8_lossy(&output.stderr)
				));
			}
		}
		ProjectType::Node(_) => {
			// Update package-lock.json or yarn.lock
			println!("🔄 Updating Node.js lock file...");

			// Check if using yarn or npm
			let current_dir = std::env::current_dir()?;
			if current_dir.join("yarn.lock").exists() {
				let output = Command::new("yarn")
					.args(["install", "--frozen-lockfile"])
					.output()?;

				if !output.status.success() {
					return Err(anyhow::anyhow!(
						"Failed to update yarn.lock: {}",
						String::from_utf8_lossy(&output.stderr)
					));
				}
			} else {
				let output = Command::new("npm")
					.args(["install", "--package-lock-only"])
					.output()?;

				if !output.status.success() {
					return Err(anyhow::anyhow!(
						"Failed to update package-lock.json: {}",
						String::from_utf8_lossy(&output.stderr)
					));
				}
			}
		}
		ProjectType::Php(_) => {
			// Update composer.lock
			println!("🔄 Updating composer.lock...");
			let output = Command::new("composer")
				.args(["update", "--lock"])
				.output()?;

			if !output.status.success() {
				return Err(anyhow::anyhow!(
					"Failed to update composer.lock: {}",
					String::from_utf8_lossy(&output.stderr)
				));
			}
		}
		ProjectType::Go(_) => {
			// Update go.sum and go.mod
			println!("🔄 Updating go.mod and go.sum...");
			let output = Command::new("go").args(["mod", "tidy"]).output()?;

			if !output.status.success() {
				return Err(anyhow::anyhow!(
					"Failed to update go.mod/go.sum: {}",
					String::from_utf8_lossy(&output.stderr)
				));
			}
		}
		ProjectType::Unknown => {
			// No lock file to update
		}
	}
	Ok(())
}

fn update_cargo_version(content: &str, new_version: &str) -> Result<String> {
	// Find the version line and replace only the version value, preserving all formatting
	let mut result = content.to_string();

	// Look for the version line in the [package] section
	let lines: Vec<&str> = content.lines().collect();
	let mut in_package_section = false;

	for (i, line) in lines.iter().enumerate() {
		let trimmed = line.trim();

		// Check if we're entering the [package] section
		if trimmed == "[package]" {
			in_package_section = true;
			continue;
		}

		// Check if we're leaving the [package] section
		if trimmed.starts_with('[') && trimmed != "[package]" {
			in_package_section = false;
			continue;
		}

		// Look for version line in [package] section
		if in_package_section && line.trim_start().starts_with("version") && line.contains('=') {
			if let Some(equals_pos) = line.find('=') {
				let prefix = &line[..equals_pos + 1];
				let suffix_part = &line[equals_pos + 1..];

				// Find the start of the value (skip whitespace)
				let value_start = suffix_part.len() - suffix_part.trim_start().len();
				let value_part = suffix_part.trim_start();

				// Determine quote style and extract current version
				let (quote_char, new_value_part) = if value_part.starts_with('"') {
					('"', format!("\"{}\"", new_version))
				} else if value_part.starts_with('\'') {
					('\'', format!("'{}'", new_version))
				} else {
					// No quotes, just replace the value
					(' ', new_version.to_string())
				};

				// Find the end of the current version value
				let value_end = if quote_char == ' ' {
					// For unquoted values, find the end of the word
					value_part
						.find(char::is_whitespace)
						.unwrap_or(value_part.len())
				} else {
					// For quoted values, find the closing quote
					if let Some(end_quote) = value_part[1..].find(quote_char) {
						end_quote + 2 // +1 for the quote, +1 for 0-based indexing
					} else {
						value_part.len()
					}
				};

				// Construct the new line
				let before_value = &suffix_part[..value_start];
				let after_value = &suffix_part[value_start + value_end..];
				let new_line = format!(
					"{}{}{}{}",
					prefix, before_value, new_value_part, after_value
				);

				// Replace the entire line in the result
				let line_start = lines[..i].iter().map(|l| l.len() + 1).sum::<usize>();
				let line_end = line_start + line.len();
				result.replace_range(line_start..line_end, &new_line);
				break;
			}
		}
	}

	Ok(result)
}

fn update_json_version(content: &str, new_version: &str, field_name: &str) -> Result<String> {
	// Find and replace the version field value while preserving all formatting
	let field_pattern = format!("\"{}\"", field_name);
	let mut result = content.to_string();

	// Find the field in the JSON
	if let Some(field_start) = content.find(&field_pattern) {
		// Find the colon after the field name
		let search_start = field_start + field_pattern.len();
		if let Some(colon_pos) = content[search_start..].find(':') {
			let colon_abs_pos = search_start + colon_pos;

			// Find the start of the value (skip whitespace after colon)
			let after_colon = &content[colon_abs_pos + 1..];
			let value_start_offset = after_colon.len() - after_colon.trim_start().len();
			let value_start = colon_abs_pos + 1 + value_start_offset;

			// Find the actual value part
			let value_part = after_colon.trim_start();

			if let Some(stripped) = value_part.strip_prefix('"') {
				// Handle double-quoted string
				if let Some(end_quote) = stripped.find('"') {
					let value_end = value_start + 1 + end_quote + 1; // +1 for opening quote, +1 for closing quote
					let new_value = format!("\"{}\"", new_version);
					result.replace_range(value_start..value_end, &new_value);
				}
			} else if let Some(stripped) = value_part.strip_prefix('\'') {
				// Handle single-quoted string (less common in JSON but possible)
				if let Some(end_quote) = stripped.find('\'') {
					let value_end = value_start + 1 + end_quote + 1;
					let new_value = format!("'{}'", new_version);
					result.replace_range(value_start..value_end, &new_value);
				}
			}
		}
	}

	Ok(result)
}

async fn update_changelog(changelog_path: &str, new_content: &str) -> Result<()> {
	let changelog_path = Path::new(changelog_path);

	if changelog_path.exists() {
		// Read existing changelog as bytes to preserve exact formatting
		let existing_bytes = fs::read(changelog_path)?;
		let existing_content = String::from_utf8_lossy(&existing_bytes);

		// Detect original line ending style and file ending
		let has_final_newline =
			existing_content.ends_with('\n') || existing_content.ends_with("\r\n");
		let line_ending = if existing_content.contains("\r\n") {
			"\r\n"
		} else {
			"\n"
		};

		// Find where to insert new content (after the first heading)
		let lines: Vec<&str> = existing_content.lines().collect();
		let mut insert_index = 0;

		// Skip the main title if it exists
		for (i, line) in lines.iter().enumerate() {
			if line.starts_with("# ") {
				insert_index = i + 1;
				// Skip any blank lines after title
				while insert_index < lines.len() && lines[insert_index].trim().is_empty() {
					insert_index += 1;
				}
				break;
			}
		}

		// Insert new content while preserving original formatting
		let mut new_lines = Vec::new();
		for (i, line) in lines.iter().enumerate() {
			if i == insert_index {
				// Add new content with proper line ending
				new_lines.push(new_content.trim_end().to_string());
				new_lines.push("".to_string());
			}
			new_lines.push(line.to_string());
		}

		// Join with original line ending and preserve final newline if it existed
		let mut updated_content = new_lines.join(line_ending);
		if has_final_newline
			&& !updated_content.ends_with('\n')
			&& !updated_content.ends_with("\r\n")
		{
			updated_content.push_str(line_ending);
		}

		fs::write(changelog_path, updated_content)?;
	} else {
		// Create new changelog with proper formatting
		let content = format!(
            "# Changelog\n\nAll notable changes to this project will be documented in this file.\n\n{}\n",
            new_content.trim_end()
        );
		fs::write(changelog_path, content)?;
	}

	Ok(())
}

async fn stage_release_files(changelog_path: &str, project_type: &ProjectType) -> Result<()> {
	let mut files_to_stage = vec![changelog_path.to_string()];

	// Add project files and lock files
	let current_dir = std::env::current_dir()?;
	match project_type {
		ProjectType::Rust(path) => {
			files_to_stage.push(path.to_string_lossy().to_string());
			// Add Cargo.lock if it exists
			let cargo_lock = current_dir.join("Cargo.lock");
			if cargo_lock.exists() {
				files_to_stage.push(cargo_lock.to_string_lossy().to_string());
			}
		}
		ProjectType::Node(path) => {
			files_to_stage.push(path.to_string_lossy().to_string());
			// Add package-lock.json or yarn.lock if they exist
			let package_lock = current_dir.join("package-lock.json");
			let yarn_lock = current_dir.join("yarn.lock");
			if package_lock.exists() {
				files_to_stage.push(package_lock.to_string_lossy().to_string());
			}
			if yarn_lock.exists() {
				files_to_stage.push(yarn_lock.to_string_lossy().to_string());
			}
		}
		ProjectType::Php(path) => {
			files_to_stage.push(path.to_string_lossy().to_string());
			// Add composer.lock if it exists
			let composer_lock = current_dir.join("composer.lock");
			if composer_lock.exists() {
				files_to_stage.push(composer_lock.to_string_lossy().to_string());
			}
		}
		ProjectType::Go(go_mod_path) => {
			// Stage the VERSION file for Go projects
			let version_file = go_mod_path.parent().unwrap().join("VERSION");
			files_to_stage.push(version_file.to_string_lossy().to_string());
			// Add go.mod and go.sum if they exist
			let go_mod = current_dir.join("go.mod");
			let go_sum = current_dir.join("go.sum");
			if go_mod.exists() {
				files_to_stage.push(go_mod.to_string_lossy().to_string());
			}
			if go_sum.exists() {
				files_to_stage.push(go_sum.to_string_lossy().to_string());
			}
		}
		ProjectType::Unknown => {}
	}

	for file in files_to_stage {
		let output = Command::new("git").args(["add", &file]).output()?;

		if !output.status.success() {
			return Err(anyhow::anyhow!(
				"Failed to stage {}: {}",
				file,
				String::from_utf8_lossy(&output.stderr)
			));
		}
	}

	Ok(())
}

async fn create_commit(message: &str) -> Result<()> {
	let output = Command::new("git")
		.args(["commit", "-m", message])
		.output()?;

	if !output.status.success() {
		return Err(anyhow::anyhow!(
			"Failed to create commit: {}",
			String::from_utf8_lossy(&output.stderr)
		));
	}

	Ok(())
}

async fn create_tag(version: &str, changelog_content: &str) -> Result<()> {
	// Create annotated tag with changelog content as message
	let tag_message = format!("Release {}\n\n{}", version, changelog_content.trim());

	let output = Command::new("git")
		.args(["tag", "-a", version, "-m", &tag_message])
		.output()?;

	if !output.status.success() {
		return Err(anyhow::anyhow!(
			"Failed to create tag: {}",
			String::from_utf8_lossy(&output.stderr)
		));
	}

	Ok(())
}
