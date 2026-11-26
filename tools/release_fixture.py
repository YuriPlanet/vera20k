"""The real R2 publisher, with GitHub and R2 replaced by local files.

`.github/release/publish-r2.sh` runs unchanged. The `gh` and `aws` it finds on
its PATH are the stand-ins below: they serve a GitHub release from `assets/`
(described by `config.json`) and keep R2's objects in `objects/`, all under one
fixture folder. tools/test_release_downloads.py tests the publisher with it;
tools/update_test.py publishes the releases its installed games update to.
Needs bash, jq and OpenSSL 3 or later (on Windows, Git's bash and OpenSSL).
"""
from __future__ import annotations

import base64
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / ".github/release/publish-r2.sh"

# Called as `mock.py <tool> <arguments>`; the fixture's config.json says what
# GitHub holds and which failures to inject.
MOCK = r'''import hashlib, json, os, pathlib, shutil, sys
sys.stdout.reconfigure(newline="\n")
root = pathlib.Path(os.environ["PUBLISH_FIXTURE"])
config = json.loads((root / "config.json").read_text())
tool, args = sys.argv[1], sys.argv[2:]
with (root / "calls.jsonl").open("a") as output:
    output.write(json.dumps([tool, args]) + "\n")
def flag(name):
    return args[args.index(name) + 1]
def fail(code):
    print("An error occurred (" + code + ") when calling the fixture operation", file=sys.stderr)
    sys.exit(1)
if tool == "gh":
    if args[0] == "api":
        endpoint = args[1]
        if "/git/ref/tags/" in endpoint:
            print(json.dumps({"object": {"type": "commit", "sha": config["commit"]}}))
        elif endpoint.endswith("/releases/latest"):
            latest = config.get("latest_tag", config["tag"])
            if "--jq" in args:
                print(latest)
            else:
                print(json.dumps({"tag_name": latest, "draft": False, "prerelease": False}))
        elif "/releases/tags/" in endpoint:
            print(json.dumps(config["release"]))
        elif endpoint.startswith("repos/YuriPlanet/vera20k/contents/.github/release/updater-version?ref="):
            assert endpoint.endswith("?ref=" + config["commit"])
            if "updater_version" not in config:
                print("gh: Not Found (HTTP 404)", file=sys.stderr)
                sys.exit(1)
            print(config["updater_version"], end="")
        else:
            fail("UnexpectedGhEndpoint")
    elif args[:2] == ["release", "download"]:
        name = flag("--pattern")
        dest = pathlib.Path(flag("--dir")) / name
        shutil.copyfile(root / "assets" / name, dest)
        if config.get("tamper_name") == name:
            dest.write_bytes(b"changed download")
    elif args[:2] == ["attestation", "verify"]:
        if config.get("attestation_fail"):
            fail("InvalidProvenance")
        assert flag("--source-digest") == config["commit"]
        assert flag("--signer-workflow") == "YuriPlanet/vera20k/.github/workflows/release.yml"
        assert "--deny-self-hosted-runners" in args
    else:
        fail("UnexpectedGhCommand")
elif tool == "aws":
    operation = args[args.index("s3api") + 1]
    key = flag("--key")
    path = root / "objects" / key
    if operation == "head-object":
        if not path.exists():
            fail("404")
        print(json.dumps({"ContentLength": path.stat().st_size}))
    elif operation == "get-object":
        if not path.exists():
            fail("NoSuchKey")
        shutil.copyfile(path, args[-1])
        etag = '"' + hashlib.md5(path.read_bytes()).hexdigest() + '"'
        print(json.dumps({"ETag": etag, "ContentLength": path.stat().st_size}))
    elif operation == "put-object":
        if config.get("fail_put") == key:
            fail("InternalError")
        if config.get("raced_put") == key:
            # Another publisher stores the same bytes first; R2's bucket lock
            # then refuses this write before checking its precondition.
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(flag("--body"), path)
            fail("ObjectLockedByBucketPolicy")
        if "--if-none-match" in args and path.exists():
            fail("PreconditionFailed")
        if "--if-match" in args:
            if not path.exists() or flag("--if-match") != '"' + hashlib.md5(path.read_bytes()).hexdigest() + '"':
                fail("PreconditionFailed")
        if key == "latest.json" and config.get("competing_latest"):
            path.write_text(json.dumps(config["competing_latest"]))
            fail("PreconditionFailed")
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(flag("--body"), path)
        if config.get("corrupt_put") == key:
            path.write_bytes(b"storage corruption")
        print("{}")
    else:
        fail("UnexpectedS3Command")
else:
    fail("UnexpectedTool")
'''


def openssl() -> str:
    """An OpenSSL that signs with Ed25519 (`pkeyutl -rawin`, OpenSSL 3): macOS's
    own openssl is LibreSSL, and Windows has Git's beside its bash."""
    candidates = [shutil.which("openssl"), "/opt/homebrew/opt/openssl@3/bin/openssl",
                  "/usr/local/opt/openssl@3/bin/openssl"]
    if git := shutil.which("git"):
        candidates += [str(Path(git).resolve().parents[1] / folder / "openssl.exe")
                       for folder in ("mingw64/bin", "usr/bin")]
    for candidate in candidates:
        if candidate and Path(candidate).is_file():
            version = subprocess.run([candidate, "version"], capture_output=True, text=True).stdout.split()
            if version[:1] == ["OpenSSL"] and int(version[1].split(".")[0]) >= 3:
                return candidate
    raise RuntimeError("the publisher needs OpenSSL 3 or later")


def bash() -> str:
    """Git's bash on Windows, never the WSL launcher in System32. Git is in
    Git's cmd folder, or in mingw64/bin when this runs under Git's bash."""
    if os.name == "nt" and (git := shutil.which("git")):
        for folder in Path(git).resolve().parents:
            if (folder / "bin/bash.exe").is_file():
                return str(folder / "bin/bash.exe")
    return "bash"


def signing_key(directory: Path) -> tuple[str, str]:
    """A new Ed25519 key in `directory`/signing-key.pem: its PEM, and the base64
    raw public key that update-key.pub holds."""
    path = Path(directory) / "signing-key.pem"
    subprocess.run([openssl(), "genpkey", "-algorithm", "ed25519", "-out", str(path)],
                   check=True, capture_output=True)
    return path.read_text(), public_key(path)


def public_key(pem: Path) -> str:
    der = subprocess.run([openssl(), "pkey", "-in", str(pem), "-pubout", "-outform", "DER"],
                         check=True, capture_output=True).stdout
    return base64.b64encode(der[-32:]).decode()


def verify_signature(public_key: str, message: bytes, signature: str, directory: Path) -> bool:
    """OpenSSL's verdict on a base64 signature, with the key update-key.pub would hold."""
    der = Path(directory) / "verify-key.der"
    der.write_bytes(bytes.fromhex("302a300506032b6570032100") + base64.b64decode(public_key))
    (Path(directory) / "verify-message").write_bytes(message)
    (Path(directory) / "verify-signature").write_bytes(base64.b64decode(signature))
    return subprocess.run([openssl(), "pkeyutl", "-verify", "-pubin", "-keyform", "DER", "-inkey", str(der),
                           "-rawin", "-in", str(Path(directory) / "verify-message"),
                           "-sigfile", str(Path(directory) / "verify-signature")],
                          capture_output=True).returncode == 0


class Publisher:
    """publish-r2.sh beside a trusted `public_key`, in `root`: its GitHub
    release is `assets/` and `config.json`, its R2 bucket `objects/`, and
    `calls.jsonl` records every gh and aws call."""

    def __init__(self, root: Path, public_key: str):
        self.root = Path(root)
        for folder in ("bin", "assets", "objects", "publisher"):
            (self.root / folder).mkdir(parents=True, exist_ok=True)
        mock = self.root / "bin/mock.py"
        mock.write_text(MOCK)
        for tool in ("gh", "aws"):
            path = self.root / "bin" / tool
            path.write_text(f'#!/usr/bin/env bash\nexec "{Path(sys.executable).as_posix()}" '
                            f'"{mock.as_posix()}" {tool} "$@"\n', newline="\n")
            path.chmod(0o755)
        shutil.copyfile(SCRIPT, self.root / "publisher" / SCRIPT.name)
        (self.root / "publisher/update-key.pub").write_text(public_key + "\n")

    def run(self, tag: str, config: dict, signing_key: str, *arguments: str,
            build: str | None = None) -> subprocess.CompletedProcess:
        (self.root / "config.json").write_text(json.dumps(config))
        path = os.pathsep.join([str(self.root / "bin"), str(Path(openssl()).parent), os.environ["PATH"]])
        env = dict(os.environ, PATH=path, PUBLISH_FIXTURE=str(self.root), R2_ACCOUNT_ID="b" * 32,
                   AWS_ACCESS_KEY_ID="fixture-key", AWS_SECRET_ACCESS_KEY="fixture-secret",
                   R2_PUBLISH_LOCK_DIR=(self.root / "publish.lock").as_posix(), GH_TOKEN="fixture-token",
                   UPDATE_SIGNING_KEY=signing_key)
        # The multiplayer build downloads.yml read from the release's program.
        env.pop("RELEASE_BUILD", None)
        if build is not None:
            env["RELEASE_BUILD"] = build
        return subprocess.run([bash(), (self.root / "publisher" / SCRIPT.name).as_posix(), tag, *arguments],
                              env=env, capture_output=True, text=True, timeout=120)
