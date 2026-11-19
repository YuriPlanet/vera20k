"""One command for the autoplay loop: build, test, and run the pinned matches.

Why this exists: the loop is three commands and a wait, and the wait is where
the time goes. A pinned match on the dev-profile binary takes about three
minutes; the same match on the fast profile takes about one, and several seeds
run side by side because the machine has cores to spare. This script runs the
whole loop and prints what the run said, so a change can be measured in one
step instead of three.

Usage:
    python -m tools.autoplay_check                       # the pinned seed
    python -m tools.autoplay_check --seeds 0x5EED,19,10  # three seeds at once
    python -m tools.autoplay_check --skip-tests          # matches only
    python -m tools.autoplay_check --ticks 12000         # a shorter look

    # one change per measurement: record a reference run, change the policy,
    # run again, and read what moved. Newest row per cell wins.
    python -m tools.autoplay_check --history target/autoplay-results/before.jsonl
    python -m tools.autoplay_check --history target/autoplay-results/after.jsonl \\
        --baseline target/autoplay-results/before.jsonl
    # with no matches to run, re-read two files already on disk:
    python -m tools.autoplay_check --compare-only \\
        --baseline target/autoplay-results/before.jsonl \\
        --history target/autoplay-results/after.jsonl

The retail install is read from RA2_DIR. Without it the build and the tests
still run and the matches are reported as skipped.
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
import json
import os
from datetime import datetime
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]

DEFAULT_SEEDS = ("0x5EED",)
DEFAULT_MAP = "Death.mmx"
DEFAULT_TICKS = 36_000
DEFAULT_HISTORY = ROOT / "target" / "autoplay-results" / "history.jsonl"
# The engine offers three; the spec's word "冷酷" has no fourth. Defaulting to
# Normal while the spec asked for something harder is how every measurement in
# this folder came to describe an easier game than the one being judged, so the
# difficulty is now a recorded field and a flag.
DEFAULT_DIFFICULTY = "normal"
# The game speed index a headless run uses. Not cosmetic: `speed_normalize`
# divides construction and infantry-action cadence by `speed + 1`, so a run at
# the default index is not the match a player at max speed sees.
# One. The game speed changes construction and infantry cadence through
# `speed_normalize`, and it is **not** a performance knob: measured on the 1v7
# lane, 24000 ticks, three seeds,
#
#     speed 1: own 77 / 33 / 19
#     speed 6: own 11 / 5 / 47
#
# which is the difference between a lane that holds and one that does not. An
# earlier revision of this file defaulted to 6 for the 20-40% wall-clock saving,
# on a 12000-tick 1v1 measurement that showed no difference at all — the wrong
# lane and the wrong length to notice it.
DEFAULT_SPEED = 1
# Matches to run at once when the caller does not say.
#
# One per core is far too many: a single match is itself multi-threaded and
# measures 4.26 cores of CPU per 1.0 core of wall clock, so oversubscribing only
# makes every process wait. Measured on 24 cells of 12000 ticks at speed 6 on a
# 32-core host (wall seconds, lower is better):
#
#     jobs   4     6     8    10    12    16    20    24
#     wall 183   134   121   102   107   107   109   122
#
# Ten is the knee and a quarter of the cores is a rule that lands on it for this
# host without hard-coding the number.
def default_jobs() -> int:
    return max(1, (os.cpu_count() or 4) // 3)

# The fields one run is judged by. `own` is the reading at a fixed tick; the
# result says whether the house was still standing when the ticks ran out.
JUDGED = ("own", "unowned", "stuck")
# The opening a run starts from. Two runs whose openings differ are not the
# same experiment, so a drift here is a warning, not a delta.
OPENING = ("start_credits", "start_units", "start_tech", "start_own", "roster")

RESULT = re.compile(r"(won|timeout|lost) at tick (\d+)")
CRATE = re.compile(r"crate picked up at (\(\d+, \d+\)) by house (\S+)")
BUILD_LIST = re.compile(r"the build list answers (\w+) for ([A-Z0-9]+)")
STANDING = re.compile(r"(\d+) standing")
# The census row the match prints every interval: tick, credits, enemy credits,
# power, own, enemy, ready, rejected, not-ready.
CENSUS = re.compile(r"^\s*(\d+)\s+(\d+)\s+(\d+)\s+(-?\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s*$", re.M)
UNOWNED = re.compile(r"unowned=(\d+)")
STUCK = re.compile(r"stuck=(\d+)")
# One unit's decision line: the policy prints what each unit was told to do.
# This is the only reading that can say whether a capability was ever used —
# a job kind that never appears in it was never asked for, whatever the code
# contains.
#
# The target is greedy up to `standing=` rather than `\S+`: a cell target prints
# as `Cell((172, 89))`, which contains a space, and a non-space field silently
# dropped every job that named one — harvest and defend both read zero while the
# log had ninety-six harvest lines in it.
UNIT_JOB = re.compile(
    r"policy v\d+ unit: id=(\d+) (\S+) job=(\w+) target=(.+?) standing=(\d+)"
)
# The kinds the policy can actually ask for. Anything outside this list is a
# declared job nobody proposes, and the capability audit says so rather than
# reporting it as zero uses of a thing that exists.
PROPOSABLE = ("Deploy", "Harvest", "Assault", "Defend", "ReturnHome")
ALL_JOBS = (
    "Deploy",
    "Harvest",
    "ReturnOre",
    "Repair",
    "Capture",
    "Garrison",
    "Breach",
    "Assault",
    "Defend",
    "Scout",
    "Transport",
    "Retreat",
    "ReturnHome",
)
# The policy's own strength reading: what the house has to fight with, and what
# it can see of the enemy, in hit points. An object count cannot tell thirty
# tanks from thirty rifles, and the enemy grows when nothing meets it.
ARMY = re.compile(r"army=(\d+)")
EPOINTS = re.compile(r"epoints=(\d+)")
# The opening a run starts from: without it two results are not comparable.
START = re.compile(
    r"start: map=(\S+) country=(\S+) credits=(-?\d+) unit_count=(-?\d+) "
    r"tech=(-?\d+) own=(\d+) enemies=(\d+) roster=\[([^\]]*)\]"
)


@dataclass
class MatchReport:
    """What one match said about itself."""

    seed: str
    result: str = "no result"
    tick: int = 0
    seconds: float = 0.0
    crates: list[str] = field(default_factory=list)
    plots: list[str] = field(default_factory=list)
    yards: int = 0
    walls: int = 0
    map: str = ""
    own: int = 0
    enemy: int = 0
    unowned: int = 0
    stuck: int = 0
    army: int = 0
    epoints: int = 0
    country: str = ""
    start_credits: int = 0
    start_own: int = 0
    start_units: int = 0
    start_tech: int = 0
    roster: str = ""
    # The exchange, read off the census curve rather than its last row: how many
    # objects each side lost over the match. Two runs that end on the same count
    # can still have traded very differently, and that is the reading a change to
    # unit composition has to move.
    own_lost: int = 0
    enemy_lost: int = 0
    # How many distinct units were ever given each job kind, and how many
    # decisions of that kind were logged. A capability the wish list asks for
    # and the policy never asks for shows up here as zero, which is the point:
    # "the code contains it" and "the house ever did it" are different claims,
    # and only the second one is a feature.
    jobs_used: dict[str, int] = field(default_factory=dict)

    @property
    def capabilities(self) -> dict[str, int]:
        """Every job the policy could propose, with its use count.

        A kind absent from the log reads zero rather than missing: the table is
        the answer to "what does this policy actually do", and a gap in it would
        read as "not measured" when the truth is "never happened".
        """
        return {kind: self.jobs_used.get(kind, 0) for kind in ALL_JOBS}

    @property
    def unused_capabilities(self) -> list[str]:
        """The kinds declared in the registry that no unit was ever given."""
        return [kind for kind, count in self.capabilities.items() if count == 0]

    @property
    def won(self) -> bool:
        return self.result.startswith("won")

    @property
    def exchange(self) -> str:
        """Objects traded, as `own_lost:enemy_lost`.

        A ratio of one means each side destroyed as much as it lost; below one is
        a favourable trade. Kept as text so a history row stays comparable
        without dividing by zero.
        """
        return f"{self.own_lost}:{self.enemy_lost}"


def losses(rows: list[tuple[str, ...]], column: int) -> int:
    """How far a census column fell, summed over the whole curve.

    A count that drops between two readings is that many objects destroyed. Rises
    are ignored on purpose: a house that rebuilds is not undoing the loss, and a
    net final-minus-start figure would hide a fight that cost both sides an army.
    """
    total = 0
    for previous, current in zip(rows, rows[1:]):
        drop = int(previous[column]) - int(current[column])
        if drop > 0:
            total += drop
    return total


def parse_report(seed: str, text: str, seconds: float) -> MatchReport:
    """Read one match log.

    The lines that matter are the ones a policy cannot show on the map: the
    result, the crates that were collected, the plot the build list chose, the
    number of yards the house ended with, and the walls that stand on the ring.
    """
    report = MatchReport(seed=seed, seconds=seconds)
    match = RESULT.search(text)
    if match:
        report.result = f"{match.group(1)} at tick {match.group(2)}"
        report.tick = int(match.group(2))
    report.crates = [f"{cell} ({house})" for cell, house in CRATE.findall(text)]
    report.plots = sorted({
        f"{item}:{name}"
        for item, name in BUILD_LIST.findall(text)
        if item == "Superweapon"
    })
    rows = CENSUS.findall(text)
    if rows:
        report.own = int(rows[-1][4])
        report.enemy = int(rows[-1][5])
        # The whole curve, not the last row: a change to what the house builds is
        # judged by what the two armies destroyed, and the final counts can be
        # equal after very different fights.
        report.own_lost = losses(rows, 4)
        report.enemy_lost = losses(rows, 5)
    # The last reading, not the worst: the starting force is unowned until the
    # first post exists, and a maximum reports that first frame forever.
    report.unowned = int(UNOWNED.findall(text)[-1]) if UNOWNED.findall(text) else 0
    readings = STUCK.findall(text)
    report.stuck = int(readings[-1]) if readings else 0
    army = ARMY.findall(text)
    report.army = int(army[-1]) if army else 0
    epoints = EPOINTS.findall(text)
    report.epoints = int(epoints[-1]) if epoints else 0
    report.yards = max((int(value) for value in re.findall(r"yard=(\d+)", text)), default=0)
    report.walls = max((int(value) for value in STANDING.findall(text)), default=0)

    # What the house was ever actually asked to do. Counted per distinct unit
    # rather than per log line: the unit line repeats every readout, so a raw
    # count would measure log verbosity instead of how many soldiers took the
    # job. `standing` is not involved — a unit that holds a job for a whole
    # match is still one unit that took it.
    seen_units: dict[str, set[str]] = {}
    for unit, _type, kind, _target, _standing in UNIT_JOB.findall(text):
        # `none` is the absence of a job, not one of them: counting it would put
        # a row in the capability table for doing nothing.
        if kind == "none":
            continue
        seen_units.setdefault(kind, set()).add(unit)
    report.jobs_used = {kind: len(units) for kind, units in seen_units.items()}
    return report


def commit_id() -> str:
    """The commit a run was built from, so a row can be traced back to code."""
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"], cwd=ROOT, text=True
        ).strip()
    except Exception:
        return "unknown"


def append_history(reports: list[MatchReport], args, path: Path) -> None:
    """Append one JSON line per match, so two runs can be compared later."""
    stamp = datetime.now().isoformat(timespec="seconds")
    commit = commit_id()
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        for report in reports:
            handle.write(json.dumps({
                "when": stamp,
                "commit": commit,
                "brain": args.brain,
                "map": report.map,
                "country": report.country,
                "seed": report.seed,
                "opponents": args.opponents,
                "difficulty": args.difficulty,
                "ticks": args.ticks,
                "result": report.result,
                "tick": report.tick,
                "own": report.own,
                "enemy": report.enemy,
                "own_lost": report.own_lost,
                "enemy_lost": report.enemy_lost,
                "unowned": report.unowned,
                "stuck": report.stuck,
                "army": report.army,
                "epoints": report.epoints,
                "yards": report.yards,
                "walls": report.walls,
                "start_credits": report.start_credits,
                "start_own": report.start_own,
                "start_units": report.start_units,
                "start_tech": report.start_tech,
                "roster": report.roster,
                "jobs": report.capabilities,
                "seconds": round(report.seconds, 1),
            }) + "\n")


def key_of(row: dict) -> tuple[str, str, str, str, int, int, str]:
    """Which experiment a row belongs to.

    Rows match on all of it: a run is only comparable to the same brain, map,
    faction, seed, length, opponent count and difficulty. Difficulty is in the
    key because it was missing for every run before this field existed, and a
    Normal match is not comparable to a Hard one.
    """
    return (
        str(row.get("brain", "")),
        str(row.get("map", "")),
        str(row.get("country", "")),
        str(row.get("seed", "")),
        int(row.get("ticks", 0)),
        int(row.get("opponents", 1)),
        str(row.get("difficulty", "")),
    )


def read_history(path: Path) -> tuple[dict[tuple, dict], int]:
    """The newest row per cell, and how many lines could not be read.

    A history file is append-only, so a cell appears once per run in it and the
    last reading is the current one. Unreadable lines are counted rather than
    passed over in silence: a comparison that lost rows has to say so.
    """
    rows: dict[tuple, dict] = {}
    unreadable = 0
    if not path.is_file():
        return rows, unreadable
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            unreadable += 1
            continue
        if isinstance(row, dict):
            rows[key_of(row)] = row
        else:
            unreadable += 1
    return rows, unreadable


def survived(row: dict) -> bool:
    """A house still standing when the ticks ran out, or one that won."""
    return str(row.get("result", "")).startswith(("won", "timeout"))


def judge(before: dict, after: dict) -> tuple[str, list[str], list[str]]:
    """What one cell did, and why.

    Four things decide, and the docstring used to name a fifth:

    - **army**, the house's fighting strength in hit points. Thirty tanks and
      thirty rifles are thirty objects and not the same army, so the object
      count is not the judge. A row written before `army` existed falls back to
      `own`, because comparing nothing is worse than comparing the old reading;
    - **unowned** must not rise: a unit nobody owns is a bug;
    - **stuck** must not rise: the spec's H2;
    - **survival**: no seed may be lost that survived, and a house that holds
      longer is better.

    Enemy count, walls and the trade are read as context: they move for reasons
    the policy does not own, so they are printed and never decided on.
    """
    worse: list[str] = []
    better: list[str] = []

    # Strength first, and `own` only when there is no strength reading to use.
    if "army" in before and "army" in after:
        field = "army"
    else:
        field = "own"
    was, now = int(before.get(field, 0)), int(after.get(field, 0))
    if now < was:
        worse.append(f"{field} {was}->{now}")
    elif now > was:
        better.append(f"{field} {was}->{now}")

    for field in ("unowned", "stuck"):
        was, now = int(before.get(field, 0)), int(after.get(field, 0))
        if now > was:
            worse.append(f"{field} {was}->{now}")
        elif now < was:
            better.append(f"{field} {was}->{now}")

    tick_before, tick_after = int(before.get("tick", 0)), int(after.get("tick", 0))
    lived_before, lived_after = survived(before), survived(after)
    if lived_before and not lived_after:
        worse.append(f"lost at {tick_after} (was survived)")
    elif not lived_before and lived_after:
        better.append(f"survived (was lost at {tick_before})")
    elif tick_after < tick_before:
        worse.append(f"lost earlier {tick_before}->{tick_after}")
    elif tick_after > tick_before:
        better.append(f"held longer {tick_before}->{tick_after}")

    # A win is the only reading that settles a cell outright.
    won_before, won_after = str(before.get("result", "")).startswith("won"), str(
        after.get("result", "")
    ).startswith("won")
    if won_after and not won_before:
        better.append("won (was not a win)")
    elif won_before and not won_after:
        worse.append("no longer a win")

    if worse:
        return "worse", worse, better
    if better:
        return "better", worse, better
    return "same", worse, better


def opening_drift(before: dict, after: dict) -> list[str]:
    """The opening fields that differ between two readings of one cell."""
    return [
        f"{field} {before.get(field)!r}->{after.get(field)!r}"
        for field in OPENING
        if before.get(field) != after.get(field)
    ]


def arrow(before: dict, after: dict, field: str) -> str:
    """One field, before and after."""
    return f"{before.get(field, 0)}->{after.get(field, 0)}"


def lost_arrow(before: dict, after: dict) -> str:
    """The trade, before and after, as `own:enemy` losses on each side.

    Printed but never judged: a composition change is expected to raise the
    enemy's losses before it raises the object count, and the keep rule is still
    the object count. What this column is for is telling a run that traded better
    from one that traded the same.
    """
    def pair(row: dict) -> str:
        return f"{row.get('own_lost', 0)}:{row.get('enemy_lost', 0)}"

    return f"{pair(before)}->{pair(after)}"


def capability_report(reports: list[MatchReport]) -> None:
    """What the house was ever asked to do, summed over every cell of a run.

    This is the acceptance reading the feature side needs and the object count
    cannot supply. The wish list asks for capture, transport, infiltration,
    superweapons, garrisons and walls; the keep rule judges hit points. A run can
    score well on every existing field while never once exercising a capability,
    and until this line existed there was no way to tell "we built it" from "the
    house did it".

    A kind declared in the registry that reads zero here is a feature that does
    not exist in practice, whatever the code says.
    """
    ran = max((report.tick for report in reports), default=0)
    print("", flush=True)
    print(f"capabilities over {len(reports)} match(es), {ran} ticks:", flush=True)
    total: dict[str, int] = {kind: 0 for kind in ALL_JOBS}
    for report in reports:
        for kind, count in report.capabilities.items():
            total[kind] += count
    width = max(len(kind) for kind in ALL_JOBS)
    for kind in ALL_JOBS:
        count = total[kind]
        mark = "" if count else "   <- never asked for"
        print(f"  {kind:<{width}}  {count:>4}{mark}", flush=True)
    proposable_zero = [kind for kind in PROPOSABLE if total[kind] == 0]
    if proposable_zero:
        # A kind the policy issues every match when the game allows it, and did
        # not issue here, is a defect in this run rather than a missing feature:
        # something gated it off.
        print(f"\n  a job the policy can issue was never issued: "
              f"{', '.join(proposable_zero)}", flush=True)


def compare_histories(baseline: Path, current: Path) -> None:
    """Print what moved between a reference run and this one, cell by cell."""
    before, before_unreadable = read_history(baseline)
    after, after_unreadable = read_history(current)
    print("", flush=True)
    print(f"baseline {baseline}: {len(before)} cells, {before_unreadable} unreadable line(s)",
          flush=True)
    print(f"current  {current}: {len(after)} cells, {after_unreadable} unreadable line(s)",
          flush=True)

    header = (
        f"{'map':<14}{'country':<10}{'seed':<7}{'ticks':<7}{'brain':<7}{'opp':<5}{'diff':<8}"
        f"{'own':<12}{'army':<12}{'enemy':<12}{'lost':<16}{'unowned':<10}{'stuck':<9}{'yards':<9}"
        f"{'walls':<9}verdict"
    )
    print("", flush=True)
    print(header, flush=True)
    print("-" * (len(header) + 24), flush=True)

    # What makes two readings the same experiment. Enemy count, walls and the
    # opening are in it: a run that moved one of those is not a replay.
    delta_fields = ("own", "enemy", "unowned", "stuck", "yards", "walls", "tick", "result")
    worse_cells: list[str] = []
    counts = {"same": 0, "better": 0, "worse": 0, "missing": 0}
    identical = True
    for key in sorted(set(before) | set(after)):
        brain, map_name, country, seed, ticks, opponents, difficulty = key
        label = (f"{map_name} {country} seed {seed} [{brain}] {ticks} ticks, "
                 f"{opponents} opponent(s), {difficulty or 'normal'}")
        prefix = (f"{map_name:<14}{country:<10}{seed:<7}{ticks:<7}{brain:<7}{opponents:<5}"
                  f"{difficulty or 'normal':<8}")
        was, now = before.get(key), after.get(key)
        if was is None:
            counts["missing"] += 1
            identical = False
            print(f"{prefix}no baseline row: {label}", flush=True)
            continue
        if now is None:
            counts["missing"] += 1
            identical = False
            print(f"{prefix}missing from this run: {label}", flush=True)
            continue
        verdict, worse, better = judge(was, now)
        counts[verdict] += 1
        print(
            f"{prefix}"
            f"{arrow(was, now, 'own'):<12}{arrow(was, now, 'army'):<12}"
            f"{arrow(was, now, 'enemy'):<12}"
            f"{lost_arrow(was, now):<16}"
            f"{arrow(was, now, 'unowned'):<10}{arrow(was, now, 'stuck'):<9}"
            f"{arrow(was, now, 'yards'):<9}{arrow(was, now, 'walls'):<9}{verdict}",
            flush=True,
        )
        if worse:
            worse_cells.append(f"{label}: {'; '.join(worse)}")
        if worse or better:
            print(f"    {'; '.join(worse + better)}", flush=True)
        drift = opening_drift(was, now)
        if drift:
            identical = False
            print(f"    opening differs, so this is not the same experiment: "
                  f"{'; '.join(drift)}", flush=True)
        if any(was.get(field) != now.get(field) for field in delta_fields):
            identical = False

    trailing = counts["worse"] + counts["better"] + counts["same"]
    print("")
    print(f"cells: {trailing + counts['missing']} compared, {counts['same']} same, "
          f"{counts['better']} better, {counts['worse']} worse, "
          f"{counts['missing']} missing", flush=True)
    if worse_cells:
        print("cells that got worse:")
        for line in worse_cells:
            print(f"  {line}", flush=True)
    else:
        print("no cell got worse", flush=True)
    if identical:
        print("every cell reads exactly as the baseline: same seed, same result", flush=True)


def run_command(args: list[str]) -> int:
    """Run a command, streaming its output."""
    print("$", " ".join(args), flush=True)
    return subprocess.call(args, cwd=ROOT)


def match_binary() -> Path | None:
    """The fast-profile match binary, where cargo_run puts it.

    Two packages ship a binary with this name: the engine crate's
    `src/bin/autoplay-match.rs`, which drives the retail brain only, and the
    autoplay crate's `src/bin/match.rs`, which is the one that takes `--brain v2`.
    The newest wins because that is the one the build in this run just wrote — and
    that is exactly how a run went wrong after the upstream merge: the engine
    binary was rebuilt last, the harness picked it, `--brain v2` was refused as an
    unknown flag, the process exited during argument parsing, and the report read
    `own=0 enemy=0 jobs=none` instead of saying the wrong binary had been chosen.

    So the newest is still preferred, but a candidate that does not accept
    `--brain` is not offered for a v2 run at all.
    """
    owned = ROOT / "target" / "owned-worktrees"
    if not owned.is_dir():
        return None
    candidates = sorted(owned.glob("*/fast/autoplay-match.exe"))
    candidates += sorted(owned.glob("*/fast/autoplay-match"))
    if not candidates:
        return None
    ready = [path for path in candidates if accepts_brain_flag(path)]
    if ready:
        return max(ready, key=lambda path: path.stat().st_mtime)
    # No candidate takes the flag: fall back to the newest and let the run report
    # the refusal rather than silently measuring the other policy.
    return max(candidates, key=lambda path: path.stat().st_mtime)


def accepts_brain_flag(binary: Path) -> bool:
    """Whether this binary rejects `--brain`: its usage list is the answer.

    `--help` exits through the same argument parser a run does, so a binary that
    does not know the flag prints `unknown flag --brain` and a non-zero status.
    Asked once per candidate and cached nowhere: a build in the same run can
    replace the binary while the matrix is starting.
    """
    try:
        done = subprocess.run(
            [str(binary), "--brain", "v2", "--help"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=20,
        )
    except (OSError, subprocess.SubprocessError):
        return False
    text = (done.stdout or "") + (done.stderr or "")
    return "unknown flag --brain" not in text


def run_match(
    binary: Path,
    seed: str,
    map_name: str,
    ticks: int,
    log_dir: Path,
    brain: str = "retail",
    opponents: int = 1,
    country: str = "america",
    start_credits: int | None = None,
    start_units: int | None = None,
    start_tech: int | None = None,
    difficulty: str = DEFAULT_DIFFICULTY,
    speed: int = DEFAULT_SPEED,
) -> MatchReport:
    """Run one match and read its log.

    Every run is its own process, so the matrix is the pool: one process per
    map and seed, all of them side by side.
    """
    stem = map_name.replace("/", "_").replace("\\", "_")
    log = log_dir / f"match-{stem}-{seed}-{brain}-{country}-{difficulty}-s{speed}.log"
    started = time.monotonic()
    command = [
        str(binary),
        "--map", map_name,
        "--ticks", str(ticks),
        "--seed", seed,
        "--brain", brain,
        "--opponents", str(opponents),
        "--country", country,
        "--difficulty", difficulty,
        "--speed", str(speed),
    ]
    for flag, value in (
        ("--credits", start_credits),
        ("--units", start_units),
        ("--tech", start_tech),
    ):
        if value is not None:
            command += [flag, str(value)]
    with log.open("w", encoding="utf-8") as handle:
        subprocess.call(
            command,
            cwd=ROOT,
            stdout=handle,
            stderr=subprocess.STDOUT,
            env={**os.environ, "RUST_LOG": "info"},
        )
    seconds = time.monotonic() - started
    text = log.read_text(encoding="utf-8", errors="replace")
    report = parse_report(seed, text, seconds)
    start = START.search(text)
    if start:
        report.start_credits = int(start.group(3))
        report.start_units = int(start.group(4))
        report.start_tech = int(start.group(5))
        report.start_own = int(start.group(6))
        report.roster = start.group(8)
    report.map = map_name
    report.country = country
    return report


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seeds", default=",".join(DEFAULT_SEEDS),
                        help="comma separated match seeds, run side by side")
    parser.add_argument("--map", default=DEFAULT_MAP, help="the map to play")
    parser.add_argument("--maps", default="",
                        help="comma separated maps, one match per map and seed")
    parser.add_argument("--brain", default="retail", choices=["retail", "v2"],
                        help="which policy drives the local house")
    parser.add_argument("--countries", default="",
                        help="comma separated local factions: america,russia,yuri,...")
    parser.add_argument("--start-credits", type=int, default=None,
                        help="starting credits (default: the rules' Battle default)")
    parser.add_argument("--start-units", type=int, default=None,
                        help="starting units per house")
    parser.add_argument("--start-tech", type=int, default=None,
                        help="starting tech level")
    parser.add_argument("--history", default="",
                        help="append every result here (default target/autoplay-results/history.jsonl)")
    parser.add_argument("--baseline", default="",
                        help="reference history file: after the run, print what each cell "
                             "moved compared with it (implies --compare)")
    parser.add_argument("--compare", action="store_true",
                        help="compare against --baseline; that flag already implies this")
    parser.add_argument("--compare-only", action="store_true",
                        help="compare --baseline with --history and run nothing")
    parser.add_argument("--opponents", type=int, default=1,
                        help="computer houses against the local one")
    parser.add_argument("--speed", type=int, default=DEFAULT_SPEED,
                        help="the game speed index 0..=6; changes construction and "
                             "infantry cadence through speed_normalize, so a run at "
                             "the wrong index is not the match a player sees")
    parser.add_argument("--difficulty", default=DEFAULT_DIFFICULTY,
                        choices=["easy", "normal", "hard"],
                        help="the computer's difficulty; recorded in the history row because "
                             "a Normal match is not comparable to a Hard one")
    parser.add_argument("--ticks", type=int, default=DEFAULT_TICKS, help="how many frames a match runs")
    parser.add_argument("--jobs", type=int, default=0,
                        help="how many matches at once (0: cores/3, measured optimum; "
                             "a match is itself multi-threaded, so one per core is slower)")
    parser.add_argument("--skip-build", action="store_true", help="use the binary as it is")
    parser.add_argument("--skip-tests", action="store_true", help="do not run the retail tests")
    parser.add_argument("--log-dir", default=None, help="where the match logs go")
    args = parser.parse_args(argv)

    if args.compare and not args.baseline:
        parser.error("--compare needs --baseline <file>")

    seeds = [seed.strip() for seed in args.seeds.split(",") if seed.strip()]
    # The match binary reads a decimal seed; a hex one is what a person writes.
    seeds = [str(int(seed, 0)) for seed in seeds]
    log_dir = Path(args.log_dir) if args.log_dir else ROOT / "target" / "autoplay-check"
    log_dir.mkdir(parents=True, exist_ok=True)
    history = Path(args.history) if args.history else DEFAULT_HISTORY

    # Reading two files on disk needs no build and no retail install, so a
    # comparison can be re-read as often as wanted while a run is understood.
    if args.compare_only:
        if not args.baseline:
            parser.error("--compare-only needs --baseline <file>")
        compare_histories(Path(args.baseline), history)
        return 0

    if not args.skip_build:
        build = [sys.executable, "-m", "tools.cargo_run", "--", "build", "--profile", "fast"]
        if args.brain == "v2":
            build += ["-p", "vera20k-autoplay", "--features", "retail-fixture"]
        build += ["--bin", "autoplay-match"]
        code = run_command(build)
        if code != 0:
            print("the build failed; the matches would be the old binary", flush=True)
            return code
    if not args.skip_tests:
        code = run_command([sys.executable, "-m", "tools.cargo_run", "--", "test",
                            "-p", "vera20k-core", "--lib", "sim::strategy::assist"])
        if code != 0:
            print("the retail tests failed", flush=True)
            return code

    binary = match_binary()
    if binary is None:
        print("no fast-profile match binary; build it first", flush=True)
        return 1
    if not os.environ.get("RA2_DIR"):
        print("RA2_DIR is not set: the matches are skipped", flush=True)
        return 0

    maps = [name.strip() for name in args.maps.split(",") if name.strip()] or [args.map]
    countries = [name.strip() for name in args.countries.split(",") if name.strip()] or ["america"]
    combos = [(name, country, seed) for name in maps for country in countries for seed in seeds]
    jobs = args.jobs or min(len(combos), default_jobs())
    with ThreadPoolExecutor(max_workers=max(1, jobs)) as pool:
        reports = list(pool.map(
            lambda combo: run_match(
                binary, combo[2], combo[0], args.ticks, log_dir,
                args.brain, args.opponents, combo[1],
                args.start_credits, args.start_units, args.start_tech,
                args.difficulty, args.speed,
            ),
            combos,
        ))

    print("", flush=True)
    for report in reports:
        print(f"{report.map} {report.country} seed {report.seed} [{args.brain}]: "
              f"{report.result} in {report.seconds:.0f}s", flush=True)
        print(f"  start credits={report.start_credits} units={report.start_units} "
              f"tech={report.start_tech} roster=[{report.roster}]", flush=True)
        print(f"  own={report.own} enemy={report.enemy} yards={report.yards} "
              f"walls={report.walls} unowned={report.unowned} stuck={report.stuck} "
              f"lost={report.exchange}", flush=True)
        if report.plots:
            print(f"  superweapon plots: {', '.join(report.plots)}", flush=True)
        if report.crates:
            print(f"  crates picked up: {', '.join(report.crates)}", flush=True)
        # The capability line: what the house was ever asked to do. A kind at
        # zero is a feature that was never exercised, however much code it has.
        used = {kind: count for kind, count in report.capabilities.items() if count}
        idle = report.unused_capabilities
        print(f"  jobs used: {used if used else 'none'}", flush=True)
        if idle:
            print(f"  never asked for: {', '.join(idle)}", flush=True)

    append_history(reports, args, history)
    print(f"\nhistory: {history}", flush=True)
    capability_report(reports)
    if args.baseline:
        compare_histories(Path(args.baseline), history)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
