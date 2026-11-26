"""Build the native installer game with the current runner's gameplay profile."""

import argparse
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


def build(target, cargo_dir, profile_flag, package, binary):
    # Encoded flags keep an absolute PGO path with spaces one rustc argument.
    # Cargo gives them precedence over target rustflags, including the static
    # Windows CRT that release.yml sets. Carry those flags into both passes.
    rustflags_key = 'CARGO_TARGET_' + target.upper().replace('-', '_') + '_RUSTFLAGS'
    flags = os.environ.get(rustflags_key, '').split() + [profile_flag]
    command = ['cargo', 'build', '--locked', '--profile', 'shipping', '--target', target,
               '--target-dir', str(cargo_dir), '--no-default-features', '-p', package,
               '--bin', binary]
    env = {**os.environ, 'CARGO_ENCODED_RUSTFLAGS': '\x1f'.join(flags)}
    if target.endswith('-apple-darwin'):
        # cc 1.2 forwards Rust's PGO flags to Apple Clang. Its profile records
        # are incompatible with Rust's LLVM runtime (rust-lang/cc-rs#1986).
        # cc appends target C/C++ flags last, so these keep both passes Rust-only.
        for language in ('CFLAGS', 'CXXFLAGS'):
            flags_key = f'{language}_{target}'
            env[flags_key] = f'{env.get(flags_key, "")} -fno-profile-generate -fno-profile-use'.strip()
    print(f'Building {target} with {flags}', flush=True)
    subprocess.run(command, check=True, env=env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('target', help='native Rust target triple from the release matrix')
    args = parser.parse_args()
    target = args.target
    compiler = subprocess.check_output(['rustc', '-vV'], text=True)
    if f'host: {target}' not in compiler.splitlines():
        sys.exit(f'PGO needs a native {target} runner. rustc reports:\n{compiler}')
    sysroot = Path(subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip())
    suffix = '.exe' if os.name == 'nt' else ''
    profdata = sysroot / 'lib' / 'rustlib' / target / 'bin' / ('llvm-profdata' + suffix)
    if not profdata.is_file():
        sys.exit(f'Missing {profdata}. Install llvm-tools-preview for this Rust toolchain.')
    with tempfile.TemporaryDirectory(dir=os.environ.get('RUNNER_TEMP'), prefix='pgo-') as temporary:
        work = Path(temporary).resolve()
        raw = work / 'raw'
        raw.mkdir()
        cargo_dir = work / 'target'
        bins = cargo_dir / target / 'shipping'
        game = bins / ('vera20k' + suffix)
        generate = f'-Cprofile-generate={raw}'
        # Serialize the two fat-LTO links on the hosted runner. The game's
        # package and feature selection is identical for generation and use.
        build(target, cargo_dir, generate, 'vera20k', 'vera20k')
        build(target, cargo_dir, generate, 'vera20k-core', 'sim-bench')
        training = [sys.executable, '-m', 'tools.pgo_train', '--game', str(game),
                    '--sim-bench', str(bins / ('sim-bench' + suffix))]
        subprocess.run(training, check=True,
                       env={**os.environ, 'LLVM_PROFILE_FILE': str(raw / '%m-%p.profraw')})

        merged = work / 'game.profdata'
        subprocess.run([profdata, 'merge', '--failure-mode=any', '-o', merged, raw], check=True)
        summary = subprocess.check_output([profdata, 'show', merged], text=True)
        print(summary, flush=True)
        if not re.search(r'^Maximum function count: [1-9][0-9]*$', summary, re.MULTILINE):
            sys.exit('Gameplay training produced no executed function counters.')
        # Each runner gets fresh profile data and a fresh target directory.
        # Drop the instrumented binaries and rlibs before the final fat-LTO
        # pass to make room on hosted runners. Cargo can reuse the host tools.
        shutil.rmtree(cargo_dir / target)
        build(target, cargo_dir, f'-Cprofile-use={merged}', 'vera20k', 'vera20k')
        out = Path('dist/pgo-optimized')
        out.mkdir(parents=True, exist_ok=True)
        shipped = out / game.name
        shutil.copy2(game, shipped)
        with merged.open('rb') as profile, shipped.open('rb') as binary:
            print(f'{target} PGO profile SHA256 {hashlib.file_digest(profile, "sha256").hexdigest()}')
            print(f'{shipped} SHA256 {hashlib.file_digest(binary, "sha256").hexdigest()}')


if __name__ == '__main__':
    main()
