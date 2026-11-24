"""Original Building455D50 destination setter and factory Stop prerequisites.

Real Building/BuildingType vtables and unchanged retail instructions execute.
Prepared actor/type/House/map fields bound these comparisons; no Windows game,
whole input queue, native object construction or rendering is claimed here.
"""

from itertools import product
from pathlib import Path

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDI, UC_X86_REG_ESI, UC_X86_REG_ESP

from tools.native_oracle import finish_vectors, provenance, run_checked, RET_MAGIC
from tools.spatial_oracle.bridge_damage_admission import (
    base, call, words, read32, MEM, SP,
)

ACTOR, TYPE, HOUSE, OLD, NEW, SELECTED = (MEM + offset for offset in (
    0x10000, 0x12000, 0x14000, 0x1A000, 0x1A100, 0x1B000))
PROFILES = {
    'factory': dict(factory=0x28),
    'barracks': dict(factory=0x10),
    'repair': dict(unit_repair=True),
    'cloning': dict(cloning=True),
    'construction_yard': dict(construction_yard=True),
    'ordinary': {},
    'aircraft_factory': dict(factory=2),
}
TARGETS = {'null': 0, 'old': OLD, 'new': NEW}
NAMES = {value: key for key, value in TARGETS.items()}


def execute(case):
    u = base({'flags': 0, 'level': 0})
    profile = PROFILES[case['profile']]
    # Exact original vtables; every virtual actually reached runs native code.
    u.mem_write(ACTOR, words(0x7E3EBC))
    u.mem_write(TYPE, words(0x7E4570))  # constructor45E2CD
    u.mem_write(ACTOR + 0x14, words(1))
    u.mem_write(ACTOR + 0x6C, words(1000))
    u.mem_write(ACTOR + 0x81, bytes([case.get('limbo', False)]))
    u.mem_write(ACTOR + 0x90, bytes([case.get('alive', True)]))
    u.mem_write(ACTOR + 0x9C, words(10 * 256 + 128, 20 * 256 + 128, 0))
    u.mem_write(ACTOR + 0xAC, words(case.get('mission', 5)))
    u.mem_write(ACTOR + 0x218, words(OLD))
    u.mem_write(ACTOR + 0x21C, words(HOUSE))
    u.mem_write(ACTOR + 0x418, bytes([case.get('tethered', False)]))
    u.mem_write(ACTOR + 0x504, words(case.get('emp', 0)))
    u.mem_write(ACTOR + 0x520, words(TYPE))
    u.mem_write(TYPE + 0xEB8, words(case.get('factory_value', profile.get('factory', -1))))
    for offset, key in ((0x16A9, 'unit_repair'), (0x16AC, 'cloning'),
                        (0x16B9, 'construction_yard')):
        u.mem_write(TYPE + offset, bytes([profile.get(key, False)]))
    u.mem_write(TYPE + 0x408, words(NEW if case.get('undeploy', False) else 0))
    u.mem_write(ACTOR + 0x2C0, words(NEW if case.get('mind_controlled', False) else 0))
    u.mem_write(HOUSE + 0x1EC, bytes([case.get('human', True)]))
    u.mem_write(HOUSE + 0x1ED, bytes([case.get('player_control', False)]))
    u.mem_write(0xA8B238, words(case.get('session_mode', 0)))
    u.mem_write(0xA8B320, bytes([case.get('mcv_redeploy', False)]))
    u.mem_write(0xA83D4C, words(HOUSE))
    u.mem_write(0xA8ECBC, words(SELECTED))
    u.mem_write(0xA8ECC8, words(1))
    u.mem_write(SELECTED, words(ACTOR))
    before = bytes(u.mem_read(ACTOR, 0x1000))
    calls, writes = [], []
    tracked = {0x455D50: 'building_destination', 0x455DA0: 'has_rally_point',
               0x70C610: 'archive_target', 0x709A30: 'techno_destination',
               0x44F5C0: 'building_command_admission', 0x7010D0: 'armed_admission',
               0x65ACE0: 'radio_broadcast'}

    def observe(uc, address, _size, _data):
        if address in tracked:
            calls.append(tracked[address])

    def written(uc, _access, address, size, value, _data):
        if ACTOR <= address < ACTOR + 0x1000:
            writes.append(dict(offset=address - ACTOR, size=size,
                               value=NAMES.get(value, value)))

    u.hook_add(UC_HOOK_CODE, observe)
    u.hook_add(UC_HOOK_MEM_WRITE, written)
    kind = case['kind']
    result = {}
    if kind == 'control':
        result['controllable'] = bool(call(u, 0x44F5C0, ACTOR) & 255)
        # Object Select's unchanged state-admission prefix uses the same
        # Building+A0; stop before selection-vector/callback side effects.
        u.mem_write(TYPE + 0x230, b'\x01')  # Selectable
        u.mem_write(0xA8ED6B, b'\x00')  # ordinary user selection
        result['selection_admission'] = {}
        for mission_only in (False, True):
            u.mem_write(ACTOR + 0x3D4, bytes([mission_only]))
            u.mem_write(SP, words(RET_MAGIC))
            u.reg_write(UC_X86_REG_ESP, SP)
            u.reg_write(UC_X86_REG_ECX, ACTOR)
            end = run_checked(u, 0x5F4520, (0x5F45A0, RET_MAGIC), count=20000,
                              context=dict(case, mission_only=mission_only))
            result['selection_admission'][str(mission_only).lower()] = end == 0x5F45A0
        u.mem_write(ACTOR + 0x3D4, b'\x00')
    elif kind == 'destination':
        call(u, 0x455D50, ACTOR, (TARGETS[case['requested']], case.get('mode', 1)))
    elif kind == 'stop_event':
        # Event6 actor/token resolution ends at4C74E8. Keep every following
        # actor admission, terrain, radio and destination instruction intact;
        # stop before target clearing4C75F3 or at the refused-event epilogue.
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, ACTOR)
        u.reg_write(UC_X86_REG_EDI, 0)
        end = run_checked(u, 0x4C74E8, (0x4C75F3, 0x4C8109), count=20000,
                          context=case)
        result['destination_reached'] = end == 0x4C75F3
    elif kind == 'stop_input':
        # Stop's unchanged loop runs to the native enqueue entry, or the
        # speech/return boundaries. No voice or event return is supplied.
        u.reg_write(UC_X86_REG_ESP, SP)
        end = run_checked(u, 0x730EA0, (0x6FFE00, 0x730F00, 0x730F1C),
                          count=20000, context=case)
        result['event'] = read32(u, u.reg_read(UC_X86_REG_ESP) + 4) if end == 0x6FFE00 else None
    else:
        raise ValueError(kind)
    after = bytes(u.mem_read(ACTOR, 0x1000))
    result.update(archive=NAMES[read32(u, ACTOR + 0x218)], calls=calls,
                  writes=writes, changed_actor_bytes=[i for i, (a, b) in
                                                       enumerate(zip(before, after)) if a != b])
    return dict(input=case, expected=result)


def generate():
    cases = [dict(kind='destination', profile=profile, mission=mission, requested=requested)
             for profile, mission, requested in product(PROFILES, (5, 18, 19), TARGETS)]
    cases += [dict(kind='destination', profile='factory', mission=5,
                   requested='new', mode=0),
              dict(kind='destination', profile='factory', mission=5,
                   requested='null', emp=10)]
    cases += [dict(kind='stop_event', profile=profile, mission=mission)
              for profile, mission in product(PROFILES, (5, 18, 19))]
    cases += [dict(kind='stop_event', profile='factory', mission=5, **extra)
              for extra in ({'tethered': True}, {'limbo': True}, {'alive': False})]
    cases += [dict(kind='stop_input', profile=profile, emp=emp)
              for profile, emp in product(PROFILES, (-1, 0, 10))]
    return dict(cases=[execute(case) for case in cases])


def metadata():
    return provenance(
        scope='Building455D50 complete setter, Event6 admitted actor through null destination, and Stop selected-building input through enqueue admission.',
        assumptions=[
            'Real vtables, prepared live actor/type/House fields, original HasRallyPoint and supplied map cell. Type keys are separately established by the rally input reader corpus.',
            'Event6 token/house resolution is a supplied actor boundary at4C74E8; admitted execution stops before target clear4C75F3, refused execution at4C8109.',
            'Stop input uses one selected building and human House; ends at original enqueue entry6FFE00 or speech/return, excluding queue insertion, voice and full UI lifetime.',
            'Destination matrix covers null, unchanged/new pointer, Selling and Construction missions, every HasRallyPoint arm, ConstructionYard, mode0/1 and an EMP-positive setter control.',
            'No native RNG call, timer write or detach is performed by the Building setter. Event prefix includes the real empty-contact radio broadcast; nonempty contact detach remains its existing owner.',
            'EMP-positive native input refusal is recorded; Rust has no EMP504 lifecycle/producer and its active-retail creation reachability remains unestablished.',
        ],
        substitutions=[],
        entry_points=dict(building_destination=0x455D50, has_rally_point=0x455DA0,
                          archive_target=0x70C610, techno_destination=0x709A30,
                          stop_event_actor=0x4C74E8, stop_input=0x730EA0))



def generate_control():
    cases = [dict(kind='control', profile=profile, emp=emp)
             for profile, emp in product(PROFILES, (-1, 0, 10))]
    cases += [dict(kind='control', profile=profile, human=False,
                   session_mode=1, undeploy=True, mind_controlled=True)
              for profile in ('factory', 'barracks', 'repair', 'cloning',
                              'ordinary', 'aircraft_factory')]
    yard = dict(kind='control', profile='construction_yard', undeploy=True,
                session_mode=1, human=True, player_control=False,
                mcv_redeploy=True, mind_controlled=False)
    cases.append(yard)
    cases += [dict(yard, **change) for change in (
        {'undeploy': False}, {'session_mode': 0}, {'human': False},
        {'human': False, 'player_control': True}, {'mcv_redeploy': False},
        {'mind_controlled': True}, {'emp': 1}, {'emp': -1},
        {'session_mode': 0, 'human': False, 'player_control': True},
    )]
    for case in cases:
        if case['profile'] == 'aircraft_factory':
            case['factory_value'] = 3  # native AbstractType::AircraftType
    return dict(cases=[execute(case) for case in cases])


def control_metadata():
    return provenance(
        scope='Building44F5C0 virtual+A0 complete body with original HasRallyPoint455DA0 and House50B730.',
        assumptions=[
            'Prepared live Building/BuildingType/House fields with original vtables; no constructors, INI reads, action caller or command event are executed.',
            'A resolved UndeploysInto pointer is nonnull fixture memory. The body tests its presence only and never dereferences the target.',
            'Every HasRallyPoint arm, signed EMP -1/0/positive, ConstructionYard human/player-control/session/MCVRedeploy/mind-controller conditions execute. Control rows explicitly use AircraftType=3 rather than the legacy destination fixture label aircraft_factory whose stored input was2.',
            'EMP is a raw native input only: VERA has no EMP504 lifecycle producer. Constructor-zero production behavior is the supported Rust subset.',
            'For each row Object5F4520 executes its original selection-admission prefix with Selectable=true and MissionOnly false/true, stopping at5F45A0 before membership/waypoint checks and selection mutation, or at its real refusal return. No full selection commit or Building MissionOnly producer is claimed.',
            'No RNG, timer write, detach or object mutation is expected; changed_actor_bytes and native function calls are recorded.',
        ], substitutions=[], entry_points=dict(building_control=0x44F5C0,
            has_rally_point=0x455DA0, house_controlled_by_human=0x50B730,
            object_selection_admission=0x5F4520))


if __name__ == '__main__':
    import sys
    control = '--control' in sys.argv
    if control:
        sys.argv.remove('--control')
    finish_vectors(generate_control if control else generate,
                   Path(__file__).with_suffix('.control.json' if control else '.json'),
                   provenance=control_metadata if control else metadata,
                   source_paths={'oracle': Path(__file__)})
