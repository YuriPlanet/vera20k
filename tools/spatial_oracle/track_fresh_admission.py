"""Original Drive/Ship fresh-query brackets and bounded response dispatch.

Mark and CanEnter callbacks are explicit external fixture return stubs. Original
caller bytes choose every response, save the query result across Mark1, read
type coercions, construct the recursive call, and dispatch chain eligibility.
No hook changes registers, memory or control flow. This is not a complete
Process_Movement, Mark, CanEnter, recursion, scatter, gate or wall oracle.
"""
from itertools import product
from pathlib import Path
import hashlib
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_EBX, UC_X86_REG_ECX,
    UC_X86_REG_EDI, UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import (
    RET_MAGIC, SCRATCH, STACK_BASE, STACK_SIZE, finish_vectors, load_image,
    provenance, run_checked,
)
from tools.spatial_oracle.map_queries import dwords, packed

LOCO, OWNER, TYPE = SCRATCH + 0x100, SCRATCH + 0x1000, SCRATCH + 0x2000
VTABLE, MARK_STUB, QUERY_STUB = SCRATCH + 0x4000, SCRATCH + 0x5000, SCRATCH + 0x5020
CELL1, CELL2, OVERLAY = SCRATCH + 0x6000, SCRATCH + 0x6200, SCRATCH + 0x7000
SP = STACK_BASE + STACK_SIZE - 0x1000
MAP, TABLE, UNIT_VTABLE = 0x87F7E8, 0xC00000, 0x7F5C70
CODE_RANGES = ((0x49F3A0, 0x49F420), (0x4B2630, 0x4B4767), (0x6A1C80, 0x6A3DA0),
               (0x4B0F20, 0x4B2624), (0x6A05F0, 0x6A1C78))
FAMILIES = {
    'drive': dict(first=0x4B349C, first_query=0x4B34C0,
                  first_mark_calls=(0x4B34AE, 0x4B34D1),
                  first_stops={0x4B357F: 'accepted_before_terrain',
                               0x4B394D: 'redraw_response',
                               0x4B3656: 'code2_blocked_delay_response',
                               0x4B35EC: 'gate_response',
                               0x4B3AD3: 'wall_override_response',
                               0x4B36FD: 'scatter_response',
                               0x4B3AA1: 'other_blocked_response'},
                  second=0x4B40A3, second_query=0x4B4120,
                  second_cell_store=0x4B410B,
                  second_stops={0x4B45CB: 'before_two_node_shift',
                                0x4B444A: 'redraw_response',
                                0x4B2630: 'recursive_process_movement',
                                0x4B419B: 'gate_response',
                                0x4B41B3: 'clear_track_then_retry_or_stop',
                                0x4B4231: 'scatter_response',
                                0x4B4519: 'code7_retry_or_stop',
                                0x4B3A4D: 'owner_not_alive'},
                  recursive_call=0x4B4219, chain=0x4B1C3A,
                  chain_query=0x4B1C3E, chain_table=0x4B2608,
                  chain_gate=0x4B1C54, chain_accept=0x4B1C78,
                  chain_reject=0x4B1F48,
                  normalize=(0x4B3F7D, 0x4B3F93)),
    'ship': dict(first=0x6A2AEB, first_query=0x6A2B0F,
                 first_mark_calls=(0x6A2AFD, 0x6A2B20),
                 first_stops={0x6A2BCE: 'accepted_before_terrain',
                              0x6A2F9C: 'redraw_response',
                              0x6A2CA5: 'code2_blocked_delay_response',
                              0x6A2C3B: 'gate_response',
                              0x6A3122: 'wall_override_response',
                              0x6A2D4C: 'scatter_response',
                              0x6A30F0: 'other_blocked_response'},
                 second=0x6A36CF, second_query=0x6A374C,
                 second_cell_store=0x6A3736,
                 second_stops={0x6A3BF7: 'before_two_node_shift',
                               0x6A3A76: 'redraw_response',
                               0x6A1C80: 'recursive_process_movement',
                               0x6A37C7: 'gate_response',
                               0x6A37DF: 'clear_track_then_retry_or_stop',
                               0x6A385D: 'scatter_response',
                               0x6A3B45: 'code7_retry_or_stop',
                               0x6A309C: 'owner_not_alive'},
                 recursive_call=0x6A3845, chain=0x6A1284,
                 chain_query=0x6A1288, chain_table=0x6A1C5C,
                 chain_gate=0x6A129E, chain_accept=0x6A12C2,
                 chain_reject=0x6A158B,
                 normalize=(0x6A35CC, 0x6A35E2)),
}


def read_u32(u, address):
    return struct.unpack('<I', u.mem_read(address, 4))[0]


def state(u):
    return dict(head=list(struct.unpack('<iii', u.mem_read(LOCO + 0x40, 12))),
                valid=u.mem_read(LOCO + 0x63, 1)[0],
                selector=read_u32(u, LOCO + 0x58),
                cursor=read_u32(u, LOCO + 0x5C),
                residual=read_u32(u, LOCO + 0x4C))


def execute(row):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(RET_MAGIC, 0x1000)
    code_before = [bytes(u.mem_read(a, b-a)) for a, b in CODE_RANGES]
    u.mem_write(SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, SP)
    run_checked(u, 0x49F3A0, RET_MAGIC, count=100,
                required_addresses=(0x49F3A0, 0x49F413))
    # The original owner vtable is copied outside the image. Only the two
    # explicitly supplied callbacks are replaced; GetType/WhatAmI stay native.
    u.mem_write(VTABLE, bytes(u.mem_read(UNIT_VTABLE, 0x600)))
    u.mem_write(VTABLE + 0x124, dwords(MARK_STUB))
    u.mem_write(VTABLE + 0x1AC, dwords(QUERY_STUB))
    u.mem_write(MARK_STUB, b'\xB8' + dwords(row.get('mark_return', 0xDEADBEEF)) + b'\xC2\x04\x00')
    u.mem_write(QUERY_STUB, b'\xB8' + dwords(row.get('code', 0)) + b'\xC2\x14\x00')
    u.mem_write(OWNER, dwords(VTABLE))
    u.mem_write(OWNER + 0x6C4, dwords(TYPE))
    u.mem_write(OWNER + 0x90, bytes((int(row.get('alive', True)),)))
    u.mem_write(TYPE + 0xC94, bytes((int(row.get('train', False)),)))
    u.mem_write(TYPE + 0xD28, bytes((int(row.get('crusher', False)),)))
    u.mem_write(TYPE + 0xE0C, bytes((int(row.get('passive', False)),)))
    u.mem_write(LOCO + 0xC, dwords(OWNER))
    u.mem_write(LOCO + 0x40, dwords(2176, 2432, 312))
    u.mem_write(LOCO + 0x4C, dwords(123))
    u.mem_write(LOCO + 0x58, dwords(17, 3))
    u.mem_write(LOCO + 0x63, b'\x01')
    u.mem_write(CELL1 + 0x24, packed(10, 9))
    u.mem_write(CELL2 + 0x24, packed(11, 8))
    u.mem_write(CELL1 + 0x44, dwords(row.get('first_overlay', -1)))
    u.mem_write(CELL2 + 0x44, dwords(row.get('second_overlay', -1)))
    u.mem_write(0x8A0790, bytes(12))
    u.mem_write(0xB077F8, bytes(12))
    # Native second-candidate production must overwrite the saved packed cell.
    # Its later Crusher lookup uses that rewritten candidate2, not candidate1.
    u.mem_write(MAP + 0x13C, dwords(TABLE, 0x40000))
    u.mem_write(TABLE + (9 * 512 + 10) * 4, dwords(CELL1))
    u.mem_write(TABLE + (8 * 512 + 11) * 4, dwords(CELL2))
    u.mem_write(0xA83D84, dwords(OVERLAY + 0x800))
    u.mem_write(OVERLAY + 0x800, dwords(OVERLAY, OVERLAY))
    u.mem_write(SP + 0x14, packed(10, 9))
    u.mem_write(SP + 0x60, dwords(0xABCD0135, 1, 0))
    u.reg_write(UC_X86_REG_EBP, LOCO)
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_EBX, 1)
    u.reg_write(UC_X86_REG_EDI, 3)
    u.reg_write(UC_X86_REG_EAX, CELL1)
    family = FAMILIES[row['family']]
    events, calls, produced = [], [], []

    def observe(uc, address, _size, _data):
        if address in (MARK_STUB, QUERY_STUB):
            assert uc.reg_read(UC_X86_REG_ECX) == OWNER
            sp = uc.reg_read(UC_X86_REG_ESP)
            count = 1 if address == MARK_STUB else 5
            args = [read_u32(uc, sp + 4 + i*4) for i in range(count)]
            if address == MARK_STUB:
                events.append(dict(callback='mark', mode=args[0], supplied_return=row.get('mark_return', 0xDEADBEEF)))
            else:
                args[0] = {CELL1: 'first_candidate', CELL2: 'second_candidate'}[args[0]]
                events.append(dict(callback='can_enter', arguments=args, supplied_return=row['code']))
        if address in (family['first_query'], family['second_query'], family['chain_query'],
                       *family['first_mark_calls'], family['recursive_call'],
                       family['second_cell_store']):
            calls.append(f'{address:08X}')
        if address == family['second_query']:
            produced.append(dict(
                saved_cell=list(struct.unpack('<hh', uc.mem_read(SP + 0x14, 4))),
                candidate_xyz=list(struct.unpack('<iii', uc.mem_read(SP + 0x38, 12))),
            ))

    u.hook_add(UC_HOOK_CODE, observe)
    stage = row['stage']
    before = state(u)
    if stage == 'first':
        begin, stops = family['first'], family['first_stops']
        required = (*family['first_mark_calls'], family['first_query'])
    elif stage == 'second':
        # Enter after the preceding crate/life branch with its retained first
        # XYZ and selected NE direction. Original table arithmetic, packed-cell
        # publication, Map lookup and all CanEnter arguments execute here.
        u.reg_write(UC_X86_REG_EDI, OWNER)
        u.reg_write(UC_X86_REG_ESI, 1)
        u.mem_write(SP + 0x38, dwords(2688, 2432, 312))
        u.mem_write(SP + 0x28, dwords(3))
        begin, stops = family['second'], family['second_stops']
        required = (family['second_cell_store'], family['second_query'])
    elif stage == 'chain':
        u.reg_write(UC_X86_REG_ESP, SP - 16)
        u.mem_write(SP - 16, dwords(2, 3, 0, 1))
        u.reg_write(UC_X86_REG_EAX, CELL2)
        u.reg_write(UC_X86_REG_EDI, VTABLE)
        begin = family[stage]
        required = (family[stage + '_query'],)
        targets = [read_u32(u, family['chain_table'] + i*4) for i in range(7)]
        stops = {target: f'code{i}_response' for i, target in enumerate(targets)
                 if target != family['chain_gate']}
        stops[family['chain_accept']] = 'chain_eligible_after_unit_passive_gate'
        stops[family['chain_reject']] = 'chain_not_accepted'
    elif stage == 'recursive_normalization':
        begin, end = family['normalize']
        stops, required = {end: 'before_second_direction_overlay_checks'}, (begin,)
        u.reg_write(UC_X86_REG_ESI, row['second_direction'] & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EBX, row['first_direction'])
        u.mem_write(SP + 0x68, dwords(row['recursive_flag']))
    else:
        raise ValueError(stage)

    run_checked(u, begin, tuple(stops), count=250, required_addresses=required)
    end = u.reg_read(UC_X86_REG_EIP)
    if stage == 'first':
        assert [event['callback'] for event in events] == ['mark', 'can_enter', 'mark']
        assert [events[0]['mode'], events[2]['mode']] == [0, 1]
    elif stage in ('second', 'chain'):
        assert [event['callback'] for event in events] == ['can_enter']
    assert all(bytes(u.mem_read(a, b-a)) == original
               for (a, b), original in zip(CODE_RANGES, code_before))
    out = dict(input=row, callbacks=events, observed_calls=calls,
               boundary=stops[end], boundary_address=f'{end:08X}',
               before=before, after=state(u), original_code_unchanged=True)
    if stage == 'first':
        out['saved_effective_code'] = read_u32(u, SP + 0x18)
    elif stage == 'second':
        out['produced_second_candidate'] = produced
        assert produced == [dict(saved_cell=[11, 8], candidate_xyz=[2944, 2176, 312])]
        assert events[0]['arguments'] == ['second_candidate', 1, 3, 0, 1]
        out['saved_effective_code'] = u.reg_read(UC_X86_REG_ESI)
        if stops[end] == 'recursive_process_movement':
            sp = u.reg_read(UC_X86_REG_ESP)
            out['recursive_arguments'] = [read_u32(u, sp + 4 + i*4) for i in range(3)]
            assert u.reg_read(UC_X86_REG_ECX) == LOCO
            assert family['recursive_call'] in [int(address, 16) for address in calls]
    elif stage == 'recursive_normalization':
        out['selected_second_direction'] = u.reg_read(UC_X86_REG_ESI)
    return out


def generate():
    rows = []
    for family in FAMILIES:
        for stage in ('first', 'second'):
            for code in range(8):
                rows.append(dict(family=family, stage=stage, code=code))
                rows.append(dict(family=family, stage=stage, code=code, train=True))
            for code, first_overlay in product((4, 5), (-1, 0, 1)):
                rows.append(dict(family=family, stage=stage, code=code,
                                 crusher=True, first_overlay=first_overlay,
                                 second_overlay=0 if first_overlay != 0 else -1))
        for code, mark_return in product((0, 2), (0, 7)):
            rows.append(dict(family=family, stage='first', code=code, mark_return=mark_return))
        for code in (0, 2):
            rows.append(dict(family=family, stage='second', code=code, alive=False))
        for code, passive in product(range(8), (False, True)):
            rows.append(dict(family=family, stage='chain', code=code, passive=passive))
        for direction, recursive in product((-1, 1, 2, 8), (0, 1)):
            rows.append(dict(family=family, stage='recursive_normalization',
                             first_direction=1, second_direction=direction,
                             recursive_flag=recursive))
    return [execute(row) for row in rows]


def metadata():
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    hashes = {f'{a:08X}..{b:08X}': hashlib.sha256(bytes(u.mem_read(a, b-a))).hexdigest()
              for a, b in CODE_RANGES}
    result = provenance(
        scope='Drive/Ship fresh first-query Mark0/CanEnter/Mark1 saved-code dispatch, original second-candidate producer and recursive-call boundary, separate TrackProcess chain eligibility, and supplied recursive direction normalization',
        assumptions=[
            'Interior continuations supply the already-derived first Cell pointer or retained first XYZ, selected direction, effective height, owner/class pointer and native-shaped stack locals; prior pathfinding, facing, entering receiver and crate/life-guard execution are excluded',
            'Owner uses a copy of the retail Unit vtable; original GetType and WhatAmI callbacks execute with supplied UnitType IsTrain+C94, Crusher+D28 and Passive+E0C values. Flag combinations are continuation inputs, not proof of any stock-YR type or live class reaching them',
            'First query begins immediately after Map returned candidate1. Second begins at Drive4B40A3/Ship6A36CF with first XYZ(2688,2432,312), second direction1 and height3; original additions, packed-cell store, Map lookup and argument publication execute. Chain begins immediately before CanEnter with its four earlier arguments supplied on the stack',
            'Original49F3A0 startup initializes the lepton direction table before each case; direction deltas are not supplied',
            'Supplied original null coordinates are zero. Cell1 is(10,9), Cell2(11,8). Second-candidate output records original XYZ(2944,2176,312) and saved cell(11,8). Differing overlays establish that later Crusher coercion reads the newly published candidate2 cell',
            'Correction of the prior corpus: its second-query continuation supplied a stale candidate1 packed cell after skipping the producer, so its candidate1-anchor/reachability claim was invalid. These rows now execute the producer; the supplied callback and response-boundary scope is unchanged. Alive=false rows supply post-guard state solely to exercise the later life check, not the preceding crate/life branch',
            'Caller handling is sampled at the first named response boundary; scatter, gate, wall, delay, two-node shift and recursive Process_Movement bodies do not execute',
            'Recursive code2 rows run the original CALL and stop at callee entry, recording all three supplied/caller-written arguments. Separate normalization rows begin at the real second-direction checks with a supplied recursion flag; no full recursive call is claimed',
            'Chain executes the original jump table and native Unit/Passive gate only, stopping before chain installation. It has no fresh Mark bracket and code2 can reach eligibility without a fresh recursive retry',
            'Observation hooks only append evidence. Relevant original code ranges are checked byte-for-byte unchanged after each row; supplied external stubs are outside the executable image',
        ],
        substitutions=[
            'Virtual Mark+124 and CanEnter+1AC slots point to external MOV-EAX/RET fixture stubs. Mark returns an explicitly supplied clobber value; CanEnter returns the supplied code0..7. Their world side effects and classification are NOT native evidence here',
            'All other reached instructions, conditional branches, type coercions, type/WhatAmI getters, original map lookup, jump table and recursive-call argument writes execute unpatched retail bytes',
        ],
        entry_points={'direction_initializer': 0x49F3A0, **{f'{family}_{name}': data[name]
                      for family, data in FAMILIES.items()
                      for name in ('first', 'first_query', 'second', 'second_query',
                                   'chain', 'chain_query', 'recursive_call')}},
    )
    result['original_code_range_sha256'] = hashes
    return result


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=metadata)
