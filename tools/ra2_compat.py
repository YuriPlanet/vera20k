"""Reproducible RA2 (non-Yuri's Revenge) compatibility layer.

VERA20k reads Yuri's Revenge retail data by default. Running the original Red
Alert 2 campaign/data through it takes three kinds of loose-file patch, all
proven in a live run:

  1. config.toml [ini] -> custom profile with the non-md root file names
     (rules.ini / art.ini / ... instead of rulesmd.ini / artmd.ini / ...).
  2. mission.ini -> every campaign section gets LS800BkgdPal/LS640BkgdPal with
     the faction palette derived from its background SHP (A/S/B). RA2 never
     had these keys; the engine reads LS800BkgdPal and fails the loading-screen
     atlas when it is empty ("Missing usable loading palette").
  3. Campaign maps (battle.ini Scenario= list) and LDSCRNS.PAL extracted from
     the MIX archives to loose files, because the engine only mounts the md
     map archives and cannot reach the palette by name in load.mix.

This tool regenerates those patches from the player's own game installation via
the `asset` binary, so it is reproducible on any machine and after a reinstall.
It never touches the engine or rebuilds the executable. It is idempotent: a
second --apply makes no changes; --check only reports.

    python -m tools.ra2_compat --check                       # report only
    python -m tools.ra2_compat --apply                       # write patches
    python -m tools.ra2_compat --check --ra2-dir <GAME_DIR> --config <FILE>

Exit code: 0 when every patch is in place (or --apply succeeded), 1 when
--check finds a missing/wrong patch, 2 on a tooling error.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

if sys.version_info < (3, 12):
    sys.exit(f'Python 3.12 or newer is required; {sys.executable} is {sys.version.split()[0]}')

import tomllib

if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

REPO = Path(__file__).resolve().parents[1]

# The custom [ini] profile that switches the engine to RA2's non-md roots.
INI_ROOTS = {
    'profile': 'custom',
    'rules': 'rules.ini',
    'art': 'art.ini',
    'ai': 'ai.ini',
    'sound': 'sound.ini',
    'eva': 'eva.ini',
    'theme': 'theme.ini',
    'battle': 'battle.ini',
    'mission': 'mission.ini',
    'mapsel': 'mapsel.ini',
    'modes': 'mpmodes.ini',
    'map_catalog': 'missions.pkt',
}
INI_SECTION = re.compile(r'(?ms)^\[ini\]\s*\n.*?(?=^\[|\Z)')

PALETTE = 'LDSCRNS.PAL'
BG_NAME_KEY = 'LS800BkgdName'
BG_PAL_KEYS = ('LS640BkgdPal', 'LS800BkgdPal')
SCENARIO_RE = re.compile(r'(?im)^Scenario\s*=\s*(.+?)\s*$')

# RA2's loader selects the palette from the background SHP's faction letter:
# S=Soviet, A=Allied, B=Boot Camp. A single LDSCRNS.PAL for every section (the
# old patch behaviour) makes the Boot Camp and Allied loading screens use the
# wrong colours. Both Boot Camp SHPs (LS800B01/B02) share the Allied palette;
# the neutral LDSCRN.PAL is not used by any campaign loading background.
PALETTE_BY_FACTION = {'S': 'LDSCRNS.PAL', 'A': 'LDSCRNA.PAL', 'B': 'LDSCRNA.PAL'}


def palette_for_background(name: str) -> str:
    """Return the palette RA2 derives from an LS640/LS800 background SHP."""
    upper = name.upper()
    if len(upper) >= 6 and upper.startswith(('LS640', 'LS800')):
        return PALETTE_BY_FACTION.get(upper[5], PALETTE)
    return PALETTE


def log(msg: str) -> None:
    print(msg, flush=True)


class CompatError(RuntimeError):
    pass


def find_asset(explicit: str | None) -> Path:
    if explicit:
        p = Path(explicit)
        if not p.is_file():
            raise CompatError(f'asset binary not found: {p}')
        return p
    for rel in ('target/debug/asset.exe', 'target/release/asset.exe',
                'target/debug/asset', 'target/release/asset'):
        p = REPO / rel
        if p.is_file():
            return p
    raise CompatError('no asset binary found; build it with: python -m tools.cargo_run -- build --bin asset')


def asset_json(asset: Path, verb: str, *args: str, cwd: Path | None = None) -> dict:
    cmd = [str(asset), verb, *args]
    proc = subprocess.run(cmd, capture_output=True, text=True, cwd=cwd)
    if proc.returncode != 0:
        raise CompatError(f'asset {verb} failed ({proc.returncode}): {proc.stderr.strip()}')
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise CompatError(f'asset {verb} did not return JSON: {proc.stdout[:200]!r}') from exc


def read_ra2_dir(config: Path, override: str | None) -> Path:
    if override:
        return Path(override)
    if not config.is_file():
        raise CompatError(f'config not found: {config} (pass --config or --ra2-dir)')
    with config.open('rb') as handle:
        data = tomllib.load(handle)
    value = data.get('paths', {}).get('ra2_dir')
    if not value:
        raise CompatError(f'no [paths].ra2_dir in {config}; pass --ra2-dir')
    return Path(value)


def extract_bytes(asset: Path, name: str, ra2_dir: Path, work: Path) -> tuple[bytes, dict]:
    """Extract a named asset's raw bytes, ignoring any loose copy so patches
    are always rebuilt from the original MIX data."""
    out = work / name.replace('/', '_')
    info = asset_json(asset, 'extract', name, '--ra2-dir', str(ra2_dir),
                      '--all-mixes', '--out', str(out))
    path = info.get('path')
    if not path or not Path(path).is_file():
        raise CompatError(f'extract {name}: no output file in {info!r}')
    return Path(path).read_bytes(), info


def normalize_mission(raw: bytes) -> bytes:
    """Make mission.ini converge to one form regardless of starting state.

    Every section that declares LS800BkgdName gets exactly one LS640BkgdPal
    and one LS800BkgdPal right after it, using the faction palette RA2 derives
    from the SHP letter. Existing Pal lines (partial patches) are consumed and
    re-emitted in canonical order, so the result is idempotent.
    """
    newline = b'\r\n' if b'\r\n' in raw else b'\n'
    lines = raw.replace(b'\r\n', b'\n').split(b'\n')
    out: list[bytes] = []
    skip_pal = False
    for line in lines:
        stripped = line.strip()
        upper = stripped.upper()
        if upper.startswith(b'LS800BKGDPAL=') or upper.startswith(b'LS640BKGDPAL='):
            # Drop existing Pal lines; they are re-emitted after the Name line.
            continue
        out.append(line)
        if upper.startswith(b'LS800BKGDNAME='):
            name = stripped.split(b'=', 1)[1].strip().decode('ascii', 'ignore')
            palette = palette_for_background(name)
            out.append(f'LS640BkgdPal={palette}'.encode('ascii'))
            out.append(f'LS800BkgdPal={palette}'.encode('ascii'))
    result = newline.join(out)
    return result


def mission_is_patched(data: bytes) -> bool:
    lines = data.decode('ascii', 'ignore').replace('\r\n', '\n').split('\n')
    seen_name = False
    for index, line in enumerate(lines):
        stripped = line.strip()
        if not stripped.upper().startswith('LS800BKGDNAME='):
            continue
        seen_name = True
        name = stripped.split('=', 1)[1].strip()
        expected = palette_for_background(name)
        following = [item.strip() for item in lines[index + 1:index + 3]]
        if following != [f'LS640BkgdPal={expected}', f'LS800BkgdPal={expected}']:
            return False
    return seen_name


def scenario_names(asset: Path, ra2_dir: Path, work: Path) -> list[str]:
    raw, _ = extract_bytes(asset, 'battle.ini', ra2_dir, work)
    text = raw.decode('ascii', 'ignore')
    seen: dict[str, None] = {}
    for match in SCENARIO_RE.finditer(text):
        name = match.group(1).strip()
        if name:
            seen.setdefault(name.upper(), None)
    return sorted(seen)


def expected_ini_block() -> str:
    lines = ['[ini]']
    for key, value in INI_ROOTS.items():
        lines.append(f'{key} = "{value}"')
    return '\n'.join(lines) + '\n'


def config_has_ini(config: Path) -> bool:
    if not config.is_file():
        return False
    data = config.read_text(encoding='utf-8', errors='replace')
    match = INI_SECTION.search(data)
    if not match:
        return False
    block = match.group(0)
    for key, value in INI_ROOTS.items():
        if not re.search(rf'(?m)^{key}\s*=\s*"{re.escape(value)}"', block):
            return False
    return True


def write_ini_block(config: Path) -> None:
    text = config.read_text(encoding='utf-8') if config.is_file() else '[paths]\n'
    newline = '\r\n' if '\r\n' in text else '\n'
    block = expected_ini_block().replace('\n', newline)
    if INI_SECTION.search(text):
        text = INI_SECTION.sub(lambda _: block, text, count=1)
    else:
        # Insert after the [paths] section (before the next header or EOF).
        insert = re.search(r'(?ms)^\[paths\]\s*\n.*?(?=^\[|\Z)', text)
        if insert:
            text = text[:insert.end()] + block + text[insert.end():]
        else:
            text = text.rstrip(newline) + newline + block
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(text, encoding='utf-8', newline='')


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def check_one(label: str, ok: bool, detail: str) -> bool:
    log(f'  [{"OK" if ok else "MISSING"}] {label}: {detail}')
    return ok


def run_check(asset: Path, ra2_dir: Path, config: Path, work: Path) -> tuple[bool, list[str], list[str]]:
    """Return (all_ok, map_names, notes). Never modifies game/config files."""
    log(f'RA2 dir : {ra2_dir}')
    log(f'Config  : {config}')
    all_ok = True

    log('[1/4] config.toml [ini] custom profile (11 non-md roots)')
    ok = config_has_ini(config)
    all_ok &= check_one('[ini] profile', ok, 'all non-md roots set' if ok else 'profile needs update')
    if not ok:
        log(f'      expected:\n{expected_ini_block()}', end='')

    log('[2/4] mission.ini loading-screen palette keys')
    loose_mission = ra2_dir / 'mission.ini'
    if loose_mission.is_file():
        ok = mission_is_patched(loose_mission.read_bytes())
        all_ok &= check_one('loose mission.ini', ok, f'{sha256(loose_mission)[:12]} ({loose_mission.stat().st_size} B)')
    else:
        all_ok &= check_one('loose mission.ini', False, 'not present')

    log('[3/4] campaign maps from battle.ini Scenario= list')
    names = scenario_names(asset, ra2_dir, work)
    present = [n for n in names if (ra2_dir / n).is_file() and (ra2_dir / n).stat().st_size > 0]
    ok = bool(names) and len(present) == len(names)
    all_ok &= check_one('campaign maps', ok, f'{len(present)}/{len(names)} present')
    missing = [n for n in names if n not in present]

    log(f'[4/4] {PALETTE} loading palette')
    pal = ra2_dir / PALETTE
    ok = pal.is_file() and pal.stat().st_size == 768
    all_ok &= check_one(PALETTE, ok, f'{pal.stat().st_size} B' if pal.is_file() else 'not present')

    notes: list[str] = []
    if missing:
        notes.append(f'maps missing: {", ".join(missing)}')
    return all_ok, names, notes


def run_apply(asset: Path, ra2_dir: Path, config: Path, work: Path) -> None:
    log(f'RA2 dir : {ra2_dir}')
    log(f'Config  : {config}')

    log('[1/4] config.toml [ini] custom profile')
    if config_has_ini(config):
        log('  [KEEP] already correct')
    else:
        write_ini_block(config)
        log('  [WRITE] non-md roots configured')

    log('[2/4] mission.ini loading-screen palette keys')
    raw, info = extract_bytes(asset, 'mission.ini', ra2_dir, work / 'mission')
    patched = normalize_mission(raw)
    target = ra2_dir / 'mission.ini'
    if target.is_file() and target.read_bytes() == patched:
        log(f'  [KEEP] loose mission.ini already patched ({len(patched)} B)')
    else:
        target.write_bytes(patched)
        log(f'  [WRITE] loose mission.ini from {info.get("source_archive")} -> {len(patched)} B')

    log('[3/4] campaign maps from battle.ini Scenario= list')
    names = scenario_names(asset, ra2_dir, work / 'maps')
    written = kept = 0
    for name in names:
        target = ra2_dir / name
        if target.is_file() and target.stat().st_size > 0:
            kept += 1
            continue
        data, _ = extract_bytes(asset, name, ra2_dir, work / 'maps')
        target.write_bytes(data)
        written += 1
    log(f'  [DONE] {written} written, {kept} already present, {len(names)} total')

    log(f'[4/4] {PALETTE} loading palette')
    pal = ra2_dir / PALETTE
    if pal.is_file() and pal.stat().st_size == 768:
        log('  [KEEP] already present (768 B)')
    else:
        try:
            data, info = extract_bytes(asset, PALETTE, ra2_dir, work / 'pal')
            pal.write_bytes(data)
            log(f'  [WRITE] from {info.get("source_archive")} ({len(data)} B)')
        except CompatError as exc:
            log(f'  [WARN] could not extract loose palette: {exc}. The engine mounts '
                f'load.mix at runtime, so this is a safety net, not a hard requirement.')


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--check', action='store_true', help='report patch state only')
    mode.add_argument('--apply', action='store_true', help='write missing patches (idempotent)')
    parser.add_argument('--ra2-dir', help='game install dir (default: [paths].ra2_dir in --config)')
    parser.add_argument('--config', default='config.toml', help='config.toml to read/write (default: ./config.toml)')
    parser.add_argument('--asset', help='path to the asset binary (default: auto-detect target/)')
    args = parser.parse_args()

    try:
        asset = find_asset(args.asset)
        config = Path(args.config).resolve()
        ra2_dir = read_ra2_dir(config, args.ra2_dir).resolve()
        if not ra2_dir.is_dir():
            raise CompatError(f'RA2 directory does not exist: {ra2_dir}')
        with tempfile.TemporaryDirectory(prefix='ra2_compat_') as tmp:
            work = Path(tmp)
            if args.check:
                ok, _, notes = run_check(asset, ra2_dir, config, work)
                for note in notes:
                    log(f'  [NOTE] {note}')
                log(f'{"ALL PATCHES PRESENT" if ok else "PATCHES INCOMPLETE"}')
                return 0 if ok else 1
            run_apply(asset, ra2_dir, config, work)
            ok, _, notes = run_check(asset, ra2_dir, config, work)
            for note in notes:
                log(f'  [NOTE] {note}')
            log(f'{"ALL PATCHES PRESENT" if ok else "PATCHES INCOMPLETE"}')
            return 0 if ok else 1
    except CompatError as exc:
        log(f'error: {exc}')
        return 2
    except OSError as exc:
        log(f'error: {exc}')
        return 2


if __name__ == '__main__':
    sys.exit(main())
