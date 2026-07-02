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

// Octocode - Intelligent Code Indexer and Graph Builder
// Copyright (c) 2025 Muvon Un Limited
// Licensed under the MIT License

use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use clap::Args;

mod utils;

use utils::*;

#[derive(Args)]
pub struct FormatArgs {
	/// Show what would be changed without applying (dry-run mode)
	#[arg(long)]
	pub dry_run: bool,

	/// Commit changes after formatting
	#[arg(short, long)]
	pub commit: bool,

	/// Specific files to format (default: all git-tracked and unstaged files)
	pub files: Vec<PathBuf>,

	/// Show verbose output
	#[arg(short, long)]
	pub verbose: bool,
}

pub async fn execute(format_args: &FormatArgs) -> Result<()> {
	let git_root = find_git_root()
		.context("Failed to find git repository root. Make sure you're in a git repository.")?;

	let editorconfig_path = git_root.join(".editorconfig");

	if !editorconfig_path.exists() {
		return Err(anyhow!(
			".editorconfig file not found in git root: {}",
			git_root.display()
		));
	}

	if format_args.verbose {
		println!("Found .editorconfig at: {}", editorconfig_path.display());
		println!("Git root: {}", git_root.display());
	}

	let files_to_format = if format_args.files.is_empty() {
		get_git_files(&git_root)?
	} else {
		// Convert relative paths to absolute and validate they exist
		format_args
			.files
			.iter()
			.map(|f| {
				if f.is_absolute() {
					f.clone()
				} else {
					git_root.join(f)
				}
			})
			.filter(|f| f.exists())
			.collect()
	};

	if files_to_format.is_empty() {
		println!("No files to format found.");
		return Ok(());
	}

	if format_args.verbose {
		println!("Found {} files to process", files_to_format.len());
	}

	let mut formatted_files = Vec::new();
	let mut total_changes = 0;

	for file_path in &files_to_format {
		if format_args.verbose {
			println!("Processing: {}", file_path.display());
		}

		let changes = format_file(file_path, !format_args.dry_run, format_args.verbose)
			.with_context(|| format!("Failed to format file: {}", file_path.display()))?;

		if changes > 0 {
			formatted_files.push(file_path.clone());
			total_changes += changes;
		}
	}

	if total_changes == 0 {
		println!("No formatting changes needed.");
		return Ok(());
	}

	let action = if format_args.dry_run {
		"would be applied"
	} else {
		"applied"
	};
	println!(
		"Formatting complete: {} changes across {} files ({})",
		total_changes,
		formatted_files.len(),
		action
	);

	if !format_args.dry_run && format_args.commit {
		commit_changes(&formatted_files)?;
	}

	Ok(())
}

fn commit_changes(files: &[PathBuf]) -> Result<()> {
	// Add files to git
	for file in files {
		let output = Command::new("git")
			.args(["add", "--", &file.to_string_lossy()])
			.output()
			.context("Failed to execute git add command")?;

		if !output.status.success() {
			return Err(anyhow!(
				"Git add failed for {}: {}",
				file.display(),
				String::from_utf8_lossy(&output.stderr)
			));
		}
	}

	// Create commit message
	let commit_message = "Format code according to .editorconfig";

	// Commit changes
	let output = Command::new("git")
		.args(["commit", "-m", commit_message])
		.output()
		.context("Failed to execute git commit command")?;

	if !output.status.success() {
		return Err(anyhow!(
			"Git commit failed: {}",
			String::from_utf8_lossy(&output.stderr)
		));
	}

	println!("Successfully committed formatting changes:");
	println!("  Message: {}", commit_message);
	println!("  Files: {}", files.len());

	Ok(())
}
