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
	use tempfile::TempDir;

	/// A project-storage layout with a lance `storage/` dir and a branch delta.
	fn project_storage() -> TempDir {
		let dir = TempDir::new().unwrap();
		std::fs::create_dir_all(dir.path().join("storage/code_blocks.lance")).unwrap();
		std::fs::write(dir.path().join("storage/code_blocks.lance/data"), "rows").unwrap();
		std::fs::create_dir_all(dir.path().join("branches/feature")).unwrap();
		std::fs::write(
			dir.path().join("branches/feature/manifest.json"),
			"{\"version\":2}",
		)
		.unwrap();
		dir
	}

	#[test]
	fn an_archive_contains_the_marker_storage_and_branches() {
		let src = project_storage();
		let out = TempDir::new().unwrap();
		let archive = out.path().join("export.tar.zst");

		let size = write_archive(src.path(), &archive).unwrap();
		assert!(size > 0);
		assert_eq!(size, std::fs::metadata(&archive).unwrap().len());

		let extracted = TempDir::new().unwrap();
		let decoder = zstd::Decoder::new(std::fs::File::open(&archive).unwrap()).unwrap();
		tar::Archive::new(decoder).unpack(extracted.path()).unwrap();

		assert!(extracted.path().join(MARKER_FILE).is_file());
		assert!(extracted
			.path()
			.join("storage/code_blocks.lance/data")
			.is_file());
		assert!(extracted
			.path()
			.join("branches/feature/manifest.json")
			.is_file());
	}

	#[test]
	fn a_project_without_branches_still_archives_cleanly() {
		let src = TempDir::new().unwrap();
		std::fs::create_dir_all(src.path().join("storage")).unwrap();
		std::fs::write(src.path().join("storage/table"), "x").unwrap();

		let out = TempDir::new().unwrap();
		let archive = out.path().join("export.tar.zst");
		assert!(write_archive(src.path(), &archive).unwrap() > 0);

		let extracted = TempDir::new().unwrap();
		let decoder = zstd::Decoder::new(std::fs::File::open(&archive).unwrap()).unwrap();
		tar::Archive::new(decoder).unpack(extracted.path()).unwrap();
		assert!(extracted.path().join("storage/table").is_file());
		assert!(!extracted.path().join("branches").exists());
	}

	#[test]
	fn an_unwritable_destination_is_an_error() {
		let src = project_storage();
		assert!(
			write_archive(src.path(), std::path::Path::new("/no/such/dir/out.tar.zst")).is_err()
		);
	}
}
