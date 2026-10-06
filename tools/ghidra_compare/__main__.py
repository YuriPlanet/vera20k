"""Read-only decompile/caller and native stack-frame comparisons."""
import argparse
import hashlib
import json
from pathlib import Path
import sys

from .client import Client, ReadError, address, callers, spread
from .decompile import compare


def parser():
    root = argparse.ArgumentParser(description=__doc__)
    sub = root.add_subparsers(dest='command', required=True)
    for name, description in (
            ('decompile', 'Compare planned functions and their known callers'),
            ('frames', 'Compare high p-code frames against original x86 bytes')):
        cmd = sub.add_parser(name, help=description)
        for side in ('before', 'after'):
            cmd.add_argument(f'--{side}-url', required=True, help='GhidraMCP http(s) origin, e.g. http://127.0.0.1:8089')
            cmd.add_argument(f'--{side}-program', required=True, help='Explicit saved program selector')
        cmd.add_argument('--timeout', type=float, default=60, help='HTTP seconds per attempt (default 60)')
        cmd.add_argument('--attempts', type=int, default=3, help='HTTP attempts, 1..3 (default 3)')
        cmd.add_argument('--out', type=Path, required=True, help='New JSON report path (will not overwrite)')
        if name == 'decompile':
            cmd.add_argument('--plan', type=Path, required=True, help='JSON rows with addr and action; none/skip are omitted')
            cmd.add_argument('--computed', action='store_true', help='Include resolved computed/virtual call references')
            cmd.add_argument('--max-callers', type=int, help='Evenly sample known callers; default all')
            cmd.add_argument('--expect-warning', action='append', default=[], help='Exact warning text excluded from the verdict, still counted')
        else:
            group = cmd.add_mutually_exclusive_group(required=True)
            group.add_argument('--address', action='append', type=address, help='Function entry (repeatable)')
            group.add_argument('--comparison', type=Path, help='Report from decompile: inspect worse/better plus sampled remaining functions')
            cmd.add_argument('--sample', type=int, default=0, help='Additional evenly spaced functions from --comparison')
            cmd.add_argument('--census', type=Path, required=True, help='Explicit before SignatureCensus JSONL')
            cmd.add_argument('--after-census', type=Path, help='Also inspect every known direct caller of changed purges, through thunks')
            cmd.add_argument('--noreturn', action='append', type=address, default=[], help='Additional instruction-established no-return entry')
    return root


def read_input(path, inputs):
    data = path.read_bytes()
    inputs.append({'path': str(path), 'sha256': hashlib.sha256(data).hexdigest()})
    return data.decode('utf-8')


def execute(args):
    if args.out.exists():
        raise ValueError(f'Report already exists: {args.out}; choose a new path')
    before = Client(args.before_url, args.before_program, args.timeout, args.attempts)
    after = Client(args.after_url, args.after_program, args.timeout, args.attempts)
    inputs = []
    if args.command == 'decompile':
        if args.max_callers is not None and args.max_callers < 0:
            raise ValueError('--max-callers must be nonnegative')
        plan = json.loads(read_input(args.plan, inputs))
        if not isinstance(plan, list):
            raise ValueError('Plan must be a JSON array')
        targets = [address(row['addr']) for row in plan if row['action'] not in ('none', 'skip')]
        if not targets:
            raise ValueError('Plan has no active function rows')
        report = compare(before, after, targets, computed=args.computed,
                         max_callers=args.max_callers, expected=set(args.expect_warning))
    else:
        from tools import native_oracle as native
        from .frames import Image, NativeFrames, census_rows, compare_frames
        if args.sample < 0:
            raise ValueError('--sample must be nonnegative')
        rows = census_rows(read_input(args.census, inputs))
        if args.address:
            targets = args.address
        else:
            previous = json.loads(read_input(args.comparison, inputs))
            if previous.get('kind') != 'decompile' or previous['before'] != before.identity() or previous['after'] != after.identity():
                raise ValueError('Comparison report must identify these exact before/after programs and servers')
            selected = {address(row['addr']) for row in previous['worse'] + previous['better']}
            rest = sorted({address(a) for a in previous['functions']} - selected)
            targets = sorted(selected) + spread(rest, args.sample)
        purge_changed, purge_callers, discovery_errors = [], [], []
        if args.after_census:
            after_rows = census_rows(read_input(args.after_census, inputs))
            cb = {address(row['entry']): row['purge'] for row in rows}
            ca = {address(row['entry']): row['purge'] for row in after_rows}
            if set(cb) != set(ca):
                raise ValueError('Purge comparison requires the same census entries on both sides')
            purge_changed = sorted(entry for entry in cb if cb[entry] != ca[entry])
            try:
                purge_callers = callers(before, purge_changed, include_tail=False)
            except ReadError as error:
                discovery_errors.append({'phase': 'purge_callers', 'error': str(error)})
            targets += purge_callers
        targets = sorted(set(targets))
        if not targets:
            raise ValueError('No frame targets; use --address or a positive --sample')
        try:
            data = native.image_bytes()
            image = Image(data)
        except native.OracleError as error:
            raise ReadError(str(error)) from error
        frames = NativeFrames(rows, image, [int(a, 16) for a in args.noreturn])
        report = compare_frames(before, after, targets, frames)
        report.update(native_binary_sha256=native.image_sha256(data), purge_changed=purge_changed,
                      purge_callers=purge_callers, additional_noreturn=args.noreturn)
        if discovery_errors:
            report['read_errors'] += discovery_errors
            report['complete'], report['status'] = False, 'incomplete'
    report.update(schema_version=1, inputs=inputs,
                  limits='Read-only comparison evidence; not automatic semantic approval or whole-program coverage.')
    # Exclusive creation preserves prior receipts and prevents output/input aliasing.
    with args.out.open('x', encoding='utf-8') as output:
        json.dump(report, output, indent=2)
        output.write('\n')
    print(f"{report['status']}: {report['attempted']} functions; {len(report['read_errors'])} read errors; {args.out}")
    return {'ok': 0, 'findings': 1, 'incomplete': 2}[report['status']]


def main(argv=None):
    args = parser().parse_args(argv)
    try:
        return execute(args)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f'Ghidra comparison failed: {error}', file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
