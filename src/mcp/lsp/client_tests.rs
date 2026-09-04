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
	use super::super::{LspClient, ProgressState};
	use crate::mcp::lsp::protocol::{
		LspIncomingNotification, LspIncomingRequest, LspMessage, LspNotification,
	};
	use serde_json::{json, Value};
	use std::collections::HashMap;
	use std::sync::Arc;
	use std::time::Duration;
	use tokio::io::BufReader;
	use tokio::process::{Child, ChildStdout};
	use tokio::sync::RwLock;

	/// Every await on a child process is bounded so a broken pipe fails the test
	/// instead of hanging the suite.
	const LIMIT: Duration = Duration::from_secs(10);

	type Progress = Arc<RwLock<HashMap<String, ProgressState>>>;

	/// Spawn a Python one-shot writer and hand back its stdout as the reader the
	/// framing parser expects. The `Child` must stay alive for the pipe to stay
	/// readable, so it is returned alongside.
	async fn python_stream(program: &str) -> (Child, BufReader<ChildStdout>) {
		let mut child = tokio::process::Command::new("python3")
			.arg("-c")
			.arg(program)
			.stdin(std::process::Stdio::null())
			.stdout(std::process::Stdio::piped())
			.stderr(std::process::Stdio::null())
			.kill_on_drop(true)
			.spawn()
			.expect("python3 must be available for the LSP framing tests");
		let stdout = child.stdout.take().expect("piped stdout");
		(child, BufReader::new(stdout))
	}

	async fn read_one(program: &str) -> anyhow::Result<Option<LspMessage>> {
		let (_child, mut reader) = python_stream(program).await;
		tokio::time::timeout(LIMIT, LspClient::read_lsp_message(&mut reader))
			.await
			.expect("reading a framed message must not hang")
	}

	fn progress_state() -> (Progress, Arc<RwLock<bool>>) {
		(
			Arc::new(RwLock::new(HashMap::new())),
			Arc::new(RwLock::new(false)),
		)
	}

	fn notification(method: &str, params: Option<Value>) -> LspIncomingNotification {
		LspIncomingNotification {
			jsonrpc: "2.0".to_string(),
			method: method.to_string(),
			params,
		}
	}

	// --- Content-Length framing ---

	#[tokio::test]
	async fn a_framed_message_is_parsed_and_unknown_headers_are_ignored() {
		let message = read_one(
			r#"
import sys
body = b'{"jsonrpc":"2.0","id":7,"result":{"ok":true}}'
sys.stdout.buffer.write(b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n")
sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body))
sys.stdout.buffer.write(body)
sys.stdout.buffer.flush()
"#,
		)
		.await
		.expect("a well-framed message parses")
		.expect("a message, not EOF");

		match message {
			LspMessage::Response(response) => {
				assert_eq!(response.id, Some(7));
				assert_eq!(response.result, Some(json!({"ok": true})));
				assert!(response.error.is_none());
			}
			_ => panic!("a payload with an id and a result is a response"),
		}
	}

	#[tokio::test]
	async fn a_closed_stream_reports_end_of_input_rather_than_an_error() {
		let end = read_one("pass").await.expect("EOF is not an error");
		assert!(end.is_none());
	}

	#[tokio::test]
	async fn a_header_block_without_a_content_length_is_rejected() {
		let error = read_one(
			r#"
import sys
sys.stdout.buffer.write(b"X-Trace: 1\r\n\r\n")
sys.stdout.buffer.flush()
"#,
		)
		.await
		.expect_err("a message with no length cannot be read");
		assert!(
			error.to_string().contains("Content-Length"),
			"unexpected error: {error}"
		);
	}

	#[tokio::test]
	async fn a_non_numeric_content_length_is_rejected() {
		let error = read_one(
			r#"
import sys
sys.stdout.buffer.write(b"Content-Length: not-a-number\r\n\r\n")
sys.stdout.buffer.flush()
"#,
		)
		.await
		.expect_err("a malformed length cannot be parsed");
		assert!(
			error.to_string().contains("invalid digit"),
			"unexpected error: {error}"
		);
	}

	#[tokio::test]
	async fn a_body_shorter_than_its_declared_length_is_rejected() {
		let error = read_one(
			r#"
import sys
sys.stdout.buffer.write(b"Content-Length: 500\r\n\r\n{}")
sys.stdout.buffer.flush()
"#,
		)
		.await
		.expect_err("a truncated body cannot be read");
		let io_error = error
			.downcast_ref::<std::io::Error>()
			.unwrap_or_else(|| panic!("expected an io error, got: {error}"));
		assert_eq!(io_error.kind(), std::io::ErrorKind::UnexpectedEof);
	}

	#[tokio::test]
	async fn a_body_that_is_not_json_is_rejected() {
		let error = read_one(
			r#"
import sys
body = b'this is not json'
sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body))
sys.stdout.buffer.write(body)
sys.stdout.buffer.flush()
"#,
		)
		.await
		.expect_err("a non-JSON body cannot be deserialized");
		assert!(
			error.to_string().contains("expected"),
			"unexpected error: {error}"
		);
	}

	// --- Progress tracking ---

	#[tokio::test]
	async fn a_progress_run_is_tracked_from_begin_to_end() {
		let (states, indexing) = progress_state();

		LspClient::handle_progress_notification(
			&json!({"token": "t1", "value": {"kind": "begin", "title": "Indexing", "message": "start", "percentage": 0}}),
			&states,
			&indexing,
		)
		.await
		.unwrap();
		{
			let snapshot = states.read().await;
			let state = snapshot.get("t1").expect("begin registers the token");
			assert_eq!(state.token, "t1");
			assert_eq!(state.title, "Indexing");
			assert_eq!(state.message.as_deref(), Some("start"));
			assert_eq!(state.percentage, Some(0));
			assert!(!state.is_complete);
		}

		LspClient::handle_progress_notification(
			&json!({"token": "t1", "value": {"kind": "report", "message": "half", "percentage": 50}}),
			&states,
			&indexing,
		)
		.await
		.unwrap();
		{
			let snapshot = states.read().await;
			let state = snapshot.get("t1").expect("report keeps the token");
			assert_eq!(state.message.as_deref(), Some("half"));
			assert_eq!(state.percentage, Some(50));
			// A report never changes the title recorded by begin.
			assert_eq!(state.title, "Indexing");
		}

		LspClient::handle_progress_notification(
			&json!({"token": "t1", "value": {"kind": "end", "message": "done"}}),
			&states,
			&indexing,
		)
		.await
		.unwrap();
		assert!(
			states.read().await.is_empty(),
			"a completed run is cleaned up"
		);
		assert!(
			*indexing.read().await,
			"a run titled 'Indexing' marks indexing complete"
		);
	}

	#[tokio::test]
	async fn only_indexing_shaped_titles_mark_indexing_complete() {
		for (title, expected) in [
			("Indexing", true),
			("Loading workspace", true),
			("Analyzing crates", true),
			("Formatting document", false),
		] {
			let (states, indexing) = progress_state();
			LspClient::handle_progress_notification(
				&json!({"token": "t", "value": {"kind": "begin", "title": title}}),
				&states,
				&indexing,
			)
			.await
			.unwrap();
			LspClient::handle_progress_notification(
				&json!({"token": "t", "value": {"kind": "end"}}),
				&states,
				&indexing,
			)
			.await
			.unwrap();
			assert_eq!(*indexing.read().await, expected, "title: {title}");
		}
	}

	#[tokio::test]
	async fn a_report_or_end_for_an_unknown_token_is_ignored() {
		let (states, indexing) = progress_state();

		LspClient::handle_progress_notification(
			&json!({"token": "ghost", "value": {"kind": "report", "percentage": 10}}),
			&states,
			&indexing,
		)
		.await
		.unwrap();
		LspClient::handle_progress_notification(
			&json!({"token": "ghost", "value": {"kind": "end"}}),
			&states,
			&indexing,
		)
		.await
		.unwrap();

		assert!(states.read().await.is_empty());
		assert!(!*indexing.read().await);
	}

	#[tokio::test]
	async fn a_begin_without_a_title_falls_back_to_unknown() {
		let (states, indexing) = progress_state();
		LspClient::handle_progress_notification(
			&json!({"value": {"kind": "begin"}}),
			&states,
			&indexing,
		)
		.await
		.unwrap();

		let snapshot = states.read().await;
		// A missing token is tracked under the empty string, not dropped.
		let state = snapshot.get("").expect("token defaults to empty");
		assert_eq!(state.title, "Unknown");
		assert!(state.message.is_none());
		assert!(state.percentage.is_none());
	}

	#[tokio::test]
	async fn a_malformed_progress_notification_is_an_error() {
		let (states, indexing) = progress_state();

		let no_value =
			LspClient::handle_progress_notification(&json!({"token": "t"}), &states, &indexing)
				.await
				.expect_err("a progress payload must carry a value");
		assert!(no_value.to_string().contains("'value'"), "{no_value}");

		let no_kind = LspClient::handle_progress_notification(
			&json!({"token": "t", "value": {"title": "Indexing"}}),
			&states,
			&indexing,
		)
		.await
		.expect_err("a progress value must carry a kind");
		assert!(no_kind.to_string().contains("'kind'"), "{no_kind}");

		// An unrecognised kind is tolerated and changes nothing.
		LspClient::handle_progress_notification(
			&json!({"token": "t", "value": {"kind": "sideways"}}),
			&states,
			&indexing,
		)
		.await
		.unwrap();
		assert!(states.read().await.is_empty());
	}

	#[tokio::test]
	async fn progress_notifications_reach_the_tracker_through_the_dispatcher() {
		let (states, indexing) = progress_state();
		LspClient::handle_notification(
			&notification(
				"$/progress",
				Some(json!({"token": "t9", "value": {"kind": "begin", "title": "Indexing"}})),
			),
			&states,
			&indexing,
		)
		.await;
		assert!(states.read().await.contains_key("t9"));
	}

	#[tokio::test]
	async fn notifications_the_client_only_logs_leave_the_tracker_untouched() {
		let (states, indexing) = progress_state();

		// A `$/progress` that cannot be parsed is swallowed, not propagated.
		for note in [
			notification("$/progress", Some(json!({"token": "t"}))),
			notification("$/progress", None),
			notification("rust-analyzer/serverStatus", Some(json!({"health": "ok"}))),
			notification(
				"window/logMessage",
				Some(json!({"type": 1, "message": "boom"})),
			),
			notification("window/logMessage", Some(json!({"garbage": true}))),
			notification("window/logMessage", None),
			notification(
				"window/showMessage",
				Some(json!({"type": 3, "message": "hello"})),
			),
			notification("window/showMessage", Some(json!({"garbage": true}))),
			notification("textDocument/publishDiagnostics", Some(json!({"uri": "x"}))),
		] {
			LspClient::handle_notification(&note, &states, &indexing).await;
		}

		assert!(states.read().await.is_empty());
		assert!(!*indexing.read().await);
	}

	/// `handle_incoming_request` only logs; it has no return value and touches no
	/// client state, so this test just pins that every arm is reachable.
	#[tokio::test]
	async fn every_incoming_request_arm_is_reachable() {
		for method in [
			"window/workDoneProgress/create",
			"window/showMessageRequest",
			"workspace/configuration",
		] {
			LspClient::handle_incoming_request(&LspIncomingRequest {
				jsonrpc: "2.0".to_string(),
				id: 1,
				method: method.to_string(),
				params: Some(json!({})),
			})
			.await;
		}
	}

	// --- Readiness ---

	#[tokio::test]
	async fn readiness_follows_active_progress_until_indexing_completes() {
		let client = LspClient::new("unused".to_string(), std::env::temp_dir());

		// No progress reported yet — nothing is known to be in flight.
		assert!(client.is_ready_for_requests().await);
		assert!(!client.is_indexing_complete().await);

		LspClient::handle_progress_notification(
			&json!({"token": "t", "value": {"kind": "begin", "title": "Building"}}),
			&client.progress_states,
			&client.indexing_complete,
		)
		.await
		.unwrap();
		assert!(
			!client.is_ready_for_requests().await,
			"an in-flight progress run blocks readiness"
		);

		// A completed index short-circuits the active-progress check.
		*client.indexing_complete.write().await = true;
		assert!(client.is_ready_for_requests().await);
		assert!(client.is_indexing_complete().await);
	}

	// --- Process lifecycle ---

	#[tokio::test]
	async fn an_empty_command_cannot_start_a_server() {
		let client = LspClient::new(String::new(), std::env::temp_dir());
		let error = client
			.start()
			.await
			.expect_err("there is no program to run");
		assert!(error.to_string().contains("Empty LSP command"), "{error}");
		assert!(!client.is_connected());
	}

	#[tokio::test]
	async fn a_missing_binary_reports_the_program_name() {
		let client = LspClient::new(
			"octocode-no-such-language-server --stdio".to_string(),
			std::env::temp_dir(),
		);
		let error = client.start().await.expect_err("the binary does not exist");
		assert!(
			error
				.to_string()
				.contains("octocode-no-such-language-server"),
			"{error}"
		);
		assert!(!client.is_connected());
	}

	#[tokio::test]
	async fn sending_before_the_server_starts_is_an_error() {
		let client = LspClient::new("unused".to_string(), std::env::temp_dir());
		let error = client
			.send_notification(LspNotification::initialized().unwrap())
			.await
			.expect_err("there is no stdin to write to");
		assert!(error.to_string().contains("not started"), "{error}");
	}

	#[tokio::test]
	async fn stopping_a_running_server_closes_stdin_and_drops_the_connection() {
		// A server that blocks on stdin stays alive until it is killed.
		let client = LspClient::new(
			"python3 -c __import__(\"sys\").stdin.read()".to_string(),
			std::env::temp_dir(),
		);
		client.start().await.expect("the child should start");
		assert!(client.is_connected());

		client.stop().await.unwrap();
		assert!(!client.is_connected());
		assert!(client.process.lock().await.is_none(), "the child is reaped");

		// stdin is gone, so further traffic is refused rather than silently lost.
		let error = client
			.send_notification(LspNotification::initialized().unwrap())
			.await
			.expect_err("a stopped client has no stdin");
		assert!(error.to_string().contains("not started"), "{error}");

		// Stopping twice is harmless.
		client.stop().await.unwrap();
	}

	#[tokio::test]
	async fn a_cloned_client_shares_the_progress_and_connection_state() {
		let client = LspClient::new("unused".to_string(), std::env::temp_dir());
		let clone = client.clone();

		LspClient::handle_progress_notification(
			&json!({"token": "shared", "value": {"kind": "begin", "title": "Indexing"}}),
			&clone.progress_states,
			&clone.indexing_complete,
		)
		.await
		.unwrap();

		assert!(client.progress_states.read().await.contains_key("shared"));
		assert!(!client.is_ready_for_requests().await);
		assert!(!clone.is_ready_for_requests().await);
	}

	#[cfg(unix)]
	#[tokio::test]
	async fn reaps_lsp_process_that_exits_on_its_own() {
		let client = LspClient::new("/usr/bin/true".to_string(), std::env::temp_dir());
		client.start().await.expect("LSP child should start");

		tokio::time::timeout(std::time::Duration::from_secs(2), async {
			loop {
				if client.process.lock().await.is_none() {
					break;
				}
				tokio::task::yield_now().await;
			}
		})
		.await
		.expect("exited LSP child should be reaped promptly");

		assert!(!client.is_connected());
	}
}
