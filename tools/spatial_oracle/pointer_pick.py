"""Original Tactical6DA380 nearest-object controls on declared registrations.

Reuse DiscoveryFixture's native constructor/Cell/platform transport. This does
not construct a Tank Bunker or emulate flight, drawing or mouse messages.
"""
from pathlib import Path
import struct

from tools import native_oracle as native
from tools.spatial_oracle import trigger_action_values as owner


def generate():
    fixture = owner.DiscoveryFixture()
    fixture.capture = False
    u = fixture.uc
    actors = [fixture.add_actor(name) for name in ('A', 'B')]
    for actor in actors:
        u.mem_write(actor + 0x90, b'\1')
        u.mem_write(actor + 0x81, b'\0')
        u.mem_write(actor + 0x220, owner.dwords(0))
    tactical, cursor = owner.SCRATCH + 0x72000, owner.SCRATCH + 0x74000
    u.mem_write(tactical, bytes(0x1000))
    u.mem_write(tactical + 0xDB0, owner.dwords(2))
    u.mem_write(0x887324, owner.dwords(tactical))
    original = bytes(u.mem_read(0x6DA380, 0x1D3))
    controls = []
    for name, point, positions, order in (
        ('weighted_Y_AB', (100, 100), ((112, 100), (100, 116)), (0, 1)),
        ('weighted_Y_BA', (100, 100), ((112, 100), (100, 116)), (1, 0)),
        ('close_weighted_Y', (100, 100), ((106, 100), (100, 108)), (0, 1)),
        ('closer_X_control', (100, 100), ((108, 100), (100, 116)), (0, 1)),
        ('equal_weighted_AB', (100, 100), ((108, 104), (100, 112)), (0, 1)),
        ('equal_weighted_BA', (100, 100), ((108, 104), (100, 112)), (1, 0)),
        ('strict_200_control', (100, 100), ((114, 100), (100, 120)), (0, 1)),
        ('bunker_overlap_points', (378, 300), ((376, 284), (391, 302)), (0, 1)),
        ('air_nearest_points', (391, 301), ((376, 284), (391, 302)), (0, 1)),
    ):
        for slot, index in enumerate(order):
            u.mem_write(0xB0CEC8 + 12 * slot,
                        owner.dwords(actors[index], *positions[index]))
        u.mem_write(cursor, owner.dwords(*point))
        before = fixture.rng_hashes()
        fixture.call(0x6DA380, tactical, [cursor])
        result = u.reg_read(owner.UC_X86_REG_EAX)
        assert result in actors
        assert fixture.rng_hashes() == before
        assert bytes(u.mem_read(0x6DA380, 0x1D3)) == original
        fixture.check_code()
        controls.append(dict(name=name, cursor=point, positions=positions,
                             registration_order=order, picked=actors.index(result)))
    return dict(y_weight=struct.unpack('<d', u.mem_read(0x7E1738, 8))[0],
                controls=controls)


def metadata():
    return native.provenance(
        scope='Unchanged Tactical6DA380 weighted nearest-object metric, strict threshold and encounter-order ties; supplied registrations.',
        assumptions=[
            'DiscoveryFixture owns both original Infantry constructor-prefix shells, Cell transport and seeded RNG streams. Alive, non-limbo and non-cloaked fields are supplied. These are not concrete Building/Jumpjet instances or a whole UI.',
            'Two pointer/XY entries at B0CEC8 and Tactical+DB0=2 are declared inputs; camera is zero. Original40DD20 casts, lifecycle/cloak gates, 0.5 coefficient, x87 arithmetic/conversion and strict comparisons execute. No Python formula supplies a picked identity.',
            'The final two controls use declared screen points from the separately executed Building registration/projection witness for issue2235. They test this kernel only; flight, registration producers, ProcessClickCoords, DetermineAction and release are outside this corpus.',
            'All three seeded RNG states and selected native code bytes remain unchanged. This selected pick path has no timer write or detach.',
        ],
        substitutions=['Only inherited DiscoveryFixture platform/constructor boundaries; no TacticalPick instruction, cast or metric answer is replaced.'],
        entry_points=dict(pick=0x6DA380, techno_cast=0x40DD20,
                          weighted_distance=0x6DA444))


if __name__ == '__main__':
    native.finish_vectors(generate, Path(__file__).with_suffix('.json'),
                          provenance=metadata)
