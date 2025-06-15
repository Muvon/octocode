# Configuration

Octocode stores configuration in `~/.local/share/octocode/config.toml`. View current settings with:

```bash
octocode config --show
```

## Quick Setup Examples

### Local Embedding Models (No API Keys Required)

```bash
# Use SentenceTransformer (recommended for quality)
octocode config \
  --code-embedding-model "sentencetransformer:microsoft/codebert-base" \
  --text-embedding-model "sentencetransformer:sentence-transformers/all-mpnet-base-v2"

# Use FastEmbed (recommended for speed)
octocode config \
  --code-embedding-model "fastembed:all-MiniLM-L6-v2" \
  --text-embedding-model "fastembed:multilingual-e5-small"

# Mix providers as needed
octocode config \
  --code-embedding-model "sentencetransformer:microsoft/codebert-base" \
  --text-embedding-model "fastembed:multilingual-e5-small"
```

### Cloud Embedding Models (API Keys Required)

```bash
# Use cloud providers for highest quality
octocode config \
  --code-embedding-model "jinaai:jina-embeddings-v2-base-code" \
  --text-embedding-model "voyageai:voyage-3"

# Google models
octocode config \
  --code-embedding-model "google:text-embedding-004" \
  --text-embedding-model "google:text-embedding-004"
```

## Configuration File Structure

```toml
[openrouter]
model = "openai/gpt-4o-mini"
api_key = "your-openrouter-key"  # Or set OPENROUTER_API_KEY env var

[embedding]
# Direct model configuration - provider auto-detected from prefix
code_model = "sentencetransformer:microsoft/codebert-base"
text_model = "sentencetransformer:sentence-transformers/all-mpnet-base-v2"

# Provider-specific sections only for API keys
[embedding.jina]
api_key = "your-jina-key"  # Or set JINA_API_KEY env var

[embedding.voyage]
api_key = "your-voyage-key"  # Or set VOYAGE_API_KEY env var

[embedding.google]
api_key = "your-google-key"  # Or set GOOGLE_API_KEY env var

[graphrag]
enabled = true
description_model = "openai/gpt-4o-mini"
relationship_model = "openai/gpt-4o-mini"

[search]
max_results = 50
similarity_threshold = 0.1

[index]
chunk_size = 2000
graphrag_enabled = true

[memory]
enabled = true
max_memories = 10000
```

## Embedding Providers

### Supported Providers

| Provider | Format | API Key Required | Local/Cloud | Quality | Speed |
|----------|--------|------------------|-------------|---------|-------|
| **SentenceTransformer** | `sentencetransformer:model-name` | ❌ No | 🖥️ Local | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ |
| **FastEmbed** | `fastembed:model-name` | ❌ No | 🖥️ Local | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| **Jina AI** | `jinaai:model-name` | ✅ Yes | ☁️ Cloud | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| **Voyage AI** | `voyageai:model-name` | ✅ Yes | ☁️ Cloud | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| **Google** | `google:model-name` | ✅ Yes | ☁️ Cloud | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ |

### Model Recommendations

#### For Code Understanding (code_model)

**Best Quality:**
```bash
sentencetransformer:microsoft/codebert-base        # 768 dim, excellent for code
sentencetransformer:microsoft/unixcoder-base       # 768 dim, Unix/shell code
jinaai:jina-embeddings-v2-base-code               # 768 dim, specialized for code
voyageai:voyage-code-2                            # 1536 dim, high quality
```

**Fast Local:**
```bash
fastembed:all-MiniLM-L6-v2                        # 384 dim, fast and efficient
fastembed:BAAI/bge-small-en-v1.5                  # 384 dim, good balance
```

#### For Text/Documentation (text_model)

**Best Quality:**
```bash
sentencetransformer:sentence-transformers/all-mpnet-base-v2  # 768 dim, excellent quality
sentencetransformer:BAAI/bge-base-en-v1.5                   # 768 dim, high performance
jinaai:jina-embeddings-v3                                   # 1024 dim, latest Jina model
voyageai:voyage-3                                           # 1024 dim, excellent for text
```

**Fast Local:**
```bash
fastembed:multilingual-e5-small                    # 384 dim, supports multiple languages
sentencetransformer:sentence-transformers/all-MiniLM-L6-v2  # 384 dim, fast
```

## Environment Variables

```bash
# OpenRouter for AI features
export OPENROUTER_API_KEY="your-openrouter-api-key"

# Cloud embedding providers (if using)
export JINA_API_KEY="your-jina-key"
export VOYAGE_API_KEY="your-voyage-key"
export GOOGLE_API_KEY="your-google-key"
```

**Note**: Environment variables always take priority over config file settings.

## Configuration Sections

### [openrouter]
Controls AI model usage for GraphRAG and Git features.

- `model`: OpenRouter model identifier (default: "openai/gpt-4o-mini")
- `api_key`: API key (prefer environment variable)

### [embedding]
Core embedding configuration.

- `code_model`: Model for code embedding
- `text_model`: Model for text/documentation embedding

### [graphrag]
Knowledge graph generation settings.

- `enabled`: Enable/disable GraphRAG features
- `description_model`: Model for generating file descriptions
- `relationship_model`: Model for extracting relationships

### [search]
Search behavior configuration.

- `max_results`: Maximum search results to return
- `similarity_threshold`: Minimum similarity score for results

### [index]
Indexing behavior settings.

- `chunk_size`: Size of text chunks for embedding
- `graphrag_enabled`: Enable GraphRAG during indexing

### [memory]
Memory system configuration.

- `enabled`: Enable/disable memory features
- `max_memories`: Maximum number of memories to store

## Command Line Configuration

```bash
# View current configuration
octocode config --show

# Set embedding models
octocode config --code-embedding-model "fastembed:all-MiniLM-L6-v2"
octocode config --text-embedding-model "fastembed:multilingual-e5-small"

# Set OpenRouter model
octocode config --model "anthropic/claude-3.5-sonnet"

# Enable/disable GraphRAG
octocode config --graphrag-enabled true
octocode config --graphrag-enabled false

# Set search parameters
octocode config --max-results 100
octocode config --similarity-threshold 0.3
```

## MCP Server Configuration

### Basic MCP Setup

```bash
# Start MCP server with default settings
octocode mcp --path /path/to/project

# Start with custom port
octocode mcp --path /path/to/project --port 3001

# Start with debug logging
octocode mcp --path /path/to/project --debug
```

### LSP Integration

```bash
# Enable LSP integration with Rust
octocode mcp --path /path/to/rust/project --with-lsp "rust-analyzer"

# Enable LSP integration with Python
octocode mcp --path /path/to/python/project --with-lsp "pylsp"

# Enable LSP integration with TypeScript
octocode mcp --path /path/to/ts/project --with-lsp "typescript-language-server --stdio"

# Custom LSP server with arguments
octocode mcp --path /path/to/project --with-lsp "custom-lsp --config config.json"
```

### MCP Configuration File

The MCP server uses command-line arguments rather than configuration file settings. The main configuration is handled through the existing `config.toml` structure:

```toml
# Octocode configuration (config-templates/default.toml)
version = 1

[openrouter]
model = "openai/gpt-4.1-mini"
base_url = "https://openrouter.ai/api/v1"
timeout = 120

[index]
chunk_size = 2000
chunk_overlap = 100
embeddings_batch_size = 16
require_git = true

[search]
max_results = 20
similarity_threshold = 0.65
output_format = "markdown"

[embedding]
code_model = "voyage:voyage-code-3"
text_model = "voyage:voyage-3.5-lite"

[graphrag]
enabled = false
use_llm = false
```

**Note**: MCP server settings like port, debug mode, and LSP integration are controlled via command-line flags, not configuration file options.

### Claude Desktop Integration

Add to your Claude Desktop configuration file:

**macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`
**Windows**: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "octocode": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project"]
    },
    "octocode-with-lsp": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/your/project", "--with-lsp", "rust-analyzer"]
    }
  }
}
```

### Multiple Projects Setup

```json
{
  "mcpServers": {
    "octocode-rust": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/rust/project", "--with-lsp", "rust-analyzer", "--port", "3001"]
    },
    "octocode-python": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/python/project", "--with-lsp", "pylsp", "--port", "3002"]
    },
    "octocode-typescript": {
      "command": "octocode",
      "args": ["mcp", "--path", "/path/to/ts/project", "--with-lsp", "typescript-language-server --stdio", "--port", "3003"]
    }
  }
}
```

## Performance Tuning

### For Speed
```toml
[embedding]
code_model = "fastembed:all-MiniLM-L6-v2"
text_model = "fastembed:multilingual-e5-small"

[index]
chunk_size = 1000
embeddings_batch_size = 64

[search]
max_results = 20
```

### For Quality
```toml
[embedding]
code_model = "sentencetransformer:microsoft/codebert-base"
text_model = "sentencetransformer:sentence-transformers/all-mpnet-base-v2"

[index]
chunk_size = 2000

[search]
max_results = 50
similarity_threshold = 0.1
```

### For Large Codebases
```toml
[index]
chunk_size = 1500
embeddings_batch_size = 32

[search]
max_results = 30
similarity_threshold = 0.2

[memory]
max_memories = 50000
```
