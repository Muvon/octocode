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

		// class with a constructor and a method inside should not collapse into
		// one region, got {:?}
		let class_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "class_declaration")
			.collect();
		assert_eq!(
			class_regions.len(),
			0,
			"class with constructor/method inside should not collapse into one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		// Check that we have method and constructor declarations split out
		let constructor_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "constructor_declaration")
			.collect();
		assert!(
			!constructor_regions.is_empty(),
			"Should have constructor declaration"
		);

		let method_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "method_declaration")
			.collect();
		assert!(!method_regions.is_empty(), "Should have method declaration");
	}

	#[test]
	fn test_record_with_method_splits_into_method_region() {
		let contents = r#"public record Point(int x, int y) {
    int sum() {
        return x + y;
    }
}
"#;
		let lang_impl = languages::get_language("java").unwrap();
		let mut parser = Parser::new();
		parser.set_language(&lang_impl.get_ts_language()).unwrap();
		let tree = parser.parse(contents, None).unwrap();
		let mut regions = Vec::new();
		code_region_extractor::extract_meaningful_regions(
			tree.root_node(),
			contents,
			lang_impl.as_ref(),
			&mut regions,
		);

		let record_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "record_declaration")
			.collect();
		assert_eq!(
			record_regions.len(),
			0,
			"record with an explicit method should not collapse into one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		let method_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "method_declaration")
			.collect();
		assert_eq!(
			method_regions.len(),
			1,
			"expected a region for the record's explicit method"
		);
	}

	#[test]
	fn test_plain_data_record_stays_single_region() {
		let contents = r#"public record Point(int x, int y) {}
"#;
		let lang_impl = languages::get_language("java").unwrap();
		let mut parser = Parser::new();
		parser.set_language(&lang_impl.get_ts_language()).unwrap();
		let tree = parser.parse(contents, None).unwrap();
		let mut regions = Vec::new();
		code_region_extractor::extract_meaningful_regions(
			tree.root_node(),
			contents,
			lang_impl.as_ref(),
			&mut regions,
		);

		let record_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "record_declaration")
			.collect();
		assert_eq!(
			record_regions.len(),
			1,
			"plain data record with no explicit methods should remain its own single region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}
}
