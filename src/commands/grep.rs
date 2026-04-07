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
use octocode::grep;
use std::collections::HashMap;
use std::path::Path;

#[derive(Args, Debug)]
pub struct GrepArgs {
	/// AST pattern to search for (e.g. '$FUNC.unwrap()', 'if err != nil { $$$ }')
	pub pattern: String,

	/// Language to search (e.g. rust, javascript, python, go)
	#[arg(short, long)]
	pub lang: Option<String>,

	/// File paths or glob patterns to search
	#[arg(short, long)]
	pub paths: Vec<String>,

	/// Number of context lines around matches
	#[arg(short = 'C', long, default_value = "0")]
	pub context: usize,

	/// Output as JSON
	#[arg(long)]
	pub json: bool,
}

pub async fn execute(args: &GrepArgs) -> Result<(), anyhow::Error> {
	let current_dir = std::env::current_dir()?;

	// Collect files to search
	let files = collect_files(&current_dir, &args.paths, args.lang.as_deref())?;

	if files.is_empty() {
		eprintln!("No files found matching the specified criteria.");
		return Ok(());
	}

	let mut all_matches = Vec::new();
	let mut source_map: HashMap<String, String> = HashMap::new();

	for file_path in &files {
		let path = Path::new(file_path);
		let language = if let Some(ref lang) = args.lang {
			lang.as_str()
		} else if let Some(lang) = grep::language_from_extension(path) {
			lang
		} else {
			continue;
		};

		let source = match std::fs::read_to_string(path) {
			Ok(s) => s,
			Err(_) => continue,
		};

		// Use relative path for display
		let display_path = path
			.strip_prefix(&current_dir)
			.unwrap_or(path)
			.to_string_lossy()
			.to_string();

		match grep::search_file(&display_path, &source, &args.pattern, language) {
			Ok(matches) => {
				if args.context > 0 && !matches.is_empty() {
					source_map.insert(display_path.clone(), source);
				}
				all_matches.extend(matches);
			}
			Err(e) => {
				eprintln!("Error searching {}: {}", display_path, e);
			}
		}
	}

	if all_matches.is_empty() {
		println!("No matches found.");
		return Ok(());
	}

	if args.json {
		let json_matches: Vec<serde_json::Value> = all_matches
			.iter()
			.map(|m| {
				serde_json::json!({
					"file": m.file,
					"line": m.line,
					"column": m.column,
					"text": m.text,
				})
			})
			.collect();
		println!("{}", serde_json::to_string_pretty(&json_matches)?);
	} else if args.context > 0 {
		println!(
			"{}",
			grep::format_matches_with_context(&all_matches, &source_map, args.context)
		);
	} else {
		println!("{}", grep::format_matches_grouped(&all_matches));
	}

	eprintln!("\n{} matches found.", all_matches.len());
	Ok(())
}

/// Collect files to search based on paths/globs and language filter.
fn collect_files(
	base_dir: &Path,
	paths: &[String],
	language: Option<&str>,
) -> Result<Vec<String>, anyhow::Error> {
	let mut files = Vec::new();

	if paths.is_empty() {
		// Walk current directory respecting gitignore
		let walker = ignore::WalkBuilder::new(base_dir)
			.git_ignore(true)
			.git_global(true)
			.git_exclude(true)
			.hidden(true)
			.build();

		for entry in walker.flatten() {
			if entry.file_type().is_some_and(|ft| ft.is_file()) {
				let path = entry.path();
				if let Some(lang) = language {
					if grep::language_from_extension(path) == Some(lang) {
						files.push(path.to_string_lossy().to_string());
					}
				} else if grep::language_from_extension(path).is_some() {
					files.push(path.to_string_lossy().to_string());
				}
			}
		}
	} else {
		for pattern in paths {
			let path = Path::new(pattern);
			if path.is_file() {
				files.push(base_dir.join(pattern).to_string_lossy().to_string());
			} else {
				// Glob pattern — walk directory with filter
				let matcher = globset::Glob::new(pattern)
					.map_err(|e| anyhow::anyhow!("Invalid glob pattern '{}': {}", pattern, e))?
					.compile_matcher();

				let walker = ignore::WalkBuilder::new(base_dir)
					.git_ignore(true)
					.git_global(true)
					.git_exclude(true)
					.hidden(true)
					.build();

				for entry in walker.flatten() {
					if entry.file_type().is_some_and(|ft| ft.is_file()) {
						let entry_path = entry.path();
						let rel = entry_path.strip_prefix(base_dir).unwrap_or(entry_path);
						if matcher.is_match(rel) {
							if let Some(lang) = language {
								if grep::language_from_extension(entry_path) == Some(lang) {
									files.push(entry_path.to_string_lossy().to_string());
								}
							} else {
								files.push(entry_path.to_string_lossy().to_string());
							}
						}
					}
				}
			}
		}
	}

	Ok(files)
}
