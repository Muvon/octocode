#!/usr/bin/env python3
"""Octocode retrieval benchmark matrix.

Indexes a PINNED corpus once with a local embedding model, then runs score.py
across retrieval-parameter variants (vector-only / hybrid / +graph-expansion /
+reranker) and writes a comparison table to RESULTS.md.

Why pinned: benchmark/code.csv ground truth is annotated against a specific
commit (see benchmark/README.md). Index that exact checkout or the line ranges
drift. Create it with:  git worktree add <CORPUS> <REF>

Usage:
    CORPUS=/home/box/bench_corpus OCTO_BIN=./target/release/octocode \\
        python3 benchmark/run_matrix.py

Env:
    CORPUS        (required) path to the pinned corpus checkout to index+search
    OCTO_BIN      octocode binary (default: octocode on PATH)
    BASE_CONFIG   base strict config to derive variants from (default: benchmark/config.toml)
    CSV / MODE    ground truth csv + search mode (default: benchmark/code.csv, code)
    OUT           results markdown (default: benchmark/RESULTS.md)
    CODE_MODEL / TEXT_MODEL   embedding models (default: local fastembed)
    SKIP_INDEX=1  reuse the existing index (skip clear+index)
    Rerank variants run automatically iff VOYAGE_API_KEY / COHERE_API_KEY /
    JINA_API_KEY is present in the env (keys are proxied through, never stored).
"""
import json
import os
import re
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).parent
OCTO_BIN = os.environ.get("OCTO_BIN", "octocode")
CORPUS = os.environ.get("CORPUS")
BASE_CONFIG = os.environ.get("BASE_CONFIG", str(HERE / "config.toml"))
CSV = os.environ.get("CSV", str(HERE / "code.csv"))
MODE = os.environ.get("MODE", "code")
OUT = os.environ.get("OUT", str(HERE / "RESULTS.md"))
CODE_MODEL = os.environ.get("CODE_MODEL", "fastembed:jinaai/jina-embeddings-v2-base-code")
TEXT_MODEL = os.environ.get("TEXT_MODEL", "fastembed:nomic-ai/nomic-embed-text-v1.5")
SKIP_INDEX = os.environ.get("SKIP_INDEX") == "1"
TMP = "/tmp"

if not CORPUS:
    sys.exit("CORPUS env is required (path to the pinned corpus checkout). See module docstring.")


def set_key(text, section, key, value):
    """Set `key = value` inside [section] of a TOML string (section-aware text edit).

    Replaces the existing key in that exact section, or inserts it if absent.
    Exact section match so [search] is never confused with [search.reranker].
    """
    if isinstance(value, bool):
        v = "true" if value else "false"
    elif isinstance(value, (int, float)):
        v = str(value)
    else:
        v = '"%s"' % value
    out, cur, in_target, done, seen = [], None, False, False, False
    for line in text.split("\n"):
        m = re.match(r"^\s*\[([^\]]+)\]\s*$", line)
        if m:
            if in_target and not done:
                out.append(f"{key} = {v}")
                done = True
            cur = m.group(1)
            in_target = cur == section
            seen = seen or in_target
            out.append(line)
            continue
        if in_target and not done and re.match(rf"^\s*{re.escape(key)}\s*=", line):
            out.append(f"{key} = {v}")
            done = True
            continue
        out.append(line)
    if in_target and not done:
        out.append(f"{key} = {v}")
        done = True
    if not seen:
        sys.exit(f"[{section}] not found in {BASE_CONFIG}")
    return "\n".join(out)


def run(cmd, **kw):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    return subprocess.run(cmd, **kw)


def octo_version():
    try:
        return subprocess.run([OCTO_BIN, "--version"], capture_output=True, text=True).stdout.strip()
    except Exception:
        return OCTO_BIN


# --- Local index profile: fastembed embeddings, no LLM, hybrid + graphrag on ---
base_text = Path(BASE_CONFIG).read_text()
local = base_text
local = set_key(local, "embedding", "code_model", CODE_MODEL)
local = set_key(local, "embedding", "text_model", TEXT_MODEL)
local = set_key(local, "index", "contextual_descriptions", False)  # no LLM keys needed
local = set_key(local, "graphrag", "enabled", True)
local = set_key(local, "graphrag", "use_llm", False)  # structural graph from AST, no LLM
local = set_key(local, "search", "similarity_threshold", 0.0)  # don't pre-filter; measure ranking
local = set_key(local, "search", "max_results", 20)
local = set_key(local, "search", "graph_expansion", False)
local = set_key(local, "search.hybrid", "enabled", True)
local = set_key(local, "search.reranker", "enabled", False)
local = set_key(local, "search.reranker", "final_top_k", 20)

index_cfg = f"{TMP}/octo_bench_index.toml"
Path(index_cfg).write_text(local)

# --- Index the pinned corpus once (search-time toggles reuse this index) ---
if not SKIP_INDEX:
    env = {**os.environ, "OCTOCODE_CONFIG_PATH": index_cfg}
    run([OCTO_BIN, "clear", "--mode", "all"], cwd=CORPUS, env=env)
    t0 = time.time()
    r = run([OCTO_BIN, "index"], cwd=CORPUS, env=env)
    print(f"index: {time.time() - t0:.0f}s rc={r.returncode}", flush=True)
    if r.returncode != 0:
        sys.exit("indexing failed")

# --- Variant matrix (search-time params; index is shared) ---
have_key = next((k for k in ("VOYAGE_API_KEY", "COHERE_API_KEY", "JINA_API_KEY") if os.environ.get(k)), None)
RERANK_MODEL = os.environ.get("RERANK_MODEL")  # e.g. fastembed:bge-reranker-base (local, no key)
ONLY = os.environ.get("ONLY")  # optional substring filter on variant name
rerank_ok = bool(have_key or RERANK_MODEL)
# Sweep RRF weights: at 0.7/0.3 the vector signal dominates and hybrid ≈ vector;
# tilting to keyword (0.3/0.7) surfaces the BM25/FTS lift for code. graph =
# GraphRAG file-level expansion (Win #1) — only changes results WITH a reranker
# that re-scores the enlarged candidate set. rerank variants need a key or RERANK_MODEL.
variants = [
    ("vector_only", dict(hybrid=False, vw=0.7, kw=0.3, graph=False, rerank=False)),
    ("hybrid_70_30", dict(hybrid=True, vw=0.7, kw=0.3, graph=False, rerank=False)),
    ("hybrid_30_70", dict(hybrid=True, vw=0.3, kw=0.7, graph=False, rerank=False)),
    ("hybrid_30_70+graph", dict(hybrid=True, vw=0.3, kw=0.7, graph=True, rerank=False)),
    ("hybrid_30_70+rerank", dict(hybrid=True, vw=0.3, kw=0.7, graph=False, rerank=True)),
    ("hybrid_30_70+graph+rerank", dict(hybrid=True, vw=0.3, kw=0.7, graph=True, rerank=True)),
]
variants = [(n, t) for (n, t) in variants
            if (not t["rerank"] or rerank_ok) and (not ONLY or ONLY in n)]

rows = []
for name, t in variants:
    cfg = set_key(local, "search.hybrid", "enabled", t["hybrid"])
    cfg = set_key(cfg, "search.hybrid", "default_vector_weight", t["vw"])
    cfg = set_key(cfg, "search.hybrid", "default_keyword_weight", t["kw"])
    cfg = set_key(cfg, "search", "graph_expansion", t["graph"])
    cfg = set_key(cfg, "search.reranker", "enabled", t["rerank"])
    if t["rerank"] and RERANK_MODEL:
        cfg = set_key(cfg, "search.reranker", "model", RERANK_MODEL)
    cfg_path = f"{TMP}/octo_bench_{name}.toml"
    Path(cfg_path).write_text(cfg)
    jout = f"{TMP}/octo_bench_{name}.json"
    env = {**os.environ, "OCTOCODE_CONFIG_PATH": cfg_path}
    print(f"\n=== variant: {name} {t} ===", flush=True)
    t0 = time.time()
    run(["python3", str(HERE / "score.py"), "--bin", OCTO_BIN, "--mode", MODE,
         "--csv", CSV, "--json-out", jout], cwd=CORPUS, env=env)
    if not Path(jout).exists():
        print(f"!! {name} produced no metrics; skipping", flush=True)
        continue
    m = json.load(open(jout))
    m["variant"], m["secs"] = name, round(time.time() - t0)
    rows.append(m)

# --- Merge with prior results (focused/partial runs accumulate) + write ---
ref = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=CORPUS,
                     capture_output=True, text=True).stdout.strip()
ts = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
rjson = HERE / "RESULTS.json"
merged = {}
if rjson.exists():
    try:
        for r in json.load(open(rjson)):
            merged[r["variant"]] = r
    except Exception:
        pass
for r in rows:
    merged[r["variant"]] = r
ORDER = ["vector_only", "hybrid_70_30", "hybrid_30_70", "hybrid_30_70+graph",
         "hybrid_30_70+rerank", "hybrid_30_70+graph+rerank"]
allrows = [merged[n] for n in ORDER if n in merged] + \
          [r for n, r in merged.items() if n not in ORDER]

rr = RERANK_MODEL or ("voyage:rerank-2.5" if have_key else None)
reranker_note = f"reranked variants use `{rr}`" if rr else "no reranker (set RERANK_MODEL or a key)"
hdr = ("| Variant | Hit@5 | Hit@10 | MRR | NDCG@10 | Recall@5 | Recall@10 | errs |\n"
       "|---|---|---|---|---|---|---|---|")
body = "\n".join(
    f"| {r['variant']} | {r['hit_at_5']:.3f} | {r['hit_at_10']:.3f} | {r['mrr']:.3f} | "
    f"{r['ndcg_at_10']:.3f} | {r['recall_at_5']:.3f} | {r['recall_at_10']:.3f} | {r['errors']} |"
    for r in allrows)
n = allrows[0]["queries"] if allrows else 0
md = f"""# Octocode Retrieval Benchmark — Results

- **Corpus:** octocode @ `{ref}` (pinned; ground truth annotated against this commit)
- **Queries:** {n} ({MODE} mode, `benchmark/{Path(CSV).name}`)
- **Embedding:** `{CODE_MODEL}` (local, no API key); {reranker_note}
- **Binary:** `{octo_version()}`
- **Generated:** {ts}
- **Reproduce:** `CORPUS={CORPUS} OCTO_BIN={OCTO_BIN} [RERANK_MODEL=fastembed:bge-reranker-base] python3 benchmark/run_matrix.py`

Metrics defined in [README.md](README.md). All variants share one index; only
search-time parameters change. Higher is better.

{hdr}
{body}

> `vector_only` = dense only. `hybrid_70_30` = default RRF weights (vector
> dominates → ≈ vector). `hybrid_30_70` = keyword-tilted RRF + code-tuned FTS
> tokenizer (Win #2). `+graph` = GraphRAG file-level expansion (Win #1), which
> only moves results once a reranker re-scores the enlarged set. `+rerank` =
> cross-encoder reranker over the candidate set.
"""
Path(OUT).write_text(md)
rjson.write_text(json.dumps(allrows, indent=2))
print("\n" + md)
print(f"wrote {OUT}")
