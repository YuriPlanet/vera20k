"""Mechanical Rust projection of both original occupied Shrapnel hut calls."""
from pathlib import Path
import json

from tools.native_oracle import _canonical, finish_vectors, provenance
from tools.spatial_oracle.shrapnel_repair import packet_io
from .occupants_joined_test_vectors import project_joined_cases

HERE = Path(__file__).resolve().parent


def generate():
    source = packet_io.read_result(HERE / 'hut_joined.json.gz')
    promotion = json.loads((HERE / 'hut_joined_promotion.json').read_bytes())
    identity = packet_io.digest(_canonical(source))
    assert identity == promotion['results']['hut_joined.json']['published_payload_sha256']
    result = project_joined_cases(source, ('hut_call', 'hut_call_return'))
    for case, native in zip(result['cases'], source['cases'], strict=True):
        stage, = case['stages']
        raw, = native['result']
        for key in ('span_before', 'display_dirty_before', 'display_dirty_after'):
            stage[key] = raw[key]
        case['bridge_explosion_layers'] = native['art_input']['bridge_explosion_layers']
    return dict(schema=1,
                joined_native_payload_sha256=identity, scene=source['scene'],
                native_actor_inputs=source['cases'][0]['native_inputs'], **result)


def metadata():
    return provenance(scope=__doc__, assumptions=[
        'The unchanged shared joined-occupants projector retains native receiver packets, timer stores, ordered callbacks, constructor requests/state, relative identity order, memberships and complete three-stream RNG states.',
        'The only additions are observed hut-entry/walker/driver calls, before-span cells and Display dirty bytes. No native result is recalculated. All scene, physical ART and outer-lifecycle coverage limits remain in hut_joined.meta.json.',
    ], substitutions=[], entry_points=dict(hut=0x574C20, walker=0x575540,
        driver=0x57BAA0, occupants=0x487A10, constructor=0x421EA0)) | dict(
        source_files={name: packet_io.digest((HERE / name).read_bytes()) for name in (
            'hut_joined.json.gz', 'hut_joined_promotion.json')})


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'hut_joined_test_vectors.json', provenance=metadata)
