"""Original neutral tech-building placement over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.tech_buildings --check` replays whole original
`RandomMapGenerator::AddTechBuildings @ 0x005A95B0` calls: the map-type-2
passes through `PlaceTechBuildingInRegion @ 0x00595400` and the whole-map
path of the other types. `--write` replaces the corpus
`tech_buildings::tests::native_add_tech_buildings_controls` consumes.
Original code computes every pass, count, type and anchor draw, the 200-draw
clear-anchor reject, the 100-attempt cap and the foundation gate. The scene
and the declared foundations are component inputs, not a Generate prefix.
"""
from pathlib import Path
import struct

ENTRY = 0x5A95B0
REGION_PLACE = 0x595400
RNG_NEXT = 0x65C780
BUILDING_CTOR = 0x43B740   # BuildingClass::BuildingClass(type, house), ret 8
RULES = 0x8871E0           # RulesClass*; +AE0 NeutralTechBuildings items, +AEC count
NEUTRAL_ITEMS, NEUTRAL_COUNT = 0xAE0, 0xAEC
BUILDING_TYPE = 0x520      # BuildingClass type pointer
CELL_OBJECT, CELL_LAND = 0xE4, 0xEC
STUBS = 0x4FFF0000         # host stub page: vtable targets only
FOUNDATION, UNLIMBO, DESTROY = STUBS, STUBS + 0x10, STUBS + 0x20
TYPE_VTABLE, BUILDING_VTABLE = STUBS + 0x100, STUBS + 0x200
END = (0x7FFF, 0x7FFF)


def rect(w, h):
    return [[x, y] for y in range(h) for x in range(w)]


def controls():
    """Declared inputs only. `types` are foundations (cell offsets) in
    NeutralTechBuildings order; `rough` cells hold a non-clear tile,
    `occupied` a prior object, `rock` land type 3, `locked` the scratch +45
    flag; `terrain` is `[x, y, level, slope]`; `regions` are region records
    `[quota, cells]` in vector order."""
    from tools.rmg_oracle.start_points import box
    from tools.rmg_oracle.tiberium_field import in_diamond
    three = [rect(2, 2), rect(1, 1), rect(3, 2)]
    walls = [[x, 16] for x in range(8, 26)] + [[16, y] for y in range(8, 26)]
    open_box = box(12, 12, 20, 20)
    rows = [
        dict(id='whole_map_open', seed=7, map_type=1, types=three),
        dict(id='whole_map_island_walls_and_objects', seed=11, map_type=3, types=three,
             rough=walls, occupied=[[x, y] for x in range(18, 22) for y in range(13, 17)],
             locked=box(12, 18, 15, 22)),
        # A raised plateau, sloped edge and Rock land split foundations.
        dict(id='whole_map_levels_and_rock', seed=5, map_type=4, types=[rect(3, 3), rect(2, 1)],
             terrain=[[x, y, 2, 0] for x in range(15, 20) for y in range(14, 19)]
                     + [[x, 13, 1, 1] for x in range(15, 20)],
             rock=[[x, y] for x in range(19, 23) for y in range(11, 16)]),
        # Two isolated clear cells: every 2x2 fails, the anchor draw often
        # hits the 200 cap, and the building is destroyed after 100 attempts.
        dict(id='whole_map_exhausted_and_destroyed', seed=4, map_type=1, types=[rect(2, 2)],
             rough=[[x, y] for x in range(33) for y in range(33)
                    if in_diamond(x, y) and (x, y) not in ((16, 16), (20, 14))]),
        # A clipped playfield rejects foundations near the view edge.
        dict(id='whole_map_clipped_playfield', seed=1, map_type=1, types=three, gen=[6, 3]),
        dict(id='regions_open', seed=3, map_type=2, types=three,
             regions=[[1, open_box], [0, box(12, 12, 14, 14)], [2, box(15, 15, 19, 19)]]),
        # A region with one admissible 2x2; the second building is destroyed.
        dict(id='regions_exhausted', seed=1, map_type=2, types=[rect(2, 2)],
             rough=[[x, y] for x, y in open_box if not (16 <= x <= 17 and 16 <= y <= 17)],
             regions=[[1, open_box], [1, box(15, 15, 18, 18)]]),
        dict(id='regions_walls_and_locks', seed=21, map_type=2, types=three,
             rough=walls, locked=box(12, 12, 14, 14),
             regions=[[1, box(10, 10, 15, 15)], [1, box(17, 17, 22, 22)]]),
    ]
    # The playfield (2, 5, w, h) lies inside the 16x16 map only while h <= 4.
    rows = [dict(gen=[12, 4], rough=[], occupied=[], rock=[], locked=[], terrain=[],
                 regions=[]) | row for row in rows]
    for row in rows:
        for key in ('rough', 'occupied', 'rock', 'locked', 'terrain'):
            row[key] = [item for item in row[key] if in_diamond(item[0], item[1])]
    return rows


class TechBuildingsFixture:
    def __init__(self):
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.start_points import StartPointsFixture
        self.starts = StartPointsFixture()
        self.base, self.terrain = self.starts.base, self.starts.terrain
        self.m, self.u = self.starts.m, self.starts.u
        self.u.mem_map(STUBS, 0x1000)
        self.u.mem_write(STUBS, b'\xCC' * 0x100)
        self.u.mem_write(TYPE_VTABLE + 0x90, words(FOUNDATION))
        self.u.mem_write(BUILDING_VTABLE + 0x20, words(DESTROY))
        self.u.mem_write(BUILDING_VTABLE + 0xD8, words(UNLIMBO))

    def prepare(self, case):
        from tools.rmg_oracle.cliff_materialization import words
        f, m, u = self.terrain, self.m, self.u
        self.starts.scene(dict(seed=case['seed'], gen=case['gen'], terrain=case['terrain']),
                          rough=case['rough'])
        for p in f.cells.values():
            u.mem_write(p + CELL_OBJECT, words(0))
            u.mem_write(p + CELL_LAND, words(0))
        self.prior_object = m.alloc(4)
        for x, y in case['occupied']:
            u.mem_write(f.cells[(x, y)] + CELL_OBJECT, words(self.prior_object))
        for x, y in case['rock']:
            u.mem_write(f.cells[(x, y)] + CELL_LAND, words(3))
        for x, y in case['locked']:
            u.mem_write(self.base.record(x, y) + 0x45, b'\1')
        # One type object per foundation; its vtable +90 is the foundation stub.
        self.types, self.foundations = [], {}
        items = m.alloc(4 * len(case['types']))
        for index, cells in enumerate(case['types']):
            kind = m.alloc(0x10)
            data = m.alloc(4 * (len(cells) + 1))
            u.mem_write(kind, words(TYPE_VTABLE))
            u.mem_write(data, b''.join(struct.pack('<hh', *xy) for xy in cells + [END]))
            u.mem_write(items + index * 4, words(kind))
            self.types.append(kind)
            self.foundations[kind] = (data, cells)
        # The VM holds no RulesClass; only the read vector is supplied.
        rules = m.alloc(NEUTRAL_COUNT + 4)
        u.mem_write(rules, bytes(NEUTRAL_COUNT + 4))
        u.mem_write(RULES, words(rules))
        u.mem_write(rules + NEUTRAL_ITEMS, words(items))
        u.mem_write(rules + NEUTRAL_COUNT, words(len(case['types'])))
        records = m.alloc(4 * max(1, len(case['regions'])))
        for index, (quota, cells) in enumerate(case['regions']):
            record, items = m.alloc(0x40), m.alloc(4 * len(cells))
            u.mem_write(record, bytes(0x40))
            for n, xy in enumerate(cells):
                u.mem_write(items + n * 4, struct.pack('<hh', *xy))
            u.mem_write(record + 0x20, words(quota))
            u.mem_write(record + 0x2C, words(items))
            u.mem_write(record + 0x38, words(len(cells)))
            u.mem_write(records + index * 4, words(record))
        u.mem_write(0xABDF94, words(records))
        u.mem_write(0xABDFA0, words(len(case['regions'])))
        actor = m.alloc(0x40)
        u.mem_write(actor, bytes(0x40))
        u.mem_write(actor + 0x3C, words(case['map_type']))
        return actor

    def run(self, case, pool):
        from tools import native_oracle as native
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.tiberium_field import in_diamond
        from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
        actor = self.prepare(case)
        buildings, live = [], {}

        def back(machine, sp, cleanup, eax):
            machine.reg_write(UC_X86_REG_EAX, eax)
            machine.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)
            machine.reg_write(UC_X86_REG_EIP, self.m.read32(sp))

        def stub(machine, pc, sp):
            this = machine.reg_read(UC_X86_REG_ECX)
            if pc == BUILDING_CTOR:
                kind = self.m.read32(sp + 4)
                native.require(kind in self.types, 'Constructor received an undeclared type')
                machine.mem_write(this, words(BUILDING_VTABLE))
                machine.mem_write(this + BUILDING_TYPE, words(kind))
                live[this] = len(buildings)
                buildings.append([self.types.index(kind), -1, -1])
                back(machine, sp, 8, this)
            elif pc == FOUNDATION:
                native.require(self.m.read32(sp + 4) == 1, 'Foundation read without its bib flag')
                back(machine, sp, 4, self.foundations[this][0])
            elif pc == UNLIMBO:
                x, y, z = struct.unpack('<iii', machine.mem_read(self.m.read32(sp + 4), 12))
                native.require(x & 255 == 128 and y & 255 == 128 and z == 0
                               and self.m.read32(sp + 8) == 0, 'Unexpected Unlimbo arguments')
                cell = (x >> 8, y >> 8)
                record = buildings[live[this]]
                record[1:] = cell
                # Admit and occupy the foundation the gate passed.
                kind = self.m.read32(this + BUILDING_TYPE)
                for dx, dy in self.foundations[kind][1]:
                    xy = (cell[0] + dx, cell[1] + dy)
                    native.require(xy in self.terrain.cells and in_diamond(*xy),
                                   f'Unlimbo foundation cell {xy} outside the fixture map')
                    machine.mem_write(self.terrain.cells[xy] + CELL_OBJECT, words(this))
                back(machine, sp, 8, 1)
            elif pc == DESTROY:
                native.require(self.m.read32(sp + 4) == 1 and buildings[live.pop(this)][1] == -1,
                               'Destroyed a placed building')
                back(machine, sp, 4, 0)
            else:
                return False
            return True

        row = self.starts.execute(case, pool, ENTRY, actor, (), extra=stub)
        placed = [b for b in buildings if b[1] != -1]
        native.require(len(live) == len(placed), 'A discarded building was not destroyed')
        return row | dict(buildings=buildings)


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = TechBuildingsFixture(), StatePool()
    rows = [fixture.run(case, pool) for case in controls()]
    return dict(schema=1, scene=dict(rough=fixture.terrain.bases['cliff']),
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='8 whole original AddTechBuildings calls: 5 whole-map rows (map types 1, 3 and 4; walls, prior objects, start locks, raised and sloped cells, Rock land, a clipped playfield, the 200-draw anchor cap with a destroyed building) and 3 map-type-2 rows through PlaceTechBuildingInRegion (quota-0 regions skipped, a region exhausted to its 100-attempt cap, walls and locks); every pass, count, type and anchor draw and each building\'s cell',
        assumptions=[
            'Existing CliffFixture, TiberiumFieldFixture and StartPointsFixture own original-constructed Cell backing, admission, the stride-33 scratch, the physical clear/cliff TMP registration and the guarded arena; bounds 16..48, MapClass +F4 width 16, +FC local rect (2, 5, w, h) with h <= 4, so the playfield lies inside the map diamond as on generated maps. Scratch region and stamp 0, +45 lock 0 unless declared. Cells start tile 0, level 0, slope 0, no object (+E4), land type Clear (+EC).',
            'Declared scene: CliffSet-base (non-clear) cells, a prior object pointer in +E4, land type 3 in +EC, +45 locks, levels and slopes. Land types are declared, not computed by RecalcAttributes 47D2B0. Not a complete Generate prefix.',
            'MapSeed +3C map type; region vector ABDF94/ABDFA0 records with +20 start quota and +2C/+38 cell list. A zeroed RulesClass block whose +AE0/+AEC NeutralTechBuildings holds one declared type object per row foundation. FPCW 0E7F. All streams seeded by original 65C6D0.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS, with guard bytes after each block that refuse a pool overrun; native code bytes unchanged. No draw, anchor or gate result is supplied.',
            'BuildingClass constructor 43B740 records its type and writes only the vtable and +520 type. The type vtable +90 (GetFoundationData 45EC20) returns the declared foundation. Building vtable +D8 Unlimbo always succeeds, recording the cell and occupying the foundation cells\' +E4; vtable +20 records the destroy. BuildingClass::Unlimbo\'s own admission and side effects are not executed.'],
        entry_points=dict(add_tech_buildings=ENTRY, place_tech_building_in_region=REGION_PLACE,
                          building_ctor=BUILDING_CTOR, next=RNG_NEXT))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
