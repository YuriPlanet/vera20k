"""Lossless Rust-facing projection of the executed FV Cell range observations."""
from pathlib import Path

from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.shrapnel_repair.packet_io import digest, read_result

HERE = Path(__file__).resolve().parent
SOURCE = HERE / 'range.json.gz'


def generate():
    data = read_result(SOURCE)
    rows = []
    for row in data['rows']:
        target = row['input'].get('target', [87, 54])
        result = row['results'][0]
        assert result['entry'] == 'can_fire_at'
        cells = []
        for item in row['projection']['cells']:
            cell = {key: item['facts'][key] for key in
                    ('coord', 'level', 'slope', 'tile', 'flags', 'land')}
            if cell['coord'] == target and 'target_flags' in row['input']:
                cell['flags'] = row['input']['target_flags']
            cells.append(cell)
        rows.append(dict(
            name=row['input']['name'], source_xyz=row['source_xyz'], target=target,
            cells=cells, water_base=row['projection']['water_base'],
            target_geometry=result['target_geometry'][0], in_range=result['returned'],
            distances=result['distances'], fire_error=row['results'][1]['returned'],
        ))
    return dict(source=SOURCE.name,
                source_sha256=digest(SOURCE.read_bytes()), rows=rows)


def metadata():
    result = provenance(scope=__doc__, assumptions=[
        'This projection executes no game code. Every expected outcome is copied '
        'from the separately replayed native range packet; no Rust output or '
        'hand-calculated expected value contributes to the vectors.',
    ], substitutions=[], entry_points={'projected_original_in_range': 0x6F7220})
    result['source_sha256'] = digest(SOURCE.read_bytes())
    return result


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'range_vectors.json', provenance=metadata)
