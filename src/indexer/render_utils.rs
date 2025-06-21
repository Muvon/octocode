use super::FileSignature;
use crate::config::Config;
use crate::store::{CodeBlock, DocumentBlock, TextBlock};
use anyhow::Result;

// Extracted rendering functions:
pub fn render_to_markdown<T: std::fmt::Display>(_title: &str, content: T) -> String {
	format!("{}", content)
}

/// Smart content truncation that preserves beginning and end when content is too long
/// Returns (truncated_content, was_truncated)
pub fn truncate_content_smartly(content: &str, max_characters: usize) -> (String, bool) {
	// If max_characters is 0, return full content (disabled)
	if max_characters == 0 {
		return (content.to_string(), false);
	}

	// If content fits within limit, return as-is
	if content.len() <= max_characters {
		return (content.to_string(), false);
	}

	let lines: Vec<&str> = content.lines().collect();

	// If it's just one long line, truncate it differently
	if lines.len() == 1 {
		let chars: Vec<char> = content.chars().collect();
		if chars.len() <= max_characters {
			return (content.to_string(), false);
		}

		// For single long line, show first and last parts
		let show_start = max_characters / 3;
		let show_end = max_characters / 3;
		let start_part: String = chars.iter().take(show_start).collect();
		let end_part: String = chars.iter().skip(chars.len() - show_end).collect();

		let truncated = format!(
			"{}\n[... {} characters omitted ...]\n{}",
			start_part.trim_end(),
			chars.len() - show_start - show_end,
			end_part.trim_start()
		);
		return (truncated, true);
	}

	// For multi-line content, work with lines
	let mut current_length = 0;
	let mut start_lines = Vec::new();
	let mut end_lines = Vec::new();

	// Reserve space for the middle message
	let middle_message_size = 50; // Approximate size of "[... X lines omitted ...]"
	let target_size = max_characters.saturating_sub(middle_message_size);
	let start_target = target_size / 2;
	let end_target = target_size / 2;

	// Collect start lines
	for line in &lines {
		let line_len = line.len() + 1; // +1 for newline
		if current_length + line_len <= start_target {
			start_lines.push(*line);
			current_length += line_len;
		} else {
			break;
		}
	}

	// Collect end lines (working backwards)
	current_length = 0;
	for line in lines.iter().rev() {
		let line_len = line.len() + 1; // +1 for newline
		if current_length + line_len <= end_target {
			end_lines.insert(0, *line);
			current_length += line_len;
		} else {
			break;
		}
	}

	// Ensure we don't overlap
	let start_count = start_lines.len();
	let end_count = end_lines.len();
	let total_lines = lines.len();

	if start_count + end_count >= total_lines {
		// If we would show most lines anyway, just show all
		return (content.to_string(), false);
	}

	let omitted_lines = total_lines - start_count - end_count;

	// Build the truncated content
	let mut result = String::new();

	// Add start lines
	for line in &start_lines {
		result.push_str(line);
		result.push('\n');
	}

	// Add truncation message
	if omitted_lines > 0 {
		result.push_str(&format!("[... {} more lines ...]\n", omitted_lines));
	}

	// Add end lines
	for line in &end_lines {
		result.push_str(line);
		result.push('\n');
	}

	// Remove trailing newline
	if result.ends_with('\n') {
		result.pop();
	}

	(result, true)
}

/// Render signatures as markdown string
pub fn signatures_to_markdown(signatures: &[FileSignature]) -> String {
	let mut markdown = String::new();

	if signatures.is_empty() {
		markdown.push_str("No signatures found.");
		return markdown;
	}

	markdown.push_str(&format!(
		"# Found signatures in {} files\n\n",
		signatures.len()
	));

	for file in signatures {
		markdown.push_str(&format!("## File: {}\n", file.path));
		markdown.push_str(&format!("**Language:** {}\n\n", file.language));

		// Show file comment if available
		if let Some(comment) = &file.file_comment {
			markdown.push_str("### File description\n");
			markdown.push_str(&format!("> {}\n\n", comment.replace("\n", "\n> ")));
		}

		if file.signatures.is_empty() {
			markdown.push_str("*No signatures found in this file.*\n\n");
		} else {
			for signature in &file.signatures {
				// Display line range if it spans multiple lines, otherwise just the start line
				let line_display = if signature.start_line == signature.end_line {
					format!("{}", signature.start_line + 1)
				} else {
					format!("{}-{}", signature.start_line + 1, signature.end_line + 1)
				};

				markdown.push_str(&format!(
					"### {} `{}` (line {})\n",
					signature.kind, signature.name, line_display
				));

				// Show description if available
				if let Some(desc) = &signature.description {
					markdown.push_str(&format!("> {}\n\n", desc.replace("\n", "\n> ")));
				}

				// Format the signature for display
				markdown.push_str("```");

				// Add language identifier for syntax highlighting when possible
				if !file.language.is_empty() && file.language != "text" {
					markdown.push_str(&file.language);
				}
				markdown.push('\n');

				let lines = signature.signature.lines().collect::<Vec<_>>();
				if lines.len() > 5 {
					// Show first 2 lines and last 2 lines to maintain context
					for line in lines.iter().take(2) {
						markdown.push_str(line.as_ref());
						markdown.push('\n');
					}
					// If signature is too long, note how many lines are omitted
					markdown.push_str(&format!("// ... {} more lines\n", lines.len() - 4));
					for line in lines.iter().skip(lines.len() - 2) {
						markdown.push_str(line.as_ref());
						markdown.push('\n');
					}
				} else {
					for line in &lines {
						markdown.push_str(line.as_ref());
						markdown.push('\n');
					}
				}
				markdown.push_str("```\n\n");
			}
		}

		// Add spacing between files
		markdown.push_str("---\n\n");
	}

	markdown
}

/// Render signatures as text string (token-efficient)
pub fn render_signatures_text(signatures: &[FileSignature]) -> String {
	let mut output = String::new();

	if signatures.is_empty() {
		output.push_str("No signatures found.");
		return output;
	}

	output.push_str(&format!("SIGNATURES ({} files)\n\n", signatures.len()));

	for file in signatures {
		output.push_str(&format!("FILE: {}\n", file.path));
		output.push_str(&format!("Language: {}\n", file.language));

		// Show file comment if available
		if let Some(comment) = &file.file_comment {
			output.push_str(&format!("Description: {}\n", comment.replace('\n', " ")));
		}

		if file.signatures.is_empty() {
			output.push_str("No signatures found.\n");
		} else {
			for signature in &file.signatures {
				// Display line range
				// let line_display = if signature.start_line == signature.end_line {
				// 	format!("{}", signature.start_line + 1)
				// } else {
				// 	format!("{}-{}", signature.start_line + 1, signature.end_line + 1)
				// };

				// output.push_str(&format!(
				// 	"{} {} (line {})\n",
				// 	signature.kind, signature.name, line_display
				// ));

				// Show description if available
				if let Some(desc) = &signature.description {
					output.push_str(&format!("// {}\n", desc.replace('\n', " ")));
				}

				// Add signature content with line numbers (smart truncation showing first and last lines)
				let lines = signature.signature.lines().collect::<Vec<_>>();
				let content_with_lines = if lines.len() > 5 {
					// Show first 2 lines, then "...", then last 2 lines to maintain context
					let first_lines: Vec<String> = lines
						.iter()
						.take(2)
						.enumerate()
						.map(|(i, line)| format!("{}: {}", signature.start_line + 1 + i, line))
						.collect();

					let last_lines: Vec<String> = lines
						.iter()
						.skip(lines.len() - 2)
						.enumerate()
						.map(|(i, line)| {
							let line_num = signature.start_line + 1 + (lines.len() - 2) + i;
							format!("{}: {}", line_num, line)
						})
						.collect();

					let mut result = first_lines.join("\n");
					result.push_str(&format!("\n// ... {} more lines", lines.len() - 4));
					result.push('\n');
					result.push_str(&last_lines.join("\n"));
					result
				} else {
					lines
						.iter()
						.enumerate()
						.map(|(i, line)| format!("{}: {}", signature.start_line + 1 + i, line))
						.collect::<Vec<_>>()
						.join("\n")
				};
				output.push_str(&content_with_lines);
				if !content_with_lines.ends_with('\n') {
					output.push('\n');
				}
				output.push('\n');
			}
		}
		output.push('\n');
	}

	output
}

/// Render code blocks (search results) as markdown string
pub fn code_blocks_to_markdown(blocks: &[CodeBlock]) -> String {
	code_blocks_to_markdown_with_config(blocks, &Config::default())
}

/// Render code blocks (search results) as markdown string with configuration
pub fn code_blocks_to_markdown_with_config(blocks: &[CodeBlock], config: &Config) -> String {
	let mut markdown = String::new();

	if blocks.is_empty() {
		markdown.push_str("No code blocks found for the query.");
		return markdown;
	}

	markdown.push_str(&format!("# Found {} code blocks\n\n", blocks.len()));

	// Group blocks by file path for better organization
	let mut blocks_by_file: std::collections::HashMap<String, Vec<&CodeBlock>> =
		std::collections::HashMap::new();

	for block in blocks {
		blocks_by_file
			.entry(block.path.clone())
			.or_default()
			.push(block);
	}

	// Print results organized by file
	for (file_path, file_blocks) in blocks_by_file.iter() {
		markdown.push_str(&format!("## File: {}\n\n", file_path));

		for (idx, block) in file_blocks.iter().enumerate() {
			markdown.push_str(&format!("### Block {} of {}\n", idx + 1, file_blocks.len()));
			markdown.push_str(&format!("**Language:** {}  ", block.language));
			markdown.push_str(&format!(
				"**Lines:** {}-{}  ",
				block.start_line, block.end_line
			));

			// Show similarity score if available
			if let Some(distance) = block.distance {
				markdown.push_str(&format!("**Similarity:** {:.4}  ", 1.0 - distance));
			}
			markdown.push('\n');

			if !block.symbols.is_empty() {
				markdown.push_str("**Symbols:**  \n");
				// Deduplicate symbols in display
				let mut display_symbols = block.symbols.clone();
				display_symbols.sort();
				display_symbols.dedup();

				for symbol in display_symbols {
					// Only show non-type symbols to users
					if !symbol.contains("_") {
						markdown.push_str(&format!("- `{}`  \n", symbol));
					}
				}
			}

			markdown.push_str("```");
			// Add language for syntax highlighting
			if !block.language.is_empty() && block.language != "text" {
				markdown.push_str(&block.language);
			}
			markdown.push('\n');

			// Use smart truncation based on configuration
			let max_chars = config.search.search_block_max_characters;
			let (content, was_truncated) = truncate_content_smartly(&block.content, max_chars);

			markdown.push_str(&content);
			if !content.ends_with('\n') {
				markdown.push('\n');
			}

			// Add note if content was truncated
			if was_truncated {
				markdown.push_str(&format!(
					"// Content truncated (limit: {} chars)\n",
					max_chars
				));
			}

			markdown.push_str("```\n\n");
		}

		markdown.push_str("---\n\n");
	}

	markdown
}

/// Render text blocks (text search results) as markdown string
pub fn text_blocks_to_markdown(blocks: &[TextBlock]) -> String {
	text_blocks_to_markdown_with_config(blocks, &Config::default())
}

/// Render text blocks (text search results) as markdown string with configuration
pub fn text_blocks_to_markdown_with_config(blocks: &[TextBlock], config: &Config) -> String {
	let mut markdown = String::new();

	if blocks.is_empty() {
		markdown.push_str("No text blocks found for the query.");
		return markdown;
	}

	markdown.push_str(&format!("# Found {} text blocks\n\n", blocks.len()));

	// Group blocks by file path for better organization
	let mut blocks_by_file: std::collections::HashMap<String, Vec<&TextBlock>> =
		std::collections::HashMap::new();

	for block in blocks {
		blocks_by_file
			.entry(block.path.clone())
			.or_default()
			.push(block);
	}

	// Print results organized by file
	for (file_path, file_blocks) in blocks_by_file.iter() {
		markdown.push_str(&format!("## File: {}\n\n", file_path));

		for (idx, block) in file_blocks.iter().enumerate() {
			markdown.push_str(&format!("### Block {} of {}\n", idx + 1, file_blocks.len()));
			markdown.push_str(&format!("**Language:** {}  ", block.language));
			markdown.push_str(&format!(
				"**Lines:** {}-{}  ",
				block.start_line, block.end_line
			));

			// Show relevance score if available
			if let Some(distance) = block.distance {
				markdown.push_str(&format!("**Relevance:** {:.4}  ", 1.0 - distance));
			}
			markdown.push_str("\n\n");

			// Use smart truncation based on configuration
			let max_chars = config.search.search_block_max_characters;
			let (content, was_truncated) = truncate_content_smartly(&block.content, max_chars);

			markdown.push_str(&content);
			if !content.ends_with('\n') {
				markdown.push('\n');
			}

			// Add note if content was truncated
			if was_truncated {
				markdown.push_str(&format!(
					"\n*Content truncated (limit: {} chars)*\n",
					max_chars
				));
			}

			markdown.push('\n');
		}

		markdown.push_str("---\n\n");
	}

	markdown
}

/// Render document blocks (documentation search results) as markdown string
pub fn document_blocks_to_markdown(blocks: &[DocumentBlock]) -> String {
	document_blocks_to_markdown_with_config(blocks, &Config::default())
}

/// Render document blocks (documentation search results) as markdown string with configuration
pub fn document_blocks_to_markdown_with_config(
	blocks: &[DocumentBlock],
	config: &Config,
) -> String {
	let mut markdown = String::new();

	if blocks.is_empty() {
		markdown.push_str("No documentation found for the query.");
		return markdown;
	}

	markdown.push_str(&format!(
		"# Found {} documentation sections\n\n",
		blocks.len()
	));

	// Group blocks by file path for better organization
	let mut blocks_by_file: std::collections::HashMap<String, Vec<&DocumentBlock>> =
		std::collections::HashMap::new();

	for block in blocks {
		blocks_by_file
			.entry(block.path.clone())
			.or_default()
			.push(block);
	}

	// Print results organized by file
	for (file_path, file_blocks) in blocks_by_file.iter() {
		markdown.push_str(&format!("## File: {}\n\n", file_path));

		for (idx, block) in file_blocks.iter().enumerate() {
			markdown.push_str(&format!(
				"### {} (Section {} of {})\n",
				block.title,
				idx + 1,
				file_blocks.len()
			));
			markdown.push_str(&format!("**Level:** {}  ", block.level));
			markdown.push_str(&format!(
				"**Lines:** {}-{}  ",
				block.start_line, block.end_line
			));

			// Show relevance score if available
			if let Some(distance) = block.distance {
				markdown.push_str(&format!("**Relevance:** {:.4}  ", 1.0 - distance));
			}
			markdown.push_str("\n\n");

			// Use smart truncation based on configuration
			let max_chars = config.search.search_block_max_characters;
			let (content, was_truncated) = truncate_content_smartly(&block.content, max_chars);

			markdown.push_str(&content);
			if !content.ends_with('\n') {
				markdown.push('\n');
			}

			// Add note if content was truncated
			if was_truncated {
				markdown.push_str(&format!(
					"\n*Content truncated (limit: {} chars)*\n",
					max_chars
				));
			}

			markdown.push('\n');
		}

		markdown.push_str("---\n\n");
	}

	markdown
}

/// Render signatures as CLI output with box drawing
pub fn render_signatures_cli(signatures: &[FileSignature]) {
	if signatures.is_empty() {
		println!("No signatures found.");
		return;
	}

	println!("Found signatures in {} files:\n", signatures.len());

	for file in signatures {
		println!("╔══════════════════ File: {} ══════════════════", file.path);
		println!("║ Language: {}", file.language);

		// Show file comment if available
		if let Some(comment) = &file.file_comment {
			println!("║");
			println!("║ File description:");
			for line in comment.lines() {
				println!("║   {}", line);
			}
		}

		if file.signatures.is_empty() {
			println!("║");
			println!("║ No signatures found in this file.");
		} else {
			for signature in &file.signatures {
				println!("║");

				// Display line range if it spans multiple lines, otherwise just the start line
				let line_display = if signature.start_line == signature.end_line {
					format!("{}", signature.start_line + 1)
				} else {
					format!("{}-{}", signature.start_line + 1, signature.end_line + 1)
				};

				println!(
					"║ {} `{}` (line {})",
					signature.kind, signature.name, line_display
				);

				// Show description if available
				if let Some(desc) = &signature.description {
					println!("║ Description:");
					for line in desc.lines() {
						println!("║   {}", line);
					}
				}

				// Format the signature for display
				println!("║ Signature:");
				let lines = signature.signature.lines().collect::<Vec<_>>();
				if lines.len() > 1 {
					println!("║ ┌────────────────────────────────────");
					if lines.len() > 5 {
						// Show first 2 lines with line numbers
						for (i, line) in lines.iter().take(2).enumerate() {
							let line_num = signature.start_line + 1 + i;
							println!("║ │ {}: {}", line_num, line);
						}
						// Show truncation message
						println!("║ │ // ... {} more lines", lines.len() - 4);
						// Show last 2 lines with line numbers
						for (i, line) in lines.iter().skip(lines.len() - 2).enumerate() {
							let line_num = signature.start_line + 1 + (lines.len() - 2) + i;
							println!("║ │ {}: {}", line_num, line);
						}
					} else {
						// Show all lines with line numbers for short signatures
						for (i, line) in lines.iter().enumerate() {
							let line_num = signature.start_line + 1 + i;
							println!("║ │ {}: {}", line_num, line);
						}
					}
					println!("║ └────────────────────────────────────");
				} else if !lines.is_empty() {
					let line_num = signature.start_line + 1;
					println!("║   {}: {}", line_num, lines[0]);
				}
			}
		}

		println!("╚════════════════════════════════════════\n");
	}
}

/// Render signatures as JSON
pub fn render_signatures_json(signatures: &[FileSignature]) -> Result<()> {
	let json = serde_json::to_string_pretty(signatures)?;
	println!("{}", json);
	Ok(())
}
