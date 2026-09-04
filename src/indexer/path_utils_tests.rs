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
	use crate::indexer::path_utils::PathUtils;
	use std::path::Path;

	#[test]
	fn strips_the_working_directory_prefix() {
		let out = PathUtils::to_relative_string(Path::new("/repo/src/main.rs"), Path::new("/repo"));
		assert_eq!(out, "src/main.rs");
	}

	#[test]
	fn unrelated_path_is_returned_verbatim() {
		let out = PathUtils::to_relative_string(Path::new("/other/lib.rs"), Path::new("/repo"));
		assert_eq!(out, "/other/lib.rs");
	}

	#[test]
	fn display_never_leaks_an_absolute_path() {
		let out = PathUtils::for_display(Path::new("/other/secret/lib.rs"), Path::new("/repo"));
		assert_eq!(out, "lib.rs");
	}

	#[test]
	fn display_keeps_relative_paths_intact() {
		let out = PathUtils::for_display(Path::new("/repo/src/a/b.rs"), Path::new("/repo"));
		assert_eq!(out, "src/a/b.rs");
	}

	#[test]
	fn display_falls_back_to_unknown_without_a_file_name() {
		let out = PathUtils::for_display(Path::new("/"), Path::new("/repo"));
		assert_eq!(out, "unknown");
	}
}
