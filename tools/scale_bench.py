#!/usr/bin/env python3
"""Time the scale benchmark: one defined 20,000-unit, 30-house battle.

Usage (from the checkout root, Python >= 3.12, `RA2_DIR` set):
    python -m tools.scale_bench                  # the simulation tick, then the client frame
    python -m tools.scale_bench sim              # only the simulation tick
    python -m tools.scale_bench client           # only the client frame
    python -m tools.scale_bench hashes           # equal state hashes across thread counts and runs
    python -m tools.scale_bench --runs 5 --json target/scale/bench.json

docs/scale-benchmark.md defines the battle and what is timed. In short: the
battle is a map (`tools.scale_map --battle`), played as an ordinary skirmish of
one local house and 29 computer houses; `sim-bench --scenario battle` plays it
without a window and times every simulation tick, and the game itself plays it
from a launcher's `-SPAWN` start with its window hidden and reports what each
picture cost its main thread. Both are built with the `shipping` profile, as
released games are.

A timing is only as good as the machine is quiet, so each timed run holds the
build lock `tools.cargo_run` uses: no cooperating Cargo build starts while it
runs, and it starts only once none is running. Every figure is taken `--runs`
times (three by default); the report gives each run and their spread, and the
benchmark fails when the runs of one figure differ by more than `--spread`
percent (5 by default), since a figure that does not repeat measures the
machine, not the game.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import sys
import time
from typing import Iterator, Sequence

from tools import cargo_run

ROOT = Path(__file__).resolve().parents[1]
# The battle's map, written beside the other generated scale maps.
MAP = Path('target/scale/battle30.map')
MAP_COMMAND = ('-m', 'tools.scale_map', str(MAP), '--size', '250', '--starts', '30', '--battle')
# The worker-pool sizes the tick is timed with: every CPU thread, and the four
# an older PC has.
POOLS = (None, 4)
# The pool sizes `hashes` compares; one thread is also what a build without a
# pool runs (the browser's default package, wasm32-wasip1).
HASH_POOLS = (1, 2, 4, None)
MEASURED = re.compile(
    r'^measured ticks=(?P<ticks>\d+) mean_ms=(?P<mean>[\d.]+) p50_ms=(?P<p50>[\d.]+) '
    r'p95_ms=(?P<p95>[\d.]+) p99_ms=(?P<p99>[\d.]+) max_ms=(?P<max>[\d.]+)'
    r'(?: process_cpu_ms=(?P<cpu>[\d.]+) cores=(?P<cores>[\d.]+))?$', re.M)
FINAL_HASH = re.compile(r'^final_hash=([0-9a-f]{16})$', re.M)
BATTLE = re.compile(r'^battle fewest_mobile=(\d+) lost=(\d+) objects_made=(\d+)$', re.M)


class BenchError(Exception):
    pass


def shipping_binary(package: str, binary: str, extra: Sequence[str] = ()) -> Path:
    """Build `binary` with the shipping profile and return its executable."""
    status = cargo_run.invoke(
        ['build', '--locked', '--profile', 'shipping', '-p', package, '--bin', binary, *extra], cwd=ROOT)
    if status != 0:
        raise BenchError(f'building {binary} failed with status {status}')
    return built_binary(binary)


def built_binary(binary: str) -> Path:
    # The runner records only its host profiles; a custom profile's output is
    # in the checkout's own cache, under the profile's name.
    _, namespace = cargo_run.build_store(ROOT)
    suffix = '.exe' if os.name == 'nt' else ''
    path = cargo_run.owned_target(ROOT, namespace) / 'shipping' / f'{binary}{suffix}'
    if not path.is_file():
        raise BenchError(f'{path} is missing; run without --no-build')
    return path


def write_map() -> Path:
    """Write the battle's map. The generator is deterministic, so every run
    plays the map the checkout's generator describes."""
    subprocess.run([sys.executable, *MAP_COMMAND], cwd=ROOT, check=True)
    return (ROOT / MAP).resolve()


@contextmanager
def quiet_machine(wait_seconds: float) -> Iterator[None]:
    """Hold the shared build lock, taken once no Cargo or rustc is running."""
    store, _ = cargo_run.build_store(ROOT)
    with cargo_run.build_lock(store / 'cargo.lock', wait_seconds):
        yield


def sim_run(binary: Path, map_path: Path, threads: int | None, extra: Sequence[str]) -> dict:
    """One battle through sim-bench: its measured window and its result."""
    command = [str(binary), '--map', str(map_path), '--scenario', 'battle', *extra]
    if threads is not None:
        command += ['--threads', str(threads)]
    started = time.monotonic()
    done = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    measured = MEASURED.search(done.stdout)
    final_hash = FINAL_HASH.search(done.stdout)
    battle = BATTLE.search(done.stdout)
    if done.returncode != 0 or not (measured and final_hash and battle):
        tail = '\n'.join((done.stdout + done.stderr).splitlines()[-12:])
        raise BenchError(f'{" ".join(command)} failed with status {done.returncode}:\n{tail}')
    return {
        'threads': threads,
        'ticks': int(measured['ticks']),
        'mean_ms': float(measured['mean']),
        'p50_ms': float(measured['p50']),
        'p95_ms': float(measured['p95']),
        'p99_ms': float(measured['p99']),
        'max_ms': float(measured['max']),
        'cores': float(measured['cores']) if measured['cores'] else None,
        'final_hash': final_hash[1],
        'fewest_mobile': int(battle[1]),
        'lost': int(battle[2]),
        'objects_made': int(battle[3]),
        'seconds': round(time.monotonic() - started, 1),
    }


def spread_percent(values: Sequence[float]) -> float:
    return 100.0 * (max(values) - min(values)) / min(values) if min(values) > 0 else float('inf')


def summarize(name: str, runs: Sequence[dict], keys: Sequence[str], spread_limit: float) -> dict:
    """The median of each figure over the runs, and the mean's spread."""
    summary = {key: statistics.median(run[key] for run in runs) for key in keys}
    summary['spread_percent'] = round(spread_percent([run[keys[0]] for run in runs]), 1)
    summary['runs'] = list(runs)
    summary['repeatable'] = len(runs) < 2 or summary['spread_percent'] <= spread_limit
    figures = ' '.join(f'{key}={summary[key]:.2f}' for key in keys)
    each = ', '.join(f'{run[keys[0]]:.2f}' for run in runs)
    print(f'{name}: {figures}  (runs: {each}; spread {summary["spread_percent"]}%)', flush=True)
    return summary


def note_run(figure: str, run: dict) -> None:
    """Append one finished run to target/scale/runs.jsonl: a long benchmark
    that fails late keeps what it measured."""
    path = ROOT / 'target' / 'scale' / 'runs.jsonl'
    with open(path, 'a', encoding='utf-8', newline='\n') as log:
        log.write(json.dumps({'figure': figure, 'at': time.strftime('%Y-%m-%dT%H:%M:%S'), **run}) + '\n')


def pool_name(threads: int | None) -> str:
    return 'all_threads' if threads is None else f'{threads}_threads'


def time_sim(binary: Path, map_path: Path, args: argparse.Namespace) -> dict:
    extra = ['--ticks', str(args.ticks)] if args.ticks else []
    report = {}
    for threads in args.pools:
        runs = []
        for _ in range(args.runs):
            with quiet_machine(args.wait_seconds):
                runs.append(sim_run(binary, map_path, threads, extra))
            note_run(f'sim tick, {pool_name(threads)}', runs[-1])
        hashes = {run['final_hash'] for run in runs}
        if len(hashes) != 1:
            raise BenchError(f'{pool_name(threads)}: runs ended in different states: {sorted(hashes)}')
        report[pool_name(threads)] = summarize(
            f'sim tick, {pool_name(threads)}', runs, ('mean_ms', 'p50_ms', 'p99_ms', 'max_ms'), args.spread)
    states = {pool: summary['runs'][0]['final_hash'] for pool, summary in report.items()}
    if len(set(states.values())) != 1:
        raise BenchError(f'the battle ended in different states by thread count: {states}')
    report['final_hash'] = next(iter(states.values()))
    return report


def compare_hashes(binary: Path, map_path: Path, args: argparse.Namespace) -> dict:
    """Every frame's state hash, equal across thread counts and across runs."""
    ticks = args.ticks or 900
    out = ROOT / 'target' / 'scale' / 'hashes'
    out.mkdir(parents=True, exist_ok=True)
    files = {}
    for threads in (*HASH_POOLS, HASH_POOLS[-1]):
        name = pool_name(threads) + ('_again' if pool_name(threads) in files else '')
        path = out / f'{name}.txt'
        sim_run(binary, map_path, threads, ['--ticks', str(ticks), '--frame-hashes', str(path)])
        files[name] = path
    reference_name, reference = next(iter(files.items()))
    frames = reference.read_text().splitlines()
    different = [name for name, path in files.items() if path.read_bytes() != reference.read_bytes()]
    if different or len(frames) != ticks:
        raise BenchError(f'frame hashes differ from {reference_name}: {different or "too few frames"}')
    print(f'hashes: {ticks} frames equal across {", ".join(files)}; last {frames[-1]}', flush=True)
    return {'ticks': ticks, 'pools': list(files), 'last_frame': frames[-1]}


def pools(text: str) -> tuple[int | None, ...]:
    """`--pools`: 'all,4' is every CPU thread, then four."""
    try:
        sizes = tuple(None if part == 'all' else int(part) for part in text.split(','))
    except ValueError:
        raise argparse.ArgumentTypeError(f'{text!r}: sizes are "all" or thread counts, comma-separated') from None
    if not sizes or any(size is not None and size < 1 for size in sizes):
        raise argparse.ArgumentTypeError(f'{text!r}: a thread count is at least 1')
    return sizes


def machine() -> dict:
    name = platform.processor() or platform.machine()
    if sys.platform == 'darwin':
        name = subprocess.run(['sysctl', '-n', 'machdep.cpu.brand_string'],
                              capture_output=True, text=True).stdout.strip() or name
    return {'cpu': name, 'cpu_threads': os.cpu_count(), 'system': platform.platform()}


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('what', nargs='?', choices=('all', 'sim', 'client', 'hashes'), default='all')
    parser.add_argument('--runs', type=int, default=3, help='timed runs of each figure (default 3)')
    parser.add_argument('--ticks', type=int, help="simulation frames per run (default: sim-bench's battle run)")
    parser.add_argument('--spread', type=float, default=5.0,
                        help='most the runs of one figure may differ, in percent (default 5)')
    parser.add_argument('--json', type=Path, help='also write the report to this file')
    parser.add_argument('--no-build', action='store_true', help='time the executables already built')
    parser.add_argument('--pools', type=pools, default=POOLS, metavar='all,4',
                        help="worker-pool sizes the tick is timed with: 'all' for every CPU thread, or a count "
                             '(default all,4)')
    parser.add_argument('--wait-seconds', type=float, default=3600.0,
                        help='longest wait for a quiet machine before a run (default 3600)')
    args = parser.parse_args(argv)
    if args.runs < 1:
        parser.error('--runs must be at least 1')
    if not os.environ.get('RA2_DIR'):
        parser.error('RA2_DIR must name the retail install')
    report: dict = {
        'commit': cargo_run.git(ROOT, 'rev-parse', 'HEAD'),
        'dirty': bool(cargo_run.git(ROOT, 'status', '--porcelain')),
        'machine': machine(),
        'profile': 'shipping',
    }
    try:
        map_path = write_map()
        if args.what in ('all', 'sim', 'hashes'):
            sim = (built_binary('sim-bench') if args.no_build
                   else shipping_binary('vera20k-core', 'sim-bench'))
        if args.what in ('all', 'sim'):
            report['sim_tick'] = time_sim(sim, map_path, args)
        if args.what == 'hashes':
            report['hashes'] = compare_hashes(sim, map_path, args)
        if args.what in ('all', 'client'):
            from tools import scale_bench_client
            report['client_frame'] = scale_bench_client.time_client(map_path, args, quiet_machine, summarize)
    except BenchError as error:
        print(f'scale_bench: {error}', file=sys.stderr)
        # What was timed before the failure is still worth reading.
        report['failed'] = str(error)
    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8', newline='\n')
    if 'failed' in report:
        return 1
    unrepeatable = [name for section in ('sim_tick', 'client_frame')
                    for name, figure in report.get(section, {}).items()
                    if isinstance(figure, dict) and not figure.get('repeatable', True)]
    if unrepeatable:
        print(f'scale_bench: runs differ by more than {args.spread}%: {", ".join(unrepeatable)}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
