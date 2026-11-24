"""Original Fly phase dispatcher, pure-takeoff arm with real Mark/Display calls."""
from pathlib import Path

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.fly_takeoff import cases, execute


def generate():
    inputs = [case for case in cases() if not case.get('landing', False)]
    inputs += [dict(z=900,health=health) for health in (0,-1)]
    inputs += [dict(z=900,taking_off=False)]
    inputs += [dict(z=z,marked=True) for z in (0,900)]
    return [execute(case,phase_transaction=True) for case in inputs]


if __name__ == '__main__':
    finish_vectors(generate,Path(__file__).with_suffix('.json'),provenance=lambda:provenance(
        entry_points={'phase_dispatch':0x4CD2A0,'takeoff':0x4CE680,'foot_mark':0x4D3780,
                      'display_init':0x4A8630,'submit':0x4A9720,'remove':0x4A9770},
        assumptions=['Landable Aircraft with supplied taking-off flag, no landing flag, health and original facing histories from fly_takeoff.',
                     'Original display-array constructor prefix stops before atexit; supplied capacity16 buffers avoid allocator growth.',
                     'Owner is initially unmarked, or initialized through original Mark(PUT), and display-registered ahead of a peer; real Foot/Techno/Object Mark and Display calls execute.',
                     'Peer shares the owner locomotor only for its layer query and remains untouched; supplied map/cargo state is not a lifecycle proof.'],
        substitutions=[],
        scope='75 full original phase-dispatch calls on the pure-takeoff arm: native callback thresholds, flags/facings/speed, Mark/remove/callback/submit/Mark order and same-layer display reordering. Includes health and absent-takeoff gates and initially marked/unmarked owners. Excludes preceding Process horizontal/vertical updates, landing and non-Landable branches, BeginTakeoff and allocator failures.',
    ))
