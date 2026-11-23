"""Fail-closed resolution of the native source frames named by a sealed guard.

The guard and Oracle run tree are immutable inputs.  Resolution rejects links,
traversal, non-regular files, unstable reads, digest disagreement, duplicate
source identities, and mixed environment identities before returning bytes.
"""

from __future__ import annotations

import stat
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Mapping

from .core import (
    BYTES_PER_PIXEL,
    ValidatedGuard,
    ValidationError,
    parse_json_bytes,
    _read_regular_bytes,
    _require_array,
    _require_keys,
    _require_object,
    _require_sha256,
    _require_string,
    absolute_path,
    sha256_bytes,
)


SOURCE_COUNT = 3
EXPECTED_SURFACE_FORMAT = "B8G8R8A8_UNORM"
MAX_GUARD_BYTES = 4 * 1024 * 1024

StableReader = Callable[..., bytes]


@dataclass(frozen=True)
class GuardSourceFrame:
    """One validated presentation-surface frame named by the sealed guard."""

    run_id: str
    frame_blob: str
    frame_pixel_sha256: str
    surface_blob: str
    surface_pixel_sha256: str
    environment_identity_sha256: str
    path: Path
    pixels: bytes


@dataclass(frozen=True)
class GuardSourceBundle:
    """Validated guard document plus its three comparable native frames."""

    guard: ValidatedGuard
    document: Mapping[str, Any]
    surface_format: str
    environment_identity_sha256: str
    sources: tuple[GuardSourceFrame, ...]


def is_link(path: Path) -> bool:
    """Recognize both symbolic links and Windows directory junctions."""

    try:
        if path.is_symlink():
            return True
        is_junction = getattr(path, "is_junction", None)
        return bool(is_junction and is_junction())
    except OSError as exc:
        raise ValidationError(f"cannot inspect path for links {path}: {exc}") from exc


def _require_oracle_runs_root(path: Path) -> Path:
    root = absolute_path(path)
    if is_link(root):
        raise ValidationError(f"Oracle runs root must not be a link: {root}")
    try:
        metadata = root.stat()
    except FileNotFoundError as exc:
        raise ValidationError(f"Oracle runs root does not exist: {root}") from exc
    except OSError as exc:
        raise ValidationError(f"cannot stat Oracle runs root {root}: {exc}") from exc
    if not stat.S_ISDIR(metadata.st_mode):
        raise ValidationError(f"Oracle runs root is not a directory: {root}")
    return root


def _relative_guard_path(value: Any, field: str, *, one_component: bool) -> Path:
    text = _require_string(value, field)
    if not text:
        raise ValidationError(f"{field} must not be empty")
    path = Path(text)
    parts = path.parts
    if (
        path.is_absolute()
        or bool(path.drive)
        or not parts
        or any(part in ("", ".", "..") for part in parts)
    ):
        raise ValidationError(f"{field} must be a traversal-free relative path")
    if one_component and len(parts) != 1:
        raise ValidationError(f"{field} must be exactly one path component")
    return path


def _validate_source_path(root: Path, relative: Path, field: str) -> Path:
    """Reject links and prove an existing source remains beneath the runs root."""

    candidate = root / relative
    cursor = root
    for component in relative.parts:
        cursor = cursor / component
        if is_link(cursor):
            raise ValidationError(f"{field} must not traverse a link: {cursor}")

    try:
        resolved_root = root.resolve(strict=True)
        resolved_candidate = candidate.resolve(strict=True)
    except FileNotFoundError as exc:
        raise ValidationError(f"{field} does not exist: {candidate}") from exc
    except OSError as exc:
        raise ValidationError(f"cannot resolve {field} {candidate}: {exc}") from exc
    try:
        resolved_candidate.relative_to(resolved_root)
    except ValueError as exc:
        raise ValidationError(
            f"{field} escapes the Oracle runs root: {candidate}"
        ) from exc
    return candidate


def _load_guard_document(
    guard: ValidatedGuard, read_regular_bytes: StableReader
) -> Mapping[str, Any]:
    raw = read_regular_bytes(
        guard.path, "native shell guard", maximum_length=MAX_GUARD_BYTES
    )
    actual_sha256 = sha256_bytes(raw)
    if actual_sha256 != guard.sha256:
        raise ValidationError(
            "native shell guard changed after validation: "
            f"got {actual_sha256}, expected {guard.sha256}"
        )
    return parse_json_bytes(raw, "native shell guard")


def _source_records(document: Mapping[str, Any]) -> tuple[Mapping[str, Any], ...]:
    values = _require_array(document.get("sources"), "guard.sources")
    if len(values) != SOURCE_COUNT:
        raise ValidationError(
            f"guard.sources has {len(values)} entries, expected {SOURCE_COUNT}"
        )
    records: list[Mapping[str, Any]] = []
    for index, value in enumerate(values):
        field = f"guard.sources[{index}]"
        source = _require_object(value, field)
        _require_keys(
            source,
            (
                "run_id",
                "frame_blob",
                "frame_pixel_sha256",
                "surface_blob",
                "surface_pixel_sha256",
                "environment_identity_sha256",
            ),
            field,
        )
        records.append(source)
    return tuple(records)


def _surface_format(document: Mapping[str, Any]) -> str:
    environment = _require_object(
        document.get("environment_identity"), "guard.environment_identity"
    )
    presentation = _require_object(
        environment.get("presentation_configuration"),
        "guard.environment_identity.presentation_configuration",
    )
    surface_format = _require_string(
        presentation.get("surface_format"),
        "guard.environment_identity.presentation_configuration.surface_format",
    )
    if surface_format != EXPECTED_SURFACE_FORMAT:
        raise ValidationError(
            "guard presentation surface format is "
            f"{surface_format!r}, expected {EXPECTED_SURFACE_FORMAT!r}"
        )
    return surface_format


def load_guard_sources(
    guard: ValidatedGuard,
    oracle_runs: Path,
    *,
    read_regular_bytes: StableReader = _read_regular_bytes,
) -> GuardSourceBundle:
    """Resolve and validate all native presentation frames named by ``guard``."""

    document = _load_guard_document(guard, read_regular_bytes)
    records = _source_records(document)
    surface_format = _surface_format(document)
    runs_root = _require_oracle_runs_root(oracle_runs)

    environment_identity_sha256: str | None = None
    observed_run_ids: set[str] = set()
    observed_blob_paths: set[str] = set()
    sources: list[GuardSourceFrame] = []
    expected_length = (
        guard.presentation_width
        * guard.presentation_height
        * BYTES_PER_PIXEL
    )

    for index, source in enumerate(records):
        field = f"guard.sources[{index}]"
        run_path = _relative_guard_path(
            source["run_id"], f"{field}.run_id", one_component=True
        )
        frame_blob_text = _require_string(
            source["frame_blob"], f"{field}.frame_blob"
        )
        surface_blob_text = _require_string(
            source["surface_blob"], f"{field}.surface_blob"
        )
        frame_blob_path = _relative_guard_path(
            frame_blob_text, f"{field}.frame_blob", one_component=False
        )
        surface_blob_path = _relative_guard_path(
            surface_blob_text, f"{field}.surface_blob", one_component=False
        )
        if frame_blob_path != surface_blob_path:
            raise ValidationError(f"{field} frame_blob and surface_blob disagree")

        run_id = str(run_path)
        source_identity = str(run_path / surface_blob_path)
        if run_id in observed_run_ids:
            raise ValidationError(f"guard has duplicate source run_id: {run_id}")
        if source_identity in observed_blob_paths:
            raise ValidationError(
                f"guard has duplicate source path: {source_identity}"
            )
        observed_run_ids.add(run_id)
        observed_blob_paths.add(source_identity)

        frame_sha256 = _require_sha256(
            source["frame_pixel_sha256"], f"{field}.frame_pixel_sha256"
        )
        surface_sha256 = _require_sha256(
            source["surface_pixel_sha256"], f"{field}.surface_pixel_sha256"
        )
        if frame_sha256 != surface_sha256:
            raise ValidationError(
                f"{field} frame and presentation-surface pixel digests disagree"
            )

        source_environment_sha256 = _require_sha256(
            source["environment_identity_sha256"],
            f"{field}.environment_identity_sha256",
        )
        if environment_identity_sha256 is None:
            environment_identity_sha256 = source_environment_sha256
        elif source_environment_sha256 != environment_identity_sha256:
            raise ValidationError(
                "guard source environment identity digests disagree"
            )

        relative_source = run_path / surface_blob_path
        source_path = _validate_source_path(
            runs_root, relative_source, f"{field} source frame"
        )
        pixels = read_regular_bytes(
            source_path,
            f"{field} source frame",
            exact_length=expected_length,
        )
        # Recheck the ancestry after the stable file read. A link swap must not
        # redirect either a later source or the identity recorded in the report.
        _validate_source_path(
            runs_root, relative_source, f"{field} source frame"
        )
        actual_sha256 = sha256_bytes(pixels)
        if actual_sha256 != surface_sha256:
            raise ValidationError(
                f"{field} source frame SHA-256 mismatch: "
                f"got {actual_sha256}, expected {surface_sha256}"
            )

        sources.append(
            GuardSourceFrame(
                run_id=run_id,
                frame_blob=frame_blob_text,
                frame_pixel_sha256=frame_sha256,
                surface_blob=surface_blob_text,
                surface_pixel_sha256=actual_sha256,
                environment_identity_sha256=source_environment_sha256,
                path=source_path,
                pixels=pixels,
            )
        )

    if environment_identity_sha256 is None:
        raise ValidationError("guard source validation produced no sources")
    return GuardSourceBundle(
        guard=guard,
        document=document,
        surface_format=surface_format,
        environment_identity_sha256=environment_identity_sha256,
        sources=tuple(sources),
    )
