"""Seed a checkout's first build of a profile with a warm cache's dependency outputs.

cargo_run calls this under both of its locks, before Cargo first builds a host
profile into a checkout's cache. Without it each new worktree recompiled every
registry dependency into an empty cache.

Only units of registry and git packages are copied. Cargo names their outputs
from the package and its features, profile and toolchain, never the checkout path,
and judges them fresh by the timestamps of sources in the shared registry. A
workspace unit has the same name in every checkout, but its freshness compares
this checkout's source timestamps with the copied fingerprint, so a copy could
pass for a build of different source. Workspace units and incremental sessions
therefore always compile here.

Files are copy-on-write clones where the filesystem supports them (APFS
`clonefile`, Linux `FICLONE` on Btrfs/XFS) and plain copies elsewhere. Timestamps
are kept, so Cargo sees the same order of outputs as in the source cache.
"""
from __future__ import annotations

import errno
import ctypes
import hashlib
import os
from pathlib import Path
import re
import shutil
import sys
import time
import tomllib

# `.fingerprint/<package>-<hash>` and `build/<package>-<hash>`: one Cargo unit.
UNIT = re.compile(r'(.+)-([0-9a-f]{16})')
# `deps/[lib]<crate>-<hash>[.<extension>...]`: an output of the unit with that hash.
OUTPUT = re.compile(r'[A-Za-z0-9_]+-([0-9a-f]{16})(?:\..*)?')
# Cargo's built-in profiles build into these directories; custom ones into their name.
PROFILE_DIRECTORIES = {'dev': 'debug', 'test': 'debug', 'release': 'release', 'bench': 'release'}


def profile_directory(args: list[str]) -> str | None:
    """The host profile directory a Cargo command builds into; None for a cross target."""
    from tools.cargo_run import HOST_PROFILES
    options = args[:args.index('--')] if '--' in args else args
    profile = 'debug'
    for i, option in enumerate(options):
        name, _, value = option.partition('=')
        if name == '--target':
            return None
        if option in {'--release', '-r'}:
            profile = 'release'
        elif name == '--profile':
            value = value or (options[i + 1] if i + 1 < len(options) else '')
            profile = PROFILE_DIRECTORIES.get(value, value)
    return profile if profile in HOST_PROFILES else None


def external_packages(root: Path) -> set[str]:
    """Registry and git package names in the checkout's lockfile, none shared with a path package."""
    with (root / 'Cargo.lock').open('rb') as handle:
        packages = tomllib.load(handle).get('package', [])
    local = {package['name'] for package in packages if 'source' not in package}
    return {package['name'] for package in packages if 'source' in package} - local


def seed_source(root: Path, store: Path, target: Path, profile: str) -> Path | None:
    """The registered cache of another live checkout with this profile.

    A cache built from the same lockfile comes first, then the most recently used.
    """
    from tools._cargo_cache import _json, _plain
    registry = store / 'cache-roots.json'
    if not registry.exists():
        return None
    try:
        lock = (root / 'Cargo.lock').read_bytes()
    except FileNotFoundError:
        return None
    roots = data.get('roots') if isinstance(data := _json(registry), dict) else None
    if not isinstance(roots, dict):
        raise ValueError('Malformed owned cache registry')
    candidates = []
    for name, entry in roots.items():
        cache, checkout = Path(name), entry.get('checkout') if isinstance(entry, dict) else None
        used = entry.get('last_use_unix') if isinstance(entry, dict) else None
        if (cache == target or cache.parent != target.parent or not isinstance(checkout, str)
                or type(used) not in (int, float)
                or cache.name != hashlib.sha256(os.fsencode(checkout)).hexdigest()[:16]
                or not Path(checkout, '.git').exists()
                or not (cache / profile / '.fingerprint').is_dir()):
            continue
        try:
            _plain(cache / profile)
            same = Path(checkout, 'Cargo.lock').read_bytes() == lock
        except (OSError, ValueError):
            continue
        candidates.append((same, used, cache))
    return max(candidates)[2] if candidates else None


_clonefile = None


def clone(source: Path, destination: Path) -> bool:
    """Copy one file, sharing its blocks where the filesystem can; True if it did."""
    global _clonefile
    if sys.platform == 'darwin':
        if _clonefile is None:
            _clonefile = ctypes.CDLL(None, use_errno=True).clonefile
        # CLONE_NOFOLLOW; the clone keeps the source's mode and timestamps.
        if _clonefile(os.fsencode(source), os.fsencode(destination), ctypes.c_uint32(1)) == 0:
            return True
        code = ctypes.get_errno()
        if code not in (errno.ENOTSUP, errno.EXDEV):
            raise OSError(code, os.strerror(code), str(destination))
    elif sys.platform.startswith('linux'):
        import fcntl
        ficlone = 0x40049409  # _IOW(0x94, 9, int), linux/fs.h
        with source.open('rb') as reader, destination.open('xb') as writer:
            try:
                fcntl.ioctl(writer.fileno(), ficlone, reader.fileno())
                cloned = True
            except OSError as error:
                if error.errno not in (errno.EOPNOTSUPP, errno.EXDEV, errno.EINVAL, errno.ENOTTY):
                    raise
                shutil.copyfileobj(reader, writer)
                cloned = False
        shutil.copystat(source, destination)
        return cloned
    shutil.copy2(source, destination)
    return False


def _copy(source: Path, destination: Path, counts: dict):
    """Copy regular files and directories beneath source; links are not copied."""
    if source.is_symlink():
        return
    if source.is_dir():
        destination.mkdir(parents=True, exist_ok=True)
        for child in sorted(source.iterdir()):
            _copy(child, destination / child.name, counts)
    elif source.is_file():
        destination.parent.mkdir(parents=True, exist_ok=True)
        counts['cloned' if clone(source, destination) else 'copied'] += 1
        counts['bytes'] += source.stat().st_size


def seed(root: Path, store: Path, target: Path, profile: str) -> str | None:
    """Copy the dependency units of a warm cache into this cache's new profile.

    Returns a summary line, or None when the profile exists or no cache has it.
    Outputs go first and fingerprints last: Cargo rebuilds a unit whose
    fingerprint or output is missing, so an interrupted seed leaves no unit that
    Cargo would trust without the files it names.
    """
    destination = target / profile
    if destination.exists():
        return None
    started, free = time.monotonic(), shutil.disk_usage(target).free
    source_cache = seed_source(root, store, target, profile)
    if source_cache is None:
        return None
    source = source_cache / profile
    external = external_packages(root)
    units = sorted(path.name for path in (source / '.fingerprint').iterdir()
                   if (match := UNIT.fullmatch(path.name)) and match[1] in external)
    hashes = {name[-16:] for name in units}
    counts = {'cloned': 0, 'copied': 0, 'bytes': 0}
    if (source / 'deps').is_dir():
        for path in sorted((source / 'deps').iterdir()):
            if (match := OUTPUT.fullmatch(path.name)) and match[1] in hashes:
                _copy(path, destination / 'deps' / path.name, counts)
    for directory in ('build', '.fingerprint'):
        for name in units:
            if (source / directory / name).exists():
                _copy(source / directory / name, destination / directory / name, counts)
    used = free - shutil.disk_usage(target).free
    method = 'cloned' if not counts['copied'] else 'copied' if not counts['cloned'] else 'cloned and copied'
    return (f'Seeded {profile} from {source_cache.name}: {len(units)} dependency units, '
            f'{counts["cloned"] + counts["copied"]} files, {counts["bytes"] / 2 ** 30:.2f} GiB {method}; '
            f'free space fell {used / 2 ** 30:.2f} GiB in {time.monotonic() - started:.1f} s')
