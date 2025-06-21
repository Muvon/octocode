# Changelog

## [0.5.0] - 2025-06-21

### 📋 Release Summary

This release introduces enhanced search and memory features, including detailed output options and multi-query support, along with new CLI commands and expanded protocol integration. Additional language support and improved documentation provide a better user experience. Several bug fixes and refinements improve rendering accuracy and overall system stability.


### ✨ Features

- **search**: add detail level option for search output (8ade06ba)
- **memory**: add multi-query support for memory retrieval (437e7d4f)
- **docs**: add new CLI commands and usage examples to README (0fdfa552)
- **mcp_proxy**: add HTTP proxy command for multiple MCP servers (26301f7b)
- **mcp**: add HTTP server mode for MCP protocol integration (8ff10302)
- **indexer**: add CSS/SCSS language support with tree-sitter parsers (fe88742a)

### 🐛 Bug Fixes

- **render_utils**: show first 2 and last 2 lines in signature renderings (6a46610f)
- **render_utils**: correct new line rendering in markdown output (a6453c6d)
- **indexer**: truncate signature text output to 5 lines with ellipsis (0f2fe910)

### 🔧 Other Changes

- **proxy**: restrict console logging to debug mode only (4199a6c0)
- **search**: render docs with detail level matching code output (33db16a0)
- **indexer**: extract file and git utilities into modules (03b8f495)
- **svelte**: simplify symbol extraction to script/style only (367f99dd)

### 📊 Commit Summary

**Total commits**: 13
- ✨ 6 new features
- 🐛 3 bug fixes
- 🔧 4 other changes

## [0.4.1] - 2025-06-17

### 📋 Release Summary

This release includes several bug fixes that improve content accuracy and output formatting. Enhancements to search functionality and indexing provide more precise results, while performance optimizations reduce build times.


### 🐛 Bug Fixes

- **embedding**: include line ranges in content hash calculation (cf7c2d1b)
- **indexer**: correct chunk merging to use sorted line numbers (2ec4d221)
- **view**: correct output format handling for view command (6fe41063)

### 🔧 Other Changes

- **view, indexer**: add line numbers to text signature and searc... (981aeb8d)
- **docker**: build release without default Cargo features (8d442bc0)

### 📊 Commit Summary

**Total commits**: 5
- 🐛 3 bug fixes
- 🔧 2 other changes

## [0.4.0] - 2025-06-16

### 📋 Release Summary

This release introduces LSP integration with external server support and enhanced pre-commit hook automation for streamlined workflows. Documentation has been expanded with detailed usage examples and development instructions, while several refinements improve versioning prompts and semantic search clarity. Minor bug fixes address changelog formatting for better readability.


### ✨ Features

- **docs**: add LSP integration docs and CLI usage examples (7dfd5c20)
- **mcp**: add LSP support with external server integration (29bbf98a)
- **commit**: add automatic pre-commit hook integration with AI commi... (07a48fde)
- **commit**: run pre-commit hooks before generating commit message (92aaf04a)
- **release**: update versioning prompt and add lock file update (786e1fe3)

### 🐛 Bug Fixes

- **docs**: remove brackets from commit hashes in changelog (92bad9dd)

### 🔧 Other Changes

- **docker**: remove Cargo.lock from .dockerignore (d72ae449)
- **cargo**: narrow Tokio and dependencies features for leaner build (3e6b6789)
- add comprehensive Octocode development instructions (75c3add1)
- **cli**: set version from Cargo.toml environment variable (6ad09c16)
- **mcp/lsp**: simplify LSP tool inputs by replacing character wi... (616032e8)
- **lsp**: simplify LSP responses to plain text format (5f8487a8)
- **mcp**: clarify semantic search guidance in tool description (83551bba)
- **mcp**: rename search_graphrag to graphrag_search for consistency (cf1d8428)
- **mcp**: rename search_code tool to semantic_search to avoid AI... (93ca7008)
- **commit**: clarify commit message rules and types (380cadcc)

### 📊 Commit Summary

**Total commits**: 16
- ✨ 5 new features
- 🐛 1 bug fix
- 🔧 10 other changes

## [0.3.0] - 2025-06-14

### 📋 Release Summary

This release enhances search functionality by increasing the maximum allowed queries and adding a text output format for results. Improvements to memory handling and command output formatting boost reliability and consistency. Additional fixes address changelog formatting, test stability, and performance optimizations across components.


### ✨ Features

- **indexer**: increase max allowed queries from 3 to 5 (9098d58e)
- **commit,release**: improve handling of breaking changes in commands (67f06276)
- **search**: add text output format for search results (b2cbbbfe)

### 🐛 Bug Fixes

- **release**: preserve trailing newline in changelog on update (cebc98e0)
- **memory**: add UTF-8 sanitization and lock timeout handling (85cb6356)
- **tests**: fix test failures and apply code formatting (7e645ae2)
- **memory,commit,review**: use char count for truncation limits (4ed5e732)
- **mcp**: use actually used original_dir variable for cwd restore (60ec9b77)

### 🔧 Other Changes

- **mcp**: reduce token usage in tool definitions and schemas (04db399f)
- **semantic_code**: clarify multi-term search usage in tool descript... (0f931263)
- **graphrag**: unify and improve text output formatting (27476075)
- **memory**: unify memory formatting and remove sanitization (00e72942)
- **commands**: unify output format handling with OutputFormat enum (9f95e7bc)
- add Cargo.lock and track it in repo (b34051b2)
- **changelog**: add initial release notes for v0.1.0 (91ae04ff)

### 📝 All Commits

- cebc98e0 fix(release): preserve trailing newline in changelog on update *by Don Hardman*
- 9098d58e feat(indexer): increase max allowed queries from 3 to 5 *by Don Hardman*
- 04db399f perf(mcp): reduce token usage in tool definitions and schemas *by Don Hardman*
- 0f931263 docs(semantic_code): clarify multi-term search usage in tool descript... *by Don Hardman*
- 27476075 refactor(graphrag): unify and improve text output formatting *by Don Hardman*
- 85cb6356 fix(memory): add UTF-8 sanitization and lock timeout handling *by Don Hardman*
- 67f06276 feat(commit,release): improve handling of breaking changes in commands *by Don Hardman*
- 7e645ae2 fix(tests): fix test failures and apply code formatting *by Don Hardman*
- 00e72942 refactor(memory): unify memory formatting and remove sanitization *by Don Hardman*
- 4ed5e732 fix(memory,commit,review): use char count for truncation limits *by Don Hardman*
- 9f95e7bc refactor(commands): unify output format handling with OutputFormat enum *by Don Hardman*
- b2cbbbfe feat(search): add text output format for search results *by Don Hardman*
- b34051b2 chore: add Cargo.lock and track it in repo *by Don Hardman*
- 60ec9b77 fix(mcp): use actually used original_dir variable for cwd restore *by Don Hardman*
- 91ae04ff docs(changelog): add initial release notes for v0.1.0 *by Don Hardman*

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
