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
mod css_tests {
	use crate::indexer::code_region_extractor::{extract_meaningful_regions, CodeRegion};
	use crate::indexer::languages;
	use tree_sitter::Parser;

	fn parse_regions(source: &str) -> Vec<CodeRegion> {
		let lang = languages::get_language("css").expect("CSS language not registered");
		let mut parser = Parser::new();
		parser.set_language(&lang.get_ts_language()).unwrap();
		let tree = parser.parse(source, None).unwrap();
		let mut regions = Vec::new();
		extract_meaningful_regions(tree.root_node(), source, lang.as_ref(), &mut regions);
		regions
	}

	#[test]
	fn test_media_block_splits_into_individual_rules() {
		// Non-trivial content so the smart single-line merge pass doesn't recombine them.
		let source = r#"@media (max-width: 768px) {
	.a {
		color: red;
		background: white;
		font-size: 14px;
		margin: 0;
	}
	.b {
		color: blue;
		background: black;
		font-size: 16px;
		padding: 0;
	}
}
"#;
		let regions = parse_regions(source);

		let media_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "media_statement")
			.collect();
		assert_eq!(
			media_regions.len(),
			0,
			"media block with rules inside should not collapse into one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		let rule_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "rule_set")
			.collect();
		assert_eq!(
			rule_regions.len(),
			2,
			"expected a region per rule inside @media, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}

	#[test]
	fn test_font_face_stays_single_region() {
		let source = r#"@font-face {
	font-family: "X";
	src: url(x.woff);
}
"#;
		let regions = parse_regions(source);

		let at_rule_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "at_rule")
			.collect();
		assert_eq!(
			at_rule_regions.len(),
			1,
			"leaf at-rule with only declarations should remain its own single region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}
}
