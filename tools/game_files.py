"""Prepare a local Yuri's Revenge data folder for VERA20k.

VERA20k never ships retail game files: the engine reads `ra2md.mix`, `ra2.mix`
and their siblings straight out of a Yuri's Revenge installation. This tool
turns a licensed source — a Steam installation, a retail folder, an unpacked
disc — into the smallest folder the game can start from, reports exactly what
is missing when it cannot, and writes the `config.toml` that points the game
at the result.

    python -m tools.game_files verify --dir <folder>
    python -m tools.game_files slim --source <folder> --target <folder>
    python -m tools.game_files steam --target <folder>
    python -m tools.game_files config --dir <folder>

What the game needs is not decided here. `verify` runs the engine's own
installer check (`vera20k --check-game-data`, the same read-only pass the
Windows installer runs before it copies anything) and prints its required and
optional lists, so the answer comes from the production readers rather than
from a second list that could drift away from them.

Nothing here downloads retail files from an unofficial source. The only source
this tool knows is a folder the player already owns; `steam` runs Valve's own
`steamcmd` against an account that owns the game and asks the player to log in.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

if sys.version_info < (3, 12):
    sys.exit(f'Python 3.12 or newer is required; {sys.executable} is {sys.version.split()[0]}')

if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

REPO = Path(__file__).resolve().parents[1]
CONFIG = 'config.toml'
CONFIG_EXAMPLE = 'config.toml.example'

# The Steam application that carries a retail Yuri's Revenge installation:
# "Command & Conquer Red Alert 2 and Yuri's Revenge" (appid 2229850). It is a
# paid application: steamcmd needs the account that owns it.
STEAM_APPID = '2229850'
STEAM_APP_NAME = "Command & Conquer Red Alert 2 and Yuri's Revenge"

# What a slim folder keeps. Everything else in a retail folder is an
# executable, a movie or an installer, none of which the engine reads.
KEEP_SUFFIXES = ('.mix', '.csf', '.pal', '.shp', '.wav', '.fnt')

# A loose INI in the game folder overrides the one inside the archives, so a
# slim copy drops them: the archives carry the retail ones.
DROP_SUFFIXES = ('.ini',)


def human_size(size: int) -> str:
    """A byte count as a short human string."""
    value = float(size)
    for unit in ('B', 'KB', 'MB', 'GB'):
        if value < 1024 or unit == 'GB':
            return f'{value:.0f} {unit}' if unit == 'B' else f'{value:.1f} {unit}'
        value /= 1024
    return f'{value:.1f} GB'


def game_binary(profile: str = 'fast') -> Path:
    """The game executable this checkout built, as the build runner records it."""
    result = subprocess.run(
        [sys.executable, '-m', 'tools.cargo_run', '--resolve', 'vera20k', '--profile', profile],
        cwd=REPO, capture_output=True, text=True)
    printed = [line.strip() for line in (result.stdout or '').splitlines() if line.strip()]
    path = Path(printed[-1]) if printed else None
    if path is None or not path.is_file():
        raise SystemExit(
            f'no vera20k executable for the {profile} profile; build it first:\n'
            f'  python -m tools.cargo_run -- build --profile {profile} -p vera20k --bin vera20k')
    return path


def run_game_data_check(folder: Path, profile: str = 'fast') -> tuple[list[str], list[str], int]:
    """The engine's install check for FOLDER: required issues, optional issues, exit code."""
    with tempfile.TemporaryDirectory(prefix='vera20k-game-files-') as report_dir:
        result = subprocess.run(
            [str(game_binary(profile)), '--check-game-data', str(folder), report_dir],
            capture_output=True, text=True)
        required = read_lines(Path(report_dir) / 'required.txt')
        optional = read_lines(Path(report_dir) / 'optional.txt')
    return required, optional, result.returncode


def read_lines(path: Path) -> list[str]:
    """The non-empty lines of PATH."""
    if not path.is_file():
        return []
    return [line for line in path.read_text(encoding='utf-8').splitlines() if line.strip()]


def verify(folder: Path, profile: str = 'fast') -> tuple[list[str], bool]:
    """Report lines for FOLDER, and whether the game can start from it."""
    lines: list[str] = []
    if not folder.is_dir():
        return [f'problem  {folder} is not a folder'], False
    folder = find_game_folder(folder)
    required, optional, code = run_game_data_check(folder, profile)
    lines.append(f'folder   {folder}')
    if required:
        lines.append('required:')
        lines += [f'  missing  {issue}' for issue in required]
    else:
        lines.append('required:  none missing')
    if optional:
        lines.append('optional:')
        lines += [f'  note     {issue}' for issue in optional]
    lines.append('')
    lines.append('the game can start from this folder' if code == 0
                 else 'the game cannot start from this folder')
    return lines, code == 0


def keep_file(path: Path) -> bool:
    """Whether a slim copy keeps PATH."""
    if path.suffix.lower() in DROP_SUFFIXES:
        return False
    return path.suffix.lower() in KEEP_SUFFIXES


def find_game_folder(path: Path) -> Path:
    """The folder in PATH that holds the retail archives.

    A Steam installation keeps the game in a subfolder of the install
    directory, so a path that holds no `ra2md.mix` is searched one level down.
    """
    if (path / 'ra2md.mix').is_file():
        return path
    if path.is_dir():
        for child in sorted(path.iterdir()):
            if child.is_dir() and (child / 'ra2md.mix').is_file():
                return child
    return path


def slim(source: Path, target: Path) -> list[str]:
    """Copy the files the engine reads from SOURCE into TARGET; report the rest."""
    source = find_game_folder(source)
    if not source.is_dir():
        raise SystemExit(f'{source} is not a folder')
    target.mkdir(parents=True, exist_ok=True)
    kept = 0
    kept_bytes = 0
    skipped = 0
    skipped_bytes = 0
    for path in sorted(source.iterdir()):
        if not path.is_file():
            continue
        if keep_file(path):
            shutil.copy2(path, target / path.name)
            kept += 1
            kept_bytes += path.stat().st_size
        else:
            skipped += 1
            skipped_bytes += path.stat().st_size
    return [
        f'copied {kept} files ({human_size(kept_bytes)}) into {target}',
        f'skipped {skipped} files ({human_size(skipped_bytes)}): executables, movies, installers',
    ]


def sha256(path: Path) -> str:
    """The file's SHA-256, as hex."""
    digest = hashlib.sha256()
    with path.open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def manifest(folder: Path) -> dict:
    """A JSON inventory of FOLDER: every kept file with its size and digest."""
    files = []
    for path in sorted(folder.iterdir()):
        if path.is_file() and keep_file(path):
            files.append({
                'name': path.name,
                'bytes': path.stat().st_size,
                'sha256': sha256(path),
            })
    return {'app': STEAM_APP_NAME, 'steam_appid': STEAM_APPID, 'files': files}


def steam_install(target: Path, appid: str = STEAM_APPID, user: str | None = None) -> int:
    """Run Valve's steamcmd to install the owned game into TARGET.

    The player logs in with their own account; steamcmd reads the password from
    the terminal. The application is paid, so an anonymous session cannot
    install it.
    """
    steamcmd = shutil.which('steamcmd')
    if steamcmd is None:
        raise SystemExit('steamcmd is not installed; install it with `brew install steamcmd`')
    target.mkdir(parents=True, exist_ok=True)
    command = [steamcmd, f'+force_install_dir {target}']
    command.append(f'+login {user}' if user else '+login')
    command += [f'+app_update {appid}', '+quit']
    print(f'+ {" ".join(command)}')
    print(f'Log in with the Steam account that owns {STEAM_APP_NAME} (appid {appid}),')
    print('then wait for the download to finish.')
    return subprocess.call(command)


def write_config(folder: Path, root: Path = REPO) -> str:
    """Write the checkout's config.toml so the game reads FOLDER."""
    example = root / CONFIG_EXAMPLE
    text = example.read_text(encoding='utf-8')
    value = json.dumps(folder.resolve().as_posix())
    lines = []
    replaced = False
    for line in text.splitlines(keepends=True):
        if not replaced and line.startswith('ra2_dir'):
            lines.append(f'ra2_dir = {value}\n')
            replaced = True
        else:
            lines.append(line)
    if not replaced:
        raise SystemExit(f'{CONFIG_EXAMPLE} has no ra2_dir line to set')
    (root / CONFIG).write_text(''.join(lines), encoding='utf-8')
    return f'wrote {CONFIG} with ra2_dir {folder.resolve().as_posix()}'


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    sub = parser.add_subparsers(dest='command', required=True)

    verify_parser = sub.add_parser('verify', help='report what a folder still lacks')
    verify_parser.add_argument('--dir', type=Path, required=True, help='the game folder')
    verify_parser.add_argument('--profile', default='fast', help='the built profile to check with')

    slim_parser = sub.add_parser('slim', help='copy only the files the engine reads')
    slim_parser.add_argument('--source', type=Path, required=True, help='the licensed source folder')
    slim_parser.add_argument('--target', type=Path, required=True, help='the folder to fill')
    slim_parser.add_argument('--profile', default='fast', help='the built profile to check with')

    steam_parser = sub.add_parser('steam', help='install the owned game with steamcmd')
    steam_parser.add_argument('--target', type=Path, required=True, help='the folder to install into')
    steam_parser.add_argument('--appid', default=STEAM_APPID, help='the Steam application id')
    steam_parser.add_argument('--user', help='the Steam account name; prompted for otherwise')

    manifest_parser = sub.add_parser('manifest', help='write a JSON inventory of a folder')
    manifest_parser.add_argument('--dir', type=Path, required=True, help='the game folder')
    manifest_parser.add_argument('--out', type=Path, help='where to write the JSON; stdout otherwise')

    config_parser = sub.add_parser('config', help='point the checkout at a game folder')
    config_parser.add_argument('--dir', type=Path, required=True, help='the game folder')

    args = parser.parse_args(argv)

    if args.command == 'verify':
        lines, ready = verify(args.dir, args.profile)
        print('\n'.join(lines))
        return 0 if ready else 1
    if args.command == 'slim':
        print('\n'.join(slim(args.source, args.target)))
        lines, ready = verify(args.target, args.profile)
        print('\n'.join(lines))
        return 0 if ready else 1
    if args.command == 'steam':
        return steam_install(args.target, args.appid, args.user)
    if args.command == 'manifest':
        text = json.dumps(manifest(args.dir), indent=2) + '\n'
        if args.out:
            args.out.write_text(text, encoding='utf-8')
            print(f'wrote {args.out}')
        else:
            print(text, end='')
        return 0
    if args.command == 'config':
        print(write_config(args.dir))
        return 0
    return 2


if __name__ == '__main__':
    sys.exit(main())
