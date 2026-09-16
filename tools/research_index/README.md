# VERA20k Research Index

Local index for retained VERA20k research and retail INI data. It retrieves
references for investigation; a search result does not establish that a claim
is current, correct or implemented.

V1 is deliberately simple:

- SQLite FTS5 full-text search.
- Markdown/INI chunks with file and line citations.
- Source-kind and status inference.
- Address, symbol, INI key, Rust path, and markdown link extraction.
- Related-document lookup by shared extracted terms.
- Deterministic docgraph edges for document navigation.
- CLI and FastMCP entry points over the same library contracts.
- No embeddings or chat UI.

## Layout

```text
tools/research_index/
  schema.sql
  index.py
  search.py
  related.py
  graph.py
  handoff.py
  map.py
  brief.py
  validate.py
  health.py
  research_index/
    chunking.py
    database.py
    formatting.py
    lifecycle.py
    locking.py
    metadata.py
    ranking.py
    touchpoints.py
```

Generated state is written to:

```text
tools/research_index/.cache/research.db
tools/research_index/.cache/research.db.meta.json
```

The sidecar manifest records the exact indexed roots, corpus file identities,
index-builder source signature, tool format, and published database identity.
Both files are generated and ignored by Git.

## Build The Index

```powershell
python tools/research_index/index.py
```

Default indexed roots:

- `docs/research`
- `ini`

Historical research is archived outside the repository and excluded from
default searches. Plans remain in the repository for task continuity, but are
also excluded by default. Search them explicitly using a separate database:

```powershell
python tools/research_index/index.py docs/plans --db tools/research_index/.cache/plans.db
python tools/research_index/search.py --db tools/research_index/.cache/plans.db "movement acceptance"
```

You can override roots in the default database:

```powershell
python tools/research_index/index.py docs/research/bridges
```

Explicit root overrides are persisted in the generation manifest, so automatic
MCP refreshes keep the same scope. Roots must stay inside the workspace and
contain at least one indexable Markdown or INI file; an unsafe or empty request
cannot replace a valid database.

After upgrading an older index, or to reset a custom scope, run
`python tools/research_index/index.py`. This explicitly restores the current
defaults; a health refresh alone preserves the previously saved roots.

### Search Historical Research Explicitly

The [research archive guide](../../docs/research/README.md) records the archive
snapshot and retrieval instructions. Preserve its original `docs/research/`
tree, and index it with a separate workspace **and** database:

```powershell
$archiveRoot = 'C:/path/to/vera20k-research-archive'
python tools/research_index/index.py docs/research --workspace $archiveRoot --db "$archiveRoot/.cache/research.db"
python tools/research_index/search.py --db "$archiveRoot/.cache/research.db" "BridgeRepairHut"
```

Paths returned by that database are relative to the archive workspace. Archived
reports describe historical investigations and may contain superseded claims;
check the current source and original native evidence before relying on them.
Do not add the archive to the current index or put a copy under its indexed roots.

Rebuilds use unique sibling temporary databases plus atomic replacement. A
cross-process lock serializes publication, and the new generation is certified
only when the corpus is unchanged across the rebuild. Already-fresh reads use a
lock-free inspection path; stale reads recheck under the publication lock before
rebuilding, so multiple MCP sessions do not serialize ordinary lookups.

## Freshness Health

Inspect the database, manifest, and current corpus without changing anything:

```powershell
python tools/research_index/health.py
python tools/research_index/health.py --json
```

Synchronously rebuild only when stale:

```powershell
python tools/research_index/health.py --refresh
```

The health command reports added, changed, and removed files with bounded text
output. It exits nonzero when inspection is stale or not ready. Freshness uses
file size plus nanosecond modification time; an unusual same-size edit that
also preserves the exact timestamp requires an explicit reindex.

## Search

```powershell
python tools/research_index/search.py "BlowUpBridge DropIn"
python tools/research_index/search.py --system bridges "Can_Enter_Cell"
python tools/research_index/search.py --source ghidra "0x0047DD70"
python tools/research_index/search.py --json "low bridge TubeClass height"
```

Results include:

- path
- heading
- line range
- source kind
- status
- ranked snippet

Source kind and status are inferred from filenames. In particular, `verified`
can mean only that a filename contains `GHIDRA_REPORT`, `TRACE` or `VERIFICATION`;
it does not certify the report, the current Rust implementation or native parity.
The direct CLI search reads the existing database without refreshing it; rebuild
after corpus changes or check freshness with `health.py` first.

## Related Docs

Find docs sharing symbols, addresses, INI keys, or Rust paths with a source doc:

```powershell
python tools/research_index/related.py docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_COLLAPSE_FALLOUT_ORDERING_GHIDRA_REPORT.md
```

Find docs related to an exact term:

```powershell
python tools/research_index/related.py --term CellClass::BlowUpBridge
```

## Docgraph

The index also builds deterministic graph edges from extracted evidence:

- `references_doc`
- `mentions_symbol`
- `mentions_address`
- `mentions_ini_key`
- `mentions_rust_path`
- `belongs_to_system`
- `belongs_to_subsystem`
- `has_source_kind`
- `has_status`

Graph commands:

```powershell
python tools/research_index/graph.py doc docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_COLLAPSE_FALLOUT_ORDERING_GHIDRA_REPORT.md
python tools/research_index/graph.py backlinks docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_COLLAPSE_FALLOUT_ORDERING_GHIDRA_REPORT.md
python tools/research_index/graph.py evidence CellClass::BlowUpBridge
python tools/research_index/graph.py implementation CellClass::BlowUpBridge
python tools/research_index/graph.py evidence 0x0047DD70 --json
```

`evidence` prioritizes research-document relationships. `implementation`
prioritizes Rust paths mentioned by the matching docs. Implementation views
also report `exists=yes|no` against the selected workspace; existence is a
freshness clue, not proof that the file still owns the cited behavior.
Hex-address lookups ignore case and leading zero padding, so `0x73e5e0` and
`0x0073E5E0` resolve the same extracted address.

## Parity Handoff

Build an implementation-oriented handoff before changing Rust:

```powershell
python tools/research_index/handoff.py "Can_Enter_Cell zone passability"
python tools/research_index/handoff.py "chrono miner refinery unload implementation handoff"
python tools/research_index/handoff.py --system bridges "collapse"
```

The handoff view combines:

- explicit implementation-handoff sections when present;
- top evidence chunks ranked by inferred status, with file and line citations;
- Rust touchpoints from extracted graph terms, including supporting doc citations;
- warnings when evidence, handoff sections, or Rust touchpoints are missing.

Handoffs require meaningful query-term coverage before presenting evidence as
implementation guidance. Generic corpus words such as `research`, `handoff`,
and `Rust` cannot make an unrelated document match by themselves. Default text
is a bounded summary with explicit omission counts; `--json` retains the
detailed structured bundle and can be much larger.

## System / Topic Map

List the research inventory for a system or topic:

```powershell
python tools/research_index/map.py --system bridges
python tools/research_index/map.py --system bridges collapse
python tools/research_index/map.py --system bridges collapse --source ghidra
```

The map view groups matching documents by subsystem/source/status, lists the
matching docs, and surfaces implementation-handoff plus contradiction,
supersession, stale, and uncertainty sections when those headings or phrases are
present.

## Pre-Implementation Brief

Build a compact planning bundle before editing Rust:

```powershell
python tools/research_index/brief.py --system miner "Mission_Harvest State 2 chrono miner return teleport drive" --anchor 0x73e5e0
```

The brief combines topic-map results, document validation, implementation
handoff candidates, Rust touchpoints, top evidence, and optional exact
symbol/address anchors.

## Validate Indexed Docs

Check that indexed docs still exist, match their indexed checksum, and have no
missing local markdown links:

```powershell
python tools/research_index/validate.py --system bridges collapse
python tools/research_index/validate.py --system miner "Mission_Harvest State 2"
```

The CLI validation command is deliberately non-mutating and detects changed,
missing, and newly added files plus local links against the live filesystem.
Rebuild with `python tools/research_index/index.py` or
`python tools/research_index/health.py --refresh` after intentional edits. An
explicitly scoped validation that matches zero documents is also invalid:
validating nothing must not certify a research scope.

## MCP Server

The repo-local `.mcp.json` launches:

```powershell
python tools/research_index/mcp_server.py
```

It exposes `research_search`, `research_related`, `research_graph`,
`research_map`, `research_handoff`, `research_validate`, `research_brief`,
`research_reindex`, and `research_health`. Text is the compact default; request
JSON only when a caller needs the full structured rows.

Every evidence-reading MCP tool checks freshness before opening the index. If
the corpus changed, the call waits for one synchronous, locked rebuild and then
continues against the certified generation. Failures are surfaced instead of
serving stale evidence. `research_health(refresh=false)` is the non-mutating way
to inspect pending changes; `research_validate` refreshes first and then checks
the requested scope and live local links. Restart an MCP process that predates
the tool before expecting it to appear in tool discovery.

## Ranking Model

V1 uses a conservative evidence preference:

1. Ghidra reports
2. traces
3. implementation contracts
4. system model syntheses
5. Rust audits
6. plans
7. unknown notes

The ranking and inferred status are retrieval hints only. Establish native
behavior from original `gamemd.exe` bodies, callers and data; establish current
Rust behavior from source and appropriate validation.

## Next Steps

- Add explicit YAML frontmatter to high-value docs for `status`,
  `supersedes`, and `superseded_by`.
- Add a `contract.py` command that emits implementation-checklist drafts from
  cited chunks.
- Add optional semantic retrieval after FTS and metadata ranking are trusted.
