"""Run stock-YR production checks using the existing capture and assertion owners."""
from __future__ import annotations

import json
from pathlib import Path
import re
import tomllib
from typing import Any

from tools import cargo_run, map_observation, shell_capture_diff
from tools.child_process import run_child
from tools.run_test_map import BY_NAME
from tools.shell_certification import core as shell_certification, skirmish
from tools.tactical_certification.core import (
    ValidationError,
    create_directory_exclusive,
    load_json_file,
    write_bytes_exclusive,
    write_json_exclusive,
)

ROOT = Path(__file__).resolve().parents[2]
CONTRACT = ROOT / 'src/app/diagnostics/tactical_capture/contract.v2.json'
MAIN_MENU = 'main-menu-0xe2-steady'
SKIRMISH = 'skirmish-0x102-steady'
CAMPAIGN = 'campaign-all1-first-live-frame'
LAN = 'lan-0xbb-steady'
MAP_CASES = ('first-unit-query', 'cell-building-query', 'command-bar', 'save-load')
MATCH = 'headless-match'
CASES = (MAIN_MENU, SKIRMISH, CAMPAIGN, LAN, *MAP_CASES, MATCH)


def _working_directory(path: Path, retail: Path, delay: int) -> Path:
    cwd = create_directory_exclusive(path, 'private working directory')
    retail_path = json.dumps(retail.as_posix(), ensure_ascii=False)
    config = f'''[paths]
ra2_dir = {retail_path}
settings_dir = "settings"
[ini]
profile = "yr"
[gameplay]
input_delay_ticks = {delay}
'''
    write_bytes_exclusive(cwd / 'config.toml', config.encode('utf-8'))
    return cwd


def _run(command: list[str], *, cwd: Path, output: Path, timeout: int) -> str:
    child = run_child(command, cwd=cwd, temporary_directory=output, timeout_seconds=timeout)
    write_bytes_exclusive(output / 'stdout.log', child.stdout)
    write_bytes_exclusive(output / 'stderr.log', child.stderr)
    if child.errors:
        raise ValidationError('; '.join(child.errors))
    if child.timed_out or child.exit_status != 0:
        reason = 'timed out' if child.timed_out else f'exited {child.exit_status}'
        raise ValidationError(f'{command[0]} {reason}; see {output / "stderr.log"}')
    return child.stdout.decode('utf-8', errors='replace')


def _shell(case: str, output: Path, retail: Path, binary: Path,
           guard: Path | None) -> dict[str, Any]:
    cwd = _working_directory(output / 'work', retail, 15)
    capture = output / 'capture'
    _run([str(binary), '--shell-capture', case, '--width', '800', '--height', '600',
          '--cursor-x', '400', '--cursor-y', '300', '--output', str(capture)],
         cwd=cwd, output=output, timeout=180)
    if case == SKIRMISH:
        skirmish.validate_bundle(capture)
    else:
        manifest, digest, width, height, _ = shell_capture_diff.read_capture(capture)
        shell = manifest.get('shell', {})
        if (manifest.get('checkpoint'), width, height, shell.get('screen'),
                shell.get('dialog_resource_id'), shell.get('main_menu_shell_failed')) != (
                    MAIN_MENU, 800, 600, 'main-menu', 0xE2, False):
            raise ValidationError('capture did not reach the expected retail main menu')
        if guard is not None:
            report = shell_certification.compare_to_file(capture, guard, output / 'comparison.json')
            return {'production': 'PASS', 'native': report['status'],
                    'native_scope': 'four sealed 800x600 logical presentation crops',
                    'errors': report['errors']}
        return {'production': 'PASS', 'native': 'UNVERIFIED', 'frame_sha256': digest,
                'reason': 'sealed native main-menu guard unavailable'}
    return {'production': 'PASS', 'native': 'UNVERIFIED',
            'reason': 'no enrolled native capture for this skirmish checkpoint'}


def _campaign(output: Path, retail: Path, binary: Path) -> dict[str, Any]:
    cwd = _working_directory(output / 'work', retail, 15)
    capture = output / 'capture'
    _run([str(binary), '--shell-capture', CAMPAIGN, '--width', '800', '--height', '600',
          '--cursor-x', '400', '--cursor-y', '300', '--output', str(capture)],
         cwd=cwd, output=output, timeout=180)
    manifest, digest, width, height, _ = shell_capture_diff.read_capture(capture)
    if (manifest.get('checkpoint'), width, height) != (CAMPAIGN, 800, 600):
        raise ValidationError('capture did not reach the expected campaign checkpoint')
    return {'production': 'PASS', 'native': 'UNVERIFIED', 'frame_sha256': digest,
            'reason': 'no original-engine run for the campaign start route'}


def _lan(output: Path, retail: Path, binary: Path) -> dict[str, Any]:
    cwd = _working_directory(output / 'work', retail, 15)
    capture = output / 'capture'
    _run([str(binary), '--shell-capture', LAN, '--width', '800', '--height', '600',
          '--cursor-x', '400', '--cursor-y', '300', '--output', str(capture)],
         cwd=cwd, output=output, timeout=180)
    manifest, digest, _, _, _ = shell_capture_diff.read_capture(capture)
    lan = manifest.get('lan') or {}
    session = lan.get('session') or {}
    actions = [row.get('action') for row in lan.get('route', [])]
    if (manifest.get('checkpoint'), lan.get('target'), session.get('mode')) != (
            LAN, 'Lobby', 'Lan'):
        raise ValidationError('capture did not reach the expected LAN lobby')
    if actions != ['MainMenu Network', 'BB native-control name input']:
        raise ValidationError(f'LAN lobby route changed: {actions}')
    return {'production': 'PASS', 'native': 'UNVERIFIED', 'frame_sha256': digest,
            'reason': 'no original-engine run for the LAN lobby route'}


def _map(case: str, output: Path, retail: Path, binary: Path) -> dict[str, Any]:
    profile = ROOT / 'tools' / f'map_observation.{case}.example.json'
    expectations = ROOT / 'tools' / f'map_observation.{case}.expectations.json'
    _, document = load_json_file(profile, 'map observation profile')
    delay = document['input_delay_ticks']
    if type(delay) is not int or delay < 0:
        raise ValidationError('input_delay_ticks must be a nonnegative integer')
    cwd = _working_directory(output / 'work', retail, delay)
    capture = output / 'capture'
    report = map_observation.capture(profile_path=profile, contract_path=CONTRACT,
                                     output=capture, working_directory=cwd, executable=binary)
    if report['status'] != 'VALID':
        return {'production': 'ERROR', 'native': 'UNVERIFIED', 'errors': report['errors']}
    report = map_observation.inspect_run(capture, expectations_path=expectations)
    write_json_exclusive(output / 'inspection.json', report)
    if report['status'] == 'INVALID':
        return {'production': 'ERROR', 'native': 'UNVERIFIED', 'errors': report['errors']}
    assertions = report['assertions']
    return {'production': assertions['status'], 'native': 'UNVERIFIED',
            'assertions': {key: assertions[key] for key in ('pass_count', 'fail_count', 'unobserved_count')},
            'failures': [row['name'] for row in assertions['results'] if row['status'] != 'PASS'],
            'reason': 'no original-engine run for this scripted scenario'}


def _match(output: Path, retail: Path, binary: Path) -> dict[str, Any]:
    stdout = _run([str(binary), '--ra2-dir', str(retail), '--map', str(BY_NAME['match'].absolute),
                   '--scenario', 'match', '--ticks', '300', '--units', '400', '--seed', '0x5EED0001'],
                  cwd=ROOT, output=output, timeout=180)
    match = re.search(r'^final_hash=([0-9a-f]{16})\s*$', stdout, re.MULTILINE)
    if match is None or re.search(r'^ticks=300\s', stdout, re.MULTILINE) is None:
        raise ValidationError('sim-bench did not report 300 completed ticks and a final hash')
    return {'production': 'PASS', 'native': 'UNVERIFIED', 'final_hash': match[1],
            'reason': 'synthetic VERA match has no comparable original-engine run'}


def _build(cases: list[str]) -> dict[str, Path]:
    args = ['build', '--profile', 'fast']
    binaries = []
    if any(case != MATCH for case in cases):
        args += ['-p', 'vera20k', '--bin', 'vera20k']
        binaries.append('vera20k')
    if MATCH in cases:
        args += ['-p', 'vera20k-core', '--bin', 'sim-bench']
        binaries.append('sim-bench')
    if cargo_run.invoke(args, cwd=ROOT):
        raise ValidationError('game build failed')
    paths = {}
    for name in binaries:
        binary, _ = cargo_run.resolve_binary(ROOT, name, 'fast')
        if binary is None:
            raise ValidationError(f'no recorded fast {name} executable after build')
        paths[name] = binary
    return paths


def retail_directory(override: Path | None) -> Path:
    if override is None:
        with (ROOT / 'config.toml').open('rb') as stream:
            value = tomllib.load(stream).get('paths', {}).get('ra2_dir')
        if not isinstance(value, str) or not value:
            raise ValidationError('set [paths] ra2_dir in config.toml or pass --ra2-dir')
        override = ROOT / value
    retail = override.resolve(strict=True)
    if not retail.is_dir():
        raise ValidationError(f'retail game directory is not a directory: {retail}')
    return retail


def run(cases: list[str], output: Path, retail: Path, guard: Path | None) -> list[dict[str, Any]]:
    output = create_directory_exclusive(output, 'suite output')
    binaries = _build(cases)
    results = []
    for case in cases:
        work = create_directory_exclusive(output / case, 'case output')
        print(f'{case} running', flush=True)
        try:
            if case in MAP_CASES:
                result = _map(case, work, retail, binaries['vera20k'])
            elif case == MATCH:
                result = _match(work, retail, binaries['sim-bench'])
            elif case == CAMPAIGN:
                result = _campaign(work, retail, binaries['vera20k'])
            elif case == LAN:
                result = _lan(work, retail, binaries['vera20k'])
            else:
                result = _shell(case, work, retail, binaries['vera20k'], guard)
        except (OSError, ValueError, KeyError, shell_capture_diff.InputError) as exc:
            result = {'production': 'ERROR', 'native': 'UNVERIFIED', 'errors': [str(exc)]}
        print(f'{case} {result["production"]} / {result["native"]}', flush=True)
        results.append({'case': case, **result})
    write_json_exclusive(output / 'report.json', {
        'schema_version': 'vera20k.e2e-parity.v1',
        'retail_directory': str(retail),
        'executables': {name: str(binary) for name, binary in binaries.items()},
        'results': results,
    })
    return results


def exit_status(results: list[dict[str, Any]], *, production_only: bool) -> int:
    if not results or any(row['production'] != 'PASS' or row['native'] in ('INVALID', 'DRIFT')
                          for row in results):
        return 1
    if not production_only and any(row['native'] != 'MATCH' for row in results):
        return 1
    return 0
