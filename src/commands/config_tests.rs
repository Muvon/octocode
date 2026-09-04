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
	use super::super::config::{execute, ConfigArgs};
	use octocode::config::Config;
	use std::path::PathBuf;
	use std::sync::{Mutex, OnceLock};

	static CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();
	/// `OCTOCODE_CONFIG_PATH` and the file behind it are process-wide, so the
	/// tests that actually write configuration take turns.
	static WRITE_LOCK: Mutex<()> = Mutex::new(());

	/// Point `Config::save()` / `Config::get_config_path()` at a scratch file so
	/// the tests never touch the developer's real configuration.
	fn scratch_config_path() -> &'static PathBuf {
		let path = CONFIG_PATH.get_or_init(|| {
			let dir = std::env::temp_dir().join("octocode-config-command-tests");
			std::fs::create_dir_all(&dir).expect("scratch dir");
			dir.join("config.toml")
		});
		std::env::set_var("OCTOCODE_CONFIG_PATH", path);
		path
	}

	fn args() -> ConfigArgs {
		ConfigArgs {
			model: None,
			code_embedding_model: None,
			text_embedding_model: None,
			chunk_size: None,
			chunk_overlap: None,
			max_results: None,
			similarity_threshold: None,
			graphrag_enabled: None,
			show: false,
			reset: false,
		}
	}

	#[test]
	fn show_renders_the_whole_configuration() {
		scratch_config_path();
		let mut config = Config::default();
		config.graphrag.enabled = true;

		let mut a = args();
		a.show = true;
		execute(&a, config).expect("show must not fail");
	}

	#[test]
	fn show_also_covers_a_configuration_without_graphrag() {
		scratch_config_path();
		let mut config = Config::default();
		config.graphrag.enabled = false;

		let mut a = args();
		a.show = true;
		execute(&a, config).expect("show must not fail");
	}

	#[test]
	fn show_reports_each_api_backed_embedding_provider() {
		scratch_config_path();
		for model in [
			"jina:jina-embeddings-v3",
			"voyage:voyage-code-3",
			"google:gemini-embedding-001",
			"openai:text-embedding-3-small",
			"fastembed:nomic-ai/nomic-embed-text-v1.5",
		] {
			let mut config = Config::default();
			config.embedding.code_model = model.to_string();
			config.embedding.text_model = model.to_string();

			let mut a = args();
			a.show = true;
			execute(&a, config).unwrap_or_else(|e| panic!("show failed for {model}: {e}"));
		}
	}

	#[test]
	fn a_malformed_embedding_model_is_rejected_before_anything_is_written() {
		scratch_config_path();

		let mut a = args();
		a.code_embedding_model = Some("no-provider-prefix".to_string());
		assert!(execute(&a, Config::default()).is_err());

		let mut a = args();
		a.text_embedding_model = Some("no-provider-prefix".to_string());
		assert!(execute(&a, Config::default()).is_err());
	}

	#[test]
	fn calling_config_with_no_flags_prints_usage_help() {
		scratch_config_path();
		execute(&args(), Config::default()).expect("no-op must succeed");
	}

	#[test]
	fn every_settable_field_is_written_back_to_disk() {
		let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
		let path = scratch_config_path();

		let mut a = args();
		a.model = Some("openai/gpt-4.1-mini".to_string());
		a.code_embedding_model = Some("voyage:voyage-code-3".to_string());
		a.text_embedding_model = Some("voyage:voyage-3.5-lite".to_string());
		a.chunk_size = Some(1234);
		a.chunk_overlap = Some(56);
		a.max_results = Some(7);
		a.similarity_threshold = Some(0.42);
		a.graphrag_enabled = Some(true);
		execute(&a, Config::default()).expect("update must succeed");

		let saved: Config = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
		assert_eq!(saved.llm.model, "openai/gpt-4.1-mini");
		assert_eq!(saved.embedding.code_model, "voyage:voyage-code-3");
		assert_eq!(saved.embedding.text_model, "voyage:voyage-3.5-lite");
		assert_eq!(saved.index.chunk_size, 1234);
		assert_eq!(saved.index.chunk_overlap, 56);
		assert_eq!(saved.search.max_results, 7);
		assert!((saved.search.similarity_threshold - 0.42).abs() < 1e-6);
		assert!(saved.graphrag.enabled);

		// Disabling takes the other branch of the graphrag message.
		let mut a = args();
		a.graphrag_enabled = Some(false);
		execute(&a, Config::default()).expect("update must succeed");
		let saved: Config = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
		assert!(!saved.graphrag.enabled);
	}

	#[test]
	fn reset_restores_the_shipped_defaults() {
		let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
		let path = scratch_config_path();
		let mut a = args();
		a.max_results = Some(99);
		execute(&a, Config::default()).unwrap();

		let mut a = args();
		a.reset = true;
		execute(&a, Config::default()).expect("reset must succeed");

		let saved: Config = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
		assert_eq!(
			saved.search.max_results,
			Config::default().search.max_results
		);
	}
}
