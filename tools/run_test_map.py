#!/usr/bin/env python3
"""Run one committed test map by scenario name, headless or interactive.

The maps under `tests/maps/` are purpose-built shapes, not retail lookalikes:
each proves one scenario the engine needs, and every one is regenerated
byte-identically by the `tools.scale_map` command its entry records, so the
committed files can never drift from that command. `tools/tests/
test_test_map.py` fails when they do.

Usage (from the checkout root, Python >= 3.12):
    python -m tools.run_test_map list
    python -m tools.run_test_map flat                          # headless march run
    python -m tools.run_test_map match --ticks 300             # headless match run
    python -m tools.run_test_map flat --debug -- --units 50    # fast debug build
    python -m tools.run_test_map flat --interactive            # launch the client

The headless run goes through `sim-bench`, whose scenario and flags pass
through: `--scenario` selects the shape's scenario (each entry carries its
default), everything after `--` reaches sim-bench unchanged, and `--debug`
skips the release build sim-bench's timings are documented for. A map name
can also follow `play` explicitly. `--interactive` launches a fast-profile
client with `RA2_QUICKPLAY` naming the map, which needs a display and the
retail archives, like any ordinary local run. Interactive previews seed a small
stock YR force including an MCV, so an empty map does not end in defeat;
`RA2_DEBUG_SPAWN_UNITS` overrides that force. Builds use `tools.cargo_run`;
the recorded executable starts after its build lock has been released.

Neither mode needs a map from the retail install; the rules and archives still
come from it through `RA2_DIR`, as every other run does.
"""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys
from typing import Sequence

from tools import cargo_run

ROOT = Path(__file__).resolve().parents[1]


class TestMap:
    """One committed test map: where it is and how it is run and rebuilt."""

    def __init__(
        self,
        name: str,
        path: str,
        scenario: str,
        summary: str,
        regenerate: Sequence[str],
        extra: Sequence[str] = (),
    ) -> None:
        self.name = name
        self.path = path
        self.scenario = scenario
        self.summary = summary
        self.regenerate = tuple(regenerate)
        self.extra = tuple(extra)

    @property
    def absolute(self) -> Path:
        return (ROOT / self.path).resolve()


# The committed set: a scenario per shape the engine needs to prove. Adding a
# map means adding it here, regenerating it with the recorded command, and
# letting tools/tests/test_test_map.py pin the bytes.
MAPS: tuple[TestMap, ...] = (
    TestMap(
        "flat",
        "tests/maps/flat.map",
        "march",
        "Flat 64x64-cell ground with no overlays: movement, pathing and "
        "scale runs over bare terrain.",
        regenerate=("python", "-m", "tools.scale_map", "tests/maps/flat.map", "--size", "32"),
    ),
    TestMap(
        "match",
        "tests/maps/match.map",
        "match",
        "160x160-cell match shape: four grid starts, an ore field beside each "
        "and tree clumps between them, for harvesting, bases and battles.",
        regenerate=(
            "python",
            "-m",
            "tools.scale_map",
            "tests/maps/match.map",
            "--size",
            "80",
            "--starts",
            "4",
        ),
    ),
)

BY_NAME = {entry.name: entry for entry in MAPS}


def test_map(name: str) -> TestMap:
    try:
        return BY_NAME[name]
    except KeyError:
        raise SystemExit(
            f"unknown test map {name!r}; known: {', '.join(sorted(BY_NAME))}"
        ) from None


def run(
    binary: str,
    package: str,
    profile: str,
    arguments: Sequence[str],
    env_extra: dict[str, str] | None = None,
    cargo_extra: Sequence[str] = (),
) -> int:
    command = ["build", "-p", package, "--bin", binary]
    if profile != "debug":
        command += ["--profile", profile]
    status = cargo_run.invoke([*command, *cargo_extra], cwd=ROOT)
    if status != 0:
        return status
    executable, _ = cargo_run.resolve_binary(ROOT, binary, profile)
    if executable is None:
        print(f"no recorded {profile} executable for {binary} after build", file=sys.stderr)
        return 2
    command = [str(executable), *arguments]
    print(f"+ {' '.join(command)}", flush=True)
    return subprocess.call(command, cwd=ROOT, env={**os.environ, **(env_extra or {})})


def headless(entry: TestMap, scenario: str, release: bool, extra: Sequence[str]) -> int:
    arguments = [
        "--map",
        str(entry.absolute),
        "--scenario",
        scenario,
        *entry.extra,
        *extra,
    ]
    return run("sim-bench", "vera20k-core", "release" if release else "debug", arguments)


def interactive(entry: TestMap, extra: Sequence[str]) -> int:
    return run(
        "vera20k", "vera20k", "fast", (),
        {
            "RA2_QUICKPLAY": str(entry.absolute),
            "RA2_DEBUG_SPAWN_UNITS": os.environ.get(
                "RA2_DEBUG_SPAWN_UNITS", "AMCV:1,MTNK:2,E1:2"
            ),
        },
        cargo_extra=extra,
    )


def command_list() -> int:
    for entry in MAPS:
        print(f"{entry.name:8} {entry.path}  (scenario: {entry.scenario})")
        print(f"  {entry.summary}")
        print(f"  regenerate: {' '.join(entry.regenerate)}")
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv and argv[0] in BY_NAME:
        argv.insert(0, "play")
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("list", help="print the committed test maps")
    play = commands.add_parser("play", help="run one test map")
    play.add_argument("map", metavar="MAP")
    play.add_argument("--scenario", help="sim-bench scenario (default: the map's own)")
    play.add_argument(
        "--debug",
        action="store_true",
        help="skip the release build; hashes stay meaningful, timings do not",
    )
    play.add_argument(
        "--interactive",
        action="store_true",
        help="launch the client on the map with RA2_QUICKPLAY",
    )
    args, extra = parser.parse_known_args(argv)
    extra = [value for value in extra if value != "--"]

    if args.command == "list":
        return command_list()
    entry = test_map(args.map)
    if not entry.absolute.is_file():
        raise SystemExit(f"{entry.path} is missing; regenerate: {' '.join(entry.regenerate)}")
    if args.interactive:
        return interactive(entry, extra)
    return headless(entry, args.scenario or entry.scenario, not args.debug, extra)


if __name__ == "__main__":
    sys.exit(main())
