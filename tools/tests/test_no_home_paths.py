"""Committed files never name a machine's home directory.

Receipts, logs and captures record absolute paths such as `/Users/<name>/...`; a
committed copy publishes that account name. Tracked files use repo-relative or
`~` paths instead, including inside `.gz` packets.
"""
import gzip
from pathlib import Path
import re
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[2]
# The pattern tests below spell out sample home paths.
THIS_FILE = Path(__file__).resolve().relative_to(ROOT).as_posix()
_NAME = rb'[^/\\\s"\'`<>:;,()\[\]{}|*$%]+'
HOME_PATH = re.compile(
    rb'(?:/Users|/home)/(?!(?:runner|Shared)(?!' + _NAME + rb'))' + _NAME
    + rb'|(?i:\b[a-z]:[\\/]+users[\\/]+)(?!(?i:public|default)\b)' + _NAME)
# Hash-pinned native evidence older than this rule: its own tests check the
# recorded hashes, so rewriting the paths inside would break it.
PINNED_EVIDENCE = frozenset(f'tools/spatial_oracle/{name}' for name in '''
_factory_infantry_output/accepted/no_rally.private.json.gz
_factory_infantry_output/accepted/no_rally.receipt.json.gz
_factory_infantry_output/accepted/publication-phase-native-after-strip-attempt-1.json.gz
_factory_infantry_output/accepted/publication-phase-native-before-strip-attempt-1.json.gz
_factory_infantry_output/accepted/rally.private.json.gz
_factory_infantry_output/accepted/rally.receipt.json.gz
_factory_infantry_output/accepted/unit-ready-pcm.native.json.gz
_factory_infantry_output/compatibility/infantry-command-gate-r1.failed.json.gz
_factory_infantry_output/compatibility/publication-after_strip-current-r12.json.gz
_factory_infantry_output/compatibility/publication-before_strip-current-r12.json.gz
_factory_infantry_output/compatibility/unit-command-pre-admission-meta.json.gz
_factory_infantry_output/compatibility/unit-publication-after_strip-candidate-r3.json.gz
_factory_infantry_output/compatibility/unit-publication-after_strip-current-r4.json.gz
_factory_infantry_output/compatibility/unit-publication-after_strip-public-r6.json.gz
_factory_infantry_output/compatibility/unit-publication-before_strip-candidate-r3.json.gz
_factory_infantry_output/compatibility/unit-publication-before_strip-current-r4.json.gz
_factory_infantry_output/compatibility/unit-publication-before_strip-public-r5.json.gz
_factory_infantry_output/consumer-meta.json
_factory_infantry_output/fixtures/initialized-no_rally.json.gz
_factory_infantry_output/fixtures/initialized-rally.json.gz
_factory_infantry_output/fixtures/ordinary-input-closure.json.gz
_factory_infantry_output/gate-meta.json
_factory_infantry_output/inputs/infantry-unlimbo-gate-retail.json
_factory_infantry_output/meta.json
_walk_first_path/as-run-r10/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-r11/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-r12/tools/spatial_oracle/_factory_infantry_output/consumer-meta.json.gz
_walk_first_path/as-run-r12/tools/spatial_oracle/_factory_infantry_output/gate-meta.json.gz
_walk_first_path/as-run-r12/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-r13/tools/spatial_oracle/_factory_infantry_output/consumer-meta.json.gz
_walk_first_path/as-run-r13/tools/spatial_oracle/_factory_infantry_output/gate-meta.json.gz
_walk_first_path/as-run-r13/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-r6/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-r7/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-r8/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-r9/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-unit-r1/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-unit-r2/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-unit-r3/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-unit-r4/tools/spatial_oracle/_factory_infantry_output/gate-meta.json.gz
_walk_first_path/as-run-unit-r4/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-unit-r5/tools/spatial_oracle/_factory_infantry_output/gate-meta.json.gz
_walk_first_path/as-run-unit-r5/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-unit-r6/tools/spatial_oracle/_factory_infantry_output/gate-meta.json.gz
_walk_first_path/as-run-unit-r6/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-unit-r7/tools/spatial_oracle/_factory_infantry_output/gate-meta.json.gz
_walk_first_path/as-run-unit-r7/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/as-run-unit-r8/tools/spatial_oracle/_factory_infantry_output/gate-meta.json.gz
_walk_first_path/as-run-unit-r8/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/compatibility/default-no_rally-r6.json.gz
_walk_first_path/compatibility/default-rally-r6.json.gz
_walk_first_path/compatibility/main-2135df0e3-prepare-fixture.json
_walk_first_path/compatibility/unit-default-no_rally-r3.json.gz
_walk_first_path/compatibility/unit-default-rally-r3.json.gz
_walk_first_path/failed/human_Doing27_B-r7.json.gz
_walk_first_path/failed/paid_head_B-r6.json.gz
_walk_first_path/failed/prone_B-r8.json.gz
_walk_first_path/failed/publication-after_strip-current-r11.json.gz
_walk_first_path/failed/publication-before_strip-current-r1.json.gz
_walk_first_path/failed/publication-before_strip-current-r11.json.gz
_walk_first_path/failed/unit_shift_outside_playfield_B-r5.json.gz
_walk_first_path/failed/unit_shift_outside_playfield_B-r6.json.gz
_walk_first_path/original/gi-cellclick-wrapper-original-source-r8.json
_walk_first_path/publication-as-run-r12/tools/spatial_oracle/_factory_infantry_output/consumer-meta.json.gz
_walk_first_path/publication-as-run-r12/tools/spatial_oracle/_factory_infantry_output/gate-meta.json.gz
_walk_first_path/publication-as-run-r12/tools/spatial_oracle/_factory_infantry_output/meta.json.gz
_walk_first_path/publication-census-as-run-r11/tools/spatial_oracle/_factory_infantry_output/consumer-meta.json.gz
_walk_first_path/publication-census-as-run-r11/tools/spatial_oracle/_factory_infantry_output/gate-meta.json.gz
'''.split())


def contents(path):
    data = path.read_bytes()
    if path.suffix == '.gz':
        try:
            return gzip.decompress(data)
        except (OSError, EOFError):
            pass
    return data


def names_home(data):
    return (b'sers' in data or b'SERS' in data or b'/home/' in data) and HOME_PATH.search(data)


class NoHomePathTests(unittest.TestCase):
    def test_tracked_files_name_no_home_directory(self):
        try:
            listed = subprocess.run(['git', 'ls-files', '-z'], cwd=ROOT, capture_output=True,
                                    check=True).stdout
        except (OSError, subprocess.CalledProcessError):
            self.skipTest('not a Git checkout')
        found = []
        for name in filter(None, listed.decode().split('\0')):
            path = ROOT / name
            if name in PINNED_EVIDENCE or name == THIS_FILE or not path.is_file():
                continue
            if match := names_home(contents(path)):
                found.append(f'{name}: {match.group().decode(errors="replace")}')
        self.assertEqual(found, [], 'write repo-relative or ~ paths instead')

    def test_pinned_evidence_exemptions_are_still_needed(self):
        stale = sorted(name for name in PINNED_EVIDENCE
                       if not (ROOT / name).is_file() or not names_home(contents(ROOT / name)))
        self.assertEqual(stale, [], 'drop these from PINNED_EVIDENCE')

    def test_pattern_finds_home_directories_and_skips_placeholders(self):
        for text in (b'/Users/someone/x', b'"/Users/someone"', b'HOME=/home/someone:',
                     b'/home/\xc3\xa9mile/', b'C:\\Users\\someone\\x', b'c:/users/someone'):
            self.assertTrue(names_home(text), text)
        for text in (b'~/Documents/x', b'/Users/<you>/x', b'/home/runner/work', b'/Users/Shared/x',
                     b'C:\\Users\\Public\\x', b'/Users/', b'$HOME/x'):
            self.assertFalse(names_home(text), text)


if __name__ == '__main__':
    unittest.main()
