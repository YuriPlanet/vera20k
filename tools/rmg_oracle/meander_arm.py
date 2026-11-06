"""Original meander-arm controls over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.meander_arm --check` replays whole original
`RandomMapGenerator::GrowMeanderArm @ 0x005A08D0` calls; `--write` replaces the
corpus that `meander::tests::native_meander_arm_controls` consumes. Original
code computes every claimed cell, flag, return value and RNG/Gaussian draw.
The supplied scene is a declared component input, not a complete Generate
prefix.
"""
from pathlib import Path
import struct

ENTRY = 0x5A08D0
RNG_NEXT, GAUSSIAN = 0x65C780, 0x5980C0
GAUSS_STATE, UNIFORM_SOURCE = 0xABDFB8, 0x598000
LINEAR_OFFSETS = 0x89A304
STRIDE, LOW, HIGH = 33, 16, 48
ARENA, ARENA_SIZE = 0x50000000, 0x01000000
FULL = [0, 0, 512, 512]


def in_diamond(x, y):
    return LOW < x + y and x - y < LOW and y - x < LOW and x + y <= HIGH


def controls():
    """Declared inputs only. `river` cells carry the grown tag; `foreign`
    cells another region id; `rough` cells a non-clear tile. Densities 0.01
    and 0.003 are the canyon's and the bridge plateau's; the others shorten
    the step budget so the growth stops partway."""
    river = [[x, 16] for x in range(12, 21)]
    rows = [
        # CarveRiver 0x0059E3C2: the canyon, full rect, frontier claimed.
        dict(id='canyon', seed=7, density=0x3C23D70A, rect=FULL, ref=[12, 16], claim=1),
        dict(id='canyon_short_budget', seed=11, density=0x3E4CCCCD, rect=FULL, ref=[12, 16],
             claim=1),
        # BuildRiverBridge 0x0059EC65: each bridge direction's rect and reference.
        dict(id='bridge_north', seed=3, density=0x3D4CCCCD, rect=[0, 12, 512, 500],
             ref=[16, 12], claim=0),
        dict(id='bridge_east', seed=42, density=0x3D4CCCCD, rect=[0, 0, 20, 512],
             ref=[21, 16], claim=0),
        dict(id='bridge_plateau_density', seed=5, density=0x3B449BA6, rect=[0, 0, 512, 20],
             ref=[16, 17], claim=0),
        # A foreign region beside the river: the scan clears the success flag.
        dict(id='foreign_region_aborts', seed=7, density=0x3D4CCCCD, rect=FULL, ref=[12, 16],
             claim=1, foreign=[[x, 18] for x in range(12, 21)]),
        dict(id='rough_ring_skipped', seed=11, density=0x3D4CCCCD, rect=FULL, ref=[12, 16],
             claim=1, rough=[[x, y] for x in range(10, 23) for y in (14, 18)]),
    ]
    return [dict(tag=7, river=river, foreign=[], rough=[], gen=[16, 16]) | row for row in rows]


class MeanderFixture:
    def __init__(self):
        from tools.rmg_oracle.cliff_materialization import CliffFixture
        self.terrain = CliffFixture()
        self.m, self.u = self.terrain.m, self.terrain.u
        self.actor = self.m.alloc(0x400)
        self.args = self.m.alloc(0x20)
        self.u.mem_map(ARENA, ARENA_SIZE)
        self.cursor = ARENA
        self.m.alloc = self.alloc

    def alloc(self, size):
        out = self.cursor
        self.cursor += (size + 15) & ~15
        assert self.cursor < ARENA + ARENA_SIZE, 'meander arena exhausted'
        return out

    def record(self, x, y):
        return self.terrain.scratch + (y * STRIDE + x) * 0x50

    def prepare(self, case):
        from tools.rmg_oracle.cliff_materialization import words
        f, u = self.terrain, self.u
        f.reset(dict(kind='meander', seed=case['seed']))
        u.mem_write(ARENA, bytes(self.cursor - ARENA))
        self.cursor = ARENA
        for index in range(STRIDE * STRIDE):
            x, y = index % STRIDE, index // STRIDE
            coord = (x, y) if in_diamond(x, y) else (0, 0)
            u.mem_write(f.scratch + index * 0x50, struct.pack('<hh', *coord))
            u.mem_write(f.scratch + index * 0x50 + 0x38, words(0, 0, -1))
            u.mem_write(f.scratch + index * 0x50 + 0x4A, b'\1\0')  # shore enable, flag clear
        for cell in f.cells.values():
            u.mem_write(cell + 0x38, words(0))
            u.mem_write(cell + 0x11A, bytes(3))
        for x, y in case['river']:
            u.mem_write(self.record(x, y) + 0x38, words(case['tag']))
        for x, y in case['foreign']:
            u.mem_write(self.record(x, y) + 0x38, words(3))
        for x, y in case['rough']:
            if in_diamond(x, y):
                u.mem_write(f.cells[(x, y)] + 0x38, words(f.bases['cliff']))
        self.prior = self.fields()
        w = STRIDE
        # PathfinderClass::ResizeMapArrays @ 0x0042AC00's offsets for this width.
        u.mem_write(LINEAR_OFFSETS, words(-w, 1 - w, 1, w + 1, w, w - 1, -1, -1 - w))
        u.mem_write(GAUSS_STATE, bytes(0x10) + words(UNIFORM_SOURCE))
        u.mem_write(self.actor, bytes(0x400))
        u.mem_write(self.actor + 0x180, words(*case['gen']))
        u.mem_write(self.args, words(*case['rect']) + struct.pack('<hh', *case['ref']))

    def fields(self):
        f, u = self.terrain, self.u
        out = {}
        for y in range(STRIDE):
            for x in range(STRIDE):
                if in_diamond(x, y):
                    record = self.record(x, y)
                    region = struct.unpack('<i', u.mem_read(record + 0x38, 4))[0]
                    out[(x, y)] = [region, u.mem_read(record + 0x4B, 1)[0]]
        return out

    def observe(self):
        # Only cells whose region or +4B flag changed, in row-major order.
        return [[x, y] + value
                for (x, y), value in sorted(self.fields().items(), key=lambda i: i[0][::-1])
                if value != self.prior[(x, y)]]

    def run(self, case, pool):
        from tools import native_oracle as native
        from tools.rmg_oracle.gen_rng_vectors import check_code
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
                assert machine.reg_read(UC_X86_REG_ECX) == f.rngs['mapgen']
                trace.entered(pc, sp, ('next', 0, 0))
            elif pc == GAUSSIAN:
                assert machine.reg_read(UC_X86_REG_ECX) == GAUSS_STATE
                trace.entered(pc, sp, ('gaussian', 0, 0))

        hook = u.hook_add(UC_HOOK_CODE, observe)
        f.active = True
        try:
            args = (case['tag'], case['density'], self.args, self.args + 0x10, case['claim'])
            result = m.invoke(ENTRY, self.actor, args, instruction_limit=400_000_000,
                              timeout_us=300_000_000, context=dict(control=case['id']))
            trace.returned(native.RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
            assert not trace.pending
            check_code(u, f.code)
            after = f.states()
            assert all(before[name] == after[name] for name in ('main', 'scenario'))
            exit_state = pool.refs(dict(after=after['mapgen']))['after']
            return dict(input=case, result=result & 255, rng_after=exit_state,
                        cells=self.observe())
        finally:
            f.active = False
            u.hook_del(hook)


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = MeanderFixture(), StatePool()
    rows = [fixture.run(case, pool) for case in controls()]
    return dict(schema=1, scene=dict(cliff_base=fixture.terrain.bases['cliff']),
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='7 whole original GrowMeanderArm calls: the canyon and bridge-plateau argument shapes, shortened step budgets, a foreign-region abort and skipped rough cells, with the border seeding, heap, Gaussian heading and frontier claim',
        assumptions=[
            'Existing CliffFixture owns original-constructed Cell backing, admission, the stride-33 scratch, the physical clear/cliff TMP registration and the 49F2F0 direction table; bounds 16..48. Scratch coordinates stamped for in-diamond records only; region and stamp start 0, mask cache -1, shore enable 1, +4B flag 0.',
            'Declared scene: clear cells, a river row carrying the grown tag, optional foreign region 3 and non-clear (CliffSet base) cells. Generator +180/+184 generated size 16x16. Densities 0x3C23D70A and 0x3B449BA6 are the live callers\' pushes; the others are declared to stop growth partway. Not a complete Generate prefix.',
            'Linear neighbour offsets written as PathfinderClass::ResizeMapArrays 42AC00 computes them for stride 33. Gaussian cache empty with source 598000. FPCW 0E7F. All streams seeded by original 65C6D0.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS; native code bytes unchanged. No region, flag, RNG or Gaussian result is supplied.'],
        entry_points=dict(grow_meander_arm=ENTRY, next=RNG_NEXT, gaussian=GAUSSIAN))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
