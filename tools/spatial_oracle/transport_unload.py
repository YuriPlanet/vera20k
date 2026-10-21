"""Original Unit Unload latch controls and bounded retail object histories.

Default: four independent hull/turret state-1 controls in the existing dock VM.
--retail: BFRT turn/no-turn and FV/HTK turret-turn histories in FootMissions'
physical Anytown crop, using its original type/actor/locomotor/placement owners.
--deploy-event: ordinary ground-transport D producer, Event9 receiver and empty
Unload admission components in the same existing dock VM.
See transport_unload.md for supplied boundaries and input preparation.
"""
from pathlib import Path
import struct
import sys

from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_ESI, UC_X86_REG_ESP,
)
from tools.native_oracle import finish_vectors, provenance, run_checked
from tools.spatial_oracle.map_queries import dwords


def gate_controls():
    from tools.spatial_oracle.refinery_dock import make_dock_fixture
    from tools.spatial_oracle.unit_scatter_state import ACTOR, TYPE, SOURCE
    from tools.spatial_oracle.unit_source_scatter import SCENARIO

    rows = []
    for turret in (False, True):
        for hull_turn in (False, True):
            u, call, read32 = make_dock_fixture(dict(
                harvester=False, linked=False, mission='unload', west_building=False, frame=100))
            u.mem_write(TYPE + 0x5E0, dwords(5))
            u.mem_write(TYPE + 0xCA1, bytes([turret]))
            u.mem_write(ACTOR + 0x2E4, dwords(0))
            u.mem_write(ACTOR + 0x2B4, dwords(0))
            u.mem_write(ACTOR + 0xBC, dwords(1))
            for offset in (0x388, 0x3A0):
                call(0x4C91C0, ACTOR + offset, [])
                call(0x4C9680, ACTOR + offset, [5])
                u.mem_write(SOURCE, dwords(0))
                call(0x4C9300, ACTOR + offset, [SOURCE])
            if hull_turn:
                u.mem_write(SOURCE, dwords(0x2000))
                call(0x4C9220, ACTOR + 0x388, [SOURCE])
            if turret:
                u.mem_write(SOURCE, dwords(0x4000))
                call(0x4C9220, ACTOR + 0x3A0, [SOURCE])
            u.mem_write(0xA8ED84, dwords(101))
            call(0x736990, ACTOR, [])
            latch = bool(u.mem_read(ACTOR + 0x6AF, 1)[0])
            call(0x4C9480, ACTOR + 0x388, [])
            hull_rotating = bool(u.reg_read(UC_X86_REG_EAX) & 255)
            before = bytes(u.mem_read(SCENARIO + 0x218, 1012)).hex()
            call(0x73D630, ACTOR, [])
            rows.append(dict(turret=turret, hull_turn=hull_turn,
                hull_rotating=hull_rotating, latch=latch, state=read32(ACTOR + 0xBC),
                delay=u.reg_read(UC_X86_REG_EAX), rng_before=before,
                rng_after=bytes(u.mem_read(SCENARIO + 0x218, 1012)).hex()))
    return rows


def retail_history(kind, initial_facing, turn_turret):
    from tools.projectile_oracle.bridge_render_inputs import BulletReader
    from tools.spatial_oracle.anytown_damage.foot_missions import FootMissions
    from tools.spatial_oracle.anytown_damage.mission import base
    from tools.spatial_oracle.building_body_rules import RULES, SP

    class TransportReader(BulletReader):
        def invoke(self, address, *args, **kwargs):
            if address == 0x747620:
                # FV's full turret/weapon list reader exceeds the ordinary
                # host-time allowance under Python instruction observers.
                kwargs.setdefault('timeout_us', 60_000_000)
            return super().invoke(address, *args, **kwargs)

    class Transport(FootMissions):
        def __init__(self):
            self.unload_calls, self.unload_returns = [], {}
            self.hull_rotating_before = False
            super().__init__(reader_factory=TransportReader)

        def setup(self):
            return super().setup(source_type_name=kind,
                source_art_bytes=(base.layers()[0][1].parent / 'ARTMD.INI').read_bytes(),
                stop_after_placement=True)

        def observe(self, u, pc, size, data):
            if pc in self.unload_returns:
                row = self.unload_returns.pop(pc)
                passenger = self.snap(self.e1)
                row.update(state=self.m.read32(self.src + 0xBC),
                    delay=u.reg_read(UC_X86_REG_EAX),
                    cargo=self.m.read32(self.src + 0x114),
                    turret_latch=bool(u.mem_read(self.src + 0x6AF, 1)[0]),
                    passenger_position=passenger['position'],
                    passenger_queued=passenger['queued'],
                    passenger_limbo=bool(u.mem_read(self.e1 + 0x81, 1)[0]),
                    transporter_cleared=self.m.read32(self.e1 + 0x11C) == 0,
                    draws=[dict(args=e['args'], value=e['returned_eax'])
                        for e in self.events[row.pop('event_start'):] if e['kind'] == 'rng'])
            if pc == 0x73D630 and u.reg_read(UC_X86_REG_ECX) == self.src:
                row = dict(frame=self.frame, state_before=self.m.read32(self.src + 0xBC),
                    hull_rotating_before=self.hull_rotating_before,
                    body=list(struct.unpack('<6I', u.mem_read(self.src + 0x388, 24))),
                    turret=list(struct.unpack('<6I', u.mem_read(self.src + 0x3A0, 24))),
                    latch_before=bool(u.mem_read(self.src + 0x6AF, 1)[0]),
                    event_start=len(self.events))
                self.unload_calls.append(row)
                self.unload_returns[self.m.read32(u.reg_read(UC_X86_REG_ESP))] = row
            if pc == 0x73D8EC:
                self.unload_calls[-1]['ejection_heading'] = (
                    self.m.read32(u.reg_read(UC_X86_REG_EAX)) & 0xffff)
            super().observe(u, pc, size, data)

    q = Transport()
    q.initialize_companion()
    m, u = q.m, q.u
    # The shared mission fixture selects other MissionControl sections. Read
    # this chain's exact Unload section through the same original Rules loop.
    unload_inputs = []
    for name, path in base.layers():
        if not path.exists():
            if name != 'LANGRULE.INI':
                raise FileNotFoundError(path)
            continue
        selected, _ = base.lexical(path.read_bytes(), {'Unload'})
        m.rules_cache(selected)
        u.reg_write(UC_X86_REG_ESP, SP)
        u.reg_write(UC_X86_REG_ESI, RULES)
        run_checked(u, 0x679C92, 0x679CAF, required_addresses=[0x5B3760])
        unload_inputs.append(dict(file=name, section=selected,
            rate_bits=bytes(u.mem_read(0xA8E3A8 + 16 * 32 + 0x10, 8)).hex()))
    q.phase = 'logic'
    out = m.alloc(16)
    u.mem_write(0xA8ED84, dwords(100))
    u.mem_write(out, dwords(initial_facing))
    for offset in (0x388, 0x3A0):
        m.invoke(0x4C9300, q.src + offset, (out,))
    if turn_turret:
        u.mem_write(out, dwords(0x6000))
        m.invoke(0x4C9220, q.src + 0x3A0, (out,))
    m.invoke(0x51DF10, q.e1)
    m.invoke(0x4733A0, q.src + 0x114, (q.e1,))
    # Already-boarded prior: Infantry Enter51A463's retained Transporter link.
    u.mem_write(q.e1 + 0x11C, dwords(q.src))
    m.invoke(0x5B35E0, q.src, (16, 0))
    m.invoke(0x5B3570, q.src)
    source_position = base.xyz(u, q.src + 0x9C)
    for frame in range(100, 260):
        q.frame = frame
        u.mem_write(0xA8ED84, dwords(frame))
        q.hull_rotating_before = bool(m.invoke(0x4C9480, q.src + 0x388) & 255)
        m.invoke(0x7360C0, q.src)
        if m.read32(q.src + 0xAC) == 5 and m.read32(q.src + 0x114) == 0:
            break
    if q.unload_returns:
        raise RuntimeError('Original Mission_Unload did not return')
    if m.read32(q.src + 0xAC) != 5 or m.read32(q.src + 0x114) != 0:
        raise RuntimeError('Original retail unload did not finish by frame 259')
    return dict(kind=kind, initial_facing=initial_facing, turn_turret=turn_turret,
        source_position=source_position, unload_inputs=unload_inputs,
        turret=bool(u.mem_read(q.typ + 0xCA1, 1)[0]),
        calls=q.unload_calls, cleanup_mission=m.read32(q.src + 0xAC))


def retail_controls():
    return dict(bfrt_turn=retail_history('BFRT', 0, False),
        bfrt_aligned=retail_history('BFRT', 0x2000, False),
        fv_turret=retail_history('FV', 0x2000, True),
        htk_turret=retail_history('HTK', 0x2000, True))


def _deploy_fixture(mission, cargo):
    from tools.spatial_oracle import refinery_dock as rd

    u, call, read = rd.make_dock_fixture(dict(
        harvester=False, linked=False, mission='guard', west_building=False,
        frame=100, seed=1, nav=[11, 10], moving=mission == 2))
    for offset, values in (
        (0xAC, (mission, 7, -1)), (0xB8, (1,)), (0xBC, (7,)),
        (0xC0, (50, 90, 40, 777, 99)),
        (0x114, (cargo, rd.OTHER if cargo else 0)),
        (0x218, (rd.cell(7, 7),)),
        (0x2B4, (rd.cell(12, 12), rd.cell(13, 13))),
        (0x5A8, (rd.cell(14, 14),)), (0x6E4, (9,)),
    ):
        u.mem_write(rd.ACTOR + offset, dwords(*values))
    u.mem_write(rd.ACTOR + 0x90, b'\x01')
    u.mem_write(rd.ACTOR + 0x81, b'\x00')
    u.mem_write(rd.TYPE + 0x5E0, dwords(2))
    return u, call, read


def _deploy_trace(u, read):
    from unicorn import UC_HOOK_CODE
    from unicorn.x86_const import UC_X86_REG_ESP
    from tools.native_oracle import NativeCallTrace

    specs = {
        0x65ACB0: ('over_out_first_contact', 1, 4),
        0x741970: ('unit_assign_destination', 2, 8),
        0x6FCDB0: ('techno_assign_target', 1, 4),
        0x5B35E0: ('queue_mission', 2, 8),
        0x5B3570: ('commence', 0, 0),
        0x700D50: ('can_deploy_slash_unload', 0, 0),
        0x65C7E0: ('scenario_random_ranged', 2, 8),
        0x65C780: ('main_random', 0, 0),
        0x750920: ('sound_play', 2, 8),
        0x7509E0: ('sound_play_at', 4, 16),
        0x708D90: ('queue_voice', 1, 4),
        0x70D150: ('action_timer_start', 1, 4),
        0x7258D0: ('detach', 2, 8),
        0x5F5280: ('object_detach', 2, 8),
        0x405190: ('allocate_sound_sample', 0, 0),
        0x4061D0: ('sound_sample_volume', 0, 0),
        0x406270: ('play_sound_sample', 0, 0),
    }
    trace = NativeCallTrace(u, read)

    def observe(_u, pc, _size, _data):
        sp = u.reg_read(UC_X86_REG_ESP)
        trace.returned(pc, sp)
        if pc in specs:
            trace.entered(pc, sp, specs[pc])

    u.hook_add(UC_HOOK_CODE, observe)
    return trace


def _deploy_calls(trace, start=0):
    rows = []
    for call in trace.calls[start:]:
        row = dict(name=call['name'], args=call['args'])
        if call['name'] == 'scenario_random_ranged':
            row['value'] = call['return_eax']
        elif call['name'] == 'can_deploy_slash_unload':
            row['returned_al'] = call['return_eax'] & 255
        rows.append(row)
    return rows


def _deploy_snapshot(u, read):
    from tools.spatial_oracle import refinery_dock as rd

    signed = lambda off: struct.unpack('<i', u.mem_read(rd.ACTOR + off, 4))[0]
    return dict(
        mission=signed(0xAC), suspended_mission=signed(0xB0), queued=signed(0xB4),
        movement_bypass=u.mem_read(rd.ACTOR + 0xB8, 1)[0], status=signed(0xBC),
        mission_start=signed(0xC0), mission_accum=signed(0xC4),
        timer_start=signed(0xC8), timer_duration=signed(0xD0),
        target=rd.cell_xy(read(rd.ACTOR + 0x2B4)),
        suspended_target=rd.cell_xy(read(rd.ACTOR + 0x2B8)),
        archive=rd.cell_xy(read(rd.ACTOR + 0x218)),
        nav=rd.cell_xy(read(rd.ACTOR + 0x5A4)),
        suspended_nav=rd.cell_xy(read(rd.ACTOR + 0x5A8)),
        cargo=signed(0x114), keep_count=signed(0x6E4),
        drive_destination=list(struct.unpack('<3i', u.mem_read(rd.LOCO + 0x34, 12))))


def deploy_event_control(mission, cargo):
    from tools.native_oracle import RET_MAGIC
    from tools.spatial_oracle import refinery_dock as rd
    from tools.spatial_oracle.unit_scatter_state import SP

    u, call, read = _deploy_fixture(mission, cargo)
    text = bytes(u.mem_read(0x401000, 0x3E0000))
    trace = _deploy_trace(u, read)
    rng = lambda: bytes(u.mem_read(rd.SCENARIO + 0x218, 1012))

    def moving():
        call(read(read(rd.LOCO + 4) + 0x10), 0, [rd.LOCO + 4])
        return bool(u.reg_read(UC_X86_REG_EAX) & 255)

    before = _deploy_snapshot(u, read)
    before['drive_is_moving'] = moving()
    initial_rng = rng()
    # Resolved actor supplied after Event9's token/planning prefix. All later
    # lifecycle, terrain, mission, class and factory gates execute unchanged.
    u.mem_write(SP, bytes(0x100))
    u.reg_write(UC_X86_REG_ESP, SP)
    u.reg_write(UC_X86_REG_ESI, rd.ACTOR)
    stop = run_checked(u, 0x4C76D7, (0x4C7818, 0x4C8109), count=300000,
                       required_addresses=[0x65ACB0, rd.ASSIGN, 0x6FCDB0, rd.QUEUE])
    trace.returned(stop, u.reg_read(UC_X86_REG_ESP))
    after = _deploy_snapshot(u, read)
    after['drive_is_moving'] = moving()
    result = dict(input=dict(mission=mission, cargo=cargo), before=before,
                  event_stop=hex(stop), after_event=after,
                  scenario_rng_before_hex=initial_rng.hex(),
                  event_rng_unchanged=rng() == initial_rng,
                  event_calls=_deploy_calls(trace))
    mark = len(trace.calls)
    # Explicit promotion component: this is not a Ready/Logic scheduler port.
    call(rd.COMMENCE, rd.ACTOR, [])
    trace.returned(RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
    result.update(commence_returned_al=u.reg_read(UC_X86_REG_EAX) & 255,
                  after_direct_commence=_deploy_snapshot(u, read),
                  commence_rng_unchanged=rng() == initial_rng,
                  commence_calls=_deploy_calls(trace, mark))
    if cargo == 0 and mission != 16:
        mark = len(trace.calls)
        call(0x73D630, rd.ACTOR, [])
        trace.returned(RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
        result.update(unload_returned_delay=u.reg_read(UC_X86_REG_EAX),
                      after_first_empty_unload=_deploy_snapshot(u, read),
                      scenario_rng_after_unload_hex=rng().hex(),
                      unload_calls=_deploy_calls(trace, mark))
    assert not trace.pending
    assert bytes(u.mem_read(0x401000, 0x3E0000)) == text
    return result


def deploy_key_producer(cargo):
    from tools.native_oracle import RET_MAGIC
    from tools.spatial_oracle import refinery_dock as rd
    from tools.spatial_oracle.unit_scatter_state import SP

    u, _call, read = _deploy_fixture(5, cargo)
    text = bytes(u.mem_read(0x401000, 0x3E0000))
    items = rd.EXTRA + 0x2C800
    for address, value in ((0xA83D4C, rd.HOUSE), (0xA8ECC8, 1),
                           (0xA8ECBC, items), (items, rd.ACTOR)):
        u.mem_write(address, dwords(value))
    u.mem_write(rd.ACTOR + 0x83, b'\x01')
    u.mem_write(rd.TYPE + 0x230, b'\x01')
    trace = _deploy_trace(u, read)
    u.mem_write(SP, dwords(RET_MAGIC))
    u.reg_write(UC_X86_REG_ESP, SP)
    stop = run_checked(u, 0x730AF0, (RET_MAGIC, 0x6FFE00), count=200000,
                       required_addresses=[0x730AF0, 0x700D50])
    trace.returned(stop, u.reg_read(UC_X86_REG_ESP))
    assert not trace.pending
    assert bytes(u.mem_read(0x401000, 0x3E0000)) == text
    return dict(cargo=cargo, stop=hex(stop),
                event_opcode=read(u.reg_read(UC_X86_REG_ESP) + 4)
                    if stop == 0x6FFE00 else None,
                calls=_deploy_calls(trace))


def deploy_default_sound():
    from unicorn.x86_const import UC_X86_REG_EBP, UC_X86_REG_EDX
    from tools.native_oracle import RET_MAGIC
    from tools.spatial_oracle import refinery_dock as rd

    u, call, read = _deploy_fixture(5, 0)
    text = bytes(u.mem_read(0x401000, 0x3E0000))
    trace = _deploy_trace(u, read)
    # Constructor components produce the default rather than supplying -1 as
    # this test's answer. The intervening constructor/layer loader is excluded.
    u.reg_write(UC_X86_REG_EBP, 0xCCCCCCCC)
    u.reg_write(UC_X86_REG_ESI, rd.RULES)
    run_checked(u, 0x66585F, 0x665862)
    run_checked(u, 0x66606A, 0x666070)
    sound = read(rd.RULES + 0x72C)
    u.mem_write(0x8464AC, b'\x01')
    u.reg_write(UC_X86_REG_EDX, 0x2000)
    call(0x750920, sound, [0x3F800000, 0])
    trace.returned(RET_MAGIC, u.reg_read(UC_X86_REG_ESP))
    assert not trace.pending
    assert bytes(u.mem_read(0x401000, 0x3E0000)) == text
    return dict(sound_id=struct.unpack('<i', dwords(sound))[0], audio_enabled=True,
                edx_flags=0x2000, volume_bits='3f800000', second_stack_argument=0,
                returned_eax=u.reg_read(UC_X86_REG_EAX), calls=_deploy_calls(trace))


def deploy_event_controls():
    return dict(
        fixture=dict(passengers=2, frame=100, seed=1, unload_rate=0.016,
                     status=7, queued=-1, movement_bypass=1,
                     mission_start=50, mission_accum=90,
                     timer_start=40, timer_duration=99, suspended_mission=7),
        deploy_key=[deploy_key_producer(cargo) for cargo in (0, 1)],
        receiver=[deploy_event_control(mission, cargo)
                  for mission, cargo in ((5, 0), (2, 0), (16, 0), (2, 1))],
        default_sound=deploy_default_sound())


def deploy_event_metadata():
    return provenance(
        scope='Ordinary ground transport D: two selected-unit producers, four '
              'Event9/Commence admission controls, two empty first Unload visits '
              'within those controls, and a default D-sound component.',
        entry_points=dict(deploy_command=0x730AF0, event_producer=0x6FFE00,
                          deploy_receiver=0x4C76D7, deploy_receiver_stop=0x4C7818,
                          unit_destination=0x741970, techno_target=0x6FCDB0,
                          queue=0x5B35E0, commence=0x5B3570, unload=0x73D630,
                          scenario_random=0x65C7E0, sound=0x750920),
        assumptions=[
            'Reuse refinery_dock.make_dock_fixture and its original Unit/Type '
            'vtables, constructed Drive, supplied 32x32 map, House and Scenario '
            'seed1. Positive Passengers2, no bunker/radio contacts/EMP/factory, '
            'ordinary land and alive nonlimbo Unit; no original Unit constructor.',
            'Supplied frame100, Unload Rate0.016, old target/destination and '
            'retained archive/suspended Cell identities. Move rows begin with '
            'original Drive IsMoving true. Status7 and bypass1 are sentinels; '
            'the already-Unload row is not dispatched as a status7 handler.',
            'The count1 head is a nonnull fixture sentinel, never dereferenced: '
            'only producer/receiver/Commence admission is executed for it.',
            'Receiver starts with ESI=resolved actor after token/planning work. '
            'Direct Commence supplies the promotion boundary; no Ready/Logic '
            'scheduler or later mission-timer scheduling is executed.',
            'Default sound executes constructor OR/store components66585F and '
            '66606A, then whole750920 with audio enabled and D argument values; '
            'not full Rules construction, sound catalog or OS playback.'],
        substitutions=[
            'Only inherited Interlocked OS imports. No original gameplay '
            'instructions, vtables, parser answers, RNG results or decisions patched.',
            'D producer stops at6FFE00 before event encoding/delivery and the '
            'success sound tail. Receiver stops at4C7818 before its epilogue.',
            'Empty land transport executes one complete73D630 call after direct '
            'Commence. No passenger ejection/placement, later Unload/Guard '
            'promotion, full pathfinding or death mechanism.'])


if __name__ == '__main__':
    if '--deploy-event' in sys.argv:
        if '--retail' in sys.argv:
            raise SystemExit('--deploy-event and --retail select different corpora')
        finish_vectors(deploy_event_controls,
            Path(__file__).with_name('ground_transport_deploy.json'),
            argv=[arg for arg in sys.argv[1:] if arg != '--deploy-event'],
            provenance=deploy_event_metadata)
        raise SystemExit(0)
    retail = '--retail' in sys.argv
    finish_vectors(retail_controls if retail else gate_controls,
        Path(__file__).with_name('transport_unload_retail.json' if retail else 'transport_unload_gate.json'),
        argv=[arg for arg in sys.argv[1:] if arg != '--retail'],
        provenance=lambda: provenance(
            entry_points=dict(unit_ai=0x7360C0, facing_update=0x736990, unload=0x73D630,
                remove_passenger=0x4DE710, infantry_unlimbo=0x51DFF0)
                if retail else dict(facing_update=0x736990, unload=0x73D630),
            assumptions=['Retail controls reuse FootMissions physical type readers, constructors and Anytown crop.',
                'Already-boarded one-E1 prior; supplied frame100, hull/turret arcs and House.']
                if retail else ['Supplied type flags, independent Facing arcs and state1.',
                    'Facing ctor/snap/Set at frame100; original Facing_Update and Unload at101.'],
            substitutions=['Inherited FootMissions input, cropped-map and OS/presentation boundaries.',
                'Only the carrier Unit AI is visited; no passenger movement or other global phases.']
                if retail else ['Inherited dock VM construction and empty-cell map boundaries.'],
            scope='Unit transport latch gate; four bounded original retail unload histories.'
                if retail else 'Four original state1 controls through the real Facing_Update latch producer.'))
