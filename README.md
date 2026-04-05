<div align="center">

<img src="https://raw.githubusercontent.com/Muvon/octocode/master/logo.svg" width="240" alt="Octocode">

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

# Search commit history
octocode search "authentication refactor" --mode commits
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
| `semantic_search` | Semantic search across code, docs, text, and commits |
| `view_signatures` | View file signatures and code structure by glob patterns |
| `graphrag` | Query code relationships, dependencies, and architecture |

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

<details>
<summary><strong>📊 Retrieval Quality Benchmark</strong></summary>

We measure semantic search quality using a hand-annotated ground truth dataset of 254 queries (127 code + 127 docs) with precise line-range annotations. Each query has 1–3 expected results scored by relevance.

Tested on commit [`b1771ba`](https://github.com/Muvon/octocode/commit/b1771ba) with [benchmark config](benchmark/config.toml) (contextual retrieval, Voyage reranker, RaBitQ quantization).

<details>
<summary><strong>Code search</strong> (<code>--mode code</code>) — Hit@10: 0.953, MRR: 0.776</summary>

| Metric | Score |
|--------|-------|
| Hit@5 | 0.929 (118/127) |
| Hit@10 | 0.953 (121/127) |
| MRR | 0.776 |
| NDCG@10 | 0.801 |
| Recall@5 | 0.902 |
| Recall@10 | 0.921 |

**Missed queries** (6 of 127):

| # | Query | Expected | Got (top 1) |
|---|-------|----------|-------------|
| 43 | how to set up MCP proxy for managing multiple repositories | `doc/MCP_INTEGRATION.md:286-311` | `doc/MCP_INTEGRATION.md:286-4` |
| 51 | what are the prerequisites before using octocode | `doc/GETTING_STARTED.md:6-12` | `doc/CONTRIBUTING.md:7-33` |
| 59 | what to do when hitting API rate limits | `doc/GETTING_STARTED.md:209-216` | `doc/PERFORMANCE.md:304-356` |
| 75 | typical performance metrics for small medium and large projects | `doc/PERFORMANCE.md:4-13` | `doc/PERFORMANCE.md:414-14` |
| 112 | how to install octocode on different operating systems | `INSTALL.md:4-14` | `INSTALL.md:49-70` |
| 115 | how to fix macOS Gatekeeper blocking the binary | `INSTALL.md:199-206` | `INSTALL.md:198-119` |

</details>

<details>
<summary><strong>Documentation search</strong> (<code>--mode docs</code>) — Hit@10: 0.992, MRR: 0.895</summary>

| Metric | Score |
|--------|-------|
| Hit@5 | 0.992 (126/127) |
| Hit@10 | 0.992 (126/127) |
| MRR | 0.895 |
| NDCG@10 | 0.906 |
| Recall@5 | 0.962 |
| Recall@10 | 0.974 |

**Missed queries** (1 of 127):

| # | Query | Expected | Got (top 1) |
|---|-------|----------|-------------|
| 105 | how does the system ensure two developers get the same database path | `src/storage.rs:60-83` | `src/mcp/proxy.rs:631-644` |

</details>

Metrics: **Hit@k** (did the answer appear?), **MRR** (how high?), **NDCG@10** (are best results ranked first?), **Recall@k** (how many found?). See [benchmark/](benchmark/) for methodology, scoring script, and the full dataset.

</details>

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
