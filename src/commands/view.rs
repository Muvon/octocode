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

use octocode::indexer;
use octocode::storage;

use crate::commands::OutputFormat;

#[derive(Args, Debug)]
pub struct ViewArgs {
	/// Files to view (may include glob patterns)
	pub files: Vec<String>,

	/// Output format
	#[arg(long, value_enum, default_value = "cli")]
	pub format: OutputFormat,
}

pub async fn execute(args: &ViewArgs) -> Result<(), anyhow::Error> {
	// Get current directory
	let current_dir = std::env::current_dir()?;

	// Note: View command doesn't require an index as it parses files directly
	let index_path = storage::get_project_database_path(&current_dir)?;
	if !index_path.exists() {
		println!("Note: No index found. The view command works without an index, but you can run 'octocode index' to create one if needed for other commands.");
	}

	// Get files matching patterns
	let mut matching_files = Vec::new();

	for pattern in &args.files {
		// Use glob pattern matching
		let glob_pattern = match globset::Glob::new(pattern) {
			Ok(g) => g.compile_matcher(),
			Err(e) => {
				println!("Invalid glob pattern '{}': {}", pattern, e);
				continue;
			}
		};

		// Use NoindexWalker to respect both .gitignore and .noindex files while finding files
		let walker = indexer::NoindexWalker::create_walker(&current_dir).build();

		for result in walker {
			let entry = match result {
				Ok(entry) => entry,
				Err(_) => continue,
			};

			// Skip directories, only process files
			if !entry.file_type().is_some_and(|ft| ft.is_file()) {
				continue;
			}

			// See if this file matches our pattern
			let relative_path = indexer::PathUtils::to_relative_string(entry.path(), &current_dir);
			if glob_pattern.is_match(&relative_path) {
				matching_files.push(entry.path().to_path_buf());
			}
		}
	}

	if matching_files.is_empty() {
		println!("No matching files found.");
		return Ok(());
	}

	// Extract signatures from matching files
	let signatures = indexer::extract_file_signatures(&matching_files)?;

	// Display results in the requested format
	if args.format.is_json() {
		indexer::render_signatures_json(&signatures)?
	} else if args.format.is_md() {
		// Use markdown format
		let markdown = indexer::signatures_to_markdown(&signatures);
		println!("{}", markdown);
	} else if args.format.is_text() {
		// Use text format for token efficiency
		indexer::render_signatures_text(&signatures);
	} else {
		indexer::render_signatures_text(&signatures);
	}

	Ok(())
}
