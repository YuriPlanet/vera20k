"""Source-aware Scatter with the original Infantry Can_Enter_Cell body."""
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.infantry_source_scatter import query

NEIGHBORS = [(10, 9), (11, 9), (11, 10), (11, 11),
             (10, 11), (9, 11), (9, 10), (9, 9)]

def generate():
    cases = [dict(),
             dict(raw=[[11, 9, 0x20, 0]]),
             dict(raw=[[11, 9, 0x1c, 0]]),
             dict(raw=[[x, y, 0x20, 0] for x, y in NEIGHBORS]),
             dict(blocked_terrain=[[11, 9]]),
             dict(blocked_terrain=NEIGHBORS),
             dict(cells=[[11, 9, 2, 0]]),
             dict(cells=[[11, 9, 1, 0]]),
             dict(cells=[[11, 9, 1, 0]], slopes=[[10, 10, 1]]),
             dict(cells=[[11, 9, 0, 0x100]], raw=[[11, 9, 0, 0x20]]),
             dict(cells=[[11, 9, 0, 0x100]], raw=[[11, 9, 0x20, 0]]),
             dict(cells=[[x, y, 0, 0x100] for x, y in NEIGHBORS]),
             dict(on_bridge=True, cells=[[10, 10, 0, 0x300], [11, 9, 0, 0x300]],
                  raw=[[11, 9, 0x20, 0]]),
             dict(on_bridge=True, cells=[[10, 10, 0, 0x300], [11, 9, 0, 0x300]],
                  raw=[[11, 9, 0, 0x20]])]
    return [query(dict(live_entry=True, **case)) for case in cases]

if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original Infantry Scatter through real Can_Enter_Cell: source selection, raw occupation, speed rows, height and bridge planes. Destination is observed; no movement continuation claim.',
        entry_points={'scatter': 0x51D0D0, 'entry': 0x51BF90,
                      'height_list': 0x4D9C60, 'height': 0x5F5F00},
        assumptions=['Inherits infantry_source_scatter setup with native Walk interface, source, RNG, map lookups and widened synthetic playfield.',
                     'No overlay or object list members, no Tubes; owner indices are -1. Land0 has nine nonzero speed rows, Land1 nine zero rows. Raw masks, cell levels/slopes/flags and actor OnBridge are declared inputs.',
                     'Native class +1AC and +1B0 execute unchanged. These cases do not establish overlay/object-list parity or retail boundary reachability.'],
        substitutions=['QueueMission and SetDestination are argument-checking observers.']))
