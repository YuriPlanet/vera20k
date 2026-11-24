"""Original586E50 map-diamond correction used by House501AC0.

Supplied sparse Cell table and map bounds; no callable substitutions. Preserve
the returned packed Cell and final shared-Dummy coordinate, not just geometry.
"""
import struct
from pathlib import Path

from tools.native_oracle import call, finish_vectors, provenance
from tools.spatial_oracle.map_queries import (
    MAP, INPUT, DUMMY, ALLOCATED, packed, state,
)

OUTPUT = INPUT + 16


def query(xy, bounds, dummy):
    writes = state(bounds, dummy)
    writes[INPUT] = packed(*xy)
    writes[OUTPUT] = packed(123, 456)
    result = call(0x00586E50, ecx=MAP, stack_args=[OUTPUT, INPUT],
                  writes=writes, timeout_instr=100000,
                  dumps={"output": (OUTPUT, 4), "input": (INPUT, 4),
                         "dummy": (DUMMY + 0x24, 4)})
    if result["eax"] != OUTPUT:
        raise RuntimeError("clamp did not return its caller's output pointer")
    if result["dumps"]["input"] != writes[INPUT].hex():
        raise RuntimeError("clamp changed its caller's source Cell")
    decode = lambda key: list(struct.unpack("<hh", bytes.fromhex(result["dumps"][key])))
    return dict(xy=xy, bounds=bounds, dummy=dummy,
                output=decode("output"), final_dummy=decode("dummy"))


def generate():
    bounds = [80, 2, 4, 76, 48]
    coordinates = [
        [40, 48], [44, 44], [43, 44], [42, 44], [45, 44],
        [90, 90], [94, 94], [95, 95], [96, 96],
        [80, 4], [81, 4], [79, 4], [4, 80], [4, 81], [4, 79],
        [0, 0], [10, 10], [50, 40], [80, 50],
        [-3, 0], [0, -3], [511, 511], [512, 0], [-1, 512],
    ]
    return [query(xy, bounds, dummy)
            for dummy in [[0, 0], [-7, 1], [7, 4]] for xy in coordinates]


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope="Original Map586E50 through RET8, including actual578460 height-aware loop. "
              "Supplied sparse map geometry; not House random-cell producer or Rust parity.",
        assumptions=[f"Sparse allocated (x,y,level,slope) cells: {ALLOCATED}.",
                     "ECX owns supplied Size/LocalSize; global Cell table and retained Dummy "
                     "match tools.spatial_oracle.map_queries.state. Source input and output "
                     "are separate caller-owned CellStructs. No RNG or House state involved."],
        substitutions=[], entry_points={"clamp": 0x00586E50, "predicate": 0x00578460}))
