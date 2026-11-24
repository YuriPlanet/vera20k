"""Original Unit73F0A0 traversal gates with real Foot4D9C60 height adjustment."""
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.unit_entry import query

DELTAS = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)]


def generate():
    cases = []
    # source level/flags/slope, target level/flags/slope, carried height.
    shapes = [(0, 0, 0, 0, 0, 0, 0), (0, 0, 0, 1, 0, 0, 0),
              (0, 0, 1, 1, 0, 0, 0), (1, 0, 0, 0, 0, 1, 1),
              (1, 0, 0, 0, 0, 0, 1), (0, 0, 0, 2, 0, 1, 0),
              (0, 0, 0, 0, 0x100, 0, 0), (0, 0x300, 0, 0, 0x300, 0, 4),
              (0, 0x300, 0, 0, 0x300, 0, -1), (0, 0x100, 0, 0, 0, 0, -1),
              (4, 0, 0, 0, 0x300, 0, 4), (4, 0, 0, 0, 0x100, 0, 4),
              (0, 0x100, 0, 4, 0, 0, 4), (0, 0, 0, 4, 0, 0, 4),
              (0, 0, 0, 0, 0, 0, 5), (0, 0, 0, 255, 0, 0, -1)]
    for direction in [-1, *range(8)]:
        dx, dy = DELTAS[(direction - 4) & 7]
        source = [11 + dx, 10 + dy]
        for sl, sf, ss, tl, tf, ts, height in shapes:
            cases.append(dict(direction=direction, height=height,
                              cells=[[*source, sl, sf], [11, 10, tl, tf]],
                              slopes=[[*source, ss], [11, 10, ts]]))
    # Independent list/raw planes; identical terrain but different carried heights.
    for height in (-1, 0, 1, 4):
        for bits, deck_bits in ((0, 0x20), (0x20, 0), (0, 0)):
            for deck in (False, True):
                cases.append(dict(direction=-1, height=height, cells=[[11, 10, 0, 0x100]],
                                  bits=bits, deck_bits=deck_bits, deck=deck, objects=[{}]))
    # Explicit previous pointer controls Foot+1B0; the Tube query still uses backstep.
    for height in (-1, 0, 4):
        cases.append(dict(direction=2, height=height, previous=[9, 10],
                          cells=[[9, 10, 0, 0x100], [11, 10, 0, 0x300]], bits=0x20))
    for direction in range(8):
        dx, dy = DELTAS[(direction - 4) & 7]
        for tube_direction in range(8):
            for at in ([11, 10], [11 + dx, 10 + dy]):
                cases.append(dict(direction=direction, height=0,
                                  tubes=[dict(cell=at, direction=tube_direction)]))
    for exit in ([0, 0], [11, 10], [20, 20], [0, 20]):
        cases.append(dict(direction=8, height=5, bits=0x20, objects=[{}],
                          tubes=[dict(cell=[11, 10], direction=2, exit=exit)]))
    cases.append(dict(direction=8, height=0, tubes=[]))
    for required in (0, 1):
        for land in (0, 1, 10):
            cases.append(dict(restricted_land=required, land=land))
        for overlay in (236, 237, 238):
            for height in (-1, 0, 1):
                cases.append(dict(restricted_land=required, land=3, overlay=overlay,
                                  direction=-1, height=height))
    return [query(case) for case in cases]


if __name__ == '__main__':
    finish_vectors(generate, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='328 complete Unit73F0A0 calls through original Foot4D9C60: direction/height/slope, bridge list and raw planes, previous pointer, Tube direction/endpoint gates and MovementRestrictedTo land/overlay exceptions. Excludes full pathfinding, Scatter continuation and map-boundary aliases.',
        entry_points={'unit_entry': 0x73F0A0, 'height_list': 0x4D9C60, 'get_tube': 0x484F20},
        assumptions=['Uses unit_entry fixture: real Drive, class tables and shared numeric tail. Map Cells are real; optional Tubes are supplied records. Explicit previous pointer changes only the Foot height source.',
                     'Restriction cases use original registered 1x1 tile dimensions and supplied nonzero land speed rows. Overlay236/237/238 has no flags. Other cases preserve unit_entry defaults; no off-map Team/shroud or lifecycle claim.'],
        substitutions=['Only OS Interlocked imports inherited from the Unit/Drive fixture. No gameplay substitution.']))
