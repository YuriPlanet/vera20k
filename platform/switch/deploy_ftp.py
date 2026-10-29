#!/usr/bin/env python3
"""Upload test artifacts with byte-for-byte readback, or fetch physical Switch reports."""
import argparse
from datetime import datetime, timezone
from ftplib import FTP, error_perm
import hashlib
from io import BytesIO
import json
from pathlib import Path, PurePosixPath
import secrets

ROOT = Path(__file__).resolve().parent


def retrieve(ftp, remote):
    result = BytesIO()
    try:
        ftp.retrbinary("RETR " + remote, result.write)
    except error_perm as error:
        if str(error).startswith("550"):
            return None
        raise
    return result.getvalue()


def directory(ftp, remote):
    ftp.cwd("/")
    for component in PurePosixPath(remote).parts:
        if component == "/":
            continue
        try:
            ftp.cwd(component)
        except error_perm:
            ftp.mkd(component)
            ftp.cwd(component)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", type=int, default=5000)
    parser.add_argument("--fetch-reports", action="store_true")
    parser.add_argument("--gpu", action="store_true", help="Deploy the live GPU NRO/NSP and retail resources")
    parser.add_argument("--client", action="store_true",
                        help="Deploy the resource-bundled client NSP (the build that carries movies)")
    parser.add_argument("--retail-dir", type=Path, help="Local retail install to stage for --gpu")
    args = parser.parse_args()
    if args.client and args.gpu:
        parser.error("--client and --gpu are mutually exclusive")
    evidence = ROOT / "target/hardware"
    evidence.mkdir(parents=True, exist_ok=True)
    ftp = FTP()
    try:
        ftp.connect(args.host, args.port, timeout=30)
    except OSError as error:
        parser.exit(1, f"FTP unavailable at {args.host}:{args.port}: {error}\n")
    ftp.login()
    try:
        if args.fetch_reports:
            reports = evidence / "reports"
            reports.mkdir(exist_ok=True)
            for folder, names in [
                ("vera20k", ["core-probe-nro.txt", "core-probe-nsp.txt",
                             "gpu-probe-nro.txt", "gpu-probe-nsp.txt", "gpu-wgpu.log"]),
                ("switch-rust-smoke", ["runtime-nro.txt", "runtime-nsp.txt"]),
            ]:
                for name in names:
                    remote = f"/switch/{folder}/{name}"
                    data = retrieve(ftp, remote)
                    if data is None:
                        print("Report not present:", remote)
                    else:
                        (reports / name).write_bytes(data)
                        print("Downloaded report:", remote)
            return
        smoke = ROOT.parents[2] / "switch-rust-smoke"
        artifacts = [
            (ROOT / "dist/vera20k-core.nro", "/switch/vera20k/vera20k-core.nro"),
            (ROOT / "dist/vera20k-core.nsp", "/nsp/vera20k-core.nsp"),
            (smoke / "dist/switch-rust-smoke.nro", "/switch/switch-rust-smoke/switch-rust-smoke.nro"),
            (smoke / "dist/switch-rust-smoke.nsp", "/nsp/switch-rust-smoke.nsp"),
        ]
        if args.client:
            # The client NSP is the artifact whose bundled retail media broke
            # once; deploy it through the same byte-readback path as the rest
            # instead of a manual copy that skips verification. Discover it by
            # glob so a version bump cannot leave this list stale.
            clients = sorted((ROOT / "dist").glob("vera20-client-*.nsp"))
            if not clients:
                raise SystemExit("No client NSP in dist; build it with build.py --client")
            client = clients[-1]
            digest = client.with_suffix(client.suffix + ".sha256")
            digest.write_text(f"{hashlib.sha256(client.read_bytes()).hexdigest()}  {client.name}\n")
            artifacts = [
                (ROOT / "dist/vera20k-client.nro", "/switch/vera20k/vera20k-client.nro"),
                (client, "/nsp/" + client.name),
                (digest, "/nsp/" + digest.name),
            ]
        if args.gpu:
            retail = args.retail_dir
            if retail is None or not all((retail / name).is_file() for name in ["ra2.mix", "ra2md.mix"]):
                raise SystemExit("--gpu requires --retail-dir containing ra2.mix and ra2md.mix")
            artifacts = [
                (ROOT / "dist/vera20k-gpu.nro", "/switch/vera20k/vera20k-gpu.nro"),
                (ROOT / "dist/vera20k-gpu.nsp", "/nsp/vera20k-gpu.nsp"),
                (ROOT.parents[1] / "tools/map_observation/examples/ai_dred_bombard.map",
                 "/switch/vera20k/maps/ai_dred_bombard.map"),
            ]
            extensions = {".mix", ".ini", ".exe", ".csf", ".fnt", ".dat", ".pal"}
            artifacts.extend(
                (local, "/game/" + local.name)
                for local in sorted(retail.iterdir())
                if local.is_file() and local.suffix.lower() in extensions and local.stat().st_size > 0
            )
        for local, _ in artifacts:
            if not local.is_file():
                raise SystemExit(f"Missing artifact: {local}")
        manifest = {"host": args.host, "port": args.port,
                    "time_utc": datetime.now(timezone.utc).isoformat(), "verified_files": []}
        for local, remote in artifacts:
            data = local.read_bytes()
            previous = retrieve(ftp, remote)
            uploaded = previous != data
            if uploaded:
                destination = PurePosixPath(remote)
                directory(ftp, str(destination.parent))
                temporary = destination.name + ".upload-" + secrets.token_hex(4)
                with local.open("rb") as source:
                    ftp.storbinary("STOR " + temporary, source, blocksize=65536)
                if retrieve(ftp, str(destination.parent / temporary)) != data:
                    raise SystemExit("FTP readback mismatch: " + remote)
                if previous is not None:
                    backup = destination.name + ".backup-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
                    ftp.rename(destination.name, backup)
                ftp.rename(temporary, destination.name)
            if retrieve(ftp, remote) != data:
                raise SystemExit("Final FTP readback mismatch: " + remote)
            manifest["verified_files"].append({"local": str(local), "remote": remote,
                                               "size": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                                               "uploaded": uploaded})
            manifest_name = "gpu-deployment.json" if args.gpu else "deployment.json"
            (evidence / manifest_name).write_text(json.dumps(manifest, indent=2) + "\n")
            print(f"Verified {'upload' if uploaded else 'existing file'}: {remote} ({len(data)} bytes)", flush=True)
    finally:
        ftp.quit()


if __name__ == "__main__":
    main()
