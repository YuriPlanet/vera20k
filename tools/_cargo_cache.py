"""Private retention implementation; cargo_run owns its lock, registry and CLI.

Only compiler objects, complete incremental sessions, executables a newer one of
the same name replaced in a Cargo deps directory, and the Cargo profile output of
a cache whose checkout is gone are disposable. Preserved labels and all surviving
references remain protected. Never infer that linking made its
debug objects disposable: Mach-O executables commonly refer to them by pathname.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
from decimal import Decimal, InvalidOperation
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import uuid

GIB = 1024 ** 3


def gib_bytes(value: str) -> int:
    try:
        number = Decimal(value)
        if not number.is_finite() or number < 0:
            raise ValueError('Cache sizes must be finite and nonnegative')
        if number > Decimal(2 ** 63 - 1) / GIB:
            raise ValueError('Cache sizes must fit a signed 64-bit byte count')
        return int(number * GIB)
    except (InvalidOperation, OverflowError) as error:
        raise ValueError('Expected a finite nonnegative size in GiB') from error


# Each worktree that builds tests and a fast game keeps 0.6-6.4 GiB of
# incremental sessions. A 4 GiB budget covered about one of the ten worktrees
# active on 2026-10-17: passes then removed sessions minutes old, and the next
# one-line edit compiled its crate from scratch (75-110 s instead of 15-20 s).
# 16 GiB keeps the newest sessions of the worktrees active in the last hours.
# The total budget fits this 228 GiB volume, which also holds Ghidra's database
# and backups and captures: at 40 GiB free space settled near 28 GiB rather
# than the reserve, which still trims whenever the volume runs short.
# The reserve covers one build and the writes beside it. In the receipts from
# 2026-09-30 to 10-18, free space starting at 20 GiB or less fell at most
# 7.2 GiB before the next pass. On 2026-10-18 the volume's 12-16 GiB free sat
# below the former 16 GiB reserve for hours, blocking builds 12 GiB admits.
# A pass the reserve starts trims past it by the headroom. Trimming only back to
# the reserve made the next build due another pass: on 2026-10-19 87 passes of
# about a minute each held the build lock for 108 minutes, each freeing 4-6 GiB
# that builds used up within about ten minutes.
@dataclass(frozen=True)
class CachePolicy:
    cache_bytes: int = 40 * GIB
    incremental_bytes: int = 16 * GIB
    min_free_bytes: int = 12 * GIB
    free_headroom_bytes: int = 8 * GIB

    def __post_init__(self):
        if any(type(value) is not int or value < 0 for value in asdict(self).values()):
            raise ValueError('Cache byte limits must be nonnegative integers')

    @classmethod
    def from_env(cls, cache=None, incremental=None, free=None, headroom=None):
        return cls(*(gib_bytes(value if value is not None else os.environ.get(key, default))
                     for value, key, default in (
                         (cache, 'VERA20K_CACHE_GIB', '40'),
                         (incremental, 'VERA20K_INCREMENTAL_GIB', '16'),
                         (free, 'VERA20K_MIN_FREE_GIB', '12'),
                         (headroom, 'VERA20K_FREE_HEADROOM_GIB', '8'))))


# A crate's newest finalized session built this recently belongs to current work:
# the soft budgets never take it, only a short free-space reserve. Under the soft
# budgets alone, passes on 2026-10-19 removed sessions minutes old, and 18% of
# focused-test compiles took 75 s or more against a 32 s median.
ACTIVE_SESSION_SECONDS = 12 * 60 * 60
# A registered cache whose checkout is gone, unused at least this long, is
# reclaimed whatever the budgets; see _abandoned_roots.
ABANDONED_SECONDS = 60 * 60
# Receipts are an audit trail of recent passes: 3,883 held 1.1 GiB on 2026-10-19.
RECEIPT_SECONDS = 7 * 24 * 60 * 60


def _json(path: Path):
    from tools.cargo_run import _manifest_object
    _regular(path)
    return json.loads(path.read_text(), object_pairs_hook=_manifest_object)


def _plain(path: Path):
    """No symlinks/junctions beneath a canonical path, including its parents."""
    for parent in [*reversed(path.parents), path]:
        info = parent.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 0x400:
            raise ValueError(f'Cache path contains a link or reparse point: {parent}')


def _regular(path: Path):
    _plain(path)
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode):
        raise ValueError(f'Expected regular retention input: {path}')
    return info


def _identity(path: Path) -> tuple:
    info = _regular(path)
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns,
            info.st_ctime_ns, info.st_nlink)


def _dependency_identity(path: Path) -> tuple | None:
    """Absent references are degraded evidence; links/errors are not absence."""
    if not path.is_absolute() or '..' in path.parts:
        raise ValueError(f'Unresolved debug dependency: {path}')
    try:
        return _identity(path)
    except FileNotFoundError:
        # _plain walks from root: a dangling symlink is rejected before its
        # nonexistent child can be mistaken for an already missing object.
        return None


def _inspection_revision() -> str:
    # Successful references depend only on the executable and inspector. Their
    # existence/identity is checked afresh each pass, including missing paths.
    files = [Path(__file__), Path(__file__).with_name('_cargo_macho.py'),
             Path(__file__).with_name('cargo_run.py')]
    digest = hashlib.sha256()
    for path in files:
        if path.exists():
            digest.update(path.read_bytes())
    tool = shutil.which('readelf')
    digest.update(repr((sys.platform, tool, _identity(Path(tool).resolve()) if tool else None)).encode())
    return digest.hexdigest()


def _inspect_cached(store: Path, binary: Path, revision: str, preserved, counters) -> set[Path]:
    """Cache inspection, never the current protection/deletion decision.

    Metadata equality includes ctime and inode, not just mtime/size. Unknown
    inode platforms re-inspect. Cached failures never authorize deletion and
    expire after five minutes, or immediately on binary/inspector changes.
    """
    identity = _identity(binary)
    expected = preserved[1]['sha256'] if preserved else None
    key = hashlib.sha256(str(binary).encode()).hexdigest()
    path = store / 'inspections' / (key + '.json')
    stamp = {'binary': str(binary), 'identity': list(identity),
             'revision': revision, 'expected_sha256': expected}
    if identity[1] and path.exists():
        record = _json(path)
        if not isinstance(record, dict):
            raise ValueError('Malformed dependency inspection cache')
        if all(record.get(k) == v for k, v in stamp.items()):
            retry = record.get('retry_after', 0)
            if isinstance(record.get('error'), str) and isinstance(retry, (int, float)) and time.time() < retry:
                counters['failure_hits'] += 1
                raise ValueError('Unchanged dependency inspection failure (cached up to 300s): ' + record['error'])
            if isinstance(record.get('references'), list) and all(isinstance(v, str) for v in record['references']):
                refs = {Path(v) for v in record['references']}
                if any(not v.is_absolute() or '..' in v.parts for v in refs):
                    raise ValueError('Invalid cached debug reference')
                counters['hits'] += 1
                return refs
    counters['misses'] += 1
    try:
        if preserved:
            from tools.cargo_run import preserved_artifact
            preserved_artifact(*preserved)
        refs = _dependencies(binary)
        if _identity(binary) != identity:
            raise ValueError(f'Executable changed during inspection: {binary}')
    except (OSError, ValueError, subprocess.SubprocessError, UnicodeError) as error:
        _write(path, dict(stamp, error=str(error), retry_after=time.time() + 300))
        raise
    _write(path, dict(stamp, references=sorted(map(str, refs))))
    return refs


def _directory_identity(path: Path) -> tuple:
    _plain(path)
    info = path.stat()
    if not stat.S_ISDIR(info.st_mode):
        raise ValueError(f'Expected retention directory: {path}')
    return (info.st_dev, info.st_ino, info.st_mtime_ns, info.st_ctime_ns)


def _write(path: Path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    _plain(path.parent)
    if path.exists():
        _regular(path)
    with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, delete=False,
                                     prefix='.retention-', encoding='utf-8') as output:
        pending = Path(output.name)
        json.dump(value, output, indent=2)
        output.write('\n')
    try:
        pending.replace(path)
    finally:
        pending.unlink(missing_ok=True)


def _reported_profile(target: Path, source: Path) -> str | None:
    """Only exact host/cross-target executable layouts actually reported by Cargo."""
    from tools.cargo_run import HOST_PROFILES
    if not source.is_relative_to(target):
        return None
    parts = source.relative_to(target).parts
    if len(parts) in (2, 3) and parts[0] in HOST_PROFILES:
        if len(parts) == 2 or parts[1] in {'deps', 'examples'}:
            return parts[0]
    if len(parts) in (3, 4) and parts[1] in HOST_PROFILES:
        if len(parts) == 3 or parts[2] in {'deps', 'examples'}:
            return '/'.join(parts[:2])
    return None


def register_locked(root: Path, store: Path, target: Path, artifacts=()) -> list[Path]:
    """Record even failed/check/unlabelled/custom-target invocations under owner lock.

    Returns this checkout's other registered caches that still exist, so the
    caller can report a split cache.
    """
    from tools.cargo_run import build_store
    target.mkdir(parents=True, exist_ok=True)
    _plain(target)
    _, namespace = build_store(root)
    if target.parent.name != 'owned-worktrees' or target.name != namespace:
        raise ValueError(f'Not this checkout\'s owned target: {target}')
    path = store / 'cache-roots.json'
    data = _json(path) if path.exists() else {'schema': 1, 'roots': {}}
    if not isinstance(data, dict) or type(data.get('schema')) is not int or data['schema'] != 1 or not isinstance(data.get('roots'), dict):
        raise ValueError('Malformed owned cache registry')
    prior = data['roots'].get(str(target), {})
    if not isinstance(prior, dict) or not isinstance(prior.get('profiles', []), list):
        raise ValueError('Malformed owned cache entry')
    profiles = set(prior.get('profiles', []))
    profiles.update(profile for source in artifacts if (profile := _reported_profile(target, source)))
    data['roots'][str(target)] = {'checkout': str(root), 'last_use_unix': time.time(),
                                'profiles': sorted(profiles)}
    _write(path, data)
    return [Path(other) for other, entry in data['roots'].items()
            if other != str(target) and isinstance(entry, dict)
            and entry.get('checkout') == str(root) and Path(other).is_dir()]


def _owned_root(checkout: str, target: Path, store: Path, stores: dict) -> Path | None:
    from tools.cargo_run import build_store
    if not isinstance(checkout, str) or not Path(checkout).is_absolute():
        raise ValueError('Cache registry requires an absolute checkout')
    if not target.is_absolute() or '..' in target.parts:
        raise ValueError('Cache registry requires an absolute target')
    # Historical compatibility aliases may point to canonical checkouts. Resolve
    # that anchor, then reject links within the actual cache tree.
    checkout_path = Path(checkout).resolve()
    if not checkout_path.exists() or not target.exists():
        return None  # Never adopt an abandoned/unverifiable cache for deletion.
    # Labels share checkouts; each lookup runs Git once per inventory.
    if checkout_path not in stores:
        stores[checkout_path] = build_store(checkout_path)
    actual_store, namespace = stores[checkout_path]
    if (actual_store.resolve() != store.resolve() or target.parent.name != 'owned-worktrees'
            or target.name != namespace):
        raise ValueError(f'Cache ownership mismatch: {target}')
    _plain(target)
    canonical = target.resolve()
    if not canonical.is_dir():
        raise ValueError(f'Owned target is not a directory: {canonical}')
    return canonical


def _abandoned_roots(root: Path, store: Path) -> set[Path]:
    """Registered caches whose checkout is gone: output nothing can build into again.

    A removed worktree leaves its cache, and _owned_root adopts only caches of live
    checkouts, so on 2026-10-19 five such caches held 17.8 GiB that no budget
    counted. The registry proves ownership: the cache sits in this repository's
    cache root under the namespace of its recorded checkout path, that path no
    longer exists, no run holds the checkout's lock, and the cache has been idle
    for ABANDONED_SECONDS.
    """
    from tools.cargo_run import build_store, owned_target
    registry = store / 'cache-roots.json'
    if not registry.exists():
        return set()
    data = _json(registry)
    if not isinstance(data, dict) or not isinstance(data.get('roots'), dict):
        raise ValueError('Malformed owned cache registry')
    _, namespace = build_store(root)
    cache_root = owned_target(root, namespace).parent
    abandoned = set()
    for name, entry in data['roots'].items():
        if not isinstance(entry, dict):
            raise ValueError('Malformed owned cache entry')
        checkout, used = entry.get('checkout'), entry.get('last_use_unix')
        target = Path(name)
        if (not isinstance(checkout, str) or not Path(checkout).is_absolute()
                or type(used) not in (int, float) or Path(checkout).exists()
                or target.parent != cache_root or not target.is_dir()
                or target.name != hashlib.sha256(os.fsencode(checkout)).hexdigest()[:16]
                or not time.time() - used >= ABANDONED_SECONDS
                or _checkout_running(store, target.name)):
            continue
        try:
            _plain(target)
        except ValueError:
            continue  # A linked cache is unverifiable; never adopt it for deletion.
        abandoned.add(target)
    return abandoned


def _abandoned_output(cache: Path, path: Path) -> bool:
    """Cargo's own output in an abandoned cache: host profile trees and root files."""
    from tools.cargo_run import HOST_PROFILES
    parts = path.relative_to(cache).parts
    return (len(parts) > 1 and parts[0] in HOST_PROFILES) or parts in (
        ('.rustc_info.json',), ('.future-incompat-report.json',), ('CACHEDIR.TAG',))


def _inputs(root: Path, store: Path, *, preserved_checks=None, profiles=None, notes=None):
    from tools.cargo_run import HOST_PROFILES, preserved_manifest, build_store, owned_target
    roots, binaries, watched, stores = set(), set(), {}, {}
    registry = store / 'cache-roots.json'
    if registry.exists():
        data = _json(registry)
        if not isinstance(data, dict) or type(data.get('schema')) is not int or data['schema'] != 1 or not isinstance(data.get('roots'), dict):
            raise ValueError('Malformed owned cache registry')
        watched[registry] = _identity(registry)
        for name, entry in data['roots'].items():
            if not isinstance(entry, dict):
                raise ValueError('Malformed owned cache entry')
            if path := _owned_root(entry.get('checkout'), Path(name), store, stores):
                roots.add(path)
                declared = entry.get('profiles', [])
                if not isinstance(declared, list):
                    raise ValueError('Malformed cache profiles')
                for profile in declared:
                    if (not isinstance(profile, str) or len(profile.split('/')) not in (1, 2)
                            or profile.split('/')[-1] not in HOST_PROFILES
                            or any(part in ('', '.', '..') or '\\' in part or ':' in part for part in profile.split('/'))):
                        raise ValueError('Malformed cache profile directory')
                    if profiles is not None:
                        profiles.add(path / profile)
    artifacts = store / 'artifacts'
    if artifacts.exists():
        _plain(artifacts)
        for label in sorted(artifacts.iterdir()):
            if label.name.startswith('.pending-'):
                raise ValueError(f'Incomplete preserved build: {label}')
            directory, target, entries = preserved_manifest(store, label.name)
            manifest_path = directory / 'manifest.json'
            data = _json(manifest_path)
            watched[manifest_path] = _identity(manifest_path)
            # Schema-1 historical manifests did not necessarily record checkout.
            # Still verify/protect every executable; adopt only proven roots.
            if data.get('checkout') is not None:
                if path := _owned_root(data['checkout'], target, store, stores):
                    roots.add(path)
                    if profiles is not None:
                        for entry in entries:
                            if profile := _reported_profile(target, Path(entry['source'])):
                                profiles.add(path / profile)
            for entry in entries:
                from tools.cargo_run import _label_path
                binary = _label_path(directory, entry['file'])
                _regular(binary)
                if preserved_checks is not None:
                    preserved_checks[binary] = (directory, entry)
                binaries.add(binary)
                watched[binary] = _identity(binary)
    latest = store / 'latest'
    if latest.exists():
        _plain(latest)
        # Latest is mutable, unlike validation labels. The resolver already
        # excludes missing/replaced outputs. Retain the record, report stale
        # entries, and inspect any replacement binary as a live cache consumer.
        for record in sorted(latest.iterdir()):
            data = _json(record)
            watched[record] = _identity(record)
            if not isinstance(data, dict):
                raise ValueError(f'Malformed latest build: {record}')
            for profile, entries in data.items():
                if profile not in HOST_PROFILES or not isinstance(entries, dict):
                    raise ValueError(f'Malformed latest profile: {record}')
                for entry in entries.values():
                    if not isinstance(entry, dict) or not isinstance(entry.get('path'), str):
                        raise ValueError(f'Malformed latest executable: {record}')
                    binary = Path(entry['path'])
                    if not binary.is_absolute() or '..' in binary.parts:
                        raise ValueError(f'Invalid latest executable: {binary}')
                    if not binary.exists():
                        if notes is not None:
                            notes.append(f'Stale latest output is absent (record retained): {binary}')
                        continue
                    watched[binary] = _identity(binary)
                    if notes is not None:
                        notes.append(f'Protecting actual output at mutable latest path: {binary}')
                    binaries.add(binary)
    # The current default root is safe to discover without a prior label.
    _, namespace = build_store(root)
    if path := _owned_root(str(root), owned_target(root, namespace), store, stores):
        roots.add(path)
    if profiles is not None:
        profiles.update(cache / profile for cache in roots for profile in HOST_PROFILES)
    return roots, binaries, watched


def _files(root: Path, directory_states=None):
    """Inventory without following directory links or accepting special files."""
    pending = [root]
    while pending:
        base = pending.pop()
        # Snapshot BEFORE scandir discovers entries; after-inventory snapshots
        # accept a new executable published in the discovery gap.
        identity = _directory_identity(base)
        if directory_states is not None:
            directory_states[base] = identity
        with os.scandir(base) as entries:
            for entry in entries:
                path = Path(entry.path)
                # DirEntry.stat reports st_ino/st_dev/st_nlink as zero on Windows.
                # Real lstat is required for inode accounting and hardlink safety.
                # https://docs.python.org/3.12/library/os.html#os.DirEntry.stat
                info = path.lstat()
                if stat.S_ISLNK(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 0x400:
                    continue
                if stat.S_ISDIR(info.st_mode):
                    pending.append(path)
                elif stat.S_ISREG(info.st_mode):
                    yield path, info


def _allocated(info) -> int:
    # st_blocks is actual allocated space on Unix, logical size is the conservative
    # portable fallback. Observed volume free space is recorded separately.
    return getattr(info, 'st_blocks', (info.st_size + 511) // 512) * 512


def _allocated_total(infos) -> int:
    unique, unidentified = {}, 0
    for info in infos:
        if info.st_ino:
            unique[info.st_dev, info.st_ino] = info
        else:
            # A filesystem may not expose inode identity. Never collapse all
            # unknown files into one entry and falsely report an empty budget.
            unidentified += _allocated(info)
    return unidentified + sum(_allocated(info) for info in unique.values())


class _CacheLinks:
    """Account for paths separately from the allocation shared by their inode."""

    def __init__(self, inventory, increments):
        self.keys = {path: (info.st_dev, info.st_ino) if info.st_ino else path
                     for path, info in inventory.items()}
        self.aliases = {}
        for path, key in self.keys.items():
            self.aliases.setdefault(key, set()).add(path)
        self.remaining = {key: set(paths) for key, paths in self.aliases.items()}
        self.incremental = {key: paths & increments for key, paths in self.aliases.items()}
        self.sizes = {key: _allocated(inventory[next(iter(paths))])
                      for key, paths in self.aliases.items()}
        self.closed = set()
        for key, paths in self.aliases.items():
            identities = {(info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns,
                           info.st_ctime_ns, info.st_nlink)
                          for path in paths for info in (inventory[path],)}
            if len(identities) == 1 and next(iter(identities))[-1] == len(paths):
                self.closed.add(key)

    def eligible(self, path):
        # Unknown outside links cannot establish a complete allocation closure.
        return self.keys[path] in self.closed

    def remove(self, path):
        key = self.keys[path]
        paths, incremental = self.remaining[key], self.incremental[key]
        paths.remove(path)
        was_incremental = path in incremental
        incremental.discard(path)
        return (self.sizes[key] if not paths else 0,
                self.sizes[key] if was_incremental and not incremental else 0)


def _magic(path: Path) -> bytes:
    with path.open('rb') as source:
        return source.read(4)


MACH = {b'\xcf\xfa\xed\xfe', b'\xce\xfa\xed\xfe', b'\xfe\xed\xfa\xcf',
        b'\xfe\xed\xfa\xce', b'\xca\xfe\xba\xbe', b'\xbe\xba\xfe\xca',
        b'\xca\xfe\xba\xbf', b'\xbf\xba\xfe\xca'}


def _stream(command: list[str], consume):
    # Large lib-test debug maps can exceed 70MiB; don't buffer symbols. Stderr
    # may exceed a pipe buffer too. Zero status alone does not prove completeness.
    with tempfile.TemporaryFile(mode='w+t', encoding='utf-8') as diagnostic:
        with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=diagnostic,
                              text=True, encoding='utf-8', errors='strict') as child:
            assert child.stdout is not None
            try:
                result = consume(child.stdout)
            except Exception:
                # Drain instead of killing anything; this is only our inspector.
                for _ in child.stdout:
                    pass
                child.wait()
                raise
            status = child.wait()
        diagnostic.seek(0)
        error = diagnostic.read(4096)
        if status or error:
            raise ValueError(f'Debug dependency inspection failed ({status}): {error.strip()}')
        return result


def _dependencies(binary: Path) -> set[Path]:
    magic = _magic(binary)
    if magic in MACH:
        from tools._cargo_macho import dependencies
        return dependencies(binary)
    if magic == b'\x7fELF':
        # GNU readelf supports this complete DWARF/link inspection. llvm-readelf
        # does not expose --debug-dump=info; do not infer compatible options.
        # https://sourceware.org/binutils/docs/binutils/readelf.html
        tool = shutil.which('readelf')
        if not tool:
            raise ValueError('ELF dependency inspection requires GNU readelf')
        def sections(lines):
            external = ('.gnu_debuglink', '.gnu_debugaltlink', '.debug_sup',
                        '.debug_cu_index', '.debug_tu_index', '.zdebug_')
            found_header = False
            for line in lines:
                found_header |= 'Section Headers:' in line
                if any(name in line for name in external):
                    raise ValueError('Unsupported external/compressed ELF debug closure')
            if not found_header:
                raise ValueError('Unrecognized ELF section inspection')
        _stream([tool, '--wide', '--sections', str(binary)], sections)
        def dwarf(lines):
            for line in lines:
                if any(name in line for name in ('DW_AT_dwo_name', 'DW_AT_GNU_dwo_name', 'DW_AT_GNU_dwo_id')):
                    raise ValueError('Unsupported split-DWARF dependency closure')
        _stream([tool, '--debug-dump=info,no-follow-links,do-not-use-debuginfod', str(binary)], dwarf)
        return set()  # Embedded DWARF needs no original compiler object.
    # PE FASTLINK can require object files beyond its PDB. Do not pretend that
    # merely retaining *.pdb establishes closure (MSVC /DEBUG documentation).
    raise ValueError(f'Unsupported executable debug dependency format: {binary}')


def _checkout_running(store: Path, namespace: str) -> bool:
    """Whether a cargo_run holds this checkout's lock; this process's own run counts.

    Probes cargo_run.file_lock's lock without creating it: a run creates the file
    before locking it, and the pass watches the store directory for changes.
    """
    try:
        handle = (store / 'checkouts' / f'{namespace}.lock').open('rb')
    except FileNotFoundError:
        return False
    with handle:
        try:
            if os.name == 'nt':
                import msvcrt
                msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
                fcntl.flock(handle, fcntl.LOCK_UN)
        except (BlockingIOError, PermissionError):
            return True
    return False


def _superseded(store: Path, roots, profiles, inventory, links, executables) -> set[Path]:
    """Cache executables an equally named newer one in its deps directory replaced.

    Cargo keeps every test and bin executable whose hash changed with features,
    flags or the toolchain. Each kept itself and every object its debug map names
    protected, so a pass under pressure met neither budget: on 2026-10-18 26 such
    Mach-O executables held 3.2 GiB plus 5.5 GiB of objects only they named. The
    newest of each name stays, as does any executable of a checkout whose run
    holds its lock: that run may execute its own recorded tests after Cargo.
    Windows executables and their PDBs stay unless a later change pairs them.
    """
    names = {}
    deps = {profile / 'deps' for profile in profiles}
    for path in executables:
        match = re.fullmatch(r'(.+)-[0-9a-f]{16}', path.name)
        if match and path.parent in deps and _magic(path) in MACH | {b'\x7fELF'}:
            names.setdefault((path.parent, match[1]), []).append(path)
    running = {}
    superseded = set()
    for paths in names.values():
        paths.sort(key=lambda path: (inventory[path].st_mtime_ns, path.name))
        for path in paths[:-1]:
            namespace = next(cache.name for cache in roots if path.is_relative_to(cache))
            if namespace not in running:
                running[namespace] = _checkout_running(store, namespace)
            if inventory[path].st_nlink == 1 and links.eligible(path) and not running[namespace]:
                superseded.add(path)
    return superseded


def trim_locked(root: Path, store: Path, policy: CachePolicy, *, dry_run=False) -> dict:
    """Called ONLY while cargo_run holds its build lock; never apply an old plan."""
    from tools.cargo_run import build_processes
    receipt = {'schema': 1, 'started_unix': time.time(), 'dry_run': dry_run,
               'policy': asdict(policy), 'state': 'blocked', 'errors': [], 'notes': [],
               'removed_files': [], 'removed_allocated_bytes': 0, 'removed_logical_bytes': 0}
    devices = {}
    receipt['degraded_debug_inputs'] = {}
    receipt['inspection_cache'] = {'hits': 0, 'misses': 0, 'failure_hits': 0}
    try:
        abandoned = _abandoned_roots(root, store)
        receipt['abandoned_roots'] = sorted(map(str, abandoned))
        if not dry_run:
            # Only resolve_binary reads a latest record, for its live checkout.
            for cache in sorted(abandoned):
                record = store / 'latest' / f'{cache.name}.json'
                if record.exists():
                    _regular(record)
                    record.unlink()
                    receipt['notes'].append(f'Removed the latest record of abandoned cache {cache}')
        receipt['cache_state'] = _cache_state(root, store)
        (store / 'inspections').mkdir(parents=True, exist_ok=True)
        _plain(store / 'inspections')
        revision = _inspection_revision()
        directory_states = {path: _directory_identity(path) for path in (
            store, store / 'artifacts', store / 'latest') if path.exists()}
        profiles = set()
        roots, binaries, watched = _inputs(root, store, profiles=profiles, notes=receipt['notes'])
        inventory = {path: info for cache in sorted(roots) for path, info in _files(cache, directory_states)}
        leftovers = {path: (cache, info) for cache in sorted(abandoned)
                     for path, info in _files(cache, directory_states)}
        inventory.update((path, info) for path, (_, info) in leftovers.items())
        total = _allocated_total(inventory.values())
        increments = {path for path in inventory if any(path.is_relative_to(
            profile / 'incremental') for profile in profiles)}
        incremental = _allocated_total(inventory[path] for path in increments)
        # An abandoned cache may be gone by the final measurement; its parent stays.
        devices = {cache.stat().st_dev: cache for cache in [
            *(cache.parent for cache in sorted(abandoned)), *sorted(roots)]}
        free = {str(device): shutil.disk_usage(cache).free for device, cache in devices.items()}
        receipt.update(roots=[str(path) for path in sorted(roots)], cache_allocated_bytes=total,
                       incremental_allocated_bytes=incremental, free_before=free,
                       selected_files=[], protected_files=0)
        pressure = bool(leftovers) or total > policy.cache_bytes or incremental > policy.incremental_bytes or any(
            count < policy.min_free_bytes for count in free.values())
        if not pressure:
            receipt['state'] = 'under_budget'
        else:
            preserved_checks = {}
            verified_roots, verified_binaries, watched = _inputs(
                root, store, preserved_checks=preserved_checks, notes=receipt['notes'])
            if verified_roots != roots or verified_binaries != binaries:
                raise ValueError('Build registry changed during cache inventory')
            # Watch discovery directories too: a newly published label/bin may
            # introduce a dependency not present in the original inventory.
            directories = {path.parent for path in watched}
            directories.update(path for path in (store / 'artifacts', store / 'latest') if path.exists())
            for path in directories:
                directory_states.setdefault(path, _directory_identity(path))
            published = set(binaries)
            # Include unlabelled lib tests, examples and build-script executables.
            # No dependence on Unix executable bits (Windows does not have them).
            for path, info in inventory.items():
                # An abandoned cache's own executables protect nothing; it goes whole.
                if path in leftovers or path.suffix in {
                        '.o', '.obj', '.rlib', '.rmeta', '.a', '.bin', '.pdb', '.dwo', '.dwp'}:
                    continue
                if _magic(path) in MACH | {b'\x7fELF'} or _magic(path)[:2] == b'MZ':
                    binaries.add(path)
            links = _CacheLinks(inventory, increments)
            superseded = _superseded(store, roots, profiles, inventory, links, binaries - published)
            receipt['superseded_executables'] = sorted(map(str, superseded))
            protected = binaries - superseded
            needed = {}  # Objects only superseded executables name, and those executables.
            # Immutable validation inputs first: an already broken label can
            # stop safely before scanning numerous rebuildable cache binaries.
            for binary in sorted(binaries, key=lambda path: (path not in preserved_checks, str(path))):
                watched[binary] = _identity(binary)
                missing = []
                for dependency in _inspect_cached(store, binary, revision,
                                                  preserved_checks.get(binary), receipt['inspection_cache']):
                    previously_seen = dependency in watched
                    identity = watched[dependency] if previously_seen else _dependency_identity(dependency)
                    watched[dependency] = identity
                    if identity is None:
                        missing.append(str(dependency))
                    else:
                        if not previously_seen:
                            with dependency.open('rb') as source:
                                if source.read(8) == b'!<thin>\n':
                                    raise ValueError(f'Unsupported thin-archive debug closure: {dependency}')
                        if binary in superseded:
                            needed.setdefault(dependency, set()).add(binary)
                        else:
                            protected.add(dependency)
                if missing:
                    receipt['degraded_debug_inputs'][str(binary)] = sorted(missing)
            receipt['protected_files'] = len(protected)
            # Each group: paths, incremental, age, newest session, executables first removed.
            groups = [([path], False, inventory[path].st_mtime_ns, 0, frozenset())
                      for path in sorted(superseded - protected)]
            # Only exact Cargo profile/deps *.rcgu.o; never arbitrary *.o.
            for path, info in inventory.items():
                if (path.name.endswith('.rcgu.o') and path.parent in {profile / 'deps' for profile in profiles}
                        and links.eligible(path) and path not in protected):
                    groups.append(([path], False, info.st_mtime_ns, 0, frozenset(needed.get(path, ()))))
            # Cargo output of an abandoned cache goes first, whatever the budgets.
            for path, (cache, info) in leftovers.items():
                if _abandoned_output(cache, path) and links.eligible(path) and path not in protected:
                    groups.append(([path], False, info.st_mtime_ns, -1, frozenset()))
            sessions = {}
            for path in increments:
                # Cargo incremental/<crate-hash>/s-<session>/{objects,*.bin}.
                parents = path.parents
                session = next((parent for parent in parents if parent.name.startswith('s-')
                                and parent.parent.parent in {profile / 'incremental' for profile in profiles}), None)
                if session:
                    sessions.setdefault(session, []).append(path)
            newest = {}
            for session, paths in sessions.items():
                age = max(inventory[path].st_mtime_ns for path in paths)
                previous = newest.get(session.parent)
                if previous is None or (age, str(session)) > (previous[0], str(previous[1])):
                    newest[session.parent] = (age, session)
            for session, paths in sessions.items():
                # rustc publishes complete immutable sessions and reuses objects
                # through hardlinks. Evict the full finalized cache, preserving
                # required deps paths even when they share its object inodes.
                # https://doc.rust-lang.org/stable/nightly-rustc/src/rustc_incremental/persist/fs.rs.html
                if session.name.endswith('-working'):
                    continue
                if any(path in protected or not links.eligible(path)
                       or not (path.suffix == '.o' or path.name in {
                           'dep-graph.bin', 'dep-graph.part.bin', 'query-cache.bin',
                           'work-products.bin', 'metadata.rmeta'}) for path in paths):
                    continue
                # Links/special entries are not in inventory: don't partially
                # delete a session containing any such unknown input.
                entries = list(session.rglob('*'))
                if any(path.is_symlink() or getattr(path.lstat(), 'st_file_attributes', 0) & 0x400
                       or not (path.is_file() or path.is_dir()) for path in entries):
                    continue
                if set(paths) != {path for path in entries if path.is_file()}:
                    continue
                groups.append((sorted(paths), True, max(inventory[path].st_mtime_ns for path in paths),
                               int(newest[session.parent][1] == session),
                               frozenset().union(*(needed.get(path, ()) for path in paths))))
            # Hottest sessions remain useful until cold caches cannot meet pressure.
            # An object an older executable names sorts after it, never alone first.
            groups.sort(key=lambda group: (
                group[3], max([group[2], *(inventory[path].st_mtime_ns for path in group[4])]),
                bool(group[4]), str(group[0][0])))
            # A short reserve is restored with headroom; see CachePolicy.
            free_target = policy.min_free_bytes + (policy.free_headroom_bytes if any(
                count < policy.min_free_bytes for count in free.values()) else 0)
            active_since = time.time_ns() - ACTIVE_SESSION_SECONDS * 1_000_000_000

            def wanted(is_incremental, age, newest, total_now, incremental_now, free_now):
                if newest < 0 or free_now < free_target:
                    return True
                if newest and age >= active_since:
                    return False  # Recent work's newest session; see ACTIVE_SESSION_SECONDS.
                return total_now > policy.cache_bytes or (
                    is_incremental and incremental_now > policy.incremental_bytes)

            receipt['removable_groups'] = sum(
                wanted(is_incremental, age, newest, total, incremental, free[str(inventory[paths[0]].st_dev)])
                for paths, is_incremental, age, newest, _ in groups)
            selected, projected_free, projected_total, projected_incremental = [], dict(free), total, incremental
            projection, chosen = _CacheLinks(inventory, increments), set()
            for paths, is_incremental, age, newest, after in groups:
                device = str(inventory[paths[0]].st_dev)
                if not wanted(is_incremental, age, newest, projected_total,
                              projected_incremental, projected_free[device]):
                    continue
                if not after <= chosen:
                    continue
                selected.append((paths, is_incremental))
                chosen.update(paths)
                for path in paths:
                    released, incremental_released = projection.remove(path)
                    projected_total -= released
                    projected_incremental -= incremental_released
                    projected_free[device] += released
            receipt['selected_files'] = [str(path) for paths, _ in selected for path in paths]
            receipt['projected_removed_allocated_bytes'] = total - projected_total
            receipt['projected_remaining_cache_bytes'] = projected_total
            receipt['projected_remaining_incremental_bytes'] = projected_incremental
            receipt['projected_free_after'] = projected_free
            # Complete preflight before FIRST deletion. Kernel lock serializes
            # cooperating tasks; stat checks also detect external mutation.
            if build_processes():
                raise ValueError('Unwrapped Cargo/rustc became active; no deletion')
            # A cache abandoned since inventory waits for the next pass.
            if not abandoned <= _abandoned_roots(root, store):
                raise ValueError('Abandoned cache ownership changed during inspection')
            for path, identity in watched.items():
                if _dependency_identity(path) != identity:
                    raise ValueError(f'Debug dependency changed during inspection: {path}')
            # Preflight every eligible fallback too: APFS clones/snapshots can
            # reclaim less than allocated bytes, requiring further cold entries.
            candidate_paths = {path for paths, *_ in groups for path in paths}
            # Every known alias participates in preflight, including required debug
            # paths which must survive removal of a sibling incremental alias.
            alias_paths = {alias for path in candidate_paths
                           for alias in links.aliases[links.keys[path]]}
            candidates = {path: _identity(path) for path in alias_paths}
            for path, identity in candidates.items():
                info = inventory[path]
                if identity != (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns, info.st_nlink):
                    raise ValueError(f'Compiler cache changed during inspection: {path}')
            receipt['state'] = 'planned' if dry_run else 'trimmed'
            for path, identity in directory_states.items():
                if _directory_identity(path) != identity:
                    raise ValueError(f'Cache/build discovery directory changed: {path}')
            if not dry_run:
                receipt['selected_files'] = []
                remaining_total, remaining_incremental, checked_at = total, incremental, 0.0
                removed = set()
                for paths, is_incremental, age, newest, after in groups:
                    device = inventory[paths[0]].st_dev
                    actual_free = shutil.disk_usage(devices[device]).free
                    if not wanted(is_incremental, age, newest, remaining_total,
                                  remaining_incremental, actual_free):
                        continue
                    if not after <= removed:
                        continue
                    if time.monotonic() - checked_at >= 1:
                        if build_processes():
                            raise ValueError('Unwrapped Cargo/rustc became active; stopping deletion')
                        if abandoned and not abandoned <= _abandoned_roots(root, store):
                            raise ValueError('Abandoned cache ownership changed before deletion')
                        for path, identity in watched.items():
                            if _dependency_identity(path) != identity:
                                raise ValueError(f'Debug dependency changed before deletion: {path}')
                        # Cache parents change from our unlinks; label/latest
                        # directories do not. Keep checking those publications.
                        for path, identity in directory_states.items():
                            if not any(path.is_relative_to(cache) for cache in roots | abandoned) and _directory_identity(path) != identity:
                                raise ValueError(f'Build publication changed before deletion: {path}')
                        checked_at = time.monotonic()
                    receipt['selected_files'].extend(str(path) for path in paths)
                    for path in paths:
                        key = links.keys[path]
                        # Validate the entire inode closure before each unlink.
                        # Checking only this path would miss mutation of a retained
                        # alias after a previous removal changed inode ctime/nlink.
                        survivors = links.remaining[key] - {path}
                        for alias in links.remaining[key]:
                            if _identity(alias) != candidates[alias]:
                                raise ValueError(f'Compiler cache changed before unlink: {alias}')
                        prior = candidates[path]
                        path.unlink()
                        removed.add(path)
                        # An older executable and its objects are inspected inputs.
                        watched.pop(path, None)
                        receipt['removed_files'].append(str(path))
                        released, incremental_released = links.remove(path)
                        receipt['removed_allocated_bytes'] += released
                        receipt['removed_logical_bytes'] += inventory[path].st_size
                        remaining_total -= released
                        remaining_incremental -= incremental_released
                        receipt['remaining_cache_allocated_bytes'] = remaining_total
                        receipt['remaining_incremental_allocated_bytes'] = remaining_incremental
                        # Own unlinks change ctime/nlink of every surviving alias.
                        # Accept exactly that scoped transition, never changes to
                        # inode, contents metadata or an unexpected link count.
                        updates = {}
                        for alias in survivors:
                            identity = _identity(alias)
                            if identity[:4] != prior[:4] or identity[5] != prior[5] - 1:
                                raise ValueError(f'Compiler cache alias changed after unlink: {alias}')
                            updates[alias] = identity
                        for alias, identity in updates.items():
                            candidates[alias] = identity
                            if alias in watched:
                                watched[alias] = identity
                    # Remove empty session directories only. Never recursive rmtree.
                    if paths and paths[0].parent.name.startswith('s-'):
                        try:
                            paths[0].parent.rmdir()
                        except OSError:
                            pass
                projected_total, projected_incremental = remaining_total, remaining_incremental
                # Empty directories of abandoned caches go too, deepest first (never
                # rmtree); a cache left empty leaves the registry.
                emptied = set()
                for cache in sorted(abandoned):
                    for directory, _, _ in os.walk(cache, topdown=False):
                        try:
                            os.rmdir(directory)
                        except OSError:
                            pass
                    if not cache.exists():
                        emptied.add(str(cache))
                if emptied:
                    registry = store / 'cache-roots.json'
                    data = _json(registry)
                    data['roots'] = {name: entry for name, entry in data['roots'].items() if name not in emptied}
                    _write(registry, data)
                    receipt['removed_abandoned_roots'] = sorted(emptied)
            receipt['free_after'] = {str(device): shutil.disk_usage(cache).free
                                     for device, cache in devices.items()}
            receipt['unmet_targets'] = {
                'cache_bytes': max(0, projected_total - policy.cache_bytes),
                'incremental_bytes': max(0, projected_incremental - policy.incremental_bytes),
                'free_bytes': {key: max(0, policy.min_free_bytes - count)
                               for key, count in (projected_free if dry_run else receipt['free_after']).items()}}
    except (OSError, ValueError, subprocess.SubprocessError, UnicodeError) as error:
        receipt['state'] = 'partial' if receipt['removed_files'] else 'blocked'
        receipt['errors'].append(str(error))
    if devices:
        measured = {}
        for device, cache in devices.items():
            try:
                measured[str(device)] = shutil.disk_usage(cache).free
            except OSError as error:
                measured[str(device)] = None
                receipt['errors'].append(f'Final free-space measurement unavailable: {error}')
                receipt['state'] = 'partial' if receipt['removed_files'] else 'blocked'
        receipt['free_after'] = measured
        receipt['observed_free_delta_bytes'] = {
            device: count - receipt['free_before'][device]
            if count is not None and device in receipt.get('free_before', {}) else None
            for device, count in measured.items()}
    receipt['finished_unix'] = time.time()
    receipt['notes'] = list(dict.fromkeys(receipt['notes']))
    path = store / 'retention' / f'{time.time_ns()}-{uuid.uuid4().hex[:8]}.json'
    _write(path, receipt)
    receipt['receipt_path'] = str(path)
    cutoff = time.time_ns() - RECEIPT_SECONDS * 1_000_000_000
    for name in os.listdir(path.parent):
        if (match := re.fullmatch(r'(\d+)-[0-9a-f]{8}\.json', name)) and int(match[1]) < cutoff:
            try:
                (path.parent / name).unlink()
            except OSError:
                pass  # The pass is complete; an old receipt can go next time.
    return receipt


def trim(root: Path, policy: CachePolicy, timeout: float, *, dry_run=False) -> dict:
    from tools.cargo_run import build_lock, build_store
    store, _ = build_store(root)
    with build_lock(store / 'cargo.lock', timeout):
        return trim_locked(root, store, policy, dry_run=dry_run)


# The soft budgets are enforced after a build at most this often; see retention_due.
# They no longer take recent work's sessions, and a short reserve always gets a
# pass, so an interval pass reclaims only cold caches.
AUTOMATIC_INTERVAL_SECONDS = 2 * 60 * 60


def _last_applied_pass(store: Path) -> tuple[Path, dict, float] | None:
    """Newest receipt that was not a dry run, with its finish time; None if unknown."""
    directory = store / 'retention'
    try:
        names = [name for name in os.listdir(directory) if re.fullmatch(r'\d+-[0-9a-f]{8}\.json', name)]
        for name in sorted(names, key=lambda name: int(name.split('-')[0]), reverse=True):
            receipt = json.loads((directory / name).read_text())
            if receipt['dry_run'] is False:
                return directory / name, receipt, float(receipt['finished_unix'])
    except (OSError, ValueError, KeyError, TypeError):
        pass
    return None


def _cache_state(root: Path, store: Path) -> str:
    """Digest of every input that decides which files a pass may remove.

    Removability depends on the registered roots and profiles, the protected
    executables and the cache trees, never on the budgets or free space. Creating,
    removing or relinking a cache file changes its directory's identity.
    """
    profiles = set()
    roots, binaries, _ = _inputs(root, store, profiles=profiles)
    state = [sorted(map(str, roots)), sorted(map(str, profiles)),
             sorted((str(path), _identity(path)) for path in binaries)]
    directories = []
    abandoned = _abandoned_roots(root, store)
    state.append(sorted(map(str, abandoned)))
    for cache in sorted(roots | abandoned):
        _plain(cache)
        pending = [cache]
        while pending:
            base = pending.pop()
            info = base.lstat()
            directories.append((str(base), info.st_dev, info.st_ino, info.st_mtime_ns, info.st_ctime_ns))
            with os.scandir(base) as entries:
                for entry in entries:
                    # The directories _files walks: no links or reparse points.
                    if entry.is_dir(follow_symlinks=False):
                        path = Path(entry.path)
                        if not getattr(path.lstat(), 'st_file_attributes', 0) & 0x400:
                            pending.append(path)
    state.append(sorted(directories))
    return hashlib.sha256(json.dumps(state).encode()).hexdigest()


def _futile_pass(root: Path, store: Path) -> Path | None:
    """Receipt of a recent pass whose result a new pass would only repeat.

    That pass found nothing removable, and nothing that decides removability has
    changed since. The interval bounds trust in the digest, as for the budgets.
    """
    if (last := _last_applied_pass(store)) is None:
        return None
    path, receipt, finished = last
    if (receipt.get('state') != 'trimmed' or receipt.get('errors') != []
            or receipt.get('removable_groups') != 0
            or not 0 <= time.time() - finished < AUTOMATIC_INTERVAL_SECONDS):
        return None
    try:
        return path if receipt.get('cache_state') == _cache_state(root, store) else None
    except (OSError, ValueError, subprocess.SubprocessError, UnicodeError):
        return None


def retention_due(store: Path, policy: CachePolicy, volumes) -> bool:
    """Whether a pass after a build can matter now.

    A pass inventories every registered cache, which takes seconds and grows with
    each worktree, so the soft budgets are enforced at most once per interval. A
    volume below the reserve, or one that cannot be measured, always gets a pass.
    """
    try:
        if any(shutil.disk_usage(volume).free < policy.min_free_bytes for volume in volumes):
            return True
    except OSError:
        return True
    last = _last_applied_pass(store)
    return last is None or not 0 <= time.time() - last[2] < AUTOMATIC_INTERVAL_SECONDS


def automatic_locked(root: Path, store: Path, policy: CachePolicy):
    # Below the reserve every build requests a pass before and after Cargo. Once
    # the rest of the cache is protected, each pass inventoried it again to remove
    # nothing: on 2026-10-18 ten such passes of 78-140 s held the shared lock
    # between 09:35 and 11:15 while builds queued. Admission still measures.
    if futile := _futile_pass(root, store):
        print(f'Cache retention: skipped; no removable cache since {futile}',
              file=sys.stderr, flush=True)
        return
    receipt = trim_locked(root, store, policy)
    print(f"Cache retention: {receipt['state']}, removed {receipt['removed_allocated_bytes']} "
          f"allocated bytes; {receipt['receipt_path']}", file=sys.stderr, flush=True)
    if receipt.get('degraded_debug_inputs'):
        print(f"Saved/live builds with already missing debug inputs: {len(receipt['degraded_debug_inputs'])}; see receipt",
              file=sys.stderr, flush=True)
    if receipt['errors']:
        print('Cache deletion stopped: ' + receipt['errors'][0].splitlines()[0][:500],
              file=sys.stderr, flush=True)
