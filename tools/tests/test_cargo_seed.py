"""Seeding a new cache profile with another checkout's dependency outputs."""
from contextlib import ExitStack, redirect_stderr, redirect_stdout
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
from unittest.mock import MagicMock, patch

from tools import _cargo_seed as seeding
from tools._cargo_cache import CachePolicy
from tools import cargo_run

LOCK = '''version = 4

[[package]]
name = "app"
version = "0.1.0"
dependencies = ["dep"]

[[package]]
name = "dep"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
'''
DEP, RUN, SCRIPT, APP = '1111111111111111', '3333333333333333', '4444444444444444', '2222222222222222'
# Dependency outputs, then a workspace unit, its incremental session and a link.
SEEDED = [f'deps/libdep-{DEP}.rlib', f'deps/libdep-{DEP}.rmeta', f'deps/dep-{DEP}.d',
          f'deps/dep-{DEP}.dep.0abc-cgu.0.rcgu.o', f'build/dep-{RUN}/out/generated.rs',
          f'build/dep-{RUN}/output', f'build/dep-{SCRIPT}/build-script-build',
          f'.fingerprint/dep-{DEP}/lib-dep', f'.fingerprint/dep-{RUN}/run-build-script-build-script-build',
          f'.fingerprint/dep-{SCRIPT}/build-script-build-script-build']
LOCAL = [f'deps/libapp-{APP}.rlib', f'deps/app-{APP}', f'.fingerprint/app-{APP}/test-lib-app',
         'incremental/app-0abc/s-session/object.o']


class SeedTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name).resolve()
        self.root = self.base / 'checkout'
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        (self.root / 'Cargo.lock').write_text(LOCK)
        subprocess.run(['git', '-C', str(self.root), '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid',
                        'commit', '-qm', 'initial', '--allow-empty'], check=True)
        self.environment = patch.dict(os.environ)
        self.environment.start()
        self.addCleanup(self.environment.stop)
        os.environ.pop('CARGO_TARGET_DIR', None)
        self.store, namespace = cargo_run.build_store(self.root)
        self.target = cargo_run.owned_target(self.root, namespace)
        self.target.mkdir(parents=True)

    def warm(self, name='other', *, lock=LOCK, used=None, files=(*SEEDED, *LOCAL)):
        """A registered cache of another live checkout, built from `lock`."""
        checkout = self.base / name
        (checkout / '.git').mkdir(parents=True)
        (checkout / 'Cargo.lock').write_text(lock)
        cache = self.target.parent / hashlib.sha256(os.fsencode(str(checkout))).hexdigest()[:16]
        for index, relative in enumerate(files):
            path = cache / 'debug' / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(relative)
            os.utime(path, ns=(1_000_000_000 * (index + 1),) * 2)
        registry = self.store / 'cache-roots.json'
        data = json.loads(registry.read_text()) if registry.exists() else {'schema': 1, 'roots': {}}
        data['roots'][str(cache)] = {'checkout': str(checkout), 'profiles': ['debug'],
                                     'last_use_unix': time.time() if used is None else used}
        registry.parent.mkdir(parents=True, exist_ok=True)
        registry.write_text(json.dumps(data))
        return cache

    def copied(self):
        profile = self.target / 'debug'
        return sorted(path.relative_to(profile).as_posix() for path in profile.rglob('*') if path.is_file())

    def test_profile_directory_follows_cargo_profile_selection(self):
        for args, expected in ((['build'], 'debug'), (['test', '--lib'], 'debug'),
                               (['build', '--release'], 'release'), (['test', '-r'], 'release'),
                               (['build', '--profile', 'fast'], 'fast'), (['build', '--profile=fast'], 'fast'),
                               (['test', '--profile', 'test'], 'debug'), (['build', '--profile=bench'], 'release'),
                               (['build', '--profile', 'shipping'], None),
                               (['build', '--target', 'x86_64-pc-windows-msvc'], None),
                               (['build', '--target=wasm32-unknown-unknown'], None),
                               (['test', '--lib', '--', '--release'], 'debug')):
            with self.subTest(args=args):
                self.assertEqual(seeding.profile_directory(args), expected)

    def test_copies_dependency_units_with_their_timestamps_and_nothing_of_the_workspace(self):
        source = self.warm()
        (source / 'debug/deps' / f'libdep-{DEP}.so').symlink_to(source / 'debug/deps' / f'libdep-{DEP}.rlib')
        summary = seeding.seed(self.root, self.store, self.target, 'debug')
        self.assertIn('3 dependency units, 10 files', summary)
        self.assertEqual(self.copied(), sorted(SEEDED))
        for relative in SEEDED:
            original, copy = source / 'debug' / relative, self.target / 'debug' / relative
            self.assertEqual(copy.read_text(), relative)
            self.assertEqual(copy.stat().st_mtime_ns, original.stat().st_mtime_ns)
            self.assertNotEqual(copy.stat().st_ino, original.stat().st_ino)
        self.assertFalse((self.target / 'debug/deps' / f'libdep-{DEP}.so').exists())
        # The profile now exists: a later run never seeds it again.
        self.assertIsNone(seeding.seed(self.root, self.store, self.target, 'debug'))

    def test_fingerprints_follow_every_output_they_vouch_for(self):
        self.warm()
        order = []
        real = seeding.clone
        def record(source, destination):
            order.append(destination.relative_to(self.target / 'debug').parts[0])
            return real(source, destination)
        with patch.object(seeding, 'clone', side_effect=record):
            seeding.seed(self.root, self.store, self.target, 'debug')
        first = order.index('.fingerprint')
        self.assertEqual(set(order[first:]), {'.fingerprint'})
        self.assertEqual(order.count('.fingerprint'), 3)

    def test_source_is_a_live_checkouts_cache_preferring_the_same_lockfile(self):
        self.assertIsNone(seeding.seed(self.root, self.store, self.target, 'debug'))
        same = self.warm('same', used=time.time() - 3600)
        self.warm('newer', lock=LOCK.replace('1.0.0', '1.1.0'))
        removed = self.warm('removed', used=time.time() + 60)
        (self.base / 'removed' / '.git').rmdir()
        self.assertEqual(seeding.seed_source(self.root, self.store, self.target, 'debug'), same)
        self.assertIsNone(seeding.seed_source(self.root, self.store, self.target, 'fast'))
        (self.base / 'same' / '.git').rmdir()
        self.assertEqual(seeding.seed_source(self.root, self.store, self.target, 'debug').name,
                         hashlib.sha256(os.fsencode(str(self.base / 'newer'))).hexdigest()[:16])

    def test_a_misnamed_or_linked_cache_is_never_a_source(self):
        misnamed = self.warm('misnamed')
        misnamed.rename(misnamed.with_name('0123456789abcdef'))
        data = json.loads((self.store / 'cache-roots.json').read_text())
        data['roots'] = {str(misnamed.with_name('0123456789abcdef')): data['roots'][str(misnamed)]}
        (self.store / 'cache-roots.json').write_text(json.dumps(data))
        self.assertIsNone(seeding.seed_source(self.root, self.store, self.target, 'debug'))
        elsewhere = self.base / 'elsewhere'
        linked = self.warm('linked')
        (linked / 'debug').rename(elsewhere)
        (linked / 'debug').symlink_to(elsewhere, target_is_directory=True)
        self.assertIsNone(seeding.seed_source(self.root, self.store, self.target, 'debug'))

    def run_cargo(self, seed=None):
        """One successful runner invocation of `cargo check`, without Cargo."""
        child = MagicMock()
        child.__enter__.return_value = child
        child.stdout = io.StringIO('')
        child.wait.return_value = 0
        original = subprocess.Popen
        stderr = io.StringIO()
        patches = [patch.object(cargo_run, 'build_processes', return_value=[]),
                   patch.object(cargo_run.subprocess, 'Popen',
                                side_effect=lambda command, **kwargs: child if command[0] == 'cargo'
                                else original(command, **kwargs))]
        if seed:
            patches.append(patch.object(seeding, 'seed', side_effect=seed))
        with ExitStack() as stack:
            for active in patches:
                stack.enter_context(active)
            stack.enter_context(redirect_stderr(stderr))
            stack.enter_context(redirect_stdout(io.StringIO()))
            policy = CachePolicy(1 << 40, 1 << 40, 0)
            self.assertEqual(cargo_run.run(self.root, ['check'], None, 0, policy=policy), 0)
        return stderr.getvalue()

    def test_runner_seeds_a_new_profile_once_and_builds_after_a_failed_seed(self):
        self.warm()
        self.assertIn('Seeded debug from', self.run_cargo())
        self.assertEqual(self.copied(), sorted(SEEDED))
        self.assertNotIn('Seeded', self.run_cargo())
        (self.target / 'debug').rename(self.target / 'old')
        def fail(*args):
            raise OSError(28, 'No space left on device')
        self.assertIn('Cache seeding stopped', self.run_cargo(fail))


if __name__ == '__main__':
    unittest.main()
