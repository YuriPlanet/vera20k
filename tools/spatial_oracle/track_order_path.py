"""Original Unit destination -> first Drive/Ship Process path-request timing.

Runs real setters and Process bodies, stopping at the first Foot::Find_Path
entry or ordinary return. No path result, route or movement is supplied.
"""
from itertools import product
from pathlib import Path
import struct
import sys

from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP
from tools.native_oracle import RET_MAGIC, finish_vectors, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords
from tools.spatial_oracle.track_destination import make_destination_fixture, ACTOR, LOCO, CELL
from tools.spatial_oracle.unit_scatter_state import SP

PROCESS = {'drive': 0x4B0500, 'ship': 0x69FC10}
FIND_PATH = 0x4D3920


def query(case):
    u, call, read32 = make_destination_fixture(dict(case, entry='unit', head=[0, 0, 0]))
    calls = []
    observed = {0x741970: 'unit', 0x4D94B0: 'foot', 0x4AFD40: 'drive_move',
                0x69F450: 'ship_move', FIND_PATH: 'find_path',
                PROCESS[case['family']]: 'process'}
    u.hook_add(UC_HOOK_CODE, lambda _u, address, _size, _data:
               calls.append(observed[address]) if address in observed else None)
    call(0x741970, ACTOR, [CELL, 1])
    assert 'find_path' not in calls

    def snapshot():
        signed = lambda address, count: list(struct.unpack('<' + 'i' * count,
                                                           u.mem_read(address, count * 4)))
        return dict(destination=signed(LOCO + 0x34, 3), head=signed(LOCO + 0x40, 3),
                    nav=[11, 10] if read32(ACTOR + 0x5A4) == CELL else None,
                    path=signed(ACTOR + 0x5E0, 4), power=u.mem_read(LOCO + 0x10, 1)[0],
                    movement_timer=[read32(ACTOR + 0x640), read32(ACTOR + 0x648)])

    accepted = snapshot()
    # Independent timer prestate represents a later Process invocation. Zero
    # is the exact accepted-setter output; 7 isolates the original wait arm.
    u.mem_write(ACTOR + 0x640, dwords(100, 0, case['delay']))
    entry = read32(read32(LOCO + 4) + 0x40)
    assert entry == PROCESS[case['family']]
    u.mem_write(SP, dwords(RET_MAGIC, LOCO + 4))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ECX, 0)
    stop = run_checked(u, entry, (RET_MAGIC, FIND_PATH), count=200000,
                       required_addresses=[entry])
    return dict(input=case, accepted=accepted, process=snapshot(), calls=calls,
                requested_path=stop == FIND_PATH)


def generate():
    return [query(dict(family=family, power_off=power_off, delay=delay))
            for family, power_off, delay in product(PROCESS, (False, True), (0, 7))]


def rest_fixture(row):
    """Reuse the sole original destination/map fixture, with declared rest inputs."""
    from tools.spatial_oracle.track_destination import SCENARIO, CELLS
    u, call, read = make_destination_fixture(dict(
        family='drive', head=row.get('head', [0, 0, 0]),
        prior=row.get('destination', [0, 0, 0]), mission=row.get('mission', 2)))
    u.mem_write(ACTOR + 0x90, b'\1')
    u.mem_write(ACTOR + 0x5E0, dwords(row.get('queue', -1)))
    u.mem_write(ACTOR + 0x578, struct.pack('<d', row.get('speed', 0.75)))
    u.mem_write(LOCO + 0x50, struct.pack('<d', 0.9))
    if row.get('nav'):
        u.mem_write(ACTOR + 0x5A4, dwords(CELLS + (10 * 32 + 10) * 0x200))
    if row.get('turn'):
        u.mem_write(ACTOR + 0x388, struct.pack('<H', 0))
        u.mem_write(ACTOR + 0x38C, struct.pack('<H', 0x2000))
        u.mem_write(ACTOR + 0x390, dwords(100, 0, 4))
        u.mem_write(ACTOR + 0x39C, struct.pack('<h', 16))
        u.mem_write(0xA8ED84, dwords(101))
    for address, seed in ((0x886B88, 37), (0xABE890, 41)):
        call(0x65C6D0, address, [seed])
    streams = [SCENARIO + 0x218, 0x886B88, 0xABE890]
    before_rng = [bytes(u.mem_read(address, 0x3F4)) for address in streams]
    calls = []
    def observe(_u, pc, _size, _data):
        if pc in (0x65C780, 0x65C7E0):
            raise AssertionError('Unexpected RNG in Drive Stop/rest/order')
        if pc in (0x4AFE00, 0x741970, 0x4B0500, 0x4B078C, 0x4B0828,
                  0x4D3710, FIND_PATH):
            calls.append(f'{pc:08X}')
    u.hook_add(UC_HOOK_CODE, observe)
    return u, call, read, calls, streams, before_rng


def rest_state(u):
    return dict(speed_bits=bytes(u.mem_read(ACTOR + 0x578, 8))[::-1].hex(),
                target_bits=bytes(u.mem_read(LOCO + 0x50, 8))[::-1].hex(),
                destination=list(struct.unpack('<iii', u.mem_read(LOCO + 0x34, 12))),
                head=list(struct.unpack('<iii', u.mem_read(LOCO + 0x40, 12))),
                queue=struct.unpack('<i', u.mem_read(ACTOR + 0x5E0, 4))[0],
                coords=list(struct.unpack('<iii', u.mem_read(ACTOR + 0x9C, 12))))


def rest_query(row):
    u, call, read, calls, streams, before_rng = rest_fixture(row)
    before = rest_state(u)
    call(PROCESS['drive'], 0, [LOCO + 4])
    assert u.reg_read(UC_X86_REG_ESP) == SP + 8
    assert [bytes(u.mem_read(address, 0x3F4)) for address in streams] == before_rng
    return dict(input=row, before=before, after=rest_state(u), calls=calls,
                rng_unchanged=True)


def rest_history(row):
    u, call, read, calls, streams, before_rng = rest_fixture(dict(
        mission=5, destination=[2944, 2688, 0]))
    states = [dict(stage='initial', **rest_state(u))]
    if row['stop'] == 'locomotor':
        call(0x4AFE00, 0, [LOCO + 4])
    else:
        # A nonnull prior NavCom makes this a real NULL setter, rather than
        # Unit741970's idempotent already-NULL return.
        u.mem_write(ACTOR + 0x5A4, dwords(CELL))
        call(0x741970, ACTOR, [0, 1])
    states.append(dict(stage='stop', **rest_state(u)))
    if row['rest']:
        u.mem_write(0xA8ED84, dwords(101))
        call(PROCESS['drive'], 0, [LOCO + 4])
        states.append(dict(stage='rest', **rest_state(u)))
    call(0x741970, ACTOR, [CELL, 1])
    # The command host has accepted Move; native Command/mission AI is an
    # external prerequisite here. The actual Unit setter and Process run.
    u.mem_write(ACTOR + 0xAC, dwords(2))
    u.mem_write(0xA8ED84, dwords(102))
    states.append(dict(stage='new_order', **rest_state(u)))
    boundary = call(PROCESS['drive'], 0, [LOCO + 4], stop=(RET_MAGIC, FIND_PATH))
    assert boundary == FIND_PATH
    states.append(dict(stage='first_path_request', **rest_state(u)))
    assert [bytes(u.mem_read(address, 0x3F4)) for address in streams] == before_rng
    return dict(input=row, states=states, calls=calls, rng_unchanged=True,
                boundary=f'{boundary:08X}')


def paid_rest_history(wait):
    """Retained Unit across Stop/rest/new order and six full Drive visits.

    Only AStar supplies a one-cell east path. Original Foot FindPath, class
    entry, fresh head, paid points and outer Process execute unchanged.
    """
    from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP
    from tools.spatial_oracle.track_destination import TYPE, HOUSE, EXTRA
    from tools.spatial_oracle.track_path_continuation import install_zone_inputs, PATH_TYPE
    from tools.spatial_oracle.track_speed_native import seed_getter_state
    u, call, read, calls, streams, before_rng = rest_fixture(dict(
        mission=5, destination=[2944, 2688, 0]))
    rules = read(0x8871E0)
    seed_getter_state(u, dict(raw=15, rank=0.0, applied=0.75),
                      ACTOR, TYPE, HOUSE, EXTRA + 0x20000, rules)
    # Original startup49F3A0 fills the LEPTON direction table (distinct
    # from49F2F0's Cell deltas); omitting it produces an invalid head.
    call(0x49F3A0, 0, [])
    u.mem_write(TYPE + 0xA0, dwords(300))
    u.mem_write(ACTOR + 0x6C, dwords(300))
    u.mem_write(rules + 0x1700, struct.pack('<d', 0.5))
    # Foot constructor4D31F1 initializes the unused planning-path index -1;
    # TurnComplete/PerCell otherwise tries to follow a nonexistent plan.
    u.mem_write(ACTOR + 0x520, dwords(-1))
    u.mem_write(TYPE + 0xDBD, b'\1')
    u.mem_write(TYPE + 0x2F8, dwords(500))
    u.mem_write(TYPE + 0x300, struct.pack('<d', 0.002))
    u.mem_write(TYPE + 0x308, struct.pack('<d', 0.03))
    install_zone_inputs(u, read)
    # An east-facing body isolates the speed timing from turn duration.
    u.mem_write(ACTOR + 0x388, dwords(0x4000, 0x4000))
    u.mem_write(ACTOR + 0x390, dwords(-1, 0, 0))
    call(0x4AFE00, 0, [LOCO + 4])
    if wait:
        call(PROCESS['drive'], 0, [LOCO + 4])
    call(0x741970, ACTOR, [CELL, 1])
    u.mem_write(ACTOR + 0xAC, dwords(2))
    path_calls = []
    def supply_path(_u, pc, _size, _data):
        if pc == 0x4CBBA0:
            sp = u.reg_read(UC_X86_REG_ESP)
            buffer = read(sp + 8)
            u.mem_write(buffer, dwords(2, -1))
            u.mem_write(PATH_TYPE, dwords(0, 1, 2, buffer, 0, 0, 0, 0))
            path_calls.append('one-cell-east')
            u.reg_write(UC_X86_REG_EAX, PATH_TYPE)
            u.reg_write(UC_X86_REG_EIP, read(sp))
            u.reg_write(UC_X86_REG_ESP, sp + 28)
    u.hook_add(UC_HOOK_CODE, supply_path)
    states = []
    for frame in range(101, 107):
        u.mem_write(0xA8ED84, dwords(frame))
        call(PROCESS['drive'], 0, [LOCO + 4])
        assert u.reg_read(UC_X86_REG_ESP) == SP + 8
        states.append(dict(frame=frame, **rest_state(u)))
    assert path_calls == ['one-cell-east']
    assert [bytes(u.mem_read(address, 0x3F4)) for address in streams] == before_rng
    return dict(rest=wait, states=states, calls=calls, rng_unchanged=True,
                astar=path_calls)


def rest():
    cases = [dict(name='turn', turn=True), dict(name='guard', mission=5),
             dict(name='ordinary', mission=2),
             dict(name='head-x', turn=True, head=[256, 0, 0]),
             dict(name='head-y', turn=True, head=[0, 256, 0]),
             dict(name='head-z', turn=True, head=[0, 0, 1]),
             dict(name='destination', turn=True, destination=[2944, 2688, 0]),
             dict(name='queue', turn=True, queue=2),
             dict(name='zero', turn=True, speed=0.0),
             dict(name='negative', turn=True, speed=-0.25),
             dict(name='same-cell', mission=2, nav=True),
             dict(name='guard-destination', mission=5, destination=[2688, 2688, 0])]
    return dict(process=[rest_query(row) for row in cases],
                histories=[rest_history(dict(stop=stop, rest=wait))
                           for stop in ('locomotor', 'unit_null') for wait in (False, True)],
                paid=[paid_rest_history(wait) for wait in (False, True)])


if __name__ == '__main__' and '--rest' in sys.argv:
    finish_vectors(rest, Path(__file__).with_name('track_rest_speed.json'),
        provenance=lambda: provenance(
            scope='Twelve whole original Drive Process rest/early-return controls '
                  'and four Stop-or-UnitNull -> optional rest -> new Unit destination '
                  'histories ending before the next Foot FindPath executes, plus two '
                  'six-frame paid departure histories with one supplied AStar route.',
            entry_points={'drive_process':0x4B0500, 'common_tail':0x4B078C,
                          'rest_speed':0x4B0828, 'drive_stop':0x4AFE00,
                          'unit_destination':0x741970, 'set_speed':0x4D3710,
                          'find_path_boundary':FIND_PATH},
            assumptions=['Inherited original destination/map/Drive construction fixture; '
                         'Unit/Type state is supplied, not a whole native scenario load.',
                         'Level0 clear map; explicit destination,head,queue,mission, '
                         'applied0.75/0/-0.25,target0.9 and facing timer. '
                         'Every complete measured RNG buffer remains identical.',
                         'Histories supply accepted Move mission2 after the Unit setter; '
                         'native command admission is outside coverage. Paid histories additionally '
                         'declare health/strength300,rawSpeed15,house/crate1,rank0,Accelerates=yes, '
                         'accel0.03,decel0.002,SlowdownDistance500 and east facing; original '
                         'lepton-direction startup49F3A0 executes before movement.'],
            substitutions=['Only inherited OS Interlocked import bodies. '
                           'Process/Stop/setter results are never substituted. Only the paid '
                           'histories supply AStar4CBBA0 one-cell-east PathType/moves; original '
                           'Foot FindPath/fresh/class entry/paid Track/Process execute.']),
        argv=[arg for arg in sys.argv[1:] if arg != '--rest'])
    raise SystemExit


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='8 original Unit Cell setter -> first Drive/Ship Process path-request boundaries',
        entry_points={'unit': 0x741970, 'foot': 0x4D94B0, 'find_path': FIND_PATH, **PROCESS},
        substitutions=['Only OS Interlocked imports inherited from track_destination fixture.'],
        assumptions=[
            'Real Unit/Foot/locomotor vtables and calls. No paid head, radio/EMP/deploy/lift/particle state.',
            'Both power values; accepted movement timer is zero. Delay7 is supplied after acceptance to isolate the Process wait arm.',
            'Shared flat map/Rules fixture; PathDelay is zero. Stops BEFORE Find_Path executes; no search result, movement or full Process claim.']))
