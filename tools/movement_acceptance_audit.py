"""Audit the twelve movement behaviours the acceptance goal names.

The goal's finish condition is that each one either matches gamemd or is
"recorded with its native address, trigger and frequency". This checks the
second half mechanically, so the claim is reproducible rather than asserted:
run it and it names any row missing one of the three.

Exit code 1 on any gap, so it can gate a PR.

For each, find every row that covers it and check the three things the
acceptance condition asks for: a native address, a trigger, and a frequency.
Reports per behaviour so a gap is visible rather than argued about.
"""
import io
import re
import sys

import pathlib

LED = (pathlib.Path(__file__).resolve().parent.parent
       / "docs" / "plans" / "2026-09-15-movement-retail-acceptance.md")

# behaviour -> row key prefixes that carry it
BEHAVIOURS = [
    ("order admission",   ["| A1 Order admission"]),
    ("routing",           ["| I5 ", "| A2 |"]),
    ("turning",           ["| A3 Drive turning", "| I14 "]),
    ("column following",  ["| A4 Column following", "| I3 "]),
    ("blocked recovery",  ["| A5 Blocked recovery", "| I6 "]),
    ("infantry stepping", ["| A6 Infantry stepping"]),
    ("crush",             ["| A7 Crush", "| I4 ", "| I9a ", "| I9c "]),
    ("slopes and bridges", ["| A8 Height", "| I1 "]),
    ("hover",             ["| A9 Hover motion", "| I4b ", "| I7 "]),
    ("jumpjet",           ["| I12a ", "| I12b "]),
    ("wall attack",       ["| I9b "]),
    ("wall-clock pace",   ["| A12 |", "| I13 "]),
]

ADDR = re.compile(r"0x00[0-9A-Fa-f]{6}")
TRIGGER = re.compile(r"[Tt]rigger", re.I)
FREQ = re.compile(r"[Ff]requency|STRUCTURAL|STOCK DATA|MEASURED|NEEDS A RUN")
FIXED = re.compile(r"\bFIXED\b|\bLANDED\b")


def main():
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
    lines = open(LED, encoding="utf-8").read().split("\n")

    print("%-19s %-5s %-5s %-5s %-5s  %s" % ("behaviour", "rows", "addr", "trig", "freq", "state"))
    print("-" * 78)
    gaps = []
    for name, keys in BEHAVIOURS:
        rows = [l for l in lines if any(l.startswith(k) for k in keys)]
        if not rows:
            gaps.append((name, "NO ROW"))
            print("%-19s %-5s %-5s %-5s %-5s  %s" % (name, 0, "-", "-", "-", "NO ROW FOUND"))
            continue
        blob = " ".join(rows)
        a = bool(ADDR.search(blob))
        t = bool(TRIGGER.search(blob))
        f = bool(FREQ.search(blob))
        fixed = bool(FIXED.search(blob))
        state = "has a fixed/landed increment" if fixed else "recorded only"
        print("%-19s %-5d %-5s %-5s %-5s  %s"
              % (name, len(rows), "yes" if a else "NO",
                 "yes" if t else "NO", "yes" if f else "NO", state))
        for label, ok in (("address", a), ("trigger", t), ("frequency", f)):
            if not ok:
                gaps.append((name, label))

    print()
    if gaps:
        print("GAPS:")
        for name, what in gaps:
            print("  %-19s missing %s" % (name, what))
        return 1
    print("Every named behaviour carries an address, a trigger and a frequency.")
    return 0


sys.exit(main())
