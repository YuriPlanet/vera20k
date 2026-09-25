"""Original VeterancyClass::Add 0x0074FF50 over costs, awards and running totals.

Executes the whole body: `vet = points / (cost * Rules.VeteranRatio) + vet` on
the x87 stack, one `FST dword` store, then the clamp to `Rules.VeteranCap`
when the unrounded sum is not below it (`FCOMPP` + `TEST AH, 0x41`) and a
final `FSTP dword`. The call site is TechnoClass::Record_The_Kill's recipient
arm (`0x00702FF0`): `Add(cost, points)` on the recipient's `+0x150`, with the
recipient type's cost (vt+0x84) first and the award second.

Supplied: Rules (+0x668 VeteranRatio, +0x698 VeteranCap as binary64) and the
recipient's starting `f32`. Runs under the harness control word.

Rust consumer: src/sim/combat/veterancy.rs (accumulate).
"""
import struct
from pathlib import Path

from unicorn.x86_const import UC_X86_REG_ECX

from tools.native_oracle import (SCRATCH, call, finish_vectors, provenance)

ADD = 0x74FF50
RULES_PTR = 0x8871E0
RULES, VETERANCY = SCRATCH + 0x1000, SCRATCH + 0x3000


def execute(case):
    writes = {
        RULES_PTR: struct.pack("<I", RULES),
        RULES + 0x668: struct.pack("<d", case["ratio"]),
        RULES + 0x698: struct.pack("<d", case["cap"]),
        VETERANCY: struct.pack("<I", case["start_bits"]),
    }
    result = call(ADD, ecx=VETERANCY, stack_args=[case["cost"] & 0xFFFFFFFF,
                                                   case["points"] & 0xFFFFFFFF],
                  writes=writes, dumps={"vet": (VETERANCY, 4)})
    bits = struct.unpack("<I", bytes.fromhex(result["dumps"]["vet"]))[0]
    return dict(input=case, result_bits=f"{bits:08x}")


def f32_bits(value):
    return struct.unpack("<I", struct.pack("<f", value))[0]


def cases():
    # Single awards over the stock victim/recipient costs and rank multipliers.
    for cost in (900, 700, 600, 1500, 1750, 2000, 100, 1, 3, 7, 13):
        for points in (700, 1400, 2100, 900, 1800, 2700, 600, 200, 1, 3):
            yield dict(cost=cost, points=points, ratio=3.0, cap=2.0,
                       start_bits=f32_bits(0.0))
    # Other ratios and caps (mods), and a cap of 1.
    for ratio in (1.5, 2.2, 0.1, 7.0, 3.3):
        for cap in (2.0, 1.0, 3.5):
            yield dict(cost=900, points=700, ratio=ratio, cap=cap, start_bits=f32_bits(0.0))
    # Running totals: each start is a native-looking f32 near the boundaries.
    for start in (0.2592593, 0.5185185, 0.7777778, 0.9999999, 1.0, 1.0370370, 1.2962962,
                  1.5555556, 1.8148148, 1.9999999, 2.0, 0.3333333, 0.6666667, 1.3333334):
        for cost, points in ((900, 700), (700, 700), (900, 900), (3, 1), (7, 3)):
            yield dict(cost=cost, points=points, ratio=3.0, cap=2.0,
                       start_bits=f32_bits(start))
    # Zero and negative costs (the divide's infinities), zero points.
    for cost, points in ((0, 700), (-900, 700), (900, 0), (900, -700)):
        yield dict(cost=cost, points=points, ratio=3.0, cap=2.0, start_bits=f32_bits(0.5))


def chained():
    """Successive kills, each step fed the last: a Rhino killing Grizzlies and
    a GI killing GIs (the equal-cost third-per-kill case)."""
    rows = []
    for chain, cost, points, kills in (("rhino_grizzly", 900, 700, 12), ("gi_gi", 200, 200, 8)):
        bits = f32_bits(0.0)
        for kill in range(kills):
            row = execute(dict(cost=cost, points=points, ratio=3.0, cap=2.0, start_bits=bits,
                               chain=chain, step=kill + 1))
            rows.append(row)
            bits = int(row["result_bits"], 16)
    return rows


def generate():
    return [execute(case) for case in cases()] + chained()


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("VeterancyClass::Add 0x0074FF50 over stock and odd costs, awards, ratios and "
               "caps, running totals near the rank and cap boundaries, infinities from zero "
               "and negative costs, a chain of twelve Rhino-kills-Grizzly awards and eight GI-kills-GI awards."),
        assumptions=["x87 control word 0x0E7F (PC53, chop), the harness default.",
                     "Rules+0x668 VeteranRatio and +0x698 VeteranCap are supplied binary64."],
        substitutions=[],
        entry_points={"add": ADD},
    ))
