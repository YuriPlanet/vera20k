"""The autoplay check tool reads a match log and compares two runs; both
readings are what a run is judged on, so they are pinned here."""
from __future__ import annotations

import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest

from tools.autoplay_check import (
    ALL_JOBS,
    PROPOSABLE,
    capability_report,
    compare_histories,
    judge,
    losses,
    lost_arrow,
    opening_drift,
    parse_report,
)

# One unit's decision lines, as the policy prints them. A cell target contains a
# space — `Cell((172, 89))` — and a parser that assumed a non-space target
# dropped every job that named one, so harvest and defend both read zero while
# the log held ninety-six harvest lines.
UNITS = """[INFO] policy v2 unit: id=2671 AMCV job=Deploy target=None standing=0
[INFO] policy v2 unit: id=2771 CMIN job=Harvest target=Cell((172, 89)) standing=90
[INFO] policy v2 unit: id=2771 CMIN job=Harvest target=Cell((172, 89)) standing=0
[INFO] policy v2 unit: id=2772 CMIN job=Harvest target=Cell((166, 98)) standing=30
[INFO] policy v2 unit: id=2673 MTNK job=Defend target=Cell((178, 100)) standing=0
[INFO] policy v2 unit: id=2673 MTNK job=Assault target=Object(3985) standing=0
[INFO] policy v2 unit: id=2673 MTNK job=ReturnHome target=Cell((169, 91)) standing=0
[INFO] policy v2 unit: id=2661 GACNST job=none target=- standing=0
"""


class CapabilityTests(unittest.TestCase):
    def test_a_job_is_counted_by_the_units_that_took_it(self):
        """Two trucks on harvest is two, not ninety-six log lines."""
        report = parse_report("24301", UNITS, 1.0)
        self.assertEqual(report.jobs_used["Harvest"], 2, "2771 and 2772, not per line")
        self.assertEqual(report.jobs_used["Deploy"], 1)
        self.assertEqual(report.jobs_used["Defend"], 1)
        self.assertEqual(report.jobs_used["Assault"], 1)
        self.assertEqual(report.jobs_used["ReturnHome"], 1)

    def test_a_cell_target_with_a_space_is_read(self):
        """The regression: `Cell((172, 89))` must not swallow the job name."""
        report = parse_report("24301", UNITS, 1.0)
        self.assertIn("Harvest", report.jobs_used)
        self.assertIn("Defend", report.jobs_used)

    def test_none_is_not_a_capability(self):
        """A unit with no job is not a unit that exercised one."""
        report = parse_report("24301", UNITS, 1.0)
        self.assertNotIn("none", report.jobs_used)

    def test_the_table_names_every_declared_kind_and_the_unused_ones(self):
        """A ratio of zero is the finding: the code has it, the house never did."""
        report = parse_report("24301", UNITS, 1.0)
        self.assertEqual(sorted(report.capabilities), sorted(ALL_JOBS))
        self.assertEqual(sorted(report.unused_capabilities), sorted(
            kind for kind in ALL_JOBS
            if kind not in report.jobs_used
        ))
        self.assertIn("Capture", report.unused_capabilities)
        self.assertIn("Transport", report.unused_capabilities)
        self.assertNotIn("Harvest", report.unused_capabilities)

    def test_the_report_names_a_job_the_policy_should_always_issue(self):
        """Defend and Harvest are issued every match; zero means something gated them."""
        report = parse_report("24301", CURVE, 1.0)
        printed = io.StringIO()
        with contextlib.redirect_stdout(printed):
            capability_report([report])
        text = printed.getvalue()
        self.assertIn("capabilities over 1 match(es)", text)
        self.assertIn("never asked for", text)
        self.assertIn("a job the policy can issue was never issued", text)
        for kind in PROPOSABLE:
            self.assertIn(kind, text)

# A census curve: tick, credits, enemy credits, power, own, enemy, ready,
# rejected, not ready. Own climbs from 13 to 15 and dips twice; enemy only falls.
CURVE = """
     0     10000     10000       0     13     17      0         0         0
  1800      6901      7014     150     15     14      0         0         0
  3600      3758      3598     150     12     14      0         0         0
  5400       626       510     150     16     11      0         0         0
  7200       400         4     150     15      8      0         0         0
"""

LOG = """[INFO] auto play: building=35 infantry=60 vehicle=40 aircraft=6 ship=0 harv=6 \
ref=3 yard=2 fighters=100 threat=[0,0,0,0,0,0,0] able=[90,98,15,90,38,100,9] \
roster=[E1:40,CAOILD:13,MTNK:12] crates=1[(61, 163)] income=4200/min seen=3/3
[INFO] auto play: the build list answers Superweapon for GACSPH
[INFO] crate picked up at (61, 163) by house Player
[INFO] crate picked up at (96, 54) by house Player
[INFO] auto play: wall segment at (189, 105), 45 standing, closes 4 cells
autoplay-match: won at tick 22915
"""


class ExchangeTests(unittest.TestCase):
    def test_losses_count_every_fall_and_ignore_a_rebuild(self):
        """A count that drops is objects destroyed; a rise is not un-destroying."""
        rows = [
            ("0", "13", "17"),
            ("1", "15", "14"),
            ("2", "12", "14"),
            ("3", "16", "11"),
            ("4", "15", "8"),
        ]
        # Own: 13->15 rises, 15->12 falls 3, 12->16 rises, 16->15 falls 1.
        self.assertEqual(losses(rows, 1), 4)
        # Enemy: 17->14 falls 3, 14->14 flat, 14->11 falls 3, 11->8 falls 3.
        self.assertEqual(losses(rows, 2), 9)

    def test_the_report_carries_the_trade(self):
        report = parse_report("24301", CURVE, 10.0)
        self.assertEqual(report.own, 15)
        self.assertEqual(report.enemy, 8)
        self.assertEqual(report.own_lost, 4)
        self.assertEqual(report.enemy_lost, 9)
        self.assertEqual(report.exchange, "4:9")

    def test_the_trade_is_printed_and_never_judged(self):
        """A better trade must not turn a cell `worse` into `better` on its own."""
        before = row(own_lost=30, enemy_lost=30)
        after = row(own_lost=10, enemy_lost=80)
        verdict, worse, better = judge(before, after)
        self.assertEqual(verdict, "same", "the keep rule is still the object count")
        self.assertEqual(worse, [])
        self.assertEqual(better, [])
        self.assertEqual(lost_arrow(before, after), "30:30->10:80")


class ParseReportTests(unittest.TestCase):
    def test_the_result_is_the_line_that_says_so(self):
        report = parse_report("24301", LOG, 57.0)
        self.assertTrue(report.won)
        self.assertEqual(report.tick, 22915)
        self.assertEqual(report.result, "won at tick 22915")

    def test_the_evidence_lines_are_read(self):
        report = parse_report("24301", LOG, 57.0)
        self.assertEqual(report.yards, 2)
        self.assertEqual(report.walls, 45)
        self.assertEqual(report.plots, ["Superweapon:GACSPH"])
        self.assertEqual(
            report.crates,
            ["(61, 163) (Player)", "(96, 54) (Player)"],
        )

    def test_a_log_without_a_result_says_so(self):
        report = parse_report("24301", "nothing here", 1.0)
        self.assertFalse(report.won)
        self.assertEqual(report.result, "no result")


def row(**overrides) -> dict:
    """One history row, in the shape append_history writes."""
    base = {
        "when": "2026-11-02T08:00:00",
        "commit": "8328ef47d",
        "brain": "v2",
        "map": "Death.mmx",
        "country": "america",
        "seed": "24301",
        "opponents": 1,
        "ticks": 12000,
        "result": "timeout at tick 12000",
        "tick": 12000,
        "own": 35,
        "army": 7000,
        "enemy": 47,
        "unowned": 0,
        "stuck": 0,
        "yards": 1,
        "walls": 0,
        "start_credits": 10000,
        "start_own": 13,
        "start_units": 10,
        "start_tech": 10,
        "roster": "AMCV:1,E1:7,MTNK:5",
        "seconds": 30.0,
    }
    base.update(overrides)
    return base


def history_file(directory: Path, name: str, rows: list[dict], broken: int = 0) -> Path:
    """A history file, with `broken` lines that are not JSON."""
    path = directory / name
    lines = [json.dumps(item) for item in rows] + ["{not json"] * broken
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return path


class CompareTests(unittest.TestCase):
    def test_a_worse_cell_names_what_moved(self):
        """The keep rule as reasons: strength down, unowned up, a seed lost."""
        verdict, worse, better = judge(
            row(),
            row(army=6400, own=32, unowned=2, result="lost at 17680", tick=17680),
        )
        self.assertEqual(verdict, "worse")
        self.assertEqual(better, [])
        self.assertEqual(
            worse,
            ["army 7000->6400", "unowned 0->2", "lost at 17680 (was survived)"],
        )

    def test_a_house_that_survives_where_it_lost_is_better_and_an_opening_is_a_warning(self):
        """A better cell is named too, and a moved opening is not a delta."""
        before = row(result="lost at 9000", tick=9000)
        after = row(own=40, army=9000, start_credits=0)
        verdict, worse, better = judge(before, after)
        self.assertEqual(verdict, "better")
        self.assertEqual(worse, [])
        self.assertIn("army 7000->9000", better)
        self.assertIn("survived (was lost at 9000)", better)
        self.assertEqual(opening_drift(before, after), ["start_credits 10000->0"])

    def test_the_comparison_marks_what_got_worse_and_never_passes_over_a_missing_row(self):
        """Two files on disk, read end to end: the printed table is the verdict."""
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            baseline = history_file(directory, "before.jsonl", [
                row(),
                row(country="russia", seed="19", own=51, enemy=48),
            ])
            current = history_file(directory, "after.jsonl", [
                row(army=6400, unowned=2),
                row(country="yuri", seed="10", own=29, enemy=46),
            ], broken=1)
            printed = io.StringIO()
            with contextlib.redirect_stdout(printed):
                compare_histories(baseline, current)
        text = printed.getvalue()
        self.assertIn("1 unreadable line(s)", text)
        self.assertIn("army 7000->6400; unowned 0->2", text)
        self.assertIn("cells that got worse:", text)
        self.assertIn("missing from this run", text)
        self.assertIn("no baseline row", text)
        self.assertNotIn("no cell got worse", text)
        # One cell got worse, one is missing from each side, and the count adds
        # up: nothing is quietly dropped from the table.
        self.assertIn("cells: 3 compared, 0 same, 0 better, 1 worse, 2 missing", text)


if __name__ == "__main__":
    unittest.main()
