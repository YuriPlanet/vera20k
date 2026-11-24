"""Original Tactical inverse after supplied live bridge flag transitions.

Reuses the unchanged 6D6590 fixture and real lookup/projector leaves. The flag
states below are declared inputs, not an execution of collapse/repair itself;
47E040's writer is independently covered by bridge_body_publication. Each
query reuses the same native CellClass addresses after the flag replacement.
"""
from pathlib import Path
import json

from tools.bridge_click_oracle import NativeFixture
from tools.native_oracle import finish_vectors, provenance


def generate():
    rows = []
    points = [[0, 420], [-15, 420], [15, 420], [0, 435], [0, 480], [30, 420]]
    for direction, cells in [
        (0, [[16, 16], [16, 15], [16, 14], [16, 17]]),
        (6, [[16, 16], [15, 16], [14, 16], [17, 16]]),
    ]:
        fixture = NativeFixture([0, 0, 31, 31], 2, [], [])
        for phase, structural in [("intact", True), ("collapsed", False), ("repaired", True)]:
            flags = (0x100 if structural else 0x400) | (0x800 if direction == 0 else 0)
            for x, y in cells:
                fixture.put32(fixture.addresses[x, y] + 0x140, flags)
            rows.append({
                "direction": direction, "phase": phase, "flags": flags,
                "cells": cells,
                "cases": [{"world_input": point, **fixture.inverse(point, [0, 0], [0, 0])}
                          for point in points],
            })
    # Reuse the same CellClass identities and original matrix while changing
    # candidate/east/south level bytes. These are declared reader inputs, not
    # an execution of bridge collapse, tile replacement or repair publication.
    level_rows = []
    for direction, cells in [
        (0, [[16, 16], [16, 15], [16, 14], [16, 17]]),
        (6, [[16, 16], [15, 16], [14, 16], [17, 16]]),
    ]:
        fixture = NativeFixture([0, 0, 31, 31], 2, [], [])
        flags = 0x100 | (0x800 if direction == 0 else 0)
        for x, y in cells:
            fixture.put32(fixture.addresses[x, y] + 0x140, flags)
        for phase, writes in [
            ("initial", []),
            ("anchor_raised", [[16, 16, 6]]),
            ("east_raised", [[16, 16, 2], [17, 16, 6]]),
            ("south_raised", [[17, 16, 2], [16, 17, 6]]),
            ("restored", [[16, 17, 2]]),
        ]:
            for x, y, level in writes:
                fixture.uc.mem_write(fixture.addresses[x, y] + 0x11B, bytes([level]))
            before = {address: bytes(fixture.uc.mem_read(address, 0x148))
                      for address in fixture.addresses.values()}
            cases = [{"world_input": point, **fixture.inverse(point, [0, 0], [0, 0])}
                     for point in points]
            if any(bytes(fixture.uc.mem_read(address, 0x148)) != state
                   for address, state in before.items()):
                raise ValueError("Tactical inverse mutated a supplied CellClass")
            level_rows.append(dict(direction=direction, phase=phase, flags=flags,
                                   cells=cells, level_writes=writes, cases=cases))
    # Coherent upstream geometry for the existing click/shroud suffix
    # controls, which supply an already-resolved20,20 and screen registration.
    shroud_upstream_rows = []
    cells = [[20, 20, 0, 0], [19, 20, 0, 0], [18, 20, 0, 0], [21, 20, 0, 0]]
    for level in [0, 4]:
        fixture = NativeFixture([0, 0, 31, 31], level, cells, [])
        for x in [0, 15]:
            point = [x, 630 - 15 * (level + 4)]
            shroud_upstream_rows.append(dict(base_height=level, bridge_cells=cells,
                world_input=point, **fixture.inverse(point, [0, 0], [0, 0])))

    # Original full-body publication outputs are inputs to this reader
    # comparison. This generator does not execute their producer or loader.
    spatial = Path(__file__).parent / "spatial_oracle"
    supplied = json.loads((spatial / "bridge_middle_stock_inputs.json").read_text())
    produced = json.loads((spatial / "bridge_middle_tiles.json").read_text())
    middle_rows = []
    for index, bounds, point in [
        (0, [86, 45, 98, 57], [1260, 2085]),
        (6, [106, 113, 118, 125], [-240, 3405]),
    ]:
        fixture = NativeFixture(bounds, 0, [], [])
        for x, y, _tile, _sub, flags, _overlay, _state, _anchor, level, _land in supplied["cells"]:
            if (x, y) in fixture.addresses:
                address = fixture.addresses[x, y]
                fixture.uc.mem_write(address + 0x11B, bytes([level]))
                fixture.put32(address + 0x140, flags)
        before = fixture.inverse(point, [0, 0], [0, 0])
        body_index = 0 if index == 0 else 1
        case = produced["body_sequences"][body_index]
        for step in case["steps"]:
            for change in step["changed_cells"]:
                cell = change["after"]
                coord = tuple(cell["coord"])
                if coord in fixture.addresses:
                    address = fixture.addresses[coord]
                    fixture.uc.mem_write(address + 0x11B, bytes([cell["level"]]))
                    fixture.put32(address + 0x140, cell["flags"])
        middle_rows.append(dict(source_map=supplied["source"]["map"],
            source_map_sha256=supplied["source"]["map_sha256"],
            source_body_sequence=body_index, axis=case["axis"], bounds=bounds, world_input=point,
            before=before, after=fixture.inverse(point, [0, 0], [0, 0])))
    return {"source": "unicorn/gamemd.exe", "bounds": [0, 0, 31, 31],
            "base_height": 2, "anchor": [16, 16], "rows": rows,
            "level_rows": level_rows, "shroud_upstream_rows": shroud_upstream_rows,
            "middle_rows": middle_rows}


def metadata():
    return provenance(
        scope="104 original6D6590 queries:36 live-flag,60 live-level,4 coherent click upstream and4 native middle-publication snapshot controls",
        assumptions=[
            "Same binary, matrix/direction initialization and unchanged native leaves as tools/bridge_click_oracle.py.",
            "Synthetic32x32 flat ground level2; four supplied structural cells match anchor/F1/F2/opposite visitation for direction0 or6.",
            "Only relevant structural100, destroyed400 and direction800 inputs are supplied; flag publication is not executed here.",
            "Flag inputs are overwritten on the same CellClass identities between phases; no map or matrix reconstruction.",
            "The60 level controls write candidate/east/south Cell+11B values2/6 on the same identities; publication and retail occurrence are not executed here. All supplied CellClass bytes remain unchanged by each query group.",
            "Four coherent EW bridge queries with deck-height centre and infantry right-subcell anchors precede the separately supplied click_shroud suffix controls; no native drawing or whole pointer chain executes.",
            "Four middle queries consume original bridge_middle_tiles.json admitted576BA0 body snapshots and supplied C3 stock inputs, whose map SHA is recorded there; their producer, damage admission/RNG and loader do not execute in this reader comparison.",
            "All map lookups must resolve real mapped cells. No dummy/off-map behavior, UI delivery, movement or GPU proof.",
        ],
        substitutions=[], entry_points={"inverse": 0x6D6590, "map_lookup": 0x5657A0,
                                        "neighbor": 0x481810, "projector": 0x6D1F10},
    )


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix("") / "vectors.json", provenance=metadata)
