"""Sign and notarize a Release-workflow disk image without executing its app.

The protected macos-signing environment supplies an encrypted, dedicated
keychain and its password. Passwords never enter subprocess arguments or logs.
Apple supports transferring file-based keychains between Macs:
https://support.apple.com/guide/keychain-access/kyca1121/mac
"""
from __future__ import annotations

import argparse
import base64
from contextlib import contextmanager
import ctypes
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile


def command(args: list[str]) -> str:
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f"{args[0]} failed ({result.returncode}): {result.stderr.strip()}")
    return result.stdout


@contextmanager
def signing_keychain(directory: Path):
    names = ("MACOS_SIGNING_KEYCHAIN", "MACOS_SIGNING_KEYCHAIN_PASSWORD")
    if any(not os.environ.get(name) for name in names):
        raise RuntimeError("The protected macos-signing environment is missing signing credentials.")
    # Child processes do not inherit the encrypted keychain or its password.
    data = base64.b64decode(os.environ.pop(names[0]), validate=True)
    password = os.environ.pop(names[1]).encode()
    path = directory / "signing.keychain-db"
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(data)
    del data
    security = ctypes.CDLL("/System/Library/Frameworks/Security.framework/Security")
    security.SecKeychainOpen.argtypes = [ctypes.c_char_p, ctypes.POINTER(ctypes.c_void_p)]
    security.SecKeychainOpen.restype = ctypes.c_int32
    security.SecKeychainUnlock.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_bool]
    security.SecKeychainUnlock.restype = ctypes.c_int32
    security.SecKeychainLock.argtypes = [ctypes.c_void_p]
    security.SecKeychainLock.restype = ctypes.c_int32
    core = ctypes.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
    core.CFRelease.argtypes = [ctypes.c_void_p]
    reference = ctypes.c_void_p()
    original = None
    try:
        status = security.SecKeychainOpen(os.fsencode(path), ctypes.byref(reference))
        if status:
            raise RuntimeError(f"Cannot open signing keychain: OSStatus {status}")
        status = security.SecKeychainUnlock(reference, len(password), password, True)
        if status:
            raise RuntimeError(f"Cannot unlock signing keychain: OSStatus {status}")
        del password
        original = shlex.split(command(["security", "list-keychains", "-d", "user"]))
        command(["security", "list-keychains", "-d", "user", "-s", *original, str(path)])
        yield path
    finally:
        if reference:
            security.SecKeychainLock(reference)
            core.CFRelease(reference)
        if original is not None:
            command(["security", "list-keychains", "-d", "user", "-s", *original])
        path.unlink(missing_ok=True)


def submit_and_wait(image: Path, keychain: Path, records: Path, timeout: str) -> str:
    auth = ["--keychain-profile", "VERA20k-notary", "--keychain", str(keychain)]
    receipt = json.loads(command(["xcrun", "notarytool", "submit", str(image), *auth,
                                  "--output-format", "json", "--no-progress"]))
    submission = receipt["id"]
    (records / "submission.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"Apple notarization submission: {submission}", flush=True)
    # Preserve the ID immediately. A first submission can outlast this job;
    # a timeout must never silently publish or resubmit the image.
    result = subprocess.run(["xcrun", "notarytool", "wait", submission, *auth,
                             "--timeout", timeout, "--output-format", "json"],
                            capture_output=True, text=True)
    raw = result.stdout.strip() or result.stderr.strip()
    try:
        status = json.loads(raw)
    except json.JSONDecodeError:
        status = {"id": submission, "message": "No structured notarization result returned."}
    (records / "status.json").write_text(json.dumps(status, indent=2) + "\n")
    if result.returncode or status.get("status") != "Accepted":
        if status.get("status") in {"Invalid", "Rejected"}:
            subprocess.run(["xcrun", "notarytool", "log", submission, *auth,
                            str(records / "log.json")], capture_output=True, text=True)
        raise RuntimeError(f"Notarization {submission} was not accepted; see the notarization artifact. "
                           "Resume that submission rather than uploading it again.")
    command(["xcrun", "notarytool", "log", submission, *auth, str(records / "log.json")])
    return submission


def notarize(image: Path, output: Path, records: Path, identity: str, team: str, timeout: str):
    if not image.is_file() or output.exists():
        raise RuntimeError("Expected an input image and a new output path.")
    output.parent.mkdir(parents=True, exist_ok=True)
    records.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="vera20k-signing-") as temporary:
        work = Path(temporary)
        with signing_keychain(work) as keychain:
            identities = command(["security", "find-identity", "-v", "-p", "codesigning", str(keychain)])
            if identity not in identities:
                raise RuntimeError("The expected Developer ID identity is not valid in the signing keychain.")
            writable = work / "image.dmg"
            mount = work / "mount"
            mount.mkdir()
            command(["hdiutil", "convert", str(image), "-format", "UDRW", "-o", str(writable)])
            command(["hdiutil", "attach", "-nobrowse", "-readwrite", "-mountpoint", str(mount), str(writable)])
            try:
                app = mount / "VERA20K.app"
                if app.is_symlink() or not (app / "Contents/MacOS/VERA20K").is_file():
                    raise RuntimeError("The disk image does not contain the expected VERA20K app.")
                command(["codesign", "--force", "--options", "runtime", "--timestamp", "--sign", identity,
                         "--keychain", str(keychain), str(app)])
                command(["codesign", "--verify", "--strict", "--verbose=2", str(app)])
                details = subprocess.run(["codesign", "-dv", "--verbose=4", str(app)],
                                         capture_output=True, text=True, check=True).stderr
                if f"TeamIdentifier={team}" not in details or "(runtime)" not in details or "Timestamp=" not in details:
                    raise RuntimeError("The app lacks the expected team, hardened runtime or secure timestamp.")
            finally:
                command(["hdiutil", "detach", str(mount)])
            command(["hdiutil", "convert", str(writable), "-format", "UDZO", "-o", str(output)])
            command(["codesign", "--force", "--timestamp", "--sign", identity,
                     "--keychain", str(keychain), str(output)])
            command(["codesign", "--verify", "--strict", str(output)])
            submit_and_wait(output, keychain, records, timeout)
        # No signing credentials remain when the distribution checks run.
        command(["xcrun", "stapler", "staple", str(output)])
        command(["xcrun", "stapler", "validate", str(output)])
        command(["codesign", "--verify", "--strict", str(output)])
        command(["spctl", "--assess", "--type", "open", "--context", "context:primary-signature",
                 "--verbose=4", str(output)])
        command(["hdiutil", "verify", str(output)])
    print("Developer ID signing, notarization and ticket validation succeeded.", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--records", type=Path, required=True)
    parser.add_argument("--identity", required=True)
    parser.add_argument("--team", required=True)
    parser.add_argument("--timeout", default="30m")
    args = parser.parse_args()
    notarize(args.image.resolve(), args.output.resolve(), args.records.resolve(),
             args.identity, args.team, args.timeout)


if __name__ == "__main__":
    main()
