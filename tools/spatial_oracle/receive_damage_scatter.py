"""Original TechnoClass::ReceiveDamage scatter-tail gates on both retaliation arms.

Executes from the refused (0x702BFE) or accepted (0x702B47) gate entry to the
Scatter call (0x702D0B), the common return (0x702D11) or the null-locomotor
E_POINTER call (0x702C3F). House control, MissionControl, Walk Is_Moving,
WhatAmI, GetTechnoType and rank readers are native; nothing is substituted.
An admitted row stops at the call, before InfantryClass/UnitClass Scatter, so
it pins the gates and arguments, not the Scatter body or its RNG draw.
"""
from itertools import product
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP
from tools.native_oracle import (
    SCRATCH, STACK_BASE, STACK_SIZE, RET_MAGIC, load_image, run_checked,
    finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords

ACTOR, TYPE, HOUSE, LOCO, RULES = [SCRATCH + i * 0x2000 for i in range(5)]
VTABLES = {'infantry': 0x7EB058, 'unit': 0x7F5C70, 'aircraft': 0x7E22A4,
           'building': 0x7E3EBC}
ENTRIES = {'refused': 0x702BFE, 'accepted': 0x702B47}
SCATTER_CALL, RETURN, COM_ERROR = 0x702D0B, 0x702D11, 0x702C3F
NULL_COORD = 0xB0EA90


def query(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    kind = case.get('kind', 'infantry')
    u.mem_write(ACTOR, dwords(VTABLES[kind]))
    # AbstractFlags +0x14: bit 2 marks a Foot; a Building has none.
    u.mem_write(ACTOR + 0x14, bytes([0x3 if kind == 'building' else 0x7]))
    # Each class's GetType reads one of these; the other is never read here.
    u.mem_write(ACTOR + 0x6C0, dwords(TYPE))
    u.mem_write(ACTOR + 0x6C4, dwords(TYPE))
    u.mem_write(ACTOR + 0x150, struct.pack('<f', case.get('rank', 0)))
    u.mem_write(ACTOR + 0x21C, dwords(HOUSE))
    u.mem_write(ACTOR + 0x674, dwords(0 if case.get('null_locomotor') else LOCO))
    u.mem_write(ACTOR + 0xAC, dwords(5))
    u.mem_write(ACTOR + 0x418, bytes([case.get('tether', False)]))
    u.mem_write(ACTOR + 0x5A4, dwords(1 if case.get('nav') else 0))
    u.mem_write(ACTOR + 0x2B4, dwords(1 if case.get('target') else 0))
    u.mem_write(TYPE + 0x29F, bytes([case.get('veteran_scatter', False)]))
    u.mem_write(TYPE + 0x2B1, bytes([case.get('elite_scatter', False)]))
    u.mem_write(HOUSE + 0x1EC, bytes([case.get('human', False)]))
    u.mem_write(HOUSE + 0x1ED, bytes([case.get('player_control', False)]))
    u.mem_write(0xA8B238, dwords(case.get('game_mode_nonzero', True)))
    u.mem_write(LOCO, dwords(0x7F69F8))
    u.mem_write(LOCO + 0x30, bytes([case.get('moving', False)]))
    u.mem_write(0xA8E3A8 + 5 * 32 + 9, bytes([case.get('mission_scatter', True)]))
    u.mem_write(0x8871E0, dwords(RULES))
    u.mem_write(RULES + 0x17ED, bytes([case.get('player_scatter', False)]))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.reg_write(UC_X86_REG_ESI, ACTOR)
    u.reg_write(UC_X86_REG_ESP, sp)
    writes = []

    def written(uc, access, address, size, value, data):
        if not STACK_BASE <= address < STACK_BASE + STACK_SIZE:
            writes.append((hex(address), size, value))

    hook = u.hook_add(UC_HOOK_MEM_WRITE, written)
    entry = ENTRIES[case['retaliation']]
    stop = run_checked(u, entry, (SCATTER_CALL, RETURN, COM_ERROR), count=5000,
                       required_addresses=[entry])
    u.hook_del(hook)
    assert not writes, writes
    row = dict(input=case, stop=hex(stop), scatter=stop == SCATTER_CALL)
    if stop == SCATTER_CALL:
        esp = u.reg_read(UC_X86_REG_ESP)
        coord, forced, no_kidding = struct.unpack('<3I', u.mem_read(esp, 12))
        assert u.reg_read(UC_X86_REG_ECX) == ACTOR
        row['arguments'] = dict(
            coordinate=hex(coord),
            coordinate_xyz=list(struct.unpack('<3i', u.mem_read(coord, 12))),
            forced=forced, no_kidding=no_kidding,
            receiver=hex(struct.unpack('<I', u.mem_read(VTABLES[kind] + 0x174, 4))[0]))
    return row


ABILITIES = [(0, False, False), (0, True, True), (1, True, False), (1, False, True),
             (2, False, True), (2, True, False), (2, False, False)]


def generate():
    cases = []
    # Owner and rank grid on both arms (a Foot with no Target, NavCom or motion).
    for retaliation, human, player in product(ENTRIES, (False, True), (False, True)):
        for rank, veteran, elite in ABILITIES:
            cases.append(dict(retaliation=retaliation, human=human, player_scatter=player,
                              rank=rank, veteran_scatter=veteran, elite_scatter=elite))
    # Every class through each gate, computer-owned and PlayerScatter set.
    for retaliation, kind in product(ENTRIES, ('infantry', 'unit', 'aircraft')):
        for mission, tether, moving, target, nav in product((False, True), repeat=5):
            cases.append(dict(retaliation=retaliation, kind=kind, mission_scatter=mission,
                              tether=tether, moving=moving, target=target, nav=nav,
                              player_scatter=True))
    # A Building is no Foot on either arm.
    for retaliation in ENTRIES:
        cases.append(dict(retaliation=retaliation, kind='building'))
    # The null locomotor reaches E_POINTER only past the mission and tether.
    for mission, tether in product((False, True), repeat=2):
        cases.append(dict(retaliation='refused', null_locomotor=True,
                          mission_scatter=mission, tether=tether))
    cases.append(dict(retaliation='accepted', null_locomotor=True))
    # The real house-control mode gate on the refused arm.
    for human, control, mode in product((False, True), repeat=3):
        cases.append(dict(retaliation='refused', human=human, player_control=control,
                          game_mode_nonzero=mode))
    return [query(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original TechnoClass::ReceiveDamage scatter-tail gates (0x702B47..0x702D0B) on '
              'the accepted and refused retaliation arms: Foot flag, MissionControl Scatter, '
              'tether, Walk Is_Moving, Target, NavCom, WhatAmI, house control, PlayerScatter and '
              'rank abilities, with the Scatter call arguments. No Scatter body, RNG, '
              'destination or full damage parity claim.',
        entry_points={'refused_gates': 0x702BFE, 'accepted_gates': 0x702B47,
                      'scatter_call': SCATTER_CALL, 'return': RETURN, 'com_error': COM_ERROR,
                      'mission_control': 0x5B3A00, 'house_control': 0x50B730,
                      'is_veteran': 0x74FF90, 'is_elite': 0x750010,
                      'walk_is_moving': 0x75AB30},
        assumptions=['Supplied Infantry/Unit/Aircraft/Building buffers with original vtables, '
                     'AbstractFlags, type, house, rank float and Walk interface state; current '
                     'Guard uses an explicit MissionControl Scatter byte.',
                     'Target/NavCom nonnull sentinels are compared only, never dereferenced.',
                     'The ShouldRetaliate answer selects the entry; the Scatter call is observed '
                     'at 0x702D0B and not executed.'],
        substitutions=[]))
