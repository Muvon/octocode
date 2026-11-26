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
fn pooling_weights_each_part_by_its_source_token_count() {
	let vector = pool(vec![(vec![1.0, 0.0], 2), (vec![0.0, 1.0], 1)]).unwrap();
	assert!((vector[0] - 0.8944272).abs() < 1e-6);
	assert!((vector[1] - 0.4472136).abs() < 1e-6);
}

#[test]
fn pooling_rejects_cancelling_vectors_instead_of_storing_nan() {
	assert!(pool(vec![(vec![1.0, 0.0], 1), (vec![-1.0, 0.0], 1)]).is_err());
}

#[test]
fn a_budget_smaller_than_a_prefixed_character_is_rejected() {
	assert!(split_input("🦀", 0, &InputType::Query, 1).is_err());
}

#[test]
fn a_multibyte_leading_character_can_be_split_from_a_short_ascii_tail() {
	let parts = split_input("🦀a", 0, &InputType::None, 3).unwrap();
	assert_eq!(
		parts.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(),
		vec!["🦀", "a"]
	);
}
