"""Serialize Cargo across worktrees and preserve identified build artifacts.

See tools/README.md. Standard library only; Python >= 3.11.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time


def git(root: Path, *args: str) -> str:
    return subprocess.check_output(['git', '-C', str(root), *args], text=True).strip()


def build_processes() -> list[str]:
    """Observe unwrapped builds too; inspection failure must not mean idle."""
    if os.name == 'nt':
        result = subprocess.run([
            'powershell', '-NoProfile', '-NonInteractive', '-Command',
            "Get-Process cargo,rustc -ErrorAction SilentlyContinue | "
            "ForEach-Object { '{0} {1}' -f $_.Id,$_.ProcessName }; exit 0",
        ], check=True, capture_output=True, text=True)
        return result.stdout.splitlines()
    result = subprocess.run(['ps', '-A', '-o', 'pid=,comm='],
                            check=True, capture_output=True, text=True)
    return [line.strip() for line in result.stdout.splitlines()
            if len(parts := line.strip().split(None, 1)) == 2
            and Path(parts[1]).name in {'cargo', 'rustc'}]


@contextmanager
def build_lock(path: Path, timeout: float):
    """Kernel lock releases on exit/crash; never unlink an active lock inode."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('a+b') as handle:
        handle.seek(0, os.SEEK_END)
        if handle.tell() == 0:
            handle.write(b'\0')
            handle.flush()
        deadline = time.monotonic() + timeout
        report_at = 0.0
        while True:
            try:
                handle.seek(0)
                if os.name == 'nt':
                    import msvcrt
                    msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
                else:
                    import fcntl
                    fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except (BlockingIOError, PermissionError):
                if time.monotonic() >= deadline:
                    raise TimeoutError('Timed out waiting for another cargo_run owner')
                if time.monotonic() >= report_at:
                    print('Waiting for another cargo_run owner...', file=sys.stderr, flush=True)
                    report_at = time.monotonic() + 30
                time.sleep(0.25)
        try:
            while active := build_processes():
                if time.monotonic() >= deadline:
                    raise TimeoutError('Timed out waiting for Cargo/rustc: ' + ', '.join(active))
                if time.monotonic() >= report_at:
                    print('Waiting for Cargo/rustc: ' + ', '.join(active), file=sys.stderr, flush=True)
                    report_at = time.monotonic() + 30
                time.sleep(0.25)
            yield
        finally:
            handle.seek(0)
            if os.name == 'nt':
                import msvcrt
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl
                fcntl.flock(handle, fcntl.LOCK_UN)


# A file written this close to a fingerprint may change again without its
# timestamp moving: FAT rounds timestamps to two seconds, HFS+ to one.
RACY_NS = 2_000_000_000


def source_identity(root: Path, hashes: dict | None = None) -> dict:
    """Fingerprint tracked and nonignored untracked files, including local edits.

    `hashes` carries each file's digest to a later call in the same run. That call
    reads again only files whose size, timestamps or file ID changed, or whose
    timestamp was too close to the earlier call to prove there was no later write.
    A same-size write that leaves all of those unchanged (a memory-mapped write on
    Windows, a tool that restores timestamps) is therefore not seen; labelled
    builds pass no table and compare contents.
    """
    started = time.time_ns()
    names = subprocess.check_output([
        'git', '-C', str(root), 'ls-files', '--cached', '--others', '--exclude-standard', '-z',
    ]).split(b'\0')
    digest = hashlib.sha256()
    for raw in sorted(set(names) - {b''}):
        path = root / os.fsdecode(raw)
        digest.update(raw + b'\0')
        if path.is_file():
            info = path.stat()
            stamp = (info.st_size, info.st_mtime_ns, info.st_ctime_ns, info.st_dev, info.st_ino)
            known = hashes.get(raw) if hashes is not None else None
            if known is None or known[0] != stamp or info.st_mtime_ns > known[2] - RACY_NS:
                # Stamp before reading: a write during the read then shows as a change.
                known = (stamp, hashlib.sha256(path.read_bytes()).digest(), started)
                if hashes is not None:
                    hashes[raw] = known
            digest.update(b'file\0' + known[1])
        elif path.is_symlink():
            digest.update(b'link\0' + os.fsencode(os.readlink(path)))
        elif path.exists():
            raise ValueError(f'Unsupported source entry (submodule/directory): {path}')
        else:
            digest.update(b'deleted\0')
    return {'head': git(root, 'rev-parse', 'HEAD'),
            'branch': git(root, 'rev-parse', '--abbrev-ref', 'HEAD'),
            'status': git(root, 'status', '--porcelain=v1', '--untracked-files=all'),
            'source_sha256': digest.hexdigest()}


def cargo_args(args: list[str], label: str | None) -> list[str]:
    if not args or args[0] not in {'build', 'test', 'check', 'clippy'}:
        raise ValueError('Expected Cargo build, test, check or clippy after --')
    options = args[:args.index('--')] if '--' in args else args
    if any(a.split('=')[0] in {'--target-dir', '--manifest-path', '--message-format', '--config'}
           for a in options):
        raise ValueError('cargo_run owns target-dir, manifest-path, config and message-format')
    if args[0] == 'test' and '--lib' not in options:
        raise ValueError('Repository tests must select --lib')
    if label and not (args[0] == 'build' or (args[0] == 'test' and '--no-run' in options)):
        raise ValueError('--label requires build or test --lib --no-run')
    return [args[0], '--message-format=json-render-diagnostics', *args[1:]]


def build_store(root: Path) -> tuple[Path, str]:
    common = Path(git(root, 'rev-parse', '--path-format=absolute', '--git-common-dir'))
    return common / 'owned-builds', hashlib.sha256(os.fsencode(root.resolve())).hexdigest()[:16]


def resolve_binary(root: Path, name: str, profile: str | None = None) -> tuple[Path | None, str | None]:
    """Discover only unchanged host executables reported by successful owned builds.

    Read per call so a long-running MCP server notices new builds, even when its
    environment differs from the build shell. Never guess a conventional target.
    An explicit profile never falls back. Omitted profile preserves the MCP's
    release-first policy. Byte identity does not establish source freshness.
    """
    if profile not in (None, 'release', 'debug'):
        raise ValueError('Expected release or debug host profile')
    store, namespace = build_store(root)
    record = store / 'latest' / f'{namespace}.json'
    if not record.exists():
        return None, None
    records = json.loads(record.read_text())
    for candidate in (profile,) if profile else ('release', 'debug'):
        if entry := records.get(candidate, {}).get(name):
            path = Path(entry['path'])
            if path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() == entry['sha256']:
                return path, candidate
    return None, None


def validate_label(label: str) -> None:
    if not isinstance(label, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]{0,79}', label):
        raise ValueError('Label must be 1-80 ASCII letters, digits, dots, dashes or underscores')


def _artifact_parts(relative: str) -> list[str]:
    if (not isinstance(relative, str) or not relative or '\\' in relative or ':' in relative
            or PureWindowsPath(relative).drive
            or PurePosixPath(relative).is_absolute()
            or any(part in ('', '.', '..') for part in relative.split('/'))):
        raise ValueError(f'Invalid preserved artifact path: {relative!r}')
    return relative.split('/')


def _label_path(base: Path, relative: str) -> Path:
    """Require canonical relative artifact paths without symlinks or junctions.

    Labels are shared by worktrees in the Git common directory. That directory
    is the trusted anchor; preserved files may not redirect lookup outside it.
    Windows reparse attributes cover junctions on Python 3.11 as well.
    """
    current = base
    for part in _artifact_parts(relative):
        current /= part
        info = current.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 0x400:
            raise ValueError(f'Preserved artifact path contains a link or reparse point: {current}')
    if not current.resolve().is_relative_to(base):
        raise ValueError(f'Preserved artifact path escapes its store: {current}')
    return current


def _manifest_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f'Duplicate preserved manifest field: {key}')
        result[key] = value
    return result


def host_bin_profile(target: Path, executable: Path) -> str | None:
    """Classify Cargo's retained path, without resolving mutable cache paths."""
    for profile in ('release', 'debug'):
        if executable.parent == target / profile:
            return profile
    return None


def preserved_manifest(store: Path, label: str) -> tuple[Path, Path, list[dict]]:
    """Validate the shared schema-1 owner; selection and retention use it alike."""
    validate_label(label)
    anchor = store.parent.resolve()
    relative_label = f'{store.name}/artifacts/{label}'
    directory = _label_path(anchor, relative_label)
    if not directory.is_dir():
        raise ValueError(f'Preserved label is not a directory: {directory}')
    manifest_path = _label_path(anchor, relative_label + '/manifest.json')
    if not manifest_path.is_file():
        raise ValueError('Preserved label manifest is not a regular file')
    manifest = json.loads(manifest_path.read_text(), object_pairs_hook=_manifest_object)
    if not isinstance(manifest, dict) or type(manifest.get('schema')) is not int or manifest['schema'] != 1:
        raise ValueError('Expected preserved build manifest schema 1')
    target_text = manifest.get('target_dir')
    if not isinstance(target_text, str) or not Path(target_text).is_absolute() or '..' in Path(target_text).parts:
        raise ValueError('Preserved manifest needs an absolute original target_dir')
    artifacts = manifest.get('artifacts')
    if not isinstance(artifacts, list):
        raise ValueError('Preserved manifest artifacts must be a list')
    for entry in artifacts:
        if (not isinstance(entry, dict) or not isinstance(entry.get('file'), str)
                or not isinstance(entry.get('source'), str)
                or not isinstance(entry.get('sha256'), str)
                or not re.fullmatch(r'[0-9a-f]{64}', entry['sha256'])):
            raise ValueError('Malformed preserved executable record')
        _artifact_parts(entry['file'])
    return directory, Path(target_text), artifacts


def preserved_artifact(directory: Path, entry: dict) -> Path:
    path = _label_path(directory.resolve(), entry['file'])
    if not stat.S_ISREG(path.stat().st_mode):
        raise ValueError(f'Preserved executable is not a regular file: {path}')
    if hashlib.sha256(path.read_bytes()).hexdigest() != entry['sha256']:
        raise ValueError(f'Preserved executable SHA-256 mismatch: {path}')
    return path


def resolve_labeled_binary(root: Path, label: str, name: str,
                           profile: str) -> tuple[Path | None, str | None]:
    """Resolve one unchanged preserved host bin; never use latest or cache paths.

    Schema 1 retains Cargo's original executable path and owned target root.
    An exact target/profile parent is the same host-bin classification used by
    publish_binaries: dependency/test binaries, examples and cross-target paths
    do not qualify. Original build outputs need no longer exist; only the
    preserved bytes are opened. This proves recorded byte identity, not source
    freshness, executable format, or a hermetic/authenticated build.
    """
    validate_label(label)
    if profile not in ('release', 'debug'):
        raise ValueError('Expected release or debug host profile')
    if not isinstance(name, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]*', name):
        raise ValueError('Expected an executable basename, not a path')
    store, _ = build_store(root)
    try:
        directory, target, artifacts = preserved_manifest(store, label)
    except FileNotFoundError:
        if not (store / 'artifacts' / label).exists():
            return None, None
        raise
    expected = name + '.exe' if os.name == 'nt' else name
    matches = []
    for entry in artifacts:
        relative = entry['file']
        if PurePosixPath(relative).name == expected:
            matches.append(entry)
    if not matches:
        return None, None
    if len(matches) != 1:
        raise ValueError(f'Ambiguous preserved executable basename: {expected}')
    entry, = matches
    source = Path(entry['source'])
    if (not source.is_absolute() or '..' in source.parts
            or host_bin_profile(target, source) != profile or source.name != expected):
        raise ValueError(f'Preserved executable is not a {profile} host bin: {entry["source"]}')
    path = preserved_artifact(directory, entry)
    return path, profile


def publish_binaries(store: Path, namespace: str, target: Path, artifacts: set[Path]):
    """Record native host release/debug bins; test/dependency/cross-target bins are excluded."""
    record = store / 'latest' / f'{namespace}.json'
    records = json.loads(record.read_text()) if record.exists() else {}
    for path in artifacts:
        if profile := host_bin_profile(target, path):
            name = path.stem if path.suffix == '.exe' else path.name
            records.setdefault(profile, {})[name] = {
                'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
            }
    record.parent.mkdir(parents=True, exist_ok=True)
    # Held build lock serializes writers; atomic replace protects concurrent readers.
    pending = record.with_suffix('.pending')
    pending.write_text(json.dumps(records, indent=2) + '\n')
    pending.replace(record)


def run(root: Path, args: list[str], label: str | None, timeout: float, *, policy=None) -> int:
    from tools._cargo_cache import CachePolicy, register_locked, automatic_locked, retention_due
    policy = policy if policy is not None else CachePolicy.from_env()
    args = cargo_args(args, label)
    if label:
        validate_label(label)
    store, namespace = build_store(root)
    # A target root may be shared, but never its package fingerprints/artifacts.
    cache_root = Path(os.environ.get('CARGO_TARGET_DIR', str(root / 'target'))).resolve()
    target = cache_root / 'owned-worktrees' / namespace
    output = store / 'artifacts' / label if label else None
    with build_lock(store / 'cargo.lock', timeout):
        if output and output.exists():
            raise ValueError(f'Label already exists; choose a new label: {output}')
        register_locked(root, store, target)
        # Check both paths even if platform device identifiers are unavailable
        # or collide; an artifact copy may consume a different volume.
        volumes = [target, store] if output else [target]

        def below_reserve():
            return [(volume, free) for volume in volumes
                    if (free := shutil.disk_usage(volume).free) < policy.min_free_bytes]

        # A retention pass inventories every registered cache, which takes seconds
        # and grows with each worktree. Before Cargo it can matter only by
        # restoring the reserve, and the reserve is cheap to measure.
        if below_reserve():
            automatic_locked(root, store, policy)
        # Admission uses a fresh measurement after cleanup, not projected file
        # allocation or a retention receipt. A protected cache must never cause
        # another compile to consume the remaining volume reserve.
        if short := below_reserve():
            volume, available = short[0]
            raise ValueError(
                f'Build blocked: {available / (1024 ** 3):.2f} GiB free at {volume}; '
                f'{policy.min_free_bytes / (1024 ** 3):.2f} GiB required. '
                'Review owned superseded builds and the cache retention receipt '
                'before retrying; protected files were preserved.')
        artifacts = set()
        try:
            hashes = {}
            before = source_identity(root, hashes)
            env = dict(os.environ, CARGO_TARGET_DIR=str(target))
            command = ['cargo', *args]
            print(f'Checkout: {root}\nTarget: {target}\nCommand: {command}', file=sys.stderr, flush=True)
            with subprocess.Popen(command, cwd=root, env=env, stdout=subprocess.PIPE,
                                  text=True, encoding='utf-8', errors='replace') as child:
                assert child.stdout is not None
                for line in child.stdout:
                    try:
                        message = json.loads(line)
                    except ValueError:
                        print(line, end='', flush=True)
                        continue
                    if not isinstance(message, dict):
                        print(line, end='', flush=True)
                    elif message.get('reason') == 'compiler-artifact' and message.get('executable'):
                        artifacts.add(Path(message['executable']))
                    elif message.get('reason') == 'compiler-message':
                        print(message['message'].get('rendered', line), end='', file=sys.stderr, flush=True)
                    elif message.get('reason') not in {'compiler-artifact', 'build-script-executed', 'build-finished'}:
                        print(line, end='', flush=True)
                result = child.wait()
            if result:
                return result
            # A label's manifest records this identity: compare contents exactly
            # there. Elsewhere the stat table is enough; see source_identity.
            after = source_identity(root, None if label else hashes)
            if before != after:
                raise ValueError('Source changed during Cargo; result is not a labeled validation')
            publish_binaries(store, namespace, target, artifacts)
            if output:
                if not artifacts:
                    raise ValueError('Cargo succeeded but emitted no executable to preserve')
                output.parent.mkdir(parents=True, exist_ok=True)
                with tempfile.TemporaryDirectory(prefix='.pending-', dir=output.parent) as staging_name:
                    staging = Path(staging_name)
                    records = []
                    for i, path in enumerate(sorted(artifacts)):
                        # Cargo JSON, not guessed target paths, establishes the produced executable.
                        if not path.resolve().is_relative_to(target.resolve()):
                            raise ValueError(f'Cargo artifact outside owned target: {path}')
                        destination = staging / str(i) / path.name
                        destination.parent.mkdir()
                        shutil.copy2(path, destination)
                        records.append({'source': str(path), 'file': destination.relative_to(staging).as_posix(),
                                        'sha256': hashlib.sha256(destination.read_bytes()).hexdigest()})
                    manifest = {'schema': 1, 'checkout': str(root), 'source': before,
                                'command': command, 'target_dir': str(target),
                                'rustc': subprocess.check_output(['rustc', '-Vv'], text=True),
                                'cargo': subprocess.check_output(['cargo', '-V'], text=True),
                                'build_environment': {key: env[key] for key in (
                                    'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC',
                                    'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
                                    'RUSTUP_TOOLCHAIN', 'CARGO_BUILD_TARGET',
                                ) if key in env},
                                'artifacts': records}
                    (staging / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
                    staging.rename(output)
                print(f'Preserved build: {output}', flush=True)
        finally:
            register_locked(root, store, target, artifacts)
            if retention_due(store, policy, volumes):
                automatic_locked(root, store, policy)
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--label', help='Preserve executables and source manifest under a unique label')
    mode.add_argument('--resolve', metavar='BIN', help='Print a verified recorded host executable path; never builds')
    mode.add_argument('--trim-cache', action='store_true', help='Trim owned compiler caches without building')
    mode.add_argument('--retire-label', action='append', metavar='LABEL',
                      help='Retire an exact saved build; repeat for multiple labels')
    mode.add_argument('--retire-export-plan', metavar='PLAN',
                      help='Retire reviewed exact legacy Rust libtest exports from a pinned JSON plan')
    parser.add_argument('--dry-run', action='store_true', help='Preview cache or saved/exported build retirement')
    parser.add_argument('--cache-gib', help='Total compiler cache budget (default/env: 32 GiB)')
    parser.add_argument('--incremental-gib', help='Incremental cache budget (default/env: 4 GiB)')
    parser.add_argument('--min-free-gib', help='Minimum volume free-space target (default/env: 16 GiB)')
    parser.add_argument('--from-label', metavar='LABEL', help='Resolve from one preserved build label, not latest')
    parser.add_argument('--profile', choices=('release', 'debug'), help='Required with --resolve; no fallback')
    parser.add_argument('--wait-seconds', type=float,
                        help='Maximum build-owner wait (default: 3600)')
    parser.add_argument('cargo', nargs=argparse.REMAINDER)
    options = parser.parse_args(argv)
    cache_options = any(value is not None for value in (
        options.cache_gib, options.incremental_gib, options.min_free_gib))
    if options.resolve is not None:
        if not options.profile or options.cargo or options.wait_seconds is not None or cache_options or options.dry_run:
            parser.error('--resolve requires --profile and accepts no Cargo arguments or --wait-seconds')
    elif options.profile or options.from_label is not None:
        parser.error('--profile and --from-label are only valid with --resolve')
    if options.dry_run and not (options.trim_cache or options.retire_label or options.retire_export_plan):
        parser.error('--dry-run requires --trim-cache, --retire-label or --retire-export-plan')
    if (options.retire_label or options.retire_export_plan) and (options.cargo or cache_options):
        parser.error('Retirement accepts no Cargo arguments or cache budgets')
    if options.trim_cache and options.cargo:
        parser.error('--trim-cache accepts no Cargo arguments')
    wait_seconds = 3600 if options.wait_seconds is None else options.wait_seconds
    if not 0 <= wait_seconds < float('inf'):
        parser.error('--wait-seconds must be finite and nonnegative')
    args = options.cargo[1:] if options.cargo[:1] == ['--'] else options.cargo
    try:
        root = Path(git(Path.cwd(), 'rev-parse', '--show-toplevel')).resolve()
        if options.retire_export_plan:
            from tools._cargo_labels import retire_exports
            receipt = retire_exports(root, Path(options.retire_export_plan), wait_seconds, dry_run=options.dry_run)
            print(json.dumps(receipt, indent=2))
            return 2 if receipt['errors'] else 0
        if options.retire_label:
            from tools._cargo_labels import retire
            receipt = retire(root, options.retire_label, wait_seconds, dry_run=options.dry_run)
            print(json.dumps(receipt, indent=2))
            return 2 if receipt['errors'] else 0
        if options.resolve is not None:
            if options.from_label is not None:
                path, _ = resolve_labeled_binary(root, options.from_label, options.resolve, options.profile)
                context = f'in preserved label {options.from_label!r}'
            else:
                path, _ = resolve_binary(root, options.resolve, options.profile)
                context = 'recorded for this checkout; build it through tools.cargo_run first'
            if path is None:
                raise ValueError(f'No verified {options.profile} host executable {options.resolve!r} {context}')
            print(path)
            return 0
        from tools._cargo_cache import CachePolicy, trim
        if options.trim_cache or cache_options:
            policy = CachePolicy.from_env(options.cache_gib, options.incremental_gib, options.min_free_gib)
            if options.trim_cache:
                receipt = trim(root, policy, wait_seconds, dry_run=options.dry_run)
                print(json.dumps(receipt, indent=2))
                return 2 if receipt['errors'] else 0
            return run(root, args, options.label, wait_seconds, policy=policy)
        return run(root, args, options.label, wait_seconds)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f'cargo_run: {error}', file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        print('cargo_run interrupted; no build label published', file=sys.stderr)
        return 130


if __name__ == '__main__':
    raise SystemExit(main())
