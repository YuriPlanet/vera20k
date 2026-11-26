"""Run the headless simulation workloads for release PGO.

RA2_DIR is the retail install. LLVM_PROFILE_FILE names the fresh raw-profile
directory and filename pattern the release build will merge. Each program runs
to completion so LLVM flushes its counters. Only counters go to the build.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

from tools import scale_bench

MAP = 'Death.mmx'
SEED = 73
COMPUTERS = 2
MATCH_TICKS = 1800
REPLAY_TICKS = 240


def require_profile(folder: Path, before: set[Path], workload: str) -> None:
    if not any(p.stat().st_size for p in set(folder.glob('*.profraw')) - before):
        raise RuntimeError(f'{workload} produced no new nonempty .profraw file')


def train(game: Path, bench: Path, retail: Path, work: Path, raw: Path) -> list[str]:
    reports = []
    # A mature game with working bases, scripted armies, combat and production.
    # Give it 1,200 units instead of the scale benchmark's 20,000 so the
    # instrumented match fits the hosted runners' time and memory.
    start = set(raw.glob('*.profraw'))
    common = ['--ra2-dir', str(retail), '--map', MAP, '--seed', str(SEED),
              '--computers', str(COMPUTERS)]
    match = subprocess.run([bench, *common, '--scenario', 'match', '--units', '1200',
                            '--houses', '3', '--ticks', str(MATCH_TICKS), '--client-timing'],
                           check=True, stdout=subprocess.PIPE, text=True, timeout=600)
    final = scale_bench.FINAL_HASH.search(match.stdout)
    if not final or not re.search(rf'^ticks={MATCH_TICKS} ', match.stdout, re.MULTILINE):
        raise RuntimeError('the training battle did not finish all its ticks and report its state hash')
    require_profile(raw, start, 'sim-bench battle')
    reports.append(f'AI battle: {MATCH_TICKS} frames, final hash {final[1]}.')

    # A fresh recording exercises the shipping executable's simulation path
    # and matches this build's replay layout.
    replay = work / 'skirmish.json'
    start = set(raw.glob('*.profraw'))
    subprocess.run([bench, *common, '--scenario', 'skirmish', '--ticks', str(REPLAY_TICKS),
                    '--record-skirmish-replay', replay], check=True, timeout=300)
    require_profile(raw, start, 'sim-bench recording')
    start = set(raw.glob('*.profraw'))
    verified = subprocess.run([game, '--replay-verify', replay, '--ra2-dir', retail],
                              cwd=work, check=True, stdout=subprocess.PIPE, text=True, timeout=300)
    receipt = json.loads(verified.stdout)
    if not all(receipt[field] == REPLAY_TICKS for field in ('ticks', 'checked_hashes', 'final_tick')):
        raise RuntimeError(f'shipping replay stopped before verifying {REPLAY_TICKS} frames')
    require_profile(raw, start, 'shipping game replay')
    reports.append(f'Shipping game replay: {REPLAY_TICKS} verified frame hashes.')
    return reports


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', required=True, type=Path)
    parser.add_argument('--sim-bench', required=True, type=Path)
    args = parser.parse_args()
    raw = Path(os.environ['LLVM_PROFILE_FILE']).parent
    if not raw.is_absolute() or not raw.is_dir():
        parser.error('LLVM_PROFILE_FILE must name a file inside an existing absolute directory')
    with tempfile.TemporaryDirectory(prefix='vera-pgo-') as scratch:
        reports = train(args.game.resolve(strict=True), args.sim_bench.resolve(strict=True),
                        Path(os.environ['RA2_DIR']).resolve(strict=True), Path(scratch), raw)
    text = '\n'.join(f'- {report}' for report in reports)
    print(text, flush=True)
    if summary := os.environ.get('GITHUB_STEP_SUMMARY'):
        with open(summary, 'a', encoding='utf-8') as out:
            out.write(f'\n### PGO training\n\n{text}\n')


if __name__ == '__main__':
    main()
