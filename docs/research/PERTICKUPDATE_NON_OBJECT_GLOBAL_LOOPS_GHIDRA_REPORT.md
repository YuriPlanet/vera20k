# PerTickUpdate Non-Object Global Loops - Ghidra Research Report

**Address(es):** `LogicClass::PerTickUpdate @ 0x0055AFB0`; focus ranges `0x0055B4D7..0x0055B6B1`; supporting constructors/destructors for Team, DiskLaser, RadSite, Anim feedback, Factory, House.
**Investigation Mode:** coverage-map
**Claimed Scope:** non-main-LogicClass-vector loops and direct global subsystem calls in active YR `LogicClass::PerTickUpdate`, especially `0x0055B502..0x0055B6B1`; loop order, array/global identity where proven, direction, count snapshot/live-reload behavior, vtable slot target when knowable, and Rust-facing order deltas.
**Non-Scope:** the main LogicClass object vector contract except for contrast; full internals of every `vtable+0x5C` callee; save/load reconstruction; every caller that mutates these global arrays.
**Confidence:** High for loop order, direction, counts, and named globals with constructor/destructor evidence; Medium for unnamed helper/list semantics; Low only where explicitly marked as inferred/unknown.
**Active in YR:** Yes. `Main_Tick` calls `LogicClass::PerTickUpdate` in the standard YR tick path, and each loop/call below is on that active path unless marked conditional.

## 0. Investigation Gate

**Target question:** Which non-main-object global loops/calls inside active YR `LogicClass::PerTickUpdate @ 0x0055AFB0`, especially `0x0055B502..0x0055B6B1`, correspond to teams, disk lasers, factories, houses, radiation sites, click-feedback anims, and other global subsystems, and how do their ordering/iteration contracts differ from Rust `Simulation::advance_tick`?

**Non-goals:** Do not re-prove the main LogicClass object vector; do not implement Rust; do not rename or mutate Ghidra state; do not turn unknown globals into guessed class names.

**Evidence needed to mark COMPLETE:** direct `PerTickUpdate` decompile/disassembly for each loop, xref or constructor/destructor evidence for each named global, vtable data or decompiled callee where knowable, and Rust file:line evidence for current phased/snapshot behavior.

**Stop conditions:** stop after all loops in `0x0055B4D7..0x0055B6B1` are classified as verified, touched-not-exhausted, or deferred; stop before expanding into full callee internals.

## 1. Overview

`PerTickUpdate` is not just the live object-vector scheduler. Before the main vector it runs ore/tiberium, bombs, team AI via a scratch copy, reverse global effect loops, lasers, lightning, radiation sites, and EMP; after the main vector it runs conditional click-feedback anims, alpha-shape cleanup, crate timers, Tactical, factories, houses, and last-ref-object repair.

Rust `Simulation::advance_tick` is a high-level phased pipeline (`movement -> vision -> power -> superweapons -> combat -> production/ore -> AI -> building/world effects`) rather than this native late-housekeeping ladder.

## 2. Non-Object Loop / Call Ledger

| Native order | Address/range | Global / callee | Loop shape | Count behavior | Vtable/callee | Active in YR |
|---:|---|---|---|---|---|---|
| 1 | `0x0055B4D7` | `TiberiumClass::GrowthDriver_AllTypes @ 0x00722C40` | direct call | callee-owned | direct | Yes |
| 2 | `0x0055B4DC` | `TiberiumClass::SpreadDriver_AllTypes @ 0x007221B0` | direct call | callee-owned | direct | Yes |
| 3 | `0x0055B4E1` | `BombClass::UpdateAll @ 0x00438BF0`, `ECX=0x87F5D8` | direct call | callee-owned | direct | Yes |
| 4 | `0x0055B4EB` | `FUN_0054E4D0`, `ECX=0x00ABC5F8` | direct timer/list helper | callee-owned | direct | Yes, semantics touched-not-exhausted |
| 5 | `0x0055B502..0x0055B59F` | `g_TeamClass_Array @ 0x008B40EC`, count `0x008B40F8` -> stack scratch vector | copy forward, then iterate copied list forward | source count reloads while copying; AI count is copied `local_8` | Team main vtable `0x007F4730`; slot `+0x5C` data points to `0x006E9140` | Yes |
| 6 | `0x0055B5A1..0x0055B5BC` | `g_DiskLaserClass_Array @ 0x008A020C`, count `0x008A0218` | reverse from `count-1` to `0` | count snapshotted before loop | `DiskLaserClass::AI @ 0x004A7340` from vtable entry `0x007E6014` | Yes, for DiskLaser weapons/Floating Disc |
| 7 | `0x0055B5BE` | `FUN_005FF390` over `DAT_00AC167C`, count `0x00AC1688` | callee reverse loop | callee snapshots count at entry | direct helper ages/removes entries after field `+0x0C > 0x4F` | Yes, exact class unknown |
| 8 | `0x0055B5C3` | `LaserDrawClass::UpdateAllAI @ 0x00550150` | direct call | callee-owned | direct | Yes |
| 9 | `0x0055B5C8` | `LightningStorm::Process @ 0x0053A6C0` | direct call | callee-owned | direct | Yes when storm state exists; call itself always reached |
| 10 | `0x0055B5CD..0x0055B5E8` | `RadSiteClass` array `DAT_00B04BD4`, count `DAT_00B04BE0` | reverse from `count-1` to `0` | count snapshotted before loop | `RadSiteClass::AI @ 0x0065B800` from vtable base `0x007F0810`, slot `+0x5C` | Yes, for radiation sites |
| 11 | `0x0055B5EA` | `FUN_00554D50(6, false)` | direct call | callee-owned | direct | Yes, exact class unknown/global terrain-cache helper |
| 12 | `0x0055B5F6` | `EMPulseClass::UpdateAll @ 0x004C54A0` | direct call | callee-owned | direct | Yes as code path; EMP gameplay may be feature-gated |
| 13 | `0x0055B61B..0x0055B649` | `DAT_00A83E04`, count `DAT_00A83E10` secondary click-feedback anim array | forward | count reloads after each call | `AnimClass::AI @ 0x00423AC0` for inserted anim objects | Conditional: `g_GameMode != 0 && g_GameMode != 5` and array non-empty |
| 14 | `0x0055B64B` | `FUN_0053D310` over `DAT_00AA011C`, count `DAT_00AA0128` | callee reverse loop | callee snapshots count at entry | calls `Wave_splash_forces` | Yes, exact producer set out-of-scope |
| 15 | `0x0055B650` | `AlphaShapeClass::PurgeDisabled @ 0x00420E90` | direct call | callee-owned | direct | Yes |
| 16 | `0x0055B655` | `MapClass::UpdateCrateRegenTimers @ 0x0056BBE0` | direct call | callee-owned | direct | Yes |
| 17 | `0x0055B65F..0x0055B667` | `g_Tactical @ 0x00887324` | singleton vtable call | none | `g_Tactical->vtable+0x5C` | Yes |
| 18 | `0x0055B66A..0x0055B68B` | `g_FactoryClass_Array @ 0x00A83E34`, count `0x00A83E40` | forward | count reloads after each call | `FactoryClass::AI @ 0x004C9B20` from vtable base `0x007E88D0`, slot `+0x5C` | Yes |
| 19 | `0x0055B68D..0x0055B6B1` | `g_HouseClass_Array @ 0x00A8022C`, count `0x00A80238` | forward with null guard | count reloads after each call | `HouseClass::Update @ 0x004F8440` from vtable base `0x007EA8A0`, slot `+0x5C` | Yes |

## 3. Material Findings

| Finding | Evidence | Confidence | Active in YR |
|---|---|---:|---|
| The non-object ladder runs before and after the main LogicClass object loop; it is not equivalent to Rust's staged sim order. | `PerTickUpdate` decompile/disassembly `0x0055B4D7..0x0055B6B1`; Rust `advance_tick` phases at `src/sim/world/mod.rs:1186..1835`. | High | Yes |
| Team AI uses a scratch copied pointer vector: it scans `g_TeamClass_Array` forward, copies selected pointers into a stack `DynamicVector`, then calls copied entries by copied count. | Build/copy `0x0055B502..0x0055B580`; call loop `0x0055B582..0x0055B59F`; Team constructor registers to `0x008B40EC/0x008B40F8` at `0x006E8B4D..0x006E8B9A`. | High | Yes |
| Team source count is live while building the scratch vector, but Team AI dispatch does not reload the source global count after each AI. | Source count read at `0x0055B502` and again at `0x0055B577`; copied call loop compares against stack `local_8` at `0x0055B582..0x0055B59F`. | High | Yes |
| DiskLaser and RadSite loops are reverse loops that snapshot count once before iteration; appends during the loop are not visited by that same pass. | DiskLaser `0x0055B5A1..0x0055B5BC`; RadSite `0x0055B5CD..0x0055B5E8`; constructors append to their arrays at `0x004A7A30` and `0x0065B1E0`. | High | Yes |
| Factory and House loops are forward live-count loops, separate from the main LogicClass object vector, and reload their own global counts after each vtable call. | Factory `0x0055B675..0x0055B68B`; House `0x0055B698..0x0055B6B1`; constructors append at `0x004C9974..0x004C9989` and `0x004F61D7..0x004F61EC`. | High | Yes |
| House loop null-checks each slot before `vtable+0x5C`; Factory loop does not null-check slots. | House `TEST ECX,ECX` / skip at `0x0055B69D..0x0055B6A6`; Factory direct deref/call `0x0055B675..0x0055B680`. | High | Yes |
| The `DAT_00A83E04` loop is not the ordinary AnimClass global array; it is populated by `FootClass::ClickedAction_Cell` after moving a just-created click-feedback anim out of `g_AnimClass_Array`, and is gated by `g_GameMode != 0 && != 5`. | Population/removal `0x004D7FF4..0x004D8058`; destructor conditional removal `0x00422A69..0x00422AB0`; PerTick gate `0x0055B61B..0x0055B649`. | High | Conditional |
| `RadSiteClass::AI` is the RadSite `+0x5C` target and decrements remaining duration before applying rad damage/light timers; exact internals are outside this loop-order slot. | RadSite constructor vtable base `0x007F0810` at `0x0065B228`; vtable entry `0x007F086C -> 0x0065B800`; decompile `RadSiteClass::AI @ 0x0065B800`. | High | Yes for radiation sites |
| `FactoryClass::AI` is the Factory `+0x5C` target and spends house money/progresses production after Tactical and before House updates. | Factory vtable base `0x007E88D0`, entry `0x007E892C -> 0x004C9B20`; PerTick loop `0x0055B675..0x0055B68B`; decompile `0x004C9B20`. | High | Yes |
| `HouseClass::Update` runs after Factory AI, through the House array, with a null guard and live count reload. | House vtable base `0x007EA8A0`, entry `0x007EA8FC -> 0x004F8440`; PerTick loop `0x0055B698..0x0055B6B1`. | High | Yes |

## 4. Integration And Rust Delta

| Native behavior | Current Rust evidence | Rust delta |
|---|---|---|
| Native runs tiberium growth/spread before bombs, teams, disk lasers, laser/lightning/radsites/EMP, and before the main object vector. | Rust runs ore growth/spread late in Phase 7 after movement, vision, power, superweapons, combat, particles, production/repairs/docks (`src/sim/world/mod.rs:1655..1731`). | DRIFT for any interaction where ore/rad/effects/bombs/team/object timing matters. |
| Native Factory AI runs after Tactical and before House update, both after the main object vector. | Rust production runs in Phase 7 before Rust AI, while power is Phase 4 and `tick_ai` is Phase 8 (`src/sim/world/mod.rs:1393..1409`, `1670..1678`, `1757..1777`). | DRIFT for production spend/completion/house-AI ordering. |
| Native TeamClass AI is a copied Team scratch-list pass before DiskLaser, RadSite, EMP, and main objects. | Rust AI player logic runs near the end of `advance_tick` and uses high-level `ai::tick_ai` (`src/sim/world/mod.rs:1757..1777`), not a TeamClass global object list. | Missing native TeamClass pass and wrong tick position. |
| Native later Factory and House loops are live-count forward loops, while DiskLaser and RadSite are reverse snapshot loops. | `EntityStore` is a `BTreeMap<u64, GameEntity>` with `keys_sorted()` batch iteration (`src/sim/entity_store.rs:1..12`, `31..37`, `98..108`); many Rust subsystems snapshot IDs. | Existing deterministic stable-id order is not native array order or native count semantics. |
| Native click-feedback anims in `DAT_00A83E04` tick conditionally after the main object vector and before alpha purge/crate/Tactical. | Rust world effects tick at the end of the tick in Phase 9 (`src/sim/world/mod.rs:1797..1822`) and app-layer building anims use `keys_sorted()` (`src/app_building_anim.rs:33`, `193`). | UI/world-effect cadence/order is not native for this array. |

## 5. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `TiberiumClass` growth/spread call order | verified | `0x0055B4D7`, `0x0055B4DC`; existing tiberium reports | none for order |
| `BombClass::UpdateAll` placement | verified | `0x0055B4E1`; [BOMB_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BOMB_CLASS_GHIDRA_REPORT.md) | internals not in this slot |
| `FUN_0054E4D0` global timed helper | touched-not-exhausted | decompile `0x0054E4D0`; call `0x0055B4EB` | class/name and full purpose need separate trace |
| Team scratch-list loop | verified | `0x0055B502..0x0055B59F`; Team constructor `0x006E8A90` | Team `0x006E9140` body was not decompiled because Ghidra has no function there |
| DiskLaser reverse loop | verified | `0x0055B5A1..0x0055B5BC`; constructor `0x004A7A30`; vtable data `0x007E6014` | none for order |
| `FUN_005FF390` global effect list | touched-not-exhausted | decompile `0x005FF390`, `0x005FF250`, `0x005FF2D0` | producer/class identity out-of-scope |
| LaserDraw update placement | verified | call `0x0055B5C3`; callee `0x00550150` | internals not in this slot |
| LightningStorm placement | verified | call `0x0055B5C8`; callee `0x0053A6C0` | storm internals not in this slot |
| RadSite reverse loop | verified | `0x0055B5CD..0x0055B5E8`; constructor `0x0065B1E0`; vtable `0x007F086C`; AI `0x0065B800` | none for order |
| `FUN_00554D50` helper | touched-not-exhausted | call `0x0055B5EA`; decompile `0x00554D50` | exact system/name out-of-scope |
| EMP update placement | verified | call `0x0055B5F6`; callee `0x004C54A0` | EMP feature activation details out-of-scope |
| Conditional click-feedback anim array | verified | `0x0055B61B..0x0055B649`; `0x004D7FF4..0x004D8058`; `0x00422A69..0x00422AB0` | exact `g_GameMode` enum names not resolved |
| Wave/splash helper `FUN_0053D310` | touched-not-exhausted | call `0x0055B64B`; decompile `0x0053D310`, producer `0x0053CB10` | producer/class identity out-of-scope |
| AlphaShape, crate regen, Tactical placement | verified | `0x0055B650..0x0055B667` | internals not in this slot |
| Factory loop | verified | `0x0055B66A..0x0055B68B`; constructor `0x004C98B0`; AI `0x004C9B20` | none for order |
| House loop | verified | `0x0055B68D..0x0055B6B1`; constructor `0x004F54A0`; vtable entry `0x007EA8FC` | none for order |

## 6. Open Questions - Final State

- `[RESOLVED] OQ-PTNOG-001 - Is the range active in standard YR? -> Yes; it is inside `PerTickUpdate`, called from `Main_Tick`.` (evidence: `0x0055AFB0`; prior scheduler/timing reports)
- `[RESOLVED] OQ-PTNOG-002 - What runs first in this slice? -> Tiberium growth, then spread, then bombs.` (evidence: `0x0055B4D7..0x0055B4E6`)
- `[RESOLVED] OQ-PTNOG-003 - Which array backs the copied-count loop? -> `g_TeamClass_Array @ 0x008B40EC`, count `0x008B40F8`.` (evidence: `0x0055B502..0x0055B59F`; `0x006E8B4D..0x006E8B9A`)
- `[RESOLVED] OQ-PTNOG-004 - Does the Team AI call loop use live source count? -> No; it uses copied stack count `local_8`.` (evidence: `0x0055B582..0x0055B59F`)
- `[RESOLVED] OQ-PTNOG-005 - Which reverse loop is DiskLaser? -> `0x008A020C/0x008A0218`, `DiskLaserClass::AI @ 0x004A7340`.` (evidence: `0x0055B5A1..0x0055B5BC`; `0x004A7A30`; vtable data `0x007E6014`)
- `[RESOLVED] OQ-PTNOG-006 - Which reverse loop is RadSite? -> `0x00B04BD4/0x00B04BE0`, `RadSiteClass::AI @ 0x0065B800`.` (evidence: `0x0055B5CD..0x0055B5E8`; `0x0065B1E0`; `0x007F086C`)
- `[RESOLVED] OQ-PTNOG-007 - Which later forward loop is Factory? -> `0x00A83E34/0x00A83E40`, `FactoryClass::AI @ 0x004C9B20`.` (evidence: `0x0055B675..0x0055B68B`; `0x004C98B0`; `0x007E892C`)
- `[RESOLVED] OQ-PTNOG-008 - Which later forward loop is House? -> `0x00A8022C/0x00A80238`, `HouseClass::Update @ 0x004F8440`.` (evidence: `0x0055B698..0x0055B6B1`; `0x004F54A0`; `0x007EA8FC`)
- `[RESOLVED] OQ-PTNOG-009 - Does House loop null-check slots? -> Yes; Factory does not in its loop.` (evidence: `0x0055B675..0x0055B680`; `0x0055B698..0x0055B6A6`)
- `[RESOLVED] OQ-PTNOG-010 - What is `DAT_00A83E04`? -> A conditional secondary anim/click-feedback array populated by `FootClass::ClickedAction_Cell`, not ordinary `g_AnimClass_Array`.` (evidence: `0x004D7FF4..0x004D8058`; `0x0055B61B..0x0055B649`)
- `[RESOLVED] OQ-PTNOG-011 - Which systems remain unnamed? -> `FUN_0054E4D0`, `FUN_005FF390` list, `FUN_00554D50`, and `FUN_0053D310` are touched but not fully named.` (evidence: listed decompiles)
- `[RESOLVED] OQ-PTNOG-012 - Is Rust current order native-equivalent? -> No; `advance_tick` is a phased pipeline and ore/production/AI/world effects appear in different positions.` (evidence: `src/sim/world/mod.rs:1186..1835`)
- `[DEFERRED] OQ-PTNOG-013 - What is the exact class/name of the `0x00AC167C` effect list?` (category: `requires-different-system-context`; reason: producer/caller taxonomy exceeds this loop-order slot; next-step-if-pursued: trace `FUN_005FF250` callers and vtable/list owner.)
- `[DEFERRED] OQ-PTNOG-014 - What are exact `g_GameMode` enum meanings for `0` and `5`?` (category: `requires-different-system-context`; reason: gate shape is proven but enum mapping belongs in a game-mode/session report; next-step-if-pursued: trace `g_GameMode` initialization and UI/session constants.)
- `[DEFERRED] OQ-PTNOG-015 - Full `TeamClass +0x5C` body semantics at `0x006E9140`.` (category: `bounded-cost-too-high`; reason: Ghidra has no defined function at the vtable target and this slot only needs scheduler identity/order; next-step-if-pursued: define/analyze in a separate read-only TeamClass AI investigation if mutation is approved elsewhere.)

## 7. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Native late housekeeping runs tiberium growth/spread before bombs, teams, disk lasers, lasers, lightning, radiation sites, EMP, and main object AI. | `0x0055B4D7..0x0055B5F6` | Rust ore growth/spread runs late after combat/production in Phase 7. | `src/sim/world/mod.rs::advance_tick`; `src/sim/ore_growth.rs`; future native scheduler. | Move or emulate native ordering for ore/effect systems when parity mode is enabled. | A same-tick tiberium/spread/warhead interaction observes growth/spread state before object AI. | Do not keep ore growth as a generic post-production phase and call it native parity. |
| Native TeamClass AI is copied from `g_TeamClass_Array` into a scratch list, then dispatched before DiskLaser/RadSite/EMP/main object vector. | `0x0055B502..0x0055B59F`; `0x006E8A90` | Rust `ai::tick_ai` is a late high-level owner command generator, not a TeamClass global loop. | `src/sim/ai.rs`; `src/sim/world/mod.rs:1757..1777`; future TeamClass runtime. | Add a native TeamClass runtime pass with copied-list semantics distinct from owner strategic AI. | Team created/deleted during Team AI does not necessarily tick same pass; dispatch set is the copied list. Proposed test: `teamclass_ai_uses_copied_pointer_list_before_effect_loops`. | Do not model TeamClass script AI as the end-of-tick house AI generator. |
| Native Factory AI runs through `g_FactoryClass_Array` after Tactical and before House update, with live count reload; House update follows through `g_HouseClass_Array` with null guard and live count reload. | Factory `0x0055B675..0x0055B68B`, `0x004C9B20`; House `0x0055B698..0x0055B6B1`, `0x004F8440` | Rust production and owner AI are separated and ordered differently. | `src/sim/production/*`; `src/sim/house_state.rs`; `src/sim/world/mod.rs`. | Production progress/spend must occur before House update and after native late effects. | Factory completion and House build-decision in one tick sees native Factory progress first. Proposed test: `factory_ai_progress_precedes_house_update_in_pertick_ladder`. | Do not fold Factory AI into House AI or Rust strategic AI order. |
| Native reverse global loops for DiskLaser and RadSite snapshot count at loop start and walk high-to-low. | `0x0055B5A1..0x0055B5BC`; `0x0055B5CD..0x0055B5E8` | Rust lacks these native arrays/effects or would likely use stable-ID/entity order. | future DiskLaser/RadSite effect runtimes; `src/sim/superweapon/lightning_storm.rs` adjacent effects. | New entries appended during reverse loops wait until next pass; removals from high-to-low avoid skipping lower entries. | Two RadSites where high index expires/removes; lower index still ticks same pass. Proposed test: `radsite_reverse_snapshot_loop_removal_preserves_lower_tick`. | Do not use forward stable-ID order for these native reverse arrays. |
| Native click-feedback anims in `DAT_00A83E04` are gated by `g_GameMode != 0 && != 5` and tick after the main object vector but before alpha purge/crate/Tactical. | `0x004D7FF4..0x004D8058`; `0x0055B61B..0x0055B649` | Rust world effects tick at end of `advance_tick`, and app animation helpers snapshot stable IDs. | `src/sim/world/mod.rs:1797..1822`; `src/app_building_anim.rs`. | Model click-feedback anims as their own native array/order if gameplay/UI feedback parity is targeted. | Move-command click feedback appears/advances with native same-tick cadence in non-0/5 game modes. Proposed test: `click_feedback_anim_ticks_after_main_vector_before_tactical`. | Do not treat ordinary `g_AnimClass_Array` iteration as this secondary array. |

## 8. Negative Facts / Do Not Do

- Do not generalize the main LogicClass live-vector reload rule to all global loops. Team dispatch uses copied count; DiskLaser/RadSite use reverse snapshots; Factory/House use their own live-count globals. Active in YR: Yes.
- Do not tick Factory production from House update or end-of-tick strategic AI if claiming gamemd parity. Active in YR: Yes; Factory precedes House in `PerTickUpdate`.
- Do not claim `DAT_00A83E04` is ordinary `g_AnimClass_Array`; it is a conditional secondary anim/click-feedback array. Active in YR: Conditional.
- Do not use stable-ID `BTreeMap` order as a stand-in for Team, DiskLaser, RadSite, Factory, or House global arrays. Active in YR: Yes.
- Do not silently skip the unnamed direct helpers; mark them as unresolved/touched if implementation order depends on them.

## 9. Remaining Uncertainty

- Exact semantic names/classes for `FUN_0054E4D0`, `FUN_005FF390`/`DAT_00AC167C`, `FUN_00554D50`, and `FUN_0053D310` are not fully resolved in this slot.
- `TeamClass +0x5C` vtable target is proven as `0x006E9140`, but the function is not defined in the current Ghidra project, so its internals are deferred.
- `g_GameMode` values `0` and `5` were not enum-mapped here.

## 10. Stale Docs / Follow-up Wording

- [docs/research/ANIMCLASS_SPAWN_PATHS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_SPAWN_PATHS_GHIDRA_REPORT.md) or any doc saying ordinary per-tick AnimClass AI is "for each AnimClass in `g_AnimClass_Array`" should use: "Ordinary revealed AnimClass objects tick through the live LogicClass object vector. `PerTickUpdate` also has a later conditional `DAT_00A83E04` loop populated by `FootClass::ClickedAction_Cell` for click-feedback anims in `g_GameMode != 0 && != 5`; that secondary loop is not ordinary `g_AnimClass_Array`."
- Any timing overview that says "ore/bombs/lasers/factories/houses" without order should use: "`LogicClass::PerTickUpdate` order in the verified slice is growth, spread, bombs, unknown timed helper, Team scratch-list AI, DiskLaser reverse loop, unknown effect aging, LaserDraw update, LightningStorm, RadSite reverse loop, terrain/cache helper, EMP, main LogicClass vector, conditional click-feedback anims, wave/splash helper, alpha purge, crate regen, Tactical, Factory AI, House update, then last-ref-object handling."

## Sources

- Ghidra read-only decompile/disassembly: `LogicClass::PerTickUpdate @ 0x0055AFB0`; `0x0055B4D7..0x0055B6B1`.
- Ghidra read-only supporting functions: `TeamClass::Constructor @ 0x006E8A90`; `DiskLaserClass::Constructor @ 0x004A7A30`; `DiskLaserClass::AI @ 0x004A7340`; `RadSiteClass::Constructor @ 0x0065B1E0`; `RadSiteClass::AI @ 0x0065B800`; `FootClass::ClickedAction_Cell @ 0x004D7D50`; `AnimClass::Destructor @ 0x004228E0`; `FactoryClass::Constructor @ 0x004C98B0`; `FactoryClass::AI @ 0x004C9B20`; `HouseClass::Constructor @ 0x004F54A0`; `HouseClass::Update @ 0x004F8440`; `FUN_0054E4D0`; `FUN_005FF390`; `FUN_00554D50`; `FUN_0053D310`.
- Prior docs: [LOGICCLASS_PERTICKUPDATE_SCHEDULER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_PERTICKUPDATE_SCHEDULER_GHIDRA_REPORT.md); [BOMB_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BOMB_CLASS_GHIDRA_REPORT.md); [DISK_LASER_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DISK_LASER_CLASS_GHIDRA_REPORT.md); tiberium/radiation/WaveClass docs cited by research index.
- Rust read-only scan: `src/sim/world/mod.rs:1186..1835`; `src/sim/entity_store.rs:1..108`; `src/app_building_anim.rs:33`, `193`.
