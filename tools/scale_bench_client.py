"""The scale benchmark's client half: the game plays the battle, hidden.

`tools.scale_bench` calls [`time_client`]. The game uses the shipping profile
and release features (`--profile shipping --no-default-features`). It starts as
a launcher starts it, `vera20k -SPAWN` in a folder holding
`spawn.ini` and the map (docs/spawn.md), one human against 29 computer houses.
Three spawn-only test switches make it a benchmark (tools/spawn/README.md):
`VERA20K_SPAWN_HIDDEN_WINDOW=1` keeps the window invisible,
`VERA20K_SPAWN_FRAME_COST=<frame>` records what each picture and each
simulation frame cost the main thread from that frame on, and
`VERA20K_SPAWN_LEAVE_AT_FRAME=<frame>` leaves the match there. The record comes
back in `spawn_result.json` as `frame_cost`.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import time
from typing import Callable, Sequence

from tools import scale_bench

ROOT = scale_bench.ROOT
# The battle's first contacts and the frames timed after them, as sim-bench's
# battle scenario has them (`BATTLE_WARMUP`, `BATTLE_TICKS`).
WARMUP_FRAMES = 300
MEASURED_FRAMES = 2700
# One human and 29 computer houses fill the map's 30 starts.
COMPUTERS = 29
# Seed and game speed of the launch. Speed 1 is the 45 frames a second the
# simulation's step is named for.
SEED = 0x5EED0001
GAME_SPEED = 1


def spawn_ini(computers: int = COMPUTERS, seed: int = SEED) -> str:
    """A launcher's file for one human against `computers` computer houses
    (docs/spawn.md): house Multi1 is the human, Multi2 onward the computers."""
    keys = [f'Multi{index + 2}' for index in range(computers)]
    rows = lambda value: '\n'.join(f'{key}={value(index)}' for index, key in enumerate(keys))
    return (
        '[Settings]\n'
        'Name=Bench\n'
        'Side=0\n'
        'Color=0\n'
        'PlayerCount=1\n'
        f'AIPlayers={computers}\n'
        f'Seed={seed}\n'
        f'GameSpeed={GAME_SPEED}\n'
        'Scenario=spawnmap.ini\n'
        'SpecialHouseIsAlly=No\n'
        'Credits=10000\n'
        '[HouseHandicaps]\n'
        f'{rows(lambda index: index % 3)}\n'
        '[HouseCountries]\n'
        f'{rows(lambda index: index % 10)}\n'
        '[HouseColors]\n'
        f'{rows(lambda index: (index + 1) % 8)}\n'
    )


def launch_folder(folder: Path, map_path: Path, *, computers: int = COMPUTERS, seed: int = SEED) -> Path:
    """Write the folder a launcher starts the game in. Machine paths stay in
    it and never enter the repository."""
    shutil.rmtree(folder, ignore_errors=True)
    folder.mkdir(parents=True)
    (folder / 'spawn.ini').write_text(spawn_ini(computers, seed), encoding='ascii', newline='\n')
    shutil.copyfile(map_path, folder / 'spawnmap.ini')
    (folder / 'config.toml').write_text(
        f'[paths]\nra2_dir = {json.dumps(os.environ["RA2_DIR"])}\nsettings_dir = "."\n',
        encoding='utf-8', newline='\n')
    # An empty profile, so every option is its default: with none, the game
    # would import the retail folder's RA2MD.INI (`profile_file::import_legacy`).
    # The picture is the hidden window's, 1024x768 at the default zoom: the
    # window opens at VERA's launch size (`VERA_LAUNCH_WINDOW_SIZE`) and a match
    # draws at its window's size. `[Video]` `ScreenWidth` and `ScreenHeight` do
    # not size the window.
    (folder / 'VERA.INI').write_bytes(b'')
    return folder


def client_run(binary: Path, folder: Path, frames: tuple[int, int], timeout: float) -> dict:
    """One hidden match: its `frame_cost` record."""
    first, last = frames
    env = {
        **os.environ,
        'VERA20K_SPAWN_HIDDEN_WINDOW': '1',
        'VERA20K_SPAWN_FRAME_COST': str(first),
        'VERA20K_SPAWN_LEAVE_AT_FRAME': str(last),
    }
    result = folder / 'spawn_result.json'
    result.unlink(missing_ok=True)
    started = time.monotonic()
    with open(folder / 'stdout.log', 'w', encoding='utf-8') as log:
        done = subprocess.run([str(binary), '-SPAWN'], cwd=folder, env=env, stdout=log,
                              stderr=subprocess.STDOUT, timeout=timeout)
    try:
        report = json.loads(result.read_text(encoding='utf-8'))
        cost = report['frame_cost']
    except (OSError, ValueError, KeyError) as error:
        raise scale_bench.BenchError(
            f'the game left no frame cost (exit {done.returncode}, {error!r}); '
            f'see {folder / "stdout.log"} and {folder / "logs"}') from None
    # A win, loss or draw has its own exit code. A crash after writing a
    # complete receipt must still fail the run.
    if done.returncode != report.get('exit_code'):
        raise scale_bench.BenchError(
            f'the game exited with {done.returncode}, but reported {report.get("result")} '
            f'with exit code {report.get("exit_code")}')
    if report.get('result') == 'failed' or cost.get('last_frame', 0) < last:
        raise scale_bench.BenchError(
            f'the match ended early ({report.get("result")}, frame {cost.get("last_frame")} of {last}): '
            f'{report.get("reason")}')
    cost['seconds'] = round(time.monotonic() - started, 1)
    return cost


def time_client(map_path: Path, args, quiet_machine: Callable, summarize: Callable) -> dict:
    binary = (scale_bench.built_binary('vera20k') if args.no_build else scale_bench.shipping_binary(
        'vera20k', 'vera20k', ('--no-default-features',)))
    measured = args.ticks or MEASURED_FRAMES
    frames = (WARMUP_FRAMES, WARMUP_FRAMES + measured)
    folder = ROOT / 'target' / 'scale' / 'client'
    runs: list[dict] = []
    for _ in range(args.runs):
        launch_folder(folder, map_path)
        with quiet_machine(args.wait_seconds):
            # A frame of this battle takes far longer than its 22 ms step
            # until the goal is met, so the bound is generous.
            runs.append(client_run(binary, folder, frames, timeout=1800.0))
        scale_bench.note_run('client frame', runs[-1])
    keys: Sequence[str] = ('frame_ms', 'draw_ms', 'service_ms', 'tick_ms')
    return {'frame': summarize('client frame', runs, keys, args.spread)}
