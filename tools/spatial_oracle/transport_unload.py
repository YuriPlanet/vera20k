"""Original Unit Unload latch controls and bounded retail object histories.

Default: four independent hull/turret state-1 controls in the existing dock VM.
--retail: BFRT turn/no-turn and FV/HTK turret-turn histories in FootMissions'
physical Anytown crop, using its original type/actor/locomotor/placement owners.
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


if __name__ == '__main__':
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
