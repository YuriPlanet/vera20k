"""Original CarveRiver branch/section controls over the existing Cell/TMP VM.

`python -m tools.rmg_oracle.river_branch --check` replays the bounded component;
`--write` deliberately replaces the Rust-consumed corpus. Original code computes
all section, branch, RNG and cleanup results. Supplied seeded entry state is not
a complete Generate prefix or a claim about a whole map's native seed identity.
"""
from pathlib import Path
import math
import struct

ENTRY = 0x59D510
SECTION_INPUT, SECTION_CELL, SECTION_END = 0x59DAAF, 0x59DAD9, 0x59DC7D
BRANCH_CALL, RNG_NEXT, GAUSSIAN = 0x59DF9D, 0x65C780, 0x5980C0


def controls():
    # First eight branch witnesses in an unmodified native seed0..68 search.
    rows = [dict(id=f'natural_seed_{seed}', seed=seed)
            for seed in (2, 6, 20, 22, 50, 52, 55, 68)]
    rows += [dict(id=f'no_branch_seed_{seed}', seed=seed) for seed in (1, 42)]
    rows += [dict(id=name, seed=2, heading=angle) for name, angle in (
        ('heading_quarter_turn', math.pi / 2), ('heading_half_turn', math.pi),
        ('heading_negative_quarter', -math.pi / 2))]
    rows += [dict(id='narrow_section', seed=2, water_percent=30),
             dict(id='wide_section', seed=2, water_percent=100),
             dict(id='descendant_cannot_branch', seed=2, is_branch=True)]
    return [dict(start=[16, 16], heading=0.0, water_percent=60,
                 region=7, base_level=0, is_branch=False) | row for row in rows]


def bits(u, address):
    return f'{struct.unpack("<Q", u.mem_read(address, 8))[0]:016x}'


class RiverFixture:
    def __init__(self):
        from tools.rmg_oracle.cliff_materialization import CliffFixture
        self.terrain = CliffFixture()
        self.m, self.u = self.terrain.m, self.terrain.u
        self.actor = self.m.alloc(0x400)
        self.start = self.m.alloc(4)
        self.water = self.m.read32(0xAA0738)

    def fields(self, xy):
        f, m, u = self.terrain, self.m, self.u
        cell = f.cells[xy]
        return [m.read32(cell + 0x38), *u.mem_read(cell + 0x11A, 3),
                m.read32(f.region_at(xy)), u.mem_read(f.region_at(xy) + 0x13, 1)[0]]

    def run(self, case, pool):
        from tools import native_oracle as native
        from tools.rmg_oracle.cliff_materialization import (
            TRANSPORT_RETURNS, TRANSPORT_REDIRECTS, words)
        from tools.rmg_oracle.gen_rng_vectors import check_code
        from tools.rmg_oracle.gen_x87_vectors import control_block, CTRL_BLOCK
        from unicorn import UC_HOOK_CODE
        from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX,
                                      UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW)
        f, m, u = self.terrain, self.m, self.u
        f.reset(dict(kind='river', seed=case['seed']))
        u.mem_write(self.actor, bytes(0x400))
        for offset, value in ((0x4C, case['water_percent']), (0x308, case['region']),
                              (0x30C, case['base_level'])):
            u.mem_write(self.actor + offset, words(value))
        # Generator+310 remains0: no waterfall attempt in these controls.
        u.mem_write(self.start, struct.pack('<hh', *case['start']))
        u.mem_write(CTRL_BLOCK, control_block())
        common = [0, 0, 0, 0, 0, 0]
        assert all(self.fields(xy) == common for xy in f.cells)
        before = pool.refs(f.states())
        trace = native.NativeCallTrace(u, m.read32)
        kernels, branches, transports = [], [], []
        active = None

        def observe(machine, pc, _size, _data):
            nonlocal active
            if machine.reg_read(UC_X86_REG_EIP) != pc:
                assert pc in TRANSPORT_RETURNS | TRANSPORT_REDIRECTS, f'Nontransport redirect {pc:08X}'
                transports.append(hex(pc))
                return
            sp = machine.reg_read(UC_X86_REG_ESP)
            trace.returned(pc, sp)
            if pc == ENTRY:
                assert machine.reg_read(UC_X86_REG_ECX) == self.actor
                trace.entered(pc, sp, ('carve', 4, 16))
            elif pc == RNG_NEXT:
                assert machine.reg_read(UC_X86_REG_ECX) == f.rngs['mapgen']
                trace.entered(pc, sp, ('next', 0, 0))
            elif pc == GAUSSIAN:
                trace.entered(pc, sp, ('gaussian', 0, 0))
            elif pc == SECTION_INPUT:
                assert active is None
                active = dict(input=dict(
                    centre=[bits(u, sp + 0xB0), bits(u, sp + 0xC0)],
                    sin=bits(u, sp + 0x40), cos=bits(u, sp + 0x80),
                    span=machine.reg_read(UC_X86_REG_EAX)), cells=[])
            elif pc == SECTION_CELL:
                # Both ftol calls have completed; read the actual signed words.
                from unicorn.x86_const import UC_X86_REG_EDI
                x = machine.reg_read(UC_X86_REG_EDI) & 0xFFFF
                y = machine.reg_read(UC_X86_REG_EAX) & 0xFFFF
                active['cells'].append([x - 65536 if x & 32768 else x,
                                        y - 65536 if y & 32768 else y])
            elif pc == SECTION_END:
                assert active and len(active['cells']) == active['input']['span']
                active.update(after=[bits(u, sp + 0x68), bits(u, sp + 0x30)],
                              column_straight=bool(u.mem_read(sp + 0x13, 1)[0]),
                              row_straight=bool(u.mem_read(sp + 0x10, 1)[0]))
                kernels.append(active)
                active = None
            elif pc == BRANCH_CALL:
                branches.append(dict(section=len(kernels) - 1,
                    start=list(struct.unpack('<hh', u.mem_read(m.read32(sp), 4))),
                    heading=bits(u, sp + 4)))

        hook = u.hook_add(UC_HOOK_CODE, observe)
        f.active = True
        try:
            lo, hi = struct.unpack('<2I', struct.pack('<d', case['heading']))
            result = m.invoke(ENTRY, self.actor,
                (self.start, lo, hi, int(case['is_branch'])),
                instruction_limit=8_000_000, timeout_us=60_000_000,
                context=dict(control=case['id']))
            trace.returned(native.RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
            assert not trace.pending and active is None
            assert all('return_eax' in call for call in trace.calls)
            check_code(u, f.code)
            after = pool.refs(f.states())
            assert all(before[name] == after[name] for name in ('main', 'scenario'))
            changed = [dict(coords=list(xy), fields=self.fields(xy))
                       for xy in sorted(f.cells) if self.fields(xy) != common]
            return dict(input=case, result=result & 255, sections=kernels,
                branches=branches, raw_draws=[c['return_eax'] for c in trace.calls if c['name'] == 'next'],
                gaussian_calls=sum(c['name'] == 'gaussian' for c in trace.calls),
                rng_before=before, rng_after=after, changed_cells=changed,
                placed=m.read32(self.actor + 0x304), region=m.read32(self.actor + 0x308),
                base_level=m.read32(self.actor + 0x30C),
                fpcw=u.reg_read(UC_X86_REG_FPCW), transports=transports)
        finally:
            f.active = False
            u.hook_del(hook)


def generate():
    from tools import native_oracle as native
    from tools.rmg_oracle.cliff_materialization import StatePool
    fixture, pool = RiverFixture(), StatePool()
    rows = [fixture.run(case, pool) for case in controls()]
    return dict(schema=1, native_sha256=native.image_sha256(),
                scene=dict(stride=33, bounds=[16, 48], map_size=[16, 16],
                           common=[0, 0, 0, 0, 0, 0], water_base=fixture.water),
                rng_states_hex=pool.states, rows=rows)


def provenance():
    from tools import native_oracle as native
    return native.provenance(
        scope='16 full original CarveRiver component calls, including original cross-section increments, branch arguments, nested recursion, RNG and cleanup',
        assumptions=[
            'Existing CliffFixture owns original-constructed Cell/TMP/theater/scratch preparation and native admission/lookup membership. Flat declared32x32 backing, MapSize16x16, scratch stride33, bounds16..48; component input rather than native Generate allocation history.',
            'Generator+4C WaterAmount, +304 placed0, +308 region7, +30C base0 and +310 false; explicit start16,16, heading and branch flag. Real original65C6D0 seeds all streams. These are declared CarveRiver entries, not a complete Generate RNG prefix.',
            'Original table trig, ftol, Gaussian and Random Next execute. Gaussian cache is empty and its callback is598000. FPCW0E7F is the existing RMG component prerequisite. FYL2X follows Unicorn softfloat and has the existing physical-x87 low-bit caveat.',
            'Section inputs/updated doubles/ftol cells, live branch arguments, ordered raw MapGen words and complete stream before/after bytes are observed. Scenario/Main streams remain unchanged. No simulation timers/detach calls belong to this pre-play generation chain.',
            'Waterfalls are disabled and base0 excludes the canyon gate. Results establish this bounded branch/section/rollback coverage, not whole Generate, renderer or arbitrary map seed equivalence.'],
        substitutions=[
            'Existing PaletteReader CRT allocation/free/TLS and immutable physical file transport during preparation; every hooked return/redirect during controls is admitted by the existing transport allowlist. Native code bytes remain unchanged. No river, Map/Cell, trig, Gaussian or RNG result is supplied.'],
        entry_points=dict(carve=ENTRY, section_input=SECTION_INPUT,
                          section_end=SECTION_END, branch_call=BRANCH_CALL,
                          next=RNG_NEXT, gaussian=GAUSSIAN,
                          sine=0x4CACB0, cosine=0x4CAD00, ftol=0x7C5F00))


if __name__ == '__main__':
    from tools.native_oracle import finish_vectors
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=provenance)
