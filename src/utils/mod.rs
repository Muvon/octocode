// Copyright 2025 Muvon Un Limited
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

//! Shared utilities for the Octocode application

pub mod diff_chunker;
pub mod path;

/// Largest prefix of `s` that fits in `max_bytes` and ends on a UTF-8 char
/// boundary, so byte-budget truncation can never split a character (which
/// would panic when slicing).
pub fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> &str {
	let mut end = max_bytes.min(s.len());
	while end > 0 && !s.is_char_boundary(end) {
		end -= 1;
	}
	&s[..end]
}

#[cfg(test)]
mod tests {
	#[test]
	fn truncate_at_char_boundary_respects_utf8() {
		let s = "aé啊"; // 1 + 2 + 3 bytes
		assert_eq!(super::truncate_at_char_boundary(s, 0), "");
		assert_eq!(super::truncate_at_char_boundary(s, 1), "a");
		assert_eq!(super::truncate_at_char_boundary(s, 2), "a");
		assert_eq!(super::truncate_at_char_boundary(s, 3), "aé");
		assert_eq!(super::truncate_at_char_boundary(s, 5), "aé");
		assert_eq!(super::truncate_at_char_boundary(s, 6), s);
		assert_eq!(super::truncate_at_char_boundary(s, 100), s);
	}
}
