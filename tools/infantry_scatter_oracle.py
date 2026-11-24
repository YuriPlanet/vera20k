"""Bounded native Infantry scatter gates and Walk code-2 obstruction timers.

Run ``python -m tools.infantry_scatter_oracle --check``; --write regenerates.

Live /gamemd.exe inspection, 2026-09-10: Infantry Scatter 51D162..51D226
demotes its first boolean after Walk Is_Moving, then applies mission, target,
and Fraidycat gates. Cell Scatter 4817D3 dispatches Infantry vtable+174 to
51D0D0; Walk obstruction result6 calls Cell Scatter at75B891 with NullCoord
and both booleans1. Result2 instead runs timers/repath at75B8A0..75B9F9.

Successful NullCoord scatter calls SetDestination51D455 and normal Walk
Process51D478. Walk speed comes from ordinary owner virtual538 at75BFC0,
resolved to Infantry521D80 -> Foot4DB1A0. No synthetic scatter speed exists.

These fixtures execute original bytes, not a Python gate implementation.
They exclude the early human/special-sequence gates, destination selection,
RNG, pathfinding, complete scheduler and full speed calculation. The scatter
fixture stops BEFORE RNG/destination work; gate admission is not successful
scatter. Walk code2 fixtures stop BEFORE FindPath or at the native wait exit.
"""

from itertools import product
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_EBP, UC_X86_REG_ESI,
    UC_X86_REG_ESP, UC_X86_REG_EIP,
)
from tools.native_oracle import (
    SCRATCH, SCRATCH_SIZE, STACK_BASE, STACK_SIZE, load_image, run_checked,
    finish_vectors, provenance,
)

OWNER = SCRATCH
LOCO = SCRATCH + 0x2000
TYPE = SCRATCH + 0x3000
MISSION = SCRATCH + 0x4000
RULES = SCRATCH + 0x5000
STACK = STACK_BASE + STACK_SIZE - 0x200


def u32(value):
    return struct.pack('<I', value & 0xffffffff)


def fresh():
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(uc)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(STACK_BASE, STACK_SIZE)
    uc.reg_write(UC_X86_REG_ESP, STACK)
    return uc


def scatter_gates(family='walk'):
    assert family in ('walk', 'jumpjet')
    cases = []
    for moving, first_bool, mission_scatter, fraidycat, has_target, global_scatter in product((False, True), repeat=6):
        uc = fresh()
        uc.mem_write(OWNER + 0x674, u32(LOCO))
        uc.mem_write(OWNER + 0x6c0, u32(TYPE))
        uc.mem_write(OWNER + 0x6c4, u32(-1))  # accepted animation-sequence sentinel
        uc.mem_write(OWNER + 0x2b4, u32(1 if has_target else 0))
        uc.mem_write(LOCO, u32(0x7f69f8 if family == 'walk' else 0x7ecd68))
        uc.mem_write(LOCO + (0x30 if family == 'walk' else 0x48), bytes([moving]))
        if family == 'jumpjet':
            uc.mem_write(LOCO + 0x4c, u32(2))  # holding; IsMoving is independent
        uc.mem_write(TYPE + 0xebf, bytes([fraidycat]))
        uc.mem_write(MISSION + 9, bytes([mission_scatter]))
        uc.mem_write(0x8871e0, u32(RULES))
        uc.mem_write(RULES + 0x17ed, bytes([global_scatter]))
        uc.mem_write(STACK + 0x5c, u32(1))  # original second boolean, as obstruction caller
        uc.reg_write(UC_X86_REG_ESI, OWNER)
        uc.reg_write(UC_X86_REG_EBP, 0)
        uc.reg_write(UC_X86_REG_EBX, int(first_bool))
        owner_before = bytes(uc.mem_read(OWNER, 0x700))

        def supplied_calls(machine, address, _size, _data):
            if address == 0x51d176:  # GetMissionTimerEntry: return supplied table entry
                machine.reg_write(UC_X86_REG_EAX, MISSION)
                machine.reg_write(UC_X86_REG_EIP, 0x51d17b)
            elif address == 0x51d1dc:  # HasWeaponAbility(3): supplied false
                machine.reg_write(UC_X86_REG_EAX, 0)
                machine.reg_write(UC_X86_REG_ESP, machine.reg_read(UC_X86_REG_ESP) + 4)
                machine.reg_write(UC_X86_REG_EIP, 0x51d1e1)

        uc.hook_add(UC_HOOK_CODE, supplied_calls)
        stop = run_checked(uc, 0x51d162, (0x51d226, 0x51d6e6), count=300,
                           required_addresses=[0x75ab30 if family == 'walk' else 0x54ae50,
                                               0x51d16e, 0x51d17b])
        if bytes(uc.mem_read(OWNER, 0x700)) != owner_before:
            raise RuntimeError('scatter refusal/admission gate mutated owner state')
        if uc.reg_read(UC_X86_REG_ESP) != STACK:
            raise RuntimeError('scatter gate fixture stack balance changed')
        cases.append(dict(moving=moving, first_bool=first_bool, second_bool=True,
                          mission_scatter=mission_scatter, fraidycat=fraidycat,
                          has_target=has_target, global_scatter=global_scatter,
                          gate_admitted=stop == 0x51d226,
                          effective_first_bool=bool(uc.reg_read(UC_X86_REG_EBX) & 255)))
    return cases


def walk_code2_timers():
    cases = []
    # timer tuples are (start, duration), native frame100. -1 is stopped.
    for already_blocked, grace, movement in product(
        (False, True), ((100, 6), (95, 6), (94, 6), (-1, 0), (-1, 6)),
        ((-1, 0), (99, 2), (98, 2)),
    ):
        uc = fresh()
        uc.mem_write(LOCO + 0xc, u32(OWNER))
        uc.mem_write(LOCO + 0x1c, struct.pack('<iii', 10 * 256 + 128, 12 * 256 + 128, 0))
        uc.mem_write(OWNER + 0x6b7, bytes([already_blocked]))
        uc.mem_write(OWNER + 0x668, struct.pack('<iii', grace[0], 0, grace[1]))
        uc.mem_write(OWNER + 0x640, struct.pack('<iii', movement[0], 0, movement[1]))
        uc.mem_write(0xa8ed84, u32(100))
        uc.mem_write(0x8871e0, u32(RULES))
        uc.mem_write(RULES + 0x1768, u32(6))
        uc.mem_write(STACK + 0x40, u32(0))
        uc.reg_write(UC_X86_REG_EBP, LOCO)
        uc.reg_write(UC_X86_REG_ESI, 2)
        stop = run_checked(uc, 0x75b8a0, (0x75b979, 0x75c1f1), count=200)
        esp = uc.reg_read(UC_X86_REG_ESP)
        call_args = struct.unpack('<III', uc.mem_read(esp, 12)) if stop == 0x75b979 else None
        cases.append(dict(already_blocked=already_blocked, grace_start=grace[0],
                          grace_duration=grace[1], movement_start=movement[0],
                          movement_duration=movement[1], frame=100,
                          configured_grace=6, repath=call_args is not None,
                          urgency=call_args[2] if call_args else None,
                          goal=[call_args[0] & 65535, call_args[0] >> 16] if call_args else None,
                          out_blocked=bool(uc.mem_read(OWNER + 0x6b7, 1)[0]),
                          out_grace_start=struct.unpack('<i', uc.mem_read(OWNER + 0x668, 4))[0],
                          out_grace_duration=struct.unpack('<i', uc.mem_read(OWNER + 0x670, 4))[0]))
    return cases


def generate():
    return dict(source='unicorn/gamemd.exe', scatter_gates=scatter_gates(),
                walk_code2_timers=walk_code2_timers())


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='64 original Infantry interior scatter gate cases and 30 Walk code2 timer/urgency cases',
        assumptions=[
            'Fresh emulator per case; original Walk vtable and Is_Moving body executed',
            'Scatter starts after human special-sequence early gates; sequence=-1 bypasses animation permissions',
            'Both caller booleans1 matches Walk/Drive obstruction calls; first boolean also varied at the gate boundary',
            'Original second boolean1 bypasses human-only fallback; global scatter flag varied',
            'Fraidycat type+EBF confirmed through ReadINI52449A and string8259C8; ctor defaultfalse523795',
            'Scatter stops before destination/RNG work; admitted does not mean full Scatter success',
            'Code2 starts after obstruction classification and owner animation-stop virtual548; stops before FindPath',
            'Native timer padding initialized0; frame100 and configured grace6 supplied',
            'No complete scatter, pathfinding, scheduler, speed-calculation or gameplay parity claim',
        ],
        substitutions=[
            'GetMissionTimerEntry call51D176 supplied pointer to configured mission entry',
            'HasWeaponAbility(3) call51D1DC supplied false; original second boolean1 still takes native proceed branch',
        ],
        entry_points={'Infantry_scatter_gate':0x51d162, 'Walk_Is_Moving':0x75ab30,
                      'Walk_code2_timers':0x75b8a0}))
