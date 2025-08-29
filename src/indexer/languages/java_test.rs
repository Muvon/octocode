#[cfg(test)]
mod java_tests {
	use crate::indexer::{code_region_extractor, languages};
	use tree_sitter::Parser;

	#[test]
	fn test_java_region_extraction() {
		let contents = r#"package com.example.test;

import java.util.List;
import java.util.ArrayList;

public class SimpleTest {
    private String name;

    public SimpleTest(String name) {
        this.name = name;
    }

    public String getName() {
        return name;
    }
}
"#;

		// Get Java language implementation
		let lang_impl = languages::get_language("java").unwrap();

		// Set up parser
		let mut parser = Parser::new();
		parser.set_language(&lang_impl.get_ts_language()).unwrap();

		// Parse the file
		let tree = parser.parse(contents, None).unwrap();

		// Extract regions
		let mut regions = Vec::new();
		code_region_extractor::extract_meaningful_regions(
			tree.root_node(),
			contents,
			lang_impl.as_ref(),
			&mut regions,
		);

		println!("Extracted {} regions:", regions.len());
		for (i, region) in regions.iter().enumerate() {
			println!("\n--- Region {} ---", i + 1);
			println!("Kind: {}", region.node_kind);
			println!("Lines: {}-{}", region.start_line, region.end_line);
			println!("Symbols: {:?}", region.symbols);
			println!("Content preview:");
			let preview = if region.content.len() > 200 {
				format!("{}...", &region.content[..200])
			} else {
				region.content.clone()
			};
			println!("{}", preview);
		}

		// Verify we have the expected regions
		assert!(!regions.is_empty(), "Should extract some regions");

		// Check that we have class declaration
		let class_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "class_declaration")
			.collect();
		assert!(!class_regions.is_empty(), "Should have class declaration");

		// Check that we have method declarations
		let method_regions: Vec<_> = regions
			.iter()
			.filter(|r| {
				r.node_kind == "method_declaration" || r.node_kind == "constructor_declaration"
			})
			.collect();
		assert!(
			!method_regions.is_empty(),
			"Should have method/constructor declarations"
		);
	}
}
