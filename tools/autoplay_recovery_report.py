"""V54 focused skirmish report. A partial run is never a passed suite."""
from __future__ import annotations
import argparse
from collections import Counter
import json
from pathlib import Path
import re
import statistics
import sys
from tools import autoplay_benchmark as b
from tools import autoplay_acceptance as acceptance

FOCUS = (20024, 20150, 20218, 20591, 20688, 20824)
VALID = frozenset(b.OUTCOMES[:4])
CONTROL_KEYS = ("config", "retail_inputs", "jobs", "platform", "cpu_count",
                "runner_sha256", "performance_env")
ASSET_MARK = re.compile(r"auto play: asset_control_metrics tick=\d+ schema=1\b")
ASSET_EVENT = re.compile(r"auto play: asset_control_confirmed tick=\d+ target=\d+"
                         r" type=\S+ from_nonhostile=(true|false) income=(true|false)\b")


def asset_counts(text):
    # Lack of a schema marker in an older log is missing instrumentation.
    if not ASSET_MARK.search(text):
        return None
    events = ASSET_EVENT.findall(text)
    return {"all": len(events),
            "from_nonhostile": sum(a == "true" for a, _ in events),
            "nonhostile_income": sum(a == c == "true" for a, c in events)}


def valid(row):
    return (row.get("outcome") in VALID and row.get("opening_matches") is True
            and type(row.get("tick")) is int and row["tick"] >= 0)


def read_run(path, manifest):
    meta = json.loads((path / "run.json").read_text(encoding="utf-8"))
    if meta["manifest_sha256"] != b.sha(manifest) or meta["config"] != b.CONFIG:
        raise ValueError("manifest/config mismatch")
    _, target = b.load_manifest(manifest, meta["suite"])
    _, dev = b.load_manifest(manifest, "dev")
    expected = {c["seed"] for c in dev}
    if not expected <= {c["seed"] for c in target}:
        # A separate smoke-only run remains useful as a partial report.
        _, smoke = b.load_manifest(manifest, "smoke")
        if not {c["seed"] for c in smoke} <= {c["seed"] for c in target}:
            raise ValueError("not a fixed skirmish smoke/dev run")
    rows = b.load_receipts(path, target, meta["run_id"])
    rows = {s: r for s, r in rows.items() if s in expected}
    return meta, rows, expected


def summarize(rows, expected):
    clean = {s: r for s, r in rows.items() if s in expected and valid(r)}
    rates = [b.observed_tick_rate(r) for r in clean.values()]
    rates = [r for r in rates if r is not None and r > 0]
    outcomes = Counter(r["outcome"] for r in clean.values())
    return {"complete_dev": set(clean) == expected and len(expected) == 120,
            "valid": len(clean), "missing": sorted(expected - set(rows)),
            "invalid": sorted(s for s in expected & rows.keys() if s not in clean),
            "outcomes": {k: outcomes[k] for k in b.OUTCOMES[:4]},
            "early_losses": sum(r["outcome"] == "lost" and r["tick"] < 30000
                                for r in clean.values()),
            "median_tick_rate": statistics.median(rates) if rates else None,
            "throughput_samples": len(rates),
            "formal_gate": acceptance.evaluate(rows, expected)["gate_pass"],
            "focus": [{"seed": s, "outcome": clean.get(s, {}).get("outcome"),
                       "tick": clean.get(s, {}).get("tick")} for s in FOCUS]}


def paired(before, after, expected):
    pairs = {s: (before[s], after[s]) for s in sorted(expected)
             if s in before and s in after and valid(before[s]) and valid(after[s])}
    transitions = Counter(a["outcome"] + "->" + c["outcome"] for a, c in pairs.values())
    return {"paired": len(pairs), "transitions": dict(sorted(transitions.items())),
            "gained_wins": [s for s,(a,c) in pairs.items()
                            if a["outcome"] != "won" and c["outcome"] == "won"],
            "lost_wins": [s for s,(a,c) in pairs.items()
                          if a["outcome"] == "won" and c["outcome"] != "won"]}


def observed_assets(path, rows):
    totals = Counter()
    measured = 0
    ticks = 0
    for row in rows.values():
        if not valid(row):
            continue
        count = asset_counts((path / row["log"]).read_text(encoding="utf-8", errors="replace"))
        if count is None:
            continue
        measured += 1
        ticks += row["tick"]
        totals.update(count)
    return {"observed_cases": measured, "observed_ticks": ticks,
            "counts": {k: totals[k] for k in ("all", "from_nonhostile", "nonhostile_income")}
                      if measured else None,
            "nonhostile_per_10000_ticks": round(totals["from_nonhostile"]*10000/ticks, 4)
                                       if ticks else None}


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--run-dir", type=Path, required=True)
    p.add_argument("--before", type=Path)
    p.add_argument("--manifest", type=Path, default=b.DEFAULT_MANIFEST)
    p.add_argument("--output", type=Path)
    args = p.parse_args(argv)
    try:
        meta, rows, expected = read_run(args.run_dir, args.manifest)
        result = summarize(rows, expected)
        result["label"] = meta["label"]
        result["jobs"] = meta["jobs"]
        result["formal_gate"] = result["formal_gate"] and meta["jobs"] == 16
        result["asset_control"] = observed_assets(args.run_dir, rows)
        result["notes"] = ["Acquisitions are observed ownership transitions for surveyed assets, not all captures.",
                           "Missing focus rows and uninstrumented assets are unknown, not zero.",
                           "A numerical gate is not proof of tactical correctness."]
        if args.before:
            old, before, _ = read_run(args.before, args.manifest)
            for key in CONTROL_KEYS:
                if key not in old or key not in meta or old[key] != meta[key]:
                    raise ValueError("comparison configuration mismatch: " + key)
            result["paired_against"] = old["label"]
            result["comparison"] = paired(before, rows, expected)
            result["different_source_head"] = old.get("source_head") != meta.get("source_head")
        if args.output:
            b.atomic_json(args.output, result)
        print(json.dumps(result, indent=2, ensure_ascii=False, sort_keys=True))
        return 0
    except (OSError, ValueError, TypeError, KeyError) as exc:
        print(f"recovery report: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
