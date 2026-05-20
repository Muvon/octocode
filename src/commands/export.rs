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
use chrono::Utc;
use clap::Args;
use std::path::{Path, PathBuf};

use octocode::lock::IndexLock;
use octocode::storage::{get_project_identifier, get_project_storage_path};

/// Magic marker written at the archive root so `import` can validate the file
/// was produced by `octocode export` (and not some random tar.zst).
const MARKER_FILE: &str = "octocode-export.marker";
const MARKER_CONTENT: &str = "octocode-export-v1\n";

#[derive(Args, Debug)]
pub struct ExportArgs {
	/// Destination directory for the archive file.
	/// Defaults to the current directory. The full archive path is printed on success.
	pub dest: Option<PathBuf>,
}

pub async fn execute(args: &ExportArgs) -> Result<()> {
	let current_dir = std::env::current_dir()?;

	// Refuse to export when there is no project storage for this directory.
	let project_storage = get_project_storage_path(&current_dir)?;
	let lance_storage = project_storage.join("storage");
	if !lance_storage.exists() {
		bail!(
			"No octocode database found for this directory.\nExpected at: {}",
			lance_storage.display()
		);
	}

	// Resolve destination (default: current dir). Create it if missing.
	let dest_dir = args.dest.clone().unwrap_or_else(|| current_dir.clone());
	if !dest_dir.exists() {
		std::fs::create_dir_all(&dest_dir)
			.with_context(|| format!("Failed to create destination {}", dest_dir.display()))?;
	}
	if !dest_dir.is_dir() {
		bail!("Destination is not a directory: {}", dest_dir.display());
	}
	let dest_dir = dest_dir.canonicalize().unwrap_or_else(|_| dest_dir.clone());

	let project_id = get_project_identifier(&current_dir)?;
	let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
	let filename = format!("octocode-{}-{}.tar.zst", project_id, timestamp);
	let out_path = dest_dir.join(&filename);

	// Hold the index lock for the duration of the snapshot. This is the same
	// lock the indexer/watcher/MCP indexer take, so an ongoing index run will
	// either complete before we start reading, or wait until we release —
	// guaranteeing the LanceDB files are not mutated mid-snapshot.
	println!("Acquiring index lock...");
	let mut lock = IndexLock::new(&current_dir)?;
	lock.acquire_async().await?;

	println!("Creating snapshot from {}", project_storage.display());

	let src_root = project_storage.clone();
	let out_clone = out_path.clone();
	let bytes = tokio::task::spawn_blocking(move || write_archive(&src_root, &out_clone)).await??;

	lock.release()?;

	println!("Exported {:.2} MB", bytes as f64 / (1024.0 * 1024.0));
	println!("{}", out_path.display());
	Ok(())
}

/// Write `storage/` and `branches/` (when present) from `src_root` into a
/// tar.zst at `out_path`. Returns the on-disk size in bytes.
///
/// Index lock files, log files and other transient state are excluded so the
/// archive only carries the data the importer needs.
fn write_archive(src_root: &Path, out_path: &Path) -> Result<u64> {
	let file = std::fs::File::create(out_path)
		.with_context(|| format!("Failed to create {}", out_path.display()))?;
	// Level 3 is the zstd default — fast compression with good ratio for
	// binary LanceDB pages. Higher levels barely help on already-dense data
	// and make export noticeably slower.
	let mut encoder = zstd::Encoder::new(file, 3)?;
	let workers = std::thread::available_parallelism()
		.map(|n| n.get() as u32)
		.unwrap_or(1);
	encoder.multithread(workers)?;
	let mut tar = tar::Builder::new(encoder.auto_finish());

	// Marker file at root so `import` can validate provenance cheaply.
	let mut header = tar::Header::new_gnu();
	header.set_size(MARKER_CONTENT.len() as u64);
	header.set_mode(0o644);
	header.set_mtime(Utc::now().timestamp() as u64);
	header.set_cksum();
	tar.append_data(&mut header, MARKER_FILE, MARKER_CONTENT.as_bytes())?;

	let storage_dir = src_root.join("storage");
	if storage_dir.exists() {
		tar.append_dir_all("storage", &storage_dir)
			.with_context(|| format!("Failed to add {} to archive", storage_dir.display()))?;
	}
	let branches_dir = src_root.join("branches");
	if branches_dir.exists() {
		tar.append_dir_all("branches", &branches_dir)
			.with_context(|| format!("Failed to add {} to archive", branches_dir.display()))?;
	}

	tar.finish()?;
	drop(tar);

	let meta = std::fs::metadata(out_path)?;
	Ok(meta.len())
}
