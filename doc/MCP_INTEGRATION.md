# MCP Server Integration Guide

Complete guide for integrating Octocode with AI assistants using the Model Context Protocol (MCP).

## Overview

Octocode provides a built-in MCP server that enables AI assistants to interact with your codebase through semantic search, memory management, and LSP integration. The server supports both stdin/stdout mode (for direct AI assistant integration) and HTTP mode (for web-based integrations).

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

### semantic_search

Semantic search across your codebase with multi-query support.

**Parameters:**
- `query` (string or array) - Search query or multiple queries
- `mode` (string, optional) - Search scope: "all", "code", "docs", "text"
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

### graphrag_search

Relationship-aware search using the knowledge graph.

**Parameters:**
- `query` (string) - Complex architectural query about relationships

**Example:**
```json
{
  "query": "How does user authentication flow through the system?"
}
```

### memorize

Store important information for future reference.

**Parameters:**
- `title` (string) - Short descriptive title
- `content` (string) - Detailed content to remember
- `memory_type` (string, optional) - Type of memory (code, bug_fix, feature, etc.)
- `importance` (number, optional) - Importance score 0.0-1.0
- `tags` (array, optional) - Tags for categorization
- `related_files` (array, optional) - Related file paths

**Example:**
```json
{
  "title": "JWT Authentication Bug Fix",
  "content": "Fixed race condition in token refresh logic by adding mutex lock around token validation",
  "memory_type": "bug_fix",
  "importance": 0.8,
  "tags": ["security", "jwt", "race-condition"],
  "related_files": ["src/auth/jwt.rs", "src/middleware/auth.rs"]
}
```

### remember

Retrieve stored information with semantic search.

**Parameters:**
- `query` (string or array) - Search query or multiple related queries
- `memory_types` (array, optional) - Filter by memory types
- `tags` (array, optional) - Filter by tags
- `related_files` (array, optional) - Filter by related files
- `limit` (integer, optional) - Maximum memories to return

**Single Query Example:**
```json
{
  "query": "JWT authentication issues",
  "memory_types": ["bug_fix", "security"],
  "limit": 5
}
```

**Multi-Query Example:**
```json
{
  "query": ["authentication", "security", "bugs"],
  "tags": ["jwt", "security"],
  "limit": 10
}
```

### forget

Remove stored information.

**Parameters:**
- `memory_id` (string, optional) - Specific memory ID to forget
- `query` (string, optional) - Query to find memories to forget
- `memory_types` (array, optional) - Filter by memory types when using query
- `tags` (array, optional) - Filter by tags when using query
- `confirm` (boolean) - Must be true to confirm deletion

**Example:**
```json
{
  "memory_id": "abc123-def456-789",
  "confirm": true
}
```

## LSP Integration Tools

When started with `--with-lsp`, additional tools become available:

### lsp_goto_definition

Navigate to symbol definition.

**Parameters:**
- `file_path` (string) - Relative path to file
- `line` (integer) - Line number (1-indexed)
- `symbol` (string) - Symbol name

**Example:**
```json
{
  "file_path": "src/main.rs",
  "line": 15,
  "symbol": "authenticate_user"
}
```

### lsp_hover

Get symbol information and documentation.

**Parameters:**
- `file_path` (string) - Relative path to file
- `line` (integer) - Line number (1-indexed)
- `symbol` (string) - Symbol name

### lsp_find_references

Find all references to a symbol.

**Parameters:**
- `file_path` (string) - Relative path to file
- `line` (integer) - Line number (1-indexed)
- `symbol` (string) - Symbol name
- `include_declaration` (boolean, optional) - Include declaration in results

### lsp_completion

Get code completion suggestions.

**Parameters:**
- `file_path` (string) - Relative path to file
- `line` (integer) - Line number (1-indexed)
- `symbol` (string) - Partial symbol to complete

### lsp_document_symbols

List all symbols in a document.

**Parameters:**
- `file_path` (string) - Relative path to file

### lsp_workspace_symbols

Search symbols across workspace.

**Parameters:**
- `query` (string) - Symbol search query

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

**Ask Claude:**
> "Can you search for authentication-related code in my project?"

**Claude uses:**
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

**Ask Claude:**
> "How are the database components connected in this system?"

**Claude uses:**
```json
{
  "tool": "graphrag_search",
  "arguments": {
    "query": "database component relationships and data flow patterns"
  }
}
```

### Memory Management

**Ask Claude:**
> "Remember this bug fix: We fixed the JWT token validation by adding proper error handling"

**Claude uses:**
```json
{
  "tool": "memorize",
  "arguments": {
    "title": "JWT Token Validation Bug Fix",
    "content": "Fixed JWT token validation by adding proper error handling to prevent authentication bypass",
    "memory_type": "bug_fix",
    "tags": ["jwt", "security", "authentication"]
  }
}
```

### Code Navigation

**Ask Claude:**
> "Show me the definition of the authenticate_user function in src/auth.rs line 42"

**Claude uses:**
```json
{
  "tool": "lsp_goto_definition",
  "arguments": {
    "file_path": "src/auth.rs",
    "line": 42,
    "symbol": "authenticate_user"
  }
}
```

## Advanced Configuration

### Custom MCP Server Settings

```bash
# Start with custom settings
octocode mcp \
  --path /path/to/project \
  --port 3001 \
  --debug \
  --with-lsp "rust-analyzer"

# HTTP mode with custom binding
octocode mcp \
  --bind "0.0.0.0:8080" \
  --path /path/to/project \
  --debug
```

### Multiple Language Servers

For projects with multiple languages, start separate MCP servers:

```bash
# Terminal 1: Rust project
octocode mcp --path /rust/project --with-lsp "rust-analyzer" --port 3001

# Terminal 2: Python project
octocode mcp --path /python/project --with-lsp "pylsp" --port 3002

# Terminal 3: TypeScript project
octocode mcp --path /ts/project --with-lsp "typescript-language-server --stdio" --port 3003
```

### Environment-Specific Configuration

```bash
# Development environment
octocode mcp --path . --debug --with-lsp "rust-analyzer"

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
const response = await fetch('http://localhost:8080/tools/semantic_search', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    query: ["authentication", "middleware"],
    mode: "code",
    max_results: 5
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
  --with-lsp "rust-analyzer" \
  --debug

# Configure search limits
octocode config --max-results 20 --similarity-threshold 0.3
```

### Memory Management

```bash
# Monitor memory usage
octocode memory stats

# Clean up old memories periodically
octocode memory cleanup

# Limit memory storage
octocode config --max-memories 10000
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
