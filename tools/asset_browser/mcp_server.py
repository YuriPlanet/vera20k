# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "mcp>=1.2.0,<2",
# ]
# ///
"""
asset_browser MCP Server — exposes the compiled `asset` CLI as MCP tools.

This server has no importable library: the asset browser is Rust, living in
`src/asset_tools/` behind `src/bin/asset.rs`. Every tool therefore shells out to
the compiled binary and returns its stdout, which is JSON by contract — the CLI
prints JSON even when it fails, so a caller parsing stdout always gets JSON.

Because the child process is the whole implementation, every subprocess failure
is wrapped: a missing binary, a timeout, or a crash must come back as an
actionable string, never as a traceback and never as something a caller could
mistake for an empty result.
"""

from __future__ import annotations

import json
import logging
import os
import subprocess
import sys
from pathlib import Path
from typing import Literal

# Self-locating: the server is launched from an unpredictable working directory
# (Claude Code uses a repo-relative command, Codex an absolute one), so nothing
# here may use Path.cwd().
_SERVER_DIR = Path(__file__).resolve().parent
# Repo root is two parents up from this file's directory.
WORKSPACE = _SERVER_DIR.parents[1]

# Reconfigure stdout to UTF-8; CSF strings and archive names contain non-ASCII.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

from mcp.server.fastmcp import FastMCP

# stdout belongs to the stdio transport; one stray print corrupts the protocol.
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
    stream=sys.stderr,
)
logger = logging.getLogger("asset-browser-mcp")

mcp = FastMCP("asset-browser")

# --- Binary resolution -------------------------------------------------------

_BINARY_NAME = "asset.exe" if os.name == "nt" else "asset"
RELEASE_BINARY = WORKSPACE / "target" / "release" / _BINARY_NAME
DEBUG_BINARY = WORKSPACE / "target" / "debug" / _BINARY_NAME
BUILD_COMMAND = "cargo build --release -p vera20k --bin asset"

# --- Timeouts ----------------------------------------------------------------
#
# Every call pays a few seconds to mount ~57 archives before it does any work,
# so even the cheap verbs are not instant. The corpus-wide verbs walk every
# mounted archive and sniff every entry, which is a different order of cost.

TIMEOUT_QUICK_S = 120.0
"""Single-asset verbs: one archive lookup plus one parse."""

TIMEOUT_SWEEP_S = 300.0
"""Verbs that build the full INI-derived name dictionary or sweep archives."""

TIMEOUT_CORPUS_S = 900.0
"""`scan` and `parse-check`: every entry in every mounted archive."""

# Cap on how much child stderr is echoed back, so a chatty log cannot bury the
# payload.
_STDERR_TAIL_CHARS = 800


def _resolve_binary() -> tuple[Path | None, list[str]]:
    """Find the `asset` binary, preferring release.

    Resolved per call rather than cached at import, so a build that lands while
    the server is running is picked up without a restart. Returns the path plus
    any notes to attach to the result.
    """
    if RELEASE_BINARY.is_file():
        return RELEASE_BINARY, []
    if DEBUG_BINARY.is_file():
        return DEBUG_BINARY, [
            f"Ran the DEBUG build at {DEBUG_BINARY} because no release binary exists. "
            f"It is many times slower over the ~8000-entry retail corpus, and "
            f"asset_scan/asset_parse_check may well time out. Build the fast one with: "
            f"{BUILD_COMMAND}"
        ]
    return None, []


def _missing_binary_hint() -> str:
    """Actionable text for the not-built case. Never raised, always returned."""
    return (
        "The `asset` binary is not built, so no asset_* tool can run yet.\n"
        f"Looked for:\n"
        f"  {RELEASE_BINARY}   (preferred)\n"
        f"  {DEBUG_BINARY}\n"
        "Build it with:\n"
        f"  {BUILD_COMMAND}\n"
        f"run from {WORKSPACE}. Use the release profile — a debug build of this tool is "
        "many times slower over the ~8000-entry retail corpus."
    )


# --- Argument assembly -------------------------------------------------------


def _flag(name: str, value: object | None) -> list[str]:
    """One `--name value` pair, or nothing when the caller left it unset."""
    if value is None:
        return []
    return [name, str(value)]


def _switch(name: str, enabled: bool) -> list[str]:
    """One bare `--name`, or nothing."""
    return [name] if enabled else []


def _tail(text: str) -> str:
    """Last few hundred characters of child stderr, for failure reports."""
    stripped = (text or "").strip()
    if len(stripped) <= _STDERR_TAIL_CHARS:
        return stripped
    return "..." + stripped[-_STDERR_TAIL_CHARS:]


def _decorate(stdout: str, notes: list[str]) -> str:
    """Attach MCP-side notes without breaking the JSON contract.

    The CLI's stdout is the payload; notes are injected under `_mcp_notes` so a
    caller that json.loads() the result still gets valid JSON. `archives` prints
    a bare array, so that case is wrapped under `rows` rather than mangled.
    """
    if not notes:
        return stdout
    try:
        payload = json.loads(stdout)
    except (ValueError, TypeError):
        # Not JSON (should not happen, but a crashed child could print anything).
        return stdout + "\n\n" + "\n".join(f"NOTE: {note}" for note in notes)
    if isinstance(payload, dict):
        payload["_mcp_notes"] = notes
        return json.dumps(payload, indent=2)
    return json.dumps({"_mcp_notes": notes, "rows": payload}, indent=2)


def _invoke(
    verb_args: list[str],
    *,
    ra2_dir: str | None = None,
    all_mixes: bool = False,
    timeout_s: float = TIMEOUT_QUICK_S,
    slow_advice: str = "Narrow the request and try again.",
) -> str:
    """Run one `asset` invocation and return its stdout.

    `verb_args` starts with the verb and its positional target: the CLI's parser
    reads the verb as the first word, so global flags have to follow it, not
    precede it.

    Never raises. A missing binary, a timeout, a spawn failure, or a crash all
    return text a caller can act on.
    """
    binary, notes = _resolve_binary()
    if binary is None:
        return _missing_binary_hint()

    argv = [str(binary), *verb_args]
    argv += _flag("--ra2-dir", ra2_dir)
    argv += _switch("--all-mixes", all_mixes)

    logger.info("asset %s (timeout %.0fs)", " ".join(verb_args), timeout_s)
    try:
        proc = subprocess.run(
            argv,
            # The CLI resolves config.toml relative to its working directory, and
            # writes PNG/WAV output under a relative `target/asset` root. Pinning
            # the workspace makes both behave as if run from the repo, and makes
            # the absolute paths it reports land inside the repo's target/.
            cwd=str(WORKSPACE),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_s,
        )
    except subprocess.TimeoutExpired:
        return (
            f"`asset {' '.join(verb_args)}` exceeded the {timeout_s:.0f}s MCP timeout and "
            f"was killed, so it produced no output. {slow_advice}"
        )
    except OSError as err:
        return (
            f"Could not run {binary}: {err}\n"
            f"Rebuild it with: {BUILD_COMMAND}  (from {WORKSPACE})"
        )

    stderr_tail = _tail(proc.stderr)

    if proc.returncode != 0:
        # Exit 1 is a failed verb, exit 2 a bad command line. Both still print a
        # JSON {"error":..., "hint":...} on stdout, which is more useful to the
        # caller than anything this layer could synthesise.
        if proc.stdout.strip():
            return _decorate(proc.stdout.strip(), notes)
        return json.dumps(
            {
                "error": (
                    f"`asset {' '.join(verb_args)}` exited {proc.returncode} without "
                    f"printing JSON"
                ),
                "hint": (
                    "This is a crash, not an empty result. Check the stderr tail; "
                    f"rebuild with {BUILD_COMMAND} if the binary is stale."
                ),
                "stderr_tail": stderr_tail,
            },
            indent=2,
        )

    if not proc.stdout.strip():
        return json.dumps(
            {
                "error": f"`asset {' '.join(verb_args)}` exited 0 but printed nothing",
                "hint": "Every verb prints JSON on success; treat this as a tool bug.",
                "stderr_tail": stderr_tail,
            },
            indent=2,
        )

    # The CLI logs at warn level by default, so anything on stderr after a
    # successful run is a real warning worth surfacing.
    if stderr_tail:
        notes = [*notes, f"asset logged to stderr: {stderr_tail}"]

    return _decorate(proc.stdout.strip(), notes)


# --- Tools -------------------------------------------------------------------


@mcp.tool()
def asset_find(
    name: str,
    winner_only: bool = False,
    ra2_dir: str | None = None,
    all_mixes: bool = False,
) -> str:
    """Locate one asset by filename: which archive wins, and what shadows it.

    Use when you have a name and need to know whether it exists and where the
    game would actually read it from. Reports the winning archive, loose-file
    overrides, shadowed copies in later archives, and — separately — hits in
    catalogued nested archives that normal name lookup cannot reach, so a
    present asset is never reported as missing. To list an archive's contents
    use asset_ls; to see parsed structure use asset_info; to search the whole
    corpus by format or field rather than by name use asset_scan.

    Args:
        name: Filename as the game would request it, e.g. "POWERP.SHP".
        winner_only: Skip the shadow/catalogue sweep and report only the archive
            normal lookup resolves to. Default False (the CLI's `--all`, which
            sweeps every mounted archive).
        ra2_dir: Retail install root. Omit to let the CLI resolve it from
            $RA2_DIR then config.toml.
        all_mixes: Also mount archives the game's startup path skips. Tooling
            only — hits found this way are NOT what the game would resolve.
            Default False.
    """
    args = ["find", name, "--winner-only" if winner_only else "--all"]
    return _invoke(
        args,
        ra2_dir=ra2_dir,
        all_mixes=all_mixes,
        timeout_s=TIMEOUT_SWEEP_S,
        slow_advice="Pass winner_only=True to skip the corpus-wide shadow sweep.",
    )


@mcp.tool()
def asset_ls(
    archive: str,
    filter: str | None = None,
    format: str | None = None,
    sort: Literal["index", "name", "size", "hash"] = "index",
    limit: int = 100,
    offset: int = 0,
    ra2_dir: str | None = None,
    all_mixes: bool = False,
) -> str:
    """Paged listing of one archive's entries, with hashes reversed to names.

    Use when you know the archive and want to see what is inside it. This is the
    one verb that reverse-looks-up every row, so it builds the full INI-derived
    name dictionary and is slower than the single-asset verbs; the report says
    which dictionary backed it (`name_db`) and how many rows it could name. To
    list the archives themselves use asset_archives; to find one name across all
    of them use asset_find; to sweep every archive at once use asset_scan.

    Args:
        archive: Archive name as asset_archives reports it, e.g. "ra2md.mix".
        filter: Case-insensitive substring match on the resolved name.
        format: Keep only this sniffed format tag: shp, tmp, vxl, hva, pal, vpl,
            pcx, aud, csf, fnt, mix, xcc, bik, vqa, text, tiny, unknown.
        sort: "index" (default, archive order) | "name" | "size" | "hash".
        limit: Page size. Default 100, matching the CLI.
        offset: Rows to skip. Default 0.
        ra2_dir: Retail install root. Omit to let the CLI resolve it.
        all_mixes: Also mount archives the game's startup path skips. Default
            False.
    """
    args = ["ls", archive]
    args += _flag("--filter", filter)
    args += _flag("--format", format)
    args += _flag("--sort", sort)
    args += _flag("--limit", limit)
    args += _flag("--offset", offset)
    return _invoke(
        args,
        ra2_dir=ra2_dir,
        all_mixes=all_mixes,
        timeout_s=TIMEOUT_SWEEP_S,
        slow_advice="Lower limit=, or narrow with filter= / format=.",
    )


@mcp.tool()
def asset_info(
    name: str,
    frame: int = 0,
    ascii: bool = False,
    limit: int = 64,
    ra2_dir: str | None = None,
    all_mixes: bool = False,
) -> str:
    """Parsed structure of one asset without rendering it.

    Returns SHP frame tables (position, size, compression, transparent-pixel
    counts), TMP tile fields, VXL limbs plus the paired .hva, palette contents,
    and CSF/AUD/PCX/FNT/VPL headers. Use this first when the question is about
    geometry, frame counts, or file structure — it is much cheaper than a render
    and does not depend on guessing a palette. To actually look at the art use
    asset_render; for raw bytes use asset_extract; for which palette applies use
    asset_palette_for.

    Args:
        name: Filename, e.g. "POWERP.SHP" or "unittem.pal".
        frame: Frame selected by ascii=True. Default 0.
        ascii: Emit a palette-index grid for the selected frame — exact and free
            where a render is neither. Frames up to 4096 pixels only. Default
            False.
        limit: Max frames/tiles listed. Default 64, matching the CLI.
        ra2_dir: Retail install root. Omit to let the CLI resolve it.
        all_mixes: Also mount archives the game's startup path skips. Default
            False.
    """
    args = ["info", name]
    args += _flag("--frame", frame)
    args += _switch("--ascii", ascii)
    args += _flag("--limit", limit)
    return _invoke(
        args,
        ra2_dir=ra2_dir,
        all_mixes=all_mixes,
        timeout_s=TIMEOUT_QUICK_S,
        slow_advice="Lower limit=, or drop ascii=True.",
    )


@mcp.tool()
def asset_render(
    name: str,
    frame: int | None = None,
    palette: str | None = None,
    house: int | None = None,
    crop: bool = False,
    scale: int | None = None,
    limit: int = 64,
    out: str | None = None,
    layout: Literal["grid", "isometric"] = "grid",
    facings: list[int] | None = None,
    vpl: str | None = None,
    transparent_index: int | None = None,
    ra2_dir: str | None = None,
    all_mixes: bool = False,
) -> str:
    """Render an asset to PNG files on disk and return their paths. READ the paths.

    THIS TOOL WRITES IMAGE FILES AND RETURNS PATHS, NOT PIXELS. The JSON it
    returns lists absolute paths under `outputs.frames`, `outputs.sheet` and
    `outputs.index`. Reporting that JSON is not looking at the art — after
    calling this you MUST open those paths with your file-reading tool to
    actually see the sprite. That round trip is the entire point of the tool.

    THE PALETTE IS INFERRED, NOT KNOWN. The report's `palette.reason` says how it
    was picked and `palette.confidence` says how much to trust it; only
    "declared" means art.ini named it outright, everything else is a heuristic.
    A render that looks plausible is NOT proof the palette is right — RA2 art
    frequently renders as recognisable shapes in the wrong colours. When colours
    look off, or before you conclude anything from colour, call
    asset_palette_for to see the whole inference chain, then re-render with an
    explicit palette=.

    Handles SHP, TMP, PCX, PAL and VXL. For structure without pixels use
    asset_info (cheaper, exact); for raw bytes use asset_extract.

    Args:
        name: Filename, e.g. "POWERP.SHP", "GAPOWR.VXL", "isotem.pal".
        frame: Render one frame (or one TMP tile). Default None = all, up to
            limit.
        palette: Force a palette, e.g. "sidebar.pal". Default: inferred.
        house: Apply the rules [Colors] scheme N to the [16,32) remap band, i.e.
            render in a player's colour. Default: no remap.
        crop: Draw the bare frame sub-rect instead of the full file canvas.
            Default False — the default keeps frame_x/frame_y placement visible,
            which is what you want when checking alignment.
        scale: Integer upscale. Default: fit the long edge into 256-1024. PNG
            dimensions must be divided by the reported `scale` to get real ones.
        limit: Max frames rendered. Default 64, matching the CLI.
        out: Output root directory. Default: the repo's target/asset (gitignored,
            so nothing rendered can be committed by accident).
        layout: TMP only. "grid" (default) lays tiles out labelled; "isometric"
            composes the template as it appears in game.
        facings: VXL only. Facing bytes to render, 0-255 (0x00 N, 0x40 E,
            0x80 S, 0xC0 W). Default: the 8 compass facings.
        vpl: VXL only. Voxel lighting table. Default "voxels.vpl".
        transparent_index: PCX only. Palette index to treat as transparent.
        ra2_dir: Retail install root. Omit to let the CLI resolve it.
        all_mixes: Also mount archives the game's startup path skips. Default
            False.
    """
    args = ["render", name]
    args += _flag("--frame", frame)
    args += _flag("--palette", palette)
    args += _flag("--house", house)
    args += _switch("--crop", crop)
    args += _flag("--scale", scale)
    args += _flag("--limit", limit)
    args += _flag("--out", out)
    args += _switch("--isometric", layout == "isometric")
    args += _switch("--grid", layout == "grid")
    args += _flag("--vpl", vpl)
    args += _flag("--transparent-index", transparent_index)
    for facing in facings or []:
        args += _flag("--facing", facing)
    return _invoke(
        args,
        ra2_dir=ra2_dir,
        all_mixes=all_mixes,
        timeout_s=TIMEOUT_SWEEP_S,
        slow_advice=(
            "Lower limit=, pick a single frame=, or reduce scale=; a large VXL "
            "facing sweep is the usual cause."
        ),
    )


@mcp.tool()
def asset_palette_for(
    name: str,
    palette: str | None = None,
    ra2_dir: str | None = None,
    all_mixes: bool = False,
) -> str:
    """Which palette applies to an asset, and the full reasoning chain behind it.

    Use when a render's colours look wrong, or before trusting anything a render
    implies about colour. Returns the chosen palette with its reason, alpha
    policy and confidence, plus the rejected candidates and why each was
    proposed — so you can see whether the choice was declared in art.ini or
    merely guessed from the filename. asset_render already reports its own
    choice; reach for this tool when you need to challenge it. To map a rules
    type id to its art files use asset_art_for.

    Args:
        name: Asset filename the palette is being chosen for, e.g. "POWERP.SHP".
        palette: Test one specific palette against the inference chain instead of
            letting it pick.
        ra2_dir: Retail install root. Omit to let the CLI resolve it.
        all_mixes: Also mount archives the game's startup path skips. Default
            False.
    """
    args = ["palette-for", name]
    args += _flag("--palette", palette)
    return _invoke(
        args,
        ra2_dir=ra2_dir,
        all_mixes=all_mixes,
        timeout_s=TIMEOUT_QUICK_S,
    )


@mcp.tool()
def asset_archives(
    ra2_dir: str | None = None,
    all_mixes: bool = False,
) -> str:
    """Every mounted archive, with entry counts and lookup reachability marked.

    Start here when you do not yet know which archive to list. The
    `name_lookup_reachable` field matters: a large share of mounted archives are
    catalogued nested ones that name-based lookup cannot reach, so an asset can
    exist in the corpus and still not resolve by name. To list one archive's
    entries use asset_ls; to find one name use asset_find.

    Args:
        ra2_dir: Retail install root. Omit to let the CLI resolve it.
        all_mixes: Also mount archives the game's startup path skips, which makes
            this listing longer than what the game itself would mount. Default
            False.
    """
    return _invoke(
        ["archives"],
        ra2_dir=ra2_dir,
        all_mixes=all_mixes,
        timeout_s=TIMEOUT_QUICK_S,
    )


@mcp.tool()
def asset_extract(
    name: str,
    out: str | None = None,
    ra2_dir: str | None = None,
    all_mixes: bool = False,
) -> str:
    """Write one asset's raw bytes to disk and return the path and provenance.

    Use when a parser or a viewer outside this toolset needs the real file, or
    when you want to hexdump something the browser reports as `unknown`. For
    parsed structure use asset_info, for a viewable PNG use asset_render, and for
    audio (which lives in bag files, not archives) use asset_sound.

    Args:
        name: Filename, e.g. "POWERP.SHP".
        out: Output root directory. Default: the repo's target/asset, which is
            gitignored.
        ra2_dir: Retail install root. Omit to let the CLI resolve it.
        all_mixes: Also mount archives the game's startup path skips. Default
            False.
    """
    args = ["extract", name]
    args += _flag("--out", out)
    return _invoke(
        args,
        ra2_dir=ra2_dir,
        all_mixes=all_mixes,
        timeout_s=TIMEOUT_QUICK_S,
    )


@mcp.tool()
def asset_csf(
    mode: Literal["get", "grep"],
    query: str,
    source: str | None = None,
    raw: bool = False,
    limit: int = 50,
    offset: int = 0,
    ra2_dir: str | None = None,
    all_mixes: bool = False,
) -> str:
    """Read the CSF string table: one key by name, or a substring search.

    Use for any question about the text the game shows — unit names, sidebar
    labels, EVA lines, error strings. Values are returned after the parser's
    load-time normalisation, which is what the game displays; retail carries
    hundreds of strings that normalisation changes, so pass raw=True when the
    stored bytes matter. For the table's own header and entry count, call
    asset_info on the .csf file instead.

    Args:
        mode: "get" for one exact key (case-insensitive, matched in full),
            "grep" to search keys and values by substring.
        query: The key for mode="get", the search text for mode="grep".
        source: Which .csf to read. Default: ra2md.csf, then ra2.csf.
        raw: Also report the stored text before normalisation. Default False.
        limit: Max entries returned by mode="grep". Default 50, matching the CLI.
        offset: Entries to skip in mode="grep". Default 0.
        ra2_dir: Retail install root. Omit to let the CLI resolve it.
        all_mixes: Also mount archives the game's startup path skips. Default
            False.
    """
    args = ["csf-get" if mode == "get" else "csf-grep", query]
    args += _flag("--source", source)
    args += _switch("--raw", raw)
    if mode == "grep":
        args += _flag("--limit", limit)
        args += _flag("--offset", offset)
    return _invoke(
        args,
        ra2_dir=ra2_dir,
        all_mixes=all_mixes,
        timeout_s=TIMEOUT_QUICK_S,
        slow_advice="Lower limit=, or use mode='get' with an exact key.",
    )


@mcp.tool()
def asset_sound(
    mode: Literal["one", "list"],
    name: str | None = None,
    bag: str | None = None,
    prefix: str | None = None,
    wav: bool = False,
    limit: int = 100,
    offset: int = 0,
    out: str | None = None,
    ra2_dir: str | None = None,
    all_mixes: bool = False,
) -> str:
    """Read the audio bag: one entry's header (optionally decoded to .wav), or a listing.

    Audio lives in .idx/.bag pairs rather than in the mix archives, so this is
    the only way to reach it — asset_find and asset_ls will not see bag entries.
    mode="list" is the discovery step; mode="one" with wav=True writes a playable
    file and returns its path. For .aud entries that do live inside an archive,
    asset_info reports the same header fields.

    Args:
        mode: "one" to fetch a single entry by name; "list" to page the bag index.
        name: Required for mode="one" — the sound entry name. Ignored by "list".
        bag: Bag pair to open, without extension. Default: the standard bag.
        prefix: mode="list" only — keep entries whose name starts with this.
        wav: mode="one" only — decode the entry and write a .wav, whose path
            comes back in the entry's `wav` field. Read or play that path to hear
            it; the JSON alone only describes the header. Default False.
        limit: Page size. Default 100, matching the CLI.
        offset: Entries to skip. Default 0.
        out: Output root for a written .wav. Default: the repo's target/asset.
        ra2_dir: Retail install root. Omit to let the CLI resolve it.
        all_mixes: Also mount archives the game's startup path skips. Default
            False.
    """
    if mode == "one":
        if not name:
            return (
                "asset_sound(mode='one') needs name=<sound entry>. "
                "Call asset_sound(mode='list', prefix=...) first to discover entry names."
            )
        args = ["sound", name]
        args += _switch("--wav", wav)
        args += _flag("--out", out)
    else:
        if name:
            return (
                "asset_sound(mode='list') pages the whole bag and ignores name=. "
                "Use mode='one' to fetch that entry, or prefix= to filter the listing."
            )
        args = ["bag-ls"]
        args += _flag("--prefix", prefix)
    args += _flag("--bag", bag)
    args += _flag("--limit", limit)
    args += _flag("--offset", offset)
    return _invoke(
        args,
        ra2_dir=ra2_dir,
        all_mixes=all_mixes,
        timeout_s=TIMEOUT_QUICK_S,
        slow_advice="Lower limit=, or narrow with prefix=.",
    )


@mcp.tool()
def asset_art_for(
    type_id: str,
    theater: Literal["tem", "sno", "urb", "lun", "des", "ubn"] = "tem",
    image: str | None = None,
    ra2_dir: str | None = None,
    all_mixes: bool = False,
) -> str:
    """Resolve a rules type id to the art files that actually back it.

    Use when you have a rules id — "GAPOWR", "HARV", "E1" — and need the real
    filenames: the theater-substituted SHP or VXL, the cameo, and the declared
    palette, each checked for existence with its source archive. This is the
    bridge from an INI id to something asset_render or asset_info can open. For
    palette reasoning use asset_palette_for.

    Args:
        type_id: Rules type id, e.g. "GAPOWR".
        theater: "tem" (temperate, default) | "sno" | "urb" | "lun" | "des" |
            "ubn". Drives the theater-extension substitution.
        image: The rules Image= value, when you already know it. Supplying it
            skips the lookup and pins the art id.
        ra2_dir: Retail install root. Omit to let the CLI resolve it.
        all_mixes: Also mount archives the game's startup path skips. Default
            False.
    """
    args = ["art-for", type_id]
    args += _flag("--theater", theater)
    args += _flag("--image", image)
    return _invoke(
        args,
        ra2_dir=ra2_dir,
        all_mixes=all_mixes,
        timeout_s=TIMEOUT_QUICK_S,
    )


@mcp.tool()
def asset_compare(
    name: str,
    frame: int = 0,
    scale: int | None = None,
    no_render: bool = False,
    out: str | None = None,
    ra2_dir: str | None = None,
    all_mixes: bool = False,
) -> str:
    """Every archive's copy of one filename, diffed and rendered side by side. READ the sheet.

    One name can exist several times over: the RA2 and YR copies of a sidebar
    sprite, a per-theater terrain variant, an ecache override. They are not
    interchangeable — POWERP.SHP is 12x2 in sidec01.mix and 16x2 in sidec02.mix —
    and every other verb reports only the one copy that won.

    Use this when art looks right in one faction/theater and wrong in another,
    or before concluding anything from a single `asset_info` result. For which
    single copy the engine would actually load, use asset_find; for the frame
    table of one copy, asset_info.

    LIKE asset_render, THIS WRITES A PNG AND RETURNS ITS PATH. `outputs.sheet`
    holds every variant at one shared scale so the sizes are directly
    comparable; open it with your file-reading tool to see them. The JSON's
    `differences` list names each field that varies and `differ` is the verdict,
    so a structural answer is available without the image.

    Each variant is rendered with its own inferred palette, since a sidec01 copy
    and a sidec02 copy legitimately want different ones — the same palette
    caveat as asset_render applies to each cell.

    Args:
        name: Filename to compare, e.g. "POWERP.SHP".
        frame: Frame (or TMP tile) rendered from each variant. Default 0.
        scale: Shared integer upscale across all variants. Default: fitted.
        no_render: Report the structural diff only and write no PNGs. Cheap, and
            enough when you only need to know whether the copies differ.
        out: Output root directory. Default: the repo's target/asset.
        ra2_dir: Retail install root. Omit to let the CLI resolve it.
        all_mixes: Also mount archives the game's startup path skips, which can
            surface further copies. Default False.
    """
    args = ["compare", name]
    if frame:
        args += _flag("--frame", frame)
    args += _flag("--scale", scale)
    args += _switch("--no-render", no_render)
    args += _flag("--out", out)
    return _invoke(
        args,
        ra2_dir=ra2_dir,
        all_mixes=all_mixes,
        timeout_s=TIMEOUT_SWEEP_S,
        slow_advice=(
            "Pass no_render=True for the structural diff alone, or lower scale=."
        ),
    )


@mcp.tool()
def asset_scan(
    format: str | None = None,
    archive: str | None = None,
    where: str | None = None,
    limit: int | None = None,
    offset: int = 0,
    ra2_dir: str | None = None,
    all_mixes: bool = False,
) -> str:
    """Corpus-wide search across every mounted archive by format and field predicates. SLOW.

    This walks and sniffs every entry in every archive — registered and
    catalogued alike — so it answers questions no single-archive verb can:
    "every SHP wider than 200 px", "every palette outside ra2md.mix". Costly
    enough that it is the wrong first move for a known name (use asset_find) or
    a known archive (use asset_ls). Always pass format= or archive= when you can;
    an unfiltered scan sweeps the whole corpus.

    Args:
        format: Keep only this sniffed format tag: shp, tmp, vxl, hva, pal, vpl,
            pcx, aud, csf, fnt, mix, xcc, bik, vqa, text, tiny, unknown.
        archive: Case-insensitive substring on the archive name, e.g. "sidec".
        where: Field predicates as "k=v,k=v". The queryable keys per row come
            back in each hit's `fields` map, so run one broad scan first to see
            what a predicate could match against. The parsed predicates are
            echoed in the report, so a typo is visible rather than silent.
        limit: Page size. Omit for the CLI default of 200.
        offset: Rows to skip. Default 0.
        ra2_dir: Retail install root. Omit to let the CLI resolve it.
        all_mixes: Also mount archives the game's startup path skips, widening
            the sweep further. Default False.
    """
    args = ["scan"]
    args += _flag("--format", format)
    args += _flag("--archive", archive)
    args += _flag("--where", where)
    args += _flag("--limit", limit)
    if offset:
        args += _flag("--offset", offset)
    return _invoke(
        args,
        ra2_dir=ra2_dir,
        all_mixes=all_mixes,
        timeout_s=TIMEOUT_CORPUS_S,
        slow_advice=(
            "Add format= or archive= to cut the sweep down, and set limit= to page "
            "the results."
        ),
    )


@mcp.tool()
def asset_parse_check(
    format: str | None = None,
    limit: int | None = None,
    ra2_dir: str | None = None,
    all_mixes: bool = False,
) -> str:
    """Run every retail entry through its parser and tally what still parses. SLOW.

    A corpus-wide health check for the asset parsers, per format: ok/failed
    counts, total bytes, and a capped sample of failures with their errors. "ok"
    means the parser returned Ok — structural validity only, never a statement
    that the result matches gamemd semantics. Use after touching a parser, or to
    find which formats the sniffer leaves uncovered. For a single asset use
    asset_info, which reports the same parse but in full detail.

    Args:
        format: Restrict the check to one sniffed format tag: shp, tmp, vxl, hva,
            pal, vpl, pcx, aud, csf, fnt, mix, xcc, bik, vqa, text, tiny,
            unknown. Omit to check every format.
        limit: Cap on the failure samples reported per format. Omit to use the
            CLI's own default; the ok/failed counts stay authoritative either
            way.
        ra2_dir: Retail install root. Omit to let the CLI resolve it.
        all_mixes: Also mount archives the game's startup path skips, adding
            entries the game never loads. Default False.
    """
    args = ["parse-check"]
    args += _flag("--format", format)
    args += _flag("--limit", limit)
    return _invoke(
        args,
        ra2_dir=ra2_dir,
        all_mixes=all_mixes,
        timeout_s=TIMEOUT_CORPUS_S,
        slow_advice="Restrict to one format= and lower limit=.",
    )


if __name__ == "__main__":
    binary, _ = _resolve_binary()
    logger.info(
        "asset-browser MCP server starting (workspace=%s, binary=%s)",
        WORKSPACE,
        binary if binary else f"NOT BUILT — run `{BUILD_COMMAND}`",
    )
    mcp.run()
