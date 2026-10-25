#!/usr/bin/env python3
"""Build the isolated Android NativeActivity crate under the shared Cargo lock.

The repository's desktop cargo_run deliberately selects only its root
manifest. This native entry point is a separate workspace and still reuses
that runner's owned cache, source fingerprint and process-wide build lock.
"""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.cargo_run import build_lock, build_store, file_lock, owned_target, source_identity


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--offline', action='store_true')
    parser.add_argument('--ndk', type=Path, default=Path.home() / 'Library/Android/sdk/ndk/30.0.16248370')
    args = parser.parse_args()
    clang = args.ndk / 'toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android29-clang'
    if not clang.is_file():
        parser.error('Android arm64 API 29 clang was not found in --ndk')
    store, namespace = build_store(ROOT)
    target = owned_target(ROOT, namespace)
    command = ['cargo', 'check' if args.check else 'build', '--locked', '--manifest-path', str(ROOT / 'android/native/Cargo.toml'), '--target', 'aarch64-linux-android', '--lib']
    if not args.check:
        command.append('--release')
    if args.offline:
        command.append('--offline')
    env = dict(os.environ)
    env['CARGO_TARGET_DIR'] = str(target)
    env['CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER'] = str(clang)
    env['CC_aarch64_linux_android'] = str(clang)
    env['CXX_aarch64_linux_android'] = str(clang).replace('-clang', '-clang++')
    env['AR_aarch64_linux_android'] = str(clang.parent / 'llvm-ar')
    env['RUSTFLAGS'] = (env.get('RUSTFLAGS', '') + ' -C link-arg=-Wl,-z,max-page-size=16384').strip()
    deadline = time.monotonic() + 3600
    with file_lock(store / 'checkouts' / f'{namespace}.lock', deadline), build_lock(store / 'cargo.lock', 3600):
        before = source_identity(ROOT)
        print('Android target: ' + str(target), flush=True)
        result = subprocess.run(command, cwd=ROOT, env=env)
        if source_identity(ROOT) != before:
            raise RuntimeError('Sources changed during the Android build; result is not a validation')
        return result.returncode


if __name__ == '__main__':
    raise SystemExit(main())
