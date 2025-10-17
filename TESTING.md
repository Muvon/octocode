# Testing Documentation for Octocode Timeout Fix

## Overview

This document provides comprehensive testing guidance for the HTTP client timeout bug fixes in octocode.

## Bug Summary

Two critical bugs caused infinite hangs:
1. **GraphRAG LLM calls** (`src/indexer/graphrag/builder.rs:74`)
2. **HuggingFace downloads** (`src/embedding/provider/huggingface.rs:460`)

Both used `Client::new()` instead of the builder pattern, preventing timeouts from being applied.

## Test Categories

### 1. Unit Tests

Run the complete test suite:

```bash
cd /Users/briansquires/octocode
cargo test --all-features
```

Expected: All tests pass with no regressions.

### 2. Integration Tests

#### Test A: GraphRAG Timeout Behavior

**Configuration:**
```toml
# octocode.toml
[graphrag]
enabled = true
use_llm = true

[graphrag.llm]
batch_timeout_seconds = 120  # 2 minutes
ai_batch_size = 10
description_model = "openai/gpt-4.1-mini"
relationship_model = "google/gemini-2.0-flash-001"
```

**Test Steps:**
```bash
# Index a non-trivial codebase
cd /path/to/test/codebase
octocode index 2>&1 | tee /tmp/octocode_test.log

# Monitor for timeout
tail -f /tmp/octocode_test.log | grep -E "(relationship|timeout|error)"
```

**Expected Results:**
- File indexing completes: `✓ Indexing complete! N of N files processed`
- GraphRAG blocks created: `GraphRAG: N blocks`
- Relationship extraction either:
  - Completes successfully, OR
  - Times out after 120s with error message: `GraphRAG::LLM timeout processing AI batch`

**Before Fix:** Process hangs indefinitely at "AI analyzing X files for architectural relationships"

**After Fix:** Process times out gracefully after 120 seconds

#### Test B: HuggingFace Download Timeout

**Configuration:**
```toml
[embedding]
provider = "huggingface"
model = "some-model-name"
```

**Test Steps:**
```bash
# Trigger HuggingFace model download
octocode index --force-reindex

# Simulate slow network (optional)
# sudo tc qdisc add dev eth0 root netem delay 5000ms
```

**Expected Results:**
- Model download either completes or times out after 30 seconds
- No infinite hangs

#### Test C: Large Batch Processing

**Purpose:** Verify timeout works with large file counts

**Configuration:**
```toml
[graphrag.llm]
batch_timeout_seconds = 60  # Shorter timeout for testing
ai_batch_size = 100  # Large batch
```

**Test on codebase with 100+ files:**
```bash
cd /path/to/large/codebase
time octocode index
```

**Expected:**
- Relationship extraction times out after ~60 seconds
- Process doesn't hang indefinitely

### 3. Regression Tests

#### Verify Other Client::new() Uses

Three files use `Client::new()` but apply request-level timeouts correctly:

```bash
cd /Users/briansquires/octocode
grep -n "Client::new()" src/commands/*.rs
```

**Files to verify:**
1. `src/commands/release.rs:552` - Uses `.timeout()` on request (line 586)
2. `src/commands/review.rs:520` - Uses `.timeout()` on request (line 598)
3. `src/commands/commit.rs:552` - Uses `.timeout()` on request (line 586)

**Test:** Run commands to ensure they still work:
```bash
# These commands should work normally
octocode review --help
octocode commit --help
octocode release --help
```

### 4. Error Message Tests

#### Test Improved Error Context

**Scenario 1:** HTTP client build failure (GraphRAG)

Simulate by injecting an error condition, then verify error message includes:
```
Failed to create HTTP client for LLM API calls
```

**Scenario 2:** HTTP client build failure (HuggingFace)

Verify error message includes:
```
Failed to create HTTP client for HuggingFace downloads
```

### 5. Performance Tests

#### Test: No Performance Regression

**Baseline:** Measure indexing time on rust-daq codebase before fix

**After Fix:** Measure indexing time with same configuration

**Expected:** No significant difference in successful indexing operations

```bash
# Benchmark
time octocode index --force-reindex
```

## Real-World Test Results

### rust-daq Codebase (113 files)

**Configuration:**
```toml
batch_timeout_seconds = 120
ai_batch_size = 10
```

**Results:**
- File indexing: ✅ 113/113 files (100%)
- GraphRAG blocks: ✅ 896 blocks created
- Relationship extraction: ✅ Timeout after 120s (expected)
- Error message: `GraphRAG::LLM timeout processing AI batch (72 files)`

**Before Fix:** Infinite hang
**After Fix:** Graceful timeout

## Code Quality Tests

### Clippy

```bash
cargo clippy --all-targets --all-features
```

**Expected:** Zero warnings

**Result:** ✅ Clean

### Format Check

```bash
cargo fmt --check
```

**Expected:** No formatting issues

### Documentation Tests

```bash
cargo test --doc
```

**Expected:** All documentation examples pass

## Continuous Integration

### GitHub Actions Workflow

Recommended CI checks:

```yaml
name: Timeout Fix Validation
on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable

      - name: Run tests
        run: cargo test --all-features

      - name: Run clippy
        run: cargo clippy --all-targets --all-features -- -D warnings

      - name: Integration test
        run: |
          cargo build --release
          ./target/release/octocode index --help
```

## Manual Testing Checklist

- [ ] Unit tests pass (`cargo test`)
- [ ] Clippy passes with zero warnings
- [ ] Integration test on small codebase (<50 files)
- [ ] Integration test on medium codebase (50-200 files)
- [ ] Timeout triggers correctly with short timeout (30s)
- [ ] Timeout triggers correctly with long timeout (300s)
- [ ] Error messages are clear and helpful
- [ ] No infinite hangs observed
- [ ] Existing commands still work (review, commit, release)
- [ ] No performance regression

## Troubleshooting

### Test Failures

**Issue:** Tests hang during execution
**Fix:** Ensure all test fixtures use the builder pattern with timeouts

**Issue:** Integration tests fail with timeout errors
**Fix:** Increase `batch_timeout_seconds` in test configuration

**Issue:** HuggingFace tests fail
**Fix:** Verify network connectivity and model availability

### Environment Setup

```bash
# Install dependencies
brew install hdf5  # macOS
sudo apt-get install libhdf5-dev  # Linux

# Configure test environment
export OCTOCODE_LOG=debug
export RUST_BACKTRACE=1
```

## Test Coverage

Target coverage for timeout-related code:
- HTTP client initialization: 100%
- Timeout configuration loading: 100%
- Error handling paths: 100%
- Integration scenarios: 80%+

## Version Information

- **octocode version:** 0.10.0
- **reqwest version:** Check Cargo.toml
- **Rust version:** 1.70+ required

## Related Documentation

- [OCTOCODE_PR_DESCRIPTION.md](./OCTOCODE_PR_DESCRIPTION.md) - PR details
- [octocode_timeout_analysis.md](../octocode_timeout_analysis.md) - Technical analysis
- [CLAUDE.md](./CLAUDE.md) - Project overview
