# Changelog

## [0.8.0] - 2025-07-11

### 📋 Release Summary

This release introduces enhanced multi-language import resolution and expanded semantic graph operations for improved code indexing and search (f53d5bc9, 10841fbe, c1e4e5f7). New AI architectural analysis settings and additional embedding providers, including OpenAI and Google models, offer greater flexibility and accuracy (386526a7, 3aec68da, e920ae56). Several bug fixes and refinements improve cross-platform stability, indexing reliability, and relationship extraction, enhancing overall system robustness (e6340d09, 2efe3df3, aed122e3, 8b648f71).


### ✨ New Features & Enhancements

- **graphrag**: add import resolver for multi-language imports `f53d5bc9`
- **graphrag**: store relationships incrementally during processing `10841fbe`
- **graphrag**: expand GraphRAG with node and path operations `c1e4e5f7`
- **config**: add AI architectural analysis settings and prompts `386526a7`
- **embedding**: add OpenAI as new embeddings provider `3aec68da`
- **jina**: add new jina embedding models with dimensions `bd0d3146`
- **embedding**: rename get_model_dimension and add Google models map `e726cdda`
- **embedding**: add modular Google embedding provider and docs `e920ae56`
- **models**: add dynamic model discovery and CLI commands `5beba7c0`
- **markdown**: add signature extraction for markdown files `b27a50dc`

### 🔧 Improvements & Optimizations

- **rust**: unify file existence checks with helper method `0385c5e8`
- **import**: remove legacy GraphRAG import resolver module `e8e8740f`
- **models**: unify models list output format with dimensions `123be1db`
- **fastembed**: use fully qualified model names in mapping and l... `e6f1647a`
- **embedding**: unify huggingface prefix and improve model docs `4709ce2d`
- **huggingface**: rename sentence transformers and fix URL resol... `ac193c4b`
- **watch**: remove deprecated ignore patterns and config field `66df3c6c`

### 🐛 Bug Fixes & Stability

- **config**: remove deprecated top_k in favor of max_results `e6340d09`
- **indexer**: prevent infinite recursion by disabling symlink follow `2efe3df3`
- **test**: normalize paths to fix Windows test failures `83b415cd`
- **indexer**: normalize paths and fix Windows tests for parent-child ... `510047b2`
- **graphrag**: prevent duplicate nodes and clean up stale data `aed122e3`
- **config**: correct confidence_threshold value in tests `b014ab7a`
- **graphrag**: apply AI descriptions before node persistence `93f40ddf`
- **graphrag**: improve AI relationship fetching criteria and logging `8b648f71`
- **graphrag**: correctly update nodes with AI-generated descriptions `3ee81fd6`
- **graphrag**: improve import/export extraction and relationship dete... `12d199a0`
- **clear**: skip memory tables when clearing all data `b8075122`
- **test**: simplify embedding provider tests to assume Voyage only `a6383b36`
- **graphrag**: extract and display node relationships with correct in... `34bf6c3e`
- **graphrag**: correct node indexing and display relationships `e93ea0cb`
- **graphrag**: fix incremental indexing and cleanup for GraphRAG data `abc32e2b`
- **graphrag**: fix graph_nodes persistence by unifying storage method `a9ab31af`
- **store**: update file metadata using LanceDB UpdateBuilder API `e1d39b9a`
- **search**: use config default for similarity threshold if unset `103ef056`
- **website**: correct code block indentation in index.html `41676c9f`

### 📚 Documentation & Examples

- add import resolution and safe discovery details in docs `ded521dd`
- **cli**: update model commands and GraphRAG usage details `57abf90d`
- **readme**: remove accidental OpenAI promo from README `0aa869c6`
- **commands**: expand Voyage models list and add OpenAI provider `1bff845a`
- **mcp**: clarify view_signatures tool description and add Markdown ... `ea64de3e`

### 🔄 Other Changes

- **embedding**: add FastEmbed provider creation and validation tests `fb4b38ca`
- **graphrag**: add edge case tests for import resolution `d84f90a5`
- **graphrag**: upgrade config with LLM batching and fallback options `ea884005`
- **release**: clarify purpose of crates upload release `86a490ad`

### 📊 Release Summary

**Total commits**: 45 across 5 categories

✨ **10** new features - *Enhanced functionality*
🔧 **7** improvements - *Better performance & code quality*
🐛 **19** bug fixes - *Improved stability*
📚 **5** documentation updates - *Better developer experience*
🔄 **4** other changes - *Maintenance & tooling*

## [0.7.1] - 2025-06-28

### 📋 Release Summary

This release is made exclusively for uploading the proper version to crates that had broken Rust indexing with an incorrect tree-sitter parser version.


### 📚 Documentation & Examples

- **ci**: fix markdown code block formatting in release workflow `39c0361e`

### 📊 Release Summary

**Total commits**: 1 across 1 categories

📚 **1** documentation update - *Better developer experience*

## [0.7.0] - 2025-06-27

### 📋 Release Summary

This release introduces enhanced search capabilities with distance-based result sorting and improved input handling, alongside streamlined environment configuration and automated changelog generation (97af3e9c, ea78232f, f3c50bbc, 9fab3f2c). Several bug fixes improve search accuracy, error handling, and repository detection (fa171584, 057c7832, e950d9c4). Additional updates include dependency upgrades, codebase optimizations, documentation fixes, and new website deployment.


### ✨ New Features & Enhancements

- **store**: include and sort search results by distance score `97af3e9c`
- **embedding**: add input_type support with manual prefix injection `ea78232f`
- **config**: load environment variables from .env file on startup `f3c50bbc`
- **release**: enhance changelog generation and categorization `9fab3f2c`

### 🔧 Improvements & Optimizations

- **store**: unify vector search optimization with VectorOptimizer `3d9d4281`
- **store**: replace deprecated nearest_to with vector_search API `867a65a9`
- **commit**: add file-type checks to enforce docs type rules `ee523291`

### 🐛 Bug Fixes & Stability

- **mcp**: unify error handling in GraphRAG search execution `fa171584`
- **search**: correct threshold conversion from similarity to distance `057c7832`
- **cli**: use git root for repository detection in commands `e950d9c4`

### 📚 Documentation & Examples

- fix install script URLs to use master branch `8c304970`

### 🔄 Other Changes

- **deps**: upgrade notify, notify-debouncer-mini, dirs, and tree-si... `1d71d547`
- **deps**: upgrade arrow crates to 55.2.0 for dependency updates `1ad07e85`
- **ci**: update Rust version to 1.82 in Cargo.toml and clean workfl... `4bcc8d53`
- **deps**: update Cargo `08a6c6cb`
- Add website `7ace9672`
- **release**: add GitHub Action job to publish crate to crates.io `84af3b80`

### 📊 Release Summary

**Total commits**: 17 across 5 categories

✨ **4** new features - *Enhanced functionality*
🔧 **3** improvements - *Better performance & code quality*
🐛 **3** bug fixes - *Improved stability*
📚 **1** documentation update - *Better developer experience*
🔄 **6** other changes - *Maintenance & tooling*

## [0.6.0] - 2025-06-24

### 📋 Release Summary

This release introduces new filtering options for code searches and limits output size to improve usability. Documentation has been updated for clearer build instructions, and indexing processes have been enhanced for better performance. Several bug fixes address search stability, language detection, and memory management.


### ✨ Features

- **mcp**: add max_tokens parameter to limit tool output size (ba81bd24)
- **mcp**: add max_tokens limit to truncate large outputs in memory a... (0bce4ada)
- **search**: add --language filter for code block searches (daeeb62a)

### 🐛 Bug Fixes

- **graphrag**: use quiet mode in GraphBuilder during search (44d4fd74)
- **mcp**: remove redundant cwd changes and fix startup directory (5cb62cc5)
- **indexer**: correct language detection for file extensions (bd6f0aeb)
- **memory**: remove lock timeouts to prevent premature failures (d27a0987)

### 🔧 Other Changes

- **instructions**: clarify mandatory cargo build flags and usage (a14befc6)
- **readme**: streamline and condense README key features section (bbe70ef2)
- **indexer**: add batch processing and code region extraction mo... (43f50282)
- **indexer**: extract markdown processing into dedicated module (17cc2ab5)
- **indexer**: move signature extraction to dedicated module (a29fb64b)

### 📊 Commit Summary

**Total commits**: 12
- ✨ 3 new features
- 🐛 4 bug fixes
- 🔧 5 other changes

## [0.5.2] - 2025-06-22

### 📋 Release Summary

This release improves search accuracy with enhanced query validation and adjusts memory limits for better resource management. Performance optimizations streamline data processing, complemented by updated documentation to help users fine-tune vector indexing. Several bug fixes enhance overall system reliability and user experience.


### 🐛 Bug Fixes

- **search**: enforce stricter query validation and correct detail levels (9442589a)
- **memory**: reduce max and default memories returned to 5 (50a84fe7)

### 🔧 Other Changes

- **store**: optimize sub-vector factor selection and milestone checks (98bdfdd1)
- **store**: add LanceDB vector index tuning and performance guide (b2c2fa8d)
- **constants**: extract MAX_QUERIES to shared constant (c2722c7c)

### 📊 Commit Summary

**Total commits**: 5
- 🐛 2 bug fixes
- 🔧 3 other changes

## [0.5.1] - 2025-06-21

### 📋 Release Summary

This release includes several bug fixes that enhance command pattern recognition and improve code efficiency. These updates contribute to a smoother and more reliable user experience.


### 🐛 Bug Fixes

- **view**: resolve files with ./ prefix in view command patterns (4ecc5900)
- **clippy**: reduntant conversion (c53c046b)

### 📊 Commit Summary

**Total commits**: 2
- 🐛 2 bug fixes

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
