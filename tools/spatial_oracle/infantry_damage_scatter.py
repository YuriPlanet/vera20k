"""Original Infantry Scatter admission for ReceiveDamage's two Scatter calls.

The Infantry receiver passes false/false (0x518C56); TechnoClass's scatter tail
passes true/false (0x702D0B). Executes from the real entry through refusal or
the first coordinate read. House control, mission, Walk moving, Doing and
rank/ability readers are native. No calls are substituted. Accepted cases stop
before RNG/destination selection; this corpus proves admission only, not full
displacement or damage ordering.
"""
from itertools import product
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import (
    SCRATCH, STACK_BASE, STACK_SIZE, RET_MAGIC, load_image, run_checked,
    finish_vectors, provenance,
)
from tools.spatial_oracle.map_queries import dwords

ACTOR, TYPE, HOUSE, LOCO, RULES, SOURCE = [SCRATCH + i * 0x2000 for i in range(6)]


def query(case):
    u = Uc(UC_ARCH_X86, UC_MODE_32)
    load_image(u)
    u.mem_map(SCRATCH, 0x10000)
    u.mem_map(STACK_BASE, STACK_SIZE)
    u.mem_map(RET_MAGIC, 0x1000)
    u.mem_write(ACTOR, dwords(0x7EB058))
    u.mem_write(ACTOR + 0x6C0, dwords(TYPE))
    u.mem_write(ACTOR + 0x6C4, dwords(case.get('doing', -1)))
    u.mem_write(ACTOR + 0x150, struct.pack('<f', case.get('rank', 0)))
    u.mem_write(ACTOR + 0x21C, dwords(HOUSE))
    u.mem_write(ACTOR + 0x674, dwords(LOCO))
    u.mem_write(ACTOR + 0xAC, dwords(5))
    u.mem_write(ACTOR + 0x5D4, dwords(1 if case.get('team') else 0))
    u.mem_write(ACTOR + 0x5A4, dwords(1 if case.get('nav') else 0))
    u.mem_write(ACTOR + 0x2B4, dwords(1 if case.get('target') else 0))
    u.mem_write(TYPE + 0xEBF, bytes([case.get('fraidycat', True)]))
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
    u.mem_write(SOURCE, dwords(1000, 1000, 0))
    sp = STACK_BASE + STACK_SIZE - 0x1000
    u.mem_write(sp, dwords(RET_MAGIC, SOURCE, int(case.get('forced', False)),
                           int(case.get('no_kidding', False))))
    u.reg_write(UC_X86_REG_ECX, ACTOR)
    u.reg_write(UC_X86_REG_ESP, sp)
    before = bytes(u.mem_read(ACTOR, 0x700))
    stop = run_checked(u, 0x51D0D0, (0x51D226, RET_MAGIC), count=2000,
                       required_addresses=[0x51D0D0])
    admitted = stop == 0x51D226
    assert bytes(u.mem_read(ACTOR, 0x700)) == before
    assert u.reg_read(UC_X86_REG_ESP) == sp + (-80 if admitted else 16)
    return dict(input=case, admitted=admitted)


def generate():
    cases = []
    abilities = [(0, False, False), (0, True, True), (1, True, False),
                 (2, False, True), (2, True, False)]
    for fraidy, human, team, nav, player in product((False, True), repeat=5):
        for rank, veteran, elite in abilities:
            cases.append(dict(fraidycat=fraidy, human=human, team=team, nav=nav,
                              player_scatter=player, rank=rank,
                              veteran_scatter=veteran, elite_scatter=elite))
    for doing, human in product(range(-1, 42), (False, True)):
        cases.append(dict(doing=doing, human=human, player_scatter=True))
    for moving, mission, target, fraidy in product((False, True), repeat=4):
        cases.append(dict(moving=moving, mission_scatter=mission,
                          target=target, fraidycat=fraidy, player_scatter=True))
    for human, control, mode, team in product((False, True), repeat=4):
        cases.append(dict(human=human, player_control=control,
                          game_mode_nonzero=mode, team=team))
    # The Techno tail's true/false call: a moving locomotor drops forced.
    for doing, human, moving in product(range(-1, 42), (False, True), (False, True)):
        cases.append(dict(forced=True, doing=doing, human=human, moving=moving))
    for moving, mission, target, fraidy, human, team in product((False, True), repeat=6):
        cases.append(dict(forced=True, moving=moving, mission_scatter=mission,
                          target=target, fraidycat=fraidy, human=human, team=team))
    for moving, player, human in product((False, True), repeat=3):
        for rank, veteran, elite in abilities:
            cases.append(dict(forced=True, moving=moving, player_scatter=player,
                              human=human, rank=rank, veteran_scatter=veteran,
                              elite_scatter=elite))
    return [query(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='554 original Infantry Scatter admission cases for the damage calls: 278 false/false (Infantry receiver) and 276 true/false (Techno scatter tail). Includes Doing -1..41, real house-control mode gate, mission, Walk moving, target, rank/abilities, PlayerScatter, Team and independent NavCom. No RNG/destination or full damage parity claim.',
        entry_points={'scatter': 0x51D0D0, 'admitted_stop': 0x51D226,
                      'house_control': 0x50B730, 'ability': 0x70D0D0,
                      'walk_is_moving': 0x75AB30},
        assumptions=['Supplied valid Infantry, type, house and Walk interface state, original vtables.',
                     'Boolean arguments as each ReceiveDamage call passes them; current Guard uses an explicit MissionControl Scatter byte.',
                     'The coordinate is first read at the admitted stop, so true/false rows keep the source coordinate the tail passes as null.',
                     'Team/NavCom/Target nonnull sentinels are compared only, never dereferenced in this prefix.'],
        substitutions=[]))
