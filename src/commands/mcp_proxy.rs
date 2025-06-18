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

use octocode::mcp::proxy::McpProxyServer;

#[derive(Args, Clone)]
pub struct McpProxyArgs {
	/// Bind to HTTP server on host:port (required for proxy mode)
	#[arg(long, value_name = "HOST:PORT")]
	pub bind: String,

	/// Root path to scan for git repositories (defaults to current directory)
	#[arg(long, default_value = ".")]
	pub path: String,

	/// Enable debug logging for MCP proxy server
	#[arg(long)]
	pub debug: bool,
}

pub async fn run(args: McpProxyArgs) -> Result<()> {
	// Convert path to absolute PathBuf
	let root_path = if args.path == "." {
		std::env::current_dir()
			.map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?
	} else {
		std::path::Path::new(&args.path)
			.canonicalize()
			.map_err(|e| anyhow::anyhow!("Invalid path '{}': {}", args.path, e))?
	};

	// Verify the path exists and is a directory
	if !root_path.is_dir() {
		return Err(anyhow::anyhow!(
			"Path '{}' is not a directory",
			root_path.display()
		));
	}

	// Parse bind address
	let bind_addr = args
		.bind
		.parse::<std::net::SocketAddr>()
		.map_err(|e| anyhow::anyhow!("Invalid bind address '{}': {}", args.bind, e))?;

	// Print startup info to console since this is HTTP mode
	println!("🚀 Starting MCP Proxy Server...");
	println!("📁 Root path: {}", root_path.display());
	println!("🌐 Bind address: {}", bind_addr);
	println!("🐛 Debug mode: {}", args.debug);

	// Create and run the proxy server
	let mut proxy_server = McpProxyServer::new(bind_addr, root_path, args.debug).await?;
	proxy_server.run().await
}
