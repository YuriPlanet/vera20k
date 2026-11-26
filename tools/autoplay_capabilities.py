"""Read-only V56 capability report. Does not start games or change the gate."""
from __future__ import annotations

import argparse
from collections import Counter
import json
from pathlib import Path
import re
import sys

from tools import autoplay_benchmark as b
from tools import autoplay_recovery_report as recovery

SAMPLE = re.compile(
    r"auto play: v56_capabilities tick=(\d+) roster=\[([^\]\r\n]*)\]"
    r" hulls=(\d+) mobile_aa=(\d+) repair_mode=(\d+) garrisoned=(\d+)"
    r" capture_mission=(\d+) ready_attack=(\d+) available_idle=(\d+)\b")
METRICS = ("hulls", "mobile_aa", "repair_mode", "garrisoned",
           "capture_mission", "ready_attack", "available_idle")
BUDGET = re.compile(r"auto play: v56_quality_budget tick=(\d+) research_bank=(\d+) unit_bank=(\d+)\b")
EVENTS = ("mix_purchase", "quality_purchase", "air_raid_purchase",
          "rotation_extract", "rotation_order", "rotation_recovered",
          "rotation_rejoin", "spread_post", "collateral_dodge",
          "local_opportunity", "garrison_purchase")
TRACE_EVENTS = ("purchase_proposal", "engineer_veto", "garrison_proposal",
                "garrison_confirmed")
EVENT = re.compile(r"auto play: v56_(\w+) tick=\d+\b([^\r\n]*)")
TYPE = re.compile(r"(?:^|\s)type=(\S+)")
REASON = re.compile(r"(?:^|\s)reason=(\S+)")

def summarize_log(text: str) -> dict:
    # Keep chronological samples separate from requests, which need not
    # execute. A missing trace is not evidence that a type was never owned.
    samples = sorted(SAMPLE.findall(text), key=lambda m: int(m[0]))
    budget = BUDGET.findall(text)
    peaks: dict[str, int] = {}
    first: dict[str, int] = {}
    metric_peaks: dict[str, int] = {}
    first_metric: dict[str, int] = {}
    for sample in samples:
        tick = int(sample[0])
        for entry in sample[1].split(","):
            if not entry:
                continue
            name, sep, count = entry.rpartition(":")
            if not sep or not name or not count.isdigit():
                raise ValueError(f"malformed V56 roster entry: {entry!r}")
            count = int(count)
            if count:
                peaks[name] = max(peaks.get(name, 0), count)
                first.setdefault(name, tick)
        for name, count in zip(METRICS, map(int, sample[2:])):
            metric_peaks[name] = max(metric_peaks.get(name, 0), count)
            if count:
                first_metric.setdefault(name, tick)
    events = Counter()
    proposed_types = Counter()
    veto_reasons = Counter()
    for kind, fields in EVENT.findall(text):
        events[kind] += 1
        if kind == "purchase_proposal" and (m := TYPE.search(fields)):
            proposed_types[m[1]] += 1
        if kind == "engineer_veto" and (m := REASON.search(fields)):
            veto_reasons[m[1]] += 1
    version_seen = bool(samples or budget or any(events[k] for k in EVENTS + TRACE_EVENTS))
    return {
        "v56_instrumentation_seen": version_seen,
        "capability_samples": len(samples),
        "first_sample_tick": int(samples[0][0]) if samples else None,
        "last_sample_tick": int(samples[-1][0]) if samples else None,
        "observed_type_peak": dict(sorted(peaks.items())) if samples else None,
        "observed_type_first_tick": dict(sorted(first.items())) if samples else None,
        "sampled_metric_peaks": metric_peaks if samples else None,
        "sampled_metric_first_positive_tick": {
            name: first_metric.get(name) for name in METRICS} if samples else None,
        "budget_samples": len(budget),
        "peak_research_hold": max((int(m[1]) for m in budget), default=None),
        "peak_unit_hold": max((int(m[2]) for m in budget), default=None),
        "simultaneous_hold_samples": sum(int(m[1]) > 0 and int(m[2]) > 0 for m in budget)
                                     if budget else None,
        "event_counts": {k: events[k] if (events[k] or
                          (bool(samples) if k in TRACE_EVENTS else version_seen)) else None
                         for k in EVENTS + TRACE_EVENTS},
        "purchase_proposals_by_type": dict(sorted(proposed_types.items()))
                                     if samples or proposed_types else None,
        "engineer_veto_reasons": dict(sorted(veto_reasons.items()))
                                 if samples or veto_reasons else None,
        "asset_control": recovery.asset_counts(text),
    }

def report_run(path: Path, manifest: Path) -> dict:
    meta, rows, expected = recovery.read_run(path, manifest)
    cases = []
    type_cases = Counter()
    measured = 0
    for seed, row in sorted(rows.items()):
        if not recovery.valid(row):
            continue
        data = summarize_log((path / row["log"]).read_text(encoding="utf-8", errors="replace"))
        if data["capability_samples"]:
            measured += 1
            type_cases.update(data["observed_type_peak"].keys())
        cases.append(dict(seed=seed, map=row["map"], local=row["local"],
                          outcome=row["outcome"], tick=row["tick"], **data))
    return {"schema": 1, "label": meta["label"], "valid_cases": len(cases),
            "cases_with_capability_samples": measured,
            "cases_with_type_observed": dict(sorted(type_cases.items())) if measured else None,
            "missing_dev_seeds": sorted(expected - set(rows)),
            "invalid_seeds": sorted(seed for seed, row in rows.items() if not recovery.valid(row)),
            "cases": cases,
            "notes": [
                "Receipts and log SHA-256 validated by the original recovery report loader.",
                "Census presence can include captures or crates; not a production completion count.",
                "First observed tick is sampled, not the exact acquisition time.",
                "Ready attack is not damage; repair mode is not HP restored; capture mission is not capture.",
                "Rotation, purchase and dodge event counts are requests or policy observations, not native execution.",
                "A null means missing instrumentation, not zero. A sampled zero does not prove absence between samples.",
                "Research hold is the logged yard reserve, which may include critical economic commitments.",
                "This report does not modify or pass the formal gate."]}

def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--run-dir", type=Path, required=True)
    p.add_argument("--manifest", type=Path, default=b.DEFAULT_MANIFEST)
    p.add_argument("--output", type=Path)
    args = p.parse_args(argv)
    try:
        result = report_run(args.run_dir, args.manifest)
        if args.output:
            b.atomic_json(args.output, result)
        print(json.dumps(result, indent=2, ensure_ascii=False, sort_keys=True))
        return 0
    except (OSError, ValueError, TypeError, KeyError) as exc:
        print(f"capability report: {exc}", file=sys.stderr)
        return 2

if __name__ == "__main__":
    raise SystemExit(main())
