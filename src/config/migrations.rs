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

//! Octocode's configuration version chain.
//!
//! The driver (version walk, guards, table merging) lives in
//! `octolib::utils::config_migration`; only the per-version steps are here.

use anyhow::Result;
use octolib::utils::config_migration::{
	copy_item, copy_missing_item, required_table, required_table_mut, toml_edit, MigrationPlan,
	VersionMigration,
};

pub(super) use octolib::utils::config_migration::Migration;

fn plan() -> MigrationPlan {
	MigrationPlan::new(
		"octocode",
		vec![VersionMigration {
			from: 1,
			to: 2,
			apply: migrate_v1_to_v2,
		}],
	)
}

/// Upgrade an existing configuration to the version declared by the embedded
/// default template. `Ok(None)` means the file is already current and must be
/// left untouched.
pub(super) fn migrate(existing: &str, template: &str) -> Result<Option<Migration>> {
	plan().migrate(existing, template)
}

/// v1 is the schema shipped in octocode 0.18.1. v2 adds reasoning search and
/// the RRF tuning fields. Existing values always win; only missing v2 fields
/// are copied from the v2 template.
fn migrate_v1_to_v2(
	document: &mut toml_edit::DocumentMut,
	template: &toml_edit::DocumentMut,
) -> Result<()> {
	let template_search = required_table(template.as_table(), "search", "template")?;

	if !document.as_table().contains_key("search") {
		copy_item(document.as_table_mut(), template.as_table(), "search")?;
		return Ok(());
	}

	let search = required_table_mut(document.as_table_mut(), "search", "user configuration")?;
	let template_hybrid = required_table(template_search, "hybrid", "template search config")?;

	if !search.contains_key("hybrid") {
		copy_item(search, template_search, "hybrid")?;
	} else {
		let hybrid = required_table_mut(search, "hybrid", "user search config")?;
		copy_missing_item(hybrid, template_hybrid, "rrf_k")?;
		copy_missing_item(hybrid, template_hybrid, "auto_weight")?;
	}

	if !search.contains_key("reasoning") {
		copy_item(search, template_search, "reasoning")?;
	} else {
		let reasoning = required_table_mut(search, "reasoning", "user search config")?;
		let template_reasoning =
			required_table(template_search, "reasoning", "template search config")?;
		for key in [
			"enabled",
			"model",
			"max_candidates",
			"final_top_k",
			"context_level",
			"reasoning_weight",
		] {
			copy_missing_item(reasoning, template_reasoning, key)?;
		}
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	const V1_SEARCH_CONFIG: &str = r#"# User values and comments must survive migration.
version = 1

[search]
max_results = 17
similarity_threshold = 0.72
output_format = "markdown"
max_files = 8
context_lines = 5
search_block_max_characters = 600
graph_expansion = true

[search.reranker]
enabled = true
model = "fastembed:jina-reranker-v2-base-multilingual"
top_k_candidates = 40
final_top_k = 8

[search.hybrid]
enabled = true
default_vector_weight = 0.7
default_keyword_weight = 0.3
"#;

	#[test]
	fn migrates_released_v1_search_shape_to_v2() {
		let migration = migrate(V1_SEARCH_CONFIG, super::super::DEFAULT_CONFIG_TEMPLATE)
			.expect("v1 migration should succeed")
			.expect("v1 should require migration");

		assert_eq!(migration.from_version, 1);
		assert_eq!(migration.to_version, 2);
		assert!(migration
			.content
			.contains("# User values and comments must survive migration."));
		assert!(migration
			.content
			.contains("# PageIndex-style reasoning retrieval"));

		let migrated: toml::Value =
			toml::from_str(&migration.content).expect("migrated config should be valid TOML");
		assert_eq!(migrated["version"].as_integer(), Some(2));
		assert_eq!(
			migrated["search"]["hybrid"]["default_vector_weight"].as_float(),
			Some(0.7)
		);
		assert_eq!(migrated["search"]["hybrid"]["rrf_k"].as_float(), Some(60.0));
		assert_eq!(
			migrated["search"]["hybrid"]["auto_weight"].as_bool(),
			Some(false)
		);
		assert_eq!(
			migrated["search"]["reasoning"]["enabled"].as_bool(),
			Some(false)
		);
	}

	#[test]
	fn preserves_existing_v2_field_values_during_v1_migration() {
		let existing = format!(
			"{V1_SEARCH_CONFIG}\n[search.reasoning]\nenabled = true\nmodel = \"openrouter:custom/model\"\n"
		);
		let migration = migrate(&existing, super::super::DEFAULT_CONFIG_TEMPLATE)
			.expect("migration should succeed")
			.expect("v1 should require migration");
		let migrated: toml::Value = toml::from_str(&migration.content).unwrap();

		assert_eq!(
			migrated["search"]["reasoning"]["enabled"].as_bool(),
			Some(true)
		);
		assert_eq!(
			migrated["search"]["reasoning"]["model"].as_str(),
			Some("openrouter:custom/model")
		);
		assert_eq!(
			migrated["search"]["reasoning"]["max_candidates"].as_integer(),
			Some(25)
		);
	}

	#[test]
	fn migrates_v1_without_search_table() {
		let migration = migrate("version = 1\n", super::super::DEFAULT_CONFIG_TEMPLATE)
			.expect("minimal v1 config should migrate")
			.expect("v1 should require migration");
		let migrated: toml::Value = toml::from_str(&migration.content).unwrap();

		assert_eq!(migrated["version"].as_integer(), Some(2));
		assert_eq!(migrated["search"]["hybrid"]["rrf_k"].as_float(), Some(60.0));
		assert_eq!(
			migrated["search"]["reasoning"]["enabled"].as_bool(),
			Some(false)
		);
	}

	#[test]
	fn current_version_is_not_rewritten() {
		assert!(migrate(
			super::super::DEFAULT_CONFIG_TEMPLATE,
			super::super::DEFAULT_CONFIG_TEMPLATE
		)
		.expect("current config should load")
		.is_none());
	}

	#[test]
	fn rejects_future_versions_without_migrating() {
		let future =
			super::super::DEFAULT_CONFIG_TEMPLATE.replacen("version = 2", "version = 3", 1);
		let error = migrate(&future, super::super::DEFAULT_CONFIG_TEMPLATE)
			.expect_err("future version should fail");
		assert!(error
			.to_string()
			.contains("newer than this octocode binary"));
	}
}
