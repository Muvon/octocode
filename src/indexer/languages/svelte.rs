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
			// Script section functions and declarations
			"function_declaration",
			"method_definition",
			"arrow_function",
			"variable_declaration",
			"lexical_declaration",
			"class_declaration",
			// Svelte-specific reactive statements and stores
			"reactive_statement",
			"reactive_declaration",
			// Component-related
			"component",
			"element",
			// Style blocks with meaningful CSS
			"style_element",
			// Script blocks
			"script_element",
		]
	}

	fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
		let mut symbols = Vec::new();

		match node.kind() {
			"function_declaration" | "method_definition" => {
				// Extract function/method name
				for child in node.children(&mut node.walk()) {
					if child.kind() == "identifier" || child.kind().contains("name") {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							symbols.push(name.to_string());
						}
						break;
					}
				}
				// Extract variables within function body
				self.extract_svelte_variables(node, contents, &mut symbols);
			}
			"arrow_function" => {
				// Extract parent variable name for arrow functions
				if let Some(parent) = node.parent() {
					if parent.kind() == "variable_declarator" {
						for child in parent.children(&mut parent.walk()) {
							if child.kind() == "identifier" {
								if let Ok(name) = child.utf8_text(contents.as_bytes()) {
									symbols.push(name.to_string());
								}
								break;
							}
						}
					}
				}
			}
			"variable_declaration" | "lexical_declaration" => {
				// Extract variable names
				self.extract_variable_names(node, contents, &mut symbols);
			}
			"reactive_statement" | "reactive_declaration" => {
				// Extract reactive variable names (Svelte $: syntax)
				Self::extract_reactive_symbols(node, contents, &mut symbols);
			}
			"class_declaration" => {
				// Extract class name
				for child in node.children(&mut node.walk()) {
					if child.kind() == "identifier" {
						if let Ok(name) = child.utf8_text(contents.as_bytes()) {
							symbols.push(name.to_string());
						}
						break;
					}
				}
			}
			"component" | "element" => {
				// Extract component/element names and props
				self.extract_component_symbols(node, contents, &mut symbols);
			}
			"script_element" => {
				// Extract symbols from script content
				self.extract_script_symbols(node, contents, &mut symbols);
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
	/// Extract variable names from variable declarations
	fn extract_variable_names(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		for child in node.children(&mut node.walk()) {
			if child.kind() == "variable_declarator" {
				for decl_child in child.children(&mut child.walk()) {
					if decl_child.kind() == "identifier" {
						if let Ok(name) = decl_child.utf8_text(contents.as_bytes()) {
							let name = name.trim();
							if !name.is_empty() && !symbols.contains(&name.to_string()) {
								symbols.push(name.to_string());
							}
						}
						break;
					}
				}
			}
		}
	}

	/// Extract symbols from reactive statements (Svelte $: syntax)
	fn extract_reactive_symbols(node: Node, contents: &str, symbols: &mut Vec<String>) {
		// Look for identifiers in reactive statements
		for child in node.children(&mut node.walk()) {
			if child.kind() == "identifier" {
				if let Ok(name) = child.utf8_text(contents.as_bytes()) {
					let name = name.trim();
					if !name.is_empty() && !symbols.contains(&name.to_string()) {
						symbols.push(name.to_string());
					}
				}
			} else {
				// Recursively search in child nodes
				Self::extract_reactive_symbols(child, contents, symbols);
			}
		}
	}

	/// Extract symbols from component/element nodes
	fn extract_component_symbols(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		// Extract component name
		for child in node.children(&mut node.walk()) {
			if child.kind() == "tag_name" || child.kind() == "component_name" {
				if let Ok(name) = child.utf8_text(contents.as_bytes()) {
					let name = name.trim();
					if !name.is_empty() && !self.is_html_tag(name) {
						symbols.push(name.to_string());
					}
				}
			}
			// Extract prop names
			else if child.kind() == "attribute" {
				for attr_child in child.children(&mut child.walk()) {
					if attr_child.kind() == "attribute_name" {
						if let Ok(prop_name) = attr_child.utf8_text(contents.as_bytes()) {
							let prop_name = prop_name.trim();
							if !prop_name.is_empty() && !self.is_html_attribute(prop_name) {
								symbols.push(prop_name.to_string());
							}
						}
					}
				}
			}
		}
	}

	/// Extract symbols from script blocks
	fn extract_script_symbols(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		// Process script content similar to JavaScript
		for child in node.children(&mut node.walk()) {
			if child.kind() == "raw_text" {
				// The script content is in raw_text, but we need to parse it properly
				// For now, just extract basic identifiers
				self.extract_identifiers(child, contents, symbols);
			}
		}
	}

	/// Extract variables from Svelte components
	fn extract_svelte_variables(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		for child in node.children(&mut node.walk()) {
			if child.kind() == "statement_block" {
				self.extract_variables_from_block(child, contents, symbols);
			}
		}
	}

	/// Extract variables from a statement block
	fn extract_variables_from_block(&self, node: Node, contents: &str, symbols: &mut Vec<String>) {
		for child in node.children(&mut node.walk()) {
			match child.kind() {
				"variable_declaration" | "lexical_declaration" => {
					self.extract_variable_names(child, contents, symbols);
				}
				"statement_block" => {
					// Recursive search in nested blocks
					self.extract_variables_from_block(child, contents, symbols);
				}
				_ => {}
			}
		}
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

	/// Check if a string is a common HTML attribute
	fn is_html_attribute(&self, text: &str) -> bool {
		matches!(
			text.to_lowercase().as_str(),
			"id" | "class"
				| "style" | "src"
				| "href" | "alt"
				| "title" | "type"
				| "name" | "value"
				| "placeholder"
				| "disabled" | "readonly"
				| "required" | "checked"
				| "selected" | "multiple"
				| "size" | "maxlength"
				| "minlength"
				| "pattern" | "width"
				| "height" | "data"
				| "aria" | "role"
				| "tabindex"
		)
	}
}
