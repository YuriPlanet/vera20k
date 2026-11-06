"""Original temperate rock pass over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.rock_pass --check` replays the rock tail of
`RandomMapGenerator::PaintLatPatchesTreesRocks_Temperate @ 0x005A3AE0`
(0x005A40C3 up to its epilogue at 0x005A4277): the `SurfaceDataCount
@ 0x0042B1F0` target draw, the slot draws and their (0, 0) border refusal,
the overlay test, `Tile_Is_ClearToSandLAT @ 0x00486790`, the clear and green
predicates (0x00486380, 0x004867B0), the variant draws and the density clear;
`--write` replaces the corpus `rocks::tests::native_rock_pass_controls`
consumes. `PaintLatPatchesTrees_NonTemperate @ 0x005A4280` has no rock pass.
The scene is a declared component input, not a complete Generate prefix.
"""
from pathlib import Path
import struct

ENTRY, EPILOGUE = 0x5A40C3, 0x5A4277
RECALC = 0x47D2B0  # CellClass::RecalcAttributes(-1), RET 4
MAP = 0x87F7E8     # +F4/+F8 map rect width/height SurfaceDataCount reads
CELL_OVERLAY, CELL_DENSITY = 0x44, 0x11E
# Tile-set globals the predicates read: ClearToSandLAT (486790), GreenTile
# and ClearToGreenLAT (4867B0).
TILE_GLOBALS = dict(sand_lat=0xAA0E24, green=0xAA0E18, green_lat=0xAA0748)


def controls():
    """Declared inputs only. `size` is the map rect the target scales by;
    `tiles` are `[x, y, family, offset]` (family base plus offset), `overlay`
    cells hold overlay 0, `density` cells a prior density byte."""
    from tools.rmg_oracle.start_points import box
    from tools.rmg_oracle.tiberium_field import in_diamond
    every = box(0, 0, 32, 32)
    rows = [
        dict(id='clear_map', seed=1),
        dict(id='clear_map_large_target', seed=4, size=[60, 60]),
        # Sand LAT transitions take SROCK; the sand base itself is not one.
        dict(id='sand_lat_and_green', seed=7, size=[60, 60],
             tiles=[[x, y, 'sand_lat', (x + y) % 16] for x, y in box(8, 8, 24, 16)]
                   + [[x, y, 'green', 0] for x, y in box(8, 17, 24, 20)]
                   + [[x, y, 'green_lat', (x * y) % 16] for x, y in box(8, 21, 24, 26)]),
        # Rough cells count toward the target without a rock.
        dict(id='rough_counts_without_rock', seed=9, size=[60, 60],
             tiles=[[x, y, 'rough', 0] for x, y in every if (x + y) % 3]),
        # Overlay cells do not count: the attempt budget ends the pass.
        dict(id='overlays_spend_the_budget', seed=11, size=[60, 60],
             overlay=[xy for xy in every if xy[0] % 4 or xy[1] % 4]),
        dict(id='prior_density_cleared', seed=13, size=[40, 40],
             density=[[x, y, 9] for x, y in every]),
        # A zero target draws nothing more.
        dict(id='small_map_target', seed=3, size=[8, 8]),
    ]
    rows = [dict(size=[16, 16], tiles=[], overlay=[], density=[]) | row for row in rows]
    for row in rows:
        for key in ('tiles', 'overlay', 'density'):
            row[key] = [item for item in row[key] if in_diamond(item[0], item[1])]
    return rows


class RockPassFixture:
    def __init__(self):
        from tools.rmg_oracle.start_points import StartPointsFixture
        self.starts = StartPointsFixture()
        self.base, self.terrain = self.starts.base, self.starts.terrain
        self.m, self.u = self.starts.m, self.starts.u
        self.tiles = dict(rough=self.terrain.bases['cliff'],
                          **{k: self.m.read32(a) for k, a in TILE_GLOBALS.items()})

    def diamond(self):
        from tools.rmg_oracle.tiberium_field import in_diamond, STRIDE
        return [(x, y) for y in range(STRIDE) for x in range(STRIDE) if in_diamond(x, y)]

    def state(self):
        f, u = self.terrain, self.u
        return {xy: (struct.unpack('<i', u.mem_read(f.cells[xy] + CELL_OVERLAY, 4))[0],
                     u.mem_read(f.cells[xy] + CELL_DENSITY, 1)[0]) for xy in self.diamond()}

    def run(self, case, pool):
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.start_points import published
        from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
        f, m, u = self.terrain, self.m, self.u
        self.starts.scene(dict(seed=case['seed']))
        u.mem_write(MAP + 0xF4, words(*case['size']))
        for x, y, family, offset in case['tiles']:
            u.mem_write(f.cells[(x, y)] + 0x38, words(self.tiles[family] + offset))
        for x, y in case['overlay']:
            u.mem_write(f.cells[(x, y)] + CELL_OVERLAY, words(0))
        for x, y, value in case['density']:
            u.mem_write(f.cells[(x, y)] + CELL_DENSITY, bytes([value]))
        before = self.state()
        recalcs = []

        def stub(machine, pc, sp):
            if pc != RECALC:
                return False
            cell = machine.reg_read(UC_X86_REG_ECX)
            recalcs.append([*struct.unpack('<hh', machine.mem_read(cell + 0x24, 4)),
                            struct.unpack('<i', machine.mem_read(sp + 4, 4))[0]])
            machine.reg_write(UC_X86_REG_ESP, sp + 8)
            machine.reg_write(UC_X86_REG_EIP, m.read32(sp))
            return True

        row = self.starts.execute(case, pool, ENTRY, 0, (), extra=stub, until=EPILOGUE)
        after = self.state()
        changed = [[x, y, *after[(x, y)]] for (x, y) in self.diamond()
                   if after[(x, y)] != before[(x, y)]]
        return published(row) | dict(changed=changed, recalc_count=len(recalcs))


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = RockPassFixture(), StatePool()
    rows = [fixture.run(case, pool) for case in controls()]
    return dict(schema=1, scene=dict(tiles=fixture.tiles), rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='7 original runs of the temperate rock tail 5A40C3..5A4277: clear, sand-LAT, green, green-LAT and rough tiles, prior overlays that spend the attempt budget, prior densities and a zero target; every target, slot and variant draw, overlay, density clear and RecalcAttributes call',
        assumptions=[
            'Existing CliffFixture, TiberiumFieldFixture and StartPointsFixture own original-constructed Cell backing, admission, the stride-33 scratch with in-diamond coordinates and (0, 0) elsewhere, the physical temperate clear/cliff TMP registration, the native theater tile-set globals and the guarded arena; bounds 16..48. Cells start tile 0, overlay -1, density 0.',
            'Declared scene: MapClass +F4/+F8 rect, which only SurfaceDataCount reads here; CliffSet-base, ClearToSandLAT, GreenTile and ClearToGreenLAT tiles; overlay 0 in +44; densities in +11E. The slice starts at 5A40C3 with EBX 0, the zero register 5A3AE0 holds there, on a fresh frame, and stops before the epilogue. FPCW 0E7F. All streams seeded by original 65C6D0. Not a complete Generate prefix.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS; native code bytes unchanged. No draw, overlay or density is supplied.',
            'CellClass::RecalcAttributes 47D2B0 records its Cell and argument and returns (RET 4) without running: its attribute, LAT and land-type writes are outside this comparison.'],
        entry_points=dict(rock_tail=ENTRY, epilogue=EPILOGUE, surface_data_count=0x42B1F0,
                          clear_to_sand_lat=0x486790, recalc=RECALC, next=0x65C780))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
