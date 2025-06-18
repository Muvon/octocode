# Octocode Development Instructions

## Core Principles

### Strict Configuration Management
- **NO DEFAULTS**: All configuration must be explicitly defined in `config-templates/default.toml`
- **Template-First**: Update template file when adding new config options
- **Environment Override**: Use env vars for sensitive data (API keys)
- **Version Control**: Config has version field for future migrations

### Code Reuse & Architecture

#### Indexer Core Pattern
```rust
// Always use this pattern for file processing
let lang_impl = languages::get_language(language)?;
parser.set_language(&lang_impl.get_ts_language())?;
extract_meaningful_regions(tree.root_node(), contents, lang_impl.as_ref(), &mut regions);
```

#### Watcher Integration
- Use `NoindexWalker` for file discovery (respects .gitignore + .noindex)
- Git optimization: only reindex changed files between commits
- File metadata caching for skip-unchanged logic

#### Storage Pattern
```rust
// Batch processing for efficiency
if should_process_batch(&blocks_batch, |b| &b.content, config) {
    process_blocks_batch(store, &blocks_batch, config).await?;
    blocks_batch.clear();
    flush_if_needed(store, &mut batches_processed, config, false).await?;
}
```

## Project Structure

### Core Modules
- `src/indexer/` - Tree-sitter parsing, semantic extraction
- `src/indexer/languages/` - Language-specific implementations
- `src/indexer/graphrag/` - Knowledge graph generation
- `src/embedding/` - Multi-provider embedding system
- `src/commands/` - CLI command implementations
- `src/mcp/` - Model Context Protocol server

### MCP Server Modes
- **Stdin Mode** (default): Standard MCP protocol over stdin/stdout for AI assistant integration
- **HTTP Mode** (`--bind=host:port`): HTTP server for web-based integrations and testing
  ```bash
  # Stdin mode (default)
  octocode mcp --path=/path/to/project

  # HTTP mode
  octocode mcp --bind=0.0.0.0:12345 --path=/path/to/project
  ```

### Key Files
- `config-templates/default.toml` - Single source of configuration truth
- `src/config.rs` - Config loading with template fallback
- `src/indexer/mod.rs` - File processing pipeline
- `src/store.rs` - Lance database operations

## Development Patterns

### Adding New Language Support
1. Create `src/indexer/languages/{lang}.rs`
2. Implement `Language` trait with meaningful_kinds
3. Add to `languages/mod.rs` registry
4. Update `detect_language()` function

### Adding Config Options
1. Update struct in `src/config.rs`
2. Add defaults in `Default` impl
3. **MANDATORY**: Update `config-templates/default.toml`
4. Add validation if needed

### Adding MCP Server Features
1. **Stdin Mode**: Default mode for AI assistant integration
2. **HTTP Mode**: Add `--bind=host:port` for web-based access
3. **Tool Providers**: Implement in `src/mcp/{provider}.rs` with Clone trait
4. **Request Handling**: Use existing pattern for both stdin and HTTP modes
5. **State Management**: Use `Arc<Mutex<>>` for shared state across async handlers

### File Processing Pipeline
1. `create_walker()` - Respects .gitignore/.noindex
2. Git optimization check for changed files
3. Language detection → Tree-sitter parsing
4. Semantic region extraction with smart merging
5. Batch embedding generation
6. Lance database storage

### GraphRAG Integration
- Enabled via `config.graphrag.enabled`
- Builds relationships from AST imports/exports
- Uses LLM for file descriptions (optional)
- Incremental updates on file changes

## Performance Guidelines

### Indexing Optimization
- Batch size: 16 files per embedding batch
- Flush frequency: Every 2 batches (32 files)
- Token limit: 100k tokens per batch
- Git optimization: Skip unchanged files

### Memory Management
- Progressive file counting during indexing
- Preload file metadata in HashMap for O(1) lookup
- Smart merging of single-line declarations
- Context-aware markdown chunking

### Database Efficiency
- Use `content_exists()` before processing
- Batch operations for inserts/updates
- Regular flush cycles for persistence
- Differential processing for file changes

## Watch Mode & File Handling

### File Discovery
```rust
let walker = NoindexWalker::create_walker(&current_dir).build();
// Respects both .gitignore and .noindex patterns
```

### Change Detection
- Git commit hash tracking for optimization
- File modification time caching
- Differential block processing
- Cleanup of deleted/ignored files

### Ignore Patterns
- `.gitignore` - Standard git ignore
- `.noindex` - Octocode-specific ignore
- Config ignore patterns for global exclusions

## Quick Start Checklist

1. **Config First**: Always update `config-templates/default.toml`
2. **No Defaults**: Explicit configuration for all options
3. **Reuse Patterns**: Follow existing indexer/storage patterns
4. **Batch Processing**: Use established batch sizes and flush cycles
5. **Git Integration**: Leverage commit-based optimization
6. **Test Incrementally**: Use watch mode for development iteration

## Development Best Practices

### Fast Development Workflow
- **Use `--no-default-features`** for faster cargo builds during development:
  ```bash
  cargo check --no-default-features
  cargo build --no-default-features
  cargo test --no-default-features
  ```
- **Always run clippy** before finalizing code to ensure clean, warning-free code:
  ```bash
  cargo clippy --all-features --all-targets -- -D warnings
  ```
- **Prefer tokio primitives** over external dependencies when possible (e.g., use tokio for HTTP instead of axum)

### Code Quality Standards
- **Zero clippy warnings** - All code must pass `cargo clippy` without warnings
- **Minimal dependencies** - Reuse existing dependencies before adding new ones
- **Clone trait** - Add `#[derive(Clone)]` to structs that need to be shared across async contexts
- **Error handling** - Use proper `Result<T>` types and meaningful error messages

### MCP Server Development
- **Stdin mode** (default): Use for standard MCP protocol compliance
- **HTTP mode** (`--bind=host:port`): Use for web-based integrations
- **Pure tokio** implementation for HTTP to avoid unnecessary dependencies
- **CORS headers** included for browser compatibility

### Testing Approach
- **Unit tests** for individual components
- **Integration tests** for full workflows
- **Manual testing** with real projects during development
- **HTTP endpoint testing** using curl or similar tools

### Architecture Decisions
- **Configuration-first** - All features configurable via `config-templates/default.toml`
- **Modular providers** - Each tool provider (semantic search, GraphRAG, memory, LSP) is independent
- **Async-first** - Use tokio throughout for non-blocking operations
- **Error resilience** - Graceful degradation when optional features fail
