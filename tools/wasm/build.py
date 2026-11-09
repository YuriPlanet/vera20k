#!/usr/bin/env python3
"""Build the opt-in browser client and its minimal, asset-free host page."""
import argparse
import html
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

# The standard library ships without Wasm atomics, and rebuilding it needs nightly.
THREADS_TOOLCHAIN = 'nightly-2026-10-15'
# wasm-bindgen needs shared, imported memory and these thread-local symbols. A
# retail install needs the full 4 GiB of address space.
THREADS_RUSTFLAGS = ' '.join((
    '-C target-feature=+atomics,+bulk-memory',
    *(f'-C link-arg={arg}' for arg in (
        '--shared-memory', '--import-memory', '--max-memory=4294967296',
        '--export=__wasm_init_tls', '--export=__tls_size', '--export=__tls_align', '--export=__tls_base',
    )),
))


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


def build_label(profile, network, lan, threads=False):
    """Name this package in the page footer and diagnostics: source revision, profile and mode."""
    try:
        revision = subprocess.check_output(['git', 'describe', '--tags', '--always', '--dirty'],
                                           cwd=ROOT, text=True, stderr=subprocess.DEVNULL).strip()
    except (OSError, subprocess.CalledProcessError):
        revision = 'unknown revision'
    mode = 'LAN' if lan else 'online' if network else 'single player'
    return f'Build {revision} · {profile} · {mode}' + (' · threads' if threads else '')


def package_host(stage, destination, network, lan=False, label=None):
    """Publish only this mode's host files, retiring known network-only outputs."""
    host = (HOST / 'host.js').read_text(encoding='utf-8')
    page = (HOST / 'index.html').read_text(encoding='utf-8')
    if label:
        page = page.replace('<span id="build">Unpackaged page</span>', f'<span id="build">{html.escape(label)}</span>')
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


def package_fonts(destination):
    """Copy the bundled fonts, glyph tables and their licences to `pkg/fonts/`. The browser engine
    embeds none: it fetches Noto Sans and the CJK glyph table before the game starts, and each
    other font the first time text needs one (src/render/text_engine/web_fonts.rs)."""
    shutil.rmtree(destination, ignore_errors=True)
    destination.mkdir()
    for source in sorted((ROOT / 'src/render/fonts').iterdir()):
        if source.suffix in ('.ttf', '.otf', '.bin') or (source.name.startswith('OFL') and source.suffix == '.txt'):
            shutil.copyfile(source, destination / source.name)


def snapshot_layout(target):
    """The saved-game layout id the browser build compiles in instead of tracing the saved types
    at run time (src/sim/snapshot.rs), from this source built for the host."""
    subprocess.run([sys.executable, '-m', 'tools.cargo_run', '--', 'build', '-p', 'vera20k-core',
                    '--bin', 'snapshot-layout', '--profile', 'fast', '--locked'], cwd=ROOT, check=True)
    tool = target / 'fast' / ('snapshot-layout.exe' if os.name == 'nt' else 'snapshot-layout')
    return subprocess.check_output([str(tool)], text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    # `wasm` is the size-optimized package players download; `fast` rebuilds quickly after an edit.
    parser.add_argument('--profile', choices=['wasm', 'fast', 'release'], default='wasm')
    parser.add_argument('--network', action='store_true', help='Enable the optional WS/UDP browser transport')
    parser.add_argument('--lan', action='store_true', help='Also enable browser LAN play over WebRTC (requires --network)')
    parser.add_argument('--threads', action='store_true', help=f'Run simulation work on a Web Worker pool (builds with {THREADS_TOOLCHAIN})')
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
    features = 'wasm-lan' if args.lan else 'wasm-net' if args.network else 'wasm'
    threads = ['-Z', 'build-std=std,panic_abort'] if args.threads else []
    target = owned_target(ROOT, build_store(ROOT)[1])
    env = {**os.environ, 'VERA20K_SNAPSHOT_LAYOUT': snapshot_layout(target)}
    if args.threads:
        env |= {'RUSTUP_TOOLCHAIN': THREADS_TOOLCHAIN, 'RUSTFLAGS': THREADS_RUSTFLAGS}
    subprocess.run([
        sys.executable, '-m', 'tools.cargo_run', '--', 'rustc', '-p', 'vera20k', '--lib', *threads,
        '--target', 'wasm32-unknown-unknown', '--no-default-features', '--features', f'{features},wasm-threads' if args.threads else features,
        '--profile', args.profile, '--locked', '--crate-type', 'cdylib',
        '--', '-C', 'link-arg=-zstack-size=8388608',
    ], cwd=ROOT, env=env, check=True)
    binary = target / 'wasm32-unknown-unknown' / args.profile / 'vera20k.wasm'
    (ROOT / 'target').mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='wasm-package-', dir=ROOT / 'target') as temporary:
        stage = Path(temporary)
        subprocess.run([cli, '--target', 'web', '--out-dir', str(stage / 'pkg'), str(binary)], check=True)
        destination = ROOT / 'target/wasm-dist'
        # Only generated public files; retail data never enters the package.
        package_host(stage, destination, args.network, args.lan, build_label(args.profile, args.network, args.lan, args.threads))
        shutil.copytree(stage / 'pkg', destination / 'pkg', dirs_exist_ok=True)
        package_fonts(destination / 'pkg' / 'fonts')
    serve = 'python3 tools/wasm/serve.py' if args.threads else 'python3 -m http.server 8000 --bind 127.0.0.1 --directory target/wasm-dist'
    print(f'Browser build: {destination}\nServe with: {serve}')


if __name__ == '__main__':
    main()
