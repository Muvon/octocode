# Configuration

Octocode stores configuration in `~/.local/share/octocode/config.toml`. View current settings with:

```bash
octocode config --show
```

## Quick Setup Examples

### Local Embedding Models (No API Keys Required)

```bash
# Use HuggingFace models (recommended for quality)
octocode config \
  --code-embedding-model "huggingface:microsoft/codebert-base" \
  --text-embedding-model "huggingface:sentence-transformers/all-mpnet-base-v2"

# Use FastEmbed (recommended for speed)
octocode config \
  --code-embedding-model "fastembed:Xenova/all-MiniLM-L6-v2" \
  --text-embedding-model "fastembed:intfloat/multilingual-e5-small"

# Mix providers as needed
octocode config \
  --code-embedding-model "huggingface:microsoft/codebert-base" \
  --text-embedding-model "fastembed:intfloat/multilingual-e5-small"
```

### Cloud Embedding Models (API Keys Required)

```bash
# Use cloud providers for highest quality (current defaults)
octocode config \
  --code-embedding-model "voyage:voyage-code-3" \
  --text-embedding-model "voyage:voyage-3.5-lite"

# Jina AI models (specialized for code)
octocode config \
  --code-embedding-model "jina:jina-embeddings-v2-base-code" \
  --text-embedding-model "jina:jina-embeddings-v4"

# Google models
octocode config \
  --code-embedding-model "google:text-embedding-005" \
  --text-embedding-model "google:gemini-embedding-001"

# OpenAI models (high quality)
octocode config \
  --code-embedding-model "openai:text-embedding-3-small" \
  --text-embedding-model "openai:text-embedding-3-small"
```

## Configuration File Structure

```toml
version = 1

[openrouter]
model = "openai/gpt-4.1-mini"
base_url = "https://openrouter.ai/api/v1"
timeout = 120
# api_key = "" # Set via environment variable OPENROUTER_API_KEY

[embedding]
# Current defaults - provider auto-detected from prefix
code_model = "voyage:voyage-code-3"
text_model = "voyage:voyage-3.5-lite"

[graphrag]
enabled = false
use_llm = false

[graphrag.llm]
description_model = "openai/gpt-4.1-mini"
relationship_model = "openai/gpt-4.1-mini"
ai_batch_size = 8
max_batch_tokens = 16384
batch_timeout_seconds = 60
fallback_to_individual = true
max_sample_tokens = 1500
confidence_threshold = 0.6
architectural_weight = 0.9

[search]
max_results = 20
similarity_threshold = 0.65
output_format = "markdown"
max_files = 10
context_lines = 3
search_block_max_characters = 400

[index]
chunk_size = 2000
chunk_overlap = 100
embeddings_batch_size = 16
embeddings_max_tokens_per_batch = 100000
flush_frequency = 2
require_git = true
```

## Embedding Providers

### Supported Providers

| Provider | Format | API Key Required | Local/Cloud | Quality | Speed |
|----------|--------|------------------|-------------|---------|-------|
| **HuggingFace** | `huggingface:model-name` | ❌ No | 🖥️ Local | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ |
| **FastEmbed** | `fastembed:model-name` | ❌ No | 🖥️ Local | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| **Jina AI** | `jina:model-name` | ✅ Yes | ☁️ Cloud | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| **Voyage AI** | `voyage:model-name` | ✅ Yes | ☁️ Cloud | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| **Google** | `google:model-name` | ✅ Yes | ☁️ Cloud | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| **OpenAI** | `openai:model-name` | ✅ Yes | ☁️ Cloud | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ |

### Model Recommendations

#### For Code Understanding (code_model)

**Best Quality:**
```bash
huggingface:microsoft/codebert-base                    # 768 dim, BERT, excellent for code
huggingface:jinaai/jina-embeddings-v2-base-code       # 768 dim, JinaBERT, code-optimized
jina:jina-embeddings-v2-base-code                     # 768 dim, specialized for code
voyage:voyage-code-3                                  # Dynamic dim, latest code model
openai:text-embedding-3-small                         # 1536 dim, versatile for code
```

**Fast Local:**
```bash
fastembed:Xenova/all-MiniLM-L6-v2                     # 384 dim, fast and efficient
fastembed:Xenova/bge-small-en-v1.5                    # 384 dim, good balance
```

#### For Text Understanding (text_model)

**Best Quality:**
```bash
huggingface:sentence-transformers/all-mpnet-base-v2   # 768 dim, BERT, excellent quality
huggingface:BAAI/bge-base-en-v1.5                     # 768 dim, BERT, high performance
jina:jina-embeddings-v4                               # 2048 dim, latest Jina model
voyage:voyage-3.5-lite                                # Dynamic dim, excellent for text
openai:text-embedding-3-large                         # 3072 dim, highest quality
openai:text-embedding-3-small                         # 1536 dim, cost-effective
```

**Fast Local:**
```bash
fastembed:intfloat/multilingual-e5-small                 # 384 dim, supports multiple languages
huggingface:sentence-transformers/all-MiniLM-L6-v2    # 384 dim, BERT, fast
```

**Note**: HuggingFace provider supports BERT and JinaBERT architectures with automatic dimension detection.
```

## Environment Variables

```bash
# OpenRouter for AI features
export OPENROUTER_API_KEY="your-openrouter-api-key"

# Cloud embedding providers (if using)
export JINA_API_KEY="your-jina-key"
export VOYAGE_API_KEY="your-voyage-key"
export GOOGLE_API_KEY="your-google-key"
export OPENAI_API_KEY="your-openai-key"
```

**Note**: Environment variables always take priority over config file settings. API keys are sourced from environment variables only - they are not stored in the configuration file for security.

## Configuration Sections

### LLM Configuration

```toml
[llm]
# Model in provider:model format
model = "openrouter:openai/gpt-4o-mini"
timeout = 120
temperature = 0.7
max_tokens = 4000
```

**Fields:**
- `model`: LLM model in `provider:model` format (e.g., "openai:gpt-4o-mini", "anthropic:claude-3-5-haiku-20241022")
- `timeout`: Request timeout in seconds (default: 120)
- `temperature`: Sampling temperature 0.0-1.0 (default: 0.7)
- `max_tokens`: Maximum tokens in response (default: 4000)

**Supported Providers:**
- `openrouter:` - Access multiple providers through OpenRouter
- `openai:` - Direct OpenAI API
- `anthropic:` - Anthropic Claude models
- `google:` - Google Gemini models
- `deepseek:` - DeepSeek models

**API Keys:** Set via environment variables (OPENAI_API_KEY, ANTHROPIC_API_KEY, OPENROUTER_API_KEY, etc.)

### [embedding]
Core embedding configuration.

- `code_model`: Model for code embedding
- `text_model`: Model for text/documentation embedding

### [graphrag]

Knowledge graph generation settings.

- `enabled`: Enable/disable GraphRAG features
- `use_llm`: Enable AI-powered relationship discovery and file descriptions

### [graphrag.llm]

LLM-specific configuration for GraphRAG AI features.

- `description_model`: Model for generating file descriptions
- `relationship_model`: Model for extracting relationships
- `ai_batch_size`: Number of files to analyze per AI call for cost optimization (default: 8)
- `max_batch_tokens`: Maximum tokens per batch request to avoid model limits (default: 16384)
- `batch_timeout_seconds`: Timeout for batch AI requests in seconds (default: 60)
- `fallback_to_individual`: Whether to fallback to individual AI calls if batch fails (default: true)
- `max_sample_tokens`: Maximum content sample size sent to AI (default: 1500)
- `confidence_threshold`: Confidence threshold for AI relationships (default: 0.8)
- `architectural_weight`: Weight for AI-discovered relationships (default: 0.9)
- `relationship_system_prompt`: System prompt for relationship discovery
- `description_system_prompt`: System prompt for file descriptions

**Performance Note**: Increasing `ai_batch_size` reduces API costs by processing multiple files per request, but may increase latency. Adjust `max_batch_tokens` to stay within model context limits.

### [search]
Search behavior configuration.

- `max_results`: Maximum search results to return
- `similarity_threshold`: Minimum similarity score for results

### [index]
Indexing behavior settings.
# Set LLM model
octocode config --model "openrouter:openai/gpt-4o-mini"

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
code_model = "huggingface:microsoft/codebert-base"
text_model = "huggingface:sentence-transformers/all-mpnet-base-v2"

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
