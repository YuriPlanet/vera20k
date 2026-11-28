"""Sign and verify VERA20k's ARM64 release APK with Android SDK Build Tools."""

import argparse
import base64
from datetime import datetime, timezone
import os
from pathlib import Path
import re
import struct
import subprocess
import tempfile
import zipfile


CERTIFICATE = Path(__file__).with_name("certificate.sha256")
ENGINE = "lib/arm64-v8a/libvera20k_android.so"


def version_code(stamp):
    """Dev and stable names for the same bytes use the same Android versionCode."""
    if not re.fullmatch(r"[0-9]{8}-[0-9]{6}", stamp):
        raise ValueError("Expected the UTC build stamp YYYYMMDD-HHMMSS")
    built = datetime.strptime(stamp, "%Y%m%d-%H%M%S").replace(tzinfo=timezone.utc)
    code = int((built - datetime(2020, 1, 1, tzinfo=timezone.utc)).total_seconds())
    if not 1 <= code <= 2_100_000_000:
        raise ValueError("Build time is outside Android's permitted versionCode range")
    return code


def check_native_library(archive, library):
    # zipalign checks where each uncompressed .so starts in the APK. Android
    # also maps its ELF PT_LOAD segments on devices with 16 KB pages.
    if library.compress_type != zipfile.ZIP_STORED:
        raise ValueError(f"Compressed native library: {library.filename}")
    with archive.open(library) as stream:
        header = stream.read(64)
        if (len(header) != 64 or header[:6] != b"\x7fELF\x02\x01"
                or struct.unpack_from("<HH", header, 16) != (3, 183)):
            raise ValueError(f"Expected an AArch64 ELF shared object: {library.filename}")
        phoff, = struct.unpack_from("<Q", header, 32)
        phsize, count = struct.unpack_from("<HH", header, 54)
        if phsize != 56 or phoff + count * phsize > library.file_size:
            raise ValueError(f"Invalid ELF program headers: {library.filename}")
        stream.seek(phoff)
        headers = struct.iter_unpack("<IIQQQQQQ", stream.read(count * phsize))
        loads = [ph for ph in headers if ph[0] == 1]
        if not loads or any(ph[7] < 16384 or ph[7] & (ph[7] - 1)
                            or (ph[3] - ph[2]) % 16384 for ph in loads):
            raise ValueError(f"Native library lacks 16 KB LOAD alignment: {library.filename}")


def check_package(tools, apk, version, code, unsigned=False):
    if not apk.is_file() or apk.is_symlink():
        raise ValueError(f"Expected a regular APK: {apk}")
    badging = subprocess.check_output([tools / "aapt2", "dump", "badging", apk], text=True)
    identity = re.match(r"^package: name='([^']*)' versionCode='([^']*)' versionName='([^']*)'", badging)
    if not identity or identity.groups() != ("org.vera20k", str(code), version):
        raise ValueError(f"Expected package org.vera20k version {version} ({code})")
    lines = badging.splitlines()
    if ("application-debuggable" in lines or "minSdkVersion:'26'" not in lines
            or "native-code: 'arm64-v8a'" not in lines):
        raise ValueError("Expected a non-debuggable ARM64 APK supporting API 26")
    subprocess.run([tools / "zipalign", "-c", "-P", "16", "4", apk], check=True)
    with zipfile.ZipFile(apk) as archive:
        if archive.namelist().count(ENGINE) != 1:
            raise ValueError(f"Expected exactly one {ENGINE}")
        for entry in archive.infolist():
            if entry.filename.startswith("lib/") and entry.filename.endswith(".so"):
                check_native_library(archive, entry)
    if unsigned:
        # The Android verifier distinguishes a missing signature from a broken
        # one. A signing block alone can also contain non-signature metadata.
        result = subprocess.run([tools / "apksigner", "verify", "--min-sdk-version", "26", apk],
                                capture_output=True, text=True)
        errors = [line for line in result.stderr.splitlines() if line.startswith("ERROR:")]
        if result.returncode != 1 or errors not in (["ERROR: Missing META-INF/MANIFEST.MF"],
                                                     ["ERROR: No JAR signatures"]):
            raise ValueError("Release signing requires an unsigned APK")


def check_signer(tools, apk):
    expected = CERTIFICATE.read_text(encoding="ascii").strip().lower()
    if not re.fullmatch(r"[0-9a-f]{64}", expected):
        raise ValueError("Invalid pinned Android certificate SHA-256")
    result = subprocess.check_output([tools / "apksigner", "verify", "--verbose", "--print-certs",
                                      "--min-sdk-version", "26", apk], text=True)
    fingerprints = re.findall(r"^Signer #\d+ certificate SHA-256 digest: ([0-9a-fA-F]+)$", result, re.M)
    if (re.findall(r"^Number of signers: (\d+)$", result, re.M) != ["1"]
            or [value.lower() for value in fingerprints] != [expected]):
        raise ValueError("APK must have one signer matching the official Android certificate")


def sign(tools, unsigned, output, version, code):
    # Only apksigner receives the password. No build tool receives the keystore
    # bytes in its environment, and the password value never enters argv.
    encoded = os.environ.pop("ANDROID_SIGNING_KEYSTORE")
    password = os.environ.pop("ANDROID_SIGNING_PASSWORD")
    check_package(tools, unsigned, version, code, unsigned=True)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=output.parent) as staging:
        signed = Path(staging) / "signed.apk"
        with tempfile.NamedTemporaryFile(dir=os.environ.get("RUNNER_TEMP"), suffix=".p12") as key:
            key.write(base64.b64decode(encoded, validate=True))
            key.flush()
            subprocess.run([tools / "apksigner", "sign", "--ks", key.name, "--ks-type", "PKCS12",
                            "--ks-key-alias", "vera20k", "--ks-pass", "env:ANDROID_SIGNING_PASSWORD",
                            "--key-pass", "env:ANDROID_SIGNING_PASSWORD", "--v4-signing-enabled", "false",
                            "--out", signed, unsigned], check=True,
                           env={**os.environ, "ANDROID_SIGNING_PASSWORD": password})
        check_package(tools, signed, version, code)
        check_signer(tools, signed)
        os.link(signed, output)  # Publish the verified bytes, refusing to replace an existing APK.


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("version-code").add_argument("stamp")
    for command in ("check-unsigned", "sign", "verify"):
        sub = commands.add_parser(command)
        sub.add_argument("apk", type=Path)
        if command == "sign":
            sub.add_argument("output", type=Path)
        sub.add_argument("--version", required=True)
        sub.add_argument("--version-code", required=True, type=int)
    args = parser.parse_args()
    try:
        if args.command == "version-code":
            print(version_code(args.stamp))
            return
        tools = Path(os.environ["ANDROID_HOME"]) / "build-tools/36.1.0"
        if args.command == "sign":
            sign(tools, args.apk, args.output, args.version, args.version_code)
        else:
            check_package(tools, args.apk, args.version, args.version_code, unsigned=args.command == "check-unsigned")
            if args.command == "verify":
                check_signer(tools, args.apk)
    except (KeyError, ValueError, OSError, zipfile.BadZipFile, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Android release APK: {error}\n")


if __name__ == "__main__":
    main()
