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
			.filter(|(idx, section)| section.level == level && !processed[*idx])
			.map(|(idx, _)| idx)
			.collect();

		for section_idx in sections_at_level {
			if processed[section_idx] {
				continue;
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

	// Convert ChunkResults to DocumentBlocks
	chunk_results
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

	for (line_num, line) in lines.iter().enumerate() {
		let trimmed = line.trim_start();

		if trimmed.starts_with('#') {
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
