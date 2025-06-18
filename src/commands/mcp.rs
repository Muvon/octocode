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

use anyhow::Result;
use clap::Args;

use octocode::config::Config;
use octocode::mcp::McpServer;

#[derive(Args, Clone)]
pub struct McpArgs {
	/// Enable debug logging for MCP server
	#[arg(long)]
	pub debug: bool,

	/// Path to the directory to serve (defaults to current directory)
	#[arg(long, default_value = ".")]
	pub path: String,

	/// Skip git repository requirement and git-based optimizations
	#[arg(long)]
	pub no_git: bool,

	/// External LSP server command to launch (e.g., "rust-analyzer", "typescript-language-server --stdio")
	#[arg(long, value_name = "COMMAND")]
	pub with_lsp: Option<String>,

	/// Bind to HTTP server on host:port instead of using stdin/stdout (e.g., "0.0.0.0:12345")
	#[arg(long, value_name = "HOST:PORT")]
	pub bind: Option<String>,
}

pub async fn run(args: McpArgs) -> Result<()> {
	let config = Config::load()?;

	// Convert path to absolute PathBuf
	let working_directory = std::path::Path::new(&args.path)
		.canonicalize()
		.map_err(|e| anyhow::anyhow!("Invalid path '{}': {}", args.path, e))?;

	// Verify the path exists and is a directory
	if !working_directory.is_dir() {
		return Err(anyhow::anyhow!(
			"Path '{}' is not a directory",
			working_directory.display()
		));
	}

	// Note: No console output here - MCP protocol compliance requires clean stdout/stderr
	// All debug information is logged to files via structured logging in the server

	let mut server = McpServer::new(
		config,
		args.debug,
		working_directory,
		args.no_git,
		args.with_lsp,
	)
	.await?;

	// Check if HTTP binding is requested
	if let Some(bind_addr) = args.bind {
		server.run_http(&bind_addr).await
	} else {
		server.run().await
	}
}
