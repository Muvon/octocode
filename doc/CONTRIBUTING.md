# Contributing to Octocode

We welcome contributions! This project is part of the larger Muvon ecosystem and follows our open-source contribution guidelines.

## Development Setup

### Prerequisites

- **Rust 1.70+** (install from [rustup.rs](https://rustup.rs/))
- **Git** for version control
- **Basic understanding** of Rust, embeddings, and vector databases

### Getting Started

```bash
# Clone the repository
git clone https://github.com/muvon/octocode.git
cd octocode

# Build the project
cargo build

# Run tests
cargo test

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

```
octocode/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── config/              # Configuration management
│   ├── indexer/             # Code indexing and parsing
│   │   ├── languages/       # Language-specific parsers
│   │   └── embeddings/      # Embedding providers
│   ├── search/              # Search engine implementation
│   ├── graphrag/            # Knowledge graph functionality
│   ├── memory/              # Memory management system
│   ├── git/                 # Git integration features
│   ├── mcp/                 # MCP server implementation
│   └── utils/               # Utility functions
├── tests/                   # Integration tests
├── docs/                    # Documentation
└── examples/                # Usage examples
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

Create `src/indexer/languages/your_lang.rs`:

```rust
use tree_sitter::{Language, Query};
use crate::indexer::languages::{LanguageParser, ParsedSymbol, SymbolType};

pub struct YourLanguageParser;

impl LanguageParser for YourLanguageParser {
    fn language() -> Language {
        tree_sitter_your_language::language()
    }

    fn file_extensions() -> &'static [&'static str] {
        &[".your_ext"]
    }

    fn extract_symbols(&self, source: &str) -> Vec<ParsedSymbol> {
        // Implementation for extracting functions, classes, etc.
        vec![]
    }

    fn extract_imports(&self, source: &str) -> Vec<String> {
        // Implementation for extracting import statements
        vec![]
    }

    fn extract_exports(&self, source: &str) -> Vec<String> {
        // Implementation for extracting export statements
        vec![]
    }
}
```

### 3. Registration

Add to `src/indexer/languages/mod.rs`:

```rust
pub mod your_lang;

// In the get_parser function:
match extension {
    // ... existing cases
    ".your_ext" => Some(Box::new(your_lang::YourLanguageParser)),
    _ => None,
}
```

### 4. Testing

Create tests in `tests/languages/test_your_lang.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_your_language_parsing() {
        let source = r#"
            // Your language sample code
        "#;

        let parser = YourLanguageParser;
        let symbols = parser.extract_symbols(source);

        assert!(!symbols.is_empty());
        // Add specific assertions
    }
}
```

## Adding Embedding Providers

Embedding providers are in `src/indexer/embeddings/`. To add a new provider:

### 1. Provider Implementation

Create `src/indexer/embeddings/your_provider.rs`:

```rust
use async_trait::async_trait;
use crate::indexer::embeddings::{EmbeddingProvider, EmbeddingResult};

pub struct YourProvider {
    api_key: String,
    model: String,
}

#[async_trait]
impl EmbeddingProvider for YourProvider {
    async fn embed_texts(&self, texts: &[String]) -> EmbeddingResult<Vec<Vec<f32>>> {
        // Implementation for generating embeddings
        Ok(vec![])
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        // Return embedding dimensions
        768
    }
}
```

### 2. Provider Registration

Add to `src/indexer/embeddings/mod.rs`:

```rust
pub mod your_provider;

// In the create_provider function:
if model.starts_with("yourprovider:") {
    let model_name = model.strip_prefix("yourprovider:").unwrap();
    return Ok(Box::new(your_provider::YourProvider::new(api_key, model_name)?));
}
```

## Code Style and Guidelines

### Rust Style

- Follow standard Rust formatting (`cargo fmt`)
- Use `cargo clippy` for linting
- Write comprehensive tests for new features
- Document public APIs with rustdoc comments

### Error Handling

Use the project's error types:

```rust
use crate::error::{OctocodeError, Result};

fn your_function() -> Result<String> {
    // Use ? operator for error propagation
    let result = some_operation()?;
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
cargo test

# Run specific test module
cargo test test_rust_parser

# Run with output
cargo test -- --nocapture

# Run integration tests
cargo test --test integration
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
/// Extracts symbols from source code using tree-sitter parsing.
///
/// # Arguments
///
/// * `source` - The source code to parse
/// * `language` - The programming language
///
/// # Returns
///
/// A vector of parsed symbols including functions, classes, and variables.
///
/// # Examples
///
/// ```
/// let symbols = extract_symbols("fn main() {}", Language::Rust);
/// assert!(!symbols.is_empty());
/// ```
pub fn extract_symbols(source: &str, language: Language) -> Vec<ParsedSymbol> {
    // Implementation
}
```

### Updating Documentation

When adding features, update:

1. **README.md**: If it affects the main workflow
2. **doc/CONFIGURATION.md**: For new configuration options
3. **doc/ADVANCED_USAGE.md**: For new advanced features
4. **doc/ARCHITECTURE.md**: For architectural changes

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
