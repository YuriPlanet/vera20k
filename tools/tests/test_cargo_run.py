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

REPOSITORY = Path(cargo_run.__file__).resolve().parents[1]
# Stands in for a test executable: records whether each lock is free, where it
# runs and one variable Cargo set, optionally edits a source, then exits.
PROBE = '''
import json, os, sys, time
from pathlib import Path
from tools.cargo_run import file_lock

def free(path):
    try:
        with file_lock(Path(path), time.monotonic()):
            return True
    except TimeoutError:
        return False

shared, checkout, report, status, edit = sys.argv[1:]
with open(report, 'a') as handle:
    handle.write(json.dumps({'shared': free(shared), 'checkout': free(checkout), 'cwd': os.getcwd(),
                             'package': os.environ.get('CARGO_PKG_NAME')}) + '\\n')
if edit:
    Path(edit).write_text('edited during the tests')
sys.exit(int(status))
'''


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

    def test_core_only_tests_build_with_the_workspace_core_features(self):
        features = ['--features', 'dev-overlays,test-support']
        cases = (
            (['test', '-p', 'vera20k-core', '--lib', 'sim::'], features),
            (['test', '--package=vera20k-core', '--lib'], features),
            (['test', '-pvera20k-core', '--lib'], features),
            (['test', '-p', 'vera20k', '--lib'], []),
            (['test', '--workspace', '--lib'], []),
            (['test', '-p', 'vera20k-core', '-p', 'vera20k', '--lib'], []),
            (['test', '-p', 'vera20k-core', '--features', 'dev-overlays', '--lib'], []),
            (['test', '-p', 'vera20k-core', '--no-default-features', '--lib'], []),
            (['test', '-p', 'vera20k-core', '--features', 'dev-overlays,test-support',
              '--lib', '--', '--ignored', '--nocapture'], []),
            (['test', '-p', 'vera20k-core', '--all-features', '--lib', '--', '--exact'], []),
            (['build', '-p', 'vera20k-core', '--release'], []),
        )
        for args, added in cases:
            with self.subTest(args=args):
                rendered = cargo_run.cargo_args(args, None)
                self.assertEqual(rendered, [args[0], '--message-format=json-render-diagnostics', *args[1:], *added])

    def test_automatic_core_features_stay_before_libtest_separator(self):
        # Cargo consumes the feature selection; everything after -- belongs
        # to libtest. Issue #1841 built successfully, then ran zero assertions.
        options = ['test', '-p', 'vera20k-core', '--lib', 'wall_sale_detach']
        for libtest_args in (['--nocapture'], ['--ignored', '--nocapture'],
                             ['--exact', '--nocapture'], []):
            with self.subTest(libtest_args=libtest_args):
                rendered = cargo_run.cargo_args([*options, '--', *libtest_args], None)
                separator = rendered.index('--')
                self.assertEqual(rendered[separator + 1:], libtest_args)
                self.assertEqual(rendered[:separator], [
                    'test', '--message-format=json-render-diagnostics',
                    '-p', 'vera20k-core', '--lib', 'wall_sale_detach',
                    '--features', 'dev-overlays,test-support',
                ])

    def test_core_only_and_workspace_tests_resolve_core_dependencies_alike(self):
        # Any feature difference gives `-p vera20k-core` its own core test
        # executable; src/Cargo.toml's dev-dependencies must then gain it.
        def tree(*selection):
            listing = subprocess.run(
                ['cargo', 'tree', '--locked', '--offline', *selection, '-e', 'normal,build,dev',
                 '--prefix', 'none', '--no-dedupe', '-f', '{p} {f}'],
                cwd=REPOSITORY, check=True, capture_output=True, text=True).stdout
            features = {}
            for line in filter(None, listing.splitlines()):
                package, version, *rest = line.replace(' (*)', '').split(' ')
                if package != 'vera20k' and not rest[:1] == ['(proc-macro)']:
                    features[package, version] = rest[-1] if rest and not rest[-1].startswith('(') else ''
            return features
        core = tree('-p', 'vera20k-core', '--features', ','.join(cargo_run.WORKSPACE_CORE_FEATURES))
        workspace = tree('--workspace')
        self.assertEqual({key: value for key, value in workspace.items() if key in core}, core)

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

    def invoke(self, label='build-a', *, root=None, **options):
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
            return cargo_run.run(root or self.root, ['build'], label, 0)

    def test_worktree_builds_into_one_cache_under_the_primary_checkout(self):
        # Sessions differ in CARGO_TARGET_DIR (Claude sets the primary's target/,
        # Codex sets nothing); a checkout built from both must share one cache.
        home = tempfile.TemporaryDirectory()
        self.addCleanup(home.cleanup)
        worktree = Path(home.name, 'worktree').resolve()
        subprocess.run(['git', '-C', str(self.root), 'worktree', 'add', '-q', str(worktree)], check=True)
        store, namespace = cargo_run.build_store(worktree)
        self.assertEqual(store, self.root / '.git' / 'owned-builds')
        self.assertNotEqual(namespace, cargo_run.build_store(self.root)[1])
        shared = self.root / 'target' / 'owned-worktrees' / namespace
        self.assertEqual(cargo_run.owned_target(worktree, namespace), shared)
        os.environ['CARGO_TARGET_DIR'] = str(self.root / 'target')
        self.assertEqual(cargo_run.owned_target(worktree, namespace), shared)
        os.environ['CARGO_TARGET_DIR'] = str(Path(home.name, 'elsewhere'))
        self.assertEqual(cargo_run.owned_target(worktree, namespace),
                         Path(home.name, 'elsewhere').resolve() / 'owned-worktrees' / namespace)
        del os.environ['CARGO_TARGET_DIR']
        with redirect_stderr(io.StringIO()):
            self.assertEqual(self.invoke(None, root=worktree), 0)
        self.assertTrue((shared / 'debug').is_dir())
        self.assertFalse((worktree / 'target').exists())
        registry = json.loads((store / 'cache-roots.json').read_text())
        self.assertEqual(registry['roots'][str(shared)]['checkout'], str(worktree))
        # A cache built under another root (an older runner, or a session whose
        # environment points elsewhere) is reported, never removed by the build.
        from tools import _cargo_cache as cache
        legacy = worktree / 'target' / 'owned-worktrees' / namespace
        self.assertEqual(cache.register_locked(worktree, store, legacy), [shared])
        with redirect_stderr(io.StringIO()) as stderr:
            self.assertEqual(self.invoke(None, root=worktree), 0)
        self.assertIn(f'also has caches at {legacy}', stderr.getvalue())
        self.assertTrue(legacy.is_dir())

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

    def run_tests(self, calls, *options):
        """`cargo test` whose Cargo hands each (status, edited path) call to its runner."""
        store, namespace = cargo_run.build_store(self.root)
        package = self.root / 'crate'
        package.mkdir(exist_ok=True)
        report = self.root / 'target' / 'report.jsonl'  # Ignored: not a source edit.
        report.parent.mkdir(exist_ok=True)
        report.unlink(missing_ok=True)
        commands = []
        actual_popen = subprocess.Popen
        def start(command, **kwargs):
            if command[0] != 'cargo':
                return actual_popen(command, **kwargs)
            commands.append(command)
            if '--config' in command:
                key, value = command[command.index('--config') + 1].split('=', 1)
                self.assertEqual(key, 'target."cfg(all())".runner')
                env = dict(kwargs['env'], CARGO_PKG_NAME='fixture', PYTHONPATH=str(REPOSITORY))
                for status, edit in calls:
                    subprocess.run([*json.loads(value), sys.executable, '-c', PROBE, str(store / 'cargo.lock'),
                                    str(store / 'checkouts' / f'{namespace}.lock'), str(report), str(status), edit],
                                   cwd=package, env=env, check=True)
            child = unittest.mock.MagicMock()
            child.__enter__.return_value = child
            child.stdout = io.StringIO('')
            child.wait.return_value = 0
            return child
        with patch.object(cargo_run, 'build_processes', return_value=[]), \
             patch.object(cargo_run.subprocess, 'Popen', side_effect=start):
            result = cargo_run.run(self.root, ['test', '--lib', *options], None, 0)
        reports = [json.loads(line) for line in report.read_text().splitlines()] if report.exists() else []
        return result, commands, reports

    def test_every_run_ends_by_reporting_where_its_time_went(self):
        summary = (r'cargo_run: [\d.]+ s in all: [\d.]+ s waiting for the build lock, '
                   r'[\d.]+ s cache retention, [\d.]+ s Cargo, [\d.]+ s tests')
        for calls in ([(0, '')], [(3, '')]):
            with self.subTest(calls=calls), redirect_stderr(io.StringIO()) as stderr:
                self.run_tests(calls)
            self.assertRegex(stderr.getvalue().splitlines()[-1], '^' + summary + '$')
        with redirect_stderr(io.StringIO()) as stderr, self.assertRaises(ValueError):
            self.run_tests([(0, str(self.root / 'source.rs'))])
        self.assertRegex(stderr.getvalue().splitlines()[-1], '^' + summary + '$')

    def test_tests_run_after_releasing_the_shared_lock_but_not_the_checkout_lock(self):
        result, _, reports = self.run_tests([(0, '')])
        self.assertEqual(result, 0)
        self.assertEqual(reports, [{'shared': True, 'checkout': False,
                                    'cwd': str(self.root / 'crate'), 'package': 'fixture'}])
        result, commands, reports = self.run_tests([(0, '')], '--no-run')
        self.assertEqual((result, reports), (0, []))
        self.assertNotIn('--config', commands[0])

    def test_failing_test_fails_the_run_after_every_executable_ends(self):
        for options in ((), ('--no-fail-fast',)):
            with self.subTest(options=options):
                result, _, reports = self.run_tests([(3, ''), (0, '')], *options)
                self.assertEqual((result, len(reports)), (101, 2))
                result, _, reports = self.run_tests([(0, ''), (3, '')], *options)
                self.assertEqual((result, len(reports)), (101, 2))

    def test_source_edit_during_the_tests_fails_validation(self):
        with self.assertRaisesRegex(ValueError, 'Source changed during the tests'):
            self.run_tests([(0, str(self.root / 'source.rs'))])

    def test_tests_another_runner_already_ran_keep_cargo_result(self):
        self.assertEqual(self.run_tests([])[0::2], (0, []))

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

    def test_fast_profile_records_and_resolves_as_a_host_profile(self):
        fast = self.recorded_binary('fast')
        self.assertEqual(self.resolve_cli('--resolve', 'asset', '--profile', 'fast'),
                         (0, str(fast) + '\n', ''))
        self.assertEqual(cargo_run.resolve_binary(self.root, 'asset'), (fast, 'fast'))
        # An omitted profile still prefers release.
        release = self.recorded_binary('release')
        self.assertEqual(cargo_run.resolve_binary(self.root, 'asset'), (release, 'release'))
        _, binary, _ = self.preserved_label('fast-a', source_suffix='fast')
        self.assertEqual(cargo_run.resolve_labeled_binary(self.root, 'fast-a', 'asset', 'fast'),
                         (binary, 'fast'))
        with self.assertRaisesRegex(ValueError, 'not a release host bin'):
            cargo_run.resolve_labeled_binary(self.root, 'fast-a', 'asset', 'release')
        # Without a profile the label's own host profile is returned.
        self.assertEqual(cargo_run.resolve_labeled_binary(self.root, 'fast-a', 'asset', None),
                         (binary, 'fast'))
        self.preserved_label('deps-a', source_suffix='fast/deps')
        with self.assertRaisesRegex(ValueError, 'not a host bin'):
            cargo_run.resolve_labeled_binary(self.root, 'deps-a', 'asset', None)

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
