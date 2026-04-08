<div align="center">

<img src="https://raw.githubusercontent.com/Muvon/octocode/master/logo.svg" width="240" alt="Octocode">

### **Structural Code Intelligence for AI Agents — MCP Server + Knowledge Graph + Semantic Search**

[![GitHub stars](https://img.shields.io/github/stars/Muvon/octocode?style=social)](https://github.com/Muvon/octocode/stargazers)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Rust](https://img.shields.io/badge/Rust-1.82%2B-orange.svg)](https://www.rust-lang.org)
[![Release](https://img.shields.io/github/v/release/Muvon/octocode)](https://github.com/Muvon/octocode/releases)

**Give your AI assistant a brain for your codebase.** Octocode transforms your project into a navigable knowledge graph that Claude, Cursor, and other AI agents can search, understand, and navigate.

[🚀 Quick Start](#-quick-start) • [🤖 MCP Integration](#-mcp-server-integration) • [📖 Documentation](#-documentation) • [🌐 Website](https://octocode.muvon.io)

<a href="https://glama.ai/mcp/servers/Muvon/octocode">
  <img width="300" src="https://glama.ai/mcp/servers/Muvon/octocode/badge" alt="Octocode MCP server" />
</a>

</div>

---

## 🤖 Built for AI Agents

**The Problem:** AI assistants are blind to your codebase. They can't search your files, understand dependencies, or remember context across sessions.

**The Solution:** Octocode's MCP server gives AI agents:
- 🔍 **Semantic search** — Find code by meaning, not keywords
- 🕸️ **Knowledge graph** — Navigate imports, calls, and dependencies
- 📝 **Code signatures** — View structure without reading entire files
- 🧠 **Persistent memory** — Remember decisions across conversations

**Works with:** Claude Desktop • Cursor • Windsurf • Any MCP-compatible AI

```json
// Add to your AI assistant config
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/your/project"]
    }
  }
}
```

Now your AI assistant can:
```
You: "Where is authentication handled?"
AI: *searches your codebase* "Authentication is in src/middleware/auth.rs,
    which imports jwt.rs for token validation and calls user_store.rs for lookup."

You: "What files depend on the payment module?"
AI: *queries knowledge graph* "src/api/handlers/payment.rs imports payment/mod.rs,
    which is also used by src/workers/refund.rs and src/cron/billing.rs"

You: "Remember this bug fix for future reference"
AI: *stores in memory* "Got it. I'll remember this authentication bypass fix
    and apply similar patterns when reviewing security code."
```

## 🤔 Why Octocode?

**Standard RAG treats your code as flat text chunks.** It finds similar-sounding snippets but has no idea that `auth_middleware.rs` imports `jwt.rs`, calls `user_store.rs`, and is wired into `router.rs`. Octocode understands *structure*.

```
# Semantic search finds the right code
octocode search "authentication middleware"
→ src/middleware/auth.rs | Similarity 0.923

# GraphRAG reveals the full dependency chain
octocode graphrag get-relationships --node_id src/middleware/auth.rs
Outgoing:
  imports → jwt (src/auth/jwt.rs): token validation logic
  calls   → user_store (src/db/user_store.rs): user lookup by token
Incoming:
  imports ← router (src/router.rs): wires auth into the request pipeline
```

Octocode uses **tree-sitter AST parsing** to extract real symbols (functions, imports, dependencies), builds a **GraphRAG knowledge graph** of relationships between files, and exposes everything via **MCP** — so AI tools can *navigate* your project architecture, not just search it.

## 🔬 How It Works

```
Source Code → Tree-sitter AST → Symbols & Relationships → Knowledge Graph
                                        ↓
                    Embeddings + Hybrid Search + Reranking → MCP Server
```

1. **AST Parsing** — tree-sitter extracts real code symbols (functions, classes, imports), not arbitrary text chunks
2. **Knowledge Graph** — GraphRAG maps relationships between files: `imports`, `calls`, `implements`, `extends`, `configures`, and 9 more types — each with importance weighting
3. **Hybrid Search** — semantic similarity + BM25 full-text search + reranking — not just vector embeddings
4. **MCP Server** — exposes `semantic_search`, `view_signatures`, and `graphrag` tools to any MCP-compatible client

## ✨ What Makes It Different

| | Standard RAG | Doc Lookup Tools | **Octocode** |
|---|---|---|---|
| **Indexes** | Text chunks | External library docs | Your codebase structure (AST) |
| **Understands** | Similar text | API specs & usage | Functions, imports, dependencies |
| **Cross-file** | No | No | Yes — navigates the dependency graph |
| **Relationships** | No | No | `imports`, `calls`, `implements`, `extends`... |
| **AI integration** | Varies | MCP | Native MCP server + LSP |

> **Doc tools give AI the manual for libraries you use. Octocode gives AI the blueprint of how you put them together.**

**Built with Rust** for performance. **Local-first** for privacy. **Open source** (Apache 2.0) for transparency.

## 🚀 Quick Start

### 1. Install

```bash
# Universal installer (Linux, macOS, Windows)
curl -fsSL https://raw.githubusercontent.com/Muvon/octocode/master/install.sh | sh

# macOS with Homebrew
brew install muvon/tap/octocode
```

<details>
<summary><strong>Other installation methods</strong></summary>

```bash
# Cargo (build from source)
cargo install --git https://github.com/Muvon/octocode

# Download binary from releases
# https://github.com/Muvon/octocode/releases
```

See [Installation Guide](INSTALL.md) for platform-specific instructions.
</details>

### 2. Set Up API Keys

```bash
# Required: Embedding provider (Voyage AI has 200M free tokens/month)
export VOYAGE_API_KEY="your-voyage-api-key"

# Optional: LLM for commit messages, code review
export OPENROUTER_API_KEY="your-openrouter-api-key"
```

**Get your Voyage API key:** [voyageai.com](https://www.voyageai.com/) (free tier available)

<details>
<summary><strong>Other embedding providers</strong></summary>

Octocode supports multiple embedding providers:

```bash
# OpenAI
export OPENAI_API_KEY="your-key"
octocode config --code-embedding-model "openai:text-embedding-3-small"

# Jina AI
export JINA_API_KEY="your-key"
octocode config --code-embedding-model "jina:jina-embeddings-v3"

# Google
export GOOGLE_API_KEY="your-key"
octocode config --code-embedding-model "google:text-embedding-005"
```

See [API Keys guide](doc/API_KEYS.md) for all supported providers.
</details>

### 3. Index Your Codebase

```bash
cd /your/project
octocode index
# → Indexed 12,847 blocks across 342 files
```

### 4. Search Your Code

```bash
# Natural language search
octocode search "authentication middleware"

# Multi-query for broader results
octocode search "auth" "middleware" "session"

# Filter by language
octocode search "database connection pool" --lang rust

# Search commit history
octocode search "authentication refactor" --mode commits
```

### 5. Connect Your AI Assistant

Add to your MCP client config (Claude Desktop, Cursor, Windsurf):

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/your/project"]
    }
  }
}
```

Done! Your AI assistant now understands your codebase structure.

## 🔌 MCP Server Integration

Octocode includes a **built-in MCP server** that exposes your codebase as tools to AI assistants. This is the primary way to use Octocode — give your AI assistant direct access to search and navigate your code.

### Available Tools

| Tool | What It Does |
|------|--------------|
| `semantic_search` | Find code by meaning — "authentication flow", "error handling", "database queries" |
| `view_signatures` | View file structure — function signatures, class definitions, imports |
| `graphrag` | Query relationships — "what calls this function?", "what does this module import?" |
| `structural_search` | AST pattern matching — find `.unwrap()` calls, `new` instantiations, specific patterns |

### Conversational AI Examples

Once connected, your AI assistant can answer questions about your codebase:

```
You: "Where is user authentication implemented?"
AI: *uses semantic_search* "Found in src/auth/login.rs. The authenticate() function
    validates credentials against the database, generates a JWT token, and stores
    the session in Redis."

You: "What files depend on the payment module?"
AI: *uses graphrag* "src/api/handlers/payment.rs imports payment/mod.rs, which is also
    used by src/workers/refund.rs and src/cron/billing.rs. The payment module exports
    process_payment() and validate_transaction() functions."

You: "Show me all error handling in the API layer"
AI: *uses structural_search* "Found 23 error handling patterns in src/api/:
    - 15 use Result<T, ApiError> with explicit error types
    - 8 use .unwrap() (potential panics in handlers/user.rs:42, handlers/auth.rs:87)
    - 3 use .expect() with custom messages"
```

### Quick Setup

**Octomind (Recommended)** — Zero setup, Octocode pre-configured:
```bash
curl -fsSL https://raw.githubusercontent.com/muvon/octomind/master/install.sh | bash
octomind run developer:rust
```

**Claude Code (CLI)** — Command-line setup:
```bash
claude mcp add octocode -- octocode mcp --path /path/to/your/project
```

**Claude Desktop / Cursor / Windsurf** — Add to config:
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

**Config locations:**
- Claude Desktop: `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS)
- Cursor: `~/.cursor/mcp.json` or Settings → MCP Servers
- Windsurf: Settings → MCP

📖 **[Complete MCP Client Setup Guide](doc/MCP_CLIENTS.md)** — Detailed instructions for 15+ clients including VS Code (Cline/Continue), Zed, Replit, and more.

## 🎯 What Can You Do With It?

**New developer onboarding:**
```
You: "How does the authentication system work?"
AI: *searches and navigates* "Authentication starts in src/middleware/auth.rs which
    validates JWT tokens. It calls src/auth/jwt.rs for token verification, which uses
    the public key from config. Failed auth returns 401 via src/errors/auth_error.rs.
    Sessions are stored in Redis via src/cache/session.rs."
```

**Code archaeology:**
```
You: "Find all places we handle database errors"
AI: *structural search* "Found 47 error handling patterns:
    - 32 use Result<T, DbError> with proper error types
    - 15 use .unwrap() (potential issues in src/db/user.rs:23, src/db/order.rs:156)
    - Recommend adding proper error handling to those locations"
```

**Refactoring with confidence:**
```
You: "What depends on the PaymentProcessor trait?"
AI: *queries graph* "src/api/handlers/checkout.rs, src/workers/refund_worker.rs,
    and src/cron/billing.rs all depend on PaymentProcessor. The trait is defined
    in src/domain/payment.rs and implemented by src/infrastructure/stripe.rs
    and src/infrastructure/paypal.rs."
```

**Code review assistance:**
```
You: "Review this PR for security issues"
AI: *analyzes changes* "The PR adds password hashing in src/auth/hash.rs. However,
    it uses SHA256 which is fast and vulnerable to brute force. Recommend using
    bcrypt or argon2 instead. Also found 3 instances of .unwrap() that could panic
    in production."
```

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
- **[MCP Client Setup](doc/MCP_CLIENTS.md)** — Connect to Claude, Cursor, Windsurf, and 15+ clients
- **[MCP Integration](doc/MCP_INTEGRATION.md)** — MCP server details and advanced configuration
- **[Commands Reference](doc/COMMANDS.md)** — Complete CLI reference
- **[Configuration](doc/CONFIGURATION.md)** — Templates and customization
- **[API Keys](doc/API_KEYS.md)** — Provider setup guide
- **[Architecture](doc/ARCHITECTURE.md)** — How it works under the hood
- **[Contributing](doc/CONTRIBUTING.md)** — Development setup

## 🔒 Privacy & Security

- **🏠 Local-first** — local embedding models available on supported platforms (macOS ARM default builds); cloud providers on all platforms
- **🔐 Secure** — API keys stored locally, env vars supported
- **🚫 Respects .gitignore** — Never indexes sensitive files
- **🛡️ MCP security** — Local-only server, no external network for search
- **📤 Cloud-safe** — Embeddings process only metadata, never source code

<details>
<summary><strong>📊 Retrieval Quality Benchmark</strong></summary>

We measure semantic search quality using a hand-annotated ground truth dataset of 254 queries (127 code + 127 docs) with precise line-range annotations. Each query has 1–3 expected results scored by relevance.

Tested on commit [`b1771ba`](https://github.com/Muvon/octocode/commit/b1771ba) with [benchmark config](benchmark/config.toml) (contextual retrieval, Voyage reranker, RaBitQ quantization).

<details>
<summary><strong>Documentation search</strong> (<code>--mode docs</code>) — Hit@10: 0.953, MRR: 0.776</summary>

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
<summary><strong>Code search</strong> (<code>--mode code</code>) — Hit@10: 0.992, MRR: 0.895</summary>

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
