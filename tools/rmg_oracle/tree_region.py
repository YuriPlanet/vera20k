"""Original tree regions over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.tree_region --check` replays whole original
`RandomMapGenerator::PlaceTreeRegion @ 0x005A45E0` calls; `--write` replaces
the corpus `trees::tests::native_place_tree_region_controls` consumes.
Original code computes every density, variant and jitter draw, the
closest-first walk, the visited marks and each tree's name and cell. The
scene is a declared component input, not a complete Generate prefix.
"""
from pathlib import Path
import struct

ENTRY = 0x5A45E0
TERRAIN_FIND, TERRAIN_CTOR = 0x71DD80, 0x71BB90
CELL_OBJECT, CELL_OVERLAY, CELL_LAND = 0xE4, 0x44, 0xEC


def controls():
    """Declared inputs only. `rough` cells hold a non-clear tile, `occupied`
    a prior object, `overlay` a prior overlay, `rock` land type 3, `locked`
    the scratch +45 flag, `visited` a prior +47 mark."""
    from tools.rmg_oracle.start_points import box
    from tools.rmg_oracle.tiberium_field import in_diamond
    walls = [[x, 19] for x in range(10, 26)] + [[19, y] for y in range(10, 19)]
    rows = [
        dict(id='open_dense', seed=1, size=20, density=0.4),
        dict(id='open_sparse', seed=2, size=35, density=0.05),
        dict(id='obstacles', seed=3, size=25, density=0.3, rough=walls,
             occupied=box(13, 13, 14, 14), overlay=box(17, 13, 18, 15), rock=box(13, 17, 15, 18)),
        # A locked ring admits no neighbour: the origin alone is walked.
        dict(id='locked_ring', seed=4, size=10, density=0.4,
             locked=[xy for xy in box(15, 15, 17, 17) if xy != [16, 16]]),
        # Low density: the 25 * size node budget ends the walk first.
        dict(id='budget_bound', seed=5, size=10, density=0.05),
        dict(id='diamond_edge_origin', seed=6, size=15, density=0.3, origin=[24, 24]),
        # Cells an earlier region walked stay closed.
        dict(id='prior_visited', seed=7, size=20, density=0.3, visited=box(12, 12, 20, 15)),
    ]
    rows = [dict(origin=[16, 16], rough=[], occupied=[], overlay=[], rock=[], locked=[],
                 visited=[]) | row for row in rows]
    for row in rows:
        for key in ('rough', 'occupied', 'overlay', 'rock', 'locked', 'visited'):
            row[key] = [xy for xy in row[key] if in_diamond(*xy)]
    return rows


class TreeRegionFixture:
    def __init__(self):
        from tools.rmg_oracle.start_points import StartPointsFixture
        self.starts = StartPointsFixture()
        self.base, self.terrain = self.starts.base, self.starts.terrain
        self.m, self.u = self.starts.m, self.starts.u

    def diamond(self):
        from tools.rmg_oracle.tiberium_field import in_diamond, STRIDE
        return [(x, y) for y in range(STRIDE) for x in range(STRIDE) if in_diamond(x, y)]

    def run(self, case, pool):
        from tools import native_oracle as native
        from tools.rmg_oracle.cliff_materialization import words
        from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
        f, m, u = self.terrain, self.m, self.u
        self.starts.scene(dict(seed=case['seed']), rough=case['rough'])
        prior = m.alloc(4)
        for p in f.cells.values():
            u.mem_write(p + CELL_OBJECT, words(0))
            u.mem_write(p + CELL_LAND, words(0))
        for xy in self.diamond():
            u.mem_write(self.base.record(*xy) + 0x47, b'\0')
        for key, offset, value in (('occupied', CELL_OBJECT, prior), ('overlay', CELL_OVERLAY, 0),
                                   ('rock', CELL_LAND, 3)):
            for x, y in case[key]:
                u.mem_write(f.cells[(x, y)] + offset, words(value))
        for key, offset in (('locked', 0x45), ('visited', 0x47)):
            for x, y in case[key]:
                u.mem_write(self.base.record(x, y) + offset, b'\1')
        names, trees = [], []

        def back(machine, sp, cleanup, eax):
            machine.reg_write(UC_X86_REG_EAX, eax)
            machine.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)
            machine.reg_write(UC_X86_REG_EIP, m.read32(sp))

        def stub(machine, pc, sp):
            if pc == TERRAIN_FIND:
                raw = bytes(machine.mem_read(machine.reg_read(UC_X86_REG_ECX), 16))
                names.append(raw.split(b'\0')[0].decode('ascii'))
                back(machine, sp, 0, 0)
            elif pc == TERRAIN_CTOR:
                cell = struct.unpack('<hh', machine.mem_read(m.read32(sp + 8), 4))
                trees.append([names.pop(), *cell])
                back(machine, sp, 8, machine.reg_read(UC_X86_REG_ECX))
            else:
                return False
            return True

        origin = f.cells[tuple(case['origin'])]
        density = struct.unpack('<II', struct.pack('<d', case['density']))
        row = self.starts.execute(case, pool, ENTRY, 0, (origin, case['size'], *density),
                                  extra=stub)
        native.require(not names, 'A tree name was formatted without a constructor')
        visited = [list(xy) for xy in self.diamond()
                   if u.mem_read(self.base.record(*xy) + 0x47, 1)[0]]
        return row | dict(placed=u.reg_read(UC_X86_REG_EAX), trees=trees, visited=visited)


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = TreeRegionFixture(), StatePool()
    rows = [fixture.run(case, pool) for case in controls()]
    return dict(schema=1, scene=dict(rough=fixture.terrain.bases['cliff']),
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='7 whole original PlaceTreeRegion calls: dense and sparse open regions, walls, prior objects, overlays and Rock land, a locked ring, the 25 * size node budget, a diamond-edge origin and cells an earlier region visited; every density, variant and jitter draw, the visited marks, the returned count and each tree name and cell',
        assumptions=[
            'Existing CliffFixture, TiberiumFieldFixture and StartPointsFixture own original-constructed Cell backing, admission, the stride-33 scratch, the physical clear/cliff TMP registration and the guarded arena; bounds 16..48. Cells start tile 0, overlay -1, no object (+E4), land type Clear (+EC). Scratch +45 lock and +47 visited 0 unless declared.',
            'Declared scene: CliffSet-base cells, a prior object pointer in +E4, overlay 0 in +44, land type 3 in +EC, +45 locks and +47 marks. Stack arguments: the origin Cell, the size and the density double. FPCW 0E7F. All streams seeded by original 65C6D0. Not a complete Generate prefix.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS, with guard bytes after each block that refuse a pool overrun; native code bytes unchanged. No draw, priority, mark or name is supplied.',
            'TerrainTypeClass::Find_By_Name_Index 71DD80 records its original-formatted name and returns slot 0; the TerrainClass constructor 71BB90 records its cell and returns. Object construction and placement are not executed: within one region no walked cell is read again.'],
        entry_points=dict(place_tree_region=ENTRY, terrain_find=TERRAIN_FIND,
                          terrain_ctor=TERRAIN_CTOR, next=0x65C780))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
