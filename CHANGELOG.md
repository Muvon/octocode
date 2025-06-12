# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2025-06-12

### ✨ Features

- add mode option to selectively clear tables
- add multi-query search usage and support details
- add hierarchical bottom-up chunking for docs
- add show-file option to display file chunks
- add --no-verify flag to skip git hooks
- add GraphRAG data cleanup on file removal
- improve UTF-8 slicing and path handling; build from D...
- build GraphRAG from existing DB if enabled
- add detailed multi-mode search with markdown output

### 🐛 Bug Fixes

- preserve formatting when updating version fields
- merge tiny chunks to reduce excessive chunk creation
- add optional context field to data schema
- update default model names and versions
- suppress MCP server logs during graph loading
- properly handle .noindex ignore files
- remove unnecessary timeouts on memory ops
- update Rust version and copy config templates
- require curl and update repo URLs to Muvon/octocode
- fix variable interpolation in release workflow URLs

### 🔧 Other Changes

- docs: replace "reindex" with "index" for accuracy in docs
- refactor: centralize search embeddings generation logic
- docs: add AI-powered release management docs and CLI usage
- refactor: unify GraphRAG config under graphrag section
- refactor: use shared HTTP client with pooling
- chore: update Apache License text to latest version
- chore: add Rust formatting and linting hooks
- refactor: move git file detection to utils module and clean code

## [0.1.0] - 2025-06-06

**Intelligent Code Indexer and Semantic Search Engine**

### ✨ Core Features
- **🔍 Semantic Code Search** - Natural language queries across your entire codebase
- **🕸️ Knowledge Graph (GraphRAG)** - Automatic relationship discovery between files and modules
- **🧠 AI Memory System** - Store and search project insights, decisions, and context
- **🔌 MCP Server** - Built-in Model Context Protocol for AI assistant integration

### 🌐 Language Support
**11 Languages**: Rust, Python, JavaScript, TypeScript, Go, PHP, C++, Ruby, JSON, Bash, Markdown

### 🛠️ AI-Powered Tools
- Smart commit message generation
- Code review with best practices analysis
- Auto-reindexing with file watching
- Multi-LLM support via OpenRouter

### ⚡ Performance & Privacy
- **Local-first option** (FastEmbed/SentenceTransformer on macOS)
- **Cloud embeddings** (Voyage AI - 200M free tokens/month)
- Respects `.gitignore` - never indexes sensitive files
- Optimized batch processing with Lance columnar database
