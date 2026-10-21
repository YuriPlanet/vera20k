"""Original581F50 hierarchy reconstruction at four Shrapnel wooden-bridge states.

The shared restore owner executes the complete native rebuild. This module
supplies the frozen full-map Shrapnel input factory and stage identities only.
"""
import argparse
import json
import sys
from pathlib import Path

from tools.native_oracle import _canonical
from tools.spatial_oracle import anytown_navigation_restore as shared
from tools.spatial_oracle.shrapnel_repair import packet_io
from . import navigation as nav

HERE = Path(__file__).resolve().parent
FROZEN = HERE / 'navigation.json.gz'
FROZEN_SHA = '5145c4fab77dcdef1d3bdc99c9b056c5afca11495a9c14deaf2573ed7e877674'
FROZEN_PAYLOAD_SHA = '24cc5905dbfaacc31ab11f93bdb1314e8b58b8f463d619b245908b234e95d50c'
OUTPUT = HERE / 'navigation_restore.json.gz'
PROMOTION = HERE / 'navigation_restore_promotion.json'


class RestoreProbe(shared.RestoreProbe, nav.Navigation):
    """Reuse both owners' observers; the native builder/rebuild body is shared."""


def input_factory():
    readers, theater, tiles, assets, inputs = nav.prepare_inputs()

    def machine_factory(stop_after):
        return RestoreProbe(readers, theater, tiles, stop_after,
                            primitive_entries=(0x570050, 0x57BAA0), **inputs)

    return readers, assets, machine_factory


def generate():
    return shared.generate(
        frozen_path=FROZEN, frozen_sha256=FROZEN_SHA,
        frozen_payload_sha256=FROZEN_PAYLOAD_SHA,
        stages=('healthy', 'damaged', 'collapsed', 'repaired'),
        input_factory=input_factory,
    )


def metadata():
    result = shared.metadata()
    result['scope'] = __doc__
    result['assumptions'][1] = (
        'The existing Navigation.run driver executes preparation; the observer stops '
        'after selected original570050 repair or57BAA0 damage returns. Four independent '
        'VMs match the frozen Shrapnel initial/state/trace exactly before581F50.'
    )
    result['rebuild_owner'] = 'tools/spatial_oracle/anytown_navigation_restore.py'
    result['navigation_owner'] = 'tools/spatial_oracle/anytown_damage/navigation.py'
    return result


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--compare-prefix', type=Path)
    parser.add_argument('--comparison-output', type=Path)
    args, remaining = parser.parse_known_args()
    if args.compare_prefix is not None:
        if remaining:
            parser.error('Unexpected comparison arguments: ' + ' '.join(remaining))
        result = shared.compare_restored_prefix(
            args.compare_prefix, native_path=OUTPUT,
            suffixes=('healthy', 'damaged', 'collapsed', 'repaired'),
        )
        text = json.dumps(result, indent=2) + '\n'
        if args.comparison_output:
            args.comparison_output.write_text(text)
        else:
            print(text, end='')
        assert result['all_compared_equal'], 'Native restored navigation mismatch'
        return
    if args.comparison_output:
        parser.error('--comparison-output requires --compare-prefix')
    result = generate()
    if '--write' in remaining and not PROMOTION.exists():
        projected = packet_io.publication_projection(result)
        digest = shared.sha(_canonical(json.loads(_canonical(projected))))
        PROMOTION.write_text(json.dumps(dict(results={OUTPUT.name.removesuffix('.gz'):
            dict(published_payload_sha256=digest)}), indent=2) + '\n')
    packet_io.finish_vectors(result, OUTPUT, provenance=metadata, argv=remaining,
                             promotion_path=PROMOTION)


if __name__ == '__main__':
    main()
