"""Compare all five live Shrapnel phases through the shared graph comparator."""
import argparse
import json
from pathlib import Path

from tools.spatial_oracle.anytown_damage.navigation_compare import compare

STAGES = [('loaded', None), ('healthy', 0), ('damaged', 1), ('collapsed', 2), ('repaired', 3)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('prefix', type=Path)
    parser.add_argument('--native', type=Path, default=Path(__file__).with_name('navigation.json.gz'))
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    result = compare(args.native, args.prefix, stage_indices=STAGES)
    text = json.dumps(result, indent=2) + '\n'
    if args.output:
        args.output.write_text(text)
    else:
        print(text, end='')
    assert result['all_compared_equal'], 'Native live navigation mismatch; inspect receipt'


if __name__ == '__main__':
    main()
