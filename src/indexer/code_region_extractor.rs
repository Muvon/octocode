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

//! Code region extraction and smart merging utilities
//!
//! This module handles the extraction of meaningful code regions from tree-sitter ASTs,
//! including smart merging of single-line declarations to create optimal code blocks
//! for indexing and embedding.

use crate::indexer::languages;
use tree_sitter::Node;

/// Represents a meaningful code block/region with tree-sitter node information.
#[derive(Clone)]
pub struct CodeRegion {
	pub content: String,
	pub symbols: Vec<String>,
	pub start_line: usize,
	pub end_line: usize,
	pub node_kind: String, // Store the original tree-sitter node kind
	pub node_id: usize,    // Store a unique node identifier for grouping
}

// Configuration for single-line block merging
const MAX_LINES_PER_BLOCK: usize = 15; // Maximum lines in a merged block
const MIN_LINES_TO_MERGE: usize = 2; // Minimum consecutive single-lines to merge

/// Recursively extracts meaningful regions based on node kinds.
/// Includes smart merging of single-line declarations.
pub fn extract_meaningful_regions(
	node: Node,
	contents: &str,
	lang_impl: &dyn languages::Language,
	regions: &mut Vec<CodeRegion>,
) {
	let meaningful_kinds = lang_impl.get_meaningful_kinds();
	let mut candidate_regions = Vec::new();

	// First pass: collect all meaningful regions without merging
	collect_meaningful_regions_recursive(
		node,
		contents,
		lang_impl,
		&meaningful_kinds,
		None,
		&mut candidate_regions,
	);

	// Second pass: apply smart merging logic
	apply_smart_merging(candidate_regions, regions, lang_impl);
}

/// Recurses into a node's children, collecting meaningful regions from each.
/// Tracks the previous sibling in true O(1) steps as it walks, so callers
/// never need to re-derive it by rescanning from the parent's first child.
fn descend_children(
	node: Node,
	contents: &str,
	lang_impl: &dyn languages::Language,
	meaningful_kinds: &[&str],
	regions: &mut Vec<CodeRegion>,
) {
	let mut cursor = node.walk();
	if cursor.goto_first_child() {
		let mut prev: Option<Node> = None;
		loop {
			let current = cursor.node();
			collect_meaningful_regions_recursive(
				current,
				contents,
				lang_impl,
				meaningful_kinds,
				prev,
				regions,
			);
			prev = Some(current);
			if !cursor.goto_next_sibling() {
				break;
			}
		}
	}
}

/// Recursively collects meaningful regions without merging.
/// `prev_sibling` is the sibling immediately preceding `node` in its own
/// parent's children (as discovered by `descend_children`'s walk), or `None`
/// at the root — used to attach a preceding comment/attribute in O(1).
fn collect_meaningful_regions_recursive(
	node: Node,
	contents: &str,
	lang_impl: &dyn languages::Language,
	meaningful_kinds: &[&str],
	prev_sibling: Option<Node>,
	regions: &mut Vec<CodeRegion>,
) {
	let node_kind = node.kind();

	if meaningful_kinds.contains(&node_kind) {
		// A language may replace this node outright with independently
		// produced sub-regions (e.g. an embedded language's own extractor)
		// instead of a single verbatim-text region.
		if let Some(sub_regions) = lang_impl.expand_meaningful_node(node, contents) {
			regions.extend(sub_regions);
			return;
		}

		let is_meaningful = lang_impl.is_meaningful_node(node, contents);
		let descend_first = lang_impl.descend_first_kinds().contains(&node_kind);

		if descend_first {
			// Look for smaller, independently-meaningful regions among the
			// children before settling for this node as one big region.
			let regions_before = regions.len();
			descend_children(node, contents, lang_impl, meaningful_kinds, regions);
			if regions.len() > regions_before {
				return;
			}
			// Nothing meaningful found below — fall through to the single-
			// region fallback so this node's content is never silently
			// dropped, even if `is_meaningful_node` itself said no (e.g. a
			// plain wrapper element with no directives and no meaningful
			// descendants).
		} else if !is_meaningful {
			descend_children(node, contents, lang_impl, meaningful_kinds, regions);
			return;
		}

		let (combined_content, start_line) =
			combine_with_preceding_comments(node, contents, prev_sibling);
		let end_line = node.end_position().row;
		let symbols = lang_impl.extract_symbols(node, contents);

		// Only create a region if we have meaningful content
		if !combined_content.trim().is_empty() {
			// Ensure we have at least one symbol by using the node kind if necessary
			let mut final_symbols = symbols;
			if final_symbols.is_empty() {
				// Create a default symbol from the node kind
				final_symbols.push(format!("{}_{}", node_kind, start_line));
			}

			regions.push(CodeRegion {
				content: combined_content,
				symbols: final_symbols,
				start_line,
				end_line,
				node_kind: node_kind.to_string(),
				node_id: node.id(),
			});
		}
		return;
	}

	descend_children(node, contents, lang_impl, meaningful_kinds, regions);
}

/// Applies smart merging logic to consolidate single-line declarations
fn apply_smart_merging(
	candidate_regions: Vec<CodeRegion>,
	final_regions: &mut Vec<CodeRegion>,
	lang_impl: &dyn languages::Language,
) {
	if candidate_regions.is_empty() {
		return;
	}

	let mut i = 0;
	while i < candidate_regions.len() {
		let current = &candidate_regions[i];

		// Check if this is a single-line block
		if is_single_line_declaration(current) {
			// Look ahead to find consecutive single-line blocks
			let mut consecutive_single_lines = vec![current];
			let mut j = i + 1;

			while j < candidate_regions.len() {
				let next = &candidate_regions[j];
				if is_single_line_declaration(next)
					&& are_consecutive_or_related(
						consecutive_single_lines.last().unwrap(),
						next,
						lang_impl,
					) {
					consecutive_single_lines.push(next);
					j += 1;
				} else {
					break;
				}
			}

			// If we have enough single-line blocks to merge
			if consecutive_single_lines.len() >= MIN_LINES_TO_MERGE {
				merge_single_line_blocks(consecutive_single_lines, final_regions, lang_impl);
				i = j; // Skip the processed blocks
			} else {
				// Not enough to merge, add as-is
				final_regions.push(current.clone());
				i += 1;
			}
		} else {
			// Multi-line block, add as-is
			final_regions.push(current.clone());
			i += 1;
		}
	}
}

/// Checks if a code region is a single-line declaration
fn is_single_line_declaration(region: &CodeRegion) -> bool {
	// Consider it single-line if it's 1 line or very short content
	let line_count = region.end_line - region.start_line;
	let is_short = region.content.trim().lines().count() <= 1 || region.content.len() < 80;

	line_count <= 1 || (line_count <= 2 && is_short)
}

/// Checks if two single-line regions are consecutive or thematically related
fn are_consecutive_or_related(
	first: &CodeRegion,
	second: &CodeRegion,
	lang_impl: &dyn languages::Language,
) -> bool {
	// Consider consecutive if they're within a few lines of each other
	let line_gap = second.start_line.saturating_sub(first.end_line);

	// Allow small gaps (for comments or empty lines)
	if line_gap <= 3 {
		return true;
	}

	// Check if they're thematically related (same type of declaration)
	if are_thematically_related(first, second, lang_impl) {
		return line_gap <= 10; // Allow larger gaps for related content
	}

	false
}

/// Checks if two regions contain thematically related content using language-specific logic
fn are_thematically_related(
	first: &CodeRegion,
	second: &CodeRegion,
	lang_impl: &dyn languages::Language,
) -> bool {
	// Direct node kind match
	if first.node_kind == second.node_kind {
		return true;
	}

	// Use language-specific semantic equivalence
	lang_impl.are_node_types_equivalent(&first.node_kind, &second.node_kind)
}

/// Merges consecutive single-line blocks into optimally-sized chunks
fn merge_single_line_blocks(
	single_lines: Vec<&CodeRegion>,
	final_regions: &mut Vec<CodeRegion>,
	lang_impl: &dyn languages::Language,
) {
	if single_lines.is_empty() {
		return;
	}

	// Group by theme first
	let mut themed_groups: Vec<Vec<&CodeRegion>> = Vec::new();
	let mut current_group = vec![single_lines[0]];

	for region in single_lines.iter().skip(1) {
		if are_thematically_related(current_group[0], region, lang_impl) {
			current_group.push(region);
		} else {
			themed_groups.push(current_group);
			current_group = vec![region];
		}
	}
	themed_groups.push(current_group);

	// Process each themed group
	for group in themed_groups {
		create_merged_blocks_from_group(group, final_regions, lang_impl);
	}
}

/// Creates merged blocks from a thematically grouped set of single-line regions
fn create_merged_blocks_from_group(
	group: Vec<&CodeRegion>,
	final_regions: &mut Vec<CodeRegion>,
	lang_impl: &dyn languages::Language,
) {
	if group.is_empty() {
		return;
	}

	// Split into chunks if the group is too large
	let chunks: Vec<&[&CodeRegion]> = group.chunks(MAX_LINES_PER_BLOCK).collect();

	for chunk in chunks {
		if chunk.is_empty() {
			continue;
		}

		let start_line = chunk[0].start_line;
		let end_line = chunk.last().unwrap().end_line;

		// Combine content from all regions in the chunk
		let mut combined_content = String::new();
		let mut all_symbols = Vec::new();

		for (i, region) in chunk.iter().enumerate() {
			if i > 0 {
				combined_content.push('\n');
			}
			combined_content.push_str(&region.content);
			all_symbols.extend(region.symbols.clone());
		}

		// Deduplicate symbols
		all_symbols.sort();
		all_symbols.dedup();

		// Add a descriptive comment to the merged block using language-specific description
		let block_type = determine_block_type(chunk[0], lang_impl);
		let block_description =
			format!("// Merged {} ({} declarations)\n", block_type, chunk.len());
		let final_content = block_description + &combined_content;

		final_regions.push(CodeRegion {
			content: final_content,
			symbols: all_symbols,
			start_line,
			end_line,
			node_kind: chunk[0].node_kind.clone(),
			node_id: chunk[0].node_id, // Use the first region's node_id as representative
		});
	}
}

/// Determines the type of block for descriptive purposes using language-specific logic
fn determine_block_type(
	sample_region: &CodeRegion,
	lang_impl: &dyn languages::Language,
) -> &'static str {
	lang_impl.get_node_type_description(&sample_region.node_kind)
}

/// Combines a preceding comment or attribute node with a declaration node.
/// `prev_sibling` must be the node's immediately-preceding sibling (or `None`
/// if it has none), as already known by the caller's tree walk.
pub fn combine_with_preceding_comments(
	node: Node,
	contents: &str,
	prev_sibling: Option<Node>,
) -> (String, usize) {
	let mut combined_start = node.start_position().row;
	let mut snippet = String::new();
	if let Some(last) = prev_sibling {
		let kind = last.kind();
		if kind.contains("comment") || kind.contains("attribute") {
			combined_start = last.start_position().row;
			snippet.push_str(&contents[last.start_byte()..last.end_byte()]);
			snippet.push('\n');
		}
	}
	snippet.push_str(&contents[node.start_byte()..node.end_byte()]);
	(snippet, combined_start)
}
