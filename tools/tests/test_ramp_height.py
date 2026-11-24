"""Checked ramp execution contracts; synthetic x86 is never native evidence."""
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from tools import native_oracle, ramp_height_oracle as ramp
from tools.tests.pe_fixture import pe_image
from tools.tests.test_oracle_lifecycle import absent_retail_environment


ROOT = Path(__file__).resolve().parents[2]
LOOKUP, EVALUATOR, SETTER = 0x578080, 0x47B3A0, 0x5F5FA0
FAILURES = (
    ('early stop', b'\xf4', 'Incomplete execution'),
    ('instruction budget', b'\xeb\xfe', 'Incomplete execution'),
    ('invalid instruction', b'\x0f\x0b', 'faulted'),
)


def synthetic_image(lookup=b'\xc3', setter=b'\xc3', *,
                    unit_owner=0x747EB0, infantry_owner=0x5247D0):
    rows = (
        (LOOKUP, lookup), (EVALUATOR, b'\xc3'), (SETTER, setter),
        (0x7F6284, struct.pack('<I', unit_owner)),
        (0x7EB67C, struct.pack('<I', infantry_owner)),
    )
    return pe_image([
        (address - native_oracle.IMAGE_BASE, 0x400 + index * 0x100,
         data, len(data), 0x60000020)
        for index, (address, data) in enumerate(rows)
    ])


def setter_calling(address):
    return b'\xe8' + struct.pack('<i', address - (SETTER + 5)) + b'\xc3'


class RampHeightCompletionTests(unittest.TestCase):
    def test_early_stop_budget_and_fault_do_not_return_native_results(self):
        for label, opcodes, message in FAILURES:
            with self.subTest(failure=label), patch.object(
                    native_oracle, 'image_bytes', return_value=synthetic_image(lookup=opcodes)):
                fixture = ramp.NativeFixture()
                with self.assertRaisesRegex(native_oracle.OracleError, message):
                    fixture.call(LOOKUP)

    def test_setter_must_visit_both_required_callees_in_its_own_call(self):
        # execute() calls LOOKUP separately before SETTER. Its earlier visit
        # must not satisfy the setter's requirement to run that callee itself.
        for label, setter in (
                ('returns without callees', b'\xc3'),
                ('only lookup', setter_calling(LOOKUP)),
                ('only evaluator', setter_calling(EVALUATOR))):
            with self.subTest(setter=label), patch.object(
                    native_oracle, 'image_bytes', return_value=synthetic_image(setter=setter)):
                fixture = ramp.NativeFixture()
                with self.assertRaisesRegex(native_oracle.OracleError,
                                            'Required native instruction addresses not reached'):
                    fixture.execute(ramp.fixtures()[0])

    def test_unit_and_infantry_vtable_identity_guards_are_independent(self):
        for owner in ('unit_owner', 'infantry_owner'):
            with self.subTest(owner=owner), patch.object(
                    native_oracle, 'image_bytes', return_value=synthetic_image(**{owner: 0})):
                with self.assertRaises(native_oracle.OracleError):
                    ramp.NativeFixture()

    def test_failed_write_preserves_reference_and_creates_no_sidecar(self):
        for label, opcodes, message in FAILURES:
            for existing in (False, True):
                with self.subTest(failure=label, existing=existing), \
                     tempfile.TemporaryDirectory() as directory:
                    output = Path(directory) / 'ramp.json'
                    original = b'{"existing-reference":"preserve exact bytes"}\n'
                    if existing:
                        output.write_bytes(original)
                    with patch.object(native_oracle, 'image_bytes',
                                      return_value=synthetic_image(lookup=opcodes)):
                        with self.assertRaisesRegex(native_oracle.OracleError, message):
                            ramp.main(['--write', '--output', str(output)])
                    self.assertFalse(output.with_suffix('.meta.json').exists())
                    if existing:
                        self.assertEqual(output.read_bytes(), original)
                        self.assertEqual(list(output.parent.iterdir()), [output])
                    else:
                        self.assertEqual(list(output.parent.iterdir()), [])

    def test_failure_identity_and_publication_gates_survive_optimized_python(self):
        tests = (
            'test_early_stop_budget_and_fault_do_not_return_native_results',
            'test_setter_must_visit_both_required_callees_in_its_own_call',
            'test_unit_and_infantry_vtable_identity_guards_are_independent',
            'test_failed_write_preserves_reference_and_creates_no_sidecar',
        )
        result = subprocess.run(
            [sys.executable, '-O', '-m', 'unittest']
            + [__name__ + '.RampHeightCompletionTests.' + name for name in tests],
            cwd=ROOT, env=absent_retail_environment(), capture_output=True,
            text=True, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
