"""Original LAT patches over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.lat_patch --check` replays whole original
`RandomMapGenerator::PlaceLatPatch @ 0x005A4B60` calls with the pave flag
both LAT painters pass (0); `--write` replaces the corpus
`lat_patches::tests::native_place_lat_patch_controls` consumes. Original code
computes every jitter draw, the closest-first growth, the stamped tiles and
the patch-id stamps. The scene is a declared component input, not a complete
Generate prefix.
"""
from pathlib import Path
import struct

ENTRY = 0x5A4B60
PATCH_TILE = 300  # any non-clear tile id; the patch only writes it
CELL_OBJECT, CELL_OVERLAY = 0xE4, 0x44


def controls():
    """Declared inputs only. `rough` cells hold a non-clear tile, `pave` the
    native MiscPaveTile, `occupied` a prior object, `overlay` a prior overlay,
    `sloped` slope 1, `claimed` the patch id already in scratch +3C."""
    from tools.rmg_oracle.start_points import box
    from tools.rmg_oracle.tiberium_field import in_diamond
    walls = [[x, 19] for x in range(10, 26)] + [[19, y] for y in range(10, 19)]
    rows = [
        dict(id='open_small', seed=1, size=4),
        dict(id='open_large', seed=2, size=80),
        dict(id='obstacles', seed=3, size=40, rough=walls, pave=box(13, 17, 15, 18),
             occupied=box(13, 13, 14, 14), overlay=box(17, 13, 18, 15), sloped=box(20, 20, 22, 21)),
        # Cells already holding this patch id are never admitted.
        dict(id='claimed_cells', seed=4, size=30, claimed=box(14, 14, 18, 15)),
        # A walled pocket runs out of cells before the size.
        dict(id='walled_pocket', seed=5, size=60,
             rough=[xy for xy in box(13, 13, 19, 19) if 13 in xy or 19 in xy]),
        dict(id='diamond_edge_origin', seed=6, size=25, origin=[24, 24]),
    ]
    rows = [dict(origin=[16, 16], rough=[], pave=[], occupied=[],
                 overlay=[], sloped=[], claimed=[]) | row for row in rows]
    for row in rows:
        row['stamp'] = row['origin'][1] * 0x200 + row['origin'][0]
        for key in ('rough', 'pave', 'occupied', 'overlay', 'sloped', 'claimed'):
            row[key] = [xy for xy in row[key] if in_diamond(*xy)]
    return rows


class LatPatchFixture:
    def __init__(self):
        from tools.rmg_oracle.start_points import StartPointsFixture
        self.starts = StartPointsFixture()
        self.base, self.terrain = self.starts.base, self.starts.terrain
        self.m, self.u = self.starts.m, self.starts.u

    def diamond(self):
        from tools.rmg_oracle.tiberium_field import in_diamond, STRIDE
        return [(x, y) for y in range(STRIDE) for x in range(STRIDE) if in_diamond(x, y)]

    def state(self):
        out = {}
        for xy in self.diamond():
            tile = struct.unpack('<i', self.u.mem_read(self.terrain.cells[xy] + 0x38, 4))[0]
            stamp = struct.unpack('<i', self.u.mem_read(self.base.record(*xy) + 0x3C, 4))[0]
            out[xy] = (tile, stamp)
        return out

    def run(self, case, pool):
        from tools.rmg_oracle.cliff_materialization import words
        f, m, u = self.terrain, self.m, self.u
        tiles = [[x, y, 'misc_pave'] for x, y in case['pave']]
        self.starts.scene(dict(seed=case['seed'], tiles=tiles), rough=case['rough'])
        prior = m.alloc(4)
        for p in f.cells.values():
            u.mem_write(p + CELL_OBJECT, words(0))
        for x, y in case['occupied']:
            u.mem_write(f.cells[(x, y)] + CELL_OBJECT, words(prior))
        for x, y in case['overlay']:
            u.mem_write(f.cells[(x, y)] + CELL_OVERLAY, words(0))
        for x, y in case['sloped']:
            u.mem_write(f.cells[(x, y)] + 0x11C, b'\1')
        for x, y in case['claimed']:
            u.mem_write(self.base.record(x, y) + 0x3C, words(case['stamp']))
        before = self.state()
        origin = f.cells[tuple(case['origin'])]
        row = self.starts.execute(case, pool, ENTRY, 0,
                                  (origin, PATCH_TILE, case['size'], case['stamp'], 0))
        after = self.state()
        changed = [[x, y, *after[(x, y)]] for (x, y) in self.diamond()
                   if after[(x, y)] != before[(x, y)]]
        return row | dict(changed=changed)


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = LatPatchFixture(), StatePool()
    rows = [fixture.run(case, pool) for case in controls()]
    return dict(schema=1, scene=dict(rough=fixture.terrain.bases['cliff'],
                                     misc_pave=fixture.starts.tiles['misc_pave'],
                                     patch_tile=PATCH_TILE),
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='6 whole original PlaceLatPatch calls with the pave flag 0: small and large open patches, walls, MiscPaveTile cells, prior objects, overlays and slopes, cells already holding the patch id, a walled pocket smaller than the size and a diamond-edge origin; every jitter draw, stamped tile and patch-id stamp',
        assumptions=[
            'Existing CliffFixture, TiberiumFieldFixture and StartPointsFixture own original-constructed Cell backing, admission, the stride-33 scratch, the physical clear/cliff TMP registration, the native theater tile-set globals and the guarded arena; bounds 16..48. Cells start tile 0, overlay -1, slope 0, no object (+E4); scratch stamp 0 unless declared.',
            'Declared scene: CliffSet-base and native MiscPaveTile cells, a prior object pointer in +E4, overlay 0 in +44, slope 1 in +11C, the patch id in +3C. Stack arguments: the origin Cell, tile 300, the size, the patch id y*512+x and pave flag 0, as both LAT painters (5A3EF7, 5A43D3) pass EBX = 0. FPCW 0E7F. All streams seeded by original 65C6D0. Not a complete Generate prefix.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS, with guard bytes after each block that refuse a pool overrun; native code bytes unchanged. No draw, priority, tile or stamp is supplied.'],
        entry_points=dict(place_lat_patch=ENTRY, next=0x65C780))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
