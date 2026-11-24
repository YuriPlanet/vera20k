"""Native HSV byte conversion, without a Python copy of the conversion formula.

Run from the repository root: python -m tools.color_oracle.hsv_to_rgb --check
Use --write explicitly to generate/update the reviewed native golden.

Ghidra body/caller inspection, 2026-09-08, /gamemd.exe, x86 image base 0x400000:
  HSV_To_RGB 0x00517440..0x00517554 reads ECX+[0,1,2], writes three bytes
  through its single stack argument, returns that pointer, and RET 4 cleans it.
  This leaf has no calls, external globals, floating-point, or OS dependencies.
  ProgressMeterClass__DrawFill calls it at 0x006434B5 with ECX=scheme+0x308
  and a stack RGB destination when meter+0x71 is set. The active non-dialog
  loading path reaches DrawFill through 0x00643AE0 -> 0x00643720; see
  docs/research/LOADING_BAR_COLOR_VERIFICATION_2026_05_30.md for that path.
  ColorScheme__BuildRampPalette also calls it at 0x0068C475, then copies the
  three output bytes to palette entries 16..31 in its 16-iteration loop.

Production Rust: rules::color_scheme::hsv_to_rgb, directly called by
app/loading/pump.rs::resolve_player_colors and house_colors::build_scheme_ramp.
This comparison proves neither the callers' input selection nor final GPU pixels.
"""

from pathlib import Path

from tools.native_oracle import SCRATCH, call, finish_vectors, provenance

HSV_TO_RGB = 0x00517440
HSV = SCRATCH
RGB = SCRATCH + 0x10
SV_PAIRS = (
    (0, 255),  # achromatic
    (1, 255),  # minimum nonzero saturation
    (127, 128),
    (128, 127),  # adjacent midrange rounding
    (254, 255),
    (255, 0),  # black
    (255, 1),  # minimum nonzero value
    (255, 254),
    (255, 255),
)


def native_rgb(hsv: bytes) -> bytes:
    # Guard both sides of the destination and confirm the original input survives.
    guard = b"\xa5" + b"\xcc" * 3 + b"\x5a"
    result = call(
        HSV_TO_RGB,
        ecx=HSV,
        stack_args=[RGB],
        writes={HSV: hsv, RGB - 1: guard},
        dumps={"input": (HSV, 3), "guarded_rgb": (RGB - 1, 5)},
        timeout_instr=1000,
        context={"case": "HSV_To_RGB", "input_hsv": list(hsv)},
    )
    output = bytes.fromhex(result["dumps"]["guarded_rgb"])
    if result["eax"] != RGB or output[0] != guard[0] or output[-1] != guard[-1]:
        raise RuntimeError(f"native HSV calling convention/output extent failed: {hsv.hex()}")
    if bytes.fromhex(result["dumps"]["input"]) != hsv:
        raise RuntimeError(f"native HSV input unexpectedly mutated: {hsv.hex()}")
    return output[1:4]


def generate() -> dict:
    # Every hue includes every sextant boundary and the 255 -> red wrap. Each
    # call gets a fresh emulator via the shared runner, preventing state leakage.
    return {
        "source": "unicorn/gamemd.exe",
        "function": hex(HSV_TO_RGB),
        "cases": [
            {
                "saturation": saturation,
                "value": value,
                "rgb_by_hue": b"".join(
                    native_rgb(bytes((hue, saturation, value))) for hue in range(256)
                ).hex(),
            }
            for saturation, value in SV_PAIRS
        ],
    }


if __name__ == "__main__":
    finish_vectors(
        generate,
        Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="2304 HSV byte inputs: every hue at nine saturation/value pairs; not all 256^3 inputs",
            assumptions=[
                "ECX points to three HSV bytes; one stack argument points to writable three-byte RGB output",
                "fresh emulator per case; destination guards and unchanged source checked",
                "pure integer leaf; no runtime global initialization or floating-point assumptions required",
                "caller input selection, ramp modulation, display packing and GPU output are outside this comparison",
            ],
            substitutions=[],
            entry_points={"HSV_To_RGB": HSV_TO_RGB},
        ),
    )
