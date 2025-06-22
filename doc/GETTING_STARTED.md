# Getting Started with Octocode

This guide will help you get up and running with Octocode quickly.

## Prerequisites

Before you start, make sure you have:

- **Octocode installed** - See [Installation Guide](../INSTALL.md)
- **API keys configured** - See [API Keys Setup](API_KEYS.md)
- **Git repository** - Octocode works best with git repositories

## First Steps

### 1. Navigate to Your Project

```bash
cd /path/to/your/project
```

### 2. Index Your Codebase

```bash
# Index current directory
octocode index

# Watch for progress
octocode index --verbose
```

**What happens during indexing:**
- Scans all supported files in your project
- Extracts code symbols and structure
- Generates embeddings for semantic search
- Builds knowledge graph relationships
- Stores everything in local database

### 3. Try Your First Search

```bash
# Basic semantic search
octocode search "user authentication"

# Search specific content types
octocode search "database connection" --mode code
octocode search "API documentation" --mode docs
```

### 4. Explore Multi-Query Search

```bash
# Combine related terms for comprehensive results
octocode search "authentication" "middleware"
octocode search "jwt" "token" "validation"
octocode search "database" "connection" "pool"
```

## Basic Workflow

### Daily Development Cycle

```bash
# 1. Start watching for changes (optional)
octocode watch &

# 2. Work on your code...
# Files are automatically indexed as you work

# 3. Search for relevant code
octocode search "error handling patterns"

# 4. Use AI-powered git workflow
git add .
octocode commit  # Generates intelligent commit message

# 5. Review your changes
octocode review --focus security
```

### Working with Memory System

```bash
# Store important insights
octocode memory memorize \
  --title "Authentication Bug Fix" \
  --content "Fixed JWT token validation race condition" \
  --memory-type bug_fix \
  --tags security,jwt

# Search your memory
octocode memory remember "JWT authentication issues"

# Get memories by type
octocode memory by-type bug_fix
```

## Configuration Basics

### View Current Configuration

```bash
octocode config --show
```

### Essential Configuration

```bash
# Use faster local models (macOS only)
octocode config \
  --code-embedding-model "fastembed:all-MiniLM-L6-v2" \
  --text-embedding-model "fastembed:multilingual-e5-small"

# Enable GraphRAG for relationship analysis
octocode config --graphrag-enabled true

# Set search preferences
octocode config --max-results 20 --similarity-threshold 0.3
```

## MCP Server for AI Assistants

### Basic MCP Setup

1. **Start the server:**
   ```bash
   octocode mcp --path /path/to/your/project
   ```

2. **Configure in Claude Desktop:**
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

3. **Use with AI assistant:**
   - Ask: "Search for authentication functions in my codebase"
   - Ask: "What are the main components in this project?"
   - Ask: "Remember this bug fix for future reference"

### With LSP Integration

```bash
# Start with language server support
octocode mcp --path /path/to/rust/project --with-lsp "rust-analyzer"
octocode mcp --path /path/to/python/project --with-lsp "pylsp"
```

## Common Use Cases

### Code Exploration

```bash
# Understand new codebase
octocode view "**/*.rs" --md > project-overview.md
octocode graphrag overview --md > architecture.md

# Find similar patterns
octocode search "error handling" --expand
```

### Debugging and Maintenance

```bash
# Find related code
octocode search "authentication" "session" "login"

# Search for specific patterns
octocode search "TODO" "FIXME" --mode all

# Review recent changes
git add .
octocode review --severity high
```

### Documentation Generation

```bash
# Generate API documentation
octocode view "src/**/*.rs" --json > api-docs.json

# Create project structure overview
octocode graphrag overview --md > STRUCTURE.md
```

## Troubleshooting

### Slow Indexing

```bash
# Use faster embedding models
octocode config --code-embedding-model "fastembed:all-MiniLM-L6-v2"

# Disable GraphRAG temporarily
octocode config --graphrag-enabled false
```

### Poor Search Results

```bash
# Adjust similarity threshold
octocode config --similarity-threshold 0.1  # More results
octocode config --similarity-threshold 0.5  # Fewer, more relevant results

# Try different search modes
octocode search "your query" --mode code     # Only code
octocode search "your query" --mode docs     # Only documentation
```

### API Rate Limits

```bash
# Switch to local models (macOS only)
octocode config \
  --code-embedding-model "fastembed:all-MiniLM-L6-v2" \
  --text-embedding-model "fastembed:multilingual-e5-small"
```

## Next Steps

Once you're comfortable with the basics:

1. **Explore Advanced Features** - See [Advanced Usage](ADVANCED_USAGE.md)
2. **Optimize Performance** - See [Performance Guide](PERFORMANCE.md)
3. **Set Up MCP Integration** - See [MCP Integration](MCP_INTEGRATION.md)
4. **Configure for Your Workflow** - See [Configuration Guide](CONFIGURATION.md)

## Getting Help

- **Documentation**: Browse all guides in the `doc/` directory
- **Issues**: [GitHub Issues](https://github.com/Muvon/octocode/issues)
- **Email**: [opensource@muvon.io](mailto:opensource@muvon.io)
