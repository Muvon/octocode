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
mod cpp_tests {
	use crate::indexer::file_utils::FileUtils;
	use crate::indexer::languages::{self, resolution_utils};
	use std::path::Path;

	const CPP_MODULE_EXTENSIONS: [&str; 5] = ["cppm", "ixx", "mxx", "ccm", "cxxm"];

	#[test]
	fn test_cpp_module_extensions_are_registered_on_language() {
		let lang = languages::get_language("cpp").expect("C++ language should be registered");
		let extensions = lang.get_file_extensions();

		for extension in CPP_MODULE_EXTENSIONS {
			assert!(
				extensions.contains(&extension),
				"C++ language should support .{extension} module files"
			);
		}
	}

	#[test]
	fn test_cpp_module_extensions_detect_as_cpp() {
		for extension in CPP_MODULE_EXTENSIONS {
			let file_name = format!("math.{extension}");

			assert_eq!(
				FileUtils::detect_language(Path::new(&file_name)),
				Some("cpp"),
				"FileUtils should detect .{extension} files as C++"
			);
			assert_eq!(
				resolution_utils::detect_language_from_path(&file_name).as_deref(),
				Some("cpp"),
				"resolution_utils should detect .{extension} files as C++"
			);
		}
	}

	#[test]
	fn test_namespace_splits_into_individual_functions() {
		use crate::indexer::code_region_extractor::extract_meaningful_regions;
		use crate::indexer::languages::Language;
		use tree_sitter::Parser;

		// Non-trivial content so the smart single-line merge pass doesn't recombine them.
		let code = r#"
namespace app {
	void foo() {
		int x = 1;
		int y = 2;
		int z = x + y;
		return;
	}
	void bar() {
		int a = 10;
		int b = 20;
		int c = a * b;
		return;
	}
}
"#;
		let lang = languages::get_language("cpp").unwrap();
		let mut parser = Parser::new();
		parser.set_language(&lang.get_ts_language()).unwrap();
		let tree = parser.parse(code, None).unwrap();
		let mut regions = Vec::new();
		extract_meaningful_regions(tree.root_node(), code, lang.as_ref(), &mut regions);

		let namespace_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "namespace_definition")
			.collect();
		assert_eq!(
			namespace_regions.len(),
			0,
			"namespace with functions inside should not collapse into one region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);

		let function_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "function_definition")
			.collect();
		assert_eq!(
			function_regions.len(),
			2,
			"expected a region per function inside namespace"
		);
	}

	#[test]
	fn test_empty_namespace_falls_back_to_single_region() {
		use crate::indexer::code_region_extractor::extract_meaningful_regions;
		use crate::indexer::languages::Language;
		use tree_sitter::Parser;

		let code = r#"
namespace empty_ns {
	// nothing meaningful in here
}
"#;
		let lang = languages::get_language("cpp").unwrap();
		let mut parser = Parser::new();
		parser.set_language(&lang.get_ts_language()).unwrap();
		let tree = parser.parse(code, None).unwrap();
		let mut regions = Vec::new();
		extract_meaningful_regions(tree.root_node(), code, lang.as_ref(), &mut regions);

		let namespace_regions: Vec<_> = regions
			.iter()
			.filter(|r| r.node_kind == "namespace_definition")
			.collect();
		assert_eq!(
			namespace_regions.len(),
			1,
			"empty namespace should fall back to its own single region, got {:?}",
			regions.iter().map(|r| &r.node_kind).collect::<Vec<_>>()
		);
	}
}
