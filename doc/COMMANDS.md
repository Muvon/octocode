# Commands Reference

Complete reference for all Octocode commands with examples and options.

## Core Commands

### `octocode index`

Index your codebase for semantic search.

```bash
# Basic indexing
octocode index

# Verbose output
octocode index --verbose

# Force reindex (ignore cache)
octocode index --force

# Index specific directory
octocode index /path/to/project
```

**What it does:**
- Scans all supported files in your project
- Extracts code symbols and structure using Tree-sitter
- Generates embeddings for semantic search
- Builds knowledge graph relationships (if enabled)
- Stores everything in local LanceDB database

### `octocode search`

Semantic search across your codebase.

```bash
# Basic search
octocode search "user authentication"

# Multi-query search (NEW!)
octocode search "authentication" "middleware"
octocode search "jwt" "token" "validation"

# Search specific content types
octocode search "database connection" --mode code
octocode search "API documentation" --mode docs
octocode search "configuration" --mode text
octocode search "error handling" --mode all

# Control result details
octocode search "auth" --detail-level signatures  # Function signatures only
octocode search "auth" --detail-level partial     # Smart truncation (default)
octocode search "auth" --detail-level full        # Complete implementations

# Adjust similarity and results
octocode search "auth" --threshold 0.7 --max-results 10

# Output formats
octocode search "auth" --json     # JSON output
octocode search "auth" --md       # Markdown output

# Symbol expansion
octocode search "user authentication" --expand
```

**Search modes:**
- `all` - Search across all content types (default)
- `code` - Search only in code blocks
- `docs` - Search only in documentation files
- `text` - Search only in plain text files

### `octocode view`

View code signatures and structure.

```bash
# View current directory
octocode view

# View specific files with patterns
octocode view "src/**/*.rs"
octocode view "**/*.py"
octocode view "src/auth/*.ts"

# Output formats
octocode view --json              # JSON format
octocode view --md                # Markdown format
octocode view "src/**/*.rs" --md  # Specific files in markdown
```

### `octocode config`

Manage configuration settings.

```bash
# View current configuration
octocode config --show

# Set embedding models
octocode config --code-embedding-model "voyage:voyage-code-3"
octocode config --text-embedding-model "voyage:voyage-3.5-lite"

# Set LLM model
octocode config --model "anthropic/claude-3.5-sonnet"

# Search settings
octocode config --max-results 20
octocode config --similarity-threshold 0.3

# Enable/disable features
octocode config --graphrag-enabled true
octocode config --graphrag-enabled false

# Performance tuning
octocode config --chunk-size 2000
octocode config --embeddings-batch-size 16
```

## AI-Powered Git Commands

### `octocode commit`

Generate intelligent commit messages with AI.

```bash
# Basic usage - analyze staged changes
git add .
octocode commit

# Add all files and commit in one step
octocode commit --all

# Provide context for better commit messages
octocode commit --message "Refactoring authentication system"

# Auto-commit without confirmation
octocode commit --all --yes

# Skip pre-commit hooks
octocode commit --no-verify

# Dry run (show what would be committed)
octocode commit --dry-run
```

**Pre-commit Integration:**
- Automatically runs pre-commit hooks if available
- Uses `--all-files` when `--all` flag is specified
- Re-stages modified files after pre-commit runs
- Generates AI commit message after pre-commit completes

### `octocode review`

AI-powered code review for best practices.

```bash
# Review staged changes
git add .
octocode review

# Review all changes
octocode review --all

# Focus on specific areas
octocode review --focus security
octocode review --focus performance
octocode review --focus maintainability

# Filter by severity
octocode review --severity critical    # Only critical issues
octocode review --severity high        # Critical and high issues
octocode review --severity low         # All issues

# Output format
octocode review --json                 # JSON output for tooling
```

### `octocode release`

AI-powered release management with version calculation.

```bash
# Preview release (recommended first step)
octocode release --dry-run

# Create release with AI version calculation
octocode release

# Force specific version
octocode release --force-version "2.0.0"

# Skip confirmation prompt
octocode release --yes

# Custom changelog file
octocode release --changelog "HISTORY.md"
```

**Supported project types:**
- Rust (Cargo.toml)
- Node.js (package.json)
- PHP (composer.json)
- Go (go.mod)

## MCP Server Commands

### `octocode mcp`

Start Model Context Protocol server for AI assistants.

```bash
# Basic MCP server
octocode mcp --path /path/to/your/project

# With LSP integration
octocode mcp --path /path/to/project --with-lsp "rust-analyzer"
octocode mcp --path /path/to/project --with-lsp "pylsp"
octocode mcp --path /path/to/project --with-lsp "typescript-language-server --stdio"

# HTTP mode (instead of stdin/stdout)
octocode mcp --bind "127.0.0.1:8080" --path /path/to/project

# Custom port
octocode mcp --path /path/to/project --port 3001

# Debug mode with enhanced logging
octocode mcp --path /path/to/project --debug
```

**Available MCP tools:**
- `semantic_search` - Semantic code search (supports multi-query)
- `graphrag_search` - Relationship-aware search
- `memorize` - Store information for future reference
- `remember` - Retrieve stored information (supports multi-query)
- `forget` - Remove stored information
- `lsp_*` - LSP integration tools (when --with-lsp is used)

### `octocode mcp-proxy`

Start MCP proxy server for multiple repositories.

```bash
# Start proxy server
octocode mcp-proxy --bind "127.0.0.1:8080" --path /path/to/parent/directory

# Custom configuration
octocode mcp-proxy --bind "0.0.0.0:9000" --path /workspace --debug
```

**Features:**
- Automatically discovers git repositories
- Creates MCP instances for each repository
- Provides unified access to multiple projects

## Knowledge Graph Commands

### `octocode graphrag`

Knowledge graph operations using GraphRAG.

```bash
# Search the relationship graph
octocode graphrag search --query "authentication modules"

# Get detailed information about a file
octocode graphrag get-node --node-id "src/auth/mod.rs"

# Find relationships for a specific file
octocode graphrag get-relationships --node-id "src/auth/mod.rs"

# Find connections between two modules
octocode graphrag find-path \
  --source-id "src/auth/mod.rs" \
  --target-id "src/database/mod.rs"

# Get graph overview
octocode graphrag overview

# Export formats
octocode graphrag overview --md > project-structure.md
octocode graphrag search --query "auth" --json
```

## Memory Management Commands

### `octocode memory`

Manage the memory system for storing insights and context.

```bash
# Store new information
octocode memory memorize \
  --title "Authentication Bug Fix" \
  --content "Fixed JWT token validation race condition" \
  --memory-type bug_fix \
  --importance 0.8 \
  --tags security,jwt,auth \
  --files src/auth.rs,src/middleware/auth.rs

# Search memories semantically
octocode memory remember "JWT authentication issues"
octocode memory remember "authentication" "security" "bugs"

# Retrieve specific memory
octocode memory get abc123

# Update existing memory
octocode memory update abc123 --add-tags performance

# Filter memories
octocode memory by-type bug_fix
octocode memory by-tags security,auth
octocode memory for-files src/auth.rs

# List recent memories
octocode memory recent --limit 10

# Memory statistics
octocode memory stats

# Create relationships between memories
octocode memory relate source-id target-id

# Cleanup old memories
octocode memory cleanup

# Delete specific memory
octocode memory forget --memory-id abc123

# Delete all memories (careful!)
octocode memory clear-all --yes
```

**Memory types:**
- `code` - Code-related insights
- `architecture` - Architectural decisions
- `bug_fix` - Bug reports and solutions
- `feature` - Feature implementations
- `documentation` - Documentation notes
- `user_preference` - User preferences
- `decision` - Project decisions
- `learning` - Insights and lessons
- `configuration` - Setup notes
- `testing` - Test strategies
- `performance` - Performance optimizations
- `security` - Security considerations
- `insight` - General observations

## Utility Commands

### `octocode format`

Format code according to .editorconfig rules.

```bash
# Format all supported files
octocode format

# Preview changes without applying
octocode format --dry-run

# Format specific files
octocode format src/main.rs src/lib.rs

# Format and commit changes
octocode format --commit

# Verbose output
octocode format --verbose
```

### `octocode logs`

View MCP server logs.

```bash
# View logs for current project
octocode logs

# Follow logs in real-time
octocode logs --follow

# Show only error logs
octocode logs --errors-only

# Show more/fewer lines
octocode logs --lines 50

# View logs for all projects
octocode logs --all
```

### `octocode watch`

Auto-index files when they change.

```bash
# Basic watch mode
octocode watch

# Custom debounce timing (1-30 seconds)
octocode watch --debounce 5

# Custom additional delay (0-5000ms)
octocode watch --additional-delay 2000

# Combine timing options
octocode watch --debounce 3 --additional-delay 1500

# Quiet mode (less output)
octocode watch --quiet

# Watch without git requirements
octocode watch --no-git
```

### `octocode clear`

Clear database tables.

```bash
# Clear all data
octocode clear --all

# Clear specific collections
octocode clear --documents
octocode clear --graphs
octocode clear --memories

# Skip confirmation prompt
octocode clear --all --yes
```

### `octocode completion`

Generate shell completion scripts.

```bash
# Generate completions for your shell
octocode completion bash > ~/.bash_completion.d/octocode
octocode completion zsh > ~/.zsh/completions/_octocode
octocode completion fish > ~/.config/fish/completions/octocode.fish

# Or install using make (if available)
make install-completions
```

## Global Options

Most commands support these global options:

```bash
# Verbose output
octocode <command> --verbose

# JSON output (where applicable)
octocode <command> --json

# Markdown output (where applicable)
octocode <command> --md

# Help for any command
octocode <command> --help
octocode help <command>
```

## Command Combinations

### Complete Reindex Workflow

```bash
# Clear old data and reindex
octocode clear --all --yes
octocode index --verbose

# Start MCP server
octocode mcp --path . &
```

### Daily Development Workflow

```bash
# Start watching for changes
octocode watch --quiet &

# Work on code...
# Files are automatically indexed

# Search for relevant code
octocode search "error handling patterns"

# Commit with AI assistance
git add .
octocode commit

# Review changes
octocode review --focus security
```

### Documentation Generation

```bash
# Generate comprehensive documentation
octocode view "src/**/*.rs" --md > docs/api-reference.md
octocode graphrag overview --md > docs/architecture.md

# Create project structure overview
octocode search "main components" --md > docs/components.md
```

### Batch Memory Operations

```bash
# Store multiple related memories
octocode memory memorize --title "Auth System" --content "..." --tags auth,security
octocode memory memorize --title "DB Layer" --content "..." --tags database,performance

# Search across all memories
octocode memory remember "system architecture"

# Get statistics
octocode memory stats
```

For more detailed information about specific features, see:
- [Advanced Usage](ADVANCED_USAGE.md) - Advanced workflows and techniques
- [MCP Integration](MCP_INTEGRATION.md) - Detailed MCP server setup
- [Configuration](CONFIGURATION.md) - Complete configuration reference
