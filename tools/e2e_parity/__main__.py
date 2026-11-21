"""Build and run the stock-YR production checks and available native comparison."""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import sys
from tempfile import mkdtemp

from tools.e2e_parity.runner import CASES, ROOT, exit_status, retail_directory, run


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    commands.add_parser('list', help='list the available production checks')
    command = commands.add_parser('run', help='build and run the selected checks')
    command.add_argument('--only', action='append', choices=CASES, help='repeat to select multiple cases')
    command.add_argument('--ra2-dir', type=Path, help='retail directory (defaults to the checkout config)')
    command.add_argument('--shell-guard', type=Path, default=os.environ.get('VERA20K_SHELL_GUARD'),
                         help='sealed native main-menu guard')
    command.add_argument('--output', type=Path, help='new output directory under an existing parent')
    command.add_argument('--production-only', action='store_true',
                         help='allow cases without an enrolled native reference')
    args = parser.parse_args(argv)
    if args.command == 'list':
        print('\n'.join(CASES))
        return 0

    try:
        retail = retail_directory(args.ra2_dir)
        output = args.output
        if output is None:
            root = ROOT / 'target' / 'e2e-parity'
            root.mkdir(parents=True, exist_ok=True)
            output = Path(mkdtemp(prefix='run-', dir=root)) / 'results'
        else:
            output = output.absolute()
        cases = [case for case in CASES if args.only is None or case in args.only]
        guard = args.shell_guard.absolute() if args.shell_guard is not None else None
        results = run(cases, output, retail, guard)
    except (OSError, ValueError) as exc:
        print(f'e2e parity: {exc}', file=sys.stderr)
        return 2
    print(f'Report {output / "report.json"}')
    return exit_status(results, production_only=args.production_only)


if __name__ == '__main__':
    raise SystemExit(main())
