"""Original CreateStartingPoints region rebuild and quota split.

`python -m tools.rmg_oracle.create_starting_points --check` replays two
slices of `RandomMapGenerator::CreateStartingPoints @ 0x00594B50`; `--write`
replaces the corpus `starts::tests::native_create_starting_points_controls`
consumes:
- 0x00594B50..0x00594C47, the teardown
  `RmgRegion::ClearAll @ 0x0058CE90` and the rebuild: every unclaimed
  reference-ground cell seeds `RmgRegion::FloodFromCell @ 0x00594420`, whose
  regions `RmgRegion::RmgRegion @ 0x0058BF70` constructs;
- 0x00594C47..0x00594E52, the small-region deletion
  (`RmgRegion::~RmgRegion @ 0x0058C070`), the score sort and the cumulative
  quota split, on declared region records.
The zones are declared: `MapClass::RebuildZoneConnectivity @ 0x0056C510` and
`MapClass::GetZoneID @ 0x0056D230` return supplied values. The per-region
`PlaceStartingPoints` calls are compared by `start_points`; the map-type-0
tech coin flips and the scenario preview header are not run.
"""
from pathlib import Path
import struct

ENTRY, REBUILT, SPLIT_END = 0x594B50, 0x594C47, 0x594E52
REBUILD_ZONES, GET_ZONE = 0x56C510, 0x56D230
MAP = 0x87F7E8
ZONE_TABLES = MAP + 0x18   # per zone type: the derived-zone ushort table 594BBA reads
CELL_ZONE_TYPE = 0x4C      # Cell field 594C14 requires zero
REFERENCE, OTHER = 7, 9    # declared derived zone ids
LIST_SLOTS = 64
THRESHOLD_SLOT = 0x18      # [ESP+18] holds the threshold from 594BA5


def rebuild_controls():
    """Declared inputs: `ground` cells report the reference zone, all others
    another; `typed` cells a nonzero Cell zone type; `terrain` `[x, y,
    level]`; `water` cells the WaterSet base tile."""
    from tools.rmg_oracle.start_points import box
    from tools.rmg_oracle.tiberium_field import ring
    open_ground = box(0, 0, 32, 32)
    walls = ring(11, 11, 19, 19)
    rows = [
        dict(id='one_open_region', map_type=1, ground=open_ground),
        # A ring of other-zone cells splits the ground into two regions.
        dict(id='walls_split_regions', map_type=3,
             ground=[xy for xy in open_ground if xy not in walls]),
        # Typed cells refuse even inside the reference zone; levels mark seeds.
        dict(id='typed_cells_and_levels', map_type=0, ground=open_ground,
             typed=ring(16, 16, 24, 24), terrain=[[x, y, 3] for x, y in box(17, 17, 23, 23)]),
        # Diagonal steps join: the flood walks all eight neighbours.
        dict(id='diagonal_islands', map_type=2,
             ground=[xy for xy in open_ground if (xy[0] + xy[1]) % 2 == 0]),
        # A region seeded on a water tile takes the water flag (+14).
        dict(id='water_seed_flag', map_type=1, ground=box(9, 9, 13, 13) + box(18, 18, 24, 24),
             water=box(9, 9, 13, 13)),
    ]
    return [dict(ground=[], typed=[], terrain=[], water=[]) | row for row in rows]


def split_controls():
    """Declared region records `[id, cells]` in list order, the threshold
    and the start quota (MapSeed +50)."""
    rows = [
        dict(id='two_regions_even', threshold=400, quota=2, regions=[[0, 900], [1, 900]]),
        dict(id='deletions_and_rounding', threshold=400, quota=4,
             regions=[[0, 1200], [1, 399], [2, 2500], [3, 400], [4, 700], [5, 120]]),
        dict(id='quota_exceeds_regions', threshold=500, quota=8,
             regions=[[4, 3000], [9, 600], [2, 1800]]),
        # Equal sizes order by id; the last bucket takes the remainder.
        dict(id='equal_sizes_and_remainder', threshold=400, quota=3,
             regions=[[5, 1000], [3, 1000], [8, 1000], [1, 1000]]),
        dict(id='every_region_deleted', threshold=2000, quota=2, regions=[[0, 1999], [1, 50]]),
        dict(id='zero_quota', threshold=400, quota=0, regions=[[0, 800], [1, 4000]]),
    ]
    return rows


class CreateStartsFixture:
    def __init__(self):
        from tools.rmg_oracle.start_points import StartPointsFixture
        self.starts = StartPointsFixture()
        self.base, self.terrain = self.starts.base, self.starts.terrain
        self.m, self.u = self.starts.m, self.starts.u

    def diamond(self):
        from tools.rmg_oracle.tiberium_field import in_diamond, STRIDE
        return [(x, y) for y in range(STRIDE) for x in range(STRIDE) if in_diamond(x, y)]

    def prepare(self, case):
        """The scene, an empty region list as region_split builds it, and the
        map type and generated size the threshold reads."""
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.region_split import (REGION_LIST, REGION_LIST_VTABLE,
                                                   NEXT_REGION_ID, MAP_TYPE, GEN_W)
        from tools.rmg_oracle.start_points import MAPSEED
        m, u = self.m, self.u
        self.starts.scene(dict(seed=1, terrain=[[x, y, level, 0] for x, y, level
                                                in case.get('terrain', [])]))
        self.items = m.alloc(LIST_SLOTS * 4)
        u.mem_write(REGION_LIST, words(REGION_LIST_VTABLE, self.items, LIST_SLOTS))
        u.mem_write(REGION_LIST + 0x10, words(0, 10))
        u.mem_write(REGION_LIST + 0xD, b'\0')
        u.mem_write(NEXT_REGION_ID, words(0))
        u.mem_write(MAP_TYPE, words(case.get('map_type', 1)))
        u.mem_write(GEN_W, words(12, 12))
        u.mem_write(MAPSEED + 0x50, words(case.get('quota', 2)))
        for p in self.terrain.cells.values():
            u.mem_write(p + CELL_ZONE_TYPE, words(0))

    def regions(self, items, count):
        """[id, cells, level, water flag, quota, cell list] per record."""
        from tools.rmg_oracle.region_split import REGION_LIST
        m, u = self.m, self.u
        out = []
        for slot in range(count):
            obj = m.read32(items + slot * 4)
            ident, cells, level = struct.unpack('<iii', u.mem_read(obj + 8, 12))
            listed = m.read32(obj + 0x2C)
            coords = [list(struct.unpack('<hh', u.mem_read(listed + n * 4, 4)))
                      for n in range(m.read32(obj + 0x38))]
            out.append([ident, cells, level, u.mem_read(obj + 0x14, 1)[0],
                        struct.unpack('<i', u.mem_read(obj + 0x20, 4))[0], coords])
        return out

    def run_rebuild(self, case, pool):
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.region_split import REGION_LIST, NEXT_REGION_ID
        from tools.rmg_oracle.start_points import published
        from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP
        f, m, u = self.terrain, self.m, self.u
        self.prepare(case)
        for x, y in case['typed']:
            u.mem_write(f.cells[(x, y)] + CELL_ZONE_TYPE, words(1))
        for x, y in case['water']:
            u.mem_write(f.cells[(x, y)] + 0x38, words(m.read32(0xAA0738)))
        # RebuildZoneConnectivity returns index 0; every zone type's table
        # names the reference zone there.
        table = m.alloc(4)
        u.mem_write(table, struct.pack('<H', REFERENCE))
        for kind in range(8):
            u.mem_write(ZONE_TABLES + kind * 4, words(table))
        ground = {tuple(xy) for xy in case['ground']}
        zone_calls = []

        def back(machine, sp, cleanup, eax):
            machine.reg_write(UC_X86_REG_EAX, eax)
            machine.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)
            machine.reg_write(UC_X86_REG_EIP, m.read32(sp))

        def stub(machine, pc, sp):
            if pc == REBUILD_ZONES:
                back(machine, sp, 0, 0)
            elif pc == GET_ZONE:
                cell = struct.unpack('<hh', machine.mem_read(m.read32(sp + 4), 4))
                zone_calls.append(m.read32(sp + 8))
                back(machine, sp, 0xC, REFERENCE if cell in ground else OTHER)
            else:
                return False
            return True

        frame = m.alloc(0x400) + 0x200
        row = self.starts.execute(case, pool, ENTRY, 0, (), extra=stub, until=REBUILT, frame=frame)
        kinds = sorted(set(zone_calls))
        assert kinds == [[5, 5, 5, 0, 0][case['map_type']]] or not zone_calls, kinds
        records = [[x, y, *struct.unpack('<ii', u.mem_read(self.base.record(x, y) + 0x38, 8))]
                   for x, y in self.diamond()]
        return published(row) | dict(
            regions=self.regions(self.items, m.read32(REGION_LIST + 0x10)),
            next_region_id=m.read32(NEXT_REGION_ID),
            records=[r for r in records if r[2:] != [-1, -1]])

    def run_split(self, case, pool):
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.region_split import REGION_LIST, REGION_CTOR
        from tools.rmg_oracle.start_points import published
        m, u = self.m, self.u
        self.prepare(case)
        seed = struct.unpack('<I', struct.pack('<hh', 16, 16))[0]
        for ident, cells in case['regions']:
            obj = m.alloc(0x50)
            m.invoke(REGION_CTOR, obj, (seed,))
            u.mem_write(obj + 8, words(ident, cells))
        frame = m.alloc(0x400) + 0x200
        u.mem_write(frame + THRESHOLD_SLOT, words(case['threshold']))
        row = self.starts.execute(case, pool, REBUILT, 0, (), until=SPLIT_END, frame=frame)
        sorted_items, sorted_count = m.read32(frame + 0x50), m.read32(frame + 0x5C)
        survivors = self.regions(self.items, m.read32(REGION_LIST + 0x10))
        ordered = self.regions(sorted_items, sorted_count)
        return published(row) | dict(survivors=[r[0] for r in survivors],
                                     order=[r[0] for r in ordered],
                                     quotas=[r[4] for r in ordered])


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = CreateStartsFixture(), StatePool()
    rebuild = [fixture.run_rebuild(case, pool) for case in rebuild_controls()]
    split = [fixture.run_split(case, pool) for case in split_controls()]
    return dict(schema=1, scene=dict(water_base=fixture.m.read32(0xAA0738)),
                rng_states_hex=pool.states, rebuild=rebuild, split=split)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='5 runs of CreateStartingPoints 594B50..594C47 (region teardown, the rebuild scan and its eight-neighbour floods over walls, typed cells, levels, diagonal islands and a water seed) and 6 runs of 594C47..594E52 (small-region deletion, score sort, cumulative quota split with rounding, equal sizes, every region deleted, quota 0); every region record, cell list, scratch region and stamp, survivor, order and quota',
        assumptions=[
            'Existing CliffFixture, TiberiumFieldFixture and StartPointsFixture own original-constructed Cell backing, admission, the stride-33 scratch, the physical clear/cliff TMP registration and the guarded arena; bounds 16..48. The region list is built as region_split builds it (vtable 7ED858, empty, next id 0). Generated size 12x12, so the threshold is the 400 floor.',
            'Declared zones: RebuildZoneConnectivity returns 0 and every zone type table holds the reference zone 7 at index 0; GetZoneID returns 7 for declared ground cells and 9 elsewhere. Cell +4C zone type 0 unless declared 1. Declared levels and WaterSet tiles. Map type selects the zone type the calls pass (5 for types 0..2, 0 for 3, 4).',
            'The split slice starts at 594C47 on a fresh frame with the threshold in [ESP+18] and EBX 0; region records come from the original constructor with declared ids and cell counts. Start quota in MapSeed +50. FPCW 0E7F. Not a complete Generate prefix.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS; native code bytes unchanged.',
            'MapClass::RebuildZoneConnectivity 56C510 and MapClass::GetZoneID 56D230 return the declared zones without running: the zone computation itself is not compared here.'],
        entry_points=dict(create_starting_points=ENTRY, rebuilt=REBUILT, split_end=SPLIT_END,
                          clear_all=0x58CE90, flood=0x594420, region_ctor=0x58BF70,
                          region_dtor=0x58C070, rebuild_zones=REBUILD_ZONES, get_zone=GET_ZONE))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
