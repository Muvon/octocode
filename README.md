<div align="center">

<img src="https://raw.githubusercontent.com/Muvon/octocode/master/assets/logo.svg" width="120" alt="Octocode Logo">

# Octocode

### **AI-Powered Code Intelligence with Built-in MCP Server**

[![GitHub stars](https://img.shields.io/github/stars/Muvon/octocode?style=social)](https://github.com/Muvon/octocode/stargazers)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Rust](https://img.shields.io/badge/Rust-1.82%2B-orange.svg)](https://www.rust-lang.org)
[![Release](https://img.shields.io/github/v/release/Muvon/octocode)](https://github.com/Muvon/octocode/releases)

**Transform your codebase into a queryable knowledge graph. Ask questions in plain English, get precise answers.**

[🚀 Quick Start](#quick-start) • [📖 Documentation](#documentation) • [🔌 MCP Server](#mcp-server) • [🌐 Website](https://octocode.muvon.io)

<a href="https://glama.ai/mcp/servers/Muvon/octocode">
  <img width="300" src="https://glama.ai/mcp/servers/Muvon/octocode/badge" alt="Octocode MCP server" />
</a>

</div>

---

## 🤔 Why Octocode?

**You know that feeling** — staring at a 100k+ line codebase, trying to find where authentication is handled, or how the database layer connects to the API. Traditional search (`grep`, `ripgrep`) only finds exact matches. Octocode understands *meaning*.

```bash
# Instead of this:
grep -r "auth" --include="*.rs" | head -20  # 847 results, mostly noise

# Do this:
octocode search "how is user authentication implemented"
# → 3 relevant files with context and relationships
```

Octocode builds a **semantic knowledge graph** of your entire codebase using tree-sitter AST parsing and vector embeddings. It connects your code to AI assistants via the **Model Context Protocol (MCP)** — giving Claude, Cursor, and other AI tools deep contextual understanding of your project.

## ✨ What Makes It Different

| Feature | Traditional Search | Octocode |
|---------|-------------------|----------|
| **Query style** | Exact keywords | Natural language |
| **Results** | Text matches | Semantic meaning + relationships |
| **Context** | Single file | Cross-file dependencies & imports |
| **AI integration** | None | Native MCP server + LSP |
| **Speed** | Instant | <2s indexing, instant queries |

**Built with Rust** for performance. **Local-first** for privacy. **Open source** (Apache 2.0) for transparency.

## 🚀 Quick Start

### 1. Install (30 seconds)

```bash
# Universal installer (Linux, macOS, Windows)
curl -fsSL https://raw.githubusercontent.com/Muvon/octocode/master/install.sh | sh

# Or with Cargo
cargo install --git https://github.com/Muvon/octocode
```

### 2. Configure API Keys (1 minute)

```bash
# Required: Voyage AI (200M free tokens/month)
export VOYAGE_API_KEY="your-voyage-api-key"

# Optional: OpenRouter (for AI commit messages, code review)
export OPENROUTER_API_KEY="your-openrouter-api-key"
```

[Get Voyage AI key](https://www.voyageai.com/) • [Get OpenRouter key](https://openrouter.ai/)

### 3. Index Your Codebase (2-5 minutes)

```bash
cd /your/project
octocode index
# → Indexed 12,847 blocks across 342 files
```

### 4. Search with Natural Language

```bash
# Single query
octocode search "HTTP middleware pattern"

# Multi-query for comprehensive results
octocode search "authentication" "middleware" "session"

# With filters
octocode search "database connection pool" --lang rust
```

### 5. Connect AI Assistants (MCP Server)

```bash
# Start MCP server for Claude Desktop, Cursor, etc.
octocode mcp --path /your/project

# Or HTTP mode for custom integrations
octocode mcp --bind 0.0.0.0:12345
```

## 🔌 MCP Server Integration

Octocode includes a **built-in MCP server** that exposes your codebase as tools to AI assistants:

| Tool | What It Does |
|------|--------------|
| `search_code` | Semantic search across your entire codebase |
| `get_file_context` | Retrieve specific files with surrounding context |
| `query_graph` | Ask questions about code relationships and dependencies |
| `get_symbols` | Find functions, structs, classes by name or purpose |

**Works with:** Claude Desktop • Cursor • Any MCP-compatible client

See [MCP Integration Guide](doc/MCP_INTEGRATION.md) for setup instructions.

## 🎯 Use Cases

- **🆕 Onboarding** — "How does the auth system work?" → Get the full picture in seconds
- **🔍 Code Archaeology** — Find legacy code patterns without knowing exact names
- **🤖 AI Pair Programming** — Give your AI assistant complete codebase context
- **📝 Refactoring** — Understand dependencies before making changes
- **🔎 Code Review** — "Show me all error handling in the API layer"

## 🌐 Supported Languages

| Language | Extensions | Features |
|----------|------------|----------|
| **Rust** | `.rs` | Full AST parsing, pub/use detection, module structure |
| **Python** | `.py` | Import/class/function extraction, docstring parsing |
| **TypeScript/JavaScript** | `.ts`, `.tsx`, `.js`, `.jsx` | ES6 imports/exports, type definitions |
| **Go** | `.go` | Package/import analysis, struct/interface parsing |
| **PHP** | `.php` | Class/function extraction, namespace support |
| **C++** | `.cpp`, `.hpp`, `.h` | Include analysis, class/function extraction |
| **Ruby** | `.rb` | Class/module extraction, method definitions |
| **Java** | `.java` | Import analysis, class/method extraction |
| **JSON** | `.json` | Structure analysis, key extraction |
| **Bash** | `.sh`, `.bash` | Function and variable extraction |
| **Markdown** | `.md` | Document section indexing, header extraction |

*Plus: CSS, Lua, Svelte, and more via tree-sitter*

## 📚 Documentation

- **[Getting Started](doc/GETTING_STARTED.md)** — First steps and basic workflow
- **[Installation Guide](INSTALL.md)** — Detailed methods and building from source
- **[MCP Integration](doc/MCP_INTEGRATION.md)** — Connect to Claude, Cursor, etc.
- **[Commands Reference](doc/COMMANDS.md)** — Complete CLI reference
- **[Configuration](doc/CONFIGURATION.md)** — Templates and customization
- **[API Keys](doc/API_KEYS.md)** — Provider setup guide
- **[Architecture](doc/ARCHITECTURE.md)** — How it works under the hood
- **[Contributing](doc/CONTRIBUTING.md)** — Development setup

## 🔒 Privacy & Security

- **🏠 Local-first** — FastEmbed runs entirely offline (macOS)
- **🔐 Secure** — API keys stored locally, env vars supported
- **🚫 Respects .gitignore** — Never indexes sensitive files
- **🛡️ MCP security** — Local-only server, no external network for search
- **📤 Cloud-safe** — Embeddings process only metadata, never source code

## 🤝 Community & Support

- ⭐ **Star us on GitHub** — It really helps!
- 🐛 [Report Issues](https://github.com/Muvon/octocode/issues)
- 💬 [Discussions](https://github.com/Muvon/octocode/discussions)
- 📧 [opensource@muvon.io](mailto:opensource@muvon.io)
- 🌐 [muvon.io](https://muvon.io)

## ⚖️ License

Apache License 2.0 — See [LICENSE](LICENSE) for details.

---

<div align="center">

**Built with 🦀 Rust by [Muvon](https://muvon.io) in Hong Kong**

[⭐ Star](https://github.com/Muvon/octocode) • [🍴 Fork](https://github.com/Muvon/octocode/fork) • [📣 Share](https://twitter.com/intent/tweet?text=Octocode%20-%20AI-powered%20code%20intelligence%20with%20built-in%20MCP%20server&url=https://github.com/Muvon/octocode)

</div>

## Hosted deployment

A hosted deployment is available on [Fronteir AI](https://fronteir.ai/mcp/muvon-octocode).

