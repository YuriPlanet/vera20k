"""Original BridgeAndConnectorPass over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.bridge_pass --check` replays whole original
`RandomMapGenerator::BridgeAndConnectorPass @ 0x0058EF10` calls: the
pinched-cliff raise (`MapClass::RaisePinchedCliffCell @ 0x00579010`), the
region teardown and `FloodBuildRegion @ 0x0058C800` rebuild,
`RmgRegion::BuildNeighbourListAndCellCount @ 0x0058F0C0`, and
`RmgRegion::CarveConnectorsOrBridges @ 0x005905D0` with its ramp carvers,
`PlaceLowBridgeDeck @ 0x0058F2C0` and `PlaceBridgeRepairHut @ 0x005904B0`.
`--write` replaces the corpus `island_passes::tests::native_bridge_pass_controls`
consumes. Original code computes every level, tile, overlay, region, hut cell
and draw. The scene is a declared component input, not a complete Generate
prefix.
"""
from pathlib import Path
import struct

ENTRY = 0x58EF10
RNG_NEXT = 0x65C780
REGION_LIST = 0xABDF90  # DynamicVectorClass<RmgRegion*>: +4 items, +8 size, +0x10 count
REGION_LIST_VTABLE = 0x7ED858
NEXT_REGION_ID = 0xABED14
LINEAR_OFFSETS = 0x89A304
LIST_SLOTS = 512
MAPSEED = 0xABDFD8  # +0x6C accessibility, +0x180/+0x184 generated size, +0x30C default level
SIZE = 24  # map side: backing 48x48, scratch stride 49, diamond 24..72
STRIDE = 2 * SIZE + 1
GEN = [22, 16]  # generated size; MapClass +FC local rect (2, 5, 22, 16)
# Tile-set globals the scene and the stampers use, with the spans registered.
RAMP_BASE, SLOPE_SET_PIECES = 0xABC1D8, 0xABC1F8
PAVED_ROADS, PAVED_ROAD_ENDS = 0xABBEC8, 0xABBEC4
EXTRA = ((RAMP_BASE, 20), (SLOPE_SET_PIECES, 10), (PAVED_ROADS, 15), (PAVED_ROAD_ENDS, 4))
TILE_GLOBALS = dict(water=0xAA0738, shore=0xABAD28, green=0xAA0E18, green_lat=0xAA0748,
                    ramp_base=RAMP_BASE, slope_set_pieces=SLOPE_SET_PIECES,
                    paved_roads=PAVED_ROADS, paved_road_ends=PAVED_ROAD_ENDS,
                    misc_pave=0xAA10A4, pave=0xABC2B0)
WATERFALLS = (0xAA073C, 0xABB110, 0xAA1050, 0xAA10A0)
# PlaceBridgeRepairHut's lookups and the Building it constructs.
HOUSE_TYPE_INDEX, HOUSE_BY_INDEX = 0x5117D0, 0x502D30
BUILDING_TYPE_INDEX, BUILDING_CTOR = 0x45E7B0, 0x43B740
BUILDING_TYPES = 0xA83C6C
STUBS = 0x4FFF0000
UNLIMBO, BUILDING_VTABLE = STUBS, STUBS + 0x200
CELL_TILE, CELL_OVERLAY, CELL_OBJECT = 0x38, 0x44, 0xE4
CELL_SUBTILE, CELL_LEVEL, CELL_SLOPE, CELL_DENSITY = 0x11A, 0x11B, 0x11C, 0x11E
DUMMY = 0xABDC50


def scene(kind):
    """Declared terrain: `(family, offset, level)` per in-diamond cell of the
    24x24 map (diamond 24..72, centre (24, 24); the first linear record (24, 1)
    seeds region 0)."""
    def at(x, y):
        if kind == 'river_x':       # a water band across x: east/west decks
            return ('water', y % 14, 0) if 23 <= x <= 25 else ('clear', 0, 0)
        if kind == 'river_y':       # across y: north/south decks
            return ('water', y % 14, 0) if 23 <= y <= 24 else ('clear', 0, 0)
        if kind == 'wide_river':    # wider than the early span limits
            return ('water', 0, 0) if 19 <= x <= 29 else ('clear', 0, 0)
        if kind == 'plateau_low_first':  # region 0 is the low ground
            return ('clear', 0, 4) if x >= 27 else ('clear', 0, 0)
        if kind == 'plateau':       # region 0 is a raised north plateau: ramps
            return ('clear', 0, 4) if y <= 24 else ('clear', 0, 0)
        if kind == 'plateau_river':
            if 31 <= y <= 32:
                return ('water', 0, 0)
            return ('clear', 0, 4) if y <= 22 else ('clear', 0, 0)
        if kind == 'trench':        # one-wide trenches between high ground: raise
            if x in (24, 33) and 14 <= y <= 38:
                return ('clear', 0, 0)
            return ('clear', 0, 4) if x >= 18 else ('clear', 0, 0)
        if kind == 'small_first':   # a small first land region dissolves
            if x + y <= 26:
                return ('clear', 0, 4)
            return ('water', 0, 0) if 23 <= x <= 25 else ('clear', 0, 0)
        if kind == 'green_river':   # green ground is the flood's water class too
            if 23 <= x <= 25:
                return ('water', 0, 0)
            return ('green', 0, 0) if x <= 14 else ('clear', 0, 0)
        raise ValueError(kind)
    return at


def controls():
    rows = [
        dict(id='river_x_seed_1', kind='river_x', seed=1),
        dict(id='river_x_seed_9_access_100', kind='river_x', seed=9, accessibility=100),
        dict(id='river_y_seed_4', kind='river_y', seed=4),
        dict(id='wide_river', kind='wide_river', seed=6),
        dict(id='plateau_low_first', kind='plateau_low_first', seed=2),
        dict(id='plateau_access_0', kind='plateau', seed=2, accessibility=0),
        dict(id='plateau_access_100', kind='plateau', seed=3, accessibility=100),
        dict(id='plateau_river', kind='plateau_river', seed=5, accessibility=60),
        dict(id='trench_raise', kind='trench', seed=7),
        dict(id='small_first_region', kind='small_first', seed=8),
        dict(id='green_river', kind='green_river', seed=10),
    ]
    return [dict(accessibility=50, gen=GEN) | row for row in rows]


def runs(values):
    """Run-length `[count, *value]` lists over the row-major diamond cells."""
    out = []
    for value in values:
        if out and out[-1][1:] == list(value):
            out[-1][0] += 1
        else:
            out.append([1, *value])
    return out


class BridgePassFixture:
    def __init__(self):
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.start_points import StartPointsFixture
        self.starts = StartPointsFixture(EXTRA, SIZE)
        self.base, self.terrain = self.starts.base, self.starts.terrain
        self.m, self.u = self.starts.m, self.starts.u
        self.tiles = {k: self.m.read32(a) for k, a in TILE_GLOBALS.items()}
        self.tiles['clear'] = 0
        self.cells = sorted((xy for xy in self.terrain.cells if self.base.in_diamond(*xy)),
                            key=lambda xy: xy[::-1])
        from tools import native_oracle as native
        native.require(set(self.cells) == set(self.terrain.cells),
                       'Admitted cells differ from the declared diamond')
        self.u.mem_map(STUBS, 0x1000)
        self.u.mem_write(STUBS, b'\xCC' * 0x100)
        self.u.mem_write(BUILDING_VTABLE + 0xD8, words(UNLIMBO))
        self.house, self.hut_type = self.m.alloc(0x10), self.m.alloc(0x10)

    def prepare(self, case):
        from tools.rmg_oracle.cliff_materialization import words
        f, m, u = self.terrain, self.m, self.u
        self.starts.scene(dict(seed=case['seed'], gen=case['gen']))
        at = scene(case['kind'])
        terrain = []
        for x, y in self.cells:
            family, offset, level = at(x, y)
            cell = f.cells[(x, y)]
            terrain.append((self.tiles[family] + offset, level))
            u.mem_write(cell + CELL_TILE, words(terrain[-1][0]))
            u.mem_write(cell + CELL_LEVEL, bytes([level]))
            u.mem_write(cell + CELL_OBJECT, words(0))
        for index in range(STRIDE * STRIDE):
            u.mem_write(f.scratch + index * 0x50 + 0x38, words(-1, -1))
        # An empty region vector; the pass deletes nothing and rebuilds all.
        items = m.alloc(LIST_SLOTS * 4)
        u.mem_write(REGION_LIST, words(REGION_LIST_VTABLE, items, LIST_SLOTS))
        u.mem_write(REGION_LIST + 0x10, words(0, 10))
        u.mem_write(REGION_LIST + 0xD, b'\0')
        u.mem_write(NEXT_REGION_ID, words(0))
        w = STRIDE
        u.mem_write(LINEAR_OFFSETS, words(-w, 1 - w, 1, w + 1, w, w - 1, -1, -1 - w))
        u.mem_write(MAPSEED + 0x6C, words(case['accessibility']))
        u.mem_write(MAPSEED + 0x30C, words(0))
        # Generated width/height: the border collectors' growth step (5A0700).
        u.mem_write(MAPSEED + 0x180, words(*case['gen']))
        # The Building type vector PlaceBridgeRepairHut indexes.
        types = m.alloc(4)
        u.mem_write(types, words(self.hut_type))
        u.mem_write(BUILDING_TYPES, words(types))
        self.items = items
        return runs(terrain)

    def cell_state(self, p):
        u = self.u
        tile, = struct.unpack('<i', u.mem_read(p + CELL_TILE, 4))
        overlay, = struct.unpack('<i', u.mem_read(p + CELL_OVERLAY, 4))
        sub, level, slope = u.mem_read(p + CELL_SUBTILE, 3)
        return [tile, sub, level, slope, overlay, u.mem_read(p + CELL_DENSITY, 1)[0],
                int(self.m.read32(p + CELL_OBJECT) != 0)]

    def run(self, case, pool):
        from tools import native_oracle as native
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.start_points import published
        from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
        f, m, u = self.terrain, self.m, self.u
        terrain = self.prepare(case)
        before = {xy: self.cell_state(f.cells[xy]) for xy in self.cells}
        dummy_before = self.cell_state(DUMMY)
        huts = []

        def back(machine, sp, cleanup, eax):
            machine.reg_write(UC_X86_REG_EAX, eax)
            machine.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)
            machine.reg_write(UC_X86_REG_EIP, m.read32(sp))

        def text(address):
            return bytes(u.mem_read(address, 16)).split(b'\0')[0].decode()

        def stub(machine, pc, sp):
            this = machine.reg_read(UC_X86_REG_ECX)
            if pc == HOUSE_TYPE_INDEX:
                native.require(text(this) == 'Neutral', 'Unexpected house type name')
                back(machine, sp, 0, 7)
            elif pc == HOUSE_BY_INDEX:
                native.require(this == 7, 'House lookup without the Neutral index')
                back(machine, sp, 0, self.house)
            elif pc == BUILDING_TYPE_INDEX:
                native.require(text(this) == 'CABHUT', 'Unexpected building type name')
                back(machine, sp, 0, 0)
            elif pc == BUILDING_CTOR:
                native.require(m.read32(sp + 4) == self.hut_type and m.read32(sp + 8) == self.house,
                               'Hut constructed without the CABHUT type and Neutral house')
                machine.mem_write(this, words(BUILDING_VTABLE))
                back(machine, sp, 8, this)
            elif pc == UNLIMBO:
                x, y, z = struct.unpack('<iii', machine.mem_read(m.read32(sp + 4), 12))
                native.require(x & 255 == 128 and y & 255 == 128 and z == 0
                               and m.read32(sp + 8) == 0, 'Unexpected Unlimbo arguments')
                cell = (x >> 8, y >> 8)
                huts.append(list(cell))
                # A 1x1 foundation: the hut occupies its own cell.
                if cell in f.cells:
                    machine.mem_write(f.cells[cell] + CELL_OBJECT, words(this))
                back(machine, sp, 8, 1)
            else:
                return False
            return True

        row = self.starts.execute(case, pool, ENTRY, 0, (), extra=stub)
        changed = []
        for xy in self.cells:
            now = self.cell_state(f.cells[xy])
            if now != before[xy]:
                changed.append([*xy, *now])
        regions = runs([struct.unpack('<i', u.mem_read(self.base.record(*xy) + 0x38, 4))
                        for xy in self.cells])
        listed = []
        for slot in range(m.read32(REGION_LIST + 0x10)):
            obj = m.read32(self.items + slot * 4)
            listed.append([struct.unpack('<i', u.mem_read(obj + off, 4))[0] for off in (8, 0xC, 0x10)]
                          + [u.mem_read(obj + 0x14, 1)[0]])
        dummy = self.cell_state(DUMMY)
        return published(row) | dict(
            terrain=terrain, changed=changed, regions=regions, listed=listed,
            next_region_id=m.read32(NEXT_REGION_ID), huts=huts,
            dummy=None if dummy == dummy_before else dummy)


def generate():
    from tools.rmg_oracle.cliff_materialization import FAMILY_GLOBALS, StatePool
    fixture, pool = BridgePassFixture(), StatePool()
    rows = [fixture.run(case, pool) for case in controls()]
    read = fixture.m.read32
    inputs = dict(family_globals={f'{p:08X}': read(p) for p in FAMILY_GLOBALS},
                  # The stamped sets only: their physical TMP footprints.
                  registered_tiles=[t for t in fixture.terrain.inputs['registered_tiles']
                                    if any(read(a) <= t['tile'] < read(a) + n for a, n in EXTRA)])
    signed = lambda v: v - (1 << 32) if v & 0x80000000 else v
    tiles = {k: signed(v) for k, v in fixture.tiles.items()}
    return dict(schema=1, inputs=inputs, scene=dict(size=SIZE, gen=GEN, tiles=tiles, cell_count=len(fixture.cells)),
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='11 whole original BridgeAndConnectorPass calls over declared terrains: east/west and north/south water bands, a band eleven cells wide, a raised plateau as the first region at accessibility 0 and 100 and as the second, a plateau with a river, one-wide trenches the pinched-cliff raise fills, a small first region that dissolves and green ground; every changed Cell tile, subtile, level, slope, overlay, density and occupation, every scratch region, the rebuilt region list, each repair hut cell and the exit RNG state',
        assumptions=[
            'Existing CliffFixture, TiberiumFieldFixture and StartPointsFixture own original-constructed Cell backing, admission (equal to the declared 24..72 diamond), the stride-49 scratch, the physical temperate clear/cliff/shore TMP registration plus RampBase (20), SlopeSetPieces (10), PavedRoads (15) and PavedRoadEnds (4) tiles, the native theater tile-set globals and the guarded arena. MapClass +F4 map size 24x24 and +FC local rect (2, 5, 22, 16).',
            'Declared scene: per in-diamond cell a clear, WaterSet or GreenTile tile and a level; no overlay, object or slope. Scratch regions and stamps start -1; the region vector starts empty. Linear neighbour offsets as PathfinderClass::ResizeMapArrays 42AC00 writes them for stride 49. MapSeed ABDFD8 +6C accessibility, +180/+184 generated size (22, 16), +30C default level 0. FPCW 0E7F. All streams seeded by original 65C6D0. Not a complete Generate prefix.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS, with guard bytes after each block that refuse a pool overrun; native code bytes unchanged. No level, tile, region or draw is supplied.',
            'PlaceBridgeRepairHut lookups: HouseTypeClass index of "Neutral" 5117D0 and HouseClass by index 502D30 return a declared house; BuildingTypeClass index of "CABHUT" 45E7B0 returns slot 0 of a declared type vector. BuildingClass constructor 43B740 writes only a vtable whose +D8 Unlimbo records the cell, occupies that one cell (+E4) and succeeds. The real constructor\'s scenario-stream draw and Unlimbo\'s own admission are not executed.'],
        entry_points=dict(bridge_and_connector_pass=ENTRY, raise_pinched=0x579010,
                          flood_build_region=0x58C800, neighbour_list=0x58F0C0,
                          carve_connectors_or_bridges=0x5905D0, try_carve_connector=0x590970,
                          place_low_bridge_deck=0x58F2C0, place_bridge_repair_hut=0x5904B0,
                          next=RNG_NEXT))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
