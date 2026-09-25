# VERA20k Asset Browser MCP

An MCP server that exposes the `asset` CLI — the headless browser for retail
RA2/YR assets — so Claude Code and Codex can call it as tools.

The implementation is Rust, in `src/asset_tools/` behind `src/bin/asset.rs`.
The server has nothing importable to reach for: every tool `subprocess.run`s the
compiled binary and returns its stdout, which is JSON by contract. The CLI prints
JSON on stdout even when it fails, so a caller parsing stdout always gets JSON.

Read-only against the retail install. The only files it writes are PNGs, WAVs and
extracted bytes under `target/asset`, which is gitignored.

## Prerequisite: build the binary

```powershell
cargo build --release -p vera20k --bin asset
```

The server looks for `target/release/asset.exe` (Windows) or
`target/release/asset`, and falls back to the `target/debug` build only if the
release one is missing — in which case every result carries a note saying so.
Use release: a debug build of this tool is many times slower over the ~8000-entry
retail corpus, and `asset_scan` / `asset_parse_check` will likely hit the
timeout.

Nothing is cached at import, so a build that lands while the server is running is
picked up on the next call. A *new tool* still needs an MCP restart before it
appears in discovery.

## Registration

Claude Code, in the repo-local `.mcp.json` — repo-relative path, no `cwd` key:

```json
{
  "mcpServers": {
    "asset-browser": {
      "command": "python",
      "args": [
        "tools/asset_browser/mcp_server.py"
      ]
    }
  }
}
```

Codex, in `.codex/config.toml` — absolute path:

```toml
[mcp_servers.asset-browser]
command = "python"
args = ['C:\Users\enok\Documents\ra2-rust-game\tools\asset_browser\mcp_server.py']
```

The server locates the workspace from its own file path, never from the working
directory, so both launch styles resolve the same repo. Child processes are then
run with the workspace as their working directory, which is what lets the CLI
find `config.toml` and keeps its default `target/asset` output root inside the
repo.

## Retail root

Every tool takes an optional `ra2_dir`. Omit it and the CLI resolves the retail
install itself, in its own order: `--ra2-dir` → `$RA2_DIR` → `config.toml`.

## Tools

| Tool | What it answers |
| --- | --- |
| `asset_find` | Which archive wins for a filename, what shadows it, and what is catalogued but unreachable by name lookup. |
| `asset_ls` | Paged listing of one archive's entries, with hashes reversed to names. |
| `asset_info` | Parsed structure without rendering: SHP frame tables, TMP tiles, VXL limbs, palettes, CSF/AUD/PCX/FNT/VPL headers. |
| `asset_render` | Writes PNGs for SHP/TMP/PCX/PAL/VXL and returns their paths — the caller then reads those paths to look at the art. |
| `asset_palette_for` | Which palette applies, the full inference chain, and how much to trust it. |
| `asset_archives` | Every mounted archive, with entry counts and lookup reachability marked. |
| `asset_extract` | An asset's raw bytes written to disk, with provenance. |
| `asset_csf` | The string table: `mode="get"` for one key, `mode="grep"` to search keys and values. |
| `asset_sound` | The audio bag: `mode="one"` for one entry (`wav=True` decodes it), `mode="list"` to page the index. |
| `asset_art_for` | A rules type id resolved to the art files that back it, per theater. |
| `asset_compare` | Every archive's copy of one filename, diffed and rendered side by side. |
| `asset_scan` | Corpus-wide search across every mounted archive by format, archive and field predicates. **Slow.** |
| `asset_parse_check` | Every retail entry run through its parser, tallied per format. **Slow.** |

Two things worth knowing before trusting output:

- **`asset_render` returns paths, not pixels.** The JSON lists absolute PNG paths
  under `outputs.frames`, `outputs.sheet` and `outputs.index`. Reporting that JSON
  is not looking at the art — open the paths to actually see the sprite.
- **The palette is inferred.** `palette.reason` says how it was picked and
  `palette.confidence` says how much to trust it: `production` means a real engine
  code path binds that palette to this asset class and `palette.production_site`
  cites the line it was read from; `declared` means art.ini named it; `heuristic`
  is a guess. A render that looks plausible is not proof the palette is right. Use
  `asset_palette_for` when colour matters.

## Failure behaviour

Every subprocess failure is wrapped, because a crashed child must not look like
an empty result:

- **Binary not built** → a hint naming the exact build command and both paths
  searched. Never an exception.
- **Timeout** → the call is killed and a hint comes back naming the arguments
  that shrink the job (`limit`, `format`, `archive`). Budgets are 120s for
  single-asset verbs, 300s for the sweeping ones (`ls`, `find`, `render`), and
  900s for `scan` / `parse-check`.
- **Non-zero exit** → the CLI's own `{"error": ..., "hint": ...}` JSON is
  returned as-is. Exit 1 is a failed verb, exit 2 a bad command line.
- **Exit 0 with no stdout, or a spawn failure** → a synthesised JSON error saying
  so, with a tail of the child's stderr. This is a tool bug, not a miss.

stdout and stderr are captured separately. The CLI logs at warn level to stderr,
so anything it prints there on a successful run is surfaced as a note under
`_mcp_notes` — injected into the JSON rather than printed beside it, so the
result stays parseable.

## CLI equivalent

Nothing here is MCP-only; the binary does the same work directly.

```powershell
cargo build --release -p vera20k --bin asset

./target/release/asset --help

./target/release/asset find POWERP.SHP
./target/release/asset archives
./target/release/asset ls ra2md.mix --format shp --limit 20
./target/release/asset info POWERP.SHP --ascii --frame 0
./target/release/asset render POWERP.SHP --house 0 --limit 4
./target/release/asset palette-for POWERP.SHP
./target/release/asset extract POWERP.SHP --out target/asset
./target/release/asset csf-get Name:GAPOWR
./target/release/asset csf-grep "Power Plant" --limit 10
./target/release/asset bag-ls --prefix ir
./target/release/asset sound irbuild --wav
./target/release/asset art-for GAPOWR --theater sno
./target/release/asset compare POWERP.SHP
./target/release/asset scan --format vxl --limit 40
./target/release/asset parse-check --format shp
```

Global flags follow the verb, not precede it: `asset find X --ra2-dir <PATH>`,
not `asset --ra2-dir <PATH> find X`. `--all-mixes` additionally mounts archives
the game's startup path skips — a tooling-only widening, so hits found only that
way are not what the game would resolve.
