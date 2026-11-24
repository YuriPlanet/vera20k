"""Original Drive/Ship MoveTo, Foot destination and ordinary Unit caller.

All class and locomotor functions run unchanged. The supplied objects have no
radio contacts, lift links, retained fire particles, EMP or deploy timer.
"""
from pathlib import Path
import struct
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords, packed
from tools.spatial_oracle.unit_source_scatter import (
    make_source_fixture, ACTOR, TYPE, LOCO, CELLS, SCENARIO,
)
from tools.spatial_oracle.unit_entry import EXTRA, HOUSE, CELL
from tools.spatial_oracle.unit_scatter_state import SP


def make_destination_fixture(case, *, uc=None):
    u, call, read32 = make_source_fixture(dict(case, live_entry=True), uc=uc)
    u.mem_map(EXTRA, 0x30000)
    family = case['family']
    if family == 'ship':
        call(0x69EC50, LOCO, [])
        u.mem_write(LOCO + 0xC, dwords(ACTOR))
        u.mem_write(LOCO + 0x14, dwords(1))
    u.mem_write(LOCO + 0x10, bytes([not case.get('power_off', False)]))
    u.mem_write(ACTOR, dwords(0x7F5C70))
    u.mem_write(TYPE, dwords(0x7F6218))
    u.mem_write(ACTOR + 0x21C, dwords(HOUSE))
    u.mem_write(ACTOR + 0x14, dwords(5))
    # Unit735416, Radio65A750 and Foot4D31E0 constructor prestates.
    u.mem_write(ACTOR + 0x6D8, dwords(-1))
    u.mem_write(ACTOR + 0xE0, dwords(0x7E180C, EXTRA + 0x2F000, 1))
    u.mem_write(ACTOR + 0xEC, b'\x01\x01')
    for offset in (0x588, 0x5AC):
        call(0x4E0E80, ACTOR + offset, [0, 0])
        u.mem_write(ACTOR + offset, dwords(0x7E91EC))
        u.mem_write(ACTOR + offset + 0x10, dwords(0, 10))
    rules = case.get('rules_pointer', EXTRA + 0x10000)
    u.mem_write(0x8871E0, dwords(rules))
    if 'rules_pointer' not in case:
        u.mem_write(rules + 0x1768, dwords(22))
    if family == 'jumpjet':
        # Original constructor54AC40 reads this supplied Rules rotation before
        # native Link54AD30 replaces it with the retained type's actual ROT.
        # A retained original Rules constructor/reader supplies its own value.
        if 'rules_pointer' not in case:
            u.mem_write(rules + 0x40C, dwords(4))
        call(0x54AC40, LOCO, [])
        u.mem_write(LOCO + 0xC, dwords(ACTOR))
        u.mem_write(LOCO + 0x14, dwords(1))
        u.mem_write(LOCO + 0x10, bytes([not case.get('power_off', False)]))
        u.mem_write(ACTOR + 0x6C0, dwords(TYPE))
    u.mem_write(ACTOR + 0x270, bytes([case.get('warp_out', False), case.get('warp_in', False)]))
    u.mem_write(ACTOR + 0x5A0, dwords(CELLS))
    u.mem_write(ACTOR + 0x5A4, dwords(CELL if case.get('same_nav') else 0))
    u.mem_write(ACTOR + 0x5E0, dwords(2, 3, 4, 5))
    u.mem_write(ACTOR + 0x558, packed(9, 8))
    u.mem_write(ACTOR + 0x640, dwords(50, 0, 5))
    u.mem_write(ACTOR + 0x64C, dwords(7))
    u.mem_write(ACTOR + 0x668, dwords(40, 0, 6))
    u.mem_write(ACTOR + 0x6B7, b'\x01')
    u.mem_write(ACTOR + 0x6AC, bytes([case.get('skip_move', False)]))
    u.mem_write(ACTOR + 0x1F8, bytes([case.get('force_reassign', False)]))
    if case.get('nav_queue'):
        items = EXTRA + 0x2C000
        u.mem_write(items, dwords(*([CELL] * case['nav_queue'])))
        u.mem_write(ACTOR + 0x588 + 4, dwords(items, case['nav_queue']))
        u.mem_write(ACTOR + 0x598, dwords(case['nav_queue']))
    if family != 'jumpjet':
        u.mem_write(LOCO + 0x34, dwords(*case.get('prior', [700, 800, 900])))
        u.mem_write(LOCO + 0x40, dwords(*case.get('head', [2816, 2688, 123])))
    u.mem_write(CELL + 0x140, dwords(0x100 if case.get('bridge') else 0))
    # Original bridge-scale leaves, with their established level scale104.
    for address in (0x8A07D0, 0xB07838):
        u.mem_write(address, dwords(104))
    call(0x4AF4A0, 0, [])
    call(0x69EBB0, 0, [])
    assert read32(0x8A07C4) == read32(0xB0782C) == 416
    return u, call, read32


def query(case):
    u, call, read32 = make_destination_fixture(case)
    events = []
    addresses = {0x741970:'unit', 0x4D94B0:'foot', 0x4AFD40:'drive_move',
                 0x69F450:'ship_move', 0x4E0190:'clear_queue', 0x565730:'lookup'}
    def observe(_u, address, _size, _data):
        # Destination acceptance never calls Foot::Find_Path. The first
        # locomotor Process owns that request, even for an obstructed route.
        assert address != 0x4D3920, 'destination setter performed Find_Path'
        if address in addresses:
            events.append(addresses[address])
    u.hook_add(UC_HOOK_CODE, observe)
    before = bytes(u.mem_read(ACTOR, 0x700))
    entry = case['entry']
    if entry == 'move':
        call(0x4AFD40 if case['family'] == 'drive' else 0x69F450, 0,
             [LOCO + 4, *case['request']])
        assert u.reg_read(UC_X86_REG_ESP) == SP + 20
        assert bytes(u.mem_read(ACTOR, 0x700)) == before
    else:
        target = 0 if case.get('null') else CELL
        call(0x741970 if entry == 'unit' else 0x4D94B0, ACTOR, [target, case.get('flag', 1)])
        assert u.reg_read(UC_X86_REG_ESP) == SP + 12
    call(read32(read32(LOCO + 4) + 0x10), 0, [LOCO + 4])
    moving = bool(u.reg_read(UC_X86_REG_EAX) & 255)
    signed = lambda address, count: list(struct.unpack('<' + 'i' * count, u.mem_read(address, count * 4)))
    return dict(input=case, events=events,
                destination=signed(LOCO + 0x34, 3), head=signed(LOCO + 0x40, 3),
                power=u.mem_read(LOCO + 0x10, 1)[0], moving=moving,
                nav=[11, 10] if read32(ACTOR + 0x5A4) == CELL else None,
                aux=bool(read32(ACTOR + 0x5A0)), path=signed(ACTOR + 0x5E0, 4),
                reference=list(struct.unpack('<hh', u.mem_read(ACTOR + 0x558, 4))),
                movement_timer=[read32(ACTOR + 0x640), read32(ACTOR + 0x648)],
                blocked_timer=[read32(ACTOR + 0x668), read32(ACTOR + 0x670)],
                blocked=u.mem_read(ACTOR + 0x6B7, 1)[0], retries=read32(ACTOR + 0x64C),
                skip_move=u.mem_read(ACTOR + 0x6AC, 1)[0],
                force_reassign=u.mem_read(ACTOR + 0x1F8, 1)[0],
                **({'nav_queue': read32(ACTOR + 0x598)} if 'nav_queue' in case else {}))


def generate():
    cases = []
    for family in ('drive', 'ship'):
        for warp in ({}, {'warp_out':True}, {'warp_in':True}):
            for power_off in (False, True):
                for bridge in (False, True):
                    base = dict(family=family, power_off=power_off, bridge=bridge, **warp)
                    for request in ([2944, 2688, -123], [0, 0, 0]):
                        cases.append(dict(base, entry='move', request=request))
                    cases.append(dict(base, entry='move', request=[0, 0, 0], head=[0, 0, 0]))
                    cases.append(dict(base, entry='foot'))
                    cases.append(dict(base, entry='unit'))
        for extra in ({'same_nav':True}, {'same_nav':True, 'force_reassign':True}, {'skip_move':True}):
            cases.append(dict(family=family, entry='unit', **extra))
        # NavQueue: the Cell setter keeps it; only a NULL destination reaches
        # the Clear at 0x7423BE (0x742091 TEST EBX,EBX / JZ).
        # A NULL destination needs a prior NavCom (0x741A80 returns without one).
        for null in (False, True):
            cases.append(dict(family=family, entry='unit', same_nav=null, nav_queue=2, null=null))
        # The Cell setter's clear (0x7422E8..0x7422F4) is gated by its flag.
        cases.append(dict(family=family, entry='unit', same_nav=False, nav_queue=2, null=False, flag=0))
    return [query(case) for case in cases]


def null_boundary_query(case):
    """Run the real NULL caller with a declared Drive/Ship locomotor boundary.

    An Aircraft vtable is deliberately paired with a constructed Drive/Ship:
    this isolates Foot's class/mission gate without claiming Fly Stop parity.
    No class, locomotor or gameplay function is substituted.
    """
    u, call, read32 = make_destination_fixture(dict(case, same_nav=case.get('nav', True)))
    aircraft = case['entry'] == 'aircraft'
    actor_vtable = 0x7E22A4 if aircraft else 0x7F5C70
    u.mem_write(ACTOR, dwords(actor_vtable))
    assert read32(0x7E22A4 + 0x480) == 0x41AA80
    assert read32(0x7EB058 + 0x480) == 0x51AA40
    u.mem_write(ACTOR + 0xAC, dwords(case['mission']))
    u.mem_write(ACTOR + 0xB4, dwords(case['queued']))
    u.mem_write(ACTOR + 0x2B4, dwords(CELL if case['target'] else 0))
    u.mem_write(ACTOR + 0x6AD, bytes([case.get('swap', False)]))
    u.mem_write(ACTOR + 0x82, bytes([case.get('open_transport', False)]))
    u.mem_write(ACTOR + 0x2E4, dwords(CELL if case.get('bunker', False) else 0))
    u.mem_write(EXTRA + 0x10000 + 0x1768, dwords(case['blockage']))
    u.mem_write(ACTOR + 0x640, dwords(50, 777, 5))
    u.mem_write(ACTOR + 0x668, dwords(40, 888, 6))
    # NULL does not query the owner's type-dependent coordinates or radio
    # interfaces; the original Aircraft prefix jumps straight to Foot.
    stop = read32(read32(LOCO + 4) + 0x48)
    assert stop == (0x4AFE00 if case['family'] == 'drive' else 0x69F510)
    observed = {
        0x41AA80: 'aircraft', 0x4D94B0: 'foot',
        0x4D94C7: 'aux_clear', 0x4D9510: 'nav_clear',
        0x4D9672: 'class_gate', 0x4D969C: 'target_gate',
        0x4D96B9: 'stop_call', stop: 'stop_entry',
        0x4D96BC: 'nav_clear_again', 0x4D96C2: 'timer_tail',
    }
    events, boundaries, random_calls = [], [], []

    def observe(_u, address, _size, _data):
        if address in observed:
            events.append(observed[address])
        if address in (stop, 0x4D96BC, 0x4D96C2):
            boundaries.append(dict(
                at=observed[address], nav=bool(read32(ACTOR + 0x5A4)),
                aux=bool(read32(ACTOR + 0x5A0)),
                destination=list(struct.unpack('<iii', u.mem_read(LOCO + 0x34, 12))),
            ))
        if address in (0x65C780, 0x65C7E0):
            random_calls.append(hex(address))

    u.hook_add(UC_HOOK_CODE, observe)
    call(0x41AA80 if aircraft else 0x4D94B0, ACTOR, [0, case['flag']])
    assert u.reg_read(UC_X86_REG_ESP) == SP + 12
    recorded_random_calls = list(random_calls)
    call(0x65C780, SCENARIO + 0x218, [])
    continuation = u.reg_read(UC_X86_REG_EAX)
    signed = lambda address, count: list(struct.unpack('<' + 'i' * count,
                                                       u.mem_read(address, count * 4)))
    return dict(
        input=case, events=events, boundaries=boundaries,
        destination=signed(LOCO + 0x34, 3), head=signed(LOCO + 0x40, 3),
        nav=bool(read32(ACTOR + 0x5A4)), aux=bool(read32(ACTOR + 0x5A0)),
        movement_timer=[read32(ACTOR + 0x640), signed(ACTOR + 0x648, 1)[0]],
        blocked_timer=[read32(ACTOR + 0x668), signed(ACTOR + 0x670, 1)[0]],
        blocked=bool(u.mem_read(ACTOR + 0x6B7, 1)[0]),
        retries=read32(ACTOR + 0x64C),
        path=signed(ACTOR + 0x5E0, 4),
        skip_move=bool(u.mem_read(ACTOR + 0x6AC, 1)[0]),
        random_calls=recorded_random_calls, next_random=continuation,
    )


def generate_null_boundary():
    cases = []
    for family in ('drive', 'ship'):
        for entry in ('foot', 'aircraft'):
            base = dict(family=family, entry=entry, mission=0, queued=-1,
                        target=False, frame=123, blockage=22, flag=1)
            for mission, queued in ((0, -1), (1, -1), (0, 1), (1, 1), (2, -1)):
                for target in (False, True):
                    cases.append(dict(base, name=f'gate_{mission}_{queued}_{target}',
                                      mission=mission, queued=queued, target=target))
            for gate in ('swap', 'open_transport', 'bunker'):
                cases.append(dict(base, name=f'null_bypasses_{gate}', **{gate: True}))
            cases += [dict(base, name='flag_zero', flag=0),
                      dict(base, name='already_null', nav=False),
                      dict(base, name='skip_move_latch', skip_move=True),
                      dict(base, name='unpowered', power_off=True)]
            for frame, blockage in ((0, 0), (0xFFFFFFFF, -1),
                                    (0x80000000, -0x80000000), (123, 65536)):
                cases.append(dict(base, name=f'timers_{frame}_{blockage}',
                                  frame=frame, blockage=blockage))
    return [null_boundary_query(case) for case in cases]


def null_boundary_metadata():
    return provenance(
        scope='84 original NULL calls: Foot4D94B0 over Unit vtables and Aircraft41AA80 over Aircraft vtables, each with constructed original Drive/Ship locomotors. Complete current/queued Attack and retained-target gate combinations, NULL bypass of nonnull admission gates, setter flags, already-null NavCom, one-shot skip latch, power, frame/duration edges; ordered pre-Stop and timer boundaries, retained destinations/heads and Scenario RNG continuation. This is Foot/class NULL control and timer evidence, not native Fly Stop or a legal stock Aircraft locomotor combination.',
        entry_points={'foot': 0x4D94B0, 'aircraft': 0x41AA80,
                      'drive_stop': 0x4AFE00, 'ship_stop': 0x69F510,
                      'timer_tail': 0x4D96C2},
        assumptions=[
            'Inherited track_destination map/House/constructor fixture, original class vtables and original constructed Drive/Ship COM entries. Unit class Set_Destination is intentionally not called: Foot NULL is tested directly.',
            'Aircraft vtable +480=41AA80 and Infantry vtable +480=51AA40 are asserted from retail bytes. Aircraft NULL prelude executes unchanged; its Drive/Ship locomotor is a supplied boundary probe, not a stock Aircraft definition and not Fly Stop evidence.',
            'Live or empty NavCom, auxiliary sentinel, destination700/800/900, head2816/2688/123, retries7, old timer middle words777/888, path2/3/4/5 and skip latch are supplied. Timer comparisons observe start/duration only; native middle words are not claimed as zero or modeled timer state.',
            'No linked lift partner, retained fire particles, radio contacts, docking selection, death or locomotor Process. The swap probe supplies6AD but no2B0 link. These are prerequisite/lifecycle limits.',
            'Seeded original ScenarioRandom, unchanged gameplay bytes and no substituted gameplay callable. No random entry is reached by these NULL calls; the subsequent original Random continuation is recorded separately.',
        ],
        substitutions=['Only inherited OS Interlocked import operations. No gameplay replacement.'],
    )


if __name__ == '__main__':
    import sys
    if '--null-boundary' in sys.argv:
        from tools import native_oracle
        from tools.spatial_oracle import (
            map_queries, unit_entry, unit_source_scatter, unit_scatter_state,
        )
        finish_vectors(
            generate_null_boundary, Path(__file__).with_name('track_destination_null_boundary.json'),
            provenance=null_boundary_metadata,
            argv=[arg for arg in sys.argv[1:] if arg != '--null-boundary'],
            source_paths={'track_destination': Path(__file__),
                          'native_oracle': Path(native_oracle.__file__),
                          'map_queries': Path(map_queries.__file__),
                          'unit_entry': Path(unit_entry.__file__),
                          'unit_source_scatter': Path(unit_source_scatter.__file__),
                          'unit_scatter_state': Path(unit_scatter_state.__file__)},
        )
    else:
        finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
            scope='132 complete original calls:72 Drive/Ship MoveTo,24 Foot destination,30 ordinary Unit Cell destination and 6 Unit Cell/NULL destinations with a 2-entry NavQueue (setter flag 1, and flag 0 for Cell), each followed by original IsMoving. Warp-in/out, power, zero/raw/bridge coordinates, same NavCom/force, one-shot skip-MoveTo and NavQueue survival. No full Scatter/Process parity.',
            entry_points={'unit':0x741970,'foot':0x4D94B0,'drive_move':0x4AFD40,'ship_move':0x69F450,
                          'drive_bridge_scale':0x4AF4A0,'ship_bridge_scale':0x69EBB0},
            assumptions=['Original constructors for Drive/Ship and embedded vectors; supplied Unit constructor6D8=-1, empty Radio contact slot, House, map and Rules BlockagePathDelay22. No EMP, Foot6A0 timer, lift/particle links, deploy, Jumpjet/Teleporter type arms, docking buildings or queue allocation.',
                         'Original bridge scales execute from supplied established level scale104. Native Unit/Foot/locomotor vtables are unchanged; actor and head are supplied prestates. Power remains unchanged; MoveTo refusal does not refuse the enclosing accepted Foot setter.'],
            substitutions=['Only inherited OS Interlocked import operations. No gameplay replacement.']))
