"""Original Gattling Cannon stages on a building.

The building_guard_attack fixture's building (the slave_manager refinery with
Building vtables) is made a gattling type: IsGattling (TechnoType +0xCD5),
WeaponStages +0xCD8, Stage1..6 +0xCDC, EliteStage1..6 +0xCF4, RateUp +0xD0C,
RateDown +0xD10 (retail [YAGGUN]: 3, 200/400/600, 100/200/300, 1, 50) and six
supplied WeaponTypes in Weapon[0..5] (+0x898 + 0x1C * i; elite +0xA94), each
with the fixture BulletType and a one-item Report= list naming its slot.
The bodies IncreaseGattlingStage (0x70DE70), UpdateGattlingStage (0x70E000),
DecreaseValue (0x70DE40) and SetStage (0x70DDD0), SelectWeapon (0x6F3330),
BuildingClass::GetWeapon (0x4526F0), IsElite (0x750010), Queue_Mission and
Commence run natively; their entries are recorded with the caller and the
argument. RandomClass::Next (0x65C780) is recorded by stream: g_MainRng
(0x886B88, seeded per row through 0x65C6D0) or the Scenario's. VocClass::PlayAt
(0x7509E0) and SoundEvent::Release (0x406060) are answered by the
building_construction fixture and recorded; VocHandle::StopAndClear
(0x405D40) runs natively and takes its early exit ([0x87E2A0] is 0).

- `attack` rows: one BuildingClass::Mission_Attack (0x44ACF0) with
  GetFireError answered from the row (building_guard_attack's hooks).
- `guard` rows: one BuildingClass::Mission_Guard (0x4496B0).
- `idle_decay` rows: BuildingClass::Update's block 0x43FE5B..0x43FF91 (the
  IsAlive test, the first +0x148 step, the ammo refill, the idle decay and the
  second +0x148 step), with Rules+0xE04 (GuardAreaTargetingDelay) and
  LastFireFrame (+0x120) from the row. A dead building ends at 0x440573.
- `cadence` rows: frames through BuildingClass::Update's pieces in its order,
  each native: the ready check that commences unless BState is 0
  (0x43FE27..0x43FE54), TechnoClass::AI_Update's +0xC4 count and mission
  dispatch (0x6FA646..0x6FA65A), the block above, and the ready check that
  commences (0x43FF91..0x43FFB4). FireAt is answered with its rearm write
  (+0x2EC = {Frame, _, ROF of the fired weapon}) and LastFireFrame
  (+0x120 = Frame, 0x6FF743); GetFireError with the rearm test.
- `report_gate` rows: TechnoClass::FireAt's report block (0x6FF349..0x6FF394)
  on the building with ESI = the building and EBX = the weapon.

Usage: python -m tools.spatial_oracle.building_gattling [--check|--write]
"""
from pathlib import Path
import struct

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDI,
                               UC_X86_REG_EIP, UC_X86_REG_ESI, UC_X86_REG_ESP)
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.spatial_oracle import building_construction as bc
from tools.spatial_oracle import building_guard_attack as bga
from tools.spatial_oracle import slave_manager as sm
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.unit_source_scatter import SCENARIO
from tools.spatial_oracle.unit_scatter_state import SP

INCREASE, UPDATE, DECREASE_VALUE, SET_STAGE = 0x70DE70, 0x70E000, 0x70DE40, 0x70DDD0
STAGE_ENTRIES = {INCREASE: 'increase', UPDATE: 'update', DECREASE_VALUE: 'decrease_value',
                 SET_STAGE: 'set_stage'}
NEXT, SEED = 0x65C780, 0x65C6D0
MAIN_RNG = 0x886B88
PLAY_AT, RELEASE = 0x7509E0, 0x406060
AMBIENT_SOUND_RETURNS = (0x5F3EDE, 0x5F3F11)
AI_UPDATE_DISPATCH = (0x6FA646, 0x6FA65A)
OVERRIDE_MISSION = 0x7013A0
IDLE_DECAY = (0x43FE5B, (0x43FF91, 0x440573))
REPORT_GATE = (0x6FF349, 0x6FF394)
RULES = 0x200B0000
# Retail [YAGGUN] and its weapons' ROF (base; the elite set is dormant in
# retail and uses the same rates here).
RETAIL = dict(stages=3, stage=[200, 400, 600, 0, 0, 0], elite_stage=[100, 200, 300, 0, 0, 0],
              rate_up=1, rate_down=50)
ROF = [16, 16, 16, 8, 16, 4]
WEAPONS = [sm.REGION + 0xB000 + 0x200 * i for i in range(6)]
ELITE_WEAPONS = [sm.REGION + 0xD000 + 0x200 * i for i in range(6)]
REPORTS = sm.REGION + 0xF000


def signed(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


def fixture(case):
    """building_guard_attack's fixture made a gattling type, with the row's
    stage state and the recording hooks."""
    u, read32, building, frame, events = bga.fixture(case)
    kind = sm.YTYPE
    table = {**RETAIL, **case.get('table', {})}
    gattling = case.get('gattling', True)
    u.mem_write(kind + 0xCD5, bytes([gattling]))
    u.mem_write(kind + 0xCD8, dwords(table['stages'], *table['stage'], *table['elite_stage']))
    u.mem_write(kind + 0xD0C, dwords(table['rate_up'], table['rate_down']))
    if gattling:
        for i, (weapon, elite) in enumerate(zip(WEAPONS, ELITE_WEAPONS)):
            for index, pointer in ((i, weapon), (6 + i, elite)):
                u.mem_write(pointer, bytes(0x200))
                u.mem_write(pointer + 0xA0, dwords(bga.BULLET))
                u.mem_write(pointer + 0xA4, dwords(ROF[i]))
                count = case.get('report_count', 1)
                u.mem_write(REPORTS + 0x10 * index, dwords(*[index + 1] * 4))
                u.mem_write(pointer + 0xC0, dwords(REPORTS + 0x10 * index))
                u.mem_write(pointer + 0xCC, dwords(count))
            u.mem_write(kind + 0x898 + 0x1C * i, dwords(weapon))
            u.mem_write(kind + 0xA94 + 0x1C * i, dwords(elite))
        if not case.get('armed', True):
            # Unarmed as building_guard_attack means it: no weapon 0 (Is_Armed).
            u.mem_write(kind + 0x898, dwords(0))
            u.mem_write(kind + 0xA94, dwords(0))
    state = case.get('state', {})
    u.mem_write(building + 0x140, dwords(state.get('stage', 0), state.get('value', 0)))
    u.mem_write(building + 0x4B8, bytes([state.get('latch', 0)]))
    u.mem_write(building + 0xC4, dwords(state.get('counter', 0)))
    u.mem_write(building + 0x150, struct.pack('<f', state.get('veterancy', 0.0)))
    u.mem_write(building + 0x120, dwords(state.get('last_fire_frame', -100)))
    u.mem_write(building + 0x148, dwords(state.get('turret_count', 0)))
    # A nonzero +0x2FC keeps Update's ammo refill (0x43FE8E..0x43FEB8) inert.
    u.mem_write(building + 0x2FC, dwords(1))
    u.mem_write(RULES + 0xE04, dwords(case.get('delay', 36)))
    assert read32(0x87E2A0) == 0, 'StopAndClear must take its early exit'
    bc.invoke(u, SEED, MAIN_RNG, case.get('main_seed', 1))

    def hook(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        if address in STAGE_ENTRIES:
            events.append(['stage_call', STAGE_ENTRIES[address], read32(sp), signed(u, sp + 4)])
        elif address == NEXT:
            ecx = u.reg_read(UC_X86_REG_ECX)
            stream = {MAIN_RNG: 'main', SCENARIO + 0x218: 'scenario'}.get(ecx, hex(ecx))
            events.append(['rng', stream])
        elif address == PLAY_AT:
            # building_construction's hook answers PlayAt first: ECX still
            # holds the sound index and EIP is already the return address.
            # ObjectClass::AI's two ambient-sound calls (0x5F3ED9, 0x5F3F0C)
            # run every frame of the cadence and are not recorded.
            caller = u.reg_read(UC_X86_REG_EIP)
            if caller not in AMBIENT_SOUND_RETURNS:
                events.append(['play_at', u.reg_read(UC_X86_REG_ECX), caller])
        elif address == RELEASE:
            events.append(['release', u.reg_read(UC_X86_REG_ECX) - building])

    u.hook_add(UC_HOOK_CODE, hook)
    return u, read32, building, frame, events


def gattling_state(u, building):
    return dict(stage=signed(u, building + 0x140), value=signed(u, building + 0x144),
                latch=u.mem_read(building + 0x4B8, 1)[0], counter=signed(u, building + 0xC4),
                turret_count=signed(u, building + 0x148),
                last_fire_frame=signed(u, building + 0x120))


def kept(events):
    """The recorded events. building_guard_attack's RandomRanged entries are the
    Scenario draws (RandomRanged 0x65C7E0 does not call Next); its Next entries
    are dropped for the `rng` entries, which name the stream."""
    return [['rng', 'scenario_ranged', *event[1:]] if event[0] == 'random_ranged' else event
            for event in events if event[0] != 'random_next']


def one_call(entry):
    def run(case):
        u, read32, building, frame, events = fixture(case)
        value = bc.invoke(u, entry, building, *case.get('args', []))
        return dict(input=case, returns=struct.unpack('<i', dwords(value))[0], events=kept(events),
                    **bga.state(u, building), gattling=gattling_state(u, building))
    return run


bga.MISSION.setdefault('area_guard', 0xB)
bga.MISSION.setdefault('construction', 0x12)
bga.MISSION_NAME.setdefault(0xB, 'area_guard')
bga.MISSION_NAME.setdefault(0x12, 'construction')
AREA_GUARD = 0x449A40


def attack_cases():
    base = dict(mission='attack', target=True)
    cases = [dict(base, name=f'code_{code}', errors=[code], state=dict(value=199, counter=3))
             for code in range(12)]
    cases += [
        # OK at a stage threshold with the loop live: the stage-up restarts it.
        dict(base, name='ok_stage_up', errors=[0], state=dict(value=200, latch=1, counter=1)),
        # REARM overshooting the cap, and FACING at the cap: no add.
        dict(base, name='rearm_overshoot', errors=[3], state=dict(value=599, stage=1, latch=1, counter=2)),
        dict(base, name='facing_at_cap', errors=[2], state=dict(value=600, stage=2, latch=0, counter=2)),
        # RANGE at stage 2: the drop tail's decay, stage down once.
        dict(base, name='range_stage2', errors=[8], state=dict(value=450, stage=2, latch=1, counter=3)),
        # BUSY with no count: Update(0) zeroes the value.
        dict(base, name='busy_zero_ticks', errors=[10], state=dict(value=300, stage=1, latch=1, counter=0)),
        # The drop tail of a building whose effective mission is Wait.
        dict(name='drop_waiting', mission='none', queued='wait', target=True, errors=[5],
             state=dict(value=300, stage=1, latch=1, counter=3)),
        dict(name='no_target', mission='attack', state=dict(value=300, stage=1, latch=1, counter=3)),
        dict(base, name='ok_no_report', errors=[0], report_count=0, state=dict(value=10, counter=1)),
        dict(base, name='ok_elite', errors=[0], state=dict(value=99, counter=1, veterancy=2.0)),
        dict(base, name='ok_latched', errors=[0], state=dict(value=50, latch=1, counter=1)),
        # A plain building: OK and REARM advance +0x148, the rest keep +0xC4.
        *[dict(base, name=f'plain_code_{code}', gattling=False, errors=[code], state=dict(counter=3))
          for code in (0, 2, 3, 10)],
    ]
    return cases


def guard_cases():
    state = dict(value=450, stage=2, latch=1, counter=7)
    return [
        dict(name='armed_idle', mission='guard', seed=1, state=state),
        dict(name='armed_target', mission='guard', target=True, state=state),
        dict(name='armed_emp_cannon', mission='guard', target=True, emp_cannon=True, state=state),
        dict(name='sticky', mission='sticky', seed=1, state=dict(state, counter=2)),
        dict(name='area_guard', mission='area_guard', seed=1, state=dict(state, counter=16),
             entry=AREA_GUARD),
        dict(name='unarmed_status0', mission='guard', armed=False, status=0, seed=1,
             state=dict(state, counter=15)),
        dict(name='unarmed_status1', mission='guard', armed=False, status=1, seed=1,
             state=dict(state, counter=1)),
        dict(name='plain_armed_idle', mission='guard', seed=1, gattling=False, state=dict(counter=7)),
    ]


def guard(case):
    return one_call(case.get('entry', bga.MISSION_GUARD))(case)


def idle_decay(case):
    u, read32, building, frame, events = fixture(case)
    if case.get('dead'):
        u.mem_write(building + 0x90, bytes([0]))
    u.mem_write(building + 0x120, dwords(frame - case['since']))
    bc.run_block(u, building, IDLE_DECAY)
    ended = u.reg_read(UC_X86_REG_EIP)
    return dict(input=case, ended=ended, events=kept(events), gattling=gattling_state(u, building))


def idle_decay_cases():
    cases = []
    for delay in (36, 0, -10):
        for since in (delay + 5, delay + 6):
            cases.append(dict(name=f'delay{delay}_since{since}', mission='guard', delay=delay, since=since,
                              state=dict(value=449, stage=2)))
    cases += [
        dict(name='constructor_last_fire', mission='guard', since=300, state=dict(value=449, stage=2)),
        *[dict(name=f'mission_{mission}', mission=mission, since=42, state=dict(value=449, stage=2))
          for mission in ('sticky', 'selling', 'construction', 'attack')],
        dict(name='queued_attack', mission='none', queued='attack', since=42, state=dict(value=449, stage=2)),
        dict(name='attack_queued_guard', mission='attack', queued='guard', since=42,
             state=dict(value=449, stage=2)),
        *[dict(name=f'value{value}_stage{stage}', mission='guard', since=42,
               state=dict(value=value, stage=stage)) for value, stage in ((0, 0), (30, 0), (450, 2), (0, 1))],
        dict(name='elite', mission='guard', since=42, state=dict(value=249, stage=2, veterancy=2.0)),
        dict(name='rate_down_zero', mission='guard', since=42, table=dict(rate_down=0),
             state=dict(value=449, stage=2)),
        dict(name='latched', mission='guard', since=42, state=dict(value=449, stage=2, latch=1)),
        dict(name='plain', mission='guard', since=42, gattling=False, state=dict(value=449, stage=2)),
        dict(name='dead', mission='guard', since=42, dead=True, state=dict(value=449, stage=2)),
    ]
    return cases


def cadence(case):
    """Per frame (module doc), with the row's events applied at the start of
    their frame."""
    u, read32, building, frame, events = fixture(dict(case, rof=0))
    bc.invoke(u, bc.BEGIN_MODE, building, 1)
    u.mem_write(building + 0x2EC, dwords(-1, 0, 0))
    u.mem_write(bc.SCENARIO_INIT, dwords(1))
    u.mem_write(building + 0xAC, dwords(-1))
    bc.invoke(u, bga.ENTER_CONSTRUCTION, building, 1, 1)
    u.mem_write(building + 0x6DD, bytes([1]))
    u.mem_write(bc.SCENARIO_INIT, dwords(0))

    forced = []

    def answers(_u, address, _size, _data):
        # building_guard_attack's hooks answered first; these adjust them.
        if address == bga.FIRE_AT:
            weapon = events[-1][2]
            now = read32(bc.FRAME)
            u.mem_write(building + 0x2EC, dwords(now, 0, ROF[weapon]))
            u.mem_write(building + 0x120, dwords(now))
        elif address == bga.GET_FIRE_ERROR and forced:
            code = forced[0]
            events[-1][4] = code
            u.reg_write(UC_X86_REG_EAX, code)

    u.hook_add(UC_HOOK_CODE, answers)
    script = {}
    for event in case.get('events', []):
        script.setdefault(event[0], []).append(event[1])
    frames = []
    for k in range(1, case['frames'] + 1):
        u.mem_write(bc.FRAME, dwords(frame + k))
        forced.clear()
        for action in script.get(k, []):
            if action == 'acquire':
                u.mem_write(building + 0x2B4, dwords(bga.ACTOR))
            elif action == 'lose':
                u.mem_write(building + 0x2B4, dwords(0))
            elif action == 'override':
                # A retaliation: TechnoClass::Override_Mission(Attack, target, 0).
                bc.invoke(u, OVERRIDE_MISSION, building, 1, bga.ACTOR, 0)
            elif action.startswith('error:'):
                # This frame's GetFireError answers the code instead.
                forced.append(int(action[6:]))
        before = len(events)
        bc.run_block(u, building, bc.READY_COMMENCE_UNLESS_BUILDING)
        bc.run_block(u, building, AI_UPDATE_DISPATCH)
        bc.run_block(u, building, IDLE_DECAY)
        bc.run_block(u, building, bc.READY_COMMENCE)
        frames.append(dict(frame=k, mission=bga.state(u, building)['mission'],
                           queued=bga.state(u, building)['queued'], events=kept(events[before:]),
                           gattling=gattling_state(u, building)))
    return dict(input=case, frames=frames)


def cadence_cases():
    # Each row seeds the Scenario stream with 1, so every Guard delay is
    # reproducible (RandomClass 0x65C6D0).
    return [dict(case, seed=1) for case in (
        # Spin-up to stage 2, the target lost at 440, the wind-down and the
        # idle decay from the last shot + 42.
        dict(name='spin_up_and_down', frames=520, events=[[5, 'acquire'], [440, 'lose']]),
        # Past the cap (600).
        dict(name='spin_to_cap', frames=700, events=[[5, 'acquire']]),
        # A RANGE answer mid-spin, on a frame whose rearm has run out (the
        # 18th shot's): the drop tail's decay, then Guard.
        dict(name='range_mid_spin', frames=340, events=[[5, 'acquire'], [306, 'error:8']]),
        # FACING answers for a stretch: the FACING arm charges too.
        dict(name='facing_stretch', frames=80, events=[[5, 'acquire'], *[[k, 'error:2'] for k in range(30, 50)]]),
        # A retaliation from Guard: the first Attack dispatch gets the Guard's
        # whole count.
        dict(name='override_from_guard', frames=60, events=[[10, 'override']]),
    )]


def report_gate(case):
    u, read32, building, frame, events = fixture(case)
    weapon = WEAPONS[0]
    u.mem_write(weapon + 0xC0, dwords(REPORTS))
    u.mem_write(weapon + 0xCC, dwords(case['count']))
    u.mem_write(REPORTS, dwords(11, 12, 13, 14))
    u.mem_write(building + 0x3C8, struct.pack('<H', case['sequence']))
    u.mem_write(SP, bytes(0x80))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ESI, building)
    u.reg_write(UC_X86_REG_EBX, weapon)
    u.reg_write(UC_X86_REG_EDI, 0)
    run_checked(u, REPORT_GATE[0], REPORT_GATE[1], count=10_000)
    return dict(input=case, events=kept(events))


def report_gate_cases():
    return [dict(name=f'{"gattling" if g else "plain"}_count{count}_seq{seq}', gattling=g, count=count,
                 sequence=seq)
            for g in (True, False) for count in (0, 1, 3) for seq in (0, 4)]


def generate():
    return {'source': 'unicorn/gamemd.exe',
            'attack': [one_call(bga.MISSION_ATTACK)(case) for case in attack_cases()],
            'guard': [guard(case) for case in guard_cases()],
            'idle_decay': [idle_decay(case) for case in idle_decay_cases()],
            'cadence': [cadence(case) for case in cadence_cases()],
            'report_gate': [report_gate(case) for case in report_gate_cases()]}


def main(argv=None):
    finish_vectors(
        generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: provenance(
            scope='The Gattling Cannon stage calls of BuildingClass::Mission_Attack 0x44ACF0, '
                  'Mission_Guard 0x4496B0 and BuildingClass::Update 0x43FE5B..0x43FF91, their '
                  'dispatch cadence, and TechnoClass::FireAt\'s report gate 0x6FF349..0x6FF394',
            entry_points={'mission_attack': bga.MISSION_ATTACK, 'mission_guard': bga.MISSION_GUARD,
                          'area_guard': AREA_GUARD, 'idle_decay': IDLE_DECAY[0],
                          'ai_update_dispatch': AI_UPDATE_DISPATCH[0], 'report_gate': REPORT_GATE[0],
                          'increase': INCREASE, 'update': UPDATE},
            assumptions=['building_guard_attack\'s fixture building made a gattling type with the retail '
                         '[YAGGUN] stage table and six supplied WeaponTypes (ROF 16, 16, 16, 8, 16, 4) whose '
                         'one-item Report= lists name their slot',
                         'g_MainRng seeded per row through RandomClass 0x65C6D0; the recorded draws name '
                         'their stream, not their values',
                         'cadence rows run only BuildingClass::Update\'s mission pieces and the +0xC4 count '
                         'with the dispatch (0x6FA646..0x6FA65A); the passive scan is replaced by the '
                         'row\'s acquire and lose events'],
            substitutions=['GetFireError answered from the row or by the rearm test; FireAt answered with '
                           'its rearm write at the fired weapon\'s ROF and LastFireFrame (+0x120, '
                           '0x6FF743); PlayAt 0x7509E0 and Release 0x406060 answered and recorded; '
                           'building_guard_attack\'s other answers']),
        argv=argv)


if __name__ == '__main__':
    main()
