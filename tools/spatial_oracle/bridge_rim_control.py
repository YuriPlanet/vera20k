"""Original edge-loop pointer retention across untagged notifications.

Synthetic control cases complement, but do not broaden, the retail stock
witness. No original object damage or whole-world loader is claimed.
"""
from pathlib import Path
from tools.native_oracle import finish_vectors, provenance
from tools.spatial_oracle.bridge_rim import OriginalRim, GLOBALS, COORD, DUMMY
from tools.spatial_oracle.map_queries import packed


def cases():
    results = []
    for name, rows, start in [
        ('notification_moves_retained_dummy', [
            [100, 100, 0, 0, 0x80, None, 0, None, 0, 0],
            [102, 100, 0, 0, 0x80, None, 0, None, 0, 0],
            [104, 100, 200, 4, 0, None, 0, None, 0, 0],
        ], [100, 100]),
        ('requested_endpoint_before_fixed_stride_alias', [
            [511, 100, 0, 0, 0, None, 0, None, 0, 0],
            [0, 101, 200, 4, 0, None, 0, None, 0, 0],
        ], [511, 100]),
    ]:
        keys = {key: 9000+i*10 for i, key in enumerate(GLOBALS)}
        keys['BridgeBottomRight1'] = 201
        supplied = dict(bridge_base=0, rim_keys=keys, size=[136, 140], cells=rows)
        native = OriginalRim(supplied)
        before = [native.snapshot(p) for p in native.ptrs.values()]
        native.uc.mem_write(COORD, packed(*start))
        result = native.call(0x576200, args=(COORD, 2, 0))
        results.append(dict(name=name, input=supplied, start=start, direction=2,
            result=result, before=before, after=[native.snapshot(p) for p in native.ptrs.values()],
            calls=native.events, writes=native.writes, dummy=native.snapshot(DUMMY)))
    return dict(cases=results)


if __name__ == '__main__':
    finish_vectors(cases, Path(__file__).with_suffix('.json'), provenance=lambda: provenance(
        scope='Original576200, actual untagged575EE0, retained dummy and requested endpoint control cases',
        assumptions=[
            'Synthetic sparse supplied cells; not a stock-map reachability claim',
            'Direct edge entry direction2 with null rectangle; selector and display work excluded',
            'Missing slots share the original dummy initialized with tile=-1, no tags/objects',
        ], substitutions=[
            'Inherited OriginalRim output sinks; neither case enters a setter or object fallout',
        ], entry_points={'edge':0x576200, 'notification':0x575EE0, 'lookup':0x5657A0}))
