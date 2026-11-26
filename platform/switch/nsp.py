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
CLIENT_TITLE_ID = "0100f52556454000"  # 尤里的复仇（全数字ID；与共和国之辉 …58000、原版 …52000 区分）


def icon_path(root: Path, sdk: Path, artifact: str) -> Path:
    return root / "yuris-revenge-icon.jpg" if artifact == "vera20k-client" else sdk / "libnx/default_icon.jpg"


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_package(path: Path, keys: Path, sdk: Path, title_id: str, log_directory: Path,
                   *, expect_wildcard_services: bool = False):
    """Verify the three NCAs and require an ExeFS-only Program NCA."""
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
        if program_partitions != {"ExeFS"}:
            raise SystemExit(f"Program NCA must contain only ExeFS, found {sorted(program_partitions)}")
    print(f"NSP verified: ExeFS-only Program, Control/Meta, content hashes, NPDM, title ID {title_id}")


def verify_runtime_file(runtime: Path, *, max_bytes: int = 32 * 1024 * 1024):
    """Reject an unexpectedly large runtime image before packaging."""
    size = runtime.stat().st_size
    if size > max_bytes:
        raise SystemExit(f"Refusing oversized Switch NSO runtime: {size} bytes, limit {max_bytes}")
    with runtime.open("rb") as source:
        header = source.read(4)
    if header != b"NSO0":
        raise SystemExit("Expected an NSO runtime for NSP packaging")


def package(root: Path, sdk: Path, elf: Path, nacp: Path, *,
            artifact: str = "vera20k-core", title_id: str = TITLE_ID,
            output_artifact: str | None = None):
    keys = Path(os.environ.get("SWITCH_KEYS", Path.home() / ".switch/prod.keys"))
    if not keys.is_file():
        raise SystemExit("NRO built. Set SWITCH_KEYS to a local device key file for NSP packaging.")
    work = root / f"target/switch-build/{artifact}-nsp"
    exefs, control, output = work / "exefs", work / "control", work / "output"
    for directory in [exefs, control, output]:
        directory.mkdir(parents=True, exist_ok=True)
    subprocess.run([str(sdk / "tools/bin/elf2nso"), str(elf), str(exefs / "main")], check=True)
    # The full client includes multilingual fonts, Vulkan and the GUI renderer.
    # Keep the smaller original limit for diagnostic probes.
    client_artifacts = {"vera20k-client", "vera20k-client-profile"}
    if artifact in client_artifacts:
        verify_runtime_file(exefs / "main", max_bytes=96 * 1024 * 1024)
    else:
        verify_runtime_file(exefs / "main")
    metadata = json.loads((root / "npdm.json").read_text())
    for field in ("program_id", "program_id_range_min", "program_id_range_max"):
        metadata[field] = "0x" + title_id
    if artifact == "vera20k-gpu":
        metadata["name"] = "VeraGpuProbe"
    elif artifact == "vera20k-client":
        metadata["name"] = "YurisRevenge"
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
    for directory, expected in (
        (exefs, {"main", "main.npdm"}),
        (control, {"control.nacp", "icon_AmericanEnglish.dat"}),
    ):
        if {file.name for file in directory.iterdir()} != expected:
            raise SystemExit(f"Unexpected files in NSP staging directory: {directory}")
    packer = Path(os.environ.get("HACBREWPACK", root.parents[3] / "toolchains/switch/hacbrewpack/hacbrewpack"))
    arguments = [str(packer), "-k", str(keys.resolve()), "--titleid", title_id,
                 "--exefsdir", str(exefs), "--controldir", str(control), "--nologo",
                 "--nspdir", str(output), "--ncadir", str(work / "nca"),
                 "--tempdir", str(work / "temp"), "--backupdir", str(work / "backup"),
                 "--keygeneration", "1", "--keepncadir", "--noromfs"]
    process = subprocess.run(arguments, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    for line in process.stdout.splitlines():
        print("Key area key: [redacted]" if line.startswith("Key area key") else line)
    process.check_returncode()
    target = root / f"dist/{output_artifact or artifact}.nsp"
    produced = output / f"{title_id}.nsp"
    verify_package(produced, keys, sdk, title_id, work / "verification",
                   expect_wildcard_services=artifact == "vera20k-client")
    produced.replace(target)
    print(f"Built standalone NSP: {target} (title ID {title_id})")
