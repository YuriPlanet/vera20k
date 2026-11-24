"""Publish native Cell-attack evidence through the shared compressed owner.

Promotion preserves executed values, traces and input identities. Only lexical
INI copies and checkout-dependent reference paths receive a portable projection.
The shared publisher refuses changed native payloads even with --write.
"""
import copy
from pathlib import Path

from tools.native_oracle import _canonical
from tools.spatial_oracle.shrapnel_repair import packet_io

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]


def publication_projection(data):
    result = copy.deepcopy(data)

    def visit(value):
        if isinstance(value, dict):
            for key in tuple(value):
                item = value[key]
                if key in ('sections', 'source_lines', 'art_lines'):
                    value[key + '_sha256'] = packet_io.digest(_canonical(value.pop(key)))
                elif key == 'file' and isinstance(item, str) and Path(item).is_absolute():
                    path = Path(item)
                    if path.is_relative_to(REPO):
                        value[key] = path.relative_to(REPO).as_posix()
                else:
                    visit(item)
        elif isinstance(value, list):
            for item in value:
                visit(item)

    visit(result)
    return result


def finish_vectors(data, default_path, *, provenance, argv=None):
    packet_io.finish_vectors(
        data, default_path, provenance=provenance, argv=argv,
        promotion_path=HERE / 'promotion.json', projection=publication_projection,
    )
