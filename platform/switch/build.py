#!/usr/bin/env python3
"""Cross-build the VERA20k client or diagnostic probes with Horizon std/newlib."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
from nsp import CLIENT_TITLE_ID, GPU_TITLE_ID, TITLE_ID, icon_path, package

ROOT = Path(__file__).resolve().parent
WORKSPACE = ROOT.parents[3]
TOOLCHAIN = "nightly-2026-10-04"
UPSTREAM_COMMIT = "3eac779b232ee47cd0eb957676c64386ac7b0f1d"
DEFAULT_CLIENT_ASSETS = ROOT / "target/emulator/master-xdg-data/eden/sdmc/game"
CLIENT_VERSION = "0.0.1"
# NACP application title shown in the Switch menu (Yuri's Revenge build).
CLIENT_TITLE = "尤里的复仇"
PROFILE_FEATURES = "horizon-frame-profile,telemetry"
# Media that the client cannot substitute: a zero-byte or absent member here
# silently removes in-game movies, music or subtitles from the installed NSP
# (a 0.0.3 build once shipped empty movmd03.mix/movies01.mix placeholders).
REQUIRED_MEDIA_ASSETS = (
    "movmd03.mix",
    "movies01.mix",
    "theme.mix",
    "thememd.mix",
    "subtitlemd.txt",
)


def check_asset_directory(assets: Path, require_media: bool) -> list[str]:
    """Return warnings for media that is absent or empty in `assets`.

    The two main MIX files are fatal; optional media only warns because some
    installs legitimately lack it, but it must never be an empty placeholder.
    """
    for required in ("ra2.mix", "ra2md.mix"):
        path = assets / required
        if not path.is_file() or not path.stat().st_size:
            return [f"NSP asset directory is missing {required}: {assets}"]
    if not require_media:
        return []
    warnings = []
    for name in REQUIRED_MEDIA_ASSETS:
        path = assets / name
        if not path.is_file():
            warnings.append(f"{name} is absent from {assets}: movies/music/subtitles will be missing")
        elif path.stat().st_size == 0:
            warnings.append(f"{name} is empty in {assets}: it is a placeholder, not retail media")
    return warnings


def configure_client_savedata(nacp: Path, title_id: str):
    """Set NacpStruct fields from the SDK's switch/nacp.h layout."""
    data = bytearray(nacp.read_bytes())
    if len(data) != 0x4000:
        raise SystemExit(f"Unexpected NACP size: {len(data):#x}")
    data[0x3025] = 1  # StartupUserAccount: Required.
    # SaveDataOwnerId, UserAccountSaveDataSize, UserAccountSaveDataJournalSize.
    struct.pack_into("<QQQ", data, 0x3078, int(title_id, 16),
                     256 * 1024 * 1024, 64 * 1024 * 1024)
    nacp.write_bytes(data)


def client_nsp_artifact(version: str) -> str:
    """Name client NSPs by their three-part release version."""
    match = re.fullmatch(r"(\d+\.\d+\.\d+)(?:\s+.+)?", version)
    if match is None:
        raise SystemExit(f"Client NSP version must start with x.y.z: {version!r}")
    return f"vera20-client-v{match.group(1)}"


def run(*arguments, **kwargs):
    print("+", " ".join(map(str, arguments)), flush=True)
    return subprocess.run(list(map(str, arguments)), cwd=ROOT, check=True, **kwargs)


def horizon_ring(work: Path, config: Path) -> Path:
    """Select ring's existing AArch64 ELF assembly for the Horizon target."""
    patch = ROOT / "third_party/ring-horizon.patch"
    patched = work / ("ring-0.17.14-" + hashlib.sha256(patch.read_bytes()).hexdigest()[:12])
    if patched.is_dir():
        return patched
    cargo_root = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    sources = list((cargo_root / "registry/src").glob("*/ring-0.17.14"))
    if not sources:
        run("cargo", f"+{TOOLCHAIN}", "fetch", "--config", config)
        sources = list((cargo_root / "registry/src").glob("*/ring-0.17.14"))
    if not sources:
        raise SystemExit("ring 0.17.14 source unavailable after cargo fetch")
    staged = patched.with_name(patched.name + ".stage")
    shutil.copytree(sources[0], staged)
    subprocess.run(["patch", "-p1", "-i", str(patch)], cwd=staged, check=True)
    staged.rename(patched)
    return patched


def normalize_std_lock(lock: Path, patch_names: set[str]) -> bool:
    """Keep std's own workspace lock from retaining stale `patch.unused` rows.

    Cargo treats `--locked` as authoritative even for build-std. A prior
    non-applied patch can therefore pin an old libc/memmap2 indefinitely. Drop
    only unresolved patch rows whose patched package is now expected to apply;
    normal packages and all unrelated std dependencies stay byte-for-byte.
    """
    source = lock.read_text()
    paragraphs = source.split("[[")
    kept = [paragraphs[0]]
    for body in paragraphs[1:]:
        match = re.match(r'patch\.unused\]\]\nname = "([^"]+)"', body)
        if match and match.group(1) in patch_names:
            continue
        kept.append(body)
    updated = "[[".join(kept)
    if updated != source:
        try:
            lock.write_text(updated)
        except OSError as error:
            print(f"warning: cannot normalize read-only std lock {lock}: {error}", file=sys.stderr)
            return False
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Compile-check without linking")
    parser.add_argument("--nsp", action="store_true",
                        help="Also package a standalone NSP with local device keys (default for linked --client)")
    parser.add_argument("--nsp-assets", metavar="DIR",
                        help=f"Bundle retail assets into client NSP ROMFS (default: {DEFAULT_CLIENT_ASSETS})")
    frontend = parser.add_mutually_exclusive_group()
    frontend.add_argument("--gpu", action="store_true",
                          help="Build the native-window production GPU probe")
    frontend.add_argument("--client", action="store_true",
                          help="Build the shared full client with the Horizon event backend")
    parser.add_argument("--profile-sim", action="store_true",
                        help="Include the opt-in per-phase simulation profiler (client only)")
    parser.add_argument("--host-test", action="store_true", help="Run the same core probe on the host")
    parser.add_argument("--check-libc-abi", action="store_true", help="Compare patched libc against SDK headers")
    parser.add_argument("--check-client-deps", action="store_true",
                        help="Audit the desktop client's graphics/window/audio dependencies for Horizon")
    parser.add_argument("--without-winit", action="store_true",
                        help="Audit Vulkan/audio for a native Switch frontend instead of the desktop window stack")
    args = parser.parse_args()
    linked_client = args.client and not (
        args.check or args.host_test or args.check_libc_abi or args.check_client_deps
    )
    if linked_client:
        args.nsp = True
        args.nsp_assets = args.nsp_assets or str(DEFAULT_CLIENT_ASSETS)
    if args.nsp_assets and not args.nsp:
        parser.error("--nsp-assets requires --nsp")
    if args.nsp_assets and not args.client:
        parser.error("--nsp-assets currently requires --client")
    if args.profile_sim and not args.client:
        parser.error("--profile-sim currently requires --client")
    if args.nsp_assets and (args.check or args.host_test or args.check_libc_abi or args.check_client_deps):
        parser.error("--nsp-assets requires a linked client build")
    if args.nsp_assets:
        assets = Path(args.nsp_assets).expanduser().resolve()
        media_warnings = check_asset_directory(assets, require_media=True)
        if any("is missing" in warning for warning in media_warnings):
            parser.error("\n".join(media_warnings))
        args.nsp_assets = str(assets)
        if media_warnings:
            print("NSP asset media warnings:", flush=True)
            for warning in media_warnings:
                print("  warning:", warning, flush=True)
    os.chdir(ROOT)
    rust_bin = Path.home() / ".cargo/bin"
    os.environ["PATH"] = str(rust_bin) + os.pathsep + os.environ["PATH"]
    pinned = WORKSPACE / "toolchains/switch/bevy-horizon-pinned"
    default_upstream = pinned if pinned.is_dir() else WORKSPACE / "toolchains/switch/bevy-horizon-reference"
    upstream = Path(os.environ.get("VERA_SWITCH_UPSTREAM", default_upstream))
    if not upstream.is_dir():
        raise SystemExit("Missing bevy-horizon reference; see README.md")
    revision = subprocess.check_output(["git", "-C", str(upstream), "rev-parse", "HEAD"], text=True).strip()
    if revision != UPSTREAM_COMMIT:
        raise SystemExit(f"Expected upstream {UPSTREAM_COMMIT}, found {revision}; preserving checkout")
    sdk = Path(os.environ.get("DEVKITPRO", WORKSPACE / "toolchains/switch/sdk/opt/devkitpro")).resolve()
    os.environ["DEVKITPRO"] = str(sdk)
    os.environ["PATH"] = f"{sdk}/devkitA64/bin:{sdk}/tools/bin:" + os.environ["PATH"]
    # cc-rs otherwise treats Horizon as the 32-bit Nintendo 3DS target.
    # Native dependencies such as ring must match the Rust AArch64 objects.
    os.environ["CC_aarch64_nintendo_horizon"] = str(sdk / "devkitA64/bin/aarch64-none-elf-gcc")
    os.environ["AR_aarch64_nintendo_horizon"] = str(sdk / "devkitA64/bin/aarch64-none-elf-ar")
    os.environ["CFLAGS_aarch64_nintendo_horizon"] = "-march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIE"
    work = ROOT / "target/switch-build"
    work.mkdir(parents=True, exist_ok=True)
    config = work / "cargo.toml"
    patches = {name: upstream / "patches" / name for name in ["libc", "memmap2", "rustix"]}
    if args.check_client_deps or args.client:
        patches["cpal"] = upstream / "patches/cpal"
    if args.client:
        patches["winit"] = ROOT / "winit-horizon"
        patches["rustix"] = upstream / "patches/rustix"
    if args.check_client_deps or args.gpu or args.client:
        patches["raw-window-handle"] = upstream / "patches/raw-window-handle"
        for name in ["wgpu", "wgpu-core", "wgpu-hal", "wgpu-core-deps-windows-linux-android"]:
            patches[name] = upstream / "patches" / name
    config.write_text("[patch.crates-io]\n" + "\n".join(
        f'{name} = {{ path = {json.dumps(str(path))} }}' for name, path in patches.items()) + "\n")
    if args.client:
        patched_ring = horizon_ring(work, config)
        with config.open("a") as fh:
            fh.write(f'ring = {{ path = {json.dumps(str(patched_ring))} }}\n')
    # getrandom 0.3.x has no Horizon backend; select the custom backend and
    # let vera20k-switch provide __getrandom_v03_custom via csrngGetRandomBytes.
    if args.client:
        with config.open("a") as fh:
            fh.write('\n[target.aarch64-nintendo-horizon]\n'
                     'rustflags = ["--cfg", "getrandom_backend=\\"custom\\""]\n')
    if args.check_libc_abi:
        # Reuse the pinned upstream checker rather than maintaining a second
        # list of C/Rust layouts. Run its Python body locally, without Docker.
        source = (upstream / "scripts/check_libc_abi.sh").read_text()
        source = source.split("python3 - <<'PYIN'\n", 1)[1].rsplit("\nPYIN", 1)[0]
        source = source.replace("/workspace", str(upstream)).replace("/opt/devkitpro", str(sdk))
        source = source.replace(str(upstream / "target/libc-abi-check"), str(ROOT / "target/libc-abi-check"))
        checker = work / "check_libc_abi.py"
        checker.write_text(source)
        run(sys.executable, checker, env=dict(os.environ, RUSTUP_TOOLCHAIN=TOOLCHAIN))
        return
    if args.host_test:
        run("cargo", f"+{TOOLCHAIN}", "test", "--lib", "--config", config, "--", "--nocapture")
        return
    sysroot = Path(subprocess.check_output(["rustc", f"+{TOOLCHAIN}", "--print", "sysroot"], text=True).strip())
    std_manifest = sysroot / "lib/rustlib/src/rust/library/Cargo.toml"
    std_lock_ready = normalize_std_lock(std_manifest.with_name("Cargo.lock"), {
        "libc", "memmap2", "rustix", "raw-window-handle", "wgpu", "wgpu-core", "wgpu-hal",
        "wgpu-core-deps-windows-linux-android",
    })
    # build-std uses a separate workspace; patching only the app would silently
    # compile std against libc's 3DS ABI. Its lock must use the ARM64 patch too.
    if std_lock_ready:
        run("cargo", f"+{TOOLCHAIN}", "update", "--manifest-path", std_manifest,
            "--config", config, "-p", "libc", "--precise", "0.2.190")
    else:
        print("warning: skipping std lock update because its source tree is read-only", file=sys.stderr)
    target = ROOT / "aarch64-nintendo-horizon.json"
    manifest = []
    if args.check_client_deps:
        probe = work / "client-deps"
        (probe / "src").mkdir(parents=True, exist_ok=True)
        desktop = (ROOT.parents[1] / "Cargo.toml").read_text()
        desktop_lock = (ROOT.parents[1] / "Cargo.lock").read_text()
        versions = {name: re.search(r'\nname = "' + name + r'"\nversion = "([^"]+)"',
                                    desktop_lock).group(1)
                    for name in ["wgpu", "winit", "rodio"]}
        # The first wgpu table is the desktop declaration; Horizon follows it.
        desktop_dependencies = {}
        for line in desktop.splitlines():
            if re.match(r"^(wgpu|winit|rodio)\s*=", line):
                desktop_dependencies.setdefault(line.split("=", 1)[0].strip(), line)
        dependencies = list(desktop_dependencies.values())
        dependencies = [re.sub(r'(?<=")\d[^\"]*(?=")',
                               "=" + versions[line.split("=", 1)[0].strip()], line, count=1)
                        for line in dependencies]
        if args.without_winit:
            dependencies = [line for line in dependencies if not line.startswith("winit")]
            dependencies = [
                f'wgpu = {{ version = "={versions["wgpu"]}", default-features = false, features = ["vulkan", "wgsl"] }}'
                if line.startswith("wgpu") else line for line in dependencies]
        (probe / "Cargo.toml").write_text(
            '[package]\nname="vera20k-client-deps-probe"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\n' + "\n".join(dependencies) + "\n")
        (probe / "src/lib.rs").write_text("// Compile the actual client dependency versions, without desktop app code.\n")
        manifest = ["--manifest-path", probe / "Cargo.toml"]
        os.environ["CARGO_TARGET_DIR"] = str(ROOT / "target")
    selected_feature = "client" if args.client else "gpu" if args.gpu else None
    selected_features = [selected_feature] if selected_feature else []
    if args.profile_sim:
        selected_features.extend(PROFILE_FEATURES.split(","))
        source_revision = subprocess.check_output(
            ["git", "-C", str(ROOT), "rev-parse", "--short=12", "HEAD"],
            text=True,
        ).strip()
        os.environ["VERA_SWITCH_BUILD"] = (
            source_revision + "@" + datetime.now(timezone.utc).isoformat()
        )
    features = ["--features", ",".join(selected_features)] if selected_features else []
    run("cargo", f"+{TOOLCHAIN}", "check" if args.check or args.check_client_deps else "build", *manifest, *features,
        "--release", "--config", config, "--target", target,
        "-Zbuild-std=std,panic_abort", "-Zjson-target-spec", "--lib")
    if args.check or args.check_client_deps:
        return
    settings = json.loads(target.read_text())
    linker_script = work / "switch.ld"
    # Upstream uses LLD. GNU ld evaluates the self-referential ALIGNOF here
    # differently; page-align TLS explicitly and keep the alignment assertion.
    script = settings["link-script"].replace(
        ".tdata ALIGN(MAX(ALIGNOF(.tdata), ALIGNOF(.tbss)))",
        ".tdata ALIGN(0x1000)")
    # GNU ld can evaluate this assertion before its final relaxation/layout.
    # Check the actual PT_TLS in the finished ELF instead (below).
    script = "\n".join(line for line in script.splitlines()
                       if "ASSERT(ADDR(.tdata)" not in line) + "\n"
    linker_script.write_text(script)
    specs = work / "switch.specs"
    original = (sdk / "libnx/switch.specs").read_text()
    original = original.replace("%:getenv(DEVKITPRO /libnx/switch.ld)", str(linker_script))
    wrappers = "--wrap=threadCreate" + (" --wrap=threadClose" if args.profile_sim else "")
    original = original.replace("--nx-module-name", "--nx-module-name " + wrappers)
    specs.write_text(original)
    cc = sdk / "devkitA64/bin/aarch64-none-elf-gcc"
    artifact = "vera20k-client" if args.client else "vera20k-gpu" if args.gpu else "vera20k-core"
    graphics = ["-DVERA_WGPU"] if args.gpu or args.client else []
    if args.client:
        graphics.append("-DVERA_CLIENT")
    if args.profile_sim:
        graphics.append("-DVERA_PERF")
    architecture = ["-march=armv8-a+crc+crypto", "-mtune=cortex-a57", "-mtp=soft", "-fPIE"]
    cflags = [*architecture, *graphics, "-O2", "-g", "-Wall", "-Wextra", "-Werror", "-D__SWITCH__",
        "-ffunction-sections", "-fdata-sections", "-I", sdk / "libnx/include",
        "-I", sdk / "portlibs/switch/include"]
    runtime_objects = [work / "runtime.o"]
    run(cc, *cflags, "-c", "runtime.c", "-o", runtime_objects[0])
    if args.profile_sim:
        runtime_objects.append(work / "perf.o")
        run(cc, *cflags, "-c", "perf.c", "-o", runtime_objects[-1])
    if args.client:
        runtime_objects.append(work / "fps_report.o")
        run(cc, *cflags, "-c", "fps_report.c", "-o", runtime_objects[-1])
    if args.gpu or args.client:
        runtime_objects.append(work / "memory.o")
        run(cc, *cflags, "-c", "memory.c", "-o", runtime_objects[-1])
    elf = work / f"{artifact}.elf"
    if args.gpu or args.client:
        nvk = Path(os.environ.get(
            "VERA_SWITCH_NVK_LIB", WORKSPACE / "toolchains/switch/bevy-horizon-reference/nvk/lib"))
        if not (nvk / "libvulkan_bh.a").is_file():
            raise SystemExit(
                "Missing localized NVK archive; set VERA_SWITCH_NVK_LIB to the "
                "directory containing libvulkan_bh.a. See README.md."
            )
        libraries = ["-L", nvk, "-Wl,--start-group", "-lvulkan_bh", "-lexpat", "-lzstd", "-lz",
                     "-lnx", "-Wl,--end-group"]
    else:
        libraries = ["-Wl,--start-group", "-lEGL", "-lGLESv2", "-lglapi", "-ldrm_nouveau", "-lnx",
                     "-Wl,--end-group"]
    run(cc, *architecture, f"-specs={specs}", "-g", *runtime_objects,
        "target/aarch64-nintendo-horizon/release/libvera20k_switch.a",
        "-L", sdk / "libnx/lib", "-L", sdk / "portlibs/switch/lib",
        *libraries, "-lstdc++", "-lm", "-o", elf)
    from elftools.elf.elffile import ELFFile
    with elf.open("rb") as source:
        image = ELFFile(source)
        tls = next(segment for segment in image.iter_segments() if segment["p_type"] == "PT_TLS")
        alignment = tls["p_align"]
        if not alignment or tls["p_vaddr"] % alignment:
            raise SystemExit("Final ELF TLS is misaligned")
        print(f"Final ELF TLS verified: address={tls['p_vaddr']:#x} alignment={alignment}")
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    nacp = work / f"{artifact}.nacp"
    title = (
        CLIENT_TITLE if args.client
        else "VERA20k GPU Probe" if args.gpu
        else "VERA20k Core Probe"
    )
    title_id = CLIENT_TITLE_ID if args.client else GPU_TITLE_ID if args.gpu else TITLE_ID
    version = (CLIENT_VERSION + "-profile") if args.client and args.profile_sim else (
        CLIENT_VERSION if args.client else "0.1.0"
    )
    output_artifact = (
        "vera20-client-profile"
        if args.client and args.profile_sim
        else client_nsp_artifact(CLIENT_VERSION)
        if args.client
        else f"{artifact}-assets" if args.nsp_assets else None
    )
    run(sdk / "tools/bin/nacptool", "--create", title, "Homebrew", version, nacp,
        f"--titleid={title_id}")
    if args.client:
        configure_client_savedata(nacp, title_id)
    nro_artifact = "vera20k-client-profile" if args.profile_sim else artifact
    run(sdk / "tools/bin/elf2nro", elf, dist / f"{nro_artifact}.nro", f"--nacp={nacp}",
        f"--icon={icon_path(ROOT, sdk, artifact)}")
    if args.nsp:
        package(
            ROOT,
            sdk,
            elf,
            nacp,
            artifact=nro_artifact,
            title_id=title_id,
            romfs=Path(args.nsp_assets) if args.nsp_assets else None,
            output_artifact=output_artifact,
        )


if __name__ == "__main__":
    main()
