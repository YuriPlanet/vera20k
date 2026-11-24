"""Run released clients' relay protocol code against the checkout's relay.

`services/relay-protocol/examples/relay_compat.rs` is built against the checkout
itself (a control that the check works) and against the protocol crate of every
build the lobby admits: each stable release from `OLDEST_ADMITTED` on and the
newest dev build, once per distinct protocol crate. Each client then plays one
short loopback match against the relay built from the checkout. The match
carries frame ends, a batch and two departures, one of them the departure the
relay writes when it removes a silent player. A client that cannot read one
stalls, as a released game would, and one that cannot read a Status fails.

It runs only in GitHub Actions (`.github/workflows/relay-compat.yml`), on PRs
that touch the relay and on demand with other `--client` tags or commits: it
builds several trees with plain `cargo`, which a hosted job may do because it
has no cooperating worktrees, and which a local checkout must leave to
`tools/cargo_run.py`. Release tags come from `gh`.

A release the checkout's example no longer builds against, because the protocol
API changed, plays the example it tracks instead, with a warning.

A change that breaks admitted releases on purpose raises `OLDEST_ADMITTED` past
them, so the lobby refuses them at sign-in and this check no longer plays them.
Its PR carries the `relay-compat-break` label, and `--allow-break` reports the
client failures that remain, such as an earlier dev build of the new floor's
version, as warnings.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

from tools.cargo_run import invoke

ROOT = Path(__file__).resolve().parents[1]
REPO = "YuriPlanet/vera20k"
EXAMPLE = Path("services/relay-protocol/examples/relay_compat.rs")
# The oldest version the lobby admits: lobby/service.js OLDEST_ADMITTED_VERSION
# in the private services repository, which must equal it.
OLDEST_ADMITTED = "0.2.7"
STABLE_TAG = re.compile(r"^v\d+\.\d+\.\d+$")
DEV_TAG = re.compile(r"^v\d+\.\d+\.\d+-\d{8}-\d{6}$")
FIXTURE_KEY = "49" * 32  # Public loopback key; the client signs with the same bytes.
CLIENT_TIMEOUT = 200


def git(*args: str, cwd: Path = ROOT) -> str:
    return subprocess.check_output(["git", "-C", str(cwd), *args], text=True).strip()


def version(tag: str) -> tuple[int, int, int]:
    """MAJOR.MINOR.PATCH of a version or release tag: what a game names at sign-in."""
    major, minor, patch = re.match(r"v?(\d+)\.(\d+)\.(\d+)", tag).groups()
    return int(major), int(minor), int(patch)


def supported_clients(releases: list[dict]) -> list[str]:
    """Every stable release the lobby admits, oldest first, then the newest dev build if it admits
    that too, from `gh release list` rows."""
    floor = version(OLDEST_ADMITTED)
    published = [row for row in releases if not row.get("isDraft")]
    stable = sorted((row["tagName"] for row in published
                     if not row["isPrerelease"] and STABLE_TAG.match(row["tagName"])
                     and version(row["tagName"]) >= floor), key=version)
    if stable and stable[0] != f"v{OLDEST_ADMITTED}":
        raise SystemExit(f"v{OLDEST_ADMITTED}, the oldest version the lobby admits, is not among the listed releases "
                         "though newer ones are: raise released_tags' --limit or correct OLDEST_ADMITTED")
    dev = [row for row in published if row["isPrerelease"] and DEV_TAG.match(row["tagName"])]
    newest_dev = max(dev, key=lambda row: row["createdAt"])["tagName"] if dev else None
    tags = stable + ([newest_dev] if newest_dev and version(newest_dev) >= floor else [])
    if not tags:
        print(f"::warning::No published build is at or above v{OLDEST_ADMITTED}, the oldest version the lobby "
              "admits; only the checkout's own client plays", flush=True)
    return tags


def released_tags() -> list[str]:
    rows = subprocess.check_output(
        ["gh", "release", "list", "--repo", REPO, "--limit", "100",
         "--json", "tagName,isPrerelease,isDraft,createdAt"], text=True)
    return supported_clients(json.loads(rows))


def commit_of(ref: str) -> str:
    try:
        return git("rev-parse", "--verify", f"{ref}^{{commit}}")
    except subprocess.CalledProcessError:
        # A shallow CI checkout holds neither tags nor history; fetch just this
        # tag, or a full commit hash.
        commit = re.fullmatch(r"[0-9a-f]{40}", ref)
        git("fetch", "--quiet", "--depth=1", "--no-tags", "origin", ref if commit else f"refs/tags/{ref}:refs/tags/{ref}")
        return git("rev-parse", "--verify", f"{ref}^{{commit}}")


def use_checkout_example(tree: Path) -> bytes | None:
    """Puts the checkout's example in TREE; returns the copy TREE's release tracks, if any (v0.2.4 on)."""
    example = tree / EXAMPLE
    original = example.read_bytes() if example.exists() else None
    example.parent.mkdir(exist_ok=True)
    shutil.copyfile(ROOT / EXAMPLE, example)
    return original


def restore_example(tree: Path, original: bytes | None) -> None:
    """Leaves TREE's example as its release has it, so `git worktree remove` finds nothing changed."""
    if original is None:
        (tree / EXAMPLE).unlink(missing_ok=True)
    else:
        (tree / EXAMPLE).write_bytes(original)


class BuildFailed(SystemExit):
    """A target that did not build; uncaught, it ends the run like any SystemExit."""


def build(tree: Path, package: str, target: list[str], output: Path) -> Path:
    """Builds one target and copies its executable, named by Cargo's artifact report, to OUTPUT.

    Every tree shares CARGO_TARGET_DIR, so the next build overwrites the original.
    """
    executables = []

    def artifact(line: str) -> None:
        try:
            message = json.loads(line)
        except ValueError:
            return
        if message.get("reason") == "compiler-artifact" and message.get("executable"):
            executables.append(message["executable"])

    code = invoke(["build", "--locked", "--message-format=json-render-diagnostics", "-p", package, *target],
                  cwd=tree, isolated_ci=True, on_line=artifact)
    if code or len(executables) != 1:
        raise BuildFailed(f"Building {package} {' '.join(target)} in {tree} failed")
    shutil.copy2(executables[0], output)
    return output


def released_client(tree: Path, original: bytes | None, output: Path) -> Path:
    """Builds TREE's client from the checkout's example in it.

    A release the checkout's example does not build against builds the copy it
    tracks (ORIGINAL), which reads records as that release does.
    """
    target = ["--example", "relay_compat"]
    try:
        return build(tree, "vera20k-relay-protocol", target, output)
    except BuildFailed:
        if original is None:
            raise
    print(f"::warning::The checkout's relay_compat example does not build against {tree.name}; "
          "playing the example that release tracks", flush=True)
    restore_example(tree, original)
    return build(tree, "vera20k-relay-protocol", target, output)


def start_relay(relay: Path, state: Path) -> tuple[subprocess.Popen, str]:
    env = {**os.environ, "VERA_RELAY_BIND": "127.0.0.1:0", "VERA_RELAY_TICKET_KEY": FIXTURE_KEY,
           "VERA_RELAY_STATE_DIR": str(state)}
    for name in ("STATE_DIRECTORY", "CREDENTIALS_DIRECTORY", "VERA_RELAY_TICKET_KEY_FILE"):
        env.pop(name, None)
    server = subprocess.Popen([str(relay)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    assert server.stderr is not None
    for line in server.stderr:
        match = re.search(r"listening on (127\.0\.0\.1:\d+)", line)
        if match:
            return server, match.group(1)
    raise SystemExit("The relay exited before it listened")


def play(client: Path, address: str) -> dict:
    try:
        run = subprocess.run([str(client), address], capture_output=True, text=True, timeout=CLIENT_TIMEOUT)
    except subprocess.TimeoutExpired:
        return {"ok": False, "stage": "client_timeout"}
    try:
        return json.loads(run.stdout.strip().splitlines()[-1])
    except (ValueError, IndexError):
        return {"ok": False, "stage": f"client_exit_{run.returncode}", "stderr": run.stderr[-400:]}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--client", action="append",
                        help="Release tag or full commit hash (default: every build the lobby admits)")
    parser.add_argument("--allow-break", action="store_true", help="Report client failures as warnings")
    args = parser.parse_args(argv)
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise SystemExit("Run this through the Relay compatibility workflow: "
                         "gh workflow run relay-compat.yml --ref <branch> [-f clients='<tags>']")
    refs = args.client or released_tags()
    # Builds whose protocol crates are the same play once, as the first one's commit.
    by_crate: dict[str, tuple[str, list[str]]] = {}
    for ref in refs:
        commit = commit_of(ref)
        by_crate.setdefault(git("rev-parse", f"{commit}:services/relay-protocol"), (commit, []))[1].append(ref)
    with tempfile.TemporaryDirectory(prefix="relay-compat-") as scratch:
        scratch = Path(scratch)
        relay = build(ROOT, "vera20k-relay", ["--bin", "vera20k-relay"], scratch / "relay")
        clients = {"checkout": build(ROOT, "vera20k-relay-protocol", ["--example", "relay_compat"],
                                     scratch / "checkout-client")}
        trees: dict[Path, bytes | None] = {}
        try:
            for commit, labels in by_crate.values():
                tree = scratch / commit[:12]
                git("worktree", "add", "--quiet", "--detach", str(tree), commit)
                trees[tree] = None  # removed below even if the copy fails
                trees[tree] = use_checkout_example(tree)
                clients[" = ".join(labels)] = released_client(tree, trees[tree], scratch / f"{commit[:12]}-client")
            server, address = start_relay(relay, scratch / "relay-state")
            try:
                print(f"Playing {len(clients)} clients against the checkout's relay at {address}; "
                      "a compatible one finishes in about a minute.", flush=True)
                with concurrent.futures.ThreadPoolExecutor(len(clients)) as pool:
                    results = dict(zip(clients, pool.map(lambda client: play(client, address), clients.values())))
            finally:
                server.terminate()
                server.wait(timeout=10)
        finally:
            for tree, original in trees.items():
                restore_example(tree, original)
                git("worktree", "remove", str(tree))
    for label, result in results.items():
        print(f"{'ok  ' if result['ok'] else 'FAIL'} {label}: {json.dumps(result)}")
        if result.get("undecodable"):
            print(f"::warning::{label} could not decode {result['undecodable']} relay datagrams but finished its match")
    if not results["checkout"]["ok"]:
        print("::error::The checkout's own client failed, so the check itself is broken")
        return 1
    failed = [label for label, result in results.items() if not result["ok"]]
    for label in failed:
        kind = "warning" if args.allow_break else "error"
        print(f"::{kind}::Released client {label} cannot play against this relay")
    return 0 if args.allow_break or not failed else 1


if __name__ == "__main__":
    sys.exit(main())
