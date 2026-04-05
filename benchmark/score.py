#!/usr/bin/env python3
"""Benchmark scoring for octocode semantic search retrieval quality.

Reads ground truth CSV, runs octocode search for each query,
and computes Hit@k, MRR, NDCG@10, and Recall@k metrics.

Usage:
    python3 benchmark/score.py [--threshold 0.5] [--max-results 20] [--verbose] [--mode code]
    python3 benchmark/score.py --mode docs --csv benchmark/docs.csv
"""

import argparse
import csv
import json
import math
import subprocess
import sys
import time
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
GROUND_TRUTH_CSV = SCRIPT_DIR / "code.csv"


def parse_ground_truth(row):
    """Parse a CSV row into query + list of (path, start, end, relevance)."""
    query = row["query"]
    results = []
    for key in ("result1", "result2", "result3"):
        val = row.get(key, "").strip()
        if not val:
            continue
        # format: src/path/file.rs:start-end:relevance
        parts = val.rsplit(":", 2)
        if len(parts) != 3:
            print(f"  WARN: bad format '{val}', skipping", file=sys.stderr)
            continue
        path_with_lines, line_range, relevance = parts[0], parts[1], parts[2]
        # path_with_lines = "src/path/file.rs" and line_range = "start-end"
        # but rsplit split from right, so we need to re-parse
        # Actually let's re-parse properly
        pass

    # Re-parse properly
    results = []
    for key in ("result1", "result2", "result3"):
        val = row.get(key, "").strip()
        if not val:
            continue
        # format: src/path/file.rs:41-61:2
        # Split from right: last ":" gives relevance, second-to-last gives line range
        last_colon = val.rfind(":")
        if last_colon == -1:
            continue
        relevance = int(val[last_colon + 1:])
        rest = val[:last_colon]
        second_colon = rest.rfind(":")
        if second_colon == -1:
            continue
        line_range = rest[second_colon + 1:]
        path = rest[:second_colon]
        start, end = line_range.split("-")
        results.append({
            "path": path,
            "start_line": int(start),
            "end_line": int(end),
            "relevance": relevance,
        })
    return query, results


def run_search(query, mode="code", threshold=None, max_results=20):
    """Run octocode search and return parsed JSON results."""
    cmd = ["octocode", "search", query, "--format", "json", "--mode", mode]
    if threshold is not None:
        cmd.extend(["--threshold", str(threshold)])
    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=60,
        )
        if result.returncode != 0:
            return None, result.stderr.strip()
        output = result.stdout.strip()
        if not output:
            return [], None
        data = json.loads(output)
        # --mode code returns array directly
        if isinstance(data, list):
            return data[:max_results], None
        # --mode all returns dict with code_blocks key
        if isinstance(data, dict) and "code_blocks" in data:
            return data["code_blocks"][:max_results], None
        return data[:max_results] if isinstance(data, list) else [], None
    except subprocess.TimeoutExpired:
        return None, "timeout"
    except json.JSONDecodeError as e:
        return None, f"json parse error: {e}"


def ranges_overlap(gt_start, gt_end, res_start, res_end):
    """Check if two line ranges overlap."""
    return gt_start <= res_end and res_start <= gt_end


def match_result(result, ground_truths):
    """Find best matching ground truth for a search result. Returns relevance or 0."""
    res_path = result.get("path", "")
    res_start = result.get("start_line", 0)
    res_end = result.get("end_line", 0)

    best_relevance = 0
    for gt in ground_truths:
        if gt["path"] == res_path and ranges_overlap(
            gt["start_line"], gt["end_line"], res_start, res_end
        ):
            best_relevance = max(best_relevance, gt["relevance"])
    return best_relevance


def compute_hit_at_k(results, ground_truths, k):
    """1 if any result in top-k matches any ground truth, else 0."""
    for result in results[:k]:
        if match_result(result, ground_truths) > 0:
            return 1
    return 0


def compute_mrr(results, ground_truths):
    """Reciprocal rank of first matching result."""
    for i, result in enumerate(results):
        if match_result(result, ground_truths) > 0:
            return 1.0 / (i + 1)
    return 0.0


def compute_ndcg_at_k(results, ground_truths, k):
    """NDCG@k using ground truth relevance scores."""
    # DCG of actual results
    dcg = 0.0
    for i, result in enumerate(results[:k]):
        rel = match_result(result, ground_truths)
        if rel > 0:
            dcg += rel / math.log2(i + 2)  # i+2 because i is 0-indexed

    # Ideal DCG: sort ground truth by relevance desc
    ideal_rels = sorted([gt["relevance"] for gt in ground_truths], reverse=True)
    idcg = 0.0
    for i, rel in enumerate(ideal_rels[:k]):
        idcg += rel / math.log2(i + 2)

    if idcg == 0:
        return 0.0
    return dcg / idcg


def compute_recall_at_k(results, ground_truths, k):
    """Fraction of ground truth entries found in top-k results."""
    if not ground_truths:
        return 0.0
    found = set()
    for result in results[:k]:
        res_path = result.get("path", "")
        res_start = result.get("start_line", 0)
        res_end = result.get("end_line", 0)
        for j, gt in enumerate(ground_truths):
            if gt["path"] == res_path and ranges_overlap(
                gt["start_line"], gt["end_line"], res_start, res_end
            ):
                found.add(j)
    return len(found) / len(ground_truths)


def main():
    parser = argparse.ArgumentParser(description="Benchmark octocode retrieval quality")
    parser.add_argument("--threshold", type=float, default=None, help="Similarity threshold")
    parser.add_argument("--max-results", type=int, default=20, help="Max results per query")
    parser.add_argument("--mode", default="code", help="Search mode (default: code)")
    parser.add_argument("--verbose", action="store_true", help="Show per-query details")
    parser.add_argument("--csv", default=str(GROUND_TRUTH_CSV), help="Ground truth CSV path")
    args = parser.parse_args()

    # Load ground truth
    csv_path = Path(args.csv)
    if not csv_path.exists():
        print(f"ERROR: Ground truth CSV not found: {csv_path}", file=sys.stderr)
        sys.exit(1)

    queries = []
    with open(csv_path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            query, ground_truths = parse_ground_truth(row)
            if query and ground_truths:
                queries.append((query, ground_truths))

    print(f"Loaded {len(queries)} queries from {csv_path}")
    print(f"Settings: mode={args.mode}, threshold={args.threshold}, max_results={args.max_results}")
    print(f"{'=' * 70}")

    # Run searches and collect metrics
    metrics = {
        "hit_at_5": [],
        "hit_at_10": [],
        "mrr": [],
        "ndcg_at_10": [],
        "recall_at_5": [],
        "recall_at_10": [],
    }
    failures = []
    errors = []

    for i, (query, ground_truths) in enumerate(queries):
        results, err = run_search(
            query, mode=args.mode, threshold=args.threshold, max_results=args.max_results
        )

        if results is None:
            errors.append((i + 1, query, err))
            if args.verbose:
                print(f"  [{i+1:2d}] ERROR: {query[:60]}... => {err}")
            continue

        h5 = compute_hit_at_k(results, ground_truths, 5)
        h10 = compute_hit_at_k(results, ground_truths, 10)
        mrr = compute_mrr(results, ground_truths)
        ndcg = compute_ndcg_at_k(results, ground_truths, 10)
        r5 = compute_recall_at_k(results, ground_truths, 5)
        r10 = compute_recall_at_k(results, ground_truths, 10)

        metrics["hit_at_5"].append(h5)
        metrics["hit_at_10"].append(h10)
        metrics["mrr"].append(mrr)
        metrics["ndcg_at_10"].append(ndcg)
        metrics["recall_at_5"].append(r5)
        metrics["recall_at_10"].append(r10)

        if h10 == 0:
            failures.append((i + 1, query, ground_truths, results[:3]))

        if args.verbose:
            status = "HIT" if h5 else ("hit@10" if h10 else "MISS")
            print(f"  [{i+1:2d}] {status:6s} MRR={mrr:.2f} NDCG={ndcg:.2f} R@10={r10:.2f}  {query[:55]}")

        # Small delay to avoid hammering
        time.sleep(0.05)

    # Summary
    print(f"\n{'=' * 70}")
    print("RESULTS SUMMARY")
    print(f"{'=' * 70}")

    n = len(metrics["hit_at_5"])
    if n == 0:
        print("No successful queries. Check octocode installation.")
        sys.exit(1)

    def avg(lst):
        return sum(lst) / len(lst) if lst else 0.0

    print(f"  Queries evaluated : {n} / {len(queries)}")
    print(f"  Errors            : {len(errors)}")
    print()
    print(f"  Hit@5             : {avg(metrics['hit_at_5']):.3f}  ({sum(metrics['hit_at_5'])}/{n})")
    print(f"  Hit@10            : {avg(metrics['hit_at_10']):.3f}  ({sum(metrics['hit_at_10'])}/{n})")
    print(f"  MRR               : {avg(metrics['mrr']):.3f}")
    print(f"  NDCG@10           : {avg(metrics['ndcg_at_10']):.3f}")
    print(f"  Recall@5          : {avg(metrics['recall_at_5']):.3f}")
    print(f"  Recall@10         : {avg(metrics['recall_at_10']):.3f}")

    # Show failures
    if failures:
        print(f"\n{'=' * 70}")
        print(f"MISSED QUERIES ({len(failures)} queries with no hit in top-10)")
        print(f"{'=' * 70}")
        for idx, query, gts, top3 in failures:
            print(f"\n  [{idx}] {query}")
            print(f"       Expected:")
            for gt in gts:
                print(f"         {gt['path']}:{gt['start_line']}-{gt['end_line']} (rel={gt['relevance']})")
            if top3:
                print(f"       Got (top 3):")
                for r in top3:
                    print(f"         {r.get('path', '?')}:{r.get('start_line', '?')}-{r.get('end_line', '?')}")
            else:
                print(f"       Got: (no results)")

    # Show errors
    if errors:
        print(f"\n{'=' * 70}")
        print(f"ERRORS ({len(errors)})")
        print(f"{'=' * 70}")
        for idx, query, err in errors:
            print(f"  [{idx}] {query[:60]}... => {err}")

    # Exit code: 0 if Hit@5 > 0.7, else 1
    hit5 = avg(metrics["hit_at_5"])
    if hit5 < 0.7:
        print(f"\nWARN: Hit@5 ({hit5:.3f}) is below 0.70 threshold")
        sys.exit(1)


if __name__ == "__main__":
    main()
