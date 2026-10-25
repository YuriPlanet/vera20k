"""Project compact Rust inputs/outputs from the frozen native scalar packet.

This is extraction only. It neither executes gamemd nor computes expected
behavior. The complete raw cells, writes and native provenance remain in scalar.
"""
import gzip
import hashlib
import json
from pathlib import Path

from tools.native_oracle import finish_vectors

HERE = Path(__file__).resolve().parent
SOURCE = HERE / 'scalar.json.gz'


def result(row):
    return {key: row[key] for key in ('returned', 'trace', 'final', 'dummy')}


def generate():
    source = json.loads(gzip.decompress(SOURCE.read_bytes()))
    return dict(
        schema_version=1,
        cases=[dict(input={key: row['input'][key] for key in ('name', 'cells', 'start')},
                    result=result(row['result'])) for row in source['cases']],
        physical_sequences=[dict(input=row['input'], steps=[result(step) for step in row['steps']])
                            for row in source['physical_sequences']],
    )


def metadata():
    source_meta = json.loads((HERE / 'scalar.meta.json').read_bytes())
    return dict(
        schema_version=1,
        scope=__doc__,
        source='tools/spatial_oracle/shrapnel_damage/scalar.json.gz',
        source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
        source_payload_sha256=source_meta['payload_sha256'],
        transformation='Select original input fields and returned/trace/final/dummy; no derived expectations',
    )


if __name__ == '__main__':
    finish_vectors(generate, HERE / 'scalar_test_vectors.json', provenance=metadata)
