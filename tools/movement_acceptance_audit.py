"""Audit the movement behaviours the acceptance goal names, per row.

The goal's finish condition is that each named behaviour either matches gamemd
or is "recorded with its native address, trigger and frequency". This checks the
second half so the claim is reproducible rather than asserted.

What it can and cannot tell you
-------------------------------
It reads text. It can tell you a row never states a trigger, or states one and
then admits the frequency is unestablished. It **cannot** tell you a stated
trigger is true, or that an address is the right address. A reviewer gamed an
earlier version by filling every row with "Trigger: unknown. Frequency:
unknown." and it passed, so:

- a row that says its trigger or frequency is unknown/unestablished now FAILS
  that axis rather than passing on the word;
- `NEEDS A RUN` is reported as **unestablished**, not as a frequency. It is the
  ledger's honest label for "we could not measure this", and counting it as a
  pass was the earlier version's worst flaw;
- every mapped row is checked **on its own**. The earlier version OR-ed a
  behaviour's rows together, so one row's trigger covered a sibling that had
  none - which is exactly how A7's missing trigger went unnoticed.

The behaviour list is the goal's, quoted below, not the ledger's criteria list:
the ledger also carries A10 Scale, which the goal does not name, so it is out of
scope here and audited by its own row's instruments instead. There is no artifact
in the repo that defines "the twelve", so this list is the audit's own premise -
change it and you change what passes.

It exits 1 today, and should: every row states all three axes, but thirteen state
one of them as unestablished. So this is a diagnostic, NOT yet a CI gate - wiring
it as one would fail the build for a condition the programme has not met. It
becomes a gate when that list is empty.
"""

import io
import pathlib
import re
import sys

LED = (pathlib.Path(__file__).resolve().parent.parent
       / "docs" / "plans" / "2026-09-15-movement-retail-acceptance.md")

# "order admission, routing, turning, column following, blocked recovery,
#  infantry stepping, crush, slopes and bridges, hover, jumpjet, wall attack
#  and wall-clock pace"
#
# Each behaviour lists the rows that must EACH carry the three facts. A row that
# only records matching behaviour, with no divergence, is listed in NO_DIVERGENCE.
BEHAVIOURS = [
    ("order admission",    ["| A1 Order admission"]),
    ("routing",            ["| I5 "]),
    ("turning",            ["| A3 Drive turning", "| I14 "]),
    ("column following",   ["| A4 Column following"]),
    ("blocked recovery",   ["| A5 Blocked recovery", "| I6 "]),
    ("infantry stepping",  ["| A6 Infantry stepping"]),
    ("crush",              ["| A7 Crush", "| I9c "]),
    ("slopes and bridges", ["| A8 Height"]),
    ("hover",              ["| A9 Hover motion", "| I4b ", "| I7 "]),
    ("jumpjet",            ["| I12a ", "| I12b "]),
    ("wall attack",        ["| I9b "]),
    ("wall-clock pace",    ["| A12 |", "| I13 "]),
]

ADDR = re.compile(r"0x00[0-9A-Fa-f]{6}")
TRIGGER = re.compile(r"[Tt]rigger")
FREQ = re.compile(r"[Ff]requency|\bSTRUCTURAL\b|\bSTOCK DATA\b|\bMEASURED\b")
# Words that mean "we have not established this" - present near a label, the
# axis is NOT recorded, however many times the label itself appears.
UNESTABLISHED = re.compile(
    r"NEEDS A RUN|\bUNCHECKED\b|\bunknown\b|\bunestablished\b|NOT established",
    re.I,
)


# A row may declare that it carries nothing a player can see - a coverage limit,
# dead code, or a documentation audit. It must SAY so in these words; the tool
# never infers it, so "nothing to record" cannot be confused with "not recorded".
NO_DIVERGENCE = re.compile(r"[Nn]o player-visible divergence")


def axis(row, label, pattern):
    """(recorded, note) for one axis of one row."""
    if not pattern.search(row):
        return False, "no %s stated" % label
    return True, ""


def main():
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
    lines = LED.read_text(encoding="utf-8").split("\n")

    print("%-19s %-26s %-5s %-5s %-5s" % ("behaviour", "row", "addr", "trig", "freq"))
    print("-" * 68)

    gaps = []
    caveats = []
    for name, keys in BEHAVIOURS:
        for key in keys:
            rows = [l for l in lines if l.startswith(key)]
            if len(rows) != 1:
                gaps.append((name, key.strip(), "matched %d rows" % len(rows)))
                print("%-19s %-26s %s" % (name, key.strip(), "NO UNIQUE ROW"))
                continue
            row = rows[0]
            if NO_DIVERGENCE.search(row):
                print("%-19s %-26s %s" % (name, key.strip(),
                                          "declares no player-visible divergence"))
                continue
            a, an = axis(row, "address", ADDR)
            t, tn = axis(row, "trigger", TRIGGER)
            f, fn = axis(row, "frequency", FREQ)
            print("%-19s %-26s %-5s %-5s %-5s"
                  % (name, key.strip(),
                     "yes" if a else "NO", "yes" if t else "NO", "yes" if f else "NO"))
            for ok, note in ((a, an), (t, tn), (f, fn)):
                if not ok:
                    gaps.append((name, key.strip(), note))
            if UNESTABLISHED.search(row):
                caveats.append((name, key.strip()))

    print()
    if gaps:
        print("MISSING - the row never states this axis at all:")
        for name, key, note in gaps:
            print("  %-19s %-24s %s" % (name, key, note))
        print()

    if caveats:
        print("UNESTABLISHED - the row states the axis and then says it is not")
        print("known (UNCHECKED / NEEDS A RUN / unknown). Honest, but the")
        print("condition asks for a frequency, and \"we could not measure it\"")
        print("is not one. These are the rows an instrument would close:")
        for name, key in caveats:
            print("  %-19s %s" % (name, key))
        print()

    if gaps or caveats:
        print("NOT SATISFIED: %d row(s) missing an axis, %d recording one as"
              % (len(gaps), len(caveats)))
        print("unestablished. Counting the latter as a pass is what made the")
        print("first version of this tool say yes when the answer was no.")
        return 1

    print("Every mapped row states an address, a trigger and a frequency, none")
    print("of them as unestablished. That is a statement about the TEXT, not")
    print("its truth: this tool cannot tell a correct address from a plausible")
    print("one.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
