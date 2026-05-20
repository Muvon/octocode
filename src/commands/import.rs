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

use anyhow::{bail, Context, Result};
use clap::Args;
use std::path::{Path, PathBuf};

use octocode::lock::IndexLock;
use octocode::storage::ensure_project_storage_exists;

const MARKER_FILE: &str = "octocode-export.marker";
const MARKER_PREFIX: &str = "octocode-export-v";

#[derive(Args, Debug)]
pub struct ImportArgs {
	/// Archive file produced by `octocode export` (.tar.zst).
	pub file: PathBuf,
}

pub async fn execute(args: &ImportArgs) -> Result<()> {
	let current_dir = std::env::current_dir()?;

	if !args.file.exists() {
		bail!("Import file not found: {}", args.file.display());
	}
	if !args.file.is_file() {
		bail!("Import path is not a file: {}", args.file.display());
	}

	// Make sure the project storage directory exists so we have a parent
	// directory to atomically rename into.
	let project_storage = ensure_project_storage_exists(&current_dir)?;

	// Extract to a temp dir that sits next to the final target so the final
	// rename stays on the same filesystem (and therefore atomic).
	let pid = std::process::id();
	let temp_dir = project_storage.join(format!(".octocode-import-{}", pid));
	if temp_dir.exists() {
		let _ = std::fs::remove_dir_all(&temp_dir);
	}
	std::fs::create_dir_all(&temp_dir)
		.with_context(|| format!("Failed to create temp dir {}", temp_dir.display()))?;

	// Acquire lock so no concurrent indexer/watcher mutates the DB while we
	// validate and swap directories. The lock file lives inside
	// project_storage; the rename steps below leave it in place.
	println!("Acquiring index lock...");
	let mut lock = IndexLock::new(&current_dir)?;
	lock.acquire_async().await?;

	// Run the IO-heavy work on the blocking pool.
	let file = args.file.clone();
	let temp_clone = temp_dir.clone();
	let extract_res =
		tokio::task::spawn_blocking(move || extract_and_validate(&file, &temp_clone)).await?;

	if let Err(e) = extract_res {
		let _ = std::fs::remove_dir_all(&temp_dir);
		lock.release()?;
		return Err(e);
	}

	// Atomically swap storage/ and (optionally) branches/ into place. On any
	// failure we restore the backed-up directory so the user is never left
	// with a half-installed dataset.
	let install_res = install_atomic(&temp_dir, &project_storage, pid);
	let _ = std::fs::remove_dir_all(&temp_dir);
	lock.release()?;
	install_res?;

	println!("Imported into {}", project_storage.display());
	Ok(())
}

/// Decompress + extract the archive into `dest`, then verify it is an octocode
/// export. Returns an error (and leaves `dest` populated for the caller to
/// clean up) when the archive is invalid.
fn extract_and_validate(file: &Path, dest: &Path) -> Result<()> {
	let f =
		std::fs::File::open(file).with_context(|| format!("Failed to open {}", file.display()))?;
	let decoder = zstd::Decoder::new(f).context("Not a valid zstd-compressed archive")?;
	let mut archive = tar::Archive::new(decoder);
	archive
		.unpack(dest)
		.context("Failed to extract tar.zst archive")?;

	// Validate the marker file. This rules out arbitrary tar.zst files being
	// passed in by accident.
	let marker = dest.join(MARKER_FILE);
	if !marker.exists() {
		bail!(
			"Invalid octocode export: missing marker file '{}'",
			MARKER_FILE
		);
	}
	let content = std::fs::read_to_string(&marker)
		.with_context(|| format!("Failed to read marker {}", marker.display()))?;
	if !content.trim_start().starts_with(MARKER_PREFIX) {
		bail!(
			"Invalid octocode export: marker content does not start with '{}'",
			MARKER_PREFIX
		);
	}

	// Must contain the lance storage directory; branches/ is optional.
	if !dest.join("storage").is_dir() {
		bail!("Invalid octocode export: missing 'storage' directory in archive");
	}

	Ok(())
}

/// Move extracted `storage/` and `branches/` into `project_storage`, taking
/// backups of the existing entries first so we can roll back on failure.
fn install_atomic(temp_dir: &Path, project_storage: &Path, pid: u32) -> Result<()> {
	swap_dir(
		&temp_dir.join("storage"),
		&project_storage.join("storage"),
		&project_storage.join(format!(".octocode-backup-storage-{}", pid)),
		true,
	)?;

	let extracted_branches = temp_dir.join("branches");
	if extracted_branches.exists() {
		swap_dir(
			&extracted_branches,
			&project_storage.join("branches"),
			&project_storage.join(format!(".octocode-backup-branches-{}", pid)),
			false,
		)?;
	}

	Ok(())
}

/// Atomically replace `target` with `source`:
///   1. If `target` exists, rename it to `backup`.
///   2. Rename `source` to `target`.
///   3. Delete `backup`.
///
/// If step 2 fails we restore `backup` to `target` so the project is left in
/// its prior state. Caller guarantees `source`, `target`, `backup` are all on
/// the same filesystem so each rename is atomic.
///
/// When `required` is true and `source` does not exist, this returns an error.
fn swap_dir(source: &Path, target: &Path, backup: &Path, required: bool) -> Result<()> {
	if !source.exists() {
		if required {
			bail!(
				"Missing required directory in archive: {}",
				source.display()
			);
		}
		return Ok(());
	}

	let had_target = target.exists();
	if had_target {
		std::fs::rename(target, backup).with_context(|| {
			format!(
				"Failed to back up existing {} to {}",
				target.display(),
				backup.display()
			)
		})?;
	}

	if let Err(e) = std::fs::rename(source, target) {
		// Restore prior state before bubbling up.
		if had_target {
			let _ = std::fs::rename(backup, target);
		}
		return Err(anyhow::anyhow!(
			"Failed to install {} into {}: {}",
			source.display(),
			target.display(),
			e
		));
	}

	if had_target {
		let _ = std::fs::remove_dir_all(backup);
	}
	Ok(())
}
