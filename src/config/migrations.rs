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
use toml_edit::{value, DocumentMut, Item, Table};

#[derive(Debug)]
pub(super) struct Migration {
	pub content: String,
	pub from_version: u32,
	pub to_version: u32,
}

/// Upgrade an existing configuration to the version declared by the embedded
/// default template. Each migration advances exactly one version, making the
/// chain deterministic and preventing later migrations from duplicating older
/// work.
pub(super) fn migrate(existing: &str, template: &str) -> Result<Option<Migration>> {
	let mut document = parse_document(existing, "user configuration")?;
	let template = parse_document(template, "embedded default configuration")?;

	let from_version = document_version(&document, "user configuration")?;
	let target_version = document_version(&template, "embedded default configuration")?;

	if from_version > target_version {
		bail!(
			"configuration version {from_version} is newer than this octocode binary supports ({target_version})"
		);
	}

	let mut version = from_version;
	while version < target_version {
		version = match version {
			1 => {
				migrate_v1_to_v2(&mut document, &template)?;
				2
			}
			unsupported => {
				bail!("no configuration migration exists from version {unsupported}")
			}
		};

		document["version"] = value(i64::from(version));
	}

	if from_version == target_version {
		return Ok(None);
	}

	Ok(Some(Migration {
		content: document.to_string(),
		from_version,
		to_version: target_version,
	}))
}

fn parse_document(content: &str, description: &str) -> Result<DocumentMut> {
	content
		.parse::<DocumentMut>()
		.with_context(|| format!("failed to parse {description}"))
}

fn document_version(document: &DocumentMut, description: &str) -> Result<u32> {
	let version = document
		.get("version")
		.and_then(Item::as_integer)
		.with_context(|| format!("{description} must contain an integer 'version' field"))?;

	u32::try_from(version)
		.with_context(|| format!("{description} contains invalid version {version}"))
}

/// v1 is the schema shipped in octocode 0.18.1. v2 adds reasoning search and
/// the RRF tuning fields. Existing values always win; only missing v2 fields
/// are copied from the v2 template.
fn migrate_v1_to_v2(document: &mut DocumentMut, template: &DocumentMut) -> Result<()> {
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

fn required_table<'a>(table: &'a Table, key: &str, description: &str) -> Result<&'a Table> {
	table
		.get(key)
		.and_then(Item::as_table)
		.with_context(|| format!("{description} must contain a '{key}' table"))
}

fn required_table_mut<'a>(
	table: &'a mut Table,
	key: &str,
	description: &str,
) -> Result<&'a mut Table> {
	table
		.get_mut(key)
		.and_then(Item::as_table_mut)
		.with_context(|| format!("{description} must contain a '{key}' table"))
}

fn copy_missing_item(target: &mut Table, source: &Table, key: &str) -> Result<()> {
	if target.contains_key(key) {
		return Ok(());
	}

	copy_item(target, source, key)
}

fn copy_item(target: &mut Table, source: &Table, key: &str) -> Result<()> {
	let (formatted_key, item) = source
		.get_key_value(key)
		.with_context(|| format!("embedded default configuration is missing '{key}'"))?;
	target.insert_formatted(formatted_key, item.clone());
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
