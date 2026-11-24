"""Derive an immutable RGB565 presentation profile from sealed shell evidence.

The profile is deliberately scoped to the enrolled guard environment. This
module never edits the guard or its Oracle source runs and never overwrites an
existing output.
"""

from __future__ import annotations

from pathlib import Path

from .core import (
    GUARD_ID,
    GUARD_SCHEMA_VERSION,
    GUARD_STATE,
    SEALED_MAIN_MENU_GUARD_SHA256,
    OutputExistsError,
    ValidationError,
    _read_regular_bytes,
    absolute_path,
    utc_now,
    validate_guard,
    write_json_exclusive,
)
from .native_sources import is_link, load_guard_sources


PROFILE_SCHEMA_VERSION = "vera20k.shell-presentation-profile.v1"
PROFILE_CHECKPOINT = "main-menu-0xe2-rgb565-presentation"
BYTES_PER_PIXEL = 4
EXPECTED_BLUE_CARDINALITY = 32
EXPECTED_GREEN_CARDINALITY = 64
EXPECTED_RED_CARDINALITY = 32
EXPECTED_ALPHA_VALUES = (255,)


def derive_channel_codebooks(
    frame: bytes, width: int, height: int
) -> tuple[tuple[int, ...], tuple[int, ...], tuple[int, ...], tuple[int, ...]]:
    """Return sorted B, G, R, A sets after exact BGRA length validation."""

    if type(width) is not int or type(height) is not int:
        raise ValidationError("frame width and height must be integers")
    if width <= 0 or height <= 0:
        raise ValidationError("frame width and height must be positive")
    expected_length = width * height * BYTES_PER_PIXEL
    if len(frame) != expected_length:
        raise ValidationError(
            f"source frame byte length is {len(frame)}, expected {expected_length}"
        )
    return (
        tuple(sorted(set(frame[0::BYTES_PER_PIXEL]))),
        tuple(sorted(set(frame[1::BYTES_PER_PIXEL]))),
        tuple(sorted(set(frame[2::BYTES_PER_PIXEL]))),
        tuple(sorted(set(frame[3::BYTES_PER_PIXEL]))),
    )


def _require_cardinalities(
    blue: tuple[int, ...],
    green: tuple[int, ...],
    red: tuple[int, ...],
    alpha: tuple[int, ...],
    field: str,
) -> None:
    cardinalities = (len(blue), len(green), len(red))
    expected = (
        EXPECTED_BLUE_CARDINALITY,
        EXPECTED_GREEN_CARDINALITY,
        EXPECTED_RED_CARDINALITY,
    )
    if cardinalities != expected:
        raise ValidationError(
            f"{field} B/G/R cardinalities are {cardinalities}, expected {expected}"
        )
    if alpha != EXPECTED_ALPHA_VALUES:
        raise ValidationError(
            f"{field} alpha values are {alpha}, expected {EXPECTED_ALPHA_VALUES}"
        )


def _merge_source_codebooks(
    expected: tuple[tuple[int, ...], tuple[int, ...], tuple[int, ...]] | None,
    observed: tuple[tuple[int, ...], tuple[int, ...], tuple[int, ...]],
) -> tuple[tuple[int, ...], tuple[int, ...], tuple[int, ...]]:
    if expected is not None and observed != expected:
        raise ValidationError("guard source B/G/R codebooks disagree")
    return observed if expected is None else expected


def derive_presentation_profile(
    guard_path: Path,
    oracle_runs: Path,
    *,
    expected_guard_sha256: str = SEALED_MAIN_MENU_GUARD_SHA256,
) -> dict[str, object]:
    """Validate the sealed guard and all guarded source frames, then report."""

    validated_guard = validate_guard(
        guard_path, expected_sha256=expected_guard_sha256
    )
    source_bundle = load_guard_sources(
        validated_guard,
        oracle_runs,
        read_regular_bytes=_read_regular_bytes,
    )

    expected_codebooks: tuple[
        tuple[int, ...], tuple[int, ...], tuple[int, ...]
    ] | None = None
    output_sources: list[dict[str, str]] = []

    for index, source in enumerate(source_bundle.sources):
        field = f"guard.sources[{index}]"
        blue, green, red, alpha = derive_channel_codebooks(
            source.pixels,
            validated_guard.presentation_width,
            validated_guard.presentation_height,
        )
        _require_cardinalities(blue, green, red, alpha, field)
        codebooks = (blue, green, red)
        expected_codebooks = _merge_source_codebooks(
            expected_codebooks, codebooks
        )

        output_sources.append(
            {
                "run_id": source.run_id,
                "frame_blob": source.frame_blob,
                "frame_pixel_sha256": source.frame_pixel_sha256,
                "surface_blob": source.surface_blob,
                "surface_pixel_sha256": source.surface_pixel_sha256,
            }
        )

    if expected_codebooks is None:
        raise ValidationError("guard source validation produced no profile")
    blue, green, red = expected_codebooks
    if blue != red:
        raise ValidationError("guard source blue and red five-bit codebooks disagree")

    return {
        "schema_version": PROFILE_SCHEMA_VERSION,
        "checkpoint": PROFILE_CHECKPOINT,
        "evidence_status": "DERIVED_FROM_SEALED_NATIVE_SOURCES",
        "parity_certification": "NONE",
        "generated_at_utc": utc_now(),
        "guard": {
            "schema_version": GUARD_SCHEMA_VERSION,
            "guard_id": GUARD_ID,
            "state": GUARD_STATE,
            "path": str(validated_guard.path),
            "sha256": validated_guard.sha256,
        },
        "environment_identity_sha256": (
            source_bundle.environment_identity_sha256
        ),
        "surface": {
            "width": validated_guard.presentation_width,
            "height": validated_guard.presentation_height,
            "format": source_bundle.surface_format,
            "pixel_layout": "BGRA8",
            "bytes_per_pixel": BYTES_PER_PIXEL,
        },
        "sources": output_sources,
        "channel_cardinalities": {
            "blue": len(blue),
            "green": len(green),
            "red": len(red),
            "alpha": len(EXPECTED_ALPHA_VALUES),
        },
        "codebooks": {
            "five_bit": list(blue),
            "six_bit": list(green),
            "channel_mapping": {
                "blue": "five_bit",
                "green": "six_bit",
                "red": "five_bit",
            },
        },
        "alpha_values": list(EXPECTED_ALPHA_VALUES),
        "evidence_scope": (
            "enrolled active retail/AMD/DDrawCompat/DXGI presentation only"
        ),
        "not_certified": [
            "universal DirectDraw expansion",
            "title geometry and final glyph tint",
            "Bink phase and packed-domain blend order",
            "transitions, input, cursor states, audio, or another resolution",
        ],
    }


def write_presentation_profile(
    guard_path: Path,
    oracle_runs: Path,
    output: Path,
    *,
    expected_guard_sha256: str = SEALED_MAIN_MENU_GUARD_SHA256,
) -> dict[str, object]:
    """Write canonical JSON once; reject existing output or link targets."""

    output_path = absolute_path(output)
    if output_path.exists() or is_link(output_path):
        raise OutputExistsError(f"refusing to overwrite output: {output_path}")
    profile = derive_presentation_profile(
        guard_path,
        oracle_runs,
        expected_guard_sha256=expected_guard_sha256,
    )
    write_json_exclusive(output_path, profile)
    return profile
