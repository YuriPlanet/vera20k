"""Explicit saved-build retirement under cargo_run's shared owner lock.

This never follows manifests back into compiler caches or external evidence.
Retirement currently requires POSIX lsof; unavailable/failed inspection (including
Windows) fails closed. Inspection covers processes visible to the invoking user.
As with cache retention, the lock excludes cooperating writers, not arbitrary
programs launched outside cargo_run: stop consumers before retiring their labels.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import uuid

from tools._cargo_cache import _allocated, _directory_identity, _identity, _plain, _regular

def _idle(paths: list[Path]):
    if sys.platform not in {'darwin', 'linux'}:
        raise ValueError('Saved-build retirement requires Linux/macOS lsof process inspection')
    executable = shutil.which('lsof')
    if not executable:
        raise ValueError('Saved-build retirement requires lsof; no files removed')
    for offset in range(0, len(paths), 64):
        result = subprocess.run([executable, '-nP', '-Fpn', '--',
                                 *(str(path) for path in paths[offset:offset + 64])],
                                capture_output=True, text=True, timeout=30)
        if result.stdout.strip():
            raise ValueError('Selected saved-build files are in use: ' + result.stdout.strip())
        if result.returncode != 1 or result.stderr.strip():
            raise ValueError('Cannot establish idle saved-build files: ' +
                             (result.stderr.strip() or f'lsof exit {result.returncode}'))

def _durable_receipt(path: Path, value: dict):
    """Persist the complete plan before unlinking; atomic updates record progress."""
    _plain(path.parent)
    pending = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, delete=False,
                                         prefix='.retirement-', encoding='utf-8') as output:
            pending = Path(output.name)
            json.dump(value, output, indent=2)
            output.write('\n')
            output.flush()
            os.fsync(output.fileno())
        pending.replace(path)
        descriptor = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    finally:
        if pending is not None:
            pending.unlink(missing_ok=True)


def _start_receipt(store: Path, volume: Path, receipt: dict) -> Path:
    parent = store / 'label-retirements'
    parent.mkdir(exist_ok=True)
    _plain(parent)
    descriptor = os.open(store, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    path = parent / f'{time.time_ns()}-{uuid.uuid4().hex}.json'
    receipt['receipt_path'] = str(path)
    receipt['free_before'] = shutil.disk_usage(volume).free
    _durable_receipt(path, receipt)
    return path

def _finish_receipt(path: Path | None, volume: Path | None, receipt: dict):
    if path is None:
        return
    try:
        receipt['free_after'] = shutil.disk_usage(volume).free
        receipt['observed_free_delta_bytes'] = receipt['free_after'] - receipt['free_before']
    except OSError as error:
        receipt['state'] = 'partial' if receipt['removed_files'] else 'blocked'
        receipt['free_after'] = None
        receipt['observed_free_delta_bytes'] = None
        receipt['errors'].append(f'Cannot measure final free space: {error}')
    try:
        _durable_receipt(path, receipt)
    except OSError as error:
        receipt['state'] = 'partial' if receipt['removed_files'] else 'blocked'
        receipt['errors'].append(f'Cannot persist final retirement receipt: {error}')

def _inventory(store: Path, label: str):
    from tools.cargo_run import _label_path, preserved_artifact, preserved_manifest
    manifest = _label_path(store.parent.resolve(), f'{store.name}/artifacts/{label}/manifest.json')
    verified = {manifest: _identity(manifest)}
    directory, _, artifacts = preserved_manifest(store, label)
    expected = {directory / 'manifest.json'}
    paths = []
    for artifact in artifacts:
        path = _label_path(directory, artifact['file'])
        before = _identity(path)
        preserved_artifact(directory, artifact)
        if _identity(path) != before:
            raise ValueError(f'Saved artifact changed while hashing: {path}')
        verified[path] = before
        if path in expected:
            raise ValueError(f'Duplicate/overlapping saved artifact: {path}')
        expected.add(path)
        paths.append(path)
    allowed_dirs = {directory}
    for path in expected:
        allowed_dirs.update(parent for parent in path.parents if parent.is_relative_to(directory))
    directories = {path: _directory_identity(path) for path in allowed_dirs}
    # No recursive deletion: even unexpected empty directories prevent retirement.
    actual = set()
    for parent, names, filenames in os.walk(directory, followlinks=False):
        for name in names:
            path = Path(parent) / name
            if path not in allowed_dirs:
                raise ValueError(f'Unexpected saved-build directory: {path}')
            _directory_identity(path)
        actual.update(Path(parent) / name for name in filenames)
    if actual != expected:
        raise ValueError(f'Unexpected/missing saved-build files: {directory}: {actual ^ expected}')
    identities = {}
    allocated = 0
    for path in expected:
        info = _regular(path)
        if info.st_nlink != 1:
            raise ValueError(f'Shared hardlink cannot be retired: {path}')
        identities[path] = _identity(path)
        if identities[path] != verified[path]:
            raise ValueError(f'Saved-build file changed during inventory: {path}')
        allocated += _allocated(info)
    raw = manifest.read_bytes()
    if _identity(manifest) != verified[manifest]:
        raise ValueError(f'Saved manifest changed during inventory: {manifest}')
    return {
        'label': label, 'directory': directory, 'paths': paths,
        'identities': identities, 'directories': directories,
        'manifest_text': raw.decode('utf-8'),
        'manifest_sha256': hashlib.sha256(raw).hexdigest(),
        'allocated_bytes': allocated,
    }

def _unchanged(item):
    for path, identity in item['directories'].items():
        if _directory_identity(path) != identity:
            raise ValueError(f'Saved-build directory changed during retirement: {path}')
    for path, identity in item['identities'].items():
        if _identity(path) != identity:
            raise ValueError(f'Saved-build file changed during retirement: {path}')

def retire(root: Path, labels: list[str], timeout: float, dry_run: bool = False) -> dict:
    """Retire exact named labels, preserving original manifests and byte identities.

    All labels preflight before any deletion. Empty/glob/duplicate selections fail.
    Dry runs return a plan without writing a receipt or changing source artifacts.
    A durable receipt remains after success or partial failure. Recovery after a
    process/system crash can reconcile planned paths with disk; completed unlinks
    are recorded after each file. This is not a transactional filesystem operation.
    """
    from tools.cargo_run import build_lock, build_processes, build_store, validate_label
    if not labels or len(set(labels)) != len(labels):
        raise ValueError('Supply one or more unique exact saved-build labels')
    for label in labels:
        validate_label(label)
    store, _ = build_store(root)
    receipt = {
        'schema': 1, 'operation': 'retire-saved-builds', 'state': 'blocked',
        'started_unix': time.time(), 'dry_run': dry_run, 'labels': list(labels),
        'builds': [], 'removed_files': [], 'removed_labels': [], 'errors': [],
        'removed_allocated_bytes': 0, 'projected_removed_allocated_bytes': 0,
        'process_inspection_scope': 'Linux/macOS lsof; processes visible to invoking user',
    }
    receipt_path = None
    with build_lock(store / 'cargo.lock', timeout):
        try:
            items = [_inventory(store, label) for label in labels]
            paths = [path for item in items for path in item['paths']]
            _idle(paths)
            for item in items:
                _unchanged(item)
            if build_processes():
                raise ValueError('Unwrapped Cargo/rustc became active; no deletion')
            receipt['builds'] = [
                {key: item[key] for key in ('label', 'manifest_text', 'manifest_sha256', 'allocated_bytes')}
                for item in items]
            receipt['projected_removed_allocated_bytes'] = sum(item['allocated_bytes'] for item in items)
            receipt['selected_files'] = [str(path) for item in items for path in item['identities']]
            receipt['state'] = 'planned'
            if dry_run:
                return receipt
            receipt_path = _start_receipt(store, store, receipt)
            for item in items:
                _unchanged(item)
                _idle(item['paths'])
                if build_processes():
                    raise ValueError('Unwrapped Cargo/rustc became active; stopping retirement')
                # Manifest last: an interrupted label retains its lookup metadata.
                for path in [*item['paths'], item['directory'] / 'manifest.json']:
                    if _identity(path) != item['identities'][path]:
                        raise ValueError(f'Saved-build file changed before unlink: {path}')
                    allocated = _allocated(path.stat())
                    path.unlink()
                    receipt['removed_files'].append(str(path))
                    receipt['removed_allocated_bytes'] += allocated
                    _durable_receipt(receipt_path, receipt)
                for directory in sorted(item['directories'], key=lambda path: len(path.parts), reverse=True):
                    directory.rmdir()
                receipt['removed_labels'].append(item['label'])
                _durable_receipt(receipt_path, receipt)
            receipt['state'] = 'retired'
        except (OSError, ValueError, UnicodeError, subprocess.SubprocessError) as error:
            receipt['state'] = 'partial' if receipt['removed_files'] else 'blocked'
            receipt['errors'].append(str(error))
        receipt['finished_unix'] = time.time()
        _finish_receipt(receipt_path, store, receipt)
    return receipt

def _sha(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()

def _absolute(text, what: str) -> Path:
    if (not isinstance(text, str) or not Path(text).is_absolute()
            or '..' in Path(text).parts or any(ord(c) < 32 for c in text)):
        raise ValueError(f'{what} must be an absolute path without traversal')
    path = Path(text)
    _plain(path)
    if str(path) != text or path.resolve() != path:
        raise ValueError(f'{what} must use its canonical path')
    return path

def _fields(value, names: set[str], what: str):
    if not isinstance(value, dict) or set(value) != names:
        raise ValueError(f'{what} requires exactly {sorted(names)}')

def _digest(value):
    if not isinstance(value, str) or not re.fullmatch(r'[0-9a-f]{64}', value):
        raise ValueError('Expected lowercase SHA-256')
    return value

def _reference(value: dict):
    _fields(value, {'path', 'sha256'}, 'Evidence reference')
    path = _absolute(value['path'], 'Evidence reference')
    before = _identity(path)
    if _sha(path) != _digest(value['sha256']) or _identity(path) != before:
        raise ValueError(f'Evidence reference changed or SHA mismatch: {path}')
    return path, before

def _libtest(binary: Path) -> set[Path]:
    """Inspect, never execute, an unstripped native Rust VERA libtest.

    The existing format/debug-closure owner validates Mach-O. Native nm
    supplies symbols; names alone do not establish ownership (pinned reviewed
    provenance does). Stripped or unusual symbol mangling fails closed.
    """
    from tools._cargo_cache import MACH, _magic, _stream
    from tools._cargo_macho import dependencies
    if sys.platform != 'darwin' or _magic(binary) not in MACH:
        raise ValueError(f'Export is not a supported native Rust executable: {binary}')
    if not (_regular(binary).st_mode & 0o111):
        raise ValueError(f'Export is not executable: {binary}')
    tool = shutil.which('nm')
    if not tool:
        raise ValueError('Rust libtest identification requires native nm')
    # Darwin nm -j outputs one symbol per line; -U excludes undefined symbols.
    command = [tool, '-j', '-U', str(binary)]
    def symbols(lines):
        harness = False
        tests = False
        for line in lines:
            name = line.strip()
            harness |= ('4test' in name and 'test_main_static' in name)
            # Legacy mangling prefixes each path segment with its length:
            # the client crate `vera20k` or the engine crate `vera20k_core`.
            tests |= (('7vera20k' in name or '12vera20k_core' in name) and '5tests' in name)
        if not harness or not tests:
            raise ValueError(f'Cannot identify VERA Rust libtest symbols: {binary}')
    _stream(command, symbols)
    return dependencies(binary, require_executable=True)


def _required_debug_inputs(binary: Path, dependencies: set[Path]) -> dict:
    from tools._cargo_cache import _dependency_identity
    identities = {path: _dependency_identity(path) for path in sorted(dependencies)}
    missing = [str(path) for path, identity in identities.items() if identity is None]
    if missing:
        raise ValueError(f'{binary}: {len(missing)} of {len(dependencies)} required debug inputs missing; '
                         f'first missing: {missing[:3]}')
    return identities


def _replacement(store: Path, root: Path, value: dict):
    from tools.cargo_run import preserved_manifest, validate_label
    _fields(value, {'label', 'file', 'sha256', 'manifest_sha256', 'purpose'}, 'Replacement')
    if not isinstance(value['label'], str):
        raise ValueError('Replacement label must be a string')
    validate_label(value['label'])
    if value['purpose'] != 'vera20k-libtest':
        raise ValueError('Replacement purpose must be vera20k-libtest')
    item = _inventory(store, value['label'])
    if item['manifest_sha256'] != _digest(value['manifest_sha256']):
        raise ValueError('Retained replacement manifest SHA mismatch')
    manifest = json.loads(item['manifest_text'])
    command = manifest.get('command')
    source = manifest.get('source')
    if (manifest.get('checkout') != str(root) or not isinstance(source, dict)
            or not isinstance(source.get('source_sha256'), str)
            or not re.fullmatch(r'[0-9a-f]{64}', source['source_sha256'])
            or not isinstance(command, list) or command[:2] != ['cargo', 'test']
            or '--lib' not in command or '--no-run' not in command
            or '-p' not in command
            or command[command.index('-p') + 1:command.index('-p') + 2] not in (['vera20k'], ['vera20k-core'])):
        raise ValueError('Replacement is not a recorded same-checkout VERA libtest build')
    directory, target, artifacts = preserved_manifest(store, value['label'])
    matches = [entry for entry in artifacts if entry['file'] == value['file']]
    if len(matches) != 1 or matches[0]['sha256'] != _digest(value['sha256']):
        raise ValueError('Replacement artifact identity is missing or ambiguous')
    entry, = matches
    original = Path(entry['source'])
    if (not original.is_absolute() or '..' in original.parts
            or original.parent not in (target / 'debug' / 'deps', target / 'release' / 'deps')):
        raise ValueError('Replacement is not a Cargo host library-test artifact')
    binary = directory / entry['file']
    dependencies = _libtest(binary)
    # Required debugging inputs for the retained replacement must remain usable.
    refs = _required_debug_inputs(binary, dependencies)
    _unchanged(item)
    return item, binary, refs

def retire_exports(root: Path, plan_path: Path, timeout: float, dry_run: bool = False) -> dict:
    """Retire only reviewed exact legacy LIBTEST exports; never discover candidates.

    The plan is authority from its reviewer, not authenticated build provenance.
    Pinned pre-existing records must include each original SHA and checkout.
    Missing ownership/consumer records, unsupported formats, lost replacements
    or incomplete dependency inspection block the entire selected batch.
    """
    from tools.cargo_run import _manifest_object, build_lock, build_processes, build_store
    root = root.resolve()
    plan_path = _absolute(str(plan_path.absolute()), 'Plan')
    store, _ = build_store(root)
    receipt = {'schema': 1, 'operation': 'retire-exported-libtests', 'state': 'blocked',
               'started_unix': time.time(), 'dry_run': dry_run, 'removed_files': [],
               'errors': [], 'removed_allocated_bytes': 0,
               'process_inspection_scope': 'Linux/macOS lsof; processes visible to invoking user'}
    receipt_path = None
    export_root = None
    with build_lock(store / 'cargo.lock', timeout):
        try:
            plan_identity = _identity(plan_path)
            raw = plan_path.read_bytes()
            plan = json.loads(raw, object_pairs_hook=_manifest_object)
            _fields(plan, {'schema', 'checkout', 'export_root', 'files'}, 'Export plan')
            if type(plan['schema']) is not int or plan['schema'] != 1 or plan['checkout'] != str(root):
                raise ValueError('Export plan must bind schema 1 and this exact checkout')
            export_root = _absolute(plan['export_root'], 'Export root')
            _directory_identity(export_root)
            if (export_root.is_relative_to(root) or root.is_relative_to(export_root)
                    or export_root.is_relative_to(store) or store.is_relative_to(export_root)):
                raise ValueError('Export root must be separate from source and shared builds')
            if not isinstance(plan['files'], list) or not plan['files']:
                raise ValueError('Export plan needs a nonempty exact file selection')
            items = []
            selected = set()
            identities = {plan_path: plan_identity}
            protected = set()
            replacements = []
            replacement_cache = {}
            for entry in plan['files']:
                _fields(entry, {'file', 'sha256', 'reason', 'consumer_status', 'provenance',
                                'results', 'replacement'}, 'Export entry')
                relative = entry['file']
                if (not isinstance(relative, str) or not relative
                        or PurePosixPath(relative).is_absolute() or '\\' in relative
                        or PurePosixPath(relative).as_posix() != relative
                        or any(part in {'.', '..'} or part.lower().endswith('.app') for part in relative.split('/'))
                        or any(c in relative for c in '*?[]') or any(ord(c) < 32 for c in relative)):
                    raise ValueError('Export selection needs an exact relative non-app file path')
                path = export_root / relative
                if path in selected:
                    raise ValueError('Duplicate export selection')
                selected.add(path)
                before = _identity(path)
                if before[-1] != 1:
                    raise ValueError(f'Shared hardlink cannot be retired: {path}')
                if _sha(path) != _digest(entry['sha256']):
                    raise ValueError(f'Export SHA mismatch: {path}')
                if not isinstance(entry['reason'], str) or not entry['reason'].strip():
                    raise ValueError('Export retirement needs a concrete reviewed reason')
                if entry['consumer_status'] != 'reviewed-no-active-or-required-consumers':
                    raise ValueError('Export consumers are active, required or ambiguous')
                if not isinstance(entry['provenance'], list) or not entry['provenance']:
                    raise ValueError('Export needs pinned pre-existing ownership provenance')
                if not isinstance(entry['results'], list) or not entry['results']:
                    raise ValueError('Export needs pinned retained validation results')
                provenance = []
                for reference in [*entry['provenance'], *entry['results']]:
                    reference_path, identity = _reference(reference)
                    identities[reference_path] = identity
                    protected.add(reference_path)
                    if reference in entry['provenance']:
                        provenance.append(reference_path.read_text(encoding='utf-8'))
                # A review cannot manufacture a historical source identity in the
                # plan: it must bind an already recorded SHA and checkout.
                if not any(entry['sha256'] in text and str(root) in text for text in provenance):
                    raise ValueError('Original SHA and checkout are absent from pinned provenance')
                dependencies = _libtest(path)
                dependency_identities = _required_debug_inputs(path, dependencies)
                identities.update(dependency_identities)
                protected.update(dependencies)
                # One reviewed replacement often serves many superseded exports.
                # Inspect it once per exact binding in this locked batch; retain
                # the first identities for every subsequent pre-unlink recheck.
                replacement_key = json.dumps(entry['replacement'], sort_keys=True)
                if replacement_key not in replacement_cache:
                    replacement_cache[replacement_key] = _replacement(store, root, entry['replacement'])
                    replacements.append(replacement_cache[replacement_key][0])
                replacement, replacement_binary, refs = replacement_cache[replacement_key]
                identities.update(replacement['identities'])
                identities.update(refs)
                protected.update(replacement['identities'])
                protected.update(refs)
                if _identity(path) != before:
                    raise ValueError(f'Export changed during inspection: {path}')
                identities[path] = before
                items.append({'path': path, 'allocated_bytes': _allocated(path.stat()),
                              'dependencies': sorted(map(str, dependencies)), 'review': entry,
                              'replacement_manifest_text': replacement['manifest_text']})
            if selected & protected or plan_path in selected:
                raise ValueError('Selection overlaps provenance, results or required debug/replacement inputs')
            # Directory inode/device binding detects substituted ancestors without
            # interpreting unrelated evidence files or deleting any directory.
            directories = {parent: _directory_identity(parent)[:2]
                           for path in identities for parent in path.parents}
            def unchanged():
                for parent, identity in directories.items():
                    if _directory_identity(parent)[:2] != identity:
                        raise ValueError(f'Retention ancestor changed: {parent}')
                for path, identity in identities.items():
                    if _identity(path) != identity:
                        raise ValueError(f'Retention input changed: {path}')
                for replacement in replacements:
                    _unchanged(replacement)
            def safe():
                _idle(list(selected))
                unchanged()
                if build_processes():
                    raise ValueError('Unwrapped Cargo/rustc became active; stopping retirement')
            safe()
            receipt.update({'state': 'planned', 'checkout': str(root), 'export_root': str(export_root),
                            'plan_path': str(plan_path), 'plan_text': raw.decode('utf-8'),
                            'plan_sha256': hashlib.sha256(raw).hexdigest(),
                            'files': [{**item, 'path': str(item['path'])} for item in items],
                            'input_identities': {str(path): identity for path, identity in identities.items()},
                            'projected_removed_allocated_bytes': sum(item['allocated_bytes'] for item in items)})
            if dry_run:
                return receipt
            receipt_path = _start_receipt(store, export_root, receipt)
            for item in items:
                safe()  # Replacement, plan, evidence, dependencies and consumers afresh.
                path = item['path']
                path.unlink()
                receipt['removed_files'].append(str(path))
                receipt['removed_allocated_bytes'] += item['allocated_bytes']
                identities.pop(path)
                selected.remove(path)
                descriptor = os.open(path.parent, os.O_RDONLY)
                try:
                    os.fsync(descriptor)
                finally:
                    os.close(descriptor)
                _durable_receipt(receipt_path, receipt)
            receipt['state'] = 'retired'
        except (OSError, ValueError, UnicodeError, subprocess.SubprocessError) as error:
            receipt['state'] = 'partial' if receipt['removed_files'] else 'blocked'
            receipt['errors'].append(str(error))
        receipt['finished_unix'] = time.time()
        _finish_receipt(receipt_path, export_root, receipt)
    return receipt
