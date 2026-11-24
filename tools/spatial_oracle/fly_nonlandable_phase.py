"""Original non-Landable Fly phase branch, with live type FlightLevel getter.

Real constructor, Aircraft vtable, height/facing and Mark/Display setup come
from fly_takeoff. No gameplay call or original instruction is substituted.
"""
from pathlib import Path

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.fly_takeoff import execute


def generate():
    cases = [dict(z=z, landable=False, taking_off=takeoff, landing=landing,
                  flight_level=level, mode=mode, marked=True)
             for z in (0, 900)
             for takeoff, landing in ((False, False), (True, False),
                                     (False, True), (True, True))
             for level in (-1, 0, 120, 40000)
             for mode in (False, True)]
    rows = [execute(case, phase_transaction=True, capture_flight_controls=True)
            for case in cases]
    for row in rows:
        assert row['phase_calls'] == [] and row['calls'] == []
        assert row['phase'] == [0, 0] and row['mode']
        level = row['input']['flight_level']
        assert row['target_height'] == (1500 if level == -1 else level)
        assert row['speed'] == 0.25 and row['marked']
        assert any(layer == [0, 1] for layer in row['layers'])
        assert row['coordinates'] == [2688, 2688, row['input']['z']]
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        entry_points={'phase': 0x4CD2A0, 'constructor': 0x4CC9A0,
                      'flight_level': 0x717800},
        assumptions=['Health100 Aircraft with Landable=false, real Aircraft/Fly vtables.',
                     'Original Fly ctor followed by supplied callback flags, mode, target height1500, target speed0.25 and type FlightLevel.',
                     'Original Mark/Display registration retains owner ahead of peer; no callback, Mark or Display call may run during this phase branch.',
                     'Both facing controllers retain original frame90 turns, queried at frame100 after dispatch.'],
        substitutions=[],
        scope='64 complete original phase calls: all flag pairs, previous modes, physical heights0/900, type FlightLevel fallback/zero/positive/40000. Proves unconditional mode/height/flag writes and unchanged XYZ, speed, facings, Mark and Display order on non-Landable branch. Excludes outer Process admission, movement, landing, null MoveTo, mode-driven speed selection and rendered output.',
    ))
