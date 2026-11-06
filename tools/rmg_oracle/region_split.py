"""Original SplitOversizedRegions controls over the existing Cell/scratch VM.

`python -m tools.rmg_oracle.region_split --check` replays whole original
`RandomMapGenerator::SplitOversizedRegions @ 0x0058EBC0` calls (each split runs
`RmgRegion::Split @ 0x0058D620`); `--write` replaces the corpus that
`regions::tests::native_split_oversized_controls` consumes. Original code
computes every level, region id, list entry and RNG draw. The supplied
partition is a declared component input, not a complete Generate prefix.
"""
from pathlib import Path
import struct

ENTRY = 0x58EBC0
REGION_CTOR = 0x58BF70
RNG_NEXT, GAUSSIAN = 0x65C780, 0x5980C0
SPLIT = 0x58D620
REGION_LIST = 0xABDF90  # DynamicVectorClass<RmgRegion*>: +4 items, +8 size, +0x10 count
# vtable__DynamicVectorClass<MapRegionClass*>, as Split stores it at 0x0058DC55;
# the list's static initializer has not run in the VM.
REGION_LIST_VTABLE = 0x7ED858
NEXT_REGION_ID = 0xABED14
MAP_TYPE, REGION_SIZE, GEN_W, GEN_H = 0xABE014, 0xABE048, 0xABE158, 0xABE15C
GAUSS_STATE = 0xABDFB8  # +0 has-spare, +8 spare, +0x10 uniform source
UNIFORM_SOURCE = 0x598000
STRIDE, LOW, HIGH = 33, 16, 48
LINEAR_OFFSETS = 0x89A304
LIST_SLOTS = 512
ARENA, ARENA_SIZE = 0x50000000, 0x10000000


def in_diamond(x, y):
    return LOW < x + y and x - y < LOW and y - x < LOW and x + y <= HIGH


def scene(kind):
    """Declared partition: (region, level, tile_family) per in-diamond cell."""
    def at(x, y):
        if kind == 'flat':
            return 0, 4, 'clear'
        if x + y <= 21:
            return 0, 0, 'clear'          # water-flagged region
        if x - y >= 9:
            return 1, 8, 'clear'          # high strip
        cliff = (x, y) in ((24, 24), (25, 24), (24, 25), (30, 14))
        return 2, 4, 'cliff' if cliff else 'clear'
    return at


def controls():
    # Thresholds trunc(2*(size*0.005+0.05)*h*w) stay above the 100-cell merge
    # size, as on retail-sized maps; below it, merged pieces regrow past the
    # threshold and the native restart loop need not end.
    rows = [dict(id=f'banded_seed_{seed}_size_{size}_gen_{w}', scene='banded', seed=seed,
                 region_size=size, gen=[w, w]) for seed, size, w in
            ((11, 0, 40), (2, 0, 40), (42, 0, 36), (5, 5, 40), (77, 0, 44))]
    rows += [dict(id='banded_no_split_size_100', scene='banded', seed=11,
                  region_size=100, gen=[40, 40]),
             dict(id='flat_seed_3_size_0', scene='flat', seed=3, region_size=0, gen=[40, 40]),
             dict(id='flat_seed_9_wide', scene='flat', seed=9, region_size=0, gen=[64, 24])]
    return rows


class SplitFixture:
    def __init__(self):
        from tools.rmg_oracle.cliff_materialization import CliffFixture
        self.terrain = CliffFixture()
        self.m, self.u = self.terrain.m, self.terrain.u
        self.items = self.m.alloc(LIST_SLOTS * 4)
        # Each component flood reserves a 100000-cell vector and the bump
        # allocator never frees, so controls allocate from a private arena
        # that every control starts afresh.
        self.u.mem_map(ARENA, ARENA_SIZE)
        self.cursor = ARENA
        self.m.alloc = self.alloc

    def alloc(self, size):
        out = self.cursor
        self.cursor += (size + 15) & ~15
        assert self.cursor < ARENA + ARENA_SIZE, 'split arena exhausted'
        return out

    def record(self, x, y):
        return self.terrain.scratch + (y * STRIDE + x) * 0x50

    def prepare(self, case):
        from tools.rmg_oracle.cliff_materialization import words
        f, m, u = self.terrain, self.m, self.u
        f.reset(dict(kind='split', seed=case['seed']))
        u.mem_write(ARENA, bytes(self.cursor - ARENA))
        self.cursor = ARENA
        cliff = f.bases['cliff']
        at = scene(case['scene'])
        for index in range(STRIDE * STRIDE):
            x, y = index % STRIDE, index // STRIDE
            coord = (x, y) if in_diamond(x, y) else (0, 0)
            u.mem_write(f.scratch + index * 0x50, struct.pack('<hh', *coord))
            u.mem_write(f.scratch + index * 0x50 + 0x38, words(-1, -1))
        seeds = {}
        for (x, y), cell in f.cells.items():
            if not in_diamond(x, y):
                continue
            region, level, family = at(x, y)
            u.mem_write(cell + 0x38, words(cliff if family == 'cliff' else 0))
            u.mem_write(cell + 0x11B, bytes([level]))
            u.mem_write(self.record(x, y) + 0x38, words(region))
            if family == 'clear':
                seeds.setdefault(region, (x, y))
        u.mem_write(REGION_LIST, words(REGION_LIST_VTABLE, self.items, LIST_SLOTS))
        u.mem_write(REGION_LIST + 0x10, words(0, 10))
        u.mem_write(REGION_LIST + 0xD, b'\0')
        u.mem_write(NEXT_REGION_ID, words(0))
        regions = []
        for region in sorted(seeds):
            obj = m.alloc(0x50)
            seed = struct.unpack('<I', struct.pack('<hh', *seeds[region]))[0]
            m.invoke(REGION_CTOR, obj, (seed,))
            water = region == 0 and case['scene'] == 'banded'
            u.mem_write(obj + 0x14, bytes([int(water)]))  # the fill's class bit
            regions.append(obj)
        assert m.read32(REGION_LIST + 0x10) == len(regions)
        # PathfinderClass::ResizeMapArrays @ 0x0042AC00 writes the linear
        # width and these eight neighbour offsets together; in game it runs
        # from Generate's map setup. Split stamps through them (0x0058DA41).
        w = STRIDE
        u.mem_write(LINEAR_OFFSETS, words(-w, 1 - w, 1, w + 1, w, w - 1, -1, -1 - w))
        u.mem_write(MAP_TYPE, words(3))
        u.mem_write(REGION_SIZE, words(case['region_size']))
        u.mem_write(GEN_W, words(case['gen'][0], case['gen'][1]))
        u.mem_write(GAUSS_STATE, bytes(0x10) + words(UNIFORM_SOURCE))
        return [dict(id=r, seed=list(seeds[r])) for r in sorted(seeds)]

    def observe(self):
        f, m, u = self.terrain, self.m, self.u
        levels = bytearray(32 * 32)
        for (x, y), cell in f.cells.items():
            levels[y * 32 + x] = u.mem_read(cell + 0x11B, 1)[0]
        records = [struct.unpack('<ii', u.mem_read(self.record(x, y) + 0x38, 8))
                   for y in range(STRIDE) for x in range(STRIDE) if in_diamond(x, y)]
        assert all(stamp == -1 for _, stamp in records), 'Split leaves every stamp at -1'
        listed = []
        for slot in range(m.read32(REGION_LIST + 0x10)):
            obj = m.read32(self.items + slot * 4)
            listed.append([struct.unpack('<i', u.mem_read(obj + off, 4))[0]
                           for off in (8, 0xC, 0x10)] + [u.mem_read(obj + 0x14, 1)[0]])
        return dict(levels_hex=bytes(levels).hex(), regions=[r for r, _ in records],
                    listed=listed, next_region_id=m.read32(NEXT_REGION_ID))

    def run(self, case, pool):
        from tools import native_oracle as native
        from tools.rmg_oracle.gen_rng_vectors import check_code
        from unicorn import UC_HOOK_CODE
        from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
        f, m, u = self.terrain, self.m, self.u
        partition = self.prepare(case)
        before = pool.refs(f.states())
        trace = native.NativeCallTrace(u, m.read32)
        splits = []

        def observe(machine, pc, _size, _data):
            if machine.reg_read(UC_X86_REG_EIP) != pc:
                return
            sp = machine.reg_read(UC_X86_REG_ESP)
            trace.returned(pc, sp)
            if pc == RNG_NEXT:
                assert machine.reg_read(UC_X86_REG_ECX) == f.rngs['mapgen']
                trace.entered(pc, sp, ('next', 0, 0))
            elif pc == SPLIT:
                splits.append(pc)
                assert len(splits) < 400, 'native restart loop did not settle'
            elif pc == GAUSSIAN:
                assert machine.reg_read(UC_X86_REG_ECX) == GAUSS_STATE
                trace.entered(pc, sp, ('gaussian', 0, 0))

        hook = u.hook_add(UC_HOOK_CODE, observe)
        f.active = True
        try:
            m.invoke(ENTRY, 0, (), instruction_limit=400_000_000, timeout_us=600_000_000,
                     context=dict(control=case['id']))
            trace.returned(native.RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
            assert not trace.pending
            check_code(u, f.code)
            after = pool.refs(f.states())
            assert all(before[name] == after[name] for name in ('main', 'scenario'))
            return dict(input=case, partition=partition, rng_before=before['mapgen'],
                        rng_after=after['mapgen'], **self.observe())
        finally:
            f.active = False
            u.hook_del(hook)


def generate():
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = SplitFixture(), StatePool()
    rows = [fixture.run(case, pool) for case in controls()]
    read = fixture.m.read32
    signed = lambda v: v - (1 << 32) if v & 0x80000000 else v
    # The bases CellClass::HasBridgeOverlay @ 0x004865D0 compares (water set,
    # shore pieces, four waterfall sets): the piece constructor's water flag.
    water = dict(water=signed(read(0xAA0738)), shore=signed(read(0xABAD28)),
                 waterfalls=[signed(read(a)) for a in (0xAA073C, 0xABB110, 0xAA1050, 0xAA10A0)])
    return dict(schema=1, scene=dict(stride=STRIDE, bounds=[LOW, HIGH],
                                     cliff_base=fixture.terrain.bases['cliff'], water_family=water),
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='8 whole original SplitOversizedRegions calls (map type 3), each split running RmgRegion::Split, SplitFrontierCost, NextGaussian and the merge/level passes',
        assumptions=[
            'Existing CliffFixture owns original-constructed Cell backing, admission and the stride-33 scratch; bounds 16..48. Scratch coordinates are stamped for in-diamond records only, regions and stamps start -1.',
            'Declared partition: regions built by the original RmgRegion constructor in id order, list positions equal ids, the water region flag written as the partition fill writes it. Not a complete Generate prefix.',
            'The linear neighbour offsets written as PathfinderClass::ResizeMapArrays 42AC00 computes them for stride 33. MapType 3, RegionSize and generated dimensions written to their globals. Gaussian cache empty with source 598000. FPCW 0E7F. All streams seeded by original 65C6D0.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS; native code bytes unchanged. No region, RNG, Gaussian or level result is supplied.'],
        entry_points=dict(split_oversized=ENTRY, split=SPLIT, frontier_cost=0x58C6F0,
                          region_ctor=REGION_CTOR, next=RNG_NEXT, gaussian=GAUSSIAN))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
