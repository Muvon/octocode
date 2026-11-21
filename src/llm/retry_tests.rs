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

use super::*;

#[test]
fn client_errors_from_providers_are_not_retried() {
	// The exact shapes octolib's providers return for a non-2xx answer.
	let bad_request = anyhow::anyhow!("OpenAI API error 400 Bad Request: {{\"error\":\"context_length_exceeded\"}}");
	let bad_key = anyhow::anyhow!("Anthropic API error 401 Unauthorized: invalid x-api-key");
	assert_eq!(error_status(&bad_request), Some(400));
	assert!(!should_retry(&bad_request));
	assert!(!should_retry(&bad_key));
	assert!(!should_retry(&bad_key.context("LLM description batch")));

	let api_error: anyhow::Error = ProviderError::ApiError {
		provider: "openrouter".into(),
		status: 403,
		message: "no access".into(),
	}
	.into();
	assert!(!should_retry(&api_error));
}

#[test]
fn transient_failures_are_retried() {
	assert!(should_retry(&anyhow::anyhow!(
		"OpenAI API error 429 Too Many Requests: slow down"
	)));
	assert!(should_retry(&anyhow::anyhow!("Anthropic API error 529 : overloaded")));
	assert!(should_retry(&anyhow::anyhow!(
		"Anthropic API error 503 Service Unavailable"
	)));
	// No status: connection reset, timeout, unparseable JSON. Unchanged behaviour.
	assert!(should_retry(&anyhow::anyhow!(
		"error sending request: connection reset"
	)));
	assert!(should_retry(&anyhow::anyhow!("LLM returned unparseable JSON: <html>")));
}

#[test]
fn status_parsing_ignores_lookalikes() {
	assert_eq!(status_from_message("API error 4000"), None);
	assert_eq!(status_from_message("API error abc"), None);
	assert_eq!(status_from_message("something else 400"), None);
}

#[test]
fn retry_budget_refuses_once_spent() {
	// A separate budget, so this test does not drain the process-wide one.
	let budget = RetrySpend::new(0.10);
	assert!(budget.try_retry(0.08).is_ok());
	assert!(budget.try_retry(0.08).is_err());
	budget.record_spend(1.0); // 10% of real spend joins the allowance
	assert!(budget.try_retry(0.08).is_ok());
}
