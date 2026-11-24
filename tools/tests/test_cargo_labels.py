"""Exact-label retirement preserves evidence and fails before unsafe deletion."""
import hashlib
import json
import os
import shutil
import sys
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from tools import _cargo_labels as labels
from tools import cargo_run


@unittest.skipUnless(sys.platform in {'darwin', 'linux'},
                     'saved-build retirement requires POSIX inspection and durable directory fsync')
class LabelRetirementTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        self.store, _ = cargo_run.build_store(self.root)
        idle = patch.object(cargo_run, 'build_processes', return_value=[])
        idle.start()
        self.addCleanup(idle.stop)
        inspection = patch.object(labels, '_idle')
        self.idle = inspection.start()
        self.addCleanup(inspection.stop)

    def label(self, name):
        directory = self.store / 'artifacts' / name
        binary = directory / '0' / 'game'
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b'saved executable ' + name.encode())
        manifest = directory / 'manifest.json'
        manifest.write_text(json.dumps({
            'schema': 1, 'target_dir': str(self.root / 'target'),
            'checkout': str(self.root), 'source': {'head': 'original-source-identity'},
            'artifacts': [{'file': '0/game', 'source': str(self.root / 'target/release/game'),
                           'sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}],
        }, indent=3) + '\n')
        return binary, manifest

    def retire(self, names, dry_run=False):
        return labels.retire(self.root, names, 0, dry_run)

    def test_exact_retirement_preserves_original_manifest_and_unselected_data(self):
        binary, manifest = self.label('old')
        original = manifest.read_bytes()
        kept_binary, kept_manifest = self.label('final')
        evidence = self.root / 'native-evidence.json'
        evidence.write_text('native data')
        cache = self.root / 'target' / 'cache.o'
        cache.parent.mkdir()
        cache.write_bytes(b'compiler object')
        protected = {path: path.read_bytes() for path in (kept_binary, kept_manifest, evidence, cache)}
        result = self.retire(['old'])
        self.assertEqual(result['state'], 'retired', result)
        self.assertEqual(result['removed_labels'], ['old'])
        self.assertFalse(manifest.parent.exists())
        receipt = json.loads(Path(result['receipt_path']).read_text())
        self.assertEqual(receipt['builds'][0]['manifest_text'].encode(), original)
        self.assertEqual(receipt['builds'][0]['manifest_sha256'], hashlib.sha256(original).hexdigest())
        self.assertEqual(set(receipt['removed_files']), {str(binary), str(manifest)})
        self.assertGreater(receipt['removed_allocated_bytes'], 0)
        self.assertEqual({path: path.read_bytes() for path in protected}, protected)

    def test_dry_run_does_not_write_receipt_or_change_labels(self):
        binary, manifest = self.label('old')
        original = {path: path.read_bytes() for path in (binary, manifest)}
        result = self.retire(['old'], True)
        self.assertEqual(result['state'], 'planned')
        self.assertEqual(result['removed_files'], [])
        self.assertEqual(result['removed_allocated_bytes'], 0)
        self.assertNotIn('receipt_path', result)
        self.assertFalse((self.store / 'label-retirements').exists())
        self.assertEqual({path: path.read_bytes() for path in original}, original)

    def test_invalid_selection_is_rejected_before_lock_or_any_deletion(self):
        binary, _ = self.label('old')
        for selection in ([], ['old', 'old'], ['old*'], ['../old']):
            with self.subTest(selection=selection), self.assertRaises(ValueError):
                self.retire(selection)
        self.assertTrue(binary.exists())

    def test_all_labels_preflight_before_deleting_the_first(self):
        binary, _ = self.label('first')
        corrupt, _ = self.label('corrupt')
        corrupt.write_bytes(b'corrupt')
        result = self.retire(['first', 'corrupt'])
        self.assertEqual(result['state'], 'blocked', result)
        self.assertTrue(binary.exists())
        self.assertEqual(result['removed_files'], [])

    def test_unexpected_files_and_empty_directories_block(self):
        for name, is_dir in [('file', False), ('directory', True)]:
            with self.subTest(name=name):
                binary, manifest = self.label(name)
                extra = manifest.parent / 'evidence'
                extra.mkdir() if is_dir else extra.write_bytes(b'evidence')
                result = self.retire([name])
                self.assertEqual(result['state'], 'blocked', result)
                self.assertTrue(binary.exists())
                self.assertTrue(extra.exists())

    def test_links_and_shared_inodes_are_never_deleted(self):
        binary, manifest = self.label('hardlink')
        outside = self.root / 'shared'
        os.link(binary, outside)
        result = self.retire(['hardlink'])
        self.assertEqual(result['state'], 'blocked', result)
        self.assertTrue(binary.exists())
        self.assertTrue(outside.exists())
        linked, _ = self.label('symlink')
        linked.unlink()
        linked.symlink_to(outside)
        result = self.retire(['symlink'])
        self.assertEqual(result['state'], 'blocked', result)
        self.assertTrue(linked.is_symlink())

    def test_active_executable_blocks_every_label(self):
        first, _ = self.label('first')
        second, _ = self.label('second')
        self.idle.side_effect = ValueError('Selected saved-build files are in use')
        result = self.retire(['first', 'second'])
        self.assertEqual(result['state'], 'blocked')
        self.assertTrue(first.exists())
        self.assertTrue(second.exists())

    def test_receipt_is_durable_before_unlink_and_partial_failure_is_recorded(self):
        binary, manifest = self.label('old')
        original_unlink = Path.unlink
        observed = []

        def unlink(path, *args, **kwargs):
            if path in (binary, manifest):
                receipts = list((self.store / 'label-retirements').glob('*.json'))
                self.assertEqual(len(receipts), 1)
                receipt = json.loads(receipts[0].read_text())
                self.assertEqual(receipt['builds'][0]['manifest_text'], manifest.read_text())
                observed.append(path)
                if path == manifest:
                    raise PermissionError('simulated manifest deletion denied')
            return original_unlink(path, *args, **kwargs)

        with patch.object(Path, 'unlink', unlink):
            result = self.retire(['old'])
        self.assertEqual(observed, [binary, manifest])
        self.assertEqual(result['state'], 'partial', result)
        self.assertEqual(result['removed_files'], [str(binary)])
        self.assertTrue(manifest.exists())
        self.assertEqual(json.loads(Path(result['receipt_path']).read_text())['state'], 'partial')

    def test_no_deletion_when_receipt_cannot_be_preserved(self):
        binary, manifest = self.label('old')
        with patch.object(labels, '_durable_receipt', side_effect=OSError('disk full')):
            result = self.retire(['old'])
        self.assertEqual(result['state'], 'blocked', result)
        self.assertEqual(result['removed_files'], [])
        self.assertTrue(binary.exists())
        self.assertTrue(manifest.exists())

    def test_final_measurement_failure_is_saved_in_partial_receipt(self):
        binary, manifest = self.label('old')
        with patch.object(labels.shutil, 'disk_usage', side_effect=[
                SimpleNamespace(free=100), OSError('measurement unavailable')]):
            result = self.retire(['old'])
        self.assertEqual(result['state'], 'partial', result)
        self.assertEqual(result['removed_labels'], ['old'])
        self.assertFalse(binary.exists())
        self.assertFalse(manifest.exists())
        self.assertIsNone(result['observed_free_delta_bytes'])
        receipt = json.loads(Path(result['receipt_path']).read_text())
        self.assertEqual(receipt['state'], 'partial')
        self.assertIn('Cannot measure final free space', receipt['errors'][0])

    def test_files_changed_after_inspection_block_before_unlink(self):
        binary, manifest = self.label('old')
        self.idle.side_effect = lambda paths: binary.write_bytes(b'changed after hashing')
        result = self.retire(['old'])
        self.assertEqual(result['state'], 'blocked', result)
        self.assertTrue(manifest.exists())
        self.assertEqual(result['removed_files'], [])


class ProcessInspectionTests(unittest.TestCase):
    def test_idle_check_requires_clean_lsof_no_matches(self):
        with patch.object(labels.sys, 'platform', 'linux'), patch.object(labels.shutil, 'which', return_value='/usr/bin/lsof'):
            for result, succeeds in [
                (SimpleNamespace(stdout='', stderr='', returncode=1), True),
                (SimpleNamespace(stdout='p123\nn/executable', stderr='', returncode=0), False),
                (SimpleNamespace(stdout='', stderr='permission denied', returncode=1), False),
                (SimpleNamespace(stdout='', stderr='', returncode=2), False),
                (SimpleNamespace(stdout='', stderr='', returncode=0), False),
            ]:
                with self.subTest(result=result), patch.object(labels.subprocess, 'run', return_value=result):
                    if succeeds:
                        labels._idle([Path('/executable')])
                    else:
                        with self.assertRaises(ValueError):
                            labels._idle([Path('/executable')])

    @unittest.skipUnless(sys.platform in {'darwin', 'linux'} and shutil.which('lsof') and Path('/bin/sleep').exists()
                         and (sys.platform == 'linux' or shutil.which('codesign')),
                         'requires POSIX sleep and lsof, and codesign on macOS')
    def test_real_running_executable_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary).resolve() / 'sleep-copy'
            shutil.copyfile('/bin/sleep', binary)
            binary.chmod(0o755)
            if sys.platform == 'darwin':
                # macOS launch constraints kill a copied Apple binary at exec. An
                # ad-hoc signature makes it an ordinary executable, like a Cargo build.
                subprocess.run(['codesign', '--force', '--sign', '-', str(binary)],
                               check=True, capture_output=True)
            child = subprocess.Popen([str(binary), '30'])
            try:
                with self.assertRaisesRegex(ValueError, 'in use'):
                    labels._idle([binary])
            finally:
                child.terminate()
                child.wait(timeout=5)
            labels._idle([binary])

    def test_unsupported_or_missing_inspection_fails_closed(self):
        with patch.object(labels.sys, 'platform', 'win32'), self.assertRaises(ValueError):
            labels._idle([Path('game.exe')])
        with patch.object(labels.sys, 'platform', 'linux'), patch.object(labels.shutil, 'which', return_value=None), self.assertRaises(ValueError):
            labels._idle([Path('/game')])


if __name__ == '__main__':
    unittest.main()
