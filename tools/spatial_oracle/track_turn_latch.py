"""Original Drive/Ship Process turn sampling, latch and completion corridor.

Facing::IsRotating executes unchanged retail instructions. Only owner+18C's
completion receiver is an explicitly supplied external callback; observation
hooks never change execution. Active-dispatch rows stop at Process_Track entry.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords

LOCO, OWNER = SCRATCH + 0x1000, SCRATCH + 0x2000
VTABLE, CALLBACK = SCRATCH + 0x4000, SCRATCH + 0x5000
SP = STACK_BASE + STACK_SIZE - 0x1000  # After the native Process prologue.
UNIT_VTABLE, SAMPLER, FRAME = 0x7F5C70, 0x4C9480, 0xA8ED84
FAMILIES = {
    'drive': dict(active_gate=0x4B055A, sample=0x4B0775,
                  sampler_call=0x4B077B, common_tail=0x4B078C,
                  latch_test=0x4B0896, latch_clear=0x4B089B,
                  completion_call=0x4B08A4, guards=0x4B08AA,
                  guarded_return=0x4B08CE, after_completion=0x4B08D1,
                  track_call=0x4B0576, track_entry=0x4B0F20),
    'ship': dict(active_gate=0x69FC6A, sample=0x69FE22,
                 sampler_call=0x69FE28, common_tail=0x69FE39,
                 latch_test=0x69FF5D, latch_clear=0x69FF62,
                 completion_call=0x69FF6B, guards=0x69FF71,
                 guarded_return=0x69FF95, after_completion=0x69FF98,
                 track_call=0x69FC86, track_entry=0x6A05F0),
}
CODE_RANGES = ((0x4B055A, 0x4B08D1), (0x69FC6A, 0x69FF98),
               (SAMPLER, 0x4C94AD))
PROFILES = {
    'running': dict(rate=16, start=100, duration=4, frame=101),
    'expired': dict(rate=16, start=100, duration=4, frame=104),
    'zero_rate_running_timer': dict(rate=0, start=100, duration=4, frame=101),
    'negative_rate_running_timer': dict(rate=-1, start=100, duration=4, frame=101),
    'paused_nonzero': dict(rate=16, start=-1, duration=4, frame=101),
    'paused_zero': dict(rate=16, start=-1, duration=0, frame=101),
    'zero_duration': dict(rate=16, start=100, duration=0, frame=100),
    'future_anchor': dict(rate=16, start=100, duration=4, frame=99),
    'signed_frame_wrap': dict(rate=16, start=0x7FFFFFFF, duration=4, frame=0x80000000),
    'equal_angles_running_timer': dict(rate=16, start=100, duration=4, frame=101,
                                      desired=0),
}


def byte(u, address):
    return u.mem_read(address, 1)[0]


def read_u32(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def state(u):
    return dict(latch=byte(u, LOCO + 0x62), alive=byte(u, OWNER + 0x90),
                limbo=byte(u, OWNER + 0x81), falling=byte(u, OWNER + 0x8D))


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    originals = [bytes(u.mem_read(a, b - a)) for a, b in CODE_RANGES]
    original_vtable = bytes(u.mem_read(UNIT_VTABLE, 0x600))
    u.mem_write(VTABLE, original_vtable)
    u.mem_write(VTABLE + 0x18C, dwords(CALLBACK))
    u.mem_write(OWNER, dwords(VTABLE))
    u.mem_write(LOCO + 0xC, dwords(OWNER))
    u.mem_write(LOCO + 0x58, dwords(row.get('selector', -1)))
    u.mem_write(LOCO + 0x62, bytes((row['latch'], row.get('track_valid', 0))))
    before = row.get('initial_life', dict(alive=1, limbo=0, falling=0))
    for name, offset in (('alive', 0x90), ('limbo', 0x81), ('falling', 0x8D)):
        u.mem_write(OWNER + offset, bytes((before[name],)))
    # Owner NavCom+5A4 and current mission+AC are supplied null/zero, so an
    # inactive-track entry reaches the sampler without other world receivers.
    profile = PROFILES[row['profile']]
    facing = OWNER + 0x388
    u.mem_write(facing, struct.pack('<H', 0))
    u.mem_write(facing + 4, struct.pack('<H', profile.get('desired', 0x2000)))
    u.mem_write(facing + 8, dwords(profile['start'], 0, profile['duration']))
    u.mem_write(facing + 0x14, struct.pack('<h', profile['rate']))
    u.mem_write(FRAME, dwords(profile['frame']))

    # Declare callback mutations as x86 fixture instructions outside the image.
    # The native virtual CALL executes this body, then original guards resume.
    callback = bytearray()
    mutations = row.get('callback_after', {})
    for name, address in (('alive', OWNER + 0x90), ('limbo', OWNER + 0x81),
                          ('falling', OWNER + 0x8D), ('latch', LOCO + 0x62)):
        if name in mutations:
            callback += b'\xC6\x05' + dwords(address) + bytes((mutations[name],))
    callback += b'\xB8' + dwords(row.get('callback_return', 0xDEADBEEF)) + b'\xC2\x04\x00'
    u.mem_write(CALLBACK, bytes(callback))
    # Native Process has16 local bytes and four saved registers at this point.
    saved = (0x27182818, 0x31415926, 0x2468ACE0, 0x13579BDF)
    u.mem_write(SP, dwords(*saved))
    u.mem_write(SP + 0x20, dwords(RET_MAGIC, LOCO + 4))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_EBX, 0)
    u.reg_write(UC_X86_REG_EBP, 0xFFFFFFFF)
    u.reg_write(UC_X86_REG_ESI, LOCO + 4)
    u.reg_write(UC_X86_REG_EDI, LOCO)
    u.reg_write(UC_X86_REG_ECX, OWNER)
    initial = state(u)
    original_facing = bytes(u.mem_read(facing, 0x18))
    family = FAMILIES[row['family']]
    events, visits = [], []
    observed = set(family.values()) | {SAMPLER, CALLBACK}

    def observe(uc, address, _size, _data):
        if address in observed:
            visits.append(f'{address:08X}')
        if address == SAMPLER:
            assert uc.reg_read(UC_X86_REG_ECX) == facing
            events.append(dict(event='sample_live_turn', state=state(uc)))
        if address == family['sampler_call'] + 5:
            events.append(dict(event='sample_return', value=uc.reg_read(UC_X86_REG_EAX)))
        if address == CALLBACK:
            sp = uc.reg_read(UC_X86_REG_ESP)
            assert uc.reg_read(UC_X86_REG_ECX) == OWNER
            events.append(dict(event='completion_callback', reason=read_u32(uc, sp + 4),
                               state=state(uc), supplied_return=row.get('callback_return', 0xDEADBEEF)))
        if address == family['guards']:
            events.append(dict(event='post_callback_guards', state=state(uc)))

    def observe_write(_uc, _access, address, size, value, _data):
        if address == LOCO + 0x62:
            events.append(dict(event='latch_write', size=size, value=value))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, observe_write)
    stops = {family['common_tail']: 'common_process_tail',
             family['after_completion']: 'before_mission_precheck',
             family['track_entry']: 'active_track_dispatch',
             RET_MAGIC: 'guarded_original_return'}
    begin = family['active_gate'] if row['stage'] == 'active_gate' else family['sample']
    run_checked(u, begin, tuple(stops), count=200, required_addresses=(begin,))
    end = u.reg_read(UC_X86_REG_EIP)
    completions = [event for event in events if event['event'] == 'completion_callback']
    if completions:
        assert len(completions) == 1
        assert completions[0]['reason'] == 0
        assert completions[0]['state']['latch'] == 0
        assert events.index(completions[0]) > next(
            index for index, event in enumerate(events)
            if event['event'] == 'latch_write' and event['value'] == 0)
    if end == RET_MAGIC:
        assert f"{family['guarded_return']:08X}" in visits
        assert u.reg_read(UC_X86_REG_EAX) & 0xFF == 0
        assert u.reg_read(UC_X86_REG_ESP) == SP + 0x28
        assert tuple(u.reg_read(reg) for reg in
                     (UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_EBX)) == saved
    out = dict(input=row, profile=profile, before=initial, after=state(u), events=events,
               boundary=stops[end], boundary_address=f'{end:08X}', observed_calls=visits,
               returned_al=0 if end == RET_MAGIC else None,
               original_code_and_vtable_unchanged=True, facing_sampler_read_only=True)
    if end == family['track_entry']:
        assert events == []
        assert u.reg_read(UC_X86_REG_ECX) == LOCO
        out['track_argument'] = read_u32(u, u.reg_read(UC_X86_REG_ESP) + 4)
        assert out['track_argument'] == 0
    assert bytes(u.mem_read(facing, 0x18)) == original_facing
    assert bytes(u.mem_read(UNIT_VTABLE, 0x600)) == original_vtable
    assert all(bytes(u.mem_read(a, b - a)) == original
               for (a, b), original in zip(CODE_RANGES, originals))
    return out


def generate():
    rows = []
    for family in FAMILIES:
        for profile, latch in product(PROFILES, (0, 1)):
            rows.append(dict(family=family, stage='sampler', profile=profile, latch=latch))
        for alive, limbo, falling, latch_after in product((0, 1), repeat=4):
            rows.append(dict(family=family, stage='completion', profile='expired', latch=1,
                             callback_after=dict(alive=alive, limbo=limbo, falling=falling,
                                                 latch=latch_after),
                             callback_return=0 if latch_after == 0 else 0xDEADBEEF))
        for alive, limbo, falling in product((0, 1), repeat=3):
            rows.append(dict(family=family, stage='without_latch_guards', profile='expired',
                             latch=0, initial_life=dict(alive=alive, limbo=limbo, falling=falling)))
        for selector, valid, profile, latch in product((-1, 0, -2), (0, 1),
                                                       ('running', 'expired'), (0, 1)):
            rows.append(dict(family=family, stage='active_gate', profile=profile,
                             latch=latch, selector=selector, track_valid=valid))
    return [execute(row) for row in rows]


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    result = provenance(
        scope='Original Drive/Ship Process live Facing sampler, retained turn latch publication, reason0 completion callback order and post-callback lifecycle gates; bounded active-track dispatch precedence',
        assumptions=[
            'Interior Process frames begin at Drive4B0775/Ship69FE22 for sampler rows, or4B055A/69FC6A for active-dispatch rows. Floor/interpolation prelude and full Process invocation are excluded',
            'ESI is supplied class+4 interface; EDI is complete class. Retained latch is base+62, active byte base+63, selector base+58; head/destination remain supplied null. Owner NavCom+5A4 and mission+AC are zero, bypassing the preceding building/idle receivers on inactive-track rows',
            'Original Facing::IsRotating4C9480 executes on supplied Foot+388 rate/timer fields and binary frame. Ten profiles include running/expired, signed rate0/-1, paused timer, zero duration, future anchor, signed frame wrap and equal-angle running timer. Facing fields are verified unchanged',
            'Forty sampler rows combine each profile with old latch0/1 across both families. Thirty-two completion rows cross all alive/limbo/falling and callback-rewritten latch0/1 values. Sixteen old-latch0 rows cross lifecycle bits to show these particular guards are not reached without a completion callback',
            'Forty-eight active-gate rows cross selector-1/0/-2, valid0/1, running/expired sampler and retained latch0/1. Native active CALL executes and stops before Process_Track body; bypass rows do not establish its result or later Process re-entry',
            'Completion stops before the next owner mission precheck, live turning stops before the common Process tail, and rejected lifecycle rows execute the original RET4. These boundaries do not certify subsequent alive handling, world callbacks, speed or movement',
            'Read-only observers record latch writes, sampler results, callback input reason and lifecycle guard inputs. Original executable code and original Unit vtable bytes remain unchanged; only the external copied vtable slot is replaced',
        ],
        substitutions=[
            'Only owner+18C completion is supplied: external x86 fixture body writes explicitly listed callback_after lifecycle/latch bytes, returns a declared EAX and performs RET4. Copied Unit vtable+18C points to that body. Real completion-receiver side effects and producer reachability of supplied mutations are excluded',
            'No sampler, branch, active-track direct CALL or guard is hooked or replaced; observation hooks never alter registers, memory or control flow',
        ],
        entry_points={'facing_is_rotating': SAMPLER,
                      **{f'{name}_{key}': value for name, family in FAMILIES.items()
                         for key, value in family.items()}},
    )
    result['original_code_range_sha256'] = {
        f'{a:08X}..{b:08X}': hashlib.sha256(bytes(u.mem_read(a, b - a))).hexdigest()
        for a, b in CODE_RANGES
    }
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
