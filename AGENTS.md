# Octocode — AGENTS.md

Rust CLI (v0.26.x) that indexes codebases into LanceDB vector stores, builds GraphRAG knowledge graphs, and serves AI assistants over MCP (stdin or HTTP). Stack: Rust + tokio + LanceDB + tree-sitter + `octolib`. All embedding/LLM/reranker providers live in `octolib` — never in this repo.

## Commands

- Setup: `make setup` (clippy, rustfmt, cross-targets) · `pre-commit install` (fmt/clippy/check hooks) · `protoc` required for full-feature builds
- Fast dev build — skips local ONNX model compilation: `cargo build --no-default-features` · Full: `cargo build`
- Run: `cargo run -- index` · `RUST_LOG=debug cargo run -- search "query"` · `cargo run -- mcp --path .` (add `--bind 0.0.0.0:12345`, `--with-lsp rust-analyzer`, or `--multi`/`--auto` for multi-repo)
- Fast checks: `cargo check --no-default-features --message-format=short` · `cargo clippy --no-default-features`
- Single test: `cargo test --all-features indexer::graphrag::builder_tests::<name>`
- Full local gate: `make ci-quick` (fmt `--check` + clippy `-D warnings` + `cargo test`)

## Where to look

| Task | Start here |
|------|------------|
| Add CLI command | `src/commands/{cmd}.rs` + `mod`/`pub use` in `src/commands/mod.rs` + variant in `main.rs`; dispatch lives in TWO places — early return before `Store::new()` if the command needs no store, else the final `match` (storeless arms there are `unreachable!()`) |
| Add/change config | `config-templates/default.toml` (single source of truth; embedded) + struct in `src/config.rs` + `src/config/migrations.rs` for versioned migrations |
| Add language | `src/indexer/languages/{lang}.rs` + sibling `{lang}_test.rs` + register in `get_language()` in `languages/mod.rs` + tree-sitter crate in `Cargo.toml` |
| Add MCP tool | `src/mcp/server.rs` (router) — existing providers: `semantic_code.rs`, `structural.rs`, `graphrag.rs`, `lsp/provider.rs`, `multi.rs` |
| Search relevance | `src/indexer/search.rs` pipeline → `src/reranker.rs` → `src/reasoning.rs` (LLM re-rank) → `src/store/weighted_rrf.rs` (fusion) |
| Structural search | engine: `src/grep.rs` (ast-grep) · CLI: `src/commands/grep.rs` |
| GraphRAG | `src/indexer/graphrag/` (`builder.rs`, `symbols.rs`, `ai.rs`, `relationships.rs`, `runtime.rs`) · storage: `src/store/graphrag.rs` · CLI: `src/commands/graphrag.rs` |
| Store internals | `src/store/mod.rs` (block types: code/text/document/commit) · `table_ops.rs` · `batch_converter.rs` · auto index tuning: `vector_optimizer.rs` |
| Branch delta indexes | `src/indexer/branch.rs` + `src/commands/branch.rs` |
| Commit history indexing | `src/indexer/commits/` + `src/indexer/git_utils.rs` |
| Markdown/docs indexing | `src/indexer/markdown_processor.rs` |
| AI commit/review/release | `src/commands/{commit,review,release}.rs` — git-heavy, shell out to `git` |
| EditorConfig formatter | `src/commands/format/` |
| Paths, locks, state | `src/storage.rs` (per-project hash dir) · `src/lock.rs` (stale-lock detection) · `src/state.rs` |
| Embedding/LLM calls | only via wrappers `src/embedding/mod.rs` and `src/llm/mod.rs` |

## Conventions

- Tests live in sibling files: `foo_tests.rs` (languages: `foo_test.rs`), declared `#[cfg(test)] mod foo_tests;` — coverage excludes them by `(_tests?\.rs|/tests\.rs)$`
- rustfmt `hard_tabs = true` — tabs, never spaces, in `.rs`
- Every `.rs` file starts with the Apache 2.0 header (`// Copyright <year> Muvon Un Limited` + license block); bump the year when modifying
- Models are always `"provider:model"` (e.g. `voyage:voyage-code-3`, `openrouter:openai/gpt-4o-mini`)
- `Config::default()` loads the embedded template — keep `Default` impls and `config-templates/default.toml` in sync; never introduce a third value elsewhere
- MCP server code never uses `println!`/`eprintln!` — stdout is the protocol; log files-only via `src/mcp/logging.rs`
- MCP tool failures return a `CallToolResult` with `is_error = true`, never `Err` (see `src/mcp/server.rs`)
- Gate `fastembed`/`huggingface` code behind `#[cfg(feature = "...")]` with an `Err(...)` fallback for the off case
- Async-first tokio; `#[derive(Clone)]` for structs shared across async contexts; no `.unwrap()` outside tests
- One codebase for Linux/macOS/Windows — CI tests all three plus musl static builds; no platform-specific forks

## Done

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo check --all-targets --all-features`
- `cargo test --all-features`

## Gotchas

- `--no-default-features` is the fast dev loop; CI and the Done gate run `--all-features` (local ONNX models, needs `protoc`)
- Linux full builds need `ORT_LIB_LOCATION` pointing at a static ONNX Runtime; Windows needs `RUSTFLAGS=-C target-feature=-crt-static` (see `.github/workflows/ci.yml`)
- `time` is pinned `=0.3.47` (E0119 vs tantivy-common) and `esaxx-rs` is a SHA-pinned `[patch.crates-io]` fork (MSVC /MT vs /MD) — both look like cruft but are load-bearing
- Index data lives outside the repo (`~/.local/share/octocode/<project-hash>/`); changing embedding dimensions auto-drops tables and wipes the index
- `index.require_git = true` by default — indexing non-git dirs fails; `mcp --no-git` opts out
- File walking respects `.gitignore` AND `.noindex`
- The MCP server serves the existing index read-only unless `index.mcp_index = true`
- In TOML config files, scalars must precede nested table headers (e.g. `[search.reranker]`) or they are silently ignored
- Remote AI features need provider API keys (`OPENROUTER_API_KEY`, `VOYAGE_API_KEY`, …); `.env` is auto-loaded

## Never

- Implement embedding/LLM/reranker providers here — they belong in `octolib`
- Call `octolib` APIs directly from commands — go through `src/embedding/mod.rs` / `src/llm/mod.rs`
- Open LanceDB tables or create vector indexes manually; hardcode dimensions, partitions, or nprobes — `Store` + `vector_optimizer.rs` own this
- Print to stdout/stderr in MCP server code
- Use `--release` during development
- Commit `.rs` files without the Apache header · commit `.env` or `.mcpregistry_*` token files
- Add a config field without a matching entry in `config-templates/default.toml`

## References

- `doc/ARCHITECTURE.md` — module boundaries; read before cross-cutting changes
- `doc/CONFIGURATION.md` — every option · `doc/COMMANDS.md` — full CLI surface
- `doc/MCP_INTEGRATION.md` / `doc/MCP_CLIENTS.md` / `doc/LSP_INTEGRATION.md` — server modes, clients, LSP tools
- `doc/RELEASE_MANAGEMENT.md` — before touching `src/commands/release.rs` or publishing (`make release`, `make git-tag`)
- `doc/API_KEYS.md` — provider keys · `benchmark/` — retrieval benchmark (matrix runner, ground truth)
