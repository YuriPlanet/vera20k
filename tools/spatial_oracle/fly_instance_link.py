"""Original Fly LinkToObject copies Aircraft-only AirportBound into instance+18."""
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.fly_landing_phase import fixture, LOCO, OWNER, TYPE, dwords


def generate():
    rows = []
    for aircraft in (False, True):
        for bound in (False, True):
            f, _ = fixture({})
            u = f.u
            u.mem_write(OWNER, dwords(0x7E22A4 if aircraft else 0x7F5C70))
            u.mem_write(TYPE+0xE0D, bytes([bound]))
            result = f.call(0x4CCA20, 0, [LOCO+4, OWNER])
            copied = bool(u.mem_read(LOCO+0x18, 1)[0])
            u.mem_write(TYPE+0xE0D, bytes([not bound]))
            rows.append(dict(aircraft=aircraft, type_airport_bound=bound, result=result,
                             linked=copied, after_type_change=bool(u.mem_read(LOCO+0x18, 1)[0])))
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'link':0x4CCA20,'base_link':0x55A710,'constructor':0x4CC9A0},
        assumptions=['Real Fly constructor and original Aircraft/Unit RTTI vtables; supplied type+E0D.',
                     'No allocation, owner class construction, locomotor replacement or save/load coverage.'],
        substitutions=[], scope='Four original Link calls proving Aircraft-only instance copy and retention after a supplied live type change.'))
