"""Validation and comparison for one-shot VERA20k shell captures.

The native guard is treated as immutable evidence. This module only reads it,
validates its sealed content hash and schema, point-scales the full logical
production frame, and compares tight physical presentation-region BGRA8 bytes.

JSON parsing and writing, digests and the error types are the tactical owner's
(`tools.tactical_certification.core`): one strict policy (unique keys, finite
numbers, no NaN output) for every capture tool. File reads differ on purpose:
shell inputs reject a final link only, because guards, configs and bundles are
read where they are written, including linked checkouts and temporary
directories under a linked root (/tmp, /var on macOS); tactical sealed runs
reject every link ancestor.
"""

from __future__ import annotations

import hashlib
import os
import stat
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence

from tools.tactical_certification.core import (
    OutputExistsError,
    ValidationError,
    absolute_path,
    canonical_json_bytes,
    parse_json_bytes,
    sha256_bytes,
    utc_now,
)


MATCH = "MATCH"
DRIFT = "DRIFT"
INVALID = "INVALID"

CAPTURE_SCHEMA_VERSION_V1 = "vera20k.shell-capture.v1"
CAPTURE_SCHEMA_VERSION = "vera20k.shell-capture.v2"
COMPARISON_SCHEMA_VERSION = "vera20k.shell-comparison.v1"
GUARD_SCHEMA_VERSION = "vera20k.shell-presentation-guard.v4"
SEALED_MAIN_MENU_GUARD_SHA256 = (
    "fe32f218137b76a91dc3bac07bc96372a61c22e48ea083519f1ecbdbd97d601c"
)

CHECKPOINT = "main-menu-0xe2-steady"
GUARD_ID = "main-menu-0xe2"
GUARD_STATE = "main_menu_0xe2"
LOGICAL_WIDTH = 800
LOGICAL_HEIGHT = 600
PRESENTATION_WIDTH = 1920
PRESENTATION_HEIGHT = 1080
CONTENT_RECT = (240, 0, 1680, 1080)
SCALE_NUMERATOR = 9
SCALE_DENOMINATOR = 5
MAPPING_KIND = "ddrawcompat.integer-800x600-to-1920x1080.v1"
BYTES_PER_PIXEL = 4
DIALOG_RESOURCE_ID = 226
CURSOR_POINT = (400, 300)
FRAME_FILENAME = "frame.bgra"
CAPTURE_MANIFEST_FILENAME = "capture.json"

ALLOWED_SURFACE_FORMATS = frozenset(("Bgra8Unorm", "Bgra8UnormSrgb"))

EXPECTED_REGION_RECTS: Mapping[str, tuple[int, int, int, int]] = {
    "title": (635, 9, 798, 27),
    "single_player": (644, 199, 800, 241),
    "options": (644, 367, 800, 409),
    "exit_game": (644, 535, 800, 577),
}
EXPECTED_PRESENTATION_REGION_RECTS: Mapping[
    str, tuple[int, int, int, int]
] = {
    "title": (1383, 16, 1676, 48),
    "single_player": (1399, 358, 1680, 433),
    "options": (1399, 660, 1680, 736),
    "exit_game": (1399, 963, 1680, 1038),
}
EXPECTED_REGION_ORDER = tuple(EXPECTED_REGION_RECTS)

_HEX_DIGITS = frozenset("0123456789abcdef")
_MAX_JSON_BYTES = 4 * 1024 * 1024


@dataclass(frozen=True)
class Rect:
    """A half-open rectangle in its explicitly named coordinate domain."""

    left: int
    top: int
    right: int
    bottom: int

    def as_dict(self) -> dict[str, int]:
        return {
            "left": self.left,
            "top": self.top,
            "right": self.right,
            "bottom": self.bottom,
        }


@dataclass(frozen=True)
class GuardRegion:
    """One sealed native comparison region."""

    name: str
    logical_rect: Rect
    presentation_rect: Rect
    expected_presentation_sha256: str


@dataclass(frozen=True)
class ValidatedGuard:
    """Validated immutable native guard metadata."""

    path: Path
    sha256: str
    width: int
    height: int
    presentation_width: int
    presentation_height: int
    content_rect: Rect
    scale_numerator: int
    scale_denominator: int
    cursor_x: int
    cursor_y: int
    regions: tuple[GuardRegion, ...]


@dataclass(frozen=True)
class ValidatedCapture:
    """Validated Rust capture manifest and raw frame."""

    directory: Path
    manifest_path: Path
    manifest_sha256: str
    manifest: Mapping[str, Any]
    frame_path: Path
    frame_sha256: str
    frame_bytes: bytes
    width: int
    height: int
    surface_format: str
    cursor_x: int
    cursor_y: int


def _require_regular_file(path: Path, label: str) -> os.stat_result:
    try:
        if path.is_symlink():
            raise ValidationError(f"{label} must not be a symbolic link: {path}")
        metadata = path.stat()
    except FileNotFoundError as exc:
        raise ValidationError(f"{label} does not exist: {path}") from exc
    except OSError as exc:
        raise ValidationError(f"cannot stat {label} {path}: {exc}") from exc
    if not stat.S_ISREG(metadata.st_mode):
        raise ValidationError(f"{label} is not a regular file: {path}")
    return metadata


def _read_regular_bytes(
    path: Path,
    label: str,
    *,
    maximum_length: int | None = None,
    exact_length: int | None = None,
) -> bytes:
    """Read one stable regular file and reject links or concurrent mutation."""

    before_path = _require_regular_file(path, label)
    if maximum_length is not None and before_path.st_size > maximum_length:
        raise ValidationError(
            f"{label} is too large: {before_path.st_size} > {maximum_length}"
        )
    if exact_length is not None and before_path.st_size != exact_length:
        raise ValidationError(
            f"{label} byte length is {before_path.st_size}, expected {exact_length}"
        )

    try:
        with path.open("rb") as stream:
            before_handle = os.fstat(stream.fileno())
            data = stream.read()
            after_handle = os.fstat(stream.fileno())
    except OSError as exc:
        raise ValidationError(f"cannot read {label} {path}: {exc}") from exc

    try:
        after_path = path.stat()
    except OSError as exc:
        raise ValidationError(f"cannot re-stat {label} {path}: {exc}") from exc

    identity_before = (
        before_path.st_dev,
        before_path.st_ino,
        before_path.st_size,
        before_path.st_mtime_ns,
    )
    identity_handle_before = (
        before_handle.st_dev,
        before_handle.st_ino,
        before_handle.st_size,
        before_handle.st_mtime_ns,
    )
    identity_handle_after = (
        after_handle.st_dev,
        after_handle.st_ino,
        after_handle.st_size,
        after_handle.st_mtime_ns,
    )
    identity_after = (
        after_path.st_dev,
        after_path.st_ino,
        after_path.st_size,
        after_path.st_mtime_ns,
    )
    if not (
        identity_before
        == identity_handle_before
        == identity_handle_after
        == identity_after
    ):
        raise ValidationError(f"{label} changed while it was being validated: {path}")
    if len(data) != before_handle.st_size:
        raise ValidationError(
            f"{label} short read: got {len(data)} bytes, expected {before_handle.st_size}"
        )
    if maximum_length is not None and len(data) > maximum_length:
        raise ValidationError(f"{label} exceeds the maximum accepted byte length")
    if exact_length is not None and len(data) != exact_length:
        raise ValidationError(
            f"{label} byte length is {len(data)}, expected {exact_length}"
        )
    return data


def sha256_file(path: Path, label: str = "file") -> str:
    """Hash a stable regular file without following a symlink."""

    before_path = _require_regular_file(path, label)
    digest = hashlib.sha256()
    try:
        with path.open("rb") as stream:
            before_handle = os.fstat(stream.fileno())
            while chunk := stream.read(1024 * 1024):
                digest.update(chunk)
            after_handle = os.fstat(stream.fileno())
        after_path = path.stat()
    except OSError as exc:
        raise ValidationError(f"cannot hash {label} {path}: {exc}") from exc

    identities = (
        (
            before_path.st_dev,
            before_path.st_ino,
            before_path.st_size,
            before_path.st_mtime_ns,
        ),
        (
            before_handle.st_dev,
            before_handle.st_ino,
            before_handle.st_size,
            before_handle.st_mtime_ns,
        ),
        (
            after_handle.st_dev,
            after_handle.st_ino,
            after_handle.st_size,
            after_handle.st_mtime_ns,
        ),
        (
            after_path.st_dev,
            after_path.st_ino,
            after_path.st_size,
            after_path.st_mtime_ns,
        ),
    )
    if len(set(identities)) != 1:
        raise ValidationError(f"{label} changed while it was being hashed: {path}")
    return digest.hexdigest()


def _require_object(value: Any, field: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise ValidationError(f"{field} must be an object")
    return value


def _require_array(value: Any, field: str) -> Sequence[Any]:
    if not isinstance(value, list):
        raise ValidationError(f"{field} must be an array")
    return value


def _require_string(value: Any, field: str) -> str:
    if not isinstance(value, str):
        raise ValidationError(f"{field} must be a string")
    return value


def _require_int(value: Any, field: str) -> int:
    if type(value) is not int:
        raise ValidationError(f"{field} must be an integer")
    return value


def _require_bool(value: Any, field: str) -> bool:
    if type(value) is not bool:
        raise ValidationError(f"{field} must be a boolean")
    return value


def _require_value(value: Any, expected: Any, field: str) -> None:
    if type(value) is not type(expected) or value != expected:
        raise ValidationError(f"{field} is {value!r}, expected {expected!r}")


def _require_exact_keys(
    value: Mapping[str, Any], expected: Iterable[str], field: str
) -> None:
    expected_set = set(expected)
    actual_set = set(value)
    missing = sorted(expected_set - actual_set)
    extra = sorted(actual_set - expected_set)
    if missing or extra:
        details: list[str] = []
        if missing:
            details.append(f"missing={missing}")
        if extra:
            details.append(f"unexpected={extra}")
        raise ValidationError(f"{field} keys are invalid ({', '.join(details)})")


def _require_keys(
    value: Mapping[str, Any], required: Iterable[str], field: str
) -> None:
    missing = sorted(set(required) - set(value))
    if missing:
        raise ValidationError(f"{field} is missing required keys: {missing}")


def _require_sha256(value: Any, field: str) -> str:
    digest = _require_string(value, field)
    if len(digest) != 64 or any(character not in _HEX_DIGITS for character in digest):
        raise ValidationError(f"{field} must be a lowercase SHA-256 digest")
    return digest


def _rect_from_mapping(value: Any, field: str) -> Rect:
    rectangle = _require_object(value, field)
    _require_keys(rectangle, ("left", "top", "right", "bottom"), field)
    return Rect(
        left=_require_int(rectangle["left"], f"{field}.left"),
        top=_require_int(rectangle["top"], f"{field}.top"),
        right=_require_int(rectangle["right"], f"{field}.right"),
        bottom=_require_int(rectangle["bottom"], f"{field}.bottom"),
    )


def _validate_rect(rect: Rect, width: int, height: int, field: str) -> None:
    if not (0 <= rect.left < rect.right <= width):
        raise ValidationError(
            f"{field} has invalid horizontal half-open bounds: {rect.as_dict()}"
        )
    if not (0 <= rect.top < rect.bottom <= height):
        raise ValidationError(
            f"{field} has invalid vertical half-open bounds: {rect.as_dict()}"
        )


def crop_tight_bgra(
    frame: bytes | bytearray | memoryview,
    width: int,
    height: int,
    rect: Rect | tuple[int, int, int, int],
) -> bytes:
    """Extract a half-open rectangle from tight top-left BGRA8 rows."""

    width = _require_int(width, "width")
    height = _require_int(height, "height")
    if width <= 0 or height <= 0:
        raise ValidationError("frame dimensions must be positive")
    if isinstance(rect, tuple):
        if len(rect) != 4:
            raise ValidationError("rectangle tuple must contain four integers")
        rectangle = Rect(
            _require_int(rect[0], "rect.left"),
            _require_int(rect[1], "rect.top"),
            _require_int(rect[2], "rect.right"),
            _require_int(rect[3], "rect.bottom"),
        )
    elif isinstance(rect, Rect):
        rectangle = rect
    else:
        raise ValidationError("rectangle must be a Rect or four-integer tuple")
    _validate_rect(rectangle, width, height, "rect")

    expected_length = width * height * BYTES_PER_PIXEL
    view = memoryview(frame).cast("B")
    if view.nbytes != expected_length:
        raise ValidationError(
            f"frame byte length is {view.nbytes}, expected {expected_length}"
        )

    row_bytes = width * BYTES_PER_PIXEL
    crop_row_bytes = (rectangle.right - rectangle.left) * BYTES_PER_PIXEL
    result = bytearray(crop_row_bytes * (rectangle.bottom - rectangle.top))
    destination = 0
    for y in range(rectangle.top, rectangle.bottom):
        source = y * row_bytes + rectangle.left * BYTES_PER_PIXEL
        result[destination : destination + crop_row_bytes] = view[
            source : source + crop_row_bytes
        ]
        destination += crop_row_bytes
    return bytes(result)


def point_scale_presentation_crop_bgra(
    frame: bytes | bytearray | memoryview,
    logical_width: int,
    logical_height: int,
    content_rect: Rect,
    presentation_rect: Rect,
    scale_numerator: int,
    scale_denominator: int,
) -> bytes:
    """Point-scale a logical BGRA8 frame into one tight presentation crop.

    DDrawCompat's verified point mapping selects a logical source pixel at
    ``floor((2*d + 1) * denominator / (2*numerator))`` for each physical
    destination coordinate ``d`` relative to the content rectangle. No direct
    logical crop digest is comparable to the sealed presentation digest.
    """

    width = _require_int(logical_width, "logical_width")
    height = _require_int(logical_height, "logical_height")
    numerator = _require_int(scale_numerator, "scale_numerator")
    denominator = _require_int(scale_denominator, "scale_denominator")
    if width <= 0 or height <= 0:
        raise ValidationError("logical frame dimensions must be positive")
    if numerator <= 0 or denominator <= 0:
        raise ValidationError("point-scale numerator and denominator must be positive")
    if not isinstance(content_rect, Rect):
        raise ValidationError("content_rect must be a Rect")
    if not isinstance(presentation_rect, Rect):
        raise ValidationError("presentation_rect must be a Rect")
    if not (
        0 <= content_rect.left < content_rect.right
        and 0 <= content_rect.top < content_rect.bottom
    ):
        raise ValidationError(f"content_rect is invalid: {content_rect.as_dict()}")

    content_width = content_rect.right - content_rect.left
    content_height = content_rect.bottom - content_rect.top
    if content_width * denominator != width * numerator:
        raise ValidationError(
            "content width is inconsistent with logical width and point scale"
        )
    if content_height * denominator != height * numerator:
        raise ValidationError(
            "content height is inconsistent with logical height and point scale"
        )
    if not (
        content_rect.left <= presentation_rect.left < presentation_rect.right
        <= content_rect.right
        and content_rect.top <= presentation_rect.top < presentation_rect.bottom
        <= content_rect.bottom
    ):
        raise ValidationError(
            "presentation_rect must be a non-empty subset of content_rect"
        )

    expected_length = width * height * BYTES_PER_PIXEL
    try:
        source = memoryview(frame).cast("B")
    except (TypeError, ValueError) as exc:
        raise ValidationError("frame must be contiguous byte-addressable data") from exc
    if source.nbytes != expected_length:
        raise ValidationError(
            f"frame byte length is {source.nbytes}, expected {expected_length}"
        )

    crop_width = presentation_rect.right - presentation_rect.left
    crop_height = presentation_rect.bottom - presentation_rect.top
    output = bytearray(crop_width * crop_height * BYTES_PER_PIXEL)
    output_offset = 0
    point_denominator = 2 * numerator
    for presentation_y in range(
        presentation_rect.top, presentation_rect.bottom
    ):
        destination_y = presentation_y - content_rect.top
        source_y = (
            (2 * destination_y + 1) * denominator // point_denominator
        )
        if not 0 <= source_y < height:
            raise ValidationError("point-scale y mapping escaped the logical frame")
        for presentation_x in range(
            presentation_rect.left, presentation_rect.right
        ):
            destination_x = presentation_x - content_rect.left
            source_x = (
                (2 * destination_x + 1) * denominator // point_denominator
            )
            if not 0 <= source_x < width:
                raise ValidationError("point-scale x mapping escaped the logical frame")
            source_offset = (
                (source_y * width + source_x) * BYTES_PER_PIXEL
            )
            output[output_offset : output_offset + BYTES_PER_PIXEL] = source[
                source_offset : source_offset + BYTES_PER_PIXEL
            ]
            output_offset += BYTES_PER_PIXEL
    return bytes(output)


def _map_logical_rect_to_presentation(
    logical_rect: Rect,
    content_rect: Rect,
    numerator: int,
    denominator: int,
) -> Rect:
    """Map logical half-open boundaries into physical presentation boundaries."""

    return Rect(
        left=content_rect.left + logical_rect.left * numerator // denominator,
        top=content_rect.top + logical_rect.top * numerator // denominator,
        right=content_rect.left + logical_rect.right * numerator // denominator,
        bottom=content_rect.top + logical_rect.bottom * numerator // denominator,
    )


def cursor_overlaps_rect(
    cursor_x: int, cursor_y: int, rect: Rect | tuple[int, int, int, int]
) -> bool:
    """Return whether a cursor point lies inside a half-open logical region."""

    x = _require_int(cursor_x, "cursor.x")
    y = _require_int(cursor_y, "cursor.y")
    if isinstance(rect, tuple):
        if len(rect) != 4:
            raise ValidationError("rectangle tuple must contain four integers")
        rectangle = Rect(*(_require_int(v, "rect coordinate") for v in rect))
    elif isinstance(rect, Rect):
        rectangle = rect
    else:
        raise ValidationError("rectangle must be a Rect or four-integer tuple")
    return (
        rectangle.left <= x < rectangle.right
        and rectangle.top <= y < rectangle.bottom
    )


def validate_guard(
    guard_path: str | os.PathLike[str],
    *,
    expected_sha256: str = SEALED_MAIN_MENU_GUARD_SHA256,
) -> ValidatedGuard:
    """Validate the exact sealed 0xE2 native presentation-surface guard."""

    path = absolute_path(guard_path)
    raw = _read_regular_bytes(
        path, "native shell guard", maximum_length=_MAX_JSON_BYTES
    )
    actual_digest = sha256_bytes(raw)
    expected_digest = _require_sha256(expected_sha256, "expected guard SHA-256")
    if actual_digest != expected_digest:
        raise ValidationError(
            "native shell guard SHA-256 mismatch: "
            f"got {actual_digest}, expected {expected_digest}"
        )
    document = parse_json_bytes(raw, "native shell guard")
    _require_keys(
        document,
        (
            "schema_version",
            "guard_id",
            "state",
            "mapping",
            "neutral_cursor",
            "regions",
            "signature_identity",
            "environment_identity",
        ),
        "guard",
    )
    _require_value(
        document["schema_version"], GUARD_SCHEMA_VERSION, "guard.schema_version"
    )
    _require_value(document["guard_id"], GUARD_ID, "guard.guard_id")
    _require_value(document["state"], GUARD_STATE, "guard.state")

    mapping = _require_object(document["mapping"], "guard.mapping")
    _require_exact_keys(
        mapping,
        (
            "kind",
            "logical_width",
            "logical_height",
            "presentation_width",
            "presentation_height",
            "content_rect",
            "scale_numerator",
            "scale_denominator",
        ),
        "guard.mapping",
    )
    _require_value(mapping["kind"], MAPPING_KIND, "guard.mapping.kind")
    width = _require_int(mapping["logical_width"], "guard.mapping.logical_width")
    height = _require_int(mapping["logical_height"], "guard.mapping.logical_height")
    _require_value(width, LOGICAL_WIDTH, "guard.mapping.logical_width")
    _require_value(height, LOGICAL_HEIGHT, "guard.mapping.logical_height")
    presentation_width = _require_int(
        mapping["presentation_width"], "guard.mapping.presentation_width"
    )
    presentation_height = _require_int(
        mapping["presentation_height"], "guard.mapping.presentation_height"
    )
    _require_value(
        presentation_width,
        PRESENTATION_WIDTH,
        "guard.mapping.presentation_width",
    )
    _require_value(
        presentation_height,
        PRESENTATION_HEIGHT,
        "guard.mapping.presentation_height",
    )
    content_rect = _rect_from_mapping(
        mapping["content_rect"], "guard.mapping.content_rect"
    )
    _validate_rect(
        content_rect,
        presentation_width,
        presentation_height,
        "guard.mapping.content_rect",
    )
    actual_content_rect = (
        content_rect.left,
        content_rect.top,
        content_rect.right,
        content_rect.bottom,
    )
    if actual_content_rect != CONTENT_RECT:
        raise ValidationError(
            "guard.mapping.content_rect is "
            f"{actual_content_rect}, expected {CONTENT_RECT}"
        )
    scale_numerator = _require_int(
        mapping["scale_numerator"], "guard.mapping.scale_numerator"
    )
    scale_denominator = _require_int(
        mapping["scale_denominator"], "guard.mapping.scale_denominator"
    )
    _require_value(
        scale_numerator, SCALE_NUMERATOR, "guard.mapping.scale_numerator"
    )
    _require_value(
        scale_denominator, SCALE_DENOMINATOR, "guard.mapping.scale_denominator"
    )
    if (
        (content_rect.right - content_rect.left) * scale_denominator
        != width * scale_numerator
        or (content_rect.bottom - content_rect.top) * scale_denominator
        != height * scale_numerator
    ):
        raise ValidationError(
            "guard mapping dimensions are inconsistent with its exact scale"
        )

    signature = _require_object(
        document["signature_identity"], "guard.signature_identity"
    )
    _require_keys(signature, ("dialog_resource_id",), "guard.signature_identity")
    _require_value(
        signature["dialog_resource_id"],
        DIALOG_RESOURCE_ID,
        "guard.signature_identity.dialog_resource_id",
    )

    environment = _require_object(
        document["environment_identity"], "guard.environment_identity"
    )
    _require_keys(
        environment, ("presentation_configuration",), "guard.environment_identity"
    )
    presentation = _require_object(
        environment["presentation_configuration"],
        "guard.environment_identity.presentation_configuration",
    )
    _require_keys(
        presentation,
        ("surface_format",),
        "guard.environment_identity.presentation_configuration",
    )
    _require_value(
        presentation["surface_format"],
        "B8G8R8A8_UNORM",
        "guard.environment_identity.presentation_configuration.surface_format",
    )

    cursor = _require_object(document["neutral_cursor"], "guard.neutral_cursor")
    _require_keys(cursor, ("input_client_point",), "guard.neutral_cursor")
    cursor_point = _require_object(
        cursor["input_client_point"], "guard.neutral_cursor.input_client_point"
    )
    _require_keys(
        cursor_point, ("x", "y"), "guard.neutral_cursor.input_client_point"
    )
    cursor_x = _require_int(
        cursor_point["x"], "guard.neutral_cursor.input_client_point.x"
    )
    cursor_y = _require_int(
        cursor_point["y"], "guard.neutral_cursor.input_client_point.y"
    )
    _require_value(cursor_x, CURSOR_POINT[0], "guard neutral cursor x")
    _require_value(cursor_y, CURSOR_POINT[1], "guard neutral cursor y")

    region_values = _require_array(document["regions"], "guard.regions")
    if len(region_values) != len(EXPECTED_REGION_ORDER):
        raise ValidationError(
            f"guard.regions has {len(region_values)} entries, "
            f"expected {len(EXPECTED_REGION_ORDER)}"
        )
    regions: list[GuardRegion] = []
    observed_names: list[str] = []
    for index, raw_region in enumerate(region_values):
        field = f"guard.regions[{index}]"
        region = _require_object(raw_region, field)
        _require_keys(
            region,
            (
                "name",
                "logical_rect",
                "presentation_rect",
                "raw_bgra_sha256",
                "surface_raw_bgra_sha256",
            ),
            field,
        )
        name = _require_string(region["name"], f"{field}.name")
        if name in observed_names:
            raise ValidationError(f"guard has duplicate region name: {name}")
        observed_names.append(name)
        if name not in EXPECTED_REGION_RECTS:
            raise ValidationError(f"guard has unsupported region name: {name}")
        logical_rect = _rect_from_mapping(
            region["logical_rect"], f"{field}.logical_rect"
        )
        _validate_rect(logical_rect, width, height, f"{field}.logical_rect")
        expected_tuple = EXPECTED_REGION_RECTS[name]
        actual_tuple = (
            logical_rect.left,
            logical_rect.top,
            logical_rect.right,
            logical_rect.bottom,
        )
        if actual_tuple != expected_tuple:
            raise ValidationError(
                f"{field}.logical_rect is {actual_tuple}, expected {expected_tuple}"
            )
        presentation_rect = _rect_from_mapping(
            region["presentation_rect"], f"{field}.presentation_rect"
        )
        _validate_rect(
            presentation_rect,
            presentation_width,
            presentation_height,
            f"{field}.presentation_rect",
        )
        expected_presentation_tuple = EXPECTED_PRESENTATION_REGION_RECTS[name]
        actual_presentation_tuple = (
            presentation_rect.left,
            presentation_rect.top,
            presentation_rect.right,
            presentation_rect.bottom,
        )
        if actual_presentation_tuple != expected_presentation_tuple:
            raise ValidationError(
                f"{field}.presentation_rect is {actual_presentation_tuple}, "
                f"expected {expected_presentation_tuple}"
            )
        mapped_presentation_rect = _map_logical_rect_to_presentation(
            logical_rect,
            content_rect,
            scale_numerator,
            scale_denominator,
        )
        if presentation_rect != mapped_presentation_rect:
            raise ValidationError(
                f"{field}.presentation_rect is inconsistent with logical_rect "
                "and the sealed point-scale mapping"
            )
        surface_digest = _require_sha256(
            region["surface_raw_bgra_sha256"],
            f"{field}.surface_raw_bgra_sha256",
        )
        raw_digest = _require_sha256(
            region["raw_bgra_sha256"], f"{field}.raw_bgra_sha256"
        )
        if raw_digest != surface_digest:
            raise ValidationError(
                f"{field} raw and presentation-surface digests disagree"
            )
        regions.append(
            GuardRegion(
                name,
                logical_rect,
                presentation_rect,
                surface_digest,
            )
        )
    if tuple(observed_names) != EXPECTED_REGION_ORDER:
        raise ValidationError(
            f"guard region order is {tuple(observed_names)}, "
            f"expected {EXPECTED_REGION_ORDER}"
        )

    for region in regions:
        if cursor_overlaps_rect(cursor_x, cursor_y, region.logical_rect):
            raise ValidationError(
                f"guard neutral cursor overlaps compared region {region.name}"
            )

    return ValidatedGuard(
        path=path,
        sha256=actual_digest,
        width=width,
        height=height,
        presentation_width=presentation_width,
        presentation_height=presentation_height,
        content_rect=content_rect,
        scale_numerator=scale_numerator,
        scale_denominator=scale_denominator,
        cursor_x=cursor_x,
        cursor_y=cursor_y,
        regions=tuple(regions),
    )


def _validate_capture_manifest(document: Mapping[str, Any]) -> dict[str, Any]:
    _require_exact_keys(
        document,
        ("schema_version", "checkpoint", "surface", "cursor", "shell", "frame"),
        "capture",
    )
    schema_version = _require_string(
        document["schema_version"], "capture.schema_version"
    )
    if schema_version not in (CAPTURE_SCHEMA_VERSION_V1, CAPTURE_SCHEMA_VERSION):
        raise ValidationError(
            "capture.schema_version must be "
            f"{CAPTURE_SCHEMA_VERSION_V1!r} or {CAPTURE_SCHEMA_VERSION!r}, "
            f"got {schema_version!r}"
        )
    _require_value(document["checkpoint"], CHECKPOINT, "capture.checkpoint")

    surface = _require_object(document["surface"], "capture.surface")
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
        "capture.surface",
    )
    width = _require_int(surface["width"], "capture.surface.width")
    height = _require_int(surface["height"], "capture.surface.height")
    _require_value(width, LOGICAL_WIDTH, "capture.surface.width")
    _require_value(height, LOGICAL_HEIGHT, "capture.surface.height")
    surface_format = _require_string(surface["format"], "capture.surface.format")
    if surface_format not in ALLOWED_SURFACE_FORMATS:
        raise ValidationError(
            f"capture.surface.format is {surface_format!r}; "
            f"accepted formats are {sorted(ALLOWED_SURFACE_FORMATS)}"
        )
    _require_value(
        surface["pixel_layout"], "BGRA8", "capture.surface.pixel_layout"
    )
    _require_value(surface["row_order"], "top-left", "capture.surface.row_order")
    _require_value(
        surface["bytes_per_pixel"],
        BYTES_PER_PIXEL,
        "capture.surface.bytes_per_pixel",
    )
    _require_value(
        surface["row_stride"],
        width * BYTES_PER_PIXEL,
        "capture.surface.row_stride",
    )

    cursor = _require_object(document["cursor"], "capture.cursor")
    _require_exact_keys(cursor, ("x", "y", "policy"), "capture.cursor")
    cursor_x = _require_int(cursor["x"], "capture.cursor.x")
    cursor_y = _require_int(cursor["y"], "capture.cursor.y")
    _require_value(cursor_x, CURSOR_POINT[0], "capture.cursor.x")
    _require_value(cursor_y, CURSOR_POINT[1], "capture.cursor.y")
    _require_value(
        cursor["policy"], "software-composited", "capture.cursor.policy"
    )

    shell = _require_object(document["shell"], "capture.shell")
    shell_keys = (
        "screen",
        "dialog_resource_id",
        "movie_owner",
        "movie_base",
        "main_menu_shell_failed",
        "single_player_active",
        "skirmish_active",
        "modal_open",
        "quit_active",
        "first_paint_slide_active",
    )
    if schema_version == CAPTURE_SCHEMA_VERSION:
        shell_keys += ("title_terminal_persistent",)
    _require_exact_keys(shell, shell_keys, "capture.shell")
    _require_value(shell["screen"], "main-menu", "capture.shell.screen")
    _require_value(
        shell["dialog_resource_id"],
        DIALOG_RESOURCE_ID,
        "capture.shell.dialog_resource_id",
    )
    _require_value(
        shell["movie_owner"], "main-menu-0xe2", "capture.shell.movie_owner"
    )
    _require_value(shell["movie_base"], "ra2ts-l", "capture.shell.movie_base")
    for field in (
        "main_menu_shell_failed",
        "single_player_active",
        "skirmish_active",
        "modal_open",
        "quit_active",
        "first_paint_slide_active",
    ):
        value = _require_bool(shell[field], f"capture.shell.{field}")
        if value:
            raise ValidationError(
                f"capture.shell.{field} must be false for a steady 0xE2 frame"
            )
    if schema_version == CAPTURE_SCHEMA_VERSION:
        title_terminal = _require_bool(
            shell["title_terminal_persistent"],
            "capture.shell.title_terminal_persistent",
        )
        if not title_terminal:
            raise ValidationError(
                "capture.shell.title_terminal_persistent must be true "
                "for a steady 0xE2 frame"
            )

    frame = _require_object(document["frame"], "capture.frame")
    _require_exact_keys(frame, ("path", "byte_length"), "capture.frame")
    _require_value(frame["path"], FRAME_FILENAME, "capture.frame.path")
    byte_length = _require_int(
        frame["byte_length"], "capture.frame.byte_length"
    )
    expected_length = width * height * BYTES_PER_PIXEL
    _require_value(
        byte_length, expected_length, "capture.frame.byte_length"
    )
    return {
        "width": width,
        "height": height,
        "surface_format": surface_format,
        "cursor_x": cursor_x,
        "cursor_y": cursor_y,
        "frame_byte_length": byte_length,
    }


def validate_capture_bundle(
    capture_directory: str | os.PathLike[str],
    *,
    required_schema_version: str | None = None,
) -> ValidatedCapture:
    """Validate one immutable Rust capture bundle and recompute its digests."""

    directory = absolute_path(capture_directory)
    try:
        if directory.is_symlink():
            raise ValidationError(
                f"capture directory must not be a symbolic link: {directory}"
            )
        directory_metadata = directory.stat()
    except FileNotFoundError as exc:
        raise ValidationError(f"capture directory does not exist: {directory}") from exc
    except OSError as exc:
        raise ValidationError(
            f"cannot stat capture directory {directory}: {exc}"
        ) from exc
    if not stat.S_ISDIR(directory_metadata.st_mode):
        raise ValidationError(f"capture path is not a directory: {directory}")

    manifest_path = directory / CAPTURE_MANIFEST_FILENAME
    manifest_raw = _read_regular_bytes(
        manifest_path, "capture manifest", maximum_length=_MAX_JSON_BYTES
    )
    manifest_sha256 = sha256_bytes(manifest_raw)
    manifest = parse_json_bytes(manifest_raw, "capture manifest")
    values = _validate_capture_manifest(manifest)
    if (
        required_schema_version is not None
        and manifest["schema_version"] != required_schema_version
    ):
        raise ValidationError(
            "capture.schema_version must be current for this operation: "
            f"expected {required_schema_version!r}, "
            f"got {manifest['schema_version']!r}"
        )

    frame_path = directory / FRAME_FILENAME
    frame_bytes = _read_regular_bytes(
        frame_path,
        "capture frame",
        exact_length=values["frame_byte_length"],
    )
    frame_sha256 = sha256_bytes(frame_bytes)

    return ValidatedCapture(
        directory=directory,
        manifest_path=manifest_path,
        manifest_sha256=manifest_sha256,
        manifest=manifest,
        frame_path=frame_path,
        frame_sha256=frame_sha256,
        frame_bytes=frame_bytes,
        width=values["width"],
        height=values["height"],
        surface_format=values["surface_format"],
        cursor_x=values["cursor_x"],
        cursor_y=values["cursor_y"],
    )


def _safe_digest(path: Path, label: str) -> str | None:
    try:
        return sha256_file(path, label)
    except ValidationError:
        return None


def _invalid_region_results(
    guard: ValidatedGuard | None,
    capture: ValidatedCapture | None,
) -> list[dict[str, Any]]:
    if guard is None:
        return []
    results: list[dict[str, Any]] = []
    for region in guard.regions:
        actual: str | None = None
        if capture is not None:
            try:
                actual = sha256_bytes(
                    point_scale_presentation_crop_bgra(
                        capture.frame_bytes,
                        capture.width,
                        capture.height,
                        guard.content_rect,
                        region.presentation_rect,
                        guard.scale_numerator,
                        guard.scale_denominator,
                    )
                )
            except ValidationError:
                actual = None
        results.append(
            {
                "name": region.name,
                "logical_rect": region.logical_rect.as_dict(),
                "presentation_rect": region.presentation_rect.as_dict(),
                "hash_domain": (
                    "tight row-major BGRA8 bytes of the half-open 1920x1080 "
                    "presentation_rect after verified DDrawCompat point scaling"
                ),
                "expected_surface_raw_bgra_sha256": (
                    region.expected_presentation_sha256
                ),
                "actual_presentation_raw_bgra_sha256": actual,
                "status": INVALID,
            }
        )
    return results


def build_comparison_report(
    capture_directory: str | os.PathLike[str],
    guard_path: str | os.PathLike[str],
    *,
    expected_guard_sha256: str = SEALED_MAIN_MENU_GUARD_SHA256,
    additional_errors: Iterable[str] = (),
) -> dict[str, Any]:
    """Build a complete MATCH/DRIFT/INVALID report without writing inputs."""

    capture_dir = absolute_path(capture_directory)
    native_guard_path = absolute_path(guard_path)
    errors: list[str] = []

    def append_error(error: object) -> None:
        message = str(error)
        if message and message not in errors:
            errors.append(message)

    for additional_error in additional_errors:
        append_error(additional_error)
    guard: ValidatedGuard | None = None
    capture: ValidatedCapture | None = None

    try:
        guard = validate_guard(
            native_guard_path, expected_sha256=expected_guard_sha256
        )
    except ValidationError as exc:
        append_error(exc)
    try:
        capture = validate_capture_bundle(capture_dir)
    except ValidationError as exc:
        append_error(exc)

    if guard is not None and capture is not None:
        if (capture.width, capture.height) != (guard.width, guard.height):
            append_error(
                "capture and guard logical dimensions differ: "
                f"{capture.width}x{capture.height} vs {guard.width}x{guard.height}"
            )
        if (capture.cursor_x, capture.cursor_y) != (
            guard.cursor_x,
            guard.cursor_y,
        ):
            append_error(
                "capture and guard neutral cursor points differ: "
                f"({capture.cursor_x},{capture.cursor_y}) vs "
                f"({guard.cursor_x},{guard.cursor_y})"
            )
        for region in guard.regions:
            if cursor_overlaps_rect(
                capture.cursor_x, capture.cursor_y, region.logical_rect
            ):
                append_error(
                    f"capture cursor overlaps compared region {region.name}"
                )

    if errors:
        status = INVALID
        region_results = _invalid_region_results(guard, capture)
    else:
        assert guard is not None
        assert capture is not None
        region_results = []
        for region in guard.regions:
            actual_digest = sha256_bytes(
                point_scale_presentation_crop_bgra(
                    capture.frame_bytes,
                    capture.width,
                    capture.height,
                    guard.content_rect,
                    region.presentation_rect,
                    guard.scale_numerator,
                    guard.scale_denominator,
                )
            )
            region_status = (
                MATCH
                if actual_digest == region.expected_presentation_sha256
                else DRIFT
            )
            region_results.append(
                {
                    "name": region.name,
                    "logical_rect": region.logical_rect.as_dict(),
                    "presentation_rect": region.presentation_rect.as_dict(),
                    "hash_domain": (
                        "tight row-major BGRA8 bytes of the half-open "
                        "1920x1080 presentation_rect after verified "
                        "DDrawCompat point scaling"
                    ),
                    "expected_surface_raw_bgra_sha256": (
                        region.expected_presentation_sha256
                    ),
                    "actual_presentation_raw_bgra_sha256": actual_digest,
                    "status": region_status,
                }
            )
        status = (
            MATCH
            if all(region["status"] == MATCH for region in region_results)
            else DRIFT
        )

    guard_digest = guard.sha256 if guard is not None else _safe_digest(
        native_guard_path, "native shell guard"
    )
    manifest_path = capture_dir / CAPTURE_MANIFEST_FILENAME
    frame_path = capture_dir / FRAME_FILENAME
    manifest_digest = (
        capture.manifest_sha256
        if capture is not None
        else _safe_digest(manifest_path, "capture manifest")
    )
    frame_digest = (
        capture.frame_sha256
        if capture is not None
        else _safe_digest(frame_path, "capture frame")
    )

    return {
        "schema_version": COMPARISON_SCHEMA_VERSION,
        "generated_at_utc": utc_now(),
        "checkpoint": CHECKPOINT,
        "status": status,
        "errors": errors,
        "guard": {
            "path": str(native_guard_path),
            "sha256": guard_digest,
            "expected_sha256": expected_guard_sha256,
            "schema_version": GUARD_SCHEMA_VERSION,
            "guard_id": GUARD_ID,
            "state": GUARD_STATE,
            "mapping": (
                {
                    "kind": MAPPING_KIND,
                    "logical_width": guard.width,
                    "logical_height": guard.height,
                    "presentation_width": guard.presentation_width,
                    "presentation_height": guard.presentation_height,
                    "content_rect": guard.content_rect.as_dict(),
                    "scale_numerator": guard.scale_numerator,
                    "scale_denominator": guard.scale_denominator,
                    "display_filter": "point(0)",
                }
                if guard is not None
                else None
            ),
        },
        "capture": {
            "directory": str(capture_dir),
            "manifest_path": str(manifest_path),
            "manifest_sha256": manifest_digest,
            "frame_path": str(frame_path),
            "frame_sha256": frame_digest,
            "surface": (
                {
                    "width": capture.width,
                    "height": capture.height,
                    "format": capture.surface_format,
                    "pixel_layout": "BGRA8",
                    "row_order": "top-left",
                    "bytes_per_pixel": BYTES_PER_PIXEL,
                    "row_stride": capture.width * BYTES_PER_PIXEL,
                }
                if capture is not None
                else None
            ),
            "cursor": (
                {
                    "x": capture.cursor_x,
                    "y": capture.cursor_y,
                    "policy": "software-composited",
                }
                if capture is not None
                else None
            ),
        },
        "comparison_domain": {
            "source": (
                "tight top-left 800x600 BGRA8 Rust production swapchain readback"
            ),
            "transform": (
                "DDrawCompat point(0) into content_rect [240,0,1680,1080) "
                "at exact scale 9/5"
            ),
            "sample_formula": (
                "source=floor((2*destination_offset+1)*5/18) independently "
                "for x and y"
            ),
            "hashed_bytes": (
                "tight row-major BGRA8 bytes of each half-open presentation_rect"
            ),
            "expected_guard_field": "surface_raw_bgra_sha256",
            "forbidden_comparison": (
                "direct logical_rect crop bytes are never compared with the "
                "presentation-domain guard digest"
            ),
        },
        "regions": region_results,
        "scope": {
            "certifies": (
                "only the four named point-scaled 1920x1080 presentation BGRA8 "
                "crops in this report"
            ),
            "does_not_certify": [
                "whole-frame pixels",
                "RA2TS movie frame or timer phase",
                "OS compositor or display-output behavior beyond the verified "
                "DDrawCompat point mapping",
                "cursor shape",
                "route transitions",
                "input",
                "audio",
                "other dialogs",
                "other resolutions",
            ],
        },
    }


def write_json_exclusive(
    output_path: str | os.PathLike[str], document: Mapping[str, Any]
) -> Path:
    """Write one canonical JSON artifact exactly once, never replacing existing data."""

    return write_bytes_exclusive(output_path, canonical_json_bytes(document))


def write_bytes_exclusive(
    output_path: str | os.PathLike[str], content: bytes
) -> Path:
    """Write one binary diagnostic artifact exactly once."""

    path = absolute_path(output_path)
    parent = path.parent
    if not parent.exists() or not parent.is_dir():
        raise ValidationError(f"output parent directory does not exist: {parent}")
    try:
        with path.open("xb") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
    except FileExistsError as exc:
        raise OutputExistsError(f"refusing to overwrite output: {path}") from exc
    return path


def compare_to_file(
    capture_directory: str | os.PathLike[str],
    guard_path: str | os.PathLike[str],
    output_path: str | os.PathLike[str],
    *,
    expected_guard_sha256: str = SEALED_MAIN_MENU_GUARD_SHA256,
    additional_errors: Iterable[str] = (),
) -> dict[str, Any]:
    """Build and exclusively persist one comparison report."""

    output = absolute_path(output_path)
    if output.exists() or output.is_symlink():
        raise OutputExistsError(f"refusing to overwrite output: {output}")
    report = build_comparison_report(
        capture_directory,
        guard_path,
        expected_guard_sha256=expected_guard_sha256,
        additional_errors=additional_errors,
    )
    write_json_exclusive(output, report)
    return report
