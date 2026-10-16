"""Reviewed legacy libtest export retirement fails closed and preserves evidence."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import stat
import sys
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

from tools import _cargo_labels as labels, cargo_run


@unittest.skipUnless(sys.platform in {'darwin', 'linux'},
                     'retirement transaction fixtures require POSIX directory fsync')
class ExportRetirementTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        base = Path(self.temp.name).resolve()
        self.root = base / 'checkout'
        self.root.mkdir()
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        self.export = base / 'evidence'
        self.export.mkdir()
        self.store, _ = cargo_run.build_store(self.root)
        self.binary = self.export / 'old-libtests'
        self.binary.write_bytes(b'old Rust test executable')
        self.binary.chmod(0o755)
        self.debug = base / 'compiler-debug.o'
        self.debug.write_bytes(b'keep debug dependency')
        self.replacement = self.store / 'artifacts/final-tests/0/vera20k-abc'
        self.replacement.parent.mkdir(parents=True)
        self.replacement.write_bytes(b'final Rust test executable')
        self.replacement.chmod(0o755)
        self.manifest = self.replacement.parent.parent / 'manifest.json'
        self.manifest.write_text(json.dumps({
            'schema': 1, 'checkout': str(self.root),
            'source': {'source_sha256': 'a' * 64},
            'target_dir': str(self.root / 'target'),
            'command': ['cargo', 'test', '-p', 'vera20k', '--lib', '--no-run'],
            'artifacts': [{'file': '0/vera20k-abc', 'source': str(self.root / 'target/debug/deps/vera20k-abc'),
                           'sha256': self.sha(self.replacement)}],
        }))
        self.provenance = self.export / 'original-build.json'
        self.provenance.write_text(json.dumps({'checkout': str(self.root), 'binary_sha256': self.sha(self.binary)}))
        self.results = self.export / 'native-results.json'
        self.results.write_text('{"results": "keep native evidence"}')
        self.plan_path = base / 'plan.json'
        self.plan = {'schema': 1, 'checkout': str(self.root), 'export_root': str(self.export), 'files': [{
            'file': 'old-libtests', 'sha256': self.sha(self.binary), 'reason': 'Reviewed superseded test validation',
            'consumer_status': 'reviewed-no-active-or-required-consumers',
            'provenance': [self.reference(self.provenance)], 'results': [self.reference(self.results)],
            'replacement': {'label': 'final-tests', 'file': '0/vera20k-abc',
                            'sha256': self.sha(self.replacement), 'manifest_sha256': self.sha(self.manifest),
                            'purpose': 'vera20k-libtest'},
        }]}
        for owner, name, value in [(cargo_run, 'build_processes', []), (labels, '_idle', None),
                                    (labels, '_libtest', {self.debug})]:
            mock = patch.object(owner, name, return_value=value)
            result = mock.start()
            setattr(self, name, result)
            self.addCleanup(mock.stop)

    @staticmethod
    def sha(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()

    def reference(self, path):
        return {'path': str(path), 'sha256': self.sha(path)}

    def run_plan(self, dry_run=False):
        self.plan_path.write_text(json.dumps(self.plan))
        return labels.retire_exports(self.root, self.plan_path, 0, dry_run)

    def test_only_exact_binary_deleted_and_durable_provenance_retained(self):
        other = self.export / 'unselected-libtests'
        other.write_bytes(b'another task')
        asset = self.export / 'assets.mix'
        asset.write_bytes(b'retail asset')
        protected = {p: p.read_bytes() for p in (other, asset, self.provenance, self.results,
                                                 self.replacement, self.manifest, self.debug)}
        unlink = Path.unlink
        def observe(path, *args, **kwargs):
            if path == self.binary:
                records = list((self.store / 'label-retirements').glob('*.json'))
                self.assertEqual(len(records), 1)
                record = json.loads(records[0].read_text())
                self.assertEqual(record['plan_text'], self.plan_path.read_text())
                self.assertEqual(record['files'][0]['replacement_manifest_text'], self.manifest.read_text())
                self.assertEqual(record['files'][0]['review']['provenance'], [self.reference(self.provenance)])
            return unlink(path, *args, **kwargs)
        with patch.object(Path, 'unlink', observe):
            result = self.run_plan()
        self.assertEqual(result['state'], 'retired', result)
        self.assertEqual(result['removed_files'], [str(self.binary)])
        self.assertGreater(result['removed_allocated_bytes'], 0)
        self.assertIn('observed_free_delta_bytes', result)
        self.assertEqual(protected, {p: p.read_bytes() for p in protected})
        self.assertTrue(self.binary.parent.exists())
        self.assertEqual(json.loads(Path(result['receipt_path']).read_text())['state'], 'retired')

    def test_dry_run_writes_nothing(self):
        result = self.run_plan(True)
        self.assertEqual(result['state'], 'planned', result)
        self.assertTrue(self.binary.exists())
        self.assertFalse((self.store / 'label-retirements').exists())

    def test_path_escape_links_apps_globs_duplicates_and_unknown_fields_block(self):
        for name in ('../old-libtests', '/old-libtests', './old-libtests', 'old*', 'old\\libtests',
                     'Game.app/old-libtests', 'a//old-libtests'):
            with self.subTest(name=name):
                self.plan['files'][0]['file'] = name
                self.assertEqual(self.run_plan()['state'], 'blocked')
        self.plan['files'][0]['file'] = 'old-libtests'
        self.plan['files'].append(dict(self.plan['files'][0]))
        self.assertEqual(self.run_plan()['state'], 'blocked')
        self.plan['files'].pop()
        self.plan['unexpected'] = True
        self.assertEqual(self.run_plan()['state'], 'blocked')
        self.assertTrue(self.binary.exists())

    def test_hardlink_and_symlink_block(self):
        alias = self.export / 'alias'
        os.link(self.binary, alias)
        self.assertEqual(self.run_plan()['state'], 'blocked')
        alias.unlink()
        self.binary.rename(alias)
        self.binary.symlink_to(alias)
        self.assertEqual(self.run_plan()['state'], 'blocked')
        self.assertTrue(alias.exists())

    def test_missing_dependency_or_replacement_blocks(self):
        self.debug.unlink()
        self.assertEqual(self.run_plan()['state'], 'blocked')
        self.debug.write_bytes(b'keep debug dependency')
        self.replacement.unlink()
        self.assertEqual(self.run_plan()['state'], 'blocked')
        self.assertTrue(self.binary.exists())

    def test_provenance_corruption_and_wrong_checkout_block(self):
        self.provenance.write_text('corrupt')
        self.assertEqual(self.run_plan()['state'], 'blocked')
        self.plan['files'][0]['provenance'] = [self.reference(self.provenance)]
        self.assertEqual(self.run_plan()['state'], 'blocked')
        self.plan['checkout'] = '/other/checkout'
        self.assertEqual(self.run_plan()['state'], 'blocked')
        self.assertTrue(self.binary.exists())

    def test_canonical_replacement_purpose_and_manifest_corruption_block(self):
        self.plan['files'][0]['replacement']['purpose'] = 'game-capture'
        self.assertEqual(self.run_plan()['state'], 'blocked')
        self.plan['files'][0]['replacement']['purpose'] = 'vera20k-libtest'
        self.manifest.write_text(self.manifest.read_text().replace('--lib', '--bins'))
        self.plan['files'][0]['replacement']['manifest_sha256'] = self.sha(self.manifest)
        self.assertEqual(self.run_plan()['state'], 'blocked')

    def test_engine_crate_replacement_is_a_libtest_build(self):
        self.manifest.write_text(self.manifest.read_text().replace('"vera20k"', '"vera20k-core"'))
        self.plan['files'][0]['replacement']['manifest_sha256'] = self.sha(self.manifest)
        self.assertEqual(self.run_plan(True)['state'], 'planned')
        self.manifest.write_text(self.manifest.read_text().replace('"vera20k-core"', '"vera20k-ui"'))
        self.plan['files'][0]['replacement']['manifest_sha256'] = self.sha(self.manifest)
        self.assertEqual(self.run_plan(True)['state'], 'blocked')

    def test_busy_files_block_all_selected(self):
        self._idle.side_effect = ValueError('open consumer')
        self.assertEqual(self.run_plan()['state'], 'blocked')
        self.assertTrue(self.binary.exists())

    def test_changed_file_or_lost_replacement_after_preflight_blocks(self):
        for path in (self.binary, self.replacement, self.provenance):
            original = path.read_bytes()
            self._idle.side_effect = lambda paths: path.write_bytes(b'changed')
            self.assertEqual(self.run_plan()['state'], 'blocked')
            path.write_bytes(original)
        self.assertTrue(self.binary.exists())

    def test_all_files_preflight_before_first_unlink(self):
        second = dict(self.plan['files'][0], file='missing-libtests')
        self.plan['files'].append(second)
        result = self.run_plan()
        self.assertEqual(result['state'], 'blocked')
        self.assertEqual(result['removed_files'], [])
        self.assertTrue(self.binary.exists())

    def test_receipt_failure_cannot_delete(self):
        with patch.object(labels, '_durable_receipt', side_effect=OSError('disk full')):
            self.assertEqual(self.run_plan()['state'], 'blocked')
        self.assertTrue(self.binary.exists())

    def test_lost_replacement_after_durable_plan_blocks_before_unlink(self):
        count = 0
        def idle(paths):
            nonlocal count
            count += 1
            if count == 2:
                self.replacement.unlink()
        self._idle.side_effect = idle
        result = self.run_plan()
        self.assertEqual(result['state'], 'blocked', result)
        self.assertEqual(result['removed_files'], [])
        self.assertTrue(self.binary.exists())
        self.assertEqual(json.loads(Path(result['receipt_path']).read_text())['state'], 'blocked')

    def test_partial_failure_records_only_first_actual_unlink(self):
        second = self.export / 'second-libtests'
        second.write_bytes(self.binary.read_bytes())
        second.chmod(0o755)
        self.plan['files'].append(dict(self.plan['files'][0], file=second.name))
        unlink = Path.unlink
        def fail_second(path, *args, **kwargs):
            if path == second:
                raise PermissionError('denied exact second file')
            return unlink(path, *args, **kwargs)
        with patch.object(Path, 'unlink', fail_second):
            result = self.run_plan()
        self.assertEqual(result['state'], 'partial', result)
        self.assertEqual(result['removed_files'], [str(self.binary)])
        self.assertTrue(second.exists())
        record = json.loads(Path(result['receipt_path']).read_text())
        self.assertEqual(record['removed_files'], [str(self.binary)])
        self.assertEqual(record['state'], 'partial')

    def test_selection_cannot_delete_other_selected_debug_dependency(self):
        self._libtest.return_value = {self.binary}
        result = self.run_plan()
        self.assertEqual(result['state'], 'blocked', result)
        self.assertIn('overlaps', result['errors'][0])
        self.assertTrue(self.binary.exists())

    def test_progress_receipt_failure_stops_after_first_file_and_records_partial(self):
        second = self.export / 'second-libtests'
        second.write_bytes(self.binary.read_bytes())
        second.chmod(0o755)
        self.plan['files'].append(dict(self.plan['files'][0], file=second.name))
        durable = labels._durable_receipt
        count = 0
        def failure(path, value):
            nonlocal count
            count += 1
            if count == 2:
                raise OSError('progress fsync failed')
            return durable(path, value)
        with patch.object(labels, '_durable_receipt', failure):
            result = self.run_plan()
        self.assertEqual(result['state'], 'partial', result)
        self.assertEqual(result['removed_files'], [str(self.binary)])
        self.assertTrue(second.exists())
        self.assertEqual(json.loads(Path(result['receipt_path']).read_text())['state'], 'partial')

    def test_shared_replacement_reuses_inspection_but_keeps_first_identity(self):
        second = self.export / 'second-libtests'
        second.write_bytes(self.binary.read_bytes())
        second.chmod(0o755)
        self.plan['files'].append(dict(self.plan['files'][0], file=second.name))
        with patch.object(labels, '_replacement', wraps=labels._replacement) as inspect:
            result = self.run_plan(True)
        self.assertEqual(result['state'], 'planned', result)
        self.assertEqual(inspect.call_count, 1)
        def changed_during_second_export(binary):
            if binary == second:
                self.replacement.write_bytes(b'changed retained replacement')
            return {self.debug}
        self._libtest.side_effect = changed_during_second_export
        result = self.run_plan()
        self.assertEqual(result['state'], 'blocked', result)
        self.assertEqual(result['removed_files'], [])
        self.assertTrue(self.binary.exists())
        self.assertTrue(second.exists())

    def test_plan_duplicate_fields_block(self):
        self.plan_path.write_text('{"schema":1,"schema":1}')
        self.assertEqual(labels.retire_exports(self.root, self.plan_path, 0)['state'], 'blocked')

    def test_unsupported_or_ambiguous_executable_stops_deletion(self):
        self._libtest.side_effect = ValueError('unsupported PE or ambiguous binary')
        self.assertEqual(self.run_plan()['state'], 'blocked')
        self.assertTrue(self.binary.exists())


class ExportFormatTests(unittest.TestCase):
    def test_non_native_non_executable_and_missing_symbols_fail_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary).resolve() / 'candidate'
            for payload in (b'MZ00', b'#!/bin/sh', b'retail asset', b'\x7fELF'):
                binary.write_bytes(payload)
                binary.chmod(0o755)
                with patch.object(labels.sys, 'platform', 'darwin'), self.assertRaises(ValueError):
                    labels._libtest(binary)
            binary.write_bytes(b'\xcf\xfa\xed\xfe')
            binary.chmod(0o644)
            with patch.object(labels.sys, 'platform', 'darwin'), self.assertRaisesRegex(ValueError, 'not executable'):
                labels._libtest(binary)
            binary.chmod(0o755)
            with (patch.object(labels.sys, 'platform', 'darwin'),
                  patch.object(labels, '_regular', return_value=SimpleNamespace(st_mode=stat.S_IFREG | 0o755)),
                  patch.object(labels.shutil, 'which', return_value=None)):
                with self.assertRaisesRegex(ValueError, 'requires native nm'):
                    labels._libtest(binary)
            for platform in ('linux', 'win32'):
                with patch.object(labels.sys, 'platform', platform), self.assertRaises(ValueError):
                    labels._libtest(binary)

    def test_symbols_need_defined_rust_harness_and_project_tests(self):
        from tools import _cargo_cache
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary).resolve() / 'candidate'
            binary.write_bytes(b'\xcf\xfa\xed\xfe')
            binary.chmod(0o755)
            def stream(command, consumer):
                self.assertEqual(command[1:3], ['-j', '-U'])
                return consumer(iter(['_ordinary_symbol\n']))
            with (patch.object(labels.sys, 'platform', 'darwin'),
                  patch.object(labels, '_regular', return_value=SimpleNamespace(st_mode=stat.S_IFREG | 0o755)),
                  patch.object(labels.shutil, 'which', return_value='/mock/native-nm'),
                  patch.object(_cargo_cache, '_stream', side_effect=stream)):
                with self.assertRaisesRegex(ValueError, 'Cannot identify'):
                    labels._libtest(binary)


if __name__ == '__main__':
    unittest.main()
