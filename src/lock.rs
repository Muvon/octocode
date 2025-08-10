use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::thread;
use std::time::Duration;
use tracing::{debug, warn};

use crate::storage::get_project_storage_path;

/// Simple file-based lock for atomic indexing operations
pub struct IndexLock {
	lock_file: PathBuf,
	acquired: bool,
}

impl IndexLock {
	/// Create a new lock instance for the given project directory
	pub fn new(project_dir: &Path) -> io::Result<Self> {
		// Use project-specific storage directory for lock file
		let project_storage = get_project_storage_path(project_dir).map_err(io::Error::other)?;

		// Lock file goes inside the project's storage folder
		let lock_file = project_storage.join("index.lock");

		Ok(Self {
			lock_file,
			acquired: false,
		})
	}

	/// Acquire the lock, waiting if necessary
	/// This will block until the lock is available
	pub fn acquire(&mut self) -> io::Result<()> {
		// Ensure .octocode directory exists
		if let Some(parent) = self.lock_file.parent() {
			fs::create_dir_all(parent)?;
		}

		let my_pid = process::id();

		loop {
			// Try to read existing lock file
			match fs::read_to_string(&self.lock_file) {
				Ok(contents) => {
					// Lock file exists, check if process is still alive
					if let Ok(pid) = contents.trim().parse::<u32>() {
						if pid == my_pid {
							// We already own the lock
							self.acquired = true;
							return Ok(());
						}

						if Self::is_process_alive(pid) {
							// Process is still alive, wait and retry
							debug!("Waiting for indexing lock held by PID {}", pid);
							thread::sleep(Duration::from_millis(500));
							continue;
						} else {
							// Process is dead, clean up stale lock
							debug!("Cleaning stale lock from dead PID {}", pid);
							let _ = fs::remove_file(&self.lock_file);
						}
					} else {
						// Invalid lock file content, remove it
						warn!("Invalid lock file content, removing");
						let _ = fs::remove_file(&self.lock_file);
					}
				}
				Err(e) if e.kind() == io::ErrorKind::NotFound => {
					// Lock file doesn't exist, we can proceed
				}
				Err(e) => {
					// Some other error reading the lock file
					return Err(e);
				}
			}

			// Try to create the lock file atomically
			match fs::OpenOptions::new()
				.write(true)
				.create_new(true)
				.open(&self.lock_file)
			{
				Ok(mut file) => {
					// Successfully created the lock file
					file.write_all(my_pid.to_string().as_bytes())?;
					file.sync_all()?;
					self.acquired = true;
					debug!("Acquired indexing lock (PID {})", my_pid);
					return Ok(());
				}
				Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
					// Someone else created it between our check and create
					// Loop will retry
					thread::sleep(Duration::from_millis(100));
				}
				Err(e) => {
					return Err(e);
				}
			}
		}
	}

	/// Release the lock if we own it
	pub fn release(&mut self) -> io::Result<()> {
		if self.acquired {
			match fs::remove_file(&self.lock_file) {
				Ok(_) => {
					self.acquired = false;
					debug!("Released indexing lock");
					Ok(())
				}
				Err(e) if e.kind() == io::ErrorKind::NotFound => {
					// Already removed
					self.acquired = false;
					Ok(())
				}
				Err(e) => Err(e),
			}
		} else {
			Ok(())
		}
	}

	/// Check if a process with the given PID is still alive
	#[cfg(unix)]
	fn is_process_alive(pid: u32) -> bool {
		// On Unix, check if /proc/{pid} exists (Linux) or use kill(0) signal
		// Try kill with signal 0 - it doesn't actually send a signal, just checks if process exists
		use std::process::Command;

		Command::new("kill")
			.args(["-0", &pid.to_string()])
			.output()
			.map(|output| output.status.success())
			.unwrap_or(false)
	}

	#[cfg(windows)]
	fn is_process_alive(pid: u32) -> bool {
		// On Windows, try to open the process with query access
		use std::process::Command;

		// Use tasklist command to check if process exists
		Command::new("tasklist")
			.args(&["/FI", &format!("PID eq {}", pid)])
			.output()
			.map(|output| {
				let output_str = String::from_utf8_lossy(&output.stdout);
				output_str.contains(&pid.to_string())
			})
			.unwrap_or(false)
	}

	#[cfg(not(any(unix, windows)))]
	fn is_process_alive(_pid: u32) -> bool {
		// On other platforms, assume the process is alive
		// This is conservative but safe
		true
	}
}

impl Drop for IndexLock {
	fn drop(&mut self) {
		// Automatically release the lock when dropped
		let _ = self.release();
	}
}

/// Convenience function to run a closure with the lock held
pub fn with_index_lock<F, R>(project_dir: &Path, f: F) -> io::Result<R>
where
	F: FnOnce() -> R,
{
	let mut lock = IndexLock::new(project_dir)?;
	lock.acquire()?;
	let result = f();
	lock.release()?;
	Ok(result)
}
