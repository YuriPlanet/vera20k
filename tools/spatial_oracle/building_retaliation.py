"""Original building retaliation: BuildingClass::ReceiveDamage's block after
the TechnoClass tail (0x442942..0x442A95).

The block runs on the building_guard_attack fixture's building (ESI) with the
damage source in EBP and ReceiveDamage's result at [esp+0x30], as
BuildingClass::ReceiveDamage (0x442230) leaves them at 0x442942:

- no source: straight to the tail (0x442A95); result 0: to 0x442B37;
- unless the type is Insignificant= (+0x232) or vt+0x80 answers (1x1 with
  UndeploysInto=, 0x465D40), HouseClass::NotifyUnderAttack (0x4F93E0, skipped
  here and recorded with its argument);
- +0x53C = the source house's index (+0x30), which only a CRC fold and a
  `!= -1` test read;
- the tail when the raw mission (+0xAC) is Selling, the owner is allied with
  the source (0x4F9A90), weapon 0 (vt+0x3F8, 0x4526F0) has no WeaponType or
  its projectile is AA= (BulletType+0x2A4), or TarCom (+0x2B4) is in range
  (vt+0x3AC);
- an Aircraft source (WhatAmI 2), or a human owner (0x50B730) while
  [CombatDamage] PlayerReturnFire= (Rules+0x17EC) is clear: unless the +0x388
  FacingClass is rotating (0x4C9480) or the building is not operational
  (vt+0x350 0x4555D0), one Scenario Random::Next (0x65C780) and
  Set_Desired(+0x388, (r & 0xFF) << 8) (0x4C9220);
- any other: BuildingClass::SetTarget(source) (vt+0x3C8, 0x443B90).

Native: the block, vt+0x80, IsAlliedWith, BuildingClass::GetWeapon, WhatAmI,
BuildingClass::SetTarget with Queue_Mission and Commence, FacingClass's
Is_Rotating and Set_Desired, and Random::Next; IsOperational (0x4555D0) on the
`native_operational` rows. Answered by the fixture from the row: IsOperational
on the others, IsCloseEnough (vt+0x3AC's target 0x6F7780), IsHumanPlayer, the
EMP test, and TechnoClass::SetTarget (0x6FCDB0, which writes +0x2B4). The +0x388 FacingClass is built by its constructor (0x4C91C0)
and Set_ROT (0x4C9680) with the row's ROT=.

The source is the fixture's unit (the Techno at refinery_dock.ACTOR, also the
fixture's TarCom when a row has one), owned by unit_entry.ENEMY (house index
3), or a stand-in Aircraft (the AircraftClass vtable 0x7E22A4 over zeroed
memory, owned by ENEMY). The building's owner is refinery_dock.HOUSE (index 0),
allied with ENEMY on `allied` rows (+0x5788 bits).

Usage: python -m tools.spatial_oracle.building_retaliation [--check|--write]
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBP, UC_X86_REG_ECX, UC_X86_REG_EIP,
                               UC_X86_REG_ESI, UC_X86_REG_ESP)
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.spatial_oracle import building_construction as bc
from tools.spatial_oracle import building_guard_attack as bga
from tools.spatial_oracle import slave_manager as sm
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.refinery_dock import ACTOR, HOUSE, RULES
from tools.spatial_oracle.unit_entry import ENEMY
from tools.spatial_oracle.unit_scatter_state import SP
from tools.spatial_oracle.unit_source_scatter import SCENARIO

BLOCK, TAIL, NO_RESULT = 0x442942, 0x442A95, 0x442B37
NOTIFY, NEXT, NEXT_RETURN = 0x4F93E0, 0x65C780, 0x442A78
SET_DESIRED, IS_ROTATING = 0x4C9220, 0x4C9480
FACING_CTOR, SET_ROT = 0x4C91C0, 0x4C9680
QUEUE, COMMENCE = 0x5B35E0, 0x5B3570
# Is_Operational_For_Output's two returns (0x4555D0), reached only when it runs.
OPERATIONAL_TRUE, OPERATIONAL_FALSE = 0x4556BA, 0x4556C0
AIRCRAFT_VTABLE = 0x7E22A4
AIRCRAFT = sm.REGION + 0xE000
SCRATCH = sm.REGION + 0xE800
# Set_Desired runs natively: building_guard_attack answers its entry unless the
# address it watches is moved off it.
bga.SET_DESIRED = 0xDEAD0001


def signed(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


def word(u, address):
    return struct.unpack('<H', u.mem_read(address, 2))[0]


def facing(u, building):
    """The +0x388 FacingClass: desired and start words, the timer's start and
    time left, and the ROT word."""
    f = building + 0x388
    return dict(desired=word(u, f), start=word(u, f + 4), timer_start=signed(u, f + 8),
                timer_left=signed(u, f + 0x10), rot=word(u, f + 0x14))


def run(case):
    u, read32, building, frame, events = bga.fixture(dict(case, mission=case.get('mission', 'guard')))
    log = []

    def hook(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address == NOTIFY:
            log.append(['notify', read32(sp + 4) == building])
            u.reg_write(UC_X86_REG_EIP, read32(sp))
            u.reg_write(UC_X86_REG_ESP, sp + 8)
        elif address == NEXT:
            stream = 'scenario' if u.reg_read(UC_X86_REG_ECX) == SCENARIO + 0x218 else 'other'
            log.append(['next', stream])
        elif address == NEXT_RETURN:
            log.append(['drawn', u.reg_read(UC_X86_REG_EAX)])
        elif address == SET_DESIRED:
            log.append(['set_desired', word(u, read32(sp + 4))])
        elif address == IS_ROTATING:
            log.append(['is_rotating'])
        elif address == QUEUE:
            log.append(['queue_mission', bga.MISSION_NAME.get(read32(sp + 4), read32(sp + 4)),
                        read32(sp + 8)])
        elif address == COMMENCE:
            log.append(['commence'])
        elif address in (OPERATIONAL_TRUE, OPERATIONAL_FALSE):
            log.append(['operational', address == OPERATIONAL_TRUE])

    u.hook_add(UC_HOOK_CODE, hook)
    u.mem_write(HOUSE + 0x30, dwords(0))
    u.mem_write(ENEMY + 0x30, dwords(3))
    u.mem_write(HOUSE + 0x5788, dwords(1 | ((1 << 3) if case.get('allied') else 0)))
    u.mem_write(RULES + 0x17EC, bytes([case.get('player_return_fire', 0)]))
    kind = sm.YTYPE
    u.mem_write(kind + 0x232, bytes([case.get('insignificant', 0)]))
    u.mem_write(kind + 0x408, dwords(kind if case.get('undeploys') else 0))
    u.mem_write(kind + 0xEF0, dwords(case.get('foundation', 3)))
    source = {'unit': ACTOR, 'aircraft': AIRCRAFT, None: 0}[case.get('source', 'unit')]
    u.mem_write(ACTOR + 0x21C, dwords(ENEMY))
    u.mem_write(AIRCRAFT, bytes(0x100))
    u.mem_write(AIRCRAFT, dwords(AIRCRAFT_VTABLE))
    u.mem_write(AIRCRAFT + 0x21C, dwords(ENEMY))
    bc.invoke(u, FACING_CTOR, building + 0x388)
    bc.invoke(u, SET_ROT, building + 0x388, case.get('rot', 10) & 0xFFFFFFFF)
    if case.get('initial_desired') is not None:
        u.mem_write(building + 0x388, struct.pack('<HH', case['initial_desired'], case['initial_desired']))
    if case.get('rotating'):
        u.mem_write(SCRATCH, dwords(0x4000))
        bc.invoke(u, SET_DESIRED, building + 0x388, SCRATCH)
        u.mem_write(bc.FRAME, dwords(read32(bc.FRAME) + case.get('advance', 0)))
    u.mem_write(building + 0x53C, dwords(0x7777))
    before = facing(u, building)
    indices = lambda: [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]
    rng_before = indices()
    log.clear()
    events.clear()
    u.mem_write(SP, bytes(0x80))
    u.mem_write(SP + 0x30, dwords(case.get('result', 1)))
    u.mem_write(SP + 0x2C, dwords(case.get('stack_junk', 0)))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ESI, building)
    u.reg_write(UC_X86_REG_EBP, source)
    stubbed = bga.IS_OPERATIONAL
    if case.get('native_operational'):
        # Is_Operational_For_Output runs natively: online (+0x660), so its
        # answer is the building's health, its type's power terms and
        # vt+0x184's mission (current, or queued while current is -1).
        u.mem_write(building + 0x660, bytes([1]))
        bga.IS_OPERATIONAL = 0xDEAD0002
    try:
        end = run_checked(u, BLOCK, (TAIL, NO_RESULT), count=200_000)
    finally:
        bga.IS_OPERATIONAL = stubbed
    set_desired_calls = [entry for entry in log if entry[0] == 'set_desired']
    return dict(
        input=case,
        end='tail' if end == TAIL else 'no_result',
        frame=read32(bc.FRAME),
        ping=[entry[1] for entry in log if entry[0] == 'notify'],
        who_last=signed(u, building + 0x53C),
        target=bga.name_of(read32(building + 0x2B4)),
        mission=bga.MISSION_NAME.get(signed(u, building + 0xAC), signed(u, building + 0xAC)),
        queued=bga.MISSION_NAME.get(signed(u, building + 0xB4), signed(u, building + 0xB4)),
        set_target=[event[1] for event in events if event[0] == 'set_target'],
        missions=[entry for entry in log if entry[0] in ('queue_mission', 'commence')],
        rotating_tested=any(entry[0] == 'is_rotating' for entry in log),
        operational=[entry[1] for entry in log if entry[0] == 'operational'],
        draws=[entry[1] for entry in log if entry[0] == 'next'],
        drawn=[entry[1] for entry in log if entry[0] == 'drawn'],
        random_ranged=sum(1 for event in events if event[0] == 'random_ranged'),
        set_desired=[entry[1] for entry in set_desired_calls],
        set_desired_changed=(u.reg_read(UC_X86_REG_EAX) & 0xFF) if set_desired_calls else None,
        facing_before=before,
        facing_after=facing(u, building),
        rng=[rng_before, indices()],
    )


def cases():
    human = dict(human=True)
    return [
        # The early exits.
        dict(name='no_source', source=None),
        dict(name='result0', result=0),
        # A computer-owned building: SetTarget(source), which keeps it in range and
        # drops it out of range.
        dict(name='ai_unit_no_target', human=False),
        dict(name='ai_unit_out_of_range', human=False, in_range=False),
        dict(name='ai_unit_result2', human=False, result=2),
        dict(name='ai_unit_result3', human=False, result=3),
        dict(name='ai_target_out_of_range', human=False, target=True, in_range=False),
        # A human owner with PlayerReturnFire clear, or an aircraft source: the twitch.
        dict(name='human_unit', **human),
        dict(name='human_unit_player_return_fire', player_return_fire=1, **human),
        dict(name='ai_aircraft', source='aircraft', human=False),
        dict(name='ai_aircraft_player_return_fire', source='aircraft', human=False, player_return_fire=1),
        dict(name='human_aircraft_player_return_fire', source='aircraft', player_return_fire=1, **human),
        dict(name='ai_aircraft_seed2', source='aircraft', human=False, seed=2),
        *[dict(name=f'human_seed{seed}', seed=seed, **human) for seed in (1, 8, 9, 12345)],
        # The twitch's gates: rotating (up to its last frame), not operational.
        dict(name='human_aircraft_rotating', source='aircraft', rotating=True, **human),
        dict(name='rotating_last_frame', rotating=True, advance=5, **human),
        dict(name='rotating_done', rotating=True, advance=6, **human),
        dict(name='human_not_operational', operational=False, **human),
        dict(name='construction', mission='none', operational=False, **human),
        # ROT: none, negative, clamped, one.
        dict(name='human_rot0', rot=0, **human),
        dict(name='human_rot_negative', rot=-1, **human),
        dict(name='human_rot200', rot=200, **human),
        dict(name='human_rot1', rot=1, **human),
        # A draw that repeats the desired word, and junk in the direction's high word.
        dict(name='human_same_desired', initial_desired=0xD500, **human),
        dict(name='human_stack_junk', stack_junk=0x5A5A0000, **human),
        # The tail's exits.
        dict(name='selling', mission='selling', **human),
        # A queued sale passes the raw mission test; Is_Operational, run
        # natively, reads the queued mission and refuses both arms.
        dict(name='queued_selling', mission='none', queued='selling', native_operational=True,
             **human),
        dict(name='ai_queued_selling', mission='none', queued='selling', native_operational=True,
             human=False),
        dict(name='native_operational', native_operational=True, **human),
        dict(name='allied', allied=True, **human),
        dict(name='unarmed', armed=False, **human),
        dict(name='anti_air', anti_air=True, **human),
        dict(name='target_in_range', target=True, in_range=True, **human),
        dict(name='target_out_of_range', target=True, in_range=False, **human),
        # The ping's gates.
        dict(name='insignificant', insignificant=1, **human),
        dict(name='undeploys_1x1', undeploys=True, foundation=0, **human),
        dict(name='undeploys_2x2', undeploys=True, foundation=3, **human),
        dict(name='result4', result=4, **human),
        # BuildingClass::SetTarget's sale arm (no retail TickTank= or Artillary= type).
        dict(name='ai_artillary_out_of_range', human=False, in_range=False, artillary=True),
        dict(name='ai_artillary_emp', human=False, in_range=False, artillary=True, under_emp=True),
        dict(name='tick_tank_human_player_return_fire', player_return_fire=1, in_range=False,
             tick_tank=True, **human),
    ]


def generate():
    return dict(source='tools/spatial_oracle/building_retaliation.py',
                retaliation=[run(case) for case in cases()])


def main(argv=None):
    finish_vectors(
        generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: provenance(
            scope='BuildingClass::ReceiveDamage\'s retaliation block 0x442942..0x442A95: the '
                  'NotifyUnderAttack gate, the +0x53C write, the early exits, BuildingClass::SetTarget '
                  '(0x443B90) and the Scenario-drawn +0x388 turn',
            entry_points={'block': BLOCK, 'building_set_target': bga.BUILDING_SET_TARGET,
                          'set_desired': SET_DESIRED, 'is_rotating': IS_ROTATING, 'next': NEXT},
            assumptions=['building_guard_attack\'s fixture building (the slave_manager refinery with '
                         'Building vtables), armed by its supplied WeaponType in Weapon[0]',
                         'the source is the fixture\'s Techno (refinery_dock.ACTOR) or a stand-in '
                         'object with the AircraftClass vtable, owned by house index 3',
                         'ReceiveDamage\'s result and the direction temporary\'s high word are placed '
                         'at [esp+0x30] and [esp+0x2E] as 0x442230 leaves them'],
            substitutions=['HouseClass::NotifyUnderAttack 0x4F93E0 skipped and recorded',
                           'IsOperational 0x4555D0 (except on native_operational rows), IsCloseEnough '
                           '0x6F7780, IsHumanPlayer 0x50B730, the EMP test 0x70EFD0 and '
                           'TechnoClass::SetTarget 0x6FCDB0 answered from the row (building_guard_attack)']),
        argv=argv)


if __name__ == '__main__':
    main()
