#!/usr/bin/env python3
"""Measure the vertical scroll rate of a scrolling shell screen from timed screenshots.

Input is a TSV of ``<png path>\t<capture time in seconds>`` rows in capture
order (for retail captures use the screenshot file's modification time, which
the writer sets when the frame is saved). For each consecutive pair the tool
finds the upward shift that best aligns the text rows of the second image with
the first, using a per-row profile of bright pixels inside a column band. Pairs
that align exactly are fitted to one rate; pairs that do not (scene changes,
too little overlap) are reported and excluded.

Usage:
    python -m tools.shell_scroll_rate --shots shots.tsv \
        [--columns 140:660] [--rows 40:560] [--output report.json]
"""

import argparse
import json
import sys
from pathlib import Path

from tools.shell_capture_diff import InputError, read_png_rgb

SCHEMA = "vera20k.shell-scroll-rate.v1"


def row_profile(path, columns):
    width, height, pixels = read_png_rgb(Path(path).read_bytes())
    x0, x1 = columns
    return [
        sum(1 for x in range(x0, min(x1, width)) if min(pixels[y * width + x][:2]) > 150)
        for y in range(height)
    ]


def best_shift(a, b, rows):
    """Upward shift d minimising |a[y] - b[y - d]| relative to the ink compared."""
    y0, y1 = rows
    best = None
    for shift in range(0, y1 - y0):
        pairs = [(a[y], b[y - shift]) for y in range(y0 + shift, y1) if y - shift >= y0]
        if len(pairs) < 60:
            break
        ink = sum(p + q for p, q in pairs)
        if ink == 0:
            continue
        error = sum(abs(p - q) for p, q in pairs) / ink
        if best is None or error < best[0]:
            best = (error, shift)
    return best


def parse_range(text):
    start, _, end = text.partition(":")
    return int(start), int(end)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--shots", required=True, help="TSV of png path and capture time")
    parser.add_argument("--columns", default="140:660", help="x range of the text band")
    parser.add_argument("--rows", default="40:560", help="y range compared (skip fades)")
    parser.add_argument("--output", help="write the JSON report here (default: stdout)")
    args = parser.parse_args(argv)
    try:
        shots = []
        for line in Path(args.shots).read_text(encoding="utf-8").splitlines():
            if line.strip():
                path, time = line.split("\t")[:2]
                shots.append((path, float(time)))
        columns, rows = parse_range(args.columns), parse_range(args.rows)
        profiles = [row_profile(path, columns) for path, _ in shots]
    except (InputError, OSError, ValueError) as error:
        print(f"invalid input: {error}", file=sys.stderr)
        return 2
    pairs = []
    for (first, second), (a, b) in zip(zip(shots, shots[1:]), zip(profiles, profiles[1:])):
        match = best_shift(a, b, rows)
        pairs.append(
            {
                "from": Path(first[0]).name,
                "to": Path(second[0]).name,
                "seconds": second[1] - first[1],
                "shift_px": match[1] if match else None,
                "error": match[0] if match else None,
            }
        )
    exact = [p for p in pairs if p["error"] == 0 and p["shift_px"]]
    total_px = sum(p["shift_px"] for p in exact)
    total_s = sum(p["seconds"] for p in exact)
    report = {
        "schema": SCHEMA,
        "pairs": pairs,
        "exact_pairs": len(exact),
        "px_per_second": total_px / total_s if total_s else None,
        "ms_per_2px": 2000 * total_s / total_px if total_px else None,
    }
    text = json.dumps(report, indent=2) + "\n"
    if args.output:
        Path(args.output).write_text(text, encoding="utf-8")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
