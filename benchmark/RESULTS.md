# Octocode Retrieval Benchmark — Results

- **Corpus:** octocode @ `b1771ba` (pinned; ground truth annotated against this commit)
- **Queries:** 127 (code mode, `benchmark/code.csv`)
- **Embedding:** `fastembed:jinaai/jina-embeddings-v2-base-code` (local, no API key); reranked variants use `fastembed:bge-reranker-base`
- **Binary:** `octocode 0.17.1`
- **Generated:** 2026-06-21 23:23 UTC
- **Reproduce:** `CORPUS=/home/box/bench_corpus OCTO_BIN=/home/box/work/muvon/octocode/target/release/octocode [RERANK_MODEL=fastembed:bge-reranker-base] python3 benchmark/run_matrix.py`

Metrics defined in [README.md](README.md). All variants share one index; only
search-time parameters change. Higher is better.

| Variant | Hit@5 | Hit@10 | MRR | NDCG@10 | Recall@5 | Recall@10 | errs |
|---|---|---|---|---|---|---|---|
| vector_only | 0.598 | 0.717 | 0.485 | 0.528 | 0.538 | 0.671 | 0 |
| hybrid_70_30 | 0.598 | 0.717 | 0.485 | 0.528 | 0.538 | 0.671 | 0 |
| hybrid_30_70 | 0.732 | 0.835 | 0.572 | 0.620 | 0.675 | 0.807 | 0 |
| hybrid_30_70+graph | 0.732 | 0.835 | 0.572 | 0.620 | 0.675 | 0.807 | 0 |
| hybrid_30_70+rerank | 0.598 | 0.630 | 0.499 | 0.524 | 0.555 | 0.594 | 0 |
| hybrid_30_70+graph+rerank | 0.598 | 0.630 | 0.499 | 0.524 | 0.555 | 0.594 | 0 |

> `vector_only` = dense only. `hybrid_70_30` = default RRF weights (vector
> dominates → ≈ vector). `hybrid_30_70` = keyword-tilted RRF + code-tuned FTS
> tokenizer (Win #2). `+graph` = GraphRAG file-level expansion (Win #1), which
> only moves results once a reranker re-scores the enlarged set. `+rerank` =
> cross-encoder reranker over the candidate set.
