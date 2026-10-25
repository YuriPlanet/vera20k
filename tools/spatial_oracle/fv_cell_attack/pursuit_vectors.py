"""Project original MissionAttack observations for production Rust comparisons."""
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.shrapnel_repair.packet_io import digest, read_result

HERE = Path(__file__).resolve().parent
SOURCE = HERE / 'pursuit.json.gz'


def generate():
    data = read_result(SOURCE)
    rows = []
    for stage in data['stages']:
        for row in stage['results']:
            rows.append(dict(input=row['row'], source_xyz=row['source_xyz'],
                             before=row['before']['actor'], after=row['after']['actor'],
                             nav_cell=row['after']['nav_cell'],
                             drive_destination=row['after']['drive_destination'],
                             movement_timer=row['after']['movement_timer'],
                             blockage_timer=row['after']['blockage_timer'],
                             rng_before=row['before']['rng'], rng_after=row['after']['rng'],
                             returned=row['returned']))
    return dict(source=SOURCE.name,
                source_sha256=digest(SOURCE.read_bytes()), rows=rows)


def metadata():
    result = provenance(scope=__doc__, assumptions=[
        'Projection only: input coordinates and expected results are copied '
        'from the separately executed original MissionAttack packet; no Rust '
        'values or calculated golden outcomes contribute.'
    ], substitutions=[], entry_points={'projected_original_approach': 0x4D5690})
    result['source_sha256'] = digest(SOURCE.read_bytes())
    return result


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'pursuit_vectors.json', provenance=metadata)
