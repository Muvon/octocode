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

#[cfg(test)]
mod tests {
	use super::super::*;

	const RUST: &str = "\
use std::fs;

pub fn helper() -> u32 {
	let value = 1;
	value + 1
}

pub fn other() -> u32 {
	2
}
";

	#[test]
	fn a_rust_definition_is_extracted_up_to_its_closing_brace() {
		let snippet = extract_symbol_from_content(RUST, "helper").expect("definition found");
		assert!(snippet.starts_with("pub fn helper() -> u32 {"), "{snippet}");
		assert!(snippet.contains("value + 1"), "{snippet}");
		assert!(
			!snippet.contains("pub fn other"),
			"the scan must stop at the closing brace: {snippet}"
		);
	}

	#[test]
	fn definitions_in_other_languages_are_recognised_by_their_keyword() {
		let python = "def run():\n    return 1\n";
		assert!(extract_symbol_from_content(python, "run").is_some());

		let javascript = "function run() {\n  return 1;\n}\n";
		assert!(extract_symbol_from_content(javascript, "run").is_some());

		let class = "class Widget {\n  build() {}\n}\n";
		assert!(extract_symbol_from_content(class, "Widget").is_some());

		let constant = "const MAX = 10;\n";
		assert!(extract_symbol_from_content(constant, "MAX").is_some());

		let export = "export default Widget;\n";
		assert!(extract_symbol_from_content(export, "Widget").is_some());
	}

	#[test]
	fn a_bare_mention_is_not_a_definition() {
		let content = "let x = helper();\nprintln!(\"{x}\");\n";
		assert_eq!(extract_symbol_from_content(content, "helper"), None);
	}

	#[test]
	fn an_absent_symbol_yields_nothing() {
		assert_eq!(extract_symbol_from_content(RUST, "missing"), None);
		assert_eq!(extract_symbol_from_content("", "anything"), None);
	}

	#[test]
	fn a_definition_without_braces_is_capped_at_fifty_lines() {
		let content = format!(
			"def run():\n{}",
			(1..=100)
				.map(|i| format!("    line_{i} = {i}"))
				.collect::<Vec<_>>()
				.join("\n")
		);
		let snippet = extract_symbol_from_content(&content, "run").unwrap();
		assert!(snippet.lines().count() <= 50, "{}", snippet.lines().count());
	}
}
