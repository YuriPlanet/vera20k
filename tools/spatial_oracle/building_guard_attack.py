"""Original building Guard and Attack missions.

- `guard` rows: one BuildingClass::Mission_Guard (0x4496B0) call on the
  building_construction fixture's refinery (the slave_manager fixture's 2x2
  building), armed by a WeaponType pointer in Weapon[0] (Type+0x898, the slot
  TechnoClass::GetWeapon 0x70E140 reads for a non-elite techno) or unarmed,
  with the row's mission (+0xAC), status (+0xBC), TarCom (+0x2B4), type flags
  (EMPulseCannon +0x16C3, HasStupidGuardMode +0x16B5, UnitRepair +0x16A9,
  WeaponsFactory +0x16BD) and MissionControl Rate/AARate (0xA8E3A8 +
  mission * 0x20 + 0x10/+0x18) stored as MissionControlClass::Read_INI's
  CCINIClass::ReadDouble leaves them (`%f` into a float, widened). Begin_Mode
  (0x447780), Queue_Mission (0x5B35E0), Commence (0x5B3570) and the Scenario
  RandomRanged (0x65C7E0) run natively; ClearBibArea (0x449540) is observed.
- `attack` rows: one BuildingClass::Mission_Attack (0x44ACF0) call with
  BuildingClass::GetFireError (0x447F10) answered from the row, each code
  0..11 and a null TarCom; IsAnimDelayedFire (+0x16A7) with DelayedFireDelay
  (+0x16EC) for the delayed arm.
- `set_target` rows: BuildingClass::SetTarget (0x443B90) with Is_Operational
  (0x4555D0), IsCloseEnough (vt+0x3AC 0x6F7780), IsHumanPlayer (0x50B730) and
  the EMP test (vt+0x37C 0x70EFD0) answered from the row; slot 0's projectile
  AA flag (BulletType+0x2A4) supplied.
- `unlimbo` rows: BuildingClass 0x44D6A0 (Unlimbo's vt+0x484) with the row's
  arguments, Scenario-init flag (0xA8E7AC) and 0xA8ED6B.
- `cadence` rows: an armed building's frames through BuildingClass::Update's
  mission pieces in its order, each native: the ready check that commences a
  queued mission unless BState is 0 (0x43FE27..0x43FE54), MissionClass::AI
  (0x5B3060, dispatching Mission_Guard and Mission_Attack) and the ready check
  that commences (0x43FF91..0x43FFB4). Fire_At (0x6FDD50) is answered with its
  rearm write (+0x2EC = {Frame, _, ROF}, 0x6FE4A4..0x6FE4C5) at the row's
  ROF, and GetFireError with TechnoClass::GetFireError's rearm test
  (0x6FC94F..0x6FC975: REARM while the timer has time left, else OK). Row
  events set or clear TarCom (the passive scan's or a pointer detach's write)
  or queue Attack as a player order (Queue_Mission(Attack, false)).
- `hidden_startup` rows: original map idle/Guard queue, then UpdateAnimation,
  both ready checks, the Techno AI counter/MissionClass dispatch and queued
  body. A turret with ready0 stays on mission None; ready1 and the non-turret
  control commence Guard. All three RNG streams and native timer stores are
  retained. Discovery/first Grand_Opening and the rest of AI are not executed.

- `--gate`: additive original Gate admission/Open true-RET controls and the
  shared Door / building mission cadence. Its separate corpus records complete
  RNG states, original code identity and declared caller/presentation seams.

Usage: python -m tools.spatial_oracle.building_guard_attack [--gate] [--check|--write]
"""
from pathlib import Path
import hashlib
import struct
import sys

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW
from tools.native_oracle import finish_vectors, provenance, image_bytes, file_span
from tools.spatial_oracle import building_construction as bc
from tools.spatial_oracle import slave_manager as sm
from tools.spatial_oracle import building_sale as sale
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.refinery_dock import ACTOR
from tools.spatial_oracle.unit_source_scatter import SCENARIO

MISSION_GUARD, MISSION_ATTACK, BUILDING_SET_TARGET = 0x4496B0, 0x44ACF0, 0x443B90
GET_FIRE_ERROR, FIRE_AT, TECHNO_SET_TARGET = 0x447F10, 0x6FDD50, 0x6FCDB0
SET_DESIRED, DIRECTION_TO, UNCLOAK = 0x4C9220, 0x43ED40, 0x7036C0
PLAY_ANIM, CLEAR_BIB = 0x451890, 0x449540
IS_OPERATIONAL, IS_CLOSE_ENOUGH, IS_HUMAN, UNDER_EMP = 0x4555D0, 0x6F7780, 0x50B730, 0x70EFD0
RANDOM_RANGED, RANDOM_NEXT, RANDOM_SEED = 0x65C7E0, 0x65C780, 0x65C6D0
ENTER_CONSTRUCTION = bc.ENTER_CONSTRUCTION
# Weapon[0] (TechnoTypeClass +0x898, WeaponStruct stride 0x1C) and a supplied
# WeaponType/BulletType pair in the fixture's free region.
WEAPON_SLOT0 = sm.YTYPE + 0x898
WEAPON, BULLET = sm.REGION + 0x9000, sm.REGION + 0xA000
MISSION = {'none': -1, 'attack': 1, 'guard': 5, 'sticky': 6, 'selling': 0x13, 'wait': 0x1C}
MISSION_NAME = {value: name for name, value in MISSION.items()}
# Retail [Guard] Rate=.030 AARate=.016, [Sticky] Rate=.016 (no AARate: the
# reader copies Rate), [Attack] Rate=.016 AARate=.016.
RATES = {'guard': ('.030', '.016'), 'sticky': ('.016', '.016'), 'attack': ('.016', '.016')}
TYPE_FLAGS = {'emp_cannon': 0x16C3, 'stupid_guard': 0x16B5, 'unit_repair': 0x16A9,
              'weapons_factory': 0x16BD, 'delayed_fire': 0x16A7, 'tick_tank': 0x16C4,
              'artillary': 0x16CA}


def read_double(text):
    """CCINIClass::ReadDouble (0x5283D0): `%f` into a float, widened."""
    return struct.unpack('<f', struct.pack('<f', float(text)))[0]


def name_of(pointer):
    return {ACTOR: 'target', 0: None}.get(pointer, hex(pointer))


def signed(u, address):
    return struct.unpack('<i', u.mem_read(address, 4))[0]


def fixture(case):
    u, read32, building, frame, calls = bc.building_fixture(dict(name=case['name'], control=[0, 1, 0],
                                                                 mission='none'))
    kind = sm.YTYPE
    if 'seed' in case:
        bc.invoke(u, RANDOM_SEED, SCENARIO + 0x218, case['seed'])
    u.mem_write(building + 0xAC, dwords(MISSION[case.get('mission', 'guard')]))
    u.mem_write(building + 0xB4, dwords(MISSION[case.get('queued', 'none')]))
    u.mem_write(building + 0xBC, dwords(case.get('status', 0)))
    u.mem_write(building + 0xC0, dwords(frame, 0))
    u.mem_write(building + 0xC8, dwords(frame, 0, 0))
    u.mem_write(building + 0x90, bytes([1]))
    u.mem_write(building + 0x2B4, dwords(ACTOR if case.get('target') else 0))
    u.mem_write(building + 0x6DD, bytes([case.get('ready', 0)]))
    u.mem_write(WEAPON_SLOT0, dwords(WEAPON if case.get('armed', True) else 0))
    u.mem_write(WEAPON + 0xA0, dwords(BULLET))
    u.mem_write(BULLET + 0x2A4, bytes([case.get('anti_air', False)]))
    for flag, offset in TYPE_FLAGS.items():
        u.mem_write(kind + offset, bytes([case.get(flag, False)]))
    u.mem_write(kind + 0x16EC, dwords(case.get('delayed_fire_delay', 0)))
    u.mem_write(kind + 0x16F0, dwords(-1))  # SuperWeapon= unset
    for mission, (rate, aa_rate) in {**RATES, **case.get('rates', {})}.items():
        entry = 0xA8E3A8 + MISSION[mission] * 0x20
        u.mem_write(entry + 0x10, struct.pack('<dd', read_double(rate), read_double(aa_rate)))
    events = []
    answers = dict(errors=list(case.get('errors', [])), rof=case.get('rof'))

    def ret(cleanup, value=0):
        sp = u.reg_read(UC_X86_REG_ESP)
        u.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        u.reg_write(UC_X86_REG_EIP, read32(sp))
        u.reg_write(UC_X86_REG_ESP, sp + 4 + cleanup)

    def rearm_left():
        start, delay = signed(u, building + 0x2EC), signed(u, building + 0x2F4)
        if start == -1:
            return delay
        elapsed = (read32(bc.FRAME) - start) & 0xFFFFFFFF
        return 0 if elapsed >= delay else delay - elapsed

    def hook(_u, address, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        arg = lambda n: read32(sp + 4 * n)
        if address == GET_FIRE_ERROR:
            if answers['rof'] is not None:
                code = 3 if rearm_left() else 0
            else:
                code = answers['errors'].pop(0)
            events.append(['fire_error', name_of(arg(1)), arg(2), arg(3), code])
            ret(12, code)
        elif address == FIRE_AT:
            events.append(['fire_at', name_of(arg(1)), arg(2)])
            if answers['rof'] is not None:
                u.mem_write(building + 0x2EC, dwords(read32(bc.FRAME), 0, answers['rof']))
            ret(8, 0)
        elif address == SET_DESIRED:
            events.append(['set_desired'])
            ret(4)
        elif address == DIRECTION_TO:
            u.mem_write(arg(1), dwords(0))
            ret(8, arg(1))
        elif address == TECHNO_SET_TARGET:
            events.append(['set_target', name_of(arg(1))])
            u.mem_write(building + 0x2B4, dwords(arg(1)))
            ret(4)
        elif address == UNCLOAK:
            events.append(['uncloak', arg(1)])
            ret(4)
        elif address == PLAY_ANIM:
            events.append(['play_anim'])
            ret(20)
        elif address == CLEAR_BIB:
            events.append(['clear_bib'])
            ret(0)
        elif address == IS_OPERATIONAL:
            ret(0, case.get('operational', True))
        elif address == IS_CLOSE_ENOUGH:
            events.append(['in_range', name_of(arg(1))])
            ret(4, case.get('in_range', True))
        elif address == IS_HUMAN:
            ret(0, case.get('human', False))
        elif address == UNDER_EMP:
            ret(0, case.get('under_emp', False))
        elif address == RANDOM_RANGED:
            events.append(['random_ranged', arg(1), arg(2)])
        elif address == RANDOM_NEXT:
            events.append(['random_next'])
        elif address == MISSION_GUARD:
            events.append(['mission_guard'])
        elif address == MISSION_ATTACK:
            events.append(['mission_attack'])

    u.hook_add(UC_HOOK_CODE, hook)
    return u, read32, building, frame, events


def state(u, building):
    return dict(ready=u.mem_read(building + 0x6DD, 1)[0],
                mission=MISSION_NAME.get(signed(u, building + 0xAC), signed(u, building + 0xAC)),
                queued=MISSION_NAME.get(signed(u, building + 0xB4), signed(u, building + 0xB4)),
                status=signed(u, building + 0xBC), mission_start=signed(u, building + 0xC0),
                counter=signed(u, building + 0xC4),
                timer=[signed(u, building + 0xC8), signed(u, building + 0xD0)],
                target=name_of(struct.unpack('<I', u.mem_read(building + 0x2B4, 4))[0]),
                bstate=signed(u, building + 0x534), queued_bstate=signed(u, building + 0x538),
                delayed_fire=[signed(u, building + 0x704), signed(u, building + 0x708),
                              signed(u, building + 0x714)],
                prism_count=signed(u, building + 0x664), turret_count=signed(u, building + 0x148))


def draws(events):
    return [event for event in events if event[0] in ('random_ranged', 'random_next')]


def one_call(entry):
    def run(case):
        u, read32, building, frame, events = fixture(case)
        before = [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]
        value = bc.invoke(u, entry, building, *case.get('args', []))
        after = [read32(SCENARIO + 0x21C), read32(SCENARIO + 0x220)]
        calls = [event for event in events if event[0] not in ('random_ranged', 'random_next')]
        return dict(input=case, frame=frame, returns=struct.unpack('<i', dwords(value))[0], calls=calls,
                    draws=draws(events), random_indices=dict(before=before, after=after), **state(u, building))
    return run


def guard_cases():
    return [
        # Armed: the AARate delay without a target, the Attack commit with one.
        # Seeds 1, 2 and 8 draw 1, 2 and 0; seed 9 re-draws (raw & 3 == 3).
        *[dict(name=f'armed_idle_seed{seed}', mission='guard', seed=seed) for seed in (1, 2, 8, 9)],
        dict(name='armed_target', mission='guard', target=True),
        dict(name='armed_sticky_idle', mission='sticky'),
        dict(name='armed_sticky_target', mission='sticky', target=True),
        dict(name='armed_emp_cannon', mission='guard', target=True, emp_cannon=True),
        # A modded AARate whose x900 has a fraction above .5 (ftol chops).
        dict(name='armed_modded_aarate', mission='guard', rates={'guard': ('.030', '.0206')}),
        # Unarmed: status 0 enters the idle mode; status 1 and later.
        dict(name='unarmed_status0', mission='guard', armed=False, status=0),
        dict(name='unarmed_status1', mission='guard', armed=False, status=1),
        dict(name='unarmed_status2', mission='guard', armed=False, status=2),
        dict(name='unarmed_repair_status1', mission='guard', armed=False, status=1, unit_repair=True),
        dict(name='unarmed_factory_status1', mission='guard', armed=False, status=1, weapons_factory=True),
        dict(name='unarmed_factory_status0', mission='guard', armed=False, status=0, weapons_factory=True),
        dict(name='unarmed_stupid', mission='guard', armed=False, stupid_guard=True),
        dict(name='unarmed_sticky', mission='sticky', armed=False, status=1),
    ]


def attack_cases():
    cases = [dict(name=f'code_{code}', mission='attack', target=True, errors=[code]) for code in range(12)]
    cases += [
        dict(name='no_target', mission='attack'),
        dict(name='delayed_ok', mission='attack', target=True, errors=[0], delayed_fire=True,
             delayed_fire_delay=28),
        dict(name='delayed_rearm', mission='attack', target=True, errors=[3], delayed_fire=True,
             delayed_fire_delay=28),
    ]
    return cases


def set_target_cases():
    return [
        dict(name='null', mission='guard', args=[0]),
        dict(name='in_range', mission='guard', args=[ACTOR], in_range=True),
        dict(name='out_of_range', mission='guard', args=[ACTOR], in_range=False),
        dict(name='no_weapon', mission='guard', args=[ACTOR], in_range=False, armed=False),
        dict(name='anti_air', mission='guard', args=[ACTOR], in_range=False, anti_air=True),
        dict(name='selling', mission='selling', args=[ACTOR], in_range=True),
        dict(name='not_operational', mission='guard', args=[ACTOR], in_range=True, operational=False),
        dict(name='artillery_out_of_range', mission='guard', args=[ACTOR], in_range=False, artillary=True),
        dict(name='artillery_human', mission='guard', args=[ACTOR], in_range=False, artillary=True, human=True),
        dict(name='artillery_emp', mission='guard', args=[ACTOR], in_range=False, artillary=True,
             under_emp=True),
    ]


def unlimbo(case):
    u, read32, building, frame, events = fixture(dict(case, mission='none'))
    u.mem_write(bc.SCENARIO_INIT, dwords(case['scenario_init']))
    u.mem_write(bc.SCENARIO_FLAG_ED6B, bytes([case['flag_ed6b']]))
    bc.invoke(u, ENTER_CONSTRUCTION, building, *case['args'])
    return dict(input=case, **state(u, building))


def unlimbo_cases():
    return [dict(name=f'args{a}{b}_init{i}_ed6b{f}', args=[a, b], scenario_init=i, flag_ed6b=f)
            for a, b in ((1, 1), (0, 1), (1, 0)) for i in (0, 1) for f in (0, 1)]


def cadence(case):
    """Per frame: the pre-AI commence block, MissionClass::AI and the post-AI
    commence block (module doc), with the row's events applied at the start
    of their frame."""
    u, read32, building, frame, events = fixture(case)
    bc.invoke(u, bc.BEGIN_MODE, building, 1)
    u.mem_write(building + 0x2EC, dwords(-1, 0, 0))
    if case['start'] == 'map':
        # Unlimbo during scenario init queues Guard; Grand_Opening's first
        # opening sets +0x6DD (0x4467C9).
        u.mem_write(bc.SCENARIO_INIT, dwords(1))
        u.mem_write(building + 0xAC, dwords(-1))
        bc.invoke(u, ENTER_CONSTRUCTION, building, 1, 1)
        u.mem_write(building + 0x6DD, bytes([1]))
        u.mem_write(bc.SCENARIO_INIT, dwords(0))
    script = {event[0]: event[1:] for event in case.get('events', [])}
    frames = []
    for k in range(1, case['frames'] + 1):
        u.mem_write(bc.FRAME, dwords(frame + k))
        for action in script.get(k, []):
            if action == 'acquire':
                u.mem_write(building + 0x2B4, dwords(ACTOR))
            elif action == 'lose':
                u.mem_write(building + 0x2B4, dwords(0))
            elif action == 'order':
                u.mem_write(building + 0x2B4, dwords(ACTOR))
                bc.invoke(u, bc.QUEUE_MISSION, building, 1, 0)
        before = len(events)
        bc.run_block(u, building, bc.READY_COMMENCE_UNLESS_BUILDING)
        bc.invoke(u, bc.MISSION_AI, building)
        bc.run_block(u, building, bc.READY_COMMENCE)
        frames.append(dict(frame=k, events=events[before:], **state(u, building)))
    return dict(input=case, frames=frames)


def cadence_cases():
    return [
        # A map building: Guard queued at unlimbo, +0x6DD from the first
        # opening; a target acquired at frame 5 (after that frame's
        # dispatch in native order is modelled by the next frame's read).
        dict(name='map_acquire_rof20', start='map', frames=70, rof=20, events=[[5, 'acquire']]),
        dict(name='map_acquire_rof21', start='map', frames=70, rof=21, events=[[5, 'acquire']]),
        dict(name='map_acquire_rof1', start='map', frames=24, rof=1, events=[[5, 'acquire']]),
        # The target is lost mid-reload: Attack's null-target tail, then Guard.
        dict(name='lose_target', start='map', frames=60, rof=20, events=[[5, 'acquire'], [30, 'lose']]),
        # A player order on an idle armed building: Queue(Attack) with the
        # target, promoted by the next ready check.
        dict(name='player_order', start='map', frames=40, rof=20, events=[[20, 'order']]),
    ]


def generate():
    return {'source': 'unicorn/gamemd.exe',
            'guard': [one_call(MISSION_GUARD)(case) for case in guard_cases()],
            'attack': [one_call(MISSION_ATTACK)(case) for case in attack_cases()],
            'set_target': [one_call(BUILDING_SET_TARGET)(case) for case in set_target_cases()],
            'unlimbo': [unlimbo(case) for case in unlimbo_cases()],
            'cadence': [cadence(case) for case in cadence_cases()],
            'hidden_startup': [hidden_startup(case) for case in hidden_startup_cases()]}


GATE_OPEN, GATE_ADMISSION = 0x44E440, 0x452540
DOOR_CTOR, DOOR_OPEN, DOOR_CLOSE = 0x4A50F0, 0x4A51F0, 0x4A5240
# This is the shared completion slot of Techno AI, before MissionClass::AI.
# It is a declared caller seam, not whole Techno/Building Update execution.
DOOR_COMPLETION = (0x6FA5BE, 0x6FA5D6)
GATE_MISSION_HOST = (0x6FA646, 0x6FA65A)
GATE_PHASES = {'closed': (0, 0), 'opening': (1, 1),
               'open': (0, 1), 'closing': (1, 0)}
GATE_RNGS = {'main': 0x886B88, 'scenario': SCENARIO + 0x218, 'mapgen': 0xABE890}


def gate_rng(u):
    return {name: bytes(u.mem_read(pointer, 1012)).hex()
            for name, pointer in GATE_RNGS.items()}


def hidden_startup_cases():
    return [dict(name=name, frame=200, frames=2, seed=1, turret=turret, ready=ready,
                 armed=True)
            for name, turret, ready in (
                ('hidden_turret', True, 0),
                ('opened_turret', True, 1),
                ('hidden_nonturret', False, 0))]


def hidden_startup(case):
    """The existing Building VM, with supplied discovery/ready prior.

    Original44D6A0 establishes the map's idle body and Guard queue. No
    successful discovery or Grand_Opening return is fabricated: ready is an
    explicit input, contrasting the unopened and already-ready entries.
    The non-turret remains armed to isolate UpdateAnimation's ready gate;
    it is not a claim about CABARR02's absent weapon.
    """
    u, _read32, building, frame, events = fixture(dict(case, mission='none'))
    assert frame == case['frame']
    u.mem_write(sm.YTYPE + 0xCA1, bytes([case['turret']]))
    u.mem_write(bc.SCENARIO_INIT, dwords(1))
    bc.invoke(u, ENTER_CONSTRUCTION, building, 1, 1)
    u.mem_write(bc.SCENARIO_INIT, dwords(0))
    for pointer in GATE_RNGS.values():
        bc.invoke(u, RANDOM_SEED, pointer, case['seed'])
    events.clear()
    initial = state(u, building)
    before = gate_rng(u)
    timer_fields = {building + offset: (timer, field) for offset, timer, field in (
        (0xC8, 'mission', 'start'), (0xD0, 'mission', 'duration'),
        (0x100, 'body', 'start'), (0x108, 'body', 'duration'))}
    writes = []

    def timer_write(_u, _access, address, size, value, _data):
        if address in timer_fields:
            assert size == 4
            timer, field = timer_fields[address]
            writes.append(dict(timer=timer, field=field,
                               value=struct.unpack('<i', dwords(value))[0],
                               pc=f'0x{u.reg_read(UC_X86_REG_EIP):08X}'))

    hook = u.hook_add(UC_HOOK_MEM_WRITE, timer_write)
    frames = []
    for step in range(1, case['frames'] + 1):
        u.mem_write(bc.FRAME, dwords(frame + step))
        start = len(events)
        writes.clear()
        bc.invoke(u, bc.UPDATE_ANIMATION, building)
        bc.run_block(u, building, bc.READY_COMMENCE_UNLESS_BUILDING)
        bc.run_block(u, building, GATE_MISSION_HOST)
        bc.run_block(u, building, bc.READY_COMMENCE)
        bc.run_block(u, building, bc.QUEUED_BSTATE, ebp=-1)
        frame_events = events[start:]
        frames.append(dict(frame=frame + step, events=frame_events,
                           draws=draws(frame_events), timer_writes=list(writes),
                           **state(u, building)))
    u.hook_del(hook)
    assert bytes(u.mem_read(0x401000, 0x3E0000)) == file_span(
        image_bytes(), 0x401000, 0x3E0000)[1], 'fixture patched original .text'
    return dict(input=case, initial=initial, frames=frames,
                rng_before=before, rng_after=gate_rng(u))


def gate_state(u, building):
    return dict(**state(u, building),
                phase=list(u.mem_read(building + 0x368, 2)),
                door=[signed(u, building + offset) for offset in (0x358, 0x360, 0x364)],
                hold=[signed(u, building + offset) for offset in (0x604, 0x60C, 0x610)],
                gate_stage=u.mem_read(building + 0x703, 1)[0])


def gate_fixture(case):
    """Extend this owner's supplied Building fixture. Constructor/type/INI
    closure is excluded; Gate fields and retained timers are declared inputs.
    Original Door constructor and any requested Open/Close producer execute."""
    u, read32, building, _frame, events = fixture(
        dict(case, mission='guard', armed=False))
    frame = case.get('frame', 100)
    u.mem_write(bc.FRAME, dwords(frame))
    u.mem_write(building + 0xAC, dwords(case.get('current', 24)))
    u.mem_write(building + 0xB4, dwords(case.get('queue', -1)))
    u.mem_write(building + 0xBC, dwords(case.get('status', 0)))
    u.mem_write(building + 0xC0, dwords(frame, 0))
    u.mem_write(building + 0xC8, dwords(frame, 0, 0))
    u.mem_write(sm.YTYPE + 0x16B7, bytes([case.get('gate', True)]))
    u.mem_write(sm.YTYPE + 0x3C8, struct.pack('<d', read_double(case.get('deploy', '.044'))))
    u.mem_write(sm.YTYPE + 0xE28, struct.pack('<d', read_double(case.get('close_delay', '.2'))))
    u.mem_write(sm.YTYPE + 0x16F8, dwords(9))
    u.mem_write(0xA8E3A8 + 24 * 32 + 0x10,
                struct.pack('<d', read_double(case.get('rate', '.016'))))
    bc.invoke(u, DOOR_CTOR, building + 0x350)
    phase = case.get('phase', 'closed')
    if phase in ('opening', 'closing'):
        # Actual duration producer, at the row's retained earlier frame.
        u.mem_write(bc.FRAME, dwords(case.get('door_start', frame)))
        value = read_double(case.get('door_time', case.get('deploy', '.044')))
        # Close has an original no-op on a stable-closed Door: supply the
        # retained open direction before invoking it when it is a producer.
        if phase == 'closing':
            u.mem_write(building + 0x369, b'\x01')
        bc.invoke(u, DOOR_OPEN if phase == 'opening' else DOOR_CLOSE,
                  building + 0x350, *struct.unpack('<2I', struct.pack('<d', value)))
        u.mem_write(bc.FRAME, dwords(frame))
    else:
        # Stable controls retain a completed producer's timer unless the row
        # uses the constructor's closed state. These are supplied byte inputs.
        u.mem_write(building + 0x368, bytes(GATE_PHASES[phase]))
    hold_start, hold_ticks = case.get('hold', [frame, 180])
    u.mem_write(building + 0x604, dwords(hold_start, 0, hold_ticks, hold_ticks))
    # The fixture's original 2x2 foundation is already self in Ground+E4.
    # Ground and Bridge contrasts supply the actor only on their stated list.
    blocker = case.get('blocker')
    if blocker:
        cell = sm.cell(*sm.YAREFN_NW)
        u.mem_write(cell + (0xE4 if blocker == 'ground' else 0xE8), dwords(ACTOR))
        u.mem_write(ACTOR + 0x30, dwords(building))
        u.mem_write(ACTOR + 0x6C, dwords(case.get('blocker_health', 1000)))
    for pointer in GATE_RNGS.values():
        bc.invoke(u, RANDOM_SEED, pointer, case.get('seed', 1))
    events.clear()

    def observe_rectangle(_u, address, _size, _data):
        if address == 0x455C20:
            # Gate setup only copies this returned drawing rectangle to
            # Tactical::DirtyScreenRect; it never feeds a gameplay decision.
            sp = u.reg_read(UC_X86_REG_ESP)
            rectangle = read32(sp + 4)
            events.append(['drawing_rectangle'])
            u.mem_write(rectangle, bytes(16))
            u.reg_write(UC_X86_REG_EAX, rectangle)
            u.reg_write(UC_X86_REG_EIP, read32(sp))
            u.reg_write(UC_X86_REG_ESP, sp + 8)
        elif address == 0x7C978A:
            # CRT atexit registration for Object's lazy terminal CellStruct;
            # the original lazy writes and type/foundation dispatch execute.
            sp = u.reg_read(UC_X86_REG_ESP)
            u.reg_write(UC_X86_REG_EAX, 0)
            u.reg_write(UC_X86_REG_EIP, read32(sp))
            u.reg_write(UC_X86_REG_ESP, sp + 4)

    u.hook_add(UC_HOOK_CODE, observe_rectangle)
    bc.invoke(u, sale.OCCUPY_INIT, 0)
    # BuildingType::ReadINI461541 publishes this pointer after Foundation.
    # Actual initializer owns its contents; no Rust/manual foundation golden.
    u.mem_write(sm.YTYPE + 0xDFC, dwords(sale.OCCUPY_LISTS + 3 * sale.OCCUPY_STRIDE))
    return u, read32, building, frame, events


def gate_controls():
    return [
        dict(name='setup_closed', status=0),
        dict(name='setup_zero', status=0, deploy='0'),
        dict(name='setup_negative_duration', status=0, deploy='-.044'),
        dict(name='setup_opening_retains', status=0, phase='opening', door_start=90),
        dict(name='setup_closing_reverses', status=0, phase='closing', door_start=90),
        *[dict(name=f'setup_open_seed{seed}', status=0, phase='open', seed=seed)
          for seed in (1, 2, 8, 9)],
        dict(name='opening_wait', status=1, phase='opening', door_start=90),
        dict(name='opening_wait_open', status=1, phase='open'),
        dict(name='opening_wait_closed', status=1),
        dict(name='hold_before', status=2, phase='open', frame=279, hold=[100, 180]),
        *[dict(name=f'hold_expired_seed{seed}', status=2, phase='open', frame=280,
               hold=[100, 180], seed=seed) for seed in (1, 2, 8, 9)],
        dict(name='hold_ground_reseed', status=2, phase='open', frame=280,
             hold=[100, 180], blocker='ground'),
        dict(name='hold_dead_ground_reseed', status=2, phase='open', frame=280,
             hold=[100, 180], blocker='ground', blocker_health=0),
        dict(name='hold_bridge_does_not_reseed', status=2, phase='open', frame=280,
             hold=[100, 180], blocker='bridge'),
        dict(name='hold_negative_duration', status=2, phase='open', hold=[100, -180], close_delay='-.2'),
        dict(name='hold_zero_duration', status=2, phase='open', hold=[100, 0], close_delay='0'),
        dict(name='hold_negative_rate', status=2, phase='open', rate='-.016'),
        dict(name='begin_close', status=3, phase='open'),
        dict(name='begin_close_zero', status=3, phase='open', deploy='0'),
        dict(name='closing_wait', status=4, phase='closing', door_start=90),
        dict(name='closing_wait_open_retains', status=4, phase='open'),
        dict(name='closing_wait_closed', status=4),
        dict(name='post_close', status=5),
        dict(name='unknown_status', status=0xFFFFFFFF),
        dict(name='non_gate', status=0, gate=False),
        *[dict(name=f'admission_{name}', operation='admission', current=current,
               queue=queue, phase=phase, status=4)
          for name, current, queue, phase in (
              ('guard_closed', 5, -1, 'closed'), ('guard_open', 5, -1, 'open'),
              ('open_closing', 24, -1, 'closing'), ('open_opening', 24, -1, 'opening'),
              ('open_open', 24, -1, 'open'), ('none_queued_open', -1, 24, 'open'))],
    ]


def gate_one_call(case):
    u, _read32, building, frame, events = gate_fixture(case)
    initial = gate_state(u, building)
    rng_before = gate_rng(u)
    text = bytes(u.mem_read(0x401000, 0x3E0000))
    original = file_span(image_bytes(), 0x401000, 0x3E0000)[1]
    assert text == original, 'supplied fixture patched original .text'
    entry = GATE_ADMISSION if case.get('operation') == 'admission' else GATE_OPEN
    value = bc.invoke(u, entry, building)
    assert u.reg_read(UC_X86_REG_ESP) == bc.SP + 4, 'Gate did not reach its true RET'
    assert bytes(u.mem_read(0x401000, 0x3E0000)) == original
    return dict(input=case, frame=frame, initial=initial, returns=value & (255 if entry == GATE_ADMISSION else 0xFFFFFFFF),
                after=gate_state(u, building), events=events, draws=draws(events),
                rng_before=rng_before, rng_after=gate_rng(u),
                fpcw=u.reg_read(UC_X86_REG_FPCW),
                text_sha256=hashlib.sha256(original).hexdigest())


def gate_cadence():
    case = dict(name='gate_open_hold_close_guard', status=0, seed=9, ready=1, frame=100)
    u, _read32, building, frame, events = gate_fixture(case)
    # Actual normal idle body prerequisite, not a supplied BeginMode reply.
    bc.invoke(u, ENTER_CONSTRUCTION, building, 0, 1)
    # A movement request synchronously commences Open before the first turn.
    u.mem_write(building + 0xAC, dwords(5))
    admission = bc.invoke(u, GATE_ADMISSION, building) & 255
    rng_before = gate_rng(u)
    frames = []
    for k in range(1, 351):
        u.mem_write(bc.FRAME, dwords(frame + k))
        before = len(events)
        bc.invoke(u, bc.UPDATE_ANIMATION, building)
        bc.run_block(u, building, bc.READY_COMMENCE_UNLESS_BUILDING)
        bc.run_block(u, building, DOOR_COMPLETION)
        bc.run_block(u, building, GATE_MISSION_HOST)
        bc.run_block(u, building, bc.READY_COMMENCE)
        bc.run_block(u, building, bc.QUEUED_BSTATE, ebp=-1)
        frames.append(dict(frame=frame + k, events=events[before:], **gate_state(u, building)))
    original = file_span(image_bytes(), 0x401000, 0x3E0000)[1]
    assert bytes(u.mem_read(0x401000, 0x3E0000)) == original
    return dict(input=case, admission=admission, frames=frames,
                rng_before=rng_before, rng_after=gate_rng(u),
                fpcw=u.reg_read(UC_X86_REG_FPCW), text_sha256=hashlib.sha256(original).hexdigest())


def generate_gate():
    return dict(source='unicorn/gamemd.exe', controls=[gate_one_call(case) for case in gate_controls()],
                cadence=gate_cadence())


def gate_provenance():
    return provenance(
        scope='Original Building Gate MissionOpen44E440/admission452540 true RETs and a declared '
              'Building per-turn mission/Door-completion cadence using this existing fixture owner',
        entry_points={'mission_open': GATE_OPEN, 'admission': GATE_ADMISSION, 'door_ctor': DOOR_CTOR,
                      'door_open': DOOR_OPEN, 'door_close': DOOR_CLOSE, 'door_completion_start': DOOR_COMPLETION[0],
                      'door_completion_endpoint': DOOR_COMPLETION[1], 'mission_ai': bc.MISSION_AI,
                      'mission_counter_start': GATE_MISSION_HOST[0],
                      'mission_counter_endpoint': GATE_MISSION_HOST[1],
                      'update_animation': bc.UPDATE_ANIMATION,
                      'building_idle': ENTER_CONSTRUCTION, 'scenario_random': RANDOM_RANGED},
        assumptions=[
            'Existing building_construction/slave_manager supplied 2x2 Building at12,12, original '
            'Building and BuildingType vtables. Complete class constructors, physical retail INI '
            'readers and Gate map placement are excluded. Gate/type booleans, current/queued mission, '
            'handler cursor and hold clock are explicit retained boundary inputs.',
            'Original foundation initializer45B1C0 executes; BuildingTypeDFC is supplied to its '
            '2x2 list as the original461541 publisher does. Actual Object5F5B90 lazy terminal '
            'initialization, BuildingType45EC20, Foundation/GetCell/Map lookups and Ground list '
            'walk execute; no obstruction answer is supplied.',
            'DeployTime=.044, GateCloseDelay=.2 and Open Rate=.016 are supplied ReadDouble-shaped '
            'float-widened controls, not a claim of complete retail Gate rules closure. Original '
            'Door constructor/producers, ftol7C5F00, reversal4A5290 and stable completion execute; '
            'stable phase bytes and previous hold clock are supplied retained states.',
            'Full1012-byte Main/Scenario/MapGen originally seeded per row through65C6D0. Scenario '
            'rejection draws execute without supplied values; before/after state and calls retained. '
            'FPCW0E7F is fixture ambient state, not an established WinMain producer.',
            'Single controls reach original RETs44E794/4525E0 or early true RETs. Cadence starts '
            'with actual normal BuildingIdle44D6A0(0,1), then composes original UpdateAnimation4509D0, '
            'pre-Ready43FE27..43FE54, Door completion6FA5BE..6FA5D6, counter and full MissionAI '
            '6FA646..6FA65A, post-Ready43FF91..43FFB4 and queued-body43FFB4..440042 in order. '
            'All other Building/Techno AI steps, object-to-object '
            'Logic scheduling, damage, freeze, drawing and entire active-game reachability are excluded.',
            'Ground and Bridge blockers are supplied pointers only on the stated cell list. Dead '
            'Ground pointer control establishes no Health test; it does not execute object death/Mark.'
        ],
        substitutions=[
            'Reuse existing fixture boundaries: sound7509E0, repaint6D2790 and ClearBib449540 '
            'are observers; unrelated existing FireError/FireAt/target/facing predicates remain '
            'outside the measured Gate call path. The added Gate-only GetBoundsRect455C20 observer '
            'returns an empty supplied rectangle copied solely to repaint; native Tactical camera/'
            'SHP closure and Gate drawing parity are excluded. CRT atexit7C978A registration is a '
            'no-op; original lazy terminal writes execute. No Gate, Door, foundation query, mission verb, '
            'dispatch timer or RNG return/state is replaced. Original .text checked byte-for-byte.'
        ])


def gate_source_paths():
    """Conservative imported-module inventory.

    Import membership binds source identity; it does not claim that every
    imported body executes in the bounded Gate controls. The central
    finish_vectors owner hashes normalized source before and after generation.
    The native run executes no Rust; the Rust consumers' own tests compare them.
    """
    repo = Path(__file__).resolve().parents[2]
    paths = (
        # Conservative repository Python import closure, including this owner.
        'tools/native_oracle.py',
        'tools/projectile_oracle/bridge_render_inputs.py',
        'tools/projectile_oracle/bridge_render_inputs_selection.py',
        'tools/projectile_oracle/flat_art.py',
        'tools/projectile_oracle/guided_step.py',
        'tools/rmg_oracle/gen_rng_vectors.py',
        'tools/rules_oracle/bridge_anim_inputs.py',
        'tools/rules_oracle/bridge_anim_lists.py',
        'tools/rules_oracle/bridge_child_sound.py',
        'tools/rules_oracle/bridge_landing_inputs.py',
        'tools/spatial_oracle/__init__.py',
        'tools/spatial_oracle/air_locomotor_moving.py',
        'tools/spatial_oracle/anim_bouncer_launch.py',
        'tools/spatial_oracle/bridge_rim.py',
        'tools/spatial_oracle/building_art_transition.py',
        'tools/spatial_oracle/building_body_rules.py',
        'tools/spatial_oracle/building_construction.py',
        'tools/spatial_oracle/building_guard_attack.py',
        'tools/spatial_oracle/building_sale.py',
        'tools/spatial_oracle/building_slot_replacement.py',
        'tools/spatial_oracle/cmin_dock.py',
        'tools/spatial_oracle/estimated_damage.py',
        'tools/spatial_oracle/harvest_attack_return.py',
        'tools/spatial_oracle/harvest_field.py',
        'tools/spatial_oracle/infantry_entry_raw.py',
        'tools/spatial_oracle/jumpjet_coordinates.py',
        'tools/spatial_oracle/jumpjet_entry_discovery.py',
        'tools/spatial_oracle/locomotor_at_coord.py',
        'tools/spatial_oracle/map_queries.py',
        'tools/spatial_oracle/mapgen_range.py',
        'tools/spatial_oracle/object_health.py',
        'tools/spatial_oracle/refinery_dock.py',
        'tools/spatial_oracle/shrapnel_repair/shrapnel_repair.py',
        'tools/spatial_oracle/slave_manager.py',
        'tools/spatial_oracle/track_destination.py',
        'tools/spatial_oracle/unit_entry.py',
        'tools/spatial_oracle/unit_scatter_state.py',
        'tools/spatial_oracle/unit_source_scatter.py',
        'tools/spatial_oracle/walk_head_occupation.py',
    )
    return {name: repo / name for name in paths}


def main(argv=None):
    args = list(sys.argv[1:] if argv is None else argv)
    if '--gate' in args:
        args.remove('--gate')
        finish_vectors(generate_gate, Path(__file__).with_name('building_guard_attack_gate.json'),
                       provenance=gate_provenance, source_paths=gate_source_paths(), argv=args)
        return
    finish_vectors(
        generate, Path(__file__).with_suffix('.json'),
        provenance=lambda: provenance(
            scope='BuildingClass::Mission_Guard 0x4496B0, BuildingClass::Mission_Attack 0x44ACF0, '
                  'BuildingClass::SetTarget 0x443B90, BuildingClass 0x44D6A0 and their dispatch by '
                  'MissionClass::AI 0x5B3060 between BuildingClass::Update\'s ready checks; '
                  'additive hidden-startup UpdateAnimation and native mission-counter controls',
            entry_points={'mission_guard': MISSION_GUARD, 'mission_attack': MISSION_ATTACK,
                          'set_target': BUILDING_SET_TARGET, 'enter_construction': ENTER_CONSTRUCTION,
                          'mission_ai': bc.MISSION_AI, 'update_ready_commence_unless_building': 0x43FE27,
                          'update_ready_commence': 0x43FF91,
                          'update_animation': bc.UPDATE_ANIMATION,
                          'mission_counter_host': GATE_MISSION_HOST[0],
                          'queued_body': bc.QUEUED_BSTATE[0]},
            assumptions=['the building_construction fixture refinery (slave_manager fixture, Building '
                         'vtables), armed through Weapon[0] = a supplied WeaponType whose Projectile is a '
                         'supplied BulletType; TarCom is the fixture\'s unit',
                         'MissionControl Rate/AARate written as float-widened doubles (ReadDouble '
                         '0x5283D0); retail [Guard] .030/.016, [Sticky] .016/.016, [Attack] .016/.016',
                         'cadence rows run only the Update mission pieces; UpdateAnimation, TechnoClass::'
                         'AI_Update\'s other steps and the passive scan are not run (the row events stand '
                         'in for the scan\'s TarCom write)',
                         'hidden_startup adds three two-frame controls through original44D6A0 under '
                         'ScenarioInit, then4509D0,43FE27..43FE54,6FA646..6FA65A,'
                         '43FF91..43FFB4 and43FFB4..440042. Type+CA1 Turret and prior+6DD ready '
                         'are inputs; the actor is armed and has no target. The non-turret is an '
                         'isolated ready-gate contrast, not a retail CABARR02 type replay. Discovery, '
                         'first Grand_Opening, passive targeting and whole BuildingUpdate are excluded. '
                         'All three1012-byte RNG buffers are seeded1 after setup; each frame records '
                         'native mission/body timer start/duration stores, excluding stack padding'],
            substitutions=['BuildingClass::GetFireError 0x447F10 answered (per row, or the rearm test in '
                           'cadence rows); Fire_At 0x6FDD50 answered (cadence rows write its rearm timer); '
                           'TechnoClass::SetTarget 0x6FCDB0 answered with its +0x2B4 write; FacingClass::'
                           'Set_Desired 0x4C9220, the direction 0x43ED40, StartUncloaking 0x7036C0, the '
                           'delayed-fire anim 0x451890 and ClearBibArea 0x449540 observed and answered '
                           '(DestroyNthAnim 0x451E40 by the refinery_dock observer); Is_Operational 0x4555D0, IsCloseEnough 0x6F7780, IsHumanPlayer '
                           '0x50B730 and the EMP test 0x70EFD0 answered from the row']),
        argv=argv)


if __name__ == '__main__':
    main()
