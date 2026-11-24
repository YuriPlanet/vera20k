"""Compressed native references with the shared oracle comparison contract.

Only copied lexical INI inputs are replaced by hashes. Native stores, traces,
states, graph records and numeric reader outputs remain intact.
"""
import argparse
import copy
import gzip
import hashlib
import io
import json
from pathlib import Path

from tools import oracle_sources
from tools.native_oracle import OracleError, _canonical, first_difference

HERE = Path(__file__).resolve().parent


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def publication_projection(data):
    result = copy.deepcopy(data)
    for layer in result.get('native_inputs', {}).get('layers', []):
        for key in ('sections', 'source_lines'):
            if key in layer:
                layer[key + '_sha256'] = digest(_canonical(layer.pop(key)))
    for tileset in result.get('theater', {}).get('sets', []):
        if 'physical' in tileset:
            tileset['physical_sha256'] = digest(_canonical(tileset.pop('physical')))
    return result


def compressed(raw):
    # GzipFile avoids embedding a filename or a host-dependent gzip OS byte.
    buffer = io.BytesIO()
    with gzip.GzipFile(fileobj=buffer, mode='wb', filename='', mtime=0) as stream:
        stream.write(raw)
    return buffer.getvalue()


def read_result(path):
    return json.loads(gzip.decompress(path.read_bytes()))


def finish_vectors(data, default_path: Path, *, provenance, argv=None,
                   promotion_path=None, projection=publication_projection):
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--check', action='store_true', help='compare only (default)')
    mode.add_argument('--write', action='store_true', help='write the frozen reference again')
    parser.add_argument('--output', type=Path, default=default_path)
    args = parser.parse_args(argv)
    actual = data() if callable(data) else data
    # Normalize integer address keys and tuples exactly as finish_vectors does,
    # then canonicalize the normalized JSON object for a stable payload digest.
    raw = _canonical(json.loads(_canonical(projection(actual))))
    promotion_path = promotion_path or HERE / 'promotion.json'
    expected_conversion = json.loads(promotion_path.read_bytes())['results']
    identity = default_path.name.removesuffix('.gz')
    # Portability work must not silently accept new native values under --write.
    if digest(raw) != expected_conversion[identity]['published_payload_sha256']:
        difference = first_difference(read_result(default_path), json.loads(raw))
        raise OracleError(f'Frozen native publication projection changed: {identity}: {difference}')
    supplied = provenance() if callable(provenance) else provenance
    metadata = dict(supplied, payload_sha256=digest(raw),
                    packaging='canonical JSON; gzip mtime=0; copied lexical inputs replaced by SHA256')
    target = args.output
    sidecar = target.with_suffix('').with_suffix('.meta.json')
    if args.write:
        target.write_bytes(compressed(raw))
        sidecar.write_text(json.dumps(metadata, indent=2) + '\n', encoding='utf-8')
        print(f'WROTE {target.name} and {sidecar.name}; native values match frozen packet')
        return
    if difference := first_difference(read_result(target), json.loads(raw)):
        raise OracleError(f'Reference mismatch in {target.name}: {difference}')
    if difference := first_difference(oracle_sources.evidence(json.loads(sidecar.read_bytes())),
                                       oracle_sources.evidence(metadata)):
        raise OracleError(f'Provenance mismatch in {sidecar.name}: {difference}')
    print(f'PASS {target.name}: native outputs match; no files written')
