"""Original tiberium-field placements over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.tiberium_field --check` replays whole original
`RandomMapGenerator::PlaceTiberiumField @ 0x005A28C0` calls and whole
`RandomMapGenerator::CreateTiberium @ 0x005A23A0` driver calls; `--write`
replaces the corpus that `tiberium::tests::native_place_field_controls` and
`native_create_tiberium_controls` consume. Original code computes every
overlay, density, stamp, TIBTRE name, Gaussian and RNG draw. The supplied scene
is a declared component input, not a complete Generate prefix.
"""
from pathlib import Path
import struct

ENTRY, DRIVER = 0x5A28C0, 0x5A23A0
RNG_NEXT, GAUSSIAN = 0x65C780, 0x5980C0
GAUSS_STATE, UNIFORM_SOURCE = 0xABDFB8, 0x598000
REGIONS, REGION_COUNT = 0xABDF94, 0xABDFA0
WAYPOINTS = 0x632  # ScenarioClass waypoint cells, read by 68BCC0
TERRAIN_FIND, TERRAIN_CTOR = 0x71DD80, 0x71BB90
# TerrainClass::Unlimbo @ 0x0071D000's resource clear, from its first cell
# lookup to the epilogue.
UNLIMBO_CLEAR, UNLIMBO_DONE = 0x71D0E2, 0x71D132
MAP = 0x87F7E8
TERRAIN_TYPES = 0xA8E31C  # g_TerrainTypeClass_Array items
OVERLAY_TYPES = 0xA83D84  # g_OverlayTypeClass_Array items
TIBERIUMS, TIBERIUM_COUNT = 0xB0F4EC, 0xB0F4F8
STRIDE, LOW, HIGH = 33, 16, 48
ARENA, ARENA_SIZE = 0x50000000, 0x01000000
GUARD, GUARD_BYTE = 0x40, 0xA5
FULL = [0, 0, 16, 16]
CENTER = [16, 16]


def in_diamond(x, y):
    return LOW < x + y and x - y < LOW and y - x < LOW and x + y <= HIGH


def square(x0, y0, x1, y1):
    return [[x, y] for y in range(y0, y1 + 1) for x in range(x0, x1 + 1)]


def ring(x0, y0, x1, y1):
    return [xy for xy in square(x0, y0, x1, y1)
            if xy[0] in (x0, x1) or xy[1] in (y0, y1)]


def controls():
    """Declared inputs only. `rough` cells hold a non-clear tile, `locked`
    records the scratch +45 flag, `ore` cells a prior `[overlay, density]`;
    overlay 0 is not tiberium. Sizes 1 and 2 put the pool and heap at 10 and
    20 nodes."""
    pocket = ring(14, 14, 18, 18)
    rows = [
        dict(id='ore_plain', seed=7, size=40),
        dict(id='gem_plain', seed=11, size=40, gem=1),
        dict(id='ore_size_one', seed=3, size=1),
        dict(id='gem_size_two', seed=42, size=2, gem=1),
        # A walled 3x3 pocket saturates; each re-seed then lands on the origin.
        dict(id='ore_saturated_pocket_reseeds', seed=5, size=150, rough=pocket),
        dict(id='gem_saturated_pocket_reseeds', seed=5, size=150, gem=1, rough=pocket),
        # A locked origin: the anchor rebinds to the first unlocked pop.
        dict(id='ore_locked_origin', seed=9, size=30, locked=square(15, 15, 17, 17)),
        dict(id='ore_over_prior_resources', seed=13, size=25,
             ore=[[16, 16, 105, 4], [17, 16, 110, 11], [15, 16, 102, 10],
                  [16, 17, 0, 0], [16, 15, 30, 2]]),
        dict(id='ore_clipped_rect', seed=21, size=60, rect=[3, 2, 9, 11], origin=[14, 13]),
    ]
    return [dict(origin=CENTER, field=5, gem=0, rect=FULL, rough=[], locked=[], ore=[])
            | row for row in rows]


def driver_controls():
    """Declared generator inputs: map type (+3C), Resources (+40), the
    Tiberium option (+54), RMGMinimum/MaximumTiberium (+2BC/+2C0), start
    waypoints and regions (`slots` None is a region with no field list)."""
    two_starts = dict(quota=2, slots=[[14, 14], [18, 16], [16, 19]], cells=[[16, 16]])
    pocket = dict(quota=0, slots=[[22, 21], [24, 19]],
                  cells=[[20, 20], [21, 21], [22, 22], [23, 23], [24, 20]])
    rows = [
        # Pass-1 gem anchor from the start mean and from a random region cell.
        dict(id='water_anchor_from_starts_and_cells', seed=7, map_type=1, resources=0,
             regions=[two_starts, pocket]),
        # Land with Resources 3: anchor gem; skipped regions keep the start base.
        dict(id='land_gem_anchor_after_skipped_regions', seed=11, map_type=0, resources=3,
             regions=[dict(quota=1, slots=None, cells=[[16, 16]]),
                      dict(quota=1, slots=[], cells=[[16, 16]]),
                      dict(quota=1, slots=[[13, 18], [19, 19]], cells=[[16, 16]])]),
        # Island with Resources 3: no anchor, gem second pass.
        dict(id='island_gem_second_pass', seed=3, map_type=3, resources=3,
             regions=[two_starts]),
        # A zero region total after the random anchor draw.
        dict(id='zero_total_after_anchor_draw', seed=42, map_type=2, resources=0,
             tib_option=0, min_tib=1, max_tib=1, regions=[pocket]),
    ]
    return [dict(tib_option=50, min_tib=20, max_tib=60, waypoints=[[15, 17], [19, 15]])
            | row for row in rows]


class TiberiumFieldFixture:
    def __init__(self):
        from tools.rmg_oracle.cliff_materialization import CliffFixture, words
        from tools.spatial_oracle.harvest_field import IMAGES, OVERLAY_COUNT
        self.terrain = CliffFixture()
        m, u = self.m, self.u = self.terrain.m, self.terrain.u
        self.coord, self.origin_cell = m.alloc(0x10), m.alloc(4)
        # Prepared retail overlay/Tiberium registries (the harvest_field
        # ordinals): GetTiberiumIndex 5FDD20 and Unlimbo's clear read them.
        overlays = m.alloc(OVERLAY_COUNT * 0x300)
        items = m.alloc(OVERLAY_COUNT * 4)
        for index in range(OVERLAY_COUNT):
            entry = overlays + index * 0x300
            u.mem_write(items + index * 4, words(entry))
            u.mem_write(entry + 0x294, words(index))
            tiberium = any(base <= index < base + main + extra for base, main, extra in IMAGES.values())
            u.mem_write(entry + 0x2A9, bytes([tiberium]))
        u.mem_write(OVERLAY_TYPES, words(items))
        tiberiums = m.alloc(len(IMAGES) * 0x200)
        tib_items = m.alloc(len(IMAGES) * 4)
        for index, (base, main, extra) in sorted(IMAGES.items()):
            tib = tiberiums + index * 0x200
            u.mem_write(tib_items + index * 4, words(tib))
            u.mem_write(tib + 0x98, words(index))
            u.mem_write(tib + 0xE0, words(overlays + base * 0x300))
            u.mem_write(tib + 0xE8, words(main, extra))
        u.mem_write(TIBERIUMS, words(tib_items))
        u.mem_write(TIBERIUM_COUNT, words(len(IMAGES)))
        # One readable TerrainType slot; only the hooked constructor sees it.
        u.mem_write(TERRAIN_TYPES, words(m.alloc(4)))
        u.mem_map(ARENA, ARENA_SIZE)
        self.cursor, self.blocks = ARENA, []
        m.alloc = self.alloc

    def alloc(self, size):
        # Guard bytes after each block refuse a native pool overrun.
        out = self.cursor
        self.u.mem_write(out + size, bytes([GUARD_BYTE]) * GUARD)
        self.blocks.append((out + size, GUARD))
        self.cursor += (size + GUARD + 15) & ~15
        assert self.cursor < ARENA + ARENA_SIZE, 'tiberium arena exhausted'
        return out

    def record(self, x, y):
        return self.terrain.scratch + (y * STRIDE + x) * 0x50

    def prepare(self, case):
        from tools.rmg_oracle.cliff_materialization import words
        case = dict(rect=FULL, rough=[], locked=[], ore=[], origin=CENTER) | case
        f, u = self.terrain, self.u
        f.reset(dict(kind='tiberium', seed=case['seed']))
        u.mem_write(ARENA, bytes(self.cursor - ARENA))
        self.cursor, self.blocks = ARENA, []
        for index in range(STRIDE * STRIDE):
            x, y = index % STRIDE, index // STRIDE
            coord = (x, y) if in_diamond(x, y) else (0, 0)
            u.mem_write(f.scratch + index * 0x50, struct.pack('<hh', *coord))
            u.mem_write(f.scratch + index * 0x50 + 0x38, words(0, 0, -1))
            u.mem_write(f.scratch + index * 0x50 + 0x45, b'\0')
        for cell in f.cells.values():
            u.mem_write(cell + 0x38, words(0))
            u.mem_write(cell + 0x44, words(-1))
            u.mem_write(cell + 0x11A, bytes(3))
            u.mem_write(cell + 0x11E, b'\0')
        for x, y in case['rough']:
            u.mem_write(f.cells[(x, y)] + 0x38, words(f.bases['cliff']))
        for x, y in case['locked']:
            u.mem_write(self.record(x, y) + 0x45, b'\1')
        for x, y, overlay, density in case['ore']:
            u.mem_write(f.cells[(x, y)] + 0x44, words(overlay))
            u.mem_write(f.cells[(x, y)] + 0x11E, bytes([density]))
        # MapClass +F4 width and +FC local rect that Is_Cell_In_Playfield 578460 reads.
        u.mem_write(MAP + 0xF4, words(16))
        u.mem_write(MAP + 0xFC, words(*case['rect']))
        u.mem_write(self.origin_cell, struct.pack('<hh', *case['origin']))
        self.prior = self.fields()

    def prepare_driver(self, case):
        from tools.rmg_oracle.cliff_materialization import words
        m, u = self.m, self.u
        self.prepare(case)
        actor = m.alloc(0x400)
        u.mem_write(actor, bytes(0x400))
        for offset, key in ((0x3C, 'map_type'), (0x40, 'resources'), (0x54, 'tib_option'),
                            (0x2BC, 'min_tib'), (0x2C0, 'max_tib')):
            u.mem_write(actor + offset, words(case[key]))
        scenario = m.read32(0xA8B230)
        for index, xy in enumerate(case['waypoints']):
            u.mem_write(scenario + WAYPOINTS + index * 4, struct.pack('<hh', *xy))
        items = m.alloc(4 * len(case['regions']))
        for index, region in enumerate(case['regions']):
            record = m.alloc(0x40)
            u.mem_write(record, bytes(0x40))
            if region['slots'] is not None:
                slots = m.alloc(0x20)
                cells = m.alloc(4 * max(1, len(region['slots'])))
                u.mem_write(slots, bytes(0x20))
                u.mem_write(slots + 0x4, words(cells))
                u.mem_write(slots + 0x10, words(len(region['slots'])))
                for n, xy in enumerate(region['slots']):
                    u.mem_write(cells + n * 4, struct.pack('<hh', *xy))
                u.mem_write(record, words(slots))
            cells = m.alloc(4 * len(region['cells']))
            for n, xy in enumerate(region['cells']):
                u.mem_write(cells + n * 4, struct.pack('<hh', *xy))
            u.mem_write(record + 0x20, words(region['quota']))
            u.mem_write(record + 0x2C, words(cells))
            u.mem_write(record + 0x38, words(len(region['cells'])))
            u.mem_write(items + index * 4, words(record))
        u.mem_write(REGIONS, words(items))
        u.mem_write(REGION_COUNT, words(len(case['regions'])))
        u.mem_write(GAUSS_STATE, bytes(0x10) + words(UNIFORM_SOURCE))
        return actor

    def fields(self):
        f, u = self.terrain, self.u
        out = {}
        for (x, y), cell in f.cells.items():
            if in_diamond(x, y):
                overlay = struct.unpack('<i', u.mem_read(cell + 0x44, 4))[0]
                stamp = struct.unpack('<i', u.mem_read(self.record(x, y) + 0x3C, 4))[0]
                out[(x, y)] = [overlay, u.mem_read(cell + 0x11E, 1)[0], stamp]
        return out

    def observe(self):
        # Only cells whose overlay, density or stamp changed, row-major.
        return [[x, y] + value
                for (x, y), value in sorted(self.fields().items(), key=lambda i: i[0][::-1])
                if value != self.prior[(x, y)]]

    def run(self, case, pool):
        self.prepare(case)
        args = (self.origin_cell, case['size'], case['field'], case['gem'])
        return self.execute(case, pool, ENTRY, 0, args)

    def run_driver(self, case, pool):
        actor = self.prepare_driver(case)
        row = self.execute(case, pool, DRIVER, actor, ())
        flag = self.u.mem_read(GAUSS_STATE, 1)[0]
        cached = struct.unpack('<Q', self.u.mem_read(GAUSS_STATE + 8, 8))[0]
        return row | dict(gauss_cached=flag, gauss_value_bits=cached if flag else 0)

    def execute(self, case, pool, entry, this, args):
        from tools import native_oracle as native
        from tools.rmg_oracle.gen_rng_vectors import check_code
        from tools.rmg_oracle.cliff_materialization import words
        from unicorn import UC_HOOK_CODE
        from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX,
                                      UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_EIP,
                                      UC_X86_REG_ESI, UC_X86_REG_ESP)
        f, m, u = self.terrain, self.m, self.u
        before = f.states()
        trace = native.NativeCallTrace(u, m.read32)
        trees, names, saved = [], [], {}

        def string(address):
            return bytes(u.mem_read(address, 16)).split(b'\0')[0].decode('ascii')

        def observe(machine, pc, _size, _data):
            if machine.reg_read(UC_X86_REG_EIP) != pc:
                return
            sp = machine.reg_read(UC_X86_REG_ESP)
            trace.returned(pc, sp)
            if pc == RNG_NEXT:
                assert machine.reg_read(UC_X86_REG_ECX) == f.rngs['mapgen']
                trace.entered(pc, sp, ('next', 0, 0))
            elif pc == GAUSSIAN:
                assert machine.reg_read(UC_X86_REG_ECX) == GAUSS_STATE
                trace.entered(pc, sp, ('gaussian', 0, 0))
            elif pc == TERRAIN_FIND:
                # Record the original-formatted name; the index only selects
                # the type the hooked constructor receives.
                names.append(string(machine.reg_read(UC_X86_REG_ECX)))
                machine.reg_write(UC_X86_REG_EAX, 0)
                machine.reg_write(UC_X86_REG_ESP, sp + 4)
                machine.reg_write(UC_X86_REG_EIP, m.read32(sp))
            elif pc == TERRAIN_CTOR:
                cell = struct.unpack('<hh', machine.mem_read(m.read32(sp + 8), 4))
                trees.append([names.pop(), *cell])
                saved.update(sp=sp, this=machine.reg_read(UC_X86_REG_ECX),
                             regs={r: machine.reg_read(r) for r in
                                   (UC_X86_REG_EBX, UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_EBP)})
                # Unlimbo's centre coordinate, then its original clear.
                machine.mem_write(self.coord, words(cell[0] * 256 + 128, cell[1] * 256 + 128, 0))
                machine.mem_write(sp - 4, words(self.coord))
                machine.reg_write(UC_X86_REG_EBP, self.coord)
                machine.reg_write(UC_X86_REG_ECX, MAP)
                machine.reg_write(UC_X86_REG_ESP, sp - 4)
                machine.reg_write(UC_X86_REG_EIP, UNLIMBO_CLEAR)
            elif pc == UNLIMBO_DONE and saved:
                assert sp == saved['sp'], 'Unlimbo clear left the stack unbalanced'
                for register, value in saved['regs'].items():
                    machine.reg_write(register, value)
                machine.reg_write(UC_X86_REG_EAX, saved['this'])
                machine.reg_write(UC_X86_REG_ESP, sp + 12)
                machine.reg_write(UC_X86_REG_EIP, m.read32(sp))
                saved.clear()

        hook = u.hook_add(UC_HOOK_CODE, observe)
        f.active = True
        try:
            m.invoke(entry, this, args, instruction_limit=2_000_000_000,
                     timeout_us=300_000_000, context=dict(control=case['id']))
            trace.returned(native.RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
            assert not trace.pending and not names and not saved
            for start, size in self.blocks:
                assert bytes(u.mem_read(start, size)) == bytes([GUARD_BYTE]) * size, \
                    f"{case['id']}: native pool overran its allocation"
            check_code(u, f.code)
            after = f.states()
            assert all(before[name] == after[name] for name in ('main', 'scenario'))
            exit_state = pool.refs(dict(after=after['mapgen']))['after']
            return dict(input=case, rng_after=exit_state, trees=trees, cells=self.observe())
        finally:
            f.active = False
            u.hook_del(hook)


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = TiberiumFieldFixture(), StatePool()
    rows = [fixture.run(case, pool) for case in controls()]
    drivers = [fixture.run_driver(case, pool) for case in driver_controls()]
    return dict(schema=1, scene=dict(cliff_base=fixture.terrain.bases['cliff']),
                rng_states_hex=pool.states, rows=rows, drivers=drivers)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='9 whole original PlaceTiberiumField calls (ore and gem growth, 1- and 2-cell fields at the 10x node pool, saturated walled pockets that re-seed up to the cap, a locked origin, prior ore/gem/foreign overlays, a clipped playfield) and 4 whole CreateTiberium calls (start-mean and random-cell gem anchors, skipped regions, gem second pass, a zero region total), with the heap, Gaussian, TIBTRE draws and names, overlay and density writes, stamps and Unlimbo\'s resource clear',
        assumptions=[
            'Existing CliffFixture owns original-constructed Cell backing, admission, the stride-33 scratch, the physical clear/cliff TMP registration and the 49F2F0 direction table; bounds 16..48. Scratch coordinates stamped for in-diamond records only; region and stamp start 0, mask cache -1, +45 lock 0 unless declared. Cells start tile 0, overlay -1, density 0.',
            'Declared scene: origin, size, field id, gem flag, non-clear (CliffSet base) cells, locked records and prior overlay/density pairs. MapClass +F4 width 16 and +FC local rect per row. Not a complete Generate prefix.',
            'Driver rows: generator +3C map type, +40 Resources, +54 Tiberium option, +2BC/+2C0 minimum/maximum; region vector ABDF94/ABDFA0 records with +0 field-slot vector (or null), +20 start quota, +2C/+38 cell list; ScenarioClass +632 start waypoints read by 68BCC0. Gaussian cache empty with source 598000.',
            'Prepared retail overlay/Tiberium registries as harvest_field declares them (Riparius 102 +12/+8, Cruentus 27 +12, Vinifera 127 and Aboreus 147 +12/+8); OverlayType +2A9 Tiberium set on those ordinals only. FPCW 0E7F. All streams seeded by original 65C6D0.',
            'ObjectClass::Unlimbo 5F4EC0 admits each TIBTRE: Generate raises ScenarioInit A8E7AC at 598A9A and lowers it at 59934A around CreateTiberium (598EF4), so Can_Enter_Cell is skipped at 5F4F1B; Main__PrepareSession sets GameActive at 52D9D7; a new object is in limbo.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS, with guard bytes after each block that refuse a pool overrun; native code bytes unchanged. No overlay, density, stamp, Gaussian or RNG result is supplied.',
            'TerrainTypeClass::Find_By_Name_Index 71DD80 records its original-formatted name and returns slot 0. The TerrainClass constructor 71BB90 is entered only to record its cell; original TerrainClass::Unlimbo 71D0E2..71D132 then runs on the cell centre, and the constructor returns. Object construction, ID registry, Mark/Place_Down, neighbour counters, render point and TerrainClass::Array are not executed.'],
        entry_points=dict(place_tiberium_field=ENTRY, create_tiberium=DRIVER, next=RNG_NEXT,
                          gaussian=GAUSSIAN, terrain_find=TERRAIN_FIND,
                          terrain_ctor=TERRAIN_CTOR, unlimbo_clear=UNLIMBO_CLEAR))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
