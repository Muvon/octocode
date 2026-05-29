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

//! Helpers for building safe LanceDB / DataFusion filter predicates.
//!
//! LanceDB evaluates `only_if` / `delete` / `update` predicates as DataFusion
//! SQL. String literals are single-quoted, and a single quote inside the value
//! must be doubled (`''`) — standard SQL escaping. Without this, any value
//! containing an apostrophe (file paths like `src/it's.rs`, symbol names, graph
//! node IDs) produces a malformed predicate. In the best case the query errors;
//! in the worst case (a `delete`) it silently matches nothing and leaves stale
//! rows in the database. Every predicate built from a runtime string MUST route
//! its values through [`escape_single_quotes`].

/// Escape a string for safe interpolation inside a single-quoted SQL literal by
/// doubling embedded single quotes. The returned value is the *inner* content —
/// callers still wrap it in quotes, e.g. `format!("path = '{}'", escape_single_quotes(p))`.
pub fn escape_single_quotes(value: &str) -> String {
	value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn no_quotes_unchanged() {
		assert_eq!(escape_single_quotes("src/main.rs"), "src/main.rs");
	}

	#[test]
	fn single_quote_is_doubled() {
		assert_eq!(escape_single_quotes("src/it's.rs"), "src/it''s.rs");
	}

	#[test]
	fn multiple_quotes_all_doubled() {
		assert_eq!(escape_single_quotes("a'b'c"), "a''b''c");
		assert_eq!(escape_single_quotes("''"), "''''");
	}

	#[test]
	fn empty_string() {
		assert_eq!(escape_single_quotes(""), "");
	}

	#[test]
	fn produces_balanced_predicate() {
		// A path with an apostrophe must yield a syntactically valid predicate.
		let p = "weird/o'brien.rs";
		let predicate = format!("path = '{}'", escape_single_quotes(p));
		assert_eq!(predicate, "path = 'weird/o''brien.rs'");
	}
}
