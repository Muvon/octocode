# Contributing to Octocode

We welcome contributions! This project is part of the larger Muvon ecosystem and follows our open-source contribution guidelines.

## Development Setup

### Prerequisites

- **Rust 1.95+** (install from [rustup.rs](https://rustup.rs/))
- **Git** for version control
- **Basic understanding** of Rust, embeddings, and vector databases

### Getting Started

```bash
# Clone the repository
git clone https://github.com/muvon/octocode.git
cd octocode

# Build the project (use --no-default-features to skip local model compilation)
cargo build --no-default-features

# Run tests
cargo test --no-default-features

# Check code quality
cargo check --no-default-features --message-format=short
cargo clippy --no-default-features

# Run with debug logging
RUST_LOG=debug cargo run -- index
```

### Development Dependencies

The project uses several key dependencies:

- **Tree-sitter**: For parsing multiple programming languages
- **Lance**: Vector database for embeddings storage
- **Tokio**: Async runtime
- **Clap**: Command-line interface
- **Serde**: Serialization/deserialization
- **Reqwest**: HTTP client for API calls

## Project Structure

octocode/
├── src/
│   ├── main.rs              # CLI entry point and command dispatch
│   ├── config.rs            # All config structs; values come from config-templates/default.toml
│   ├── storage.rs           # Per-project database path resolution
│   ├── store/               # LanceDB operations (block types, table ops, vector optimizer)
│   ├── embedding/           # Thin wrapper over octolib embedding providers
│   ├── llm/                 # Thin wrapper over octolib LLM providers
│   ├── indexer/             # Code indexing and parsing
│   │   ├── languages/       # Language-specific parsers (one file per language)
│   │   ├── graphrag/        # GraphRAG builder and AI relationship discovery
│   │   └── commits/         # Git commit history indexing
│   ├── mcp/                 # MCP server implementation
│   ├── commands/            # One file per CLI subcommand
│   ├── grep.rs              # Structural code search (ast-grep)
│   ├── reranker.rs          # Search result reranking
│   └── reasoning.rs         # LLM re-ranking fused with hybrid results
├── doc/                     # Documentation
└── benchmark/               # Retrieval-quality benchmark and ground truth
```

## Adding Language Support

Language parsers are located in `src/indexer/languages/`. Each language needs:

### 1. Tree-sitter Grammar Dependency

Add the tree-sitter grammar to `Cargo.toml`:

```toml
[dependencies]
tree-sitter-your-language = "0.x.x"
```

### 2. Language Implementation
```rust
use tree_sitter::Node;

use super::{deduplicate_symbols, extract_symbols_by_kinds, CallTarget, Language};

pub struct YourLanguage;

impl Language for YourLanguage {
    fn name(&self) -> &'static str {
        "your_lang"
    }

    fn get_ts_language(&self) -> tree_sitter::Language {
        tree_sitter_your_language::LANGUAGE.into()
    }

    fn get_meaningful_kinds(&self) -> Vec<&'static str> {
        vec!["function_definition", "class_definition"]
    }

    fn extract_symbols(&self, node: Node, contents: &str) -> Vec<String> {
        let mut symbols = extract_symbols_by_kinds(node, contents, self.get_meaningful_kinds());
        deduplicate_symbols(&mut symbols);
        symbols
    }

    fn get_file_extensions(&self) -> Vec<&'static str> {
        vec!["yourext"]
    }

    // implement the remaining `Language` trait methods
}
```

The full trait lives in `src/indexer/languages/mod.rs`; `Language` requires `Send + Sync`.

### 3. Registration
```rust
pub mod your_lang;

// In get_language():
match name {
    // ... existing arms
    "your_lang" => Some(Box::new(YourLanguage {})),
    _ => None,
}
```

Language detection resolves through `crate::language::associated_language` first (project `[index.file_associations]` overrides), then falls back to the built-in extension map in `src/indexer/file_utils.rs::detect_language`. Add the new extension there so indexing and GraphRAG pick the language up, and to `src/grep.rs::language_from_extension` if structural search should support it too.

### 4. Testing

Tests live next to the implementation as a `#[cfg(test)] mod tests` block in the same file (e.g. `src/indexer/languages/your_lang.rs`), matching the rest of the crate.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_your_language_parsing() {
        let source = "fn main() {}";
        let language = YourLanguage {};
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language.get_ts_language())
            .expect("loading grammar");
        let tree = parser.parse(source, None).expect("parsing source");

        let symbols = language.extract_symbols(tree.root_node(), source);
        assert!(!symbols.is_empty());
    }
}
```

## Adding Embedding Providers

Embedding providers live in the `octolib` crate, not here. To add one, implement it in `octolib` and re-export it from `src/embedding/mod.rs`. This repo only wraps octolib with retrying batch generation, mode-aware query embedding, and content hashing.

Providers currently reachable through `octolib`: Voyage, Jina, Google, OpenAI, OctoHub, Together, FastEmbed, HuggingFace.

### 1. Provider Implementation

Implement the provider inside `octolib` (its embedding module owns provider construction, model validation, and dimension lookup). `src/embedding/mod.rs` only re-exports octolib types and adds the retrying batch helpers, so nothing here needs a new provider file.

### 2. Provider Availability

Provider availability is gated by the `fastembed` and `huggingface` features declared in `Cargo.toml` and forwarded to `octolib`. A provider that is compiled out fails fast at construction rather than silently degrading.

## Code Style and Guidelines

### Rust Style

- Follow standard Rust formatting (`cargo fmt`)
- Use `cargo clippy` for linting
- Write comprehensive tests for new features
- Document public APIs with rustdoc comments

### Error Handling

Use `anyhow` for fallible command and library code, adding context at the boundary that handles the failure:

```rust
use anyhow::{Context, Result};

fn your_function() -> Result<String> {
    let result = some_operation().context("loading project config")?;
    Ok(result)
}
```

### Async Code

Use `tokio` for async operations:

```rust
use tokio::fs;

async fn read_file(path: &str) -> Result<String> {
    let content = fs::read_to_string(path).await?;
    Ok(content)
}
```

## Testing

### Running Tests

```bash
# Run all tests
cargo test --no-default-features

# Run specific test module
cargo test --no-default-features test_rust_parser

# Run with output
cargo test --no-default-features -- --nocapture

# Clippy must pass clean
cargo clippy --all-features --all-targets -- -D warnings
```

### Test Categories

1. **Unit Tests**: Test individual functions and modules
2. **Integration Tests**: Test complete workflows
3. **Language Tests**: Test language parser implementations
4. **Embedding Tests**: Test embedding provider integrations

### Writing Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_indexing_workflow() {
        let temp_dir = TempDir::new().unwrap();
        // Test implementation
    }

    #[test]
    fn test_symbol_extraction() {
        let source = "fn main() {}";
        let symbols = extract_symbols(source);
        assert_eq!(symbols.len(), 1);
    }
}
```

## Documentation

### Code Documentation

Use rustdoc comments for public APIs:

```rust
/// Deduplicates and sorts a symbol list in place.
///
/// # Arguments
///
/// * `symbols` - Symbols collected from a tree-sitter walk
///
/// # Examples
///
/// ```ignore
/// let mut symbols = vec!["b".to_string(), "a".to_string(), "a".to_string()];
/// deduplicate_symbols(&mut symbols);
/// assert_eq!(symbols, vec!["a".to_string(), "b".to_string()]);
/// ```
pub fn deduplicate_symbols(symbols: &mut Vec<String>) {
    symbols.sort();
    symbols.dedup();
}
```

### Updating Documentation

When adding features, update:

1. **README.md**: If it affects the main workflow
2. **doc/COMMANDS.md**: For new or changed CLI flags and subcommands
3. **doc/CONFIGURATION.md**: For new configuration options (`config-templates/default.toml` is the source of truth)
4. **doc/ADVANCED_USAGE.md**: For new advanced features
5. **doc/ARCHITECTURE.md**: For architectural changes

## Submitting Changes

### Pull Request Process

1. **Fork the repository** and create a feature branch
2. **Make your changes** following the style guidelines
3. **Add tests** for new functionality
4. **Update documentation** as needed
5. **Run the test suite** to ensure everything passes
6. **Submit a pull request** with a clear description

### Commit Messages

Follow conventional commit format:

```
feat(indexer): add support for Go language parsing

- Implement Go-specific symbol extraction
- Add import/export detection for Go modules
- Include comprehensive test coverage

Closes #123
```

### PR Description Template

```markdown
## Description
Brief description of the changes.

## Type of Change
- [ ] Bug fix
- [ ] New feature
- [ ] Breaking change
- [ ] Documentation update

## Testing
- [ ] Unit tests added/updated
- [ ] Integration tests added/updated
- [ ] Manual testing performed

## Checklist
- [ ] Code follows style guidelines
- [ ] Self-review completed
- [ ] Documentation updated
- [ ] Tests pass locally
```

## Release Process

### Version Numbering

We follow [Semantic Versioning](https://semver.org/):

- **MAJOR**: Breaking changes
- **MINOR**: New features (backward compatible)
- **PATCH**: Bug fixes (backward compatible)

### Release Checklist

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md`
3. Run full test suite
4. Create release tag
5. Build and test release binary
6. Update documentation

## Getting Help

### Communication Channels

- **GitHub Issues**: Bug reports and feature requests
- **Email**: [opensource@muvon.io](mailto:opensource@muvon.io)
- **Discussions**: GitHub Discussions for questions

### Reporting Issues

When reporting bugs, include:

1. **Environment**: OS, Rust version, Octocode version
2. **Steps to reproduce**: Clear reproduction steps
3. **Expected behavior**: What should happen
4. **Actual behavior**: What actually happens
5. **Logs**: Relevant error messages or debug output

### Feature Requests

For feature requests, provide:

1. **Use case**: Why is this feature needed?
2. **Proposed solution**: How should it work?
3. **Alternatives**: Other approaches considered
4. **Additional context**: Any other relevant information

## Code of Conduct

We are committed to providing a welcoming and inclusive environment. Please:

- Be respectful and constructive in discussions
- Focus on what is best for the community
- Show empathy towards other community members
- Accept constructive criticism gracefully

## License

By contributing to Octocode, you agree that your contributions will be licensed under the Apache License 2.0.
