"""Publish the frozen packet through the shared compressed-reference owner."""
import json
from pathlib import Path

from tools.native_oracle import OracleError, _canonical
from tools.spatial_oracle.shrapnel_repair import packet_io

HERE = Path(__file__).resolve().parent


def publication_projection(data):
    result = packet_io.publication_projection(data)

    def lexical_fields(record, names=('sections', 'source_lines')):
        for name in names:
            if name in record:
                record[name + '_sha256'] = packet_io.digest(_canonical(record.pop(name)))

    for layer in result.get('combat_inputs', {}).get('layers', []):
        lexical_fields(layer)
    for layer in result.get('base_reader_layers', []):
        lexical_fields(layer)
    for control in result.get('jumpjet', []):
        for layer in control.get('layers', []):
            lexical_fields(layer)
    lexical_fields(result.get('sound', {}), ('selected',))
    inputs = result.get('inputs', {})
    for layer in inputs.get('layers', []):
        lexical_fields(layer)
    lexical_fields(inputs, ('art_lines',))
    lexical_fields(inputs.get('sound', {}), ('selected',))
    lexical_fields(inputs.get('special_flags', {}), ('map_sections',))
    return result


def finish_vectors(data, default_path, *, provenance, argv=None):
    actual = data() if callable(data) else data
    raw = _canonical(json.loads(_canonical(actual)))
    promotion_path = HERE / 'promotion.json'
    identity = default_path.name.removesuffix('.gz')
    expected = json.loads(promotion_path.read_bytes())['results'][identity]
    if packet_io.digest(raw) != expected['original_payload_sha256']:
        raise OracleError(f'Original frozen native payload changed: {identity}')
    packet_io.finish_vectors(actual, default_path, provenance=provenance, argv=argv,
                             promotion_path=promotion_path, projection=publication_projection)
