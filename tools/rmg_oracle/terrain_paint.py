"""Original green spread and temperate LAT probabilities over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.terrain_paint --check` replays whole original
`RandomMapGenerator::SpreadGreenTerrain @ 0x0059B740` calls (with
`CellClass::IsGreenTerrainTile @ 0x004867B0`, `IsClearTile @ 0x00486380`,
`MapCoord_StepByDir_GetCell @ 0x00481810` and the candidate vector's growth)
and whole `RandomMapGenerator::SeedLatPatchProbabilities_Temperate
@ 0x005A38C0` calls; `--write` replaces the corpus
`green_spread::tests::native_spread_green_controls` and
`lat_patches::tests::native_temperate_probability_controls` consume. Original
code computes every candidate, conversion, Dummy write, probability and draw.
The scene is a declared component input, not a complete Generate prefix.
"""
from pathlib import Path
import struct

SPREAD, PROBABILITIES = 0x59B740, 0x5A38C0
MAPSEED = 0xABDFD8  # +0x5C Vegetation, read by 5A38C0
# Tile-set globals the predicates read: GreenTile and ClearToGreenLAT
# (4867B0), ShorePieces (4865B0).
TILE_GLOBALS = dict(green=0xAA0E18, green_lat=0xAA0748, shore=0xABAD28)
DUMMY = 0xABDC50


def spread_controls():
    """Declared inputs only. Every in-diamond cell starts as the CliffSet base
    (`rough`, neither clear nor green) unless `clear` names it (tile 0) or
    `tiles` gives `[x, y, family, offset]`."""
    from tools.rmg_oracle.start_points import box
    rows = [
        dict(id='open_field_one_green', seed=7, clear=box(12, 12, 28, 28),
             tiles=[[20, 20, 'green', 0]]),
        dict(id='green_lat_window_edges', seed=11, clear=box(12, 12, 28, 28),
             tiles=[[16, 16, 'green_lat', 0], [24, 24, 'green_lat', 15],
                    [16, 24, 'green_lat', 16], [24, 16, 'green_lat', -1]]),
        # A clear cell between two green cells is a duplicate candidate.
        dict(id='duplicate_candidates', seed=3, clear=box(14, 14, 26, 26),
             tiles=[[19, 20, 'green', 0], [21, 20, 'green', 0], [20, 19, 'green', 0],
                    [20, 21, 'green', 0]]),
        # Green against the diamond edge appends the clear (FFFF) Dummy, which
        # a conversion paints; its later steps read the stamped coordinate.
        dict(id='edge_green_paints_dummy', seed=5, clear=box(0, 0, 32, 32),
             tiles=[[x, y, 'green', 0] for x, y in ((9, 8), (8, 9), (24, 8), (31, 17), (17, 31))]),
        dict(id='many_greens_open_map', seed=42, clear=box(0, 0, 32, 32),
             tiles=[[x, y, 'green', 0] for x, y in box(0, 0, 32, 32) if (x * 7 + y * 3) % 11 == 0]),
        # Two candidates make the conversion count zero: no draw.
        dict(id='fewer_than_three_candidates', seed=9, clear=[[20, 19], [20, 21]],
             tiles=[[20, 20, 'green', 0]]),
        dict(id='no_green', seed=13, clear=box(12, 12, 28, 28)),
    ]
    return [admitted(dict(tiles=[]) | row, 'clear', 'tiles') for row in rows]


def probability_controls():
    """Declared Vegetation (MapSeed +5C), ShorePieces tiles `[x, y, offset]`
    and prior sand probabilities (scratch +28) on named records."""
    from tools.rmg_oracle.start_points import box
    shores = [[x, 20, x - 10] for x in range(10, 33)] + [[20, y, 41 - y] for y in range(8, 33)]
    rows = [
        dict(id='vegetation_zero_no_shore', vegetation=0),
        dict(id='vegetation_fifty_shore_band', vegetation=50, shore=shores),
        # Offsets -1 and 42 sit outside the 42-piece window.
        dict(id='vegetation_odd_shore_window_edges', vegetation=37,
             shore=[[16, 16, -1], [17, 16, 0], [18, 16, 41], [19, 16, 42],
                    [9, 8, 3], [31, 17, 5], [17, 31, 7]]),
        # A prior sand probability keeps a non-shore record; a shore record
        # is rewritten.
        dict(id='vegetation_full_prior_records', vegetation=100, shore=[[20, 20, 4], [21, 21, 9]],
             prior=[[x, y] for x, y in box(18, 18, 23, 23)]),
    ]
    return [admitted(dict(shore=[], prior=[]) | row, 'shore', 'prior') for row in rows]


def admitted(row, *keys):
    """Scene overrides name in-diamond (admitted) cells only."""
    from tools.rmg_oracle.tiberium_field import in_diamond
    for key in keys:
        row[key] = [item for item in row[key] if in_diamond(item[0], item[1])]
    return row


def bits(value):
    return struct.unpack('<Q', struct.pack('<d', value))[0]


class TerrainPaintFixture:
    def __init__(self):
        from tools.rmg_oracle.start_points import StartPointsFixture
        from tools.rmg_oracle.tiberium_field import in_diamond
        self.starts = StartPointsFixture()
        self.base, self.terrain = self.starts.base, self.starts.terrain
        self.m, self.u = self.starts.m, self.starts.u
        self.tiles = dict(rough=self.terrain.bases['cliff'],
                          **{k: self.m.read32(a) for k, a in TILE_GLOBALS.items()})
        self.cells = sorted((xy for xy in self.terrain.cells if in_diamond(*xy)),
                            key=lambda xy: xy[::-1])
        from tools import native_oracle as native
        native.require(set(self.cells) == set(self.terrain.cells),
                       'Admitted cells differ from the declared diamond')

    def tile(self, xy):
        return struct.unpack('<i', self.u.mem_read(self.terrain.cells[xy] + 0x38, 4))[0]

    def run_spread(self, case, pool):
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.start_points import published
        f, u = self.terrain, self.u
        self.starts.scene(dict(seed=case['seed']))
        clear = {tuple(xy) for xy in case['clear']}
        for xy in self.cells:
            u.mem_write(f.cells[xy] + 0x38, words(0 if xy in clear else self.tiles['rough']))
        for x, y, family, offset in case['tiles']:
            u.mem_write(f.cells[(x, y)] + 0x38, words(self.tiles[family] + offset))
        before = {xy: self.tile(xy) for xy in self.cells}
        dummy = struct.unpack('<i', u.mem_read(DUMMY + 0x38, 4))[0]
        row = self.starts.execute(case, pool, SPREAD, 0, ())
        changed = [[x, y, self.tile((x, y))] for x, y in self.cells
                   if self.tile((x, y)) != before[(x, y)]]
        return published(row) | dict(
            changed=changed, dummy_tile_before=dummy,
            dummy_tile=struct.unpack('<i', u.mem_read(DUMMY + 0x38, 4))[0])

    def records(self):
        u = self.u
        return {xy: [bits(struct.unpack('<d', u.mem_read(self.base.record(*xy) + off, 8))[0])
                     for off in (0x18, 0x20, 0x28)] for xy in self.cells}

    def run_probabilities(self, case, pool):
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.start_points import published
        f, u = self.terrain, self.u
        self.starts.scene(dict(seed=1))
        for x, y, offset in case['shore']:
            u.mem_write(f.cells[(x, y)] + 0x38, words(self.tiles['shore'] + offset))
        for x, y in case['prior']:
            u.mem_write(self.base.record(x, y) + 0x18, struct.pack('<ddd', 0.25, 0.5, 0.75))
        u.mem_write(MAPSEED + 0x5C, words(case['vegetation']))
        row = self.starts.execute(dict(id=case['id']), pool, PROBABILITIES, MAPSEED, ())
        after = self.records()
        # Every record ends with the common triple except the listed ones.
        triples = [tuple(value) for value in after.values()]
        common = max(set(triples), key=triples.count)
        others = [[x, y, *after[(x, y)]] for x, y in self.cells if tuple(after[(x, y)]) != common]
        return published(row) | dict(input=case, common=list(common), others=others)


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = TerrainPaintFixture(), StatePool()
    spread = [fixture.run_spread(case, pool) for case in spread_controls()]
    probabilities = [fixture.run_probabilities(case, pool) for case in probability_controls()]
    return dict(schema=1, scene=dict(tiles=fixture.tiles), rng_states_hex=pool.states,
                spread=spread, probabilities=probabilities)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='7 whole original SpreadGreenTerrain calls (green base and the 16-tile ClearToGreenLAT window with its edges, duplicate candidates, Dummy appends and paints at the diamond edge, a zero conversion count, no green) and 4 whole SeedLatPatchProbabilities_Temperate calls (Vegetation 0, 37, 50 and 100, the 42-piece ShorePieces window with its edges and diamond-edge 5x5 neighbourhoods, prior sand probabilities); every converted tile, the Dummy tile, the rough/green/sand probabilities of every record (as the common triple and the records that differ from it) and the exit RNG state',
        assumptions=[
            'Existing CliffFixture, TiberiumFieldFixture and StartPointsFixture own original-constructed Cell backing, admission (equal to the declared 16..48 diamond), the stride-33 scratch, the physical temperate TMP registration, the native theater tile-set globals and the guarded arena. Scratch probabilities start 0.0 unless declared.',
            'Declared scene: SpreadGreenTerrain rows put the CliffSet base on every in-diamond cell except declared clear (tile 0), GreenTile, ClearToGreenLAT and ShorePieces tiles; the Dummy starts with its constructor tile FFFF. SeedLatPatchProbabilities rows put tile 0 everywhere except declared ShorePieces tiles, MapSeed ABDFD8 +5C Vegetation. FPCW 0E7F. All streams seeded by original 65C6D0. Not a complete Generate prefix.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS, with guard bytes after each block that refuse a pool overrun; native code bytes unchanged. No candidate, draw, tile or probability is supplied.'],
        entry_points=dict(spread_green_terrain=SPREAD, is_green_terrain_tile=0x4867B0,
                          is_clear_tile=0x486380, step_by_dir=0x481810,
                          seed_lat_patch_probabilities=PROBABILITIES, next=0x65C780))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
