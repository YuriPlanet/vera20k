"""Original Foot4DDC60 landing-space predicate with real Cell/Map callees."""
from pathlib import Path
import struct

from tools.native_oracle import SCRATCH, finish_vectors, provenance
from tools.spatial_oracle.fly_landing_phase import fixture, OWNER, TYPE, HOUSE, dwords
from tools.spatial_oracle.aircraft_fire_location import BLOCKER, BLOCKER_TYPE, cell, SCENARIO


def execute(case):
    f, _ = fixture({})
    u = f.u
    query = case.get('query', [64, 64])
    out = SCRATCH + 0x92000
    u.mem_write(out, dwords(query[0]*256+128, query[1]*256+128, 0))
    target = f.call(0x565730, 0x87F7E8, [out])
    assert target == cell(64, 64)
    u.mem_write(0x89EA40, struct.pack('<90f', *[case.get('land_cost', 1.0)]*90))
    u.mem_write(target+0x11B, bytes([case.get('level', 0), 0]))
    u.mem_write(target+0x124, bytes([case.get('occupation', 0)]))
    occupant = case.get('occupant')
    if occupant:
        pointer = OWNER if occupant == 'self' else BLOCKER
        u.mem_write(target+0xE4, dwords(pointer))
        u.mem_write(pointer+0x9C, dwords(16512, 16512, 0))
        u.mem_write(pointer+0x21C, dwords(HOUSE))
        if occupant == 'contact':
            u.mem_write(SCRATCH+0x70000, dwords(pointer))
        u.mem_write(TYPE+0xD54, bytes([case.get('spawned', False)]))
        u.mem_write(BLOCKER_TYPE+0xD54, bytes([case.get('occupant_spawned', False)]))
        u.mem_write(BLOCKER+0x2D0, dwords(int(case.get('spawn_manager', False))))
    if 'reservation' in case:
        actor = SCRATCH+0x90000
        u.mem_write(actor+0x90, bytes([case.get('alive', True)]))
        u.mem_write(actor+0x81, bytes([case.get('limbo', False)]))
        r = case['reservation']
        u.mem_write(out, dwords(r[0]*256+128, r[1]*256+128, 0))
        destination = f.call(0x565730, 0x87F7E8, [out])
        if case.get('reservation_self'):
            actor = OWNER
        u.mem_write(actor+0x5A4, dwords(destination))
        u.mem_write(0xA8E394, dwords(SCRATCH+0x91000))
        u.mem_write(SCRATCH+0x91000, dwords(actor))
        u.mem_write(0xA8E3A0, dwords(1))
    before_rng = bytes(u.mem_read(SCENARIO+0x218, 0x3F4))
    result = bool(f.call(0x4DDC60, OWNER, [target]) & 0xFF)
    assert before_rng == bytes(u.mem_read(SCENARIO+0x218, 0x3F4))
    return dict(input=case, result=result, rng_changed=False)


def cases():
    rows = [dict(name='clear'), dict(name='blocked_land', land_cost=0),
            dict(name='infantry_bit', occupation=1), dict(name='unit_bit', occupation=0x20),
            dict(name='generic_object_bit', occupation=0x40)]
    rows += [dict(name=f'occupant_{o}', occupant=o) for o in ('self', 'other', 'contact')]
    rows += [dict(name=f'spawned_{s}_manager_{m}_other_{o}', occupant='other',
                  spawned=s, spawn_manager=m, occupant_spawned=o)
             for s in (False, True) for m in (False, True) for o in (False, True)]
    rows += [dict(name=f'reserved_alive_{a}_limbo_{l}', reservation=[64,64], alive=a, limbo=l)
             for a in (False, True) for l in (False, True)]
    rows += [dict(name='other_reservation', reservation=[65,64]),
             dict(name='self_reservation', reservation=[64,64], reservation_self=True)]
    rows += [dict(name='alias_clear', query=[576,63]),
             dict(name='alias_occupied', query=[576,63], occupant='other'),
             dict(name='alias_reservation', reservation=[576,63]),
             dict(name='alias_current_reservation', query=[576,63], reservation=[64,64])]
    return rows


if __name__ == '__main__':
    finish_vectors(lambda: [execute(c) for c in cases()], Path(__file__).with_suffix('.json'),
                   provenance=lambda: provenance(
        entry_points={'landing_space':0x4DDC60, 'cell_at_coord':0x565730,
                      'nearest_techno':0x47C3D0, 'passability':0x4834A0},
        assumptions=['Supplied ordinary marked Aircraft, real Cell/Map vtables and allocated 128x128 cells.',
                     'Flat Clear cells, no overlay, Track land cost zero or one; supplied raw occupation and contacts.',
                     'Aircraft registry has at most one supplied live/dead/limbo reservation.'],
        substitutions=[],
        scope='26 full Foot landing-space calls: occupancy, radio contact, spawned exceptions, Track cost, reservation liveness/self and fixed-grid aliases. No caller lifecycle, missing-cell Dummy, docking or complete Fly Process proof.'))
