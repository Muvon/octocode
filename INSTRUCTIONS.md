# Octocode — AI-Powered Code Intelligence

Rust CLI tool (v0.14.x) that indexes codebases into LanceDB vector stores, builds knowledge graphs via GraphRAG, and exposes everything through an MCP server for AI assistant integration. Core stack: Rust + tokio + LanceDB + tree-sitter + octolib (embedding/LLM providers). Embedding providers and LLM are delegated entirely to `octolib`.

## Project Structure

```
src/
  main.rs                    — CLI entry point, command dispatch
  config.rs                  — All config structs; NO runtime defaults (panic on missing)
  storage.rs                 — Project DB path resolution (~/.local/share/octocode/<hash>/)
  store/                     — LanceDB operations
    mod.rs                   — Store struct, block types (CodeBlock, TextBlock, DocumentBlock, CommitBlock)
    vector_optimizer.rs      — Auto-tuned IVF_RQ / IVF_HNSW_SQ index selection
    batch_converter.rs       — Arrow RecordBatch construction
    table_ops.rs             — content_exists(), low-level table ops
    graphrag.rs              — GraphRAG node storage
    metadata.rs              — Table metadata helpers
  embedding/mod.rs           — Thin wrapper over octolib; generate_embeddings*, hash utilities
  llm/mod.rs                 — Thin wrapper over octolib LLM; LlmClient::from_config()
  indexer/
    mod.rs                   — NoindexWalker, file pipeline orchestration
    languages/               — One file per language, all implement Language trait
    graphrag/                — GraphRAG builder, AI relationship discovery
    batch_processor.rs       — Batch embedding + flush logic
    differential_processor.rs — Incremental file change processing
    contextual.rs            — LLM-based contextual chunk enrichment
    commits/                 — Git commit history indexing
  mcp/
    server.rs                — McpServer (rmcp-based, stdin + HTTP modes)
    semantic_code.rs         — SemanticCodeProvider tool impl
    graphrag.rs              — GraphRagProvider tool impl
    lsp/                     — LSP integration tools
    proxy.rs                 — Multi-repo MCP proxy
  commands/                  — One file per CLI subcommand
config-templates/default.toml — Single source of truth for ALL config defaults
```

## Where to Look

| Task | Start here |
|------|------------|
| Add/change config option | `config-templates/default.toml` first, then matching struct in `src/config.rs` |
| Add embedding provider/model | `octolib` repo — providers live there, not here |
| Add language support | `src/indexer/languages/{lang}.rs` + register in `languages/mod.rs` |
| Add MCP tool | `src/mcp/semantic_code.rs` or `src/mcp/graphrag.rs` (new provider file if distinct domain) |
| Add CLI command | `src/commands/{cmd}.rs` + register in `commands/mod.rs` + dispatch in `main.rs` |
| Store/query LanceDB | `src/store/mod.rs` (block types, Store methods) + `src/store/table_ops.rs` |
| Vector index tuning | `src/store/vector_optimizer.rs` — automatic, rarely needs changes |
| File discovery / ignore | `src/indexer/mod.rs` → `NoindexWalker` |
| Project DB path logic | `src/storage.rs` |
| Batch embedding pipeline | `src/indexer/batch_processor.rs` |
| GraphRAG relationships | `src/indexer/graphrag/` |
| Git commit indexing | `src/indexer/commits/` |

## How Things Work

### Configuration — No Defaults Rule
Config structs use `panic!()` in `Default::default()` for all non-trivial sections (GraphRAG, Reranker, HybridSearch). This is intentional — every value must come from `config-templates/default.toml`. When adding any config field:
1. Add to struct in `src/config.rs`
2. Add default value in `config-templates/default.toml`
3. Never add a real fallback value in `Default::default()`

### Embedding — Delegated to octolib
All provider implementations live in `octolib`. `src/embedding/mod.rs` is a thin wrapper that:
- Re-exports `octolib::embedding` types
- Adds `generate_embeddings()` / `generate_embeddings_batch()` with 3-attempt exponential backoff
- Adds `generate_search_embeddings()` for mode-aware query embedding (`code` / `docs` / `text` / `commits` / `all`)
- Provides `calculate_content_hash_with_lines()` for block dedup

Model format everywhere: `"provider:model"` (e.g., `"voyage:voyage-code-3"`, `"openrouter:openai/gpt-4o-mini"`).

### LLM — Delegated to octolib
`src/llm/mod.rs` wraps `octolib::llm`. Use `LlmClient::from_config(config)` for the default model, `LlmClient::with_model(config, model_str)` to override. Never call octolib LLM directly from commands — always go through `LlmClient`.

### Store — Block Types and Tables
Four LanceDB tables: `code_blocks`, `text_blocks`, `document_blocks`, `commit_blocks`. Each maps to a typed struct in `src/store/mod.rs`. Schema is created on first use; dimension mismatch triggers automatic table drop + recreate. Table handles are cached in `Arc<RwLock<HashMap>>` — never open tables manually.

```rust
// ✅ Use typed store methods
store.store_code_blocks(&blocks, &embeddings).await?;
store.content_exists(hash, "code_blocks").await?;

// ❌ Never open tables or create indexes manually
// db.open_table("code_blocks")  — use store methods
// table.create_index(...)       — optimizer handles this
```

### Vector Index — Automatic Optimization
`src/store/vector_optimizer.rs` selects index type and parameters automatically:
- `< 1K rows` → brute force (no index)
- `≥ 1K rows` → `IVF_RQ` (32x compression, default) or `IVF_HNSW_SQ` (4x) based on `config.index.quantization`
- Parameters (partitions, nprobes, refine_factor) calculated from row count — never hardcode them
- Always Cosine distance

### Indexer Pipeline
```
NoindexWalker (respects .gitignore + .noindex)
  → git optimization (skip unchanged files by commit hash)
  → language detection → tree-sitter parse
  → extract_meaningful_regions() → smart merge single-line declarations
  → optional: contextual LLM description prepended before embedding
  → batch embedding (16 files/batch, 100K token limit)
  → store_*_blocks() → flush every 2 batches
```

### Language Support Pattern
```rust
// src/indexer/languages/{lang}.rs
pub struct MyLang;
impl Language for MyLang {
    fn name(&self) -> &'static str { "mylang" }
    fn get_ts_language(&self) -> tree_sitter::Language { ... }
    fn meaningful_kinds(&self) -> &[&str] { &["function_definition", ...] }
    // implement remaining trait methods
}
// Then in languages/mod.rs: add mod, pub use, and register in get_language()
```

### MCP Server
Two transport modes via `rmcp`:
- **Stdin** (default): `octocode mcp --path=/repo` — standard MCP protocol for AI assistants
- **HTTP**: `octocode mcp --bind=0.0.0.0:12345 --path=/repo` — streamable HTTP with CORS

Tool providers implement `Clone` and are registered in `src/mcp/server.rs`. Use `Arc<Mutex<>>` for shared async state.

### Feature Gating
`fastembed` and `huggingface` are optional features (default-enabled). Gate provider code:
```rust
#[cfg(feature = "fastembed")]
pub mod fastembed;

#[cfg(feature = "fastembed")]
{ Ok(Box::new(FastEmbedProvider::new(model)?)) }
#[cfg(not(feature = "fastembed"))]
{ Err(anyhow::anyhow!("fastembed feature not compiled")) }
```

Build without local model features: `cargo build --no-default-features`

### Copyright Header — Mandatory
Every `.rs` file must start with:
```rust
// Copyright 2026 Muvon Un Limited
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
```
Update year to current year when modifying a file. New files must include the full header.

## Code Quality Standards

- **Zero clippy warnings** — `cargo clippy --all-features --all-targets -- -D warnings` must pass clean
- **Fail-fast validation** — validate model names / config during construction (`new()` returns `Result`)
- **Async-first** — tokio throughout; prefer tokio primitives over new dependencies
- **Minimal deps** — reuse existing crates before adding new ones
- **Single responsibility** — each provider/language/command in its own file
- **`#[derive(Clone)]`** on structs shared across async contexts
- **`Result<T>`** with meaningful error messages everywhere; no `.unwrap()` in non-test code

## Gotchas

- **`--no-default-features`** skips `fastembed`/`huggingface` — use this during dev to avoid heavy local model compilation; CI tests with `--all-features`
- **Config `Default::default()` panics** on GraphRAG/Reranker/HybridSearch — this is intentional; always load from template
- **DB path is outside the repo** — stored at `~/.local/share/octocode/<project-hash>/storage/`; project hash derived from git remote URL (normalized) or path hash
- **Schema mismatch auto-drops tables** — changing embedding model dimensions wipes existing index data; coordinate model changes carefully
- **`NoindexWalker` respects both `.gitignore` AND `.noindex`** — add `.noindex` for octocode-specific exclusions without polluting `.gitignore`
- **Embedding providers live in `octolib`** — don't add provider implementations here; update `octolib` instead
- **`require_git = true` by default** — indexing fails without a git repo; override in config for non-git projects

## Never

- Add config fields without a corresponding entry in `config-templates/default.toml`
- Hardcode vector dimensions, index partition counts, or search parameters — the optimizer calculates them
- Open LanceDB tables directly — always use `Store` methods
- Call `octolib` embedding/LLM APIs directly from commands — go through `src/embedding/mod.rs` and `src/llm/mod.rs`
- Add a new embedding provider implementation in this repo — it belongs in `octolib`
- Use `--release` builds during development
- Commit `.rs` files without the Apache 2.0 copyright header
