"""Full original InRange on explicit distance and range controls.

These direct calls reuse the existing sparse-map range owner. Supplied numeric
fields isolate the shared range arithmetic changed by the FV Cell migration;
they do not claim a Kirov type/source override or a whole firing cycle.
"""
from pathlib import Path
import json
import struct

from tools.native_oracle import (
    file_span, finish_vectors, first_difference, image_bytes, provenance,
)
from tools.spatial_oracle.in_range_cell_boundary import execute

HERE = Path(__file__).resolve().parent


def generate():
    rows = []
    for limit in (384, 768, 1024, 6400):
        for distance in (limit, limit + 1, limit + 2):
            rows.append(execute(dict(
                name=f'range_{limit}_distance_{distance}',
                source_xyz=[10368, 10624, 0], target_xyz=[10368 + distance, 10624, 0],
                target_marked=0, target_on_bridge=0, range=limit, minimum_range=0,
                dummy=dict(xy=[111, -222], level=0, flags=0),
                mapped_source=dict(xy=[40, 41], level=0, flags=0),
            )))
    reference = HERE / 'range_ties.json'
    if reference.exists():
        actual = {row['input']['name']: row for row in rows}
        for old in json.loads(reference.read_bytes())['rows']:
            difference = first_difference(old, actual[old['input']['name']])
            assert difference is None, difference
    return dict(rows=rows)


def metadata():
    result = provenance(scope=__doc__, assumptions=[
        'The complete InRange6F7220 receiver executes with physical FV/weapon '
        'constructors/readers from in_range_cell_boundary.execute. Range, minimum, '
        'source coordinate argument and unmarked target placement are declared '
        'controls. Native floating-point startup, map queries and arithmetic '
        'execute; the range inputs are not retail HoverMissile values.',
        'Range6400 controls exercise the shared maximum used by the HornetLauncher '
        'regression. They do not construct a Carrier or execute SpawnManager AI. '
        'Original caller bytes and Unit vtable bindings separately establish '
        '6B7B43 -> vslot3AC/6F7780 -> vslot3A8/6F77B0 -> 6F78B8/6F7220. '
        'The manager sets mode1 on true and clears targets on false; no additional '
        'distance comparison occurs in this Idle admission caller.',
    ], substitutions=['Inherited input, heap and OS seams only; no range result substitution.'],
        entry_points={'in_range': 0x6F7220})
    native = image_bytes()
    result['caller_instruction_bytes'] = {
        f'{address:08x}': file_span(native, address, length)[1].hex()
        for address, length in ((0x6B7B29, 0x3A), (0x6F7780, 0x22), (0x6F78AF, 0x17))
    }
    result['unit_vtable_bindings'] = {
        f'{offset:03x}': f'{struct.unpack("<I", file_span(native, 0x7F5C70 + offset, 4)[1])[0]:08x}'
        for offset in (0x2E4, 0x3A8, 0x3AC)
    }
    return result


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'range_ties.json', provenance=metadata)
