# Yuri's Revenge `gamemd.exe` System Inventory Coverage Map

**Date:** 2026-07-20  
**Target:** active retail Yuri's Revenge `gamemd.exe` loaded in the local Ghidra project  
**Document kind:** broad discovery inventory / coverage map  
**Status:** useful baseline, **not an exhaustive inventory and not a parity certification**

## Verdict

Yes: maintaining a stable list of the systems that make up the game is the right
first step. Without that denominator, statements such as "70% complete" or
"mostly parity-correct" have no defensible meaning.

This pass establishes a hierarchical registry that can be audited system by
system. It is broad enough to establish the initial denominator candidate, but
mixed/group nodes are not yet scorable and the registry is not proven complete.
In particular, thousands of non-custom binary functions and more
than a thousand stale-or-unknown indexed research/plan records remain to be assigned.
Consequently:

- no whole-game completion percentage is reported;
- no listed system is assumed parity-correct merely because Rust code exists;
- a missing item found later is added with a new stable ID rather than hidden;
- stock-active, mode-conditional, and compiled-but-dormant behavior must be
  classified separately before status rollups are attempted.

## Question and scope

The question for this pass is:

> What independently ownable systems and subsystems are presently discoverable
> from the research corpus, retail INI/assets surfaces, current Rust ownership,
> and a broad live-binary scan of `gamemd.exe`?

This is a discovery pass, not a leaf-by-leaf behavioral investigation. It does
not prove every vtable slot, unnamed function, mode transition, content-specific
special case, or edge state. It does not change Rust code.

## What counts as one system

A registry item is an independently auditable behavior owner with at least one
of these boundaries:

1. persistent state or lifecycle of its own;
2. a distinct algorithm or ordered state machine;
3. a protocol between subsystems;
4. a data-loading or asset-decoding contract;
5. an input, presentation, audio, persistence, or network contract whose bytes,
   timing, or pixels matter to parity.

An individual tank, weapon, warhead, projectile, sound, art section, or map is
normally content inside a system, not a separate system. It becomes a separate
registry item only when `gamemd.exe` gives it a distinct hardcoded mechanism.
Classes are evidence for ownership, not automatically systems; templates,
runtime wrappers, stale labels, and class fragments are not promoted on name
alone.

## Evidence and method

### Research corpus

The repo-local research index was rebuilt after the final classification pass.
The rebuilt index contains 2,972 documents and 69,242 chunks: 2,481 under
`docs/research/`, 464 under `docs/plans/`, and 27 INIs. For `docs/research/`, the
indexed source-kind totals were 1,351 Ghidra reports, 269 traces, 50 synthesis
documents, 14 audits, 14 plans, 2 contracts, and 781 documents whose kind was
not classified. Status totals were 1,639 `verified`, 50 `synthesis`, 14 `plan`,
2 `stale`, and 776 `unknown` within the `docs/research/` subset.

Index validation found no missing indexed file and no checksum mismatch after
the rebuild, but the corpus is not fully healthy: the validator still reports
broken local links and 1,014 stale-or-unknown records across indexed research
and plan inputs. An indexed or
`VERIFIED`-headed document is navigation evidence; it is not automatically a
current whole-document binary audit.

The strongest current cross-system backbone is
`CORE_ENGINE_SERVICES_MAP.md`, which maps 41 services (18 studied services and
23 frontier profiles). It is intentionally an architecture/service map, not a
complete inventory of game features, modes, content mechanics, and platform
services. `GAMEMD_ARCHITECTURE.md` is broader, but its "Complete Architecture
Map" title is not a certification: the audit index marks it `NEVER_AUDITED`, and
some of its labels and counts predate later corrections.

### Retail data surfaces

Twenty-seven INI files under `ini/` were enumerated. The largest files expose
the following raw section/key-row surfaces before base-plus-`*md` merge:

| File | Sections | Key rows |
|---|---:|---:|
| `rulesmd.ini` | 1,477 | 23,390 |
| `rules.ini` | 1,190 | 17,476 |
| `artmd.ini` | 1,582 | 13,767 |
| `art.ini` | 1,300 | 10,342 |
| `aimd.ini` | 388 | 6,625 |
| `ai.ini` | 241 | 4,020 |
| `soundmd.ini` | 821 | 3,797 |
| `sound.ini` | 501 | 2,356 |
| `evamd.ini` | 486 | 2,106 |
| `eva.ini` | 357 | 1,545 |

The remaining files cover theater data, missions/campaigns, battles, themes,
random-map generation, and multiplayer modes. `*md` data patches the base data;
absence from an `*md` file is not evidence that the base behavior is absent.
Parser keys and commented retail sections prove a compiled or data-facing
surface, not stock-YR reachability.

### Live `gamemd.exe` discovery scan

The local Ghidra program reports a 32-bit x86 PE at image base `0x00400000`,
with 10,035 recovered functions, 40,355 symbols, and 966 data types. These are
Ghidra-project counts, not original-source completeness metrics.

The discovery scan used these read-only Ghidra queries on 2026-07-20:

- `get_program_info()` for program metadata;
- `list_classes()` for the 1,366 recovered class/namespace entries;
- `search_functions_enhanced(has_custom_name=true)` for 3,033 custom-named
  functions, subsequently classified by normalized owner/name prefix;
- `batch_string_anchor_report(pattern=".CPP")` for 69 embedded source-file
  anchors;
- decompile and caller checks at `0x0055AFB0` and `0x0048CCC0` as two runtime
  spine spot checks.

Templates, standard-library/runtime types, interface fragments, and obvious
generic wrappers were filtered from the class list. Function-family prefixes
were then used to recover systems that do not have a distinctive class owner,
including building placement, audio codecs, crate/strategic effects, shell
services, and online services. Labels were treated as navigation hints only.

The `0x0055AFB0` body confirms that the scenario tick has an explicit global
order spanning scenario/cell work, shroud/fog gates, bridges, ore growth/spread,
bombs, teams, visual/status managers, the active object vector, factories,
houses, and final-reference cleanup. Its live caller is the main tick at
`0x0055D360`. The `0x0048CCC0` outer loop confirms a distinct scenario
initialization, run, state-machine, and teardown lifecycle. A label collision at
the scenario-loader call demonstrates why current Ghidra names are not accepted
as proof without body and caller checks. These two checks validate the top-level
spine only; they do not validate every registry node below.

A replay/recording spot check was also used to resolve an inventory ambiguity.
`search_strings("(?i)record")` found the active `-record` option and recording
log strings. The existing exhaustive-slice replay report verifies conditional
recording (`DAT_00A8D5F8 & 1`) and playback (`& 2`) through `Main_Game @
0x0052D9A0` and `Main_Tick @ 0x0055D360`. This proves a native replay system;
it does not prove the complete recording format or every event type.

### Current Rust scan

Rust ownership was mapped independently from parity. The principal top-level
surfaces are `assets/`, `rules/`, `map/`, `sim/`, `render/`, `sidebar/`, `ui/`,
`audio/`, `net/`, and the app orchestration modules. Presence means only that a
Rust owner exists. It does not mean that the owner has the same mechanism,
ordering, state bytes, RNG use, timing, audio, or pixels as `gamemd.exe`.

### Specialist-report reconciliation

An adversarial review against narrower reports changed the first draft in
material ways:

- House/player authority is a canonical owner, not merely an AI/economy detail.
  The live name scan has 114 `HouseClass` functions; specialist evidence anchors
  `HouseClass::AI @ 0x004F8440` and alliance mutation at `0x004F9B70`.
- Match-start policy is distinct from setup UI and network handshake. The
  skirmish option reports anchor packing at `0x006ACEE0`, scenario full init at
  `0x00686B20`, house creation at `0x00687F10`, and post-map initialization at
  `0x00686890`.
- `MissionClass` exposes 32 mission rows with different handler/reachability
  outcomes through dispatch `0x005B3060`; they cannot share one parity status.
- Mech, DropPod, and Tunnel locomotors are compiled but dormant under stock YR
  data according to [TS_DORMANT_LOCOMOTORS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TS_DORMANT_LOCOMOTORS_GHIDRA_REPORT.md).
- Magnetron locomotor hijack (`0x004690B0`, `0x00710000`) and IFV/Gunner weapon
  selection (`0x0070DC70`) are independent stock mechanisms.
- EBolt, LaserDraw, DiskLaser, WaveClass, AlphaShape, gattling, prism support,
  and RadSite have distinct state/update owners and therefore need separate
  audit rows instead of one "weapon effects" bucket.
- Audio has both logical cue systems and a platform output cadence: the core
  service map anchors `SoundSystem__UpdateTick @ 0x004041D0` and the separate
  theme stream pump at `0x00406F70`.

## Stable-ID candidate system registry

This pass records **336 candidate nodes across 18 families**. That is a discovery
count, not a scored denominator: mixed/group nodes must be split, and later
discoveries may add new IDs. All current IDs are unique and contiguous within
their family.

These IDs are the initial canonical names for future audits. A slash in a name
means one tightly coupled contract, not evidence that its parts are identical.
The `Discovery scope` column is a provisional routing clue, not a controlled
status field. Items marked **mixed** contain stock-active and conditional
variants that still need child-node reachability classification. Items marked
**unknown** were discovered but have not had stock reachability established in
this pass. The controlled reachability vocabulary for the next phase is defined
under the status model.

### GSI-01 — Runtime, platform, and global execution

| ID | System | Discovery scope |
|---|---|---|
| GSI-01.01 | executable bootstrap, process environment, and startup checks | stock/platform |
| GSI-01.02 | window creation, Windows message pump, activation, and focus | stock/platform |
| GSI-01.03 | top-level shell/game state machine | stock |
| GSI-01.04 | scenario initialization, start, exit, and teardown lifecycle | stock |
| GSI-01.05 | deterministic per-tick global scheduler and rung order | stock |
| GSI-01.06 | clocks, frame pacing, game speed, pause, and modal pumping | mixed |
| GSI-01.07 | scenario-owned deterministic RNG stream (`ScenarioClass+0x218`) | stock |
| GSI-01.08 | main/global deterministic gameplay RNG (`g_MainRng`) | stock |
| GSI-01.09 | separately seeded random-map-generation RNG (`g_MapGenRng`) | stock/mode |
| GSI-01.10 | timers, countdowns, delays, and cadence conversion | stock |
| GSI-01.11 | coordinates, cells/leptons/pixels, facing, fixed math, and lookup tables | stock |
| GSI-01.12 | allocation, object pools, vectors, reference tracking, and final cleanup | stock/platform |
| GSI-01.13 | CD/install/registry/path discovery and retail media checks | stock/platform |
| GSI-01.14 | localization/code-page/platform string services | stock/platform |

### GSI-02 — Files, configuration, type data, and asset decoding

| ID | System | Discovery scope |
|---|---|---|
| GSI-02.01 | virtual file system and MIX archive search/precedence | stock |
| GSI-02.02 | loose-file, language-pack, theater-pack, and map-pack resolution | stock |
| GSI-02.03 | INI lexical parsing, defaults, inheritance, and base/`*md` overlay | stock |
| GSI-02.04 | rules globals and object/type registries | stock |
| GSI-02.05 | house, side, country, color, and ownership data | stock |
| GSI-02.06 | AI TaskForce/Script/Team/AITrigger data loading | mixed/mode |
| GSI-02.07 | art/type-image metadata and animation declarations | stock |
| GSI-02.08 | theater metadata, tilesets, LAT/ramp/morph tables | stock |
| GSI-02.09 | scenario/map INI and packed-section decoding | stock |
| GSI-02.10 | SHP sprite decoding and frame metadata | stock |
| GSI-02.11 | TMP tile decoding, subtile geometry, and extra-data planes | stock |
| GSI-02.12 | VXL/HVA/VPL voxel data, transforms, and lighting tables | stock |
| GSI-02.13 | palettes, remap tables, color conversion, and translucency tables | stock |
| GSI-02.14 | CSF strings, fonts, text layout inputs, and UI string lookup | stock |
| GSI-02.15 | audio index/bag/AUD/VOC/WAV decoding and sample lookup | stock |
| GSI-02.16 | PCX/images, compression codecs, and packed-data helpers | stock |
| GSI-02.17 | Bink/VQA/cinematic media discovery and decoding interface | mixed/mode |

### GSI-03 — Shell, setup, loading, and game modes

| ID | System | Discovery scope |
|---|---|---|
| GSI-03.01 | main-menu composition and shell transitions | stock |
| GSI-03.02 | options, hotkeys, display/audio settings, quit confirmation | stock |
| GSI-03.03 | single-player campaign catalog and side/difficulty selection | stock/mode |
| GSI-03.04 | campaign progression, scenario mapping, carryover, and unlock state | stock/mode |
| GSI-03.05 | mission selection, briefing, restate, and objective presentation | stock/mode |
| GSI-03.06 | victory/defeat transition, score screen, and final results | mixed/mode |
| GSI-03.07 | movies, sneak preview, credits, and final-movie selection | stock/mode |
| GSI-03.08 | load/save shell dialogs and slot metadata | stock |
| GSI-03.09 | scenario loading screen, progress manager, and transition art | stock |
| GSI-03.10 | skirmish setup, player slots, factions, teams, colors, and options | stock/mode |
| GSI-03.11 | map browser, filters, preview, metadata, and start positions | mixed/mode |
| GSI-03.12 | random-map generator configuration, generation, and preview | stock/mode |
| GSI-03.13 | multiplayer mode catalog: battle, co-op, siege, team, world domination | mixed/mode |
| GSI-03.14 | LAN session discovery, host/join setup, and lobby shell | stock/mode |
| GSI-03.15 | Westwood Online account/chat/game/download shell | stock/mode |
| GSI-03.16 | observer setup, multiplayer score, and post-game flow | mixed/mode |
| GSI-03.17 | session/mode policy, packed options, house/start generation, and runtime match-start handoff | mixed/mode |

### GSI-04 — World, map, terrain, and environment

| ID | System | Discovery scope |
|---|---|---|
| GSI-04.01 | map dimensions, cell grid, playable bounds, and coordinate lookup | stock |
| GSI-04.02 | theater selection, tile placement, and isometric ground geometry | stock |
| GSI-04.03 | elevation, ramps, cliffs, slopes, and height conversion | stock |
| GSI-04.04 | cell-owned land type, movement-zone labels, and passability state | stock |
| GSI-04.05 | cell occupancy, object-content lists, layers, and entry reservations | stock |
| GSI-04.06 | zone/subzone grid state and connectivity topology | stock |
| GSI-04.07 | overlay placement, ownership, damage, and removal | stock |
| GSI-04.08 | walls, gates, fences, pavement, and buildable overlays | mixed |
| GSI-04.09 | ore/gems/tiberium overlay identity, placement, and per-cell amount state | stock |
| GSI-04.10 | terrain objects: trees, rocks, flammability, crush, and destruction | stock |
| GSI-04.11 | smudges, craters, scorch marks, and persistence | stock |
| GSI-04.12 | high-bridge topology, occupancy, and traversal | stock |
| GSI-04.13 | low/water bridge topology, decks, ramps, and traversal | stock |
| GSI-04.14 | bridge damage, collapse, debris, repair, and control huts | stock |
| GSI-04.15 | low-bridge tubes/tunnels and endpoint movement | stock |
| GSI-04.16 | waypoints, player starts, regions, and scenario navigation anchors | stock |
| GSI-04.17 | tags, cell tags, local/global variables, and map flags | mixed/mode |
| GSI-04.18 | cell-owned unexplored-shroud counters/bits and persisted map knowledge | stock |
| GSI-04.19 | optional fog-cell storage, concealment timers, and regrowth gates | conditional/legacy |
| GSI-04.20 | ambient lighting, global tint, light sources, and day/night transitions | mixed |
| GSI-04.21 | radiation sites, cell hazards, fire, and environmental damage | stock/mixed |
| GSI-04.22 | weather/ambient environmental events and map ambience | mixed/unknown |
| GSI-04.23 | crates: placement, timers, pickup, contents, and powerups | mixed/mode |

### GSI-05 — Entity model, ownership, and lifecycle

| ID | System | Discovery scope |
|---|---|---|
| GSI-05.01 | type-instance registration and stable object identity | stock |
| GSI-05.02 | active-object vector membership and deterministic iteration | stock |
| GSI-05.03 | create, reveal, conceal, limbo, unlimbo, uninit, and delete lifecycle | stock |
| GSI-05.04 | target/reference notices, expiration, detach, and final-reference handling | stock |
| GSI-05.05 | Abstract/Object base state and spatial identity | stock |
| GSI-05.06 | Mission/Radio/Techno/Foot behavioral spine | stock |
| GSI-05.07 | infantry instances, stances, sequences, and occupation | stock |
| GSI-05.08 | vehicle/naval unit instances and unit-specific state | stock |
| GSI-05.09 | aircraft instances, flight state, airports, and airborne identity | stock |
| GSI-05.10 | building instances, foundations, upgrades, occupants, and animation state | stock |
| GSI-05.11 | bullet/projectile instances and target references | stock |
| GSI-05.12 | animation instances and attached animation ownership | stock |
| GSI-05.13 | particle and particle-system instances | stock |
| GSI-05.14 | voxel-animation, debris, and falling-object instances | stock |
| GSI-05.15 | terrain/overlay/smudge instance lifecycles | stock |
| GSI-05.16 | House authority: identity/control, diplomacy/alliance, owned registries, defeat/winner flags, and statistics | mixed |
| GSI-05.17 | Factory runtime identity, house registration, reference ownership, and lifecycle only | stock |
| GSI-05.18 | Team runtime identity, membership/reference state, and lifecycle only | mixed/mode |
| GSI-05.19 | Trigger/Tag runtime identity, cross-references, persistence, and lifecycle only | mixed/mode |
| GSI-05.20 | Super runtime identity, house registration, charge-state ownership, and lifecycle only | mixed |
| GSI-05.21 | shared attached-manager registration, reference detach, and lifecycle infrastructure only | mixed |

### GSI-06 — Navigation, locomotion, and physical movement

| ID | System | Discovery scope |
|---|---|---|
| GSI-06.01 | movement request admission, destination choice, and cell-entry gates | stock |
| GSI-06.02 | queries over zone topology for reachable destinations and admission decisions | stock |
| GSI-06.03 | path search, open/closed state, tie-breaking, and path reconstruction | stock |
| GSI-06.04 | locomotor consumption of cell state: effective terrain cost, speed type, and modifiers | stock |
| GSI-06.05 | path smoothing, retries, fallback, and blocked-path recovery | stock |
| GSI-06.06 | path queueing, reservations, traffic arbitration, and same-tick commits | stock |
| GSI-06.07 | occupancy enter/leave commits and bridge/layer transitions | stock |
| GSI-06.08 | collision, scatter, pushing, bumping, crushing, and overlap recovery | stock |
| GSI-06.09 | FootClass convoy chain, follower links, spacing, and persistent cohesion state | mixed |
| GSI-06.10 | TeamClass AI formation/group movement and team-level coordination | mixed/mode |
| GSI-06.11 | facing, rotation, drive tracks, curves, acceleration, and braking | stock |
| GSI-06.12 | locomotor dispatch, link ownership, piggyback infrastructure, and authority handoff | stock |
| GSI-06.13 | Drive locomotion | stock |
| GSI-06.14 | Walk locomotion | stock |
| GSI-06.15 | Ship locomotion | stock |
| GSI-06.16 | Fly locomotion | stock |
| GSI-06.17 | Hover locomotion | stock |
| GSI-06.18 | Jumpjet locomotion | stock |
| GSI-06.19 | Rocket locomotion | stock |
| GSI-06.20 | Teleport locomotion | stock |
| GSI-06.21 | Mech locomotion | dormant/legacy in stock YR |
| GSI-06.22 | DropPod locomotion | dormant/legacy in stock YR |
| GSI-06.23 | Tunnel/subterranean locomotion, distinct from active bridge tubes | dormant/legacy in stock YR |
| GSI-06.24 | air takeoff, landing, altitude, circling, and airport approach | stock |

### GSI-07 — Player orders, missions, radio, docking, and transport

| ID | System | Discovery scope |
|---|---|---|
| GSI-07.01 | command admission, ownership validation, and order replacement | stock |
| GSI-07.02 | 32-row mission-control metadata, rates, flags, and name lookup | stock/mixed rows |
| GSI-07.03 | mission verb API: assign, queue, override, suspend, restore, and guard rules | stock |
| GSI-07.04 | mission dispatcher, current/queued/substate fields, timer rewrite, and vtable routing | stock |
| GSI-07.05 | Mission 0: Sleep handler and idle cadence | stock |
| GSI-07.06 | Mission 1: Attack handler | stock |
| GSI-07.07 | Mission 2: Move handler | stock |
| GSI-07.08 | Mission 3: QMove selector and Sleep-handler fallback | stock |
| GSI-07.09 | Mission 4: Retreat handler | stock |
| GSI-07.10 | Mission 5: Guard handler | stock |
| GSI-07.11 | Mission 6: Sticky selector and Guard-handler routing | stock |
| GSI-07.12 | Mission 7: Enter handler | stock |
| GSI-07.13 | Mission 8: Capture handler | stock |
| GSI-07.14 | Mission 9: Eaten handler/row | legacy/unchecked |
| GSI-07.15 | Mission 10: Harvest handler | stock |
| GSI-07.16 | Mission 11: Area Guard handler | stock |
| GSI-07.17 | Mission 12: Return handler | stock |
| GSI-07.18 | Mission 13: Stop handler | stock |
| GSI-07.19 | Mission 14: Ambush dead TS stub | dormant/legacy |
| GSI-07.20 | Mission 15: Hunt handler | stock |
| GSI-07.21 | Mission 16: Unload handler | stock |
| GSI-07.22 | Mission 17: Sabotage selector and Capture-slot routing | stock/mixed |
| GSI-07.23 | Mission 18: Construction handler | stock |
| GSI-07.24 | Mission 19: Selling handler | stock |
| GSI-07.25 | Mission 20: Repair handler | stock |
| GSI-07.26 | Mission 21: Rescue handler and AI-only assignment path | stock/AI-only |
| GSI-07.27 | Mission 22: Missile handler | stock/mixed |
| GSI-07.28 | Mission 23: Harmless handler | stock/mixed |
| GSI-07.29 | Mission 24: Open handler | stock |
| GSI-07.30 | Mission 25: Patrol handler | stock/mixed |
| GSI-07.31 | Mission 26: Paradrop Approach handler | stock/conditional |
| GSI-07.32 | Mission 27: Paradrop Overfly handler | stock/conditional |
| GSI-07.33 | Mission 28: Wait/Deliberate handler | stock/mixed |
| GSI-07.34 | Mission 29: Attack Move assign-side selector with no dispatcher case | stock selector/non-dispatched |
| GSI-07.35 | Mission 30: Spyplane Approach handler | stock/conditional |
| GSI-07.36 | Mission 31: Spyplane Overfly handler | stock/conditional |
| GSI-07.37 | Radio contact protocol, link negotiation, messages, and teardown | stock |
| GSI-07.38 | generic docking reservations, queues, and authority handoff | stock |
| GSI-07.39 | refinery docking, ore transfer, credit display, and release | stock |
| GSI-07.40 | aircraft docking, pad choice, landing, rearm, and release | stock |
| GSI-07.41 | factory exit, spawn cell, rally point, and blocked-exit recovery | stock |
| GSI-07.42 | cargo/passenger load, unload, capacity, and transporter destruction | stock |
| GSI-07.43 | open-topped passenger fire, garrison, bunker, and occupant coordination | mixed |
| GSI-07.44 | IFV/Gunner passenger-dependent weapon slot, cached pointer, and turret-variant selection | stock |
| GSI-07.45 | gate opening/closing protocol and linked traversal | stock |

### GSI-08 — Combat, weapons, damage, and status mechanics

| ID | System | Discovery scope |
|---|---|---|
| GSI-08.01 | target legality, acquisition, threat scoring, and opportunity selection | stock |
| GSI-08.02 | weapon selection, primary/secondary/elite choice, and target filters | stock |
| GSI-08.03 | fire gates, reload readiness, ammo, power, transport, and mission gates | stock |
| GSI-08.04 | range, line of fire, fire location/FLH, facing, and fire-error calculation | stock |
| GSI-08.05 | ROF, burst, distributed/radial fire, rearm, and veterancy modifiers | stock |
| GSI-08.06 | projectile creation, source/target bookkeeping, and launch side effects | stock |
| GSI-08.07 | ballistic, straight, arcing, homing, torpedo, and vertical flight | mixed |
| GSI-08.08 | projectile collision, proximity, fuse, interception, and detonation | mixed |
| GSI-08.09 | area damage, cell spread, distance falloff, and target collection | stock |
| GSI-08.10 | damage kernel, armor/Verses, clamps, immunities, and healing | stock |
| GSI-08.11 | death, destruction, kill credit, passengers/crew, debris, and explosions | stock |
| GSI-08.12 | veterancy, experience, promotion, elite weapons, and ability modifiers | stock |
| GSI-08.13 | infantry fear, prone/crawl, death sequences, and suppression-like state | stock |
| GSI-08.14 | body/turret/barrel facing, recoil, rocking, and firing animation state | stock |
| GSI-08.15 | air-to-air, anti-air, strafing, bombing, and airstrike control | stock |
| GSI-08.16 | garrison/open-topped/bunker firing and occupant damage routing | mixed |
| GSI-08.17 | crushing and crush-death combat consequences | stock |
| GSI-08.18 | C4, Ivan bombs, timed bombs, bridge charges, and disarm/cleanup | mixed |
| GSI-08.19 | prism support network, support targeting, charge contribution, and firing handoff | stock |
| GSI-08.20 | gattling stage, stage timer, weapon/turret selection, and reset | stock |
| GSI-08.21 | sonic weapon triple path: projectile damage, ambient path damage, and WaveClass handoff | stock |
| GSI-08.22 | Tesla electrical strike and EBolt creation/gameplay handoff | stock |
| GSI-08.23 | laser weapon fire/damage path and LaserDraw creation handoff | stock/mixed |
| GSI-08.24 | radiation-beam/eruption gameplay, radiation application, and visual handoff | stock/mixed |
| GSI-08.25 | Magnetron locomotor hijack, piggyback swap, lift, carry, drop, and landing damage | stock |
| GSI-08.26 | RadSite runtime manager, cell radiation decay/damage cadence, and emitter state | stock |
| GSI-08.27 | fire, chaos, berserk, poison-like, and other persistent status effects | mixed |
| GSI-08.28 | mind control, capture manager, psychic immunity, and overload | mixed |
| GSI-08.29 | temporal targeting, warp-out, erase, and release | mixed |
| GSI-08.30 | parasite attach/attack/exit and host interactions | mixed |
| GSI-08.31 | spawn manager, spawned aircraft, reload, launch, and recovery | mixed |
| GSI-08.32 | slave manager, slave work/respawn, and owner transitions | mixed |
| GSI-08.33 | warhead special flags, animations, sounds, terrain, bridge, and ore effects | mixed/group node |
| GSI-08.34 | crate/powerup combat modifiers and temporary bonuses | mixed/mode |

### GSI-09 — Economy, technology, construction, and production

| ID | System | Discovery scope |
|---|---|---|
| GSI-09.01 | credits, income/spending, displayed money, and transaction ordering | stock |
| GSI-09.02 | storage capacity, refinery storage, silo behavior, and resource loss | stock |
| GSI-09.03 | ore/gem value lookup, harvester capacity, collection, and unload conversion | stock |
| GSI-09.04 | resource growth/spread scheduling and map resource state | stock |
| GSI-09.05 | standard miner/harvester work-site selection and economy-side return decisions | stock |
| GSI-09.06 | slave miner deployment, slaves, grinding, and mobile refinery behavior | stock |
| GSI-09.07 | power production/drain, low power, blackout, and powered-state effects | stock |
| GSI-09.08 | tech tree, prerequisites, build limits, stolen tech, and availability | stock |
| GSI-09.09 | factory ownership, build queues, parallel production, and abandonment | stock |
| GSI-09.10 | build time, cost, difficulty/house modifiers, and production progress | stock |
| GSI-09.11 | placement legality, foundations, adjacency, buildable cells, and previews | stock |
| GSI-09.12 | building buildup, construction state, completion, and activation | stock |
| GSI-09.13 | service-facility eligibility, repair/rearm/hospital/armory effects, and costs | mixed |
| GSI-09.14 | sell, refund, occupants/crew, undeploy, and teardown consequences | stock |
| GSI-09.15 | capture/ownership transfer effects on power, tech, radar, and production | stock |
| GSI-09.16 | MCV deploy/undeploy and construction-yard authority | stock |
| GSI-09.17 | upgrades, powers-up-building, and building slot effects | stock |
| GSI-09.18 | Grinder intake, occupant destruction, soylent conversion, and release | stock |
| GSI-09.19 | Cloning Vat duplicate-production selection and free-clone creation | stock |
| GSI-09.20 | Bio Reactor occupant slots, power contribution, ejection, and destruction | stock |
| GSI-09.21 | Ore Purifier house-income modifier and ownership transitions | stock |

### GSI-10 — AI, teams, scripts, triggers, and outcomes

| ID | System | Discovery scope |
|---|---|---|
| GSI-10.01 | House AI brain, state, update cadence, and strategic priorities | mixed/mode |
| GSI-10.02 | base planning, build placement, defense zones, and rebuilding | mixed/mode |
| GSI-10.03 | AI economy/resource management and spending priorities | mixed/mode |
| GSI-10.04 | AI production choice, factory assignment, and build queues | mixed/mode |
| GSI-10.05 | threat maps, target scoring, defense response, and enemy selection | mixed/mode |
| GSI-10.06 | AITrigger eligibility, weights, selection, cooldowns, and team creation | mixed/mode |
| GSI-10.07 | TaskForce composition and member acquisition | mixed/mode |
| GSI-10.08 | Team formation, ownership, recruitment, state, and dissolution | mixed/mode |
| GSI-10.09 | ScriptType steps, arguments, branching, completion, and failure | mixed/mode |
| GSI-10.10 | scenario Trigger/Tag evaluation and persistence | mixed/mode |
| GSI-10.11 | Event predicates, occurrence counts, houses, timers, and variables | mixed/mode |
| GSI-10.12 | Action dispatch, parameters, side effects, and action ordering | mixed/mode |
| GSI-10.13 | campaign/mission scripting, objectives, reinforcements, and cinematics | stock/mode |
| GSI-10.14 | difficulty, IQ, handicap, aggression, and campaign modifiers | mixed/mode |
| GSI-10.15 | win/loss evaluation, end-delay, surrender, alliance, and scenario outcome | mixed |
| GSI-10.16 | scoring, kills/losses, economy statistics, ranking, and result aggregation | mixed/mode |

### GSI-11 — Superweapons and strategic special actions

| ID | System | Discovery scope |
|---|---|---|
| GSI-11.01 | generic superweapon ownership, charge, hold, readiness, targeting, and launch | mixed |
| GSI-11.02 | nuclear missile launch, flight, warning, detonation, and aftermath | stock |
| GSI-11.03 | Lightning Storm scheduling, weather, strikes, damage, and cleanup | stock |
| GSI-11.04 | Iron Curtain targeting, invulnerability, expiry, and feedback | stock |
| GSI-11.05 | Force Shield targeting, protection, power consequence, and expiry | stock |
| GSI-11.06 | Chronosphere/ChronoWarp selection, source/target areas, transport, and arrival | stock |
| GSI-11.07 | Psychic Dominator targeting, control transfer, damage, and visuals | stock |
| GSI-11.08 | Genetic Mutator targeting, eligibility, conversion, damage, and visuals | stock |
| GSI-11.09 | paradrop superweapon charge, target selection, aircraft/payload creation, and launch | mixed |
| GSI-11.10 | spy-plane/recon flight, reveal footprint, aircraft lifecycle, and recharge | mixed |
| GSI-11.11 | Psychic Reveal superweapon charge, target selection, and reveal-effect dispatch | stock |
| GSI-11.12 | superweapon building links, availability loss, one-time flags, and sidebar state | mixed |

### GSI-12 — Visibility, intelligence, disguise, and radar

| ID | System | Discovery scope |
|---|---|---|
| GSI-12.01 | per-object sight range and reveal-footprint algorithm | stock |
| GSI-12.02 | reveal/conceal update protocol that mutates GSI-04.18 cell state | stock |
| GSI-12.03 | elevation, cliffs, bridges, buildings, and visibility reference frames | stock |
| GSI-12.04 | per-house/object visibility policy when optional fog-cell state is enabled | conditional/legacy |
| GSI-12.05 | cloak state, cloak progress, decloak triggers, and visual state | stock |
| GSI-12.06 | sensors, detection, stealth legality, and sensor arrays | stock |
| GSI-12.07 | disguise, spy identity, mirage presentation, and reveal conditions | mixed |
| GSI-12.08 | Gap Generator concealment, radius updates, power, and house effects | stock |
| GSI-12.09 | SpySat/Psychic Reveal/global-reveal effect application to map-cell knowledge | mixed |
| GSI-12.10 | radar availability, jam/blackout, radar events, and minimap knowledge | stock |
| GSI-12.11 | observer/allied vision sharing and multiplayer visibility policy | mixed/mode |

### GSI-13 — World rendering and visual effects

| ID | System | Discovery scope |
|---|---|---|
| GSI-13.01 | frame buffers, dirty regions, tactical redraw, and final composition | stock |
| GSI-13.02 | isometric projection, camera origin, clipping, and tactical scrolling | stock |
| GSI-13.03 | object layer assignment, Y/Z ordering, occlusion, and draw traversal | stock |
| GSI-13.04 | TMP ground, ramps, cliffs, shore, LAT, and tile transitions | stock |
| GSI-13.05 | overlay, wall, ore, bridge, terrain-object, and smudge drawing | stock |
| GSI-13.06 | SHP sprite frame selection, facings, sequences, and blitters | stock |
| GSI-13.07 | voxel/HVA transform, facing, turret/barrel, rasterization, and bounds | stock |
| GSI-13.08 | palette selection, house remap, translucency, color conversion, and blending | stock |
| GSI-13.09 | Z-buffer, A-buffer/alpha surfaces, clipping masks, and overlap handling | stock |
| GSI-13.10 | ambient/global lighting, per-object light, tint, and palette effects | stock/mixed |
| GSI-13.11 | ground/object/voxel shadows and height projection | stock |
| GSI-13.12 | animations, damage/fire/smoke, debris, and voxel-animation drawing | stock |
| GSI-13.13 | particle systems, particles, trails, sparks, and smoke composition | stock |
| GSI-13.14 | LaserDraw object/vector update, lifetime, endpoints, and rendering | stock |
| GSI-13.15 | transient laser-segment lifetime and purge manager | stock/mixed |
| GSI-13.16 | DiskLaser manager, update order, geometry, lifetime, and rendering | stock |
| GSI-13.17 | EBolt vector, electrical-arc lifetime, update, and drawing | stock |
| GSI-13.18 | WaveClass vector, sonic/magnetic wave lifetime, update, and drawing | stock/mixed |
| GSI-13.19 | AlphaShape vector, alpha-surface lifetime, purge, and drawing | stock/mixed |
| GSI-13.20 | remaining animation/particle-backed weapon and superweapon visual dispatch | mixed/group node |
| GSI-13.21 | shroud/fog/visibility overlay composition | stock/mixed |
| GSI-13.22 | selections, health bars, pips, action lines, rally lines, and markers | stock |
| GSI-13.23 | radar/minimap terrain, units, events, shroud, and viewport | stock |
| GSI-13.24 | sidebar/power strip/cameos/tabs/clock/queue presentation | stock |
| GSI-13.25 | cursor, tooltip, placement ghost, range, and target feedback | stock |
| GSI-13.26 | shell/dialog/loading/score/movie presentation | mixed/mode |

### GSI-14 — Input, commands, and in-game user interface

| ID | System | Discovery scope |
|---|---|---|
| GSI-14.01 | keyboard sampling, hotkeys, key bindings, and command classes | stock |
| GSI-14.02 | mouse sampling, button state, wheel, drag, and double-click handling | stock |
| GSI-14.03 | cursor/action determination and target-context resolution | stock |
| GSI-14.04 | single/bandbox/type selection, selection filters, and selection order | stock |
| GSI-14.05 | control groups, select-same-type, next/previous, and camera recall | stock |
| GSI-14.06 | edge/key scrolling, camera clamp, bookmarks, and tactical navigation | stock |
| GSI-14.07 | local semantic player-order construction before deterministic/network envelope encoding | stock |
| GSI-14.08 | building placement, repair/sell/power modes, deploy, and rally input | stock |
| GSI-14.09 | superweapon target mode, range/legality feedback, and cancel | stock |
| GSI-14.10 | sidebar tabs, production buttons, queues, power strip, and tooltips | stock |
| GSI-14.11 | radar clicks, drag, map navigation, events, and tactical recenter | stock |
| GSI-14.12 | in-game options, save/load, surrender, restart, and quit flow | mixed |
| GSI-14.13 | chat, beacons, taunts, planning mode, and allied communication | mixed/mode |

### GSI-15 — Audio, speech, music, and video

| ID | System | Discovery scope |
|---|---|---|
| GSI-15.01 | sound-type registry, sample lookup, variants, priority, and volume | stock |
| GSI-15.02 | positional attenuation, stereo pan, listener/camera relation, and cutoff | stock |
| GSI-15.03 | channels, handles, interruption, looping, stop/fade, and lifetime | stock |
| GSI-15.04 | DirectSound device/channel pool, mixer/buffer servicing, and audio update-thread cadence | stock/platform |
| GSI-15.05 | gameplay/UI/weapon/animation/ambient sound-trigger routing | stock |
| GSI-15.06 | EVA event selection, queueing, suppression, interruption, and house voice | stock |
| GSI-15.07 | theme/music catalog, selection, shuffle, transitions, and stream pump | stock |
| GSI-15.08 | unit voices, acknowledgements, attack/move/death pools, and taunts | stock/mixed |
| GSI-15.09 | subtitles/captions and speech-linked text presentation | mixed/unknown |
| GSI-15.10 | Bink/VQA movie playback, audio sync, skip, and shell transition | stock/mode |
| GSI-15.11 | briefing/cinematic speech, camera cues, and scripted media | stock/mode |

### GSI-16 — Networking and multiplayer protocol

| ID | System | Discovery scope |
|---|---|---|
| GSI-16.01 | deterministic `EventClass`/command-envelope encoding, serialization, and lockstep admission | stock/mode |
| GSI-16.02 | synchronized command queue, ordering, frame stamps, and dispatch | stock/mode |
| GSI-16.03 | lockstep frame pacing, maximum-ahead window, latency, and stalls | stock/mode |
| GSI-16.04 | scenario seed, options, house/slot, map, and content handshake | stock/mode |
| GSI-16.05 | connection objects, packet framing, send/receive queues, and retry | stock/mode |
| GSI-16.06 | LAN transport, discovery, addressing, and Winsock/IPX-facing services | stock/mode |
| GSI-16.07 | lobby/session membership, ready state, teams, colors, and launch | stock/mode |
| GSI-16.08 | map negotiation, scenario transfer, file transfer, and progress | stock/mode |
| GSI-16.09 | Westwood Online login, chat, game listing, quick match, and WDT services | stock/mode |
| GSI-16.10 | multiplayer chat, beacons, taunts, alliances, and player messages | stock/mode |
| GSI-16.11 | checksums, synchronization checks, desync detection, and diagnostics | stock/mode |
| GSI-16.12 | reconnect/reestablish, timeout, drop, abort, and player removal | stock/mode |
| GSI-16.13 | observer/spectator data policy and score distribution | mixed/mode |
| GSI-16.14 | legacy modem, serial, null-modem, and phonebook transports | legacy/mode |

### GSI-17 — Persistence, restoration, and deterministic evidence

| ID | System | Discovery scope |
|---|---|---|
| GSI-17.01 | scenario/map load pipeline and object construction order | stock |
| GSI-17.02 | save-game object serialization and class/version contracts | stock |
| GSI-17.03 | pointer/reference swizzling, object identity, and fixup tables | stock |
| GSI-17.04 | load restoration, global re-registration, post-load fixups, and resume | stock |
| GSI-17.05 | campaign progress/carryover persistence | stock/mode |
| GSI-17.06 | user settings, hotkeys, profile, and shell-state persistence | stock |
| GSI-17.07 | native replay recording/header, scenario relaunch, per-frame playback, sync/selection/cursor records, and replay availability flags | active/conditional |
| GSI-17.08 | debug logs, assertions, exception/crash reporting, and diagnostic dumps | platform/mode |
| GSI-17.09 | screenshot/capture and diagnostic visual output | stock/platform |

### GSI-18 — Compiled legacy, stock-disabled, and developer/tooling surfaces

These remain in the inventory because silently deleting compiled behaviors would
make later reachability audits impossible. They are excluded from a stock-active
completion denominator unless a stock-YR caller or retail data path is proven.
Inherited systems with a natural domain owner are not repeated here: optional
fog is GSI-04.19/GSI-12.04, dormant locomotors are GSI-06.21–06.23, and legacy
network transports are GSI-16.14.

| ID | System | Discovery scope |
|---|---|---|
| GSI-18.01 | veins/veinhole terrain, growth, damage, and monsters | dormant/legacy |
| GSI-18.02 | Firestorm wall and laser-fence style inherited mechanics | legacy/unchecked |
| GSI-18.03 | EMPulse state/application and EMP superweapon path | dormant in retail YR |
| GSI-18.04 | Ion Cannon blast/superweapon path | dormant in retail YR |
| GSI-18.05 | Hunter Seeker inherited strategic path | legacy/unchecked |
| GSI-18.06 | Chemical Missile inherited strategic path | legacy/unchecked |
| GSI-18.07 | map/scenario editor-facing metadata and editor-only behavior | tooling/unchecked |
| GSI-18.08 | cheats, debug keys, developer overlays, and developer control modes | tooling/unchecked |

## Composite feature routing and non-duplication rules

Several player-visible actions cross many atomic systems. This table adds no
new systems and is not a denominator. It routes a composite player feature to
the relevant canonical rows above so the same mechanism is not counted again.

| Cross-cutting feature | Primary registry route | Important dependencies |
|---|---|---|
| harvesting | GSI-07.15/07.17/07.21 mission handlers | GSI-04.09, GSI-09.03–09.06, GSI-07.37–07.39 |
| firing a weapon | GSI-08.01–08.10 | GSI-07.06, GSI-13.12–13.20, GSI-15.05, GSI-16.01–16.03 |
| building placement | GSI-09.11 | GSI-04.04–04.06, GSI-14.08, GSI-13.25 |
| Chronosphere | GSI-11.06 | GSI-06.20, GSI-05.03–05.04, GSI-13.12–13.20, GSI-15.05 |
| shroud/radar | GSI-12 | GSI-04.18–04.19, GSI-13.21/13.23, GSI-14.11 |
| save/load | GSI-17.02–17.04 | every serialized owner plus GSI-03.08 and GSI-14.12 |
| multiplayer lockstep | GSI-16.01–16.04 | all deterministic simulation owners, GSI-01.07–01.10 |

## Current Rust ownership map — presence only

This is a coarse routing map for the next pass. `PRESENT` and `PARTIAL` are not
parity verdicts.

| Family | Principal Rust owners | Presence finding |
|---|---|---|
| GSI-01 runtime | `src/app.rs`, `src/app_instances/`, `src/app_render/`, shell modules, `src/sim/world/` | present, partial, parity unchecked |
| GSI-02 data/assets | `src/assets/`, `src/rules/`, `src/map/` | broad presence, uneven format/key coverage |
| GSI-03 shell/modes | `src/ui/`, `src/app.rs`, skirmish shell modules | partial; campaign launch, WOL flow, Movies & Credits picker/content launch, and credits roller are material gaps; Bink shell playback exists |
| GSI-04 world/map | `src/map/`, `src/sim/map/`, `src/sim/bridge_state/`, `src/sim/tiberium/` | broad partial presence |
| GSI-05 entities | `src/sim/game_entity.rs`, `src/sim/world/`, component/house/factory modules | broad partial presence |
| GSI-06 movement | `src/sim/movement/`, `src/sim/pathfinding/`, `src/sim/aircraft/` | broad partial presence |
| GSI-07 missions | `src/sim/mission/`, `src/sim/radio/`, `src/sim/docking/`, `src/sim/miner/` | broad partial presence |
| GSI-08 combat | `src/sim/combat/`, combat-related world/component modules | broad partial presence; some special managers incomplete |
| GSI-09 economy | `src/sim/production/`, `src/sim/miner/`, house/power/world modules | broad partial presence |
| GSI-10 AI/scripts | map trigger modules and simulation world/AI surfaces | partial; no structured TaskForce/Script/Team/AITrigger rules pipeline was found |
| GSI-11 superweapons | `src/sim/superweapon/`, `src/rules/superweapon_type.rs` | partial; parsed kinds exceed implemented launch dispatch |
| GSI-12 visibility | `src/sim/vision/`, radar/sidebar/render owners | partial |
| GSI-13 rendering | `src/render/`, `src/app_render/`, sidebar/UI render owners | broad partial presence |
| GSI-14 input/UI | `src/ui/`, `src/sidebar/`, `src/app.rs`, simulation commands | partial |
| GSI-15 audio/media | `src/audio/`, audio asset decoders, UI/app routing | partial; media shell is incomplete |
| GSI-16 networking | `src/net/`, app command scheduling, and the simulation command envelope | tick-stamped command/input-delay scheduling is present; `src/net/lockstep.rs` is a scaffold; socket transport, lobby, online, sync, reconnect/resync were not found |
| GSI-17 persistence | `src/sim/snapshot.rs`, `src/sim/replay.rs`, `src/app_save_load_panel.rs`, `src/app_input.rs`, `src/app_sim_tick.rs`, and loaders | Rust-native snapshot save/load and replay logging/runner are present; campaign persistence breadth, shell replay loading, and native-format/mechanism equivalence remain unchecked |
| GSI-18 legacy/tools | scattered parsers/modules or intentionally absent | must follow reachability verdict, not implementation count |

Direct source evidence for several high-level gaps:

- `src/ui/main_menu_dialogs.rs:13–15,261–280,364–367` states that campaign-side
  selection is not yet mapped into scenario launch parameters. Its Movies &
  Credits actions are open-level only; `src/app.rs:2207–2217` leaves picker,
  selected-content launch, and credits actions as no-ops, while separate Bink
  shell playback code does exist.
- `src/app.rs` has shell actions for online/menu surfaces without the native
  downstream session/dialog stack.
- `src/net/mod.rs:1–20` exposes only the lockstep module. Tick-stamped command
  scheduling exists in app/simulation code, but no socket transport,
  lobby/matchmaking, map transfer, reconnect, or resynchronization owner was
  found.
- `src/map/map_file.rs:197–198` retains general/raw INI access but no complete structured
  TaskForce/ScriptType/TeamType/AITriggerType pipeline was found.
- `src/rules/superweapon_type.rs:26–51` parses 12 superweapon kinds, while
  `src/sim/world/world_commands.rs:1268–1327` dispatches seven and warns for the
  remaining five.
- session-option and overlay crate metadata are parsed and hashed, but no
  complete crate spawn/pickup runtime owner was found. Spy infiltration is
  explicitly incomplete at `src/sim/production/production_tech.rs:48–54`.

These are presence findings from the current tree, not a disparity audit against
each native mechanism.

## Status model for the next phase

Every atomic stable ID should receive five independent fields. They must not be merged
into a single optimistic percentage.

Rows whose provisional scope contains `mixed` or `group node` are discovery
parents, not scorable leaves. They must be split before inclusion in a completion
or parity denominator.

### 1. Activity and reachability

- `STOCK-ACTIVE` — reached by standard retail YR defaults/content.
- `MODE-ACTIVE` — reached only by a named campaign, skirmish, multiplayer, online,
  replay, observer, or tooling mode.
- `CONTENT-CONDITIONAL` — compiled and reachable when a stock/mod data flag or
  object configuration selects it.
- `COMPILED-INACTIVE` — present in the binary but proved unreachable under stock
  retail YR data/defaults.
- `UNKNOWN` — reachability has not been proved.

### 2. Inventory evidence

- `DISCOVERED` — named by at least one source.
- `BOUNDED` — entry points, owners, data surface, and dependent systems mapped.
- `EXHAUSTIVE-SLICE` — all relevant functions/data variants for the declared
  slice were enumerated and the residual unclassified queue is zero.

### 3. Native research depth

- `UNCHECKED` — taxonomy only.
- `ANCHORED` — at least one live body/caller or retail-data anchor verified.
- `CONTRACTED` — state, ordering, formulas, side effects, activity gates, and edge
  cases have an implementation-grade evidence contract.
- `NATIVE-ORACLE` — a gamemd/retail-derived executable check or exhaustive proof
  exists for the declared input space.

### 4. Rust implementation state

- `ABSENT`
- `SCAFFOLD`
- `PARTIAL`
- `PRESENT`
- `COMPLETE-FOR-CONTRACT`

### 5. Parity verdict

- `UNCHECKED` — the required default for this inventory.
- `DRIFT` — any verified formula, field, ordering, timing, RNG, byte, audio, UI,
  input, or pixel difference.
- `TRACE-MATCHED` — one or more bounded native-vs-Rust scenarios agree; not proof
  for other inputs.
- `VERIFIED` — only with a named native/retail executable oracle or exhaustive
  equivalence proof over the declared input space.

Progress percentages may be machine-derived later for a clearly declared
denominator, for example "37 of 52 stock-active movement contracts are
CONTRACTED." A percentage must never include unknown-activity items silently,
and a Rust regression test does not count as a native parity oracle.

## Coverage ledger

| Surface | What this pass covered | What remains |
|---|---|---|
| research corpus | rebuilt index; classified source/status totals; read broad architecture, scheduler, sector indexes, and sampled specialist reports | assign the stale/unknown research-and-plan queue; repair broken links; reconcile duplicate/stale generations |
| class/type discovery | classified 1,366 recovered class/namespace entries into domain families and filtered obvious runtime/template noise | verify every game-like class body, vtable identity, reachability, and canonical owner |
| named function discovery | classified 3,033 custom-named functions by normalized owner/name prefixes; manually folded generic function-only owners | map 6,577 default-named functions and reconcile 425 imported/other-source functions; detect incorrect/missing boundaries |
| source anchors | classified 69 live `.CPP` strings as a secondary cross-check | source strings are optimizer/build artifacts and cannot prove absence |
| runtime spine | live-checked scenario outer loop and ordered per-tick dispatcher | all callers, alternate mode paths, error exits, modal paths, and shutdown edges |
| retail INI | enumerated 27 files and their major domain surfaces | prove each key's parser consumer, default, merge behavior, and stock reachability |
| assets | represented every major retail format/domain visible in code/data/docs | enumerate all format variants, palette paths, codec edges, and lookup precedence |
| Rust | routed all 18 families to current ownership and recorded obvious whole-family gaps | perform stable-ID disparity scans and exact-mechanism contracts |
| activity | separated stock, mode/conditional, legacy, mixed, and unknown at discovery level | split group nodes and obtain live caller/default-mode proof for every conditional or inherited atomic item |
| edge cases | identified cross-cutting audit requirement | empty/zero/max, first/last tick, pause/modal, save/restore, network drop, and every mode transition per atomic item |

## Completeness assessment

The registry is **structurally broad but not proven complete**.

Evidence that it is broad:

- it reconciles research, retail configuration, Rust ownership, live class names,
  named function families, embedded source anchors, and two top-level runtime
  spine checks;
- it includes engine services, gameplay mechanics, content/data loading,
  presentation, shell/modes, audio/media, networking, persistence, and dormant
  inherited surfaces;
- only a small residue of obvious generic/custom-named prefixes remained after
  manual family folding.

Evidence that it is not complete:

- 6,577 recovered functions are default-named, with another 425
  imported/other-source functions outside the custom-named set pending category
  reconciliation;
- the validator reports a large stale-or-unknown queue spanning both research
  and plan inputs;
- the existing AI frontier documents are primarily structural and do not prove
  the full decision algorithms;
- campaign, online services, save/load, replay semantics, score/mode variants,
  localization details, and some media/platform paths are comparatively thin;
- class enumeration cannot discover optimized free functions, inlined behavior,
  data-driven state machines, or polluted/missing function boundaries;
- individual content types can select hardcoded paths that a family-level scan
  misses. For scale, the combat index alone inventories 105 `[Warheads]` entries
  and 55 case-sensitive projectile references representing 54 canonical
  projectile IDs/sections.

The inventory can be called complete only for a declared scope when all relevant
binary entry points, vtables, function bodies, data keys/assets, modes, and
content-dispatched variants map to a stable ID; every ID has a reachability
classification; and the residual unclassified queue for that scope is zero.

## Investigation handoff

The next useful artifact is a machine-readable status matrix keyed by these IDs,
not an immediate global percentage. For each chosen family:

1. validate the relevant research docs and repair stale links;
2. enumerate native entry points, callers, state owners, INI/assets, modes, and
   hardcoded content variants;
3. prove stock-active versus conditional/legacy reachability;
4. map the current Rust owner without treating code presence as equivalence;
5. record every verified delta as `DRIFT`, regardless of size or normal-play
   frequency;
6. define native-derived tests/traces and only then roll up bounded progress.

Recommended order is the dependency spine rather than the most visible feature:

1. GSI-01 runtime/determinism and GSI-05 lifecycle;
2. GSI-04 world/cells and GSI-06 movement/pathfinding;
3. GSI-07 missions/radio and GSI-08 combat;
4. GSI-09 economy/production, GSI-10 AI/scripts, and GSI-11 superweapons;
5. GSI-12 visibility, GSI-13 rendering, GSI-14 input, and GSI-15 audio;
6. GSI-03 shell/modes, GSI-16 networking, and GSI-17 persistence;
7. GSI-18 reachability audit so dormant code does not inflate the active target.

## Open Questions

| ID | Resolution |
|---|---|
| OQ-01 | **RESOLVED for this map.** A system is an independently auditable state, algorithm, protocol, data-loader, or presentation contract; classes and content rows are evidence, not automatic systems. |
| OQ-02 | **RESOLVED at spine level.** `0x0048CCC0` bounds the outer scenario lifecycle and `0x0055AFB0`, called by `0x0055D360`, bounds the ordered per-tick global dispatcher. Alternate/error paths remain per-family work. |
| OQ-03 | **RESOLVED for discovery.** Runtime/templates/interfaces were filtered; game-like labels remain hints until body/caller/vtable verification. |
| OQ-04 | **RESOLVED for custom-named functions; DEFERRED for non-custom functions.** Function-prefix and source-anchor scans recovered classless services, but 6,577 default-named plus 425 imported/other-source functions remain to reconcile. |
| OQ-05 | **RESOLVED quantitatively.** Rebuilt index counts and the stale/unknown research-and-plan queue are recorded above; per-document correction is deferred. |
| OQ-06 | **DEFERRED per atomic item.** This pass records coarse activity scopes, but every mixed/conditional/legacy item needs caller plus retail-default reachability proof. |
| OQ-07 | **RESOLVED taxonomically.** Modes share core simulation but own distinct setup, policy, UI, scoring, networking, or progression contracts, so mode services are separate nodes where those contracts differ. |
| OQ-08 | **RESOLVED taxonomically.** Serialization, swizzling, restoration, campaign persistence, and replay are separate contracts. Native replay is verified active conditionally; its complete stream/event format remains only partially researched. |
| OQ-09 | **RESOLVED taxonomically.** VFS/precedence, INI/type loading, asset decoders, theater data, and palettes have distinct ownership IDs under GSI-02. |
| OQ-10 | **RESOLVED as a discovery list.** GSI-04 enumerates the current world/map candidate nodes; exhaustive behavior remains deferred per atomic ID. |
| OQ-11 | **RESOLVED as a discovery list.** GSI-01 and GSI-05 enumerate the current simulation spine and lifecycle owners. |
| OQ-12 | **RESOLVED as a discovery list.** GSI-06 plus docking/transport dependencies in GSI-07 enumerate current movement nodes; inherited locomotor reachability is classified dormant for stock YR and mod reachability remains separate. |
| OQ-13 | **RESOLVED as a discovery list.** GSI-08 and GSI-11 enumerate combat/effect owners; content-dispatched hardcoded variants remain a residual queue. |
| OQ-14 | **RESOLVED as a discovery list.** GSI-09 enumerates economy/production owners. |
| OQ-15 | **RESOLVED structurally, behavior DEFERRED.** GSI-10 separates house AI, planning, threat, AI triggers, teams, scripts, trigger/event/action, difficulty, and outcomes; algorithms are not yet fully contracted. |
| OQ-16 | **RESOLVED as a discovery list.** GSI-13 through GSI-15 separate render, input/UI, audio, speech, music, and video ownership. |
| OQ-17 | **RESOLVED as a discovery list.** GSI-03 owns shell/setup/loading/mode flows. |
| OQ-18 | **RESOLVED as a discovery list.** GSI-16 separates lockstep, pacing, transport, lobby, transfer, WOL, sync, reconnect, and communication. |
| OQ-19 | **RESOLVED as a discovery list.** Platform/bootstrap/localization/media support is represented in GSI-01, GSI-02, GSI-15, and GSI-17. |
| OQ-20 | **RESOLVED.** Use independent reachability, inventory-evidence, native-depth, Rust-presence, and parity fields; derive percentages only from a declared machine-counted denominator. |
| OQ-21 | **RESOLVED at module-presence level.** The Rust routing table records ownership without parity judgment; atomic-ID implementation status is the next pass. |
| OQ-22 | **RESOLVED.** Completeness requires all relevant code/data/mode/content variants assigned, reachability classified, and a zero residual queue for a declared scope. |
| OQ-23 | **DEFERRED per system.** Empty/zero/max containers, first/last tick, pause/modal, save/restore, network interruption, and mode transitions require bounded leaf investigations. |
| OQ-24 | **DEFERRED with explicit queues.** The principal residues are 6,577 default-named functions, 425 imported/other-source functions pending category reconciliation, and the stale/unknown research-and-plan queue; generic named prefixes were manually folded where their role was clear. |

## Principal source set

- `docs/research/CORE_ENGINE_SERVICES_MAP.md`
- `docs/research/GAMEMD_ARCHITECTURE.md` — historical, never-audited broad map;
  used only as a discovery lead
- [docs/research/LOGICCLASS_GLOBAL_SUBSYSTEM_ORDER_0055AFB0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_GLOBAL_SUBSYSTEM_ORDER_0055AFB0_GHIDRA_REPORT.md)
- [docs/research/MISSIONCLASS_STATE_MACHINE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MISSIONCLASS_STATE_MACHINE.md)
- [docs/research/REPLAY_ACTIVE_VECTOR_RESTORE_CORNER_RESWARM_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/REPLAY_ACTIVE_VECTOR_RESTORE_CORNER_RESWARM_20260528.md)
- [docs/research/TS_DORMANT_LOCOMOTORS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TS_DORMANT_LOCOMOTORS_GHIDRA_REPORT.md)
- [docs/research/MAGNETRON_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAGNETRON_SYSTEM_GHIDRA_REPORT.md)
- [docs/research/IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md)
- [docs/research/EBOLT_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/EBOLT_SYSTEM_GHIDRA_REPORT.md)
- [docs/research/GATTLING_WEAPON_STAGE_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GATTLING_WEAPON_STAGE_SYSTEM_GHIDRA_REPORT.md)
- [docs/research/skirmish-ui/SKIRMISH_PACKED_OPTION_GLOBAL_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_PACKED_OPTION_GLOBAL_CONSUMERS_GHIDRA_REPORT.md)
- [docs/research/skirmish-ui/SKIRMISH_TEAM_ADJUNCT_HOUSE_ALLIANCE_HANDOFF_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_TEAM_ADJUNCT_HOUSE_ALLIANCE_HANDOFF_GHIDRA_REPORT.md)
- [docs/research/combat/INDEX_COMBAT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/combat/INDEX_COMBAT.md)
- [docs/research/units/INDEX_UNITS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/units/INDEX_UNITS.md)
- [docs/research/ENGINE_STATE_OVERVIEW.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ENGINE_STATE_OVERVIEW.md)
- [docs/research/.audit-coverage-index.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/.audit-coverage-index.md)
- rebuilt repo-local research index database and validator output, 2026-07-20
- retail repo INIs under `ini/`, with `*md` override precedence
- live read-only Ghidra discovery and spine queries listed above
- current Rust module/source scan under `src/`

## Final boundary

This document answers "what should we measure?" It does not yet answer "how
complete is each system?" or "is each one parity-correct?" The stable IDs make
those questions measurable. Until each ID has a bounded native contract and a
native-derived comparison, its parity verdict is `UNCHECKED`.
