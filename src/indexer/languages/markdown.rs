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

//! Markdown language support for signature extraction
//! Extracts headings as signatures with smart content preview

use super::Language;
use regex::Regex;
use std::sync::LazyLock;
use tree_sitter::Node;

static MARKDOWN_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"\[[^\]]*\]\(([^)]+\.md)(?:#[^)]*)?\)").expect("markdown link regex must be valid")
});

fn extract_markdown_links(contents: &str) -> Vec<String> {
	let mut links = Vec::new();
	for capture in MARKDOWN_LINK_RE.captures_iter(contents) {
		if let Some(target) = capture.get(1) {
			let path = target.as_str();
			if !path.starts_with("http://") && !path.starts_with("https://") {
				links.push(path.to_string());
			}
		}
	}
	links.sort_unstable();
	links.dedup();
	links
}

/// Markdown language implementation
pub struct Markdown;

impl Language for Markdown {
	fn name(&self) -> &'static str {
		"markdown"
	}

	fn get_ts_language(&self) -> tree_sitter::Language {
		// For markdown, we'll use a simple text-based approach since tree-sitter doesn't have
		// a reliable markdown parser. We'll create a dummy language that matches everything.
		// This is a placeholder - we'll handle markdown parsing in extract_symbols
		tree_sitter_json::LANGUAGE.into()
	}

	fn get_meaningful_kinds(&self) -> Vec<&'static str> {
		// For markdown, we'll handle this in extract_symbols instead
		// Return empty vec since we don't use tree-sitter parsing for markdown
		vec![]
	}

	fn extract_symbols(&self, _node: Node, contents: &str) -> Vec<String> {
		// Extract markdown headings as symbols
		let mut symbols = Vec::new();

		for line in contents.lines() {
			let trimmed = line.trim();
			if trimmed.starts_with('#') && !trimmed.starts_with("```") {
				// Extract heading text (remove # and trim)
				let heading_text = trimmed.trim_start_matches('#').trim();
				if !heading_text.is_empty() {
					symbols.push(heading_text.to_string());
				}
			}
		}

		symbols
	}

	fn extract_identifiers(&self, _node: Node, _contents: &str, _symbols: &mut Vec<String>) {
		// Not used for markdown
	}

	fn get_node_type_description(&self, _node_type: &str) -> &'static str {
		"markdown headings"
	}

	fn extract_imports_exports(&self, node: Node, contents: &str) -> (Vec<String>, Vec<String>) {
		// Only extract at the root node to avoid redundant passes during
		// the recursive AST walk (the JSON placeholder parser produces
		// multiple nodes, each receiving the same contents string).
		if node.parent().is_some() {
			return (Vec::new(), Vec::new());
		}

		// Match [text](path.md) — standard markdown links to .md files
		// Skip external URLs (http:// or https://)
		// Strip anchor fragments (#section)
		let links = extract_markdown_links(contents);

		// Markdown "exports" are section headings
		let exports = self.extract_symbols(node, contents);
		(links, exports)
	}

	fn resolve_import(
		&self,
		import_path: &str,
		source_file: &str,
		all_files: &[String],
	) -> Option<String> {
		use super::resolution_utils::resolve_relative_path;
		use crate::utils::path::PathNormalizer;

		let resolved = resolve_relative_path(source_file, import_path)?;
		PathNormalizer::find_path_in_collection(&resolved.to_string_lossy(), all_files)
			.map(str::to_string)
	}

	fn get_file_extensions(&self) -> Vec<&'static str> {
		vec!["md", "markdown"]
	}
}

impl Markdown {}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_extract_markdown_links() {
		let content = r#"
# Credit Accounts

See [Credit Suite](../core-architecture/credit-suite.md) for details.
Also check [Pool](../core-architecture/pool.md#liquidity) and
[Adapters](./adapters.md).

External links are ignored: [Docs](https://docs.example.com/guide.md)
Non-md links are ignored: [Image](./photo.png)
"#;

		let links = extract_markdown_links(content);

		assert_eq!(links.len(), 3);
		assert!(links.contains(&"../core-architecture/credit-suite.md".to_string()));
		assert!(links.contains(&"../core-architecture/pool.md".to_string()));
		assert!(links.contains(&"./adapters.md".to_string()));
		assert!(!links.iter().any(|l| l.contains("https://")));
		assert!(!links.iter().any(|l| l.contains(".png")));
	}

	#[test]
	fn test_resolve_markdown_import() {
		let md = Markdown;
		let all_files = vec![
			"core-architecture/credit-suite.md".to_string(),
			"core-architecture/pool.md".to_string(),
			"introduction/adapters.md".to_string(),
		];

		// Relative link: introduction/credit-accounts.md → ../core-architecture/credit-suite.md
		let resolved = md.resolve_import(
			"../core-architecture/credit-suite.md",
			"introduction/credit-accounts.md",
			&all_files,
		);
		assert_eq!(
			resolved,
			Some("core-architecture/credit-suite.md".to_string())
		);

		// Same-directory link
		let resolved = md.resolve_import(
			"./adapters.md",
			"introduction/credit-accounts.md",
			&all_files,
		);
		assert_eq!(resolved, Some("introduction/adapters.md".to_string()));

		// Non-existent target
		let resolved = md.resolve_import(
			"../nonexistent.md",
			"introduction/credit-accounts.md",
			&all_files,
		);
		assert_eq!(resolved, None);
	}

	#[test]
	fn test_resolve_with_deep_project_paths() {
		let md = Markdown;
		// Real-world paths from ai-assistant repo
		let all_files = vec![
			"projects/gearbox/autodocs-about/docs/core-architecture/credit-suite.md".to_string(),
			"projects/gearbox/autodocs-about/docs/core-architecture/pool.md".to_string(),
			"projects/gearbox/autodocs-about/docs/introduction/credit-accounts.md".to_string(),
		];

		// adapters-integrations.md links to credit-suite.md (same dir)
		let resolved = md.resolve_import(
			"credit-suite.md",
			"projects/gearbox/autodocs-about/docs/core-architecture/adapters-integrations.md",
			&all_files,
		);
		assert_eq!(
			resolved,
			Some(
				"projects/gearbox/autodocs-about/docs/core-architecture/credit-suite.md"
					.to_string()
			),
			"Same-dir link should resolve"
		);

		// adapters-integrations.md links to ../introduction/credit-accounts.md
		let resolved = md.resolve_import(
			"../introduction/credit-accounts.md",
			"projects/gearbox/autodocs-about/docs/core-architecture/adapters-integrations.md",
			&all_files,
		);
		assert_eq!(
			resolved,
			Some(
				"projects/gearbox/autodocs-about/docs/introduction/credit-accounts.md".to_string()
			),
			"Parent-dir link should resolve"
		);
	}

	#[test]
	fn test_no_links_in_empty_doc() {
		let content = "# Simple heading\nNo links here.";
		let links = extract_markdown_links(content);
		assert!(links.is_empty());
	}
}
