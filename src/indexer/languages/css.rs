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

//! CSS/SCSS/SASS language implementation for the indexer

use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct Css {}

impl Language for Css {
	fn name(&self) -> &'static str {
		"css"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		// Use CSS parser for both CSS and SCSS/SASS files
		// SCSS parser can handle CSS syntax as well
		tree_sitter_css::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec![
			"rule_set",            // CSS rules like .class { ... }
			"at_rule",             // @media, @keyframes, @import, etc.
			"keyframes_statement", // @keyframes specific
			"media_statement",     // @media specific
			"import_statement",    // @import specific
			                       // Removed "declaration" to avoid duplication with rule_set
			                       // rule_set already contains all declarations within it
		]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"rule_set" => {
				// Extract selectors from CSS rules
				for child in node.children(&mut node.walk()) {
					if child.kind() == "selectors" {
						Self::extract_css_selectors(child, contents, &mut symbols);
						break;
					}
				}
			}
			"at_rule" | "keyframes_statement" | "media_statement" | "import_statement" => {
				// Extract at-rule names (e.g., keyframe names, media query names)
				for child in node.children(&mut node.walk()) {
					if child.kind() == "identifier" || child.kind() == "keyframes_name" {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							symbols.push(name.trim().to_string());
						}
					}
				}
			}
			// Removed declaration handling to avoid duplication with rule_set
			_ => self.extract_identifiers(node, contents, &mut symbols),
		}

		// Deduplicate symbols before returning
		symbols.sort();
		symbols.dedup();

		symbols
	}

	fn extract_identifiers(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		let kind = node.kind();

		// Extract meaningful CSS identifiers
		if matches!(
			kind,
			"identifier"
				| "class_name"
				| "id_name" | "tag_name"
				| "property_name"
				| "keyframes_name"
				| "custom_property_name"
		) {
			if let Ok(text) = node.utf8_text(contents.as_bytes()) {
				let t = text.trim();
				if !t.is_empty() && !symbols.contains(&t.to_string()) {
					symbols.push(t.to_string());
				}
			}
		}

		// Continue with normal recursion for other nodes
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				self.extract_identifiers(cursor.node(), contents, symbols);
				if !cursor.goto_next_sibling() {
					break;
				}
			}
		}
	}

	fn are_node_types_equivalent(&self, type1: &str, type2: &str) -> bool {
		// Direct match
		if type1 == type2 {
			return true;
		}

		// CSS-specific semantic groups
		let semantic_groups = [
			// CSS rules and selectors
			&["rule_set", "selector", "selectors"] as &[&str],
			// At-rules
			&[
				"at_rule",
				"keyframes_statement",
				"media_statement",
				"import_statement",
				"supports_statement",
			],
			// Selectors
			&[
				"class_selector",
				"id_selector",
				"tag_name",
				"universal_selector",
			],
		];

		// Check if both types belong to the same semantic group
		for group in &semantic_groups {
			let contains_type1 = group.contains(&type1);
			let contains_type2 = group.contains(&type2);

			if contains_type1 && contains_type2 {
				return true;
			}
		}

		false
	}

	fn get_node_type_description(&self, node_type: &str) -> &'static str {
		match node_type {
			"rule_set" => "CSS rules",
			"at_rule" | "keyframes_statement" | "media_statement" | "import_statement" => {
				"at-rule declarations"
			}
			"selector" | "selectors" | "class_selector" | "id_selector" => "CSS selectors",
			_ => "CSS declarations",
		}
	}
}

impl Css {
	/// Extract CSS selectors from a selectors node
	pub fn extract_css_selectors(node: Node, contents: &str, symbols: &mut Vec<String>) {
		let mut cursor = node.walk();
		if cursor.goto_first_child() {
			loop {
				let child = cursor.node();

				// Extract different types of selectors
				match child.kind() {
					"class_selector" | "id_selector" | "tag_name" | "universal_selector" => {
						if let Ok(selector_text) = child.utf8_text(contents.as_bytes()) {
							let selector = selector_text.trim();
							if !selector.is_empty() && !symbols.contains(&selector.to_string()) {
								symbols.push(selector.to_string());
							}
						}
					}
					"pseudo_class_selector" | "pseudo_element_selector" => {
						// Extract pseudo-class/element names
						for pseudo_child in child.children(&mut child.walk()) {
							if pseudo_child.kind() == "identifier" {
								if let Ok(pseudo_name) = pseudo_child.utf8_text(contents.as_bytes())
								{
									let name = format!(":{}", pseudo_name.trim());
									if !symbols.contains(&name) {
										symbols.push(name);
									}
								}
							}
						}
					}
					_ => {
						// Recursively process other selector components
						Self::extract_css_selectors(child, contents, symbols);
					}
				}

				if !cursor.goto_next_sibling() {
					break;
				}
			}
		}
	}
}
