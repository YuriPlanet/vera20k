"""Original RmgFinalizeWaterShore controls over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.water_shore --check` replays whole original
`MapClass::RmgFinalizeWaterShore @ 0x0057A0C0` calls (erosion 0x0057A430,
thin-water cleanup 0x0057A320, both piece-selection passes 0x0057ACF0);
`--write` replaces the corpus that
`cliff_native_tests::native_water_shore_controls` consumes. The fixture's
physical shore TMPs are the ones `cliff_materialization` registers, so the
Rust test takes its blocks and family ranges from that corpus. Original code
computes every tile, subtile, level, region tag, verdict and draw.
"""
from pathlib import Path
import struct

ENTRY = 0x57A0C0
ANCHOR_INIT = 0x57A9C0
MAP = 0x87F7E8
RNG_NEXT = 0x65C780
LINEAR_OFFSETS = 0x89A304
STRIDE, LOW, HIGH = 33, 16, 48
ARENA, ARENA_SIZE = 0x50000000, 0x01000000


def in_diamond(x, y):
    return LOW < x + y and x - y < LOW and y - x < LOW and x + y <= HIGH


def rect(x0, y0, x1, y1):
    return [[x, y] for y in range(y0, y1 + 1) for x in range(x0, x1 + 1)]


def controls():
    """Declared scenes: `water` cells hold the water-set base and `region`;
    `owned` cells keep their tile and take a region id; everything else is
    clear and unowned at level 0."""
    pond = rect(13, 13, 18, 18)
    # A one-cell land column into the pond: erosion floods it.
    spit = [c for c in rect(12, 12, 20, 18) if not (c[0] == 16 and c[1] <= 15)]
    # A one-cell water spike off the pond's east edge: the cleanup reverts it.
    spike = pond + [[19, 15]]
    diamond = [[x, y] for y in range(33) for x in range(33) if abs(x - 16) + abs(y - 16) <= 4]
    rows = [
        dict(id='square_pond', seed=11, water=pond),
        dict(id='land_spit_erodes', seed=7, water=spit),
        dict(id='land_spit_keep', seed=7, water=spit, keep=1, region=0),
        dict(id='water_spike', seed=3, water=spike),
        dict(id='diamond_pond', seed=42, water=diamond),
        # The pinched land cell belongs to another region: erosion fails.
        dict(id='foreign_owned_spit', seed=7, water=spit, owned=[[16, 13, 3]]),
    ]
    return [dict(keep=0, region=7, owned=[]) | row for row in rows]


class ShoreFixture:
    def __init__(self):
        from tools.rmg_oracle.cliff_materialization import CliffFixture
        self.terrain = CliffFixture()
        self.m, self.u = self.terrain.m, self.terrain.u
        self.water = self.m.read32(0xAA0738)
        # The shore-piece anchor table 0xABDB64 is .bss that the C++ static
        # initializer FUN_0057A9C0 (startup table entry 0x813AB4) fills.
        self.m.invoke(ANCHOR_INIT, 0, ())
        self.u.mem_map(ARENA, ARENA_SIZE)
        self.cursor = ARENA
        self.m.alloc = self.alloc

    def alloc(self, size):
        out = self.cursor
        self.cursor += (size + 15) & ~15
        assert self.cursor < ARENA + ARENA_SIZE, 'shore arena exhausted'
        return out

    def record(self, x, y):
        return self.terrain.scratch + (y * STRIDE + x) * 0x50

    def prepare(self, case):
        from tools.rmg_oracle.cliff_materialization import words
        f, u = self.terrain, self.u
        f.reset(dict(kind='shore', seed=case['seed']))
        u.mem_write(ARENA, bytes(self.cursor - ARENA))
        self.cursor = ARENA
        for index in range(STRIDE * STRIDE):
            x, y = index % STRIDE, index // STRIDE
            record = f.scratch + index * 0x50
            coord = (x, y) if in_diamond(x, y) else (0, 0)
            u.mem_write(record, struct.pack('<hh', *coord))
            u.mem_write(record + 0x38, words(0, 0, -1))
            u.mem_write(record + 0x4A, b'\1')  # shore enable, as RmgScratch defaults it
        for (x, y), cell in f.cells.items():
            u.mem_write(cell + 0x38, words(0))
            u.mem_write(cell + 0x11A, bytes(3))
        for x, y in case['water']:
            u.mem_write(f.cells[(x, y)] + 0x38, words(self.water))
            u.mem_write(self.record(x, y) + 0x38, words(case['region']))
        for x, y, region in case['owned']:
            u.mem_write(self.record(x, y) + 0x38, words(region))
        w = STRIDE
        # PathfinderClass::ResizeMapArrays @ 0x0042AC00's offsets for this width.
        u.mem_write(LINEAR_OFFSETS, words(-w, 1 - w, 1, w + 1, w, w - 1, -1, -1 - w))

    def observe(self):
        f, u = self.terrain, self.u
        cells = []
        for y in range(STRIDE):
            for x in range(STRIDE):
                if not in_diamond(x, y) or (x, y) not in f.cells:
                    continue
                tile = struct.unpack('<i', u.mem_read(f.cells[(x, y)] + 0x38, 4))[0]
                sub, level = u.mem_read(f.cells[(x, y)] + 0x11A, 2)
                region = struct.unpack('<i', u.mem_read(self.record(x, y) + 0x38, 4))[0]
                if tile != 0 or region != 0 or level != 0:
                    cells.append([x, y, tile, sub, level, region])
        return cells

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

        hook = u.hook_add(UC_HOOK_CODE, observe)
        f.active = True
        try:
            result = m.invoke(ENTRY, MAP, (case['region'], case['keep']),
                              instruction_limit=200_000_000, timeout_us=300_000_000,
                              context=dict(control=case['id']))
            trace.returned(native.RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
            assert not trace.pending
            check_code(u, f.code)
            after = f.states()
            assert all(before[name] == after[name] for name in ('main', 'scenario'))
            # Entry states are the seeded constructor's, which rng.json covers.
            exit_state = pool.refs(dict(after=after['mapgen']))['after']
            return dict(input=case, result=result & 255, rng_after=exit_state,
                        cells=self.observe())
        finally:
            f.active = False
            u.hook_del(hook)


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = ShoreFixture(), StatePool()
    rows = [fixture.run(case, pool) for case in controls()]
    return dict(schema=1, scene=dict(water_base=fixture.water,
                                     shore_base=fixture.terrain.bases['shore']),
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='6 whole original RmgFinalizeWaterShore calls: erosion, thin-water cleanup, both piece-selection passes and the shared TMP placement, on physical shore TMPs',
        assumptions=[
            'Existing CliffFixture owns original-constructed Cell backing, admission, the stride-33 scratch and the physical clear/cliff/shore TMP registration; bounds 16..48. Scratch coordinates stamped for in-diamond records only; region and stamp 0, mask cache -1, shore enable 1.',
            'Declared scene: clear level-0 cells, water-set base cells with a region id, optional owned land cells. Not a complete Generate prefix.',
            'Shore-piece anchor table filled by its original static initializer 57A9C0. Linear neighbour offsets written as PathfinderClass::ResizeMapArrays 42AC00 computes them for stride 33. FPCW 0E7F. All streams seeded by original 65C6D0.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS; native code bytes unchanged. No shore, tile, RNG or region result is supplied.'],
        entry_points=dict(finalize_water_shore=ENTRY, anchor_init=ANCHOR_INIT, next=RNG_NEXT))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
