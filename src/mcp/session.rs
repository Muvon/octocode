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

//! Per-session routing (`experimental.session.workdir` in the client capabilities).
//!
//! Octomind shares one connection to a server between all of its sessions and
//! names the calling session's directory on every request. Without routing,
//! every session would search the directory octocode was started in. A request
//! from another directory is served like `mcp --auto` would serve that
//! directory: its git repository, the repositories one level under it (with the
//! `project` argument), or the directory itself. Clients that send no session
//! context, and sessions in the served directory, reach the started server
//! unchanged.

use anyhow::Result;
use rmcp::{
	handler::server::ServerHandler,
	model::{
		CacheScope, CallToolRequestParams, CallToolResponse, ListToolsResult, PaginatedRequestParams, ServerConfig,
		Tool,
	},
	service::RequestContext,
	ErrorData, RoleServer,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

use crate::config::Config;
use crate::indexer;
use crate::mcp::multi::{has_child_repos, Projects, RepoPool};
use crate::mcp::server::{list_tools_result, McpServer};

#[cfg(test)]
#[path = "session_tests.rs"]
mod session_tests;

/// What serves a session's directory.
#[derive(Clone)]
enum Target {
	/// The started server: the session is in the served directory.
	Root,
	/// A single repository (or plain directory) other than the served one.
	Repo(PathBuf),
	/// The repositories one level under a directory, selected by `project`.
	Projects(Arc<Projects>),
}

/// Session routing state shared by every connection of one server process.
#[derive(Clone)]
pub struct Sessions {
	/// The served directory (`--path`), canonical.
	root: PathBuf,
	/// The started server serves the whole tree under `root` (a single
	/// repository), so sessions in its subdirectories stay on it. A
	/// multi-repository root serves only the repositories under it.
	root_spans_subdirs: bool,
	pool: RepoPool,
	/// Tool set of a per-repo handler, served to sessions in a single repository.
	repo_tools: Arc<Vec<Tool>>,
	/// Resolved target per session directory, as sent by the client.
	targets: Arc<Mutex<HashMap<PathBuf, Target>>>,
}

impl Sessions {
	/// Routing for a single-repository server serving `root`.
	pub fn new(config: Config, root: PathBuf, no_git: bool, debug: bool) -> Self {
		Self::build(root, true, RepoPool::new(config, no_git, debug))
	}

	/// Routing for a multi-repository server, sharing its `pool` so a repository
	/// reached both ways runs one watcher and indexer.
	pub(crate) fn with_pool(root: PathBuf, pool: RepoPool) -> Self {
		Self::build(root, false, pool)
	}

	fn build(root: PathBuf, root_spans_subdirs: bool, pool: RepoPool) -> Self {
		let repo_tools = McpServer::new_repo_core(pool.config.clone(), root.clone()).list_tool_defs();
		Self {
			root,
			root_spans_subdirs,
			pool,
			repo_tools: Arc::new(repo_tools),
			targets: Arc::new(Mutex::new(HashMap::new())),
		}
	}

	/// Drop per-repo handlers left idle by sessions. Call once per process.
	pub fn spawn_idle_cleanup(&self) {
		self.pool.spawn_idle_cleanup();
	}

	async fn target(&self, workdir: Option<PathBuf>) -> Result<Target, ErrorData> {
		let Some(workdir) = workdir else {
			return Ok(Target::Root);
		};
		if let Some(target) = self.targets.lock().await.get(&workdir) {
			return Ok(target.clone());
		}
		let root = self.root.clone();
		let root_spans_subdirs = self.root_spans_subdirs;
		let no_git = self.pool.no_git;
		let repo_tools = self.repo_tools.to_vec();
		let path = workdir.clone();
		let target =
			tokio::task::spawn_blocking(move || resolve_target(&root, root_spans_subdirs, &path, no_git, repo_tools))
				.await
				.map_err(|e| ErrorData::internal_error(format!("Failed to resolve session directory: {}", e), None))?
				.map_err(|e| {
					ErrorData::internal_error(
						format!("Failed to discover repositories in '{}': {}", workdir.display(), e),
						None,
					)
				})?;
		self.targets.lock().await.insert(workdir, target.clone());
		Ok(target)
	}
}

/// Serve `workdir` the way `mcp --auto` serves a directory: a git repository
/// root as one repository, a directory with repositories one level under it as
/// projects, anything else as a plain directory. The started server keeps the
/// served directory, and its subdirectories when it serves the whole tree.
fn resolve_target(
	root: &Path,
	root_spans_subdirs: bool,
	workdir: &Path,
	no_git: bool,
	repo_tools: Vec<Tool>,
) -> Result<Target> {
	let workdir = match workdir.canonicalize() {
		Ok(path) if path.is_dir() => path,
		_ => {
			debug!(
				"Session workdir '{}' is not a directory, serving the started directory",
				workdir.display()
			);
			return Ok(Target::Root);
		}
	};
	if workdir == root || (root_spans_subdirs && workdir.starts_with(root)) {
		return Ok(Target::Root);
	}
	if !indexer::git::is_git_repo_root(&workdir) && has_child_repos(&workdir, no_git) {
		return Ok(Target::Projects(Arc::new(Projects::discover(
			&workdir, no_git, repo_tools,
		)?)));
	}
	Ok(Target::Repo(workdir))
}

/// The calling session's directory: `None` when the client sends no session
/// context, `Some(None)` when it sends one without a usable directory.
fn session_workdir(context: &RequestContext<RoleServer>) -> Option<Option<PathBuf>> {
	let session = context.client_capabilities()?.experimental?.remove("session")?;
	let workdir = session
		.get("workdir")
		.and_then(|value| value.as_str())
		.filter(|workdir| !workdir.trim().is_empty())
		.map(PathBuf::from);
	Some(workdir)
}

/// A server handler behind per-session routing.
#[derive(Clone)]
pub struct SessionRouter<H> {
	inner: H,
	sessions: Sessions,
}

impl<H> SessionRouter<H> {
	pub fn new(inner: H, sessions: Sessions) -> Self {
		Self { inner, sessions }
	}
}

impl<H: ServerHandler> ServerHandler for SessionRouter<H> {
	fn get_info(&self) -> ServerConfig {
		self.inner.get_info()
	}

	async fn list_tools(
		&self,
		request: Option<PaginatedRequestParams>,
		context: RequestContext<RoleServer>,
	) -> Result<ListToolsResult, ErrorData> {
		let Some(workdir) = session_workdir(&context) else {
			return self.inner.list_tools(request, context).await;
		};
		let mut result = match self.sessions.target(workdir).await? {
			Target::Root => self.inner.list_tools(request, context).await?,
			Target::Repo(_) => list_tools_result(self.sessions.repo_tools.to_vec(), &context),
			Target::Projects(projects) => list_tools_result(projects.tools().to_vec(), &context),
		};
		// The list depends on the session's directory: never reuse it for another session.
		if result.cache_scope.is_some() {
			result.cache_scope = Some(CacheScope::Private);
		}
		Ok(result)
	}

	fn get_tool(&self, name: &str) -> Option<Tool> {
		self.inner.get_tool(name)
	}

	async fn call_tool(
		&self,
		mut request: CallToolRequestParams,
		context: RequestContext<RoleServer>,
	) -> Result<CallToolResponse, ErrorData> {
		let workdir = session_workdir(&context).flatten();
		let repo_path = match self.sessions.target(workdir).await? {
			Target::Root => return self.inner.call_tool(request, context).await,
			Target::Repo(repo_path) => repo_path,
			Target::Projects(projects) => projects.route(&mut request)?,
		};
		let server = self.sessions.pool.get(&repo_path).await?;
		server.dispatch_tool(request, context).await
	}
}
