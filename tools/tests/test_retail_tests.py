"""Selection and failure reporting of tools/retail_tests.py, without Cargo."""

import contextlib
import io
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from tools import retail_tests


class RetailTestsTests(unittest.TestCase):
    def test_ignore_reasons_select_only_what_the_retail_install_satisfies(self):
        for reason, expected in {
            'slow; run when changing what it covers': 'run',
            'requires a retail RA2/YR install (RA2_DIR or config.toml)': 'run',
            'physical stock audio device/IDX/BAG; real gameplay sample preparation, '
            'not audible/native timing parity': 'run',
            'requires retail assets and GPU; original MTNK health strip A/Z and Display replay': 'gpu',
            'requires active-retail RA2_DIR and an audio output device': 'device',
            'requires retail assets and VERA20K_XMP34U4_MAP': 'input',
            'manual paired CPU timing; requires retail INIs and an optimized quiet host': 'manual',
            'production Drive path drops both head-on orders (VERA on retail Dustbowl too)': 'other',
        }.items():
            with self.subTest(reason=reason):
                self.assertEqual(retail_tests.classify(reason), expected)

    def test_missing_inputs_fail_before_any_build(self):
        with tempfile.TemporaryDirectory() as root:
            problems = retail_tests.prerequisite_problems(Path(root), {'RA2_DIR': root})
        self.assertEqual(len(problems), 2)
        self.assertIn('ra2md.mix', problems[0])
        self.assertIn('rulesmd.ini', problems[1])
        with (mock.patch.dict(os.environ, {'RA2_DIR': ''}),
              mock.patch.object(retail_tests, 'invoke') as invoke,
              contextlib.redirect_stderr(io.StringIO()) as stderr):
            self.assertEqual(retail_tests.main(['run']), 2)
        invoke.assert_not_called()
        self.assertIn('RA2_DIR is unset', stderr.getvalue())

    def test_a_failed_or_silent_selected_test_fails_the_run(self):
        inventory = {name: [('src/a.rs', reason)] for name, reason in {
            'passes': 'slow; run when changing what it covers',
            'fails': 'requires a retail RA2/YR install (RA2_DIR or config.toml)',
            'silent': 'requires RA2_DIR with installed retail RA2/YR assets',
            'draws': 'requires retail assets and GPU',
        }.items()}
        calls = []

        def cargo(args, *, cwd, env_extra, on_line):
            calls.append(args)
            self.assertEqual(env_extra, retail_tests.REQUIRE_ENV)
            listing = '--list' in args
            for line in ([f'a::{name}: test\n' for name in inventory] if listing
                         else ['test a::passes ... ok\n', 'test a::fails ... FAILED\n']):
                on_line(line)
            return 0 if listing else 101

        with (mock.patch.object(retail_tests, 'prerequisite_problems', return_value=[]),
              mock.patch.object(retail_tests, 'scan', return_value=inventory),
              mock.patch.object(retail_tests, 'invoke', side_effect=cargo),
              contextlib.redirect_stdout(io.StringIO()) as stdout):
            self.assertEqual(retail_tests.main(['run', '--', '--test-threads=2']), 1)
        self.assertEqual(calls[1][calls[1].index('--exact') + 1:],
                         ['a::passes', 'a::fails', 'a::silent', '--test-threads=2'])
        self.assertIn('3 selected, 1 passed, 1 failed, 1 reported no result', stdout.getvalue())
        self.assertIn('NO RESULT  a::silent', stdout.getvalue())


if __name__ == '__main__':
    unittest.main()
