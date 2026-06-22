#!/usr/bin/env python3
"""Loc-Bench evaluation harness for octocode issue->code localization.

Evaluates octocode semantic search on Loc-Bench V1 (czlll/Loc-Bench_V1):
560 real GitHub issues from popular Python repos. Queries are the original
issue texts; ground truth is the files/functions edited by the real merged
fix (curated `edit_functions` field). Nothing is self-annotated.

Per instance: clone repo (cached, blobless), checkout base_commit, run
`octocode index`, search with the issue text, score retrieved files against
ground truth.

Requirements on the machine running this:
    pip install datasets
    git, octocode in PATH, embedding API keys configured for octocode

Usage:
    python3 benchmark/locbench.py --limit 10 --verbose      # smoke test
    python3 benchmark/locbench.py                           # full 560
    python3 benchmark/locbench.py --category "Bug Report"
    python3 benchmark/locbench.py --out results.jsonl       # per-instance log
"""

import argparse
import json
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

DATASET = "czlll/Loc-Bench_V1"
SPLIT = "test"
DEFAULT_WORKDIR = Path(__file__).parent / ".locbench"
INDEX_TIMEOUT = 3600
SEARCH_TIMEOUT = 120


def load_instances(category=None, limit=None, instance_id=None):
    try:
        from datasets import load_dataset
    except ImportError:
        print("ERROR: pip install datasets", file=sys.stderr)
        sys.exit(1)

    ds = load_dataset(DATASET, split=SPLIT)
    instances = list(ds)
    if instance_id:
        instances = [i for i in instances if i["instance_id"] == instance_id]
    if category:
        instances = [i for i in instances if i["category"] == category]
    # Group by repo, oldest first, so differential indexing reuses work
    instances.sort(key=lambda i: (i["repo"], i["created_at"]))
    if limit:
        instances = instances[:limit]
    return instances


def ensure_repo(repo, workdir):
    """Clone (blobless, cached) and return repo directory."""
    repo_dir = workdir / "repos" / repo.replace("/", "__")
    if not (repo_dir / ".git").exists():
        repo_dir.parent.mkdir(parents=True, exist_ok=True)
        result = subprocess.run(
            ["git", "clone", "--filter=blob:none", f"https://github.com/{repo}.git", str(repo_dir)],
            capture_output=True, text=True, timeout=1800,
        )
        if result.returncode != 0:
            raise RuntimeError(f"clone failed: {result.stderr.strip()[:200]}")
    return repo_dir


def checkout(repo_dir, sha):
    result = subprocess.run(
        ["git", "-C", str(repo_dir), "checkout", "-f", sha],
        capture_output=True, text=True, timeout=600,
    )
    if result.returncode != 0:
        # Blobless clones may need an explicit fetch of the commit
        subprocess.run(
            ["git", "-C", str(repo_dir), "fetch", "origin", sha],
            capture_output=True, text=True, timeout=600,
        )
        result = subprocess.run(
            ["git", "-C", str(repo_dir), "checkout", "-f", sha],
            capture_output=True, text=True, timeout=600,
        )
        if result.returncode != 0:
            raise RuntimeError(f"checkout failed: {result.stderr.strip()[:200]}")


def octocode_index(repo_dir, octocode_bin):
    result = subprocess.run(
        [octocode_bin, "index"],
        cwd=str(repo_dir), capture_output=True, text=True, timeout=INDEX_TIMEOUT,
    )
    if result.returncode != 0:
        raise RuntimeError(f"index failed: {result.stderr.strip()[:200]}")


def octocode_search(repo_dir, query, octocode_bin, threshold=None, max_results=20):
    cmd = [octocode_bin, "search", query, "--format", "json", "--mode", "code"]
    if threshold is not None:
        cmd.extend(["--threshold", str(threshold)])
    result = subprocess.run(
        cmd, cwd=str(repo_dir), capture_output=True, text=True, timeout=SEARCH_TIMEOUT,
    )
    if result.returncode != 0:
        raise RuntimeError(f"search failed: {result.stderr.strip()[:200]}")
    output = result.stdout.strip()
    if not output:
        return []
    data = json.loads(output)
    if isinstance(data, dict) and "code_blocks" in data:
        data = data["code_blocks"]
    return data[:max_results] if isinstance(data, list) else []


def gt_files(instance):
    """Ground truth files from curated edit_functions ('path.py:Class.func')."""
    files = []
    for ef in instance["edit_functions"]:
        path = ef.split(":")[0]
        if path not in files:
            files.append(path)
    return files


HUNK_RE = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+\d+(?:,\d+)? @@")


def parse_patch_old_ranges(patch):
    """Old-side (base commit) line ranges per file from the gold patch."""
    ranges = defaultdict(list)
    current = None
    for line in patch.splitlines():
        if line.startswith("--- a/"):
            current = line[6:]
        elif line.startswith("--- /dev/null"):
            current = None  # added file: nothing to locate at base commit
        elif current:
            m = HUNK_RE.match(line)
            if m:
                start = int(m.group(1))
                count = int(m.group(2)) if m.group(2) else 1
                if count > 0:
                    ranges[current].append((start, start + count - 1))
    return ranges


def ranked_files(results):
    """Collapse chunk results to ordered unique file paths."""
    files = []
    for r in results:
        path = r.get("path", "")
        if path and path not in files:
            files.append(path)
    return files


def file_metrics(ranked, gt):
    gt_set = set(gt)
    hits = [1 if f in gt_set else 0 for f in ranked]
    first = next((i for i, h in enumerate(hits) if h), None)
    found_at = lambda k: set(f for f in ranked[:k] if f in gt_set)
    return {
        "hit@1": 1 if hits[:1] == [1] else 0,
        "hit@5": 1 if any(hits[:5]) else 0,
        "hit@10": 1 if any(hits[:10]) else 0,
        "acc@5": 1 if found_at(5) == gt_set else 0,
        "acc@10": 1 if found_at(10) == gt_set else 0,
        "recall@5": len(found_at(5)) / len(gt_set),
        "recall@10": len(found_at(10)) / len(gt_set),
        "mrr": 1.0 / (first + 1) if first is not None else 0.0,
    }


def hunk_hit_at_10(results, old_ranges):
    """Did any top-10 chunk overlap a gold patch hunk (base-commit lines)?"""
    for r in results[:10]:
        path = r.get("path", "")
        rs, re_ = r.get("start_line", 0), r.get("end_line", 0)
        for gs, ge in old_ranges.get(path, []):
            if gs <= re_ and rs <= ge:
                return 1
    return 0


def main():
    parser = argparse.ArgumentParser(description="Loc-Bench localization benchmark for octocode")
    parser.add_argument("--limit", type=int, default=None, help="Max instances to run")
    parser.add_argument("--category", default=None,
                        help="Bug Report | Feature Request | Performance Issue | Security Vulnerability")
    parser.add_argument("--instance-id", default=None, help="Run a single instance")
    parser.add_argument("--threshold", type=float, default=None, help="Similarity threshold")
    parser.add_argument("--max-results", type=int, default=20, help="Max search results")
    parser.add_argument("--query-chars", type=int, default=2000,
                        help="Truncate problem_statement to this many chars")
    parser.add_argument("--workdir", default=str(DEFAULT_WORKDIR), help="Clone cache directory")
    parser.add_argument("--octocode", default="octocode", help="Path to octocode binary")
    parser.add_argument("--out", default=None, help="Append per-instance results to this JSONL file")
    parser.add_argument("--verbose", action="store_true")
    args = parser.parse_args()

    workdir = Path(args.workdir)
    instances = load_instances(args.category, args.limit, args.instance_id)
    print(f"Loaded {len(instances)} instances from {DATASET} ({SPLIT})")
    print(f"Settings: threshold={args.threshold}, max_results={args.max_results}, "
          f"query_chars={args.query_chars}, workdir={workdir}")
    print("=" * 70)

    metric_keys = ["hit@1", "hit@5", "hit@10", "acc@5", "acc@10", "recall@5", "recall@10", "mrr"]
    totals = defaultdict(list)
    by_category = defaultdict(lambda: defaultdict(list))
    hunk_hits = []
    errors = []
    out_f = open(args.out, "a") if args.out else None

    for n, inst in enumerate(instances, 1):
        iid, repo, cat = inst["instance_id"], inst["repo"], inst["category"]
        try:
            repo_dir = ensure_repo(repo, workdir)
            checkout(repo_dir, inst["base_commit"])
            octocode_index(repo_dir, args.octocode)
            query = inst["problem_statement"][:args.query_chars]
            results = octocode_search(
                repo_dir, query, args.octocode,
                threshold=args.threshold, max_results=args.max_results,
            )
        except (RuntimeError, subprocess.TimeoutExpired, json.JSONDecodeError) as e:
            errors.append((iid, str(e)[:120]))
            if args.verbose:
                print(f"  [{n:3d}] ERROR  {iid}: {str(e)[:80]}")
            continue

        gt = gt_files(inst)
        m = file_metrics(ranked_files(results), gt)
        hh = hunk_hit_at_10(results, parse_patch_old_ranges(inst["patch"]))
        hunk_hits.append(hh)
        for k in metric_keys:
            totals[k].append(m[k])
            by_category[cat][k].append(m[k])

        if out_f:
            out_f.write(json.dumps({
                "instance_id": iid, "repo": repo, "category": cat,
                "gt_files": gt, "top_files": ranked_files(results)[:10],
                **m, "hunk_hit@10": hh,
            }) + "\n")
            out_f.flush()

        if args.verbose:
            status = "HIT" if m["hit@5"] else ("hit@10" if m["hit@10"] else "MISS")
            print(f"  [{n:3d}] {status:6s} MRR={m['mrr']:.2f} R@10={m['recall@10']:.2f} "
                  f"hunk={hh}  {iid}")

    if out_f:
        out_f.close()

    n_ok = len(totals["mrr"])
    print(f"\n{'=' * 70}\nRESULTS SUMMARY\n{'=' * 70}")
    if n_ok == 0:
        print("No successful instances.")
        sys.exit(1)

    avg = lambda lst: sum(lst) / len(lst) if lst else 0.0
    print(f"  Instances evaluated : {n_ok} / {len(instances)}  (errors: {len(errors)})\n")
    for k in metric_keys:
        print(f"  file {k:10s} : {avg(totals[k]):.3f}")
    print(f"  hunk hit@10     : {avg(hunk_hits):.3f}")

    print(f"\n  Per category (file hit@10 / acc@10 / MRR):")
    for cat in sorted(by_category):
        c = by_category[cat]
        print(f"    {cat:25s} n={len(c['mrr']):3d}  "
              f"{avg(c['hit@10']):.3f} / {avg(c['acc@10']):.3f} / {avg(c['mrr']):.3f}")

    if errors:
        print(f"\nERRORS ({len(errors)})")
        for iid, err in errors:
            print(f"  {iid}: {err}")


if __name__ == "__main__":
    main()
