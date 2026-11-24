"""Bind mission packaging to the independently frozen native execution."""
import json
from pathlib import Path

from tools.native_oracle import OracleError, _canonical
from tools.spatial_oracle.shrapnel_repair import packet_io
from .publication import publication_projection as shared_projection

HERE = Path(__file__).resolve().parent


def publication_projection(data):
    result = shared_projection(data)
    inputs = result.get('inputs', {})
    records = list(inputs.get('mission_layers', []))
    records.extend(inputs.get('country_layers', []))
    records.append(inputs.get('country_side_lists', {}))
    for record in records:
        for key in ('sections', 'source_lines'):
            if key in record:
                record[key + '_sha256'] = packet_io.digest(_canonical(record.pop(key)))
    return result


def finish_vectors(data, default_path, *, provenance, argv=None):
    actual = data() if callable(data) else data
    promotion_path = HERE / 'mission_promotion.json'
    identity = default_path.name.removesuffix('.gz')
    expected = json.loads(promotion_path.read_bytes())['results'][identity]
    if packet_io.digest(_canonical(json.loads(_canonical(actual)))) != expected['original_payload_sha256']:
        raise OracleError(f'Original frozen mission payload changed: {identity}')
    packet_io.finish_vectors(actual, default_path, provenance=provenance, argv=argv,
                             promotion_path=promotion_path, projection=publication_projection)
