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

	/// Build a tar.zst archive from `layout`, a list of (relative path, contents).
	fn archive(dir: &std::path::Path, layout: &[(&str, &str)]) -> std::path::PathBuf {
		let staged = dir.join("staged");
		for (path, contents) in layout {
			let full = staged.join(path);
			std::fs::create_dir_all(full.parent().unwrap()).unwrap();
			std::fs::write(full, contents).unwrap();
		}

		let out = dir.join("export.tar.zst");
		let encoder = zstd::Encoder::new(std::fs::File::create(&out).unwrap(), 1).unwrap();
		let mut tar = tar::Builder::new(encoder.auto_finish());
		tar.append_dir_all(".", &staged).unwrap();
		tar.finish().unwrap();
		drop(tar);
		out
	}

	fn valid_layout() -> Vec<(&'static str, &'static str)> {
		vec![
			("octocode-export.marker", "octocode-export-v1\n"),
			("storage/code_blocks.lance/data", "rows"),
			("branches/feature/manifest.json", "{}"),
		]
	}

	#[test]
	fn a_valid_export_extracts_and_passes_validation() {
		let dir = TempDir::new().unwrap();
		let file = archive(dir.path(), &valid_layout());
		let dest = dir.path().join("dest");

		extract_and_validate(&file, &dest).expect("a well-formed export must validate");
		assert!(dest.join("storage/code_blocks.lance/data").is_file());
	}

	#[test]
	fn a_file_that_is_not_an_archive_is_rejected() {
		let dir = TempDir::new().unwrap();
		let bogus = dir.path().join("notes.txt");
		std::fs::write(&bogus, "plain text").unwrap();
		assert!(extract_and_validate(&bogus, &dir.path().join("dest")).is_err());
		assert!(
			extract_and_validate(&dir.path().join("missing.tar.zst"), &dir.path().join("d"))
				.is_err()
		);
	}

	#[test]
	fn an_archive_without_the_marker_is_rejected() {
		let dir = TempDir::new().unwrap();
		let file = archive(dir.path(), &[("storage/table", "rows")]);
		let err = extract_and_validate(&file, &dir.path().join("dest")).unwrap_err();
		assert!(err.to_string().contains("missing marker file"), "{err}");
	}

	#[test]
	fn a_marker_with_foreign_content_is_rejected() {
		let dir = TempDir::new().unwrap();
		let file = archive(
			dir.path(),
			&[
				("octocode-export.marker", "some other tool"),
				("storage/table", "rows"),
			],
		);
		let err = extract_and_validate(&file, &dir.path().join("dest")).unwrap_err();
		assert!(err.to_string().contains("marker content"), "{err}");
	}

	#[test]
	fn an_archive_without_a_storage_directory_is_rejected() {
		let dir = TempDir::new().unwrap();
		let file = archive(
			dir.path(),
			&[("octocode-export.marker", "octocode-export-v1\n")],
		);
		let err = extract_and_validate(&file, &dir.path().join("dest")).unwrap_err();
		assert!(err.to_string().contains("missing 'storage'"), "{err}");
	}

	#[test]
	fn a_swap_replaces_the_target_and_removes_the_backup() {
		let dir = TempDir::new().unwrap();
		let source = dir.path().join("source");
		let target = dir.path().join("target");
		let backup = dir.path().join("backup");
		std::fs::create_dir_all(&source).unwrap();
		std::fs::write(source.join("new"), "new").unwrap();
		std::fs::create_dir_all(&target).unwrap();
		std::fs::write(target.join("old"), "old").unwrap();

		swap_dir(&source, &target, &backup, true).unwrap();

		assert!(target.join("new").is_file());
		assert!(!target.join("old").exists());
		assert!(!backup.exists());
		assert!(!source.exists());
	}

	#[test]
	fn a_swap_onto_a_missing_target_still_installs_the_source() {
		let dir = TempDir::new().unwrap();
		let source = dir.path().join("source");
		std::fs::create_dir_all(&source).unwrap();
		std::fs::write(source.join("new"), "new").unwrap();

		swap_dir(
			&source,
			&dir.path().join("target"),
			&dir.path().join("backup"),
			true,
		)
		.unwrap();
		assert!(dir.path().join("target/new").is_file());
	}

	#[test]
	fn a_missing_source_is_an_error_only_when_required() {
		let dir = TempDir::new().unwrap();
		let missing = dir.path().join("nope");
		let target = dir.path().join("target");
		let backup = dir.path().join("backup");

		assert!(swap_dir(&missing, &target, &backup, true).is_err());
		swap_dir(&missing, &target, &backup, false).expect("optional sources are skipped");
		assert!(!target.exists());
	}

	#[test]
	fn installing_moves_storage_and_branches_into_the_project() {
		let dir = TempDir::new().unwrap();
		let temp = dir.path().join("extracted");
		std::fs::create_dir_all(temp.join("storage")).unwrap();
		std::fs::write(temp.join("storage/table"), "rows").unwrap();
		std::fs::create_dir_all(temp.join("branches/feature")).unwrap();
		std::fs::write(temp.join("branches/feature/manifest.json"), "{}").unwrap();

		let project = dir.path().join("project");
		std::fs::create_dir_all(&project).unwrap();

		install_atomic(&temp, &project, 4242).unwrap();
		assert!(project.join("storage/table").is_file());
		assert!(project.join("branches/feature/manifest.json").is_file());
		assert!(
			std::fs::read_dir(&project)
				.unwrap()
				.filter_map(Result::ok)
				.all(|e| !e
					.file_name()
					.to_string_lossy()
					.starts_with(".octocode-backup")),
			"backups must be cleaned up on success"
		);
	}

	#[test]
	fn installing_without_a_storage_directory_fails() {
		let dir = TempDir::new().unwrap();
		let temp = dir.path().join("extracted");
		std::fs::create_dir_all(&temp).unwrap();
		let project = dir.path().join("project");
		std::fs::create_dir_all(&project).unwrap();

		assert!(install_atomic(&temp, &project, 1).is_err());
	}
}
