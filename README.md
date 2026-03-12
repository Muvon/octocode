# Octocode - Intelligent Code Indexer and Graph Builder

**© 2025 Muvon Un Limited (Hong Kong)** | [Website](https://muvon.io) | [Product Page](https://octocode.muvon.io)

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org)

<a href="https://glama.ai/mcp/servers/Muvon/octocode">
  <img width="380" height="200" src="https://glama.ai/mcp/servers/Muvon/octocode/badge" alt="Octocode MCP server" />
</a>

## 🚀 Overview

Octocode is a powerful code indexer and semantic search engine that builds intelligent knowledge graphs of your codebase. It combines advanced AI capabilities with local-first design to provide deep code understanding, relationship mapping, and intelligent assistance for developers.

## ✨ Key Features

- **🔍 Semantic Code Search** - Natural language queries with multi-query support
- **🕸️ Knowledge Graph (GraphRAG)** - Automatic relationship discovery between files
- **🌐 Multi-Language Support** - Rust, Python, JavaScript, TypeScript, Go, PHP, C++, Ruby, and more
- **🧠 AI-Powered Features** - Smart commits, code review, memory system with semantic search
- **🔌 MCP Server Integration** - Built-in Model Context Protocol server with LSP support
- **⚡ Performance & Flexibility** - Intelligent LanceDB optimization, local/cloud embedding models

## 📦 Quick Installation

```bash
# Universal install script (Linux, macOS, Windows)
curl -fsSL https://raw.githubusercontent.com/Muvon/octocode/master/install.sh | sh
```

**Alternative methods:**
- Download from [GitHub Releases](https://github.com/Muvon/octocode/releases)
- Install with Cargo: `cargo install --git https://github.com/Muvon/octocode`

For detailed installation instructions, see [Installation Guide](INSTALL.md).

## 🔑 API Keys Setup

**⚠️ Required for functionality:**

```bash
# Required: Voyage AI (embeddings) - 200M free tokens/month
export VOYAGE_API_KEY="your-voyage-api-key"

# Optional: OpenRouter (LLM features)
export OPENROUTER_API_KEY="your-openrouter-api-key"
```

**Get your free API keys:**
- **Voyage AI**: [Get free API key](https://www.voyageai.com/) (200M tokens/month free)
- **OpenRouter**: [Get API key](https://openrouter.ai/) (optional, for AI features)

## 🚀 Quick Start

```bash
# 1. Index your codebase
octocode index

# 2. Search with natural language
octocode search "HTTP request handling"

# 3. Multi-query search for comprehensive results
octocode search "authentication" "middleware"

# 4. AI-powered git workflow
octocode commit --all

# 5. Start MCP server for AI assistants
octocode mcp --path /path/to/your/project
```

## 📚 Complete Documentation

📖 **Quick Navigation**

- **[Installation Guide](INSTALL.md)** - Detailed installation methods and building from source
- **[Getting Started](doc/GETTING_STARTED.md)** - First steps and basic workflow
- **[API Keys Setup](doc/API_KEYS.md)** - Complete API configuration guide
- **[Configuration Guide](doc/CONFIGURATION.md)** - Configuration system, templates, and customization
- **[Commands Reference](doc/COMMANDS.md)** - Complete command reference with examples
- **[Advanced Usage](doc/ADVANCED_USAGE.md)** - Advanced features and workflows
- **[MCP Integration](doc/MCP_INTEGRATION.md)** - Model Context Protocol server setup
- **[LSP Integration](doc/LSP_INTEGRATION.md)** - Language Server Protocol integration
- **[Memory System](doc/MEMORY_SYSTEM.md)** - Memory management and semantic search
- **[Release Management](doc/RELEASE_MANAGEMENT.md)** - AI-powered release automation
- **[Architecture](doc/ARCHITECTURE.md)** - Core components and system design
- **[Performance](doc/PERFORMANCE.md)** - Performance metrics and optimization
- **[Contributing](doc/CONTRIBUTING.md)** - Development setup and contribution guidelines

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

## 🔒 Privacy & Security

- **🏠 Local-first option**: FastEmbed and SentenceTransformer run entirely offline (macOS only)
- **🔑 Secure storage**: API keys stored locally, environment variables supported
- **📁 Respects .gitignore**: Never indexes sensitive files or directories
- **🛡️ MCP security**: Server runs locally with no external network access for search
- **🌐 Cloud embeddings**: Voyage AI and other providers process only file metadata, not source code

## 🤝 Support & Community

- **🐛 Issues**: [GitHub Issues](https://github.com/Muvon/octocode/issues)
- **📧 Email**: [opensource@muvon.io](mailto:opensource@muvon.io)
- **🏢 Company**: Muvon Un Limited (Hong Kong)

## ⚖️ License

This project is licensed under the **Apache License 2.0** - see the [LICENSE](LICENSE) file for details.

---

**Built with ❤️ by the Muvon team in Hong Kong**