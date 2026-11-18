"""Check that a branch plays and draws exactly like a base commit.

Builds the base and head refs in their own git worktrees (one labelled
`--release` build of `sim-bench` and `vera20k` each, through `tools.cargo_run`),
then compares:

- simulation: sim-bench's standard battle (1v29 skirmish on the 30-start
  `tools.scale_map` map, client timing) by `final_hash`, and the state-isolation
  report's Rules config hash (`rules_hash`) on XMP03T4; frame times are printed.
- pixels: `tools.map_observation` hidden-window captures at each step and
  Options zoom, compared with `map_observation compare` (which checks
  `frame.bgra` byte for byte); the first differing pixel is named.

Mouse-wheel zoom is not covered: map_observation has no wheel gesture.

    python3 -m tools.same_as_base --ra2-dir /path/to/game --head feature/x \\
        --work /absolute/scratch/folder

Exits 0 when everything matches, 1 on a difference, 2 when a step fails.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CONTRACT = 'src/app/diagnostics/tactical_capture/contract.v2.json'
PROFILE = ROOT / 'tools' / 'map_observation.barracks-output.example.json'
BENCH_LINES = ('simulation_tick_ms=', 'ticks=', 'main_thread_cpu_mean_ms=',
               'after_orders_mean_ms=', 'live_objects', 'final_hash=')


class Failure(Exception):
    pass


def run(command, cwd, env=None, log=None):
    print('+', ' '.join(map(str, command)), flush=True)
    result = subprocess.run(list(map(str, command)), cwd=cwd, env=env, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if log:
        Path(log).write_text(result.stdout)
    return result


def git(*args):
    return subprocess.run(['git', *args], cwd=ROOT, check=True, text=True,
                           stdout=subprocess.PIPE).stdout.strip()


def default_base(head):
    for upstream in ('upstream/main', 'origin/main'):
        try:
            return git('merge-base', upstream, head)
        except subprocess.CalledProcessError:
            continue
    raise Failure('no upstream/main or origin/main to take the merge-base of; pass --base')


def capture_profile(source, step, zoom):
    """The profile captured at `step`: commands from later steps dropped."""
    profile = json.loads(Path(source).read_text())
    profile['ticks'] = step
    profile['commands'] = [c for c in profile.get('commands', []) if c['issue_after_step'] < step]
    profile['presentation'] = {'zoom': zoom, 'sidebar': 100}
    return profile


def first_difference(before, after, width):
    """(x, y, before_bgra, after_bgra) of the first differing pixel, or None."""
    if len(before) != len(after):
        return ('length', len(before), len(after))
    for offset in range(0, len(before), 4):
        if before[offset:offset + 4] != after[offset:offset + 4]:
            pixel = offset // 4
            return (pixel % width, pixel // width, before[offset:offset + 4].hex(),
                    after[offset:offset + 4].hex())
    return None


class Side:
    def __init__(self, name, sha, work, ra2_dir, stamp):
        self.name, self.sha = name, sha
        self.tree = work / name
        self.label = f'same-as-base-{name}-{sha[:9]}-{stamp}'
        self.ra2_dir = ra2_dir

    def prepare(self, config):
        git('worktree', 'add', '--detach', str(self.tree), self.sha)
        # Our own config (the same bytes on both sides), written before
        # prepare_checkout so it does not copy the player's.
        (self.tree / 'config.toml').write_text(config)
        env = dict(os.environ, RA2_DIR=str(self.ra2_dir))
        run([sys.executable, '-m', 'tools.prepare_checkout'], self.tree, env)

    def build(self, logs):
        result = run([sys.executable, '-m', 'tools.cargo_run', '--label', self.label, '--',
                      'build', '--locked', '--release', '-p', 'vera20k-core', '--bin', 'sim-bench',
                      '-p', 'vera20k', '--bin', 'vera20k'], self.tree, log=logs / f'build-{self.name}.log')
        if result.returncode:
            raise Failure(f'{self.name} build failed; see {logs}/build-{self.name}.log')
        resolved = run([sys.executable, '-m', 'tools.cargo_run', '--resolve', 'sim-bench',
                        '--profile', 'release', '--from-label', self.label], self.tree)
        if resolved.returncode:
            raise Failure(f'{self.name}: cannot resolve sim-bench from {self.label}')
        self.sim_bench = resolved.stdout.strip().splitlines()[-1]

    def bench(self, map_path, ticks, logs):
        command = [self.sim_bench, '--ra2-dir', self.ra2_dir, '--map', map_path,
                   '--scenario', 'skirmish', '--client-timing', '--hash-every', '1000']
        if ticks:
            command += ['--ticks', ticks]
        result = run(command, self.tree, log=logs / f'sim-bench-{self.name}.log')
        lines = [line for line in result.stdout.splitlines() if line.startswith(BENCH_LINES)
                 or line.startswith('hash tick=')]
        final = re.search(r'^final_hash=(\w+)$', result.stdout, re.M)
        if result.returncode or not final:
            raise Failure(f'{self.name} sim-bench failed; see {logs}/sim-bench-{self.name}.log')
        return final.group(1), lines

    def rules_hash(self, out):
        result = run([self.sim_bench, '--ra2-dir', self.ra2_dir, '--state-isolation-report', out,
                      '--map', 'XMP03T4.MAP', '--seed', '73', '--client-timing',
                      '--isolation-mode', 'wol', '--isolation-humans', '1', '--computers', '1'],
                     self.tree)
        if result.returncode:
            raise Failure(f'{self.name} state-isolation report failed:\n{result.stdout[-2000:]}')
        target = json.loads(Path(out).read_text())['target']
        return target['initial']['rules_hash'], target

    def capture(self, profile, output, settings):
        shutil.rmtree(settings, ignore_errors=True)
        settings.mkdir()
        env = {k: v for k, v in os.environ.items()
               if k not in json.loads((self.tree / CONTRACT).read_text())['environment_denylist']}
        result = run([sys.executable, '-m', 'tools.map_observation', '--profile', profile,
                      '--contract', self.tree / CONTRACT, '--cwd', self.tree, '--output', output,
                      '--build-label', self.label], self.tree, env)
        if result.returncode:
            raise Failure(f'{self.name} capture {output.name} failed:\n{result.stdout[-2000:]}')

    def remove(self):
        result = run(['git', 'worktree', 'remove', self.tree], ROOT)
        if result.returncode:
            print(f'left {self.tree} in place: {result.stdout.strip()}')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--ra2-dir', type=Path, default=os.environ.get('RA2_DIR'),
                        help='retail game folder (default RA2_DIR)')
    parser.add_argument('--head', default='HEAD')
    parser.add_argument('--base', help='default: merge-base of upstream/main (or origin/main) and --head')
    parser.add_argument('--work', type=Path, required=True,
                        help='new absolute scratch folder for worktrees, map and runs')
    parser.add_argument('--ticks', type=int, help='sim-bench ticks (default: its skirmish 9000)')
    parser.add_argument('--steps', default='300,1200,2200', help='capture steps')
    parser.add_argument('--zooms', default='100,150', help='Options zoom percents')
    parser.add_argument('--profile', type=Path, default=PROFILE, help='map_observation profile')
    parser.add_argument('--skip-sim', action='store_true')
    parser.add_argument('--skip-pixels', action='store_true')
    parser.add_argument('--keep-labels', action='store_true',
                        help='keep the two labelled builds (default: retire them at the end)')
    args = parser.parse_args(argv)
    if not args.ra2_dir or not Path(args.ra2_dir, 'ra2md.mix').is_file():
        parser.error('--ra2-dir (or RA2_DIR) must be the retail game folder')
    work = args.work.resolve()
    if work.exists() and any(work.iterdir()):
        parser.error(f'{work} must be new or empty')
    work.mkdir(parents=True, exist_ok=True)
    logs, runs, settings = work / 'logs', work / 'runs', work / 'vera-settings'
    logs.mkdir(), runs.mkdir()
    ra2_dir = Path(args.ra2_dir).resolve()

    head = git('rev-parse', '--verify', f'{args.head}^{{commit}}')
    base = git('rev-parse', '--verify', f'{args.base or default_base(head)}^{{commit}}')
    stamp = time.strftime('%Y%m%d%H%M%S')
    sides = [Side('base', base, work, ra2_dir, stamp), Side('head', head, work, ra2_dir, stamp)]
    print(f'base {base}\nhead {head}', flush=True)
    config = (f'[paths]\nra2_dir = "{ra2_dir.as_posix()}"\n'
              f'settings_dir = "{settings.as_posix()}"\n')
    summary = {'base': base, 'head': head, 'differences': [], 'errors': []}
    try:
        for side in sides:
            side.prepare(config)
        for side in sides:
            side.build(logs)
        summary['labels'] = [side.label for side in sides]
        if not args.skip_sim:
            map_path = work / 'match30.map'
            if run([sys.executable, '-m', 'tools.scale_map', map_path, '--starts', '30'],
                   ROOT).returncode:
                raise Failure('tools.scale_map failed')
            summary['sim'] = {}
            for side in sides:
                final, lines = side.bench(map_path, args.ticks, logs)
                rules_hash, target = side.rules_hash(runs / f'isolation-{side.name}.json')
                summary['sim'][side.name] = {'final_hash': final, 'rules_hash': rules_hash,
                                             'lines': lines, 'isolation_target': target}
            sim = summary['sim']
            for key in ('final_hash', 'rules_hash', 'isolation_target'):
                if sim['base'][key] != sim['head'][key]:
                    summary['differences'].append(f'sim {key}')
            hashes = [[line for line in sim[side]['lines'] if line.startswith('hash tick=')]
                      for side in ('base', 'head')]
            first = next((b for b, h in zip(*hashes) if b != h), None)
            if first:
                summary['differences'].append(f'sim periodic hash first differs at {first}')
        if not args.skip_pixels:
            summary['pixels'] = []
            width = json.loads(args.profile.read_text())['width']
            for zoom in map(int, args.zooms.split(',')):
                for step in map(int, args.steps.split(',')):
                    name = f'zoom{zoom}-step{step}'
                    profile = work / f'profile-{name}.json'
                    profile.write_text(json.dumps(capture_profile(args.profile, step, zoom), indent=1))
                    outputs = [runs / f'{side.name}-{name}' for side in sides]
                    for side, output in zip(sides, outputs):
                        side.capture(profile, output, settings)
                    report = runs / f'compare-{name}.json'
                    run([sys.executable, '-m', 'tools.map_observation', 'compare', '--before',
                         outputs[0], '--after', outputs[1], '--output', report], sides[1].tree)
                    if not report.exists():
                        raise Failure(f'map_observation compare wrote no report for {name}')
                    status = json.loads(report.read_text())
                    frames = [(o / 'child-output' / 'frame.bgra').read_bytes() for o in outputs]
                    pixel = first_difference(*frames, width)
                    row = {'run': name, 'compare': status['status'],
                           'fields': [d['field'] for d in status['differences']][:10],
                           'errors': status['errors'], 'frame_identical': pixel is None,
                           'first_pixel': pixel}
                    summary['pixels'].append(row)
                    if status['status'] != 'MATCH' or pixel:
                        summary['differences'].append(f'pixels {name}')
    except (Failure, subprocess.CalledProcessError) as error:
        summary['errors'].append(str(error))
    finally:
        for side in sides:
            if side.tree.exists():
                side.remove()
        if not args.keep_labels and 'labels' in summary:
            run([sys.executable, '-m', 'tools.cargo_run',
                 *[part for label in summary['labels'] for part in ('--retire-label', label)]], ROOT)
        (work / 'summary.json').write_text(json.dumps(summary, indent=1))

    print(f'\nbase {base}\nhead {head}')
    for name, data in summary.get('sim', {}).items():
        print(f'[{name}] rules_hash={data["rules_hash"]}')
        for line in data['lines']:
            if not line.startswith('hash tick='):
                print(f'[{name}] {line}')
    for row in summary.get('pixels', []):
        print(f'pixels {row["run"]}: compare={row["compare"]} frame_identical={row["frame_identical"]}'
              + (f' first_pixel={row["first_pixel"]}' if row['first_pixel'] else '')
              + (f' fields={row["fields"]}' if row['fields'] else '')
              + (f' errors={row["errors"]}' if row['errors'] else ''))
    for error in summary['errors']:
        print(f'ERROR: {error}')
    print('DIFFERENCES: ' + ', '.join(summary['differences']) if summary['differences']
          else ('INCOMPLETE' if summary['errors'] else 'SAME: hashes and pixels equal'))
    print(f'summary: {work / "summary.json"}')
    return 2 if summary['errors'] else 1 if summary['differences'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
