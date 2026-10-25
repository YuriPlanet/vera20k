"""Execute bounded Foot Z fixtures in the original gamemd image.

Run ``python -m tools.render_depth_oracle`` with RA2_DIR or
VERA20K_GAMEMD_EXE set. ``--check`` compares fresh native results to the saved
JSON; checking is the default, and only --write replaces the reference.
Import and --help do not load retail data. Unicorn is an already-installed
dependency; this tool downloads nothing.

This is raw-function emulation, not a game/session or rendered-pixel oracle.
Fixtures supply a Unit, its actual UnitType, ordinary mapped cells, and relocated
TMP headers. Native functions, virtual methods, cell lookup, TMP dimension
lookup, x87 AdjustForZ, and Foot max/add composition execute unmodified. Code
hooks observe addresses only; no native return values or branches are replaced.
The null locomotor contributes zero. Constructors, INI/map loading, active
tunnels, overlays, infantry-specific alternatives, and drawing are not covered.
No executable or retail art bytes are copied into the vector file.
"""

from __future__ import annotations

import hashlib
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, __version__ as unicorn_version
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EBX,
    UC_X86_REG_ESP, UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_EDI,
    UC_X86_REG_EFLAGS, UC_X86_REG_FPCW,
)

from tools.native_oracle import (
    NATIVE_FPCW, OracleError, load_image, run_checked,
    finish_vectors, provenance,
)


STANDARD_Z_MULTIPLIER_BITS = 0x3FC25E5374344960  # startup store 0x006D1BDD
MEM = 0x21000000
CELL_TABLE = MEM
CELLS = MEM + 0x100000
UNIT = MEM + 0x180000
UNIT_TYPE = MEM + 0x190000
TILE_REGISTRY = MEM + 0x1A0000
TILE_TYPES = MEM + 0x1A1000
TMP_HEADERS = MEM + 0x1C0000
TMP_CELLS = MEM + 0x1C8000
STACK = MEM + 0x1FFFF0
RETURN = 0x30000000
CURRENT = (2, 2)
CELL_STRIDE = 0x148
DEFAULT_CELL = {"level": 0, "ramp": 0, "flags": 0, "tile_index": 65535, "tmp_height": 30}
TARGETS = {
    "cliff_multiplier": 0x704240,
    "column_multiplier": 0x703E70,
    "tunnel_multiplier": 0x704000,
    "near_bridge": 0x703B10,
    "base_z_adjust": 0x704350,
    "foot_z_adjust": 0x4DAFC0,
}
REGIONS = {
    "direction_initializer": (0x49F2F0, 0x49F39C),
    "cliff_multiplier": (0x704240, 0x704343),
    "base_z_adjust": (0x704350, 0x7049BD),
    "foot_z_adjust": (0x4DAFC0, 0x4DB09F),
    "tmp_dimensions": (0x547150, 0x5471A3),
}


def signed(value: int) -> int:
    return ((value + 0x80000000) & 0xFFFFFFFF) - 0x80000000


class NativeFixture:
    def __init__(self) -> None:
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.uc)
        self.uc.mem_map(MEM, 0x200000)
        self.uc.mem_map(RETURN, 0x1000)
        self.region_hashes = {
            name: hashlib.sha256(bytes(self.uc.mem_read(start, end - start))).hexdigest()
            for name, (start, end) in REGIONS.items()
        }
        # Execute the retail initializer instead of copying inferred directions.
        self.call(0x49F2F0)
        self.directions = list(struct.iter_unpack("<hh", bytes(self.uc.mem_read(0x89F688, 32))))
        self.put32(0x87F924, CELL_TABLE)  # MapClass cell-pointer vector
        self.put32(0x87F928, 512 * 512)
        self.put32(0xA8ED2C, TILE_REGISTRY)  # IsoTileType** backing array
        self.put32(0xAA10B0, 0)  # clear-tile index for 0xFF/0xFFFF fallback
        self.uc.mem_write(0xB0CD48, struct.pack("<Q", STANDARD_Z_MULTIPLIER_BITS))
        self.uc.mem_write(0x822D80, struct.pack("<H", NATIVE_FPCW))
        for tile_index in range(65):
            tile_type = TILE_TYPES + tile_index * 0x400
            header = TMP_HEADERS + tile_index * 0x100
            tile = TMP_CELLS + tile_index * 0x100
            self.put32(TILE_REGISTRY + tile_index * 4, tile_type)
            self.put32(tile_type, 0x7ECC48)  # actual IsoTileType virtual methods
            self.put32(tile_type + 0xA4, header)
            # Already-relocated one-cell TMP: +0x10 is an absolute cell pointer.
            for offset, value in [(0, 1), (4, 1), (8, 60), (12, 30), (16, tile)]:
                self.put32(header + offset, value)
        for y in range(8):
            for x in range(8):
                self.put32(CELL_TABLE + (y * 512 + x) * 4, self.cell_address(x, y))

    def put32(self, address: int, value: int) -> None:
        self.uc.mem_write(address, struct.pack("<I", value & 0xFFFFFFFF))

    @staticmethod
    def cell_address(x: int, y: int) -> int:
        if not (0 <= x < 8 and 0 <= y < 8):
            raise OracleError("This fixture covers only its mapped 8x8 ordinary cell grid")
        return CELLS + (y * 8 + x) * CELL_STRIDE

    def call(self, address: int, receiver: int = UNIT, required_addresses=()) -> int:
        self.uc.mem_write(STACK - 0x1000, bytes(0x1004))
        self.put32(STACK, RETURN)
        for register in [UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EBX,
                         UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_EDI]:
            self.uc.reg_write(register, 0)
        self.uc.reg_write(UC_X86_REG_ESP, STACK)
        self.uc.reg_write(UC_X86_REG_ECX, receiver)
        self.uc.reg_write(UC_X86_REG_EFLAGS, 2)
        self.uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)
        run_checked(self.uc, address, RETURN, count=100000,
                    required_addresses=required_addresses)
        return signed(self.uc.reg_read(UC_X86_REG_EAX))

    def configure(self, case: dict) -> None:
        self.uc.mem_write(UNIT, bytes(0x1000))
        self.uc.mem_write(UNIT_TYPE, bytes(0x1800))
        self.put32(UNIT, 0x7F5C70)  # actual Unit vtable (type/current cell/Z getters)
        self.put32(UNIT + 0x520, UNIT_TYPE)
        self.put32(UNIT + 0x6C4, UNIT_TYPE)
        context = case["context"]
        self.uc.mem_write(UNIT + 0x9C, struct.pack("<iii", 640, 640, context["world_z_leptons"]))
        self.uc.mem_write(UNIT + 0x8C, bytes([int(context["on_bridge"])]))
        self.uc.mem_write(UNIT + 0x388, struct.pack("<H", context["facing_u16"]))
        for name, offset in [("cliff", 0xDC0), ("column", 0xDC4), ("tunnel", 0xDC8), ("bridge", 0xDCC)]:
            self.put32(UNIT_TYPE + offset, case["coefficients"][name])
        self.put32(0xAA0E28, 0)  # supplied bridge tile range base for column cases
        overrides = {tuple(cell["coords"]): cell for cell in case["cells"]}
        for index in range(65):
            self.uc.mem_write(TMP_CELLS + index * 0x100, bytes(0x34))
        for y in range(8):
            for x in range(8):
                cell = DEFAULT_CELL | overrides.get((x, y), {})
                address = self.cell_address(x, y)
                self.uc.mem_write(address, bytes(CELL_STRIDE))
                self.put32(address, 0x7E4EEC)  # actual CellClass vtable
                self.uc.mem_write(address + 0x24, struct.pack("<hh", x, y))
                self.put32(address + 0x38, cell["tile_index"])
                self.put32(address + 0x44, -1)  # no overlay
                self.uc.mem_write(address + 0x116, struct.pack("<h", -1))  # no tube
                self.uc.mem_write(address + 0x11B, bytes([cell["level"] & 0xFF, cell["ramp"]]))
                self.put32(address + 0x140, cell["flags"])
                if cell["tmp_height"] != 30:
                    index = cell["tile_index"]
                    if not 0 <= index < 65:
                        raise OracleError("Non-default TMP needs a mapped tile index")
                    tile = TMP_CELLS + index * 0x100
                    self.put32(tile + 4, 100)  # stored Y
                    self.put32(tile + 0x18, 130 - cell["tmp_height"])  # raw extra Y
                    self.put32(tile + 0x24, 1)  # HasExtraData

    def execute(self, case: dict) -> dict:
        self.configure(case)
        values = {}
        for name, address in TARGETS.items():
            # Require the helper visits in the full Foot run itself, not in
            # the separate probes above it.
            required = tuple(TARGETS[helper] for helper in (
                "cliff_multiplier", "column_multiplier", "tunnel_multiplier", "base_z_adjust",
            )) if name == "foot_z_adjust" else ()
            result = self.call(address, required_addresses=required)
            values[name] = bool(result & 0xFF) if name == "near_bridge" else result
        return values


def fixtures() -> list[dict]:
    cases = []

    def add(name: str, *, current: int = 0, first: int = 0, second: int = 0,
            cliff: int = 10, column: int = 5, tunnel: int = 10, bridge: int = 0,
            world_z: int = 0, facing: int = 0, on_bridge: bool = False,
            cells: list[dict] | None = None) -> None:
        overrides = {CURRENT: {"level": current}, (3, 3): {"level": first}, (4, 4): {"level": second}}
        for cell in cells or []:
            coords = tuple(cell["coords"])
            overrides[coords] = overrides.get(coords, {}) | cell
        cases.append({
            "name": name,
            "context": {"world_z_leptons": world_z, "facing_u16": facing, "on_bridge": on_bridge},
            "coefficients": {"cliff": cliff, "column": column, "tunnel": tunnel, "bridge": bridge},
            "cells": [DEFAULT_CELL | values | {"coords": list(coords)}
                      for coords, values in sorted(overrides.items())],
        })

    for first, second, name in [(0, 0, "neither"), (4, 0, "first"), (0, 4, "second"), (4, 4, "both"),
                                 (3, 0, "first_below_threshold"), (0, 3, "second_below_threshold"),
                                 (4, 3, "first_kept"), (3, 4, "second_only")]:
        add(f"stock_{name}", first=first, second=second)
    for current, first, second in [(-4, 0, -4), (-4, -4, 0), (-128, -124, -128),
                                    (127, -128, 127), (-128, 127, -128), (10, 13, 14)]:
        add(f"signed_levels_{current}_{first}_{second}", current=current, first=first, second=second)
    for coefficient in [0, 7, -7, 1073741824, 2147483647, -2147483648]:
        for first, second, probe in [(4, 0, "first"), (0, 4, "second")]:
            add(f"coefficient_{coefficient}_{probe}", first=first, second=second, cliff=coefficient)
    for world_z in [-832, -104, -1, 1, 104, 727, 728, 832]:
        add(f"world_z_{world_z}", first=4, world_z=world_z)
    for first, second, probe in [(4, 0, "first"), (0, 4, "second"), (4, 4, "both")]:
        add(f"on_bridge_{probe}", first=first, second=second, on_bridge=True)
    for direction in range(8):
        for height in [36, 37]:
            # (3,3) belongs to both cardinal triplets, but diagonal facings
            # skip this TMP-height branch entirely.
            add(f"facing_{direction}_tmp_{height}", first=4, facing=direction << 13,
                cells=[{"coords": [3, 3], "tile_index": 28, "tmp_height": height}])
    for direction in range(8):
        for position in [(1, 3), (3, 1)]:
            # SW and NE distinguish the two cardinal probe triplets.
            add(f"facing_{direction}_tall_at_{position[0]}_{position[1]}", first=4,
                facing=direction << 13, cells=[{"coords": list(position),
                    "tile_index": position[1] * 8 + position[0] + 1, "tmp_height": 37}])
    for facing in [4095, 4096, 12287, 12288, 61439, 61440, 65535]:
        add(f"facing_round_boundary_{facing}", first=4, facing=facing,
            cells=[{"coords": [3, 3], "tile_index": 28, "tmp_height": 37}])
    for coefficient in [7, 20, 37, -7]:
        add(f"bridge_max_{coefficient}", first=4, bridge=coefficient,
            cells=[{"coords": [2, 2], "flags": 0x100}])
    for coefficient in [7, 20, 37, -7]:
        add(f"column_max_{coefficient}", first=4, column=coefficient,
            cells=[{"coords": [2, 2], "flags": 0x100}, {"coords": [3, 3], "tile_index": 6}])
    add("bridge_on_bridge_gate", first=4, bridge=37, on_bridge=True,
        cells=[{"coords": [2, 2], "flags": 0x100}])
    return cases


def generate() -> dict:
    fixture = NativeFixture()
    cases = fixtures()
    for case in cases:
        case["native"] = fixture.execute(case)
    return {
        "schema_version": 2,
        "unicorn_version": unicorn_version,
        "scope": "Raw-function emulation of original Foot Z composition and its original helpers over supplied Unit/type/cell/TMP fixtures; not full-game or pixel parity.",
        "restrictions": [
            "Only the mapped ordinary 8x8 cell grid is exercised; no alias, dummy, invalid coordinate, or map-load equivalence claim.",
            "Actual Unit vtable; null locomotor, no transporter, harvester alternative, overlays, low bridges or active tubes.",
            "Ramp and cell-flag 0x10000 gates remain zero. SHP/voxel caller admission and active-game object lifecycle are outside the oracle.",
            "Native direction initializer runs; fixture supplies runtime type coefficients and relocated TMP metadata without executing their constructors/loaders.",
            "Code hooks only record instruction addresses. No executable instruction or function result is replaced.",
            "No image, disassembly, retail art or decoded pixel bytes are stored in this JSON.",
        ],
        "defaults": {
            "current_coords": list(CURRENT), "world_xy_leptons": [640, 640],
            "mapped_grid_width": 8, "mapped_grid_height": 8, "native_cell_index_stride": 512,
            "cell": DEFAULT_CELL, "locomotor_z_adjust": 0, "bridge_tile_base": 0,
            "adjust_for_z_multiplier_bits": f"{STANDARD_Z_MULTIPLIER_BITS:016x}",
            "fpcw": f"{NATIVE_FPCW:04x}", "directions": fixture.directions,
        },
        "entries": {name: f"{address:08x}" for name, address in TARGETS.items()},
        "region_sha256": fixture.region_hashes,
        "cases": cases,
    }


def metadata() -> dict:
    result = provenance(
        scope="Raw-function execution of Foot Z composition and its original helpers; not full-game, loader or rendered-pixel parity.",
        assumptions=[
            "Supplied Unit using its original vtable and supplied UnitType storage, ordinary mapped 8x8 cells with a 512-wide native lookup stride, and relocated one-cell TMP headers; original constructors, INI readers and map loading do not execute.",
            "Original direction initializer 0x0049F2F0 executes first with live x87 control 0x0E7F; the cached control word at 0x00822D80 is written afterward. Every call resets the live control word to 0x0E7F.",
            "The fixture supplies startup AdjustForZ multiplier bits 0x3FC25E5374344960, current world XY 640/640, runtime type coefficients, clear-tile index and bridge tile base zero.",
            "Actual Unit vtable with a null locomotor, no transporter, harvester alternative, overlays, low bridges or active tubes; ramp and cell flag 0x10000 remain zero. SHP/voxel caller admission, dummy/alias coordinates and active-game lifecycle are outside coverage.",
            "Each complete Foot call must execute the original cliff, column, tunnel and base-Z helpers in that same run. Code hooks observe execution and stop only at the declared return boundary; no native instruction or result is replaced.",
        ],
        substitutions=[],
        entry_points={"direction_initializer": REGIONS["direction_initializer"][0],
                      **TARGETS, "tmp_dimensions": REGIONS["tmp_dimensions"][0]},
    )
    return result


def main(argv=None) -> None:
    root = Path(__file__).resolve().parents[1]
    finish_vectors(generate, Path(__file__).with_name("render_depth_vectors.json"),
                   provenance=metadata, argv=argv, source_paths={
                       name: root / name for name in
                       ("tools/render_depth_oracle.py", "tools/native_oracle.py")
                   })


if __name__ == "__main__":
    main()
