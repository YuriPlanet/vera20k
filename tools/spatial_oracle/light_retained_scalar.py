"""Original Cell484680 scalar refresh of retained native normalization state.

Initial Cell fields come from the existing original-byte palette finalization
fixture. The complete scalar-refresh function and effect predicates execute;
no call is substituted. Palette rebuilding and controller timing are excluded.
"""
import hashlib
import struct
from pathlib import Path

from tools.native_oracle import call, finish_vectors, provenance
from tools.spatial_oracle.map_queries import dwords

FIXTURE = Path(__file__).parents[1] / 'palette_oracle/fixtures/cell-light-finalization.bin'
CELL, SCENARIO = 0xB00000, 0xB10000


def vectors():
    raw = FIXTURE.read_bytes()
    rows = list(struct.iter_unpack('<15i', raw))
    result = []
    for index in [0, 5, 6, 7]:
        row = rows[index]
        for ambient in [30, 250]:
            for ion in [False, True]:
                initial = bytearray(0x200)
                struct.pack_into('<i4h', initial, 0x104, *row[7:12])
                initial[0x11B] = 7
                scenario = bytearray(0x3600)
                struct.pack_into('<i', scenario, 0x352C, ambient)
                struct.pack_into('<ii', scenario, 0x3540, 50, 8)
                struct.pack_into('<ii', scenario, 0x3558, 0, 0)
                output = call(0x484680, ecx=CELL, writes={
                    CELL: bytes(initial), SCENARIO: bytes(scenario),
                    0xA8B230: dwords(SCENARIO),
                    0xA9FAB4: bytes([int(ion)]),
                    0xA9FABC: dwords(0), 0xA9FAC0: dwords(0),
                }, dumps={'cell': (CELL + 0x104, 12)}, timeout_instr=1000)
                fields = list(struct.unpack('<i4h', bytes.fromhex(output['dumps']['cell'])))
                if fields[:2] != list(row[7:9]):
                    raise RuntimeError('scalar refresh changed retained scale/additive')
                result.append(dict(finalization_index=index, initial_record=list(row),
                                   ambient_percent=ambient, level=7, ion=ion,
                                   normal_ground=50, normal_level=8,
                                   ion_ground=0, ion_level=0, fields=fields))
    return dict(initial_fixture_sha256=hashlib.sha256(raw).hexdigest(), cases=result)


if __name__ == '__main__':
    finish_vectors(vectors, Path(__file__).with_suffix('.json'),
                   provenance=lambda: provenance(
        scope='Complete Cell484680 refresh using retained native Cell104/108',
        assumptions=[
            'Initial normalization/additive/scalars are selected original palette_oracle finalization records; their file hash and full records accompany results',
            'Scenario352C current ambient and Normal/Ion Ground/Level are supplied; original effect-query globals are explicit',
            'Prepared Cell level7 is ordinary positive terrain height; not exhaustive signed byte/word overflow coverage',
            'No full-cell source gather, palette rebuild, map loop, controller scheduling or gameplay scene is executed',
        ], substitutions=[],
        entry_points={'scalar_refresh': 0x484680, 'ion': 0x53A100,
                      'dominator': 0x53B400, 'nuke': 0x53A110},
    ))
