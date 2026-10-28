<div align="center">

<img src="logo.svg" width="240" alt="Octocode logo">

# Octocode

**Code search and dependency graphs for AI assistants.**

[![CI](https://github.com/Muvon/octocode/actions/workflows/ci.yml/badge.svg)](https://github.com/Muvon/octocode/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fmuvon%2Foctocode%2Fbadges%2Fcoverage.json&style=flat-square)](https://github.com/Muvon/octocode/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/octocode)](https://crates.io/crates/octocode)
[![Release](https://img.shields.io/github/v/release/Muvon/octocode)](https://github.com/Muvon/octocode/releases)
[![Rust](https://img.shields.io/badge/Rust-1.95%2B-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

[Quick start](#quick-start) · [MCP integration](#mcp-integration) · [OctoFS](#use-octocode-with-octofs) · [Documentation](#documentation) · [Contributing](#contributing) · [Website](https://octomind.run/product/octocode)

<a href="https://glama.ai/mcp/servers/Muvon/octocode">
  <img width="300" src="https://glama.ai/mcp/servers/Muvon/octocode/badge" alt="Octocode MCP server listing on Glama" />
</a>

</div>

Octocode is a Rust CLI and [MCP](https://modelcontextprotocol.io) server for exploring codebases. It combines semantic search, structural search, and a live graph of symbols and dependencies. Connect it to Claude Code, Codex, Cursor, Windsurf, or another MCP client, or search directly from your terminal.

- **Find code by intent** with hybrid vector and keyword search, plus configurable reranking.
- **Locate definitions and usages** with structural search over the current source.
- **Trace relationships** between files and symbols: imports, calls, inheritance, and implementations.
- **Read signatures** without loading entire files; add a language server for definitions, references, and hover information.
- **Run locally** with the default embedding and reranking models, or configure remote providers.

## Quick start

### 1. Install

Use the installer on Linux or macOS, or in a Unix-compatible shell on Windows:

```bash
curl -fsSL https://raw.githubusercontent.com/Muvon/octocode/master/install.sh | sh
```

Alternatively, on macOS:

```bash
brew install muvon/tap/octocode
```

<details>
<summary>Other installation methods</summary>

Download a binary from [GitHub Releases](https://github.com/Muvon/octocode/releases/latest), or install the published crate:

```bash
cargo install octocode
```

Building from source requires Rust 1.95 or later and `protoc`. See the [installation guide](INSTALL.md) for platform requirements, build features, and manual installation.

</details>

Verify the installation:

```bash
octocode --version
```

### 2. Index and search

Run these commands in your project's Git repository:

```bash
cd /path/to/your/project
octocode index
octocode search "authentication middleware"
```

The default configuration uses local FastEmbed models for embeddings and reranking. No API key is required; model files download on first use. For a directory without Git, use `octocode index --no-git`.

```bash
# Combine related queries
octocode search "auth" "middleware" "session"

# Filter code results by language
octocode search "database connection pool" --language rust

# Search documentation
octocode search "deployment configuration" --mode docs
```

### 3. Connect your AI assistant

Choose your client in [MCP integration](#mcp-integration). Structural search, signatures, and the live dependency graph read the current source directly and work without an index. Semantic search uses the index created above.

## MCP integration

Install Octocode where your client can find it on `PATH`, and replace `/path/to/your/project` with your project's absolute directory path.

### Claude Code

```bash
claude mcp add --transport stdio octocode -- octocode mcp --path /path/to/your/project
claude mcp list
```

See the [Claude Code MCP documentation](https://code.claude.com/docs/en/mcp) for configuration scopes and connection details.

### Codex CLI

```bash
codex mcp add octocode -- octocode mcp --path /path/to/your/project
codex mcp list
```

See the [Codex MCP documentation](https://developers.openai.com/codex/mcp) for CLI and configuration-file options.

### Claude Desktop, Cursor, and Windsurf

Add an Octocode entry to your client's MCP configuration:

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

Restart the client after saving. The [MCP client setup guide](doc/MCP_CLIENTS.md) covers configuration locations and other clients.

### Octomind

[Octomind](https://github.com/Muvon/octomind) is a companion AI agent runtime. Its `developer:general` agent includes Octocode and OctoFS capabilities. After installing Octomind, start it in your project:

```bash
cd /path/to/your/project
octomind run developer:general
```

The first run may install required tools and ask for model credentials. See the [Octomind quick start](https://github.com/Muvon/octomind#quick-start) for installation and model access.

### Available tools

| Tool | Use it for | Requires an index |
|------|------------|-------------------|
| `semantic_search` | Find code, documentation, and indexed history by meaning | Yes |
| `structural_search` | Find definitions, usages, and AST patterns in current source | No |
| `view_signatures` | Read function signatures, declarations, and imports | No |
| `graphrag` | Explore live file and symbol relationships and dependency paths | No |
| `lsp_*` | Definitions, references, hover information, symbols, and completion | No; requires a language server |

To enable LSP tools, install a language server and add `--with-lsp` to the server command, for example:

```bash
octocode mcp --path /path/to/your/project --with-lsp rust-analyzer
```

By default, the MCP server uses the existing semantic index without updating it. Rerun `octocode index` after changes, or run `octocode watch` separately in the project directory. In-process indexing is available through `index.mcp_index = true`.

See the [MCP integration guide](doc/MCP_INTEGRATION.md) for tool parameters, HTTP transport, and serving multiple repositories, and the [LSP guide](doc/LSP_INTEGRATION.md) for language server setup.

## Use Octocode with OctoFS

[OctoFS](https://github.com/Muvon/octofs) adds file reading and atomic editing tools to your assistant. Run both MCP servers against the same project directory.

Octocode's `structural_search` returns source lines as `N:hh|content`, with the same line IDs used by OctoFS `view`. An agent can use these IDs directly in OctoFS `batch_edit`, without reading the same lines again to obtain edit targets. Exact-name lookups include complete definitions up to 32 lines; larger results identify omitted ranges for a targeted read.

Each ID hashes the original line, including indentation. OctoFS checks IDs before applying edits and reports stale targets when the content no longer matches. Group known edits to a file into one atomic batch, then use the fresh IDs returned by the edit for follow-up changes. The `N:hh|` prefix is metadata, not part of the source code.

After [installing OctoFS](https://github.com/Muvon/octofs#installation), add it alongside the Octocode server configured above. Choose your client:

**Claude Code:**

```bash
claude mcp add --transport stdio octofs -- octofs mcp --path /path/to/your/project
```

**Codex CLI:**

```bash
codex mcp add octofs -- octofs mcp --path /path/to/your/project
```

Use the same absolute project path for both servers. OctoFS is optional; Octocode's search and navigation tools work independently.

## How it works

```text
Current source → Language parsers → Live file and symbol graph → MCP tools
Indexed code   → Embeddings + BM25 → Hybrid search + reranking → CLI / MCP
```

The live graph uses Tree-sitter to extract symbols and relationships from current source, without embeddings or an LLM. It remains available through MCP when `[graphrag].enabled = false`.

Enabling indexed GraphRAG adds persisted file-level enrichment, including semantic file discovery and optional LLM descriptions and architectural relationships. The `octocode graphrag` CLI uses this persisted layer:

```bash
octocode config --graphrag-enabled true
octocode index
octocode graphrag overview
```

See the [architecture guide](doc/ARCHITECTURE.md) for module boundaries and storage details.

## Supported languages

| Language | Extensions |
|----------|------------|
| Rust | `.rs` |
| Python | `.py` |
| TypeScript / JavaScript | `.ts`, `.tsx`, `.js`, `.jsx` |
| Go | `.go` |
| PHP | `.php` |
| C / C++ | `.c`, `.cpp`, `.cc`, `.cxx`, `.c++`, `.h`, `.hpp`, `.hxx`, `.cppm`, `.ixx`, `.mxx`, `.ccm`, `.cxxm` |
| Ruby | `.rb` |
| Elixir | `.ex`, `.exs` |
| Java | `.java` |
| Swift | `.swift` |
| Svelte | `.svelte` |
| Lua | `.lua` |
| CSS | `.css`, `.scss`, `.sass` |
| JSON | `.json` |
| Bash | `.sh`, `.bash` |
| Markdown | `.md`, `.markdown` |

## Retrieval benchmark

The repository includes [ground truth, scoring scripts, and reproduction instructions](benchmark/README.md). The published local matrix used 127 code-search queries, corpus commit `b1771ba`, Octocode **0.17.1**, and `fastembed:jinaai/jina-embeddings-v2-base-code`, with reranking disabled for the variants below.

| Configuration | Hit@5 | Hit@10 | MRR | NDCG@10 | Recall@10 |
|---------------|-------|--------|-----|---------|-----------|
| Dense vector only | 0.598 | 0.717 | 0.485 | 0.528 | 0.671 |
| Hybrid, vector/keyword weights 0.7/0.3 | 0.598 | 0.717 | 0.485 | 0.528 | 0.671 |
| Hybrid, vector/keyword weights 0.3/0.7 | 0.732 | 0.835 | 0.572 | 0.620 | 0.807 |

These are historical results from June 21, 2026; they do not measure the current release or its default configuration. See the [full variant matrix](benchmark/RESULTS.md) for settings and reranker comparisons.

## Privacy and configuration

- **Local models by default.** Embedding and reranking run locally after model downloads. Remote embedding, reranking, and LLM providers are optional; when enabled, they receive the queries and source content needed for those features.
- **Local storage.** Indexes live outside your repository in Octocode's per-project data directory.
- **File exclusions.** Indexing respects `.gitignore` and `.noindex`. Configure exclusions for files you do not want indexed.
- **Client access.** The default MCP transport is stdio. The model used by your AI client may receive source content returned by tools, even when Octocode uses local models.
- **Provider credentials.** API keys can be supplied through environment variables. See the [API key guide](doc/API_KEYS.md) before enabling remote providers.

The [configuration guide](doc/CONFIGURATION.md) explains model selection and indexing options. The [embedded default template](config-templates/default.toml) contains the current defaults.

## Documentation

- [Getting started](doc/GETTING_STARTED.md) — basic workflow
- [Installation](INSTALL.md) — binaries, source builds, and platform requirements
- [MCP clients](doc/MCP_CLIENTS.md) — client setup and configuration locations
- [MCP integration](doc/MCP_INTEGRATION.md) — tools, transports, and multiple repositories
- [LSP integration](doc/LSP_INTEGRATION.md) — language server tools
- [Commands](doc/COMMANDS.md) — CLI reference
- [Configuration](doc/CONFIGURATION.md) — models and customization
- [API keys](doc/API_KEYS.md) — remote provider setup
- [Architecture](doc/ARCHITECTURE.md) — internals
- [Changelog](CHANGELOG.md) — release history

## Contributing

Bug reports, documentation improvements, and code contributions are welcome. Read the [contributing guide](doc/CONTRIBUTING.md) for development setup and the [engineering standards](AGENTS.md) before changing code.

Use [GitHub Issues](https://github.com/Muvon/octocode/issues) for bugs and feature requests, [Discussions](https://github.com/Muvon/octocode/discussions) for questions, or contact [opensource@muvon.io](mailto:opensource@muvon.io).

## License

[Apache License 2.0](LICENSE). Maintained by [Muvon](https://muvon.io).

<!-- mcp-name: io.github.Muvon/octocode -->
