"""Failure and ownership contracts for the build runner (no game/retail required)."""
import hashlib
from contextlib import redirect_stderr, redirect_stdout
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from tools import cargo_run


class CargoRunTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        subprocess.run(['git', '-C', str(self.root), 'config', 'user.name', 'Test'], check=True)
        subprocess.run(['git', '-C', str(self.root), 'config', 'user.email', 'test@example.invalid'], check=True)
        (self.root / '.gitignore').write_text('/target/\n')
        (self.root / 'source.rs').write_text('first')
        subprocess.run(['git', '-C', str(self.root), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(self.root), 'commit', '-qm', 'initial'], check=True)
        self.env = patch.dict(os.environ)
        self.env.start()
        self.addCleanup(self.env.stop)
        os.environ.pop('CARGO_TARGET_DIR', None)
        # Ordinary runner fixtures do not depend on the host disk reserve.
        os.environ['VERA20K_MIN_FREE_GIB'] = '0'

    def test_export_retirement_routes_only_explicit_plan_without_building(self):
        from tools import _cargo_labels
        with patch.object(cargo_run, 'git', return_value=str(self.root)), \
             patch.object(_cargo_labels, 'retire_exports', return_value={'state': 'planned', 'errors': []}) as retire, \
             patch.object(cargo_run, 'run') as build, redirect_stdout(io.StringIO()):
            self.assertEqual(cargo_run.main(['--retire-export-plan', '/plan.json', '--dry-run']), 0)
            retire.assert_called_once_with(self.root, Path('/plan.json'), 3600, dry_run=True)
            build.assert_not_called()
        for arguments in (
            ['--retire-export-plan', '/plan.json', '--label', 'another'],
            ['--retire-export-plan', '/plan.json', '--cache-gib', '0'],
            ['--retire-export-plan', '/plan.json', '--profile', 'debug'],
            ['--retire-export-plan', '/plan.json', '--', 'build'],
            ['--dry-run'],
        ):
            with self.subTest(arguments=arguments), redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                cargo_run.main(arguments)

    def test_fingerprint_covers_dirty_deleted_and_untracked_sources(self):
        first = cargo_run.source_identity(self.root)
        (self.root / 'source.rs').write_text('second')
        second = cargo_run.source_identity(self.root)
        self.assertNotEqual(first['source_sha256'], second['source_sha256'])
        (self.root / 'source.rs').unlink()
        third = cargo_run.source_identity(self.root)
        self.assertNotEqual(second['source_sha256'], third['source_sha256'])
        (self.root / 'new.rs').write_text('new')
        self.assertNotEqual(third['source_sha256'], cargo_run.source_identity(self.root)['source_sha256'])

    def test_second_fingerprint_reads_only_files_that_may_have_changed(self):
        settled = time.time() - 60
        for name in ('.gitignore', 'source.rs'):
            os.utime(self.root / name, (settled, settled))
        hashes = {}
        first = cargo_run.source_identity(self.root, hashes)
        with patch.object(Path, 'read_bytes', side_effect=AssertionError('unchanged file was read again')):
            self.assertEqual(cargo_run.source_identity(self.root, hashes), first)
        (self.root / 'source.rs').write_text('other')  # Same length, new timestamp.
        self.assertNotEqual(cargo_run.source_identity(self.root, hashes)['source_sha256'],
                            first['source_sha256'])

    def test_file_written_just_before_a_fingerprint_is_read_again(self):
        # A coarse filesystem clock would not move on a second write this soon,
        # so a timestamp inside the window cannot vouch for the cached digest.
        path = self.root / 'source.rs'
        now = time.time_ns()
        os.utime(path, ns=(now - 10 ** 9, now - 10 ** 9))
        os.utime(self.root / '.gitignore', ns=(now - 60 * 10 ** 9, now - 60 * 10 ** 9))
        hashes = {}
        with patch.object(cargo_run.time, 'time_ns', return_value=now):
            first = cargo_run.source_identity(self.root, hashes)
        written = path.stat()
        path.write_text('other')  # Same length.
        os.utime(path, ns=(written.st_atime_ns, written.st_mtime_ns))
        reads = []
        original = Path.read_bytes
        def counting(target):
            reads.append(target.name)
            return original(target)
        with patch.object(Path, 'read_bytes', counting):
            second = cargo_run.source_identity(self.root, hashes)
        self.assertEqual(reads, ['source.rs'])
        self.assertNotEqual(second['source_sha256'], first['source_sha256'])

    def test_lock_excludes_other_process_and_releases_after_owner_exit(self):
        lock = self.root / 'build.lock'
        code = "from pathlib import Path; from tools.cargo_run import build_lock; " \
               "\nwith build_lock(Path(__import__('sys').argv[1]), 0): print('acquired')"
        with patch.object(cargo_run, 'build_processes', return_value=[]):
            with cargo_run.build_lock(lock, 0):
                result = subprocess.run([sys.executable, '-c', code, str(lock)], capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Timed out', result.stderr)
            with cargo_run.build_lock(lock, 0):
                pass

    def test_unwrapped_cargo_blocks_build_and_inspection_errors_propagate(self):
        with patch.object(cargo_run, 'build_processes', return_value=['12 cargo']):
            with self.assertRaisesRegex(TimeoutError, '12 cargo'):
                with cargo_run.build_lock(self.root / 'lock', 0):
                    self.fail('entered while another compile was active')
        with patch.object(cargo_run, 'build_processes', side_effect=OSError('ps failed')):
            with self.assertRaisesRegex(OSError, 'ps failed'):
                with cargo_run.build_lock(self.root / 'lock', 0):
                    self.fail('entered without process inspection')

    def test_invalid_target_and_label_requests_never_build(self):
        for args in (['test'], ['build', '--target-dir=/tmp/shared'],
                     ['build', '--manifest-path', 'other.toml'], ['build', '--config=x']):
            with self.subTest(args=args), self.assertRaises(ValueError):
                cargo_run.cargo_args(args, None)
        with self.assertRaisesRegex(ValueError, 'requires build'):
            cargo_run.cargo_args(['test', '--lib'], 'bad')
        with self.assertRaisesRegex(ValueError, 'Label'):
            cargo_run.run(self.root, ['build'], '../escape', 0)

    def fake_cargo(self, *, code=0, mutate=False, artifact=True, host_profile=None):
        # Emulates Cargo's *public JSON output*, including a misleading stale
        # conventional target path. The runner must use the emitted path.
        def start(command, **kwargs):
            target = Path(kwargs['env']['CARGO_TARGET_DIR'])
            path = (target / host_profile / ('asset.exe' if os.name == 'nt' else 'asset')
                    if host_profile else target / 'debug' / 'deps' / ('actual-hash.exe' if os.name == 'nt' else 'actual-hash'))
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'current executable')
            (self.root / 'target' / 'stale').write_bytes(b'wrong checkout')
            if mutate:
                (self.root / 'source.rs').write_text('edited during build')
            child = unittest.mock.MagicMock()
            child.__enter__.return_value = child
            message = {'reason': 'compiler-artifact', 'executable': str(path)}
            child.stdout = io.StringIO(json.dumps(message) + '\n' if artifact else '')
            child.wait.return_value = code
            return child
        return start

    def invoke(self, label='build-a', **options):
        original = subprocess.check_output
        def output(command, **kwargs):
            if command[0] in {'rustc', 'cargo'}:
                return 'test toolchain\n'
            return original(command, **kwargs)
        actual_popen = subprocess.Popen
        fake = self.fake_cargo(**options)
        def start(command, **kwargs):
            return fake(command, **kwargs) if command[0] == 'cargo' else actual_popen(command, **kwargs)
        with patch.object(cargo_run, 'build_processes', return_value=[]), \
             patch.object(cargo_run.subprocess, 'check_output', side_effect=output), \
             patch.object(cargo_run.subprocess, 'Popen', side_effect=start):
            return cargo_run.run(self.root, ['build'], label, 0)

    def test_low_free_space_blocks_before_cargo_or_label_publication(self):
        from tools import _cargo_cache
        policy = _cargo_cache.CachePolicy(1 << 40, 1 << 40, 4096)
        real_popen = subprocess.Popen
        def start(command, **kwargs):
            self.assertNotEqual(command[0], 'cargo', 'Cargo must not start below reserve')
            return real_popen(command, **kwargs)
        with patch.object(cargo_run, 'build_processes', return_value=[]), \
             patch.object(_cargo_cache, 'automatic_locked'), \
             patch.object(cargo_run.shutil, 'disk_usage', return_value=unittest.mock.Mock(free=4095)), \
             patch.object(cargo_run.subprocess, 'Popen', side_effect=start):
            with self.assertRaisesRegex(ValueError, 'Build blocked'):
                cargo_run.run(self.root, ['build'], 'blocked', 0, policy=policy)
        self.assertFalse((self.root / '.git/owned-builds/artifacts/blocked').exists())
        self.assertEqual((self.root / 'source.rs').read_text(), 'first')

    def test_free_space_is_measured_after_cleanup_at_exact_admission_boundary(self):
        from tools import _cargo_cache
        policy = _cargo_cache.CachePolicy(1 << 40, 1 << 40, 4096)
        free = 1
        def cleanup(*args):
            nonlocal free
            free = 4096
        original = subprocess.Popen
        child = unittest.mock.MagicMock()
        child.__enter__.return_value = child
        child.stdout = io.StringIO('')
        child.wait.return_value = 0
        launched = []
        def start(command, **kwargs):
            if command[0] == 'cargo':
                launched.append(command)
                return child
            return original(command, **kwargs)
        with patch.object(cargo_run, 'build_processes', return_value=[]), \
             patch.object(_cargo_cache, 'automatic_locked', side_effect=cleanup), \
             patch.object(cargo_run.shutil, 'disk_usage', side_effect=lambda path: unittest.mock.Mock(free=free)), \
             patch.object(cargo_run.subprocess, 'Popen', side_effect=start):
            self.assertEqual(cargo_run.run(self.root, ['check'], None, 0, policy=policy), 0)
        self.assertEqual(len(launched), 1)

    def test_label_admission_checks_artifact_volume_as_well_as_target(self):
        from tools import _cargo_cache
        from types import SimpleNamespace
        store, namespace = cargo_run.build_store(self.root)
        target = self.root / 'target/owned-worktrees' / namespace
        policy = _cargo_cache.CachePolicy(1 << 40, 1 << 40, 4096)
        real_popen = subprocess.Popen
        measured = []
        def usage(path):
            measured.append(path)
            return SimpleNamespace(free=4096 if path == target else 4095)
        def start(command, **kwargs):
            self.assertNotEqual(command[0], 'cargo', 'Cargo must not start with full artifact volume')
            return real_popen(command, **kwargs)
        with patch.object(cargo_run, 'build_processes', return_value=[]), \
             patch.object(_cargo_cache, 'automatic_locked'), \
             patch.object(cargo_run.shutil, 'disk_usage', side_effect=usage), \
             patch.object(cargo_run.subprocess, 'Popen', side_effect=start):
            with self.assertRaisesRegex(ValueError, 'Build blocked'):
                cargo_run.run(self.root, ['build'], 'blocked-artifact', 0, policy=policy)
        # Both volumes are measured to find the short reserve, and again after cleanup.
        self.assertEqual(measured, [target, store, target, store])
        self.assertFalse((store / 'artifacts/blocked-artifact').exists())

    def retention_order(self, *, free, due):
        """When the retention pass runs relative to Cargo in one successful run."""
        from tools import _cargo_cache
        policy = _cargo_cache.CachePolicy(1 << 40, 1 << 40, 4096)
        events = []
        child = unittest.mock.MagicMock()
        child.__enter__.return_value = child
        child.stdout = io.StringIO('')
        child.wait.return_value = 0
        original = subprocess.Popen
        def start(command, **kwargs):
            if command[0] == 'cargo':
                events.append('cargo')
                return child
            return original(command, **kwargs)
        def cleanup(*args):
            nonlocal free
            events.append('retention')
            free = 4096
        with patch.object(cargo_run, 'build_processes', return_value=[]), \
             patch.object(_cargo_cache, 'automatic_locked', side_effect=cleanup), \
             patch.object(_cargo_cache, 'retention_due', return_value=due), \
             patch.object(cargo_run.shutil, 'disk_usage', side_effect=lambda path: unittest.mock.Mock(free=free)), \
             patch.object(cargo_run.subprocess, 'Popen', side_effect=start):
            self.assertEqual(cargo_run.run(self.root, ['check'], None, 0, policy=policy), 0)
        return events

    def test_retention_pass_runs_only_when_it_can_matter(self):
        # Reserve intact and a recent pass: no cache inventory around this command.
        self.assertEqual(self.retention_order(free=4096, due=False), ['cargo'])
        # Soft budgets are enforced after the build, once the cache has grown.
        self.assertEqual(self.retention_order(free=4096, due=True), ['cargo', 'retention'])
        # A short reserve is restored before admission, whatever ran recently.
        self.assertEqual(self.retention_order(free=4095, due=False), ['retention', 'cargo'])

    def test_free_space_measurement_failure_never_launches_cargo(self):
        from tools import _cargo_cache
        real_popen = subprocess.Popen
        def start(command, **kwargs):
            self.assertNotEqual(command[0], 'cargo', 'Cargo must not start without free-space measurement')
            return real_popen(command, **kwargs)
        with patch.object(cargo_run, 'build_processes', return_value=[]), \
             patch.object(_cargo_cache, 'automatic_locked'), \
             patch.object(cargo_run.shutil, 'disk_usage', side_effect=OSError('volume unavailable')), \
             patch.object(cargo_run.subprocess, 'Popen', side_effect=start):
            with self.assertRaisesRegex(OSError, 'volume unavailable'):
                cargo_run.run(self.root, ['check'], None, 0)

    def test_copies_only_emitted_executable_and_refuses_overwrite(self):
        self.assertEqual(self.invoke(), 0)
        output = self.root / '.git/owned-builds/artifacts/build-a'
        manifest = json.loads((output / 'manifest.json').read_text())
        entry, = manifest['artifacts']
        self.assertEqual((output / entry['file']).read_bytes(), b'current executable')
        self.assertEqual(Path(entry['file']).name, 'actual-hash.exe' if os.name == 'nt' else 'actual-hash')
        self.assertEqual(entry['sha256'], hashlib.sha256(b'current executable').hexdigest())
        self.assertEqual(manifest['source']['head'], cargo_run.git(self.root, 'rev-parse', 'HEAD'))
        with self.assertRaisesRegex(ValueError, 'already exists'):
            self.invoke()

    def test_binary_discovery_uses_recorded_output_not_stale_conventional_binary(self):
        stale = self.root / 'target/release/asset'
        stale.parent.mkdir(parents=True)
        stale.write_bytes(b'stale')
        self.assertEqual(cargo_run.resolve_binary(self.root, 'asset'), (None, None))
        store, namespace = cargo_run.build_store(self.root)
        target = self.root / 'target/shared-parent/owned-worktrees' / namespace
        binary = target / 'release/asset'
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b'owned')
        cargo_run.publish_binaries(store, namespace, target, {binary})
        self.assertEqual(cargo_run.resolve_binary(self.root, 'asset'), (binary, 'release'))
        binary.write_bytes(b'overwritten')
        self.assertEqual(cargo_run.resolve_binary(self.root, 'asset'), (None, None))

    def test_failed_or_mutating_build_never_publishes_label(self):
        self.assertEqual(self.invoke(code=7), 7)
        with self.assertRaisesRegex(ValueError, 'Source changed'):
            self.invoke(mutate=True)
        self.assertFalse((self.root / '.git/owned-builds/artifacts/build-a').exists())

    def recorded_binary(self, profile):
        store, namespace = cargo_run.build_store(self.root)
        target = self.root / 'target/owned-worktrees' / namespace
        binary = target / profile / ('asset.exe' if os.name == 'nt' else 'asset')
        binary.parent.mkdir(parents=True, exist_ok=True)
        binary.write_bytes(profile.encode())
        cargo_run.publish_binaries(store, namespace, target, {binary})
        return binary

    def resolve_cli(self, *args):
        stdout, stderr = io.StringIO(), io.StringIO()
        with patch.object(Path, 'cwd', return_value=self.root), \
             patch.object(cargo_run, 'run') as build, \
             redirect_stdout(stdout), redirect_stderr(stderr):
            code = cargo_run.main(list(args))
        build.assert_not_called()
        return code, stdout.getvalue(), stderr.getvalue()

    def test_resolve_cli_selects_explicit_profile_and_prints_only_recorded_path(self):
        release = self.recorded_binary('release')
        debug = self.recorded_binary('debug')
        os.environ['CARGO_TARGET_DIR'] = str(self.root / 'different-shell-target')
        for profile, binary in [('release', release), ('debug', debug)]:
            self.assertEqual(self.resolve_cli('--resolve', 'asset', '--profile', profile),
                             (0, str(binary) + '\n', ''))
        # Existing MCP consumers retain their documented preference.
        self.assertEqual(cargo_run.resolve_binary(self.root, 'asset'), (release, 'release'))

    def test_explicit_resolve_never_falls_back_after_missing_or_changed_binary(self):
        debug = self.recorded_binary('debug')
        for state in ('missing_record', 'changed_bytes', 'missing_file'):
            if state == 'changed_bytes':
                release = self.recorded_binary('release')
                release.write_bytes(b'changed since successful build')
            elif state == 'missing_file':
                release.unlink()
            code, stdout, stderr = self.resolve_cli('--resolve', 'asset', '--profile', 'release')
            self.assertEqual(code, 2, state)
            self.assertEqual(stdout, '', state)
            self.assertIn('No verified release host executable', stderr)
            self.assertEqual(cargo_run.resolve_binary(self.root, 'asset'), (debug, 'debug'))
        code, stdout, _ = self.resolve_cli('--resolve', 'unknown', '--profile', 'debug')
        self.assertEqual((code, stdout), (2, ''))
        with self.assertRaises(ValueError):
            cargo_run.resolve_binary(self.root, 'asset', 'custom')

    def test_resolve_rejects_ambiguous_or_build_options_before_dispatch(self):
        for args in [
            ['--resolve', 'asset'],
            ['--resolve', 'asset', '--profile', 'release', '--', 'build'],
            ['--resolve', 'asset', '--profile', 'release', '--label', 'no'],
            ['--resolve', 'asset', '--profile', 'release', '--wait-seconds', '1'],
            ['--profile', 'debug', '--', 'build'],
        ]:
            with self.subTest(args=args), patch.object(cargo_run, 'run') as build, \
                 redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
                cargo_run.main(args)
            self.assertEqual(error.exception.code, 2)
            build.assert_not_called()

    def test_build_cli_preserves_wait_label_and_cargo_forwarding(self):
        for flags, label, timeout in [([], None, 3600),
                                     (['--wait-seconds', '0', '--label', 'candidate'], 'candidate', 0)]:
            with patch.object(Path, 'cwd', return_value=self.root), \
                 patch.object(cargo_run, 'run', return_value=7) as build:
                self.assertEqual(cargo_run.main([*flags, '--', 'build', '--release', '--bin', 'asset']), 7)
            build.assert_called_once_with(self.root, ['build', '--release', '--bin', 'asset'], label, timeout)

    def test_success_without_artifact_is_not_a_preserved_build(self):
        with self.assertRaisesRegex(ValueError, 'no executable'):
            self.invoke(artifact=False)
        self.assertFalse((self.root / '.git/owned-builds/artifacts/build-a').exists())

    def preserved_label(self, label='release-a', *, source_suffix=None):
        store, namespace = cargo_run.build_store(self.root)
        directory = store / 'artifacts' / label
        binary_name = 'asset.exe' if os.name == 'nt' else 'asset'
        binary = directory / '0' / binary_name
        binary.parent.mkdir(parents=True, exist_ok=True)
        binary.write_bytes(b'preserved executable')
        target = self.root / 'old-deleted-cache/owned-worktrees' / namespace
        manifest = {
            'schema': 1, 'target_dir': str(target),
            'artifacts': [{'source': str(target / (source_suffix or 'release') / binary_name),
                           'file': '0/' + binary_name,
                           'sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}],
        }
        (directory / 'manifest.json').write_text(json.dumps(manifest))
        return directory, binary, manifest

    def test_label_resolution_uses_preserved_bytes_without_live_cache_or_latest(self):
        directory, binary, _ = self.preserved_label()
        latest = self.recorded_binary('release')
        self.assertNotEqual(latest, binary)
        self.assertEqual(cargo_run.resolve_labeled_binary(self.root, 'release-a', 'asset', 'release'),
                         (binary, 'release'))
        self.assertEqual(self.resolve_cli('--resolve', 'asset', '--profile', 'release',
                                          '--from-label', 'release-a'),
                         (0, str(binary) + '\n', ''))
        self.assertEqual(cargo_run.resolve_binary(self.root, 'asset', 'release'), (latest, 'release'))
        binary.write_bytes(b'changed preserved bytes')
        code, stdout, stderr = self.resolve_cli('--resolve', 'asset', '--profile', 'release',
                                              '--from-label', 'release-a')
        self.assertEqual((code, stdout), (2, ''))
        self.assertIn('SHA-256 mismatch', stderr)

    def test_schema_one_producer_output_resolves_only_its_host_bin(self):
        self.assertEqual(self.invoke(label='host-build', host_profile='debug'), 0)
        path, profile = cargo_run.resolve_labeled_binary(self.root, 'host-build', 'asset', 'debug')
        self.assertEqual(profile, 'debug')
        self.assertEqual(path.read_bytes(), b'current executable')
        self.assertEqual(self.invoke(label='test-build'), 0)
        with self.assertRaisesRegex(ValueError, 'not a debug host bin'):
            cargo_run.resolve_labeled_binary(self.root, 'test-build', 'actual-hash', 'debug')

    def test_absent_label_and_name_never_fall_back_to_latest(self):
        self.recorded_binary('release')
        self.assertEqual(cargo_run.resolve_labeled_binary(self.root, 'absent', 'asset', 'release'),
                         (None, None))
        self.preserved_label()
        self.assertEqual(cargo_run.resolve_labeled_binary(self.root, 'release-a', 'absent', 'release'),
                         (None, None))
        code, stdout, stderr = self.resolve_cli('--resolve', 'asset', '--profile', 'release',
                                              '--from-label', 'absent')
        self.assertEqual((code, stdout), (2, ''))
        self.assertIn("preserved label 'absent'", stderr)

    def test_labeled_host_classification_rejects_other_artifact_layouts(self):
        for suffix in ('debug', 'release/deps', 'release/examples',
                       'release/build', 'x86_64-unknown-linux-gnu/release'):
            with self.subTest(suffix=suffix):
                directory, binary, manifest = self.preserved_label(source_suffix=suffix)
                with self.assertRaisesRegex(ValueError, 'not a release host bin'):
                    cargo_run.resolve_labeled_binary(self.root, 'release-a', 'asset', 'release')
        manifest['artifacts'][0]['source'] = str(self.root / 'elsewhere' / binary.name)
        (directory / 'manifest.json').write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, 'not a release host bin'):
            cargo_run.resolve_labeled_binary(self.root, 'release-a', 'asset', 'release')

    def test_label_resolver_rejects_ambiguous_basename_before_choosing(self):
        directory, binary, manifest = self.preserved_label()
        duplicate = dict(manifest['artifacts'][0], file='1/' + binary.name)
        # Even a second record with the wrong profile must not be hidden by
        # filtering: a requested preserved basename must identify one record.
        duplicate['source'] = str(Path(manifest['target_dir']) / 'debug' / binary.name)
        manifest['artifacts'].append(duplicate)
        (directory / 'manifest.json').write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, 'Ambiguous'):
            cargo_run.resolve_labeled_binary(self.root, 'release-a', 'asset', 'release')

    def test_label_manifest_schema_and_record_shape_are_checked(self):
        directory, _, base = self.preserved_label()
        cases = [[], {'schema': True}, dict(base, schema=2),
                 dict(base, target_dir='relative'), dict(base, artifacts={}),
                 dict(base, artifacts=[None]), dict(base, artifacts=[{'file': '0/asset'}])]
        for manifest in cases:
            with self.subTest(manifest=manifest):
                (directory / 'manifest.json').write_text(json.dumps(manifest))
                with self.assertRaises(ValueError):
                    cargo_run.resolve_labeled_binary(self.root, 'release-a', 'asset', 'release')
        (directory / 'manifest.json').write_text('{"schema": 1, "schema": 1}')
        with self.assertRaisesRegex(ValueError, 'Duplicate preserved manifest field'):
            cargo_run.resolve_labeled_binary(self.root, 'release-a', 'asset', 'release')
        (directory / 'manifest.json').write_text('{broken')
        with self.assertRaises(ValueError):
            cargo_run.resolve_labeled_binary(self.root, 'release-a', 'asset', 'release')

    def test_label_artifact_paths_cannot_escape_or_alias_basename(self):
        directory, binary, base = self.preserved_label()
        for relative in ('../' + binary.name, '/' + binary.name, '0/../' + binary.name,
                         '0//' + binary.name, './0/' + binary.name,
                         'C:/outside/' + binary.name, '0\\' + binary.name,
                         '0/asset:stream'):
            with self.subTest(path=relative):
                manifest = dict(base, artifacts=[dict(base['artifacts'][0], file=relative)])
                (directory / 'manifest.json').write_text(json.dumps(manifest))
                with self.assertRaisesRegex(ValueError, 'Invalid preserved artifact path'):
                    cargo_run.resolve_labeled_binary(self.root, 'release-a', 'asset', 'release')
        manifest = dict(base, artifacts=[dict(base['artifacts'][0], source=str(Path(base['target_dir']) / 'release/different'))])
        (directory / 'manifest.json').write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, 'not a release host bin'):
            cargo_run.resolve_labeled_binary(self.root, 'release-a', 'asset', 'release')

    def test_label_paths_reject_symlinked_artifact_manifest_or_directory(self):
        directory, binary, manifest = self.preserved_label()
        outside = self.root / 'outside'
        outside.mkdir()
        target = outside / binary.name
        target.write_bytes(binary.read_bytes())
        try:
            (outside / 'probe').symlink_to(target)
        except OSError as error:
            self.skipTest(f'OS does not permit test symlinks: {error}')
        for location in ('binary', 'manifest', 'numbered_directory', 'label', 'artifacts'):
            with self.subTest(location=location):
                selected = {'binary': binary, 'manifest': directory / 'manifest.json',
                            'numbered_directory': binary.parent, 'label': directory,
                            'artifacts': directory.parent}[location]
                saved = selected.with_name(selected.name + '.saved')
                selected.rename(saved)
                selected.symlink_to(saved, target_is_directory=saved.is_dir())
                try:
                    with self.assertRaisesRegex(ValueError, 'link or reparse'):
                        cargo_run.resolve_labeled_binary(self.root, 'release-a', 'asset', 'release')
                finally:
                    selected.unlink()
                    saved.rename(selected)

    def test_label_resolver_rejects_missing_or_nonregular_preserved_binary(self):
        _, binary, _ = self.preserved_label()
        binary.unlink()
        with self.assertRaises(OSError):
            cargo_run.resolve_labeled_binary(self.root, 'release-a', 'asset', 'release')
        binary.mkdir()
        with self.assertRaisesRegex(ValueError, 'not a regular file'):
            cargo_run.resolve_labeled_binary(self.root, 'release-a', 'asset', 'release')

    def test_label_options_and_lookup_identifiers_are_validated(self):
        for label in ('', '../escape', '/absolute', 'a/b', 'a\\b', '.', '..', 'x' * 81):
            with self.subTest(label=label), self.assertRaises(ValueError):
                cargo_run.resolve_labeled_binary(self.root, label, 'asset', 'release')
        for name in ('../asset', '/asset', '', 'asset/other', 'C:asset'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                cargo_run.resolve_labeled_binary(self.root, 'release-a', name, 'release')
        with self.assertRaises(ValueError):
            cargo_run.resolve_labeled_binary(self.root, 'release-a', 'asset', 'custom')
        for args in (['--from-label', 'release-a'], ['--from-label', 'release-a', '--', 'build'],
                     ['--resolve', 'asset', '--from-label', 'release-a']):
            with self.subTest(args=args), redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                cargo_run.main(args)



if __name__ == '__main__':
    unittest.main()
