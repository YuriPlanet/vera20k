"""Complete Unit admission with real Fly/Jumpjet blocker queries.

Aircraft and Infantry/Unit nodes use their real type/object/interface tables.
Lists and occupation are supplied independently; no flight lifecycle is claimed.
"""
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.unit_entry import query
from tools.spatial_oracle.air_locomotor_moving import inputs, ENTRIES


def generate():
    rows = []
    for state in inputs():
        for infantry in ((False, True) if state['family'] == 'jumpjet' else (False,)):
            for occupation in (False, True):
                for bits in (0, 0x20):
                    row = query(dict(objects=[dict(motion=state, occupation=occupation,
                                                   infantry=infantry)],
                                     bits=bits, trace_motion=True))
                    assert hex(ENTRIES[state['family']]) in row['calls']
                    rows.append(row)
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Complete Unit73F0A0 admission with allied Fly Aircraft and Jumpjet Unit/Infantry blockers',
        entry_points={'unit_entry': 0x73F0A0, **ENTRIES},
        assumptions=['Shared unit_entry fixture; real object/type/interface vtables. Supplied ordered ground list and independent raw occupation.',
                     'Fly pitch/request and Jumpjet phase/request are independent. Blocker NavCom absent and body turn idle.',
                     'All calls require the original selected IsMoving leaf and preserve object/house memory. No flight, unlimbo, scatter or scenario load.'],
        substitutions=['Only OS Interlocked imports from shared Unit fixture; no gameplay callable substitution.']))
