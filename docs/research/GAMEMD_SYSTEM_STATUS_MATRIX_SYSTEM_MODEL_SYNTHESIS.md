# Yuri's Revenge `gamemd.exe` System Status Matrix

**Date:** 2026-07-20  
**Native target:** active retail Yuri's Revenge `gamemd.exe` in the local Ghidra project  
**Rust snapshot:** `a97ce88454d2ab938e6f8892dcac861845302c09`  
**Parent inventory:** `GAMEMD_SYSTEM_INVENTORY_COVERAGE_MAP_GHIDRA_REPORT.md`  
**Document kind:** evidence-ranked system-model synthesis / row-level status matrix  
**Status:** investigation baseline; **not a completeness percentage and not a parity certification**

## Verdict

The matrix is useful, and it exposes why a single whole-game completion number would currently be misleading.

- The registry contains **336 rows**. **77** are mixed/group nodes that must be split before scoring, leaving **259 atomic candidates**.
- Of the atomic candidates, **245** are stock-active, mode-active, or content-conditional. The other **14** are compiled-inactive or have unresolved reachability.
- Native behavior is implementation-grade (`CONTRACTED`) for **66** atomic rows, anchored but not closed for **89**, and still `UNCHECKED` for **104**.
- Current Rust ownership is `PRESENT` for **32** atomic rows, `PARTIAL` for **187**, `SCAFFOLD` for **24**, and `ABSENT` for **16**. These are structural states, not parity verdicts.
- There are **115 evidence-backed `DRIFT` rows**, all inside the active 245-row set. The remaining **130 active rows are `UNCHECKED`**, not passes.
- There are **zero `TRACE_MATCHED` rows, zero `VERIFIED` rows, and zero native-oracle certifications** at this whole-system row width.

The immediate conclusion is therefore: the system list is broad enough to organize the work, but the denominator is not complete, the implementation is not complete, and no whole-game parity claim is supportable yet.

## Scope and non-scope

This synthesis reconciles the parent inventory, existing research reports, current Rust ownership at the named commit, retail INI defaults, and two live-binary spot checks. It does not modify Rust, INI data, or existing research reports. It does not claim that every binary function, content-specific hardcoded branch, vtable slot, shell transition, network protocol field, or render/audio edge has been found.

A row is deliberately excluded from scoring when its parent inventory scope contains `mixed` or identifies a group node. That prevents a broad family containing several independently auditable mechanisms from receiving one misleading status.

## Controlled status model

The axes are independent. For example, `PRESENT` Rust plus `CONTRACTED` native behavior can still be `DRIFT`, while `ABSENT` on a compiled-inactive legacy path remains `UNCHECKED` rather than active-stock drift.

### Activity

| Value | Meaning |
|---|---|
| `STOCK_ACTIVE` | Used by ordinary retail YR gameplay or shell execution. |
| `MODE_ACTIVE` | Active in a stock campaign, skirmish, multiplayer, shell, or platform mode. |
| `CONTENT_CONDITIONAL` | Active when stock content, an INI flag, or a specific mechanic selects it. |
| `COMPILED_INACTIVE` | Compiled but dormant/disabled in standard retail YR according to current evidence. |
| `UNKNOWN` | Stock reachability is not yet resolved. |
| `GROUP_NODE` | Mixed parent requiring decomposition; all scoring axes are `N/A`. |

### Inventory evidence

| Value | Meaning |
|---|---|
| `DISCOVERED` | The system is named and plausibly owned, but its boundary has not been closed. |
| `BOUNDED` | Scope, owner, and at least one load-bearing native/source surface are identified. |
| `EXHAUSTIVE_SLICE` | Reserved for an exhaustive proof over the whole row; no broad row currently qualifies. |
| `GROUP_NODE` | Deliberately unscored mixed parent. |

### Native evidence depth

| Value | Meaning |
|---|---|
| `UNCHECKED` | No current load-bearing native contract for the full row. |
| `ANCHORED` | Active native entry point/state owner is verified, but important branches remain open. |
| `CONTRACTED` | Existing evidence is detailed enough to state the native implementation contract for this row. |
| `NATIVE_ORACLE` | A retail-derived executable oracle or exhaustive proof exists for the full row; none currently qualify. |

### Current Rust state

| Value | Meaning |
|---|---|
| `ABSENT` | No in-scope runtime owner was found. |
| `SCAFFOLD` | Types, parser keys, flags, or a stub exist, but the runtime mechanism does not. |
| `PARTIAL` | A meaningful subset exists, but the row is visibly incomplete or too broad to call present. |
| `PRESENT` | The principal current runtime owner/path exists; this says nothing about parity. |
| `COMPLETE_FOR_CONTRACT` | Every known contract point is implemented and checked; no row currently qualifies. |

### Parity

| Value | Meaning |
|---|---|
| `UNCHECKED` | Exact equivalence is unproved; this is not a pass. |
| `DRIFT` | At least one mechanism, state, order, timing, byte, input, audio, or pixel difference is evidenced. |
| `TRACE_MATCHED` | A named retail-vs-Rust trace matches for the full row; none currently qualify. |
| `VERIFIED` | A native oracle or exhaustive proof certifies the full relevant input space; none currently qualify. |

## Status summary

| Measure | Count |
|---|---:|
| Registry rows | 336 |
| Mixed/group rows (`GROUP_NODE`) | 77 |
| Atomic candidates | 259 |
| Stock-active atomic | 212 |
| Mode-active atomic | 26 |
| Content-conditional atomic | 7 |
| Compiled-inactive atomic | 7 |
| Unknown-reachability atomic | 7 |
| Inventory `BOUNDED` | 153 |
| Inventory `DISCOVERED` | 106 |
| Native `CONTRACTED` | 66 |
| Native `ANCHORED` | 89 |
| Native `UNCHECKED` | 104 |
| Rust `PRESENT` | 32 |
| Rust `PARTIAL` | 187 |
| Rust `SCAFFOLD` | 24 |
| Rust `ABSENT` | 16 |
| Parity `DRIFT` | 115 |
| Parity `UNCHECKED` | 144 |
| Parity `TRACE_MATCHED` | 0 |
| Parity `VERIFIED` | 0 |

### Active-denominator view

| Axis | Active-row distribution |
|---|---|
| Inventory | 144 bounded; 101 discovered |
| Native | 63 contracted; 83 anchored; 99 unchecked |
| Rust | 30 present; 184 partial; 20 scaffold; 11 absent |
| Parity | 115 drift; 130 unchecked; 0 trace-matched; 0 verified |

No percentage is computed. Treating `UNCHECKED` as matching, treating `PRESENT` as complete, or scoring the 77 group nodes would manufacture confidence that the evidence does not support.

## Family rollup

| GSI | Family | Rows | Atomic | Group | Bounded | Contracted | Anchored | Present | Partial | Scaffold | Absent | Drift | Unchecked |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 01 | Runtime, platform, and global execution | 14 | 13 | 1 | 7 | 6 | 1 | 1 | 12 | 0 | 0 | 3 | 10 |
| 02 | Files, configuration, types, and assets | 17 | 15 | 2 | 7 | 5 | 2 | 0 | 15 | 0 | 0 | 3 | 12 |
| 03 | Shell, setup, loading, and modes | 17 | 12 | 5 | 8 | 3 | 5 | 0 | 8 | 1 | 3 | 6 | 6 |
| 04 | World, map, terrain, and environment | 23 | 17 | 6 | 17 | 15 | 2 | 0 | 16 | 1 | 0 | 14 | 3 |
| 05 | Entity, ownership, and lifecycle | 21 | 16 | 5 | 11 | 10 | 1 | 1 | 14 | 1 | 0 | 7 | 9 |
| 06 | Navigation, locomotion, and movement | 24 | 22 | 2 | 21 | 18 | 3 | 1 | 20 | 1 | 0 | 14 | 8 |
| 07 | Orders, missions, radio, docking, and transport | 45 | 38 | 7 | 6 | 1 | 5 | 3 | 35 | 0 | 0 | 3 | 35 |
| 08 | Combat, weapons, damage, and status | 34 | 20 | 14 | 6 | 3 | 3 | 1 | 14 | 4 | 1 | 6 | 14 |
| 09 | Economy, tech, construction, and production | 21 | 20 | 1 | 6 | 0 | 6 | 6 | 14 | 0 | 0 | 6 | 14 |
| 10 | AI, teams, scripts, triggers, and outcomes | 16 | 1 | 15 | 0 | 0 | 0 | 0 | 1 | 0 | 0 | 0 | 1 |
| 11 | Superweapons and strategic effects | 12 | 8 | 4 | 8 | 3 | 5 | 5 | 0 | 3 | 0 | 6 | 2 |
| 12 | Visibility, intelligence, disguise, and radar | 11 | 8 | 3 | 7 | 2 | 5 | 3 | 3 | 2 | 0 | 5 | 3 |
| 13 | World rendering and effects | 26 | 19 | 7 | 14 | 0 | 14 | 5 | 11 | 3 | 0 | 10 | 9 |
| 14 | Input, commands, and UI | 13 | 11 | 2 | 9 | 0 | 9 | 0 | 11 | 0 | 0 | 9 | 2 |
| 15 | Audio, speech, music, and video | 11 | 9 | 2 | 7 | 0 | 7 | 0 | 9 | 0 | 0 | 7 | 2 |
| 16 | Networking and multiplayer | 14 | 13 | 1 | 10 | 0 | 12 | 0 | 0 | 5 | 8 | 12 | 1 |
| 17 | Persistence, restoration, and evidence | 9 | 9 | 0 | 4 | 0 | 4 | 5 | 4 | 0 | 0 | 4 | 5 |
| 18 | Legacy, disabled, and tooling | 8 | 8 | 0 | 5 | 0 | 5 | 1 | 0 | 3 | 4 | 0 | 8 |

The family table is a count of classifications, not a weighted progress score. A broad render pipeline and a small mission handler each occupy one atomic row.

## Full row-level matrix

`Basis` is a compact evidence topic code. `INV+FAMILY_SCAN` means the parent inventory plus current top-level Rust ownership scan only; it does not mean that the row was behaviorally audited. `MIXED-SCOPE` marks excluded group nodes. The evidence ledger after the matrix maps the reviewed topic groups to their primary sources.

### GSI-01 — Runtime, platform, and global execution

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-01.01 | executable bootstrap, process environment, and startup checks | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-01.02 | window creation, Windows message pump, activation, and focus | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-01.03 | top-level shell/game state machine | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | UNCHECKED | CORE-MAIN-GAME |
| GSI-01.04 | scenario initialization, start, exit, and teardown lifecycle | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | UNCHECKED | CORE-LIFECYCLE |
| GSI-01.05 | deterministic per-tick global scheduler and rung order | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | CORE-SCHEDULER |
| GSI-01.06 | clocks, frame pacing, game speed, pause, and modal pumping | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-01.07 | scenario-owned deterministic RNG stream (`ScenarioClass+0x218`) | STOCK_ACTIVE | BOUNDED | CONTRACTED | PRESENT | UNCHECKED | RNG-SCENARIO |
| GSI-01.08 | main/global deterministic gameplay RNG (`g_MainRng`) | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | RNG-MAIN |
| GSI-01.09 | separately seeded random-map-generation RNG (`g_MapGenRng`) | MODE_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | UNCHECKED | RNG-MAPGEN |
| GSI-01.10 | timers, countdowns, delays, and cadence conversion | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | TIMER-CADENCE |
| GSI-01.11 | coordinates, cells/leptons/pixels, facing, fixed math, and lookup tables | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-01.12 | allocation, object pools, vectors, reference tracking, and final cleanup | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-01.13 | CD/install/registry/path discovery and retail media checks | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-01.14 | localization/code-page/platform string services | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |

### GSI-02 — Files, configuration, types, and assets

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-02.01 | virtual file system and MIX archive search/precedence | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | MIX-VFS |
| GSI-02.02 | loose-file, language-pack, theater-pack, and map-pack resolution | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-02.03 | INI lexical parsing, defaults, inheritance, and base/`*md` overlay | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | UNCHECKED | INI-PARSING |
| GSI-02.04 | rules globals and object/type registries | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-02.05 | house, side, country, color, and ownership data | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-02.06 | AI TaskForce/Script/Team/AITrigger data loading | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-02.07 | art/type-image metadata and animation declarations | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-02.08 | theater metadata, tilesets, LAT/ramp/morph tables | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-02.09 | scenario/map INI and packed-section decoding | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-02.10 | SHP sprite decoding and frame metadata | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | UNCHECKED | SHP-DECODE |
| GSI-02.11 | TMP tile decoding, subtile geometry, and extra-data planes | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | TMP-DECODE |
| GSI-02.12 | VXL/HVA/VPL voxel data, transforms, and lighting tables | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | VXL-HVA-VPL |
| GSI-02.13 | palettes, remap tables, color conversion, and translucency tables | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | UNCHECKED | PAL-CONVERT |
| GSI-02.14 | CSF strings, fonts, text layout inputs, and UI string lookup | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-02.15 | audio index/bag/AUD/VOC/WAV decoding and sample lookup | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-02.16 | PCX/images, compression codecs, and packed-data helpers | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | UNCHECKED | PCX-DECODE |
| GSI-02.17 | Bink/VQA/cinematic media discovery and decoding interface | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |

### GSI-03 — Shell, setup, loading, and modes

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-03.01 | main-menu composition and shell transitions | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | MAIN-MENU |
| GSI-03.02 | options, hotkeys, display/audio settings, quit confirmation | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-03.03 | single-player campaign catalog and side/difficulty selection | MODE_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-03.04 | campaign progression, scenario mapping, carryover, and unlock state | MODE_ACTIVE | BOUNDED | ANCHORED | ABSENT | DRIFT | CAMPAIGN-FLOW |
| GSI-03.05 | mission selection, briefing, restate, and objective presentation | MODE_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-03.06 | victory/defeat transition, score screen, and final results | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-03.07 | movies, sneak preview, credits, and final-movie selection | MODE_ACTIVE | BOUNDED | ANCHORED | SCAFFOLD | DRIFT | MOVIE-CREDITS |
| GSI-03.08 | load/save shell dialogs and slot metadata | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | UNCHECKED | SAVE-SHELL |
| GSI-03.09 | scenario loading screen, progress manager, and transition art | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-03.10 | skirmish setup, player slots, factions, teams, colors, and options | MODE_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | SKIRMISH-SETUP |
| GSI-03.11 | map browser, filters, preview, metadata, and start positions | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-03.12 | random-map generator configuration, generation, and preview | MODE_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | UNCHECKED | RMG-CURRENT |
| GSI-03.13 | multiplayer mode catalog: battle, co-op, siege, team, world domination | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-03.14 | LAN session discovery, host/join setup, and lobby shell | MODE_ACTIVE | BOUNDED | ANCHORED | ABSENT | DRIFT | LAN-SHELL |
| GSI-03.15 | Westwood Online account/chat/game/download shell | MODE_ACTIVE | BOUNDED | ANCHORED | ABSENT | DRIFT | WOL-SHELL |
| GSI-03.16 | observer setup, multiplayer score, and post-game flow | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-03.17 | session/mode policy, packed options, house/start generation, and runtime match-start handoff | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |

### GSI-04 — World, map, terrain, and environment

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-04.01 | map dimensions, cell grid, playable bounds, and coordinate lookup | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | MAP-CELL |
| GSI-04.02 | theater selection, tile placement, and isometric ground geometry | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | ISO-TILE |
| GSI-04.03 | elevation, ramps, cliffs, slopes, and height conversion | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | ELEVATION |
| GSI-04.04 | cell-owned land type, movement-zone labels, and passability state | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | CELL-STATE |
| GSI-04.05 | cell occupancy, object-content lists, layers, and entry reservations | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | OCCUPANCY |
| GSI-04.06 | zone/subzone grid state and connectivity topology | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | ZONES |
| GSI-04.07 | overlay placement, ownership, damage, and removal | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | OVERLAY-LIFE |
| GSI-04.08 | walls, gates, fences, pavement, and buildable overlays | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-04.09 | ore/gems/tiberium overlay identity, placement, and per-cell amount state | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | RESOURCE-CELLS |
| GSI-04.10 | terrain objects: trees, rocks, flammability, crush, and destruction | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | TERRAIN-LIFE |
| GSI-04.11 | smudges, craters, scorch marks, and persistence | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | UNCHECKED | SMUDGE-CURRENT |
| GSI-04.12 | high-bridge topology, occupancy, and traversal | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | HIGH-BRIDGE |
| GSI-04.13 | low/water bridge topology, decks, ramps, and traversal | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | LOW-BRIDGE |
| GSI-04.14 | bridge damage, collapse, debris, repair, and control huts | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | BRIDGE-DAMAGE |
| GSI-04.15 | low-bridge tubes/tunnels and endpoint movement | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | BRIDGE-TUBE |
| GSI-04.16 | waypoints, player starts, regions, and scenario navigation anchors | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | START-WAYPOINT |
| GSI-04.17 | tags, cell tags, local/global variables, and map flags | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-04.18 | cell-owned unexplored-shroud counters/bits and persisted map knowledge | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | UNCHECKED | SHROUD-CELLS |
| GSI-04.19 | optional fog-cell storage, concealment timers, and regrowth gates | CONTENT_CONDITIONAL | BOUNDED | ANCHORED | SCAFFOLD | UNCHECKED | FOG-DORMANT |
| GSI-04.20 | ambient lighting, global tint, light sources, and day/night transitions | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-04.21 | radiation sites, cell hazards, fire, and environmental damage | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-04.22 | weather/ambient environmental events and map ambience | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-04.23 | crates: placement, timers, pickup, contents, and powerups | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |

### GSI-05 — Entity, ownership, and lifecycle

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-05.01 | type-instance registration and stable object identity | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-05.02 | active-object vector membership and deterministic iteration | STOCK_ACTIVE | BOUNDED | CONTRACTED | PRESENT | UNCHECKED | LOGIC-VECTOR |
| GSI-05.03 | create, reveal, conceal, limbo, unlimbo, uninit, and delete lifecycle | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | OBJECT-LIFECYCLE |
| GSI-05.04 | target/reference notices, expiration, detach, and final-reference handling | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | REFERENCE-LIFE |
| GSI-05.05 | Abstract/Object base state and spatial identity | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | UNCHECKED | ABSTRACT-OBJECT |
| GSI-05.06 | Mission/Radio/Techno/Foot behavioral spine | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | TECHNO-SPINE |
| GSI-05.07 | infantry instances, stances, sequences, and occupation | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-05.08 | vehicle/naval unit instances and unit-specific state | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-05.09 | aircraft instances, flight state, airports, and airborne identity | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-05.10 | building instances, foundations, upgrades, occupants, and animation state | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-05.11 | bullet/projectile instances and target references | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | BULLET-LIFE |
| GSI-05.12 | animation instances and attached animation ownership | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | ANIM-LIFE |
| GSI-05.13 | particle and particle-system instances | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | PARTICLE-LIFE |
| GSI-05.14 | voxel-animation, debris, and falling-object instances | STOCK_ACTIVE | BOUNDED | CONTRACTED | SCAFFOLD | DRIFT | VOXELANIM-LIFE |
| GSI-05.15 | terrain/overlay/smudge instance lifecycles | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | UNCHECKED | MAPOBJ-LIFE |
| GSI-05.16 | House authority: identity/control, diplomacy/alliance, owned registries, defeat/winner flags, and statistics | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-05.17 | Factory runtime identity, house registration, reference ownership, and lifecycle only | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | UNCHECKED | FACTORY-LIFE |
| GSI-05.18 | Team runtime identity, membership/reference state, and lifecycle only | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-05.19 | Trigger/Tag runtime identity, cross-references, persistence, and lifecycle only | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-05.20 | Super runtime identity, house registration, charge-state ownership, and lifecycle only | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-05.21 | shared attached-manager registration, reference detach, and lifecycle infrastructure only | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |

### GSI-06 — Navigation, locomotion, and movement

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-06.01 | movement request admission, destination choice, and cell-entry gates | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | MOVE-ADMIT |
| GSI-06.02 | queries over zone topology for reachable destinations and admission decisions | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | ZONE-QUERY |
| GSI-06.03 | path search, open/closed state, tie-breaking, and path reconstruction | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | PATH-SEARCH |
| GSI-06.04 | locomotor consumption of cell state: effective terrain cost, speed type, and modifiers | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | MOVE-COST |
| GSI-06.05 | path smoothing, retries, fallback, and blocked-path recovery | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | PATH-RETRY |
| GSI-06.06 | path queueing, reservations, traffic arbitration, and same-tick commits | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | TRAFFIC-ORDER |
| GSI-06.07 | occupancy enter/leave commits and bridge/layer transitions | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | MOVE-COMMIT |
| GSI-06.08 | collision, scatter, pushing, bumping, crushing, and overlap recovery | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-06.09 | FootClass convoy chain, follower links, spacing, and persistent cohesion state | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-06.10 | TeamClass AI formation/group movement and team-level coordination | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-06.11 | facing, rotation, drive tracks, curves, acceleration, and braking | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | DRIVE-FACING |
| GSI-06.12 | locomotor dispatch, link ownership, piggyback infrastructure, and authority handoff | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | LOCOMOTOR-PIGGY |
| GSI-06.13 | Drive locomotion | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | DRIVE-LOCO |
| GSI-06.14 | Walk locomotion | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | UNCHECKED | WALK-LOCO |
| GSI-06.15 | Ship locomotion | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | UNCHECKED | SHIP-LOCO |
| GSI-06.16 | Fly locomotion | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | FLY-LOCO |
| GSI-06.17 | Hover locomotion | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | UNCHECKED | HOVER-LOCO |
| GSI-06.18 | Jumpjet locomotion | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | UNCHECKED | JUMPJET-LOCO |
| GSI-06.19 | Rocket locomotion | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | ROCKET-LOCO |
| GSI-06.20 | Teleport locomotion | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | TELEPORT-LOCO |
| GSI-06.21 | Mech locomotion | COMPILED_INACTIVE | BOUNDED | CONTRACTED | PARTIAL | UNCHECKED | DORMANT-LOCO |
| GSI-06.22 | DropPod locomotion | COMPILED_INACTIVE | BOUNDED | CONTRACTED | PRESENT | UNCHECKED | DORMANT-LOCO |
| GSI-06.23 | Tunnel/subterranean locomotion, distinct from active bridge tubes | COMPILED_INACTIVE | BOUNDED | CONTRACTED | SCAFFOLD | UNCHECKED | DORMANT-LOCO |
| GSI-06.24 | air takeoff, landing, altitude, circling, and airport approach | STOCK_ACTIVE | BOUNDED | CONTRACTED | PARTIAL | DRIFT | AIR-MOVEMENT |

### GSI-07 — Orders, missions, radio, docking, and transport

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-07.01 | command admission, ownership validation, and order replacement | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.02 | 32-row mission-control metadata, rates, flags, and name lookup | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-07.03 | mission verb API: assign, queue, override, suspend, restore, and guard rules | STOCK_ACTIVE | BOUNDED | CONTRACTED | PRESENT | UNCHECKED | MISSION-VERBS |
| GSI-07.04 | mission dispatcher, current/queued/substate fields, timer rewrite, and vtable routing | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | LIVE-MISSION |
| GSI-07.05 | Mission 0: Sleep handler and idle cadence | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.06 | Mission 1: Attack handler | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.07 | Mission 2: Move handler | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.08 | Mission 3: QMove selector and Sleep-handler fallback | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.09 | Mission 4: Retreat handler | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.10 | Mission 5: Guard handler | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.11 | Mission 6: Sticky selector and Guard-handler routing | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.12 | Mission 7: Enter handler | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.13 | Mission 8: Capture handler | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.14 | Mission 9: Eaten handler/row | UNKNOWN | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.15 | Mission 10: Harvest handler | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | UNCHECKED | PHASE7-PR242-248 |
| GSI-07.16 | Mission 11: Area Guard handler | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.17 | Mission 12: Return handler | COMPILED_INACTIVE | BOUNDED | ANCHORED | ABSENT | UNCHECKED | PHASE7-DEAD-SLOT-0x234 |
| GSI-07.18 | Mission 13: Stop handler | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.19 | Mission 14: Ambush dead TS stub | COMPILED_INACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.20 | Mission 15: Hunt handler | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.21 | Mission 16: Unload handler | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | UNCHECKED | PHASE7-PR251 |
| GSI-07.22 | Mission 17: Sabotage selector and Capture-slot routing | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-07.23 | Mission 18: Construction handler | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.24 | Mission 19: Selling handler | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.25 | Mission 20: Repair handler | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.26 | Mission 21: Rescue handler and AI-only assignment path | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | MISSION-RESCUE |
| GSI-07.27 | Mission 22: Missile handler | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-07.28 | Mission 23: Harmless handler | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-07.29 | Mission 24: Open handler | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.30 | Mission 25: Patrol handler | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-07.31 | Mission 26: Paradrop Approach handler | CONTENT_CONDITIONAL | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.32 | Mission 27: Paradrop Overfly handler | CONTENT_CONDITIONAL | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.33 | Mission 28: Wait/Deliberate handler | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-07.34 | Mission 29: Attack Move assign-side selector with no dispatcher case | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | UNCHECKED | LIVE-MISSION |
| GSI-07.35 | Mission 30: Spyplane Approach handler | CONTENT_CONDITIONAL | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.36 | Mission 31: Spyplane Overfly handler | CONTENT_CONDITIONAL | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.37 | Radio contact protocol, link negotiation, messages, and teardown | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | PHASE7-PR253 |
| GSI-07.38 | generic docking reservations, queues, and authority handoff | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | UNCHECKED | PHASE7-PR253 |
| GSI-07.39 | refinery docking, ore transfer, credit display, and release | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | UNCHECKED | PHASE7-PR245-246 |
| GSI-07.40 | aircraft docking, pad choice, landing, rearm, and release | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.41 | factory exit, spawn cell, rally point, and blocked-exit recovery | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.42 | cargo/passenger load, unload, capacity, and transporter destruction | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-07.43 | open-topped passenger fire, garrison, bunker, and occupant coordination | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-07.44 | IFV/Gunner passenger-dependent weapon slot, cached pointer, and turret-variant selection | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | UNCHECKED | IFV-GUNNER |
| GSI-07.45 | gate opening/closing protocol and linked traversal | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |

### GSI-08 — Combat, weapons, damage, and status

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-08.01 | target legality, acquisition, threat scoring, and opportunity selection | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.02 | weapon selection, primary/secondary/elite choice, and target filters | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.03 | fire gates, reload readiness, ammo, power, transport, and mission gates | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.04 | range, line of fire, fire location/FLH, facing, and fire-error calculation | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.05 | ROF, burst, distributed/radial fire, rearm, and veterancy modifiers | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.06 | projectile creation, source/target bookkeeping, and launch side effects | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.07 | ballistic, straight, arcing, homing, torpedo, and vertical flight | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-08.08 | projectile collision, proximity, fuse, interception, and detonation | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-08.09 | area damage, cell spread, distance falloff, and target collection | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.10 | damage kernel, armor/Verses, clamps, immunities, and healing | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.11 | death, destruction, kill credit, passengers/crew, debris, and explosions | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.12 | veterancy, experience, promotion, elite weapons, and ability modifiers | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.13 | infantry fear, prone/crawl, death sequences, and suppression-like state | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.14 | body/turret/barrel facing, recoil, rocking, and firing animation state | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.15 | air-to-air, anti-air, strafing, bombing, and airstrike control | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.16 | garrison/open-topped/bunker firing and occupant damage routing | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-08.17 | crushing and crush-death combat consequences | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-08.18 | C4, Ivan bombs, timed bombs, bridge charges, and disarm/cleanup | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-08.19 | prism support network, support targeting, charge contribution, and firing handoff | STOCK_ACTIVE | BOUNDED | CONTRACTED | ABSENT | DRIFT | PRISM-SUPPORT |
| GSI-08.20 | gattling stage, stage timer, weapon/turret selection, and reset | STOCK_ACTIVE | BOUNDED | CONTRACTED | SCAFFOLD | DRIFT | GATTLING-STAGE |
| GSI-08.21 | sonic weapon triple path: projectile damage, ambient path damage, and WaveClass handoff | STOCK_ACTIVE | BOUNDED | ANCHORED | SCAFFOLD | DRIFT | SONIC-WAVE |
| GSI-08.22 | Tesla electrical strike and EBolt creation/gameplay handoff | STOCK_ACTIVE | BOUNDED | ANCHORED | SCAFFOLD | DRIFT | EBOLT-HANDOFF |
| GSI-08.23 | laser weapon fire/damage path and LaserDraw creation handoff | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-08.24 | radiation-beam/eruption gameplay, radiation application, and visual handoff | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-08.25 | Magnetron locomotor hijack, piggyback swap, lift, carry, drop, and landing damage | STOCK_ACTIVE | BOUNDED | CONTRACTED | SCAFFOLD | DRIFT | MAGNETRON |
| GSI-08.26 | RadSite runtime manager, cell radiation decay/damage cadence, and emitter state | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | RADSITE |
| GSI-08.27 | fire, chaos, berserk, poison-like, and other persistent status effects | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-08.28 | mind control, capture manager, psychic immunity, and overload | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-08.29 | temporal targeting, warp-out, erase, and release | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-08.30 | parasite attach/attack/exit and host interactions | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-08.31 | spawn manager, spawned aircraft, reload, launch, and recovery | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-08.32 | slave manager, slave work/respawn, and owner transitions | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-08.33 | warhead special flags, animations, sounds, terrain, bridge, and ore effects | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-08.34 | crate/powerup combat modifiers and temporary bonuses | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |

### GSI-09 — Economy, tech, construction, and production

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-09.01 | credits, income/spending, displayed money, and transaction ordering | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | UNCHECKED | PHASE7-PR244 |
| GSI-09.02 | storage capacity, refinery storage, silo behavior, and resource loss | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | UNCHECKED | PHASE7-STORAGE |
| GSI-09.03 | ore/gem value lookup, harvester capacity, collection, and unload conversion | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | PHASE7-PR242 |
| GSI-09.04 | resource growth/spread scheduling and map resource state | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | PHASE7-PR243 |
| GSI-09.05 | standard miner/harvester work-site selection and economy-side return decisions | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | UNCHECKED | PHASE7-PR245-248 |
| GSI-09.06 | slave miner deployment, slaves, grinding, and mobile refinery behavior | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-09.07 | power production/drain, low power, blackout, and powered-state effects | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | POWER-STATE |
| GSI-09.08 | tech tree, prerequisites, build limits, stolen tech, and availability | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-09.09 | factory ownership, build queues, parallel production, and abandonment | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | FACTORY-ORDER |
| GSI-09.10 | build time, cost, difficulty/house modifiers, and production progress | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | FACTORY-PROGRESS |
| GSI-09.11 | placement legality, foundations, adjacency, buildable cells, and previews | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-09.12 | building buildup, construction state, completion, and activation | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-09.13 | service-facility eligibility, repair/rearm/hospital/armory effects, and costs | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-09.14 | sell, refund, occupants/crew, undeploy, and teardown consequences | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-09.15 | capture/ownership transfer effects on power, tech, radar, and production | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-09.16 | MCV deploy/undeploy and construction-yard authority | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | MCV-DEPLOY |
| GSI-09.17 | upgrades, powers-up-building, and building slot effects | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-09.18 | Grinder intake, occupant destruction, soylent conversion, and release | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-09.19 | Cloning Vat duplicate-production selection and free-clone creation | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-09.20 | Bio Reactor occupant slots, power contribution, ejection, and destruction | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-09.21 | Ore Purifier house-income modifier and ownership transitions | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |

### GSI-10 — AI, teams, scripts, triggers, and outcomes

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-10.01 | House AI brain, state, update cadence, and strategic priorities | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.02 | base planning, build placement, defense zones, and rebuilding | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.03 | AI economy/resource management and spending priorities | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.04 | AI production choice, factory assignment, and build queues | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.05 | threat maps, target scoring, defense response, and enemy selection | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.06 | AITrigger eligibility, weights, selection, cooldowns, and team creation | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.07 | TaskForce composition and member acquisition | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.08 | Team formation, ownership, recruitment, state, and dissolution | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.09 | ScriptType steps, arguments, branching, completion, and failure | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.10 | scenario Trigger/Tag evaluation and persistence | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.11 | Event predicates, occurrence counts, houses, timers, and variables | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.12 | Action dispatch, parameters, side effects, and action ordering | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.13 | campaign/mission scripting, objectives, reinforcements, and cinematics | MODE_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-10.14 | difficulty, IQ, handicap, aggression, and campaign modifiers | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.15 | win/loss evaluation, end-delay, surrender, alliance, and scenario outcome | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-10.16 | scoring, kills/losses, economy statistics, ranking, and result aggregation | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |

### GSI-11 — Superweapons and strategic effects

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-11.01 | generic superweapon ownership, charge, hold, readiness, targeting, and launch | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-11.02 | nuclear missile launch, flight, warning, detonation, and aftermath | STOCK_ACTIVE | BOUNDED | CONTRACTED | SCAFFOLD | DRIFT | NUKE-LAUNCH |
| GSI-11.03 | Lightning Storm scheduling, weather, strikes, damage, and cleanup | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | LIGHTNING-STORM |
| GSI-11.04 | Iron Curtain targeting, invulnerability, expiry, and feedback | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | IRON-CURTAIN |
| GSI-11.05 | Force Shield targeting, protection, power consequence, and expiry | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | FORCE-SHIELD |
| GSI-11.06 | Chronosphere/ChronoWarp selection, source/target areas, transport, and arrival | STOCK_ACTIVE | BOUNDED | CONTRACTED | SCAFFOLD | DRIFT | CHRONOSPHERE |
| GSI-11.07 | Psychic Dominator targeting, control transfer, damage, and visuals | STOCK_ACTIVE | BOUNDED | CONTRACTED | SCAFFOLD | DRIFT | DOMINATOR |
| GSI-11.08 | Genetic Mutator targeting, eligibility, conversion, damage, and visuals | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | UNCHECKED | GENETIC-MUTATOR |
| GSI-11.09 | paradrop superweapon charge, target selection, aircraft/payload creation, and launch | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-11.10 | spy-plane/recon flight, reveal footprint, aircraft lifecycle, and recharge | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-11.11 | Psychic Reveal superweapon charge, target selection, and reveal-effect dispatch | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | UNCHECKED | PSYCHIC-REVEAL |
| GSI-11.12 | superweapon building links, availability loss, one-time flags, and sidebar state | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |

### GSI-12 — Visibility, intelligence, disguise, and radar

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-12.01 | per-object sight range and reveal-footprint algorithm | STOCK_ACTIVE | BOUNDED | CONTRACTED | PRESENT | UNCHECKED | SHROUD-FOOTPRINT |
| GSI-12.02 | reveal/conceal update protocol that mutates GSI-04.18 cell state | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | SHROUD-PROTOCOL |
| GSI-12.03 | elevation, cliffs, bridges, buildings, and visibility reference frames | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | UNCHECKED | HEIGHT-LOS-CURRENT |
| GSI-12.04 | per-house/object visibility policy when optional fog-cell state is enabled | CONTENT_CONDITIONAL | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-12.05 | cloak state, cloak progress, decloak triggers, and visual state | STOCK_ACTIVE | BOUNDED | CONTRACTED | SCAFFOLD | DRIFT | CLOAK-FSM |
| GSI-12.06 | sensors, detection, stealth legality, and sensor arrays | STOCK_ACTIVE | BOUNDED | ANCHORED | SCAFFOLD | DRIFT | SENSOR-CLOAK |
| GSI-12.07 | disguise, spy identity, mirage presentation, and reveal conditions | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-12.08 | Gap Generator concealment, radius updates, power, and house effects | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | GAP-GENERATOR |
| GSI-12.09 | SpySat/Psychic Reveal/global-reveal effect application to map-cell knowledge | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-12.10 | radar availability, jam/blackout, radar events, and minimap knowledge | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | RADAR-STATE |
| GSI-12.11 | observer/allied vision sharing and multiplayer visibility policy | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |

### GSI-13 — World rendering and effects

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-13.01 | frame buffers, dirty regions, tactical redraw, and final composition | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | RENDER-TICK |
| GSI-13.02 | isometric projection, camera origin, clipping, and tactical scrolling | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | CAMERA-MOTION |
| GSI-13.03 | object layer assignment, Y/Z ordering, occlusion, and draw traversal | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | DRAW-ORDER |
| GSI-13.04 | TMP ground, ramps, cliffs, shore, LAT, and tile transitions | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-13.05 | overlay, wall, ore, bridge, terrain-object, and smudge drawing | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-13.06 | SHP sprite frame selection, facings, sequences, and blitters | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-13.07 | voxel/HVA transform, facing, turret/barrel, rasterization, and bounds | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | UNCHECKED | VXL-RASTER |
| GSI-13.08 | palette selection, house remap, translucency, color conversion, and blending | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-13.09 | Z-buffer, A-buffer/alpha surfaces, clipping masks, and overlap handling | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | DEPTH-BUFFERS |
| GSI-13.10 | ambient/global lighting, per-object light, tint, and palette effects | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-13.11 | ground/object/voxel shadows and height projection | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | SHADOWS |
| GSI-13.12 | animations, damage/fire/smoke, debris, and voxel-animation drawing | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | UNCHECKED | ANIM-DRAW |
| GSI-13.13 | particle systems, particles, trails, sparks, and smoke composition | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | UNCHECKED | PARTICLE-DRAW |
| GSI-13.14 | LaserDraw object/vector update, lifetime, endpoints, and rendering | STOCK_ACTIVE | BOUNDED | ANCHORED | SCAFFOLD | DRIFT | LASERDRAW |
| GSI-13.15 | transient laser-segment lifetime and purge manager | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-13.16 | DiskLaser manager, update order, geometry, lifetime, and rendering | STOCK_ACTIVE | BOUNDED | ANCHORED | SCAFFOLD | DRIFT | DISKLASER |
| GSI-13.17 | EBolt vector, electrical-arc lifetime, update, and drawing | STOCK_ACTIVE | BOUNDED | ANCHORED | SCAFFOLD | DRIFT | EBOLT |
| GSI-13.18 | WaveClass vector, sonic/magnetic wave lifetime, update, and drawing | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-13.19 | AlphaShape vector, alpha-surface lifetime, purge, and drawing | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-13.20 | remaining animation/particle-backed weapon and superweapon visual dispatch | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-13.21 | shroud/fog/visibility overlay composition | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-13.22 | selections, health bars, pips, action lines, rally lines, and markers | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | UNCHECKED | SELECTION-DRAW |
| GSI-13.23 | radar/minimap terrain, units, events, shroud, and viewport | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | RADAR-DRAW |
| GSI-13.24 | sidebar/power strip/cameos/tabs/clock/queue presentation | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | SIDEBAR-DRAW |
| GSI-13.25 | cursor, tooltip, placement ghost, range, and target feedback | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-13.26 | shell/dialog/loading/score/movie presentation | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |

### GSI-14 — Input, commands, and UI

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-14.01 | keyboard sampling, hotkeys, key bindings, and command classes | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | HOTKEYS |
| GSI-14.02 | mouse sampling, button state, wheel, drag, and double-click handling | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | MOUSE-SELECT |
| GSI-14.03 | cursor/action determination and target-context resolution | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | ACTION-RESOLVE |
| GSI-14.04 | single/bandbox/type selection, selection filters, and selection order | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | SELECTION |
| GSI-14.05 | control groups, select-same-type, next/previous, and camera recall | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | CONTROL-GROUPS |
| GSI-14.06 | edge/key scrolling, camera clamp, bookmarks, and tactical navigation | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | AUTOSCROLL |
| GSI-14.07 | local semantic player-order construction before deterministic/network envelope encoding | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | ORDER-CONSTRUCT |
| GSI-14.08 | building placement, repair/sell/power modes, deploy, and rally input | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-14.09 | superweapon target mode, range/legality feedback, and cancel | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-14.10 | sidebar tabs, production buttons, queues, power strip, and tooltips | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | SIDEBAR-INPUT |
| GSI-14.11 | radar clicks, drag, map navigation, events, and tactical recenter | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | MINIMAP-INPUT |
| GSI-14.12 | in-game options, save/load, surrender, restart, and quit flow | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-14.13 | chat, beacons, taunts, planning mode, and allied communication | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |

### GSI-15 — Audio, speech, music, and video

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-15.01 | sound-type registry, sample lookup, variants, priority, and volume | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | AUDIO-REGISTRY |
| GSI-15.02 | positional attenuation, stereo pan, listener/camera relation, and cutoff | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | AUDIO-SPATIAL |
| GSI-15.03 | channels, handles, interruption, looping, stop/fade, and lifetime | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | AUDIO-CHANNELS |
| GSI-15.04 | DirectSound device/channel pool, mixer/buffer servicing, and audio update-thread cadence | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | AUDIO-BACKEND |
| GSI-15.05 | gameplay/UI/weapon/animation/ambient sound-trigger routing | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | AUDIO-TRIGGERS |
| GSI-15.06 | EVA event selection, queueing, suppression, interruption, and house voice | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | PHASE7-PR249-254 |
| GSI-15.07 | theme/music catalog, selection, shuffle, transitions, and stream pump | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | UNCHECKED | PHASE7-PR247 |
| GSI-15.08 | unit voices, acknowledgements, attack/move/death pools, and taunts | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-15.09 | subtitles/captions and speech-linked text presentation | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-15.10 | Bink/VQA movie playback, audio sync, skip, and shell transition | MODE_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | MOVIE-PLAYBACK |
| GSI-15.11 | briefing/cinematic speech, camera cues, and scripted media | MODE_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |

### GSI-16 — Networking and multiplayer

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-16.01 | deterministic `EventClass`/command-envelope encoding, serialization, and lockstep admission | MODE_ACTIVE | BOUNDED | ANCHORED | SCAFFOLD | DRIFT | NET-EVENT |
| GSI-16.02 | synchronized command queue, ordering, frame stamps, and dispatch | MODE_ACTIVE | BOUNDED | ANCHORED | SCAFFOLD | DRIFT | NET-QUEUE |
| GSI-16.03 | lockstep frame pacing, maximum-ahead window, latency, and stalls | MODE_ACTIVE | BOUNDED | ANCHORED | SCAFFOLD | DRIFT | NET-PACING |
| GSI-16.04 | scenario seed, options, house/slot, map, and content handshake | MODE_ACTIVE | DISCOVERED | ANCHORED | ABSENT | DRIFT | NET-HANDSHAKE |
| GSI-16.05 | connection objects, packet framing, send/receive queues, and retry | MODE_ACTIVE | BOUNDED | ANCHORED | ABSENT | DRIFT | NET-CONNECTION |
| GSI-16.06 | LAN transport, discovery, addressing, and Winsock/IPX-facing services | MODE_ACTIVE | BOUNDED | ANCHORED | ABSENT | DRIFT | NET-LAN |
| GSI-16.07 | lobby/session membership, ready state, teams, colors, and launch | MODE_ACTIVE | BOUNDED | ANCHORED | ABSENT | DRIFT | NET-LOBBY |
| GSI-16.08 | map negotiation, scenario transfer, file transfer, and progress | MODE_ACTIVE | DISCOVERED | UNCHECKED | ABSENT | DRIFT | NET-FILE |
| GSI-16.09 | Westwood Online login, chat, game listing, quick match, and WDT services | MODE_ACTIVE | BOUNDED | ANCHORED | ABSENT | DRIFT | NET-WOL |
| GSI-16.10 | multiplayer chat, beacons, taunts, alliances, and player messages | MODE_ACTIVE | DISCOVERED | ANCHORED | SCAFFOLD | DRIFT | NET-MESSAGES |
| GSI-16.11 | checksums, synchronization checks, desync detection, and diagnostics | MODE_ACTIVE | BOUNDED | ANCHORED | SCAFFOLD | DRIFT | NET-DESYNC |
| GSI-16.12 | reconnect/reestablish, timeout, drop, abort, and player removal | MODE_ACTIVE | BOUNDED | ANCHORED | ABSENT | DRIFT | NET-RECONNECT |
| GSI-16.13 | observer/spectator data policy and score distribution | GROUP_NODE | GROUP_NODE | N/A | N/A | N/A | MIXED-SCOPE |
| GSI-16.14 | legacy modem, serial, null-modem, and phonebook transports | UNKNOWN | BOUNDED | ANCHORED | ABSENT | UNCHECKED | NET-LEGACY |

### GSI-17 — Persistence, restoration, and evidence

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-17.01 | scenario/map load pipeline and object construction order | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-17.02 | save-game object serialization and class/version contracts | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | SAVE-FORMAT |
| GSI-17.03 | pointer/reference swizzling, object identity, and fixup tables | STOCK_ACTIVE | BOUNDED | ANCHORED | PARTIAL | DRIFT | SAVE-SWIZZLE |
| GSI-17.04 | load restoration, global re-registration, post-load fixups, and resume | STOCK_ACTIVE | BOUNDED | ANCHORED | PRESENT | DRIFT | SAVE-RESTORE |
| GSI-17.05 | campaign progress/carryover persistence | MODE_ACTIVE | DISCOVERED | UNCHECKED | PARTIAL | UNCHECKED | INV+FAMILY_SCAN |
| GSI-17.06 | user settings, hotkeys, profile, and shell-state persistence | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PRESENT | UNCHECKED | SETTINGS-CURRENT |
| GSI-17.07 | native replay recording/header, scenario relaunch, per-frame playback, sync/selection/cursor records, and replay availability flags | CONTENT_CONDITIONAL | BOUNDED | ANCHORED | PARTIAL | DRIFT | LIVE-REPLAY |
| GSI-17.08 | debug logs, assertions, exception/crash reporting, and diagnostic dumps | MODE_ACTIVE | DISCOVERED | UNCHECKED | PRESENT | UNCHECKED | DIAGNOSTICS |
| GSI-17.09 | screenshot/capture and diagnostic visual output | STOCK_ACTIVE | DISCOVERED | UNCHECKED | PRESENT | UNCHECKED | SCREENSHOT |

### GSI-18 — Legacy, disabled, and tooling

| ID | System | Activity | Inventory | Native | Rust | Parity | Basis |
|---|---|---|---|---|---|---|---|
| GSI-18.01 | veins/veinhole terrain, growth, damage, and monsters | COMPILED_INACTIVE | BOUNDED | ANCHORED | ABSENT | UNCHECKED | LEGACY-VEINS |
| GSI-18.02 | Firestorm wall and laser-fence style inherited mechanics | UNKNOWN | BOUNDED | ANCHORED | SCAFFOLD | UNCHECKED | LEGACY-FIRESTORM |
| GSI-18.03 | EMPulse state/application and EMP superweapon path | COMPILED_INACTIVE | BOUNDED | ANCHORED | SCAFFOLD | UNCHECKED | LEGACY-EMP |
| GSI-18.04 | Ion Cannon blast/superweapon path | COMPILED_INACTIVE | BOUNDED | ANCHORED | SCAFFOLD | UNCHECKED | LEGACY-ION |
| GSI-18.05 | Hunter Seeker inherited strategic path | UNKNOWN | DISCOVERED | UNCHECKED | ABSENT | UNCHECKED | INV+FAMILY_SCAN |
| GSI-18.06 | Chemical Missile inherited strategic path | UNKNOWN | DISCOVERED | UNCHECKED | ABSENT | UNCHECKED | INV+FAMILY_SCAN |
| GSI-18.07 | map/scenario editor-facing metadata and editor-only behavior | UNKNOWN | DISCOVERED | UNCHECKED | ABSENT | UNCHECKED | INV+FAMILY_SCAN |
| GSI-18.08 | cheats, debug keys, developer overlays, and developer control modes | UNKNOWN | BOUNDED | ANCHORED | PRESENT | UNCHECKED | DEBUG-TOOLS |

## Implementation-safe facts

These facts are sufficiently grounded to guide narrower implementation contracts; they do not certify their whole matrix rows:

- The active scenario/global scheduler is an ordered native mechanism, not an unordered bag of systems. The mapped `LogicClass`/scenario spine and current staged Rust schedule differ, so GSI-01.05 is `DRIFT`.
- Scenario RNG, main gameplay RNG, and random-map RNG are distinct streams with distinct ownership. Matching an algorithm without matching every consumer and draw order is not parity.
- `Mission_Dispatch @ 0x005B3060` is reached from `TechnoClass::AI_Update @ 0x006F9E50`. Its switch has no case for mission 29, while the current Rust mission router remains a coarse partial owner; GSI-07.04 is `DRIFT` and GSI-07.34 remains `UNCHECKED` rather than certified.
- Native replay playback is a real conditional system. `Main_Game @ 0x0052D9A0` tests the replay flags, reads replay startup data, and relaunches scenario state. The current Rust JSON replay runner is not the same mechanism, so GSI-17.07 is `DRIFT`.
- Standard retail defaults keep optional TS-style fog and several inherited environment systems off. Those rows must not inflate the active-stock parity denominator.
- An active system absent from Rust is a `DRIFT` even when the exact native internals are not fully contracted. This is why missing active campaign/LAN/WOL/network launch paths and several special gameplay managers are surfaced explicitly.

## Doc-patch-ready facts and stale current-Rust claims

Current source at the recorded commit supersedes older current-Rust absence/stub statements in some research prose:

- Random-map generation is substantial under `src/map/rmg/` and is launched from `src/app_init.rs`; GSI-03.12 is `PARTIAL/UNCHECKED`, not absent.
- Smudge parsing, state, combat dispatch, persistence, and draw integration now exist under `src/rules/smudge_type.rs`, `src/sim/smudge_grid.rs`, `src/sim/combat/smudge_dispatch.rs`, and render/app modules; GSI-04.11 is `PARTIAL/UNCHECKED`, not absent.
- Height-based `RevealByHeight` logic, mirror-table sampling, and tests exist in `src/sim/vision/`; GSI-12.03 is `PRESENT/UNCHECKED`, not absent.
- IFV/Gunner passenger weapon selection exists in `src/sim/passenger.rs` and combat weapon selection; GSI-07.44 is `PRESENT/UNCHECKED`, not absent.
- These corrections update only the Rust-state column. They do not turn the corresponding systems into parity passes.

The older title `GAMEMD_ARCHITECTURE.md` "Complete Architecture Map" must also not be read as an exhaustive or parity-certified denominator. The parent inventory already records it as never audited.

## Conflicts and uncertainty

- Research documents are strongest for native facts at their verified anchors, but their embedded current-Rust audits can age quickly. This matrix gives the current source snapshot precedence for the Rust-state column.
- Documents named `COMPLETE` may close a native slice while leaving Rust comparison, executable equivalence, or the broader matrix row open.
- `CONTRACTED` describes native evidence depth, not implementation completion. `PRESENT` describes structural Rust ownership, not exact mechanism.
- The default `INV+FAMILY_SCAN` classification is intentionally conservative. It is a navigation baseline and must be replaced by a focused row audit before implementation or parity claims.
- No hand-computed golden, Rust-vs-Rust hash, or prior prose status was accepted as a native parity oracle.

## Needs reinvestigation

The highest-leverage next evidence work is:

1. Split the **77 group nodes**, especially GSI-10 AI/teams/scripts/triggers, where 15 of 16 rows are currently unscorable.
2. Close the **104 native-`UNCHECKED` atomic rows** and convert the **106 merely discovered rows** into bounded systems or remove false candidates.
3. Audit the remaining mission handlers separately. The inventory has row-level mission IDs, but most are still `UNCHECKED`.
4. Build retail-derived executable traces/oracles for scheduler order, RNG consumption, pathing, combat, production, shroud, input, render pixels, audio cadence, save/load, and replay.
5. Re-audit current Rust after recent implementation waves in smudges, RMG, visibility, IFV, radiation, movement, production, and other fast-moving areas.
6. Resolve stock reachability for the seven `UNKNOWN` atomic rows before assigning active parity consequences.

## Do not implement as active-stock parity work yet

- GSI-04.19 optional fog-cell timers/regrowth unless an explicitly enabled mode is targeted; retail `FogOfWar=no` is the default.
- GSI-06.21 through GSI-06.23 Mech, DropPod, and subterranean locomotion as stock-active behavior; current evidence classifies them as compiled-inactive in retail YR. Active low-bridge TubeClass work belongs to GSI-04.15 instead.
- GSI-18.01 veins, GSI-18.03 EMP, and GSI-18.04 Ion Cannon as active retail systems without new reachability evidence.
- GSI-18.02, GSI-18.05, and GSI-18.06 inherited strategic mechanics until their stock reachability is resolved.
- Editor/debug/tooling rows as gameplay parity requirements unless the target scope is explicitly expanded.

## Source ledger

| Evidence area | Primary inputs used |
|---|---|
| Stable inventory and activity seed | `GAMEMD_SYSTEM_INVENTORY_COVERAGE_MAP_GHIDRA_REPORT.md`; current inventory row scopes |
| Live mission spot check | live decompile of `Mission_Dispatch @ 0x005B3060`; caller check to `TechnoClass::AI_Update @ 0x006F9E50` |
| Live replay spot check | live decompile of `Main_Game @ 0x0052D9A0`; replay flag/startup branches |
| Runtime/RNG/timers | [LOGICCLASS_GLOBAL_SUBSYSTEM_ORDER_0055AFB0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_GLOBAL_SUBSYSTEM_ORDER_0055AFB0_GHIDRA_REPORT.md); `ADVANCE_TICK_PHASE_PARTITION_NATIVE_SPINE_GHIDRA_REPORT.md`; [RANDOM_SCENARIO_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RANDOM_SCENARIO_ENGINE_SUBSTRATE_SERVICE_STUDY.md); timer-family reports |
| Assets/configuration | MIX/VFS frontier report; [CCINICLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CCINICLASS_GHIDRA_REPORT.md); SHP/TMP/VXL/HVA/PAL/PCX reports |
| Shell/modes/RMG | `docs/research/skirmish-ui/` reports; current `src/ui/`, `src/app.rs`, `src/app_init.rs`, and `src/map/rmg/` |
| World/cells/bridges/smudges | CellClass, zone, tiberium, terrain, bridge/tube, shroud, and smudge reports; current map/sim/render owners |
| Lifecycle/movement | active-object, deferred-delete, Bullet/Anim/Particle/VoxelAnim, A*, zone, reservation, and locomotor reports |
| Missions/radio/transport | MissionClass verb/dispatcher/Rescue reports; [RADIO_SYSTEM_MODEL_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADIO_SYSTEM_MODEL_SYNTHESIS.md); IFV/open-topped report; current mission/passenger code |
| Combat/economy | prism, gattling, sonic, EBolt, Magnetron, radiation, harvest, tiberium, power, factory, and MCV reports; current `src/sim/` owners |
| Superweapons/visibility | missing-superweapon and per-superweapon reports; shroud, cloak, sensors, gap, radar, and visibility syntheses |
| Rendering/input/UI | render coupling, GScreen/Tactical, draw-order/depth, VXL, transient-effect, selection, hotkey, action, autoscroll, sidebar, and minimap reports |
| Audio/video | audio channel/spatial/backend/EVA reports and current `src/audio/`; Bink/VQA shell evidence |
| Network/persistence | network frontier reports; hotkey/event evidence; save serialization/swizzle/restore reports; replay reports; current `src/net/` and persistence code |
| Retail defaults | `ini/rulesmd.ini` (`IonStorms=no`, `FogOfWar=no`, `Visceroids=no`, `Meteorites=no`, `Crates=yes`, active `[SuperWeaponTypes]`); `ini/mpmodesmd.ini` mode definitions |
| Current Rust snapshot | direct source scan at commit `a97ce88454d2ab938e6f8892dcac861845302c09`; no Rust files changed |

## Validation

- Mechanical matrix validation found **336 inventory IDs and 336 matrix rows**, all unique, with zero missing IDs, zero extra IDs, and zero invalid controlled-status values.
- Rebuilding the repo-local research index produced **2,974 documents and 69,326 chunks**.
- Focused `research_validate` checked this document and returned `valid=true`, with zero missing files, checksum mismatches, missing links, or stale/unknown status flags.
- The research graph resolves this file as `source=synthesis`, `status=synthesis`, and indexes the live mission/replay addresses plus current Rust paths cited above.
- No Cargo/build/test run was needed because this task changed documentation only.

## Open questions

| Question | Classification |
|---|---|
| Is the 336-row registry exhaustive? | `OPEN`: no; unassigned binary functions, stale/unknown corpus entries, mixed nodes, and content-specific mechanisms remain. |
| Can a whole-game completion percentage be computed now? | `RESOLVED`: no defensible percentage until group nodes are split and active atomic rows have bounded contracts. |
| Is any broad row parity-certified? | `RESOLVED`: no; zero rows meet the native-oracle/exhaustive-proof burden. |
| Are all 115 drifts equally urgent? | `RESOLVED`: no; parity verdict and fix priority are separate. Trigger frequency/player visibility can rank work but cannot erase drift. |
| Which rows should be audited next? | `DEFERRED TO PRIORITIZATION`: use the group-node, native-unchecked, and active-drift queues above. |

## Overall safety assessment

**Whole-game implementation/parity status: investigation-blocked.** The matrix is safe as a work registry and triage baseline. It is not safe as evidence that an unchecked row matches, that a present Rust owner is complete, or that the game is any numeric percentage finished. Narrow rows with `CONTRACTED` native evidence and explicit `DRIFT` can proceed to implementation-contract work; all others require additional synthesis, tracing, or reverse engineering first.
