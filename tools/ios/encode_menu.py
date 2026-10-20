"""Losslessly package the approved branded menu video as PNG/XOR frames.

Requires ffmpeg, Pillow and numpy. This only converts the playback format: no
cropping, retiming, resizing or visual editing is performed. Every restored RGBA
frame is checked against ffmpeg's decoded output before the archive is installed.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile

import numpy as np
from PIL import Image


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--replace", action="store_true")
    args = parser.parse_args()
    if args.output.exists() and not args.replace:
        parser.error("output exists; pass --replace explicitly")
    width, height, fps, count, key_interval = 632, 570, 15, 431, 15
    args.output.parent.mkdir(parents=True, exist_ok=True)
    header = struct.pack("<8s6I", b"VWMV0001", width, height, fps, 1, count, key_interval)
    frame_bytes = width * height * 4
    offsets = [0]
    previous = np.zeros(frame_bytes, dtype=np.uint8)
    decoded_digest = hashlib.sha256()
    process = subprocess.Popen(
        [args.ffmpeg, "-v", "error", "-i", str(args.source), "-map", "0:v:0",
         "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"],
        stdout=subprocess.PIPE,
    )
    assert process.stdout is not None
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(dir=args.output.parent, suffix=".frames.tmp", delete=False) as out:
            temporary = Path(out.name)
            out.write(header)
            out.write(bytes((count + 1) * 8))
            for index in range(count):
                chunks = bytearray()
                while len(chunks) < frame_bytes:
                    chunk = process.stdout.read(frame_bytes - len(chunks))
                    if not chunk:
                        raise ValueError(f"frame {index} truncated: {len(chunks)} / {frame_bytes}")
                    chunks.extend(chunk)
                current = np.frombuffer(chunks, dtype=np.uint8)
                delta = current if index % key_interval == 0 else np.bitwise_xor(current, previous)
                png = io.BytesIO()
                Image.fromarray(delta.reshape(height, width, 4)).save(
                    png, format="PNG", compress_level=6, optimize=False,
                )
                payload = png.getvalue()
                # Transparent delta pixels still carry significant RGB bytes.
                restored = np.asarray(Image.open(io.BytesIO(payload)).convert("RGBA")).reshape(-1)
                if index % key_interval:
                    restored = np.bitwise_xor(restored, previous)
                if not np.array_equal(restored, current):
                    raise ValueError(f"frame {index}: PNG/XOR roundtrip changed pixels")
                decoded_digest.update(chunks)
                out.write(payload)
                offsets.append(offsets[-1] + len(payload))
                previous = current.copy()
            if process.stdout.read(1):
                raise ValueError("source has more than the expected 431 frames")
            if process.wait() != 0:
                raise ValueError("ffmpeg decoding failed")
            out.seek(len(header))
            out.write(struct.pack(f"<{count + 1}Q", *offsets))
            out.flush()
            os.fsync(out.fileno())
        temporary.replace(args.output)
        args.output.chmod(0o644)
        temporary = None
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait()
        process.stdout.close()
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    print(json.dumps({
        "source": str(args.source), "source_sha256": hashlib.sha256(args.source.read_bytes()).hexdigest(),
        "archive": str(args.output), "archive_sha256": hashlib.sha256(args.output.read_bytes()).hexdigest(),
        "archive_bytes": args.output.stat().st_size, "width": width, "height": height,
        "fps": fps, "frames": count, "key_interval": key_interval,
        "decoded_rgba_sha256": decoded_digest.hexdigest(), "roundtrip": "431 frames, every RGBA byte identical",
    }, indent=2))


if __name__ == "__main__":
    main()
