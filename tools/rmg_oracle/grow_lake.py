"""Original GrowLake controls over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.grow_lake --check` replays whole original
`RandomMapGenerator::GrowLake @ 0x0059C920` calls; `--write` replaces the
corpus that `lake::tests::native_grow_lake_controls` consumes. Original code
computes every carved cell, region tag, return value, quota and RNG draw. The
supplied scene is a declared component input, not a complete Generate prefix.
"""
from pathlib import Path
import struct

ENTRY = 0x59C920
RNG_NEXT, GAUSSIAN = 0x65C780, 0x5980C0
GAUSS_STATE, UNIFORM_SOURCE = 0xABDFB8, 0x598000
LINEAR_OFFSETS = 0x89A304
STRIDE, LOW, HIGH = 33, 16, 48
ARENA, ARENA_SIZE = 0x50000000, 0x01000000


def in_diamond(x, y):
    return LOW < x + y and x - y < LOW and y - x < LOW and x + y <= HIGH


def controls():
    """Declared inputs only. `water` cells carry a foreign region id; `rough`
    cells hold a non-clear tile."""
    pond = [[x, y] for x in range(26, 29) for y in range(18, 21)]
    rows = [dict(id=f'given_seed_{seed}', seed=seed, start=[24, 24]) for seed in (1, 7, 11, 42)]
    rows += [dict(id='given_seed_near_foreign_water', seed=11, start=[24, 19], water=pond),
             dict(id='given_seed_rough_ring', seed=7, start=[24, 24],
                  rough=[[x, 20] for x in range(18, 29)]),
             # A hunted lake that grows past the size gates runs the shore
             # pass, a component of its own; these pockets stop it first.
             dict(id='hunted_pocket_near_foreign_water', seed=11, start=[0, 0], water=pond,
                  pocket=[20, 18, 27, 27]),
             dict(id='hunted_pocket_seed_3', seed=3, start=[0, 0], pocket=[18, 20, 25, 27]),
             dict(id='hunted_seed_exhausted', seed=5, start=[0, 0], pocket=[0, 0, -1, -1])]
    return [dict(water_percent=60, gen=[16, 16], region=7, placed=0, water=[], rough=[],
                 pocket=None) | row for row in rows]


class LakeFixture:
    def __init__(self):
        from tools.rmg_oracle.cliff_materialization import CliffFixture
        self.terrain = CliffFixture()
        self.m, self.u = self.terrain.m, self.terrain.u
        self.actor = self.m.alloc(0x400)
        self.start = self.m.alloc(4)
        self.water = self.m.read32(0xAA0738)
        self.u.mem_map(ARENA, ARENA_SIZE)
        self.cursor = ARENA
        self.m.alloc = self.alloc

    def alloc(self, size):
        out = self.cursor
        self.cursor += (size + 15) & ~15
        assert self.cursor < ARENA + ARENA_SIZE, 'lake arena exhausted'
        return out

    def record(self, x, y):
        return self.terrain.scratch + (y * STRIDE + x) * 0x50

    def prepare(self, case):
        from tools.rmg_oracle.cliff_materialization import words
        f, u = self.terrain, self.u
        f.reset(dict(kind='lake', seed=case['seed']))
        u.mem_write(ARENA, bytes(self.cursor - ARENA))
        self.cursor = ARENA
        for index in range(STRIDE * STRIDE):
            x, y = index % STRIDE, index // STRIDE
            coord = (x, y) if in_diamond(x, y) else (0, 0)
            u.mem_write(f.scratch + index * 0x50, struct.pack('<hh', *coord))
            u.mem_write(f.scratch + index * 0x50 + 0x38, words(0, 0))
        for x, y in case['water']:
            u.mem_write(f.cells[(x, y)] + 0x38, words(self.water))
            u.mem_write(self.record(x, y) + 0x38, words(3))
        for x, y in case['rough']:
            u.mem_write(f.cells[(x, y)] + 0x38, words(f.bases['cliff']))
        if case['pocket'] is not None:
            x0, y0, x1, y1 = case['pocket']
            for (x, y), cell in f.cells.items():
                if in_diamond(x, y) and not (x0 <= x <= x1 and y0 <= y <= y1):
                    u.mem_write(cell + 0x38, words(f.bases['cliff']))
        w = STRIDE
        # PathfinderClass::ResizeMapArrays @ 0x0042AC00's offsets for this width.
        u.mem_write(LINEAR_OFFSETS, words(-w, 1 - w, 1, w + 1, w, w - 1, -1, -1 - w))
        u.mem_write(GAUSS_STATE, bytes(0x10) + words(UNIFORM_SOURCE))
        u.mem_write(self.actor, bytes(0x400))
        for offset, value in ((0x4C, case['water_percent']), (0x180, case['gen'][0]),
                              (0x184, case['gen'][1]), (0x304, case['placed']),
                              (0x308, case['region'])):
            u.mem_write(self.actor + offset, words(value))
        u.mem_write(self.start, struct.pack('<hh', *case['start']))

    def observe(self):
        f, m, u = self.terrain, self.m, self.u
        cells = []
        for y in range(STRIDE):
            for x in range(STRIDE):
                if not in_diamond(x, y) or (x, y) not in f.cells:
                    continue
                tile = struct.unpack('<i', u.mem_read(f.cells[(x, y)] + 0x38, 4))[0]
                region = struct.unpack('<i', u.mem_read(self.record(x, y) + 0x38, 4))[0]
                if tile == self.water or region != 0:
                    cells.append([x, y, int(tile == self.water), region])
        return dict(cells=cells, placed=m.read32(self.actor + 0x304),
                    region=m.read32(self.actor + 0x308))

    def run(self, case, pool):
        from tools import native_oracle as native
        from tools.rmg_oracle.gen_rng_vectors import check_code
        from unicorn import UC_HOOK_CODE
        from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
        f, m, u = self.terrain, self.m, self.u
        self.prepare(case)
        before = pool.refs(f.states())
        trace = native.NativeCallTrace(u, m.read32)

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

        hook = u.hook_add(UC_HOOK_CODE, observe)
        f.active = True
        try:
            result = m.invoke(ENTRY, self.actor, (self.start,), instruction_limit=200_000_000,
                              timeout_us=300_000_000, context=dict(control=case['id']))
            trace.returned(native.RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
            assert not trace.pending
            check_code(u, f.code)
            after = pool.refs(f.states())
            assert all(before[name] == after[name] for name in ('main', 'scenario'))
            return dict(input=case, result=result & 255, rng_before=before['mapgen'],
                        rng_after=after['mapgen'], **self.observe())
        finally:
            f.active = False
            u.hook_del(hook)


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = LakeFixture(), StatePool()
    rows = [fixture.run(case, pool) for case in controls()]
    return dict(schema=1, scene=dict(stride=STRIDE, bounds=[LOW, HIGH], water_base=fixture.water,
                                     cliff_base=fixture.terrain.bases['cliff']),
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='9 whole original GrowLake calls: given seeds, hunted seeds stopped before the shore pass, an exhausted hunt, the Gaussian size draw, the growth heap, its drain and the rollback',
        assumptions=[
            'Existing CliffFixture owns original-constructed Cell backing, admission and the stride-33 scratch; bounds 16..48. Scratch coordinates stamped for in-diamond records only; region and stamp start 0.',
            'Declared scene: clear cells, optional foreign water (WaterSet base, region 3) and non-clear (CliffSet base) cells, or every in-diamond cell outside a clear pocket non-clear. Generator +4C WaterAmount, +180/+184 generated size, +304 placed, +308 region. Not a complete Generate prefix.',
            'Linear neighbour offsets written as PathfinderClass::ResizeMapArrays 42AC00 computes them for stride 33. Gaussian cache empty with source 598000. FPCW 0E7F. All streams seeded by original 65C6D0.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS; native code bytes unchanged. No lake, RNG, Gaussian or tile result is supplied.'],
        entry_points=dict(grow_lake=ENTRY, next=RNG_NEXT, gaussian=GAUSSIAN))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
