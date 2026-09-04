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
	use crate::config::Config;
	use crate::mcp::semantic_code::SemanticCodeProvider;
	use serde_json::json;
	use tempfile::TempDir;

	fn project() -> (TempDir, SemanticCodeProvider) {
		let dir = TempDir::new().unwrap();
		std::fs::create_dir_all(dir.path().join("src")).unwrap();
		std::fs::write(
			dir.path().join("src/lib.rs"),
			"/// Adds two numbers.\npub fn add(a: u32, b: u32) -> u32 {\n\ta + b\n}\n\npub struct Config {\n\tpub name: String,\n}\n",
		)
		.unwrap();
		std::fs::write(dir.path().join("README.md"), "# Docs\n").unwrap();

		let provider = SemanticCodeProvider::new(Config::default(), dir.path().to_path_buf());
		(dir, provider)
	}

	#[tokio::test]
	async fn a_missing_query_is_rejected() {
		let (_dir, provider) = project();
		let err = provider.execute_search(&json!({})).await.unwrap_err();
		assert_eq!(err.code, -32602);
		assert!(err.message.contains("Missing required parameter 'query'"));
	}

	#[tokio::test]
	async fn an_empty_query_array_is_rejected() {
		let (_dir, provider) = project();
		let err = provider
			.execute_search(&json!({"query": []}))
			.await
			.unwrap_err();
		assert!(
			err.message.contains("at least one non-empty string"),
			"{err}"
		);
	}

	#[tokio::test]
	async fn queries_are_length_bounded() {
		let (_dir, provider) = project();

		let short = provider
			.execute_search(&json!({"query": "ab"}))
			.await
			.unwrap_err();
		assert!(short.message.contains("at least 3 characters"), "{short}");

		let long = provider
			.execute_search(&json!({"query": "x".repeat(501)}))
			.await
			.unwrap_err();
		assert!(
			long.message.contains("no more than 500 characters"),
			"{long}"
		);
	}

	#[tokio::test]
	async fn too_many_queries_are_rejected() {
		let (_dir, provider) = project();
		let queries: Vec<String> = (0..64).map(|i| format!("query number {i}")).collect();
		let err = provider
			.execute_search(&json!({"query": queries}))
			.await
			.unwrap_err();
		assert!(err.message.contains("Too many queries"), "{err}");
	}

	#[tokio::test]
	async fn invalid_enum_parameters_are_rejected() {
		let (_dir, provider) = project();

		let mode = provider
			.execute_search(&json!({"query": "adding", "mode": "sideways"}))
			.await
			.unwrap_err();
		assert!(mode.message.contains("Invalid mode 'sideways'"), "{mode}");

		let detail = provider
			.execute_search(&json!({"query": "adding", "detail_level": "verbose"}))
			.await
			.unwrap_err();
		assert!(
			detail.message.contains("Invalid detail_level 'verbose'"),
			"{detail}"
		);
	}

	#[tokio::test]
	async fn numeric_parameters_are_range_checked() {
		let (_dir, provider) = project();

		for max_results in [0u64, 21] {
			let err = provider
				.execute_search(&json!({"query": "adding", "max_results": max_results}))
				.await
				.unwrap_err();
			assert!(err.message.contains("Invalid max_results"), "{err}");
		}

		let threshold = provider
			.execute_search(&json!({"query": "adding", "threshold": 1.5}))
			.await
			.unwrap_err();
		assert!(
			threshold.message.contains("Invalid similarity threshold"),
			"{threshold}"
		);
	}

	#[tokio::test]
	async fn the_language_filter_must_name_a_supported_language() {
		let (_dir, provider) = project();

		let unsupported = provider
			.execute_search(&json!({"query": "adding", "language": "cobol"}))
			.await
			.unwrap_err();
		assert!(
			unsupported.message.contains("Invalid language 'cobol'"),
			"{unsupported}"
		);

		let wrong_type = provider
			.execute_search(&json!({"query": "adding", "language": 7}))
			.await
			.unwrap_err();
		assert!(
			wrong_type.message.contains("must be a string"),
			"{wrong_type}"
		);
	}

	#[tokio::test]
	async fn view_signatures_validates_the_files_array() {
		let (_dir, provider) = project();

		let missing = provider
			.execute_view_signatures(&json!({}))
			.await
			.unwrap_err();
		assert!(missing
			.message
			.contains("Missing required parameter 'files'"));

		let empty = provider
			.execute_view_signatures(&json!({"files": []}))
			.await
			.unwrap_err();
		assert!(empty.message.contains("at least one file path"), "{empty}");

		let too_many: Vec<String> = (0..101).map(|i| format!("f{i}.rs")).collect();
		let over = provider
			.execute_view_signatures(&json!({"files": too_many}))
			.await
			.unwrap_err();
		assert!(over.message.contains("no more than 100 patterns"), "{over}");

		let wrong_type = provider
			.execute_view_signatures(&json!({"files": [7]}))
			.await
			.unwrap_err();
		assert!(
			wrong_type.message.contains("must be strings"),
			"{wrong_type}"
		);

		let blank = provider
			.execute_view_signatures(&json!({"files": ["  "]}))
			.await
			.unwrap_err();
		assert!(blank.message.contains("cannot be empty"), "{blank}");

		let long = provider
			.execute_view_signatures(&json!({"files": ["x".repeat(501)]}))
			.await
			.unwrap_err();
		assert!(
			long.message.contains("no more than 500 characters"),
			"{long}"
		);
	}

	#[tokio::test]
	async fn view_signatures_reads_a_direct_file_path() {
		let (_dir, provider) = project();
		let out = provider
			.execute_view_signatures(&json!({"files": ["src/lib.rs"]}))
			.await
			.unwrap();
		assert!(out.contains("src/lib.rs"), "{out}");
		assert!(out.contains("add"), "{out}");
	}

	#[tokio::test]
	async fn view_signatures_expands_glob_patterns() {
		let (_dir, provider) = project();
		let out = provider
			.execute_view_signatures(&json!({"files": ["src/*.rs"]}))
			.await
			.unwrap();
		assert!(out.contains("src/lib.rs"), "{out}");
	}

	#[tokio::test]
	async fn view_signatures_reports_when_nothing_matches() {
		let (_dir, provider) = project();
		let out = provider
			.execute_view_signatures(&json!({"files": ["nowhere/*.zig"]}))
			.await
			.unwrap();
		assert_eq!(out, "No matching files found for the specified patterns.");
	}

	#[tokio::test]
	async fn view_signatures_refuses_to_read_outside_the_working_directory() {
		let (_dir, provider) = project();
		let outside = TempDir::new().unwrap();
		let secret = outside.path().join("secret.rs");
		std::fs::write(&secret, "pub fn secret() {}\n").unwrap();

		let err = provider
			.execute_view_signatures(&json!({"files": [secret.to_string_lossy()]}))
			.await
			.unwrap_err();
		assert!(
			err.message
				.contains("resolves outside the working directory"),
			"{err}"
		);
	}

	#[tokio::test]
	async fn an_unparseable_glob_is_rejected() {
		let (_dir, provider) = project();
		let err = provider
			.execute_view_signatures(&json!({"files": ["src/[unclosed"]}))
			.await
			.unwrap_err();
		assert!(err.message.contains("Invalid glob pattern"), "{err}");
	}
}
