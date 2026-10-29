"""Guard the Switch client's bundled retail media.

A 0.0.3 beta NSP once shipped `movmd03.mix`/`movies01.mix` as zero-byte
placeholders because its asset source directory held empty files, which
silently removed every in-game movie from the installed build. These checks
keep that regression detectable without a real Switch or device keys.
"""
from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BUILD = ROOT / 'platform' / 'switch' / 'build.py'


def load_build_module():
    spec = importlib.util.spec_from_file_location('vera_switch_build', BUILD)
    module = importlib.util.module_from_spec(spec)
    saved = sys.argv
    saved_path = list(sys.path)
    # build.py imports its sibling `nsp` module by bare name.
    sys.path.insert(0, str(BUILD.parent))
    sys.argv = ['build.py']
    try:
        spec.loader.exec_module(module)
    except SystemExit:
        pass
    finally:
        sys.argv = saved
        sys.path[:] = saved_path
    return module


BUILD_MODULE = load_build_module()


class AssetMediaChecks(unittest.TestCase):
    def asset_directory(self, media: dict[str, int | None]) -> Path:
        """Build a temporary install root; `None` leaves the member absent."""
        directory = Path(tempfile.mkdtemp())
        (directory / 'ra2.mix').write_bytes(b'retail')
        (directory / 'ra2md.mix').write_bytes(b'retail')
        for name, size in media.items():
            if size is None:
                continue
            (directory / name).write_bytes(b'\0' * size)
        return directory

    def test_complete_install_reports_nothing(self):
        media = {name: 4096 for name in BUILD_MODULE.REQUIRED_MEDIA_ASSETS}
        self.assertEqual(BUILD_MODULE.check_asset_directory(
            self.asset_directory(media), True), [])

    def test_empty_placeholders_are_reported(self):
        media = {name: 0 for name in BUILD_MODULE.REQUIRED_MEDIA_ASSETS}
        warnings = BUILD_MODULE.check_asset_directory(self.asset_directory(media), True)
        self.assertEqual(len(warnings), len(BUILD_MODULE.REQUIRED_MEDIA_ASSETS))
        for warning in warnings:
            self.assertIn('is empty', warning)

    def test_absent_media_is_reported(self):
        media = {name: None for name in BUILD_MODULE.REQUIRED_MEDIA_ASSETS}
        warnings = BUILD_MODULE.check_asset_directory(self.asset_directory(media), True)
        self.assertEqual(len(warnings), len(BUILD_MODULE.REQUIRED_MEDIA_ASSETS))
        for warning in warnings:
            self.assertIn('is absent', warning)

    def test_missing_main_archive_is_fatal(self):
        for missing in ('ra2.mix', 'ra2md.mix'):
            with self.subTest(missing=missing):
                directory = self.asset_directory(
                    {name: 4096 for name in BUILD_MODULE.REQUIRED_MEDIA_ASSETS})
                (directory / missing).write_bytes(b'')
                warnings = BUILD_MODULE.check_asset_directory(directory, True)
                self.assertTrue(any('is missing' in w for w in warnings))

    def test_media_checks_are_opt_in(self):
        media = {name: None for name in BUILD_MODULE.REQUIRED_MEDIA_ASSETS}
        self.assertEqual(BUILD_MODULE.check_asset_directory(
            self.asset_directory(media), False), [])


if __name__ == '__main__':
    unittest.main()
