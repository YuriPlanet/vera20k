"""Complete Unit entry with allied Drive/Ship/Walk blockers and real IsMoving.

The linked list, raw occupation, NavCom and retained locomotor state are supplied
independently. This validates query consumption, not displacement or lifecycle.
"""
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.unit_entry import query
from tools.spatial_oracle.locomotor_moving import ENTRIES


def generate():
    zero, current, ahead = [0, 0, 0], [2944, 2688, 0], [2945, 2688, 0]
    states = [dict(family=family, destination=destination, head=head)
              for family in ('drive', 'ship')
              for destination, head in ((zero, zero), (zero, current),
                                        (zero, [2944, 2688, 104]), (zero, ahead), (ahead, zero))]
    states += [dict(family='walk', head=head, moving=moving)
               for moving in (False, True) for head in (zero, ahead)]
    rows = []
    for state in states:
        for occupation in (False, True):
            for bits in (0, 0x20):
                row = query(dict(objects=[dict(motion=state, occupation=occupation)],
                                 bits=bits, trace_motion=True))
                assert hex(ENTRIES[state['family']]) in row['calls']
                rows.append(row)
    return rows


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Complete Unit73F0A0 admission with allied Unit Drive/Ship and Infantry Walk blockers',
        entry_points={'unit_entry': 0x73F0A0, **ENTRIES},
        assumptions=['Shared unit_entry fixture; supplied allied ordered list and independent raw occupation.',
                     'Real Unit/Infantry/type/locomotor vtables. Drive/Ship constructed; Walk query fields supplied. Blocker NavCom absent and body turn idle.',
                     'No unlimbo, move/stop lifetime, full scatter or scenario load. Every row must execute the selected original IsMoving body and preserve object/house memory.'],
        substitutions=['Only OS Interlocked imports from shared Unit fixture; no gameplay callable substitution.']))
