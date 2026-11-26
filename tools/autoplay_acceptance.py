"""V52 development gate, using authenticated fixed-suite run receipts.
A pass is a numerical development gate, NOT proof of stable gameplay.
Run from repository root: python -m tools.autoplay_acceptance --run-dir ...
"""
from __future__ import annotations
import argparse
from collections import Counter
import json
from pathlib import Path
import statistics
import sys
from tools import autoplay_benchmark as b

SUITE = "dev"
TARGETS = {"wins_min": 36, "losses_max": 60, "early_losses_max": 24,
           "wallclock_max": 24, "median_tick_rate_min": 192.58}


def evaluate(rows, seeds, targets=TARGETS):
    """Fail closed on missing/foreign/invalid receipts or absent throughput."""
    expected = set(seeds)
    missing = sorted(expected - set(rows))
    extra = sorted(set(rows) - expected)
    invalid = sorted(s for s in expected & set(rows)
        if rows[s].get("outcome") not in b.OUTCOMES[:4]
        or rows[s].get("opening_matches") is not True
        or not isinstance(rows[s].get("tick"), int) or rows[s]["tick"] < 0)
    valid = [rows[s] for s in sorted(expected & set(rows)) if s not in invalid]
    counts = Counter(r["outcome"] for r in valid)
    rates = [b.observed_tick_rate(r) for r in valid]
    known = [v for v in rates if v is not None and v > 0]
    median = statistics.median(known) if known else None
    early = sum(r["outcome"] == "lost" and r["tick"] < 30000 for r in valid)
    checks = {
        "complete_valid_suite": not missing and not extra and not invalid and len(valid)==120,
        "wins": counts["won"] >= targets["wins_min"],
        "losses": counts["lost"] <= targets["losses_max"],
        "early_losses": early <= targets["early_losses_max"],
        "wallclock": counts["wallclock"] <= targets["wallclock_max"],
        "throughput": len(known)==120 and median is not None
                      and median+1e-8 >= targets["median_tick_rate_min"],
    }
    # Requests/reports are not actual shots, accepted orders, or casualties.
    def diagnostic(key):
        measured = [r for r in valid if r.get(key) is not None]
        ticks = sum(r["tick"] for r in measured)
        return {"observed_cases":len(measured),
                "total":sum(r[key] for r in measured) if measured else None,
                "per_10000_ticks":round(sum(r[key] for r in measured)*10000/ticks,4)
                    if ticks else None}
    return {"gate_pass":all(checks.values()), "checks":checks, "targets":targets,
            "valid":len(valid), "missing":missing, "extra":extra, "invalid":invalid,
            "outcomes":{k:counts[k] for k in b.OUTCOMES[:4]}, "early_losses":early,
            "throughput_samples":len(known), "median_tick_rate":median,
            "diagnostics":{k:diagnostic(k) for k in (
                "safety_veto_reports","siege_post_requests","front_guard_veto_reports",
                "assault_commits","assault_loss_aborts","front_deferred_events",
                "local_counter_requests","assault_removed_value","assault_target_damage",
                "budget_low_cash_samples","rest_proposal_samples")},
            "notes":["Numerical gate only. Compare same hardware, jobs, retail data and engine controls.",
                     "The V41/V48 references are historical; tick/s is whole-match throughput.",
                     "Verify compile, Rust tests, tactical fixtures and paired comparisons separately."]}


def main(argv=None):
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-dir",type=Path,required=True)
    parser.add_argument("--manifest",type=Path,default=b.DEFAULT_MANIFEST)
    parser.add_argument("--output",type=Path)
    args=parser.parse_args(argv)
    try:
        _, cases=b.load_manifest(args.manifest,SUITE)
        meta=json.loads((args.run_dir/"run.json").read_text(encoding="utf-8"))
        if meta["manifest_sha256"]!=b.sha(args.manifest):
            raise ValueError("run manifest mismatch")
        if meta["config"]!=b.CONFIG or meta["jobs"]!=16:
            raise ValueError("fixed dev configuration mismatch (requires 16 jobs)")
        _, run_cases=b.load_manifest(args.manifest,meta["suite"])
        seeds={c["seed"] for c in cases}
        if not seeds<={c["seed"] for c in run_cases}:
            raise ValueError("run suite does not contain dev suite")
        rows=b.load_receipts(args.run_dir,run_cases,meta["run_id"])
        report=evaluate({s:r for s,r in rows.items() if s in seeds},seeds)
        report["label"]=meta["label"]
        report["binary_sha256"]=meta["binary_sha256"]
        report["manifest_sha256"]=meta["manifest_sha256"]
        if args.output:
            b.atomic_json(args.output,report)
        print(json.dumps(report,ensure_ascii=False,indent=2,sort_keys=True))
        return 0 if report["gate_pass"] else 2
    except (OSError,ValueError,KeyError,TypeError) as exc:
        print(f"acceptance: {exc}",file=sys.stderr)
        return 2


if __name__=="__main__":
    raise SystemExit(main())
