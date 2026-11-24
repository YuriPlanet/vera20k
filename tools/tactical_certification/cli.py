"""Command-line interface for hidden tactical production certification."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Sequence

from .core import INVALID, VALID, OutputExistsError, ValidationError, write_json_exclusive
from .orchestrator import (
    capture_once,
    validate_existing,
    validate_repeat,
)
from .profile import (
    load_contract,
    load_profile,
    repository_contract_path,
    validate_contract_source_coverage,
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="python -m tools.tactical_certification",
        description=(
            "Validate and run immutable VERA20k radar-online-v2 tactical captures."
        ),
    )
    commands = parser.add_subparsers(dest="command_name", required=True)

    profile = commands.add_parser(
        "validate-profile",
        help="strictly validate one sealed tactical profile and the shared contract",
    )
    profile.add_argument("--profile", required=True, type=Path)
    profile.add_argument(
        "--contract", type=Path, default=repository_contract_path()
    )

    capture = commands.add_parser(
        "capture",
        help="launch one hidden no-input production capture child",
    )
    capture.add_argument("--profile", required=True, type=Path)
    capture.add_argument(
        "--contract", type=Path, default=repository_contract_path()
    )
    capture.add_argument("--executable", required=True, type=Path)
    capture.add_argument("--working-directory", required=True, type=Path)
    capture.add_argument("--run-dir", required=True, type=Path)

    validate = commands.add_parser(
        "validate", help="validate one already-published child capture directory"
    )
    validate.add_argument("--capture", required=True, type=Path)
    validate.add_argument("--profile", required=True, type=Path)
    validate.add_argument(
        "--contract", type=Path, default=repository_contract_path()
    )
    validate.add_argument("--executable", required=True, type=Path)
    validate.add_argument("--working-directory", required=True, type=Path)
    validate.add_argument("--output", required=True, type=Path)

    repeat = commands.add_parser(
        "validate-repeat",
        help="require two same-profile captures to have identical stable evidence and BGRA",
    )
    repeat.add_argument("--first", required=True, type=Path)
    repeat.add_argument("--second", required=True, type=Path)
    repeat.add_argument("--profile", required=True, type=Path)
    repeat.add_argument(
        "--contract", type=Path, default=repository_contract_path()
    )
    repeat.add_argument("--executable", required=True, type=Path)
    repeat.add_argument("--working-directory", required=True, type=Path)
    repeat.add_argument("--output", required=True, type=Path)
    return parser


def _summary(report: dict[str, object]) -> str:
    return json.dumps(
        {
            "status": report["status"],
            "checkpoint": report.get("checkpoint"),
            "profile_id": report.get("profile_id"),
            "errors": report.get("errors", []),
        },
        sort_keys=True,
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    arguments = parser.parse_args(argv)
    try:
        if arguments.command_name == "validate-profile":
            profile = load_profile(arguments.profile)
            contract = load_contract(arguments.contract)
            validate_contract_source_coverage(contract)
            report: dict[str, object] = {
                "status": VALID,
                "checkpoint": profile.checkpoint,
                "profile_id": profile.profile_id,
                "profile_sha256": profile.snapshot.sha256,
                "contract_sha256": contract.snapshot.sha256,
                "native_comparator": "NONE",
                "parity_certification": "NONE",
                "errors": [],
            }
        elif arguments.command_name == "capture":
            report, _ = capture_once(
                arguments.executable,
                arguments.profile,
                arguments.contract,
                arguments.run_dir,
                working_directory=arguments.working_directory,
            )
        elif arguments.command_name == "validate":
            report, _ = validate_existing(
                arguments.capture,
                arguments.profile,
                arguments.contract,
                executable_path=arguments.executable,
                working_directory=arguments.working_directory,
            )
            write_json_exclusive(arguments.output, report)
        else:
            report = validate_repeat(
                arguments.first,
                arguments.second,
                arguments.profile,
                arguments.contract,
                executable_path=arguments.executable,
                working_directory=arguments.working_directory,
            )
            write_json_exclusive(arguments.output, report)
    except (ValidationError, OutputExistsError, OSError) as exc:
        print(f"tactical certification error: {exc}", file=sys.stderr)
        return 2

    print(_summary(report))
    return 0 if report["status"] == VALID else 2
