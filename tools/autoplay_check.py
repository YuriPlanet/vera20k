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
# How often the match prints a status line, in frames.
#
# The push gate holds for PUSH_REST_FRAMES (750, in
# src/sim/strategy/assist/advance.rs) before it can turn back, so an interval
# coarser than about a third of that aliases: two flips inside one interval read
# as none, and the reading the five known issues turn on would be blind. 150
# frames is 2.5 seconds and resolves the gate exactly.
PROGRESS_FRAMES = 150
DEFAULT_HISTORY = ROOT / "target" / "autoplay-results" / "history.jsonl"
# The engine offers three; the spec's word "冷酷" has no fourth. Defaulting to
# Normal while the spec asked for something harder is how every measurement in
# this folder came to describe an easier game than the one being judged, so the
# difficulty is now a recorded field and a flag.
DEFAULT_DIFFICULTY = "normal"
# The game speed index is not a flag any more: src/bin/autoplay-match.rs does
# not parse one, and the index it uses is the same for every run, so runs stay
# comparable to each other. It is written here because the harness used to pass
# `--speed`, and a run at a different index is a different match.
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
JUDGED = ("army", "idle", "unowned", "push_flips")
# The opening a run starts from. Two runs whose openings differ are not the
# same experiment, so a drift here is a warning, not a delta.
OPENING = ("start_credits", "start_own", "roster")

RESULT = re.compile(r"(won|timeout|lost) at tick (\d+)")
CRATE = re.compile(r"crate picked up at (\(\d+, \d+\)) by house (\S+)")
BUILD_LIST = re.compile(r"the build list answers (\w+) for ([A-Z0-9]+)")
STANDING = re.compile(r"(\d+) standing")
# The census row the match prints every interval: tick, credits, enemy credits,
# power, own, enemy, ready, rejected, not-ready.
CENSUS = re.compile(r"^\s*(\d+)\s+(\d+)\s+(\d+)\s+(-?\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s*$", re.M)
# The status line the brain prints once per progress interval, read field by
# field rather than whole: each one is a separate reading, and one pattern that
# spans them all would drop every field the day one of them moves.
#
# fighters is what the house has to fight with, idle how many of those have no
# order, unclaimed how many nobody has claimed. The old log called these
# army/stuck/unowned; the report keeps the old names so a history file written
# before the rename still reads.
UNCLAIMED = re.compile(r"unclaimed=(\d+)")
IDLE = re.compile(r"idle=(\d+)")
# The gate the push waits on. A gate that changes during a match is a turn the
# whole army takes together, so the sequence is kept and counted rather than
# reduced to its last value: alternating is worse than changing once.
PUSH = re.compile(r"push=(\S+)")
SQUADS = re.compile(r"squads=\[([^\]]*)\]")
INCOME = re.compile(r"income=(-?\d+)/min")
# The whole status line in one pattern, used where the question is about the
# shape of a run rather than its last reading: how big the army got, and how big
# it was the moment the push started. The fields are read separately above; this
# one exists because "did the house mass before it attacked" cannot be answered
# from the last line alone.
STATUS = re.compile(
    r"fighters=(\d+) limbo=(\d+) idle=(\d+) "
    r".*?push=(\S+) .*?squads=\[([^\]]*)\]"
)
ROSTER = re.compile(r"roster=\[([^\]]*)\]")
# The house's strength, read off the status line. An object count is what the
# brain actually has to fight with; the enemy count beside it in the census row
# is the other half of the same reading.
ARMY = re.compile(r"fighters=(\d+)")
EPOINTS = re.compile(r"pts=(\d+)/")


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

    roster: str = ""
    # The exchange, read off the census curve rather than its last row: how many
    # objects each side lost over the match. Two runs that end on the same count
    # can still have traded very differently, and that is the reading a change to
    # unit composition has to move.
    own_lost: int = 0
    enemy_lost: int = 0
    # The army at the end: how many fighters it has, how many of them are doing
    # nothing, and how the army was divided. A house with forty fighters and
    # thirty of them idle is not the army its count claims.
    idle: int = 0
    # The push gate the last status line reported, how many times it changed
    # during the match, and the squad sizes at that last reading.
    push: str = ""
    push_flips: int = 0
    squads: str = ""
    # Income at the end, per minute, as the brain reports it.
    income: int = 0
    # How big the army was at its largest, and how big it was on the first
    # status line that reported the push gate open. "The army walks out one unit
    # at a time" is a claim about the second number: a house that attacks with
    # five when it could have had thirty did not mass, whatever its final count.
    peak_army: int = 0
    peak_limbo: int = 0
    army_at_push: int = 0

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
        # The opening the match started from, read off its first census row:
        # without it two results are not the same experiment.
        report.start_credits = int(rows[0][1])
        report.start_own = int(rows[0][4])
    # The last reading, not the worst: a house that was idle at some point and
    # then got an army moving is not a house that is idle now.
    unclaimed = UNCLAIMED.findall(text)
    report.unowned = int(unclaimed[-1]) if unclaimed else 0
    readings = IDLE.findall(text)
    report.stuck = int(readings[-1]) if readings else 0
    report.idle = report.stuck
    army = ARMY.findall(text)
    report.army = int(army[-1]) if army else 0
    epoints = EPOINTS.findall(text)
    report.epoints = int(epoints[-1]) if epoints else 0
    pushes = PUSH.findall(text)
    report.push = pushes[-1] if pushes else ""
    # Every change of gate is a turn the whole army takes together. Counted on
    # the sequence so that a gate alternating between two values reads worse
    # than one that changed once, which is what the complaint describes.
    report.push_flips = sum(1 for was, now in zip(pushes, pushes[1:]) if was != now)
    squads = SQUADS.findall(text)
    report.squads = squads[-1] if squads else ""
    income = INCOME.findall(text)
    report.income = int(income[-1]) if income else 0
    status = STATUS.findall(text)
    if status:
        report.peak_army = max(int(row[0]) for row in status)
        report.peak_limbo = max(int(row[1]) for row in status)
        # The first open gate, not the last: the last one is after the army has
        # already been ground down, which measures the battle rather than the
        # decision to start it.
        report.army_at_push = next(
            (int(row[0]) for row in status if row[3] == "push"), 0
        )
    report.yards = max((int(value) for value in re.findall(r"yard=(\d+)", text)), default=0)
    report.walls = max((int(value) for value in STANDING.findall(text)), default=0)
    roster = ROSTER.findall(text)
    report.roster = roster[0] if roster else ""
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
                "roster": report.roster,
                "idle": report.idle,
                "push": report.push,
                "push_flips": report.push_flips,
                "squads": report.squads,
                "income": report.income,
                "peak_army": report.peak_army,
                "peak_limbo": report.peak_limbo,
                "army_at_push": report.army_at_push,
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

    Five things decide:

    - **army**, what the house has to fight with. A row written before `army`
      existed falls back to `own`, because comparing nothing is worse than
      comparing the old reading;
    - **stuck** must not rise. This is the reading behind "the army wanders": a
      fighter with no order is one the push did not carry. The field keeps its
      old name, but it is now read from the status line's `idle=`, which is
      what the current brain counts; the `stuck=` it used to read is gone.
      They are the same complaint from two sides: a unit the policy claimed and
      could not move, and a unit it never claimed at all.
    - **unclaimed** must not rise: a fighter nobody owns is a bug;
    - **push_flips** must not rise. Every change of push gate is a turn the whole
      army takes together, so a gate that alternates is a house that cannot
      finish what it started;
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

    for field in ("unowned", "stuck", "push_flips"):
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
        f"{'own':<12}{'army':<12}{'enemy':<12}{'lost':<16}{'unclaimed':<10}{'idle':<7}"
        f"{'limbo':<7}{'peak':<7}{'push@':<7}{'flips':<7}{'yards':<7}{'walls':<7}verdict"
    )
    print("", flush=True)
    print(header, flush=True)
    print("-" * (len(header) + 24), flush=True)

    # What makes two readings the same experiment. Enemy count, walls and the
    # opening are in it: a run that moved one of those is not a replay.
    delta_fields = ("own", "enemy", "unowned", "stuck", "peak_army", "army_at_push",
                    "push_flips", "yards", "walls", "tick", "result")
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
            f"{arrow(was, now, 'unowned'):<10}{arrow(was, now, 'stuck'):<7}"
            f"{arrow(was, now, 'peak_limbo'):<7}"
            f"{arrow(was, now, 'peak_army'):<7}{arrow(was, now, 'army_at_push'):<7}"
            f"{arrow(was, now, 'push_flips'):<7}{arrow(was, now, 'yards'):<7}"
            f"{arrow(was, now, 'walls'):<7}{verdict}",
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
    """The fast-profile match binary, found the way cargo_run finds it.

    This used to glob `ROOT/target/owned-worktrees/*/fast/autoplay-match.exe`,
    which is right only when ROOT is the checkout that also holds the cache. In a
    worktree ROOT holds no target at all, so the glob found nothing and the run
    reported no binary while a good one sat in the primary checkout. The runner
    already knows the rule — the cache is `CARGO_TARGET_DIR` when set, else the
    primary checkout's `target/`, under `owned-worktrees/<namespace>` — so ask it
    rather than keep a second copy of the rule that drifts.

    The second thing this used to do was choose between two binaries that both
    answer to this name. There is one now: src/bin/autoplay-match.rs, the assist
    brain. The autoplay crate's `--brain v2` binary is gone with the retail
    rename.
    """
    from tools import cargo_run

    # The second element is the profile the binary was built at, not a problem:
    # resolve_binary reports no error, it just reports nothing found as (None,
    # None). A run that silently fell back to a stale binary is what the old glob
    # could do, so the profile is printed with the run it belongs to.
    found, profile = cargo_run.resolve_binary(ROOT, "autoplay-match", "fast")
    if found is not None:
        print(f"autoplay_check: autoplay-match at profile {profile}", flush=True)
    else:
        print("autoplay_check: no unchanged fast-profile autoplay-match binary", flush=True)
    return found


def run_match(
    binary: Path,
    seed: str,
    map_name: str,
    ticks: int,
    log_dir: Path,
    opponents: int = 1,
    country: str = "america",
    difficulty: str = DEFAULT_DIFFICULTY,
) -> MatchReport:
    """Run one match and read its log.

    Every run is its own process, so the matrix is the pool: one process per
    map and seed, all of them side by side.

    The flags are the ones src/bin/autoplay-match.rs parses and no others: that
    binary is the assist brain and nothing else, so --brain, --speed and the
    opening-credits flags are gone with it. Passing one it does not know makes
    it exit during argument parsing, and the report then reads as an empty
    match rather than as a refusal.
    """
    stem = map_name.replace("/", "_").replace("\\", "_")
    log = log_dir / f"match-{stem}-{seed}-{country}-{difficulty}.log"
    started = time.monotonic()
    command = [
        str(binary),
        "--map", map_name,
        "--ticks", str(ticks),
        "--seed", seed,
        "--opponents", str(opponents),
        "--opponent", country,
        "--difficulty", difficulty,
        "--progress", str(PROGRESS_FRAMES),
    ]
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
    parser.add_argument("--brain", default="assist", choices=["assist"],
                        help="which policy drives the local house; the assist brain is the one that exists")
    parser.add_argument("--countries", default="",
                        help="comma separated local factions: america,russia,yuri,...")

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
                args.opponents, combo[1], args.difficulty,
            ),
            combos,
        ))

    print("", flush=True)
    for report in reports:
        print(f"{report.map} {report.country} seed {report.seed} [{args.brain}]: "
              f"{report.result} in {report.seconds:.0f}s", flush=True)
        print(f"  start credits={report.start_credits} own={report.start_own} "
              f"roster=[{report.roster}]", flush=True)
        print(f"  own={report.own} enemy={report.enemy} yards={report.yards} "
              f"walls={report.walls} army={report.army} idle={report.idle} "
              f"unclaimed={report.unowned} lost={report.exchange} "
              f"limbo={report.peak_limbo}", flush=True)
        print(f"  push={report.push or 'none'} flips={report.push_flips} "
              f"squads=[{report.squads}] income={report.income}/min", flush=True)
        print(f"  peak army={report.peak_army} army at first push={report.army_at_push}",
              flush=True)
        if report.plots:
            print(f"  superweapon plots: {', '.join(report.plots)}", flush=True)
        if report.crates:
            print(f"  crates picked up: {', '.join(report.crates)}", flush=True)

    append_history(reports, args, history)
    print(f"\nhistory: {history}", flush=True)
    if args.baseline:
        compare_histories(Path(args.baseline), history)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
