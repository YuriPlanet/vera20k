# Low Overlay Mark: All Active Load-Context Scenario RNG Lifecycle

Date: 2026-08-30
Status: **COMPLETE**
System: active-retail YR authored low-overlay Mark ordering and Scenario RNG authority (GSI-04.13; shared GSI-04.12 load boundary)

## Verdict

Every active `gamemd.exe` path that can reach authored `OverlayClass::Mark @ 0x005FC570` now has an exact incoming Scenario-RNG owner and prefix disposition. Fresh campaign, LAN/IPX, WOL, and replay loads all use the ordinary `Read_Scenario -> Full_Init -> ReadMapOverlayPacks` path and run Mark only when `[Basic] NewINIFormat > 1`. They do not share one pre-Fill prefix:

- campaign constructs one `[Houses]`-driven House set before Fill and skips multiplayer Gather/assignment;
- active LAN/IPX and ordinary noncampaign modes construct a disposable House set, run two Gather/assignment callbacks, delete the first set without RNG, and construct the final House set before Fill;
- WOL state `2` replaces the selected `+0x84` callback with common `AssignStartingPoints`, including two tightly gated chooser draws;
- replay supplies the recorded seed/session, then follows the matching campaign or noncampaign family without an extra Scenario draw;
- savegame restore, accepted generated `.SED`, and a `gamemd.exe` editor load are evidence-backed no-Mark/excluded boundaries.

After the context prefix, native Fill owns any Fill draws and passes the same `Scenario+0x218` cursor to the y-major/x-minor OverlayPack pass. Successful procedural low triggers then consume exactly `3*L` raw words before authored Techno construction. There is no mode-local replacement cursor at Mark.

No load-bearing `BLOCKED`, `UNKNOWN`, approximate, or residual behavior remains in this load-context slice.

## Evidence discipline

- Active-retail authority is the live `gamemd.exe` program plus retail INI/map data.
- `C:\Users\enok\Documents\OpenTS` was used only to navigate inherited scenario-start families. No OpenTS behavior is accepted without active-YR proof.
- This report extends the bounded offline proof in [SCENARIO_PREFIX_PLAN_INELIGIBLE_FALLBACK_REINVESTIGATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/00-system-models/SCENARIO_PREFIX_PLAN_INELIGIBLE_FALLBACK_REINVESTIGATION_GHIDRA_REPORT.md); it does not weaken or replace that report's exact stock-offline transaction.
- The parent cold-checked the fresh seed copy, both common network picker draw sites, stream-load seed-zero reset, startup editor-flag clear, and first Start_Scenario call sites after the worker completed.

## Common fresh-start owner

`Main_Game @ 0x0052D9A0` calls `Init_Random_Number_System @ 0x0052FC20` at `0x0052E619`, before either `Start_Scenario` call at `0x0052E718` and `0x0052E745`.

Both arms of `Init_Random_Number_System` seed a local Random object from current `g_RngSeed`, copy exactly `0xFD` dwords into `ScenarioClass + 0x218`, reseed from the same value, and copy `0xFD` dwords into Main RNG. MapGen is not involved. Therefore every fresh-load prefix below starts from a complete Scenario logical state produced from the authoritative launch seed, not from a draw count or a reconstructed partial cursor.

## Active context matrix

| Context | Seed authority | Prefix before Fill | Mark disposition |
|---|---|---|---|
| Campaign authored map | fresh `g_RngSeed` through `0x0052FC20` | one campaign House construction pass; no multiplayer callbacks | ordinary reader; Mark iff `NewINIFormat > 1` |
| LAN/IPX Battle-family | host/guest network seed | House pass 1 -> Gather `+0x80` -> selected Battle `+0x84` Gather/chooser -> zero-draw reset -> House pass 2 | ordinary reader; Mark iff `NewINIFormat > 1` |
| LAN/IPX Cooperative | host/guest network seed | House pass 1 -> Gather `+0x80` -> Cooperative `+0x84` Gather/chooser -> zero-draw reset -> House pass 2 | ordinary reader; Mark iff `NewINIFormat > 1` |
| WOL state `2` | network session seed | House pass 1 -> Gather `+0x80` -> common `AssignStartingPoints` (second Gather + gated chooser) -> zero-draw reset -> House pass 2 | ordinary reader; Mark iff `NewINIFormat > 1` |
| Replay playback | recorded `g_RngSeed`, scenario filename and session block | corresponding recorded campaign/noncampaign family; no replay-only draw | ordinary reader; Mark iff recorded map has `NewINIFormat > 1` |
| Savegame stream restore | serialized stream, then native seed-zero reset | no Full_Init prefix | **No Mark**; stream content loader never enters the scenario/overlay-pack reader |
| Accepted generated `.SED` | match seed for Scenario; independent MapGen seed for geometry | synthetic Full_Init is followed by direct generation | **No Mark**; synthetic `NewINIFormat` defaults to `0`, later deck writer is direct |
| Map editor | none in shipped `gamemd.exe` | persistent flag is forced off | **Excluded**; FinalAlert is a separate executable |

## Campaign prefix

Campaign reaches the same ordinary `Full_Init` overlay call at `0x00687A34`, but it does not execute the noncampaign two-House/two-Gather transaction.

Its exact pre-Fill Scenario order is:

```text
Seed
  -> one RandomRanged(450,1800) House constructor invocation per [Houses] row
     (if the section is empty, FUN_005009B0 constructs every registered HouseType instead)
  -> Fill_In_Data
  -> authored OverlayPack Mark, when NewINIFormat > 1
```

There is no disposable House set, no multiplayer Gather callback, no multiplayer chooser, and no second House pass. Retail campaign maps inspected for this boundary use `NewINIFormat=4` with both overlay packs, so this is active rather than a hypothetical branch.

Fill is on the same cursor. The inspected Scenario-RNG receiver set shows clear/missing Fill consumes zero; Water consumes one rejection-capable `RandomRanged(0,3)` per allocated cell. Mark must receive the actual continuation after those calls rather than a campaign seed or House-only reconstructed cursor.

## LAN/IPX and WOL prefixes

LAN/IPX seed authority is network-owned: the host path is rooted at `0x005B82F0`; the guest reads the packet seed at offset `+0x92` in `0x005B67F0`. LAN sets `g_GameMode=3` and adjacent state `0x00A8B24C=2`; it does **not** set WOL selector `0x00A8B244=2`. Active LAN therefore uses the selected mode `+0x84` callback after the common `+0x80` callback.

Battle-family and Cooperative both Gather twice. Each deficient Gather retry consumes its exact Y-then-X ranged calls against the resized default-cell map, as proved by the offline prefix report. Their chooser differences are retained:

- Battle-family automatically draws `RandomRanged(0,N-1)` only for the first automatic non-Special House when no start is occupied; later choices are deterministic maximum-distance selections.
- Cooperative automatically assigned humans draw within the human-start prefix and probe forward; AI suffix assignment is deterministic.

The compiled WOL-state-`2` branch calls `AssignStartingPoints @ 0x005EE9D0`. That function first Gathers, then uses picker `0x005EE6F0`. Its only Scenario chooser draws are:

1. player-controlled pass, receiver `0x005EE748`: an unassigned House draws rejection-capable `RandomRanged(0,N-1)` only while occupied count is zero; at most one such draw can occur;
2. AI pass, receiver `0x005EE792`: an unassigned AI draws rejection-capable `RandomRanged(0,N-3)` only while exactly two starts are occupied, selecting that ordinal among free starts; at most one such draw can occur.

Every other common picker arm is deterministic. After the selected or common assignment callback, rules/type reset deletes the first House set without Scenario RNG, `Read_INI_Basic` creates the same ordered final House set with the same `RandomRanged(450,1800)` constructor invocation per House, and Fill continues the cursor.

## Replay

Replay playback is a fresh scenario start, not savegame restoration. Its header supplies `g_RngSeed`, scenario filename, and the recorded session block. Playback calls the normal RNG initializer and then `Start_Scenario(-1)`. It adds zero Scenario calls before Mark and inherits the corresponding campaign/noncampaign prefix selected by the recorded session. A recorded ordinary authored map reaches Mark only when `NewINIFormat > 1`.

## No-Mark and excluded contexts

### Savegame restore

`ScenarioClass::Save_To_Stream @ 0x00689310` writes the raw `0x3740`-byte Scenario block, physically including `+0x218`. `ScenarioClass::Load_From_Stream @ 0x00689470` reads it at `0x006894AC..0x006894B7` and immediately calls `0x00683560` at `0x006894C5`; `0x00683564..0x0068356C` pushes `0`, takes `Scenario+0x218`, and calls `Random::Seed`.

Thus native overwrites the serialized Scenario cursor with seed-zero state. More importantly for this mechanism, `Load_Game_Content_From_Stream @ 0x0067E730` does not call `Read_Scenario`, `Full_Init`, `ReadMapOverlayPacks`, or `OverlayClass::Mark`. Restore has no first-Mark cursor and must not replay low endpoint expansion.

### Accepted generated `.SED`

The `.SED` arm and ordinary INI reader are mutually exclusive. Its early synthetic Full_Init omits `NewINIFormat`, so the default `0` makes `ReadMapOverlayPacks @ 0x005FD2E0` return. Later MapGen stamps complete low decks directly without Overlay construction, Unlimbo, or Mark. Generated materialization consumes exactly zero authored-Mark Scenario words.

### Editor

Persistent `g_IsMapEditor @ 0x00A8ED6B` is unconditionally cleared at startup parser `0x0052F63E`; no enable switch was found. The only other writer in `0x005A91E0` saves, temporarily sets, and restores the byte inside gameplay and is not a load entry. FinalAlert is a separate executable. Shipped `gamemd.exe` therefore has no active editor scenario-load context to implement.

## Mark boundary shared by every positive context

- Fill `0x004ACE70` precedes the sole `ReadMapOverlayPacks @ 0x005FD2E0` call at `Full_Init + 0xF14 @ 0x00687A34`.
- Pack traversal is fixed 512x512, y-major then x-minor.
- Ordinary low body/end identities call ordinary Mark and consume zero low-procedural RNG.
- Successful triggers `0x7A..0x7D` and `0xE9..0xEC` consume exactly `3*L` raw `Scenario+0x218 Random::Next` calls, opposite-end toward trigger, with inner order `j=0,1,2`.
- Fixed-row writes, scans, occupied no-op, missing opposite, all failure arms, and generated direct decks consume zero low-Mark words.
- Mark completes before authored Unit, Aircraft, Infantry, and Structure constructors consume their own Scenario words.

The 385-payload shipped-data census contains zero procedural low triggers, so shipped maps execute zero `3*L` transactions while still exercising ordinary low-row Mark. Procedural expansion remains active content-conditional retail behavior and requires a synthetic authored retail-rules fixture.

## Current Rust mismatch and implementation handoff

Current `origin/main` has one optional Battle/FFA `PreloadedBattleStartPlan` and an incidental generated-source proxy. It does not own the complete context matrix above.

Required deltas before BR-M05 can pass:

1. Replace the optional stock-offline plan with the universal two-House/two-Gather P0-R1 transaction from the companion report.
2. Give campaign and network/replay entry points typed, provenance-bearing prefix owners. Do not substitute the offline callback family for campaign or WOL state `2`.
3. Pass one complete Scenario cursor through context prefix and Fill, then expose a narrow raw-word seam to inline low Mark before authored Techno construction.
4. Preserve savegame restore as no-Mark with native seed-zero Scenario restoration behavior.
5. Preserve `GeneratedMaterialized` as no-Mark, derived from explicit `.SED`/generation provenance plus native `NewINIFormat > 1`, never from construction-trace presence or overlay geometry.
6. Test full logical cursor states, not invocation counters, across campaign, LAN Battle, LAN Cooperative, WOL state `2`, replay inheritance, save restore, generated `.SED`, and authored successful/no-op/failure low triggers.

The narrow bridge requirement is the exact cursor at Mark. This report does not require inventing unsupported network transport or campaign UI; it requires any active loader context VERA20k exposes to carry the correct typed native prefix, and requires unsupported surfaces to remain explicit rather than silently borrowing offline state.

## Coverage closure

| Question | Verdict |
|---|---|
| common fresh Scenario seed owner | VERIFIED |
| campaign first-Mark prefix | VERIFIED |
| LAN/IPX seed and selected callback family | VERIFIED |
| WOL common assignment draws | VERIFIED |
| replay extra-draw question | VERIFIED NEGATIVE |
| savegame Mark replay | VERIFIED NEGATIVE |
| generated `.SED` Mark replay | VERIFIED NEGATIVE |
| shipped `gamemd.exe` editor load | VERIFIED EXCLUDED |
| Fill-to-Mark shared cursor | VERIFIED |
| procedural Mark draw owner/order | VERIFIED |

**BLOCKED:** none.

**UNKNOWN:** none.
**Implementation readiness:** READY when combined with the offline prefix and low-Mark inner-algorithm reports.

## Certainty-gated metadata sync

After all workers stopped, the parent dry-ran, applied, saved, and read back EOL comments at:

- `0x005EE748` and `0x005EE792` for the two gated common network picker draws;
- `0x006894C5` for stream-load seed-zero/no-Mark behavior;
- `0x0052F63E` for the shipped `gamemd.exe` editor exclusion.

No rename, type change, speculative comment, or concurrent metadata mutation was applied.

## Sources

- active-retail `gamemd.exe` addresses enumerated above;
- retail `rulesmd.ini`, `MPModesMD.ini`, and inspected campaign/map payloads;
- `LOW_OVERLAY_MARK_SCENARIO_LOAD_ACTIVATION_BOUNDARY_GHIDRA_REPORT.md`;
- `LOW_OVERLAY_MARK_FIXED_MAP_STAMP_RNG_TRANSACTION_GHIDRA_REPORT.md`;
- [SCENARIO_PREFIX_PLAN_INELIGIBLE_FALLBACK_REINVESTIGATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/00-system-models/SCENARIO_PREFIX_PLAN_INELIGIBLE_FALLBACK_REINVESTIGATION_GHIDRA_REPORT.md);
- current Rust owners on `origin/main`;
- `C:\Users\enok\Documents\OpenTS` as navigation leads only.
