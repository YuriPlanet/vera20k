"""Coverage for the suite's exit status and retained run directories."""
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from tools.e2e_parity import runner
from tools.tactical_certification.core import OutputExistsError


class E2EParityTests(unittest.TestCase):
    def test_production_success_leaves_native_parity_unverified(self):
        rows = [{'production': 'PASS', 'native': 'UNVERIFIED'}]
        self.assertEqual(runner.exit_status(rows, production_only=False), 1)
        self.assertEqual(runner.exit_status(rows, production_only=True), 0)

    def test_native_drift_and_production_failures_fail_the_run(self):
        for production, native in [('PASS', 'DRIFT'), ('FAIL', 'UNVERIFIED')]:
            with self.subTest(production=production, native=native):
                rows = [{'production': production, 'native': native}]
                self.assertEqual(runner.exit_status(rows, production_only=True), 1)

    def test_existing_output_is_preserved_without_building(self):
        with TemporaryDirectory() as temporary:
            output = Path(temporary).resolve()
            receipt = output / 'report.json'
            receipt.write_text('previous run', encoding='utf-8')
            with patch.object(runner, '_build') as build:
                with self.assertRaises(OutputExistsError):
                    runner.run([runner.MATCH], output, output, None)
                build.assert_not_called()
            self.assertEqual(receipt.read_text(encoding='utf-8'), 'previous run')

    def test_every_map_case_has_a_profile_and_expectations(self):
        for case in runner.MAP_CASES:
            with self.subTest(case=case):
                profile = runner.ROOT / 'tools' / f'map_observation.{case}.example.json'
                expectations = runner.ROOT / 'tools' / f'map_observation.{case}.expectations.json'
                self.assertTrue(profile.is_file(), profile)
                self.assertTrue(expectations.is_file(), expectations)

    def test_campaign_case_runs_its_own_route(self):
        with TemporaryDirectory() as temporary:
            output = Path(temporary).resolve() / 'results'
            with patch.object(runner, '_build', return_value={'vera20k': Path('vera20k')}):
                with patch.object(runner, '_campaign') as campaign:
                    campaign.return_value = {'production': 'PASS', 'native': 'UNVERIFIED'}
                    with patch.object(runner, '_shell') as shell:
                        results = runner.run([runner.CAMPAIGN], output, output, None)
            campaign.assert_called_once()
            shell.assert_not_called()
            self.assertEqual(results[0]['case'], runner.CAMPAIGN)
            self.assertEqual(results[0]['production'], 'PASS')
