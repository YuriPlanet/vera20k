"""Original SmudgeTypeClass::CanPlace 0x006B5F80 over MapClass's own cell table,
with the scorch/crater placers and BuildingClass::DestructionEffects step 7 that
call it.

CanPlace (thiscall; the origin CellStruct by pointer, force; ret 8) checks each
footprint cell origin + (x, y), y outer and x inner, for x < Width (+0x298) and
y < Height (+0x29C), 16-bit sums: MapClass::GetCell 0x005657A0 on the footprint
cell, In_Bounds 0x00568300 on the ORIGIN, SlopeIndex +0x11C == 0,
SmudgeTypeIndex +0x48 == -1, OverlayTypeIndex +0x44 == -1, unless force the
building lookup 0x0047C520 (the first What_Am_I 6 on the ground list +0xE4,
linked through +0x30, only while g_GameActive 0x00A8E9A0 is set), and Morphable
+0x2E0 of IsoTileType IsoTileTypeIndex +0x38 (an index below 0 or at least the
count reads entry 0).

The cell table is MapClass's (0x0087F7E8): Size width/height (+0xF4/+0xF8), the
only fields In_Bounds reads, and the CellClass pointer array (+0x13C, 0x40000
slots at y * 512 + x) that GetCell reads; an empty slot or an index outside the
array returns the shared dummy cell 0x00ABDC50 with its coordinate stamped.
Every cell, the dummy included, is built by the original CellClass constructor
0x0047BBF0; a row then writes the fields its map establishes: IsoTileTypeIndex
+0x38 (the constructor's 0xFFFF otherwise), OverlayTypeIndex +0x44 and
OverlayData +0x11E, SmudgeTypeIndex +0x48 and SmudgeData +0x11F, SlopeIndex
+0x11C and the ground list (objects on their original class vtables, whose
What_Am_I +0x2C returns 6, 1, 15 or 2). A synthetic map allocates its whole Size
diamond; a retail map row allocates only the cells step 7's CanPlace reads. The
IsoTileType table (0x00A8ED2C items, 0x00A8ED38 count) holds the map's Morphable
bits; the SmudgeType table (0x00A8EC1C items, 0x00A8EC28 count) the retail
[SmudgeTypes] (ArrayIndex +0x294, Width +0x298, Height +0x29C, Crater +0x2A0,
Burn +0x2A1).

Groups:
- `can_place`: CanPlace alone, for every origin of a synthetic Size 6x4 map
  and origins outside it x six footprints x force, on three variants: theater
  tile 0 Morphable or not, and a dummy cell a Place has already marked.
- `placer`: the Burn placer 0x006B59A0 or the Crater placer 0x006B5C90
  (fastcall: the coordinate by pointer, frame width, then frame height and
  force; ret 8) at a coordinate on that map: the cell by truncation toward
  zero, the (0, 0) sentinel 0x00B0B788 (zeroed by the static initializer
  0x006B5210), CanPlace per flagged type, the preference 0x006B5B04..0x006B5BA2
  and the pick's RandomRanged (0x006B5BDE/0x006B5C1A, Crater 0x006B5ECE/
  0x006B5F0A).
- `centre_mark`: DestructionEffects 0x0044177E..0x004418EC for all 22
  foundation indices (Width 0x0045EC90 / Height 0x0045ECA0 over the original
  tables 0x008192B8/0x00819310, recorded as `foundation_size`): the >= 2x2
  gate, a RandomRanged(0, dimension - 2) per dimension over 2 (0x004417D3,
  0x00441805), the RandomRanged(0, 99) roll (0x00441819) and the placer with
  force 1 and size 0x64 at the Location cell's centre (z 0).

The SmudgeClass constructor 0x006B4A50 is recorded, not run. Read: it
Unlimbos 0x005F4EC0 at the coordinate (vt+0x1AC 0x004264C0 returns 0; the
SmudgeType's vt+0x6C 0x0041CF80 copies the coordinate; vt+0x1B4 0x005F6940
stores it as Location), whose vt+0x124 is SmudgeClass::Mark 0x006B4BE0: the
cell by truncation (vt+0x1B8 0x0041BEA0), CanPlace with force 1 (passing on the
footprint the placer just admitted with force 0 or 1) and
SmudgeTypeClass::Place 0x006B6080. The harness executes that Place natively on
the recorded type and truncated cell; its per-cell redraw 0x00486E70 is
recorded (the cell and the SmudgeTypeIndex/SmudgeData Place just wrote there)
and returns.

Schema: `smudge_types` (index order); `maps.<name>` {size, allocate_diamond,
cells[] {x, y, tile, overlay, overlay_data, smudge, smudge_data, slope,
objects}, tile_count, morphable (tile indices; the rest are not Morphable),
dummy (fields written over the constructor's)}; `can_place[]` {map, origin,
width, height, force, result, dummy_read (some GetCell returned the dummy)};
`placer[]` and `centre_mark[]` {input, events (`ranged`/`next` draws with call
site and result, `can_place` {type, origin, force, result, dummy_read},
`smudge` {type, coord, house}), marked [{cell, dummy, type, data}] (Place's
writes in order), raw_draw_count, rng_after}; `seed_states` holds each seed's
Scenario RNG state after 0x0065C6D0 (every row's state before).

Rust consumers: src/sim/smudge_grid.rs (`passes_placement_gates`, `try_place`)
and src/sim/combat/smudge_dispatch.rs (`try_dispatch_building_destruction_smudges`);
`building_death_anims` runs the retail Dustbowl rows on this cell table.
"""
import struct
from functools import lru_cache
from pathlib import Path

from unicorn.x86_const import (UC_X86_REG_AL, UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX,
                               UC_X86_REG_EDX, UC_X86_REG_ESI, UC_X86_REG_ESP)

from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.spatial_oracle import anim_bouncer_launch as launch
from tools.spatial_oracle.anim_bouncer_launch import dwords, read32, signed

MAP = 0x87F7E8
SIZE_FIELD, POINTERS_FIELD = MAP + 0xF4, MAP + 0x13C
DUMMY = 0xABDC50
CELL_CTOR, GET_CELL, GET_CELL_RET, IN_BOUNDS = 0x47BBF0, 0x5657A0, 0x5657D5, 0x568300
CAN_PLACE, CAN_PLACE_TRUE, CAN_PLACE_FALSE = 0x6B5F80, 0x6B6069, 0x6B6075
BURN, CRATER, PLACE, REDRAW = 0x6B59A0, 0x6B5C90, 0x6B6080, 0x486E70
SMUDGE_CTOR, DELETE = 0x6B4A50, 0x7C8B3D
ISO_ITEMS, ISO_COUNT = 0xA8ED2C, 0xA8ED38
SMUDGE_ITEMS, SMUDGE_COUNT = 0xA8EC1C, 0xA8EC28
GAME_ACTIVE = 0xA8E9A0
STEP7_BEGIN, STEP7_END = 0x44177E, 0x4418EC
FOUNDATION_WIDTHS, FOUNDATION_HEIGHTS = 0x8192B8, 0x819310
# Original class vtables for the ground list; What_Am_I (+0x2C) returns 6, 1, 15, 2.
OBJECT_VT = {"building": 0x7E3EBC, "unit": 0x7F5C70, "infantry": 0x7EB058,
             "aircraft": 0x7E22A4}

CELLMEM = 0x22000000
POINTERS = CELLMEM
CELLS, CELL_STRIDE = CELLMEM + 0x100000, 0x200
ISO = CELLMEM + 0x200000
ISO_PLAIN, ISO_MORPHABLE = CELLMEM + 0x238000, CELLMEM + 0x239000
SMUDGE_OBJECTS, SMUDGE_TABLE = CELLMEM + 0x240000, CELLMEM + 0x258000
OBJECTS = CELLMEM + 0x260000
BUILDING, BUILDING_TYPE = CELLMEM + 0x280000, CELLMEM + 0x281000
SCRATCH = CELLMEM + 0x290000

# rulesmd.ini [SmudgeTypes] in list order: (name, Burn, Crater, Width, Height).
SMUDGE_TYPES = ([(f"CR{i}", 0, 0, 1, 1) for i in range(1, 7)]
                + [(f"BURN{i:02}", 0, 0, 1, 1) for i in range(1, 17)]
                + [(f"BURNT{i:02}", 1, 0, 1, 1) for i in range(1, 7)]
                + [("BURNT07", 1, 0, 2, 1), ("BURNT08", 1, 0, 2, 1),
                   ("BURNT09", 1, 0, 1, 2), ("BURNT10", 1, 0, 1, 2),
                   ("BURNT11", 1, 0, 2, 2), ("BURNT12", 1, 0, 2, 2)]
                + [(f"CRATER{i:02}", 0, 1, 1, 1) for i in range(1, 11)]
                + [("CRATER11", 0, 1, 2, 2), ("CRATER12", 0, 1, 2, 2)])


def size_diamond(width, height):
    """In_Bounds 0x00568300: width < x + y <= width + 2 * height and |x - y| < width."""
    return [(x, total - x) for total in range(width + 1, width + 2 * height + 1)
            for x in range(total + 1) if abs(2 * x - total) < width]


def truncate(lepton):
    """cdq / and edx, 0xFF / add / sar 8: division by 256 toward zero."""
    return -(-lepton // 256) if lepton < 0 else lepton // 256


def ranges(*spans):
    return [value for low, high in spans for value in range(low, high + 1)]


class Cells:
    """MapClass's cell table, the IsoTileType and SmudgeType tables and the smudge
    recorders on one `anim_bouncer_launch.Machine`."""

    def __init__(self, machine):
        self.machine = machine
        self.uc = machine.uc
        self.uc.mem_map(CELLMEM, 0x300000)
        self.uc.mem_write(GAME_ACTIVE, b"\x01")
        self.addresses = {}
        self.objects = 0
        self.types = []
        self.smudges = []
        self.redraws = None
        self.current = None

    def construct(self, address):
        uc = self.uc
        uc.mem_write(address, bytes(CELL_STRIDE))
        uc.mem_write(launch.SP - 0x100, dwords(launch.STOP))
        uc.reg_write(UC_X86_REG_ESP, launch.SP - 0x100)
        uc.reg_write(UC_X86_REG_ECX, address)
        run_checked(uc, CELL_CTOR, launch.STOP, count=10_000)

    def install_map(self, spec):
        uc = self.uc
        width, height = spec["size"]
        uc.mem_write(SIZE_FIELD, dwords(width, height))
        uc.mem_write(POINTERS_FIELD, dwords(POINTERS))
        uc.mem_write(POINTERS, bytes(0x100000))
        cells = {(cell["x"], cell["y"]): cell for cell in spec["cells"]}
        allocated = set(cells)
        if spec["allocate_diamond"]:
            diamond = set(size_diamond(width, height))
            assert allocated <= diamond, f"cells outside the Size diamond: {allocated - diamond}"
            allocated = diamond
        assert len(allocated) * CELL_STRIDE <= ISO - CELLS
        self.addresses = {}
        self.objects = 0
        for index, (x, y) in enumerate(sorted(allocated, key=lambda cell: (cell[1], cell[0]))):
            assert 0 <= y * 512 + x < 0x40000
            address = CELLS + index * CELL_STRIDE
            self.construct(address)
            uc.mem_write(address + 0x24, struct.pack("<hh", x, y))
            uc.mem_write(POINTERS + (y * 512 + x) * 4, dwords(address))
            self.addresses[(x, y)] = address
        for key, cell in cells.items():
            self.write_fields(self.addresses[key], cell)
        self.construct(DUMMY)
        if spec.get("dummy"):
            self.write_fields(DUMMY, spec["dummy"])
        count = spec["tile_count"]
        assert count * 4 <= ISO_PLAIN - ISO
        morphable = set(spec["morphable"])
        uc.mem_write(ISO_PLAIN, bytes(0x400))
        uc.mem_write(ISO_MORPHABLE, bytes(0x400))
        uc.mem_write(ISO_MORPHABLE + 0x2E0, b"\x01")
        uc.mem_write(ISO, dwords(*[ISO_MORPHABLE if tile in morphable else ISO_PLAIN
                                   for tile in range(count)]))
        uc.mem_write(ISO_ITEMS, dwords(ISO))
        uc.mem_write(ISO_COUNT, dwords(count))

    def write_fields(self, address, cell):
        uc = self.uc
        for field, offset, size in (("tile", 0x38, 4), ("overlay", 0x44, 4), ("smudge", 0x48, 4),
                                    ("slope", 0x11C, 1), ("overlay_data", 0x11E, 1),
                                    ("smudge_data", 0x11F, 1)):
            if cell.get(field) is not None:
                mask = (1 << (8 * size)) - 1
                uc.mem_write(address + offset, (cell[field] & mask).to_bytes(size, "little"))
        head = 0
        for kind in reversed(cell.get("objects") or []):
            obj = OBJECTS + self.objects * 0x100
            self.objects += 1
            uc.mem_write(obj, bytes(0x100))
            uc.mem_write(obj, dwords(OBJECT_VT[kind]))
            uc.mem_write(obj + 0x30, dwords(head))
            head = obj
        if head:
            uc.mem_write(address + 0xE4, dwords(head))

    def install_smudge_types(self, table):
        uc = self.uc
        self.types = [entry[0] for entry in table]
        for index, (_, burn, crater, width, height) in enumerate(table):
            smudge = SMUDGE_OBJECTS + index * 0x400
            uc.mem_write(smudge, bytes(0x400))
            uc.mem_write(SMUDGE_TABLE + index * 4, dwords(smudge))
            uc.mem_write(smudge + 0x294, dwords(index, width, height))
            uc.mem_write(smudge + 0x2A0, bytes([crater, burn]))
        uc.mem_write(SMUDGE_ITEMS, dwords(SMUDGE_TABLE))
        uc.mem_write(SMUDGE_COUNT, dwords(len(table)))

    def type_name(self, smudge_type):
        index = read32(self.uc, smudge_type + 0x294)
        return self.types[index] if self.types else index

    def hook(self, uc, address):
        """Record the smudge path; True when this address was stubbed."""
        sp = uc.reg_read(UC_X86_REG_ESP)
        if address == GET_CELL_RET:
            if uc.reg_read(UC_X86_REG_EAX) == DUMMY and self.current is not None:
                self.current["dummy_read"] = True
        elif address == CAN_PLACE:
            origin = struct.unpack("<hh", uc.mem_read(read32(uc, sp + 4), 4))
            self.current = dict(call="can_place", type=self.type_name(uc.reg_read(UC_X86_REG_ECX)),
                                origin=list(origin), force=read32(uc, sp + 8) & 0xFF,
                                dummy_read=False)
            self.machine.events.append(self.current)
        elif address in (CAN_PLACE_TRUE, CAN_PLACE_FALSE):
            self.current["result"] = uc.reg_read(UC_X86_REG_AL)
            self.current = None
        elif address == SMUDGE_CTOR:
            smudge_type = read32(uc, sp + 4)
            coord = list(struct.unpack("<iii", uc.mem_read(read32(uc, sp + 8), 12)))
            self.machine.events.append(dict(call="smudge", type=self.type_name(smudge_type),
                                            coord=coord, house=signed(read32(uc, sp + 12))))
            self.smudges.append((smudge_type, coord))
            self.machine.ret(uc.reg_read(UC_X86_REG_ECX), 12)
            return True
        elif address == REDRAW:
            cell = uc.reg_read(UC_X86_REG_ECX)
            self.redraws.append(dict(cell=list(struct.unpack("<hh", uc.mem_read(cell + 0x24, 4))),
                                     dummy=cell == DUMMY, type=signed(read32(uc, cell + 0x48)),
                                     data=uc.mem_read(cell + 0x11F, 1)[0]))
            self.machine.ret(0, 0)
            return True
        elif address == DELETE:
            self.machine.ret(0, 0)
            return True
        return False

    def place_recorded(self):
        """Execute SmudgeTypeClass::Place for each recorded constructor; return its writes."""
        uc = self.uc
        self.redraws = []
        for smudge_type, coord in self.smudges:
            uc.mem_write(SCRATCH, struct.pack("<hh", truncate(coord[0]), truncate(coord[1])))
            uc.mem_write(launch.SP - 0x100, dwords(launch.STOP, SCRATCH))
            uc.reg_write(UC_X86_REG_ESP, launch.SP - 0x100)
            uc.reg_write(UC_X86_REG_ECX, smudge_type)
            run_checked(uc, PLACE, launch.STOP, count=100_000)
            assert uc.reg_read(UC_X86_REG_ESP) == launch.SP - 0x100 + 8
        marked, self.redraws, self.smudges = self.redraws, None, []
        return marked


class Machine(launch.Machine):
    def __init__(self, seed):
        super().__init__(seed)
        self.cells = Cells(self)

    def hook(self, uc, address, size, data):
        if not self.cells.hook(uc, address):
            super().hook(uc, address, size, data)


# -- the synthetic map ------------------------------------------------------------------------
# Size 6x4: 44 cells, x 1..9, y 1..9 (row y=5 is x 2..9). Unlisted cells keep the
# constructor's IsoTileTypeIndex 0xFFFF, which CanPlace reads as tile 0.
GATE_CELLS = [
    dict(x=5, y=4, tile=1),                          # valid, Morphable
    dict(x=6, y=4, tile=2),                          # valid, not Morphable
    dict(x=4, y=4, overlay=7, overlay_data=3),
    dict(x=7, y=4, smudge=3, smudge_data=1),
    dict(x=4, y=5, slope=2),
    dict(x=5, y=5, tile=-1),                         # below 0: tile 0
    dict(x=6, y=5, tile=4),                          # the count: tile 0
    dict(x=7, y=5, objects=["building"]),
    dict(x=8, y=5, objects=["unit"]),
    dict(x=3, y=5, objects=["infantry", "building"]),
    dict(x=5, y=6, objects=["aircraft"]),
    dict(x=6, y=6, tile=0x7FFFFFFF),
    dict(x=4, y=6, tile=-0x80000000),
    dict(x=3, y=6, tile=3),
    dict(x=7, y=6, objects=["unit", "infantry"]),
    dict(x=5, y=7, slope=4, overlay=0, overlay_data=0),
]


def gate_map(tile0_morphable=True, dummy=None):
    return dict(size=[6, 4], allocate_diamond=True, cells=GATE_CELLS, tile_count=4,
                morphable=([0] if tile0_morphable else []) + [1, 3], dummy=dummy)


MAPS = {
    "gates": gate_map(),
    "gates_tile0_plain": gate_map(tile0_morphable=False),
    # The dummy after a Place wrote it (SmudgeTypeIndex 0, SmudgeData 0).
    "gates_dummy_marked": gate_map(dummy=dict(smudge=0, smudge_data=0)),
}
FOOTPRINTS = [(1, 1), (2, 1), (1, 2), (2, 2), (3, 2), (2, 3)]
OUTSIDE = [(0, 0), (1, 1), (9, 6), (10, 5), (-1, 5), (5, -1), (0x7FFF, 2)]


def can_place_rows():
    rows = []
    for name, spec in MAPS.items():
        machine = Machine(1)
        machine.cells.install_map(spec)
        uc = machine.uc
        smudge = SMUDGE_OBJECTS
        width, height = spec["size"]
        origins = sorted(size_diamond(width, height), key=lambda cell: (cell[1], cell[0])) + OUTSIDE
        for origin in origins:
            for w, h in FOOTPRINTS:
                for force in (0, 1):
                    uc.mem_write(smudge, bytes(0x400))
                    uc.mem_write(smudge + 0x294, dwords(0, w, h))
                    uc.mem_write(SCRATCH, struct.pack("<hh", *origin))
                    uc.mem_write(launch.SP - 0x100, dwords(launch.STOP, SCRATCH, force))
                    uc.reg_write(UC_X86_REG_ESP, launch.SP - 0x100)
                    uc.reg_write(UC_X86_REG_ECX, smudge)
                    machine.events.clear()
                    run_checked(uc, CAN_PLACE, launch.STOP, count=100_000)
                    assert uc.reg_read(UC_X86_REG_ESP) == launch.SP - 0x100 + 12
                    [event] = machine.events
                    rows.append(dict(map=name, origin=list(origin), width=w, height=h, force=force,
                                     result=event["result"], dummy_read=event["dummy_read"]))
    return rows


def finish(machine, case, before):
    assert before == seed_state(case["seed"])
    events = [event for event in machine.events if event["call"] != "new"]
    return dict(input=case, events=events, marked=machine.cells.place_recorded(),
                raw_draw_count=machine.advances, rng_after=machine.rng())


@lru_cache(maxsize=None)
def seed_state(seed):
    return launch.Machine(seed).rng()


def run_placer(case):
    machine = Machine(case["seed"])
    machine.cells.install_map(MAPS[case["map"]])
    machine.cells.install_smudge_types(SMUDGE_TYPES)
    uc = machine.uc
    before = machine.rng()
    machine.events.clear()
    machine.advances = 0
    uc.mem_write(SCRATCH, struct.pack("<iii", *case["coord"]))
    uc.mem_write(launch.SP - 0x100, dwords(launch.STOP, case["height"], case["force"]))
    uc.reg_write(UC_X86_REG_ESP, launch.SP - 0x100)
    uc.reg_write(UC_X86_REG_ECX, SCRATCH)
    uc.reg_write(UC_X86_REG_EDX, case["width"])
    run_checked(uc, BURN if case["kind"] == "burn" else CRATER, launch.STOP, count=5_000_000)
    assert uc.reg_read(UC_X86_REG_ESP) == launch.SP - 0x100 + 12
    return finish(machine, case, before)


def placer_cases():
    def centre(x, y):
        return [x * 256 + 0x80, y * 256 + 0x80, 0]
    coords = [
        centre(5, 5),                 # 2x2 block (5..6, 5..6): tiles -1, 4, 0x7FFFFFFF read as 0
        centre(3, 4),                 # a building at (3, 5) under the 1x2/2x2 footprints
        centre(6, 3),                 # right (7, 3) clean, down (6, 4) not Morphable
        centre(8, 4),                 # (9, 4), (8, 5), (9, 5): on the diamond's right edge
        centre(9, 5),                 # (10, 5), (9, 6), (10, 6): outside, the dummy
        centre(4, 4),                 # overlay on the origin
        [5 * 256 + 5, 6 * 256 + 250, 7],        # off-centre in (5, 6)
        [-100, 200, 0],               # truncates to (0, 0): the sentinel
        [-300, 5 * 256, 0],           # (-1, 5)
        [10 * 256 + 0x80, 5 * 256 + 0x80, 0],   # (10, 5): past the diamond's last column
        [1 * 256 + 0x80, 1 * 256 + 0x80, 0],    # (1, 1): outside the diamond
    ]
    for coord in coords:
        for kind in ("burn", "crater"):
            for force, (width, height) in ((1, (0x64, 0x64)), (0, (30, 30)), (0, (0x3D, 0x33))):
                for seed in (1, 0x5CA1AB1E):
                    yield dict(map="gates", kind=kind, coord=coord, width=width, height=height,
                               force=force, seed=seed)


def run_centre_mark(case):
    machine = Machine(case["seed"])
    machine.cells.install_map(MAPS[case["map"]])
    machine.cells.install_smudge_types(SMUDGE_TYPES)
    uc = machine.uc
    foundation = case["foundation"]
    uc.mem_write(BUILDING_TYPE, bytes(0x1000))
    uc.mem_write(BUILDING_TYPE + 0xEF0, dwords(foundation))
    uc.mem_write(BUILDING, bytes(0x800))
    uc.mem_write(BUILDING + 0x9C, struct.pack("<iii", *case["location"]))
    uc.mem_write(BUILDING + 0x520, dwords(BUILDING_TYPE))
    before = machine.rng()
    machine.events.clear()
    machine.advances = 0
    uc.reg_write(UC_X86_REG_ESP, launch.SP - 0x800)
    uc.reg_write(UC_X86_REG_ESI, BUILDING)
    uc.reg_write(UC_X86_REG_EBX, 0)
    run_checked(uc, STEP7_BEGIN, STEP7_END, count=5_000_000)
    row = finish(machine, case, before)
    row["foundation_size"] = [read32(uc, FOUNDATION_WIDTHS + foundation * 4),
                              read32(uc, FOUNDATION_HEIGHTS + foundation * 4)]
    return row


def centre_mark_cases():
    def centre(x, y, z=0):
        return [x * 256 + 0x80, y * 256 + 0x80, z]
    locations = [
        centre(5, 5),                 # the 2x2 block whose tiles read as 0
        centre(4, 4, 104),            # overlay on the origin
        centre(5, 4),                 # origin tile 1, right tile 2 (not Morphable)
        centre(7, 5),                 # a building on the origin (force 1 skips the lookup)
    ]
    for foundation in range(22):
        for location in locations:
            for seed in (1, 0xDEADBEEF):
                yield dict(map="gates", foundation=foundation, location=location, seed=seed)


def generate():
    placer = [run_placer(case) for case in placer_cases()]
    centre_mark = [run_centre_mark(case) for case in centre_mark_cases()]
    seeds = sorted({row["input"]["seed"] for row in placer + centre_mark})
    return dict(smudge_types=[dict(name=name, burn=burn, crater=crater, width=width, height=height)
                              for name, burn, crater, width, height in SMUDGE_TYPES],
                seed_states={str(seed): seed_state(seed) for seed in seeds},
                maps=MAPS, can_place=can_place_rows(),
                placer=placer, centre_mark=centre_mark)


if __name__ == "__main__":
    finish_vectors(generate, Path(__file__).with_suffix(".json"), provenance=lambda: provenance(
        scope=("SmudgeTypeClass::CanPlace 0x006B5F80 over MapClass's cell table (GetCell "
               "0x005657A0, In_Bounds 0x00568300, the building lookup 0x0047C520 and the "
               "IsoTileType Morphable table, all original) for every origin of a Size 6x4 "
               "map and seven outside it, six footprints and both force values on three map "
               "variants; the Burn/Crater placers 0x006B59A0/0x006B5C90 at eleven "
               "coordinates; BuildingClass::DestructionEffects step 7 0x0044177E..0x004418EC "
               "for all 22 foundation indices at four locations; each recorded SmudgeClass is "
               "followed by SmudgeTypeClass::Place 0x006B6080."),
        assumptions=[
            "x87 control word 0x0E7F (PC53, chop) on entry, as anim_bouncer_launch.",
            "g_GameActive 0x00A8E9A0 set, as in a running game.",
            "Cells and the dummy come from the CellClass constructor 0x0047BBF0; rows write "
            "only the fields listed in `maps` (a synthetic map; its cells are not a retail "
            "map's).",
            "SmudgeType fields: the retail [SmudgeTypes] list's Burn=, Crater=, Width=, "
            "Height= (rulesmd.ini).",
            "The constructor's Unlimbo -> SmudgeClass::Mark -> Place path is read, not run: "
            "Location is the coordinate, Mark's cell is its truncation and Mark's CanPlace "
            "with force 1 passes on the footprint the placer admitted.",
        ],
        substitutions=[
            "the SmudgeClass constructor 0x006B4A50 records its type, coordinate and house "
            "and returns; SmudgeTypeClass::Place 0x006B6080 then runs on the recorded type "
            "and truncated cell",
            "the per-cell redraw 0x00486E70 in Place records its cell and returns",
            "operator new 0x007C8E17 is a bump allocator; operator delete 0x007C8B3D is a no-op",
        ],
        entry_points={"can_place": CAN_PLACE, "get_cell": GET_CELL, "in_bounds": IN_BOUNDS,
                      "building_lookup": 0x47C520, "cell_ctor": CELL_CTOR, "burn": BURN,
                      "crater": CRATER, "place": PLACE, "step7": STEP7_BEGIN,
                      "seed": launch.SEED},
    ))
