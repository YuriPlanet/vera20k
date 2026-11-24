"""Execute original map lookup, normalization, and playfield instructions.

Run: python -m tools.spatial_oracle.map_queries [--check | --write]
See docs/research/PHASE3_MAP_SPATIAL_NATIVE_COMPARISON_20260910.md.
This supplies sparse cell-table state; it does not emulate map construction.
"""

import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESP

from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, call, finish_vectors, load_image,
    provenance, run_checked,
)

MAP, INPUT, CELLS = SCRATCH, SCRATCH + 0x200, SCRATCH + 0x1000
# Fixture table in the runner's broad non-executable image mapping. Explicitly
# clear the entire table: mapped image data is not initialized runtime state.
TABLE, DUMMY, GLOBAL_TABLE = 0x00C00000, 0x00ABDC50, 0x0087F924
EMPTY_TABLE = bytes(0x40000 * 4)
SENTINEL = (1234, -2345)
ALLOCATED = [(0, 0, -128, 1), (511, 0, 127, 255), (0, 1, -1, 0),
             (10, 10, 0, 0), (40, 48, 2, 1), (50, 40, 7, 4), (80, 50, 0, 0)]
ENTRIES = {"packed_lookup": 0x005657A0, "world_lookup": 0x00565730,
           "playfield": 0x00578460, "world_playfield": 0x005785F0,
           "normalize": 0x00567230, "clip_rect": 0x00421B60}


def dwords(*values):
    return struct.pack("<" + "I" * len(values), *(x & 0xFFFFFFFF for x in values))


def packed(x, y):
    return struct.pack("<HH", x & 0xFFFF, y & 0xFFFF)


def state(bounds=(80, 2, 4, 76, 48), dummy=(-7, 1)):
    table = bytearray(EMPTY_TABLE)
    writes = {MAP + 0xF4: dwords(bounds[0]), MAP + 0xFC: dwords(*bounds[1:]),
              MAP + 0x13C: dwords(TABLE, 0x40000), GLOBAL_TABLE: dwords(TABLE),
              DUMMY + 0x24: packed(*SENTINEL),
              DUMMY + 0x11B: bytes((dummy[0] & 255, dummy[1]))}
    for n, (x, y, level, slope) in enumerate(ALLOCATED):
        ptr = CELLS + n * 0x200
        struct.pack_into("<I", table, (y * 512 + x) * 4, ptr)
        writes[ptr + 0x24] = packed(x, y)
        writes[ptr + 0x11B] = bytes((level & 255, slope))
    writes[TABLE] = bytes(table)
    return writes


def query(kind, xy, bounds=(80, 2, 4, 76, 48), dummy=(-7, 1), mode=1):
    writes = state(bounds, dummy)
    writes[INPUT] = dwords(*xy, 0xDEADBEEF) if kind.startswith("world") else packed(*xy)
    args = [INPUT, mode] if kind == "playfield" else [INPUT]
    result = call(ENTRIES[kind], ecx=MAP, stack_args=args, writes=writes,
                  dumps={"dummy_coord": (DUMMY + 0x24, 4),
                         "dummy_height": (DUMMY + 0x11B, 2),
                         "input": (INPUT, len(writes[INPUT]))}, timeout_instr=500)
    if result["dumps"]["input"] != writes[INPUT].hex():
        raise RuntimeError("native query changed its caller input")
    if result["dumps"]["dummy_height"] != writes[DUMMY + 0x11B].hex():
        raise RuntimeError("native query changed retained dummy level/slope")
    row = {"kind": kind, "xy": xy,
           "dummy_coord": struct.unpack("<hh", bytes.fromhex(result["dumps"]["dummy_coord"]))}
    if "lookup" in kind:
        pointer = result["eax"]
        valid = {CELLS + n * 0x200: [x, y] for n, (x, y, _, _) in enumerate(ALLOCATED)}
        if pointer != DUMMY and pointer not in valid:
            raise RuntimeError(f"unexpected native CellClass pointer {pointer:#x}: {kind} {xy}")
        row["real"] = valid.get(pointer)
    else:
        row.update(bounds=bounds, dummy=dummy, mode=mode, inside=bool(result["eax"] & 255))
    return row


def normalize(size, local):
    # Enter the full function with its real stack ABI; execute ClipRect and every
    # field write, stopping BEFORE the redraw call/Techno traversal. No hooks or
    # substituted calls. The result is only the normalization portion.
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(SCRATCH, 0x10000)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    sp = STACK_BASE + STACK_SIZE - 0x1000
    uc.mem_write(sp, dwords(0, INPUT))
    uc.mem_write(INPUT, dwords(*local))
    uc.mem_write(MAP + 0xEC, dwords(0, 0, *size))
    uc.reg_write(UC_X86_REG_ESP, sp)
    uc.reg_write(UC_X86_REG_ECX, MAP)
    run_checked(uc, ENTRIES["normalize"], 0x005672D3, count=1000,
                required_addresses=[ENTRIES["clip_rect"], 0x005672CD])
    if bytes(uc.mem_read(INPUT, 16)) != dwords(*local):
        raise RuntimeError("normalizer changed input rectangle")
    return {"size": size, "local": local,
            "normalized": struct.unpack("<iiii", uc.mem_read(MAP + 0xFC, 16))}


def retained_dummy_sequence():
    """Retain the first returned native pointer across later real/miss queries."""
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(SCRATCH, 0x10000)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.mem_map(RET_MAGIC, 0x1000)
    for address, value in state().items():
        uc.mem_write(address, value)
    rows, retained = [], None
    for xy in [(-1, 0), (10, 10), (12, 11), (-1, 1), (32767, -32768)]:
        sp = STACK_BASE + STACK_SIZE - 0x1000
        uc.mem_write(INPUT, packed(*xy))
        uc.mem_write(sp, dwords(RET_MAGIC, INPUT))
        uc.reg_write(UC_X86_REG_ECX, MAP)
        uc.reg_write(UC_X86_REG_ESP, sp)
        run_checked(uc, ENTRIES["packed_lookup"], RET_MAGIC, count=500)
        pointer = uc.reg_read(UC_X86_REG_EAX)
        if retained is None:
            retained = pointer
            if retained != DUMMY:
                raise RuntimeError("first sequence query must select the shared dummy")
        rows.append({"xy": xy, "returns_retained": pointer == retained,
                     "retained_coord": struct.unpack("<hh", uc.mem_read(retained + 0x24, 4)),
                     "retained_height": list(uc.mem_read(retained + 0x11B, 2))})
    return rows


def generate():
    lookups = []
    pairs = [(0, 0), (-1, 1), (512, 0), (0, 1), (511, 0), (10, 10), (11, 10),
             (-1, 0), (0, -1), (511, 511), (512, 511), (-1, 512),
             (-32768, -32768), (32767, 32767), (65536, 65536), (65535, 65537)]
    for xy in pairs:
        lookups.append(query("packed_lookup", xy))
    components = [-2147483648, -8388609, -8388608, -65537, -513, -512, -511,
                  -257, -256, -255, -1, 0, 1, 255, 256, 257, 511, 512,
                  65535, 65536, 8388607, 8388608, 2147483647]
    for x in components:
        for y in [-256, 0, 256, 2147483647]:
            lookups.append(query("world_lookup", (x, y)))
    for xy in [(2560, 2560), (-256, 256), (131072, 0), (16777216, -32768),
               (0, -2147483648), (2147483647, 2147483647)]:
        lookups.append(query("world_lookup", xy))

    local_inputs = [(80, 58, [2, 4, 76, 48]), (80, 80, [-5, -6, 100, 100]),
                    (0, 0, [0, 0, 0, 0]), (1, 1, [0, 0, 1, 1]),
                    (40, 50, [5, 6, 40, 50]), (40, 50, [0, 0, 0, 0])]
    for local in [[-1, 3, 1, 5], [3, -1, 5, 1], [81, 4, 5, 5], [4, 81, 5, 5],
                  [3, 4, -1, 5], [3, 4, 5, -1], [2147483647, 2, 2, 20],
                  [-2147483648, 2, 2147483647, 20], [2, 2147483647, 20, 2]]:
        local_inputs.append((80, 80, local))
    normalizations = [normalize((w, h), local) for w, h, local in local_inputs]
    fields = [(80, 2, 4, 76, 48), (8, 2, 2, 4, 0),
              (80, 2, 2, 76, 72), (0, 2, 2, -4, -8),
              (-2147483648, -2147483647, -2147483648, -32768, -32768)]
    predicates = []
    for bounds in fields:
        points = set(pairs + [(40, 48), (50, 40), (80, 50)])
        # Sample either side and exactly on every geometric bound and on the
        # slope correction threshold. Expected verdicts always come from x86.
        w, left, top, width, height = bounds
        for level in [-128, -1, 0, 1, 127]:
            for edge in [w + 2 * top + level, w + 4 + 2 * top + level,
                         w + 2 + 2 * (height + top) + level]:
                for delta in [-1, 0, 1]:
                    points.add((40, edge - 40 + delta))
        for difference in [2 * (width + left) - w, -(w - 2 * left)]:
            for delta in [-1, 0, 1]:
                points.add((difference + 50 + delta, 50))
        # Keep input API i32-shaped; native packs these words at the seam.
        points = sorted((int((x + 2**31) % 2**32 - 2**31),
                         int((y + 2**31) % 2**32 - 2**31)) for x, y in points)
        for xy in points:
            for mode, dummy in [(0, (-7, 1)), (256, (127, 255)), (1, (-128, 0)),
                                (1, (-1, 1)), (1, (0, 0)), (1, (1, 255)), (255, (127, 1))]:
                predicates.append(query("playfield", xy, bounds, dummy, mode))
    for x in components:
        for y in [-255, 0, 256, 48 * 256 + 255]:
            predicates.append(query("world_playfield", (x, y)))
    return {"source": "unicorn/gamemd.exe", "allocated": ALLOCATED,
            "dummy_seed": SENTINEL, "lookups": lookups,
            "normalizations": normalizations, "predicates": predicates,
            "retained_sequence": retained_dummy_sequence()}


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"),
        provenance=lambda: provenance(
            scope="Bounded packed/world lookup, LocalSize normalization prefix, and playfield predicates; not phase-wide parity",
            assumptions=[
                "Fresh emulator per query with seven explicit allocated slots and all remaining pointer slots zero",
                "MapClass+13C and global 87F924 select the same table; capacity is retail 0x40000",
                "Dummy coordinate seed and level/slope are supplied; queries must preserve level/slope and source input",
                "Five packed lookups also execute consecutively in one emulator, retaining the first dummy pointer",
                "Real-cell level/slope inputs and signed/overflow cases are synthetic; nearoref.map supplies the 80x58/2,4,76,48 header",
                "567230 executes original ClipRect and stops at 5672D3 before redraw and Techno membership effects",
                "No map constructors, lifecycle, final presentation, complete callers or exhaustive i32 domain are claimed",
            ], substitutions=[], entry_points=ENTRIES))
