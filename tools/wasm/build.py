#!/usr/bin/env python3
"""Build the opt-in browser client and its minimal, asset-free host page."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
from urllib.parse import urlsplit

HOST = Path(__file__).resolve().parent
ROOT = HOST.parents[1]
sys.path.insert(0, str(ROOT))
from tools.cargo_run import build_store, owned_target


def validate_proxy_url(value):
    """Validate a supplied build override; the Rust owner supplies the default."""
    if value is None:
        return
    try:
        url = urlsplit(value)
        port = url.port
    except ValueError as error:
        raise ValueError('WS_PROXY_URL must be a WebSocket origin') from error
    if (not value or value != value.strip() or any(ch.isspace() for ch in value)
            or '\\' in value or '?' in value or '#' in value
            or url.username is not None or url.password is not None
            or not url.hostname or url.path or port == 0
            or url.scheme not in ('wss', 'ws')
            or (url.scheme == 'ws' and url.hostname not in ('localhost', '127.0.0.1', '::1'))):
        raise ValueError('WS_PROXY_URL must be a wss:// origin (ws:// is allowed only on loopback)')


def package_host(stage, destination, network, lan=False):
    """Publish only this mode's host files, retiring known network-only outputs."""
    host = (HOST / 'host.js').read_text(encoding='utf-8')
    page = (HOST / 'index.html').read_text(encoding='utf-8')
    optional_files = ('network.js', 'lobby.js', 'lan_rtc.js')
    network_files = ('network.js', 'lobby.js') + (('lan_rtc.js',) if lan else ())
    if network:
        host = ''.join(f"import './{name}';\n" for name in network_files) + host
        page = page.replace(
            'Single player. Select your extracted Yuri’s Revenge game folder. Files stay on this computer.',
            'Select your extracted Yuri’s Revenge game folder. Online play can share selected maps; other game resources stay on this computer.',
        )
        for name in network_files:
            shutil.copyfile(HOST / name, stage / name)
    (stage / 'host.js').write_text(host, encoding='utf-8')
    (stage / 'index.html').write_text(page, encoding='utf-8')
    destination.mkdir(exist_ok=True)
    for name in ('index.html', 'host.js', *optional_files):
        if (stage / name).exists():
            shutil.copyfile(stage / name, destination / name)
        elif name in optional_files:
            (destination / name).unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', choices=['fast', 'release'], default='fast')
    parser.add_argument('--network', action='store_true', help='Enable the optional WS/UDP browser transport')
    parser.add_argument('--lan', action='store_true', help='Also enable browser LAN play over WebRTC (requires --network)')
    args = parser.parse_args()
    if args.lan and not args.network:
        parser.error('--lan requires --network')
    if args.network:
        try:
            validate_proxy_url(os.environ.get('WS_PROXY_URL'))
        except ValueError as error:
            parser.error(str(error))
    lock = tomllib.loads((ROOT / 'Cargo.lock').read_text())
    version, = {p['version'] for p in lock['package'] if p['name'] == 'wasm-bindgen'}
    cli = shutil.which('wasm-bindgen')
    if not cli or subprocess.check_output([cli, '--version'], text=True).strip() != f'wasm-bindgen {version}':
        parser.error(f'Install the matching binding CLI: cargo install wasm-bindgen-cli --version {version} --locked')
    subprocess.run([
        sys.executable, '-m', 'tools.cargo_run', '--', 'rustc', '-p', 'vera20k', '--lib',
        '--target', 'wasm32-unknown-unknown', '--no-default-features', '--features', 'wasm-lan' if args.lan else 'wasm-net' if args.network else 'wasm',
        '--profile', args.profile, '--locked', '--crate-type', 'cdylib',
        '--', '-C', 'link-arg=-zstack-size=8388608',
    ], cwd=ROOT, check=True)
    target = owned_target(ROOT, build_store(ROOT)[1])
    binary = target / 'wasm32-unknown-unknown' / args.profile / 'vera20k.wasm'
    (ROOT / 'target').mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='wasm-package-', dir=ROOT / 'target') as temporary:
        stage = Path(temporary)
        subprocess.run([cli, '--target', 'web', '--out-dir', str(stage / 'pkg'), str(binary)], check=True)
        destination = ROOT / 'target/wasm-dist'
        # Only generated public files; retail data never enters the package.
        package_host(stage, destination, args.network, args.lan)
        shutil.copytree(stage / 'pkg', destination / 'pkg', dirs_exist_ok=True)
    print(f'Browser build: {destination}\nServe with: python3 -m http.server 8000 --bind 127.0.0.1 --directory target/wasm-dist')


if __name__ == '__main__':
    main()
