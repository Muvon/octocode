# API Keys Setup Guide

Octocode requires API keys for embedding generation and optional AI features. This guide covers all supported providers and setup methods.

## Required: Embedding Providers

Octocode needs embeddings to function. You must configure at least one embedding provider.

### Voyage AI (Recommended)

**Free tier**: 200M tokens per month
**Best for**: High-quality embeddings with generous free tier

```bash
# Set environment variable
export VOYAGE_API_KEY="your-voyage-api-key"

# Or configure in config file
octocode config \
  --code-embedding-model "voyage:voyage-code-3" \
  --text-embedding-model "voyage:voyage-3.5-lite"
```

**Get API key**: [voyageai.com](https://www.voyageai.com/)

### Jina AI

**Best for**: Code-specialized embeddings

```bash
# Set environment variable
export JINA_API_KEY="your-jina-api-key"

# Configure models
octocode config \
  --code-embedding-model "jina:jina-embeddings-v2-base-code" \
  --text-embedding-model "jina:jina-embeddings-v3"
```

**Get API key**: [jina.ai](https://jina.ai/)

### Google AI

**Best for**: Integration with Google ecosystem

```bash
# Set environment variable
export GOOGLE_API_KEY="your-google-api-key"

# Configure models
octocode config \
  --code-embedding-model "google:text-embedding-004" \
  --text-embedding-model "google:text-embedding-004"
```

**Get API key**: [Google AI Studio](https://makersuite.google.com/app/apikey)

### Local Models (macOS Only)

**Best for**: Privacy, no API costs, offline usage

```bash
# FastEmbed (fastest)
octocode config \
  --code-embedding-model "fastembed:all-MiniLM-L6-v2" \
  --text-embedding-model "fastembed:multilingual-e5-small"

# SentenceTransformer (highest quality)
octocode config \
  --code-embedding-model "sentencetransformer:microsoft/codebert-base" \
  --text-embedding-model "sentencetransformer:sentence-transformers/all-mpnet-base-v2"
```

**Note**: Local models require building from source on macOS. Prebuilt binaries use cloud embeddings only.

## Optional: LLM Provider

For AI-powered features like commit messages, code review, and GraphRAG descriptions.

### OpenRouter (Recommended)

**Best for**: Access to multiple LLM providers through one API

```bash
# Set environment variable
export OPENROUTER_API_KEY="your-openrouter-api-key"

# Configure default model
octocode config --model "openai/gpt-4o-mini"

# Or use Claude for better code understanding
octocode config --model "anthropic/claude-3.5-sonnet"
```

**Get API key**: [openrouter.ai](https://openrouter.ai/)

**Popular models:**
- `openai/gpt-4o-mini` - Fast and cost-effective
- `openai/gpt-4o` - High quality
- `anthropic/claude-3.5-sonnet` - Excellent for code
- `google/gemini-pro` - Good balance

## Platform Limitations

### Windows/Linux
- **Must use cloud embeddings** (Voyage AI, Jina AI, Google)
- **Cannot use local models** (FastEmbed, SentenceTransformer)
- **Reason**: ONNX Runtime compatibility issues

### macOS
- **Can use all providers** (cloud + local)
- **Local models available** when building from source
- **Prebuilt binaries** use cloud embeddings only

## Configuration Methods

### Environment Variables (Recommended)

```bash
# Add to your shell profile (.bashrc, .zshrc, etc.)
export VOYAGE_API_KEY="your-voyage-api-key"
export OPENROUTER_API_KEY="your-openrouter-api-key"

# Reload your shell
source ~/.bashrc  # or ~/.zshrc
```

### Configuration File

API keys are stored in `~/.local/share/octocode/config.toml`:

```toml
[embedding.voyage]
api_key = "your-voyage-api-key"

[embedding.jina]
api_key = "your-jina-api-key"

[embedding.google]
api_key = "your-google-api-key"

[openrouter]
api_key = "your-openrouter-api-key"
model = "openai/gpt-4o-mini"
```

### Command Line Configuration

```bash
# View current configuration
octocode config --show

# Set embedding models
octocode config --code-embedding-model "voyage:voyage-code-3"
octocode config --text-embedding-model "voyage:voyage-3.5-lite"

# Set LLM model
octocode config --model "anthropic/claude-3.5-sonnet"
```

## Model Recommendations

### For Code Understanding (code_model)

**Best Quality:**
- `sentencetransformer:microsoft/codebert-base` (768 dim, local)
- `jina:jina-embeddings-v2-base-code` (768 dim, cloud)
- `voyage:voyage-code-3` (1024 dim, cloud)

**Fast Local:**
- `fastembed:all-MiniLM-L6-v2` (384 dim)
- `fastembed:BAAI/bge-small-en-v1.5` (384 dim)

### For Text/Documentation (text_model)

**Best Quality:**
- `sentencetransformer:sentence-transformers/all-mpnet-base-v2` (768 dim, local)
- `jina:jina-embeddings-v3` (1024 dim, cloud)
- `voyage:voyage-3.5-lite` (1024 dim, cloud)

**Fast Local:**
- `fastembed:multilingual-e5-small` (384 dim)
- `sentencetransformer:sentence-transformers/all-MiniLM-L6-v2` (384 dim)

## Quick Setup Examples

### Free Tier Setup (Recommended)

```bash
# Use Voyage AI free tier (200M tokens/month)
export VOYAGE_API_KEY="your-voyage-api-key"

octocode config \
  --code-embedding-model "voyage:voyage-code-3" \
  --text-embedding-model "voyage:voyage-3.5-lite"

# Optional: Add OpenRouter for AI features
export OPENROUTER_API_KEY="your-openrouter-api-key"
octocode config --model "openai/gpt-4o-mini"
```

### Local-Only Setup (macOS)

```bash
# No API keys required
octocode config \
  --code-embedding-model "fastembed:all-MiniLM-L6-v2" \
  --text-embedding-model "fastembed:multilingual-e5-small"

# AI features disabled without OpenRouter key
```

### High-Quality Setup

```bash
# Best embedding quality
export JINA_API_KEY="your-jina-api-key"
export OPENROUTER_API_KEY="your-openrouter-api-key"

octocode config \
  --code-embedding-model "jina:jina-embeddings-v2-base-code" \
  --text-embedding-model "jina:jina-embeddings-v3" \
  --model "anthropic/claude-3.5-sonnet"
```

## Verification

### Test Embedding Configuration

```bash
# Index a small project to test embeddings
octocode index

# If successful, embeddings are working
# If errors, check API keys and model names
```

### Test LLM Configuration

```bash
# Test AI features (requires staged changes)
git add .
octocode commit --dry-run

# If successful, LLM is working
# If errors, check OpenRouter API key
```

### Debug Configuration Issues

```bash
# Show current configuration
octocode config --show

# Check for configuration errors
RUST_LOG=debug octocode index
```

## Cost Management

### Free Tiers

- **Voyage AI**: 200M tokens/month (very generous)
- **OpenRouter**: Varies by model, some have free tiers
- **Google AI**: 15 requests/minute free tier

### Cost Optimization

```bash
# Use smaller, faster models
octocode config \
  --code-embedding-model "voyage:voyage-3.5-lite" \
  --text-embedding-model "voyage:voyage-3.5-lite"

# Use local models when possible (macOS)
octocode config \
  --code-embedding-model "fastembed:all-MiniLM-L6-v2" \
  --text-embedding-model "fastembed:multilingual-e5-small"

# Reduce chunk sizes to use fewer tokens
octocode config --chunk-size 1000
```

## Security Best Practices

### Environment Variables

```bash
# Add to shell profile, not to git
echo 'export VOYAGE_API_KEY="your-key"' >> ~/.bashrc

# Use different keys for different environments
export VOYAGE_API_KEY_DEV="dev-key"
export VOYAGE_API_KEY_PROD="prod-key"
```

### Configuration File Security

```bash
# Ensure config file is not world-readable
chmod 600 ~/.local/share/octocode/config.toml

# Don't commit config files with API keys
echo "config.toml" >> .gitignore
```

## Troubleshooting

### API Key Not Working

1. **Check key format**: Ensure no extra spaces or characters
2. **Verify provider**: Make sure you're using the correct provider prefix
3. **Test directly**: Try the API key with curl or provider's test tools
4. **Check quotas**: Ensure you haven't exceeded rate limits

### Model Not Found

1. **Check model name**: Verify exact model name from provider docs
2. **Check provider prefix**: Ensure correct prefix (voyage:, jina:, etc.)
3. **Update configuration**: Use `octocode config --show` to verify

### Local Models Not Available

1. **Check platform**: Local models only work on macOS
2. **Build from source**: Prebuilt binaries don't include local models
3. **Install dependencies**: Ensure ONNX Runtime is available

For more help, see [Configuration Guide](CONFIGURATION.md) or [Getting Started](GETTING_STARTED.md).
