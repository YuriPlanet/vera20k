"""Project the frozen six native hut cases into uncompressed Rust test inputs.

This reuses recorded original execution, not a second bridge implementation.
The complete native source payload must retain its publication identity.
"""
import json
from pathlib import Path

from tools.native_oracle import _canonical, finish_vectors, provenance
from tools.spatial_oracle.shrapnel_repair import packet_io

HERE = Path(__file__).resolve().parent
CELL_FIELDS = ('coord', 'overlay', 'land', 'level', 'slope', 'zone_type',
               'tile', 'subtile', 'flags', 'state')
ANIMATION_FIELDS = ('type', 'position', 'delay', 'flags', 'loops', 'z_adjust', 'reverse')


def generate():
    source = packet_io.read_result(HERE / 'hut.json.gz')
    identity = json.loads((HERE / 'hut_promotion.json').read_bytes())['results']['hut.json']
    assert packet_io.digest(_canonical(source)) == identity['published_payload_sha256']
    cases = []
    for case in source['cases']:
        result = case['result']
        cells = [{key: cell[key] for key in CELL_FIELDS} for cell in result['after']
                 if 86 <= cell['coord'][0] <= 88 and 51 <= cell['coord'][1] <= 57]
        cells.sort(key=lambda cell: (cell['coord'][1], cell['coord'][0]))
        assert len(cells) == 21
        animations = [{key: event[key] for key in ANIMATION_FIELDS}
                      for event in result['trace'] if event['kind'] == 'animation_request']
        cases.append(dict(hut=case['hut'], starting_stage=case['starting_stage'],
                          result=dict(rng_before=result['rng_before'], rng_after=result['rng_after'],
                                      final_bridge_cells=cells, animation_requests=animations)))
    assert len(cases) == 6
    return dict(schema=1,
                original_native_payload_sha256=identity['original_payload_sha256'],
                published_native_payload_sha256=identity['published_payload_sha256'], cases=cases)


def metadata():
    return provenance(scope=__doc__, assumptions=[
        'Pure projection of hash-pinned hut.json.gz: immediate hut-call before/after complete RNG states, all21 final physical span cells and ordered seven-argument animation requests.',
        'The native execution and explicit constructor/graph/lifecycle boundaries remain those of hut.meta.json. No new native execution or production comparison occurs while generating this projection.'
    ], substitutions=[], entry_points={'hut': 0x574000, 'animation_request': 0x421EA0}) | dict(
        source_file='hut.json.gz', source_file_sha256=packet_io.digest((HERE / 'hut.json.gz').read_bytes()))


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'hut_test_vectors.json', provenance=metadata)
