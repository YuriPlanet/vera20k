# Advance Tick Phase Partition Native Spine - Ghidra Research Report

**Address(es):** `LogicClass::PerTickUpdate @ 0x0055AFB0`; active caller `Main_Tick @ 0x0055D360`, call site `0x0055DC99..0x0055DC9E`; main object loop `0x0055B5FB..0x0055B619`; current Rust `Simulation::advance_tick @ src/sim/world/mod.rs:1508`
**Investigation Mode:** coverage-map
**Claimed Scope:** classify current Rust `World::advance_tick` phases into native global pre-object services, per-object `vtable+0x5C` AI migration candidates, and native global post-object services for the next tick-spine migration.
**Non-Scope:** class-specific BulletClass, AnimClass, and TechnoClass AI internals beyond spot-checking that they are live `vtable+0x5C` targets; direct removal-path audit; Rust implementation.
**Confidence:** High for `0x0055AFB0` top-level order, main object loop placement, and current Rust phase inventory; Medium for Rust phase-to-native-class mapping where a current Rust phase blends multiple native owners.
**Active in YR:** Yes. `Main_Tick` moves `ECX=0x87F778` and calls `0x0055AFB0` at `0x0055DC99..0x0055DC9E`; `0x0055AFB0` then calls global services, the live object vector, factories, and houses.

## 0. Investigation Contract

**Target question:** Which current `Simulation::advance_tick` phases should stay as native global pre/post `LogicClass::PerTickUpdate` services, and which should migrate under the live per-object `vtable+0x5C` AI pass?

**Non-goals:** Do not implement; do not re-prove the settled live-vector append/remove contract except as an anchor; do not audit every direct entity removal path; do not exhaust BulletClass/AnimClass/TechnoClass AI bodies owned by sibling slots.

**Evidence needed to mark COMPLETE:** direct Ghidra decompile of `0x0055AFB0`; assembly context for the object-loop and caller; Rust line inventory for `advance_tick`, `run_late_region`, and live vector APIs; implementation handoff with at least one safe migration slice.

**Stop conditions:** stop after every current `advance_tick` phase has a partition verdict, every handoff-critical claim has binary plus Rust evidence, and unresolved class-internal details are explicitly deferred.

## 1. Overview

The native spine is not equivalent to Rust's current staged pipeline. Active YR `LogicClass::PerTickUpdate` runs a fixed ladder: scenario/cell and timed global services, tiberium growth/spread, bombs, team/disk/laser/weather/rad/EMP services, then the main live object vector, then conditional pools, tactical, factory, house, and last-ref-object services.

Current Rust already has `LogicVector`, membership flags, `register_live_object`, `unregister_live_object`, and `for_each_live_object`, but `advance_tick` still drives most behavior through subsystem snapshots over `EntityStore`. The migration blocker is therefore not lack of a vector primitive; it is that movement, combat, mission, projectile, anim, production, and house AI work are still wired as class-wide phases.

## 2. Native Partition Anchors

| Native partition | Verified native work | Evidence | Active in YR |
|---|---|---|---|
| Global before main object vector | Scenario/cell actions and timers; one-tick scenario flag clears; shroud/fog/terrain-transition timed globals; bridge shroud `% 0x78`; `TiberiumClass` growth then spread; `BombClass::UpdateAll`; team scratch-list AI; DiskLaser reverse loop; light/laser/lightning/rad/EMP services. | `0x0055AFB0` decompile; growth/spread/bomb context `0x0055B4D7..0x0055B4F0`; EMP immediately before object loop `0x0055B5EC..0x0055B5F6`. | Yes, with TS-fog/shroud branches conditional on rule/scenario gates. |
| Main per-object AI pass | Forward live `LogicClass+0x04/+0x10` vector, call `vtable+0x5C`, reload count after call, increment index. | assembly context: `0x0055B608` item load, `0x0055B610` call, `0x0055B613` count reload, `0x0055B616..0x0055B619` increment/compare. | Yes. |
| Global after main object vector | Conditional `DAT_00A83E04` loop, WaveClass splash-force update, AlphaShape purge, crate regen, Tactical vtable+0x5C, FactoryClass array, HouseClass array, last-ref-object handling. | `0x0055B61B..0x0055B6B1`; Factory loop context begins at `0x0055B66A`; House loop follows. | Yes; conditional non-local loop is skipped when `g_GameMode == 0 || g_GameMode == 5`. |
| MainTick pre/post envelope | `Main_Tick` calls `PerTickUpdate` late, before service/network and frame increment; current Rust commits `binary_frame` late. | caller `0x0055DC99..0x0055DC9E`; current Rust late commit `src/sim/world/mod.rs:1496..1505`. | Yes. |

## 3. Current Rust Phase Partition Matrix

| Current Rust phase | Evidence | Native-spine partition verdict | Migration note |
|---|---|---|---|
| Owner index rebuild | `src/sim/world/mod.rs:1521..1524`; `EntityStore` owner index is cache-only at `src/sim/entity_store.rs:27..32`. | Rust-only cache prelude, not a native behavioral phase. | Keep outside parity ordering; must not become an observable ordering dependency. |
| Command application | `src/sim/world/mod.rs:1525..1530`; command sort/apply `1356..1402`. | Global pre-`PerTickUpdate` envelope. | Stays before native `PerTickUpdate` object/global services; command effects may append/reveal before the live pass. |
| Ground movement and gate runtimes | `1534..1566`. | Per-object AI candidate, mostly Techno/Foot/Unit locomotion plus object/mission effects. | Do not keep as one pre-object global movement pass for parity; migrate behind live-object order after class ownership is split. |
| Air/special movement: air, teleport, tunnel, rocket, homing, droppod, parachute, piggyback | `1567..1617`; several callees use `keys_sorted()` snapshots. | Per-object AI candidate, split by native class: Techno/Aircraft/Foot movement plus BulletClass projectile AI for rocket/homing-like paths. | Bullet-like projectile movement is the first safest slice; Techno/Aircraft locomotion remains broader. |
| Body rocking and slope transition | `1619..1631`. | Per-object AI candidate or render-state side effect of Techno/Locomotor AI. | Do not make it a global post-pass if it reads movement state that native writes inside each object's AI. |
| Aircraft mission state machines | `1633..1637`. | Per-object AI candidate. | Aircraft mission dispatch belongs with the object visit, not a class-wide post-movement pass. |
| Ship wake spawning | `1639..1688`; uses `keys_sorted()` at `1651`. | Per-object AI/render-effect candidate. | Native visual side effects should occur at the moving object's AI turn; current sorted snapshot is not native order. |
| Vision refresh | `1690..1701`. | Split/unchecked: not part of main object AI; some reveal/conceal happens inside object actions, but broad fog/shroud refresh is global service/app surface. | Keep as global only for surfaces proven not to be ObjectClass/MapClass side effects; current placement before combat is not native-proven. |
| Power states | `1703..1713`. | Split: House/Techno-derived state consumers, but power production/drain accounting is not the main live object loop itself. | Treat as a global or House/Building service until a HouseClass/BuildingClass trace assigns ownership. |
| Superweapons | `1714..1719`. | Split: LightningStorm process is global pre-object; HouseClass superweapon readiness/AI is post-object HouseClass; launch commands are pre-envelope. | Do not keep one monolithic `tick_superweapons` phase. |
| Deploy/fear/prone | `1721..1730`; deploy uses `keys_sorted()` in `src/sim/deploy.rs:80..81`, fear uses `src/sim/infantry.rs:130..135`. | Per-object AI candidate. | Infantry/Techno state changes should occur during that object's AI turn. |
| Bridge repair, capture, C4, order intents, attack pursuit | `1747..1756`; order code uses sorted snapshots in `src/sim/world/world_orders.rs`. | Per-object Techno/Mission/Radio AI candidate. | Local bridge skip surrogate is useful, but global migration should route these through live-object order. |
| Combat and turret rotation | `1760..1783`; combat still snapshots/sorts (`src/sim/combat/mod.rs:1174..1216`) but accepts `logic_order`. | Per-object Techno/Bullet/Weapon AI candidate, with nested snapshot helpers such as `Apply_area_damage` remaining local to detonations. | Do not leave firing/damage as one global combat phase for native scheduler parity. |
| Damage side-effect drains: bridges, walls, tiberium reduction, terrain, reveal, ejection, explosions, fire/radar events, smudges | `1784..1965`. | Split: many are immediate effects from object/BulletClass AI; smudge/tiberium reductions may feed later global ore services. | Need event-drain ownership before movement; current drain order is deterministic but not native-spine proof. |
| ParticleSystems | `1966..1969`. | Split/unchecked: native has global pre-object RadSite/light/laser services and Techno-owned particle pointers inside Techno AI. | Do not migrate all particles as a single object class without producer proof. |
| Retaliation/passengers/post-combat order intents | `1970..1975`; passenger currently uses live-order surrogate. | Per-object Techno/Building/Radio AI candidate. | Good candidate after Bullet/Anim primitives, but depends on Techno/Building mission semantics. |
| Production | `1991..1998`. | Global post-object `FactoryClass::AI`. | Must move after object AI and Tactical, before HouseClass, when native factory parity is targeted. |
| Repairs, building docks, aircraft docks | `1999..2001`. | Per-object Building/Techno/Radio AI candidate unless a traced Factory/House service owns a specific subcase. | Do not keep between production and ore as a single global tail phase without proof. |
| Ore growth/spread | `2002..2055`. | Global pre-object service. | This is the clearest global-order mismatch: native growth/spread at `0x0055B4D7/0x0055B4DC` before bombs/teams/object AI/factories/houses. |
| TIBTRE/terrain spawners | `2056..2076`. | Per-object TerrainClass/AnimClass-adjacent candidate, not proven as ore global. | Needs class-specific trace; do not bundle with TiberiumClass growth/spread. |
| Fog refresh after spawned entities | `2077..2079`. | Split global visibility service. | Keep outside object AI only for broad cache refresh; reveal side effects remain object-local. |
| AI player commands | `run_late_region 1418..1449`. | Global post-object HouseClass AI candidate, not main object AI. | Belongs after FactoryClass and within/after HouseClass update semantics; current immediate command application is a separate blocker. |
| Defeat detection | `1451..1456`. | Global post-object HouseClass/service. | Keep post-object; exact position should follow HouseClass/ShortGame trace. |
| Building-up/down animations | `1458..1462`. | Per-object BuildingClass/AnimClass candidate. | Do not keep as a generic post-House cleanup if it affects live object order or same-pass spawns. |
| Radar event aging and world-effect animation cleanup | `1464..1483`. | Split: tactical/UI/render post service, or AnimClass object AI for real anims. | App-only effects can stay above sim; parity AnimClass effects must migrate into object AI with first-AI guard. |
| Frame/tick commit and state hash | `1496..1505`, `2095..2103`. | Global post-envelope / Rust determinism service. | Late frame commit matches the native pre-increment visibility contract; state hash is Rust-only. |

## 4. First Safe Migration Slice

The safest first slice is not "move Phase 1 into `for_each_live_object`." That would mix Techno mission, locomotor, combat, RadioClass, and BuildingClass effects before class-specific contracts are ready.

Recommended sequence:

1. Keep command application and late frame commit where they are, and introduce a PerTickUpdate-shaped orchestration boundary with explicit `global_pre_object`, `object_ai`, and `global_post_object` regions. This can initially preserve behavior while making future moves reviewable.
2. Migrate authoritative BulletClass AI first: bullets are already proven to be same-pass sensitive, live-object scheduled, and removal-heavy. Move homing/rocket-like projectile behavior out of the class-wide special movement phase only after direct removal handling is centralized.
3. Migrate AnimClass AI second for first-AI guard and child-spawn/remove semantics. Same-pass visit is possible, but first visit must not be assumed to advance a frame.
4. Migrate TechnoClass/derived AI last and in narrow sub-slices: movement/mission dispatch, firing, capture/C4/passenger/radio, turret/facing, self-heal/damage, cloak, particle pointers, and building updates are all entangled in `TechnoClass::AI_Update @ 0x006F9E50`.

## 5. Negative Facts / Do Not Do

- Do not move the whole current `advance_tick` body under `for_each_live_object`. Active in YR: Yes; evidence: native global pre-object services at `0x0055AFB0..0x0055B5F6` and post-object services at `0x0055B61B..0x0055B6B1`.
- Do not keep tiberium growth/spread in Rust Phase 7 after production/repairs/docks when claiming native `PerTickUpdate` order. Active in YR: Yes; evidence: growth/spread calls `0x0055B4D7/0x0055B4DC` before the main object loop.
- Do not treat TeamClass/FactoryClass/HouseClass AI as the same category as main `LogicClass+0x04/+0x10` object AI. Active in YR: Yes; evidence: team scratch list before object loop; factories/houses after tactical at `0x0055B66A..0x0055B6B1`.
- Do not treat standard YR TS fog/shroud regrowth branches as always-on pre-object services. Active in YR: Conditional; evidence: `SpecialFlags & 0x1000` branch in `0x0055AFB0` and prior reports show standard YR fog off.
- Do not use sorted `EntityStore` snapshots as a migration substitute for live object order. Active in YR: Yes; evidence: native `0x0055B608..0x0055B619`; Rust snapshot APIs at `src/sim/entity_store.rs:100..110`, `live_object_order_snapshot` warning at `src/sim/world/mod.rs:740..746`.

## 6. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `Main_Tick` caller into `PerTickUpdate` | verified | `0x0055DC99..0x0055DC9E` | none |
| Native pre/object/post top-level partition | verified | `0x0055AFB0` decompile; assembly contexts around `0x0055B4D7`, `0x0055B608`, `0x0055B66A` | callee internals deferred |
| Rust `advance_tick` phase inventory | verified | `src/sim/world/mod.rs:1508..2108`, `run_late_region 1409..1505` | exact native owner for blended phases deferred |
| Live vector Rust primitives | verified | `src/sim/world/mod.rs:679..770`; `src/sim/world/logic_vector.rs` | integration into object AI pass missing |
| BulletClass AI suitability | touched-not-exhausted | `BulletClass::AI @ 0x004666E0`; same-tick bullet reports | sibling slot owns exact first slice |
| AnimClass AI suitability | touched-not-exhausted | `AnimClass::AI @ 0x00423AC0`; same-tick anim reports | sibling slot owns exact first slice |
| TechnoClass AI suitability | touched-not-exhausted | `TechnoClass::AI_Update @ 0x006F9E50` | sibling slot owns boundary and sub-slices |
| Direct entity removal bypasses | deferred | parent scope slot 2 | requires direct removal audit |

## 7. Open Questions - Final State

- `[RESOLVED] OQ-01 - Is `0x0055AFB0` active in YR? -> Yes, `Main_Tick` calls it with `ECX=0x87F778`.` (evidence: `0x0055DC99..0x0055DC9E`)
- `[RESOLVED] OQ-02 - Where is the main object AI pass relative to global services? -> It is after tiberium/bomb/team/laser/lightning/rad/EMP services and before conditional pool, wave, alpha, crate, tactical, factory, house, last-ref services.` (evidence: `0x0055B4D7..0x0055B6B1`)
- `[RESOLVED] OQ-03 - Does current Rust have live-vector primitives? -> Yes, but they are mostly not the main `advance_tick` execution spine.` (evidence: `src/sim/world/mod.rs:679..770`, `1508..2108`)
- `[RESOLVED] OQ-04 - Which current Rust phase is the clearest global pre-object mismatch? -> Ore growth/spread currently runs Phase 7 after production/repairs/docks, but native runs it before bombs/teams/object AI.` (evidence: `0x0055B4D7/0x0055B4DC`; `src/sim/world/mod.rs:2002..2055`)
- `[RESOLVED] OQ-05 - Which current Rust regions are object-AI candidates? -> Movement, special movement, missions, deploy/fear, combat/turret, capture/C4/repair/passenger/dock, building anims, BulletClass and AnimClass effects.` (evidence: Rust phase lines in Section 3; `BulletClass::AI @ 0x004666E0`; `AnimClass::AI @ 0x00423AC0`; `TechnoClass::AI_Update @ 0x006F9E50`)
- `[RESOLVED] OQ-06 - Which current Rust regions must stay global post-object? -> Factory production, House/AI/defeat, tactical/UI-like services, crate/alpha/wave-style services, late frame commit/state hash.` (evidence: native post-object `0x0055B61B..0x0055B6B1`; Rust `run_late_region`)
- `[DEFERRED] OQ-07 - Which direct entity removal paths still bypass unregister?` (category: `requires-different-system-context`; reason: sibling slot owns direct removal audit; next-step-if-pursued: inspect every `EntityStore::remove` and `entities.remove` call.)
- `[DEFERRED] OQ-08 - Exact BulletClass first migration subset.` (category: `requires-different-system-context`; reason: sibling slot owns BulletClass; next-step-if-pursued: use `BulletClass::AI @ 0x004666E0`, `Fire @ 0x00468670`.)
- `[DEFERRED] OQ-09 - Exact AnimClass first migration subset.` (category: `requires-different-system-context`; reason: sibling slot owns AnimClass; next-step-if-pursued: use `AnimClass::AI @ 0x00423AC0` and first-AI guard report.)
- `[DEFERRED] OQ-10 - Exact TechnoClass boundary.` (category: `requires-different-system-context`; reason: Techno AI is broad and entangled; next-step-if-pursued: use `TechnoClass::AI_Update @ 0x006F9E50` plus Unit/Infantry/Building wrappers.)

## 8. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Native has explicit pre-object, live object, and post-object regions; global services are not all outside or all after object AI. | `0x0055AFB0` decompile; object loop `0x0055B608..0x0055B619`; post services `0x0055B61B..0x0055B6B1` | Missing as an orchestration boundary: Rust comments name early/late regions, but phases still execute as staged subsystem passes. | `src/sim/world/mod.rs::advance_tick`, `run_late_region`, future scheduler wrapper. | Create a migration scaffold that can host pre-object services, `for_each_live_object`, and post-object services without moving behavior until each slice is verified. | With test hooks for one pre-service, one object AI, and one post-service, assert order is pre -> live object order including same-pass tail append -> post. Proposed test: `native_spine_runs_global_pre_then_live_object_ai_then_global_post`. | Do not migrate phases by bulk moving comments; the observable call order and same-pass mutation contract must change only in verified slices. |
| Tiberium growth/spread are global pre-object services before bombs, teams, object AI, factories, and houses. | Ghidra calls `0x0055B4D7` and `0x0055B4DC`; Rust ore phase `src/sim/world/mod.rs:2002..2055` | Mismatch: Rust runs ore after combat, particles, passengers, production, repairs, and docks. | `src/sim/world/mod.rs`, `src/sim/ore_growth.rs`, smudge/tiberium drain boundaries. | Move only the verified native growth/spread driver into the pre-object region after proving smudge/damage drains that feed ore density are placed correctly. | A frame where ore growth, a bullet detonation, and factory completion are all due: native growth/spread consumes RNG and mutates ore before object AI/factory work. Proposed test: `pertick_ore_growth_precedes_object_ai_and_factory_completion`. | Do not bundle `TIBTRE`/terrain spawners into the TiberiumClass growth/spread move without a TerrainClass/AnimClass trace. |
| BulletClass and AnimClass are live `vtable+0x5C` object-AI candidates; TechnoClass AI is live but much broader. | Bullet AI `0x004666E0`; Anim AI `0x00423AC0`; Techno AI update `0x006F9E50`; scheduler call `0x0055B610` | Missing globally: rocket/homing/projectile movement, world effects, combat, movement, and building animations are separate phases/snapshots. | `src/sim/movement/rocket_movement.rs`, `homing_movement.rs`, `src/sim/components.rs::WorldEffect`, combat projectile surfaces, future object-AI dispatch. | First migrate authoritative BulletClass AI; then AnimClass first-AI guard; defer TechnoClass until narrower sub-slices exist. | Bullet fired by an object before the old live-vector tail can receive first AI same pass; same-pass AnimClass first visit clears guard without advancing frame; Techno migration test waits for a single sub-slice. Proposed tests: `bullet_ai_tail_append_runs_same_pass`, `anim_ai_same_pass_first_visit_does_not_advance_frame`, `techno_ai_movement_subslice_preserves_live_order`. | Do not start with TechnoClass as a whole; it includes mission dispatch, firing, self-heal/damage, cloak, particles, and building interactions. |

## 9. Stale Docs / Follow-up Docs

- [docs/research/FRAME_COUNTER_PREINCREMENT_VS_RUST_BINARY_FRAME_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FRAME_COUNTER_PREINCREMENT_VS_RUST_BINARY_FRAME_GHIDRA_REPORT.md) contains stale current-Rust claims that `binary_frame` is computed at the start of `advance_tick` (examples: lines 11, 25, 83, 85, 112, 143 in current grep output). Replace with: `Current Rust now commits binary_frame late in Simulation::run_late_region at src/sim/world/mod.rs:1496..1505, so the earlier start-of-advance_tick mismatch is resolved; remaining timing work should focus on pause/menu gating and per-system placement.`
- [docs/research/MAIN_TICK_FRAME_COUNTER_PLACEMENT_VS_ADVANCE_TICK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAIN_TICK_FRAME_COUNTER_PLACEMENT_VS_ADVANCE_TICK_GHIDRA_REPORT.md) already has audit notes correcting the late-commit fix, but still has stale open-log and handoff rows at lines 114, 129, and 142 in current grep output. Replace those rows with the same late-commit wording above.
- [docs/research/LOGICCLASS_VS_MAPCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_VS_MAPCLASS_GHIDRA_REPORT.md) lines around 386..506 still describe Rust `World::advance_tick` as matching or being a richer implementation. Replace with: `World::advance_tick is not a parity-equivalent implementation of LogicClass::PerTickUpdate unless each native pre-object, live-object, and post-object service is placed in the verified order. Active YR PerTickUpdate runs tiberium growth/spread and several global services before the main live object vector, then tactical/factory/house services after it.`
- [docs/research/FACTORY_CLASS_BUILD_SPEED_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FACTORY_CLASS_BUILD_SPEED_GHIDRA_REPORT.md) section `LogicClass::AI - Global Tick Order (0x0055AFB0)` should be retitled `LogicClass::PerTickUpdate - Global Tick Order (0x0055AFB0)` and its compressed order should be replaced by the ladder in [PERTICKUPDATE_FULL_ORDERING_LADDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PERTICKUPDATE_FULL_ORDERING_LADDER_GHIDRA_REPORT.md).

## Sources

- Ghidra read-only decompile: `LogicClass::PerTickUpdate @ 0x0055AFB0`, `BulletClass::AI @ 0x004666E0`, `AnimClass::AI @ 0x00423AC0`, `TechnoClass::AI_Update @ 0x006F9E50`.
- Ghidra assembly context: `0x0055B4D7`, `0x0055B608`, `0x0055B613`, `0x0055B66A`, `0x0055DC99`.
- Prior reports: [PERTICKUPDATE_FULL_ORDERING_LADDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PERTICKUPDATE_FULL_ORDERING_LADDER_GHIDRA_REPORT.md), [LOGICCLASS_GLOBAL_SUBSYSTEM_ORDER_0055AFB0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_GLOBAL_SUBSYSTEM_ORDER_0055AFB0_GHIDRA_REPORT.md), [LOGICCLASS_PERTICKUPDATE_SCHEDULER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_PERTICKUPDATE_SCHEDULER_GHIDRA_REPORT.md), [SAME_TICK_SPAWNED_OBJECT_BEHAVIOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SAME_TICK_SPAWNED_OBJECT_BEHAVIOR_GHIDRA_REPORT.md).
- Rust source read-only: `src/sim/world/mod.rs`, `src/sim/entity_store.rs`, `src/sim/deploy.rs`, `src/sim/infantry.rs`, `src/sim/combat/mod.rs`, `src/sim/world/world_orders.rs`.

## Status

COMPLETE for phase partition coverage-map and implementation handoff. Class-specific exact migration plans are intentionally deferred to sibling BulletClass, AnimClass, TechnoClass slots.
