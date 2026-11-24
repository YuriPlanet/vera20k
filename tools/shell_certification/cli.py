"""Command-line interface for shell capture certification."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Sequence

from .core import (
    DRIFT,
    INVALID,
    MATCH,
    OutputExistsError,
    ValidationError,
    compare_to_file,
)
from .entry_sequence import (
    capture_entry_sequence,
    validate_entry_sequence_bundle,
)
from .orchestrator import (
    DEFAULT_TIMEOUT_SECONDS,
    MAX_TIMEOUT_SECONDS,
    capture_and_compare,
)
from .presentation_profile import write_presentation_profile
from .title_differential import write_title_differential


def _timeout(value: str) -> float:
    try:
        parsed = float(value)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("timeout must be a number") from exc
    if not (0.0 < parsed <= MAX_TIMEOUT_SECONDS):
        raise argparse.ArgumentTypeError(
            f"timeout must be greater than zero and at most {MAX_TIMEOUT_SECONDS:g}"
        )
    return parsed


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="python -m tools.shell_certification",
        description=(
            "Validate and compare immutable VERA20k 0xE2 shell capture bundles."
        ),
    )
    commands = parser.add_subparsers(dest="command_name", required=True)

    compare = commands.add_parser(
        "compare", help="compare an existing capture bundle to the sealed guard"
    )
    compare.add_argument("--capture", required=True, type=Path)
    compare.add_argument("--guard", required=True, type=Path)
    compare.add_argument("--output", required=True, type=Path)

    run = commands.add_parser(
        "capture-and-compare",
        help="launch one hidden capture child, validate it, and compare it",
    )
    run.add_argument("--executable", required=True, type=Path)
    run.add_argument("--working-directory", required=True, type=Path)
    run.add_argument("--guard", required=True, type=Path)
    run.add_argument("--run-dir", required=True, type=Path)
    run.add_argument(
        "--timeout",
        type=_timeout,
        default=DEFAULT_TIMEOUT_SECONDS,
        help=f"child timeout in seconds (default {DEFAULT_TIMEOUT_SECONDS:g})",
    )

    profile = commands.add_parser(
        "derive-presentation-profile",
        help="derive an immutable RGB565 profile from the sealed guard sources",
    )
    profile.add_argument("--guard", required=True, type=Path)
    profile.add_argument("--oracle-runs", required=True, type=Path)
    profile.add_argument("--output", required=True, type=Path)

    title = commands.add_parser(
        "title-differential",
        help="prove the 0x694 title position/tint mismatch from sealed sources",
    )
    title.add_argument("--capture", required=True, type=Path)
    title.add_argument("--guard", required=True, type=Path)
    title.add_argument("--oracle-runs", required=True, type=Path)
    title.add_argument("--output", required=True, type=Path)

    validate_sequence = commands.add_parser(
        "validate-entry-sequence",
        help="validate an immutable 18-frame 0xE2 entry-sequence bundle",
    )
    validate_sequence.add_argument("--capture", required=True, type=Path)

    capture_sequence = commands.add_parser(
        "capture-entry-sequence",
        help="launch one hidden child and validate its 0xE2 entry sequence",
    )
    capture_sequence.add_argument("--executable", required=True, type=Path)
    capture_sequence.add_argument("--working-directory", required=True, type=Path)
    capture_sequence.add_argument("--run-dir", required=True, type=Path)
    capture_sequence.add_argument(
        "--timeout",
        type=_timeout,
        default=DEFAULT_TIMEOUT_SECONDS,
        help=f"child timeout in seconds (default {DEFAULT_TIMEOUT_SECONDS:g})",
    )
    return parser


def _exit_for_status(status: str) -> int:
    if status == MATCH:
        return 0
    if status == DRIFT:
        return 1
    if status == INVALID:
        return 2
    return 2


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    arguments = parser.parse_args(argv)
    try:
        if arguments.command_name == "compare":
            report = compare_to_file(
                arguments.capture, arguments.guard, arguments.output
            )
        elif arguments.command_name == "capture-and-compare":
            _, report = capture_and_compare(
                arguments.executable,
                arguments.guard,
                arguments.run_dir,
                working_directory=arguments.working_directory,
                timeout_seconds=arguments.timeout,
            )
        elif arguments.command_name == "derive-presentation-profile":
            profile = write_presentation_profile(
                arguments.guard,
                arguments.oracle_runs,
                arguments.output,
            )
            print(
                json.dumps(
                    {
                        "schema_version": profile["schema_version"],
                        "guard_sha256": profile["guard"]["sha256"],
                        "output": str(arguments.output.absolute()),
                    },
                    sort_keys=True,
                )
            )
            return 0
        elif arguments.command_name == "title-differential":
            report = write_title_differential(
                arguments.capture,
                arguments.guard,
                arguments.oracle_runs,
                arguments.output,
            )
        elif arguments.command_name == "validate-entry-sequence":
            validation = validate_entry_sequence_bundle(arguments.capture)
            print(json.dumps(validation, sort_keys=True))
            return 0
        else:
            run = capture_entry_sequence(
                arguments.executable,
                arguments.run_dir,
                working_directory=arguments.working_directory,
                timeout_seconds=arguments.timeout,
            )
            print(json.dumps(run, sort_keys=True))
            return 0
    except (ValidationError, OutputExistsError, OSError) as exc:
        print(f"shell certification error: {exc}", file=sys.stderr)
        return 2

    print(
        json.dumps(
            {
                "checkpoint": report["checkpoint"],
                "status": report["status"],
                "errors": report["errors"],
            },
            sort_keys=True,
        )
    )
    return _exit_for_status(report["status"])
