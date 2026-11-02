"""Package Windows releases around external signing, without signing credentials.

The Inno 7.1.0 compiler deliberately exits with its documented signature-needed
error after writing the unsigned uninstaller. Only that exact failure is accepted:
Compiler.SetupCompiler.pas:SignSetupMemoryFile and Compiler.Messages.pas at
https://github.com/jrsoftware/issrc/tree/is-7_1_0/Projects/Src .
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile


SCRIPT = Path(__file__).with_name("vera20k.iss")
PACKAGE_FILES = {
    "vera20k.exe", "config.toml.example", "README.txt", "LICENSE-GPL",
    "LGPL-2.1-or-later.txt", "THIRD_PARTY_LICENSES.html",
}
SIGNED_FILES = {"vera20k.exe", "uninstaller.exe"}
UNINSTALLER_NAME = re.compile(r"uninst-7\.1\.0-[0-9a-f]{10}\.e(?:32|64)", re.IGNORECASE)
SIGNATURE_NEEDED = (
    "Signed uninstaller mode is enabled. Using an external code-signing tool, "
    "please attach your digital signature to the following executable file:\n\n{}\n\n"
    "and compile again"
)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def regular_files(directory, expected):
    """Reject extra files and links before copying any externally supplied payload."""
    entries = list(directory.iterdir())
    if {entry.name for entry in entries} != expected or any(
        entry.is_symlink() or not entry.is_file() for entry in entries
    ):
        raise RuntimeError(f"Unexpected files in {directory}")


def validate_identity(version, build, name):
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise RuntimeError("Expected a numeric three-part version")
    if not re.fullmatch(r"[0-9]{8}-[0-9]{6}", build):
        raise RuntimeError("Expected build time YYYYMMDD-HHMMSS")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", name):
        raise RuntimeError("Expected a plain installer name without a path")


def pe_content_digest(data, unsigned_size):
    """Hash original PE bytes, excluding the three fields Authenticode changes.

Inno also compares its cached uninstaller's original bytes before embedding its
signature. Keep this check for the app too, and permit only zero alignment padding
before the certificate table; a different signed executable is not a replacement.
"""
    try:
        if data[:2] != b"MZ" or len(data) < 64:
            raise ValueError()
        pe = struct.unpack_from("<I", data, 60)[0]
        if data[pe:pe + 4] != b"PE\0\0":
            raise ValueError()
        optional = pe + 24
        optional_size = struct.unpack_from("<H", data, pe + 20)[0]
        magic = struct.unpack_from("<H", data, optional)[0]
        directory = optional + {0x10B: 96, 0x20B: 112}[magic]
        security = directory + 4 * 8
        checksum = optional + 64
        if security + 8 > optional + optional_size or optional + optional_size > unsigned_size:
            raise ValueError()
        if struct.unpack_from("<I", data, directory - 4)[0] < 5:
            raise ValueError()
        offset, size = struct.unpack_from("<II", data, security)
        if offset or size:
            if (not offset or size < 8 or offset % 8 or offset + size != len(data)
                    or not unsigned_size <= offset <= unsigned_size + 7
                    or any(data[unsigned_size:offset])):
                raise ValueError()
        elif len(data) != unsigned_size:
            raise ValueError()
        original = bytearray(data[:unsigned_size])
        original[checksum:checksum + 4] = b"\0" * 4
        original[security:security + 8] = b"\0" * 8
        return digest(original)
    except (KeyError, ValueError, struct.error):
        raise RuntimeError("Invalid PE image or unexpected changes around its signature") from None


def compile_setup(iscc, package, output, version, build, name, uninstaller=None):
    version_result = subprocess.run([str(iscc), "--version"], capture_output=True, check=True)
    if version_result.stdout.decode("utf-8-sig").strip() != "7.1.0":
        raise RuntimeError("Windows packaging requires the pinned Inno Setup 7.1.0 compiler")
    output.mkdir(parents=True, exist_ok=True)
    if (output / f"{name}-setup.exe").exists():
        raise RuntimeError("Refusing to reuse an existing setup output")
    args = [str(iscc), "--quiet", "--messages-jsonl", "--no-ide-signtools",
            f"/DAppVersion={version}", f"/DBuild={build}", f"/DPackageDir={package}",
            f"/DOutputDir={output}", f"/DOutputBaseFilename={name}-setup"]
    if uninstaller is not None:
        args.append(f"/DSignedUninstallerDir={uninstaller}")
    return subprocess.run([*args, str(SCRIPT)], capture_output=True, timeout=300)


def require_compile_success(result):
    if result.returncode:
        raise RuntimeError("Inno Setup compilation failed: " + result.stderr.decode("utf-8-sig"))


def compiled_image(output, name):
    image = output / f"{name}-setup.exe"
    if image.is_symlink() or not image.is_file():
        raise RuntimeError("Inno Setup did not produce the expected installer")
    return image


def generated_uninstaller(result, directory):
    entries = list(directory.iterdir())
    if len(entries) != 1 or entries[0].is_symlink() or not entries[0].is_file():
        raise RuntimeError("Inno Setup did not generate exactly one unsigned uninstaller")
    candidate = entries[0]
    if not UNINSTALLER_NAME.fullmatch(candidate.name):
        raise RuntimeError("Unexpected Inno Setup uninstaller cache filename")
    if candidate.name.split("-")[-1].split(".")[0].lower() != digest(candidate.read_bytes())[:10]:
        raise RuntimeError("Uninstaller contents do not match Inno Setup's cache filename")
    try:
        messages = [json.loads(line) for line in result.stderr.decode("utf-8-sig").splitlines() if line]
        errors = [message["message"].replace("\r\n", "\n") for message in messages
                  if message.get("severity") == "error"]
    except (UnicodeError, ValueError, KeyError, TypeError, AttributeError):
        raise RuntimeError("Unexpected Inno Setup compiler diagnostics") from None
    if result.returncode != 2 or errors != [SIGNATURE_NEEDED.format(candidate), "Compile aborted."]:
        raise RuntimeError("Inno Setup failed for a reason other than the expected unsigned uninstaller")
    return candidate


def prepare(package, staging, version, build, name, iscc):
    validate_identity(version, build, name)
    package, staging = package.resolve(), staging.resolve()
    regular_files(package, PACKAGE_FILES)
    staging.mkdir(parents=True, exist_ok=False)
    for filename in sorted(PACKAGE_FILES):
        shutil.copyfile(package / filename, staging / filename)
    with tempfile.TemporaryDirectory(prefix="vera20k-inno-") as temporary:
        temporary = Path(temporary)
        cache = temporary / "uninstaller"
        cache.mkdir()
        result = compile_setup(iscc, staging, temporary / "output", version, build, name, cache)
        uninstaller = generated_uninstaller(result, cache)
        original_name = uninstaller.name
        shutil.copyfile(uninstaller, staging / "uninstaller.exe")
    files = {}
    for filename in sorted(PACKAGE_FILES | {"uninstaller.exe"}):
        data = (staging / filename).read_bytes()
        files[filename] = {"size": len(data), "sha256": digest(data)}
        if filename in SIGNED_FILES:
            files[filename]["content_sha256"] = pe_content_digest(data, len(data))
    manifest = {"schema": 1, "version": version, "build": build, "name": name,
                "uninstaller_name": original_name, "files": files}
    (staging / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    return staging


SIGNATURE_QUERY = r"""
$ErrorActionPreference = 'Stop'
$paths = ConvertFrom-Json -InputObject ([Console]::In.ReadToEnd())
$result = @(foreach ($path in $paths) {
    $signature = Get-AuthenticodeSignature -LiteralPath $path
    $fingerprint = ''
    if ($null -ne $signature.SignerCertificate) {
        $fingerprint = $signature.SignerCertificate.GetCertHashString(
            [System.Security.Cryptography.HashAlgorithmName]::SHA256)
    }
    [PSCustomObject]@{path=$path; status=[string]$signature.Status;
        type=[string]$signature.SignatureType; fingerprint=$fingerprint}
})
ConvertTo-Json -InputObject $result -Compress
"""


def signature_info(paths):
    if sys.platform != "win32":
        raise RuntimeError("Authenticode verification requires Windows")
    result = subprocess.run(["pwsh", "-NoProfile", "-NonInteractive", "-Command", SIGNATURE_QUERY],
                            input=json.dumps([str(path) for path in paths]).encode(),
                            capture_output=True, check=True, timeout=120)
    return json.loads(result.stdout.decode("utf-8-sig"))


def verify(paths, fingerprint):
    fingerprint = fingerprint.replace(":", "").upper()
    if not re.fullmatch(r"[0-9A-F]{64}", fingerprint):
        raise RuntimeError("Expected the signing certificate's SHA256 fingerprint")
    if any(path.is_symlink() or not path.is_file() for path in paths):
        raise RuntimeError("Expected regular files for signature verification")
    paths = [path.resolve() for path in paths]
    signatures = signature_info(paths)
    if (not isinstance(signatures, list) or len(signatures) != len(paths)
            or any(not isinstance(signature, dict) for signature in signatures)):
        raise RuntimeError("Missing Authenticode verification results")
    for path, signature in zip(paths, signatures, strict=True):
        if (signature.get("path") != str(path) or signature.get("status") != "Valid"
                or signature.get("type") != "Authenticode"
                or signature.get("fingerprint", "").upper() != fingerprint):
            raise RuntimeError(f"Signature is untrusted, invalid or from another certificate: {path}")


def verify_installer(original_installer, signed_directory, fingerprint):
    if original_installer.is_symlink() or not original_installer.is_file():
        raise RuntimeError("Expected the original unsigned installer file")
    original_installer, signed_directory = original_installer.resolve(), signed_directory.resolve()
    regular_files(signed_directory, {original_installer.name})
    image = signed_directory / original_installer.name
    original = original_installer.read_bytes()
    expected = pe_content_digest(original, len(original))
    if pe_content_digest(image.read_bytes(), len(original)) != expected:
        raise RuntimeError("Signing changed the installer contents")
    verify([image], fingerprint)
    return image


def check_payload(payload, original_payload):
    regular_files(original_payload, PACKAGE_FILES | {"uninstaller.exe", "manifest.json"})
    regular_files(payload, PACKAGE_FILES | {"uninstaller.exe", "manifest.json"})
    original_manifest = (original_payload / "manifest.json").read_bytes()
    manifest = json.loads(original_manifest)
    if (set(manifest) != {"schema", "version", "build", "name", "uninstaller_name", "files"}
            or manifest["schema"] != 1
            or set(manifest["files"]) != PACKAGE_FILES | {"uninstaller.exe"}
            or not UNINSTALLER_NAME.fullmatch(manifest["uninstaller_name"])):
        raise RuntimeError("Invalid signing payload manifest")
    validate_identity(manifest["version"], manifest["build"], manifest["name"])
    if (payload / "manifest.json").read_bytes() != original_manifest:
        raise RuntimeError("Signing changed the signing payload manifest")
    for filename, info in manifest["files"].items():
        original = (original_payload / filename).read_bytes()
        if len(original) != info["size"] or digest(original) != info["sha256"]:
            raise RuntimeError(f"Original signing payload does not match its manifest: {filename}")
        data = (payload / filename).read_bytes()
        if filename in SIGNED_FILES:
            expected = info["content_sha256"]
            if pe_content_digest(original, len(original)) != expected:
                raise RuntimeError(f"Original PE does not match its manifest: {filename}")
            unchanged = pe_content_digest(data, len(original)) == expected
        else:
            unchanged = data == original
        if not unchanged:
            raise RuntimeError(f"Signing changed the package contents: {filename}")
    return manifest


def package_signed(payload, original_payload, output, iscc, fingerprint):
    payload, original_payload, output = payload.resolve(), original_payload.resolve(), output.resolve()
    if payload == original_payload or output.is_relative_to(original_payload):
        raise RuntimeError("Keep the original payload separate from signed files and installer output")
    manifest = check_payload(payload, original_payload)
    verify([payload / filename for filename in sorted(SIGNED_FILES)], fingerprint)
    with tempfile.TemporaryDirectory(prefix="vera20k-inno-") as temporary:
        temporary = Path(temporary)
        package = temporary / "package"
        cache = temporary / "uninstaller"
        package.mkdir()
        cache.mkdir()
        for filename in sorted(PACKAGE_FILES):
            shutil.copyfile(payload / filename, package / filename)
        shutil.copyfile(payload / "uninstaller.exe", cache / manifest["uninstaller_name"])
        result = compile_setup(iscc, package, output, manifest["version"], manifest["build"],
                               manifest["name"], cache)
        require_compile_success(result)
    return compiled_image(output, manifest["name"])


def plain(package, output, version, build, name, iscc):
    validate_identity(version, build, name)
    package, output = package.resolve(), output.resolve()
    regular_files(package, PACKAGE_FILES)
    require_compile_success(compile_setup(iscc, package, output, version, build, name))
    return compiled_image(output, name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("plain", "prepare"):
        command = commands.add_parser(name)
        command.add_argument("--package", type=Path, required=True)
        command.add_argument("--output" if name == "plain" else "--staging", type=Path, required=True)
        command.add_argument("--version", required=True)
        command.add_argument("--build", required=True)
        command.add_argument("--name", required=True)
        command.add_argument("--iscc", type=Path, required=True)
    command = commands.add_parser("package")
    command.add_argument("--signed-payload", type=Path, required=True)
    command.add_argument("--original-payload", type=Path, required=True)
    command.add_argument("--output", type=Path, required=True)
    command.add_argument("--iscc", type=Path, required=True)
    command.add_argument("--fingerprint", required=True)
    command = commands.add_parser("verify")
    command.add_argument("--fingerprint", required=True)
    command.add_argument("files", type=Path, nargs="+")
    command = commands.add_parser("verify-installer")
    command.add_argument("--original-installer", type=Path, required=True)
    command.add_argument("--signed-directory", type=Path, required=True)
    command.add_argument("--fingerprint", required=True)
    args = vars(parser.parse_args())
    operation = args.pop("command")
    if operation == "package":
        args["payload"] = args.pop("signed_payload")
        result = package_signed(**args)
    elif operation == "verify":
        args["paths"] = args.pop("files")
        verify(**args)
        result = "Authenticode signatures and signing certificate verified."
    elif operation == "verify-installer":
        result = verify_installer(**args)
    else:
        result = {"prepare": prepare, "plain": plain}[operation](**args)
    print(result)


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error)) from None
