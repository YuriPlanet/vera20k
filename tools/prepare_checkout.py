"""Give a checkout the machine-local files Git ignores, and report what is missing.

Git never carries `config.toml` (the game folder) or `ini/` (the retail INIs that
tests and native oracles read), so a fresh clone or worktree starts without them.

- In a linked worktree, this links `ini` to the main checkout's and copies its
  `config.toml`.
- In a checkout without `config.toml`, `--ra2-dir` writes one from
  `config.toml.example`.
- It then reports what the game, retail tests and native oracles need, and how to
  fix what is missing. Retail tests and oracles read the game folder from the
  `RA2_DIR` environment variable, not from `config.toml`.

    python -m tools.prepare_checkout [--ra2-dir GAME_FOLDER]
    python -m tools.prepare_checkout --check      # report only, change nothing

It never replaces an existing file or link. It exits 1 when `config.toml`,
`RA2_DIR` or `ini/` is not ready; the Python and native-oracle lines are
informational.
"""

from __future__ import annotations

import argparse
import importlib.metadata
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

if sys.version_info < (3, 12):
    # Checked before importing tomllib, so an older Python gets this message.
    sys.exit(f'Python 3.12 or newer is required; {sys.executable} is {sys.version.split()[0]}')

import tomllib

if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

REPO = Path(__file__).resolve().parents[1]
CONFIG = 'config.toml'
INI = 'ini'
# Yuri's Revenge's own archive; its presence identifies the game folder.
GAME_ARCHIVE = 'ra2md.mix'
# Retail tests compare VERA20k's math tables with it.
GAME_EXECUTABLE = 'gamemd.exe'
EXTRACT_INI = 'python -m tools.cargo_run -- build --bin extract-ini'
RESOLVE_EXTRACT_INI = 'python -m tools.cargo_run --resolve extract-ini --profile debug'
# The pinned packages of tool tests and native oracles; sidecars record the Unicorn version.
REQUIREMENTS = REPO / 'tools' / 'requirements-test.txt'
RA2_DIR_LINE = re.compile(r'(?m)^ra2_dir\s*=.*$')


def main_checkout(root: Path) -> Path:
    """The main worktree of ROOT's repository; ROOT itself outside Git."""
    try:
        listing = subprocess.run(['git', 'worktree', 'list', '--porcelain'], cwd=root,
                                 check=True, capture_output=True, text=True).stdout
    except (OSError, subprocess.CalledProcessError):
        return root
    first = next((line for line in listing.splitlines() if line.startswith('worktree ')), None)
    return Path(first.removeprefix('worktree ')).resolve() if first else root


def game_folder_problem(folder: Path, for_tests: bool = False) -> str | None:
    if not folder.is_dir():
        return f'{folder} is not a folder'
    if not any(entry.name.lower() == GAME_ARCHIVE for entry in folder.iterdir()):
        return f"{folder} has no {GAME_ARCHIVE}; it must be the Yuri's Revenge game folder"
    if for_tests and not (folder / GAME_EXECUTABLE).is_file():
        return f'{folder} has no {GAME_EXECUTABLE}; retail tests read it'
    return None


def configured_ra2_dir(config: Path) -> tuple[str | None, str | None]:
    """ra2_dir from CONFIG, or why it could not be read."""
    try:
        value = tomllib.loads(config.read_text(encoding='utf-8')).get('paths', {}).get('ra2_dir')
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        return None, f'cannot read {config.name}: {error}'
    if not isinstance(value, str) or not value.strip():
        return None, f'{config.name} sets no [paths] ra2_dir'
    return value, None


def with_ra2_dir(text: str, folder: Path, source: str) -> str:
    """TEXT with its ra2_dir line set to FOLDER, in forward slashes as the README asks."""
    value = json.dumps(folder.resolve().as_posix())
    text, count = RA2_DIR_LINE.subn(lambda _: f'ra2_dir = {value}', text, count=1)
    if count != 1:
        raise SystemExit(f'{source} has no ra2_dir line to set')
    return text


def write_config(root: Path, ra2_dir: Path) -> str:
    """config.toml from the example, for the game folder RA2_DIR."""
    if problem := game_folder_problem(ra2_dir):
        raise SystemExit(f'Not writing {CONFIG}: {problem}')
    example = root / 'config.toml.example'
    text = with_ra2_dir(example.read_text(encoding='utf-8'), ra2_dir, example.name)
    (root / CONFIG).write_text(text, encoding='utf-8')
    return f'wrote {CONFIG} with ra2_dir {ra2_dir.resolve().as_posix()}'


def copy_config(main: Path, root: Path) -> str:
    source = main / CONFIG
    value, _ = configured_ra2_dir(source)
    if value is None or Path(value).is_absolute():
        shutil.copyfile(source, root / CONFIG)
        return f'copied {CONFIG} from {main}'
    # GameConfig::parse_from starts a relative ra2_dir at the config's folder; keep main's.
    (root / CONFIG).write_text(with_ra2_dir(source.read_text(encoding='utf-8'), main / value,
                                            str(source)), encoding='utf-8')
    return f'copied {CONFIG} from {main}, with its relative ra2_dir made absolute'


def prepare(root: Path, main: Path, ra2_dir: Path | None) -> list[str]:
    """Create what is missing and can be created; never replace anything."""
    done = []
    if os.path.lexists(root / CONFIG):
        if ra2_dir is not None:
            done.append(f'kept the existing {CONFIG}; edit its ra2_dir to change the game folder')
    elif ra2_dir is not None:
        done.append(write_config(root, ra2_dir))
    elif root != main and (main / CONFIG).is_file():
        done.append(copy_config(main, root))
    if root != main and not os.path.lexists(root / INI) and (main / INI / 'rulesmd.ini').is_file():
        try:
            os.symlink(main / INI, root / INI, target_is_directory=True)
            done.append(f'linked {INI} to {main / INI}')
        except OSError:
            # Windows without symlink rights: a copy serves the same readers.
            shutil.copytree(main / INI, root / INI)
            done.append(f'copied {INI}/ from {main}')
    return done


def requirements() -> list[tuple[str, str, str | None]]:
    """(package, pinned version, installed version or None) for each pin."""
    result = []
    for line in REQUIREMENTS.read_text(encoding='utf-8').splitlines():
        name, _, pinned = line.split('#', 1)[0].strip().partition('==')
        if name and pinned:
            try:
                result.append((name, pinned, importlib.metadata.version(name)))
            except importlib.metadata.PackageNotFoundError:
                result.append((name, pinned, None))
    return result


def install_command() -> str:
    """How to install the pins: into this virtual environment, or into a new one,
    since system Pythons may refuse pip (PEP 668)."""
    python = f'"{sys.executable}"' if ' ' in sys.executable else sys.executable
    pins = REQUIREMENTS.relative_to(REPO).as_posix()
    if sys.prefix != sys.base_prefix:
        return f'{python} -m pip install -r {pins}'
    venv_python = 'Scripts/python.exe' if os.name == 'nt' else 'bin/python'
    return (f'{python} -m venv <venv>, then <venv>/{venv_python} -m pip install -r {pins}, '
            f'and run the tools with that Python')


def report(root: Path, main: Path) -> tuple[list[str], bool]:
    """Status lines, and whether config.toml, RA2_DIR and ini/ are ready."""
    lines = [f'checkout  {root}' + (f' (worktree of {main})' if root != main else '')]

    def line(ok, what, detail, fix=None):
        lines.append(f'{"ok" if ok else "problem":8}  {what}: {detail}')
        if fix and not ok:
            lines.append(f'          fix: {fix}')
        return ok

    config = root / CONFIG
    folder = None
    if not os.path.lexists(config):
        game = line(False, CONFIG, 'absent; the game reads its folder from it',
                    "python -m tools.prepare_checkout --ra2-dir <Yuri's Revenge folder>")
    else:
        value, problem = configured_ra2_dir(config)
        if value is not None:
            # Like GameConfig::parse_from, a relative ra2_dir starts at the config's folder.
            folder = (config.parent / value).resolve()
            problem = game_folder_problem(folder)
        game = line(problem is None, CONFIG, problem or f'ra2_dir {value}',
                    f'set [paths] ra2_dir in {config} to the game folder, with forward slashes')

    # Retail tests (VERA20K_REQUIRE_RETAIL_INI, archive fixtures) and native oracles
    # read RA2_DIR and never config.toml.
    retail_dir = os.environ.get('RA2_DIR')
    if retail_dir is None:
        target = f'"{folder}"' if game else 'the game folder'
        retail = line(False, 'RA2_DIR', 'unset; retail tests and native oracles read the game folder '
                      'from it, not from config.toml', f'set RA2_DIR to {target} in the shell that runs them')
    else:
        problem = game_folder_problem(Path(retail_dir), for_tests=True) if retail_dir else 'set but empty'
        retail = line(problem is None, 'RA2_DIR', problem or retail_dir, 'point RA2_DIR at the game folder')

    ini = root / INI
    if (ini / 'rulesmd.ini').is_file():
        where = f' (linked to {os.readlink(ini)})' if ini.is_symlink() else ''
        inis = line(True, f'{INI}/', f'{len(list(ini.glob("*.ini")))} retail INIs{where}')
    else:
        if root == main or not (main / INI / 'rulesmd.ini').is_file():
            target = f'"{folder}"' if game else 'the Yuri\'s Revenge game folder'
            fix = (f'{EXTRACT_INI}\n'
                   f'          resolve: {RESOLVE_EXTRACT_INI}\n'
                   f'          run the returned executable with {target} as its argument; '
                   'quote paths, and prefix the executable with & in PowerShell')
        elif os.path.lexists(ini):
            fix = f'remove {ini}, then run python -m tools.prepare_checkout'
        else:
            fix = 'python -m tools.prepare_checkout'
        inis = line(False, f'{INI}/', 'no rulesmd.ini; retail-data tests and oracles read it', fix)
    if (ini / INI).is_symlink():
        lines.append(f'{"note":8}  {ini / INI} is a stray link inside the INI folder; remove it')

    pins = requirements()
    packages = ', '.join(f'{name} {found}' + ('' if found == pinned else f' (pinned {pinned})')
                         if found else f'no {name}' for name, pinned, found in pins)
    line(all(found == pinned for _, pinned, found in pins), 'Python tools',
         f'Python {sys.version.split()[0]}, {packages}', install_command())
    if importlib.util.find_spec('unicorn') is not None:
        from tools import native_oracle
        try:
            executable = native_oracle.configured_gamemd()
            native_oracle.image_bytes()
            line(True, 'native oracles', f'retail gamemd.exe at {executable}')
        except native_oracle.OracleError as error:
            # configured_gamemd prefers VERA20K_GAMEMD_EXE over RA2_DIR.
            line(False, 'native oracles', str(error),
                 'point VERA20K_GAMEMD_EXE at the retail gamemd.exe, or unset it to use RA2_DIR'
                 if os.environ.get('VERA20K_GAMEMD_EXE')
                 else 'point RA2_DIR at the folder with the retail gamemd.exe')
    return lines, game and retail and inis


def main(argv=None, root: Path = REPO) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--check', action='store_true', help='report only; change nothing')
    parser.add_argument('--ra2-dir', type=Path, help='game folder for a new config.toml')
    args = parser.parse_args(argv)
    if args.check and args.ra2_dir:
        parser.error('--ra2-dir writes config.toml; drop --check')
    root = root.resolve()
    main_root = main_checkout(root)
    for step in [] if args.check else prepare(root, main_root, args.ra2_dir):
        print(f'{"done":8}  {step}')
    lines, ready = report(root, main_root)
    print('\n'.join(lines))
    return 0 if ready else 1


if __name__ == '__main__':
    sys.exit(main())
