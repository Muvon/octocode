# Performance Guide

## Rust crate-root lookup in runtime GraphRAG

Rust import resolution first compares each crate-root candidate (`lib.rs`, then
`main.rs`, walking nearest ancestors first) with the registered paths. Previously
it canonicalized **every Rust registry path before trying any candidate**. For
`R` Rust files and `Q` qualified imports, repeated resolution in the graph builder
made this filesystem work grow roughly with `R × Q`, including external imports.

Canonical registry paths are now computed lazily, once per lookup, only when a
candidate exists and needs the canonical fallback. First-candidate lexical matches perform no
canonicalization. The fallback still returns the original registry spelling and
preserves candidate precedence, including symlinks and `..` aliases.

Run the deterministic regression and compatibility checks:

```sh
cargo test --no-default-features crate_root
cargo test --no-default-features runtime_rust_imports_preserve_edges_after_lazy_root_lookup
```

`lexical_crate_root_does_not_canonicalize_registry_files` uses 258 virtual
registry entries: the eager algorithm performs 258 unnecessary canonicalization
attempts; the lazy algorithm performs zero. The test counts calls rather than
asserting a machine-dependent time limit.

A public scaling fixture can be generated without private source files:

```sh
python3 - /absolute/path/to/scratch-project 200 <<'PYTHON'
from pathlib import Path
import sys
src = Path(sys.argv[1]) / "src"
src.mkdir(parents=True, exist_ok=True)
(src / "lib.rs").write_text("pub mod dep;\n")
(src / "dep.rs").write_text("pub struct Thing;\n")
for i in range(int(sys.argv[2]) - 2):
    (src / f"m{i:04}.rs").write_text(
        "use crate::dep::Thing;\nuse std::sync::Arc;\npub fn run() {}\n")
PYTHON
```

For end-to-end measurements, create a dedicated copy of your config with
`index.mcp_index = false` and `graphrag.enabled = false`. Keep embedding model
settings unchanged: MCP startup opens the store even with indexing disabled.
Build with default features for the stock FastEmbed/local-model config. A
`--no-default-features` build requires a compatible remote-provider config (for
example both embedding models set to `openai:text-embedding-3-small`); it cannot
initialize the stock FastEmbed models.

Capture the executable path in the checkout before switching to the fixture:

```sh
# In the Octocode checkout; use --no-default-features only for a compatible config.
cargo build
octocode_bin="$(pwd)/target/debug/octocode"
export OCTOCODE_CONFIG_PATH="/absolute/path/to/dedicated-config.toml"
cd /absolute/path/to/scratch-project
"$octocode_bin" mcp --path "$PWD" --no-git
```

Use a new directory outside any Git repository so its path-derived project ID
cannot share an existing index.
After JSON-RPC `initialize` and `notifications/initialized`, send
`tools/call` with `{"name":"graphrag","arguments":{"operation":"overview","format":"json"}}`.
Measure from that request to its matching result. A cold sample is a new MCP
process; warm samples repeat the request in the same process. Confirm
`mode = "runtime_structural"`, nonzero counts and no `isError`.

A separate Rust-only diagnostic harness on macOS ARM64, using upstream
`a789880` before/after this change, produced these cold graph-build measurements
(debug profile; not full MCP latency):

| Rust files | Eager | Lazy | Unchanged nodes / relationships |
|---|---:|---:|---:|
| 50 | 574 ms | 44 ms | 100 / 99 |
| 100 | 2111 ms | 91 ms | 200 / 199 |
| 200 | 8591 ms | 197 ms | 400 / 399 |

The graph node IDs and relationship triples matched at every size. These are
local diagnostic observations, not CI timing thresholds. Canonical alias-heavy
projects can still take the expensive fallback; this change removes the eager
work on ordinary lexical matches rather than changing resolution semantics.

Full stdin MCP acceptance on an immutable private mixed-language project snapshot
(754 Rust files, 1047 parsed source files; same macOS ARM64 machine and debug
binary) returned `runtime_structural` over three independent processes in
14.701 / 16.275 / 11.796 seconds. Each process then served three warm overviews
in 19–62 ms. Every cold response contained 18480 nodes and 44752 relationships;
`get-node` and `get-relationships` succeeded through JSON-RPC as well.
An imported copy of the existing index, with network access denied at the OS
level, returned `persisted_enriched` in 9.635 seconds (warm: 79–83 ms),
44959 relationships and a persisted description distinct from the structural
one. Automatic indexing and LLM generation were disabled. The original index,
installed binary and user config were not changed. These mixed-language MCP
measurements are separate from the Rust-only diagnostic table above; baseline
full MCP had timed out, so full-project graph parity is not claimed.

## Performance Metrics

### Typical Performance Characteristics

| Metric | Small Project (<1k files) | Medium Project (1k-10k files) | Large Project (10k+ files) |
|--------|---------------------------|-------------------------------|----------------------------|
| **Indexing Speed** | 500+ files/second | 200-400 files/second | 100-200 files/second |
| **Search Latency** | <50ms | <100ms | <200ms |
| **Memory Usage** | 50-100MB | 100-500MB | 500MB-2GB |
| **Storage Size** | 1-10MB | 10-100MB | 100MB-1GB |
| **Startup Time** | <1s | 1-3s | 3-10s |

### Factors Affecting Performance

1. **File Size**: Larger files take longer to parse and embed
2. **Language Complexity**: Complex languages (C++, TypeScript) slower than simple ones (JSON, Markdown)
3. **Embedding Model**: Local models faster than cloud APIs
4. **Hardware**: CPU, RAM, and storage speed impact performance
5. **Network**: Cloud embedding providers depend on network latency

## Recent Performance Improvements

### 2. Vector Index Optimization (v0.9.1+)
**New Feature**: Automatic vector index optimization based on dataset characteristics.

**How it works**:
- **Smart Index Creation**: Skips indexing for small datasets (< 1K rows) where brute force is faster
- **Optimal Parameters**: Automatically calculates partitions and search parameters from the row count
- **Growth-Aware**: Recreates indexes with better parameters as datasets grow
- **Consistent Distance**: Always uses Cosine distance for semantic similarity

**Performance Impact**:
- **Small datasets (< 1K rows)**: Brute force search (fastest)
- **Medium datasets (1K-100K rows)**: IVF_RQ (32x compression) or IVF_HNSW_SQ (4x), chosen by `index.quantization`
- **Large datasets (> 100K rows)**: Growth-aware optimization with enhanced recall
- **Search queries**: LanceDB auto-tunes IVF_HNSW_SQ search; `ef` is the only tunable parameter

**Technical Details**:
```rust
// Automatic optimization - no configuration required
let params = VectorOptimizer::calculate_index_params(row_count, vector_dimension, use_quantization);
if !params.should_create_index {
    return Ok(()); // brute force is faster below 1K rows
}
// Index type and parameters are carried by `params`
table.create_index(&[column_name], Index::IvfRq(builder)).execute().await?;
```

### 3. File Discovery Optimization (v0.8.1+)
**Issue Fixed**: The indexing process was extremely slow during the "Found X files..." counting phase, especially for large codebases.

**Root Cause**: The `NoindexWalker::has_noindex_files()` function was performing expensive full directory tree traversal to check for `.noindex` files on every indexing run.

**Solution**:
- Replaced O(n) tree traversal with O(1) targeted file system checks
- Added thread-safe caching using `OnceLock<RwLock<HashMap>>`
- Now checks only common directories: src, lib, tests, docs, target, etc.

**Performance Impact**:
- **Before**: Could take minutes for large codebases
- **After**: Instant startup, 100x-1000x improvement
- **Compatibility**: 100% backward compatible with all .gitignore and .noindex functionality

**Technical Details**:
```rust
// Fast targeted checks instead of full tree traversal
let common_paths = ["src", "lib", "tests", "docs", "target", "build", "dist"];
for subdir in &common_paths {
    if current_dir.join(subdir).join(".noindex").exists() {
        return true;
    }
}
```

## Optimization Strategies

### 1. Embedding Model Selection

#### For Speed (Local Models)
```bash
# FastEmbed - Fastest local option
# Model validation happens at startup - invalid models fail fast

octocode config \
  --code-embedding-model "fastembed:BAAI/bge-small-en-v1.5" \
  --text-embedding-model "fastembed:multilingual-e5-small"

# Optimized configuration
[embedding]
code_model = "fastembed:BAAI/bge-small-en-v1.5"  # 384 dim, fast
text_model = "fastembed:multilingual-e5-small"   # 384 dim, multilingual
```

#### For Maximum Quality (Cloud)
```bash
# High-quality cloud models (requires API keys and explicit configuration)

octocode config \
  --code-embedding-model "voyage:voyage-code-3" \
  --text-embedding-model "voyage:voyage-3.5-lite"

# Alternative: Jina models
[embedding]
code_model = "jina:jina-embeddings-v4"
text_model = "jina:jina-embeddings-v3"
```

**Note**: FastEmbed models require the `fastembed` feature flag. Cloud models (voyage, jina, google) work with `--no-default-features`.

#### Speed-Optimized Settings
```toml
[index]
chunk_size = 1000                # Smaller chunks process faster
embeddings_batch_size = 64       # Larger batches for efficiency
quantization = true              # RaBitQ 32x compression (faster than IVF_HNSW_SQ)
contextual_descriptions = false  # Disable for faster indexing

[search]
max_results = 20                 # Limit results for faster response
similarity_threshold = 0.65      # Higher threshold = fewer, more relevant results
```

#### Quality-Optimized Settings
```toml
[index]
chunk_size = 2000                # Larger chunks for better context
embeddings_batch_size = 32       # Smaller batches for stability
quantization = true              # Still uses compression, just more data
contextual_descriptions = true   # Enable AI-enriched chunks (requires LLM config)
contextual_model = "openrouter:openai/gpt-4o-mini"

[search]
max_results = 50                 # More comprehensive results
similarity_threshold = 0.3       # Lower threshold = more results
```

### 3. Vector Database Optimization

#### Automatic Optimization (Default)
Octocode automatically optimizes vector indexes based on your dataset size and characteristics. **No configuration required.**

**What happens automatically**:
- Small datasets (< 1K rows): Uses brute force search (fastest)
- Medium datasets (1K-100K rows): Creates optimized IVF_RQ or IVF_HNSW_SQ indexes
- Large datasets (> 100K rows): Recreates indexes at growth milestones
- Search parameters: Automatically calculated for best recall/latency balance

#### Manual Tuning (Advanced)
For specific use cases, you can influence performance through configuration:

```toml
[search]
max_results = 20                 # Limit results for faster response
similarity_threshold = 0.65      # Higher threshold = fewer, more relevant results

[index]
embeddings_batch_size = 16       # Batch size for embedding generation
flush_frequency = 2              # How often to flush to disk
```

**Note**: Vector index parameters (partitions, index type, etc.) are automatically calculated and cannot be manually configured.

### 4. Hardware Optimization

#### CPU Optimization
- **Multi-core**: Embedding generation uses multiple cores
- **CPU Type**: Modern CPUs with AVX2 support perform better
- **Recommended**: 4+ cores for optimal performance

#### Search Optimization
```toml
[search]
max_results = 30                 # Reduce for lower memory usage
```

#### Storage Optimization
- **SSD**: Significantly faster than HDD for database operations
- **NVMe**: Best performance for large codebases
- **Network Storage**: Avoid for database files

### 5. Network Optimization (Cloud Providers)

#### Reduce API Calls
```bash
# Use local models when possible
octocode config --code-embedding-model "fastembed:sentence-transformers/all-MiniLM-L6-v2"

# Batch operations
octocode clear && octocode index  # Index all at once vs incremental
```

#### API Rate Limiting & LLM Batching
```toml
[embedding]
# Adjust batch sizes for API limits
embeddings_batch_size = 16       # Smaller batches for cloud APIs

[graphrag.llm]
# Optimize optional persisted GraphRAG enrichment costs and rate limits
ai_batch_size = 8               # Process multiple files per AI call
max_batch_tokens = 16384        # Stay within model context limits
fallback_to_individual = true   # Reliability if batch processing fails
```

**LLM Cost Optimization**: The `ai_batch_size` parameter significantly reduces API costs by processing multiple files in a single request. With the default value of 8, you get ~87% fewer API calls compared to individual processing. Increase for more savings, decrease if hitting rate limits.

## Performance Monitoring

### Built-in Metrics

```bash
# Enable debug logging for performance metrics
RUST_LOG=debug octocode index

# Monitor indexing progress
octocode clear && octocode index 2>&1 | grep -i "processed"

# Check database size
ls -lh ~/.local/share/octocode/
```

### Custom Monitoring

```bash
#!/bin/bash
# Performance monitoring script

echo "=== Octocode Performance Report ==="
echo "Date: $(date)"
echo

# Database size
echo "Database size:"
du -sh ~/.local/share/octocode/

# Index timing
echo "Indexing performance:"
time (octocode clear && octocode index)

# Search timing
echo "Search performance:"
time octocode search "authentication" > /dev/null

# Memory usage
echo "Memory usage during search:"
/usr/bin/time -v octocode search "database" > /dev/null 2>&1 | grep "Maximum resident"
```

## Troubleshooting Performance Issues

### Slow Indexing

**Symptoms**: Indexing takes much longer than expected

**Solutions**:
1. **Reduce chunk size**: `chunk_size = 1000`
2. **Use faster embedding model**: Switch to FastEmbed
3. **Disable contextual descriptions**: `contextual_descriptions = false`
4. **Check disk space**: Ensure sufficient free space
5. **Monitor CPU usage**: Ensure no other heavy processes

```bash
# Quick fix for slow indexing
octocode config --code-embedding-model "fastembed:BAAI/bge-small-en-v1.5"
# Set in config.toml instead — there is no --contextual-descriptions flag:
#   [index]
#   contextual_descriptions = false
octocode clear && octocode index
```

### Slow Search

**Symptoms**: Search queries take several seconds

**Solutions**:
1. **Raise the similarity threshold**: `similarity_threshold = 0.8` (default is 0.65)
2. **Reduce max results**: `max_results = 10` (default is 20)
3. **Check database corruption**: `octocode clear && octocode index`
4. **Optimize query**: Use more specific search terms

```bash
# Quick fix for slow search
octocode config --max-results 10
octocode config --similarity-threshold 0.8
```

### High Memory Usage

**Symptoms**: Octocode uses excessive RAM

**Solutions**:
1. **Clear old data**: `octocode clear`
2. **Use smaller embedding models**: Switch to 384-dim models
3. **Limit search results**: `max_results = 10` (default is 20)

```bash
# Quick fix for memory issues
octocode config --max-results 10
octocode clear
octocode config --code-embedding-model "fastembed:sentence-transformers/all-MiniLM-L6-v2"
```

### API Rate Limiting

**Symptoms**: Errors from cloud embedding providers

**Solutions**:
1. **Reduce batch size**: `embeddings_batch_size = 8`
2. **Add delays**: Use local models for development
3. **Switch providers**: Try different cloud providers
4. **Use local models**: Switch to FastEmbed/SentenceTransformer

```bash
# Quick fix for rate limiting
octocode config --code-embedding-model "fastembed:sentence-transformers/all-MiniLM-L6-v2"
octocode config --text-embedding-model "fastembed:multilingual-e5-small"
```

## Benchmarking

### Standard Benchmark

```bash
#!/bin/bash
# Octocode benchmark script

PROJECT_PATH="/path/to/test/project"
cd "$PROJECT_PATH"

echo "=== Octocode Benchmark ==="
echo "Project: $PROJECT_PATH"
echo "Files: $(find . -type f -name "*.rs" -o -name "*.py" -o -name "*.js" | wc -l)"
echo

# Clear previous data
octocode clear

# Benchmark indexing
echo "Indexing benchmark:"
time octocode index

# Benchmark search
echo "Search benchmark (10 queries):"
queries=("authentication" "database" "API" "error handling" "configuration" "testing" "middleware" "validation" "logging" "security")

for query in "${queries[@]}"; do
    echo -n "Query '$query': "
    time octocode search "$query" > /dev/null
done

# Database size
echo "Final database size:"
du -sh ~/.local/share/octocode/
```

### Performance Comparison

<!-- TODO(doc-custodian): verify — these comparison figures are not produced by any in-repo benchmark; benchmark/RESULTS.md holds the measured retrieval matrix -->

| Configuration | Indexing (1000 files) | Search Latency | Memory Usage | Quality Score |
|:--------------|---------------------:|---------------:|-------------:|-------------:|
| **FastEmbed** | 45s | 50ms | 200MB | 7/10 |
| **Jina (cloud)** | 60s | 80ms | 250MB | 8.5/10 |
| **Voyage (cloud)** | 75s | 90ms | 300MB | 9/10 |
| **Google (cloud)** | 60s | 85ms | 280MB | 8.5/10 |

**Note**: Cloud model times include API latency.

## Best Practices

### Development Workflow

1. **Use local models** during development
2. **Enable cloud models** for production/final indexing
3. **Regular cleanup**: `octocode clear` periodically
4. **Monitor performance**: Track indexing and search times

### Production Deployment

1. **Optimize for your use case**: Speed vs quality tradeoff
2. **Monitor resource usage**: CPU, memory, storage
3. **Plan for scaling**: Consider hardware requirements
4. **Backup strategy**: Regular database backups

### Configuration Templates

#### Development (Speed Focus)
```toml
[embedding]
code_model = "fastembed:BAAI/bge-small-en-v1.5"
text_model = "fastembed:multilingual-e5-small"

[index]
chunk_size = 1000
contextual_descriptions = false

[search]
max_results = 20
similarity_threshold = 0.65
```

#### Production (Quality Focus)
```toml
[embedding]
code_model = "voyage:voyage-code-3"
text_model = "voyage:voyage-3.5-lite"

[index]
chunk_size = 2000
contextual_descriptions = true
contextual_model = "openrouter:openai/gpt-4o-mini"

[search]
max_results = 50
similarity_threshold = 0.3
```

#### Large Scale (Balanced)
```toml
[embedding]
code_model = "fastembed:BAAI/bge-small-en-v1.5"
text_model = "fastembed:multilingual-e5-small"

[index]
chunk_size = 1500
contextual_descriptions = true

[search]
max_results = 30
similarity_threshold = 0.5
```
