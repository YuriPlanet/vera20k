"""Original outer Drive/Ship Process dispatch with explicit external callees.

The outer CALLs, output-byte/alive branches, early destination predicates,
FootStop, GetCell and live Facing sampler execute original retail bytes. Fresh
and TrackProcess bodies are supplied fixture returns at their call boundaries;
they are not part of this oracle's native claim. No original code is patched.

Reproduce with VERA20K_GAMEMD_EXE pointing to the pinned retail image:
    python -m tools.spatial_oracle.track_outer_continuation --check
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords

LOCO, OWNER, CELL = SCRATCH + 0x1000, SCRATCH + 0x2000, SCRATCH + 0x3000
VTABLE, INTERFACE_VTABLE, CELL_VTABLE = SCRATCH + 0x4000, SCRATCH + 0x4800, SCRATCH + 0x4900
DESTINATION, IDLE, IS_MOVING, CELL_RTTI = (SCRATCH + x for x in (0x5000, 0x5100, 0x5200, 0x5300))
EXTERNAL_FRESH, EXTERNAL_TRACK = SCRATCH + 0x6000, SCRATCH + 0x6200
SP = STACK_BASE + STACK_SIZE - 0x1000
UNIT_VTABLE, FOOT_STOP, GET_CELL, SAMPLER = 0x7F5C70, 0x4DF0D0, 0x41BEA0, 0x4C9480
FRAME = 0xA8ED84
SAVED = (0x27182818, 0x31415926, 0x2468ACE0, 0x13579BDF)
FAMILIES = {
    'drive': dict(active_gate=0x4B055A, early=0x4B066C, sample=0x4B0775,
                  common_tail=0x4B078C, latch_test=0x4B0896,
                  ordinary=0x4B0A6B, fresh=0x4B2630, track=0x4B0F20,
                  first_track_return=0x4B057B, null_coord=0x8A0790),
    'ship': dict(active_gate=0x69FC6A, early=0x69FD13, sample=0x69FE22,
                 common_tail=0x69FE39, latch_test=0x69FF5D,
                 ordinary=0x6A0134, fresh=0x6A1C80, track=0x6A05F0,
                 first_track_return=0x69FC8B, null_coord=0xB077F8),
}
CODE_RANGES = ((0x4B0500, 0x4B0ACE), (0x69FC10, 0x6A0193),
               (FOOT_STOP, 0x4DF0DF), (GET_CELL, 0x41BEDD),
               (SAMPLER, 0x4C94AD), (0x746E20, 0x746E26))


def read_u32(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def byte(u, address):
    return u.mem_read(address, 1)[0]


def byte_write(address, value):
    return b'\xC6\x05' + dwords(address) + bytes((value,))


def dword_write(address, value):
    return b'\xC7\x05' + dwords(address, value)


def return_body(value, pop=0):
    return b'\xB8' + dwords(value) + (b'\xC2' + struct.pack('<H', pop) if pop else b'\xC3')


def state(u):
    return dict(owner_link=read_u32(u, LOCO + 0xC),
                alive=byte(u, OWNER + 0x90), limbo=byte(u, OWNER + 0x81),
                falling=byte(u, OWNER + 0x8D), navcom=read_u32(u, OWNER + 0x5A4),
                navcom_aux=read_u32(u, OWNER + 0x5A0),
                queue_count=read_u32(u, OWNER + 0x598),
                selector=read_u32(u, LOCO + 0x58), valid=byte(u, LOCO + 0x63),
                latch=byte(u, LOCO + 0x62), residual=read_u32(u, LOCO + 0x4C))


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    originals = [bytes(u.mem_read(a, b-a)) for a, b in CODE_RANGES]
    original_vtable = bytes(u.mem_read(UNIT_VTABLE, 0x600))
    assert read_u32(u, UNIT_VTABLE + 0x480) == 0x741970
    assert read_u32(u, UNIT_VTABLE + 0x484) == 0x738970
    assert read_u32(u, UNIT_VTABLE + 0x1B8) == GET_CELL
    u.mem_write(VTABLE, original_vtable)
    u.mem_write(VTABLE + 0x480, dwords(DESTINATION, IDLE))
    u.mem_write(INTERFACE_VTABLE + 0x10, dwords(IS_MOVING))
    u.mem_write(CELL_VTABLE + 0x2C, dwords(CELL_RTTI))
    u.mem_write(CELL_RTTI, return_body(row.get('navcom_rtti', 11)))
    u.mem_write(IS_MOVING, return_body(row.get('is_moving', 1), 4))
    receiver = byte_write(OWNER + 0x90, row.get('receiver_alive', 1))
    receiver += return_body(row.get('receiver_return', 0), 8)
    u.mem_write(DESTINATION, receiver)
    u.mem_write(IDLE, receiver)
    u.mem_write(LOCO + 4, dwords(INTERFACE_VTABLE))
    u.mem_write(LOCO + 0xC, dwords(OWNER))
    u.mem_write(OWNER, dwords(VTABLE))
    u.mem_write(CELL, dwords(CELL_VTABLE))
    u.mem_write(CELL + 0x24, struct.pack('<hh', *row.get('navcom_cell', (10, 9))))
    u.mem_write(OWNER + 0x9C, dwords(*row.get('owner_coord', (2688, 2432, 48))))
    destination = row.get('destination', (2688, 2432, 48))
    u.mem_write(LOCO + 0x34, dwords(*destination))
    u.mem_write(FAMILIES[row['family']]['null_coord'], bytes(12))
    u.mem_write(OWNER + 0xAC, dwords(row.get('mission', 0)))
    u.mem_write(OWNER + 0x598, dwords(row.get('queue_count', 0)))
    u.mem_write(OWNER + 0x5A0, dwords(0x13570246))
    u.mem_write(OWNER + 0x5A4, dwords(CELL if row.get('navcom', False) else 0))
    u.mem_write(OWNER + 0x5E0, dwords(row.get('queue_head', -1)))
    u.mem_write(OWNER + 0x6D1, bytes((row.get('unit_6d1', 0),)))
    u.mem_write(OWNER + 0x90, b'\x01')
    u.mem_write(OWNER + 0x81, bytes((row.get('limbo', 0),)))
    u.mem_write(OWNER + 0x8D, bytes((row.get('falling', 0),)))
    active = row['stage'] == 'after_active' or row.get('active_bypass', False)
    u.mem_write(LOCO + 0x58, dwords(row.get('selector', 0 if active else -1)))
    u.mem_write(LOCO + 0x62, bytes((row.get('latch', 0), row.get('valid', int(active)))))
    u.mem_write(LOCO + 0x4C, dwords(0x13579))
    # IsRotating reads timer/rate only; actual native sampler remains unchanged.
    u.mem_write(OWNER + 0x388, struct.pack('<H', 0))
    u.mem_write(OWNER + 0x38C, struct.pack('<H', 0x2000))
    u.mem_write(OWNER + 0x390, dwords(100, 0, 4))
    u.mem_write(OWNER + 0x39C, struct.pack('<h', 16))
    u.mem_write(FRAME, dwords(101 if row.get('rotating', True) else 104))
    facing_before = bytes(u.mem_read(OWNER + 0x388, 0x18))
    # Interior frame: four saved registers, sixteen locals, RET4 caller slot.
    # SP+24 is the incoming interface argument, reused by native as out-byte.
    u.mem_write(SP, dwords(*SAVED))
    u.mem_write(SP + 0x20, dwords(RET_MAGIC, LOCO + 4))
    for reg, value in ((UC_X86_REG_ESP, SP), (UC_X86_REG_EBX, 0),
                       (UC_X86_REG_EBP, 0xFFFFFFFF), (UC_X86_REG_ESI, LOCO + 4),
                       (UC_X86_REG_EDI, LOCO), (UC_X86_REG_ECX, OWNER)):
        u.reg_write(reg, value)
    before = state(u)
    family = FAMILIES[row['family']]
    events, writes, visits = [], [], []

    def observe(uc, address, _size, _data):
        if address in (DESTINATION, IDLE, IS_MOVING, CELL_RTTI,
                       FOOT_STOP, GET_CELL, SAMPLER, 0x746E20):
            sp = uc.reg_read(UC_X86_REG_ESP)
            event = dict(address=f'{address:08X}', state=state(uc))
            if address in (DESTINATION, IDLE):
                assert uc.reg_read(UC_X86_REG_ECX) == OWNER
                event.update(event='assign_destination' if address == DESTINATION else 'enter_idle',
                             original_receiver='00741970' if address == DESTINATION else '00738970',
                             args=[read_u32(uc, sp + 4), read_u32(uc, sp + 8)],
                             supplied_return=row.get('receiver_return', 0))
            elif address == IS_MOVING:
                assert read_u32(uc, sp + 4) == LOCO + 4
                event.update(event='is_moving', supplied_return=row.get('is_moving', 1))
            else:
                event['event'] = {CELL_RTTI: 'supplied_navcom_rtti', FOOT_STOP: 'foot_stop',
                                  GET_CELL: 'native_get_cell', SAMPLER: 'native_live_sampler',
                                  0x746E20: 'native_unit_rtti'}[address]
            events.append(event)
        if address in set(family.values()):
            visits.append(f'{address:08X}')

    def observe_write(_uc, _access, address, size, value, _data):
        if address in (OWNER + 0x5A0, OWNER + 0x5A4, LOCO + 0x62, SP + 0x24):
            writes.append(dict(address=f'{address:08X}', size=size, value=value))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, observe_write)
    stops = {family['common_tail']: 'common_process_tail', RET_MAGIC: 'original_return'}
    if row['stage'] == 'early':
        stops[family['sample']] = 'before_live_sampler'
    if row['stage'] == 'live_turn':
        stops[family['latch_test']] = 'before_latch_completion'
    begin = (family['ordinary'] if row['stage'] == 'ordinary' else family['sample']
             if row['stage'] == 'live_turn' else family['active_gate'])
    current = begin
    fresh_called = False
    # Stop only at actual callees or declared boundaries. The original CALL
    # instruction supplies return address/arguments. An explicit external body
    # RETs to that saved address; no hook redirects a native decision branch.
    for _ in range(6):
        end = run_checked(u, current, tuple(stops) + (family['fresh'], family['track']),
                          count=500, required_addresses=(current,))
        if end in stops:
            break
        sp = u.reg_read(UC_X86_REG_ESP)
        assert u.reg_read(UC_X86_REG_ECX) == LOCO
        return_address = read_u32(u, sp)
        if end == family['fresh']:
            assert not fresh_called
            fresh_called = True
            output = read_u32(u, sp + 4)
            args = [read_u32(u, sp + 8), read_u32(u, sp + 12)]
            assert output == SP + 0x24 and args == [1, 0]
            assert byte(u, output) == 0  # Written by the original caller.
            events.append(dict(event='external_fresh', address=f'{end:08X}',
                               caller_return=f'{return_address:08X}', args=args,
                               output_pointer=f'{output:08X}', output_before=byte(u, output),
                               supplied_al=row.get('fresh_return', 0) & 255,
                               supplied_output=row.get('output', 0), state=state(u)))
            body = byte_write(output, row.get('output', 0))
            body += byte_write(OWNER + 0x90, row.get('fresh_alive', 1))
            if row.get('unlink_fresh', False):
                body += dword_write(LOCO + 0xC, 0)
            body += return_body(row.get('fresh_return', 0), 12)
            fixture = EXTERNAL_FRESH
        else:
            initial = return_address == family['first_track_return']
            argument = read_u32(u, sp + 4)
            value = row.get('active_return', 0) if initial else row.get('track_return', 0)
            events.append(dict(event='external_track', phase='active' if initial else 'postfresh',
                               address=f'{end:08X}', caller_return=f'{return_address:08X}',
                               argument=argument, supplied_al=value & 255, state=state(u)))
            body = byte_write(OWNER + 0x90, row.get('active_alive', 1) if initial
                              else row.get('track_alive', 1))
            if initial and row.get('active_retires', True):
                body += dword_write(LOCO + 0x58, -1) + byte_write(LOCO + 0x63, 0)
            if not initial and row.get('unlink_track', False):
                body += dword_write(LOCO + 0xC, 0)
            body += return_body(value, 4)
            fixture = EXTERNAL_TRACK
        u.mem_write(fixture, body)
        run_checked(u, fixture, return_address, count=32, required_addresses=(fixture,))
        current = return_address
    else:
        raise AssertionError('Unexpected recursive outer continuation')
    after = state(u)
    if end == RET_MAGIC:
        assert u.reg_read(UC_X86_REG_EAX) & 255 == 0
        assert u.reg_read(UC_X86_REG_ESP) == SP + 0x28
        assert tuple(u.reg_read(reg) for reg in
                     (UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_EBP, UC_X86_REG_EBX)) == SAVED
    else:
        assert u.reg_read(UC_X86_REG_ESP) == SP
    assert all(bytes(u.mem_read(a, b-a)) == original
               for (a, b), original in zip(CODE_RANGES, originals))
    assert bytes(u.mem_read(UNIT_VTABLE, 0x600)) == original_vtable
    assert bytes(u.mem_read(OWNER + 0x388, 0x18)) == facing_before
    assert after['residual'] == before['residual']
    return dict(input=row, entry=f'{begin:08X}', before=before, after=after,
                events=events, writes=writes, visited_boundaries=visits,
                boundary=stops[end], boundary_address=f'{end:08X}',
                returned_al=0 if end == RET_MAGIC else None,
                output_byte=byte(u, SP + 0x24) if fresh_called else None,
                original_code_and_unit_vtable_unchanged=True)


def generate():
    rows = []
    for family in FAMILIES:
        for stage, fresh_return, output, fresh_alive, track_return in product(
                ('ordinary', 'after_active'), (0, 1, 0x100, 0xFF), (0, 1), (0, 1), (0, 1)):
            rows.append(dict(family=family, stage=stage, fresh_return=fresh_return,
                             output=output, fresh_alive=fresh_alive, track_return=track_return))
        for stage in ('ordinary', 'after_active'):
            for track_return, track_alive in product((0, 0xFF), (0, 1)):
                rows.append(dict(family=family, stage=stage, track_return=track_return,
                                 track_alive=track_alive))
            for limbo, falling in product((0, 1), repeat=2):
                rows.append(dict(family=family, stage=stage, limbo=limbo, falling=falling))
            rows.append(dict(family=family, stage=stage, output=0xFF, unlink_fresh=True))
            if family == 'drive':
                rows.append(dict(family=family, stage=stage, unlink_track=True))
        for changes in (dict(active_return=1), dict(active_return=0x100),
                        dict(active_alive=0), dict(active_retires=False),
                        dict(is_moving=0, queue_head=-1), dict(is_moving=0, queue_head=2),
                        dict(unit_6d1=1)):
            rows.append(dict(family=family, stage='after_active', **changes))
        for rotating, latch in product((False, True), (0, 1)):
            rows.append(dict(family=family, stage='live_turn', rotating=rotating, latch=latch))
        for predicate, queue_count, receiver_return, receiver_alive in product(
                ('samecell', 'guard'), (0, 1, 3), (0, 0xFF), (0, 1)):
            rows.append(dict(family=family, stage='early', predicate=predicate,
                             navcom=predicate == 'samecell', mission=5 if predicate == 'guard' else 2,
                             queue_count=queue_count, receiver_return=receiver_return,
                             receiver_alive=receiver_alive))
        for changes in (dict(navcom=True, navcom_cell=(11, 9)),
                        dict(navcom=True, navcom_cell=(10, 10)),
                        dict(navcom=True, navcom_rtti=15),
                        dict(mission=2), dict(mission=5, destination=(2688, 2432, 49)),
                        dict(mission=5, destination=(0, 0, 0)), dict(mission=5, valid=1),
                        dict(navcom=True, active_bypass=True, active_return=1),
                        dict(mission=5, active_bypass=True, active_return=1)):
            rows.append(dict(family=family, stage='early', control=True, **changes))
    results = [execute(row) for row in rows]
    assert len(results) == 254
    # These are consistency checks on observed traces, not alternate outputs.
    # The reference vectors retain the native execution outcomes themselves.
    for result in results:
        row, events = result['input'], result['events']
        fresh = [event for event in events if event['event'] == 'external_fresh']
        post = [event for event in events if event['event'] == 'external_track'
                and event['phase'] == 'postfresh']
        if fresh:
            assert bool(post) == (row.get('output', 0) == 0 and row.get('fresh_alive', 1) != 0)
            if post:
                assert post[0]['argument'] == int(row['stage'] == 'after_active')
        if row['stage'] == 'live_turn':
            assert not fresh and not post
            assert result['boundary'] == ('common_process_tail' if row['rotating']
                                           else 'before_latch_completion')
        if row['stage'] == 'early' and not row.get('control', False):
            assert result['boundary'] == 'original_return' and not fresh and not post
            names = [event['event'] for event in events]
            if row['queue_count']:
                assert names.index('foot_stop') < names.index('enter_idle')
                receiver = next(event for event in events if event['event'] == 'enter_idle')
                assert receiver['state']['navcom'] == receiver['state']['navcom_aux'] == 0
            else:
                receiver = next(event for event in events if event['event'] == 'assign_destination')
                assert receiver['state']['navcom_aux'] == 0x13570246
            assert receiver['args'] == [0, 1]
    return results


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    result = provenance(
        scope='Original outer Drive/Ship Process continuation branches, call ordering and arguments around explicitly supplied fresh/track/destination/idle callees',
        assumptions=[
            'Interior frames start at ordinary fresh setup4B0A6B/6A0134, active gate4B055A/69FC6A, or live sampler4B0775/69FE22. The saved register/local/RET4 stack, ESI=class+4, EDI=complete class, EBX=0 and ECX=owner are supplied exactly in execute(). Floor/slope interpolation and type sampling before the active gate are outside entry; this is not a full Process oracle',
            'Original direct CALLs execute and produce their stack arguments/return address. Segmented execution stops at declared external fresh/TrackProcess entry, executes a scratch x86 fixture body, then resumes at the native return address. Fresh AL/output/alive/link and TrackProcess AL/retirement/alive/link are supplied, not discovered native callee behavior',
            'Ordinary and after-active rows cross fresh EAX0/1/256/255, output0/1, alive0/1 and postfresh TrackProcess AL0/1. Supplementary rows cover output255 with unlink, post-track alive changes, Drive post-track null link, limbo/falling and active-completion eligibility controls',
            'Mode0 versus mode1 is observed as the actual caller stack argument. TrackProcess prefix, residual reset, movement and the argument1 paid-budget mask are not executed here. Residual is deliberately preserved by the supplied bodies; no result claims this matches actual TrackProcess residual behavior',
            'The original live Facing sampler executes running/expired timers with retained latch0/1. Common tail and expired-sampler latch completion are stop boundaries. No downstream common-tail slope/speed, mission or completion-receiver claim is made',
            'Early destination rows execute original same-cell/Guard predicates, original Unit GetCell41BEA0 and original FootStop4DF0D0. Queue counts0/1/3 cross supplied receiver AL0/255 and owner alive0/1. Negative controls cover mismatched cell, non-Cell supplied RTTI, Move mission, mismatched Z/null destination, valid track and active-track bypass',
            'Destination and idle receivers are only dispatch fixtures. The original Unit vtable identity is asserted: +480->741970, +484->738970, +1B8->41BEA0. No destination refusal, NavQueue dequeue, END, mission promotion or receiver parity is claimed. Original Unit RTTI746E20 remains executable',
            'Producer reachability of synthetic callback results is not established. Owner unlink after fresh is supplied only with output nonzero, matching the protected caller boundary. Ship post-track null owner has no observed null guard and is excluded rather than normalized',
            'Every original RET4 must restore all saved registers and stack and returns AL0; all rows verify unchanged original code ranges, original Unit vtable, live Facing fields and supplied residual. Observers only record calls/stores and never mutate execution',
        ],
        substitutions=[
            'Direct Process_Movement and Process_Track callee bodies are replaced at explicit execution boundaries by scratch x86 return bodies; original caller CALLs and all caller branches remain unpatched',
            'Copied owner vtable replaces only +480/+484 with scratch receiver bodies that write the declared alive byte and return declared EAX using RET8. Their entry records native receiver identity, args and pre-callback state',
            'Supplied class-interface vtable+10 IsMoving uses RET4; supplied NavCom vtable+2C returns declared RTTI. Original owner GetCell and Unit RTTI execute from original copied slots. No original vtable/code byte is replaced',
        ],
        entry_points={**{f'{name}_{key}': value for name, family in FAMILIES.items()
                        for key, value in family.items()},
                      'foot_stop': FOOT_STOP, 'get_cell': GET_CELL,
                      'facing_sampler': SAMPLER, 'unit_assign_destination': 0x741970,
                      'unit_enter_idle': 0x738970},
    )
    result['original_code_range_sha256'] = {
        f'{a:08X}..{b:08X}': hashlib.sha256(bytes(u.mem_read(a, b-a))).hexdigest()
        for a, b in CODE_RANGES
    }
    result['case_counts'] = dict(total=254, core_return_output_alive_matrix=128,
                                 supplemental_callback_state=38, active_eligibility=14,
                                 live_sampler=8, early_destination_and_controls=66)
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
