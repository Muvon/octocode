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
	use crate::lock::{with_index_lock, IndexLock};
	use crate::storage::get_project_storage_path;
	use std::fs;
	use std::path::PathBuf;
	use tempfile::TempDir;

	/// A lock lives under the shared per-project storage root keyed by the
	/// project path, so each test needs its own project directory to stay
	/// independent of the others running in parallel.
	fn project() -> (TempDir, PathBuf) {
		let dir = TempDir::new().expect("tempdir");
		let lock_file = get_project_storage_path(dir.path())
			.expect("storage path")
			.join("index.lock");
		(dir, lock_file)
	}

	#[test]
	fn acquiring_writes_our_pid_and_releasing_removes_the_file() {
		let (dir, lock_file) = project();
		let mut lock = IndexLock::new(dir.path()).unwrap();

		lock.acquire().unwrap();
		assert_eq!(
			fs::read_to_string(&lock_file).unwrap().trim(),
			std::process::id().to_string()
		);

		lock.release().unwrap();
		assert!(!lock_file.exists());
	}

	#[test]
	fn releasing_twice_is_not_an_error() {
		let (dir, _) = project();
		let mut lock = IndexLock::new(dir.path()).unwrap();
		lock.acquire().unwrap();
		lock.release().unwrap();
		lock.release().unwrap();
	}

	#[test]
	fn releasing_a_lock_we_never_took_is_a_no_op() {
		let (dir, lock_file) = project();
		fs::create_dir_all(lock_file.parent().unwrap()).unwrap();
		fs::write(&lock_file, "999999").unwrap();

		let mut lock = IndexLock::new(dir.path()).unwrap();
		lock.release().unwrap();
		// Someone else's lock file must survive.
		assert!(lock_file.exists());
		fs::remove_file(&lock_file).unwrap();
	}

	#[test]
	fn reacquiring_our_own_lock_succeeds_immediately() {
		let (dir, _) = project();
		let mut lock = IndexLock::new(dir.path()).unwrap();
		lock.acquire().unwrap();
		// Second acquire sees our own PID and returns instead of spinning.
		lock.acquire().unwrap();
		lock.release().unwrap();
	}

	#[test]
	fn a_stale_lock_from_a_dead_pid_is_cleaned_up() {
		let (dir, lock_file) = project();
		fs::create_dir_all(lock_file.parent().unwrap()).unwrap();
		// PIDs above the kernel maximum can never name a live process.
		fs::write(&lock_file, "4194303").unwrap();

		let mut lock = IndexLock::new(dir.path()).unwrap();
		lock.acquire().unwrap();
		assert_eq!(
			fs::read_to_string(&lock_file).unwrap().trim(),
			std::process::id().to_string()
		);
		lock.release().unwrap();
	}

	#[test]
	fn a_corrupt_lock_file_is_discarded() {
		let (dir, lock_file) = project();
		fs::create_dir_all(lock_file.parent().unwrap()).unwrap();
		fs::write(&lock_file, "not-a-pid").unwrap();

		let mut lock = IndexLock::new(dir.path()).unwrap();
		lock.acquire().unwrap();
		assert_eq!(
			fs::read_to_string(&lock_file).unwrap().trim(),
			std::process::id().to_string()
		);
		lock.release().unwrap();
	}

	#[test]
	fn dropping_the_lock_releases_it() {
		let (dir, lock_file) = project();
		{
			let mut lock = IndexLock::new(dir.path()).unwrap();
			lock.acquire().unwrap();
			assert!(lock_file.exists());
		}
		assert!(!lock_file.exists());
	}

	#[tokio::test]
	async fn the_async_path_acquires_the_same_lock_file() {
		let (dir, lock_file) = project();
		let mut lock = IndexLock::new(dir.path()).unwrap();

		lock.acquire_async().await.unwrap();
		assert_eq!(
			fs::read_to_string(&lock_file).unwrap().trim(),
			std::process::id().to_string()
		);
		// Re-entering with our own PID must not block the runtime.
		lock.acquire_async().await.unwrap();
		lock.release().unwrap();
	}

	#[tokio::test]
	async fn the_async_path_also_clears_a_stale_lock() {
		let (dir, lock_file) = project();
		fs::create_dir_all(lock_file.parent().unwrap()).unwrap();
		fs::write(&lock_file, "4194303").unwrap();

		let mut lock = IndexLock::new(dir.path()).unwrap();
		lock.acquire_async().await.unwrap();
		lock.release().unwrap();
		assert!(!lock_file.exists());
	}

	#[test]
	fn the_convenience_wrapper_holds_the_lock_for_the_closure_only() {
		let (dir, lock_file) = project();
		let observed = with_index_lock(dir.path(), || lock_file.exists()).unwrap();
		assert!(observed, "the closure must run while the lock is held");
		assert!(!lock_file.exists(), "the lock must be released afterwards");
	}
}
