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
	use serde_json::json;

	fn valid_response() -> serde_json::Value {
		json!({
			"summary": {"total_files": 2, "total_issues": 1, "overall_score": 80},
			"issues": [{
				"severity": "HIGH",
				"category": "Security",
				"title": "Hardcoded secret",
				"description": "A token is embedded in the source.",
				"file_path": "src/a.rs",
				"line_number": 12
			}],
			"recommendations": ["Move the token to the environment"]
		})
	}

	#[test]
	fn file_types_are_counted_by_extension() {
		let summary = analyze_file_types(&[
			"src/a.rs".to_string(),
			"src/b.rs".to_string(),
			"web/app.ts".to_string(),
			"Makefile".to_string(),
		]);
		assert!(summary.contains("rs: 2"), "{summary}");
		assert!(summary.contains("ts: 1"), "{summary}");
		// An extensionless file contributes nothing.
		assert!(!summary.contains("Makefile"), "{summary}");
	}

	#[test]
	fn no_files_produce_an_empty_type_summary() {
		assert_eq!(analyze_file_types(&[]), "");
	}

	#[test]
	fn the_prompt_carries_the_diff_statistics_and_focus() {
		let prompt = create_review_prompt(
			"diff --git a/src/a.rs b/src/a.rs",
			2,
			10,
			3,
			"rs: 2",
			"src/a.rs | 5 +++--",
			"Focus on security.",
		);
		assert!(prompt.contains("Files changed: 2"), "{prompt}");
		assert!(prompt.contains("Lines added: 10"));
		assert!(prompt.contains("Lines deleted: 3"));
		assert!(prompt.contains("File types: rs: 2"));
		assert!(prompt.contains("src/a.rs | 5 +++--"));
		assert!(prompt.contains("Focus on security."));
	}

	#[test]
	fn missing_diff_stats_fall_back_to_a_placeholder() {
		let prompt = create_review_prompt("diff", 1, 0, 0, "rs: 1", "   ", "");
		assert!(prompt.contains("No stats available"), "{prompt}");
	}

	#[test]
	fn a_well_formed_response_parses_into_a_review() {
		let review =
			parse_review_response(&valid_response(), 2, &["src/a.rs".to_string()]).unwrap();
		assert_eq!(review.summary.total_files, 2);
		assert_eq!(review.summary.overall_score, 80);
		assert_eq!(review.issues.len(), 1);
		assert_eq!(review.issues[0].severity, "HIGH");
		assert_eq!(review.issues[0].file_path.as_deref(), Some("src/a.rs"));
		assert_eq!(review.recommendations.len(), 1);
	}

	#[test]
	fn a_response_missing_required_fields_becomes_a_fallback_review() {
		for incomplete in [
			json!({"issues": [], "recommendations": []}),
			json!({"summary": {}, "recommendations": []}),
			json!({"summary": {}, "issues": []}),
			json!("not even an object"),
		] {
			let review = parse_review_response(&incomplete, 3, &[]).unwrap();
			assert_eq!(review.summary.total_files, 3);
			assert_eq!(review.issues[0].title, "Review Analysis Incomplete");
		}
	}

	#[test]
	fn a_structurally_valid_but_untyped_response_also_falls_back() {
		// The shape checks pass but the field types are wrong, so deserialization
		// fails and the fallback keeps the command usable.
		let bad_types = json!({
			"summary": {"total_files": "two", "total_issues": 1, "overall_score": 80},
			"issues": [],
			"recommendations": []
		});
		let review = parse_review_response(&bad_types, 7, &[]).unwrap();
		assert_eq!(review.summary.total_files, 7);
		assert_eq!(review.summary.overall_score, 75);
	}

	#[test]
	fn the_severity_filter_keeps_issues_at_or_above_the_threshold() {
		assert!(should_show_issue("CRITICAL", "medium"));
		assert!(should_show_issue("HIGH", "medium"));
		assert!(should_show_issue("MEDIUM", "medium"));
		assert!(!should_show_issue("LOW", "medium"));

		assert!(should_show_issue("CRITICAL", "critical"));
		assert!(!should_show_issue("HIGH", "critical"));
		assert!(should_show_issue("LOW", "low"));
	}

	#[test]
	fn an_unrecognised_severity_or_filter_shows_the_issue() {
		assert!(should_show_issue("HIGH", "everything"));
		assert!(should_show_issue("unknown", "medium"));
	}

	#[test]
	fn rendering_a_review_covers_the_filtered_and_unfiltered_paths() {
		let review = parse_review_response(&valid_response(), 2, &[]).unwrap();
		display_review_results(&review, "medium");
		display_review_results(&review, "critical");

		let empty = create_fallback_review(0, &[], &json!({})).unwrap();
		display_review_results(&empty, "low");
	}
}
