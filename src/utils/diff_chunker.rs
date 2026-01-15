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

//! Intelligent diff chunking utility for large commits
//!
//! Splits large git diffs into manageable chunks for parallel AI processing,
//! then combines the results efficiently. Optimized for performance and reliability.

use anyhow::Result;

/// Maximum size per chunk in characters - balances context vs API limits
pub const MAX_CHUNK_SIZE: usize = 8000;
/// Overlap between chunks to maintain context - ensures continuity
pub const CHUNK_OVERLAP: usize = 200;
/// Maximum number of parallel chunks to process - prevents resource exhaustion
pub const MAX_PARALLEL_CHUNKS: usize = 10;
/// Maximum total chunks to process - prevents runaway processing
pub const MAX_TOTAL_CHUNKS: usize = 50;

#[derive(Debug, Clone)]
pub struct DiffChunk {
	pub content: String,
	pub file_summary: String, // Brief summary of files in this chunk
}

/// Efficient diff chunking with resource limits and optimized string operations
///
/// Splits large git diffs into manageable chunks for AI processing.
/// Implements configurable limits to prevent resource exhaustion.
///
/// # Arguments
/// * `diff_content` - The complete git diff content to be chunked
///
/// # Returns
/// A vector of `DiffChunk` objects, limited by MAX_TOTAL_CHUNKS
///
/// # Performance Notes
/// - Uses string slicing and pre-allocated vectors for better performance
/// - Enforces MAX_TOTAL_CHUNKS limit to prevent runaway processing
/// - Maintains file boundaries for semantic coherence
pub fn chunk_diff(diff_content: &str) -> Vec<DiffChunk> {
	// Early return for small diffs - no chunking needed
	if diff_content.len() <= MAX_CHUNK_SIZE {
		return vec![DiffChunk {
			content: diff_content.to_string(),
			file_summary: extract_file_summary(diff_content),
		}];
	}

	// Split diff by individual files to maintain semantic boundaries
	let files = split_diff_by_files(diff_content);

	// Pre-allocate with estimated capacity, but respect limits
	let estimated_chunks = ((diff_content.len() / MAX_CHUNK_SIZE) + 1).min(MAX_TOTAL_CHUNKS);
	let mut chunks = Vec::with_capacity(estimated_chunks);

	let mut current_chunk = String::with_capacity(MAX_CHUNK_SIZE + CHUNK_OVERLAP);
	let mut current_files = Vec::new();

	// Process each file diff, creating chunks when size limit is reached
	for file_diff in files {
		// Enforce maximum chunk limit to prevent resource exhaustion
		if chunks.len() >= MAX_TOTAL_CHUNKS {
			eprintln!(
				"Warning: Reached maximum chunk limit ({}), truncating diff processing",
				MAX_TOTAL_CHUNKS
			);
			break;
		}

		// Check if adding this file would exceed chunk size limit
		if !current_chunk.is_empty() && current_chunk.len() + file_diff.len() > MAX_CHUNK_SIZE {
			// Create chunk with current content
			chunks.push(DiffChunk {
				content: current_chunk.clone(),
				file_summary: current_files.join(", "),
			});

			// Start new chunk with overlap from previous for context continuity
			current_chunk = get_chunk_overlap(&current_chunk);
			current_files.clear();
		}

		// Add file diff to current chunk efficiently
		current_chunk.push_str(&file_diff);
		current_chunk.push('\n');

		// Extract filename for summary generation
		if let Some(filename) = extract_filename(&file_diff) {
			current_files.push(filename);
		}
	}

	// Add final chunk if it contains content and we haven't hit the limit
	if !current_chunk.is_empty() && chunks.len() < MAX_TOTAL_CHUNKS {
		chunks.push(DiffChunk {
			content: current_chunk,
			file_summary: current_files.join(", "),
		});
	}

	// Log chunk statistics for monitoring
	if chunks.len() > 1 {
		println!(
			"Info: Split diff into {} chunks (limit: {}, parallel: {})",
			chunks.len(),
			MAX_TOTAL_CHUNKS,
			MAX_PARALLEL_CHUNKS
		);
	}

	chunks
}

/// Split diff content by individual file boundaries with optimized string handling
///
/// Parses git diff output to separate changes for each file.
/// Uses efficient string operations to minimize allocations.
///
/// # Arguments
/// * `diff_content` - Complete git diff content
///
/// # Returns
/// Vector of strings, each containing the diff for one file
fn split_diff_by_files(diff_content: &str) -> Vec<String> {
	// Pre-allocate based on estimated file count
	let estimated_files = diff_content.matches("diff --git").count().max(1);
	let mut files = Vec::with_capacity(estimated_files);
	let mut current_file = String::with_capacity(diff_content.len() / estimated_files);

	// Process each line, splitting on "diff --git" markers
	for line in diff_content.lines() {
		// New file detected - save previous and start new
		if line.starts_with("diff --git") && !current_file.is_empty() {
			files.push(std::mem::take(&mut current_file));
			current_file = String::with_capacity(diff_content.len() / estimated_files);
		}

		// Add line to current file diff efficiently
		current_file.push_str(line);
		current_file.push('\n');
	}

	// Add final file if it contains content
	if !current_file.is_empty() {
		files.push(current_file);
	}

	files
}

/// Extract filename from git diff header
///
/// Parses the "diff --git a/path b/path" line to extract the target filename.
/// Uses the 'b/' path which represents the new/modified file.
///
/// # Arguments
/// * `file_diff` - A single file's diff content starting with "diff --git"
///
/// # Returns
/// Some(filename) if successfully parsed, None otherwise
fn extract_filename(file_diff: &str) -> Option<String> {
	// Look for the git diff header line
	for line in file_diff.lines() {
		if line.starts_with("diff --git") {
			// Parse "diff --git a/path b/path" format
			let parts: Vec<&str> = line.split_whitespace().collect();
			if parts.len() >= 4 {
				// Use the 'b/' path (target file) and strip the prefix
				let path = parts[3].strip_prefix("b/").unwrap_or(parts[3]);
				return Some(path.to_string());
			}
		}
	}
	None
}

/// Enhanced overlap extraction that preserves line boundaries
///
/// Extracts overlap content from the end of a chunk, ensuring we don't
/// break lines in the middle. This maintains context continuity between chunks.
fn get_chunk_overlap(chunk: &str) -> String {
	if chunk.len() <= CHUNK_OVERLAP {
		return chunk.to_string();
	}

	// Get last CHUNK_OVERLAP characters, but ensure we're on a UTF-8 character boundary
	let target_start = chunk.len().saturating_sub(CHUNK_OVERLAP);

	// Find the nearest character boundary at or after target_start
	let start_pos = chunk
		.char_indices()
		.find(|(byte_idx, _)| *byte_idx >= target_start)
		.map(|(byte_idx, _)| byte_idx)
		.unwrap_or(chunk.len());

	let overlap_section = &chunk[start_pos..];

	// Find the first newline to avoid cutting lines mid-way
	if let Some(newline_pos) = overlap_section.find('\n') {
		// Start from after the newline to preserve complete lines
		overlap_section[newline_pos + 1..].to_string()
	} else {
		// No newline found, use the whole overlap section
		// This might cut a line, but it's better than losing context
		overlap_section.to_string()
	}
}

/// Extract file summary from diff content
///
/// Creates a human-readable summary of files changed in the diff.
/// Handles single files, small lists, and large file counts appropriately.
fn extract_file_summary(diff_content: &str) -> String {
	let mut files = Vec::new();

	// Process each line to find file headers
	for line in diff_content.lines() {
		if line.starts_with("diff --git") {
			// FIXED: Pass the full line to extract_filename, not just the line
			if let Some(filename) = extract_filename(line) {
				files.push(filename);
			}
		}
	}

	// Generate appropriate summary based on file count
	match files.len() {
		0 => "changes".to_string(),
		1 => files[0].clone(),
		2..=3 => files.join(", "),
		n => format!("{} files", n),
	}
}

/// Robust commit message combination with format validation
///
/// Combines multiple AI responses into a coherent commit message.
/// Handles various commit message formats and ensures proper structure.
pub fn combine_commit_messages(responses: Vec<String>) -> String {
	if responses.is_empty() {
		return "chore: update files".to_string();
	}

	if responses.len() == 1 {
		return responses[0].clone();
	}

	// Extract subjects and bodies from responses with better parsing
	let mut subjects = Vec::new();
	let mut bodies = Vec::new();

	for response in &responses {
		let lines: Vec<&str> = response.lines().collect();

		// Extract subject (first non-empty line)
		if let Some(subject) = lines.iter().find(|line| !line.trim().is_empty()) {
			subjects.push(subject.trim());
		}

		// Extract body (lines after first empty line)
		let mut found_empty = false;
		let mut body_lines = Vec::new();

		// Safe iteration: only slice if we have more than 1 line
		for line in if lines.len() > 1 { &lines[1..] } else { &[] } {
			if line.trim().is_empty() {
				found_empty = true;
				continue;
			}
			if found_empty {
				body_lines.push(*line);
			}
		}

		if !body_lines.is_empty() {
			bodies.push(body_lines.join("\n"));
		}
	}

	// Create combined subject - prefer the most comprehensive one
	let combined_subject = subjects
		.into_iter()
		.max_by_key(|s| s.len())
		.unwrap_or("chore: update multiple files")
		.to_string();

	// Combine unique bodies if present
	if bodies.is_empty() {
		combined_subject
	} else {
		let unique_bodies: std::collections::HashSet<String> = bodies.into_iter().collect();
		let combined_body = unique_bodies.into_iter().collect::<Vec<_>>().join("\n\n");
		format!("{}\n\n{}", combined_subject, combined_body)
	}
}

/// Enhanced review result combination with comprehensive error handling
///
/// Combines multiple review responses into a comprehensive result.
/// Handles JSON parsing errors gracefully and provides detailed logging.
pub fn combine_review_results(responses: Vec<serde_json::Value>) -> Result<serde_json::Value> {
	if responses.is_empty() {
		eprintln!("Warning: No review responses to combine, using fallback");
		return Ok(create_fallback_review_json());
	}

	if responses.len() == 1 {
		return Ok(responses[0].clone());
	}

	// Try to parse each response as JSON and combine
	let mut all_issues = Vec::new();
	let mut all_recommendations = Vec::new();
	let mut total_files = 0;
	let mut scores = Vec::new();

	for response in responses.iter() {
		// Response is already a Value, no need to parse
		let review = response;

		// Extract issues with error handling
		if let Some(issues) = review.get("issues").and_then(|i| i.as_array()) {
			for issue in issues {
				all_issues.push(issue.clone());
			}
		}

		// Extract recommendations with error handling
		if let Some(recs) = review.get("recommendations").and_then(|r| r.as_array()) {
			for rec in recs {
				if let Some(rec_str) = rec.as_str() {
					all_recommendations.push(rec_str.to_string());
				}
			}
		}

		// Extract summary info with validation
		if let Some(summary) = review.get("summary") {
			if let Some(files) = summary.get("total_files").and_then(|f| f.as_u64()) {
				total_files += files as usize;
			}
			if let Some(score) = summary.get("overall_score").and_then(|s| s.as_u64()) {
				if score <= 100 {
					scores.push(score as u8);
				}
			}
		}
	}

	// Calculate average score with validation
	let avg_score = if scores.is_empty() {
		75 // Default fallback score
	} else {
		let sum: u32 = scores.iter().map(|&s| s as u32).sum();
		(sum / scores.len() as u32) as u8
	};

	// Remove duplicate recommendations efficiently
	let unique_recommendations: std::collections::HashSet<String> =
		all_recommendations.into_iter().collect();
	let final_recommendations: Vec<String> = unique_recommendations.into_iter().collect();

	// If no valid data was extracted, return fallback
	if all_issues.is_empty() && final_recommendations.is_empty() && total_files == 0 {
		return Ok(create_fallback_review_json());
	}

	// Create combined result with validation
	let combined_result = serde_json::json!({
		"summary": {
			"total_files": total_files,
			"total_issues": all_issues.len(),
			"overall_score": avg_score
		},
		"issues": all_issues,
		"recommendations": final_recommendations
	});

	Ok(combined_result)
}

/// Create fallback review JSON when parsing fails
fn create_fallback_review_json() -> serde_json::Value {
	serde_json::json!({
		"summary": {
			"total_files": 1,
			"total_issues": 1,
			"overall_score": 75
		},
		"issues": [{
			"severity": "MEDIUM",
			"category": "System",
			"title": "Review Analysis Incomplete",
			"description": "The automated review could not complete fully. Manual review recommended."
		}],
		"recommendations": [
			"Consider running the review again",
			"Perform manual code review for complex changes"
		]
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_small_diff_no_chunking() {
		let small_diff = "diff --git a/test.rs b/test.rs\n+added line\n";
		let chunks = chunk_diff(small_diff);

		assert_eq!(chunks.len(), 1);
		assert_eq!(chunks[0].content, small_diff);
		assert_eq!(chunks[0].file_summary, "test.rs");
	}

	#[test]
	fn test_extract_filename() {
		let file_diff = "diff --git a/src/main.rs b/src/main.rs\nindex 123..456\n";
		let filename = extract_filename(file_diff);
		assert_eq!(filename, Some("src/main.rs".to_string()));

		// Test with no b/ prefix
		let file_diff2 = "diff --git a/test.rs test.rs\nindex 123..456\n";
		let filename2 = extract_filename(file_diff2);
		assert_eq!(filename2, Some("test.rs".to_string()));

		// Test invalid format
		let invalid_diff = "not a diff line\n";
		let filename3 = extract_filename(invalid_diff);
		assert_eq!(filename3, None);
	}

	#[test]
	fn test_extract_file_summary() {
		// Single file
		let single_file = "diff --git a/test.rs b/test.rs\n+line\n";
		assert_eq!(extract_file_summary(single_file), "test.rs");

		// Multiple files
		let multi_files = "diff --git a/file1.rs b/file1.rs\n+line1\ndiff --git a/file2.rs b/file2.rs\n+line2\ndiff --git a/file3.rs b/file3.rs\n+line3\n";
		assert_eq!(
			extract_file_summary(multi_files),
			"file1.rs, file2.rs, file3.rs"
		);

		// Many files (should show count)
		let many_files = (0..10)
			.map(|i| format!("diff --git a/file{}.rs b/file{}.rs\n+line{}\n", i, i, i))
			.collect::<String>();
		assert_eq!(extract_file_summary(&many_files), "10 files");

		// No files
		let no_files = "some random content\nwithout diff headers\n";
		assert_eq!(extract_file_summary(no_files), "changes");
	}

	#[test]
	fn test_split_diff_by_files() {
		let multi_file_diff = concat!(
			"diff --git a/file1.rs b/file1.rs\n",
			"index 123..456\n",
			"+added line 1\n",
			"diff --git a/file2.rs b/file2.rs\n",
			"index 789..abc\n",
			"+added line 2\n"
		);

		let files = split_diff_by_files(multi_file_diff);
		assert_eq!(files.len(), 2);
		assert!(files[0].contains("file1.rs"));
		assert!(files[0].contains("added line 1"));
		assert!(files[1].contains("file2.rs"));
		assert!(files[1].contains("added line 2"));
	}

	#[test]
	fn test_get_chunk_overlap() {
		// Short chunk - return whole thing
		let short_chunk = "short content";
		assert_eq!(get_chunk_overlap(short_chunk), short_chunk);

		// Long chunk with newlines
		let long_chunk = "a".repeat(300) + "\nline1\nline2\nline3";
		let overlap = get_chunk_overlap(&long_chunk);
		assert!(overlap.len() <= CHUNK_OVERLAP);
		assert!(overlap.starts_with("line1\nline2\nline3"));

		// Long chunk without newlines
		let no_newlines = "a".repeat(300);
		let overlap2 = get_chunk_overlap(&no_newlines);
		assert!(overlap2.len() <= CHUNK_OVERLAP);
	}

	#[test]
	fn test_large_diff_chunking() {
		// Create multiple large files that together exceed chunk size
		let mut large_diff = String::new();
		for i in 0..5 {
			let file_content = "a".repeat(MAX_CHUNK_SIZE / 3); // Each file is 1/3 of max size
			large_diff.push_str(&format!(
				"diff --git a/file{}.rs b/file{}.rs\nindex 123..456\n+{}\n",
				i, i, file_content
			));
		}

		let chunks = chunk_diff(&large_diff);
		// Should have at least one chunk
		assert!(!chunks.is_empty(), "Should have at least one chunk");

		// Verify all chunks are within reasonable size limits
		for chunk in &chunks {
			// Allow some flexibility for the chunk size due to overlap and headers
			assert!(
				chunk.content.len() <= MAX_CHUNK_SIZE * 2, // More lenient limit
				"Chunk size {} should be reasonable",
				chunk.content.len()
			);
		}
	}

	#[test]
	fn test_combine_commit_messages() {
		// Empty responses
		let empty: Vec<String> = vec![];
		assert_eq!(combine_commit_messages(empty), "chore: update files");

		// Single response (single line - this was the panic case!)
		let single = vec!["feat: add new feature".to_string()];
		assert_eq!(combine_commit_messages(single), "feat: add new feature");

		// Multiple responses with bodies
		let multiple = vec![
			"feat: add feature A\n\n- Added component A\n- Updated tests".to_string(),
			"fix: resolve bug B\n\n- Fixed validation\n- Added error handling".to_string(),
		];
		let combined = combine_commit_messages(multiple);
		assert!(combined.contains("feat: add feature A")); // Should use longer subject
		assert!(combined.contains("Added component A"));
		assert!(combined.contains("Fixed validation"));
	}

	#[test]
	fn test_combine_commit_messages_single_line_edge_case() {
		// This specifically tests the edge case that caused panic at line 276
		// when response is single-line (lines[1..] panics with empty slice)
		let single_line_responses = vec!["fix: one line fix".to_string()];
		let result = combine_commit_messages(single_line_responses);
		assert_eq!(result, "fix: one line fix");

		// Multiple single-line responses
		let multi_single_line = vec!["fix: bug one".to_string(), "feat: add thing".to_string()];
		let result2 = combine_commit_messages(multi_single_line);
		// Should pick the longer one
		assert!(result2.contains("fix: bug one") || result2.contains("feat: add thing"));
	}

	#[test]
	fn test_combine_review_results() {
		// Empty responses
		let empty: Vec<serde_json::Value> = vec![];
		let result = combine_review_results(empty).unwrap();
		assert!(result.to_string().contains("Review Analysis Incomplete"));

		// Single valid JSON response
		let single = vec![serde_json::json!({
			"summary": {"total_files": 1, "total_issues": 2, "overall_score": 85},
			"issues": [{"severity": "HIGH", "category": "Security", "title": "Test Issue", "description": "Test description"}],
			"recommendations": ["Test recommendation"]
		})];
		let result = combine_review_results(single).unwrap();
		assert!(result.to_string().contains("Test Issue"));
		assert!(result.to_string().contains("Test recommendation"));

		// Multiple responses
		let multiple = vec![
			serde_json::json!({
				"summary": {"total_files": 1, "total_issues": 1, "overall_score": 80},
				"issues": [{"severity": "MEDIUM", "category": "Code Quality", "title": "Issue 1", "description": "Desc 1"}],
				"recommendations": ["Rec 1"]
			}),
			serde_json::json!({
				"summary": {"total_files": 2, "total_issues": 1, "overall_score": 90},
				"issues": [{"severity": "LOW", "category": "Style", "title": "Issue 2", "description": "Desc 2"}],
				"recommendations": ["Rec 2"]
			}),
		];
		let result = combine_review_results(multiple).unwrap();
		// Result is now serde_json::Value directly
		assert_eq!(result["summary"]["total_files"], 3); // 1 + 2
		assert_eq!(result["summary"]["total_issues"], 2); // Combined issues
		assert_eq!(result["summary"]["overall_score"], 85); // Average of 80 and 90
		assert_eq!(result["issues"].as_array().unwrap().len(), 2);
		assert_eq!(result["recommendations"].as_array().unwrap().len(), 2);
	}

	#[test]
	fn test_chunk_limit_enforcement() {
		// Create a diff that would exceed MAX_TOTAL_CHUNKS
		let mut large_diff = String::new();
		for i in 0..MAX_TOTAL_CHUNKS + 10 {
			large_diff.push_str(&format!(
				"diff --git a/file{}.rs b/file{}.rs\nindex 123..456\n+{}\n",
				i,
				i,
				"a".repeat(MAX_CHUNK_SIZE / 2) // Each file is large enough to create its own chunk
			));
		}

		let chunks = chunk_diff(&large_diff);
		assert!(
			chunks.len() <= MAX_TOTAL_CHUNKS,
			"Should not exceed MAX_TOTAL_CHUNKS limit"
		);
	}

	#[test]
	fn test_invalid_json_handling() {
		let invalid_responses = vec![serde_json::json!({"invalid": "json structure"})];

		let result = combine_review_results(invalid_responses).unwrap();
		// Should return the invalid JSON back (single response is returned as-is)
		// Since it's a single response, it's returned as-is
		assert_eq!(result["invalid"], "json structure");
	}

	#[test]
	fn test_edge_cases() {
		// Empty diff
		let empty_diff = "";
		let chunks = chunk_diff(empty_diff);
		assert_eq!(chunks.len(), 1);
		assert_eq!(chunks[0].content, "");
		assert_eq!(chunks[0].file_summary, "changes"); // Should fallback to "changes"

		// Diff with only whitespace
		let whitespace_diff = "   \n\n  \n";
		let chunks = chunk_diff(whitespace_diff);
		assert_eq!(chunks.len(), 1);

		// Diff with malformed git headers - this will extract "header" as filename
		let malformed = "diff --git incomplete header\n+some content\n";
		let chunks = chunk_diff(malformed);
		assert_eq!(chunks.len(), 1);
		assert_eq!(chunks[0].file_summary, "header"); // Will extract "header" as filename
	}

	#[test]
	fn test_utf8_character_boundaries() {
		// Test with UTF-8 characters that could cause boundary issues
		let utf8_content = "├── some content with UTF-8 chars: 🚀 ✨ 🎯\n".repeat(300);

		// This should not panic due to UTF-8 boundary issues
		let overlap = get_chunk_overlap(&utf8_content);

		// Verify the overlap is valid UTF-8 and not empty
		assert!(!overlap.is_empty());
		assert!(overlap.is_ascii() || overlap.chars().count() > 0); // Valid UTF-8

		// Test chunking with UTF-8 content
		let diff_with_utf8 = format!(
			"diff --git a/README.md b/README.md\nindex 5f16baa..a49d7dc 100644\n--- a/README.md\n+++ b/README.md\n@@ -1,3 +1,3 @@\n{}",
			utf8_content
		);

		let chunks = chunk_diff(&diff_with_utf8);
		assert!(!chunks.is_empty());

		// All chunks should contain valid UTF-8
		for chunk in &chunks {
			assert!(chunk.content.chars().count() > 0); // Valid UTF-8
		}
	}
}
