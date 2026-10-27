#!/usr/bin/env python3
"""Build the opt-in browser client and its minimal, asset-free host page."""
import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib

HOST = Path(__file__).resolve().parent
ROOT = HOST.parents[1]
sys.path.insert(0, str(ROOT))
from tools.cargo_run import build_store, owned_target


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', choices=['fast', 'release'], default='fast')
    args = parser.parse_args()
    lock = tomllib.loads((ROOT / 'Cargo.lock').read_text())
    version, = {p['version'] for p in lock['package'] if p['name'] == 'wasm-bindgen'}
    cli = shutil.which('wasm-bindgen')
    if not cli or subprocess.check_output([cli, '--version'], text=True).strip() != f'wasm-bindgen {version}':
        parser.error(f'Install the matching binding CLI: cargo install wasm-bindgen-cli --version {version} --locked')
    subprocess.run([
        sys.executable, '-m', 'tools.cargo_run', '--', 'rustc', '-p', 'vera20k', '--lib',
        '--target', 'wasm32-unknown-unknown', '--no-default-features', '--features', 'wasm',
        '--profile', args.profile, '--locked', '--crate-type', 'cdylib',
        '--', '-C', 'link-arg=-zstack-size=8388608',
    ], cwd=ROOT, check=True)
    target = owned_target(ROOT, build_store(ROOT)[1])
    binary = target / 'wasm32-unknown-unknown' / args.profile / 'vera20k.wasm'
    (ROOT / 'target').mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='wasm-package-', dir=ROOT / 'target') as temporary:
        stage = Path(temporary)
        subprocess.run([cli, '--target', 'web', '--out-dir', str(stage / 'pkg'), str(binary)], check=True)
        for name in ['index.html', 'host.js']:
            shutil.copyfile(HOST / name, stage / name)
        destination = ROOT / 'target/wasm-dist'
        destination.mkdir(exist_ok=True)
        # Only generated public files; retail data never enters the package.
        for name in ['index.html', 'host.js']:
            shutil.copyfile(stage / name, destination / name)
        shutil.copytree(stage / 'pkg', destination / 'pkg', dirs_exist_ok=True)
    print(f'Browser build: {destination}\nServe with: python3 -m http.server 8000 --bind 127.0.0.1 --directory target/wasm-dist')


if __name__ == '__main__':
    main()
