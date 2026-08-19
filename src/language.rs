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

use anyhow::{bail, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

static FILE_ASSOCIATIONS: LazyLock<RwLock<HashMap<String, &'static str>>> =
	LazyLock::new(|| RwLock::new(HashMap::new()));

/// Replace the process-wide file associations loaded from the active config.
pub fn configure_file_associations(associations: &HashMap<String, String>) -> Result<()> {
	*FILE_ASSOCIATIONS.write() = normalize_file_associations(associations)?;
	Ok(())
}

/// Return a configured language association for a path, if one exists.
pub fn associated_language(path: &Path) -> Option<&'static str> {
	let extension = path.extension()?.to_str()?.to_ascii_lowercase();
	FILE_ASSOCIATIONS.read().get(&extension).copied()
}

fn normalize_file_associations(
	associations: &HashMap<String, String>,
) -> Result<HashMap<String, &'static str>> {
	associations
		.iter()
		.map(|(extension, language)| {
			let extension = extension
				.strip_prefix('.')
				.unwrap_or(extension)
				.to_ascii_lowercase();
			if extension.is_empty()
				|| !extension
					.chars()
					.all(|character| character.is_ascii_alphanumeric() || "_+-".contains(character))
			{
				bail!("invalid file association extension '{extension}'");
			}

			let Some(language) = canonical_language(language) else {
				bail!("unsupported file association language '{language}'");
			};
			Ok((extension, language))
		})
		.collect()
}

fn canonical_language(language: &str) -> Option<&'static str> {
	let language = language.to_ascii_lowercase();
	crate::indexer::languages::get_language(&language).map(|language| language.name())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn normalizes_extensions_and_language_names() {
		let associations = HashMap::from([(".INC".to_string(), "PHP".to_string())]);
		let normalized = normalize_file_associations(&associations).unwrap();

		assert_eq!(normalized.get("inc"), Some(&"php"));
	}

	#[test]
	fn applies_configured_association() {
		let associations = HashMap::from([("pfbinc".to_string(), "php".to_string())]);
		configure_file_associations(&associations).unwrap();

		assert_eq!(
			associated_language(Path::new("functions.pfbinc")),
			Some("php")
		);

		configure_file_associations(&HashMap::new()).unwrap();
	}

	#[test]
	fn rejects_invalid_associations() {
		let invalid_extension = HashMap::from([("*.inc".to_string(), "php".to_string())]);
		assert!(normalize_file_associations(&invalid_extension).is_err());

		let invalid_language = HashMap::from([("inc".to_string(), "pascal".to_string())]);
		assert!(normalize_file_associations(&invalid_language).is_err());
	}
}
