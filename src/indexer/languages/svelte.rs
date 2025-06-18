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

//! Svelte language implementation for the indexer

use crate::indexer::languages::Language;
use tree_sitter::Node;

pub struct Svelte {}

impl Language for Svelte {
	fn name(&self) -> &'static str {
		"svelte"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		tree_sitter_svelte_ng::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		vec![
			// Focus on script and style blocks which contain the meaningful code
			"script_element",
			"style_element",
			// Elements with meaningful attributes (components, events)
			"element", // But we'll filter this in extract_symbols
			           // Skip individual HTML tags as they create too much noise
		]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"script_element" => {
				// Extract JavaScript symbols from script content
				self.extract_script_content_symbols(node, contents, &mut symbols);
			}
			"style_element" => {
				// Extract CSS symbols from style content
				self.extract_style_content_symbols(node, contents, &mut symbols);
			}
			"element" => {
				// Only extract from elements that have meaningful Svelte-specific attributes
				self.extract_meaningful_element_symbols(node, contents, &mut symbols);
			}
			_ => self.extract_identifiers(node, contents, &mut symbols),
		}

		// Deduplicate and sort symbols
		symbols.sort();
		symbols.dedup();
		symbols
	}

	fn extract_identifiers(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		let kind = node.kind();

		// Extract meaningful identifiers, avoiding noise
		if (kind.contains("identifier") || kind.contains("name"))
			&& !kind.contains("property")
			&& kind != "tag_name"
		{
			if let Ok(text) = node.utf8_text(contents.as_bytes()) {
				let text = text.trim();
				if !text.is_empty() && !symbols.contains(&text.to_string()) {
					// Filter out common Svelte keywords and HTML tags
					if !self.is_svelte_keyword(text) && !self.is_html_tag(text) {
						symbols.push(text.to_string());
					}
				}
			}
		}

		// Recursively process children
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

		// Svelte-specific semantic groups
		let semantic_groups = [
			// Functions and methods
			&[
				"function_declaration",
				"method_definition",
				"arrow_function",
			] as &[&str],
			// Variable declarations
			&[
				"variable_declaration",
				"lexical_declaration",
				"reactive_declaration",
			],
			// Reactive statements
			&["reactive_statement", "reactive_declaration"],
			// Components and elements
			&["component", "element"],
			// Script and style blocks
			&["script_element", "style_element"],
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
			"function_declaration" | "method_definition" | "arrow_function" => {
				"function declarations"
			}
			"variable_declaration" | "lexical_declaration" => "variable declarations",
			"reactive_statement" | "reactive_declaration" => "reactive declarations",
			"class_declaration" => "class declarations",
			"component" | "element" => "component declarations",
			"script_element" => "script blocks",
			"style_element" => "style blocks",
			_ => "declarations",
		}
	}
}

impl Svelte {
	/// Extract JavaScript symbols from script element content
	fn extract_script_content_symbols(
		&self,
		node: Node,
		contents: &str,
		symbols: &mut Vec<String>,
	) {
		// Look for the raw_text content inside script element
		for child in node.children(&mut node.walk()) {
			if child.kind() == "raw_text" {
				// Get the script content
				if let Ok(script_content) = child.utf8_text(contents.as_bytes()) {
					// Parse common JavaScript patterns manually since we can't re-parse with JS parser
					self.extract_js_patterns_from_text(script_content, symbols);
				}
			}
		}
	}

	/// Extract CSS symbols from style element content
	fn extract_style_content_symbols(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		// Look for the raw_text content inside style element
		for child in node.children(&mut node.walk()) {
			if child.kind() == "raw_text" {
				// Get the style content
				if let Ok(style_content) = child.utf8_text(contents.as_bytes()) {
					// Extract CSS selectors and meaningful patterns
					self.extract_css_patterns_from_text(style_content, symbols);
				}
			}
		}
	}

	/// Extract symbols from meaningful Svelte elements (components, event handlers)
	fn extract_meaningful_element_symbols(
		&self,
		node: Node,
		contents: &str,
		symbols: &mut Vec<String>,
	) {
		// Only extract from elements that have Svelte-specific attributes
		let mut has_svelte_attributes = false;

		// Check for Svelte-specific attributes
		for child in node.children(&mut node.walk()) {
			if child.kind() == "start_tag" {
				for tag_child in child.children(&mut child.walk()) {
					if tag_child.kind() == "attribute" {
						for attr_child in tag_child.children(&mut tag_child.walk()) {
							if attr_child.kind() == "attribute_name" {
								if let Ok(attr_name) = attr_child.utf8_text(contents.as_bytes()) {
									// Check for Svelte directives
									if attr_name.starts_with("on:")
										|| attr_name.starts_with("bind:")
										|| attr_name.starts_with("use:")
										|| attr_name.starts_with("transition:")
									{
										has_svelte_attributes = true;
										// Extract the directive name
										symbols.push(attr_name.to_string());
									}
								}
							}
						}
					}
					// Extract component names (capitalized elements)
					else if tag_child.kind() == "tag_name" {
						if let Ok(tag_name) = tag_child.utf8_text(contents.as_bytes()) {
							// Svelte components are typically capitalized
							if tag_name.chars().next().is_some_and(|c| c.is_uppercase())
								&& !self.is_html_tag(tag_name)
							{
								symbols.push(tag_name.to_string());
								has_svelte_attributes = true;
							}
						}
					}
				}
			}
		}

		// Only recurse if this element has meaningful Svelte content
		if !has_svelte_attributes {}
	}

	/// Extract JavaScript patterns from text content
	fn extract_js_patterns_from_text(&self, text: &str, symbols: &mut Vec<String>) {
		// Simple regex-like pattern matching for common JS constructs
		let lines: Vec<&str> = text.lines().collect();

		for line in lines {
			let line = line.trim();

			// Extract function declarations: function name() or const name =
			if line.starts_with("function ") {
				if let Some(name) = self.extract_function_name(line) {
					symbols.push(name);
				}
			}
			// Extract variable declarations: let/const/var name =
			else if line.starts_with("let ")
				|| line.starts_with("const ")
				|| line.starts_with("var ")
			{
				if let Some(name) = self.extract_variable_name(line) {
					symbols.push(name);
				}
			}
			// Extract export declarations: export let name =
			else if line.starts_with("export let ") || line.starts_with("export const ") {
				if let Some(name) = self.extract_export_name(line) {
					symbols.push(name);
				}
			}
			// Extract reactive statements: $: name =
			else if line.trim_start().starts_with("$:") {
				if let Some(name) = self.extract_reactive_name(line) {
					symbols.push(name);
				}
			}
		}
	}

	/// Extract CSS patterns from text content
	fn extract_css_patterns_from_text(&self, text: &str, symbols: &mut Vec<String>) {
		let lines: Vec<&str> = text.lines().collect();

		for line in lines {
			let line = line.trim();

			// Extract CSS selectors (simple approach)
			if line.contains('{') && !line.trim_start().starts_with("/*") {
				let selector_part = line.split('{').next().unwrap_or("").trim();
				if !selector_part.is_empty() && !selector_part.contains(':') {
					// Simple selector extraction
					for selector in selector_part.split(',') {
						let selector = selector.trim();
						if !selector.is_empty() {
							symbols.push(selector.to_string());
						}
					}
				}
			}
		}
	}

	/// Extract function name from function declaration line
	fn extract_function_name(&self, line: &str) -> Option<String> {
		// function name() or function name(
		if let Some(start) = line.find("function ") {
			let after_function = &line[start + 9..];
			if let Some(end) = after_function.find('(') {
				let name = after_function[..end].trim();
				if !name.is_empty() && !self.is_svelte_keyword(name) {
					return Some(name.to_string());
				}
			}
		}
		None
	}

	/// Extract variable name from variable declaration line
	fn extract_variable_name(&self, line: &str) -> Option<String> {
		// let name = or const name = or var name =
		let parts: Vec<&str> = line.split_whitespace().collect();
		if parts.len() >= 4 && (parts[0] == "let" || parts[0] == "const" || parts[0] == "var") {
			let name = parts[1].trim_end_matches('=').trim_end_matches(',').trim();
			if !name.is_empty() && !self.is_svelte_keyword(name) {
				return Some(name.to_string());
			}
		}
		None
	}

	/// Extract export name from export declaration line
	fn extract_export_name(&self, line: &str) -> Option<String> {
		// export let name = or export const name =
		let parts: Vec<&str> = line.split_whitespace().collect();
		if parts.len() >= 5 && parts[0] == "export" && (parts[1] == "let" || parts[1] == "const") {
			let name = parts[2].trim_end_matches('=').trim_end_matches(',').trim();
			if !name.is_empty() && !self.is_svelte_keyword(name) {
				return Some(name.to_string());
			}
		}
		None
	}

	/// Extract reactive variable name from reactive statement
	fn extract_reactive_name(&self, line: &str) -> Option<String> {
		// $: name = something or $: if (condition)
		if let Some(colon_pos) = line.find(':') {
			let after_colon = &line[colon_pos + 1..].trim();

			// Handle $: name = value
			if let Some(eq_pos) = after_colon.find('=') {
				let name = after_colon[..eq_pos].trim();
				if !name.is_empty() && !self.is_svelte_keyword(name) {
					return Some(name.to_string());
				}
			}
			// Handle $: if (condition) - extract the reactive dependency
			else if after_colon.starts_with("if ") {
				// This is a reactive statement, could extract condition variables
				// For now, just mark it as a reactive statement
				return Some("reactive_if".to_string());
			}
		}
		None
	}

	/// Check if a string is a Svelte keyword
	fn is_svelte_keyword(&self, text: &str) -> bool {
		matches!(
			text,
			"export" | "import" | "let" | "const" | "var" | "function" | "class" | "if" | "else"
			| "for" | "while" | "do" | "switch" | "case" | "default" | "break" | "continue"
			| "return" | "try" | "catch" | "finally" | "throw" | "new" | "this" | "super"
			| "true" | "false" | "null" | "undefined" | "typeof" | "instanceof" | "in" | "of"
			// Svelte-specific keywords
			| "bind" | "on" | "use" | "transition" | "out" | "animate"
		)
	}

	/// Check if a string is a common HTML tag
	fn is_html_tag(&self, text: &str) -> bool {
		matches!(
			text.to_lowercase().as_str(),
			"div"
				| "span" | "p"
				| "a" | "img"
				| "h1" | "h2"
				| "h3" | "h4"
				| "h5" | "h6"
				| "ul" | "ol"
				| "li" | "table"
				| "tr" | "td"
				| "th" | "thead"
				| "tbody" | "tfoot"
				| "form" | "input"
				| "button" | "select"
				| "option" | "textarea"
				| "label" | "header"
				| "footer" | "nav"
				| "main" | "section"
				| "article" | "aside"
				| "html" | "head"
				| "body" | "title"
				| "meta" | "link"
				| "script" | "style"
		)
	}
}
