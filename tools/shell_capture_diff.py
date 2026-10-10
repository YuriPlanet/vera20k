#!/usr/bin/env python3
"""Compare a VERA20k shell-capture frame with a native screenshot in RGB565 units.

Both engines compose shell frames on a 16-bit R5G6B5 surface. Their final
8-bit expansions differ (cnc-ddraw shifts or replicates bits; the VERA20k
presenter uses the enrolled ``ACTIVE_RETAIL_RGB565_PRESENTATION`` codebook in
``src/render/native_surface_format.rs``), but both keep the unit in the top
5/6/5 bits, so ``r >> 3``, ``g >> 2``, ``b >> 3`` recovers each surface unit
exactly. Differences are therefore reported in RGB565 units, never in 8-bit
display values.

The capture side is a ``--shell-capture`` bundle (``capture.json`` plus the
BGRA8 ``frame.bgra`` it names, checked against the manifest's SHA-256 or, for
older schemas without one, its byte length). The native side is a PNG
screenshot of the same client area (8-bit RGB/RGBA or 1/2/4/8-bit palette,
non-interlaced). Masked rectangles are excluded, for example a cursor the
native screenshot lacks or a region the comparison does not cover.

Usage:
    python -m tools.shell_capture_diff --capture DIR --native PNG \
        [--mask label=x,y,w,h ...] [--output report.json] [--diff-png out.png] \
        [--max-differing N] [--max-unit-difference D]
    python -m tools.shell_capture_diff --capture DIR --validate-only \
        [--output report.json]

Exit status: 0 when the comparison is within the given limits (no limits:
always 0), 1 when a limit is exceeded, 2 for invalid input.
"""

import argparse
import hashlib
import json
import struct
import sys
import zlib
from pathlib import Path

SCHEMA = "vera20k.shell-capture-diff.v1"
CAMPAIGN_SCHEMA = "vera20k.campaign-start-capture.v1"
CAMPAIGN_CHECKPOINTS = {
    f"campaign-{campaign}-{phase}": (campaign, phase)
    for campaign in ("all1", "sov1")
    for phase in ("loading-first-frame", "first-live-frame", "abort-return")
}
CAMPAIGN_CHECKPOINTS["campaign-back-return"] = (None, "campaign-back-return")
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
TILE = 50


class InputError(Exception):
    pass


def _require(condition, message):
    if not condition:
        raise InputError(message)


def _integer(value):
    return isinstance(value, int) and not isinstance(value, bool)


def _hex_hash(value, digits):
    return isinstance(value, str) and len(value) == digits and all(
        char in "0123456789abcdef" for char in value
    )


def validate_campaign_capture(manifest):
    """Check route/owner receipt consistency; this does not assert native parity."""
    checkpoint = manifest.get("checkpoint")
    schema = manifest.get("schema_version")
    if checkpoint not in CAMPAIGN_CHECKPOINTS and schema != CAMPAIGN_SCHEMA:
        return None
    _require(schema == CAMPAIGN_SCHEMA, "campaign checkpoint has no campaign receipt schema")
    _require(checkpoint in CAMPAIGN_CHECKPOINTS, "unknown campaign receipt checkpoint")
    campaign_id, phase = CAMPAIGN_CHECKPOINTS[checkpoint]
    _require(manifest.get("checkpoint_phase") == phase, "campaign checkpoint phase mismatch")
    _require(manifest.get("parity_certification") == "NONE", "capture is not a parity certificate")
    _require(_hex_hash(manifest["frame"].get("sha256"), 64)
             and _integer(manifest["frame"].get("byte_length")),
             "campaign readback byte receipt missing")
    frame = manifest["capture_frame"]
    _require(_integer(frame) and frame > 0, "campaign capture has no presented frame ordinal")
    route = manifest["route"]
    _require(isinstance(route, list) and all(isinstance(row, dict) for row in route),
             "campaign route is not an ordered receipt")
    route_frames = [row.get("frame") for row in route]
    _require(all(_integer(value) and 0 <= value <= frame for value in route_frames),
             "campaign route contains an invalid presented frame")
    _require(route_frames == sorted(route_frames), "campaign route frame order changed")

    def route_action(action, dialog=None, after=-1):
        for index, row in enumerate(route):
            if index > after and row.get("action") == action and (
                dialog is None or row.get("dialog") == dialog
            ):
                return index, row
        raise InputError(f"campaign route omitted {action}")

    main_index, _ = route_action("SinglePlayer", 0xE2)
    new_index, _ = route_action("NewCampaign", 0x100, main_index)
    if campaign_id is None:
        back_index, _ = route_action("Back", 0x94, new_index)
        route_action("Back returned", 0x100, back_index)
        returned = manifest["shell_return"]
        _require(returned.get("screen") == "MainMenu" and returned.get("route") == "SinglePlayer"
                 and returned.get("campaign_page_present") is False
                 and returned.get("loading_session_present") is False,
                 "campaign Back did not retire the campaign/loading route")
        return {"phase": phase, "parity_certification": "NONE"}

    press_index, press = route_action("emblem press", 0x94, new_index)
    release_index, release = route_action("emblem release", 0x94, press_index)
    admitted_index, _ = route_action("campaign loading admitted", after=release_index)
    emblem = {"all1": 0x6EA, "sov1": 0x6EC}[campaign_id]
    _require(press.get("emblem") == emblem and release.get("emblem") == emblem
             and press.get("campaign") == campaign_id and release.get("campaign") == campaign_id,
             "campaign route selected a different emblem")
    _require(press["frame"] < release["frame"], "emblem press was not presented before release")
    startup = manifest["startup"]
    campaign = startup["campaign"]
    _require(str(campaign["id"]).lower() == campaign_id, "admitted campaign differs from emblem")
    _require(_integer(campaign["index"]) and campaign["index"] >= 0 and _integer(campaign["cd"]),
             "campaign registry admission is missing index/CD")
    difficulty = startup["difficulty"]
    _require(_integer(difficulty) and 0 <= difficulty <= 2
             and press.get("difficulty") == difficulty and release.get("difficulty") == difficulty,
             "campaign difficulty changed between emblem admission and loading")
    seed = startup["seed"]
    _require(_integer(seed["value"]) and 0 <= seed["value"] <= 0xFFFFFFFF
             and isinstance(seed["source"], str) and isinstance(seed["seed_authority_certifying"], bool),
             "campaign has no seed authority receipt")
    source = manifest["selected_map_source"]
    _require(isinstance(campaign["scenario"], str) and campaign["scenario"],
             "selected campaign filename is missing")
    _require(manifest.get("map_admission") == "prepared" and isinstance(source, dict),
             "prepared campaign map byte receipt is missing")
    _require(source.get("kind") in ("loose", "mix"), "campaign map is a generated/fallback source")
    _require(_integer(source["payload_len"]) and source["payload_len"] > 0
             and _hex_hash(source["source_sha256"], 64), "campaign map byte identity is missing")
    selected_name = source["logical_name"] if source["kind"] == "mix" else source["path"]
    _require(isinstance(selected_name, str), "selected map filename is missing")
    selected_name = selected_name.replace("\\", "/").rsplit("/", 1)[-1].lower()
    scenario_name = campaign["scenario"].replace("\\", "/").rsplit("/", 1)[-1].lower()
    _require(selected_name == scenario_name, "map byte source differs from the admitted scenario")
    if source["kind"] == "mix":
        _require(isinstance(source["source_archive"], str) and source["source_archive"]
                 and _integer(source["entry_id"]), "MIX map source provenance is missing")
    hashes = manifest["process_source_ini_hashes"]
    _require("parsed cache" in hashes["domain"], "process hashes are not labelled as parsed cache")
    sources = hashes["sources"]
    _require(isinstance(sources, list) and [row["name"] for row in sources]
             == ["RULESMD.INI", "LANGRULE.INI", "ARTMD.INI"], "process source receipt changed")
    _require(all(_hex_hash(row["parsed_cache_hash"], 16) or (
        row["name"] == "LANGRULE.INI" and row["parsed_cache_hash"] is None
    ) for row in sources), "process parsed-cache hash missing")
    runtime = manifest["runtime"]
    if phase == "loading-first-frame":
        _require(runtime is None, "first loading capture claims an installed runtime")
    else:
        _require(isinstance(runtime, dict) and _integer(runtime.get("tick")) and runtime.get("tick") == 0
                 and runtime.get("game_mode_nonzero") is False,
                 "first live campaign capture is not before simulation tick zero")
        _require(_integer(runtime["campaign_mission_counter"]), "campaign counter missing")
        rows = runtime["campaign_difficulty_rows"]
        _require(rows["player"] == difficulty and rows["computer"] == 2 - difficulty,
                 "campaign House difficulty rows differ from admission")
        roster = runtime["house_roster"]
        _require(isinstance(roster, list) and roster, "campaign House registration order missing")
        ids = [house["native_unique_id"] for house in roster]
        _require(all(_integer(value) for value in ids) and len(ids) == len(set(ids)),
                 "campaign House native identities are missing/duplicated")
        current = runtime["current_house"]
        _require(sum(house["name"] == current["name"] and house["native_unique_id"]
                     == current["native_unique_id"] for house in roster) == 1,
                 "current House is not the registered native identity")
        _require(all(len(house["scalar_difficulty_bits"]) == 9 and all(
            _hex_hash(value, 16) for value in house["scalar_difficulty_bits"]
        ) for house in roster), "House scalar difficulty receipt missing")
        _require(_integer(runtime["native_identity_cursor"]), "native identity cursor missing")
        for name in ("main", "scenario", "mapgen"):
            rng = runtime["rng"][name]
            _require(_integer(rng["disabled"]) and _integer(rng["index_a"])
                     and _integer(rng["index_b"]) and len(rng["words"]) == 250
                     and all(_integer(word) and 0 <= word <= 0xFFFFFFFF for word in rng["words"]),
                     f"{name} RNG owner receipt missing")
        rules = runtime["active_rules"]
        _require(_hex_hash(rules["source_ini_hash"], 16)
                 and _hex_hash(rules["simulation_config_hash"], 16), "active Rules receipt missing")
        camera = runtime["camera"]
        _require(len(camera["world_pixel_origin"]) == 2 and len(camera["tactical_centre_cell"]) == 2
                 and len(camera["view_bookmarks"]) == 4
                 and all(isinstance(cell, list) and len(cell) == 2 for cell in camera["view_bookmarks"]),
                 "campaign camera/bookmark receipt missing")
    if phase == "abort-return":
        _require(manifest.get("runtime_observation") == "first-live-frame-before-Escape-press",
                 "abort return has no pre-Escape installed-runtime observation")
        index = admitted_index
        observations = {}
        for action, dialog in (
            ("first live observed", None), ("Escape press", 0xB5), ("Escape release", 0xB5),
            ("pause menu presented", 0xB5), ("Abort press", 0xB5), ("Abort release", 0xB5),
            ("abort modal presented", 0xB6), ("Leave press", 0xB6), ("Leave release", 0xB6),
            ("EXIT queued", None), ("EXIT consumed", None), ("abort returned", 0xE2),
        ):
            index, row = route_action(action, dialog, index)
            observations[action] = row
        _require(observations["first live observed"].get("tick") == runtime["tick"],
                 "abort started after the first installed campaign")
        for control in ("Escape", "Abort", "Leave"):
            _require(observations[f"{control} press"]["frame"]
                     < observations[f"{control} release"]["frame"],
                     f"{control} press was not presented before release")
        for control in ("Abort", "Leave"):
            press = observations[f"{control} press"]
            point, rect = press["point"], press["rect"]
            _require(isinstance(point, list) and len(point) == 2
                     and isinstance(rect, list) and len(rect) == 4
                     and all(_integer(value) for value in point + rect),
                     f"{control} has no physical hit geometry receipt")
            x, y, width, height = rect
            _require(width > 0 and height > 0 and x <= point[0] < x + width
                     and y <= point[1] < y + height, f"{control} press missed its loaded rectangle")
        queued = observations["EXIT queued"]
        consumed = observations["EXIT consumed"]
        returned = observations["abort returned"]
        _require(_integer(queued["queued_at_tick"]) and queued["queued_at_tick"] == 0
                 and _integer(queued["execute_tick"]) and queued["execute_tick"] >= queued["queued_at_tick"]
                 and queued["owner"] == runtime["current_house"]["name"]
                 and queued["owner_native_unique_id"] == runtime["current_house"]["native_unique_id"]
                 and queued["modal_closed"] is True and queued["diagnostic_simulation_freeze"] is False
                 and queued["quit_requested"] is False,
                 "Leave did not release pause and queue the current House's ordinary EXIT")
        _require(consumed["frame"] > queued["frame"] and _integer(consumed["tick"])
                 and consumed["tick"] >= queued["execute_tick"]
                 and consumed["scenario_exit_present"] is True
                 and consumed["scenario_outcome_present"] is False and consumed["quit_requested"] is True,
                 "abort return did not observe the queued EXIT's ordinary cascade")
        cleanup = manifest["cleanup"]
        _require(cleanup.get("screen") == "MainMenu" and cleanup.get("route") == "MainMenu"
                 and cleanup.get("in_game_menu") == "Closed", "abort did not settle on the ordinary main menu")
        _require(all(cleanup.get(key) is False for key in (
            "loading_session_present", "loading_campaign_startup_present", "accepted_match_startup_present",
            "match_startup_receipt_present", "asset_manager_leased", "campaign_page_present",
            "fullscreen_movie_present", "scenario_exit_present", "scenario_outcome_present",
        )), "campaign abort retained startup/loading/modal/exit owners")
        _require(cleanup.get("asset_manager_available") is True
                 and cleanup.get("retained_runtime_present") is True
                 and cleanup.get("retained_quit_requested") is True
                 and _integer(cleanup["campaign_mission_counter"]) and cleanup["campaign_mission_counter"] == 1
                 and _integer(cleanup["pending_exit_commands"]) and cleanup["pending_exit_commands"] == 0
                 and _integer(cleanup["retained_tick"]) and cleanup["retained_tick"] >= consumed["tick"]
                 and returned["tick"] == cleanup["retained_tick"]
                 and returned["campaign_mission_counter"] == cleanup["campaign_mission_counter"],
                 "campaign abort omitted EXIT consumption/lease return/retained Session counter reset")
    _require("mission_opening_1308" in manifest["coverage"], "capture omits its mission-opening limit")
    return {"campaign": campaign_id, "phase": phase, "scenario": campaign["scenario"],
            "selected_map_sha256": source["source_sha256"],
            "map_admission": manifest["map_admission"], "parity_certification": "NONE"}


def _paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def read_png_rgb(data):
    """Decode PNG bytes to (width, height, [(r, g, b), ...] row-major)."""
    if data[:8] != PNG_SIGNATURE:
        raise InputError("not a PNG file")
    pos = 8
    header = None
    palette = None
    idat = []
    while pos + 8 <= len(data):
        length, kind = struct.unpack(">I4s", data[pos : pos + 8])
        body = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"PLTE":
            palette = [tuple(body[i : i + 3]) for i in range(0, len(body), 3)]
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
    if header is None:
        raise InputError("PNG has no IHDR")
    width, height, depth, color, _, _, interlace = header
    if interlace != 0:
        raise InputError("interlaced PNGs are not supported")
    channels = {2: 3, 3: 1, 6: 4}.get(color)
    if channels is None or (color != 3 and depth != 8) or depth not in (1, 2, 4, 8):
        raise InputError(f"unsupported PNG color type {color} depth {depth}")
    if color == 3 and palette is None:
        raise InputError("palette PNG has no PLTE")
    bits_per_pixel = depth * channels
    stride = (width * bits_per_pixel + 7) // 8
    step = max(1, bits_per_pixel // 8)
    raw = zlib.decompress(b"".join(idat))
    if len(raw) != height * (stride + 1):
        raise InputError("PNG image data has the wrong length")
    pixels = []
    previous = bytearray(stride)
    for y in range(height):
        start = y * (stride + 1)
        kind = raw[start]
        line = bytearray(raw[start + 1 : start + 1 + stride])
        for i in range(stride):
            left = line[i - step] if i >= step else 0
            up = previous[i]
            up_left = previous[i - step] if i >= step else 0
            if kind == 1:
                line[i] = (line[i] + left) & 0xFF
            elif kind == 2:
                line[i] = (line[i] + up) & 0xFF
            elif kind == 3:
                line[i] = (line[i] + ((left + up) >> 1)) & 0xFF
            elif kind == 4:
                line[i] = (line[i] + _paeth(left, up, up_left)) & 0xFF
            elif kind != 0:
                raise InputError(f"unknown PNG filter {kind}")
        previous = line
        if color == 3:
            per_byte = 8 // depth
            mask = (1 << depth) - 1
            for x in range(width):
                byte = line[x // per_byte]
                shift = 8 - depth * (x % per_byte + 1)
                pixels.append(palette[(byte >> shift) & mask])
        else:
            for x in range(width):
                offset = x * channels
                pixels.append(tuple(line[offset : offset + 3]))
    return width, height, pixels


def write_png_rgb(path, width, height, pixels):
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for r, g, b in pixels[y * width : (y + 1) * width]:
            rows.extend((r, g, b))

    def chunk(kind, body):
        crc = zlib.crc32(kind + body) & 0xFFFFFFFF
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", crc)

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    Path(path).write_bytes(
        PNG_SIGNATURE
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(bytes(rows), 9))
        + chunk(b"IEND", b"")
    )


def read_capture(directory):
    """Read a shell-capture bundle as (manifest, SHA256, width, height, RGB pixels)."""
    directory = Path(directory)
    manifest = json.loads((directory / "capture.json").read_text(encoding="utf-8"))
    validate_campaign_capture(manifest)
    surface = manifest["surface"]
    frame = manifest["frame"]
    if surface.get("pixel_layout") != "BGRA8" or surface.get("row_order") != "top-left":
        raise InputError("capture frame is not top-left BGRA8")
    width, height, stride = surface["width"], surface["height"], surface["row_stride"]
    raw = (directory / frame["path"]).read_bytes()
    if "sha256" in frame and hashlib.sha256(raw).hexdigest() != frame["sha256"]:
        raise InputError("capture frame does not match its manifest SHA-256")
    if "byte_length" in frame and len(raw) != frame["byte_length"]:
        raise InputError("capture frame does not match its manifest byte length")
    if len(raw) != stride * height or stride < width * 4:
        raise InputError("capture frame has the wrong length")
    pixels = []
    for y in range(height):
        row = raw[y * stride : y * stride + width * 4]
        for x in range(width):
            b, g, r = row[x * 4 : x * 4 + 3]
            pixels.append((r, g, b))
    return manifest, hashlib.sha256(raw).hexdigest(), width, height, pixels


def rgb565_units(pixel):
    r, g, b = pixel
    return r >> 3, g >> 2, b >> 3


def parse_mask(text):
    label, _, rect = text.rpartition("=")
    try:
        x, y, w, h = (int(value) for value in rect.split(","))
    except ValueError as error:
        raise InputError(f"mask {text!r} is not [label=]x,y,w,h") from error
    if w <= 0 or h <= 0:
        raise InputError(f"mask {text!r} is empty")
    return {"label": label or None, "rect": [x, y, w, h]}


def compare(width, height, rust, native, masks):
    """Return (report fields, diff image pixels) for two same-size RGB frames."""
    rects = [mask["rect"] for mask in masks]

    def masked(x, y):
        return any(mx <= x < mx + mw and my <= y < my + mh for mx, my, mw, mh in rects)

    histogram = {}
    tiles = {}
    bounds = None
    compared = 0
    differing = 0
    max_unit = 0
    diff = []
    for y in range(height):
        for x in range(width):
            if masked(x, y):
                diff.append((0, 0, 64))
                continue
            compared += 1
            a = rgb565_units(rust[y * width + x])
            b = rgb565_units(native[y * width + x])
            unit = max(abs(a[0] - b[0]), abs(a[1] - b[1]), abs(a[2] - b[2]))
            if unit == 0:
                diff.append((0, 0, 0))
                continue
            differing += 1
            max_unit = max(max_unit, unit)
            histogram[unit] = histogram.get(unit, 0) + 1
            tile = (x // TILE * TILE, y // TILE * TILE)
            tiles[tile] = tiles.get(tile, 0) + 1
            if bounds is None:
                bounds = [x, y, x, y]
            else:
                bounds = [min(bounds[0], x), min(bounds[1], y), max(bounds[2], x), max(bounds[3], y)]
            diff.append((255, 0, 0) if unit == 1 else (255, 255, 255))
    report = {
        "compared_pixels": compared,
        "differing_pixels": differing,
        "max_unit_difference": max_unit,
        "unit_difference_histogram": {str(k): histogram[k] for k in sorted(histogram)},
        # Inclusive pixel bounds of every difference: [x0, y0, x1, y1].
        "differing_bounds": bounds,
        "differing_tiles": [
            {"x": x, "y": y, "size": TILE, "pixels": count}
            for (x, y), count in sorted(tiles.items(), key=lambda item: (-item[1], item[0]))
        ],
    }
    return report, diff


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--capture", required=True, help="shell-capture bundle directory")
    parser.add_argument("--native", help="native screenshot PNG (required for comparison)")
    parser.add_argument("--validate-only", action="store_true", help="check bytes/receipt without native comparison")
    parser.add_argument("--mask", action="append", default=[], help="[label=]x,y,w,h")
    parser.add_argument("--output", help="write the JSON report here (default: stdout)")
    parser.add_argument("--diff-png", help="write a difference map PNG here")
    parser.add_argument("--max-differing", type=int)
    parser.add_argument("--max-unit-difference", type=int)
    args = parser.parse_args(argv)
    if not args.validate_only and not args.native:
        parser.error("--native is required unless --validate-only is used")
    if args.validate_only and (args.native or args.mask or args.diff_png
                              or args.max_differing is not None or args.max_unit_difference is not None):
        parser.error("--validate-only accepts --capture and --output only")
    try:
        masks = [parse_mask(text) for text in args.mask]
        manifest, frame_sha256, width, height, rust = read_capture(args.capture)
        if args.validate_only:
            report = {"schema": "vera20k.shell-capture-validation.v1",
                      "checkpoint": manifest.get("checkpoint"), "frame_sha256": frame_sha256,
                      "size": [width, height], "campaign": validate_campaign_capture(manifest),
                      "parity_certification": "NONE"}
            text = json.dumps(report, indent=2) + "\n"
            if args.output:
                Path(args.output).write_text(text, encoding="utf-8")
            else:
                sys.stdout.write(text)
            return 0
        native_bytes = Path(args.native).read_bytes()
        native_width, native_height, native = read_png_rgb(native_bytes)
        if (native_width, native_height) != (width, height):
            raise InputError(
                f"native {native_width}x{native_height} does not match capture {width}x{height}"
            )
    except (InputError, OSError, KeyError, TypeError, ValueError, zlib.error) as error:
        print(f"invalid input: {error}", file=sys.stderr)
        return 2
    fields, diff = compare(width, height, rust, native, masks)
    report = {
        "schema": SCHEMA,
        "domain": "rgb565-units",
        "capture": {
            "checkpoint": manifest.get("checkpoint"),
            "capture_frame": manifest.get("capture_frame"),
            "frame_sha256": frame_sha256,
        },
        "native": {
            "file": Path(args.native).name,
            "sha256": hashlib.sha256(native_bytes).hexdigest(),
        },
        "size": [width, height],
        "masks": masks,
        **fields,
    }
    text = json.dumps(report, indent=2) + "\n"
    if args.output:
        Path(args.output).write_text(text, encoding="utf-8")
    else:
        sys.stdout.write(text)
    if args.diff_png:
        write_png_rgb(args.diff_png, width, height, diff)
    exceeded = (
        args.max_differing is not None and fields["differing_pixels"] > args.max_differing
    ) or (
        args.max_unit_difference is not None
        and fields["max_unit_difference"] > args.max_unit_difference
    )
    return 1 if exceeded else 0


if __name__ == "__main__":
    sys.exit(main())
