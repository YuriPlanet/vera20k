"""Original start-point placement over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.start_points --check` replays whole original
`RmgRegion::PlaceStartingPoints @ 0x00594870` calls (with their nested
`AreaIsPavedClear @ 0x005A7250`, `MapClass::IsCellInViewRegion @ 0x00578640`
and `RmgRegion::SelectStartCandidates @ 0x00594F40`) and whole
`RandomMapGenerator::MarkStartAreas @ 0x005A1FB0` calls; `--write` replaces
the corpus `starts::tests::native_place_starting_points_controls` and
`native_mark_start_areas_controls` consume. Original code computes every
draw, candidate, waypoint, start flag, field slot, stamp and lock. The scene
is a declared component input, not a complete Generate prefix.
"""
from pathlib import Path
import struct

PLACE, MARK = 0x594870, 0x5A1FB0
RNG_NEXT = 0x65C780
MAPSEED = 0xABDFD8  # +0x50 NumPlayers (ABE028), +0x58 TiberiumLayout (ABE030)
WAYPOINTS = 0x632   # ScenarioClass waypoint cells, written by 68BF50
WAYPOINT_SLOTS = 16
CELL_FLAGS = 0x140  # Cell flags; PlaceStartingPoints sets bit 4 at 594ABD
# Tile-set globals AreaIsPavedClear's classifiers read (4866D0/4866F0/486650/486670).
TILE_GLOBALS = dict(paved_roads=0xABBEC8, paved_road_ends=0xABBEC4,
                    misc_pave=0xAA10A4, pave=0xABC2B0)


def box(x0, y0, x1, y1):
    """Row-major cell list; the region's draw order is this list's order."""
    from tools.rmg_oracle.tiberium_field import in_diamond
    return [[x, y] for y in range(y0, y1 + 1) for x in range(x0, x1 + 1) if in_diamond(x, y)]


def place_controls():
    """Declared inputs only. `gen` is the generated size the dialog path puts
    in the local rect (2, 5, w, h); `tiles` are `[x, y, family]` overrides
    (`rough` is the CliffSet base, the others the native tile-set globals);
    `terrain` are `[x, y, level, slope]`; `regions` are scratch +38 owners."""
    from tools.rmg_oracle.tiberium_field import in_diamond
    open_box = box(12, 12, 26, 26)
    rows = [
        dict(id='one_start_open', seed=7, quota=1, cells=open_box),
        dict(id='two_starts_full_layout', seed=11, quota=2, layout=100, start_quota=4,
             cells=open_box, offset=1),
        dict(id='three_starts_rough_walls', seed=3, quota=3, layout=50, start_quota=3,
             cells=open_box, tiles=[[x, 20, 'rough'] for x in range(12, 27)]),
        # Roads and road ends refuse a window; misc pave and pave admit it.
        dict(id='paved_families', seed=42, quota=2, layout=30, cells=open_box,
             tiles=[[x, 16, 'paved_roads'] for x in range(12, 27)]
                   + [[23, y, 'paved_road_ends'] for y in range(12, 27)]
                   + [[x, y, 'misc_pave'] for x in range(15, 21) for y in range(18, 23)]
                   + [[x, y, 'pave'] for x in range(15, 21) for y in range(23, 26)]),
        # Raised and sloped cells move the view-region test.
        dict(id='raised_and_sloped_view_edges', seed=5, quota=2, cells=open_box,
             terrain=[[x, y, 2, 0] for x in range(19, 23) for y in range(19, 23)]
                     + [[x, y, 0, 3] for x in range(16, 19) for y in range(16, 20)]
                     + [[x, y, -1, 0] for x in range(22, 26) for y in range(15, 18)]),
        # Quota 0 keeps the whole selection as field slots and writes nothing.
        dict(id='quota_zero_slots_only', seed=10, quota=0, layout=60, cells=open_box),
        # Candidates owned by different scratch regions take the +20 bonus.
        dict(id='cross_region_bonus', seed=13, quota=2, layout=40, cells=open_box,
             regions=[[x, y, 1 + (x > 19)] for x, y in open_box]),
        # One admissible window: every candidate duplicates its cell.
        dict(id='single_cell_duplicates', seed=21, quota=2, offset=3,
             cells=[[20, 20], [19, 19], [20, 20], [21, 21]],
             tiles=[[x, y, 'rough'] for x, y in open_box
                    if not (17 <= x <= 22 and 17 <= y <= 22)]),
        dict(id='larger_view_more_candidates', seed=29, quota=3, layout=100, start_quota=6,
             gen=[14, 14], cells=box(8, 8, 30, 30), offset=2),
    ]
    rows = [dict(gen=[12, 12], layout=0, start_quota=2, offset=0, tiles=[], terrain=[],
                 regions=[]) | row for row in rows]
    # Scene overrides name admitted (in-diamond) cells only.
    for row in rows:
        for key in ('tiles', 'terrain', 'regions'):
            row[key] = [item for item in row[key] if in_diamond(item[0], item[1])]
    return rows


def mark_controls():
    """Declared NumPlayers (MapSeed +0x50), start waypoints and non-clear cells."""
    rows = [
        dict(id='two_open_floods', seed=1, players=2, waypoints=[[18, 18], [26, 22]]),
        dict(id='flood_against_rough_walls', seed=2, players=1, waypoints=[[22, 22]],
             rough=[[x, 19] for x in range(14, 30)] + [[19, y] for y in range(19, 30)]),
        # Overlapping floods: the stamp plane resets, the +45 locks accumulate.
        dict(id='overlapping_floods_and_diamond_edge', seed=3, players=3,
             waypoints=[[20, 20], [22, 21], [9, 10]]),
        # NumPlayers bounds the floods even when more waypoints exist.
        dict(id='players_bound_floods', seed=4, players=1, waypoints=[[24, 24], [16, 16]]),
    ]
    return [dict(rough=[]) | row for row in rows]


def published(row):
    """A stage row without its raw draw count, which no Rust test reads (the
    exit RNG state covers it)."""
    return {key: value for key, value in row.items() if key != 'draws'}


class StartPointsFixture:
    def __init__(self):
        from tools.rmg_oracle.tiberium_field import TiberiumFieldFixture
        self.base = TiberiumFieldFixture()
        self.terrain, self.m, self.u = self.base.terrain, self.base.m, self.base.u
        self.tiles = dict(rough=self.terrain.bases['cliff'],
                          **{k: self.m.read32(a) for k, a in TILE_GLOBALS.items()})

    def scenario(self):
        return self.m.read32(0xA8B230)

    def waypoints(self):
        raw = self.u.mem_read(self.scenario() + WAYPOINTS, WAYPOINT_SLOTS * 4)
        return [list(struct.unpack_from('<hh', raw, n * 4)) for n in range(WAYPOINT_SLOTS)]

    def flags(self):
        return {xy: self.m.read32(p + CELL_FLAGS) for xy, p in self.terrain.cells.items()}

    def scene(self, case, rough=()):
        from tools.rmg_oracle.cliff_materialization import words
        f, u = self.terrain, self.u
        rect = [2, 5, *case.get('gen', [12, 12])]
        self.base.prepare(dict(seed=case['seed'], rect=rect, rough=list(rough)))
        for x, y, family in case.get('tiles', []):
            u.mem_write(f.cells[(x, y)] + 0x38, words(self.tiles[family]))
        for x, y, level, slope in case.get('terrain', []):
            u.mem_write(f.cells[(x, y)] + 0x11B, bytes([level & 255, slope]))
        for x, y, region in case.get('regions', []):
            u.mem_write(self.base.record(x, y) + 0x38, words(region))
        # A distinct sentinel in every observed slot; native writes replace it.
        u.mem_write(self.scenario() + WAYPOINTS, struct.pack('<hh', -1, -1) * WAYPOINT_SLOTS)

    def run_place(self, case, pool):
        from tools import native_oracle as native
        from tools.rmg_oracle.cliff_materialization import words
        m, u = self.m, self.u
        self.scene(case)
        u.mem_write(MAPSEED + 0x50, words(case['start_quota']))
        u.mem_write(MAPSEED + 0x58, words(case['layout']))
        region = m.alloc(0x40)
        cells = m.alloc(4 * len(case['cells']))
        u.mem_write(region, bytes(0x40))
        for n, xy in enumerate(case['cells']):
            u.mem_write(cells + n * 4, struct.pack('<hh', *xy))
        u.mem_write(region + 0x20, words(case['quota']))
        u.mem_write(region + 0x2C, words(cells))
        u.mem_write(region + 0x38, words(len(case['cells'])))
        flags = self.flags()
        row = self.execute(case, pool, PLACE, region, (case['offset'],))
        slots = m.read32(region)
        field = None
        if slots:
            items, count = m.read32(slots + 4), m.read32(slots + 0x10)
            field = [list(struct.unpack('<hh', u.mem_read(items + n * 4, 4))) for n in range(count)]
        marked = [list(xy) for xy, value in sorted(self.flags().items(), key=lambda i: i[0][::-1])
                  if value != flags[xy]]
        native.require(all(self.flags()[tuple(xy)] == flags[tuple(xy)] | 4 for xy in marked),
                       'PlaceStartingPoints changed a Cell flag other than bit 4')
        return published(row) | dict(waypoints=self.waypoints(), start_flags=marked, field_slots=field)

    def run_mark(self, case, pool):
        from tools.rmg_oracle.cliff_materialization import words
        u = self.u
        self.scene(case, rough=case['rough'])
        u.mem_write(MAPSEED + 0x50, words(case['players']))
        for n, xy in enumerate(case['waypoints']):
            u.mem_write(self.scenario() + WAYPOINTS + n * 4, struct.pack('<hh', *xy))
        before = self.records()
        row = self.execute(case, pool, MARK, MAPSEED, ())
        after = self.records()
        changed = [[x, y, *after[(x, y)]] for (x, y) in sorted(after, key=lambda i: i[::-1])
                   if after[(x, y)] != before[(x, y)]]
        return row | dict(records=changed)

    def records(self):
        """Scratch +3C stamp and +45 lock per in-diamond record."""
        from tools.rmg_oracle.tiberium_field import in_diamond
        out = {}
        for (x, y) in self.terrain.cells:
            if in_diamond(x, y):
                record = self.base.record(x, y)
                out[(x, y)] = [struct.unpack('<i', self.u.mem_read(record + 0x3C, 4))[0],
                               self.u.mem_read(record + 0x45, 1)[0]]
        return out

    def execute(self, case, pool, entry, this, args, extra=None):
        """`extra(machine, pc, sp)` may serve a declared substitution and
        returns whether it did."""
        from tools import native_oracle as native
        from tools.rmg_oracle.gen_rng_vectors import check_code
        from unicorn import UC_HOOK_CODE
        from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP
        f, m, u = self.terrain, self.m, self.u
        before = f.states()
        raw = []

        def observe(machine, pc, _size, _data):
            if machine.reg_read(UC_X86_REG_EIP) != pc:
                return
            if extra and extra(machine, pc, machine.reg_read(UC_X86_REG_ESP)):
                return
            if pc == RNG_NEXT:
                native.require(machine.reg_read(UC_X86_REG_ECX) == f.rngs['mapgen'], 'Unexpected RNG stream')
            elif pc in (0x65C79D, 0x65C84B):
                raw.append(machine.reg_read(UC_X86_REG_ESI) & 0xFFFFFFFF)

        hook = u.hook_add(UC_HOOK_CODE, observe)
        f.active = True
        try:
            m.invoke(entry, this, args, instruction_limit=500_000_000,
                     timeout_us=300_000_000, context=dict(control=case['id']))
            for start, size in self.base.blocks:
                native.require(bytes(u.mem_read(start, size)) == bytes([0xA5]) * size,
                               f"{case['id']}: native pool overran its allocation")
            check_code(u, f.code)
            after = f.states()
            native.require(all(before[name] == after[name] for name in ('main', 'scenario')),
                           f"{case['id']}: unexpected main/scenario draw")
            return dict(input=case, draws=len(raw), rng_after=pool.refs(dict(after=after['mapgen']))['after'])
        finally:
            f.active = False
            u.hook_del(hook)


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = StartPointsFixture(), StatePool()
    place = [fixture.run_place(case, pool) for case in place_controls()]
    mark = [fixture.run_mark(case, pool) for case in mark_controls()]
    return dict(schema=1, scene=dict(tiles=fixture.tiles), rng_states_hex=pool.states,
                place=place, mark=mark)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='9 whole original RmgRegion::PlaceStartingPoints calls (lane and cell draws, the 6x6 AreaIsPavedClear window over clear/rough/road/road-end/misc-pave/pave tiles, the level- and slope-corrected view region, SelectStartCandidates pair seed and farthest-point growth with the cross-region bonus, waypoint writes at an offset, start flags, quota-0 and duplicate-only selections, field-slot drain) and 4 whole MarkStartAreas calls (closest-first clear floods, stamp resets, accumulated locks, walls, the diamond edge, NumPlayers bound)',
        assumptions=[
            'Existing CliffFixture and TiberiumFieldFixture own original-constructed Cell backing, admission, the stride-33 scratch, physical clear/cliff TMP registration, the native theater tile-set globals and the guarded arena; bounds 16..48, MapClass +F4 width 16. Scratch region and stamp 0, +45 lock 0 unless declared. Cells start tile 0, level 0, slope 0.',
            'Declared scene: MapClass +FC local rect (2, 5, w, h) as InitMapFromSyntheticINI 599650 writes it; tile overrides by the CliffSet base and the native PavedRoads/PavedRoadEnds/MiscPaveTile/PaveTile globals; levels, slopes and scratch +38 owners. Not a complete Generate prefix.',
            'PlaceStartingPoints rows: region record +0 field-slot vector null, +20 quota, +2C/+38 cell list; MapSeed ABDFD8 +50 start quota and +58 TiberiumLayout; stack argument the waypoint slot offset. ScenarioClass +632 waypoint slots preset to (-1, -1).',
            'MarkStartAreas rows: MapSeed +50 NumPlayers and ScenarioClass +632 waypoints. FPCW 0E7F. All streams seeded by original 65C6D0.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS, with guard bytes after each block that refuse a pool overrun; native code bytes unchanged. No candidate, distance, waypoint, stamp or RNG result is supplied.',
            'The scenario start-preview header ScenarioClass +11BC.. that PlaceStartingPoints also writes is not observed.'],
        entry_points=dict(place_starting_points=PLACE, select_start_candidates=0x594F40,
                          area_is_paved_clear=0x5A7250, is_cell_in_view_region=0x578640,
                          mark_start_areas=MARK, next=RNG_NEXT))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
