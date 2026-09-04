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
	use std::collections::HashSet;
	use std::path::Path;
	use tempfile::TempDir;

	/// A workspace with source, text and ignored files. The `.git` directory is
	/// real because `ignore::WalkBuilder` only applies `.gitignore` rules inside
	/// an actual repository.
	fn workspace() -> TempDir {
		let dir = TempDir::new().unwrap();
		std::fs::create_dir_all(dir.path().join(".git")).unwrap();
		std::fs::create_dir_all(dir.path().join("src")).unwrap();
		std::fs::create_dir_all(dir.path().join("target")).unwrap();
		std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
		std::fs::write(dir.path().join("src/lib.py"), "def run():\n    pass\n").unwrap();
		std::fs::write(dir.path().join("README.md"), "# Title\n").unwrap();
		std::fs::write(dir.path().join("target/build.rs"), "fn main() {}\n").unwrap();
		std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
		dir
	}

	fn walked_paths(dir: &Path) -> HashSet<String> {
		NoindexWalker::create_walker(dir)
			.build()
			.filter_map(Result::ok)
			.filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
			.map(|e| {
				e.path()
					.strip_prefix(dir)
					.unwrap_or(e.path())
					.to_string_lossy()
					.to_string()
			})
			.collect()
	}

	#[test]
	fn the_walker_respects_gitignore_and_skips_git_internals() {
		let dir = workspace();
		std::fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
		std::fs::write(dir.path().join(".git/objects/blob"), "x").unwrap();

		let paths = walked_paths(dir.path());
		assert!(paths.contains("src/main.rs"));
		assert!(paths.contains("README.md"));
		assert!(!paths.contains("target/build.rs"), "gitignore was ignored");
		assert!(
			!paths.iter().any(|p| p.starts_with(".git/")),
			"walk descended into .git: {paths:?}"
		);
	}

	#[test]
	fn a_root_noindex_file_is_honoured() {
		let dir = TempDir::new().unwrap();
		std::fs::create_dir_all(dir.path().join("src")).unwrap();
		std::fs::write(dir.path().join("src/keep.rs"), "fn keep() {}\n").unwrap();
		std::fs::write(dir.path().join("src/skip.rs"), "fn skip() {}\n").unwrap();
		std::fs::write(dir.path().join(".noindex"), "skip.rs\n").unwrap();

		let paths = walked_paths(dir.path());
		assert!(paths.contains("src/keep.rs"));
		assert!(!paths.contains("src/skip.rs"), "got {paths:?}");
	}

	#[test]
	fn the_matcher_loads_gitignore_and_noindex_rules() {
		let dir = workspace();
		std::fs::write(dir.path().join(".noindex"), "secret.txt\n").unwrap();

		let matcher = NoindexWalker::create_matcher(dir.path(), true).unwrap();
		// "target/" names a directory, so a file inside it only matches when its
		// parents are considered.
		assert!(matcher
			.matched_path_or_any_parents(dir.path().join("target/build.rs"), false)
			.is_ignore());
		assert!(matcher
			.matched(dir.path().join("secret.txt"), false)
			.is_ignore());
		assert!(!matcher
			.matched_path_or_any_parents(dir.path().join("src/main.rs"), false)
			.is_ignore());
	}

	#[test]
	fn a_matcher_for_a_bare_directory_ignores_nothing() {
		let dir = TempDir::new().unwrap();
		let matcher = NoindexWalker::create_matcher(dir.path(), false).unwrap();
		assert!(!matcher
			.matched(dir.path().join("anything.rs"), false)
			.is_ignore());
	}

	#[test]
	fn language_detection_covers_known_and_unknown_extensions() {
		assert_eq!(detect_language(Path::new("a/main.rs")), Some("rust"));
		assert_eq!(detect_language(Path::new("a/app.py")), Some("python"));
		assert_eq!(detect_language(Path::new("a/mod.go")), Some("go"));
		assert_eq!(detect_language(Path::new("a/data.bin")), None);
		assert_eq!(detect_language(Path::new("a/noext")), None);
	}

	#[test]
	fn file_mtimes_are_readable_and_missing_files_error() {
		let dir = workspace();
		let mtime = get_file_mtime(&dir.path().join("src/main.rs")).unwrap();
		assert!(mtime > 0);
		assert!(get_file_mtime(&dir.path().join("nope.rs")).is_err());
	}

	#[test]
	fn a_markdown_graphrag_block_carries_the_whole_file() {
		let block = markdown_graphrag_block("docs/a.md", "# Title\n\ntext\n");
		assert_eq!(block.path, "docs/a.md");
		assert_eq!(block.language, "markdown");
		assert_eq!(block.content, "# Title\n\ntext\n");
		assert_eq!(block.start_line, 0);
		assert_eq!(block.end_line, 3);
		assert!(!block.hash.is_empty());
		assert!(block.symbols.is_empty());
	}

	#[test]
	fn the_indexable_file_count_matches_the_walk() {
		let dir = workspace();
		// src/main.rs, src/lib.py and README.md are indexable; target/ is ignored.
		assert_eq!(fast_count_indexable_files(dir.path(), None), 3);
	}

	#[test]
	fn a_git_changed_file_set_narrows_the_count() {
		let dir = workspace();
		let changed: HashSet<String> = ["src/main.rs", "src/gone.rs", "target/build.rs"]
			.iter()
			.map(|s| s.to_string())
			.collect();
		// Only files that still exist on disk and are indexable are counted; the
		// gitignore is not re-applied to an explicit change set.
		assert_eq!(fast_count_indexable_files(dir.path(), Some(&changed)), 2);

		assert_eq!(
			fast_count_indexable_files(dir.path(), Some(&HashSet::new())),
			0
		);
	}

	#[test]
	fn the_git_helper_module_resolves_this_repository() {
		let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
		assert!(git::is_git_repo_root(manifest));
		assert_eq!(
			git::find_git_root(&manifest.join("src")).as_deref(),
			Some(manifest)
		);
		assert!(!git::get_current_commit_hash(manifest).unwrap().is_empty());
		assert!(git::get_all_changed_files(manifest).is_ok());
	}

	#[test]
	fn structured_output_validation_passes_when_no_llm_feature_is_enabled() {
		let mut config = Config::default();
		config.graphrag.use_llm = false;
		config.index.contextual_descriptions = false;
		config.commits.use_llm = false;
		validate_llm_structured_output(&config, true).expect("nothing to validate");
	}

	fn run_git(repo: &Path, args: &[&str]) -> String {
		let out = std::process::Command::new("git")
			.args(args)
			.current_dir(repo)
			.output()
			.unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
		assert!(
			out.status.success(),
			"git {args:?} failed: {}",
			String::from_utf8_lossy(&out.stderr)
		);
		String::from_utf8_lossy(&out.stdout).trim().to_string()
	}

	#[test]
	fn a_noindex_file_in_the_root_or_a_common_subdirectory_is_detected() {
		let dir = TempDir::new().unwrap();
		assert!(!NoindexWalker::has_noindex_files(dir.path()));

		std::fs::create_dir_all(dir.path().join("src")).unwrap();
		std::fs::write(dir.path().join("src/.noindex"), "skip.rs\n").unwrap();
		assert!(NoindexWalker::has_noindex_files(dir.path()));
	}

	#[test]
	fn a_noindex_file_in_an_uncommon_subdirectory_is_not_detected() {
		// Deliberate trade-off: detection probes the root plus a fixed list of
		// common directories rather than walking the whole tree.
		let dir = TempDir::new().unwrap();
		std::fs::create_dir_all(dir.path().join("weird")).unwrap();
		std::fs::write(dir.path().join("weird/.noindex"), "skip.rs\n").unwrap();
		assert!(!NoindexWalker::has_noindex_files(dir.path()));

		std::fs::write(dir.path().join(".noindex"), "skip.rs\n").unwrap();
		assert!(NoindexWalker::has_noindex_files(dir.path()));
	}

	#[test]
	fn noindex_detection_is_cached_per_directory() {
		let dir = TempDir::new().unwrap();
		std::fs::write(dir.path().join(".noindex"), "skip.rs\n").unwrap();
		assert!(NoindexWalker::has_noindex_files_cached(dir.path()));

		std::fs::remove_file(dir.path().join(".noindex")).unwrap();
		assert!(!NoindexWalker::has_noindex_files(dir.path()));
		assert!(
			NoindexWalker::has_noindex_files_cached(dir.path()),
			"the first answer is reused for the rest of the session"
		);
	}

	#[test]
	fn committed_changes_are_listed_between_two_commits() {
		let dir = TempDir::new().unwrap();
		run_git(dir.path(), &["init", "-q", "-b", "main"]);
		run_git(dir.path(), &["config", "user.email", "t@t"]);
		run_git(dir.path(), &["config", "user.name", "t"]);
		std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
		run_git(dir.path(), &["add", "."]);
		run_git(dir.path(), &["commit", "-q", "-m", "first"]);
		let first = run_git(dir.path(), &["rev-parse", "HEAD"]);

		std::fs::write(dir.path().join("b.txt"), "b\n").unwrap();
		run_git(dir.path(), &["add", "."]);
		run_git(dir.path(), &["commit", "-q", "-m", "second"]);
		let head = run_git(dir.path(), &["rev-parse", "HEAD"]);

		assert_eq!(
			git::get_changed_files_since_commit(dir.path(), &first).unwrap(),
			vec!["b.txt".to_string()]
		);
		assert!(git::get_changed_files_since_commit(dir.path(), &head)
			.unwrap()
			.is_empty());
	}

	#[test]
	fn a_model_without_structured_output_support_is_rejected() {
		let mut config = Config::default();
		config.graphrag.enabled = true;
		config.graphrag.use_llm = true;
		config.graphrag.llm.description_model = "minimax:MiniMax-M2".to_string();

		let err = validate_llm_structured_output(&config, true)
			.expect_err("a model without JSON schema support must block indexing")
			.to_string();
		assert!(err.contains("does not support structured output"), "{err}");
		assert!(err.contains("graphrag.llm.description_model"), "{err}");
	}

	#[test]
	fn a_model_whose_provider_cannot_be_resolved_is_only_a_warning() {
		let mut config = Config::default();
		config.index.contextual_descriptions = true;
		config.index.contextual_model = "no-such-provider:some-model".to_string();

		validate_llm_structured_output(&config, true)
			.expect("an unresolvable model is reported later, not at validation time");
	}

	#[test]
	fn the_shipped_default_contextual_model_passes_validation() {
		let mut config = Config::default();
		config.index.contextual_descriptions = true;
		validate_llm_structured_output(&config, true).expect("default config must be usable");
	}

	mod store_backed {
		use super::super::super::*;
		use crate::config::Config;
		use crate::store::mod_tests::{code_block, embedding, test_store, CODE_DIM};
		use crate::store::tables;
		use tempfile::TempDir;

		#[tokio::test]
		async fn flushing_is_driven_by_the_configured_frequency() {
			let (_dir, store) = test_store().await;
			let config = Config::default();
			let mut batches = 0;

			assert!(!flush_if_needed(&store, &mut batches, &config, false)
				.await
				.unwrap());
			assert_eq!(batches, 0);

			// Forcing flushes regardless of the counter.
			assert!(flush_if_needed(&store, &mut batches, &config, true)
				.await
				.unwrap());

			batches = config.index.flush_frequency;
			assert!(flush_if_needed(&store, &mut batches, &config, false)
				.await
				.unwrap());
			assert_eq!(batches, 0, "the counter resets after a flush");
		}

		#[tokio::test]
		async fn cleanup_is_skipped_when_nothing_is_indexed() {
			let (_dir, store) = test_store().await;
			let workspace = TempDir::new().unwrap();
			cleanup_deleted_files_optimized(&store, workspace.path(), true)
				.await
				.unwrap();
		}

		#[tokio::test]
		async fn cleanup_removes_deleted_and_newly_ignored_files() {
			let (_dir, store) = test_store().await;
			let workspace = TempDir::new().unwrap();
			// `ignore` only applies .gitignore rules inside a real repository.
			std::fs::create_dir_all(workspace.path().join(".git")).unwrap();
			std::fs::create_dir_all(workspace.path().join("src")).unwrap();
			std::fs::create_dir_all(workspace.path().join("build")).unwrap();
			std::fs::write(workspace.path().join("src/keep.rs"), "fn keep() {}\n").unwrap();
			std::fs::write(workspace.path().join("build/gen.rs"), "fn gen() {}\n").unwrap();
			std::fs::write(workspace.path().join(".gitignore"), "build/\n").unwrap();

			store
				.store_code_blocks(
					&[
						code_block("src/keep.rs", "h1"),
						code_block("src/deleted.rs", "h2"),
						code_block("build/gen.rs", "h3"),
					],
					&[
						embedding(CODE_DIM, 0),
						embedding(CODE_DIM, 1),
						embedding(CODE_DIM, 2),
					],
				)
				.await
				.unwrap();

			cleanup_deleted_files_optimized(&store, workspace.path(), true)
				.await
				.unwrap();

			let remaining = store.get_all_indexed_file_paths().await.unwrap();
			assert!(remaining.contains("src/keep.rs"));
			assert!(
				!remaining.contains("src/deleted.rs"),
				"a file that vanished from disk must be dropped"
			);
			assert!(
				!remaining.contains("build/gen.rs"),
				"a file that became gitignored must be dropped"
			);
		}

		#[tokio::test]
		async fn metadata_is_only_stored_for_a_git_repository() {
			let (_dir, store) = test_store().await;
			let config = Config::default();

			// No git root: the flush still happens but no commit marker is written.
			persist_and_store_metadata(&store, None, &config, true, "test")
				.await
				.unwrap();
			assert_eq!(store.get_last_commit_hash().await.unwrap(), None);

			let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
			persist_and_store_metadata(&store, Some(manifest), &config, true, "test")
				.await
				.unwrap();
			let stored = store.get_last_commit_hash().await.unwrap();
			assert_eq!(stored.as_deref().map(str::len), Some(40));
			// GraphRAG is off by default, so its marker stays unset.
			assert_eq!(store.get_graphrag_last_commit_hash().await.unwrap(), None);
		}

		#[tokio::test]
		async fn enabling_graphrag_also_stamps_its_own_commit_marker() {
			let (_dir, store) = test_store().await;
			let mut config = Config::default();
			config.graphrag.enabled = true;

			let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
			persist_and_store_metadata(&store, Some(manifest), &config, true, "test")
				.await
				.unwrap();
			assert!(store
				.get_graphrag_last_commit_hash()
				.await
				.unwrap()
				.is_some());
		}

		#[tokio::test]
		async fn a_directory_that_is_not_a_repository_skips_the_commit_marker() {
			let (_dir, store) = test_store().await;
			let outside = TempDir::new().unwrap();
			persist_and_store_metadata(
				&store,
				Some(outside.path()),
				&Config::default(),
				true,
				"test",
			)
			.await
			.expect("a missing commit hash is not fatal");
			assert_eq!(store.get_last_commit_hash().await.unwrap(), None);
		}

		#[tokio::test]
		async fn a_change_to_a_vanished_file_only_removes_its_blocks() {
			let (_dir, store) = test_store().await;
			store
				.store_code_blocks(
					&[
						code_block("src/gone.rs", "h1"),
						code_block("src/stay.rs", "h2"),
					],
					&[embedding(CODE_DIM, 0), embedding(CODE_DIM, 1)],
				)
				.await
				.unwrap();

			handle_file_change(&store, "src/gone.rs", &Config::default())
				.await
				.unwrap();

			let remaining = store.get_all_indexed_file_paths().await.unwrap();
			assert!(!remaining.contains("src/gone.rs"));
			assert!(remaining.contains("src/stay.rs"));
			assert_eq!(
				store
					.get_table_row_count(tables::CODE_BLOCKS)
					.await
					.unwrap(),
				1
			);
		}

		/// A repository containing nothing the indexer can index, so a full run
		/// completes without ever calling an embedding provider.
		fn repo_without_indexable_files() -> TempDir {
			let dir = TempDir::new().unwrap();
			super::run_git(dir.path(), &["init", "-q", "-b", "main"]);
			super::run_git(dir.path(), &["config", "user.email", "t@t"]);
			super::run_git(dir.path(), &["config", "user.name", "t"]);
			std::fs::write(dir.path().join("blob.bin"), "not indexable\n").unwrap();
			super::run_git(dir.path(), &["add", "."]);
			super::run_git(dir.path(), &["commit", "-q", "-m", "first"]);
			dir
		}

		fn state_at(dir: &std::path::Path) -> crate::state::SharedState {
			let state = crate::state::create_shared_state();
			state.write().current_directory = dir.to_path_buf();
			state
		}

		#[tokio::test]
		async fn a_first_run_stamps_the_commit_even_with_nothing_to_index() {
			let (_db, store) = test_store().await;
			let dir = repo_without_indexable_files();
			let head = super::run_git(dir.path(), &["rev-parse", "HEAD"]);
			// Commit indexing needs embeddings; mark it done so the run is offline.
			store.store_commits_last_commit_hash(&head).await.unwrap();

			let state = state_at(dir.path());
			index_files_with_quiet(
				&store,
				state.clone(),
				&Config::default(),
				Some(dir.path()),
				true,
			)
			.await
			.unwrap();

			assert!(state.read().indexing_complete);
			assert_eq!(state.read().indexed_files, 0);
			assert_eq!(state.read().embedding_calls, 0);
			assert_eq!(
				store.get_last_commit_hash().await.unwrap(),
				Some(head),
				"the commit must be recorded even when zero files were indexed"
			);
		}

		#[tokio::test]
		async fn a_rerun_at_the_same_commit_skips_indexing_but_still_prunes_deleted_files() {
			let (_db, store) = test_store().await;
			let dir = repo_without_indexable_files();
			let head = super::run_git(dir.path(), &["rev-parse", "HEAD"]);
			store.store_commits_last_commit_hash(&head).await.unwrap();
			store.store_git_metadata(&head).await.unwrap();

			// A row whose file no longer exists on disk.
			store
				.store_code_blocks(
					&[code_block("src/vanished.rs", "h1")],
					&[embedding(CODE_DIM, 0)],
				)
				.await
				.unwrap();

			let state = state_at(dir.path());
			index_files_with_quiet(
				&store,
				state.clone(),
				&Config::default(),
				Some(dir.path()),
				true,
			)
			.await
			.unwrap();

			assert!(state.read().indexing_complete);
			assert!(
				!store
					.get_all_indexed_file_paths()
					.await
					.unwrap()
					.contains("src/vanished.rs"),
				"the same-commit path must still drop rows for deleted files"
			);
		}

		#[tokio::test]
		async fn a_new_commit_advances_the_stored_commit_hash() {
			let (_db, store) = test_store().await;
			let dir = repo_without_indexable_files();
			let first = super::run_git(dir.path(), &["rev-parse", "HEAD"]);
			store.store_git_metadata(&first).await.unwrap();

			std::fs::write(dir.path().join("second.bin"), "still not indexable\n").unwrap();
			super::run_git(dir.path(), &["add", "."]);
			super::run_git(dir.path(), &["commit", "-q", "-m", "second"]);
			let head = super::run_git(dir.path(), &["rev-parse", "HEAD"]);
			store.store_commits_last_commit_hash(&head).await.unwrap();

			let state = state_at(dir.path());
			index_files_with_quiet(
				&store,
				state.clone(),
				&Config::default(),
				Some(dir.path()),
				true,
			)
			.await
			.unwrap();

			assert_eq!(store.get_last_commit_hash().await.unwrap(), Some(head));
			assert_eq!(state.read().embedding_calls, 0);
		}
	}
}
