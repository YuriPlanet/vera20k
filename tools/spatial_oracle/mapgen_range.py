"""Candidate original598030/65C780 range evidence; native output only.

Run from the repository with PYTHONPATH=. and the configured original gamemd.
Default is read-only --check; --write deliberately creates the external corpus.
No native code or returned call values are patched. Supplied retained RNG data
and alternate ambient control words are explicit adversarial inputs, not claims
that an active retail seed or Windows callback necessarily produces them.
"""
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP,
    UC_X86_REG_FPCW, UC_X86_REG_FPSW,
)
from tools.native_oracle import (
    load_image, run_checked, finish_vectors, provenance,
    STACK_BASE, STACK_SIZE, RET_MAGIC,
)
from tools.rmg_oracle.gen_rng_vectors import seeded_struct, STRUCT_LEN

RANGE, NEXT, RNG, CACHED_CW = 0x598030, 0x65C780, 0xABE890, 0x822D80
SP = STACK_BASE + STACK_SIZE - 0x1000
SPANS = ((0x598030, 0x598090), (0x65C780, 0x65C7D1),
         (0x7C5EE4, 0x7C5F3D), (0x7C8F55, 0x7C8F5D),
         (0x7CEAAF, 0x7CEAC1), (0x7CBF14, 0x7CBF5F),
         (0x7CBF8A, 0x7CC0A5), (0x6BBFB7, 0x6BBFCE))


def dw(*values):
    return struct.pack('<' + 'I' * len(values), *(x & 0xFFFFFFFF for x in values))


def read32(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def retained(*raw, disabled=0, indices=(0, 103), padding=(0xA1, 0xB2, 0xC3)):
    """Supply full250-word data; actual Next produces these disjoint XOR words.

    The two cursor runs may wrap but are deliberately nonoverlapping for the
    short requested sequence. This is a data fixture, never a Next callback.
    """
    state = bytearray(STRUCT_LEN)
    state[:4] = bytes((disabled, *padding))
    struct.pack_into('<ii', state, 4, *indices)
    used = set()
    for i, value in enumerate(raw):
        a, b = ((indices[0] + i) % 250, (indices[1] + i) % 250)
        assert a != b and a not in used and b not in used
        used.update((a, b))
        struct.pack_into('<I', state, 12 + a * 4, value)
    return bytes(state)


class Machine:
    def __init__(self):
        self.u = u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(u)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(RET_MAGIC, 0x1000)
        self.original = [bytes(u.mem_read(a, b-a)) for a, b in SPANS]
        self.active = None
        self.next_count = 0
        self.bootstrap = self.startup()
        u.hook_add(UC_HOOK_CODE, self.observe)
        u.hook_add(UC_HOOK_MEM_WRITE, self.write)

    def startup(self):
        u = self.u
        image_cached_fpcw = read32(u, CACHED_CW)
        # CPU-reset-style control is the only supplied startup FPCW. Execute
        # actual CRT precision tail, excluding the preceding OS feature probe.
        u.reg_write(UC_X86_REG_FPCW, 0x037F)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.mem_write(SP, dw(RET_MAGIC))
        run_checked(u, 0x7C8F55, RET_MAGIC, count=10000,
                    required_addresses=(0x7CEAAF, 0x7CBF49, 0x7CBF14, 0x7CBF41))
        crt = u.reg_read(UC_X86_REG_FPCW)
        u.mem_write(CACHED_CW, dw(0))  # prove the original cache writer executes
        u.reg_write(UC_X86_REG_ESP, SP)
        run_checked(u, 0x6BBFB7, 0x6BBFCE, count=10000,
                    required_addresses=(0x7CBF49, 0x7C5EE4, 0x7C5EF9))
        return dict(image_cached_fpcw=image_cached_fpcw, supplied_initial_fpcw=0x037F, after_crt_fpcw=crt,
                    after_winmain_fpcw=u.reg_read(UC_X86_REG_FPCW),
                    cached_fpcw=read32(u, CACHED_CW),
                    windows_and_cpu_probe_executed=False)

    def state(self):
        return bytes(self.u.mem_read(RNG, STRUCT_LEN))

    def control(self):
        return dict(fpcw=self.u.reg_read(UC_X86_REG_FPCW),
                    cached_fpcw=read32(self.u, CACHED_CW),
                    fpsw=self.u.reg_read(UC_X86_REG_FPSW))

    def write(self, u, access, address, size, value, _):
        if self.active is None:
            return
        # Native execution is read-only with respect to all original code.
        assert not any(address < b and address + size > a for a, b in SPANS)
        if RNG <= address < RNG + STRUCT_LEN:
            self.active['state_writes'].append(dict(offset=address-RNG, size=size,
                                                    value=value & ((1 << (8 * size)) - 1)))
        elif not STACK_BASE <= address < STACK_BASE + STACK_SIZE:
            raise AssertionError(f'unexpected range-helper nonstack write {address:08X}')

    def observe(self, u, address, _, __):
        if self.active is None:
            return
        if address == RANGE:
            self.active['range_entry_count'] += 1
        elif address == NEXT:
            assert u.reg_read(UC_X86_REG_ECX) == RNG
            self.next_count += 1
            self.active['raw_draw_count'] += 1
            assert self.active['raw_draw_count'] <= 16, 'bounded retry exceeded'
        elif address == 0x598068:
            self.active['raw_draws'].append(dict(value=u.reg_read(UC_X86_REG_EAX),
                index_a=read32(u, RNG+4), index_b=read32(u, RNG+8),
                arithmetic_fpcw=u.reg_read(UC_X86_REG_FPCW)))
        elif address == 0x598087:
            result = u.reg_read(UC_X86_REG_EAX) & 0xFFFFFFFF
            self.active['candidates'].append(dict(value=result,
                accepted=result <= self.active['high'],
                fpcw_after_ftol=u.reg_read(UC_X86_REG_FPCW)))

    def advance(self, count):
        u = self.u
        for _ in range(count):
            u.reg_write(UC_X86_REG_ESP, SP)
            u.mem_write(SP, dw(RET_MAGIC))
            u.reg_write(UC_X86_REG_ECX, RNG)
            run_checked(u, NEXT, RET_MAGIC, count=1000, required_addresses=(NEXT,))

    def request(self, low, high):
        u = self.u
        before = self.state()
        record = dict(low=low, high=high, before_state_hex=before.hex(),
                      before_control=self.control(), range_entry_count=0,
                      raw_draw_count=0, raw_draws=[], candidates=[], state_writes=[])
        u.reg_write(UC_X86_REG_ESP, SP)
        u.mem_write(SP, dw(RET_MAGIC))
        u.reg_write(UC_X86_REG_ECX, low)
        u.reg_write(UC_X86_REG_EDX, high)
        self.active = record
        run_checked(u, RANGE, RET_MAGIC, count=20000,
                    required_addresses=(RANGE, NEXT, 0x598063, 0x598078,
                                        0x7C5F00, 0x598087, 0x598089))
        self.active = None
        assert u.reg_read(UC_X86_REG_ESP) == SP + 4
        assert (u.reg_read(UC_X86_REG_FPSW) >> 11) & 7 == 0
        assert record['range_entry_count'] == 1
        assert len(record['raw_draws']) == record['raw_draw_count']
        assert len(record['candidates']) == record['raw_draw_count']
        assert record['candidates'][-1]['accepted']
        assert all(not row['accepted'] for row in record['candidates'][:-1])
        record.update(result=u.reg_read(UC_X86_REG_EAX) & 0xFFFFFFFF,
                      state_advance_count=sum(w['offset'] >= 12 for w in record['state_writes']),
                      after_state_hex=self.state().hex(), after_control=self.control())
        assert [bytes(u.mem_read(a, b-a)) for a, b in SPANS] == self.original
        return record


def inputs():
    for seed in (0, 1, 1234, 0x7FFF, 0xFFFF):
        yield dict(name=f'seed_{seed}_repair_three', seed=seed, requests=[[0, 3]] * 3)
    for prefix in (146, 249, 250):
        yield dict(name=f'seed0_after_{prefix}_raw_draws', seed=0,
                   advance_raw=prefix, requests=[[0, 3]] * 3)
    boundaries = (0, 1, 0x3FFFFFFE, 0x3FFFFFFF, 0x40000000, 0x40000001,
                  0x7FFFFFFF, 0x80000000, 0xBFFFFFFF, 0xC0000000,
                  0xFFFFFFFE, 0xFFFFFFFF)
    for raw in boundaries:
        yield dict(name=f'repair_raw_{raw:08x}', raw_words=[raw, 0, 0], requests=[[0, 3]])
    for byte in (1, 255):
        yield dict(name=f'disabled_{byte}', disabled=byte, raw_words=[0xFFFFFFFF, 0],
                   requests=[[0, 3], [7, 7]])
    for indices in ((249, 102), (146, 249)):
        yield dict(name=f'retained_wrap_{indices[0]}_{indices[1]}', indices=list(indices),
                   raw_words=[0xFFFFFFFF, 0x80000000, 0], requests=[[0, 3]] * 3)
    for ambient in (0x0E7F, 0x027F, 0x037F, 0x007F, 0x0C7F, 0x0A7F):
        for raw in (0x7FFFFFFF, 0x80000000, 0xFFFFFFFE, 0xFFFFFFFF):
            yield dict(name=f'ambient_{ambient:04x}_raw_{raw:08x}', ambient_fpcw=ambient,
                       raw_words=[raw, 0, 0, 0], requests=[[0, 3], [0, 3]])
    ranges = ((0, 0), (7, 7), (0, 5), (1, 6), (0, 0x7FFFFFFF),
              (0, 0xFFFFFFFE), (0, 0xFFFFFFFF),
              (0x80000000, 0xFFFFFFFF), (0xFFFFFFFC, 0xFFFFFFFF))
    for low, high in ranges:
        for raw in (0, 1, 0x7FFFFFFF, 0xFFFFFFFF):
            yield dict(name=f'range_{low:08x}_{high:08x}_raw_{raw:08x}',
                       raw_words=[raw, 0, 0], requests=[[low, high]])
    yield dict(name='reversed_10_5_first_zero_then_max',
               raw_words=[0, 0xFFFFFFFF, 0], requests=[[10, 5]])
    # Wide valid spans expose intermediate PC53 rounding at a source-integer
    # boundary. Center values are multiplicative inverses modulo2^32-1;
    # neighbors remain ordinary supplied data, never expected native answers.
    for span, center in ((0x01000003, 0x897CD663), (0xFFFFFFFE, 0xFFFFFFFE)):
        for delta in (-1, 0, 1):
            raw = center + delta
            yield dict(name=f'wide_span_{span:08x}_raw_{raw:08x}',
                       raw_words=[raw, 0, 0], requests=[[0, span-1]])


def execute(case):
    machine = Machine()
    if 'seed' in case:
        initial = seeded_struct(case['seed'])
        source = 'original65C6D0_seeded_state'
    else:
        initial = retained(*case['raw_words'], disabled=case.get('disabled', 0),
                           indices=case.get('indices', (0, 103)))
        source = 'supplied_complete_retained_state_not_seed_reachability'
    machine.u.mem_write(RNG, initial)
    machine.advance(case.get('advance_raw', 0))
    if 'ambient_fpcw' in case:
        machine.u.reg_write(UC_X86_REG_FPCW, case['ambient_fpcw'])
    calls = [machine.request(low, high) for low, high in case['requests']]
    return dict(input=case, state_source=source, bootstrap=machine.bootstrap,
                initial_state_hex=initial.hex(), calls=calls,
                range_request_count=len(calls), raw_draw_count=machine.next_count,
                state_advance_count=sum(c['state_advance_count'] for c in calls),
                explicit_prefix_next_call_count=case.get('advance_raw', 0))


def startup_contract():
    u = Machine().u
    sites = ((0x7CD8B4, 0x7CBDAF), (0x7CD8EA, 0x6BB9A0),
             (0x7C8F55, 0x7CEAAF), (0x7CEAB9, 0x7CBF49),
             (0x6BBFC1, 0x7CBF49), (0x6BBFC9, 0x7C5EE4))
    calls = []
    for address, target in sites:
        code = bytes(u.mem_read(address, 5))
        assert code[0] == 0xE8
        actual = address + 5 + struct.unpack('<i', code[1:])[0]
        assert actual == target
        calls.append(dict(address=address, target=target, bytes=code.hex()))
    assert read32(u, 0x87BEB8) == 0x7C8F46
    return dict(original_direct_calls=calls, crt_initializer_pointer=read32(u, 0x87BEB8),
                dispatch_bytes=bytes(u.mem_read(0x7CBDAF, 11)).hex(),
                scope='Caller bytes establish CRT-then-WinMain order. Only precision tail and WinMain rounding block execute; OS/startup between them is excluded.')


def generate():
    rows = [execute(case) for case in inputs()]
    return dict(schema=1, cases=rows, startup_contract=startup_contract(), scope='Original native range outputs; no Rust-calculated goldens',
                range_request_count=sum(row['range_request_count'] for row in rows),
                raw_draw_count=sum(row['raw_draw_count'] for row in rows),
                state_advance_count=sum(row['state_advance_count'] for row in rows),
                explicit_prefix_next_call_count=sum(row['explicit_prefix_next_call_count'] for row in rows),
                raw_draw_count_excludes_seed_and_explicit_prefix_advance=True)


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original598030 with actual65C780,7C5F00, retained full0x3F4 MapGen state and ordered raw draws; low repair0..3 and declared boundary/ambient/range controls. No full game or Rust execution.',
        assumptions=[
            'Original CRT precision tail7C8F55 starts after its OS CPU-feature query; original7CEAAF/_controlfp executes. WinMain6BBFB7..6BBFCE then selects chop and captures the resulting word. Initial reset-style FPCW037F and valid stack are supplied; not full Windows startup.',
            'Unicorn reads0E3F after original control conversion; legacy0E7F differs only by reserved bit6. Both are explicit cases. No claim that all external runtime callbacks preserve the startup word.',
            'Seeded rows execute original65C6D0. Adversarial rows supply all250 retained words, disabled byte, padding and indices; they are not claimed reachable from a retail seed.',
            'Each range request retains complete before/after state and control; raw request/draw counts exclude the separately declared prefix advance. The native helper and Math_ftol determine rounding and retries.',
            'Alternate ambient FPCW rows retain the native-produced cached chop word. They test a supplied disturbance before the first request, not an observed retail callback.',
            'Bounded emulator execution; no hardware capture, audio, timer, renderer, map loading, repair walker or object lifecycle executed.',
        ],
        substitutions=['None: hooks only observe or reject unexpected writes; no original instructions, RNG, range result or conversion callback replaced.'],
        entry_points={'crt_precision_tail': 0x7C8F55, 'crt_precision': 0x7CEAAF,
                      'winmain_rounding_block': 0x6BBFB7, 'cache_control': 0x7C5EE4,
                      'seed': 0x65C6D0, 'range': RANGE, 'next': NEXT, 'ftol': 0x7C5F00}))
