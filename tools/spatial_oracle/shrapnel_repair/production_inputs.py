"""Small frozen projection of the recorded production scenario, not native truth.

The reference retains fields consumed by this packet. Full original capture hashes
remain separate so projection hashes cannot masquerade as whole-capture identity.
"""
from functools import lru_cache
import gzip
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CELL_FIELDS = ('coord', 'tile', 'subtile', 'level', 'slope', 'land', 'zone_type', 'bridge')
NAVIGATION_FIELDS = (
    'width', 'height', 'classes', 'levels', 'live_cell_allocated',
    'live_cell_levels', 'live_cell_slopes', 'records',
    'records_match_bridge_authority', 'rust', 'graphs', 'dummy',
)


def project_capture(capture):
    """Exact field selection; no values are recomputed from native outputs."""
    return {
        **{key: capture[key] for key in ('native_size', 'local_size', 'rng')},
        'cells': [{key: row[key] for key in CELL_FIELDS} for row in capture['cells']],
        'navigation': {key: capture['navigation'][key] for key in NAVIGATION_FIELDS},
    }


@lru_cache(maxsize=1)
def frozen_inputs():
    return json.loads(gzip.decompress((HERE / 'production_inputs.json.gz').read_bytes()))


def production(phase):
    return frozen_inputs()['captures'][phase]


def source_sha256(phase):
    return frozen_inputs()['original_capture_sha256'][phase]
