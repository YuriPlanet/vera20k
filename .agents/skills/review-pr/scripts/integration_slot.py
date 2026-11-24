"""Claim a long-lived integration checkout for one PR, and release it afterwards.

Usage (from any checkout of the repository):
    python integration_slot.py claim <owner> [--wait SECONDS]   # prints the slot path
    python integration_slot.py release <owner>
    python integration_slot.py status

tools.cargo_run gives each checkout path its own compiler cache, so a new worktree
per PR rebuilt every dependency and left a 2-14 GB cache until retention reclaimed
it an hour after removal: on 2026-10-20 seven PR worktrees from one hour of
integration held 34 GB. The slots are fixed paths, `<main checkout>-wt/integration-<n>`,
kept between PRs, so their caches stay warm and SLOTS bounds their disk.

A claim is a file under the common Git directory, created exclusively, so two
sessions never hold one slot. OWNER names the work (`pr2282`); claiming again with
the same owner returns the same slot. A slot is handed out only clean: tracked
changes or untracked files there are another session's, so it is skipped and
reported, never discarded. A claim older than STALE_SECONDS belongs to a session
that stopped without releasing; a clean slot under it is taken over.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import uuid

SLOTS = 2
STALE_SECONDS = 12 * 3600
WAIT_STEP_SECONDS = 30
OWNER = re.compile(r'[A-Za-z0-9._-]{1,64}')


def git(cwd: Path, *args: str) -> str:
    return subprocess.run(['git', *args], cwd=cwd, check=True, capture_output=True,
                          text=True).stdout.strip()


def main_checkout(root: Path) -> Path:
    listing = git(root, 'worktree', 'list', '--porcelain')
    return Path(listing.splitlines()[0].removeprefix('worktree ')).resolve()


def claims_dir(root: Path) -> Path:
    return Path(git(root, 'rev-parse', '--path-format=absolute', '--git-common-dir')) / 'integration-slots'


def slot_path(main: Path, index: int) -> Path:
    return main.parent / f'{main.name}-wt' / f'integration-{index}'


def read_claim(path: Path) -> dict | None:
    try:
        claim = json.loads(path.read_text(encoding='utf-8'))
    except FileNotFoundError:
        return None
    except (OSError, ValueError):
        return {'owner': '?', 'claimed_unix': 0}  # Unreadable: old enough to take over.
    return claim if isinstance(claim, dict) else {'owner': '?', 'claimed_unix': 0}


def write_claim(path: Path, owner: str) -> bool:
    try:
        handle = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    except FileExistsError:
        return False
    with os.fdopen(handle, 'w', encoding='utf-8') as stream:
        json.dump({'owner': owner, 'claimed_unix': time.time()}, stream)
    return True


def registered(main: Path, path: Path) -> bool:
    listing = git(main, 'worktree', 'list', '--porcelain')
    return any(Path(line.removeprefix('worktree ')).resolve() == path.resolve()
               for line in listing.splitlines() if line.startswith('worktree '))


def dirt(path: Path) -> str:
    """Tracked changes and untracked files; ignored files (ini/, config.toml) are the slot's."""
    return git(path, 'status', '--porcelain', '--untracked-files=normal')


def prepare(main: Path, path: Path) -> None:
    """Create the slot's worktree on first use and give it the ignored local files."""
    if path.exists():
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    git(main, 'worktree', 'add', '--detach', str(path))
    report = subprocess.run([sys.executable, '-m', 'tools.prepare_checkout'], cwd=path,
                            capture_output=True, text=True, check=False)
    print(report.stdout + report.stderr, end='', file=sys.stderr)


def usable(main: Path, path: Path) -> str | None:
    """Why the slot cannot be handed out, or None."""
    if not path.exists():
        return None
    if not registered(main, path):
        return f'{path} exists but is not a worktree of this repository'
    if changes := dirt(path):
        return f'{path} has uncommitted work:\n{changes}'
    return None


def try_claim(root: Path, owner: str) -> tuple[Path | None, list[str]]:
    main, claims = main_checkout(root), claims_dir(root)
    claims.mkdir(parents=True, exist_ok=True)
    notes = []
    for index in range(1, SLOTS + 1):
        path, claim_file = slot_path(main, index), claims / f'{index}.json'
        claim = read_claim(claim_file)
        if claim and claim.get('owner') == owner:
            return path, notes
    for index in range(1, SLOTS + 1):
        path, claim_file = slot_path(main, index), claims / f'{index}.json'
        claim = read_claim(claim_file)
        if claim:
            age = time.time() - float(claim.get('claimed_unix', 0))
            if age < STALE_SECONDS:
                notes.append(f'slot {index} is held by {claim.get("owner")} ({age / 60:.0f} min)')
                continue
            if problem := usable(main, path):
                notes.append(f'slot {index} has a stale claim by {claim.get("owner")}; {problem}')
                continue
            # Renaming the stale claim away is atomic, so only one session takes it over.
            try:
                os.rename(claim_file, claims / f'{index}.stale-{uuid.uuid4().hex}')
            except FileNotFoundError:
                continue
            for stale in claims.glob(f'{index}.stale-*'):
                stale.unlink(missing_ok=True)
            notes.append(f'slot {index}: took over a stale claim by {claim.get("owner")}')
        if not write_claim(claim_file, owner):
            continue
        problem = usable(main, path)
        if problem is None:
            try:
                prepare(main, path)
            except subprocess.CalledProcessError as error:
                problem = f'creating {path} failed: {error.stderr.strip()}'
        if problem is None:
            return path, notes
        claim_file.unlink()
        notes.append(f'slot {index} skipped: {problem}')
    return None, notes


def claim(root: Path, owner: str, wait: float) -> int:
    deadline = time.monotonic() + wait
    reported = False
    while True:
        path, notes = try_claim(root, owner)
        if path is not None or time.monotonic() >= deadline:
            break
        if not reported:
            print('Waiting for an integration slot: ' + '; '.join(notes), file=sys.stderr, flush=True)
            reported = True
        time.sleep(min(WAIT_STEP_SECONDS, max(0.0, deadline - time.monotonic())))
    for note in notes:
        print(note, file=sys.stderr)
    if path is None:
        print('No integration slot is free.', file=sys.stderr)
        return 1
    print(path)
    return 0


def release(root: Path, owner: str) -> int:
    main, claims = main_checkout(root), claims_dir(root)
    for index in range(1, SLOTS + 1):
        claim_file = claims / f'{index}.json'
        held = read_claim(claim_file)
        if not held or held.get('owner') != owner:
            continue
        path = slot_path(main, index)
        if path.exists() and (changes := dirt(path)):
            print(f'{path} has uncommitted work; finish or abort it (git merge --abort) first:\n'
                  f'{changes}', file=sys.stderr)
            return 1
        claim_file.unlink()
        print(f'Released {path}')
        return 0
    print(f'{owner} holds no integration slot', file=sys.stderr)
    return 1


def status(root: Path) -> int:
    main, claims = main_checkout(root), claims_dir(root)
    for index in range(1, SLOTS + 1):
        path = slot_path(main, index)
        held = read_claim(claims / f'{index}.json')
        owner = (f'held by {held.get("owner")} for '
                 f'{(time.time() - float(held.get("claimed_unix", 0))) / 60:.0f} min' if held else 'free')
        state = ('absent' if not path.exists() else
                 git(path, 'rev-parse', '--short', 'HEAD') + (' dirty' if dirt(path) else ' clean'))
        print(f'{index} {path} {owner}; {state}')
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest='command', required=True)
    take = commands.add_parser('claim', help='claim a slot and print its path')
    take.add_argument('owner', help='the work holding the slot, e.g. pr2282')
    take.add_argument('--wait', type=float, default=0.0, help='seconds to wait for a free slot')
    give = commands.add_parser('release', help='release the slot this owner holds')
    give.add_argument('owner')
    commands.add_parser('status', help='list the slots and their claims')
    args = parser.parse_args(argv)
    if args.command != 'status' and not OWNER.fullmatch(args.owner):
        parser.error('owner must be 1-64 letters, digits, ".", "_" or "-"')
    root = Path.cwd()
    if args.command == 'claim':
        return claim(root, args.owner, args.wait)
    if args.command == 'release':
        return release(root, args.owner)
    return status(root)


if __name__ == '__main__':
    sys.exit(main())
