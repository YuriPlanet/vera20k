"""Run the ignored library tests that need only the retail install, failing on missing inputs.

Library tests that need the retail game folder, that take minutes, or that need
a GPU or another device are `#[ignore]`d, so no ordinary run reaches them. This
is the routine run of those the retail install alone satisfies: the game folder
in `RA2_DIR` and the extracted INIs in `ini/`. It checks both before building,
selects the tests from their ignore reasons, and runs them through
tools.cargo_run (`--lib`) with VERA20K_REQUIRE_RETAIL_INI and
VERA20K_REQUIRE_RETAIL_ASSETS set, so a fixture that cannot find its input
fails instead of printing SKIPPED and passing. A selected test that reports no
result counts as a failure.

    python -m tools.retail_tests list [--all]
    python -m tools.retail_tests run [--filter TEXT ...] [-- LIBTEST ARGS]

`--filter` keeps the selected tests whose path contains any TEXT (a batch, such
as `sim::world::`); arguments after `--` go to the test binaries, such as
`--test-threads=4`. Exit status: 0 when every selected test passed, 1 when one
failed or reported no result, 2 when an input, the source scan, the selection or a
build is unusable. See tools/README.md.
"""
from __future__ import annotations

import argparse
from collections import defaultdict
import os
from pathlib import Path
import re
import sys

from tools.cargo_run import invoke
from tools.prepare_checkout import INI, game_folder_problem

ROOT = Path(__file__).resolve().parents[1]
PACKAGES = ('vera20k-core', 'vera20k')
REQUIRE_ENV = {'VERA20K_REQUIRE_RETAIL_INI': '1', 'VERA20K_REQUIRE_RETAIL_ASSETS': '1'}
# Windows limits a command line to 32,767 characters; each batch of exact
# names stays well under it.
BATCH_CHARACTERS = 12_000

# The first class whose pattern the ignore reason matches; `run` is the routine
# set. An ignore reason must therefore name every input beyond RA2_DIR and ini/.
EXCLUDED = (
    ('gpu', 'needs a GPU adapter', re.compile(r'\bgpu\b|wgpu|adapter|offscreen|readback', re.I)),
    ('device', 'needs a display or audio output device',
     re.compile(r'x11|audio output|requires an audio (?:output )?device', re.I)),
    ('input', 'needs another input (a VERA20K_* variable or the recorded retail corpus)',
     re.compile(r'\bVERA(?:20K)?_[A-Z0-9_]+|retail corpus', re.I)),
    ('manual', 'manual timing or local export, not a regression check',
     re.compile(r'\bmanual\b|timing;|census|writes a local|benchmark', re.I)),
)
RUN = re.compile(r'^(?:slow;|physical\b)|^(?:requires|needs)\b.*(?:retail|RA2_DIR|install|physical|stock|archives)',
                 re.I)
CLASSES = {name: description for name, description, _ in EXCLUDED}
CLASSES['run'] = 'selected: needs only RA2_DIR and ini/ (or is slow)'
CLASSES['other'] = 'ignored for another reason, such as a recorded failure'

IGNORE = re.compile(r'^\s*#\[ignore\s*=\s*"((?:[^"\\]|\\.)*)"\s*\]')
FN = re.compile(r'^\s*(?:pub(?:\([^)]*\))?\s+)?fn\s+([A-Za-z0-9_]+)')
RESULT = re.compile(r'^test (\S+) \.\.\. (ok|FAILED|ignored)\b')


def classify(reason: str) -> str:
    for name, _, pattern in EXCLUDED:
        if pattern.search(reason):
            return name
    return 'run' if RUN.search(reason) else 'other'


def scan(src: Path) -> dict[str, list[tuple[str, str]]]:
    """Each `#[ignore = "..."]` test function name: its (file, reason) pairs."""
    found = defaultdict(list)
    for path in sorted(src.rglob('*.rs')):
        lines = path.read_text(encoding='utf-8', errors='replace').splitlines()
        for i, line in enumerate(lines):
            if match := IGNORE.match(line):
                function = next((fn[1] for later in lines[i + 1:i + 12] if (fn := FN.match(later))), None)
                if function is None:
                    raise ValueError(f'{path}:{i + 1}: no fn follows the ignore attribute')
                found[function].append((path.relative_to(src.parent).as_posix(), match[1]))
    return found


def test_class(name: str, inventory: dict[str, list[tuple[str, str]]]) -> str:
    """The class of the listed test NAME, by its function's ignore reason."""
    entries = inventory.get(name.rsplit('::', 1)[-1])
    if not entries:
        raise ValueError(f'{name} is ignored, but no `#[ignore = "reason"]` in src/ names it')
    classes = {classify(reason) for _, reason in entries}
    if len(classes) > 1:
        raise ValueError(f'{name}: ignored functions of that name in {[file for file, _ in entries]} '
                         'classify differently; rename one')
    return classes.pop()


def prerequisite_problems(root: Path, environ) -> list[str]:
    problems = []
    folder = environ.get('RA2_DIR')
    if not folder:
        problems.append("RA2_DIR is unset or empty; set it to the Yuri's Revenge game folder")
    elif problem := game_folder_problem(Path(folder), for_tests=True):
        problems.append(f'RA2_DIR: {problem}')
    if not (root / INI / 'rulesmd.ini').is_file():
        problems.append(f'{root / INI} has no rulesmd.ini; run python -m tools.prepare_checkout')
    return problems


def cargo_test(arguments: list[str], on_line) -> int:
    packages = [option for package in PACKAGES for option in ('-p', package)]
    return invoke(['test', *packages, '--lib', '--', '--ignored', *arguments], cwd=ROOT,
                  env_extra=REQUIRE_ENV, on_line=on_line)


def listed_ignored() -> list[str]:
    names, output = [], []

    def collect(line):
        output.append(line)
        if line.endswith(': test\n'):
            names.append(line.removesuffix(': test\n'))

    if status := cargo_test(['--list'], collect):
        sys.stdout.writelines(output)
        raise ValueError(f'listing the ignored tests failed with status {status}')
    return names


def batches(names: list[str]) -> list[list[str]]:
    result, size = [[]], 0
    for name in names:
        if result[-1] and size + len(name) + 1 > BATCH_CHARACTERS:
            result.append([])
            size = 0
        result[-1].append(name)
        size += len(name) + 1
    return result


def run(filters: list[str], extra: list[str]) -> int:
    if problems := prerequisite_problems(ROOT, os.environ):
        for problem in problems:
            print(f'retail_tests: {problem}', file=sys.stderr)
        return 2
    inventory = scan(ROOT / 'src')
    selected = [name for name in listed_ignored() if test_class(name, inventory) == 'run'
                and (not filters or any(text in name for text in filters))]
    if not selected:
        raise ValueError('the selection is empty; check --filter')
    results = {}

    def show(line):
        sys.stdout.write(line)
        sys.stdout.flush()
        if match := RESULT.match(line):
            results[match[1]] = match[2]

    groups = batches(selected)
    unrun = 0
    for number, group in enumerate(groups, 1):
        print(f'retail_tests: batch {number} of {len(groups)}, {len(group)} tests', flush=True)
        reported = len(results)
        if (status := cargo_test(['--exact', *group, *extra], show)) and len(results) == reported:
            print(f'retail_tests: batch {number} ran no test (status {status})', file=sys.stderr)
            unrun = status
    failed = [name for name in selected if results.get(name) == 'FAILED']
    missing = [name for name in selected if results.get(name) not in ('ok', 'FAILED')]
    print(f'retail_tests: {len(selected)} selected, {sum(results.get(n) == "ok" for n in selected)} passed, '
          f'{len(failed)} failed, {len(missing)} reported no result')
    for name in failed:
        print(f'FAILED     {name}')
    for name in missing:
        print(f'NO RESULT  {name}')
    return 2 if unrun else 1 if failed or missing else 0


def list_classes(show_all: bool) -> int:
    by_class = defaultdict(list)
    for function, entries in scan(ROOT / 'src').items():
        for file, reason in entries:
            by_class[classify(reason)].append((file, function, reason))
    for name, description in CLASSES.items():
        tests = sorted(by_class[name])
        print(f'{name:7} {len(tests):4}  {description}')
        if show_all or name == 'run':
            for file, function, reason in tests:
                print(f'          {file} {function}  [{reason}]')
    print('Counts are ignored functions in src/; `run` takes those this platform compiles.')
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    commands = parser.add_subparsers(dest='command', required=True)
    listing = commands.add_parser('list', help='classify every ignored test by its reason; no build')
    listing.add_argument('--all', action='store_true', help='also name the tests of the excluded classes')
    running = commands.add_parser('run', help='check the inputs, then build and run the selected tests')
    running.add_argument('--filter', action='append', default=[], metavar='TEXT',
                         help='keep the selected tests whose path contains TEXT; repeatable')
    args, extra = parser.parse_known_args(argv)
    extra = [value for value in extra if value != '--']
    if args.command == 'list':
        if extra:
            parser.error('list takes no test arguments')
        return list_classes(args.all)
    try:
        return run(args.filter, extra)
    except (OSError, ValueError) as error:
        print(f'retail_tests: {error}', file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
