"""Strict validation and child-PID-only capture for the 0xE2 entry sequence."""

from __future__ import annotations

import hashlib
import math
import os
import subprocess
import tempfile
from pathlib import Path
from typing import Any, Mapping

from .core import (
    ALLOWED_SURFACE_FORMATS,
    OutputExistsError,
    ValidationError,
    _parse_json_bytes,
    _read_regular_bytes,
    _require_array,
    _require_bool,
    _require_exact_keys,
    _require_int,
    _require_object,
    _require_string,
    _require_value,
    absolute_path,
    sha256_bytes,
    sha256_file,
)
from .orchestrator import (
    CONFIG_FILENAME,
    DEFAULT_TIMEOUT_SECONDS,
    MAX_TIMEOUT_SECONDS,
    POST_KILL_DRAIN_SECONDS,
    _require_child_working_directory,
    _require_new_run_directory,
)


SCHEMA_VERSION = "vera20k.shell-entry-sequence-capture.v1"
CHECKPOINT = "main-menu-0xe2-entry-sequence"
WIDTH = 800
HEIGHT = 600
CURSOR_POINT = (400, 300)
FRAME_COUNT = 18
FRAME_BYTE_LENGTH = WIDTH * HEIGHT * 4
PAYLOAD_BYTE_LENGTH = FRAME_COUNT * FRAME_BYTE_LENGTH
MANIFEST_FILENAME = "capture.json"
PAYLOAD_FILENAME = "frames.bgra"
EXPECTED_ARTIFACTS = frozenset((MANIFEST_FILENAME, PAYLOAD_FILENAME))
MAX_MANIFEST_BYTES = 1024 * 1024


def _require_bundle_directory(path: Path) -> Path:
    directory = absolute_path(path)
    is_junction = getattr(directory, "is_junction", lambda: False)
    try:
        if directory.is_symlink() or is_junction():
            raise ValidationError(
                f"entry-sequence directory must not be a link or junction: {directory}"
            )
        if not directory.is_dir():
            raise ValidationError(
                f"entry-sequence directory does not exist: {directory}"
            )
        with os.scandir(directory) as scanner:
            entries = list(scanner)
    except OSError as exc:
        raise ValidationError(
            f"cannot inspect entry-sequence directory {directory}: {exc}"
        ) from exc
    names = {entry.name for entry in entries}
    if names != EXPECTED_ARTIFACTS:
        raise ValidationError(
            "entry-sequence file inventory is invalid "
            f"(expected={sorted(EXPECTED_ARTIFACTS)}, actual={sorted(names)})"
        )
    for entry in entries:
        try:
            if entry.is_symlink() or not entry.is_file(follow_symlinks=False):
                raise ValidationError(
                    f"entry-sequence artifact must be a regular non-link file: "
                    f"{entry.name}"
                )
        except OSError as exc:
            raise ValidationError(
                f"cannot inspect entry-sequence artifact {entry.name}: {exc}"
            ) from exc
    return directory


def _validate_manifest(manifest: Mapping[str, Any]) -> tuple[int, list[Mapping[str, Any]]]:
    _require_exact_keys(
        manifest,
        (
            "schema_version",
            "checkpoint",
            "surface",
            "cursor",
            "shell",
            "presenter_domain",
            "generation",
            "completion_observed",
            "payload",
            "frames",
        ),
        "entry-sequence manifest",
    )
    _require_value(manifest["schema_version"], SCHEMA_VERSION, "schema_version")
    _require_value(manifest["checkpoint"], CHECKPOINT, "checkpoint")

    surface = _require_object(manifest["surface"], "surface")
    _require_exact_keys(
        surface,
        (
            "width",
            "height",
            "format",
            "pixel_layout",
            "row_order",
            "bytes_per_pixel",
            "row_stride",
        ),
        "surface",
    )
    _require_value(surface["width"], WIDTH, "surface.width")
    _require_value(surface["height"], HEIGHT, "surface.height")
    surface_format = _require_string(surface["format"], "surface.format")
    if surface_format not in ALLOWED_SURFACE_FORMATS:
        raise ValidationError(f"unsupported surface.format: {surface_format!r}")
    _require_value(surface["pixel_layout"], "BGRA8", "surface.pixel_layout")
    _require_value(surface["row_order"], "top-left", "surface.row_order")
    _require_value(surface["bytes_per_pixel"], 4, "surface.bytes_per_pixel")
    _require_value(surface["row_stride"], WIDTH * 4, "surface.row_stride")

    cursor = _require_object(manifest["cursor"], "cursor")
    _require_exact_keys(cursor, ("x", "y", "policy"), "cursor")
    _require_value(cursor["x"], CURSOR_POINT[0], "cursor.x")
    _require_value(cursor["y"], CURSOR_POINT[1], "cursor.y")
    _require_value(cursor["policy"], "software-composited", "cursor.policy")

    shell = _require_object(manifest["shell"], "shell")
    _require_exact_keys(
        shell,
        (
            "screen",
            "dialog_resource_id",
            "movie_owner",
            "movie_base",
            "title_hidden_during_frames",
        ),
        "shell",
    )
    _require_value(shell["screen"], "main-menu", "shell.screen")
    _require_value(shell["dialog_resource_id"], 0x00E2, "shell.dialog_resource_id")
    _require_value(shell["movie_owner"], "main-menu-0xe2", "shell.movie_owner")
    _require_value(shell["movie_base"], "ra2ts-l", "shell.movie_base")
    _require_value(
        shell["title_hidden_during_frames"],
        True,
        "shell.title_hidden_during_frames",
    )
    _require_value(
        manifest["presenter_domain"],
        "final-swapchain-after-rgb565",
        "presenter_domain",
    )
    generation = _require_int(manifest["generation"], "generation")
    if generation <= 0:
        raise ValidationError("generation must be positive")
    if not _require_bool(manifest["completion_observed"], "completion_observed"):
        raise ValidationError("completion_observed must be true")

    payload = _require_object(manifest["payload"], "payload")
    _require_exact_keys(payload, ("path", "byte_length"), "payload")
    _require_value(payload["path"], PAYLOAD_FILENAME, "payload.path")
    _require_value(
        payload["byte_length"], PAYLOAD_BYTE_LENGTH, "payload.byte_length"
    )

    frames = list(_require_array(manifest["frames"], "frames"))
    if len(frames) != FRAME_COUNT:
        raise ValidationError(
            f"frames must contain exactly {FRAME_COUNT} entries, got {len(frames)}"
        )
    checked: list[Mapping[str, Any]] = []
    for tick, raw_frame in enumerate(frames):
        frame = _require_object(raw_frame, f"frames[{tick}]")
        _require_exact_keys(
            frame, ("tick", "byte_offset", "byte_length"), f"frames[{tick}]"
        )
        _require_value(frame["tick"], tick, f"frames[{tick}].tick")
        _require_value(
            frame["byte_offset"],
            tick * FRAME_BYTE_LENGTH,
            f"frames[{tick}].byte_offset",
        )
        _require_value(
            frame["byte_length"],
            FRAME_BYTE_LENGTH,
            f"frames[{tick}].byte_length",
        )
        checked.append(frame)
    return generation, checked


def validate_entry_sequence_bundle(
    path: str | os.PathLike[str],
) -> dict[str, Any]:
    """Validate one immutable two-file entry-sequence bundle."""

    directory = _require_bundle_directory(Path(path))
    manifest_bytes = _read_regular_bytes(
        directory / MANIFEST_FILENAME,
        "entry-sequence manifest",
        maximum_length=MAX_MANIFEST_BYTES,
    )
    manifest = _parse_json_bytes(manifest_bytes, "entry-sequence manifest")
    generation, frames = _validate_manifest(manifest)
    payload = _read_regular_bytes(
        directory / PAYLOAD_FILENAME,
        "entry-sequence payload",
        exact_length=PAYLOAD_BYTE_LENGTH,
    )
    frame_hashes = [
        sha256_bytes(
            payload[
                frame["byte_offset"] : frame["byte_offset"] + frame["byte_length"]
            ]
        )
        for frame in frames
    ]
    return {
        "schema_version": SCHEMA_VERSION,
        "checkpoint": CHECKPOINT,
        "directory": str(directory),
        "generation": generation,
        "completion_observed": True,
        "manifest_sha256": sha256_bytes(manifest_bytes),
        "payload_sha256": sha256_bytes(payload),
        "frame_sha256": frame_hashes,
    }


def build_entry_sequence_command(
    executable: Path, run_directory: Path
) -> list[str]:
    """Construct the only supported entry-sequence child command."""

    return [
        str(executable),
        "--shell-capture",
        CHECKPOINT,
        "--width",
        str(WIDTH),
        "--height",
        str(HEIGHT),
        "--cursor-x",
        str(CURSOR_POINT[0]),
        "--cursor-y",
        str(CURSOR_POINT[1]),
        "--output",
        str(run_directory),
    ]


def capture_entry_sequence(
    executable_path: str | os.PathLike[str],
    run_directory: str | os.PathLike[str],
    *,
    working_directory: str | os.PathLike[str],
    timeout_seconds: float = DEFAULT_TIMEOUT_SECONDS,
) -> dict[str, Any]:
    """Launch one exact child PID, validate its bundle, and never overwrite."""

    if (
        not isinstance(timeout_seconds, (int, float))
        or isinstance(timeout_seconds, bool)
        or not math.isfinite(float(timeout_seconds))
    ):
        raise ValidationError("timeout must be a finite number of seconds")
    timeout = float(timeout_seconds)
    if not (0.0 < timeout <= MAX_TIMEOUT_SECONDS):
        raise ValidationError(
            f"timeout must be greater than zero and at most {MAX_TIMEOUT_SECONDS:g}"
        )
    executable = absolute_path(executable_path)
    if executable.is_symlink() or not executable.is_file():
        raise ValidationError(
            f"executable is not a regular non-link file: {executable}"
        )
    run_dir = _require_new_run_directory(Path(run_directory))
    child_cwd, config_path, config_hash = _require_child_working_directory(
        working_directory
    )
    executable_hash = sha256_file(executable, "VERA executable")
    command = build_entry_sequence_command(executable, run_dir)

    with tempfile.TemporaryFile(
        mode="w+b", dir=run_dir.parent, prefix=f".{run_dir.name}-stdout-"
    ) as stdout_stream, tempfile.TemporaryFile(
        mode="w+b", dir=run_dir.parent, prefix=f".{run_dir.name}-stderr-"
    ) as stderr_stream:
        try:
            child = subprocess.Popen(
                command,
                stdin=subprocess.DEVNULL,
                stdout=stdout_stream,
                stderr=stderr_stream,
                shell=False,
                cwd=child_cwd,
            )
        except OSError as exc:
            raise ValidationError(f"failed to start entry-sequence child: {exc}") from exc
        timed_out = False
        try:
            child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            try:
                child.kill()
            except OSError as exc:
                raise ValidationError(
                    f"failed to kill timed-out child PID {child.pid}: {exc}"
                ) from exc
            try:
                child.wait(timeout=POST_KILL_DRAIN_SECONDS)
            except subprocess.TimeoutExpired as exc:
                raise ValidationError(
                    f"child PID {child.pid} did not terminate after exact-PID kill"
                ) from exc
        if timed_out:
            raise ValidationError(
                f"entry-sequence child PID {child.pid} exceeded {timeout:g}s timeout"
            )
        if child.returncode != 0:
            raise ValidationError(
                f"entry-sequence child exited with nonzero status {child.returncode}"
            )

    if sha256_file(executable, "VERA executable") != executable_hash:
        raise ValidationError("VERA executable changed during entry-sequence capture")
    if sha256_file(config_path, CONFIG_FILENAME) != config_hash:
        raise ValidationError(f"{CONFIG_FILENAME} changed during capture")
    validation = validate_entry_sequence_bundle(run_dir)
    return {
        "child_pid": child.pid,
        "exit_status": child.returncode,
        "executable_sha256": executable_hash,
        "config_sha256": config_hash,
        "capture": validation,
    }
