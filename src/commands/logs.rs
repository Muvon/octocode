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
use std::path::{Path, PathBuf};

#[derive(Args, Debug)]
pub struct LogsArgs {
	/// Show logs for all projects
	#[arg(long)]
	pub all: bool,

	/// Follow log output (tail -f style)
	#[arg(long, short)]
	pub follow: bool,

	/// Number of lines to show from the end
	#[arg(long, short, default_value = "100")]
	pub lines: usize,

	/// Show only error level logs
	#[arg(long)]
	pub errors_only: bool,
}

pub async fn execute(args: &LogsArgs) -> Result<(), anyhow::Error> {
	let current_dir = std::env::current_dir()?;

	if args.all {
		show_all_project_logs(&current_dir).await
	} else {
		show_current_project_logs(&current_dir, args).await
	}
}

async fn show_current_project_logs(base_dir: &Path, args: &LogsArgs) -> Result<(), anyhow::Error> {
	use octocode::mcp::logging::get_all_log_directories;

	let log_dirs = get_all_log_directories(base_dir)?;

	if log_dirs.is_empty() {
		println!("No MCP server logs found for this project.");
		println!("Logs are created when the MCP server is started.");
		return Ok(());
	}

	// Use the most recent log directory
	let latest_log_dir = &log_dirs[0];
	println!("Showing logs from: {}", latest_log_dir.display());

	show_logs_from_directory(latest_log_dir, args).await
}

async fn show_all_project_logs(base_dir: &Path) -> Result<(), anyhow::Error> {
	use octocode::mcp::logging::print_log_directories;

	println!("All MCP Server Log Directories:");
	print_log_directories(base_dir)?;

	Ok(())
}

async fn show_logs_from_directory(log_dir: &PathBuf, args: &LogsArgs) -> Result<(), anyhow::Error> {
	use std::process::Command;

	// Find the most recent log file
	let mut log_files: Vec<_> = std::fs::read_dir(log_dir)?
		.filter_map(|entry| {
			let path = entry.ok()?.path();
			if path.extension().and_then(|s| s.to_str()) == Some("log")
				|| path
					.file_name()
					.and_then(|s| s.to_str())
					.map(|s| s.contains("mcp_server"))
					.unwrap_or(false)
			{
				Some(path)
			} else {
				None
			}
		})
		.collect();

	if log_files.is_empty() {
		println!("No log files found in {}", log_dir.display());
		return Ok(());
	}

	// Sort by modification time (newest first)
	log_files.sort_by(|a, b| {
		let a_time = a
			.metadata()
			.and_then(|m| m.modified())
			.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
		let b_time = b
			.metadata()
			.and_then(|m| m.modified())
			.unwrap_or(std::time::SystemTime::UNIX_EPOCH);
		b_time.cmp(&a_time)
	});

	let log_file = &log_files[0];
	println!("Reading from: {}", log_file.display());

	if args.follow {
		use std::io::{BufRead, BufReader};
		use std::process::Stdio;

		// Use tail -f equivalent
		let mut cmd = Command::new("tail");
		cmd.arg("-f")
			.arg("-n")
			.arg(args.lines.to_string())
			.arg(log_file);

		if args.errors_only {
			// Filter tail's stream in-process. A shell `|` can't be expressed via
			// Command (it runs a single program, not a pipeline), so read tail's
			// stdout line by line and drop everything that isn't an error line.
			cmd.stdout(Stdio::piped());
			let mut child = cmd.spawn()?;
			let stdout = child
				.stdout
				.take()
				.ok_or_else(|| anyhow::anyhow!("Failed to capture tail output"))?;
			for line in BufReader::new(stdout).lines() {
				let line = line?;
				let lower = line.to_lowercase();
				if lower.contains("error") || lower.contains("critical") {
					println!("{}", line);
				}
			}
			let status = child.wait()?;
			if !status.success() {
				eprintln!("Failed to tail log file");
			}
		} else {
			let status = cmd.status()?;
			if !status.success() {
				eprintln!("Failed to tail log file");
			}
		}
	} else {
		// Read last N lines
		let content = std::fs::read_to_string(log_file)?;
		let lines: Vec<&str> = content.lines().collect();
		let start_idx = if lines.len() > args.lines {
			lines.len() - args.lines
		} else {
			0
		};

		for line in &lines[start_idx..] {
			if args.errors_only {
				if line.to_lowercase().contains("error") || line.to_lowercase().contains("critical")
				{
					println!("{}", line);
				}
			} else {
				println!("{}", line);
			}
		}
	}

	Ok(())
}
