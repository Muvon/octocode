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
mod go_tests {
	use crate::indexer::code_region_extractor::{extract_meaningful_regions, CodeRegion};
	use crate::indexer::languages;
	use tree_sitter::Parser;

	fn parse_regions(source: &str) -> Vec<CodeRegion> {
		let lang = languages::get_language("go").expect("Go language not registered");
		let mut parser = Parser::new();
		parser.set_language(&lang.get_ts_language()).unwrap();
		let tree = parser.parse(source, None).unwrap();
		let mut regions = Vec::new();
		extract_meaningful_regions(tree.root_node(), source, lang.as_ref(), &mut regions);
		regions
	}

	#[test]
	fn test_grouped_const_does_not_collapse_into_one_declaration_blob() {
		let source = r#"package main

const (
	A = 1
	B = 2
)
"#;
		let regions = parse_regions(source);
		let const_decl_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "const_declaration")
			.collect();
		assert_eq!(
			const_decl_regions.len(),
			0,
			"grouped const block should never surface as a single const_declaration region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		let const_spec_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "const_spec")
			.collect();
		assert!(
			!const_spec_regions.is_empty(),
			"grouped const block should surface as const_spec region(s), got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}

	#[test]
	fn test_large_grouped_const_splits_into_multiple_bounded_regions() {
		let mut source = String::from("package main\n\nconst (\n");
		for i in 0..40 {
			source.push_str(&format!("\tC{i} = {i}\n"));
		}
		source.push_str(")\n");

		let regions = parse_regions(&source);
		let const_spec_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "const_spec")
			.collect();
		assert!(
			const_spec_regions.len() > 1,
			"a 40-spec grouped const block must not become a single oversized region, got {} const_spec region(s)",
			const_spec_regions.len()
		);
	}

	#[test]
	fn test_single_const_stays_one_region_with_keyword() {
		let source = r#"package main

const X = 1
"#;
		let regions = parse_regions(source);
		let const_decl_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "const_declaration")
			.collect();
		assert_eq!(
			const_decl_regions.len(),
			1,
			"single const declaration should stay as one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
		assert!(
			const_decl_regions[0].content.contains("const"),
			"single const region should still include the 'const' keyword: {:?}",
			const_decl_regions[0].content
		);
	}

	#[test]
	fn test_grouped_var_does_not_collapse_into_one_declaration_blob() {
		let source = r#"package main

var (
	A = 1
	B = 2
)
"#;
		let regions = parse_regions(source);
		let var_decl_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "var_declaration")
			.collect();
		assert_eq!(
			var_decl_regions.len(),
			0,
			"grouped var block should never surface as a single var_declaration region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		let var_spec_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "var_spec")
			.collect();
		assert!(
			!var_spec_regions.is_empty(),
			"grouped var block should surface as var_spec region(s), got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}

	#[test]
	fn test_single_var_stays_one_region_with_keyword() {
		let source = r#"package main

var X = 1
"#;
		let regions = parse_regions(source);
		let var_decl_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "var_declaration")
			.collect();
		assert_eq!(
			var_decl_regions.len(),
			1,
			"single var declaration should stay as one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
		assert!(
			var_decl_regions[0].content.contains("var"),
			"single var region should still include the 'var' keyword: {:?}",
			var_decl_regions[0].content
		);
	}

	#[test]
	fn test_grouped_type_splits_per_spec() {
		// Non-trivial content so the smart single-line merge pass doesn't recombine them.
		let source = r#"package main

type (
	A struct {
		f int
		g string
		h bool
		k float64
	}
	B interface {
		M()
		N()
		O()
		P()
	}
)
"#;
		let regions = parse_regions(source);
		let type_decl_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "type_declaration")
			.collect();
		assert_eq!(
			type_decl_regions.len(),
			0,
			"grouped type block should never surface as a single type_declaration region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		let type_spec_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "type_spec")
			.collect();
		assert_eq!(
			type_spec_regions.len(),
			2,
			"grouped type block should produce one region per spec, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}

	#[test]
	fn test_single_type_stays_one_region_with_keyword() {
		let source = r#"package main

type Foo struct {
	F int
}
"#;
		let regions = parse_regions(source);
		let type_decl_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "type_declaration")
			.collect();
		assert_eq!(
			type_decl_regions.len(),
			1,
			"single type declaration should stay as one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
		assert!(
			type_decl_regions[0].content.contains("type"),
			"single type region should still include the 'type' keyword: {:?}",
			type_decl_regions[0].content
		);
	}
}
