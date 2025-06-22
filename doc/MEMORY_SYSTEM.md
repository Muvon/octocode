# Memory System Guide

Complete guide to Octocode's memory system for storing and retrieving project insights, decisions, and context.

## Overview

Octocode's memory system allows you to store important information about your project that persists across sessions. It uses semantic search with vector embeddings to help you find relevant memories based on context, not just keywords.

## Key Features

- **Semantic Search**: Find memories using natural language queries
- **Vector Embeddings**: Powered by the same embedding models as code search
- **Memory Types**: Organize memories by category (bugs, features, architecture, etc.)
- **Tag System**: Flexible tagging for better organization
- **File Relationships**: Link memories to specific files
- **Git Integration**: Automatically tag memories with commit information
- **Importance Scoring**: Prioritize memories by importance (0.0-1.0)
- **Memory Relationships**: Create connections between related memories

## Memory Operations

### Storing Information (memorize)

```bash
# Basic memory storage
octocode memory memorize \
  --title "JWT Authentication Bug Fix" \
  --content "Fixed race condition in token refresh logic by adding mutex lock"

# Comprehensive memory with all options
octocode memory memorize \
  --title "Database Connection Pool Optimization" \
  --content "Increased pool size from 10 to 50 connections and added connection health checks. This reduced database timeout errors by 90% under high load." \
  --memory-type performance \
  --importance 0.9 \
  --tags database,performance,optimization,production \
  --files src/database/pool.rs,src/config/database.rs
```

**Required parameters:**
- `--title`: Short descriptive title
- `--content`: Detailed information to store

**Optional parameters:**
- `--memory-type`: Category of memory (see Memory Types below)
- `--importance`: Score from 0.0 to 1.0 (higher = more important)
- `--tags`: Comma-separated tags for organization
- `--files`: Comma-separated file paths related to this memory

### Searching Memories (remember)

```bash
# Basic semantic search
octocode memory remember "JWT authentication issues"

# Multi-query search for comprehensive results
octocode memory remember "authentication" "security" "bugs"

# Filter by memory types
octocode memory remember "performance issues" --memory-types performance,bug_fix

# Filter by tags
octocode memory remember "database problems" --tags database,performance

# Filter by related files
octocode memory remember "auth system" --related-files src/auth.rs

# Limit results and set minimum relevance
octocode memory remember "optimization" --limit 10 --min-relevance 0.7

# JSON output for programmatic use
octocode memory remember "security" --format json
```

### Retrieving Specific Memory (get)

```bash
# Get memory by ID
octocode memory get abc123-def456-789

# JSON output
octocode memory get abc123-def456-789 --format json
```

### Updating Memories (update)

```bash
# Add tags to existing memory
octocode memory update abc123-def456-789 --add-tags critical,hotfix

# Remove tags
octocode memory update abc123-def456-789 --remove-tags outdated

# Update importance
octocode memory update abc123-def456-789 --importance 0.8

# Add related files
octocode memory update abc123-def456-789 --add-files src/new_module.rs

# Update title and content
octocode memory update abc123-def456-789 \
  --title "Updated Authentication System" \
  --content "Completely refactored auth system with OAuth2 support"
```

### Organizing Memories

```bash
# List memories by type
octocode memory by-type bug_fix
octocode memory by-type architecture

# List memories by tags
octocode memory by-tags security,authentication
octocode memory by-tags performance

# List memories for specific files
octocode memory for-files src/auth.rs
octocode memory for-files src/database/

# List recent memories
octocode memory recent --limit 20

# Get memory statistics
octocode memory stats
```

### Memory Relationships (relate)

```bash
# Create relationship between memories
octocode memory relate source-memory-id target-memory-id

# Find related memories
octocode memory get abc123 --include-related
```

### Cleanup and Maintenance

```bash
# Clean up old, low-importance memories
octocode memory cleanup

# Delete specific memory
octocode memory forget --memory-id abc123-def456-789

# Delete memories by query (careful!)
octocode memory forget --query "outdated documentation" --confirm

# Clear all memories (very careful!)
octocode memory clear-all --yes
```

## Memory Types

Organize your memories using these predefined types:

| Type | Description | Use Cases |
|------|-------------|-----------|
| `code` | Code-related insights and patterns | Code snippets, implementation notes, coding patterns |
| `architecture` | Architectural decisions and patterns | System design, component relationships, architectural choices |
| `bug_fix` | Bug reports and solutions | Bug descriptions, root causes, solutions, workarounds |
| `feature` | Feature implementations and decisions | Feature requirements, implementation details, design decisions |
| `documentation` | Documentation notes and updates | Documentation improvements, missing docs, content updates |
| `user_preference` | User-specific preferences | Personal workflow preferences, tool configurations |
| `decision` | Project decisions and rationale | Technical decisions, trade-offs, reasoning behind choices |
| `learning` | Insights and lessons learned | Lessons from failures, best practices discovered |
| `configuration` | Setup and configuration notes | Environment setup, deployment configurations |
| `testing` | Test strategies and results | Testing approaches, test results, quality metrics |
| `performance` | Performance optimizations and metrics | Performance improvements, benchmarks, bottlenecks |
| `security` | Security considerations and fixes | Security vulnerabilities, fixes, best practices |
| `insight` | General insights and observations | General observations, patterns, insights |

## Advanced Usage

### Memory Workflows

#### Bug Tracking Workflow

```bash
# 1. Document the bug
octocode memory memorize \
  --title "User Login Timeout Issue" \
  --content "Users experiencing 30-second timeouts during login. Appears to be related to database connection pool exhaustion during peak usage." \
  --memory-type bug_fix \
  --importance 0.9 \
  --tags login,timeout,database,critical \
  --files src/auth/login.rs,src/database/pool.rs

# 2. Document the investigation
octocode memory memorize \
  --title "Login Timeout Root Cause Analysis" \
  --content "Found that connection pool size of 10 is insufficient for peak load. Database queries are queuing and timing out after 30 seconds." \
  --memory-type bug_fix \
  --importance 0.8 \
  --tags login,timeout,database,analysis

# 3. Document the solution
octocode memory memorize \
  --title "Login Timeout Fix - Increased Pool Size" \
  --content "Increased database connection pool from 10 to 50 connections. Added connection health checks and monitoring. Issue resolved." \
  --memory-type bug_fix \
  --importance 0.7 \
  --tags login,timeout,database,fixed

# 4. Search related memories later
octocode memory remember "login timeout issues"
```

#### Architecture Documentation Workflow

```bash
# Document architectural decisions
octocode memory memorize \
  --title "Microservices vs Monolith Decision" \
  --content "Decided to use modular monolith architecture instead of microservices for initial MVP. Reasons: team size (3 developers), complexity overhead, deployment simplicity. Plan to extract services later if needed." \
  --memory-type architecture \
  --importance 0.9 \
  --tags architecture,decision,monolith,microservices

# Document component relationships
octocode memory memorize \
  --title "Authentication Service Integration" \
  --content "Auth service integrates with user service via direct function calls, with database via connection pool, and with external OAuth providers via HTTP client." \
  --memory-type architecture \
  --importance 0.7 \
  --tags architecture,auth,integration \
  --files src/auth/,src/user/,src/database/
```

#### Performance Optimization Workflow

```bash
# Document performance baseline
octocode memory memorize \
  --title "API Response Time Baseline" \
  --content "Current API response times: login 500ms, search 200ms, data fetch 1.2s. Target: all under 200ms." \
  --memory-type performance \
  --importance 0.8 \
  --tags performance,baseline,api

# Document optimization attempts
octocode memory memorize \
  --title "Database Query Optimization Results" \
  --content "Added indexes on user_id and created_at columns. Login time reduced from 500ms to 150ms. Search time unchanged. Data fetch still slow." \
  --memory-type performance \
  --importance 0.7 \
  --tags performance,database,optimization
```

### Integration with MCP Server

The memory system is available through the MCP server for AI assistants:

```bash
# Start MCP server
octocode mcp --path /path/to/project
```

**Available MCP tools:**
- `memorize` - Store new information
- `remember` - Search memories semantically
- `forget` - Remove memories

**Example with Claude:**
> "Remember that we fixed the authentication bug by adding proper error handling to the JWT validation logic"

Claude will use the `memorize` tool to store this information with appropriate tags and categorization.

### Memory Search Strategies

#### Semantic Search Tips

```bash
# Use descriptive phrases
octocode memory remember "database connection issues"

# Combine multiple concepts
octocode memory remember "authentication" "security" "vulnerabilities"

# Use technical terms
octocode memory remember "JWT token validation"

# Use problem descriptions
octocode memory remember "slow API response times"
```

#### Filtering Strategies

```bash
# Find all security-related memories
octocode memory by-tags security

# Find all bug fixes
octocode memory by-type bug_fix

# Find memories for specific components
octocode memory for-files src/auth/

# Combine filters for precise results
octocode memory remember "performance" \
  --memory-types performance,bug_fix \
  --tags database,optimization
```

## Configuration

### Memory System Settings

```bash
# Enable/disable memory system
octocode config --memory-enabled true

# Set maximum number of memories
octocode config --max-memories 10000

# View memory configuration
octocode config --show | grep memory
```

### Configuration File

```toml
[memory]
enabled = true
max_memories = 10000
cleanup_threshold = 0.3  # Cleanup memories below this importance
auto_cleanup = false     # Automatically cleanup old memories
```

## Best Practices

### Effective Memory Management

1. **Use Descriptive Titles**: Make titles searchable and clear
2. **Include Context**: Add enough detail for future understanding
3. **Tag Consistently**: Develop a consistent tagging strategy
4. **Set Importance**: Use importance scores to prioritize memories
5. **Link to Files**: Associate memories with relevant code files
6. **Regular Cleanup**: Periodically clean up outdated memories

### Memory Organization

```bash
# Good memory structure
octocode memory memorize \
  --title "Redis Cache Implementation for User Sessions" \
  --content "Implemented Redis-based session caching to reduce database load. Configuration in config/redis.rs. Reduced session lookup time from 50ms to 5ms. Key pattern: session:{user_id}. TTL set to 24 hours." \
  --memory-type performance \
  --importance 0.8 \
  --tags redis,cache,sessions,performance \
  --files src/session/cache.rs,config/redis.rs
```

### Search Optimization

```bash
# Use multiple related terms
octocode memory remember "cache" "redis" "performance"

# Be specific about problems
octocode memory remember "session timeout issues"

# Include context
octocode memory remember "user authentication JWT token validation"
```

## Integration Examples

### With Development Workflow

```bash
# During development
git add .
octocode commit
# AI generates commit message

# Store context about the change
octocode memory memorize \
  --title "Added User Profile Caching" \
  --content "Implemented profile caching to reduce API calls. Uses Redis with 1-hour TTL." \
  --memory-type feature \
  --tags cache,profile,api

# Later, search for related work
octocode memory remember "profile caching implementation"
```

### With Code Review

```bash
# After code review
octocode memory memorize \
  --title "Code Review Feedback - Error Handling" \
  --content "Reviewer suggested using Result<T> instead of panicking on errors. Updated all database operations to return proper error types." \
  --memory-type learning \
  --tags code-review,error-handling,best-practices
```

### With Debugging

```bash
# Document debugging process
octocode memory memorize \
  --title "Memory Leak in User Service Debug Process" \
  --content "Used valgrind to identify memory leak in user profile loading. Issue was in string allocation in profile_parser.rs line 45. Fixed by using string references instead of owned strings." \
  --memory-type bug_fix \
  --importance 0.9 \
  --tags memory-leak,debugging,valgrind \
  --files src/user/profile_parser.rs
```

## Troubleshooting

### Memory Search Not Finding Results

1. **Check similarity threshold**: Lower the threshold for broader results
2. **Try different search terms**: Use synonyms or related concepts
3. **Check memory types**: Ensure you're searching the right categories
4. **Use multi-query search**: Combine multiple related terms

### Memory Storage Issues

1. **Check disk space**: Ensure sufficient storage for the database
2. **Check permissions**: Ensure write access to the data directory
3. **Check memory limits**: Verify max_memories configuration
4. **Check embedding configuration**: Ensure embedding models are working

### Performance Issues

1. **Limit search results**: Use smaller limit values
2. **Clean up old memories**: Remove outdated or low-importance memories
3. **Optimize queries**: Use more specific search terms
4. **Check embedding performance**: Ensure embedding generation is fast

For more information, see:
- [Getting Started](GETTING_STARTED.md) - Basic memory usage
- [MCP Integration](MCP_INTEGRATION.md) - Using memory with AI assistants
- [Configuration](CONFIGURATION.md) - Memory system configuration
