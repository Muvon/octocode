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
use rmcp::model::{CallToolResult, ClientCapabilities, ContentBlock, ProtocolVersion, RequestId};
use rmcp::Peer;
use serde_json::{json, Value};
use tempfile::TempDir;

/// The started server in these tests: one tool, and every call answers "root".
#[derive(Clone)]
struct RootHandler;

impl ServerHandler for RootHandler {
	async fn list_tools(
		&self,
		_request: Option<PaginatedRequestParams>,
		context: RequestContext<RoleServer>,
	) -> Result<ListToolsResult, ErrorData> {
		let schema = json!({"type": "object", "properties": {}});
		let tool = Tool::new(
			"root_tool".to_string(),
			"served directory".to_string(),
			Arc::new(schema.as_object().unwrap().clone()),
		);
		Ok(list_tools_result(vec![tool], &context))
	}

	async fn call_tool(
		&self,
		_request: CallToolRequestParams,
		_context: RequestContext<RoleServer>,
	) -> Result<CallToolResponse, ErrorData> {
		Ok(CallToolResult::success(vec![ContentBlock::text("root")]).into())
	}
}

/// A served repository plus, beside it, another repository, a directory with
/// two repositories and a plain directory.
struct Layout {
	_dir: TempDir,
	root: PathBuf,
	repo: PathBuf,
	parent: PathBuf,
	plain: PathBuf,
}

fn layout() -> Layout {
	let dir = TempDir::new().unwrap();
	let base = dir.path().canonicalize().unwrap();
	let root = base.join("served");
	let repo = base.join("repo");
	let parent = base.join("parent");
	let plain = base.join("plain");
	for git_dir in [
		root.join(".git"),
		repo.join(".git"),
		parent.join("one/.git"),
		parent.join("two/.git"),
	] {
		std::fs::create_dir_all(git_dir).unwrap();
	}
	std::fs::create_dir_all(&plain).unwrap();
	std::fs::write(repo.join("lib.rs"), "pub fn only_in_repo() {}\n").unwrap();
	Layout {
		_dir: dir,
		root,
		repo,
		parent,
		plain,
	}
}

/// Session routing with the in-process indexer off, so per-repo handlers start
/// no watcher or indexing threads.
fn router(root: &Path) -> SessionRouter<RootHandler> {
	let mut config = Config::default();
	config.index.mcp_index = false;
	SessionRouter::new(RootHandler, Sessions::new(config, root.to_path_buf(), false, false))
}

fn context(peer: &Peer<RoleServer>, session: Option<Value>) -> RequestContext<RoleServer> {
	let mut context = RequestContext::new(RequestId::Number(1), peer.clone());
	context.meta.set_protocol_version(ProtocolVersion::V_2026_07_28);
	if let Some(session) = session {
		let capabilities: ClientCapabilities =
			serde_json::from_value(json!({"experimental": {"session": session}})).unwrap();
		context.meta.set_client_capabilities(capabilities);
	}
	context
}

fn in_dir(peer: &Peer<RoleServer>, workdir: &Path) -> RequestContext<RoleServer> {
	context(peer, Some(json!({"session_id": "s", "workdir": workdir})))
}

fn names(result: &ListToolsResult) -> Vec<String> {
	result.tools.iter().map(|tool| tool.name.to_string()).collect()
}

#[test]
fn session_directories_resolve_like_auto_mode() {
	let layout = layout();
	let src = layout.root.join("src");
	std::fs::create_dir(&src).unwrap();
	let resolve = |root: &Path, root_spans_subdirs: bool, path: &Path| {
		resolve_target(
			root,
			root_spans_subdirs,
			path,
			false,
			McpServer::new_repo_core(Config::default(), root.to_path_buf()).list_tool_defs(),
		)
		.unwrap()
	};
	let single = |path: &Path| resolve(&layout.root, true, path);

	assert!(matches!(single(&layout.root), Target::Root));
	assert!(matches!(single(&src), Target::Root));
	assert!(matches!(single(&layout.root.join("missing")), Target::Root));
	assert!(matches!(single(&layout.repo), Target::Repo(path) if path == layout.repo));
	assert!(matches!(single(&layout.plain), Target::Repo(path) if path == layout.plain));
	let Target::Projects(projects) = single(&layout.parent) else {
		panic!("a directory holding repositories must offer them as projects");
	};
	assert_eq!(
		projects.tools()[0].input_schema["properties"]["project"]["enum"],
		json!(["one", "two"])
	);

	// A multi-repository root serves only its repositories by `project`: a session
	// inside one of them gets that repository directly.
	let one = layout.parent.join("one");
	assert!(matches!(resolve(&layout.parent, false, &layout.parent), Target::Root));
	assert!(matches!(resolve(&layout.parent, false, &one), Target::Repo(path) if path == one));
}

#[tokio::test]
async fn the_session_directory_is_read_from_the_client_capabilities() {
	let (transport, _client) = tokio::io::duplex(4096);
	let service = rmcp::service::serve_directly(RootHandler, transport, None);
	let peer = service.peer();

	assert_eq!(session_workdir(&context(peer, None)), None);
	assert_eq!(
		session_workdir(&context(peer, Some(json!({"session_id": "s"})))),
		Some(None)
	);
	// Octomind 0.57.1 and older send an empty workdir when the session has no anchor.
	assert_eq!(
		session_workdir(&context(peer, Some(json!({"workdir": ""})))),
		Some(None)
	);
	assert_eq!(
		session_workdir(&in_dir(peer, Path::new("/work/repo"))),
		Some(Some(PathBuf::from("/work/repo")))
	);
}

#[tokio::test]
async fn each_session_lists_the_tools_of_its_own_directory() {
	let layout = layout();
	let router = router(&layout.root);
	let (transport, _client) = tokio::io::duplex(4096);
	let service = rmcp::service::serve_directly(RootHandler, transport, None);
	let peer = service.peer();

	// No session context: the started server's list, shared as before.
	let shared = router.list_tools(None, context(peer, None)).await.unwrap();
	assert_eq!(names(&shared), ["root_tool"]);
	assert_eq!(shared.cache_scope, Some(CacheScope::Public));

	let at_root = router.list_tools(None, in_dir(peer, &layout.root)).await.unwrap();
	assert_eq!(names(&at_root), ["root_tool"]);
	assert_eq!(at_root.cache_scope, Some(CacheScope::Private));

	let in_repo = router.list_tools(None, in_dir(peer, &layout.repo)).await.unwrap();
	assert!(names(&in_repo).contains(&"semantic_search".to_string()));
	assert!(in_repo.tools[0].input_schema["properties"].get("project").is_none());
	assert_eq!(in_repo.cache_scope, Some(CacheScope::Private));

	let in_parent = router.list_tools(None, in_dir(peer, &layout.parent)).await.unwrap();
	assert_eq!(
		in_parent.tools[0].input_schema["properties"]["project"]["enum"],
		json!(["one", "two"])
	);
	assert_eq!(in_parent.cache_scope, Some(CacheScope::Private));
}

#[tokio::test]
async fn calls_reach_the_handler_for_the_session_directory() {
	let layout = layout();
	let router = router(&layout.root);
	let (transport, _client) = tokio::io::duplex(4096);
	let service = rmcp::service::serve_directly(RootHandler, transport, None);
	let peer = service.peer();
	let text = |response: CallToolResponse| {
		let CallToolResponse::Complete(result) = response else {
			panic!("expected a completed call");
		};
		serde_json::to_string(&result.content).unwrap()
	};

	let at_root = router
		.call_tool(CallToolRequestParams::new("root_tool"), in_dir(peer, &layout.root))
		.await
		.unwrap();
	assert!(text(at_root).contains("root"));

	let signatures = CallToolRequestParams::new("view_signatures")
		.with_arguments(json!({"files": ["lib.rs"]}).as_object().unwrap().clone());
	let in_repo = router.call_tool(signatures, in_dir(peer, &layout.repo)).await.unwrap();
	let in_repo = text(in_repo);
	assert!(in_repo.contains("only_in_repo"), "{in_repo}");

	let Err(error) = router
		.call_tool(
			CallToolRequestParams::new("view_signatures"),
			in_dir(peer, &layout.parent),
		)
		.await
	else {
		panic!("a call without a project must not run in a directory of projects");
	};
	assert!(
		error.message.contains("Missing required 'project' argument"),
		"{}",
		error.message
	);
}
