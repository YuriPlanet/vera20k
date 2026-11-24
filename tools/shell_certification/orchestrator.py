"""Child-PID-only capture orchestration and provenance reporting."""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any

from tools.child_process import run_child

from .core import (
    CAPTURE_SCHEMA_VERSION,
    CHECKPOINT,
    CURSOR_POINT,
    INVALID,
    LOGICAL_HEIGHT,
    LOGICAL_WIDTH,
    SEALED_MAIN_MENU_GUARD_SHA256,
    OutputExistsError,
    ValidationError,
    absolute_path,
    build_comparison_report,
    sha256_bytes,
    sha256_file,
    utc_now,
    validate_capture_bundle,
    validate_guard,
    write_bytes_exclusive,
    write_json_exclusive,
)


RUN_SCHEMA_VERSION = "vera20k.shell-capture-run.v1"
STDOUT_FILENAME = "stdout.txt"
STDERR_FILENAME = "stderr.txt"
RUN_FILENAME = "run.json"
COMPARISON_FILENAME = "comparison.json"
DEFAULT_TIMEOUT_SECONDS = 60.0
MAX_TIMEOUT_SECONDS = 300.0
EXPECTED_CHILD_ARTIFACTS = frozenset(("capture.json", "frame.bgra"))
CONFIG_FILENAME = "config.toml"


def _require_new_run_directory(run_directory: Path) -> Path:
    run_dir = absolute_path(run_directory)
    if run_dir.exists() or run_dir.is_symlink():
        raise OutputExistsError(f"run directory already exists: {run_dir}")
    parent = run_dir.parent
    if not parent.exists() or not parent.is_dir():
        raise ValidationError(f"run-directory parent does not exist: {parent}")
    return run_dir


def _require_child_working_directory(
    working_directory: str | os.PathLike[str],
) -> tuple[Path, Path, str]:
    """Validate the explicit child resource base and hash its config."""

    supplied = Path(working_directory)
    if not supplied.is_absolute():
        raise ValidationError(
            f"child working directory must be absolute: {supplied}"
        )
    child_cwd = absolute_path(supplied)
    is_junction = getattr(child_cwd, "is_junction", lambda: False)
    try:
        if child_cwd.is_symlink() or is_junction():
            raise ValidationError(
                f"child working directory must not be a link or junction: {child_cwd}"
            )
        if not child_cwd.is_dir():
            raise ValidationError(
                f"child working directory is not a directory: {child_cwd}"
            )
    except OSError as exc:
        raise ValidationError(
            f"cannot validate child working directory {child_cwd}: {exc}"
        ) from exc

    config_path = child_cwd / CONFIG_FILENAME
    try:
        if config_path.is_symlink() or not config_path.is_file():
            raise ValidationError(
                f"{CONFIG_FILENAME} is not a regular non-link file: {config_path}"
            )
    except OSError as exc:
        raise ValidationError(
            f"cannot validate {CONFIG_FILENAME} {config_path}: {exc}"
        ) from exc
    config_sha256 = sha256_file(config_path, CONFIG_FILENAME)
    return child_cwd, config_path, config_sha256


def _ensure_diagnostic_directory(run_directory: Path) -> None:
    if run_directory.exists():
        if run_directory.is_symlink() or not run_directory.is_dir():
            raise ValidationError(
                f"child output path is not a regular directory: {run_directory}"
            )
        return
    try:
        run_directory.mkdir()
    except OSError as exc:
        raise ValidationError(
            f"cannot create diagnostic run directory {run_directory}: {exc}"
        ) from exc


def _inventory_child_output(
    run_directory: Path,
) -> tuple[list[dict[str, Any]], list[str]]:
    """Snapshot and validate entries before adding wrapper diagnostics."""

    inventory: list[dict[str, Any]] = []
    errors: list[str] = []
    try:
        with os.scandir(run_directory) as scanner:
            entries = sorted(scanner, key=lambda entry: entry.name)
    except OSError as exc:
        raise ValidationError(
            f"cannot inventory child output directory {run_directory}: {exc}"
        ) from exc

    for entry in entries:
        expected = entry.name in EXPECTED_CHILD_ARTIFACTS
        try:
            if entry.is_symlink():
                kind = "symlink"
                byte_length = None
            elif entry.is_file(follow_symlinks=False):
                kind = "file"
                byte_length = entry.stat(follow_symlinks=False).st_size
            elif entry.is_dir(follow_symlinks=False):
                kind = "directory"
                byte_length = None
            else:
                kind = "other"
                byte_length = None
        except OSError as exc:
            kind = "unreadable"
            byte_length = None
            errors.append(f"cannot inspect child output entry {entry.name!r}: {exc}")

        inventory.append(
            {
                "name": entry.name,
                "kind": kind,
                "byte_length": byte_length,
                "expected": expected,
            }
        )
        if not expected:
            errors.append(f"unexpected child output entry: {entry.name!r}")
        elif kind != "file":
            errors.append(
                f"child output {entry.name!r} must be a regular non-link file, "
                f"got {kind}"
            )
    return inventory, errors


def build_capture_command(executable: Path, run_directory: Path) -> list[str]:
    """Construct the only supported noninteractive production-capture command."""

    return [
        str(executable),
        "--shell-capture",
        CHECKPOINT,
        "--width",
        str(LOGICAL_WIDTH),
        "--height",
        str(LOGICAL_HEIGHT),
        "--cursor-x",
        str(CURSOR_POINT[0]),
        "--cursor-y",
        str(CURSOR_POINT[1]),
        "--output",
        str(run_directory),
    ]


def _diagnostic_file_metadata(path: Path, label: str) -> dict[str, Any]:
    """Return a digest for a present malformed artifact when safely possible."""

    try:
        digest = sha256_file(path, label)
        byte_length = path.stat().st_size
        return {
            "path": path.name,
            "sha256": digest,
            "byte_length": byte_length,
        }
    except (ValidationError, OSError) as exc:
        return {
            "path": path.name,
            "sha256": None,
            "byte_length": None,
            "diagnostic_error": str(exc),
        }


def capture_and_compare(
    executable_path: str | os.PathLike[str],
    guard_path: str | os.PathLike[str],
    run_directory: str | os.PathLike[str],
    *,
    working_directory: str | os.PathLike[str],
    timeout_seconds: float = DEFAULT_TIMEOUT_SECONDS,
) -> tuple[dict[str, Any], dict[str, Any]]:
    """Run VERA once, retain diagnostics, validate, compare, and never overwrite.

    The timeout cleanup calls ``kill`` only on the exact ``Popen`` child object.
    It does not use a shell, a process group, taskkill, or descendant traversal.
    """

    if not isinstance(timeout_seconds, (int, float)) or isinstance(
        timeout_seconds, bool
    ):
        raise ValidationError("timeout must be a finite number of seconds")
    timeout = float(timeout_seconds)
    if not (0.0 < timeout <= MAX_TIMEOUT_SECONDS):
        raise ValidationError(
            f"timeout must be greater than zero and at most {MAX_TIMEOUT_SECONDS}"
        )

    executable = absolute_path(executable_path)
    if executable.is_symlink() or not executable.is_file():
        raise ValidationError(f"executable is not a regular non-link file: {executable}")
    guard = absolute_path(guard_path)
    # Validate the sealed evidence before starting a process.
    validate_guard(guard, expected_sha256=SEALED_MAIN_MENU_GUARD_SHA256)
    run_dir = _require_new_run_directory(Path(run_directory))
    child_working_directory, config_path, config_sha256 = (
        _require_child_working_directory(working_directory)
    )

    executable_sha256 = sha256_file(executable, "VERA executable")
    command = build_capture_command(executable, run_dir)
    started_at = utc_now()
    child = run_child(command, cwd=child_working_directory, temporary_directory=run_dir.parent,
                      timeout_seconds=timeout)
    child_pid, exit_status, timed_out = child.pid, child.exit_status, child.timed_out
    stdout, stderr = child.stdout, child.stderr
    orchestration_errors = list(child.errors)

    finished_at = utc_now()
    _ensure_diagnostic_directory(run_dir)
    child_output_inventory, inventory_errors = _inventory_child_output(run_dir)
    for inventory_error in inventory_errors:
        if inventory_error not in orchestration_errors:
            orchestration_errors.append(inventory_error)
    write_bytes_exclusive(run_dir / STDOUT_FILENAME, stdout)
    write_bytes_exclusive(run_dir / STDERR_FILENAME, stderr)

    try:
        executable_sha256_after = sha256_file(executable, "VERA executable")
    except ValidationError as exc:
        executable_sha256_after = None
        orchestration_errors.append(str(exc))
    if (
        executable_sha256_after is not None
        and executable_sha256_after != executable_sha256
    ):
        orchestration_errors.append("VERA executable changed during the capture run")
    try:
        config_sha256_after = sha256_file(config_path, CONFIG_FILENAME)
    except ValidationError as exc:
        config_sha256_after = None
        orchestration_errors.append(str(exc))
    if config_sha256_after is not None and config_sha256_after != config_sha256:
        orchestration_errors.append(f"{CONFIG_FILENAME} changed during the capture run")
    if timed_out:
        pass
    elif exit_status is None:
        orchestration_errors.append("capture child has no exit status")
    elif exit_status != 0:
        orchestration_errors.append(
            f"capture child exited with nonzero status {exit_status}"
        )

    capture_validation: dict[str, Any]
    try:
        capture = validate_capture_bundle(
            run_dir,
            required_schema_version=CAPTURE_SCHEMA_VERSION,
        )
        capture_validation = {
            "status": "VALID",
            "errors": [],
            "manifest_path": "capture.json",
            "manifest_sha256": capture.manifest_sha256,
            "manifest_byte_length": capture.manifest_path.stat().st_size,
            "frame_path": "frame.bgra",
            "frame_sha256": capture.frame_sha256,
            "frame_byte_length": len(capture.frame_bytes),
        }
    except ValidationError as exc:
        orchestration_errors.append(str(exc))
        manifest_diagnostic = _diagnostic_file_metadata(
            run_dir / "capture.json", "capture manifest"
        )
        frame_diagnostic = _diagnostic_file_metadata(
            run_dir / "frame.bgra", "capture frame"
        )
        capture_validation = {
            "status": INVALID,
            "errors": [str(exc)],
            "manifest_path": manifest_diagnostic["path"],
            "manifest_sha256": manifest_diagnostic["sha256"],
            "manifest_byte_length": manifest_diagnostic["byte_length"],
            "manifest_diagnostic_error": manifest_diagnostic.get(
                "diagnostic_error"
            ),
            "frame_path": frame_diagnostic["path"],
            "frame_sha256": frame_diagnostic["sha256"],
            "frame_byte_length": frame_diagnostic["byte_length"],
            "frame_diagnostic_error": frame_diagnostic.get("diagnostic_error"),
        }

    comparison = build_comparison_report(
        run_dir,
        guard,
        additional_errors=orchestration_errors,
    )
    run_status = "COMPLETE" if not orchestration_errors else INVALID
    comparison_path = write_json_exclusive(
        run_dir / COMPARISON_FILENAME, comparison
    )
    comparison_sha256 = sha256_file(comparison_path, "comparison report")
    comparison_byte_length = comparison_path.stat().st_size

    run_report: dict[str, Any] = {
        "schema_version": RUN_SCHEMA_VERSION,
        "status": run_status,
        "errors": orchestration_errors,
        "started_at_utc": started_at,
        "finished_at_utc": finished_at,
        "working_directories": {
            "wrapper": str(absolute_path(Path.cwd())),
            "child": str(child_working_directory),
        },
        "config": {
            "path": str(config_path),
            "sha256": config_sha256,
            "post_run_sha256": config_sha256_after,
            "unchanged": config_sha256_after == config_sha256,
        },
        "executable": {
            "path": str(executable),
            "sha256": executable_sha256,
            "post_run_sha256": executable_sha256_after,
            "unchanged": executable_sha256_after == executable_sha256,
        },
        "command": command,
        "child": {
            "pid": child_pid,
            "exit_status": exit_status,
            "timed_out": timed_out,
            "timeout_seconds": timeout,
            "cleanup_scope": "exact-child-pid-only",
            "output_inventory_before_wrapper": child_output_inventory,
        },
        "artifacts": {
            "stdout": {
                "path": STDOUT_FILENAME,
                "byte_length": len(stdout),
                "sha256": sha256_bytes(stdout),
            },
            "stderr": {
                "path": STDERR_FILENAME,
                "byte_length": len(stderr),
                "sha256": sha256_bytes(stderr),
            },
            "capture": capture_validation,
            "comparison": {
                "path": COMPARISON_FILENAME,
                "status": comparison["status"],
                "sha256": comparison_sha256,
                "byte_length": comparison_byte_length,
            },
        },
        "guard": {
            "path": str(guard),
            "sha256": SEALED_MAIN_MENU_GUARD_SHA256,
        },
    }

    # The child is allowed to create only capture.json and frame.bgra.
    write_json_exclusive(run_dir / RUN_FILENAME, run_report)
    return run_report, comparison
