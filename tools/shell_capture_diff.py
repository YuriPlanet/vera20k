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
BGRA8 ``frame.bgra`` it names, hash-checked). The native side is a PNG
screenshot of the same client area (8-bit RGB/RGBA or 1/2/4/8-bit palette,
non-interlaced). Masked rectangles are excluded, for example a cursor the
native screenshot lacks or a region the comparison does not cover.

Usage:
    python -m tools.shell_capture_diff --capture DIR --native PNG \
        [--mask label=x,y,w,h ...] [--output report.json] [--diff-png out.png] \
        [--max-differing N] [--max-unit-difference D]

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
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
TILE = 50


class InputError(Exception):
    pass


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
    """Read a shell-capture bundle as (manifest, width, height, rgb pixels)."""
    directory = Path(directory)
    manifest = json.loads((directory / "capture.json").read_text(encoding="utf-8"))
    surface = manifest["surface"]
    frame = manifest["frame"]
    if surface.get("pixel_layout") != "BGRA8" or surface.get("row_order") != "top-left":
        raise InputError("capture frame is not top-left BGRA8")
    width, height, stride = surface["width"], surface["height"], surface["row_stride"]
    raw = (directory / frame["path"]).read_bytes()
    if hashlib.sha256(raw).hexdigest() != frame["sha256"]:
        raise InputError("capture frame does not match its manifest SHA-256")
    if len(raw) != stride * height or stride < width * 4:
        raise InputError("capture frame has the wrong length")
    pixels = []
    for y in range(height):
        row = raw[y * stride : y * stride + width * 4]
        for x in range(width):
            b, g, r = row[x * 4 : x * 4 + 3]
            pixels.append((r, g, b))
    return manifest, width, height, pixels


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
    parser.add_argument("--native", required=True, help="native screenshot PNG")
    parser.add_argument("--mask", action="append", default=[], help="[label=]x,y,w,h")
    parser.add_argument("--output", help="write the JSON report here (default: stdout)")
    parser.add_argument("--diff-png", help="write a difference map PNG here")
    parser.add_argument("--max-differing", type=int)
    parser.add_argument("--max-unit-difference", type=int)
    args = parser.parse_args(argv)
    try:
        masks = [parse_mask(text) for text in args.mask]
        manifest, width, height, rust = read_capture(args.capture)
        native_bytes = Path(args.native).read_bytes()
        native_width, native_height, native = read_png_rgb(native_bytes)
        if (native_width, native_height) != (width, height):
            raise InputError(
                f"native {native_width}x{native_height} does not match capture {width}x{height}"
            )
    except (InputError, OSError, KeyError, ValueError, zlib.error) as error:
        print(f"invalid input: {error}", file=sys.stderr)
        return 2
    fields, diff = compare(width, height, rust, native, masks)
    report = {
        "schema": SCHEMA,
        "domain": "rgb565-units",
        "capture": {
            "checkpoint": manifest.get("checkpoint"),
            "capture_frame": manifest.get("capture_frame"),
            "frame_sha256": manifest["frame"]["sha256"],
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
