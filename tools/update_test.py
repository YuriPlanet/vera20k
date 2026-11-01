"""Update an installed VERA20k to a newer release through the game's own updater.

The Release workflow's update test runs this on Windows, macOS and Linux
(.github/workflows/release.yml); on a Mac with the game it also runs attended,
where a person answers the game's offer on the main menu. It publishes the
release with the real `.github/release/publish-r2.sh` (tools/release_fixture.py
stands in for GitHub and R2), serves that R2 layout on 127.0.0.1, installs the
release's own package, or with --installed an older release's, with an older
test build of the program in place of its own, and starts it. Each scenario
passes when the game's log shows:

- update: the release starting, with config.toml, saves and replays unchanged;
- manifest: a latest.json changed after signing refused, the program unchanged;
- installer: an installer changed after signing refused, the program unchanged;
- rekey: the release published after a key change, signed by the new key its
  manifest names, offered as a download from GitHub, the program unchanged.

It installs where players keep the game, under names of its own so a player's
copy is never touched: on Windows in a folder with a space in its path, as
under a user name with one; on macOS in /Applications, whose app bundles macOS
guards (App Management); on Linux in ~/Applications, mounted with FUSE as
players start it. A run removes what it put in those two folders. With
--quarantine, an attended Mac run marks its copy as a browser download that
Finder dragged into place, so macOS asks the person to allow the first start,
as it asks players.

    python -m tools.update_test key DIR
    python -m tools.update_test run --key DIR/signing-key.pem --release INSTALLER \\
        --version X.Y.Z --program OLD_PROGRAM [--installed OLD_INSTALLER] [--game FOLDER] \\
        [--attended [--quarantine]] [--rate BYTES_PER_SECOND] [--scenario manifest installer rekey update]
    python -m tools.update_test fixture DIR

`key` makes the signing key and prints its public half. The test build of the
program embeds this server and that key, and has a Cargo version below the
release's: the release's own commit built as 0.0.1, or the older release's
from its tag. Without retail game data there is no main menu to ask on, so a
CI build also updates without asking:

    VERA20K_COMMIT=<any> VERA20K_UPDATE_URL=http://127.0.0.1:47311/
    VERA20K_UPDATE_KEY=<public key> [VERA20K_UPDATE_UNATTENDED=1]

`fixture` writes the signed manifest src/app/updater/manifest.rs tests read.
Needs what the publisher needs (tools/release_fixture.py). The macOS and Linux
games keep their files in the player's data folder; a run there uses the one
that exists (or writes its config.toml) and changes only what the game does.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass, field
import functools
import hashlib
import http.server
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time

from tools.release_fixture import Publisher, public_key, signing_key

ROOT = Path(__file__).resolve().parents[1]
PORT = 47311
# Each installer's name in the manifest (src/app/updater/manifest.rs).
SUFFIXES = {"windows": "-windows-x64-setup.exe", "macos": "-macos-arm64.dmg", "macos-x64": "-macos-x64.dmg",
            "linux": "-linux-x64.AppImage"}
# The installer's stand-in game folder: tiny MIX and CSF files that pass its
# game-data check and hold both master archives the first launch looks for.
TEST_GAME = ROOT / ".github/release/windows/test-game"
# The installed game in the players' folders, beside what an unfinished update
# leaves there (src/app/updater/install.rs).
MAC_APP = Path("/Applications/VERA20K Update Test.app")
LINUX_IMAGE = Path.home() / "Applications/VERA20K Update Test.AppImage"
INSTALLED = {"macos": [MAC_APP, MAC_APP.with_name(".VERA20K-update")],
             "linux": [LINUX_IMAGE, LINUX_IMAGE.with_name(f".{LINUX_IMAGE.name}.update")]}
# The release page, as .github/release/notes.sh writes it: the offer lists its
# changes, more than its list shows at once.
BODY = """### Changes since v0.0.1

- This test release checks that VERA20K updates itself from the downloads site.
- Its offer lists each change from the release page, below the question.
- Long changes wrap onto further rows, indented under their first word.
- **Emphasis** and [links](https://github.com/YuriPlanet/vera20k) lose their `Markdown`.
- The list scrolls with its arrows, its thumb or the mouse wheel.
- Update downloads the release with progress, installs it and restarts VERA20K.

[All changes](https://github.com/YuriPlanet/vera20k)

You need your own copy of Yuri's Revenge.
"""


def platform() -> str:
    return {"win32": "windows", "darwin": "macos"}.get(sys.platform, "linux")


def installer() -> str:
    """This machine's installer in the manifest: Intel Macs have their own."""
    return "macos-x64" if platform() == "macos" and os.uname().machine == "x86_64" else platform()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as file:
        while chunk := file.read(1 << 20):
            digest.update(chunk)
    return digest.hexdigest()


def run(*command, **options) -> None:
    subprocess.run([str(part) for part in command], check=True, **options)


def publish(work: Path, key: Path, version: str, release: Path | None) -> Path:
    """The R2 layout publish-r2.sh writes for `version`, signed with `key`:
    `release` is this machine's installer, the others are stand-ins."""
    publisher = Publisher(work / "publish", public_key(key))
    assets = []
    for index, (name_platform, suffix) in enumerate(SUFFIXES.items()):
        name = f"vera20k-{version}{suffix}"
        path = publisher.root / "assets" / name
        if release and name_platform == installer():
            shutil.copyfile(release, path)
        else:
            path.write_bytes(f"{name} stand-in\n".encode())
        assets.append({"id": index + 1, "name": name, "size": path.stat().st_size,
                       "digest": "sha256:" + sha256(path), "state": "uploaded"})
    tag = f"v{version}"
    config = {"tag": tag, "commit": "a" * 40,
              "updater_version": (ROOT / ".github/release/updater-version").read_text(),
              "release": {"id": 1, "tag_name": tag, "draft": False, "prerelease": False, "assets": assets,
                          "body": BODY}}
    result = publisher.run(tag, config, key.read_text())
    if result.returncode:
        raise SystemExit(f"publish-r2.sh failed:\n{result.stdout}{result.stderr}")
    return publisher.root / "objects"


class Server(http.server.SimpleHTTPRequestHandler):
    """The R2 layout over HTTP, optionally at `rate` bytes a second so the
    progress box stays up long enough to look at."""
    # Keep-alive, as R2 serves. ureq reuses the check's connection after an
    # HTTP/1.0 reply too, which this server would already have closed.
    protocol_version = "HTTP/1.1"
    rate: float | None = None

    def copyfile(self, source, outputfile):
        while chunk := source.read(64 * 1024):
            outputfile.write(chunk)
            if self.rate:
                time.sleep(len(chunk) / self.rate)

    def log_message(self, format, *args):
        print(f"server: {format % args}", flush=True)


def serve(objects: Path, rate: float | None) -> http.server.ThreadingHTTPServer:
    handler = type("RateServer", (Server,), {"rate": rate})
    server = http.server.ThreadingHTTPServer(("127.0.0.1", PORT), functools.partial(handler, directory=str(objects)))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


@dataclass
class Game:
    """An installed game: the file an update replaces, its data folder (with
    config.toml and logs/ra2.log) and how to start and stop it."""
    program: Path
    data: Path
    start: list[str]
    stop: list[str]
    cwd: Path | None = None
    env: dict[str, str] = field(default_factory=dict)
    # Left behind only while an update is unfinished.
    staging: Path | None = None

    @property
    def log(self) -> Path:
        return self.data / "logs/ra2.log"

    def files(self) -> dict[str, str]:
        """What the player keeps: config.toml and their saves and replays."""
        kept = {"config.toml": sha256(self.data / "config.toml")}
        for folder in ("saves", "replays"):
            for path in sorted((self.data / folder).rglob("*")) if (self.data / folder).is_dir() else ():
                if path.is_file():
                    kept[path.relative_to(self.data).as_posix()] = sha256(path)
        return kept


def remove_installed() -> None:
    """What a run put in the players' folders."""
    for path in INSTALLED.get(platform(), []):
        try:
            if path.is_dir() and not path.is_symlink():
                shutil.rmtree(path)
            else:
                path.unlink(missing_ok=True)
        except OSError as error:
            print(f"cannot remove {path}: {error}", flush=True)


def install(work: Path, release: Path, program: Path, game_folder: Path, quarantine: bool) -> Game:
    """`release`'s package installed as a player would, with `program` in
    place of its own."""
    if platform() == "windows":
        folder = work / "Test Player/VERA"
        run(release, "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", f"/DIR={folder}",
            f"/GAMEDIR={game_folder}", f"/LOG={work / 'setup.log'}")
        shutil.copyfile(program, folder / "vera20k.exe")
        return Game(folder / "vera20k.exe", folder, [str(folder / "vera20k.exe")],
                    ["taskkill", "/F", "/IM", "vera20k.exe"], cwd=folder)
    data = (Path.home() / "Library/Application Support/VERA20k" if platform() == "macos"
            else Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share") / "VERA20k")
    if not (data / "config.toml").exists():
        # What the first launch writes once the player names the game folder.
        data.mkdir(parents=True, exist_ok=True)
        config = (ROOT / "config.toml.example").read_text()
        config = config.replace('ra2_dir = "C:/Westwood/RA2"', f'ra2_dir = "{game_folder.as_posix()}"')
        config = config.replace('# settings_dir = "."', 'settings_dir = "."')
        (data / "config.toml").write_text(config)
    if platform() == "macos":
        mount = work / "release-mount"
        mount.mkdir()
        run("hdiutil", "attach", "-nobrowse", "-readonly", "-mountpoint", mount, release)
        try:
            app = work / "VERA20K.app"
            run("ditto", mount / "VERA20K.app", app)
        finally:
            run("hdiutil", "detach", mount)
        shutil.copyfile(program, app / "Contents/MacOS/VERA20K")
        run("codesign", "--force", "--sign", "-", app)
        # Dragged into place, finished.
        run("ditto", app, MAC_APP)
        if quarantine:
            # What a browser marks its download with (0x0081), plus the
            # QTN_FLAG_DO_NOT_TRANSLOCATE (0x0100) that Finder's drag adds
            # (copyfile(3) COPYFILE_RUN_IN_PLACE). Without that bit macOS runs
            # even an approved copy from a read-only one (App Translocation),
            # which the game asks to be moved to Applications instead.
            run("xattr", "-r", "-w", "com.apple.quarantine", f"0181;{int(time.time()):x};Safari;", MAC_APP)
        return Game(MAC_APP / "Contents/MacOS/VERA20K", data, ["open", str(MAC_APP)],
                    ["pkill", "-f", str(MAC_APP)], staging=INSTALLED["macos"][1])
    # `release`'s own files around the test program, packed by this
    # checkout's script, which may differ from an older release's.
    release.chmod(0o755)
    extracted = work / "release-extract"
    extracted.mkdir()
    run(release, "--appimage-extract", cwd=extracted, stdout=subprocess.DEVNULL)
    package = work / "old-package"
    package.mkdir()
    # copy, not copyfile: appimage.sh keeps the program's executable bit.
    shutil.copy(program, package / "vera20k")
    for name in ("README.txt", "LICENSE-GPL", "LGPL-2.1-or-later.txt", "THIRD_PARTY_LICENSES.html"):
        shutil.copyfile(extracted / "squashfs-root/usr/share/doc/vera20k" / name, package / name)
    LINUX_IMAGE.parent.mkdir(parents=True, exist_ok=True)
    run("bash", ROOT / ".github/release/linux/appimage.sh", package, LINUX_IMAGE)
    return Game(LINUX_IMAGE, data, [str(LINUX_IMAGE)], ["pkill", "-x", "vera20k"])


def wait_for(game: Game, offset: int, outcomes: dict[str, str], timeout: float) -> tuple[str, str]:
    """The first outcome whose pattern the log shows after `offset`."""
    deadline = time.monotonic() + timeout
    text = ""
    while time.monotonic() < deadline:
        if game.log.exists():
            with open(game.log, "rb") as log:
                log.seek(offset)
                text = log.read().decode("utf-8", "replace")
            for outcome, pattern in outcomes.items():
                if match := re.search(pattern, text):
                    return outcome, match.group(0)
        time.sleep(0.5)
    raise SystemExit(f"no outcome within {timeout:.0f} s; the log since starting:\n{text[-4000:]}")


def play(game: Game, scenario: str, version: str, attended: bool) -> None:
    """Start the installed game and check what the updater did."""
    before = sha256(game.program)
    kept = game.files()
    offset = game.log.stat().st_size if game.log.exists() else 0
    started = time.monotonic()
    subprocess.Popen(game.start, cwd=game.cwd, env=dict(os.environ, **game.env))
    failed = r"Updater: (check failed|updating to \S+ failed|cannot \w+)[^\n]*"
    if scenario == "update":
        if attended:
            print("Answer Update on the main menu's offer.", flush=True)
        outcomes = {"updated": rf"VERA20k {re.escape(version)} starting", "failed": failed,
                    "not offered": r"Updater: no newer release"}
    elif scenario == "manifest":
        outcomes = {"refused": r"Updater: check failed:[^\n]*signature[^\n]*",
                    "found": r"Updater: VERA20k \S+ (is available|needs a download by hand|is signed by another key)",
                    "other failure": failed}
    elif scenario == "rekey":
        outcomes = {"pointed": rf"Updater: VERA20k {re.escape(version)} is signed by another key",
                    "found": r"Updater: VERA20k \S+ (is available|needs a download by hand)",
                    "other failure": failed}
    else:
        if attended:
            print("Answer Update on the main menu's offer; it fails with the game's message box.", flush=True)
        outcomes = {"refused": rf"Updater: updating to {re.escape(version)} failed:[^\n]*SHA-256[^\n]*",
                    "other failure": failed, "updated": rf"VERA20k {re.escape(version)} starting"}
    outcome, line = wait_for(game, offset, outcomes, 900 if attended else 300)
    print(f"{scenario}: {outcome} after {time.monotonic() - started:.0f} s: {line}", flush=True)
    expected = {"update": "updated", "rekey": "pointed"}.get(scenario, "refused")
    if outcome != expected:
        raise SystemExit(f"{scenario}: expected {expected}, not {outcome}")
    if attended and scenario == "rekey":
        input("Answer Later on the main menu's offer of a download from GitHub, then press Enter. ")
    if scenario != "update":
        # Long enough for an update that was not refused to have replaced it.
        time.sleep(10)
    subprocess.run(game.stop, capture_output=True)
    time.sleep(2)
    after = sha256(game.program)
    if scenario == "update":
        if after == before:
            raise SystemExit("update: the program file was not replaced")
        if game.staging and game.staging.exists():
            raise SystemExit(f"update: {game.staging} was left behind")
    elif after != before:
        raise SystemExit(f"{scenario}: the program changed although the update was refused")
    if game.files() != kept:
        raise SystemExit(f"{scenario}: the player's files changed: {kept} -> {game.files()}")
    print(f"{scenario}: passed; config.toml, saves and replays unchanged", flush=True)


def tamper(objects: Path, scenario: str, version: str) -> None:
    """Change what was signed, as a compromised download server could."""
    if scenario == "manifest":
        latest = objects / "latest.json"
        manifest = json.loads(latest.read_text())
        manifest["files"][installer()]["sha256"] = "0" * 64
        latest.write_text(json.dumps(manifest, sort_keys=True, indent=2) + "\n")
    elif scenario == "installer":
        path = objects / f"releases/v{version}/vera20k-{version}{SUFFIXES[installer()]}"
        data = bytearray(path.read_bytes())
        data[len(data) // 2] ^= 0xFF
        path.write_bytes(bytes(data))


def command_run(args) -> None:
    with tempfile.TemporaryDirectory(prefix="vera20k-update-test-", ignore_cleanup_errors=True) as temp:
        work = Path(temp)
        game_folder = args.game
        if game_folder is None:
            game_folder = work / "game"
            shutil.copytree(TEST_GAME, game_folder, ignore=shutil.ignore_patterns("README.md"))
        objects = publish(work, args.key, args.version, args.release)
        pristine = work / "published"
        shutil.copytree(objects, pristine)
        if "rekey" in args.scenario:
            new_key = work / "new-key"
            new_key.mkdir()
            signing_key(new_key)
            rekeyed = publish(work / "rekey", new_key / "signing-key.pem", args.version, args.release)
        print(f"published {sorted(p.relative_to(objects).as_posix() for p in objects.rglob('*') if p.is_file())}",
              flush=True)
        server = serve(objects, args.rate)
        # A run stopped early leaves its copy.
        remove_installed()
        game = None
        try:
            game = install(work, args.installed or args.release, args.program, game_folder, args.quarantine)
            if args.quarantine:
                print("macOS asks before the first start: allow it in System Settings, Privacy & Security.",
                      flush=True)
            for scenario in args.scenario:
                shutil.rmtree(objects)
                shutil.copytree(rekeyed if scenario == "rekey" else pristine, objects)
                tamper(objects, scenario, args.version)
                play(game, scenario, args.version, args.attended)
        finally:
            server.shutdown()
            if game:
                # A scenario that failed leaves the game running.
                subprocess.run(game.stop, capture_output=True)
                time.sleep(2)
            remove_installed()
    print(f"update test passed: {', '.join(args.scenario)}")


def command_fixture(directory: Path) -> None:
    """latest.json, its signature and the public key, for a release 1.2.3 of
    small stand-in installers."""
    with tempfile.TemporaryDirectory() as temp:
        key = Path(temp) / "key"
        key.mkdir()
        signing_key(key)
        objects = publish(Path(temp), key / "signing-key.pem", "1.2.3", None)
        directory.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(objects / "latest.json", directory / "manifest.json")
        shutil.copyfile(objects / "releases/v1.2.3/manifest.json.sig", directory / "manifest.json.sig")
        (directory / "update-key.pub").write_text(public_key(key / "signing-key.pem") + "\n")


def absolute(value: str) -> Path:
    """A command-line path made absolute, since the installers run in other folders."""
    return Path(value).resolve()


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    key = commands.add_parser("key", help="make a signing key and print its public half")
    key.add_argument("directory", type=Path)
    test = commands.add_parser("run", help="update an installed older build to the release")
    test.add_argument("--key", type=absolute, required=True, help="the signing key's PEM")
    test.add_argument("--release", type=absolute, required=True, help="this platform's release installer")
    test.add_argument("--version", required=True, help="the release's version")
    test.add_argument("--program", type=absolute, required=True, help="the older test build's program")
    test.add_argument("--installed", type=absolute,
                      help="the installer the older build came with; default: --release")
    test.add_argument("--game", type=absolute, help="a game folder; default: the installer's stand-in")
    test.add_argument("--attended", action="store_true", help="a person answers the offer")
    test.add_argument("--quarantine", action="store_true",
                      help="with --attended on macOS: mark the copy as a browser download")
    test.add_argument("--rate", type=float, help="serve at most this many bytes a second")
    test.add_argument("--scenario", nargs="+", choices=["manifest", "installer", "rekey", "update"],
                      default=["manifest", "installer", "rekey", "update"])
    fixture = commands.add_parser("fixture", help="write the Rust updater tests' signed manifest")
    fixture.add_argument("directory", type=Path)
    args = parser.parse_args(argv)
    if args.command == "run" and args.quarantine and not (args.attended and platform() == "macos"):
        parser.error("--quarantine needs --attended on macOS: a person allows the first start")
    if args.command == "key":
        args.directory.mkdir(parents=True, exist_ok=True)
        print(signing_key(args.directory)[1])
    elif args.command == "run":
        command_run(args)
    else:
        command_fixture(args.directory)


if __name__ == "__main__":
    main()
