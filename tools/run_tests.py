"""Run every repository-owned Python test, including namespace and skill folders.

Python >= 3.12; install tools/requirements-test.txt. See tools/README.md.
No retail executable, GPU or game install is needed for the default suite.
Where the locale encoding is not UTF-8 (Windows), it reruns itself in UTF-8 mode.
"""
from __future__ import annotations

import argparse
import codecs
import importlib.util
import locale
import os
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]


def test_files(root: Path) -> list[Path]:
    """Discover source tests, never generated .claude skill mirrors or caches."""
    return sorted(path for base in (root / 'tools', root / '.agents' / 'skills')
                  for path in base.rglob('test_*.py')
                  if '__pycache__' not in path.parts and 'ghidra-up' not in path.parts)


def load_suite(root: Path, paths: list[Path]) -> tuple[unittest.TestSuite, list[tuple[str, int]]]:
    if not paths:
        raise ValueError('No Python test modules found')
    suite = unittest.TestSuite()
    inventory = []
    for index, path in enumerate(paths):
        relative = path.relative_to(root)
        name = ('.'.join(relative.with_suffix('').parts) if relative.parts[0] == 'tools'
                else f'_repo_skill_test_{index}')
        spec = importlib.util.spec_from_file_location(name, path)
        if spec is None or spec.loader is None:
            raise ImportError(f'Cannot load test file {relative}')
        module = importlib.util.module_from_spec(spec)
        sys.modules[name] = module
        spec.loader.exec_module(module)
        tests = unittest.defaultTestLoader.loadTestsFromModule(module)
        count = tests.countTestCases()
        if not count:
            raise ValueError(f'Test module contains no unittest tests: {relative}')
        inventory.append((relative.as_posix(), count))
        suite.addTests(tests)
    return suite, inventory


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--list', action='store_true', help='Import and enumerate all tests without executing')
    parser.add_argument('--retail', action='store_true', help='Also require optional local evidence tests; see index')
    args = parser.parse_args(argv)
    if sys.version_info < (3, 12):
        parser.error('Python 3.12 or newer is required (path junction checks)')
    # Explicit suite choice, independent of inherited machine configuration.
    os.environ['VERA20K_TEST_RETAIL'] = '1' if args.retail else '0'
    try:
        suite, inventory = load_suite(ROOT, test_files(ROOT))
    except (ImportError, OSError, ValueError) as error:
        print(f'Python test discovery failed: {error}', file=sys.stderr)
        return 2
    for path, count in inventory:
        print(f'{count:3}  {path}')
    print(f'{len(inventory)} modules; {suite.countTestCases()} tests', flush=True)
    if args.list:
        return 0
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


def needs_utf8_mode() -> bool:
    """Tool sources and evidence are UTF-8; without UTF-8 mode, Windows reads
    text opened without an encoding with its ANSI code page."""
    return (not sys.flags.utf8_mode
            and codecs.lookup(locale.getpreferredencoding(False)).name != 'utf-8')


if __name__ == '__main__':
    if needs_utf8_mode():
        raise SystemExit(subprocess.call(
            [sys.executable, '-X', 'utf8', '-m', 'tools.run_tests', *sys.argv[1:]], cwd=ROOT))
    raise SystemExit(main())
