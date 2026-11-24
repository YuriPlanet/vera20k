"""Foot Z execution/lifecycle contracts; synthetic x86 is never native evidence."""
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from tools import native_oracle, render_depth_oracle as depth
from tools.tests.pe_fixture import pe_image
from tools.tests.test_oracle_lifecycle import absent_retail_environment


ROOT = Path(__file__).resolve().parents[2]
INITIALIZER, FOOT = 0x49F2F0, 0x4DAFC0
REQUIRED = (0x704240, 0x703E70, 0x704000, 0x704350)
FAILURES = (
    ('early stop', b'\xf4', 'Incomplete execution'),
    ('instruction budget', b'\xeb\xfe', 'Incomplete execution'),
    ('invalid instruction', b'\x0f\x0b', 'faulted'),
)


def calling_helpers(addresses):
    code = bytearray()
    for address in addresses:
        code += b'\xe8' + struct.pack('<i', address - (FOOT + len(code) + 5))
    return bytes(code) + b'\xc3'


def synthetic_image(*, initializer=b'\xc3', foot=None):
    if foot is None:
        foot = calling_helpers(REQUIRED)
    rows = [(INITIALIZER, initializer), (FOOT, foot)]
    rows += [(address, b'\xc3') for address in (*REQUIRED, 0x703B10)]
    # Section table ends below0x400; the production loader owns all mapping.
    return pe_image([
        (address - native_oracle.IMAGE_BASE, 0x400 + index * 0x100,
         data, len(data), 0x60000020)
        for index, (address, data) in enumerate(rows)
    ])


class RenderDepthCompletionTests(unittest.TestCase):
    def test_initializer_and_foot_cannot_publish_early_stops_or_faults(self):
        for stage in ('initializer', 'foot'):
            for label, opcodes, message in FAILURES:
                with self.subTest(stage=stage, failure=label), patch.object(
                        native_oracle, 'image_bytes',
                        return_value=synthetic_image(**{stage: opcodes})):
                    with self.assertRaisesRegex(native_oracle.OracleError, message):
                        fixture = depth.NativeFixture()
                        fixture.execute(depth.fixtures()[0])

    def test_full_foot_requires_each_helper_in_that_call(self):
        # execute probes helpers first. Those earlier visits must not satisfy
        # the final Foot call's own obligations. Omit each helper independently.
        for missing in REQUIRED:
            foot = calling_helpers(address for address in REQUIRED if address != missing)
            with self.subTest(missing=hex(missing)), patch.object(
                    native_oracle, 'image_bytes', return_value=synthetic_image(foot=foot)):
                fixture = depth.NativeFixture()
                with self.assertRaisesRegex(native_oracle.OracleError,
                                            'Required native instruction addresses not reached'):
                    fixture.execute(depth.fixtures()[0])
        with patch.object(native_oracle, 'image_bytes', return_value=synthetic_image()):
            fixture = depth.NativeFixture()
            result = fixture.execute(depth.fixtures()[0])
            self.assertEqual(set(result), set(depth.TARGETS))

    def test_failed_write_preserves_both_reference_and_sidecar(self):
        bad_cases = [
            (stage + '/' + label, synthetic_image(**{stage: opcodes}), message)
            for stage in ('initializer', 'foot') for label, opcodes, message in FAILURES
        ] + [('missing helper', synthetic_image(foot=b'\xc3'),
              'Required native instruction addresses not reached')]
        for label, data, message in bad_cases:
            for existing in (False, True):
                with self.subTest(failure=label, existing=existing), \
                     tempfile.TemporaryDirectory() as directory:
                    output = Path(directory) / 'vectors.json'
                    sidecar = output.with_suffix('.meta.json')
                    payload, metadata = b'{"keep":"reference"}\n', b'{"keep":"provenance"}\n'
                    if existing:
                        output.write_bytes(payload)
                        sidecar.write_bytes(metadata)
                    with patch.object(native_oracle, 'image_bytes', return_value=data):
                        with self.assertRaisesRegex(native_oracle.OracleError, message):
                            depth.main(['--write', '--output', str(output)])
                    if existing:
                        self.assertEqual(output.read_bytes(), payload)
                        self.assertEqual(sidecar.read_bytes(), metadata)
                        self.assertEqual(set(output.parent.iterdir()), {output, sidecar})
                    else:
                        self.assertEqual(list(output.parent.iterdir()), [])

    def test_source_drift_cannot_relabel_or_replace_results(self):
        original_read = Path.read_text
        for source in ('tools/render_depth_oracle.py', 'tools/native_oracle.py'):
            for mode in ('--check', '--write'):
                for existing in (False, True):
                    with self.subTest(source=source, mode=mode, existing=existing), \
                         tempfile.TemporaryDirectory() as directory:
                        output = Path(directory) / 'vectors.json'
                        sidecar = output.with_suffix('.meta.json')
                        if existing:
                            output.write_bytes(b'original payload')
                            sidecar.write_bytes(b'original provenance')
                        before = {p.name: p.read_bytes() for p in output.parent.iterdir()}
                        changed = False

                        def generate():
                            nonlocal changed
                            changed = True
                            return {'synthetic_only': True}

                        def read(path, *args, **kwargs):
                            value = original_read(path, *args, **kwargs)
                            if changed and path == ROOT / source:
                                value += '\n# simulated concurrent edit\n'
                            return value

                        with patch.object(depth, 'generate', side_effect=generate), \
                             patch.object(depth, 'metadata', return_value={'scope': 'synthetic test'}), \
                             patch.object(Path, 'read_text', read):
                            with self.assertRaisesRegex(native_oracle.OracleError,
                                                        'Source changed during native generation'):
                                depth.main([mode, '--output', str(output)])
                        self.assertEqual(before, {p.name: p.read_bytes()
                                                  for p in output.parent.iterdir()})

    def test_failure_guards_survive_optimized_python(self):
        cases = (
            'test_initializer_and_foot_cannot_publish_early_stops_or_faults',
            'test_full_foot_requires_each_helper_in_that_call',
            'test_failed_write_preserves_both_reference_and_sidecar',
            'test_source_drift_cannot_relabel_or_replace_results',
        )
        result = subprocess.run(
            [sys.executable, '-O', '-m', 'unittest']
            + [__name__ + '.RenderDepthCompletionTests.' + name for name in cases],
            cwd=ROOT, env=absent_retail_environment(), capture_output=True,
            text=True, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
