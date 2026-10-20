#!/usr/bin/env python3
"""Build the dependency-free Android Traditional-to-Simplified display map.

Run on macOS with an existing `asset csf-grep '' --limit 10000` JSON export.
The table contains generic character changes and only the contextual exceptions
needed to reproduce CoreFoundation for that source corpus. No retail binary is
replaced. Android does not require an ICU library or a network request at runtime.
"""

import argparse
import ctypes
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("export", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    cf = ctypes.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
    ref = ctypes.c_void_p
    cf.CFStringCreateWithCharacters.argtypes = [ref, ref, ctypes.c_long]
    cf.CFStringCreateWithCharacters.restype = ref
    cf.CFStringCreateMutableCopy.argtypes = [ref, ctypes.c_long, ref]
    cf.CFStringCreateMutableCopy.restype = ref
    cf.CFStringTransform.argtypes = [ref, ref, ref, ctypes.c_uint8]
    cf.CFStringTransform.restype = ctypes.c_uint8
    cf.CFStringGetLength.argtypes = [ref]
    cf.CFStringGetLength.restype = ctypes.c_long
    class Range(ctypes.Structure):
        _fields_ = [("location", ctypes.c_long), ("length", ctypes.c_long)]

    cf.CFStringGetCharacters.argtypes = [ref, Range, ref]
    cf.CFRelease.argtypes = [ref]

    def string(text):
        raw = text.encode("utf-16le")
        buffer = ctypes.create_string_buffer(raw)
        value = cf.CFStringCreateWithCharacters(None, buffer, len(raw) // 2)
        if not value:
            raise RuntimeError("CFStringCreateWithCharacters failed")
        return value

    transform = string("Traditional-Simplified")

    def convert(text):
        source = string(text)
        result = None
        try:
            result = cf.CFStringCreateMutableCopy(None, 0, source)
            if not result or not cf.CFStringTransform(result, None, transform, 0):
                raise RuntimeError("CFStringTransform failed")
            length = cf.CFStringGetLength(result)
            buffer = ctypes.create_string_buffer(length * 2)
            cf.CFStringGetCharacters(result, Range(0, length), buffer)
            return buffer.raw.decode("utf-16le")
        finally:
            if result:
                cf.CFRelease(result)
            cf.CFRelease(source)

    def normalized(text):
        return text.replace("猪㑩", "猪猡").replace("𫫇心", "恶心").replace("\u3000", " ")

    try:
        characters = {}
        for cp in range(0x20, 0x10000):
            if 0xD800 <= cp <= 0xDFFF:
                continue
            original = chr(cp)
            simplified = convert(original)
            if original != simplified:
                characters[original] = simplified
        corpus = json.loads(args.export.read_text())
        contexts = {}
        for row in corpus["entries"]:
            original = row["value"]
            expected = normalized(convert(original))
            projected = normalized("".join(characters.get(ch, ch) for ch in original))
            if projected != expected:
                contexts[original] = expected
        result = {
            "format": 1,
            "transform": "Traditional-Simplified",
            "generator": "tools/build_android_locale.py",
            "characters": characters,
            "contexts": dict(sorted(contexts.items())),
        }
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
        print(f"{args.output}: {len(characters)} character changes, {len(contexts)} contexts")
    finally:
        cf.CFRelease(transform)


if __name__ == "__main__":
    main()
