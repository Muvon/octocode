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

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use ec4rs::property::{EndOfLine, IndentSize, IndentStyle, TabWidth};
use ec4rs::{properties_of, Properties};

pub(super) fn find_git_root() -> Result<PathBuf> {
	let current_dir = std::env::current_dir().context("Failed to get current directory")?;

	let mut path = current_dir.as_path();

	loop {
		if path.join(".git").exists() {
			return Ok(path.to_path_buf());
		}

		match path.parent() {
			Some(parent) => path = parent,
			None => return Err(anyhow!("Not in a git repository")),
		}
	}
}

pub(super) fn get_git_files(git_root: &Path) -> Result<Vec<PathBuf>> {
	let mut files = Vec::new();

	// Get all tracked files
	let output = Command::new("git")
		.args(["ls-files"])
		.current_dir(git_root)
		.output()
		.context("Failed to execute 'git ls-files'")?;

	if !output.status.success() {
		return Err(anyhow!(
			"Git ls-files failed: {}",
			String::from_utf8_lossy(&output.stderr)
		));
	}

	let tracked_files = String::from_utf8_lossy(&output.stdout);
	for line in tracked_files.lines() {
		if !line.trim().is_empty() {
			files.push(git_root.join(line.trim()));
		}
	}

	// Get untracked files that are not ignored
	let output = Command::new("git")
		.args(["status", "--porcelain", "--untracked-files=all"])
		.current_dir(git_root)
		.output()
		.context("Failed to execute 'git status'")?;

	if !output.status.success() {
		return Err(anyhow!(
			"Git status failed: {}",
			String::from_utf8_lossy(&output.stderr)
		));
	}

	let status_output = String::from_utf8_lossy(&output.stdout);
	for line in status_output.lines() {
		if line.len() >= 3 {
			let status = &line[0..2];
			let file_path = &line[3..];

			// Include untracked files (status starts with ??)
			if status == "??" {
				let full_path = git_root.join(file_path.trim());
				if full_path.exists() && full_path.is_file() {
					files.push(full_path);
				}
			}
		}
	}

	// Filter out files that should be ignored (check if git would ignore them)
	let mut final_files = Vec::new();
	for file in files {
		if is_text_file(&file)? {
			final_files.push(file);
		}
	}

	Ok(final_files)
}

pub(super) fn is_text_file(file_path: &Path) -> Result<bool> {
	// Check if git considers this file as text
	let output = Command::new("git")
		.args(["check-attr", "--all", &file_path.to_string_lossy()])
		.output()
		.context("Failed to execute 'git check-attr'")?;

	if !output.status.success() {
		// If git check-attr fails, fall back to simple heuristics
		return Ok(is_likely_text_file(file_path));
	}

	let attr_output = String::from_utf8_lossy(&output.stdout);

	// Check if file is marked as binary
	if attr_output.contains("binary: set") || attr_output.contains("binary: true") {
		return Ok(false);
	}

	// If no binary attribute, assume it's text if it has a reasonable extension or passes heuristic
	Ok(is_likely_text_file(file_path))
}

pub(super) fn is_likely_text_file(file_path: &Path) -> bool {
	// Common text file extensions
	let text_extensions = [
		"rs",
		"py",
		"js",
		"ts",
		"jsx",
		"tsx",
		"go",
		"java",
		"kt",
		"scala",
		"cpp",
		"c",
		"h",
		"hpp",
		"cc",
		"cxx",
		"cs",
		"php",
		"rb",
		"pl",
		"pm",
		"sh",
		"bash",
		"zsh",
		"fish",
		"ps1",
		"bat",
		"cmd",
		"html",
		"htm",
		"xml",
		"xhtml",
		"svg",
		"css",
		"scss",
		"sass",
		"less",
		"json",
		"yaml",
		"yml",
		"toml",
		"ini",
		"cfg",
		"conf",
		"config",
		"md",
		"markdown",
		"rst",
		"txt",
		"text",
		"rtf",
		"sql",
		"ddl",
		"dml",
		"graphql",
		"gql",
		"dockerfile",
		"makefile",
		"cmake",
		"gradle",
		"maven",
		"vue",
		"svelte",
		"astro",
		"ejs",
		"hbs",
		"mustache",
		"r",
		"m",
		"swift",
		"dart",
		"lua",
		"nim",
		"zig",
		"v",
	];

	if let Some(extension) = file_path.extension() {
		let ext = extension.to_string_lossy().to_lowercase();
		if text_extensions.contains(&ext.as_str()) {
			return true;
		}
	}

	// Check filename patterns
	let filename = file_path
		.file_name()
		.map(|n| n.to_string_lossy().to_lowercase())
		.unwrap_or_default();

	let text_filenames = [
		"dockerfile",
		"makefile",
		"rakefile",
		"gemfile",
		"podfile",
		"license",
		"readme",
		"changelog",
		"authors",
		"contributors",
		"copying",
		"install",
		"news",
		"todo",
		"version",
		".gitignore",
		".gitattributes",
		".editorconfig",
		".eslintrc",
		".prettierrc",
		".babelrc",
		".nvmrc",
		".rustfmt.toml",
	];

	for pattern in &text_filenames {
		if filename.contains(pattern) {
			return true;
		}
	}

	// If no extension and not a known filename, check if file starts with shebang
	if file_path.extension().is_none() {
		if let Ok(content) = fs::read_to_string(file_path) {
			if content.starts_with("#!") {
				return true;
			}
		}
	}

	false
}

pub(super) fn format_file(file_path: &Path, apply: bool, verbose: bool) -> Result<usize> {
	let content = fs::read_to_string(file_path)
		.with_context(|| format!("Failed to read file: {}", file_path.display()))?;

	// Get EditorConfig properties for this specific file
	let properties = properties_of(file_path).map_err(|e| {
		anyhow!(
			"Failed to get editorconfig properties for {}: {}",
			file_path.display(),
			e
		)
	})?;

	if verbose {
		println!("  EditorConfig properties for {}:", file_path.display());

		// Display parsed properties
		if let Ok(charset) = properties.get::<ec4rs::property::Charset>() {
			println!("    charset: {}", charset);
		}
		if let Ok(end_of_line) = properties.get::<EndOfLine>() {
			println!("    end_of_line: {:?}", end_of_line);
		}
		if let Ok(indent_style) = properties.get::<IndentStyle>() {
			println!("    indent_style: {:?}", indent_style);
		}
		if let Ok(indent_size) = properties.get::<IndentSize>() {
			println!("    indent_size: {:?}", indent_size);
		}
		if let Ok(tab_width) = properties.get::<TabWidth>() {
			println!("    tab_width: {:?}", tab_width);
		}
		if let Ok(insert_final_newline) = properties.get::<ec4rs::property::FinalNewline>() {
			println!("    insert_final_newline: {}", insert_final_newline);
		}
		if let Ok(trim_trailing_whitespace) = properties.get::<ec4rs::property::TrimTrailingWs>() {
			println!("    trim_trailing_whitespace: {}", trim_trailing_whitespace);
		}
		if let Ok(max_line_length) = properties.get::<ec4rs::property::MaxLineLen>() {
			println!("    max_line_length: {:?}", max_line_length);
		}
	}

	let mut changes = 0;
	let mut new_content = content.clone();

	// Apply EditorConfig rules in the correct order

	// 1. Handle line endings first
	if let Ok(line_ending) = properties.get::<EndOfLine>() {
		let target_ending = match line_ending {
			EndOfLine::Lf => "\n",
			EndOfLine::CrLf => "\r\n",
			EndOfLine::Cr => "\r",
		};

		// Normalize all line endings to \n first, then apply target
		let normalized = new_content.replace("\r\n", "\n").replace('\r', "\n");
		let with_target_endings = if target_ending != "\n" {
			normalized.replace('\n', target_ending)
		} else {
			normalized
		};

		if with_target_endings != new_content {
			new_content = with_target_endings;
			changes += 1;
			if verbose {
				println!("  - Fixed line endings to {:?}", line_ending);
			}
		}
	}

	// 2. Handle character encoding (verify UTF-8)
	if let Ok(charset) = properties.get::<ec4rs::property::Charset>() {
		// Content is already UTF-8 since we read it as String
		if verbose {
			println!("  - Verified charset: {}", charset);
		}
	}

	// 3. Handle indentation
	if let Ok(indent_style) = properties.get::<IndentStyle>() {
		let indent_size = get_effective_indent_size(&properties);
		if let Ok(indented_content) =
			apply_indentation(&new_content, indent_style, indent_size, verbose)
		{
			if indented_content != new_content {
				new_content = indented_content;
				changes += 1;
			}
		}
	}

	// 4. Handle trailing whitespace
	if let Ok(ec4rs::property::TrimTrailingWs::Value(true)) =
		properties.get::<ec4rs::property::TrimTrailingWs>()
	{
		let trimmed_content = trim_trailing_whitespace(&new_content);
		if trimmed_content != new_content {
			new_content = trimmed_content;
			changes += 1;
			if verbose {
				println!("  - Trimmed trailing whitespace");
			}
		}
	}

	// 5. Handle final newline
	if let Ok(final_newline) = properties.get::<ec4rs::property::FinalNewline>() {
		let ec4rs::property::FinalNewline::Value(insert_final_newline) = final_newline;
		let processed_content = handle_final_newline(&new_content, insert_final_newline);
		if processed_content != new_content {
			new_content = processed_content;
			changes += 1;
			if verbose {
				if insert_final_newline {
					println!("  - Added final newline");
				} else {
					println!("  - Removed final newline");
				}
			}
		}
	}

	// 6. Handle max line length (optional warning)
	if let Ok(max_line_length) = properties.get::<ec4rs::property::MaxLineLen>() {
		match max_line_length {
			ec4rs::property::MaxLineLen::Value(length) => {
				check_line_length(&new_content, length as u32, file_path, verbose);
			}
			ec4rs::property::MaxLineLen::Off => {
				// No line length limit
			}
		}
	}

	// Apply changes if requested
	if apply && changes > 0 {
		fs::write(file_path, &new_content)
			.with_context(|| format!("Failed to write file: {}", file_path.display()))?;
	}

	Ok(changes)
}

pub(super) fn get_effective_indent_size(properties: &Properties) -> usize {
	// Try indent_size first, fall back to tab_width, default to 2
	if let Ok(indent_size) = properties.get::<IndentSize>() {
		match indent_size {
			IndentSize::Value(size) => size,
			IndentSize::UseTabWidth => {
				// Fall back to tab_width
				if let Ok(TabWidth::Value(width)) = properties.get::<TabWidth>() {
					width
				} else {
					2 // Default
				}
			}
		}
	} else if let Ok(TabWidth::Value(width)) = properties.get::<TabWidth>() {
		width
	} else {
		2 // Default for tabs is 2
	}
}

pub(super) fn apply_indentation(
	content: &str,
	indent_style: IndentStyle,
	indent_size: usize,
	verbose: bool,
) -> Result<String> {
	let lines: Vec<&str> = content.lines().collect();
	let mut converted_lines = Vec::new();
	let mut indent_changes = 0;

	for line in lines {
		// Skip empty lines
		if line.trim().is_empty() {
			converted_lines.push(line.to_string());
			continue;
		}

		let (leading_whitespace, rest) = split_leading_whitespace(line);
		let converted_indent =
			convert_indentation_smart(&leading_whitespace, indent_style, indent_size);

		if converted_indent != leading_whitespace {
			indent_changes += 1;
		}

		converted_lines.push(format!("{}{}", converted_indent, rest));
	}

	if indent_changes > 0 && verbose {
		println!(
			"  - Converted indentation to {:?} (size: {}) on {} lines",
			indent_style, indent_size, indent_changes
		);
	}

	// Preserve the original line ending structure
	let line_ending = detect_line_ending(content);
	let result = converted_lines.join(line_ending);

	// Preserve final newline status
	let should_end_with_newline = content.ends_with('\n') || content.ends_with('\r');
	if should_end_with_newline && !result.ends_with('\n') && !result.ends_with('\r') {
		Ok(format!("{}{}", result, line_ending))
	} else {
		Ok(result)
	}
}

pub(super) fn split_leading_whitespace(line: &str) -> (String, &str) {
	let trimmed = line.trim_start();
	let leading_len = line.len() - trimmed.len();
	(line[..leading_len].to_string(), trimmed)
}

pub(super) fn convert_indentation_smart(
	whitespace: &str,
	target_style: IndentStyle,
	target_size: usize,
) -> String {
	match target_style {
		IndentStyle::Tabs => {
			// Converting TO tabs: determine logical levels and use one tab per level
			let indent_level = determine_indentation_level(whitespace, target_size);
			"\t".repeat(indent_level)
		}
		IndentStyle::Spaces => {
			// Converting TO spaces: handle more carefully
			if whitespace.chars().all(|c| c == ' ') {
				// Already all spaces - check if it follows the target pattern
				let space_count = whitespace.len();
				if space_count.is_multiple_of(target_size) {
					// Already correctly formatted for the target size
					whitespace.to_string()
				} else {
					// Reformat to target size
					let indent_level = determine_indentation_level(whitespace, target_size);
					" ".repeat(indent_level * target_size)
				}
			} else {
				// Contains tabs or mixed - convert from tabs to spaces
				let indent_level = determine_indentation_level(whitespace, target_size);
				" ".repeat(indent_level * target_size)
			}
		}
	}
}

pub(super) fn determine_indentation_level(whitespace: &str, reference_size: usize) -> usize {
	if whitespace.is_empty() {
		return 0;
	}

	let mut level = 0;
	let chars: Vec<char> = whitespace.chars().collect();
	let mut i = 0;

	while i < chars.len() {
		match chars[i] {
			'\t' => {
				// Each tab represents one indentation level
				level += 1;
				i += 1;
			}
			' ' => {
				// Count consecutive spaces
				let start_i = i;
				while i < chars.len() && chars[i] == ' ' {
					i += 1;
				}
				let space_count = i - start_i;

				if space_count > 0 {
					// Detect the likely indentation size by examining the space count
					// Common indentations are 2, 4, or 8 spaces
					let detected_indent_size =
						detect_space_indent_size(space_count, reference_size);
					level += space_count / detected_indent_size;
				}
			}
			_ => break, // Stop at non-whitespace
		}
	}

	level
}

pub(super) fn detect_space_indent_size(space_count: usize, _reference_size: usize) -> usize {
	// When converting FROM spaces TO tabs, we need to detect the actual space-based
	// indentation pattern, not use the target tab's indent_size
	// Common indentations are 4, 2, 8, or 1 spaces per logical level
	// Try the most common first (4), then others
	for size in [4, 2, 8, 1] {
		if space_count.is_multiple_of(size) {
			return size;
		}
	}

	// Fallback to 4 (most common)
	4
}

pub(super) fn trim_trailing_whitespace(content: &str) -> String {
	let line_ending = detect_line_ending(content);
	let lines: Vec<String> = content
		.lines()
		.map(|line| line.trim_end().to_string())
		.collect();

	let result = lines.join(line_ending);

	// Preserve final newline status
	if content.ends_with('\n') || content.ends_with('\r') {
		if !result.ends_with('\n') && !result.ends_with('\r') {
			format!("{}{}", result, line_ending)
		} else {
			result
		}
	} else {
		result
	}
}

pub(super) fn handle_final_newline(content: &str, insert_final_newline: bool) -> String {
	let line_ending = detect_line_ending(content);
	let ends_with_newline = content.ends_with('\n') || content.ends_with('\r');

	if insert_final_newline {
		if !content.is_empty() && !ends_with_newline {
			format!("{}{}", content, line_ending)
		} else {
			content.to_string()
		}
	} else if ends_with_newline {
		content.trim_end_matches(&['\n', '\r'][..]).to_string()
	} else {
		content.to_string()
	}
}

pub(super) fn detect_line_ending(content: &str) -> &str {
	if content.contains("\r\n") {
		"\r\n"
	} else if content.contains('\r') {
		"\r"
	} else {
		"\n"
	}
}

pub(super) fn check_line_length(
	content: &str,
	max_line_length: u32,
	file_path: &Path,
	verbose: bool,
) {
	if !verbose {
		return;
	}

	let long_lines: Vec<usize> = content
		.lines()
		.enumerate()
		.filter_map(|(i, line)| {
			if line.len() > max_line_length as usize {
				Some(i + 1)
			} else {
				None
			}
		})
		.take(5) // Limit to first 5 long lines
		.collect();

	if !long_lines.is_empty() {
		println!(
			"  - Warning: {} lines exceed max length ({}) in {}",
			long_lines.len(),
			max_line_length,
			file_path.display()
		);
		if verbose {
			for line_num in long_lines {
				println!("    Line {}", line_num);
			}
		}
	}
}
