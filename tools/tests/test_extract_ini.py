"""Retail integration check for the built extract-ini binary.

Set VERA20K_EXTRACT_INI_BIN to the candidate binary and RA2_DIR to a stock
Yuri's Revenge install. The test uses a fresh output directory, never the
checkout's ignored ini/ directory.
"""

from __future__ import annotations

import configparser
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
ROSTER_FIXTURE = ROOT / 'tests/fixtures/ini/mpmodesmd_stock_contract.ini'


@unittest.skipUnless(
    os.environ.get('VERA20K_EXTRACT_INI_BIN') and os.environ.get('RA2_DIR'),
    'requires VERA20K_EXTRACT_INI_BIN and stock RA2_DIR',
)
class ExtractIniRetailTest(unittest.TestCase):
    def test_selected_yr_inputs_are_extracted_without_ra2_base_inis(self) -> None:
        roster = configparser.ConfigParser(interpolation=None)
        roster.read(ROSTER_FIXTURE, encoding='utf-8')
        mode_files = {
            value.split(',')[2].strip().lower()
            for section in roster.sections()
            for _, value in roster.items(section)
        }
        self.assertEqual(len(mode_files), 9, 'stock fixture should name nine modes')

        binary = Path(os.environ['VERA20K_EXTRACT_INI_BIN']).resolve(strict=True)
        retail_dir = Path(os.environ['RA2_DIR']).resolve(strict=True)
        with tempfile.TemporaryDirectory() as temporary:
            subprocess.run(
                [str(binary), str(retail_dir)],
                cwd=temporary,
                check=True,
                capture_output=True,
                text=True,
            )
            extracted = {path.name.lower(): path for path in (Path(temporary) / 'ini').iterdir()}

            required = mode_files | {
                'rulesmd.ini', 'artmd.ini', 'aimd.ini', 'mpmodesmd.ini',
                'uimd.ini', 'mapselmd.ini', 'keyboardmd.ini', 'coopcampmd.ini',
            }
            self.assertFalse(required - extracted.keys(), 'missing active YR INIs')
            for name in required:
                self.assertGreater(extracted[name].stat().st_size, 0, name)

            base_files = {
                'rules.ini', 'art.ini', 'ai.ini', 'sound.ini', 'eva.ini',
                'theme.ini', 'temperat.ini', 'snow.ini', 'urban.ini',
                'urbann.ini', 'lunar.ini', 'desert.ini', 'battle.ini', 'rmg.ini',
            }
            self.assertFalse(base_files & extracted.keys(), 'RA2 base INIs were extracted')


if __name__ == '__main__':
    unittest.main()
