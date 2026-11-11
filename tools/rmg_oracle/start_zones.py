"""Original start-zone typing and connectivity for CreateStartingPoints.

`python -m tools.rmg_oracle.start_zones --check` replays, per control,
`CellClass::RecalcZoneType @ 0x00483C80` on every fixture Cell (with its
nested `MapClass::IsCellInPlayfield @ 0x00578460`), publishes each class and
level into the MapClass zone records as `CellClass::RecalcAttributes @
0x0047D2B0` does at its exits, then runs `RandomMapGenerator::
CreateStartingPoints @ 0x00594B50` up to 0x00594C47 with the original
`MapClass::RebuildZoneConnectivity @ 0x0056C510` (and its
`MapClass::FloodFillBaseZoneScanline @ 0x0056CB90`) and `MapClass::GetZoneID
@ 0x0056D230` live; `--write` replaces the corpus
`zones::tests::native_start_zone_controls` consumes. Land types, overlays and
occupiers are declared as the last RecalcAttributes leaves them.
"""
from pathlib import Path
import struct

RECALC_ZONE = 0x483C80
CELL_OVERLAY, CELL_ZONE_TYPE, CELL_OBJECT, CELL_LAND = 0x44, 0x4C, 0xE4, 0xEC
CELL_LEVEL = 0x11B
OVERLAY_LAND = 0x298        # OverlayTypeClass Land, read at 483CDF
WHEEL_ROWS, WHEEL_STRIDE = 0x89EA48, 36   # per-land Wheel float, 483CE8/483D57
BUILDING_VTABLE_PROBE = 0x2C              # WhatAmI slot the occupier walk calls
BUILDING_TYPE = 0x520                     # Building -> BuildingType, 483D8E
OBJECT_NEXT = 0x30
LAND = dict(clear=0, road=1, water=2, rock=3, beach=6)
# Declared Wheel speeds: blocked (0) for Water, Rock, Wall and Beach, open
# (100%) for every other land type.
WHEEL_BLOCKED = [land in (2, 3, 4, 6) for land in range(12)]
DECK = 0x5C                 # a low-bridge deck overlay the generator stamps
STRIDE = 33
BUCKET_SLOTS = 512


def controls():
    """Declared inputs over the 16x16 fixture map: `gen` is the generated
    size in the local rect (2, 5, w, h); `land` `[x, y, name]`; `terrain`
    `[x, y, level, slope]`; `deck` cells carry the low-bridge overlay with
    Road land; `hut` cells a building with neither FirestormWall nor
    LaserFence."""
    from tools.rmg_oracle.start_points import box
    lake = [[x, y, 'water'] for x, y in box(15, 15, 19, 19)]
    rows = [
        dict(id='open_ground_amphibious', map_type=1),
        # Amphibious row 5 joins water and beach with the shore; Normal row 0
        # does not.
        dict(id='lake_and_beach_amphibious', map_type=0,
             land=lake + [[x, 20, 'beach'] for x in range(14, 21)]),
        dict(id='lake_and_beach_normal', map_type=3,
             land=lake + [[x, 20, 'beach'] for x in range(14, 21)]),
        # Level steps: two levels split leftward and vertical runs, three do
        # not split rightward ones; four-level cliffs and a sloped edge.
        dict(id='plateaus_and_slopes', map_type=4,
             terrain=[[x, y, 2, 0] for x, y in box(12, 12, 15, 18)]
                     + [[x, y, 3, 0] for x, y in box(18, 12, 21, 15)]
                     + [[x, y, 4, 0] for x, y in box(18, 19, 22, 23)]
                     + [[x, y, 0, 2] for x, y in box(10, 20, 13, 22)]),
        # A rock wall splits the ground; the larger side is the reference.
        dict(id='rock_wall_split', map_type=2,
             land=[[x, 17, 'rock'] for x in range(0, 33)]),
        # A water channel crossed by a low-bridge deck (Road land).
        dict(id='deck_joins_shores', map_type=3,
             land=[[x, y, 'water'] for x in range(0, 33) for y in (16, 17, 18)],
             deck=[[x, y] for x in (18, 19, 20) for y in (16, 17, 18)]),
        # A bridge-repair hut on open ground stays class 0.
        dict(id='hut_stays_ground', map_type=4, hut=[[20, 20], [14, 18]]),
    ]
    rows = [dict(gen=[12, 12], land=[], terrain=[], deck=[], hut=[]) | row for row in rows]
    from tools.rmg_oracle.tiberium_field import in_diamond
    for row in rows:
        for key in ('land', 'terrain', 'deck', 'hut'):
            row[key] = [item for item in row[key] if in_diamond(item[0], item[1])]
    return rows


class StartZonesFixture:
    def __init__(self):
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.create_starting_points import CreateStartsFixture
        from tools.rmg_oracle.tiberium_field import OVERLAY_TYPES
        self.creates = CreateStartsFixture()
        self.starts, self.terrain = self.creates.starts, self.creates.terrain
        self.m, self.u = m, u = self.creates.m, self.creates.u
        for land, blocked in enumerate(WHEEL_BLOCKED):
            u.mem_write(WHEEL_ROWS + land * WHEEL_STRIDE, struct.pack('<f', 0.0 if blocked else 1.0))
        deck_type = m.read32(m.read32(OVERLAY_TYPES) + DECK * 4)
        u.mem_write(deck_type + OVERLAY_LAND, words(LAND['road']))

    def building(self):
        """A zeroed building whose type sets neither +16C0 nor +16BF; its
        vtable slot +2C is a stub that answers Building (6). Allocated per
        control: each scene restarts the arena."""
        from tools.rmg_oracle.cliff_materialization import words
        m, u = self.m, self.u
        kind = m.alloc(0x2000)
        u.mem_write(kind, bytes(0x2000))
        what_am_i = m.alloc(8)
        u.mem_write(what_am_i, bytes([0xB8, 6, 0, 0, 0, 0xC3]))  # mov eax, 6; ret
        vtable = m.alloc(0x40)
        u.mem_write(vtable, bytes(0x40))
        u.mem_write(vtable + BUILDING_VTABLE_PROBE, words(what_am_i))
        hut = m.alloc(0x700)
        u.mem_write(hut, bytes(0x700))
        u.mem_write(hut, words(vtable))
        u.mem_write(hut + BUILDING_TYPE, words(kind))
        return hut

    def zone_state(self):
        """MapClass zone records, empty edge buckets and rows, no bridge
        records; stride Size.W + Size.H + 1 = 33."""
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.create_starting_points import MAP
        m, u = self.m, self.u
        u.mem_write(MAP + 0xF4, words(16, 16))
        records = m.alloc(STRIDE * STRIDE * 4)
        u.mem_write(records, b'\x07\x00\x00\x00' * (STRIDE * STRIDE))
        plane = m.alloc(STRIDE * STRIDE * 10)
        u.mem_write(plane, bytes(STRIDE * STRIDE * 10))
        u.mem_write(MAP + 0x68, words(records, STRIDE * STRIDE, plane))
        root, buckets = m.alloc(12), m.alloc(256 * 24)
        u.mem_write(root, words(buckets, 256, 0))
        for bucket in range(256):
            data = m.alloc(BUCKET_SLOTS * 8)
            u.mem_write(buckets + bucket * 24, words(0, data, BUCKET_SLOTS, 0, 0, 16))
        u.mem_write(MAP + 0x14, words(root))
        u.mem_write(MAP + 0x18, bytes(13 * 4))
        u.mem_write(MAP + 0x60, words(0))
        return records

    def run(self, case, pool):
        from tools import native_oracle as native
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.create_starting_points import ENTRY, REBUILT, MAP
        from tools.rmg_oracle.region_split import REGION_LIST, NEXT_REGION_ID
        from tools.rmg_oracle.start_points import published
        f, m, u = self.terrain, self.m, self.u
        self.creates.prepare(dict(case, terrain=[]))
        rect = [2, 5, *case['gen']]
        u.mem_write(MAP + 0xFC, words(*rect))
        for x, y, level, slope in case['terrain']:
            u.mem_write(f.cells[(x, y)] + CELL_LEVEL, bytes([level & 255, slope]))
        for p in f.cells.values():
            u.mem_write(p + CELL_LAND, words(0))
            u.mem_write(p + CELL_OVERLAY, words(-1))
            u.mem_write(p + CELL_OBJECT, words(0))
        for x, y, name in case['land']:
            u.mem_write(f.cells[(x, y)] + CELL_LAND, words(LAND[name]))
        for x, y in case['deck']:
            u.mem_write(f.cells[(x, y)] + CELL_OVERLAY, words(DECK))
            u.mem_write(f.cells[(x, y)] + CELL_LAND, words(LAND['road']))
        for x, y in case['hut']:
            u.mem_write(f.cells[(x, y)] + CELL_OBJECT, words(self.building()))
        records = self.zone_state()
        for (x, y), p in sorted(f.cells.items()):
            m.invoke(RECALC_ZONE, p, (), context=dict(control=case['id'], cell=[x, y]))
            record = records + min(max(y * STRIDE + x, 0), STRIDE * STRIDE - 1) * 4
            u.mem_write(record, bytes([m.read32(p + CELL_ZONE_TYPE) & 255,
                                       u.mem_read(p + CELL_LEVEL, 1)[0]]))
        largest, reference = [], []

        def observe(machine, pc, sp):
            from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBP
            if pc == 0x594BAE:  # the instruction after the 56C510 call
                largest.append(machine.reg_read(UC_X86_REG_EAX))
            elif pc == 0x594BC5:  # BP holds the reference zone 594BC1 read
                reference.append(machine.reg_read(UC_X86_REG_EBP) & 0xFFFF)
            return False

        # The scanline flood recurses on the native stack: a deep frame.
        frame = m.alloc(0x80000) + 0x7FC00
        row = self.starts.execute(case, pool, ENTRY, 0, (), extra=observe, until=REBUILT,
                                  frame=frame)
        native.require(len(largest) == 1, f"{case['id']}: 56C510 return not observed")
        count = m.read32(MAP + 0x4C)
        kind = [5, 5, 5, 0, 0][case['map_type']]
        derived = list(struct.unpack(f'<{count}H', u.mem_read(m.read32(MAP + 0x18 + kind * 4), count * 2)))
        raw = bytes(u.mem_read(records, STRIDE * STRIDE * 4))
        classes = [''.join('%d' % raw[(y * STRIDE + x) * 4] for x in range(STRIDE)) for y in range(STRIDE)]
        zones = [list(struct.unpack_from('<H', raw, (y * STRIDE + x) * 4 + 2)[0] for x in range(STRIDE))
                 for y in range(STRIDE)]
        regions = self.creates.regions(self.creates.items, m.read32(REGION_LIST + 0x10))
        return published(row) | dict(
            classes=classes, zones=zones, largest=largest[0], reference=reference[0],
            derived=derived,
            regions=[r[:2] for r in regions], next_region_id=m.read32(NEXT_REGION_ID))


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = StartZonesFixture(), StatePool()
    rows = [fixture.run(case, pool) for case in controls()]
    return dict(schema=1, scene=dict(wheel_blocked=WHEEL_BLOCKED, deck=DECK),
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='7 controls of RecalcZoneType on every fixture Cell, then CreateStartingPoints 594B50..594C47 with the original RebuildZoneConnectivity, FloodFillBaseZoneScanline and GetZoneID: open ground, water and beach under Amphibious (row 5) and Normal (row 0), plateaus of two, three and four levels and a sloped edge, a Rock wall split, a water channel crossed by a low-bridge deck, a bridge-repair hut; every Cell zone class, base zone id, the returned largest zone, the requested derived row, the rebuilt region ids and sizes',
        assumptions=[
            'Existing CliffFixture, TiberiumFieldFixture, StartPointsFixture and CreateStartsFixture own original-constructed Cell backing, admission, the stride-33 scratch, the physical clear/cliff TMP registration and the guarded arena; bounds 16..48, MapClass +F4/+F8 size 16x16, +FC local rect (2, 5, 12, 12).',
            'Declared Cell state as the last RecalcAttributes 47D2B0 leaves it: land type +EC (Clear unless declared Water, Beach, Rock or Road), overlay +44 (-1, or the deck overlay 5C whose type Land +298 is Road), first object +E4, level/slope +11B. Declared Wheel row floats at 89EA48: 0 for Water, Rock, Wall and Beach, 1.0 for the rest.',
            'The class and level bytes RecalcZoneType leaves are written to each MapClass +68 record as 47D2B0 publishes them; records without a Cell start class 7. Edge buckets are empty with 512 slots each; the 13 rows start null; no bridge records (+60 count 0). Not a complete Generate prefix.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS; native code bytes unchanged.',
            'The hut is a zeroed object whose vtable +2C answers Building (6) and whose type +520 is zeroed (FirestormWall +16C0 and LaserFence +16BF clear), not a constructed CABHUT.'],
        entry_points=dict(recalc_zone_type=RECALC_ZONE, is_cell_in_playfield=0x578460,
                          create_starting_points=0x594B50, rebuilt=0x594C47,
                          rebuild_zones=0x56C510, flood=0x56CB90, get_zone=0x56D230))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
