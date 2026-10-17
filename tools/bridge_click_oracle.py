"""Execute original YR bridge destination picking, without emulating its formula.

Run ``python -m tools.bridge_click_oracle --check`` from the repository root.
``--write`` explicitly generates the reviewed reference and metadata. Unicorn
must already be installed; this module does not download dependencies.
``--plane --check`` selects the separate, small6D2280 plane-XYZ corpus; it does
not regenerate or compare the established bridge corpus.

The complete Tactical inverse at 0x6D6590 executes with its original inverse
matrix, constructed by the original constant-store block 0x6D1DC5..0x6D1E1E.
Both the whole inverse and isolated bridge block call the original projector,
neighbor lookup, map lookup, matrix multiplication and x87 conversion leaves.
No native instructions or function results are substituted. Constructors and
map loading are not executed: fixture cells supply levels and structural flags.

Native inputs are integer tactical pixels. VERA world coordinates have a +15
Y row bias; camera and viewport variations below exercise native preprocessing.
These are picking outputs, not game movement, art, GPU or fractional-zoom proof.
"""

from __future__ import annotations

from collections import Counter
from pathlib import Path
import struct
import sys

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
    UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_ESP,
    UC_X86_REG_EFLAGS, UC_X86_REG_FPCW,
)

from tools.native_oracle import (
    NATIVE_FPCW, OracleError, finish_vectors, load_image, provenance, run_checked,
)


INVERSE = 0x6D6590
PLANE_INVERSE = 0x6D2280
EDGE_ENTRY = 0x6D6751
EDGE_ENDS = (0x6D69C4, 0x6D6A02, 0x6D6A36)
MATRIX_INITIALIZER = (0x6D1DC5, 0x6D1E1E)
MEM = 0x21000000
TABLE = MEM
CELLS = MEM + 0x100000
TACTICAL = MEM + 0x300000
INPUT = MEM + 0x302000
OUTPUT = MEM + 0x302020
SP = MEM + 0x3FE000
RETURN = 0x30000000
CELL_STRIDE = 0x148
WORLD_Y_BIAS = 15
STANDARD_Z_MULTIPLIER_BITS = 0x3FC25E5374344960  # native startup 0x6D1BDD
REGISTERS = (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX,
    UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_EDI,
)


def signed32(value: int) -> int:
    return ((value + 0x80000000) & 0xFFFFFFFF) - 0x80000000


class NativeFixture:
    """Supplied cell state; original code owns every computed output."""

    def __init__(self, bounds: list[int], base_height: int,
                 bridge_cells: list[list[int]], height_overrides: list[list[int]]):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.uc)
        self.uc.mem_map(MEM, 0x400000)
        self.uc.mem_map(RETURN, 0x1000)
        self.visits: Counter[int] = Counter()
        self.native_reference: list[int] | None = None
        self.plane_matrix_bits: list[str] | None = None
        self.lookup_miss = False
        self.uc.hook_add(UC_HOOK_CODE, self.observe)
        self.reset_call()
        self.put32(SP, RETURN)
        run_checked(self.uc, 0x49F2F0, RETURN, count=200)
        self.directions = list(struct.iter_unpack(
            "<hh", bytes(self.uc.mem_read(0x89F688, 32))))
        self.reset_call()
        self.uc.reg_write(UC_X86_REG_ESI, TACTICAL)
        self.uc.reg_write(UC_X86_REG_EBX, 0)  # live constructor's xor ebx,ebx
        run_checked(self.uc, *MATRIX_INITIALIZER, count=30)
        self.matrix_bits = bytes(self.uc.mem_read(TACTICAL + 0xDE4, 48)).hex()
        self.put32(0x87F924, TABLE)
        self.uc.mem_write(0xB0CD48, struct.pack("<Q", STANDARD_Z_MULTIPLIER_BITS))
        self.uc.mem_write(0x822D80, struct.pack("<H", NATIVE_FPCW))
        xmin, ymin, xmax, ymax = bounds
        if not (0 <= xmin <= xmax < 512 and 0 <= ymin <= ymax < 512):
            raise ValueError("Fixture bounds must lie inside the native cell table")
        if (xmax - xmin + 1) * (ymax - ymin + 1) * CELL_STRIDE > 0x200000:
            raise ValueError("Fixture exceeds mapped CellClass storage")
        bridges = {(x, y): bool(direction) for x, y, _deck, direction in bridge_cells}
        heights = {(x, y): level for x, y, level in height_overrides}
        self.addresses = {}
        for y in range(ymin, ymax + 1):
            for x in range(xmin, xmax + 1):
                address = CELLS + len(self.addresses) * CELL_STRIDE
                self.addresses[x, y] = address
                self.put32(TABLE + (y * 512 + x) * 4, address)
                self.uc.mem_write(address + 0x24, struct.pack("<hh", x, y))
                self.uc.mem_write(address + 0x11B, bytes([heights.get((x, y), base_height) & 255]))
                flags = (0x100 | (0x800 if bridges[x, y] else 0)) if (x, y) in bridges else 0
                self.put32(address + 0x140, flags)

    def put32(self, address: int, value: int) -> None:
        self.uc.mem_write(address, struct.pack("<I", value & 0xFFFFFFFF))

    def observe(self, _uc, address, _size, _data):
        self.visits[address] += 1
        if address == 0x6D6901:
            self.native_reference = [signed32(self.uc.reg_read(UC_X86_REG_EBP)),
                                     signed32(self.uc.reg_read(UC_X86_REG_ECX))]
        if address == 0x6D230D:
            sp = self.uc.reg_read(UC_X86_REG_ESP)
            self.plane_matrix_bits = [hex(value) for value in struct.unpack(
                "<3I", bytes(self.uc.mem_read(sp + 0x14, 12)))]
        if address == 0x5657C8:
            # This fallback's mutable global sentinel is deliberately not supplied.
            # All fixtures promise mapped lookup cells; a miss invalidates the run.
            self.lookup_miss = True
            self.uc.emu_stop()

    def reset_call(self) -> None:
        self.visits.clear()
        self.native_reference = None
        self.plane_matrix_bits = None
        self.lookup_miss = False
        self.uc.mem_write(SP - 0x2000, bytes(0x2200))
        for register in REGISTERS:
            self.uc.reg_write(register, 0)
        self.uc.reg_write(UC_X86_REG_ESP, SP)
        self.uc.reg_write(UC_X86_REG_EFLAGS, 2)
        self.uc.reg_write(UC_X86_REG_FPCW, NATIVE_FPCW)

    def edge(self, cell: list[int], world_input: list[int], scan_y: int) -> dict:
        self.reset_call()
        x, y = cell
        input_x, input_y = world_input
        input_y -= WORLD_Y_BIAS
        native_scan = scan_y - WORLD_Y_BIAS
        for register, value in (
            (UC_X86_REG_EAX, self.addresses[x, y]), (UC_X86_REG_EBX, input_y),
            (UC_X86_REG_EBP, native_scan), (UC_X86_REG_EDI, TACTICAL),
        ):
            self.uc.reg_write(register, value)
        self.put32(SP + 0x38, native_scan)
        self.put32(SP + 0x3C, input_x)
        self.put32(SP + 0x40, input_y)
        self.uc.mem_write(SP + 0x2C, struct.pack("<hh", x, y))
        end = run_checked(
            self.uc, EDGE_ENTRY, EDGE_ENDS, count=2000,
            required_addresses=[0x6D68CE, 0x6D1F10, 0x6D6901, 0x481810, 0x5657A0],
        )
        adjusted = signed32(struct.unpack("<I", self.uc.mem_read(SP + 0x18, 4))[0])
        result = {"kind": "scan", "adjusted_scan_y": adjusted + WORLD_Y_BIAS}
        if end != 0x6D69C4:
            result.update(kind="direct", cell=[x + int(end == 0x6D6A02), y + int(end == 0x6D6A36)])
        return result

    def inverse(self, world_input: list[int], camera: list[int], viewport: list[int]) -> dict:
        self.reset_call()
        self.put32(TACTICAL + 0xB0, camera[0])
        self.put32(TACTICAL + 0xB4, camera[1])
        self.put32(0x886FA0, viewport[0])
        self.put32(0x886FA4, viewport[1])
        client_x = world_input[0] - camera[0] + viewport[0]
        client_y = world_input[1] - WORLD_Y_BIAS - camera[1] + viewport[1]
        self.uc.mem_write(INPUT, struct.pack("<ii", client_x, client_y))
        self.uc.mem_write(OUTPUT - 4, bytes.fromhex("a1a2a3a4ccccccccb1b2b3b4"))
        self.put32(SP, RETURN)
        self.put32(SP + 4, OUTPUT)
        self.put32(SP + 8, INPUT)
        self.uc.reg_write(UC_X86_REG_ECX, TACTICAL)
        run_checked(
            self.uc, INVERSE, RETURN, count=100000,
            required_addresses=[0x5AFB80, 0x7C5F00, 0x5657A0, 0x6D1F10],
        )
        raw = bytes(self.uc.mem_read(OUTPUT - 4, 12))
        if raw[:4].hex() != "a1a2a3a4" or raw[8:].hex() != "b1b2b3b4":
            raise OracleError("Native inverse wrote beyond its packed cell output")
        if self.uc.reg_read(UC_X86_REG_EAX) != OUTPUT:
            raise OracleError("Native inverse output calling convention mismatch")
        if self.uc.reg_read(UC_X86_REG_ESP) != SP + 12:
            raise OracleError("Native inverse did not clean its two stack arguments")
        exits = {0x6D69EB: "fallback", 0x6D6A02: "direct_x",
                 0x6D6A36: "direct_y", 0x6D6A6A: "scan"}
        observed = [name for address, name in exits.items() if self.visits[address]]
        if len(observed) != 1:
            raise OracleError(f"Unexpected native return path: {observed}")
        return {"expected_cell": list(struct.unpack("<hh", raw[4:8])),
                "native_exit": observed[0], "scan_attempts": self.visits[0x6D66AA],
                "bridge_candidates": self.visits[0x6D6779]}

    def plane(self, world_input: list[int], camera: list[int], viewport: list[int],
              window: list[int]) -> dict:
        """Whole6D2280; world Y includes VERA's existing15px presentation bias.

        This body does not subtract886FA0/4. ProcessClickCoords adds the
        viewport back before calling it. The supplied point is consequently
        world-camera; viewport globals are varied independently as controls.
        """
        self.reset_call()
        self.put32(TACTICAL + 0xB0, camera[0])
        self.put32(TACTICAL + 0xB4, camera[1])
        self.put32(0x886FA0, viewport[0])
        self.put32(0x886FA4, viewport[1])
        width, top, height = window
        self.put32(0xB0CE30, width)
        self.put32(0xB0CE2C, top)
        self.put32(0xB0CE34, height)
        # Deliberately distinct supplied globals prove the invalid branch's
        # copy, not the unexecuted constructor's sentinel initialization.
        invalid = [-123456789, 987654321, -31415926]
        self.uc.mem_write(0xB0CE08, struct.pack("<3i", *invalid))
        point = [world_input[0] - camera[0],
                 world_input[1] - WORLD_Y_BIAS - camera[1]]
        self.uc.mem_write(INPUT, struct.pack("<2i", *point))
        before = bytes.fromhex("a1a2a3a4" + "cc" * 12 + "b1b2b3b4")
        self.uc.mem_write(OUTPUT - 4, before)
        self.put32(SP, RETURN)
        self.put32(SP + 4, OUTPUT)
        self.put32(SP + 8, INPUT)
        self.uc.reg_write(UC_X86_REG_ECX, TACTICAL)
        run_checked(self.uc, PLANE_INVERSE, RETURN, count=1000)
        raw = bytes(self.uc.mem_read(OUTPUT - 4, len(before)))
        if raw[:4] != before[:4] or raw[-4:] != before[-4:]:
            raise OracleError("Native plane inverse wrote outside XYZ output")
        if self.uc.reg_read(UC_X86_REG_EAX) != OUTPUT:
            raise OracleError("Native plane inverse output pointer mismatch")
        if self.uc.reg_read(UC_X86_REG_ESP) != SP + 12:
            raise OracleError("Native plane inverse stack cleanup mismatch")
        invalid_path = bool(self.visits[0x6D2337])
        if not invalid_path:
            if not self.visits[0x5AFB80] or self.visits[0x7C5F00] != 2:
                raise OracleError("Native plane conversion leaves did not execute")
            if self.plane_matrix_bits is None:
                raise OracleError("Native plane matrix output was not observed")
        elif self.visits[0x5AFB80] or self.visits[0x7C5F00]:
            raise OracleError("Invalid plane point unexpectedly ran conversion")
        result = {"native_client_input": point,
                  "native_exit": "invalid_upper_bound" if invalid_path else "plane",
                  "expected_xyz": list(struct.unpack("<3i", raw[4:16]))}
        if invalid_path:
            result["supplied_invalid_xyz"] = invalid
        else:
            result["matrix_result_f32_bits"] = self.plane_matrix_bits
        return result


def edge_scenarios() -> list[dict]:
    def scenario(name, direction=True, omit=(), heights=(), cell=(87, 73)):
        x, y = cell
        cells = [[x, y], [x, y - 1], [x + 1, y], [x, y + 1], [x - 1, y]]
        return {"name": name, "cell": list(cell), "base_height": 2,
                "direction_zero": direction,
                "structural_cells": [p for index, p in enumerate(cells) if index not in omit],
                "height_overrides": list(heights)}
    return [
        scenario("north_open", omit=(1,)),
        scenario("west_open", False, (4,), cell=(73, 87)),
        scenario("enclosed"), scenario("direct_south", omit=(3,)),
        scenario("direct_east", False, (2,), cell=(73, 87)),
        scenario("east_near_height", omit=(2,)),
        scenario("east_far_height", True, (2,), [[88, 73, 4]]),
        scenario("south_near_height", False, (3,), cell=(73, 87)),
        scenario("south_far_height", False, (3,), [[73, 88, 4]], cell=(73, 87)),
    ]


def inverse_scenarios() -> list[dict]:
    scenarios = []
    for transpose in [False, True]:
        # The structural rectangle matches the prepared Hills span coordinates;
        # terrain is deliberately flat synthetic state, not the retail map load.
        cells = [[x, y, 6, 1] for x in range(76, 98) for y in range(73, 77)]
        if transpose:
            cells = [[y, x, deck, 0] for x, y, deck, _ in cells]
        scenarios.append({
            "name": "hills_coordinates_transposed" if transpose else "hills_coordinates",
            "base_height": 2, "bounds": [52, 52, 120, 120],
            "bridge_cells": cells, "height_overrides": [],
        })
    return scenarios


def generate() -> dict:
    edges = edge_scenarios()
    matrix_bits = None
    for scenario in edges:
        x, y = scenario["cell"]
        base = scenario["base_height"]
        bridges = [[cx, cy, base + 4, int(scenario["direction_zero"])]
                   for cx, cy in scenario["structural_cells"]]
        fixture = NativeFixture([x - 1, y - 1, x + 1, y + 1], base,
                                bridges, scenario["height_overrides"])
        matrix_bits = fixture.matrix_bits
        scenario["cases"] = []
        for dx in [-31, -30, -15, -3, -1, 0, 1, 3, 15, 30, 31]:
            for dy in [-61, -60, -59, -16, -15, -14, -1, 0, 1, 14, 15, 16, 29, 30, 31, 59, 60, 61]:
                point = [(x - y) * 30 + dx, (x + y) * 15 - base * 15 + WORLD_Y_BIAS + dy]
                scan = (x + y) * 15 + 30
                scenario["cases"].append({"world_input": point, "scan_y": scan,
                                          "expected": fixture.edge([x, y], point, scan)})
    inverses = inverse_scenarios()
    for scenario in inverses:
        fixture = NativeFixture(scenario["bounds"], scenario["base_height"],
                                scenario["bridge_cells"], scenario["height_overrides"])
        transpose = scenario["name"].endswith("transposed")
        scenario["cases"] = []
        anchors = [(76, 73), (76, 74), (77, 74), (87, 73), (87, 74),
                   (87, 76), (96, 74), (97, 74), (97, 76), (75, 74), (98, 74)]
        for ax, ay in anchors:
            x, y = (ay, ax) if transpose else (ax, ay)
            center = [(x - y) * 30, (x + y) * 15 + 30 - 6 * 15]
            for dx, dy in [(0, 0), (-29, 0), (29, 0), (0, -14), (0, 14),
                           (-15, -8), (15, -8), (-15, 8), (15, 8),
                           (-1, -15), (1, -15), (-1, 15), (1, 15)]:
                point = [center[0] + dx, center[1] + dy]
                for camera, viewport in [([0, 0], [0, 0]), ([217, 1981], [120, 40])]:
                    result = fixture.inverse(point, camera, viewport)
                    scenario["cases"].append({
                        "name": f"cell_{x}_{y}_offset_{dx}_{dy}_camera_{camera[0]}",
                        "world_input": point, "camera": camera, "viewport": viewport, **result,
                    })
    return {"source": "unicorn/gamemd.exe", "edge_entry": hex(EDGE_ENTRY),
            "inverse_entry": hex(INVERSE), "inverse_matrix_bits": matrix_bits,
            "world_y_bias": WORLD_Y_BIAS,
            "edge_scenarios": edges, "inverse_scenarios": inverses}


def metadata() -> dict:
    return provenance(
        scope="1782 bridge candidate cases and 572 complete Tactical inverse calls; synthetic flat maps at Hills coordinates in both orientations",
        assumptions=[
            "Retail image identity is checked by the shared loader; all code executes unchanged.",
            "Direction initialization 49F2F0 and inverse matrix constructor block 6D1DC5..6D1E1E execute natively. Constructor EBX is zero; remaining constructors are not executed.",
            "Fixture cells supply base height 2, structural flag 100 and orientation flag 800; selected edge guards use neighbor height 4. Complete inverse terrain is synthetic flat base 2 with deck 6 structural rectangles.",
            "Every requested cell lookup must stay inside mapped fixture bounds. Sentinel lookup is an error, never a zero-return substitute.",
            "Each call resets registers and stack; initialized immutable matrix, directions and cell state are reused within a scenario. Native outputs and stack cleanup are checked.",
            "x87 control word is 0E7F. The startup Z multiplier is supplied as its native-derived bits; projection inputs in this inverse always have Z zero.",
            "Camera and viewport are either zero or explicit integer offsets. VERA world inputs have a 15-pixel Y bias relative to native tactical pixels.",
            "No map loading, mouse event processing, fractional zoom policy, movement, rendering or whole-game acceptance is established by these vectors.",
        ],
        substitutions=[],
        entry_points={"inverse": INVERSE, "edge_candidate": EDGE_ENTRY,
                      "matrix_initializer": MATRIX_INITIALIZER[0],
                      "direction_initializer": 0x49F2F0,
                      "matrix_multiply": 0x5AFB80, "x87_integer_conversion": 0x7C5F00,
                      "cell_neighbor": 0x481810, "map_lookup": 0x5657A0,
                      "projector": 0x6D1F10},
    )


def generate_plane() -> dict:
    # Cell allocation is immaterial to6D2280; reuse the checked fixture and
    # original matrix initialization without introducing a second loader.
    fixture = NativeFixture([0, 0, 0, 0], 0, [], [])
    cases = []
    for name, point in [
        ("origin", [0, 15]), ("positive_pixel", [1, 16]),
        ("negative_pixel", [-1, 14]), ("mixed_signs", [-61, 46]),
        ("positive_cell_edge_before", [0, 44]),
        ("positive_cell_edge", [0, 45]),
        ("positive_cell_edge_after", [0, 46]),
        ("negative_cell_edge_before", [0, -16]),
        ("negative_cell_edge", [0, -15]),
        ("negative_cell_edge_after", [0, -14]),
        ("interior_center", [420, 2310]),
        ("east_map_extent", [15330, 15360]),
        ("west_map_extent", [-15330, 15360]),
        ("mixed_cancellation", [30690, -15330]),
        ("f32_integer_boundary_positive", [2006, 15]),
        ("f32_integer_boundary_negative", [-2006, 15]),
    ]:
        cases.append({"name": name, "world_input": point,
                      "camera": [0, 0], "viewport": [0, 0],
                      "window_width_top_height": [32768, 0, 32768]})
    for name, camera, viewport in [
        ("camera_offset", [217, 1981], [0, 0]),
        ("viewport_offset", [217, 1981], [120, 40]),
        ("negative_camera", [-219, -1979], [-120, -40]),
    ]:
        cases.append({"name": name, "world_input": [420, 2310],
                      "camera": camera, "viewport": viewport,
                      "window_width_top_height": [32768, 0, 32768]})
    for name, point in [
        ("upper_x_before", [639, 15]), ("upper_x_equal", [640, 15]),
        ("upper_y_before", [0, 514]), ("upper_y_equal", [0, 515]),
        ("no_lower_bounds", [-1, -1]),
    ]:
        cases.append({"name": name, "world_input": point,
                      "camera": [0, 0], "viewport": [0, 20],
                      "window_width_top_height": [640, 20, 480]})
    for case in cases:
        case.update(fixture.plane(case["world_input"], case["camera"],
                                  case["viewport"], case["window_width_top_height"]))
    return {"source": "unicorn/gamemd.exe", "plane_entry": hex(PLANE_INVERSE),
            "inverse_matrix_bits": fixture.matrix_bits,
            "world_y_bias": WORLD_Y_BIAS, "cases": cases}


def plane_metadata() -> dict:
    return provenance(
        scope="24 complete Tactical plane inverse6D2280 calls; integer signed, cell-edge, final-f32 integer-boundary, camera, viewport and upper-window controls",
        assumptions=[
            "Original6D2280, matrix multiply5AFB80 and conversion7C5F00 execute unchanged; no function result is substituted.",
            "Existing NativeFixture executes constructor matrix block6D1DC5..6D1E1E with EBX zero; other constructor state is supplied.",
            "world_input is integer VERA world pixels; native absolute tactical Y is world Y minus15. Incoming point is world minus camera (and Y bias), not plus viewport.",
            "6D2280 does not read886FA0/4; independently supplied viewport globals are controls, not an extra arithmetic offset.",
            "Supplied B0CE30 is signed upper X; supplied B0CE2C plus B0CE34 is signed upper Y. Equality rejects. Negative lower coordinates are admitted by this body.",
            "Invalid B0CE08/0C/10 values are explicit fixture markers, not claimed constructor defaults. Invalid controls are separate from arithmetic outputs.",
            "Each call resets stack/registers and checks output guards, return pointer and two-argument stack cleanup. Valid paths require original multiply and both conversion calls.",
            "x87 control word0E7F follows the existing fixture; the recorded preconversion matrix f32 bits are actual native stores under that rounding mode.",
            "No map lookup, current Cell bits, full ProcessClickCoords, Windows input, fractional zoom, normal map load or rendered tooltip is executed.",
        ], substitutions=[],
        entry_points={"plane_inverse": PLANE_INVERSE,
                      "matrix_initializer": MATRIX_INITIALIZER[0],
                      "matrix_multiply": 0x5AFB80,
                      "x87_integer_conversion": 0x7C5F00},
    )


if __name__ == "__main__":
    args = sys.argv[1:]
    if "--plane" in args:
        args.remove("--plane")
        finish_vectors(generate_plane, Path(__file__).with_suffix("") / "plane_vectors.json",
                       provenance=plane_metadata, argv=args)
    else:
        finish_vectors(generate, Path(__file__).with_suffix("") / "vectors.json",
                       provenance=metadata, argv=args)
