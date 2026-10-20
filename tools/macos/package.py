#!/usr/bin/env python3
"""Package the Chinese macOS client with the approved WannaFire app icon.

The local preview uses an existing retail asset directory; it does not copy or
rewrite those game files. Build the Rust executable before invoking this tool.
"""

import argparse
import datetime
import hashlib
import json
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tempfile


OWNER = "vera20k.tools.macos.package.v1"
APP_NAME = "王二火大"
ROOT = Path(__file__).resolve().parents[2]


def run(argv):
    return subprocess.check_output(argv, stderr=subprocess.STDOUT)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def check_output(output, replace):
    if output.is_symlink():
        raise ValueError("output must not be a symlink")
    if not output.exists() or not any(output.iterdir()):
        return
    marker = output / "package-manifest.json"
    if not marker.is_file() or json.loads(marker.read_text()).get("owner") != OWNER:
        raise ValueError("refusing to replace an output not owned by this tool")
    if not replace:
        raise ValueError("output already exists; use --replace")


def package(args):
    if not re.fullmatch(r"\d+(?:\.\d+){0,2}", args.build_number):
        raise ValueError("build-number must have one to three numeric components")
    output = args.output.absolute()
    check_output(output, args.replace)
    # Reject a symlink leaf above, then compare canonical paths so '..' or a
    # symlinked parent cannot hide a source inside the directory being replaced.
    output = output.resolve()
    executable = args.executable.resolve(strict=True)
    asset_root = args.asset_root.resolve(strict=True)
    icon = (ROOT / "art/branding/wannafire-app-icon.png").resolve(strict=True)
    if not asset_root.is_dir():
        raise ValueError("asset-root must be a directory")
    if any(output == path or output in path.parents for path in (executable, asset_root, icon)):
        raise ValueError("output must be separate from all source files")
    if asset_root == output or asset_root in output.parents:
        raise ValueError("output must not be inside retail assets")
    build = run(["xcrun", "vtool", "-show-build", str(executable)]).decode()
    if not re.search(r"\bplatform\s+MACOS\b", build):
        raise ValueError("executable must be a macOS Mach-O")
    minimum = re.search(r"\bminos\s+([\d.]+)", build)
    if not minimum:
        raise ValueError("missing executable minimum macOS version")
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".wannafire-macos-", dir=output.parent))
    try:
        app = staging / (APP_NAME + ".app")
        contents = app / "Contents"
        binaries = contents / "MacOS"
        resources = contents / "Resources"
        binaries.mkdir(parents=True)
        resources.mkdir()
        shutil.copy2(executable, binaries / "vera20k")
        (binaries / "vera20k").chmod(0o755)
        (binaries / "config.toml").write_text(
            "[paths]\nra2_dir = " + json.dumps(str(asset_root), ensure_ascii=False)
            + "\n\n[graphics]\nextra_animations = true\n"
            + "\n[gameplay]\ninput_delay_ticks = 2\n\n[profile]\nname = "
            + json.dumps(args.profile_name, ensure_ascii=False) + "\n"
        )
        iconset = staging / "AppIcon.iconset"
        iconset.mkdir()
        for points in (16, 32, 128, 256, 512):
            for scale in (1, 2):
                side = points * scale
                name = "icon_{}x{}{}.png".format(points, points, "@2x" if scale == 2 else "")
                run(["sips", "-z", str(side), str(side), str(icon), "--out", str(iconset / name)])
        icns = resources / "AppIcon.icns"
        run(["iconutil", "-c", "icns", str(iconset), "-o", str(icns)])
        info = {
            "CFBundleDevelopmentRegion": "zh-Hans",
            "CFBundleName": APP_NAME,
            "CFBundleDisplayName": APP_NAME,
            "CFBundleExecutable": "vera20k",
            "CFBundleIdentifier": "org.yuriplanet.vera20k.local.macos",
            "CFBundlePackageType": "APPL",
            "CFBundleInfoDictionaryVersion": "6.0",
            "CFBundleShortVersionString": "1.0",
            "CFBundleVersion": args.build_number,
            "CFBundleIconFile": "AppIcon.icns",
            "LSMinimumSystemVersion": minimum[1],
            "NSHighResolutionCapable": True,
        }
        with (contents / "Info.plist").open("wb") as target:
            plistlib.dump(info, target)
        run(["codesign", "--force", "--deep", "--sign", "-", str(app)])
        run(["codesign", "--verify", "--deep", "--strict", str(app)])
        manifest = {
            "owner": OWNER,
            "created_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "display_name": APP_NAME,
            "app_name": app.name,
            "bundle_id": info["CFBundleIdentifier"],
            "version": "1.0",
            "build_number": args.build_number,
            "retail_assets": str(asset_root),
            "executable_source": str(executable),
            "executable": {"bytes": (binaries / "vera20k").stat().st_size,
                           "sha256": digest(binaries / "vera20k"), "platform": "MACOS"},
            "app_icon": {"source": str(icon), "sha256": digest(icon), "icns_sha256": digest(icns)},
            "signing_mode": "ad-hoc",
        }
        (staging / "package-manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
        shutil.rmtree(iconset)
        check_output(output, args.replace)
        if output.exists():
            shutil.rmtree(output)
        staging.rename(output)
        print("App: " + str(output / app.name))
    finally:
        if staging.exists():
            shutil.rmtree(staging)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--asset-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--build-number", default="20261020.3")
    parser.add_argument("--profile-name", default="Player")
    parser.add_argument("--replace", action="store_true")
    args = parser.parse_args()
    try:
        package(args)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, "error: {}\n".format(error))


if __name__ == "__main__":
    main()
