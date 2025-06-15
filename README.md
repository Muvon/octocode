# Octocode - Intelligent Code Indexer and Graph Builder

**© 2025 Muvon Un Limited (Hong Kong)** | [Website](https://muvon.io) | [Product Page](https://octocode.muvon.io)

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org)

## 🚀 Overview

Octocode is a powerful code indexer and semantic search engine that builds intelligent knowledge graphs of your codebase. It combines advanced AI capabilities with local-first design to provide deep code understanding, relationship mapping, and intelligent assistance for developers.

## ✨ Key Features

### 🔍 **Semantic Code Search**
- Natural language queries across your entire codebase
- **Multi-query search**: Combine multiple terms for comprehensive results (e.g., `"auth" "middleware"`)
- Multi-mode search (code, documentation, text, or all)
- Intelligent ranking with similarity scoring
- Symbol expansion for comprehensive results

### 🕸️ **Knowledge Graph (GraphRAG)**
- Automatic relationship discovery between files and modules
- Import/export dependency tracking
- AI-powered file descriptions and architectural insights
- Path finding between code components

### 🌐 **Multi-Language Support**
- **Rust**, **Python**, **JavaScript**, **TypeScript**, **Go**, **PHP**
- **C++**, **Ruby**, **JSON**, **Bash**, **Markdown**
- Tree-sitter based parsing for accurate symbol extraction

### 🧠 **AI-Powered Features**
- Smart commit message generation with **automatic pre-commit hook integration**
- Code review with best practices analysis
- **Memory system** for storing insights, decisions, and context
- **Semantic memory search** with vector similarity
- **Memory relationships** and automatic context linking
- Multiple LLM support via OpenRouter

### 🔌 **MCP Server Integration**
- Built-in Model Context Protocol server
- Seamless integration with AI assistants (Claude Desktop, etc.)
- **Multi-query search support**: Use arrays like `["auth", "middleware"]` in semantic_search tool
- **LSP Integration**: Language Server Protocol support for enhanced code navigation
- Real-time file watching and auto-indexing
- Rich tool ecosystem for code analysis

### ⚡ **Performance & Flexibility**
- **Optimized indexing**: Batch metadata loading eliminates database query storms
- **Smart batching**: 16 files per batch with token-aware API optimization
- **Frequent persistence**: Data saved every 16 files (max 16 files at risk)
- **Fast file traversal**: Single-pass progressive counting and processing
- **Local embedding models**: FastEmbed and SentenceTransformer (macOS only)
- **Cloud embedding providers**: Voyage AI (default), Jina AI, Google
- **Free tier available**: Voyage AI provides 200M free tokens monthly
- Lance columnar database for fast vector search
- Incremental indexing and git-aware optimization

## 📦 Installation

### Download Prebuilt Binary (Recommended)
```bash
# Universal install script (Linux, macOS, Windows) - requires curl
curl -fsSL https://raw.githubusercontent.com/Muvon/octocode/master/install.sh | sh
```

Or download manually from [GitHub Releases](https://github.com/Muvon/octocode/releases).

### Using Cargo (from Git)
```bash
cargo install --git https://github.com/Muvon/octocode
```

### Build from Source
**Prerequisites:**
- **Rust 1.70+** ([install from rustup.rs](https://rustup.rs/))
- **Git** (for repository features)

```bash
git clone https://github.com/Muvon/octocode.git
cd octocode

# macOS: Full build with local embeddings
cargo build --release

# Windows/Linux: Cloud embeddings only (due to ONNX Runtime issues)
cargo build --release --no-default-features
```

**Note**: Prebuilt binaries use cloud embeddings only. Local embeddings require building from source on macOS.

## 🔑 Getting Started - API Keys

**⚠️ Important**: Octocode requires API keys to function. Local embedding models are only available on macOS builds.

### Required: Voyage AI (Embeddings)
```bash
export VOYAGE_API_KEY="your-voyage-api-key"
```
- **Free tier**: 200M tokens per month
- **Get API key**: [voyageai.com](https://www.voyageai.com/)
- **Used for**: Code and text embeddings (semantic search)

### Optional: OpenRouter (LLM Features)
```bash
export OPENROUTER_API_KEY="your-openrouter-api-key"
```
- **Get API key**: [openrouter.ai](https://openrouter.ai/)
- **Used for**: Commit messages, code review, GraphRAG descriptions
- **Note**: Basic search and indexing work without this

### Platform Limitations
- **Windows/Linux**: Must use cloud embeddings (Voyage AI default)
- **macOS**: Can use local embeddings (build from source) or cloud embeddings

## 🚀 Quick Start

### 1. Setup API Keys (Required)
```bash
# Set Voyage AI API key for embeddings (free 200M tokens/month)
export VOYAGE_API_KEY="your-voyage-api-key"

# Optional: Set OpenRouter API key for LLM features (commit, review, GraphRAG)
export OPENROUTER_API_KEY="your-openrouter-api-key"
```

**Get your free API keys:**
- **Voyage AI**: [Get free API key](https://www.voyageai.com/) (200M tokens/month free)
- **OpenRouter**: [Get API key](https://openrouter.ai/) (optional, for LLM features)

### 2. Basic Usage
```bash
# Index your current directory
octocode index

# Search your codebase with single query
octocode search "HTTP request handling"

# Search with multiple queries for comprehensive results (NEW!)
octocode search "authentication" "middleware"
octocode search "jwt" "token" "validation"

# View code signatures
octocode view "src/**/*.rs"
```

### 3. AI-Powered Git Workflow (Requires OpenRouter API Key)
```bash
# Generate intelligent commit messages with automatic pre-commit hook integration
git add .
octocode commit

# Or add all files and commit in one step
octocode commit --all

# Skip pre-commit hooks if needed
octocode commit --no-verify

# Review code for best practices
octocode review

# Create AI-powered releases with version calculation and changelog
octocode release --dry-run  # Preview what would be done
octocode release            # Create the actual release
```

### 4. MCP Server for AI Assistants
```bash
# Start MCP server
octocode mcp --path /path/to/your/project

# Start with LSP integration (NEW!)
octocode mcp --path /path/to/your/project --with-lsp "rust-analyzer"
octocode mcp --path /path/to/your/project --with-lsp "pylsp"
octocode mcp --path /path/to/your/project --with-lsp "typescript-language-server --stdio"

# Use with Claude Desktop or other MCP-compatible tools
# Provides: semantic_search, graphrag_search, memorize, remember, forget
# With LSP: lsp_goto_definition, lsp_hover, lsp_find_references, lsp_completion
```

### 5. Multi-Query Search (NEW!)
```bash
# Single query (traditional)
octocode search "authentication"

# Multi-query for comprehensive results
octocode search "authentication" "middleware"
octocode search "jwt" "token" "validation"
octocode search "database" "connection" "pool"

# Works with all search modes
octocode search "error" "handling" --mode code
octocode search "api" "documentation" --mode docs

# MCP also supports multi-query
# In Claude Desktop: semantic_search with query: ["auth", "middleware"]
```

**How Multi-Query Works:**
- **Parallel Processing**: Each query runs simultaneously for speed
- **Smart Deduplication**: Same code blocks from different queries shown once
- **Relevance Boosting**: Results matching multiple queries get higher scores
- **Maximum 3 queries**: Optimal balance of functionality vs performance
- **Same Output Format**: Results look identical to single-query search

### 6. Pre-commit Hook Integration
```bash
# Automatic pre-commit integration when available
octocode commit --all

# Pre-commit runs automatically if:
# - pre-commit binary is in PATH
# - .pre-commit-config.yaml exists
# - --no-verify flag is not used

# Skip pre-commit hooks when needed
octocode commit --no-verify

# Pre-commit behavior:
# - With --all: runs "pre-commit run --all-files"
# - Without --all: runs "pre-commit run" (staged files only)
# - Modified files are automatically re-staged
# - AI commit message generated after pre-commit completes
```

**Pre-commit Integration Features:**
- **Smart Detection**: Automatically detects pre-commit availability and configuration
- **Intelligent Execution**: Uses `--all-files` when `--all` flag is specified
- **Auto Re-staging**: Modified files are automatically added back to staging area
- **Seamless Workflow**: AI commit message generation happens after pre-commit
- **Silent Fallback**: No errors if pre-commit is not available or configured

### 7. Memory Management
```bash
# Store important insights and decisions
octocode memory memorize \
  --title "Authentication Bug Fix" \
  --content "Fixed JWT token validation in auth middleware" \
  --memory-type bug_fix \
  --tags security,jwt,auth

# Search your memory with semantic similarity
octocode memory remember "JWT authentication issues"

# Get memories by type, tags, or files
octocode memory by-type bug_fix
octocode memory by-tags security,auth
octocode memory for-files src/auth.rs

# Clear all memory data (useful for testing)
octocode memory clear-all --yes
```

### 8. Advanced Features
```bash
# Enable GraphRAG with AI descriptions (requires OpenRouter API key)
octocode config --graphrag-enabled true
octocode index

# Search the knowledge graph
octocode graphrag search --query "authentication modules"

# Watch for changes
octocode watch
```

## 📋 Command Reference

| Command | Description | Example |
|---------|-------------|---------|
| `octocode index` | Index the codebase | `octocode index` |
| `octocode search <query>` | Semantic code search (supports multiple queries) | `octocode search "error handling"` or `octocode search "auth" "middleware"` |
| `octocode graphrag <operation>` | Knowledge graph operations | `octocode graphrag search --query "auth"` |
| `octocode view [pattern]` | View code signatures | `octocode view "src/**/*.rs" --md` |
| `octocode commit` | AI-powered git commit | `octocode commit --all` |
| `octocode review` | Code review assistant | `octocode review --focus security` |
| `octocode release` | AI-powered release management | `octocode release --dry-run` |
| `octocode memory <operation>` | Memory management | `octocode memory remember "auth bugs"` |
| `octocode mcp` | Start MCP server | `octocode mcp --with-lsp "rust-analyzer"` |
| `octocode watch` | Auto-index on changes | `octocode watch --quiet` |
| `octocode config` | Manage configuration | `octocode config --show` |

## 🧠 Memory Management

Octocode includes a powerful memory system for storing and retrieving project insights, decisions, and context using semantic search and relationship mapping.

### Memory Operations

| Command | Description | Example |
|---------|-------------|---------|
| `memorize` | Store new information | `octocode memory memorize --title "Bug Fix" --content "Details..."` |
| `remember` | Search memories semantically | `octocode memory remember "authentication issues"` |
| `forget` | Delete specific memories | `octocode memory forget --memory-id abc123` |
| `update` | Update existing memory | `octocode memory update abc123 --add-tags security` |
| `get` | Retrieve memory by ID | `octocode memory get abc123` |
| `recent` | List recent memories | `octocode memory recent --limit 10` |
| `by-type` | Filter by memory type | `octocode memory by-type bug_fix` |
| `by-tags` | Filter by tags | `octocode memory by-tags security,auth` |
| `for-files` | Find memories for files | `octocode memory for-files src/auth.rs` |
| `stats` | Show memory statistics | `octocode memory stats` |
| `cleanup` | Remove old memories | `octocode memory cleanup` |
| `clear-all` | **Delete all memories** | `octocode memory clear-all --yes` |
| `relate` | Create relationships | `octocode memory relate source-id target-id` |

### Memory Types
- `code` - Code-related insights and patterns
- `bug_fix` - Bug reports and solutions
- `feature` - Feature implementations and decisions
- `architecture` - Architectural decisions and patterns
- `performance` - Performance optimizations and metrics
- `security` - Security considerations and fixes
- `testing` - Test strategies and results
- `documentation` - Documentation notes and updates

### Examples

```bash
# Store a bug fix with context
octocode memory memorize \
  --title "JWT Token Validation Fix" \
  --content "Fixed race condition in token refresh logic by adding mutex lock" \
  --memory-type bug_fix \
  --importance 0.8 \
  --tags security,jwt,race-condition \
  --files src/auth/jwt.rs,src/middleware/auth.rs

# Search for authentication-related memories
octocode memory remember "JWT authentication problems" \
  --memory-types bug_fix,security \
  --min-relevance 0.7

# Get all security-related memories
octocode memory by-tags security --format json

# Clear all memory data (useful for testing/reset)
octocode memory clear-all --yes
```

## 🚀 Release Management

Octocode provides intelligent release management with AI-powered version calculation and automatic changelog generation.

### Features
- **AI Version Calculation**: Analyzes commit history using conventional commits to determine semantic version bumps
- **Automatic Changelog**: Generates structured changelogs from commit messages
- **Multi-Project Support**: Works with Rust (Cargo.toml), Node.js (package.json), PHP (composer.json), and Go (go.mod) projects
- **Git Integration**: Creates release commits and annotated tags automatically
- **Dry Run Mode**: Preview changes before execution

### Usage

```bash
# Preview what would be done (recommended first step)
octocode release --dry-run

# Create a release with AI version calculation
octocode release

# Force a specific version
octocode release --force-version "2.0.0"

# Skip confirmation prompt
octocode release --yes

# Use custom changelog file
octocode release --changelog "HISTORY.md"
```

### How It Works

1. **Project Detection**: Automatically detects project type (Rust, Node.js, PHP, Go)
2. **Version Analysis**: Gets current version from project files or git tags
3. **Commit Analysis**: Analyzes commits since last release using conventional commit format
4. **AI Calculation**: Uses LLM to determine appropriate version bump (major/minor/patch)
5. **Changelog Generation**: Creates structured changelog with categorized changes
6. **File Updates**: Updates project files with new version (Cargo.toml, package.json, composer.json, VERSION)
7. **Git Operations**: Creates release commit and annotated tag

### Conventional Commits Support

The release command works best with conventional commit format:
- `feat:` → Minor version bump
- `fix:` → Patch version bump
- `BREAKING CHANGE` or `!` → Major version bump
- `chore:`, `docs:`, `style:`, etc. → Patch version bump

### Example Output

```
🚀 Starting release process...

📦 Project type detected: Rust (Cargo.toml)
📌 Current version: 0.1.0
📋 Analyzing commits since: v0.1.0
📊 Found 5 commits to analyze

🎯 Version calculation:
   Current: 0.1.0
   New:     0.2.0
   Type:    minor
   Reason:  New features added without breaking changes

📝 Generated changelog entry:
═══════════════════════════════════
## [0.2.0] - 2025-01-27

### ✨ Features

- Add release command with AI version calculation
- Implement dry-run mode for safe previews

### 🐛 Bug Fixes

- Fix memory search relevance scoring
═══════════════════════════════════

🔍 DRY RUN - No changes would be made
```

## 🔧 Configuration

Octocode stores configuration in `~/.local/share/octocode/config.toml`.

### Required Setup
```bash
# Set Voyage AI API key (required for embeddings)
export VOYAGE_API_KEY="your-voyage-api-key"

# Optional: Set OpenRouter API key for LLM features
export OPENROUTER_API_KEY="your-openrouter-api-key"
```

### Advanced Configuration
```bash
# View current configuration
octocode config --show

# Use local models (macOS only - requires building from source)
octocode config \
  --code-embedding-model "fastembed:jinaai/jina-embeddings-v2-base-code" \
  --text-embedding-model "fastembed:sentence-transformers/all-MiniLM-L6-v2-quantized"

# Use different cloud embedding provider
octocode config \
  --code-embedding-model "jina:jina-embeddings-v2-base-code" \
  --text-embedding-model "jina:jina-embeddings-v2-base-en"

# Enable GraphRAG with AI descriptions
octocode config --graphrag-enabled true

# Set custom OpenRouter model
octocode config --model "openai/gpt-4o-mini"
```

### Default Models
- **Code embedding**: `voyage:voyage-code-3` (Voyage AI)
- **Text embedding**: `voyage:voyage-3.5-lite` (Voyage AI)
- **LLM**: `openai/gpt-4.1-mini` (via OpenRouter)

### Platform Support
- **Windows/Linux**: Cloud embeddings only (Voyage AI, Jina AI, Google)
- **macOS**: Local embeddings available (FastEmbed, SentenceTransformer) + cloud options

## 📚 Documentation

- **[Architecture](doc/ARCHITECTURE.md)** - Core components and system design
- **[Configuration](doc/CONFIGURATION.md)** - Setup and configuration options
- **[Advanced Usage](doc/ADVANCED_USAGE.md)** - Advanced features and workflows
- **[LSP Integration](doc/LSP_INTEGRATION.md)** - Language Server Protocol integration guide
- **[Contributing](doc/CONTRIBUTING.md)** - Development setup and contribution guidelines
- **[Performance](doc/PERFORMANCE.md)** - Performance metrics and optimization tips

## 🔒 Privacy & Security

- **🏠 Local-first option**: FastEmbed and SentenceTransformer run entirely offline (macOS only)
- **🔑 Secure storage**: API keys stored locally, environment variables supported
- **📁 Respects .gitignore**: Never indexes sensitive files or directories
- **🛡️ MCP security**: Server runs locally with no external network access for search
- **🌐 Cloud embeddings**: Voyage AI and other providers process only file metadata, not source code

## 🌐 Supported Languages

| Language | Extensions | Features |
|----------|------------|----------|
| **Rust** | `.rs` | Full AST parsing, pub/use detection, module structure |
| **Python** | `.py` | Import/class/function extraction, docstring parsing |
| **JavaScript** | `.js`, `.jsx` | ES6 imports/exports, function declarations |
| **TypeScript** | `.ts`, `.tsx` | Type definitions, interface extraction |
| **Go** | `.go` | Package/import analysis, struct/interface parsing |
| **PHP** | `.php` | Class/function extraction, namespace support |
| **C++** | `.cpp`, `.hpp`, `.h` | Include analysis, class/function extraction |
| **Ruby** | `.rb` | Class/module extraction, method definitions |
| **JSON** | `.json` | Structure analysis, key extraction |
| **Bash** | `.sh`, `.bash` | Function and variable extraction |
| **Markdown** | `.md` | Document section indexing, header extraction |

## 🤝 Support & Community

- **🐛 Issues**: [GitHub Issues](https://github.com/Muvon/octocode/issues)
- **📧 Email**: [opensource@muvon.io](mailto:opensource@muvon.io)
- **🏢 Company**: Muvon Un Limited (Hong Kong)

## ⚖️ License

This project is licensed under the **Apache License 2.0** - see the [LICENSE](LICENSE) file for details.

---

**Built with ❤️ by the Muvon team in Hong Kong**
