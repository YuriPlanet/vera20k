"""Validate a diagnostic skirmish capture; this never certifies retail parity.

Run ``python -m tools.shell_certification.skirmish CAPTURE_DIRECTORY``.
Native comparison requires separately enrolled asset and profile inputs.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from .core import (
    ALLOWED_SURFACE_FORMATS, ValidationError, parse_json_bytes,
    _read_regular_bytes, _require_array, _require_exact_keys, _require_int,
    _require_object, _require_string, _require_value, sha256_bytes,
)

SCHEMA = "vera20k.skirmish-shell-capture.v1"
CHECKPOINT = "skirmish-0x102-steady"
FRAME_BYTES = 800 * 600 * 4


def _typed_fields(value: object, fields: dict[str, type], label: str) -> None:
    obj = _require_object(value, label)
    _require_exact_keys(obj, fields, label)
    for key, kind in fields.items():
        if type(obj[key]) is not kind:
            raise ValidationError(f"{label}.{key} must be {kind.__name__}")


def validate_bundle(directory: Path) -> dict:
    directory = directory.absolute()
    if directory.is_symlink() or getattr(directory, "is_junction", lambda: False)():
        raise ValidationError("capture directory must not be a link or junction")
    if not directory.is_dir() or {p.name for p in directory.iterdir()} != {"capture.json", "frame.bgra"}:
        raise ValidationError("capture inventory must be exactly capture.json and frame.bgra")
    raw = _read_regular_bytes(directory / "capture.json", "skirmish manifest", maximum_length=1024 * 1024)
    manifest = _require_object(parse_json_bytes(raw, "skirmish manifest"), "manifest")
    _require_exact_keys(manifest, (
        "schema_version", "checkpoint", "parity_certification", "presenter_domain",
        "surface", "cursor", "route", "dialog_resource_id", "capture_frame",
        "selection", "reveals_completed", "ordinary_skirmish_frame", "input_enrollment", "frame",
    ), "manifest")
    for key, expected in {
        "schema_version": SCHEMA, "checkpoint": CHECKPOINT,
        "parity_certification": "NONE", "presenter_domain": "final-swapchain-after-rgb565",
        "dialog_resource_id": 0x102, "reveals_completed": True, "ordinary_skirmish_frame": True,
        "input_enrollment": "UNENROLLED; asset/profile bytes require enrollment before native comparison",
    }.items():
        _require_value(manifest[key], expected, key)
    surface = _require_object(manifest["surface"], "surface")
    _require_exact_keys(surface, ("width", "height", "format", "pixel_layout", "row_order", "row_stride"), "surface")
    for key, expected in {"width": 800, "height": 600, "pixel_layout": "BGRA8", "row_order": "top-left", "row_stride": 3200}.items():
        _require_value(surface[key], expected, f"surface.{key}")
    if _require_string(surface["format"], "surface.format") not in ALLOWED_SURFACE_FORMATS:
        raise ValidationError("unsupported surface format")
    cursor = _require_object(manifest["cursor"], "cursor")
    _require_exact_keys(cursor, ("x", "y", "policy"), "cursor")
    for key, expected in {"x": 400, "y": 300, "policy": "software-composited"}.items():
        _require_value(cursor[key], expected, f"cursor.{key}")
    route = _require_array(manifest["route"], "route")
    if len(route) != 2:
        raise ValidationError("route requires two ordinary navigation actions")
    previous = 0
    for index, (dialog, action) in enumerate(((0xE2, "SinglePlayer"), (0x100, "Skirmish"))):
        step = _require_object(route[index], f"route[{index}]")
        _require_exact_keys(step, ("dialog", "frame", "action"), "route step")
        _require_value(step["dialog"], dialog, "route dialog")
        _require_value(step["action"], action, "route action")
        frame = _require_int(step["frame"], "route frame")
        if frame <= previous:
            raise ValidationError("route frames must increase from a positive first frame")
        previous = frame
    if _require_int(manifest["capture_frame"], "capture_frame") <= previous:
        raise ValidationError("capture frame must follow navigation")
    selection = _require_object(manifest["selection"], "selection")
    _require_exact_keys(selection, (
        "map_file", "map_label", "map_capacity", "mode_id", "preview", "parsed_map_sha256",
        "parsed_map_hash_encoding", "player", "opponents", "options",
    ), "selection")
    for key in ("map_file", "map_label", "parsed_map_hash_encoding"):
        _require_string(selection[key], f"selection.{key}")
    _require_value(selection["parsed_map_hash_encoding"],
                   "Rust Debug representation; diagnostic identity only", "parsed_map_hash_encoding")
    for key in ("map_capacity", "mode_id"):
        _require_int(selection[key], f"selection.{key}")
    row_fields = {"country": str, "country_random": bool, "color": int,
                  "color_claimed": bool, "start": str, "team": int}
    _typed_fields(selection["player"], {"name": str, **row_fields}, "player")
    _typed_fields(selection["options"], {
        "credits": int, "speed": int, "units": int, "short_game": bool,
        "super_weapons": bool, "build_off_ally": bool, "crates": bool,
        "mcv_redeploy": bool,
    }, "options")
    for opponent in _require_array(selection["opponents"], "opponents"):
        _typed_fields(opponent, {"visible": bool, "enabled": bool, "type": str, **row_fields}, "opponent")
    preview = _require_object(selection["preview"], "preview")
    _require_exact_keys(preview, ("width", "height"), "preview")
    for dimension in ("width", "height"):
        if _require_int(preview[dimension], f"preview.{dimension}") <= 0:
            raise ValidationError("preview dimensions must be positive")
    identity = _require_string(selection["parsed_map_sha256"], "parsed_map_sha256")
    if len(identity) != 64 or any(c not in "0123456789abcdef" for c in identity):
        raise ValidationError("parsed map diagnostic identity must be lowercase SHA256")
    frame = _require_object(manifest["frame"], "frame")
    _require_exact_keys(frame, ("path", "byte_length", "sha256"), "frame")
    _require_value(frame["path"], "frame.bgra", "frame.path")
    _require_value(frame["byte_length"], FRAME_BYTES, "frame.byte_length")
    pixels = _read_regular_bytes(directory / "frame.bgra", "frame", maximum_length=FRAME_BYTES)
    if len(pixels) != FRAME_BYTES:
        raise ValidationError("frame byte length mismatch")
    digest = sha256_bytes(pixels)
    _require_value(frame["sha256"], digest, "frame.sha256")
    return {"checkpoint": CHECKPOINT, "capture_valid": True, "parity_certification": "NONE",
            "frame_sha256": digest, "capture_directory": str(directory)}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", type=Path)
    args = parser.parse_args()
    try:
        print(json.dumps(validate_bundle(args.capture), sort_keys=True))
        return 0
    except (ValidationError, OSError) as exc:
        parser.exit(2, f"invalid skirmish capture: {exc}\n")


if __name__ == "__main__":
    raise SystemExit(main())
