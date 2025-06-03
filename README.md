# Octocode - Intelligent Code Indexer and Graph Builder

**© 2025 Muvon Un Limited (Hong Kong)** | [Website](https://muvon.io) | [Product Page](https://octocode.muvon.io)

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org)

## 🚀 Overview

Octocode is a powerful code indexer and semantic search engine that builds intelligent knowledge graphs of your codebase. It combines advanced AI capabilities with local-first design to provide deep code understanding, relationship mapping, and intelligent assistance for developers.

## ✨ Key Features

### 🔍 **Semantic Code Search**
- Natural language queries across your entire codebase
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
- Smart commit message generation
- Code review with best practices analysis
- **Memory system** for storing insights, decisions, and context
- **Semantic memory search** with vector similarity
- **Memory relationships** and automatic context linking
- Multiple LLM support via OpenRouter

### 🔌 **MCP Server Integration**
- Built-in Model Context Protocol server
- Seamless integration with AI assistants (Claude Desktop, etc.)
- Real-time file watching and auto-reindexing
- Rich tool ecosystem for code analysis

### ⚡ **Performance & Flexibility**
- Local embedding models (FastEmbed, SentenceTransformer)
- Cloud providers (Jina AI, Voyage AI, Google)
- Lance columnar database for fast vector search
- Incremental indexing and git-aware optimization

## 📦 Installation

### Prerequisites
- **Rust 1.70+** ([install from rustup.rs](https://rustup.rs/))
- **Git** (for repository features)

### Build from Source
```bash
git clone https://github.com/muvon/octocode.git
cd octocode
cargo build --release
```

The binary will be available at `target/release/octocode`.

## 🚀 Quick Start

### 1. Basic Setup
```bash
# Index your current directory
octocode index

# Search your codebase
octocode search "HTTP request handling"

# View code signatures
octocode view "src/**/*.rs"
```

### 2. AI-Powered Git Workflow
```bash
# Generate intelligent commit messages
git add .
octocode commit

# Review code for best practices
octocode review
```

### 3. MCP Server for AI Assistants
```bash
# Start MCP server
octocode mcp

# Use with Claude Desktop or other MCP-compatible tools
# Provides: search_code, search_graphrag, memorize, remember, forget
```

### 4. Memory Management
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

### 5. Advanced Features
```bash
# Enable GraphRAG with AI descriptions
export OPENROUTER_API_KEY="your-key"
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
| `octocode index` | Index the codebase | `octocode index --reindex` |
| `octocode search <query>` | Semantic code search | `octocode search "error handling"` |
| `octocode graphrag <operation>` | Knowledge graph operations | `octocode graphrag search --query "auth"` |
| `octocode view [pattern]` | View code signatures | `octocode view "src/**/*.rs" --md` |
| `octocode commit` | AI-powered git commit | `octocode commit --all` |
| `octocode review` | Code review assistant | `octocode review --focus security` |
| `octocode memory <operation>` | Memory management | `octocode memory remember "auth bugs"` |
| `octocode mcp` | Start MCP server | `octocode mcp --debug` |
| `octocode watch` | Auto-reindex on changes | `octocode watch --quiet` |
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

## 🔧 Configuration

Octocode stores configuration in `~/.local/share/octocode/config.toml`. Quick setup:

```bash
# View current configuration
octocode config --show

# Use local models (no API keys required) 
octocode config \
  --code-embedding-model "fastembed:all-MiniLM-L6-v2" \
  --text-embedding-model "fastembed:multilingual-e5-small"

# Enable GraphRAG with AI descriptions
export OPENROUTER_API_KEY="your-key"
octocode config --graphrag-enabled true

# Set custom OpenRouter model
octocode config --model "openai/gpt-4o-mini"
```

**Default Models:**
- Code embedding: `fastembed:jinaai/jina-embeddings-v2-base-code`
- Text embedding: `fastembed:sentence-transformers/all-MiniLM-L6-v2-quantized`
- LLM: `openai/gpt-4.1-mini` (via OpenRouter)

## 📚 Documentation

- **[Architecture](doc/ARCHITECTURE.md)** - Core components and system design
- **[Configuration](doc/CONFIGURATION.md)** - Setup and configuration options  
- **[Advanced Usage](doc/ADVANCED_USAGE.md)** - Advanced features and workflows
- **[Contributing](doc/CONTRIBUTING.md)** - Development setup and contribution guidelines
- **[Performance](doc/PERFORMANCE.md)** - Performance metrics and optimization tips

## 🔒 Privacy & Security

- **🏠 Local-first**: FastEmbed and SentenceTransformer run entirely offline
- **🔐 No code upload**: Only file metadata sent to AI APIs (when enabled)
- **🔑 Secure storage**: API keys stored locally, environment variables supported
- **📁 Respects .gitignore**: Never indexes sensitive files or directories
- **🛡️ MCP security**: Server runs locally with no external network access for search

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

- **🐛 Issues**: [GitHub Issues](https://github.com/muvon/octocode/issues)
- **📧 Email**: [opensource@muvon.io](mailto:opensource@muvon.io)
- **🏢 Company**: Muvon Un Limited (Hong Kong)

## ⚖️ License

This project is licensed under the **Apache License 2.0** - see the [LICENSE](LICENSE) file for details.

---

**Built with ❤️ by the Muvon team in Hong Kong**