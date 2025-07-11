# Architecture

## Core Components

Octocode is built with a modular architecture that separates concerns and enables efficient code analysis and search.

### 1. Indexer Engine
- **Multi-language code parser** using Tree-sitter
- **AST extraction** for semantic understanding
- **Symbol detection** (functions, classes, imports, exports)
- **Chunk-based processing** for large files
- **Safe symlink handling** - Prevents infinite recursion by disabling symlink following
- **Intelligent file discovery** with .gitignore and .noindex pattern support

### 2. Embedding System
- **Multiple providers**: FastEmbed (local), SentenceTransformer (local), Jina AI, Voyage AI, Google, OpenAI (cloud)
- **Dual embedding models**: Separate models for code and text/documentation
- **Batch processing** for efficient embedding generation
- **Provider auto-detection** from model string format
- **Input type support** for query vs document optimization

### 3. Vector Database
- **Lance columnar database** for fast similarity search
- **Efficient storage** (~10KB per file)
- **Fast retrieval** with similarity thresholds
- **Metadata indexing** for filtering
- **File metadata tracking** with modification time updates using LanceDB UpdateBuilder API

### 4. GraphRAG Builder
- **AI-powered relationship extraction** between files
- **Multi-language import resolver** - Maps import statements to actual file paths
- **Import/export dependency tracking** with intelligent path resolution
- **Module hierarchy analysis** with cross-language support
- **Intelligent file descriptions** using LLMs
- **Cached import resolution** for optimized repeated lookups
- **Language-specific import handling** (Rust, JavaScript/TypeScript, Python, Go, PHP, C/C++, Ruby, Bash)

### 5. Search Engine
- **Semantic similarity search** using vector embeddings
- **Keyword boosting** for exact matches
- **Multi-mode search** (code, docs, text, all)
- **Configurable similarity thresholds**

### 6. MCP Server
- **Model Context Protocol** server implementation
- **Intelligent file watching** with debouncing and ignore pattern support
- **Process management** to prevent concurrent indexing operations
- **Tool integration** for AI assistants
- **Debug mode** with enhanced logging and performance monitoring

### 7. Memory System
- **Persistent storage** for insights and context
- **Semantic memory search** using embeddings
- **Git integration** with automatic commit tagging
- **Memory types** (code, architecture, bug fixes, etc.)

### 8. Git Integration
- **Smart commit message generation** using AI
- **Staged changes analysis**
- **Code review assistant** with best practices checking
- **Multiple LLM support** via OpenRouter

## Knowledge Graph Structure

### Nodes
Each file/module in the codebase becomes a node with:
- **File path and metadata** (size, modification time, etc.)
- **AI-generated descriptions** explaining the file's purpose
- **Extracted symbols** (functions, classes, variables, etc.)
- **Import/export lists** for dependency tracking
- **Vector embeddings** for semantic search

### Relationships
Connections between nodes represent different types of relationships:
- **`imports`**: Direct import dependencies between files
- **`sibling_module`**: Files in the same directory
- **`parent_module`** / **`child_module`**: Hierarchical relationships

### Graph Operations
- **Search**: Find nodes by semantic query
- **Get Node**: Retrieve detailed information about a specific file
- **Get Relationships**: Find all connections for a node
- **Find Path**: Discover connection paths between two nodes
- **Overview**: Get high-level graph statistics

## Data Flow

1. **Indexing Phase**:
   ```
   Source Files → Tree-sitter Parser → Symbol Extraction → Embedding Generation → Vector Storage
                                                        ↓
   GraphRAG Analysis ← AI Description Generation ← Chunk Processing
   ```

2. **Search Phase**:
   ```
   Query → Embedding Generation → Vector Similarity Search → Result Ranking → Response
   ```

3. **Memory Phase**:
   ```
   Input → Semantic Processing → Vector Storage → Git Context Tagging → Persistence
   ```

## Supported Languages

| Language | Extensions | Parser Features |
|----------|------------|----------------|
| **Rust** | `.rs` | Full AST parsing, pub/use detection, module structure |
| **Python** | `.py` | Import/class/function extraction, docstring parsing |
| **JavaScript** | `.js`, `.jsx` | ES6 imports/exports, function declarations |
| **TypeScript** | `.ts`, `.tsx` | Type definitions, interface extraction, modules |
| **Go** | `.go` | Package/import analysis, function extraction |
| **PHP** | `.php` | Class/function extraction, namespace support |
| **C++** | `.cpp`, `.hpp`, `.h` | Include analysis, function/class extraction |
| **Ruby** | `.rb` | Class/module extraction, method definitions |
| **JSON** | `.json` | Structure analysis, key extraction |
| **Bash** | `.sh`, `.bash` | Function and variable extraction |
| **Markdown** | `.md` | Document section indexing, header extraction |

## Performance Characteristics

### Indexing Performance
- **Speed**: 100-500 files/second (varies by file size and complexity)
- **Memory**: ~50MB base + ~1KB per indexed file
- **Storage**: ~10KB per file in Lance database
- **Scalability**: Tested with codebases up to 100k+ files

### Search Performance
- **Latency**: <100ms for most queries
- **Throughput**: 1000+ queries/second
- **Memory**: Constant memory usage regardless of result size
- **Accuracy**: High semantic relevance with configurable thresholds

### Optimization Strategies
- **Chunking**: Configurable chunk sizes for different file types
- **Batch Processing**: Efficient embedding generation
- **Caching**: Vector embeddings cached for reuse
- **Incremental Updates**: Only index changed files
