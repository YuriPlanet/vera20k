"""Bind the navigation addition to its independently frozen native payload."""
import json
from pathlib import Path

from tools.native_oracle import OracleError, _canonical
from tools.spatial_oracle.shrapnel_repair import packet_io
from .publication import publication_projection

HERE = Path(__file__).resolve().parent


def finish_vectors(data, default_path, *, provenance, argv=None):
    actual = data() if callable(data) else data
    raw = _canonical(json.loads(_canonical(actual)))
    promotion_path = HERE / 'navigation_promotion.json'
    identity = default_path.name.removesuffix('.gz')
    expected = json.loads(promotion_path.read_bytes())['results'][identity]
    if packet_io.digest(raw) != expected['original_payload_sha256']:
        raise OracleError(f'Original frozen navigation payload changed: {identity}')
    packet_io.finish_vectors(actual, default_path, provenance=provenance, argv=argv,
                             promotion_path=promotion_path, projection=publication_projection)
