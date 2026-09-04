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
	use crate::commands::output_format::OutputFormat;
	use clap::ValueEnum;

	#[test]
	fn cli_is_the_default_format() {
		let format = OutputFormat::default();
		assert!(format.is_cli());
		assert!(!format.is_json());
		assert!(!format.is_md());
		assert!(!format.is_text());
	}

	#[test]
	fn every_variant_reports_exactly_one_kind() {
		for format in OutputFormat::value_variants() {
			let flags = [
				format.is_cli(),
				format.is_json(),
				format.is_md(),
				format.is_text(),
			];
			assert_eq!(
				flags.iter().filter(|f| **f).count(),
				1,
				"{format:?} matched more than one predicate"
			);
		}
	}

	#[test]
	fn variants_parse_from_their_cli_names() {
		assert!(OutputFormat::from_str("json", false).unwrap().is_json());
		assert!(OutputFormat::from_str("md", false).unwrap().is_md());
		assert!(OutputFormat::from_str("text", false).unwrap().is_text());
		assert!(OutputFormat::from_str("cli", false).unwrap().is_cli());
		assert!(OutputFormat::from_str("yaml", false).is_err());
	}
}
