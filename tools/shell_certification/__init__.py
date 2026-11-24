"""Fail-closed certification helpers for production shell frame captures."""

from .core import (
    DRIFT,
    INVALID,
    MATCH,
    SEALED_MAIN_MENU_GUARD_SHA256,
    ValidationError,
    build_comparison_report,
    compare_to_file,
    crop_tight_bgra,
    cursor_overlaps_rect,
    point_scale_presentation_crop_bgra,
    validate_capture_bundle,
    validate_guard,
)

__all__ = [
    "DRIFT",
    "INVALID",
    "MATCH",
    "SEALED_MAIN_MENU_GUARD_SHA256",
    "ValidationError",
    "build_comparison_report",
    "compare_to_file",
    "crop_tight_bgra",
    "cursor_overlaps_rect",
    "point_scale_presentation_crop_bgra",
    "validate_capture_bundle",
    "validate_guard",
]
