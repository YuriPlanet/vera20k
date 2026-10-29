"""Package a Switch client or probe as an installable NSP, using local device keys."""
import json
import os
from pathlib import Path
import hashlib
import re
import shutil
import subprocess
import tempfile

TITLE_ID = "0100f52556455241"
# Application base ID: keep the lower 13 bits clear so control and program
# content resolve to the same base (Eden file_sys/common_funcs.h).
GPU_TITLE_ID = "0100f52556456000"
CLIENT_TITLE_ID = "0100f52556458000"


def icon_path(root: Path, sdk: Path, artifact: str) -> Path:
    return root / "icon.jpg" if artifact == "vera20k-client" else sdk / "libnx/default_icon.jpg"


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_package(path: Path, keys: Path, sdk: Path, title_id: str, log_directory: Path,
                   *, expect_romfs: bool = False, expect_wildcard_services: bool = False):
    """Verify the actual NSP's three NCAs, allowing only homebrew signature failures."""
    log_directory.mkdir(parents=True, exist_ok=True)
    hactool = sdk / "tools/bin/hactool"
    with tempfile.TemporaryDirectory(prefix="nsp-verify-", dir=log_directory) as temporary:
        extracted = Path(temporary)
        listing = subprocess.run([str(hactool), "-k", str(keys), "--suppresskeys", "--disablekeywarns",
                                  "-t", "pfs0", "-x", f"--outdir={extracted}", str(path)],
                                 stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, check=True)
        (log_directory / "pfs0.log").write_text(listing.stdout)
        files = list(extracted.iterdir())
        if len(files) != 3 or any(not file.name.endswith(".nca") for file in files):
            raise SystemExit("NSP does not contain exactly three NCAs")
        types = set()
        program_partitions = set()
        for file in files:
            if file.name.split(".")[0] != file_sha256(file)[:32]:
                raise SystemExit("NCA content ID/hash mismatch")
            result = subprocess.run([str(hactool), "-k", str(keys), "--suppresskeys", "--disablekeywarns",
                                     "-t", "nca", "-y", str(file)], stdout=subprocess.PIPE,
                                    stderr=subprocess.STDOUT, text=True, check=True)
            (log_directory / (file.name + ".log")).write_text(result.stdout)
            content = re.search(r"Content Type:\s+(\w+)", result.stdout)
            identifier = re.search(r"Title ID:\s+([0-9A-Fa-f]+)", result.stdout)
            if not content or not identifier or int(identifier.group(1), 16) != int(title_id, 16):
                raise SystemExit("Unable to decrypt NCA or wrong title ID")
            for line in result.stdout.splitlines():
                if "(FAIL)" in line and not re.match(r"\s*(Fixed-Key Signature|Signature)\s+\(FAIL\):", line):
                    raise SystemExit("NCA integrity check failed; see verification logs")
            if "Superblock Hash (GOOD)" not in result.stdout:
                raise SystemExit("NCA data hashes were not verified")
            if content.group(1) == "Program" and "NPDM Signature (GOOD)" not in result.stdout:
                raise SystemExit("Program NPDM signature did not verify")
            if content.group(1) == "Program" and expect_wildcard_services:
                wildcard = re.search(
                    r"Service Access Control:\s*\n\s*Hosts:\s+\*\s*\n\s*Accesses:\s+\*\s*$",
                    result.stdout,
                    re.MULTILINE,
                )
                if wildcard is None:
                    raise SystemExit("Program NPDM does not grant wildcard service access")
            if content.group(1) == "Program":
                program_partitions.update(
                    re.findall(r"Partition Type:\s+(\w+)", result.stdout)
                )
            types.add(content.group(1))
        if types != {"Program", "Control", "Meta"}:
            raise SystemExit("Missing Program/Control/Meta NCA")
        if "ExeFS" not in program_partitions:
            raise SystemExit("Program NCA is missing ExeFS")
        if expect_romfs and "RomFS" not in program_partitions:
            raise SystemExit("Program NCA is missing the requested RomFS")
        if not expect_romfs and "RomFS" in program_partitions:
            raise SystemExit("Program NCA unexpectedly contains RomFS")
    suffix = " with RomFS" if expect_romfs else ""
    print(f"NSP verified: Program/Control/Meta{suffix}, encrypted headers, content hashes, NPDM, title ID {title_id}")


def audit_packaged_romfs(path: Path, keys: Path, sdk: Path) -> None:
    """Re-read the finished NSP's RomFS and reject empty retail media.

    `verify_package` only proves the container's hashes; an asset directory
    holding a zero-byte `movmd03.mix` placeholder still produces a valid NSP
    that silently loses every in-game movie.
    """
    hactool = sdk / "tools/bin/hactool"
    def inspect(nca: Path) -> str:
        result = subprocess.run([str(hactool), "-k", str(keys), "--suppresskeys",
                                 "--disablekeywarns", "-t", "nca", "-y", str(nca)],
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                text=True, check=True)
        match = re.search(r"Content Type:\s+(\w+)", result.stdout)
        return match.group(1) if match else ""

    with tempfile.TemporaryDirectory(prefix="romfs-audit-") as temporary:
        root = Path(temporary)
        subprocess.run([str(hactool), "-k", str(keys), "--suppresskeys", "--disablekeywarns",
                        "-t", "pfs0", "-x", f"--outdir={root}", str(path)],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        romfs = root / "romfs"
        # Only the Program NCA carries RomFS. Extracting from the others first
        # would reset the same output directory and leave a near-empty tree,
        # which would audit the wrong tree and pass a broken package.
        program = next((nca for nca in sorted(root.glob("*.nca"))
                        if not nca.name.endswith(".cnmt.nca") and inspect(nca) == "Program"),
                       None)
        if program is None:
            raise SystemExit("NSP has no Program NCA; cannot audit its RomFS")
        subprocess.run([str(hactool), "-k", str(keys), "--suppresskeys", "--disablekeywarns",
                        "-t", "nca", f"--romfsdir={romfs}", str(program)],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        if not romfs.is_dir() or not any(romfs.iterdir()):
            raise SystemExit("Program NCA has no RomFS content to audit")
        empty = sorted(item.name for item in romfs.rglob("*")
                       if item.is_file() and item.stat().st_size == 0)
        if empty:
            raise SystemExit(
                "Packaged RomFS contains empty files (retail media placeholders): "
                + ", ".join(empty[:10]) + ("..." if len(empty) > 10 else "")
            )
        files = [item for item in romfs.rglob("*") if item.is_file()]
        print(f"Packaged RomFS audited: {len(files)} files, no empty members")


def verify_runtime_file(runtime: Path):
    """Reject an unexpectedly large runtime image before packaging."""
    size = runtime.stat().st_size
    if size > 32 * 1024 * 1024:
        raise SystemExit(f"Refusing oversized Switch NSO runtime: {size} bytes")
    with runtime.open("rb") as source:
        header = source.read(4)
    if header != b"NSO0":
        raise SystemExit("Expected an NSO runtime for NSP packaging")


def package(root: Path, sdk: Path, elf: Path, nacp: Path, *,
            artifact: str = "vera20k-core", title_id: str = TITLE_ID,
            romfs: Path | None = None, output_artifact: str | None = None):
    keys = Path(os.environ.get("SWITCH_KEYS", Path.home() / ".switch/prod.keys"))
    if not keys.is_file():
        raise SystemExit("NRO built. Set SWITCH_KEYS to a local device key file for NSP packaging.")
    if romfs is not None:
        romfs = romfs.resolve()
        if not romfs.is_dir():
            raise SystemExit(f"NSP asset directory does not exist: {romfs}")
        for required in ("ra2md.mix", "ra2.mix"):
            path = romfs / required
            if not path.is_file() or path.stat().st_size == 0:
                raise SystemExit(f"NSP asset directory is missing {required}: {romfs}")
    work = root / f"target/switch-build/{artifact}-nsp"
    exefs, control, output = work / "exefs", work / "control", work / "output"
    for directory in [exefs, control, output]:
        directory.mkdir(parents=True, exist_ok=True)
    subprocess.run([str(sdk / "tools/bin/elf2nso"), str(elf), str(exefs / "main")], check=True)
    verify_runtime_file(exefs / "main")
    metadata = json.loads((root / "npdm.json").read_text())
    for field in ("program_id", "program_id_range_min", "program_id_range_max"):
        metadata[field] = "0x" + title_id
    if artifact == "vera20k-gpu":
        metadata["name"] = "VeraGpuProbe"
    elif artifact == "vera20k-client":
        metadata["name"] = "Vera20k"
        # The installed application goes through libnx's normal applet,
        # account, filesystem, audio and graphics initialization. A narrow
        # service list aborts in __appInit before rust_main runs.
        if "*" not in metadata.get("service_access", []):
            raise SystemExit("Client NSP requires wildcard service access")
    npdm_source = work / "npdm.json"
    npdm_source.write_text(json.dumps(metadata, indent=2) + "\n")
    subprocess.run([str(sdk / "tools/bin/npdmtool"), str(npdm_source),
                    str(exefs / "main.npdm")], check=True)
    shutil.copyfile(nacp, control / "control.nacp")
    shutil.copyfile(icon_path(root, sdk, artifact), control / "icon_AmericanEnglish.dat")
    packer = Path(os.environ.get("HACBREWPACK", root.parents[3] / "toolchains/switch/hacbrewpack/hacbrewpack"))
    arguments = [str(packer), "-k", str(keys.resolve()), "--titleid", title_id,
                 "--exefsdir", str(exefs), "--controldir", str(control), "--nologo",
                 "--nspdir", str(output), "--ncadir", str(work / "nca"),
                 "--tempdir", str(work / "temp"), "--backupdir", str(work / "backup"),
                 "--keygeneration", "1", "--keepncadir"]
    if romfs is None:
        arguments.append("--noromfs")
    else:
        arguments.extend(["--romfsdir", str(romfs)])
    process = subprocess.run(arguments, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    for line in process.stdout.splitlines():
        print("Key area key: [redacted]" if line.startswith("Key area key") else line)
    process.check_returncode()
    target = root / f"dist/{output_artifact or artifact}.nsp"
    produced = output / f"{title_id}.nsp"
    verify_package(produced, keys, sdk, title_id, work / "verification",
                   expect_romfs=romfs is not None,
                   expect_wildcard_services=artifact == "vera20k-client")
    produced.replace(target)
    romfs_note = f", ROMFS {romfs}" if romfs is not None else ""
    print(f"Built standalone NSP: {target} (title ID {title_id}{romfs_note})")
    if romfs is not None:
        audit_packaged_romfs(target, keys, sdk)
