"""Portable palette publication checks; synthetic opcodes are not native goldens."""
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from tools import native_oracle
from tools.palette_oracle import check, oracle
from tools.tests.pe_fixture import pe_image
from tools.tests.test_oracle_lifecycle import absent_retail_environment


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / 'tools/palette_oracle/fixtures'
ENTRY = 0x556192
FAILURES = (
    ('early stop', b'\xf4', 'Incomplete execution'),
    ('instruction budget', b'\xeb\xfe', 'Incomplete execution'),
    ('invalid instruction', b'\x0f\x0b', 'faulted'),
)


def synthetic_image(opcodes):
    # Include the constant base_scales reads before executing the first case.
    # Its supplied value is irrelevant: all three programs fail to complete.
    return pe_image([
        (ENTRY - native_oracle.IMAGE_BASE, 0x400, opcodes,
         len(opcodes), 0x60000020),
        (0x7ED0B0 - native_oracle.IMAGE_BASE, 0x500,
         struct.pack('<d', 0.065536), 8, 0x40000040),
        (0x822D80 - native_oracle.IMAGE_BASE, 0x600,
         struct.pack('<I', 0x0E7F), 4, 0x40000040),
    ])


def fixture_payloads():
    manifest = json.loads((FIXTURES / 'provenance.json').read_text())
    return {name: (FIXTURES / name).read_bytes() for name in manifest['fixtures']}


class PaletteOracleTests(unittest.TestCase):
    def test_early_stop_budget_and_fault_never_become_native_rows(self):
        for label, opcodes, message in FAILURES:
            with self.subTest(failure=label), patch.object(
                    native_oracle, 'image_bytes', return_value=synthetic_image(opcodes)):
                with self.assertRaisesRegex(native_oracle.OracleError, message):
                    oracle.base_scales()

    def test_failed_execution_does_not_create_an_export_directory(self):
        for label, opcodes, message in FAILURES:
            with self.subTest(failure=label), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / 'candidate'
                with patch.object(native_oracle, 'image_bytes',
                                  return_value=synthetic_image(opcodes)), \
                     patch.object(native_oracle, 'configured_gamemd',
                                  return_value=Path('synthetic-test-image.exe')):
                    with self.assertRaisesRegex(native_oracle.OracleError, message):
                        oracle.main(['--write', '--output', str(output)])
                self.assertEqual(list(Path(directory).iterdir()), [])

    def test_existing_export_is_rejected_before_native_execution(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'candidate'
            output.mkdir()
            sentinel = output / 'keep.bin'
            sentinel.write_bytes(b'previous candidate must remain intact')
            with patch.object(oracle, 'generate') as generate, \
                 patch.object(native_oracle, 'image_bytes') as image_bytes:
                with self.assertRaises((FileExistsError, ValueError,
                                        native_oracle.OracleError, SystemExit)):
                    oracle.main(['--write', '--output', str(output)])
                generate.assert_not_called()
                image_bytes.assert_not_called()
            self.assertEqual(sentinel.read_bytes(), b'previous candidate must remain intact')
            self.assertEqual(list(output.iterdir()), [sentinel])

    def test_verifier_rejects_corrupt_reference_and_candidate_bytes(self):
        payloads = fixture_payloads()
        self.assertEqual(len(check.verify(payloads.__getitem__, 'candidate')), 14)
        name = next(iter(payloads))
        with tempfile.TemporaryDirectory() as directory:
            copied = Path(directory) / 'fixtures'
            shutil.copytree(FIXTURES, copied)
            (copied / name).write_bytes(payloads[name] + b'corrupt')
            with self.assertRaisesRegex((ValueError, native_oracle.OracleError), name):
                check.verify(lambda key: (copied / key).read_bytes(),
                             'reference', fixture_root=copied)
        payloads[name] += b'corrupt'
        with self.assertRaisesRegex((ValueError, native_oracle.OracleError), name):
            check.verify(payloads.__getitem__, 'candidate')

    def test_hash_mismatch_cannot_publish_generated_candidate(self):
        payloads = fixture_payloads()
        name = next(iter(payloads))
        payloads[name] += b'corrupt'
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'candidate'
            with patch.object(oracle, 'generate', return_value=payloads):
                with self.assertRaisesRegex((ValueError, native_oracle.OracleError), name):
                    oracle.main(['--write', '--output', str(output)])
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_check_cli_rejects_corrupt_candidate_in_optimized_python(self):
        payloads = fixture_payloads()
        name = next(iter(payloads))
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            for key, data in payloads.items():
                (output / key).write_bytes(data)
            (output / name).write_bytes(payloads[name] + b'corrupt')
            result = subprocess.run(
                [sys.executable, '-O', '-m', 'tools.palette_oracle.check',
                 '--generated', str(output)],
                cwd=ROOT, env=absent_retail_environment(), capture_output=True,
                text=True, timeout=30,
            )
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn(name, result.stdout + result.stderr)

    def test_explicit_executable_uses_shared_resolver_and_restores_environment(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            # An explicit missing path must not silently fall back to RA2_DIR,
            # even if it contains a file named gamemd.exe.
            (root / 'gamemd.exe').write_bytes(b'not the supported image')
            missing = root / 'missing.exe'
            for original in (None, str(root / 'previous.exe')):
                environment = absent_retail_environment()
                environment['RA2_DIR'] = str(root)
                if original is None:
                    environment.pop('VERA20K_GAMEMD_EXE', None)
                else:
                    environment['VERA20K_GAMEMD_EXE'] = original
                with self.subTest(original=original), patch.dict(os.environ, environment, clear=True):
                    before = dict(os.environ)
                    with self.assertRaisesRegex(native_oracle.OracleError,
                                                'Missing original executable'):
                        oracle.main(['--check', '--exe', str(missing)])
                    self.assertEqual(dict(os.environ), before)

    def test_export_creates_missing_parent_and_preserves_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'new-parent' / 'candidate'
            payloads = {'first.bin': b'first', 'last.json': b'{"checked": true}'}
            oracle.export_new(payloads, output)
            self.assertEqual({p.name: p.read_bytes() for p in output.iterdir()}, payloads)

    def test_source_or_provenance_change_during_replay_prevents_publication(self):
        identity = oracle.source_identity()
        for field in ('sources', 'historical_provenance_sha256'):
            changed = dict(identity)
            changed[field] = {'changed.py': 'changed'} if field == 'sources' else 'changed'
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / 'candidate'
                with patch.object(oracle, 'source_identity',
                                  side_effect=[identity, identity, changed]), \
                     patch.object(oracle, 'generate', return_value=fixture_payloads()):
                    with self.assertRaisesRegex(native_oracle.OracleError, 'changed during replay'):
                        oracle.main(['--write', '--output', str(output)])
                self.assertEqual(list(Path(directory).iterdir()), [])

    def test_failure_and_publication_gates_survive_optimized_python(self):
        tests = (
            'test_early_stop_budget_and_fault_never_become_native_rows',
            'test_failed_execution_does_not_create_an_export_directory',
            'test_existing_export_is_rejected_before_native_execution',
            'test_verifier_rejects_corrupt_reference_and_candidate_bytes',
            'test_hash_mismatch_cannot_publish_generated_candidate',
            'test_explicit_executable_uses_shared_resolver_and_restores_environment',
            'test_export_creates_missing_parent_and_preserves_bytes',
            'test_source_or_provenance_change_during_replay_prevents_publication',
        )
        result = subprocess.run(
            [sys.executable, '-O', '-m', 'unittest']
            + [__name__ + '.PaletteOracleTests.' + name for name in tests],
            cwd=ROOT, env=absent_retail_environment(), capture_output=True,
            text=True, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
