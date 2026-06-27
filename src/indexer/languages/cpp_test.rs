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
}
