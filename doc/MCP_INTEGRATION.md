# MCP Server Integration Guide

Complete guide for integrating Octocode with AI assistants using the Model Context Protocol (MCP).

## Overview

Octocode provides a built-in MCP server that enables AI assistants to interact with your codebase through semantic search, code signatures, GraphRAG, and structural pattern matching. The server supports both stdin/stdout mode (for direct AI assistant integration) and HTTP mode (for web-based integrations).

**📖 For client-specific setup instructions, see [MCP Client Setup Guide](MCP_CLIENTS.md).**

## Quick Start

### Basic MCP Server

```bash
# Start MCP server for current project
octocode mcp --path .

# Start for specific project
octocode mcp --path /path/to/your/project

# Start with debug logging
octocode mcp --path . --debug
```

### HTTP Mode

```bash
# Start HTTP server on specific port
octocode mcp --bind "127.0.0.1:8080" --path .

# Bind to all interfaces
octocode mcp --bind "0.0.0.0:8080" --path /path/to/project
```

## Claude Desktop Integration

### Configuration

Add to your Claude Desktop configuration file:

**macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`
**Windows**: `%APPDATA%\\Claude\\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project"]
    }
  }
}
```

### Multiple Projects

```json
{
  "mcpServers": {
    "octocode-rust": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/rust/project", "--port", "3001"]
    },
    "octocode-python": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/python/project", "--port", "3002"]
    },
    "octocode-typescript": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/ts/project", "--port", "3003"]
    }
  }
}
```

### With LSP Integration

```json
{
  "mcpServers": {
    "octocode-rust": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/rust/project", "--with-lsp", "rust-analyzer"]
    },
    "octocode-python": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/python/project", "--with-lsp", "pylsp"]
    },
    "octocode-typescript": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/ts/project", "--with-lsp", "typescript-language-server --stdio"]
    }
  }
}
```

## Available MCP Tools

Octocode provides 4 core tools for code intelligence:

### semantic_search

Semantic search across your codebase with multi-query support.

**Parameters:**
- `query` (string or array) - Search query or multiple queries
- `mode` (string, optional) - Search scope: "all", "code", "docs", "text", "commits"
- `detail_level` (string, optional) - Detail level: "signatures", "partial", "full"
- `max_results` (integer, optional) - Maximum results to return (1-20)
- `threshold` (number, optional) - Similarity threshold (0.0-1.0)

**Single Query Example:**
```json
{
  "query": "authentication functions",
  "mode": "code",
  "detail_level": "partial",
  "max_results": 5
}
```

**Multi-Query Example:**
```json
{
  "query": ["authentication", "middleware", "security"],
  "mode": "all",
  "detail_level": "full",
  "max_results": 10
}
```

### view_signatures

Extract and view function signatures, class definitions, and other meaningful code structures from files.

**Parameters:**
- `files` (array) - Array of file paths or glob patterns to analyze
- `max_tokens` (integer, optional) - Maximum tokens in output before truncation (default: 2000)

**Examples:**

**View signatures for specific files:**
```json
{
  "files": ["src/main.rs", "src/lib.rs"]
}
```

**View signatures using glob patterns:**
```json
{
  "files": ["src/**/*.rs", "tests/**/*.rs"],
  "max_tokens": 4000
}
```

**Multi-language analysis:**
```json
{
  "files": ["**/*.{rs,py,js,ts,css,svelte,md}"]
}
```

### graphrag

Advanced relationship-aware GraphRAG operations for code analysis. Supports multiple operations for exploring the knowledge graph.

**Parameters:**
- `operation` (string, required) - Operation to perform: "search", "get-node", "get-relationships", "find-path", "overview"
- `query` (string, optional) - Search query for 'search' operation
- `node_id` (string, optional) - Node identifier for 'get-node' and 'get-relationships' operations
- `source_id` (string, optional) - Source node identifier for 'find-path' operation
- `target_id` (string, optional) - Target node identifier for 'find-path' operation
- `max_depth` (integer, optional) - Maximum path depth for 'find-path' operation (default: 3)
- `format` (string, optional) - Output format: "text", "json", "markdown" (default: "text")

**Operation Examples:**

**Search for nodes by semantic query:**
```json
{
  "operation": "search",
  "query": "How does user authentication flow through the system?"
}
```

**Get detailed node information:**
```json
{
  "operation": "get-node",
  "node_id": "src/auth/mod.rs",
  "format": "markdown"
}
```

**Find all relationships for a node:**
```json
{
  "operation": "get-relationships",
  "node_id": "src/auth/mod.rs",
  "format": "text"
}
```

**Find connection paths between nodes:**
```json
{
  "operation": "find-path",
  "source_id": "src/auth/mod.rs",
  "target_id": "src/database/mod.rs",
  "max_depth": 3,
  "format": "markdown"
}
```

**Get graph overview and statistics:**
```json
{
  "operation": "overview",
  "format": "json"
}
```

### structural_search

Search or rewrite code by AST structure using ast-grep pattern syntax. Complements `semantic_search`: use this for structural/syntactic patterns, `semantic_search` for meaning-based queries.

**Parameters:**
- `pattern` (string, required) - AST pattern to search for (e.g. `$FUNC.unwrap()`, `if let Some($X) = $Y { $$$ }`)
- `language` (string, required) - Language to search: rust, javascript, typescript, python, go, java, cpp, php, ruby, lua, bash, css, json
- `paths` (array, optional) - File path substrings to filter results
- `context` (integer, optional) - Number of context lines around matches (default: 0)
- `max_results` (integer, optional) - Maximum number of matches to return (default: 50)
- `rewrite` (string, optional) - Rewrite template with metavariable substitution (e.g. `$VAR.expect("reason")`)
- `update_all` (boolean, optional) - When true, apply rewrites to files in-place. When false/absent, returns a diff preview

**Search Examples:**

**Find all unwrap() calls in Rust:**
```json
{
  "pattern": "$VAR.unwrap()",
  "language": "rust"
}
```

**Find new expressions in JavaScript with path filter:**
```json
{
  "pattern": "new $CLASS($$$ARGS)",
  "language": "javascript",
  "paths": ["src/"],
  "context": 2
}
```

**Rewrite Examples:**

**Preview rewrite (dry run):**
```json
{
  "pattern": "$VAR.unwrap()",
  "language": "rust",
  "rewrite": "$VAR.expect(\"added context\")"
}
```

**Apply rewrite in-place:**
```json
{
  "pattern": "console.log($ARG)",
  "language": "javascript",
  "rewrite": "logger.info($ARG)",
  "update_all": true,
  "paths": ["src/"]
}
```

## MCP Proxy Server

For managing multiple repositories, use the MCP proxy server:

```bash
# Start proxy server
octocode mcp-proxy --bind "127.0.0.1:8080" --path /path/to/parent/directory
```

**Features:**
- Automatically discovers git repositories in the specified directory
- Creates MCP server instances for each repository
- Provides unified HTTP interface for multiple projects
- Supports dynamic repository addition/removal

**Configuration:**
```json
{
  "mcpServers": {
    "octocode-proxy": {
      "command": "octocode",
      "args": ["mcp-proxy", "--bind", "127.0.0.1:8080", "--path", "/workspace"]
    }
  }
}
```

## Usage Examples

### Code Exploration

**Ask your AI:**
> "Can you search for authentication-related code in my project?"

**AI uses:**
```json
{
  "tool": "semantic_search",
  "arguments": {
    "query": ["authentication", "auth", "login"],
    "mode": "code",
    "max_results": 10
  }
}
```

### Architecture Understanding

**Ask your AI:**
> "How are the database components connected in this system?"

**AI uses:**
```json
{
  "tool": "graphrag",
  "arguments": {
    "operation": "search",
    "query": "database component relationships and data flow patterns"
  }
}
```

### Code Pattern Search

**Ask your AI:**
> "Find all places where we use .unwrap() in the codebase"

**AI uses:**
```json
{
  "tool": "structural_search",
  "arguments": {
    "pattern": "$VAR.unwrap()",
    "language": "rust",
    "max_results": 50
  }
}
```

### File Structure Analysis

**Ask your AI:**
> "Show me the structure of the authentication module"

**AI uses:**
```json
{
  "tool": "view_signatures",
  "arguments": {
    "files": ["src/auth/**/*.rs"]
  }
}
```

## Next Steps

- **[MCP Client Setup Guide](MCP_CLIENTS.md)** — Detailed setup for 15+ clients
- **[Commands Reference](COMMANDS.md)** — Learn all Octocode CLI commands
- **[Configuration](CONFIGURATION.md)** — Customize indexing and search behavior
- **[Advanced Usage](ADVANCED_USAGE.md)** — Advanced configuration and optimization
# Start with custom settings
octocode mcp \
  --path /path/to/project \
  --debug

# HTTP mode with custom binding
octocode mcp \
  --bind "0.0.0.0:8080" \
  --path /path/to/project \
  --debug
```

### Multiple Projects

For projects with multiple codebases, start separate MCP servers:

```bash
# Terminal 1: Work project
octocode mcp --path /work/project --bind "127.0.0.1:8081"

# Terminal 2: Personal project
octocode mcp --path /personal/project --bind "127.0.0.1:8082"
```

### Environment-Specific Configuration

```bash
# Development environment
octocode mcp --path . --debug

# Production environment
octocode mcp --path /app --bind "127.0.0.1:8080" --quiet
```

## Integration with Other AI Assistants

### Generic MCP Client

Any MCP-compatible client can connect to Octocode:

```python
# Python example using MCP client library
import mcp

client = mcp.Client("octocode", ["mcp", "--path", "/path/to/project"])

# Use semantic search
result = await client.call_tool("semantic_search", {
    "query": "authentication functions",
    "mode": "code"
})
```

### HTTP API Integration

When using HTTP mode, you can integrate with web applications:

```javascript
// JavaScript example
const response = await fetch('http://localhost:8080', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    jsonrpc: "2.0",
    id: 1,
    method: "tools/call",
    params: {
      name: "semantic_search",
      arguments: {
        query: ["authentication", "middleware"],
        mode: "code",
        max_results: 5
      }
    }
  })
});

const results = await response.json();
```

## Performance Optimization

### For Large Codebases

```bash
# Optimize for large projects
octocode mcp \
  --path /large/project \
  --debug
```

### Memory Management

Octocode automatically manages memory for large codebases:
- Efficient vector storage with LanceDB
- Smart caching of frequently accessed data
- Incremental updates for changed files

## Troubleshooting

### Common Issues

**MCP server not appearing in client:**
- Ensure Octocode is in your PATH: `which octocode`
- Use absolute paths in MCP config
- Restart your MCP client after config changes

**Tools returning errors:**
- Ensure your project is indexed: `octocode index`
- Check API keys are set: `echo $VOYAGE_API_KEY`
- Verify the project path in MCP config matches your indexed project

**Permission errors:**
- Check file permissions: `ls -la $(which octocode)`
- Use absolute path in config: `"command": "/usr/local/bin/octocode"`

### Debug Mode

Enable debug logging to troubleshoot issues:

```bash
octocode mcp --path /your/project --debug
```

This logs all MCP requests and responses to help diagnose problems.

## Next Steps

- **[MCP Client Setup Guide](MCP_CLIENTS.md)** — Detailed setup for 15+ clients
- **[Commands Reference](COMMANDS.md)** — Learn all Octocode CLI commands
- **[Configuration](CONFIGURATION.md)** — Customize indexing and search behavior
- **[Advanced Usage](ADVANCED_USAGE.md)** — Advanced configuration and optimization

# Configure search limits
octocode config --max-results 20 --similarity-threshold 0.3
```

## Troubleshooting

### MCP Server Not Starting

1. **Check path exists**: Ensure the project path is valid
2. **Check permissions**: Ensure read access to the project directory
3. **Check port availability**: Ensure the port isn't already in use
4. **Check LSP server**: Ensure language server is installed and in PATH

### AI Assistant Not Connecting

1. **Check configuration**: Verify Claude Desktop config syntax
2. **Check paths**: Ensure absolute paths in configuration
3. **Restart assistant**: Restart Claude Desktop after config changes
4. **Check logs**: Use `--debug` flag to see detailed logs

### LSP Integration Issues

1. **Check LSP server**: Verify language server works independently
2. **Check project setup**: Ensure project files are valid
3. **Check symbol resolution**: Try broader symbol names
4. **Check file paths**: Ensure files exist and are accessible

### Performance Issues

1. **Limit search results**: Use smaller `max_results` values
2. **Increase thresholds**: Use higher similarity thresholds
3. **Optimize indexing**: Use faster embedding models
4. **Monitor resources**: Check CPU and memory usage

For more detailed information, see:
- [LSP Integration Guide](LSP_INTEGRATION.md)
- [Advanced Usage](ADVANCED_USAGE.md)
- [Configuration Guide](CONFIGURATION.md)
