# LSP Integration Guide

## Overview

Octocode integrates with Language Server Protocol (LSP) to provide enhanced code navigation and analysis capabilities through the MCP server. This integration allows AI assistants to perform intelligent code operations like go-to-definition, hover information, find references, and code completion.

## Quick Start

### Starting MCP Server with LSP

```bash
# Basic MCP server
octocode mcp --path /path/to/your/project

# With LSP integration
octocode mcp --path /path/to/your/project --with-lsp "rust-analyzer"
```

### Claude Desktop Configuration

Add to your Claude Desktop configuration:

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project", "--with-lsp", "rust-analyzer"]
    }
  }
}
```

## Supported Language Servers

### Rust
```bash
octocode mcp --path /path/to/rust/project --with-lsp "rust-analyzer"
```

### Python
```bash
# Using pylsp
octocode mcp --path /path/to/python/project --with-lsp "pylsp"

# Using pyright
octocode mcp --path /path/to/python/project --with-lsp "pyright-langserver --stdio"
```

### TypeScript/JavaScript
```bash
octocode mcp --path /path/to/ts/project --with-lsp "typescript-language-server --stdio"
```

### Go
```bash
octocode mcp --path /path/to/go/project --with-lsp "gopls"
```

### C/C++
```bash
octocode mcp --path /path/to/cpp/project --with-lsp "clangd"
```

### Java
```bash
octocode mcp --path /path/to/java/project --with-lsp "jdtls"
```

## Available LSP Tools

### lsp_goto_definition

Navigate to the definition of a symbol.

**Parameters:**
- `file_path` (string): Relative path to the file
- `line` (integer): Line number (1-indexed)
- `symbol` (string): Symbol name to find definition for

**Example:**
```json
{
  "file_path": "src/main.rs",
  "line": 15,
  "symbol": "println"
}
```

**Response:**
```
Definition found at std/io.rs:1234:5
```

### lsp_hover

Get detailed information about a symbol including type information, documentation, and signatures.

**Parameters:**
- `file_path` (string): Relative path to the file
- `line` (integer): Line number (1-indexed)
- `symbol` (string): Symbol name to get information for

**Example:**
```json
{
  "file_path": "src/auth.rs",
  "line": 42,
  "symbol": "authenticate_user"
}
```

**Response:**
```
Hover info (42:5-42:20):
fn authenticate_user(username: &str, password: &str) -> Result<User, AuthError>

Authenticates a user with the provided credentials.
Returns the authenticated user or an authentication error.
```

### lsp_find_references

Find all references to a symbol across the workspace.

**Parameters:**
- `file_path` (string): Relative path to the file
- `line` (integer): Line number (1-indexed)
- `symbol` (string): Symbol name to find references for
- `include_declaration` (boolean, optional): Include symbol declaration in results (default: true)

**Example:**
```json
{
  "file_path": "src/auth.rs",
  "line": 42,
  "symbol": "authenticate_user",
  "include_declaration": true
}
```

**Response:**
```
Found 5 reference(s):
1. src/auth.rs:42:5
2. src/api/login.rs:15:12
3. src/middleware/auth.rs:28:8
4. tests/auth_test.rs:35:9
5. tests/integration_test.rs:67:15
```

### lsp_document_symbols

List all symbols in a document with their types and locations.

**Parameters:**
- `file_path` (string): Relative path to the file

**Example:**
```json
{
  "file_path": "src/auth.rs"
}
```

**Response:**
```
Found 8 symbol(s):
1. User (struct) at 5:1
2. AuthError (enum) at 12:1
3. authenticate_user (function) at 25:1
4. hash_password (function) at 45:1
5. verify_password (function) at 58:1
6. generate_token (function) at 72:1
7. validate_token (function) at 85:1
8. refresh_token (function) at 98:1
```

### lsp_workspace_symbols

Search for symbols across the entire workspace.

**Parameters:**
- `query` (string): Symbol search query

**Example:**
```json
{
  "query": "auth"
}
```

**Response:**
```
Found 12 symbol(s) in workspace:
1. authenticate_user (function) in src/auth.rs:25
2. AuthError (enum) in src/auth.rs:12
3. AuthMiddleware (struct) in src/middleware/auth.rs:8
4. auth_required (function) in src/middleware/auth.rs:35
5. AuthConfig (struct) in src/config.rs:45
...
```

### lsp_completion

Get code completion suggestions at a specific position.

**Parameters:**
- `file_path` (string): Relative path to the file
- `line` (integer): Line number (1-indexed)
- `symbol` (string): Partial symbol or prefix to complete

**Example:**
```json
{
  "file_path": "src/api.rs",
  "line": 25,
  "symbol": "std::vec"
}
```

**Response:**
```
Found 5 completion(s):
1. Vec (struct) - A contiguous growable array type
2. VecDeque (struct) - A double-ended queue implemented with a growable ring buffer
3. vec! (macro) - Creates a Vec containing the arguments
4. vector (module) - Vector utilities
5. vec_map (module) - A vector-based map implementation
```

## Symbol Resolution

The LSP integration uses intelligent symbol resolution to find symbols on specified lines:

### Resolution Strategies

1. **Exact Match with Word Boundaries**: Finds exact symbol matches respecting word boundaries
2. **Substring Search**: Finds symbols as substrings within the line
3. **Case-Insensitive Match**: Falls back to case-insensitive matching
4. **Partial Identifier Matching**: Finds symbols within larger identifiers
5. **Namespace Handling**: Handles qualified names like `std::vec::Vec`
6. **Intelligent Fallback**: Uses first meaningful identifier if exact symbol not found

### Examples

**Line:** `let result = authenticate_user(username, password);`

- Symbol `authenticate_user` → Found at position 14
- Symbol `user` → Found within `authenticate_user` at position 14
- Symbol `auth` → Found within `authenticate_user` at position 14

**Line:** `use std::collections::HashMap;`

- Symbol `HashMap` → Found at position 21
- Symbol `std::collections::HashMap` → Found at position 5
- Symbol `collections` → Found at position 10

## Error Handling

### Common Issues

1. **LSP Server Not Found**
   ```
   Error: LSP server 'rust-analyzer' not found in PATH
   ```
   **Solution**: Install the language server and ensure it's in your PATH

2. **Symbol Not Found**
   ```
   Symbol 'unknown_symbol' not found on line 15
   ```
   **Solution**: Verify the symbol exists on the specified line or use a more general symbol

3. **File Not Opened**
   ```
   File src/main.rs not found in document contents
   ```
   **Solution**: The LSP server automatically opens files on-demand

### Debugging

Start the MCP server with debug logging:

```bash
octocode mcp --path /path/to/project --with-lsp "rust-analyzer" --debug
```

This provides detailed information about:
- LSP server startup and initialization
- Symbol resolution attempts and fallbacks
- File opening and content synchronization
- Request/response communication with the LSP server

## Advanced Configuration

### Custom LSP Server Commands

You can use any LSP-compliant language server:

```bash
# Custom command with arguments
octocode mcp --path /path/to/project --with-lsp "custom-lsp --flag value"

# Language server with specific configuration
octocode mcp --path /path/to/project --with-lsp "pylsp -v --config-file .pylsp.json"
```

### Multiple Language Support

For projects with multiple languages, start separate MCP servers:

```bash
# Terminal 1: Rust project
octocode mcp --path /path/to/rust/project --with-lsp "rust-analyzer" --port 3001

# Terminal 2: Python project
octocode mcp --path /path/to/python/project --with-lsp "pylsp" --port 3002
```

### Performance Optimization

For large projects:

1. **Use project-specific LSP configuration**
2. **Limit LSP server memory usage**
3. **Configure appropriate timeouts**
4. **Use incremental synchronization**

## Integration Examples

### With Claude Desktop

1. **Configure the MCP server:**
   ```json
   {
     "mcpServers": {
       "octocode-rust": {
         "command": "octocode",
         "args": ["mcp", "--path", "/path/to/rust/project", "--with-lsp", "rust-analyzer"]
       }
     }
   }
   ```

2. **Use in conversations:**
   ```
   Can you show me the definition of the authenticate_user function in src/auth.rs line 42?
   ```

3. **AI assistant will use:**
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

### With Other MCP Clients

The LSP tools work with any MCP-compatible client. Configure the server endpoint and use the tools programmatically or through natural language interfaces.

## Best Practices

1. **Use Specific Symbols**: Prefer exact symbol names over partial matches
2. **Combine with Semantic Search**: Use LSP tools alongside Octocode's semantic search for comprehensive code understanding
3. **Cache Results**: LSP operations can be expensive; cache results when possible
4. **Handle Errors Gracefully**: Always handle cases where symbols or definitions aren't found
5. **Use Appropriate Tools**: Choose the right LSP tool for your use case:
   - `goto_definition` for navigation
   - `hover` for documentation
   - `find_references` for understanding usage
   - `completion` for code assistance
   - `document_symbols` for file overview
   - `workspace_symbols` for project-wide search

## Troubleshooting

### LSP Server Issues

1. **Check if language server is installed:**
   ```bash
   which rust-analyzer
   which pylsp
   ```

2. **Verify language server works independently:**
   ```bash
   rust-analyzer --version
   pylsp --help
   ```

3. **Check project setup:**
   - Ensure project files are in the correct format
   - Verify language server configuration files exist
   - Check for compilation errors that might affect LSP

### Performance Issues

1. **Large Projects**: Use project-specific ignore patterns
2. **Slow Responses**: Increase timeout values or use faster hardware
3. **Memory Usage**: Monitor LSP server memory consumption
4. **Network Issues**: Ensure proper localhost connectivity

### Symbol Resolution Issues

1. **Symbol Not Found**: Try broader symbol names or check line content
2. **Wrong Position**: Verify line numbers are 1-indexed
3. **Case Sensitivity**: Use exact case or rely on fallback matching
4. **Namespace Issues**: Try both qualified and unqualified names

For additional help, see the [Advanced Usage](ADVANCED_USAGE.md) guide or open an issue on GitHub.
