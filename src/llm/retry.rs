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

//! Which failed model calls are worth sending again, and what retries may cost.
//!
//! octolib already retries 429 and 5xx inside every provider call. What reaches
//! octocode's own retry loops is either a transient failure octolib gave up on,
//! or a 4xx it deliberately did not retry (bad request, bad key, no access,
//! context too long). Sending a 400 or 401 again fails the same way and only
//! adds 5s + 10s + 20s of sleep before the user sees the real error.
//!
//! [`should_retry`] tells the two apart, using the status classification from
//! `llm-resilience`. [`RETRY_BUDGET`] caps what retries of chat calls may add
//! to the bill: a floor of $0.50 per hour plus 10% of the real spend reported
//! in `usage.cost`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

use llm_resilience::{retryable_status, RetrySpend};
use octolib::errors::ProviderError;
use octolib::llm::retry::RetryableStatusError;

/// Process-wide retry budget in USD for chat completions.
pub static RETRY_BUDGET: LazyLock<RetrySpend> = LazyLock::new(|| RetrySpend::new(0.50));

/// HTTP status carried by a provider error, if any.
///
/// Typed errors are read first (`RetryableStatusError`, `ProviderError::ApiError`).
/// Providers report other non-2xx answers as `"<Provider> API error <status> ..."`,
/// so the status is parsed from that text as a fallback.
pub fn error_status(err: &anyhow::Error) -> Option<u16> {
	for cause in err.chain() {
		if let Some(e) = cause.downcast_ref::<RetryableStatusError>() {
			return Some(e.status);
		}
		if let Some(ProviderError::ApiError { status, .. }) = cause.downcast_ref::<ProviderError>() {
			return Some(*status);
		}
	}
	err.chain().find_map(|cause| status_from_message(&cause.to_string()))
}

fn status_from_message(message: &str) -> Option<u16> {
	let rest = &message[message.find("API error ")? + "API error ".len()..];
	let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
	match digits.len() {
		3 => digits.parse().ok(),
		_ => None,
	}
}

/// Whether a failed call is worth another attempt.
///
/// An error with an HTTP status is retried only for 408, 409, 425, 429, 5xx
/// and 529. Errors without a status (connection resets, timeouts, unparseable
/// output) are retried as before.
pub fn should_retry(err: &anyhow::Error) -> bool {
	error_status(err).is_none_or(retryable_status)
}

/// Mean cost of the chat calls seen so far, used as the estimate for a retry.
/// Zero until a call has reported its cost, so a provider without pricing
/// data is never refused a retry.
pub fn estimated_call_cost() -> f64 {
	let calls = COST_SAMPLES.load(Ordering::Relaxed);
	if calls == 0 {
		return 0.0;
	}
	TOTAL_COST_MICROS.load(Ordering::Relaxed) as f64 / 1e6 / calls as f64
}

static COST_SAMPLES: AtomicU64 = AtomicU64::new(0);
static TOTAL_COST_MICROS: AtomicU64 = AtomicU64::new(0);

/// Record the real cost of a completed chat call (`usage.cost`).
pub fn record_cost(cost: f64) {
	if cost.is_finite() && cost > 0.0 {
		RETRY_BUDGET.record_spend(cost);
		COST_SAMPLES.fetch_add(1, Ordering::Relaxed);
		TOTAL_COST_MICROS.fetch_add((cost * 1e6) as u64, Ordering::Relaxed);
	}
}

/// Decide whether to retry `err`, booking the retry against the budget.
/// Returns `false` (and logs why) for a non-retryable status or when the
/// budget refuses the retry.
pub fn allow_retry(err: &anyhow::Error) -> bool {
	if !should_retry(err) {
		tracing::warn!("LLM call failed with a non-retryable error, not retrying: {}", err);
		return false;
	}
	match RETRY_BUDGET.try_retry(estimated_call_cost()) {
		Ok(()) => true,
		Err(refused) => {
			tracing::warn!("LLM call failed, not retrying: {}", refused);
			false
		}
	}
}

#[cfg(test)]
#[path = "retry_tests.rs"]
mod retry_tests;
