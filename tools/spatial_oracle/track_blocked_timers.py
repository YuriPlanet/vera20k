"""Original fresh Drive/Ship first-code2 timer and Find_Path continuation.

External Find_Path, zone precheck and Stop receivers are supplied at explicit
call boundaries. A running movement timer stops before the common rejected
tail; other rows execute the native code2 return. No original code is patched.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP, UC_X86_REG_FPCW,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords

LOCO, OWNER, RULES = SCRATCH + 0x1000, SCRATCH + 0x2000, SCRATCH + 0x4000
VTABLE, STUB, ZONE, STOP, OUTPUT = (SCRATCH + x for x in (0x7000, 0x8000, 0x8100, 0x8200, 0x9000))
SP = STACK_BASE + STACK_SIZE - 0x1000
FIND_PATH, FRAME, UNIT_VTABLE = 0x4D3920, 0xA8ED84, 0x7F5C70
SAVED = (0x27182818, 0x31415926, 0x2468ACE0, 0x13579BDF)
FAMILIES = {
    'drive': dict(entry=0x4B3607, waiting=0x4B3AA1, null=0x8A0790,
                  spans=((0x4B3607, 0x4B36F4), (0x4B39D1, 0x4B3A97))),
    'ship': dict(entry=0x6A2C56, waiting=0x6A30F0, null=0xB077F8,
                 spans=((0x6A2C56, 0x6A2D43), (0x6A3020, 0x6A30E6))),
}
SPANS = tuple(span for family in FAMILIES.values() for span in family['spans']) + ((0x7C5F00, 0x7C5F3D),)


def read_int(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


def return_stub(value, pop, retire=False):
    body = (b'\xC7\x05' + dwords(LOCO + 0xC, 0)) if retire else b''
    return body + b'\xB8' + dwords(value) + b'\xC2' + struct.pack('<H', pop)


def state(u):
    return dict(owner_link=read_int(u, LOCO + 0xC),
                head=list(struct.unpack('<iii', u.mem_read(LOCO + 0x40, 12))),
                valid=u.mem_read(LOCO + 0x63, 1)[0],
                blocked=u.mem_read(OWNER + 0x6B7, 1)[0],
                movement=list(struct.unpack('<iii', u.mem_read(OWNER + 0x640, 12))),
                blocked_timer=list(struct.unpack('<iii', u.mem_read(OWNER + 0x668, 12))),
                output=u.mem_read(OUTPUT, 1)[0])


class Fixture:
    def __init__(self):
        self.u = Uc(UC_ARCH_X86, UC_MODE_32)
        load_image(self.u)
        self.u.mem_map(STACK_BASE, STACK_SIZE)
        self.u.mem_map(SCRATCH, 0x10000)
        self.u.mem_map(RET_MAGIC, 0x1000)
        self.original = [bytes(self.u.mem_read(a, b-a)) for a, b in SPANS]
        self.vtable = bytes(self.u.mem_read(UNIT_VTABLE, 0x600))

    def execute(self, row):
        u = self.u
        family = FAMILIES[row['family']]
        u.mem_write(SCRATCH, bytes(0x10000))
        u.mem_write(VTABLE, self.vtable)
        u.mem_write(VTABLE + 0x2CC, dwords(ZONE))
        u.mem_write(VTABLE + 0x480, dwords(STOP))
        u.mem_write(OWNER, dwords(VTABLE))
        u.mem_write(LOCO + 0xC, dwords(OWNER))
        u.mem_write(LOCO + 0x34, dwords(*row['destination']))
        null = bytes(u.mem_read(family['null'], 12))
        u.mem_write(LOCO + 0x40, null if row['null_head'] else dwords(2688, 2432, 48))
        u.mem_write(LOCO + 0x63, b'\x01')
        u.mem_write(OWNER + 0x6B7, bytes((row['blocked'],)))
        u.mem_write(OWNER + 0x640, dwords(row['movement'][0], 0x11223344, row['movement'][1]))
        u.mem_write(OWNER + 0x668, dwords(row['blocked_timer'][0], 0x55667788, row['blocked_timer'][1]))
        u.mem_write(0x8871E0, dwords(RULES))
        u.mem_write(RULES + 0x1768, dwords(row['blockage_delay']))
        u.mem_write(RULES + 0x1760, struct.pack('<d', row['path_delay']))
        u.mem_write(ZONE, return_stub(row['zone_result'], 4))
        u.mem_write(STOP, return_stub(0, 8))
        calls = []
        for frame in row['frames']:
            # Native interior frame: four saves,0x4C locals,RET12 and threeargs.
            u.mem_write(SP, bytes(0x90))
            u.mem_write(SP, dwords(*SAVED))
            u.mem_write(SP + 0x18, dwords(2))
            u.mem_write(SP + 0x54, dwords(row['timer_middle_word']))
            u.mem_write(SP + 0x5C, dwords(RET_MAGIC, OUTPUT, 1, 0))
            u.mem_write(FRAME, dwords(frame))
            for register, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBP, LOCO),
                                    (UC_X86_REG_EBX, 0), (UC_X86_REG_ESI, 0),
                                    (UC_X86_REG_EDI, 0), (UC_X86_REG_FPCW, 0x0E7F)):
                u.reg_write(register, value)
            before = state(u)
            events = []
            end = run_checked(u, family['entry'], (FIND_PATH, family['waiting']), count=150)
            if end == FIND_PATH:
                sp = u.reg_read(UC_X86_REG_ESP)
                assert u.reg_read(UC_X86_REG_ECX) == OWNER
                return_address = read_int(u, sp)
                packed, append, urgency = struct.unpack('<III', u.mem_read(sp + 4, 12))
                events.append(dict(event='find_path', cell=list(struct.unpack('<hh', dwords(packed))),
                                   append=append, urgency=urgency, state=state(u)))
                u.mem_write(STUB, return_stub(row['path_result'], 12, row['retire']))
                run_checked(u, STUB, return_address, count=10)
                end = run_checked(u, return_address, (ZONE, STOP, RET_MAGIC), count=150)
                if end == ZONE:
                    sp = u.reg_read(UC_X86_REG_ESP)
                    assert u.reg_read(UC_X86_REG_ECX) == OWNER
                    assert read_int(u, sp + 4) == LOCO + 0x34
                    events.append(dict(event='zone', state=state(u)))
                    target = read_int(u, sp)
                    run_checked(u, ZONE, target, count=10)
                    end = run_checked(u, target, (STOP, RET_MAGIC), count=150)
                if end == STOP:
                    sp = u.reg_read(UC_X86_REG_ESP)
                    assert u.reg_read(UC_X86_REG_ECX) == OWNER
                    assert list(struct.unpack('<II', u.mem_read(sp + 4, 8))) == [0, 1]
                    events.append(dict(event='stop', state=state(u)))
                    target = read_int(u, sp)
                    run_checked(u, STOP, target, count=10)
                    end = run_checked(u, target, RET_MAGIC, count=30)
            if end == RET_MAGIC:
                assert u.reg_read(UC_X86_REG_ESP) == SP + 0x6C
                assert tuple(u.reg_read(r) for r in
                             (UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_EBX)) == SAVED
            assert end in (RET_MAGIC, family['waiting'])
            calls.append(dict(frame=frame, before=before, events=events, after=state(u),
                              boundary='return' if end == RET_MAGIC else 'waiting_common_tail',
                              returned_al=(u.reg_read(UC_X86_REG_EAX) & 255) if end == RET_MAGIC else None))
            if row['retire'] and end == RET_MAGIC:
                break
        assert self.original == [bytes(u.mem_read(a, b-a)) for a, b in SPANS]
        assert self.vtable == bytes(u.mem_read(UNIT_VTABLE, 0x600))
        return dict(input=row, calls=calls, original_code_and_vtable_unchanged=True)


def inputs():
    timings = [
        dict(name='arm_once_wait_expire', frames=[100, 100, 101, 104, 105, 109, 113, 113, 114], movement=[100, 4], blocked_timer=[-1, 123], blocked=0),
        dict(name='grace_boundary', frames=[100, 100, 101, 104, 105, 109, 109], movement=[100, 0], blocked_timer=[100, 4], blocked=1),
        dict(name='paused_movement', frames=[100, 100, 1000], movement=[-1, 7], blocked_timer=[100, 4], blocked=1),
        dict(name='paused_grace', frames=[100, 100, 1000], movement=[-1, 0], blocked_timer=[-1, 7], blocked=1),
        dict(name='signed_wrap', frames=[2147483647, -2147483648, -2147483647], movement=[2147483646, 3], blocked_timer=[2147483646, 4], blocked=1),
        dict(name='negative_elapsed', frames=[-2147483648], movement=[0, 6], blocked_timer=[0, 6], blocked=1),
        dict(name='negative_duration', frames=[100], movement=[-1, -7], blocked_timer=[-1, -2], blocked=1),
        dict(name='expired', frames=[100, 100], movement=[90, 4], blocked_timer=[90, 4], blocked=1),
    ]
    for family, timing, null_head, result in product(FAMILIES, timings, (False, True),
            ((1, 1, False), (0, 1, False), (0, 0, False), (0, 1, True), (1, 1, True))):
        path_result, zone_result, retire = result
        yield dict(family=family, **timing, null_head=null_head, path_result=path_result,
                   zone_result=zone_result, retire=retire, blockage_delay=5,
                   path_delay=0.01, destination=[-257, 513, 48], timer_middle_word=0)
    # SP+54 is an explicit interior input copied into the timer's middle word;
    # this witness does not establish its whole-Process producer or invariance.
    for family, timing in product(FAMILIES, (timings[0], timings[-1])):
        yield dict(family=family, **timing, null_head=False, path_result=1,
                   zone_result=1, retire=False, blockage_delay=5,
                   path_delay=0.01, destination=[-257, 513, 48], timer_middle_word=0x2468ACE0)


def generate():
    fixture = Fixture()
    return dict(schema_version=1, rows=[fixture.execute(row) for row in inputs()])


def metadata():
    fixture = Fixture()
    result = provenance(
        scope='Original Drive/Ship fresh first-code2 headclear, timer arms/expiry, FindPath urgency and post-call continuation',
        assumptions=['Interior first-code2 entry with supplied frame, retained head/valid, Foot timers and blocked latch; current owner link and native-shaped stack',
            'timer_middle_word explicitly supplies SP+54 on each interior invocation; original stores copy this word to timer+4. Zero is not established as a retail invariant; nonzero copy cases are included',
            'Repeated calls preserve all owner/locomotor fields and explicitly vary binary frame; stopped waiting common-tail effects are excluded',
            'Original headclear only writes valid0 when head differs from native null coordinate; supplied null-head/valid1 inputs test that distinction without claiming retail reachability',
            'Rules blockage delay5 and PathDelay0.01 supplied; original multiply900 and ftol run with PC53/chop FPCW0E7F',
            'Negative-duration rows stop at the movement wait and do not cover a negative blocked duration query; this is not exhaustive signed input coverage',
            'FindPath/zone/Stop bodies are not executed; explicit callback inputs validate remaining caller branches, output-byte writes and timer publication only',
            'No full fresh retry, common rejection tail, world CanEnter, path search, Stop lifecycle or complete locomotor parity claimed'],
        substitutions=['FindPath call boundary uses external scratch RET12 with supplied AL and optional owner-link retirement',
            'Cloned Unit vtable replaces only +2CC zone and +480 Stop with supplied external RET4/RET8; original table unchanged'],
        entry_points={f'{name}_{key}': value for name, family in FAMILIES.items()
                      for key, value in family.items() if key != 'spans'})
    result['original_code'] = [dict(start=f'{a:08X}', end_exclusive=f'{b:08X}',
        hex=bytes(fixture.u.mem_read(a, b-a)).hex(), sha256=hashlib.sha256(bytes(fixture.u.mem_read(a, b-a))).hexdigest()) for a, b in SPANS]
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
