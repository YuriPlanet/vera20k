#!/usr/bin/env python3
"""Package the built native VERA20k library and local retail files as an APK.

Does not build Rust, install, upload, or reuse another project's APK identity.
The persistent development key stays outside the package output.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[2]
PROJECT = ROOT / "android"
OWNER = "vera20k.tools.android.package.v1"


def run(argv, env=None):
    result = subprocess.run([str(item) for item in argv], env=env, capture_output=True)
    if result.returncode:
        # Signing passwords are passed only through environment variables;
        # command arguments and ordinary tool diagnostics contain no secrets.
        raise RuntimeError(result.stderr.decode(errors="replace") or result.stdout.decode(errors="replace"))
    return result.stdout.decode(errors="replace")


def digest(path):
    hasher = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def signing_environment(directory, java_home):
    directory.mkdir(parents=True, exist_ok=True)
    directory.chmod(0o700)
    key = directory / "vera20k-development.jks"
    password_file = directory / "store-password"
    if key.exists() != password_file.exists():
        raise RuntimeError("development signing key/password pair is incomplete; preserve and repair it")
    if password_file.exists():
        password = password_file.read_text().strip()
    else:
        password = secrets.token_urlsafe(36)
        descriptor = os.open(password_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "w") as output:
            output.write(password + "\n")
    env = dict(os.environ, VERA_ANDROID_KEYSTORE=str(key), VERA_ANDROID_STORE_PASSWORD=password)
    if not key.exists():
        run([java_home / "bin/keytool", "-genkeypair", "-noprompt", "-keystore", key,
             "-storetype", "PKCS12", "-storepass:env", "VERA_ANDROID_STORE_PASSWORD",
             "-keypass:env", "VERA_ANDROID_STORE_PASSWORD", "-alias", "vera20k-development",
             "-keyalg", "RSA", "-keysize", "3072", "-validity", "10000",
             "-dname", "CN=WannaFire VERA20k Development,O=WannaFire,C=CN"], env)
        key.chmod(0o600)
    return env


def package(args):
    library = args.library.resolve(strict=True)
    assets = args.asset_root.resolve(strict=True)
    output = args.output.resolve()
    if not library.is_file() or not assets.is_dir():
        raise RuntimeError("library must be a file and asset-root a directory")
    if output == assets or output in assets.parents or assets in output.parents:
        raise RuntimeError("output must be separate from the asset source tree")
    if output.exists() and any(output.iterdir()):
        manifest = output / "package-manifest.json"
        if not args.replace or not manifest.is_file() or json.loads(manifest.read_text()).get("owner") != OWNER:
            raise RuntimeError("refusing a nonempty output without --replace and this tool's package manifest")
    required = {"ra2.mix", "ra2md.mix", "language.mix", "langmd.mix", "gamemd.exe"}
    missing = required - {path.name.casefold() for path in assets.iterdir() if path.is_file()}
    if missing:
        raise RuntimeError("retail asset-root is missing: " + ", ".join(sorted(missing)))
    readelf = args.sdk / "ndk" / args.ndk / "toolchains/llvm/prebuilt/darwin-x86_64/bin/llvm-readelf"
    identity = run([readelf, "-h", library])
    if "AArch64" not in identity or "DYN" not in identity:
        raise RuntimeError("library must be an arm64 Android ELF shared library")
    exports = run([readelf, "--dyn-syms", "--wide", library])
    if "ANativeActivity_onCreate" not in exports:
        raise RuntimeError("native library has no Android NativeActivity entry point")

    # Reuse the existing package owner's cycle/case-collision checks and exact
    # asset manifest copier rather than inventing another retail-file policy.
    spec = importlib.util.spec_from_file_location("vera_ios_package", ROOT / "tools/ios/package.py")
    copier = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(copier)
    staging = PROJECT / "out"
    if staging.exists():
        if not (staging / ".vera-package-staging").is_file():
            raise RuntimeError("refusing an unowned Android staging directory")
        shutil.rmtree(staging)
    staging.mkdir(parents=True)
    (staging / ".vera-package-staging").write_text(OWNER)
    native = staging / "jniLibs/arm64-v8a"
    native.mkdir(parents=True)
    shutil.copyfile(library, native / "libmain.so")
    bundle = staging / "assets"
    bundle.mkdir()
    print("Copying local retail files into native Android package...", flush=True)
    records = copier.copy_assets(assets, bundle / "retail-assets")
    if args.movie_subtitles:
        subtitle = args.movie_subtitles.resolve(strict=True)
        for record in records:
            if record["path"].casefold() == "subtitlemd.txt":
                (bundle / "retail-assets" / record["path"]).unlink()
        target = bundle / "retail-assets/subtitlemd.txt"
        shutil.copyfile(subtitle, target)
        records = [record for record in records if record["path"].casefold() != "subtitlemd.txt"]
        records.append({"path": "subtitlemd.txt", "bytes": target.stat().st_size, "sha256": digest(target)})
        for name in ("creditsmd.txt", "README.upstream.md"):
            attribution = subtitle.parent / name
            if attribution.is_file():
                destination = bundle / "retail-assets" / name
                shutil.copyfile(attribution, destination)
                records = [record for record in records if record["path"].casefold() != name.casefold()]
                records.append({"path": name, "bytes": destination.stat().st_size, "sha256": digest(destination)})
    records.sort(key=lambda record: record["path"])
    asset_set = hashlib.sha256(json.dumps(records, ensure_ascii=False, sort_keys=True).encode()).hexdigest()
    (bundle / "retail-manifest.json").write_text(json.dumps({
        "format": 1, "asset_set": asset_set, "files": records,
    }, ensure_ascii=False))
    icon = staging / "res/drawable/app_icon.png"
    icon.parent.mkdir(parents=True)
    shutil.copyfile(ROOT / "art/branding/wannafire-app-icon.png", icon)
    env = signing_environment(args.signing_dir, args.java_home)
    env.update(ANDROID_HOME=str(args.sdk), ANDROID_SDK_ROOT=str(args.sdk), JAVA_HOME=str(args.java_home))
    print("Building and signing native Android APK with retained development identity...", flush=True)
    gradle_output = run([args.gradle, "--no-daemon", "-p", PROJECT, ":app:assembleRelease",
                        "-PpackageVersionName=" + args.version,
                        "-PpackageVersionCode=" + str(args.version_code)], env)
    apk = PROJECT / "app/build/outputs/apk/release/app-release.apk"
    if not apk.is_file():
        raise RuntimeError("Gradle completed without the expected signed APK")
    signer = run([args.sdk / "build-tools/36.0.0/apksigner", "verify", "--verbose", "--print-certs", apk], env)
    with zipfile.ZipFile(apk) as archive:
        names = set(archive.namelist())
        if "lib/arm64-v8a/libmain.so" not in names or "assets/retail-manifest.json" not in names:
            raise RuntimeError("APK is missing its native engine or retail manifest")
    output.mkdir(parents=True, exist_ok=True)
    target = output / ("王二火大-" + args.version + "-android-arm64.apk")
    shutil.copyfile(apk, target)
    (output / "gradle-package.log").write_text(gradle_output)
    (output / "signer-certificate.txt").write_text(signer)
    receipt = {
        "owner": OWNER, "application_id": "com.wannafire.vera20k", "display_name": "王二火大",
        "version": args.version, "version_code": args.version_code, "abi": "arm64-v8a", "minimum_android_api": 29,
        "source_commit": run(["git", "-C", ROOT, "rev-parse", "HEAD"]).strip(),
        "library": {"path": str(library), "bytes": library.stat().st_size, "sha256": digest(library)},
        "asset_set": asset_set, "asset_count": len(records), "asset_bytes": sum(record["bytes"] for record in records),
        "apk": {"path": target.name, "bytes": target.stat().st_size, "sha256": digest(target)},
        "signing": {"kind": "retained-development", "certificate": signer},
        "runtime_acceptance": "not-run-by-packager",
    }
    (output / "package-manifest.json").write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n")
    print("Packaged " + str(target), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--asset-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--version", default="1.0.1")
    parser.add_argument("--version-code", type=int, default=2)
    parser.add_argument("--sdk", type=Path, default=Path.home() / "Library/Android/sdk")
    parser.add_argument("--ndk", default="30.0.16248370")
    parser.add_argument("--java-home", type=Path, default=Path("/Applications/Android Studio.app/Contents/jbr/Contents/Home"))
    parser.add_argument("--gradle", type=Path, default=next(iter((Path.home() / ".gradle/wrapper/dists/gradle-8.11.1-bin").glob("*/gradle-8.11.1/bin/gradle")), Path("gradle")))
    parser.add_argument("--signing-dir", type=Path, default=Path.home() / ".config/vera20k/android-signing")
    parser.add_argument("--movie-subtitles", type=Path)
    parser.add_argument("--replace", action="store_true")
    args = parser.parse_args()
    if args.version_code < 1 or not args.version or any(ch not in "0123456789." for ch in args.version):
        parser.error("version must be numeric and version-code positive")
    try:
        package(args)
    except (OSError, RuntimeError, ValueError) as error:
        parser.exit(1, "Android package failed: " + str(error) + "\n")


if __name__ == "__main__":
    main()
