# LOADFILEFROMMIX_SIDEBAR_SIDE_RESOLVER_ORDER_GHIDRA_REPORT

Date: 2026-05-27

## Target question

After Soviet `InitSideMixFiles`, what is the exact generic file resolver precedence
used by sidebar SHP loads such as `REPAIR.SHP`, `SELL.SHP`, `SIDE1.SHP`,
`POWERP.SHP`, and `TAB00.SHP`?

## Non-goals

- Do not inspect retail MIX membership except as a separately marked asset-side supplement.
- Do not re-trace the sidebar filename list already proven in
  `SIDEBAR_SOVIET_SHP_LOAD_PATH_FUN_006D02B0_GHIDRA_REPORT.md`.
- Do not inspect radar/left-panel selector filenames.
- Do not modify Rust, INI, sibling docs, or Ghidra state.

## Evidence needed to mark COMPLETE

- `LoadFileFromMIX` decompile plus assembly range proving cache-before-resolver behavior.
- Resolver helper evidence proving the global MIX list scan order.
- `MixFileClass` list initialization/insertion evidence proving whether newly opened side MIXes
  prepend or append.
- `InitSideMixFiles` evidence proving Soviet side filenames, call order, and liveness before
  `FUN_006D02B0`.
- At least one Rust-facing handoff with a concrete acceptance test name.

## Stop conditions

- Stop if Ghidra MCP read-only access is unavailable.
- Stop if the list order requires mutating Ghidra to recover a missing function boundary.
- Stop if the target expands into retail archive-content auditing.
- Stop after writing this one report and updating only [.swarm-claims.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/.swarm-claims.md).

## Verified findings

### 1. `InitSideMixFiles` is live before sidebar SHP loading

Active in YR: Yes.

`ScenarioClass__Full_Init` calls `InitSideMixFiles` at `0x0068781F` / `0x00687833`;
the `0x00687833` path advances loading progress immediately after a successful return.
Inside `InitSideMixFiles`, `FUN_006D02B0` is called at `0x00535347` after side MIX
setup, palette setup, sidebar text color setup, `UIMD.INI` open/read, and
`RulesClass__ReadCommandBar`.

Evidence: `get_assembly_context` for `0x00687833` and `0x00535347`; `InitSideMixFiles`
decompile and assembly `0x00534FA0..0x00535379`.

### 2. Soviet uses side index 2 filenames, opened in fixed call order

Active in YR: Yes.

`InitSideMixFiles` rewrites input side `2` to `1`, then increments the side number before
formatting filenames. For Soviet input side `1`, the formatted side number is `2`.
The call order is:

1. `SIDEC02MD.MIX` into `DAT_00884E70` if present.
2. `SIDEC02.MIX` into `DAT_00884E74`; failure returns `0`.
3. `SIDENC02.MIX` into `DAT_00884E78` if present, gated by `DAT_00884E74 != 0`.

Evidence: `InitSideMixFiles` decompile; assembly `0x00534FB1..0x00534FB6` for
Yuri-to-Soviet rewrite, `0x005350D3..0x00535160` for `SIDEC%02dMD.MIX`,
`0x00535179..0x005351FE` for `SIDEC%02d.MIX`, and `0x00535248..0x005352D6` for
`SIDENC%02d.MIX`.

### 3. `LoadFileFromMIX` checks the filename cache before any archive search

Active in YR: Yes.

`LoadFileFromMIX` copies the requested name, uppercases it through `FUN_007DCFC4`,
hashes the uppercase bytes with `CRCEngine__AddData`, and searches cache tree
`DAT_00ABF00C` before constructing a `CCFileClass` object. A cache node with the same
CRC and nonzero payload returns immediately; only cache misses continue to
`FUN_00473C50`.

Evidence: `LoadFileFromMIX` decompile; assembly `0x005B40BC..0x005B4129` for copy,
uppercase, CRC, and cache root load; `0x005B4132..0x005B41A2` for cache-tree search and
immediate return.

### 4. On cache miss, the MIX archive resolver scans the global list from first to last

Active in YR: Yes.

`FUN_00473C50` calls `FUN_005B4430` after the object's direct availability check fails.
`FUN_005B4430` uppercases and hashes the filename, loads the first global list node from
`DAT_00ABEFE0`, binary-searches that archive's sorted entry table, and if not found moves
to the node's `+0x04` link. The first matching archive wins.

Evidence: `FUN_00473C50` decompile and assembly `0x00473C7A..0x00473C94`;
`FUN_005B4430` decompile; assembly `0x005B44A1` loads `DAT_00ABEFE0`,
`0x005B44CB..0x005B4507` searches the current archive, and `0x005B4507..0x005B450B`
advances to the saved `+0x04` next link.

### 5. Newly opened side MIX archives append before the tail sentinel, so side-MIX resolver order matches construction order

Active in YR: Yes.

The MIX list is initialized as head sentinel `0x00ABEFDC` and tail sentinel
`0x00ABEFE8`; `DAT_00ABEFE0` is `head.next`, and `DAT_00ABEFF0` is `tail.prev`.
`MixFileClass` construction at `0x005B3C20` inserts the new node after
`DAT_00ABEFF0` and before the tail sentinel, then updates `tail.prev` and
`previous.next`. Therefore new MIXes append to the end of the search list, not the
front.

For the Soviet side archives installed by this function, relative resolver precedence is:
`SIDEC02MD.MIX` first, then `SIDEC02.MIX`, then `SIDENC02.MIX`. Archives already earlier
in the global list still beat these side archives; cached filename hits beat the list
entirely.

Evidence: `MixFileSystem_InitSentinels` decompile and assembly
`0x005B3AC0..0x005B3B06`; `MixFileClass` constructor decompile and assembly
`0x005B3DE2..0x005B3E00`; `FUN_005B4430` scan evidence above.

## Implementation Handoff

1. Verified behavior: `LoadFileFromMIX` is cache first, then first-match global MIX list;
   Soviet side archive relative order is `SIDEC02MD.MIX -> SIDEC02.MIX -> SIDENC02.MIX`.
   Rust delta: add a gamemd-style side resolver view for sidebar generic SHPs instead of
   loading each theme atlas from a single direct archive. Affected surface:
   `src/assets/asset_manager.rs`, `src/render/sidebar_chrome.rs`. Acceptance scenario:
   if `repair.shp` exists in multiple Soviet side archives, the atlas uses the first
   archive in this order after any earlier global archive/cache winner. Proposed test:
   `test_soviet_sidebar_resolver_prefers_sidec02md_then_sidec02_then_sidenc02`. Risk:
   high screenshot parity risk.

2. Verified behavior: side MIX archives append to the global list rather than prepending.
   Rust delta: do not model `load_nested("sidec02.mix")` insertion-at-front as gamemd's
   side-MIX mechanism for sidebar loads. Affected surface: `AssetManager::load_nested`,
   future side-resolver helper. Acceptance scenario: constructing a test resolver with
   preexisting global archives plus Soviet side archives preserves earlier archives first
   and side archives in construction order. Proposed test:
   `test_gamemd_mix_list_appends_side_archives_preserving_existing_precedence`. Risk:
   medium-high; a front-insert resolver can choose a different duplicate SHP.

3. Verified behavior: cache key is uppercase filename CRC only and is checked before
   archive search. Rust delta: parity-sensitive sidebar asset loading should either
   model the cache boundary or prove these generic SHPs are not cached before side setup.
   Affected surface: `src/assets/asset_manager.rs`, sidebar atlas construction.
   Acceptance scenario: a previously cached `SIDE1.SHP` prevents a later side archive
   duplicate from changing the returned bytes unless the cache is explicitly invalidated
   by a verified binary path. Proposed test:
   `test_loadfilefrommix_cache_wins_before_side_archive_search`. Risk: medium; likely
   low in stock first-load startup but high for side-switch/re-init parity.

## Negative Facts / Do Not Do

- Do not make `SIDENC02.MIX` the first Soviet side archive searched merely because it is
  opened last; constructor evidence shows append-before-tail, not prepend. Evidence:
  `0x005B3DE2..0x005B3E00`.
- Do not treat `AssetManager::load_nested` inserting at index `0` as gamemd-equivalent for
  side MIX setup. Evidence: Rust `src/assets/asset_manager.rs` inserts at `0`; binary
  appends new `MixFileClass` nodes before the tail sentinel.
- Do not bypass `SIDEC02MD.MIX` when resolving generic Soviet sidebar SHPs. It is opened
  before `SIDEC02.MIX` and searched before it among these side archives. Evidence:
  `0x005350D3..0x00535160` plus list insertion/scan evidence.
- Do not conclude physical ownership of `REPAIR.SHP`, `SELL.SHP`, `SIDE1.SHP`,
  `POWERP.SHP`, or `TAB00.SHP` from this report. This report proves resolver order, not
  retail archive membership.
- Do not ignore `LoadFileFromMIX`'s filename cache when reasoning about repeated side init
  or side switches. Evidence: `DAT_00ABF00C` cache checked at `0x005B4129..0x005B41A2`
  before any resolver walk.

## Remaining Uncertainty

- Exact list positions of every non-side archive before `InitSideMixFiles` were not
  exhaustively enumerated. The proved contract is that side archives append after the
  currently loaded global list and keep their own construction order.
- Whether stock startup ever caches any of the target sidebar filenames before side setup
  was not traced. Cache-before-resolver is verified; cache population timing for each
  sidebar filename remains open.
- Retail archive membership for the target SHPs was intentionally not checked.
- `DAT_00884E68` is released by `InitSideMixFiles` but not assigned inside this function;
  its writer and list position remain outside this target.

## Stale-doc replacement wording

Suggested replacement for [docs/research/SIDEBAR_REPAIR_SELL_BUTTON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_REPAIR_SELL_BUTTON_GHIDRA_REPORT.md)
section 7.2 first paragraph:

> The art varies through `LoadFileFromMIX` precedence, not through sidebar-side filename
> branches. After Soviet `InitSideMixFiles`, the side archives appended by this function
> are searched in construction order: `SIDEC02MD.MIX`, then `SIDEC02.MIX`, then
> `SIDENC02.MIX`, after any archives already earlier in the global MIX list and after any
> filename-cache hit. Retail archive membership is a separate asset question.

Suggested replacement for `docs/research/SIDE_MIXFILE_INIT_GHIDRA_REPORT.md` section 4
heading:

> Archive construction order, not complete resolver order

Add after that table:

> `MixFileClass` construction appends these archives before the global list tail sentinel.
> Since `FUN_005B4430` scans `head.next` to tail, these side archives are searched in the
> same relative order shown here. They do not automatically outrank archives already
> earlier in the global MIX list, and `LoadFileFromMIX` filename-cache hits bypass the list.

## Status

COMPLETE.
