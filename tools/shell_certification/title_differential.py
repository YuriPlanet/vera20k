"""Evidence-derived differential for Main Menu static 0x694.

This module compares the production Rust logical frame with every sealed native
presentation source. It proves the unique glyph-mask translation and applies
the verified Path-A terminal tint through codebooks derived from those native
sources. It never edits an input or guesses a background pixel.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

from .core import (
    BYTES_PER_PIXEL,
    DRIFT,
    MATCH,
    SEALED_MAIN_MENU_GUARD_SHA256,
    OutputExistsError,
    Rect,
    ValidatedCapture,
    ValidatedGuard,
    ValidationError,
    absolute_path,
    crop_tight_bgra,
    point_scale_presentation_crop_bgra,
    sha256_bytes,
    utc_now,
    validate_capture_bundle,
    validate_guard,
    write_json_exclusive,
)
from .native_sources import GuardSourceBundle, is_link, load_guard_sources
from .presentation_profile import derive_channel_codebooks


TITLE_DIFFERENTIAL_SCHEMA_VERSION = "vera20k.shell-title-differential.v1"
TITLE_DIFFERENTIAL_CHECKPOINT = "main-menu-0xe2-title-0x694"
TITLE_REGION_NAME = "title"
TITLE_UTF16_UNITS = 9
KIND1_RANGE = 8
BASE_RGB = (255, 255, 0)
HIGHLIGHT_RGB = (255, 255, 255)
EXPECTED_MASK_PIXELS = 243
EXPECTED_TERMINAL_UNIT_PIXELS = 29


@dataclass(frozen=True)
class PixelComparison:
    exact: int
    mismatch: int
    total: int
    sha256: str

    def as_dict(self) -> dict[str, int | str]:
        return {
            "exact_pixels": self.exact,
            "mismatch_pixels": self.mismatch,
            "total_pixels": self.total,
            "sha256": self.sha256,
        }


def _pixel_count_equal(left: bytes, right: bytes) -> tuple[int, int]:
    if len(left) != len(right) or len(left) % BYTES_PER_PIXEL:
        raise ValidationError("pixel buffers are not equally sized BGRA8 data")
    total = len(left) // BYTES_PER_PIXEL
    exact = sum(
        left[offset : offset + BYTES_PER_PIXEL]
        == right[offset : offset + BYTES_PER_PIXEL]
        for offset in range(0, len(left), BYTES_PER_PIXEL)
    )
    return exact, total


def path_a_encoded_rgb(
    unit_position: int,
    count: int,
    reveal_range: int,
    base_rgb: tuple[int, int, int],
    highlight_rgb: tuple[int, int, int],
) -> tuple[int, int, int] | None:
    """Return native Path-A encoded RGB for a one-based UTF-16 unit.

    The blend starts from the font's 16-bit text colour: the shell print
    (0x00621040) truncates the COLORREF to R5G6B5 units and 0x00434DED shifts
    them back to bytes with the low bits clear (src/render/shell_text_reveal.rs).
    """

    if (
        type(unit_position) is not int
        or type(count) is not int
        or type(reveal_range) is not int
    ):
        raise ValidationError("Path-A position, count, and range must be integers")
    if unit_position <= 0 or count < 0 or reveal_range <= 0:
        raise ValidationError(
            "Path-A position/range must be positive and count nonnegative"
        )
    if len(base_rgb) != 3 or len(highlight_rgb) != 3:
        raise ValidationError("Path-A colors must contain exactly three channels")
    if any(
        type(value) is not int or not 0 <= value <= 255
        for value in (*base_rgb, *highlight_rgb)
    ):
        raise ValidationError("Path-A color channels must be 8-bit integers")
    if count != 0 and count <= unit_position:
        return None
    base_rgb = (base_rgb[0] & 0xF8, base_rgb[1] & 0xFC, base_rgb[2] & 0xF8)

    remaining = count - unit_position - 1
    if remaining >= reveal_range:
        return base_rgb
    gradient = reveal_range - remaining
    coefficient = (255 // reveal_range) * gradient

    def trunc_div_256(value: int) -> int:
        return value // 256 if value >= 0 else -((-value) // 256)

    return tuple(
        base + trunc_div_256((highlight - base) * coefficient)
        for base, highlight in zip(base_rgb, highlight_rgb)
    )


def presentation_bgra_for_encoded_rgb(
    encoded_rgb: tuple[int, int, int],
    five_bit: tuple[int, ...],
    six_bit: tuple[int, ...],
) -> bytes:
    """Pack encoded RGB565 indices and expand them through native codebooks."""

    if len(five_bit) != 32 or len(six_bit) != 64:
        raise ValidationError("RGB565 codebooks must contain 32 and 64 entries")
    red, green, blue = encoded_rgb
    return bytes(
        (
            five_bit[blue >> 3],
            six_bit[green >> 2],
            five_bit[red >> 3],
            255,
        )
    )


def collapse_presentation_crop_to_logical(
    presentation_crop: bytes,
    guard: ValidatedGuard,
    logical_rect: Rect,
    presentation_rect: Rect,
) -> bytes:
    """Invert one sealed point-scale crop after proving every cell is uniform."""

    physical_width = presentation_rect.right - presentation_rect.left
    physical_height = presentation_rect.bottom - presentation_rect.top
    expected = physical_width * physical_height * BYTES_PER_PIXEL
    if len(presentation_crop) != expected:
        raise ValidationError(
            f"presentation crop byte length is {len(presentation_crop)}, "
            f"expected {expected}"
        )
    logical_width = logical_rect.right - logical_rect.left
    logical_height = logical_rect.bottom - logical_rect.top
    cells: list[bytes | None] = [None] * (logical_width * logical_height)

    for presentation_y in range(presentation_rect.top, presentation_rect.bottom):
        destination_y = presentation_y - guard.content_rect.top
        source_y = (
            (2 * destination_y + 1)
            * guard.scale_denominator
            // (2 * guard.scale_numerator)
        )
        for presentation_x in range(
            presentation_rect.left, presentation_rect.right
        ):
            destination_x = presentation_x - guard.content_rect.left
            source_x = (
                (2 * destination_x + 1)
                * guard.scale_denominator
                // (2 * guard.scale_numerator)
            )
            if not (
                logical_rect.left <= source_x < logical_rect.right
                and logical_rect.top <= source_y < logical_rect.bottom
            ):
                raise ValidationError(
                    "presentation crop maps outside its declared logical rectangle"
                )
            logical_index = (
                (source_y - logical_rect.top) * logical_width
                + source_x
                - logical_rect.left
            )
            physical_index = (
                (presentation_y - presentation_rect.top) * physical_width
                + presentation_x
                - presentation_rect.left
            ) * BYTES_PER_PIXEL
            value = presentation_crop[
                physical_index : physical_index + BYTES_PER_PIXEL
            ]
            previous = cells[logical_index]
            if previous is not None and previous != value:
                raise ValidationError(
                    "native point-scale replicas disagree inside one logical cell"
                )
            cells[logical_index] = value

    if any(value is None for value in cells):
        raise ValidationError("presentation crop does not cover every logical cell")
    return b"".join(value for value in cells if value is not None)


def _mask_with_width(
    crop: bytes, width: int, colors: Iterable[bytes]
) -> set[tuple[int, int]]:
    accepted = set(colors)
    if not accepted or any(len(color) != BYTES_PER_PIXEL for color in accepted):
        raise ValidationError("foreground colors must be non-empty BGRA8 values")
    pixels = len(crop) // BYTES_PER_PIXEL
    if pixels * BYTES_PER_PIXEL != len(crop):
        raise ValidationError("crop is not BGRA8 data")
    if width <= 0 or pixels % width:
        raise ValidationError("crop width does not divide the BGRA8 pixel count")
    return {
        (index % width, index // width)
        for index in range(pixels)
        if crop[index * BYTES_PER_PIXEL : (index + 1) * BYTES_PER_PIXEL]
        in accepted
    }


def exhaustive_mask_shifts(
    source: set[tuple[int, int]],
    target: set[tuple[int, int]],
    width: int,
    height: int,
) -> list[tuple[int, int, int]]:
    """Return every in-bounds translation as ``(mismatches, dx, dy)``."""

    if not source or not target:
        raise ValidationError("mask shift search requires non-empty masks")
    min_x = min(x for x, _ in source)
    max_x = max(x for x, _ in source)
    min_y = min(y for _, y in source)
    max_y = max(y for _, y in source)
    results: list[tuple[int, int, int]] = []
    for dy in range(-min_y, height - max_y):
        for dx in range(-min_x, width - max_x):
            shifted = {(x + dx, y + dy) for x, y in source}
            results.append((len(shifted ^ target), dx, dy))
    return sorted(results)


def rightmost_unit_mask(mask: set[tuple[int, int]]) -> set[tuple[int, int]]:
    """Select the rightmost foreground-column run without naming its glyph."""

    if not mask:
        raise ValidationError("terminal-unit selection requires a non-empty mask")
    columns = sorted({x for x, _ in mask})
    runs: list[list[int]] = []
    for column in columns:
        if not runs or column != runs[-1][-1] + 1:
            runs.append([column])
        else:
            runs[-1].append(column)
    terminal_columns = set(runs[-1])
    return {(x, y) for x, y in mask if x in terminal_columns}


def _replace_crop(
    full_frame: bytes,
    width: int,
    height: int,
    rect: Rect,
    crop: bytes,
) -> bytes:
    expected = width * height * BYTES_PER_PIXEL
    if len(full_frame) != expected:
        raise ValidationError("full logical frame has the wrong BGRA8 length")
    crop_width = rect.right - rect.left
    crop_height = rect.bottom - rect.top
    if len(crop) != crop_width * crop_height * BYTES_PER_PIXEL:
        raise ValidationError("replacement crop has the wrong BGRA8 length")
    output = bytearray(full_frame)
    for row in range(crop_height):
        source = row * crop_width * BYTES_PER_PIXEL
        destination = ((rect.top + row) * width + rect.left) * BYTES_PER_PIXEL
        output[destination : destination + crop_width * BYTES_PER_PIXEL] = crop[
            source : source + crop_width * BYTES_PER_PIXEL
        ]
    return bytes(output)


def _paint_shifted_mask(
    current_crop: bytes,
    native_crop: bytes,
    current_mask: set[tuple[int, int]],
    terminal_unit_mask: set[tuple[int, int]],
    width: int,
    height: int,
    dx: int,
    dy: int,
    base_bgra: bytes,
    terminal_bgra: bytes | None,
) -> bytes:
    shifted = {(x + dx, y + dy) for x, y in current_mask}
    shifted_terminal = {(x + dx, y + dy) for x, y in terminal_unit_mask}
    if any(not (0 <= x < width and 0 <= y < height) for x, y in shifted):
        raise ValidationError("selected mask shift leaves the title crop")

    output = bytearray(current_crop)
    # Only old-only pixels need restoration. The sealed native crop supplies
    # those observed background bytes; overlapping pixels are overwritten below.
    for x, y in current_mask - shifted:
        offset = (y * width + x) * BYTES_PER_PIXEL
        output[offset : offset + BYTES_PER_PIXEL] = native_crop[
            offset : offset + BYTES_PER_PIXEL
        ]
    for x, y in shifted:
        offset = (y * width + x) * BYTES_PER_PIXEL
        color = (
            terminal_bgra
            if terminal_bgra is not None and (x, y) in shifted_terminal
            else base_bgra
        )
        output[offset : offset + BYTES_PER_PIXEL] = color
    return bytes(output)


def _comparison(
    candidate_crop: bytes,
    native_logical_crop: bytes,
    capture: ValidatedCapture,
    guard: ValidatedGuard,
    logical_rect: Rect,
    presentation_rect: Rect,
    native_presentation_crop: bytes,
) -> dict[str, object]:
    logical_exact, logical_total = _pixel_count_equal(
        candidate_crop, native_logical_crop
    )
    full_frame = _replace_crop(
        capture.frame_bytes,
        capture.width,
        capture.height,
        logical_rect,
        candidate_crop,
    )
    presentation = point_scale_presentation_crop_bgra(
        full_frame,
        capture.width,
        capture.height,
        guard.content_rect,
        presentation_rect,
        guard.scale_numerator,
        guard.scale_denominator,
    )
    presentation_exact, presentation_total = _pixel_count_equal(
        presentation, native_presentation_crop
    )
    return {
        "logical": PixelComparison(
            exact=logical_exact,
            mismatch=logical_total - logical_exact,
            total=logical_total,
            sha256=sha256_bytes(candidate_crop),
        ).as_dict(),
        "presentation": PixelComparison(
            exact=presentation_exact,
            mismatch=presentation_total - presentation_exact,
            total=presentation_total,
            sha256=sha256_bytes(presentation),
        ).as_dict(),
    }


def _derive_codebooks(
    sources: GuardSourceBundle,
) -> tuple[tuple[int, ...], tuple[int, ...]]:
    expected: tuple[tuple[int, ...], tuple[int, ...], tuple[int, ...]] | None = None
    for source in sources.sources:
        blue, green, red, alpha = derive_channel_codebooks(
            source.pixels,
            sources.guard.presentation_width,
            sources.guard.presentation_height,
        )
        if (len(blue), len(green), len(red), alpha) != (32, 64, 32, (255,)):
            raise ValidationError(
                "native source does not expose an opaque RGB565 presentation codebook"
            )
        observed = (blue, green, red)
        if expected is not None and observed != expected:
            raise ValidationError("native source RGB565 codebooks disagree")
        expected = observed
    if expected is None:
        raise ValidationError("native source bundle is empty")
    blue, green, red = expected
    if blue != red:
        raise ValidationError("native blue/red five-bit codebooks disagree")
    return blue, green


def build_title_differential_report(
    capture_path: Path,
    guard_path: Path,
    oracle_runs: Path,
    *,
    expected_guard_sha256: str = SEALED_MAIN_MENU_GUARD_SHA256,
) -> dict[str, object]:
    """Build the immutable-evidence title differential in memory."""

    guard = validate_guard(guard_path, expected_sha256=expected_guard_sha256)
    capture = validate_capture_bundle(capture_path)
    if (capture.width, capture.height) != (guard.width, guard.height):
        raise ValidationError("capture and native guard logical dimensions disagree")
    sources = load_guard_sources(guard, oracle_runs)
    region = next(
        (
            candidate
            for candidate in guard.regions
            if candidate.name == TITLE_REGION_NAME
        ),
        None,
    )
    if region is None:
        raise ValidationError("sealed guard has no title region")

    native_presentation_crops = tuple(
        crop_tight_bgra(
            source.pixels,
            guard.presentation_width,
            guard.presentation_height,
            region.presentation_rect,
        )
        for source in sources.sources
    )
    for index, crop in enumerate(native_presentation_crops):
        digest = sha256_bytes(crop)
        if digest != region.expected_presentation_sha256:
            raise ValidationError(
                f"native title crop {index} SHA-256 is {digest}, "
                f"expected {region.expected_presentation_sha256}"
            )
    if len(set(native_presentation_crops)) != 1:
        raise ValidationError("sealed native title crops disagree")
    native_presentation_crop = native_presentation_crops[0]
    native_logical_crop = collapse_presentation_crop_to_logical(
        native_presentation_crop,
        guard,
        region.logical_rect,
        region.presentation_rect,
    )
    current_logical_crop = crop_tight_bgra(
        capture.frame_bytes,
        capture.width,
        capture.height,
        region.logical_rect,
    )

    five_bit, six_bit = _derive_codebooks(sources)
    base_bgra = presentation_bgra_for_encoded_rgb(BASE_RGB, five_bit, six_bit)
    terminal_count = TITLE_UTF16_UNITS + KIND1_RANGE
    terminal_encoded_rgb = path_a_encoded_rgb(
        TITLE_UTF16_UNITS,
        terminal_count,
        KIND1_RANGE,
        BASE_RGB,
        HIGHLIGHT_RGB,
    )
    if terminal_encoded_rgb is None:
        raise ValidationError("terminal Path-A unit unexpectedly remained hidden")
    terminal_bgra = presentation_bgra_for_encoded_rgb(
        terminal_encoded_rgb, five_bit, six_bit
    )

    crop_width = region.logical_rect.right - region.logical_rect.left
    crop_height = region.logical_rect.bottom - region.logical_rect.top
    current_mask = _mask_with_width(
        current_logical_crop, crop_width, (base_bgra,)
    )
    native_mask = _mask_with_width(
        native_logical_crop, crop_width, (base_bgra, terminal_bgra)
    )
    native_terminal_mask = _mask_with_width(
        native_logical_crop, crop_width, (terminal_bgra,)
    )
    if (
        len(current_mask) != EXPECTED_MASK_PIXELS
        or len(native_mask) != EXPECTED_MASK_PIXELS
    ):
        raise ValidationError(
            "title foreground mask count does not match the sealed 243-pixel identity"
        )

    shift_results = exhaustive_mask_shifts(
        current_mask, native_mask, crop_width, crop_height
    )
    zero_mismatch_shifts = [
        (dx, dy) for mismatches, dx, dy in shift_results if mismatches == 0
    ]
    if zero_mismatch_shifts != [(1, 0)]:
        raise ValidationError(
            "title mask has non-unique or unexpected exact shifts: "
            f"{zero_mismatch_shifts}"
        )
    _, dx, dy = shift_results[0]

    terminal_unit_mask = rightmost_unit_mask(current_mask)
    if (
        len(terminal_unit_mask) != EXPECTED_TERMINAL_UNIT_PIXELS
        or len(native_terminal_mask) != EXPECTED_TERMINAL_UNIT_PIXELS
    ):
        raise ValidationError(
            "terminal UTF-16 unit does not have the sealed 29-pixel mask"
        )
    shifted_terminal = {(x + dx, y + dy) for x, y in terminal_unit_mask}
    if shifted_terminal != native_terminal_mask:
        raise ValidationError(
            "rightmost evidence-derived UTF-16 unit is not the native tinted mask"
        )

    unaffected = {
        (x, y) for y in range(crop_height) for x in range(crop_width)
    } - current_mask - native_mask
    background_exact = 0
    for x, y in unaffected:
        offset = (y * crop_width + x) * BYTES_PER_PIXEL
        background_exact += (
            current_logical_crop[offset : offset + BYTES_PER_PIXEL]
            == native_logical_crop[offset : offset + BYTES_PER_PIXEL]
        )
    if background_exact != len(unaffected):
        raise ValidationError("native and Rust title backgrounds disagree")

    shift_only_crop = _paint_shifted_mask(
        current_logical_crop,
        native_logical_crop,
        current_mask,
        terminal_unit_mask,
        crop_width,
        crop_height,
        dx,
        dy,
        base_bgra,
        None,
    )
    predictive_crop = _paint_shifted_mask(
        current_logical_crop,
        native_logical_crop,
        current_mask,
        terminal_unit_mask,
        crop_width,
        crop_height,
        dx,
        dy,
        base_bgra,
        terminal_bgra,
    )
    comparisons = {
        "current_rust": _comparison(
            current_logical_crop,
            native_logical_crop,
            capture,
            guard,
            region.logical_rect,
            region.presentation_rect,
            native_presentation_crop,
        ),
        "shift_only": _comparison(
            shift_only_crop,
            native_logical_crop,
            capture,
            guard,
            region.logical_rect,
            region.presentation_rect,
            native_presentation_crop,
        ),
        "shift_plus_derived_tint": _comparison(
            predictive_crop,
            native_logical_crop,
            capture,
            guard,
            region.logical_rect,
            region.presentation_rect,
            native_presentation_crop,
        ),
    }
    current_mismatch = comparisons["current_rust"]["presentation"][
        "mismatch_pixels"
    ]
    predictive_mismatch = comparisons["shift_plus_derived_tint"][
        "presentation"
    ]["mismatch_pixels"]
    if predictive_mismatch != 0:
        raise ValidationError("evidence-derived title transform is not exact")

    return {
        "schema_version": TITLE_DIFFERENTIAL_SCHEMA_VERSION,
        "checkpoint": TITLE_DIFFERENTIAL_CHECKPOINT,
        "generated_at_utc": utc_now(),
        "status": DRIFT if current_mismatch else MATCH,
        "errors": [],
        "guard": {
            "path": str(guard.path),
            "sha256": guard.sha256,
            "expected_title_presentation_sha256": (
                region.expected_presentation_sha256
            ),
        },
        "capture": {
            "directory": str(capture.directory),
            "manifest_sha256": capture.manifest_sha256,
            "frame_sha256": capture.frame_sha256,
        },
        "native_sources": [
            {
                "run_id": source.run_id,
                "path": str(source.path),
                "frame_sha256": source.surface_pixel_sha256,
            }
            for source in sources.sources
        ],
        "geometry": {
            "logical_rect": region.logical_rect.as_dict(),
            "presentation_rect": region.presentation_rect.as_dict(),
            "logical_size": {"width": crop_width, "height": crop_height},
            "unique_exact_mask_shift": {"dx": dx, "dy": dy},
            "searched_in_bounds_translations": len(shift_results),
        },
        "mechanism": {
            "utf16_units": TITLE_UTF16_UNITS,
            "range": KIND1_RANGE,
            "target_count": TITLE_UTF16_UNITS + 1 + KIND1_RANGE,
            "terminal_displayed_count": terminal_count,
            "terminal_unit_position": TITLE_UTF16_UNITS,
        },
        "colors": {
            "base_encoded_rgb": list(BASE_RGB),
            "highlight_encoded_rgb": list(HIGHLIGHT_RGB),
            "terminal_encoded_rgb": list(terminal_encoded_rgb),
            "base_presentation_bgra": list(base_bgra),
            "terminal_presentation_bgra": list(terminal_bgra),
        },
        "masks": {
            "current_pixels": len(current_mask),
            "native_pixels": len(native_mask),
            "terminal_unit_pixels": len(terminal_unit_mask),
            "native_terminal_tint_pixels": len(native_terminal_mask),
            "unaffected_background_exact_pixels": background_exact,
            "unaffected_background_total_pixels": len(unaffected),
        },
        "comparisons": comparisons,
        "conclusion": {
            "current_rust_is_red": bool(current_mismatch),
            "shift_only_is_insufficient": bool(
                comparisons["shift_only"]["presentation"]["mismatch_pixels"]
            ),
            "evidence_derived_transform_is_exact": True,
        },
        "evidence_scope": (
            "enrolled 800x600 logical / 1920x1080 active retail presentation only"
        ),
        "not_certified": [
            "native transition-frame timing",
            "another resolution",
            "other shell statics",
        ],
    }


def write_title_differential(
    capture_path: Path,
    guard_path: Path,
    oracle_runs: Path,
    output: Path,
    *,
    expected_guard_sha256: str = SEALED_MAIN_MENU_GUARD_SHA256,
) -> dict[str, object]:
    """Write one canonical differential report without replacing anything."""

    output_path = absolute_path(output)
    if output_path.exists() or is_link(output_path):
        raise OutputExistsError(f"refusing to overwrite output: {output_path}")
    report = build_title_differential_report(
        capture_path,
        guard_path,
        oracle_runs,
        expected_guard_sha256=expected_guard_sha256,
    )
    write_json_exclusive(output_path, report)
    return report
