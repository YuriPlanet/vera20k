"""Execution failures must never become successful arc-domain golden rows."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from tools.native_oracle import OracleError
from tools.projectile_oracle import arc_domain


class ArcDomainFailureTests(unittest.TestCase):
    def test_execution_errors_abort_before_any_reference_publication(self):
        # The first solver may already have returned a legitimate failure byte.
        # A fault in the next solver must still abort the complete publication.
        for fail_second in (False, True):
            for error_type in (OracleError, RuntimeError):
                with self.subTest(second=fail_second, error=error_type.__name__), \
                     tempfile.TemporaryDirectory() as directory:
                    output = Path(directory) / 'reference.json'
                    original = b'[{"existing":"reference"}]\n'
                    output.write_bytes(original)
                    effects = [error_type('execution failed')]
                    if fail_second:
                        effects.insert(0, {'eax': 0, 'dumps': {'out': 'cd' * 8}})
                    with patch.object(arc_domain, 'call', side_effect=effects):
                        with self.assertRaisesRegex(error_type, 'execution failed'):
                            arc_domain.main(['--write', '--output', str(output)])
                    self.assertEqual(output.read_bytes(), original)
                    self.assertEqual({path.name for path in output.parent.iterdir()}, {'reference.json'})

    def test_error_publication_gate_survives_optimized_python(self):
        result = subprocess.run(
            [sys.executable, '-O', '-m', 'unittest',
             __name__ + '.ArcDomainFailureTests.test_execution_errors_abort_before_any_reference_publication'],
            cwd=Path(__file__).resolve().parents[2], capture_output=True, text=True,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
