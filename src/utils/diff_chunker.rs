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

//! Simple diff chunking utility for large commits
//!
//! Splits large git diffs into manageable chunks for AI processing,
//! then combines the results. No complexity, just works.

use anyhow::Result;

const MAX_CHUNK_SIZE: usize = 8000; // characters per chunk
const CHUNK_OVERLAP: usize = 200; // overlap between chunks

#[derive(Debug, Clone)]
pub struct DiffChunk {
	pub content: String,
	pub file_summary: String, // Brief summary of files in this chunk
}

/// Split a large diff into manageable chunks
pub fn chunk_diff(diff_content: &str) -> Vec<DiffChunk> {
	if diff_content.len() <= MAX_CHUNK_SIZE {
		return vec![DiffChunk {
			content: diff_content.to_string(),
			file_summary: extract_file_summary(diff_content),
		}];
	}

	let mut chunks = Vec::new();
	let files = split_diff_by_files(diff_content);

	let mut current_chunk = String::new();
	let mut current_files = Vec::new();

	for file_diff in files {
		// If adding this file would exceed chunk size, create a chunk
		if !current_chunk.is_empty() && current_chunk.len() + file_diff.len() > MAX_CHUNK_SIZE {
			chunks.push(DiffChunk {
				content: current_chunk.clone(),
				file_summary: current_files.join(", "),
			});

			// Start new chunk with overlap from previous
			current_chunk = get_chunk_overlap(&current_chunk);
			current_files.clear();
		}

		current_chunk.push_str(&file_diff);
		current_chunk.push('\n');

		// Extract filename for summary
		if let Some(filename) = extract_filename(&file_diff) {
			current_files.push(filename);
		}
	}

	// Add final chunk
	if !current_chunk.is_empty() {
		chunks.push(DiffChunk {
			content: current_chunk,
			file_summary: current_files.join(", "),
		});
	}

	chunks
}

/// Split diff content by individual files
fn split_diff_by_files(diff_content: &str) -> Vec<String> {
	let mut files = Vec::new();
	let mut current_file = String::new();

	for line in diff_content.lines() {
		if line.starts_with("diff --git") && !current_file.is_empty() {
			files.push(current_file.clone());
			current_file.clear();
		}
		current_file.push_str(line);
		current_file.push('\n');
	}

	if !current_file.is_empty() {
		files.push(current_file);
	}

	files
}

/// Extract filename from a file diff
fn extract_filename(file_diff: &str) -> Option<String> {
	for line in file_diff.lines() {
		if line.starts_with("diff --git") {
			// Extract filename from "diff --git a/path b/path"
			let parts: Vec<&str> = line.split_whitespace().collect();
			if parts.len() >= 4 {
				let path = parts[3].strip_prefix("b/").unwrap_or(parts[3]);
				return Some(path.to_string());
			}
		}
	}
	None
}

/// Get overlap content from end of chunk
fn get_chunk_overlap(chunk: &str) -> String {
	if chunk.len() <= CHUNK_OVERLAP {
		return chunk.to_string();
	}

	// Get last CHUNK_OVERLAP characters, but try to break at line boundary
	let start_pos = chunk.len() - CHUNK_OVERLAP;
	let overlap_section = &chunk[start_pos..];

	if let Some(newline_pos) = overlap_section.find('\n') {
		overlap_section[newline_pos + 1..].to_string()
	} else {
		overlap_section.to_string()
	}
}

/// Extract file summary from diff content
fn extract_file_summary(diff_content: &str) -> String {
	let mut files = Vec::new();

	for line in diff_content.lines() {
		if line.starts_with("diff --git") {
			if let Some(filename) = extract_filename(line) {
				files.push(filename);
			}
		}
	}

	if files.is_empty() {
		"changes".to_string()
	} else if files.len() == 1 {
		files[0].clone()
	} else if files.len() <= 3 {
		files.join(", ")
	} else {
		format!("{} files", files.len())
	}
}

/// Combine multiple AI responses into a coherent result
pub fn combine_commit_messages(responses: Vec<String>) -> String {
	if responses.is_empty() {
		return "chore: update files".to_string();
	}

	if responses.len() == 1 {
		return responses[0].clone();
	}

	// Extract subjects and bodies from responses
	let mut subjects = Vec::new();
	let mut bodies = Vec::new();

	for response in &responses {
		let lines: Vec<&str> = response.lines().collect();
		if let Some(subject) = lines.first() {
			subjects.push(subject.trim());
		}

		// Collect body lines (after first line)
		if lines.len() > 2 {
			let body_lines: Vec<&str> = lines[2..].to_vec();
			if !body_lines.is_empty() {
				bodies.push(body_lines.join("\n"));
			}
		}
	}

	// Create combined subject - use the most comprehensive one
	let combined_subject = subjects
		.into_iter()
		.max_by_key(|s| s.len())
		.unwrap_or("chore: update multiple files")
		.to_string();

	// Combine bodies if present
	if bodies.is_empty() {
		combined_subject
	} else {
		let unique_bodies: std::collections::HashSet<String> = bodies.into_iter().collect();
		let combined_body = unique_bodies.into_iter().collect::<Vec<_>>().join("\n\n");
		format!("{}\n\n{}", combined_subject, combined_body)
	}
}

/// Combine multiple review responses into a comprehensive result
pub fn combine_review_results(responses: Vec<String>) -> Result<String> {
	if responses.is_empty() {
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

	for response in &responses {
		if let Ok(review) = serde_json::from_str::<serde_json::Value>(response) {
			// Extract issues
			if let Some(issues) = review.get("issues").and_then(|i| i.as_array()) {
				for issue in issues {
					all_issues.push(issue.clone());
				}
			}

			// Extract recommendations
			if let Some(recs) = review.get("recommendations").and_then(|r| r.as_array()) {
				for rec in recs {
					if let Some(rec_str) = rec.as_str() {
						all_recommendations.push(rec_str.to_string());
					}
				}
			}

			// Extract summary info
			if let Some(summary) = review.get("summary") {
				if let Some(files) = summary.get("total_files").and_then(|f| f.as_u64()) {
					total_files += files as usize;
				}
				if let Some(score) = summary.get("overall_score").and_then(|s| s.as_u64()) {
					scores.push(score as u8);
				}
			}
		}
	}

	// Calculate average score
	let avg_score = if scores.is_empty() {
		75
	} else {
		scores.iter().sum::<u8>() / scores.len() as u8
	};

	// Remove duplicate recommendations
	let unique_recommendations: std::collections::HashSet<String> =
		all_recommendations.into_iter().collect();
	let final_recommendations: Vec<String> = unique_recommendations.into_iter().collect();

	// Create combined result
	let combined_result = serde_json::json!({
		"summary": {
			"total_files": total_files,
			"total_issues": all_issues.len(),
			"overall_score": avg_score
		},
		"issues": all_issues,
		"recommendations": final_recommendations
	});

	Ok(serde_json::to_string_pretty(&combined_result)?)
}

/// Create fallback review JSON when parsing fails
fn create_fallback_review_json() -> String {
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
	.to_string()
}
