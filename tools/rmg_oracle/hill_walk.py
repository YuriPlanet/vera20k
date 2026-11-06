"""Original hill height field over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.hill_walk --check` replays whole original
`Rmg__HillsWaterAdjacencySeed @ 0x005A33F0` and `Rmg__HillHeightRandomWalk @
0x005A2F50` calls, the two stages of `CreateHills @ 0x005A35F0` that own its
RNG; `--write` replaces the corpus `hills::tests::native_hill_walk_controls`
consumes. Original code computes every seed, lock, Gaussian draw, height,
velocity and the final rounding. The scene is a declared component input,
not a complete Generate prefix.
"""
from pathlib import Path
import struct

SEED, WALK = 0x5A33F0, 0x5A2F50
GAUSS_STATE, UNIFORM_SOURCE = 0xABDFB8, 0x598000


def controls():
    """Declared inputs only. `rough` cells hold CliffSet tiles (special
    terrain), `shore` cells shore pieces, `locked` the scratch +45 flag;
    `ruggedness` is the generator's +0x44."""
    from tools.rmg_oracle.start_points import box
    from tools.rmg_oracle.tiberium_field import in_diamond
    shore = [[x, 12] for x in range(12, 22)] + [[12, y] for y in range(13, 22)]
    cliffs = [[x, 24] for x in range(18, 28)]
    rows = [
        dict(id='flat_ruggedness_50', seed=1, ruggedness=50),
        dict(id='flat_ruggedness_100', seed=2, ruggedness=100),
        # 10 * 0.0025 stays under 0.025: Ruggedness 10 skips the walk, leaving
        # only the seed's heights and locks; 11 is the first that walks.
        dict(id='seed_only_at_ruggedness_10', seed=3, ruggedness=10, shore=shore,
             rough=cliffs),
        dict(id='shore_and_cliffs_ruggedness_11', seed=4, ruggedness=11, shore=shore,
             rough=cliffs),
        dict(id='start_locks_ruggedness_70', seed=5, ruggedness=70, shore=shore,
             locked=box(18, 18, 22, 22)),
    ]
    rows = [dict(shore=[], rough=[], locked=[]) | row for row in rows]
    for row in rows:
        for key in ('shore', 'rough', 'locked'):
            row[key] = [xy for xy in row[key] if in_diamond(*xy)]
    return rows


def bits(value):
    return struct.unpack('<Q', struct.pack('<d', value))[0]


class HillWalkFixture:
    def __init__(self):
        from tools.rmg_oracle.cliff_materialization import FAMILY_GLOBALS
        from tools.rmg_oracle.start_points import StartPointsFixture
        self.starts = StartPointsFixture()
        self.base, self.terrain = self.starts.base, self.starts.terrain
        self.m, self.u = self.starts.m, self.starts.u
        self.families = {f'{a:08X}': self.m.read32(a) for a in FAMILY_GLOBALS}

    def diamond(self):
        from tools.rmg_oracle.tiberium_field import in_diamond, STRIDE
        return [(x, y) for y in range(STRIDE) for x in range(STRIDE) if in_diamond(x, y)]

    def run(self, case, pool):
        from tools import native_oracle as native
        from tools.rmg_oracle.cliff_materialization import words
        f, m, u = self.terrain, self.m, self.u
        self.starts.scene(dict(seed=case['seed']), rough=case['rough'])
        for x, y in case['shore']:
            u.mem_write(f.cells[(x, y)] + 0x38, words(self.terrain.bases['shore']))
        for xy in self.diamond():
            record = self.base.record(*xy)
            u.mem_write(record + 8, bytes(0x10))
        for x, y in case['locked']:
            u.mem_write(self.base.record(x, y) + 0x45, b'\1')
        u.mem_write(GAUSS_STATE, bytes(0x10) + words(UNIFORM_SOURCE))
        actor = m.alloc(0x48)
        u.mem_write(actor, bytes(0x48))
        u.mem_write(actor + 0x44, words(case['ruggedness']))
        seeded = self.starts.execute(case, pool, SEED, 0, ())
        native.require(seeded['draws'] == 0, 'The water seed drew')
        row = self.starts.execute(case, pool, WALK, actor, ())
        heights, velocities, locks = [], [], []
        for xy in self.diamond():
            record = self.base.record(*xy)
            height, velocity = struct.unpack('<dd', u.mem_read(record + 8, 16))
            heights.append(int(height) if height == int(height) else f'{bits(height):016x}')
            velocities.append(f'{bits(velocity):016x}' if bits(velocity) else 0)
            if u.mem_read(record + 0x45, 1)[0]:
                locks.append(list(xy))
        flag = u.mem_read(GAUSS_STATE, 1)[0]
        cached = struct.unpack('<Q', u.mem_read(GAUSS_STATE + 8, 8))[0]
        return row | dict(heights=heights, velocities=velocities, locks=locks,
                          gauss_cached=flag, gauss_value_bits=cached if flag else 0)


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = HillWalkFixture(), StatePool()
    rows = [fixture.run(case, pool) for case in controls()]
    return dict(schema=1, scene=dict(rough=fixture.terrain.bases['cliff'],
                                     shore=fixture.terrain.bases['shore'],
                                     family_globals=fixture.families),
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='5 whole original HillsWaterAdjacencySeed calls each followed by a whole HillHeightRandomWalk call: flat maps at Ruggedness 50 and 100, a shore/cliff scene at Ruggedness 10 (no walk) and 11, and start locks; every seeded height and lock, Gaussian draw, height, velocity, the final rounding and the Gaussian cache',
        assumptions=[
            'Existing CliffFixture, TiberiumFieldFixture and StartPointsFixture own original-constructed Cell backing, admission, the stride-33 scratch, the physical clear/cliff/shore TMP registration, the native theater tile-set globals and the guarded arena; bounds 16..48. Cells start tile 0. Scratch height and velocity 0, +45 lock 0 unless declared.',
            'Declared scene: CliffSet-base and shore-piece-base cells and +45 locks; generator +0x44 Ruggedness. Gaussian cache empty with source 598000. FPCW 0E7F. All streams seeded by original 65C6D0. Not a complete Generate prefix.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS, with guard bytes after each block that refuse a pool overrun; native code bytes unchanged. No height, velocity, lock or RNG result is supplied.'],
        entry_points=dict(water_seed=SEED, height_walk=WALK, gaussian=0x5980C0, next=0x65C780))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
