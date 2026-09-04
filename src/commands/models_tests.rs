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

	#[test]
	fn a_model_spec_splits_on_the_first_colon_only() {
		let (provider, model) = parse_model_spec("voyage:voyage-code-3").unwrap();
		assert_eq!(provider, "voyage");
		assert_eq!(model, "voyage-code-3");

		// A model name containing a colon keeps everything after the first one.
		let (provider, model) = parse_model_spec("openrouter:openai/gpt-4o:free").unwrap();
		assert_eq!(provider, "openrouter");
		assert_eq!(model, "openai/gpt-4o:free");
	}

	#[test]
	fn a_spec_without_a_provider_is_rejected() {
		let err = parse_model_spec("voyage-code-3").unwrap_err().to_string();
		assert!(err.contains("Expected format: 'provider:model'"), "{err}");
		assert!(parse_model_spec("").is_err());
	}

	#[test]
	fn every_supported_provider_name_resolves() {
		for name in [
			"fastembed",
			"huggingface",
			"jina",
			"voyage",
			"google",
			"openai",
			"openrouter",
			"octohub",
			"local",
			"together",
		] {
			parse_provider(name).unwrap_or_else(|e| panic!("{name} should resolve: {e}"));
			// Matching is case-insensitive.
			parse_provider(&name.to_uppercase())
				.unwrap_or_else(|e| panic!("{name} uppercase should resolve: {e}"));
		}
	}

	#[test]
	fn an_unknown_provider_lists_the_supported_ones() {
		let err = parse_provider("cohere").unwrap_err().to_string();
		assert!(err.contains("Unknown provider 'cohere'"), "{err}");
		assert!(err.contains("Supported:"), "{err}");
	}
}
