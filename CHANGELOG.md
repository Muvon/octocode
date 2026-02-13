# Changelog

## [0.11.0] - 2026-02-13

### 📋 Release Summary

This release adds new AI providers for enhanced code analysis capabilities and delivers significant performance improvements through optimized graph traversal and query processing. Multiple bug fixes enhance system stability, improve text chunking accuracy, and ensure reliable metadata handling across the codebase.


### ✨ New Features & Enhancements

- **octolib**: add zai and minmax providers `fbe220a9`

### 🔧 Improvements & Optimizations

- **graphrag**: simplify nested if-else conditionals `d339b663`
- **commands/commit**: enforce plain text format in AI prompts `af0e3ffd`
- **review**: process JSON values directly `62992723`
- **llm**: unify config and integrate octolib LlmClient `06aef91d`
- **indexer**: simplify code embedding and unify logic `ba35f2d7`
- **store**: simplify CacheStats struct initialization in test `f35032cd`
- **graphrag**: add adjacency cache for graph traversal optimization `06480d97`
- **graphrag**: improve queries with typed relations and pagination `52f2dab7`
- **embedding**: replace providers with octolib integration `3db03b98`

### 🐛 Bug Fixes & Stability

- **config**: handle missing embedding provider in test `77704c9a`
- **embedding**: handle parse_provider_model errors properly `8ef62370`
- **indexer**: guarantee forward progress in text chunking loop `72c70303`
- **indexer**: correct end_idx calculation in text chunking `9f75b3be`
- **mcp**: handle JSON-RPC notifications without response `8251b243`
- **zai**: resolve provider issue `a09718f6`
- **utils**: handle single-line commit messages safely `979188b0`
- **indexer**: flush store before saving metadata `2a135443`
- **graphrag**: align batch schema field names for source_id handling `e653ee5b`
- **docker**: update Rust base image to 1.91-slim `6d550f0c`
- **octolib**: disable default features and enable embed by default `5d176da7`
- **ci**: update cc crate and disable doctests on Windows `d46af3b8`

### 🔄 Other Changes

- upgrade Rust toolchain to 1.92.0 `0974150e`
- **deps**: bump octolib to 0.8.1 `6b0d7648`
- **deps**: upgrade octolib to 0.7.0 `63ae7cc0`
- **deps**: update lance with geospatial support `c583b1a5`
- **memory**: extract memory to separate binary `113f9753`
- **deps**: bump octolib to v0.5.1 `a8fd0156`
- **deps**: update octolib and zune-core `eee88b0c`
- **release**: update macOS runner to 14 `5c6d7a22`
- **deps**: upgrade reqwest and tree-sitter crates `6e58781c`
- **deps**: bump dependency versions in Cargo.lock `afeaba6b`
- **cargo**: bump deps and adapt to upstream API `9295417a`
- **commands**: clarify breaking change policy in prompt `4626e1d8`
- **deps**: update syn, http, and other dependencies `4b062e75`
- **deps**: lock octolib to crates.io release version `78a9e7e6`
- **deps**: update tracing-subscriber and core dependencies `eb6db5f5`

### 📊 Release Summary

**Total commits**: 37 across 4 categories

✨ **1** new feature - *Enhanced functionality*
🔧 **9** improvements - *Better performance & code quality*
🐛 **12** bug fixes - *Improved stability*
🔄 **15** other changes - *Maintenance & tooling*

## [0.10.0] - 2025-09-14

### 📋 Release Summary

This release adds Java language support and enhances tokenization for improved semantic search (eae4107f, 4f463f07). Several bug fixes improve system stability, including better metadata handling, dependency updates, and LSP initialization (1574ac91, 48978d2a, 76e1410e, 11fb216b). Documentation and configuration guidance have also been updated for clearer usage and setup.


### ✨ New Features & Enhancements

- **indexer**: add Java language support `eae4107f`
- **huggingface**: add BPE tokenizer support for RoBERTa-style models `4f463f07`

### 🔧 Improvements & Optimizations

- **java**: merge single-line nodes and deduplicate symbols `94d9e139`
- **embedding, indexer**: fix code formatting and whitespace issues `a7823895`

### 🐛 Bug Fixes & Stability

- **indexer**: clarify git metadata storage condition message `1574ac91`
- **lancedb**: update dependency to resolve aws_credential error `48978d2a`
- **lsp**: resolve initialization and update issues with --with-lsp `76e1410e`
- **embedding**: update fastembed to 5.0.2 and ensure thread safety `11fb216b`

### 📚 Documentation & Examples

- **config**: update embedding models and vector index guidance `7a6264d6`
- **mcp**: clarify semantic search tool description for queries `2078ef8a`

### 🔄 Other Changes

- **deps**: upgrade dependencies to latest versions `deb779a7`

### 📊 Release Summary

**Total commits**: 11 across 5 categories

✨ **2** new features - *Enhanced functionality*
🔧 **2** improvements - *Better performance & code quality*
🐛 **4** bug fixes - *Improved stability*
📚 **2** documentation updates - *Better developer experience*
🔄 **1** other change - *Maintenance & tooling*

## [0.9.1] - 2025-08-14

### 📋 Release Summary

This release includes several bug fixes that enhance code indexing accuracy and stability, such as improved parsing of export statements, serialized indexing to prevent conflicts, refined method-level indexing for PHP, and better handling of code fences in markdown files (bd5bc26e, ae71a02e, 19acf3d4, 9df2c44d). Additionally, the initial release process now correctly uses the current version without requiring a manual bump (693e2fd7).


### 🐛 Bug Fixes & Stability

- **svelte**: improve parsing of export statements in indexer `bd5bc26e`
- **lock**: serialize indexing with file-based lock `ae71a02e`
- **php**: index classes by methods instead of whole class `19acf3d4`
- **markdown**: prevent unrelated chunks from code fence splits `9df2c44d`
- **release**: use current version for initial release without bump `693e2fd7`

### 📊 Release Summary

**Total commits**: 5 across 1 categories

🐛 **5** bug fixes - *Improved stability*

## [0.9.0] - 2025-08-10

### 📋 Release Summary

This release adds Lua language support and enhances code indexing with enriched context for improved search accuracy (60eeadca, 8e741cd4). Performance and responsiveness are improved through faster file counting and optimized memory handling (c891595a, f9ca0766). Several bug fixes address search threshold accuracy, code parsing, rendering issues, and stability across multiple components (9461b83a, 0d04a53c, 72f31414, b8e4eb43, fdd9e5d8, bae5806d, 0b927b2a).


### ✨ New Features & Enhancements

- **indexer**: enrich code block embeddings with file context `8e741cd4`
- **parser**: add Lua language support with tree-sitter `60eeadca`

### 🔧 Improvements & Optimizations

- **indexer**: add fast file counting to optimize indexing performance `c891595a`
- **store**: unify line indexing to zero-based internally `1c282523`
- **cpp**: extract variable names from declarations alongside functions `bb941c5e`

### 🐛 Bug Fixes & Stability

- **graphrag**: load nodes from DB before relationship discovery `9461b83a`
- **search**: correct threshold handling for -t parameter `0d04a53c`
- **huggingface**: await async model init to prevent panic `72f31414`
- **cpp**: correct import and export extraction logic `b8e4eb43`
- **render**: correct line number display offset in outputs `fdd9e5d8`
- **cpp**: handle function declarations in indexing and signatures `bae5806d`
- **memory**: improve MCP responsiveness and indexing efficiency `f9ca0766`
- **diff_chunker**: prevent panic on UTF-8 boundary slicing `0b927b2a`

### 📊 Release Summary

**Total commits**: 13 across 3 categories

✨ **2** new features - *Enhanced functionality*
🔧 **3** improvements - *Better performance & code quality*
🐛 **8** bug fixes - *Improved stability*

## [0.8.2] - 2025-08-03

### 📋 Release Summary

This release enhances indexing performance with improved caching and incremental file counting, alongside refined diff processing and AI-assisted commit message generation (07758f96, 32ab2c2c, 6ca8e2ee, 4f5a5241, fd32737d). Several bug fixes improve review accuracy, knowledge graph tracking, and overall system stability (270f80c5, 542836f9).


### 🔧 Improvements & Optimizations

- **commit**: add AI refinement for chunked commit messages `4f5a5241`
- **diff_chunker**: improve diff chunking and response combination `fd32737d`
- **indexer**: optimize .noindex detection with caching and targeted ... `07758f96`

### 🐛 Bug Fixes & Stability

- **review**: add file path and line number to review schema `270f80c5`
- **indexer**: correct file counting for incremental indexing with git changes `32ab2c2c`
- **diff_chunker**: add utility to split large diffs for chunked processing `6ca8e2ee`
- **graphrag**: correctly increment graphrag_blocks counter `542836f9`

### 📊 Release Summary

**Total commits**: 7 across 2 categories

🔧 **3** improvements - *Better performance & code quality*
🐛 **4** bug fixes - *Improved stability*

## [0.8.1] - 2025-07-25

### 📋 Release Summary

This release improves indexing accuracy and metadata handling while enhancing knowledge graph consistency for more reliable code search results (254b3ac2, 42fea8e4, 5219091d). Additionally, it fixes staging issues to ensure smoother commit workflows (86ce9f85).


### 🐛 Bug Fixes & Stability

- **indexer**: avoid .noindex errors by checking file existence `254b3ac2`
- **indexer**: correct initial indexing and git metadata storage `42fea8e4`
- **commit**: re-stage only originally staged files modified by pre-co... `86ce9f85`
- **graphrag**: remove relationships by node IDs instead of file paths `5219091d`

### 📊 Release Summary

**Total commits**: 4 across 1 categories

🐛 **4** bug fixes - *Improved stability*

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
