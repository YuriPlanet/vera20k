"""Original CarveBlob controls over the existing lake/Cell/scratch VM.

`python -m tools.rmg_oracle.carve_blob --check` replays whole original
`RandomMapGenerator::CarveBlob @ 0x0059BBC0` calls (seed hunt, growth heap,
Gaussian centre drift, jittered or `Rmg__BlobDirectedGrowthKey @ 0x0059B940`
keys, `Rmg__IsCellInRectOrEllipse @ 0x0059BAB0`, drain, the shore pass
`RmgFinalizeWaterShore @ 0x0057A0C0`, shore reset and rollback); `--write`
replaces the corpus `blob::tests::native_carve_blob_controls` consumes.
Original code computes every carved cell, stamp, region tag, return value,
blob id and RNG draw. The supplied scene is a declared component input, not a
complete Generate prefix.
"""
from pathlib import Path
import struct

ENTRY = 0x59BBC0
MAP = 0x87F7E8  # +F4/+F8 map rect width/height the seed hunt scales by
SEED_CANDIDATE = 0x59BD79  # each hunted candidate is at [ESP+0x34] here
STRIDE = 33


def controls():
    """Declared inputs only. The map starts all water (WaterSet base, region 0),
    or all clear land of region `fill`; `water` cells are water of region 0,
    `land` cells are clear with a region id, `rough` cells hold a non-clear
    tile with no region. `rect` is the logical (x, y, w, h) rect, `seed`
    (0, 0) hunts a random water seed. Every max_cells here is under the
    native 400 floor."""
    def wide(x0, y0, x1, y1, region=3):
        from tools.rmg_oracle.start_points import box
        return [[x, y, region] for x, y in box(x0, y0, x1, y1)]
    rows = [
        # Archipelago-style: undirected, drift 0.25, given seed at the rect centre.
        dict(id='undirected_square', seed=7, rect=[4, 4, 8, 8], start=[17, 13]),
        dict(id='undirected_wide', seed=11, rect=[1, 5, 13, 6], start=[17, 16]),
        # Continental/islands-style: directed toward a target, drift 0.75.
        dict(id='directed_wide', seed=3, rect=[1, 1, 14, 9], start=[18, 10], target=[18, 10],
             directed=1, drift=0.75),
        dict(id='directed_off_target', seed=5, rect=[1, 1, 14, 14], start=[12, 12],
             target=[22, 20], directed=1, drift=0.75),
        # A tall rect whose shore pass fails: the blob rolls back at level 4.
        dict(id='directed_tall_rollback', seed=42, rect=[2, 0, 6, 16], start=[11, 11],
             target=[11, 11], directed=1, drift=0.75, level=4),
        # Prior land of another region and rough tiles inside the ellipse.
        dict(id='beside_foreign_land', seed=13, rect=[2, 2, 12, 12], start=[17, 9],
             land=wide(19, 14, 23, 18), rough=[[15, 12], [16, 12], [17, 12]]),
        # The random seed hunt over a partly claimed map.
        dict(id='hunted_seed', seed=21, rect=[1, 1, 14, 14], start=[0, 0],
             land=wide(15, 15, 18, 18)),
        dict(id='hunted_seed_directed', seed=7, rect=[0, 0, 16, 16], start=[0, 0],
             target=[13, 12], directed=1, drift=0.75, land=wide(16, 13, 20, 17)),
        # One water cell, the hunt's 200th candidate under seed 2: the hunt
        # refuses it after its 200th draw pair (59BD7E..59BD88).
        dict(id='hunt_refuses_200th_candidate', seed=2, rect=[1, 1, 14, 14], start=[0, 0],
             fill=3, water=[[14, 27]]),
    ]
    return [dict(blob=5, level=0, fill=0, land=[], water=[], rough=[], target=[0, 0], directed=0, drift=0.25,
                 max_cells=60) | row for row in rows]


class BlobFixture:
    def __init__(self):
        from tools.rmg_oracle.grow_lake import LakeFixture
        self.lake = LakeFixture()
        self.terrain, self.m, self.u = self.lake.terrain, self.lake.m, self.lake.u
        self.water = self.lake.water

    def prepare(self, case):
        from tools.rmg_oracle.cliff_materialization import words
        from tools.rmg_oracle.grow_lake import in_diamond
        lake, f, u = self.lake, self.terrain, self.u
        lake.prepare(dict(seed=case['seed'], water_percent=0, gen=[16, 16], placed=0,
                          region=case['blob'], water=[], rough=[], pocket=None, start=[0, 0]))
        self.args = self.m.alloc(0x20)  # rect, seed and target structs
        for (x, y), cell in f.cells.items():
            if in_diamond(x, y):
                # `fill` > 0 makes every cell clear land of that region.
                u.mem_write(cell + 0x38, words(0 if case['fill'] else self.water))
                u.mem_write(lake.record(x, y) + 0x38, words(case['fill']))
        for x, y in case['water']:
            u.mem_write(f.cells[(x, y)] + 0x38, words(self.water))
            u.mem_write(lake.record(x, y) + 0x38, words(0))
        for x, y, region in case['land']:
            if in_diamond(x, y):
                u.mem_write(f.cells[(x, y)] + 0x38, words(0))
                u.mem_write(lake.record(x, y) + 0x38, words(region))
        for x, y in case['rough']:
            u.mem_write(f.cells[(x, y)] + 0x38, words(f.bases['cliff']))
        u.mem_write(lake.actor + 0x30C, words(case['level']))
        u.mem_write(self.args, words(*case['rect']))
        # The seed is two ints (59BC74..59BC7F, 59BDEC); the target a CellStruct.
        u.mem_write(self.args + 0x10, struct.pack('<ii', *case['start']))
        u.mem_write(self.args + 0x18, struct.pack('<hh', *case['target']))
        self.prior = self.fields()

    def fields(self):
        """[tile, subtile, level, region, stamp] per in-diamond cell."""
        from tools.rmg_oracle.grow_lake import in_diamond
        f, u, lake = self.terrain, self.u, self.lake
        out = {}
        for (x, y), cell in f.cells.items():
            if in_diamond(x, y):
                tile = struct.unpack('<i', u.mem_read(cell + 0x38, 4))[0]
                sub, level = u.mem_read(cell + 0x11A, 2)
                region, stamp = struct.unpack('<ii', u.mem_read(lake.record(x, y) + 0x38, 8))
                out[(x, y)] = [tile, sub, level, region, stamp]
        return out

    def run(self, case, pool, candidates=None):
        """`candidates`, when a list, collects each hunted seed candidate."""
        from tools import native_oracle as native
        from tools.rmg_oracle.gen_rng_vectors import check_code
        from tools.rmg_oracle.grow_lake import RNG_NEXT, GAUSSIAN, GAUSS_STATE
        from unicorn import UC_HOOK_CODE
        from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
        f, m, u = self.terrain, self.m, self.u
        self.prepare(case)
        before = f.states()
        trace = native.NativeCallTrace(u, m.read32)

        def observe(machine, pc, _size, _data):
            if machine.reg_read(UC_X86_REG_EIP) != pc:
                return
            sp = machine.reg_read(UC_X86_REG_ESP)
            trace.returned(pc, sp)
            if pc == RNG_NEXT:
                native.require(machine.reg_read(UC_X86_REG_ECX) == f.rngs['mapgen'], 'Unexpected RNG stream')
                trace.entered(pc, sp, ('next', 0, 0))
            elif pc == GAUSSIAN:
                native.require(machine.reg_read(UC_X86_REG_ECX) == GAUSS_STATE, 'Unexpected Gaussian state')
                trace.entered(pc, sp, ('gaussian', 0, 0))
            elif pc == SEED_CANDIDATE and candidates is not None:
                candidates.append(list(struct.unpack('<hh', machine.mem_read(sp + 0x34, 4))))

        hook = u.hook_add(UC_HOOK_CODE, observe)
        f.active = True
        drift_lo, drift_hi = struct.unpack('<II', struct.pack('<d', case['drift']))
        try:
            result = m.invoke(ENTRY, self.lake.actor,
                              (case['max_cells'], self.args, self.args + 0x10, 1,
                               self.args + 0x18, drift_lo, drift_hi, case['directed']),
                              instruction_limit=200_000_000, timeout_us=300_000_000,
                              context=dict(control=case['id']))
            trace.returned(native.RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
            native.require(not trace.pending, f"{case['id']}: a call did not return")
            check_code(u, f.code)
            after = f.states()
            native.require(all(before[name] == after[name] for name in ('main', 'scenario')),
                           f"{case['id']}: unexpected main/scenario draw")
            now = self.fields()
            cells = [[x, y, *value] for (x, y), value in sorted(now.items(), key=lambda i: i[0][::-1])
                     if value != self.prior[(x, y)]]
            # Entry states are the seeded constructor's, which rng.json covers.
            return dict(input=case, result=result, blob_after=m.read32(self.lake.actor + 0x308),
                        rng_after=pool.refs(dict(after=after['mapgen']))['after'], cells=cells)
        finally:
            f.active = False
            u.hook_del(hook)


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = BlobFixture(), StatePool()
    size = [fixture.m.read32(MAP + 0xF4), fixture.m.read32(MAP + 0xF8)]
    assert size == [16, 16], f'fixture map rect {size}'
    rows = [fixture.run(case, pool) for case in controls()]
    return dict(schema=1, scene=dict(water_base=fixture.water,
                                     cliff_base=fixture.terrain.bases['cliff']),
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='whole original CarveBlob calls: undirected and directed growth over square, wide and tall rects, an off-centre target, foreign land and rough tiles, random seed hunts that succeed or exhaust, and a shore-pass failure that rolls back; every carved cell, stamp, region tag, blob id, return value and RNG/Gaussian draw',
        assumptions=[
            'Existing LakeFixture and CliffFixture own original-constructed Cell backing, admission, the stride-33 scratch, the physical clear/cliff/shore TMP registration, the shore anchor table (original 57A9C0), the linear neighbour offsets and the empty Gaussian cache; bounds 16..48, MapClass +F4/+F8 rect 16x16.',
            'Declared scene: every in-diamond cell WaterSet base with region 0, except clear cells with a declared region and CliffSet-base cells. Generator +308 blob id, +30C rollback level. Stack arguments: max cells, the logical rect, the seed (0, 0 hunts), shape flag 1 as all three shape drivers (59AF02, 59B10E, 59B508) pass it, the target, the drift double and the directed flag. FPCW 0E7F. All streams seeded by original 65C6D0. Not a complete Generate prefix.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS; native code bytes unchanged. No blob, key, RNG, Gaussian, shore or tile result is supplied.'],
        entry_points=dict(carve_blob=ENTRY, directed_key=0x59B940, rect_or_ellipse=0x59BAB0,
                          finalize_water_shore=0x57A0C0, next=0x65C780, gaussian=0x5980C0))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
