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

//! Markdown processing module for document analysis
//!
//! This module handles the parsing and chunking of Markdown documents into
//! meaningful sections based on header hierarchy. It provides smart chunking
//! that respects document structure and maintains context for better semantic
//! understanding.

use crate::config::Config;
use crate::embedding::calculate_content_hash_with_lines;
use crate::store::DocumentBlock;

/// Parser state for tracking code block boundaries
#[derive(Debug, Default)]
struct ParserState {
	in_code_block: bool,
	code_fence: String,      // Track the fence type (```, ~~~)
	code_block_start: usize, // Line where code block started
}
/// Represents a header section with hierarchical relationships
#[derive(Debug, Clone)]
pub struct HeaderSection {
	level: usize,
	content: String,      // ONLY actual content
	context: Vec<String>, // ["# Doc", "## Start", "### Install"] - hierarchical context
	start_line: usize,
	end_line: usize,
	children: Vec<usize>,  // Indices of child sections
	parent: Option<usize>, // Index of parent section
}

impl HeaderSection {
	/// Check if this section ends with an incomplete code block
	fn has_incomplete_code_block(&self) -> bool {
		let lines: Vec<&str> = self.content.lines().collect();
		let mut fence_count = 0;
		let mut current_fence = String::new();

		for line in lines {
			let trimmed = line.trim_start();
			if let Some(fence) = detect_code_fence(trimmed) {
				if current_fence.is_empty() {
					current_fence = fence;
					fence_count += 1;
				} else if trimmed.starts_with(&current_fence) {
					fence_count += 1;
					current_fence.clear();
				}
			}
		}

		// Odd number means unclosed code block
		fence_count % 2 != 0
	}

	/// Check if content is just a code fence with no actual content
	fn is_only_code_fence(&self) -> bool {
		let trimmed = self.content.trim();
		// Check if it's just a fence marker with optional language identifier
		if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
			let lines: Vec<&str> = trimmed.lines().collect();
			// Only fence marker(s) and no meaningful content
			return lines.len() <= 2
				&& lines.iter().all(|line| {
					let l = line.trim();
					l.starts_with("```") || l.starts_with("~~~") || l.is_empty()
				});
		}
		false
	}
}

/// Document hierarchy for smart chunking
#[derive(Debug)]
pub struct DocumentHierarchy {
	sections: Vec<HeaderSection>,
	root_sections: Vec<usize>, // Top-level section indices
}

/// Result of chunking operation
#[derive(Debug, Clone)]
pub struct ChunkResult {
	pub title: String,
	pub storage_content: String, // Content stored in database
	pub context: Vec<String>,    // Context for embeddings
	pub level: usize,
	pub start_line: usize,
	pub end_line: usize,
}

impl ChunkResult {
	/// Validate that this chunk contains meaningful content
	fn is_valid(&self) -> bool {
		// Check if content is too short to be meaningful
		if self.storage_content.len() < 10 {
			return false;
		}

		// Check if it's just code fence markers
		let trimmed = self.storage_content.trim();
		if (trimmed.starts_with("```") || trimmed.starts_with("~~~"))
			&& trimmed.lines().count() <= 2
		{
			// Just a fence and maybe a language identifier
			return false;
		}

		// Check for other patterns of invalid content
		let meaningful_lines = trimmed
			.lines()
			.filter(|line| {
				let l = line.trim();
				!l.is_empty() && !l.starts_with("```") && !l.starts_with("~~~")
			})
			.count();

		meaningful_lines > 0
	}

	/// Try to repair invalid chunk by merging with adjacent content
	fn try_repair(&self, next_chunk: Option<&ChunkResult>) -> Option<ChunkResult> {
		if self.is_valid() {
			return Some(self.clone());
		}

		// If this chunk is just a code fence and we have a next chunk
		if let Some(next) = next_chunk {
			// Check if merging would create a valid chunk
			let merged_content = format!("{}\n{}", self.storage_content, next.storage_content);

			let repaired = ChunkResult {
				title: self.title.clone(),
				storage_content: merged_content,
				context: self.context.clone(),
				level: self.level,
				start_line: self.start_line,
				end_line: next.end_line,
			};

			if repaired.is_valid() {
				return Some(repaired);
			}
		}

		None
	}
}

/// Result of child merge analysis
#[derive(Debug)]
pub struct ChildMergeResult {
	indices: Vec<usize>,
	efficiency: f64,
}

impl Default for DocumentHierarchy {
	fn default() -> Self {
		Self::new()
	}
}

impl DocumentHierarchy {
	pub fn new() -> Self {
		Self {
			sections: Vec::new(),
			root_sections: Vec::new(),
		}
	}

	pub fn add_section(&mut self, section: HeaderSection) -> usize {
		let index = self.sections.len();
		self.sections.push(section);
		index
	}

	pub fn build_parent_child_relationships(&mut self) {
		for i in 0..self.sections.len() {
			let current_level = self.sections[i].level;

			// Find parent (previous section with lower level)
			for j in (0..i).rev() {
				if self.sections[j].level < current_level {
					self.sections[i].parent = Some(j);
					self.sections[j].children.push(i);
					break;
				}
			}

			// If no parent found, it's a root section
			if self.sections[i].parent.is_none() {
				self.root_sections.push(i);
			}
		}
	}

	/// Find the next sibling section at the same level
	fn find_next_sibling(&self, section_idx: usize) -> Option<usize> {
		if section_idx >= self.sections.len() {
			return None;
		}

		let section = &self.sections[section_idx];
		let level = section.level;

		// Look for next section at same level
		for i in (section_idx + 1)..self.sections.len() {
			if self.sections[i].level == level {
				return Some(i);
			}
			// Stop if we hit a section at a higher level
			if self.sections[i].level < level {
				break;
			}
		}

		None
	}

	/// Safely merge two sections preserving code block integrity
	fn merge_sections_safe(&self, idx1: usize, idx2: usize) -> ChunkResult {
		let section1 = &self.sections[idx1];
		let section2 = &self.sections[idx2];

		// Ensure we don't break code blocks when merging
		let merged_content = if section1.has_incomplete_code_block() {
			// Don't add extra newlines that might break the code block
			format!("{}\n{}", section1.content, section2.content)
		} else {
			// Normal merge with spacing
			format!("{}\n\n{}", section1.content, section2.content)
		};

		ChunkResult {
			title: self.get_section_title(idx1),
			storage_content: merged_content,
			context: section1.context.clone(),
			level: section1.level.min(section2.level),
			start_line: section1.start_line,
			end_line: section2.end_line,
		}
	}

	fn get_target_chunk_size(&self, header_level: usize, base_chunk_size: usize) -> usize {
		match header_level {
			1 => base_chunk_size * 2,       // Top-level sections can be larger
			2 => base_chunk_size,           // Standard size
			3 => (base_chunk_size * 3) / 4, // Slightly smaller
			4 => base_chunk_size / 2,       // Smaller for detailed sections
			_ => base_chunk_size / 3,       // Very small for deep nesting
		}
	}

	pub fn bottom_up_chunking(&self, base_chunk_size: usize) -> Vec<ChunkResult> {
		let mut chunks = Vec::new();
		let mut processed = vec![false; self.sections.len()];

		// Process from deepest level to shallowest
		for level in (1..=6).rev() {
			self.process_level(level, &mut chunks, &mut processed, base_chunk_size);
		}

		self.post_process_tiny_chunks(chunks, base_chunk_size)
	}

	fn post_process_tiny_chunks(
		&self,
		chunks: Vec<ChunkResult>,
		base_chunk_size: usize,
	) -> Vec<ChunkResult> {
		let tiny_threshold = base_chunk_size / 4;
		let mut result = Vec::new();
		let mut i = 0;

		while i < chunks.len() {
			let current_chunk = &chunks[i];

			if current_chunk.storage_content.len() < tiny_threshold && i + 1 < chunks.len() {
				// Try to merge with next chunk
				if let Some(merged) = self.try_merge_tiny_chunks(&chunks[i], &chunks[i + 1]) {
					result.push(merged);
					i += 2; // Skip both chunks
					continue;
				}
			}

			result.push(chunks[i].clone());
			i += 1;
		}

		// Handle remaining single tiny chunk at the end
		if result.len() > 1 {
			let last_idx = result.len() - 1;
			if result[last_idx].storage_content.len() < tiny_threshold {
				// Merge last tiny chunk with previous one
				let tiny_chunk = result.pop().unwrap();
				let prev_chunk = result.last_mut().unwrap();

				prev_chunk.storage_content = format!(
					"{}\n\n{}\n{}",
					prev_chunk.storage_content,
					tiny_chunk.context.last().unwrap_or(&tiny_chunk.title),
					tiny_chunk.storage_content
				);
				prev_chunk.end_line = tiny_chunk.end_line;
			}
		}

		result
	}

	fn try_merge_tiny_chunks(
		&self,
		first: &ChunkResult,
		second: &ChunkResult,
	) -> Option<ChunkResult> {
		// Only merge if they're reasonably close in the document
		if second.start_line.saturating_sub(first.end_line) <= 5 {
			Some(ChunkResult {
				title: first.title.clone(),
				storage_content: format!(
					"{}\n\n{}\n{}",
					first.storage_content,
					second.context.last().unwrap_or(&second.title),
					second.storage_content
				),
				context: first.context.clone(),
				level: first.level.min(second.level),
				start_line: first.start_line,
				end_line: second.end_line,
			})
		} else {
			None
		}
	}

	fn process_level(
		&self,
		level: usize,
		chunks: &mut Vec<ChunkResult>,
		processed: &mut Vec<bool>,
		base_chunk_size: usize,
	) {
		let sections_at_level: Vec<usize> = self
			.sections
			.iter()
			.enumerate()
			.filter(|(idx, section)| {
				section.level == level && !processed[*idx] && !section.is_only_code_fence() // Skip fence-only sections
			})
			.map(|(idx, _)| idx)
			.collect();

		for section_idx in sections_at_level {
			if processed[section_idx] {
				continue;
			}

			let section = &self.sections[section_idx];

			// If section has incomplete code block, try to merge with next section
			if section.has_incomplete_code_block() {
				if let Some(next_idx) = self.find_next_sibling(section_idx) {
					if !processed[next_idx] {
						let merged = self.merge_sections_safe(section_idx, next_idx);
						chunks.push(merged);
						processed[section_idx] = true;
						processed[next_idx] = true;
						continue;
					}
				}
			}

			let target_size = self.get_target_chunk_size(level, base_chunk_size);
			let section_content = &self.sections[section_idx].content;

			if section_content.len() <= target_size {
				// Section fits in target size, merge with children if beneficial
				let chunk = self.merge_section_with_children(section_idx, processed);
				chunks.push(chunk);
				self.mark_section_tree_processed(section_idx, processed);
			} else {
				// Section is too large, process children separately
				self.process_children_smartly(section_idx, chunks, processed, base_chunk_size);

				// Create chunk for this section alone
				let chunk = self.create_chunk_for_section(section_idx);
				chunks.push(chunk);
				processed[section_idx] = true;
			}
		}
	}

	fn process_children_smartly(
		&self,
		section_idx: usize,
		chunks: &mut Vec<ChunkResult>,
		processed: &mut [bool],
		base_chunk_size: usize,
	) {
		let unprocessed_children: Vec<usize> = self.sections[section_idx]
			.children
			.iter()
			.filter(|&&child_idx| !processed[child_idx])
			.copied()
			.collect();

		if unprocessed_children.is_empty() {
			return;
		}

		// Group children by size and try to merge small ones
		let mut remaining_children = unprocessed_children;

		while !remaining_children.is_empty() {
			let best_merge = self.find_best_child_merge(&remaining_children, base_chunk_size);

			if best_merge.indices.len() > 1 {
				// Merge multiple small children together
				let merged_chunk = self.merge_multiple_sections(&best_merge.indices);
				chunks.push(merged_chunk);

				// Mark as processed and remove from remaining
				for &idx in &best_merge.indices {
					processed[idx] = true;
				}
				remaining_children.retain(|&idx| !best_merge.indices.contains(&idx));
			} else {
				// Process single child (couldn't find good merge)
				let child_idx = remaining_children.remove(0);
				let child_chunk = self.create_chunk_for_section(child_idx);
				chunks.push(child_chunk);
				processed[child_idx] = true;
			}
		}
	}

	fn find_best_child_merge(
		&self,
		children: &[usize],
		base_chunk_size: usize,
	) -> ChildMergeResult {
		let mut best_merge = ChildMergeResult {
			indices: Vec::new(),
			efficiency: 0.0,
		};

		// Try different combinations of consecutive children
		for start in 0..children.len() {
			for end in (start + 1)..=children.len().min(start + 4) {
				let candidate_indices = &children[start..end];
				let total_size: usize = candidate_indices
					.iter()
					.map(|&idx| self.sections[idx].content.len())
					.sum();

				if total_size <= base_chunk_size {
					let efficiency = total_size as f64 / base_chunk_size as f64;
					let size_bonus = candidate_indices.len() as f64 * 0.1; // Favor merging more sections
					let final_efficiency = efficiency + size_bonus;

					if final_efficiency > best_merge.efficiency {
						best_merge = ChildMergeResult {
							indices: candidate_indices.to_vec(),
							efficiency: final_efficiency,
						};
					}
				}
			}
		}

		// If no good merge found, return single item
		if best_merge.indices.is_empty() && !children.is_empty() {
			best_merge.indices.push(children[0]);
		}

		best_merge
	}

	// Note: can_merge_sections was part of old implementation, removed as unused

	fn merge_multiple_sections(&self, indices: &[usize]) -> ChunkResult {
		if indices.is_empty() {
			return ChunkResult {
				title: "Empty Section".to_string(),
				storage_content: String::new(),
				context: Vec::new(),
				level: 1,
				start_line: 0,
				end_line: 0,
			};
		}

		let first_section = &self.sections[indices[0]];
		let mut combined_content = Vec::new();
		let mut end_line = first_section.end_line;

		for &idx in indices {
			let section = &self.sections[idx];
			if !section.context.is_empty() {
				combined_content.push(section.context.last().unwrap().clone());
			}
			combined_content.push(section.content.clone());
			end_line = end_line.max(section.end_line);
		}

		ChunkResult {
			title: self.get_section_title(indices[0]),
			storage_content: combined_content.join("\n\n"),
			context: first_section.context.clone(),
			level: first_section.level,
			start_line: first_section.start_line,
			end_line,
		}
	}

	fn get_section_title(&self, section_idx: usize) -> String {
		let section = &self.sections[section_idx];
		section
			.context
			.last()
			.unwrap_or(&"Untitled Section".to_string())
			.to_string()
	}

	fn merge_section_with_children(&self, section_idx: usize, processed: &[bool]) -> ChunkResult {
		let section = &self.sections[section_idx];
		let mut content_parts = vec![section.content.clone()];
		let mut end_line = section.end_line;

		// Add unprocessed children content
		for &child_idx in &section.children {
			if !processed[child_idx] {
				let child = &self.sections[child_idx];
				if !child.context.is_empty() {
					content_parts.push(child.context.last().unwrap().clone());
				}
				content_parts.push(child.content.clone());
				end_line = end_line.max(child.end_line);
			}
		}

		ChunkResult {
			title: self.get_section_title(section_idx),
			storage_content: content_parts.join("\n\n"),
			context: section.context.clone(),
			level: section.level,
			start_line: section.start_line,
			end_line,
		}
	}

	fn create_chunk_for_section(&self, section_idx: usize) -> ChunkResult {
		let section = &self.sections[section_idx];

		ChunkResult {
			title: self.get_section_title(section_idx),
			storage_content: section.content.clone(),
			context: section.context.clone(),
			level: section.level,
			start_line: section.start_line,
			end_line: section.end_line,
		}
	}

	// Note: collect_section_tree functions were part of old implementation, removed as unused

	fn mark_section_tree_processed(&self, section_idx: usize, processed: &mut Vec<bool>) {
		processed[section_idx] = true;
		for &child_idx in &self.sections[section_idx].children {
			self.mark_section_tree_processed(child_idx, processed);
		}
	}
}

/// Parse markdown content and split it into meaningful chunks by headers
pub fn parse_markdown_content(
	contents: &str,
	file_path: &str,
	config: &Config,
) -> Vec<DocumentBlock> {
	// Parse the document into hierarchical sections
	let hierarchy = parse_document_hierarchy(contents);

	// Perform bottom-up chunking
	let chunk_results = hierarchy.bottom_up_chunking(config.index.chunk_size);

	// Validate and repair chunks to ensure meaningful content
	let mut valid_chunks = Vec::new();
	let mut i = 0;

	while i < chunk_results.len() {
		let current = &chunk_results[i];

		if current.is_valid() {
			valid_chunks.push(current.clone());
			i += 1;
		} else {
			// Try to repair by merging with next chunk
			let next = if i + 1 < chunk_results.len() {
				Some(&chunk_results[i + 1])
			} else {
				None
			};

			if let Some(repaired) = current.try_repair(next) {
				valid_chunks.push(repaired);
				i += if next.is_some() { 2 } else { 1 };
			} else {
				// Skip invalid chunk that couldn't be repaired
				eprintln!(
					"Warning: Skipping invalid chunk in {} at lines {}-{}: '{}'",
					file_path,
					current.start_line,
					current.end_line,
					current.storage_content.chars().take(50).collect::<String>()
				);
				i += 1;
			}
		}
	}

	// Convert valid ChunkResults to DocumentBlocks
	valid_chunks
		.into_iter()
		.map(|chunk| {
			let content_hash = calculate_content_hash_with_lines(
				&chunk.storage_content,
				file_path,
				chunk.start_line,
				chunk.end_line,
			);
			DocumentBlock {
				path: file_path.to_string(),
				title: chunk.title,
				content: chunk.storage_content, // Storage content only
				context: chunk.context,         // Context for embeddings
				level: chunk.level,
				start_line: chunk.start_line,
				end_line: chunk.end_line,
				hash: content_hash,
				distance: None,
			}
		})
		.collect()
}

/// Parse markdown document into hierarchical structure
pub fn parse_document_hierarchy(contents: &str) -> DocumentHierarchy {
	let mut hierarchy = DocumentHierarchy::new();
	let lines: Vec<&str> = contents.lines().collect();
	let mut header_stack: Vec<String> = Vec::new();

	let mut current_section: Option<HeaderSection> = None;
	let mut current_content = String::new();

	// Track code block state to avoid treating headers inside code blocks as actual headers
	let mut parser_state = ParserState::default();

	for (line_num, line) in lines.iter().enumerate() {
		let trimmed = line.trim_start();

		// Check for code block boundaries
		if let Some(fence) = detect_code_fence(trimmed) {
			if !parser_state.in_code_block {
				parser_state.in_code_block = true;
				parser_state.code_fence = fence;
				parser_state.code_block_start = line_num;
			} else if trimmed.starts_with(&parser_state.code_fence) {
				parser_state.in_code_block = false;
				parser_state.code_fence.clear();
			}
		}

		// Don't treat lines starting with # as headers if we're in a code block
		if trimmed.starts_with('#') && !parser_state.in_code_block {
			// Finalize previous section
			if let Some(mut section) = current_section.take() {
				section.content = current_content.trim().to_string();
				section.end_line = line_num.saturating_sub(1);
				if !section.content.is_empty() {
					hierarchy.add_section(section);
				}
			}

			// Parse new header
			let header_level = trimmed.chars().take_while(|&c| c == '#').count();
			let header_title = trimmed.trim_start_matches('#').trim().to_string();
			let header_line = format!("{} {}", "#".repeat(header_level), header_title);

			// Update header stack to maintain hierarchy
			header_stack.truncate(header_level.saturating_sub(1));
			header_stack.push(header_line.clone());

			// Start new section
			current_section = Some(HeaderSection {
				level: header_level,
				content: String::new(),
				context: header_stack.clone(),
				start_line: line_num,
				end_line: line_num,
				children: Vec::new(),
				parent: None,
			});
			current_content.clear();
		} else {
			// Add content line to current section
			if !current_content.is_empty() {
				current_content.push('\n');
			}
			current_content.push_str(line);
		}
	}

	// Don't forget the last section
	if let Some(mut section) = current_section {
		section.content = current_content.trim().to_string();
		section.end_line = lines.len().saturating_sub(1);
		if !section.content.is_empty() {
			hierarchy.add_section(section);
		}
	}

	// Build parent-child relationships
	hierarchy.build_parent_child_relationships();

	hierarchy
}

/// Helper function to detect code fences and return the fence type
fn detect_code_fence(line: &str) -> Option<String> {
	let trimmed = line.trim_start();
	if trimmed.starts_with("```") {
		// Extract the fence (``` or longer sequences like ````)
		let fence_end = trimmed.chars().take_while(|&c| c == '`').count();
		Some("`".repeat(fence_end))
	} else if trimmed.starts_with("~~~") {
		// Extract the fence (~~~ or longer sequences like ~~~~)
		let fence_end = trimmed.chars().take_while(|&c| c == '~').count();
		Some("~".repeat(fence_end))
	} else {
		None
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::Config;

	#[test]
	fn test_detect_code_fence() {
		assert_eq!(detect_code_fence("```rust"), Some("```".to_string()));
		assert_eq!(detect_code_fence("````python"), Some("````".to_string()));
		assert_eq!(detect_code_fence("~~~bash"), Some("~~~".to_string()));
		assert_eq!(detect_code_fence("~~~~yaml"), Some("~~~~".to_string()));
		assert_eq!(detect_code_fence("# Header"), None);
		assert_eq!(detect_code_fence("regular text"), None);
		assert_eq!(
			detect_code_fence("  ```  indented"),
			Some("```".to_string())
		);
	}

	#[test]
	fn test_has_incomplete_code_block() {
		let section_complete = HeaderSection {
			level: 1,
			content: "```bash\necho test\n```".to_string(),
			context: vec!["# Test".to_string()],
			start_line: 1,
			end_line: 3,
			children: Vec::new(),
			parent: None,
		};
		assert!(!section_complete.has_incomplete_code_block());

		let section_incomplete = HeaderSection {
			level: 1,
			content: "```bash\necho test".to_string(),
			context: vec!["# Test".to_string()],
			start_line: 1,
			end_line: 2,
			children: Vec::new(),
			parent: None,
		};
		assert!(section_incomplete.has_incomplete_code_block());

		let section_no_code = HeaderSection {
			level: 1,
			content: "Just regular text".to_string(),
			context: vec!["# Test".to_string()],
			start_line: 1,
			end_line: 1,
			children: Vec::new(),
			parent: None,
		};
		assert!(!section_no_code.has_incomplete_code_block());
	}

	#[test]
	fn test_is_only_code_fence() {
		let section_only_fence = HeaderSection {
			level: 1,
			content: "```bash".to_string(),
			context: vec!["# Test".to_string()],
			start_line: 1,
			end_line: 1,
			children: Vec::new(),
			parent: None,
		};
		assert!(section_only_fence.is_only_code_fence());

		let section_fence_and_content = HeaderSection {
			level: 1,
			content: "```bash\necho test\n```".to_string(),
			context: vec!["# Test".to_string()],
			start_line: 1,
			end_line: 3,
			children: Vec::new(),
			parent: None,
		};
		assert!(!section_fence_and_content.is_only_code_fence());

		let section_regular = HeaderSection {
			level: 1,
			content: "Regular content".to_string(),
			context: vec!["# Test".to_string()],
			start_line: 1,
			end_line: 1,
			children: Vec::new(),
			parent: None,
		};
		assert!(!section_regular.is_only_code_fence());
	}

	#[test]
	fn test_chunk_validation() {
		let invalid_chunk_short = ChunkResult {
			title: "Test".to_string(),
			storage_content: "```".to_string(),
			context: vec!["# Test".to_string()],
			level: 1,
			start_line: 1,
			end_line: 1,
		};
		assert!(!invalid_chunk_short.is_valid());

		let invalid_chunk_fence_only = ChunkResult {
			title: "Test".to_string(),
			storage_content: "```bash".to_string(),
			context: vec!["# Test".to_string()],
			level: 1,
			start_line: 1,
			end_line: 1,
		};
		assert!(!invalid_chunk_fence_only.is_valid());

		let valid_chunk = ChunkResult {
			title: "Test".to_string(),
			storage_content: "This is meaningful content with enough text".to_string(),
			context: vec!["# Test".to_string()],
			level: 1,
			start_line: 1,
			end_line: 1,
		};
		assert!(valid_chunk.is_valid());

		let valid_chunk_with_code = ChunkResult {
			title: "Test".to_string(),
			storage_content: "```bash\necho 'hello world'\n```".to_string(),
			context: vec!["# Test".to_string()],
			level: 1,
			start_line: 1,
			end_line: 3,
		};
		assert!(valid_chunk_with_code.is_valid());
	}

	#[test]
	fn test_chunk_repair() {
		let invalid_chunk = ChunkResult {
			title: "Test".to_string(),
			storage_content: "```bash".to_string(),
			context: vec!["# Test".to_string()],
			level: 1,
			start_line: 1,
			end_line: 1,
		};

		let next_chunk = ChunkResult {
			title: "Next".to_string(),
			storage_content: "echo 'hello'\n```".to_string(),
			context: vec!["# Next".to_string()],
			level: 1,
			start_line: 2,
			end_line: 3,
		};

		let repaired = invalid_chunk.try_repair(Some(&next_chunk));
		assert!(repaired.is_some());
		let repaired = repaired.unwrap();
		assert!(repaired.is_valid());
		assert_eq!(repaired.storage_content, "```bash\necho 'hello'\n```");
		assert_eq!(repaired.end_line, 3);
	}

	#[test]
	fn test_parse_with_code_blocks() {
		let markdown_content = r#"# Test Document

## Section One

Some content here.

### Available Configs:
```bash
echo "test"
```

## Section Two

Content after code block."#;

		let config = Config::load_from_template().expect("Failed to load config");
		let blocks = parse_markdown_content(markdown_content, "test.md", &config);

		// Should not have any blocks with only fence markers
		for block in &blocks {
			assert!(
				block.content.len() > 10,
				"Block content too short: '{}'",
				block.content
			);
			assert!(
				!block.content.trim().starts_with("```") || block.content.lines().count() > 2,
				"Block contains only fence marker: '{}'",
				block.content
			);
		}

		// Should have meaningful content
		assert!(!blocks.is_empty(), "No blocks generated");
	}

	#[test]
	fn test_code_block_state_tracking() {
		let markdown_with_header_in_code = r#"# Real Header

```markdown
# This is not a real header
## Neither is this
```

## Real Header Two

Content here."#;

		let hierarchy = parse_document_hierarchy(markdown_with_header_in_code);

		// Should only have 2 real sections, not 4
		assert_eq!(
			hierarchy.sections.len(),
			2,
			"Headers inside code blocks should be ignored"
		);

		// Check that the first section contains the entire code block
		let first_section = &hierarchy.sections[0];
		assert!(first_section
			.content
			.contains("# This is not a real header"));
		assert!(first_section.content.contains("## Neither is this"));
	}

	#[test]
	fn test_find_next_sibling() {
		let mut hierarchy = DocumentHierarchy::new();

		// Add sections: H1, H2, H2, H3, H2
		hierarchy.add_section(HeaderSection {
			level: 1,
			content: "Content 1".to_string(),
			context: vec!["# Header 1".to_string()],
			start_line: 0,
			end_line: 0,
			children: Vec::new(),
			parent: None,
		});

		hierarchy.add_section(HeaderSection {
			level: 2,
			content: "Content 2".to_string(),
			context: vec!["# Header 1".to_string(), "## Header 2".to_string()],
			start_line: 1,
			end_line: 1,
			children: Vec::new(),
			parent: None,
		});

		hierarchy.add_section(HeaderSection {
			level: 2,
			content: "Content 3".to_string(),
			context: vec!["# Header 1".to_string(), "## Header 3".to_string()],
			start_line: 2,
			end_line: 2,
			children: Vec::new(),
			parent: None,
		});

		// Next sibling of first H2 (index 1) should be second H2 (index 2)
		assert_eq!(hierarchy.find_next_sibling(1), Some(2));

		// Next sibling of second H2 (index 2) should be None
		assert_eq!(hierarchy.find_next_sibling(2), None);

		// Next sibling of H1 (index 0) should be None
		assert_eq!(hierarchy.find_next_sibling(0), None);
	}
}
