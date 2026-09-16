# House Building-Attack Responder Design

## Goal

Implement the active YR `0x00708080` House/base-defence response as one synchronous, deterministic Rust transaction, including the minimum ground mission continuation required for the selected responders to behave correctly after assignment.

## Architecture Context

Damage is committed in native receiver order by `sim/combat/mod.rs::commit_damage_events_with_isolation`. The Building wrapper already owns its pre-shared-receiver `House+0x54D8` write in `apply_building_receive_prelude`; generic Techno post-Object work is committed later in the same loop. Combat temporarily owns `EntityStore`, Houses, both RNG streams and map authority, while `SimulationCombatInlineHooks` provides one non-aliasing bridge to world-owned state such as Team VM, ZoneGrid, playfield bounds and session mode.

Persistent type data flows from `RuleSet`/`ObjectType`; map-authored Unit/Infantry/Aircraft placement state flows through `MapEntity` into `GameEntity`. EntityStore iteration is stable creation order. Missions are persistent `MissionCom` state: receiver code can queue a deferred mission immediately, while `world/techno_ai/mission_handlers.rs` owns later ground mission dispatch. `TeamScriptVm` is the only existing TeamClass storage seam, but currently lacks priority/base-defence/suspension state and has no production Team producer. `ZoneGrid` retains native raw movement-row topology, but its public `can_reach` intentionally widens distinct zones and rejects the native source-outside-playfield special admit.

The research authority is [docs/research/PHASE3_HOUSE_BUILDING_ATTACK_RESPONDER_00708080_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_HOUSE_BUILDING_ATTACK_RESPONDER_00708080_GHIDRA_REPORT.md), based on live `gamemd.exe` callers/body and retail Rules. Older Rescue research is orientation only; its probability and Foot-caller reachability claims are superseded by the new direct proof.

## Impact Analysis

Expected owners/touchpoints:

- `src/rules/object_type.rs`: persistent `ToProtect=` type gate.
- `src/rules/ruleset.rs`: four response/suspension Rules inputs and native defaults.
- `src/map/entities.rs`, `src/sim/world/world_spawn.rs`: the two scenario recruitment bytes.
- `src/sim/game_entity.rs`: recruitment bytes, archive anchor, response cooldown; serde/hash-visible state.
- `src/sim/house_state.rs`: signed last-attacker House index, default `-1`.
- `src/sim/team_script_vm.rs`: TeamType priority/base-defence metadata, membership lookup, suspension transaction/timer and member removal.
- new `src/sim/combat/base_defense_response.rs`: pure native-order selection/assignment transaction and focused tests.
- `src/sim/combat/mod.rs`, `src/sim/world/mod.rs`: two exact receiver integration sites through the existing inline hook boundary.
- `src/sim/pathfinding/zone_map.rs` or a narrow response adapter: exact raw-zone equality and source-outside-playfield admit without changing general move-order semantics.
- `src/sim/world/techno_ai/mission_handlers.rs`: ground Rescue states and exact response-anchor continuation; shared AreaGuard corrections needed by assigned responders.
- snapshot/hash tests and focused combat/mission production-path tests.

The blast radius is deterministic state shape, save compatibility, Scenario RNG order, same-receiver mission/target visibility, and Team membership. Defaults plus `#[serde(default)]` are required for prior Rust snapshots. No app/render/audio system is involved.

## Chosen Approach

Use a Rust-native synchronous transaction module called from both receiver sites through one new method on `CombatInlineHooks`.

Combat continues to own and order the receiver. The hook receives borrowed combat authority (`EntityStore`, Houses, Scenario RNG, rules/interner/alliances) and combines it with world-owned Team/Zone/session state without a second global store or a deferred event. The module exposes small pure helpers for cooldown, threat, six-slot admission/sort, exact-5 fire legality and assignment; the top-level transaction alone mutates live state. Ground Rescue remains in the mission owner, keyed by the persistent archive anchor written by the transaction.

This follows the existing inline lifecycle/smudge/wave pattern and the existing `RuleSet -> MapEntity -> GameEntity -> snapshot/hash` flow. It deliberately does not reproduce C++ global arrays, vtables or raw pointers: stable IDs/references preserve identity, and category-filtered EntityStore construction order preserves the native Infantry-then-Unit scan.

## Player-Experience Detail Ledger

- `MILESTONE-BLOCKING` — Building response and `+0x54DC` write happen before shared Techno immunity/death; generic ToProtect response happens after Object HP/visual work and before the dead-state branch. Moving either to a later tick changes which units mobilize and what nested receivers observe. [GHIDRA `0x004422BC`, `0x007027AE..0x007027EE`]
- `MILESTONE-BLOCKING` — signed wrapping budget is attacker cost × `ComputerBaseDefenseResponse`; team suspension runs before the nonpositive-budget scan stop. [GHIDRA `0x00708080`; ini `ComputerBaseDefenseResponse=3`]
- `MILESTONE-BLOCKING` — candidate scan is Infantry construction order, then Unit construction order; candidate InLimbo is not an independent reject. [GHIDRA `0x00708198..0x007085DF`]
- `MILESTONE-BLOCKING` — fire admission compares exact full-int `GetFireError != 5` with range checking disabled. Treating ammo/busy/cannot/cloaked as false loses defenders; treating all represented fire errors as legal admits genuinely illegal target pairs. [GHIDRA `0x006FC0B0`, `0x00708276`, `0x007084AC`]
- `MILESTONE-BLOCKING` — reachability uses destination/tube-exit cells, signed truncation toward zero, responder bridge layer and raw zone equality; source outside playfield but in logical diamond admits. [GHIDRA `0x004DBDF0`, `0x0056D100`]
- `MILESTONE-BLOCKING` — threat uses current 3D position, `Sqrt_Approx/ftol`, signed `cost<<10`, special negative already-shooting result and class-specific self-anchor multipliers. [GHIDRA `0x004D97A0`]
- `MILESTONE-BLOCKING` — six-slot minimum starts at zero; full-list replacement duplicates the candidate into every old-minimum slot; stable signed descending sort preserves ties. This changes the responder set and draw count. [GHIDRA `0x00708340..0x0070868F`]
- `MILESTONE-BLOCKING` — Scenario RNG draws inclusive `0..99` for every selected occurrence, including forced AreaGuard Team members and duplicates; Rescue is `0..=65`, AreaGuard `66..=99`. [GHIDRA `0x007086DB..0x00708718`]
- `MILESTONE-BLOCKING` — mission queue, archive anchor and shoot target write before cost accumulation; equality continues and only strict overshoot stops/arms cooldown. [GHIDRA `0x00708718..0x007087B3`; ini `BaseDefenseDelay=.25`]
- `MILESTONE-BLOCKING` — cooldown is attacker-owned `(start,duration)` with signed wrapping elapsed semantics; native junk companion word must not become gameplay state. [GHIDRA `0x00708172..0x00708198`, `0x0070876F..0x007087B3`]
- `MILESTONE-BLOCKING` — Rescue target loss searches around the protected archive anchor, not responder position, then moves near the anchor and becomes AreaGuard. A nearest-first global scan or missing archive anchor visibly strands or redirects mobilized defenders. [GHIDRA `0x004DDF90`; doc: Phase3 responder report OQ-34]
- `MILESTONE-BLOCKING` — AreaGuard responders must retain a guard post/anchor and use native ring scan/tie order after the assigned attacker disappears. Current nearest-first stable-ID acquisition is outcome-changing in multi-target fights. [GHIDRA `0x004D6AA0`; current Rust `combat_targeting.rs` residual]
- `COMPOUNDING` — recruitment bytes A/B, House attacker index, archive/target/mission, Team suspension and cooldown must serialize/hash; missing one changes later calls or replay after load. [GHIDRA scenario readers/writers, House raw Save/Load]
- `COMPOUNDING` — low-priority Team suspension removes members in Team order and arms a 1800-frame timer before responder selection. Leaving entity-to-Team lookup separate from the VM creates two membership authorities. [GHIDRA `0x006EC250`; ini `SuspendPriority=1`, `SuspendDelay=2`]
- `COMPOUNDING` — AreaGuard's current deployed-infantry and idle-action residuals consume different Scenario RNG in ordinary play. A response can assign such Infantry, so those paths cannot be called closed while still reached by the mechanism. [GHIDRA `0x0051F640`, `0x0051CDB0`; current Rust mission handler ledger]
- `EXACTIFICATION-RESIDUAL` — Aircraft Rescue/drop-payload handler is outside this transaction because accepted responders are exactly Infantry/Unit. Trigger frequency through this helper is zero; no downstream risk. [GHIDRA `0x00708080` WhatAmI/pool scans]
- `EXACTIFICATION-RESIDUAL` — `ShouldProtect+0x3CF` remains zero under all YR writers; persistent support is unnecessary unless a live YR writer appears. `ToProtect=` covers the active generic caller. [GHIDRA constructor/writer inventory]
- `EXACTIFICATION-RESIDUAL` — compiler junk fields `Techno+0x654` and Team timer `+0x68` have no behavior/CRC reader. Storing them would manufacture deterministic state. [GHIDRA raw store/reader inventory]

## Design

### Components

`BaseDefenseResponseContext` is a short-lived bundle of mutable entity/House/Team/RNG authority and immutable rules/map/session facts. `respond_to_base_attack(victim_id, attacker_id, context)` owns literal entry order and mutations.

`BaseDefenseResponseCooldown { start_frame: i32, duration_frames: i32 }` defaults to `(current construction frame, 0)` for new live objects and uses a serde migration default equivalent to the native inactive sentinel. A helper implements wrapping remaining-time logic.

`archive_target: Option<TargetKind>` is separate from `attack_target`. It accepts entity/self and cell anchors and is used by Rescue/AreaGuard. The response writes an Entity victim anchor.

Map/Techno recruitment bytes use neutral exact names `recruitable_a` and `recruitable_b`, default true. No speculative Team semantics are encoded in their names.

Team state remains wholly in `TeamScriptVm`. `TeamTypeDefinition` gains signed priority and base-defence/recruitment facts; `TeamScriptState` gains suspension latches and signed timer pair. VM queries membership from its one ordered member list and removes/suspends members atomically.

`ResponderCandidate { entity_id, score }` is local-only. A six-element vector reproduces native append/replacement/duplicate/sort behavior without raw pointer arrays.

### Interfaces / Contracts

- `CombatInlineHooks::respond_to_base_attack(site, victim_id, attacker_id, ...)` is synchronous and returns only after every Team/entity/RNG write is visible.
- `ResponseCallSite::{BuildingPrelude, ProtectedTechno}` documents ordering and lets tests assert each integration point; it must not change helper gates.
- a narrow `ZoneGrid::can_reach_response_zone(...)` returns native equality/special-admit behavior and does not alter general pathfinding.
- a dedicated `responder_fire_error(...) -> i32` preserves enum identity. It may share lower-level compatibility checks, but only the response caller compares `== 5`.
- Mission Rescue reads/writes the common archive anchor and queues normal Move/AreaGuard through existing mission APIs; it does not mutate combat directly.

### Data Flow

1. Building prelude validates self-damage and shape, writes both House frame/index values, then calls the synchronous hook.
2. Generic receiver mutates HP/Object state; when receiver execution reached the callsite and `ToProtect` is set, it calls the same hook before fatal branching.
3. Transaction checks entry/cooldown, suspends Teams, scans and ranks candidates, then queues missions/writes anchors/targets while consuming Scenario RNG and accumulating cost.
4. Later mission dispatch commences the queued mission through existing MissionCom timing.
5. Rescue/AreaGuard use the stored archive anchor and shared exact ring scanner. Loss/detach of attacker clears only shoot target; archive anchor persists until the native mission clears it.

### Error Handling

Native constructed invariants become explicit no-op guards in Rust where a fault would be unsafe: missing victim/attacker/type/House or absent map zone data returns without mutation and is covered by tests. Supported production callers always supply them. No fallback may consume RNG, partially suspend Teams, or write a cooldown after such an invalid context. Signed overflow uses wrapping operations; no saturating arithmetic is allowed in the response transaction.

### Testing Strategy

Pure module tests pin cooldown boundaries, threat math, signed overflow, category order, first-six minimum-zero behavior, duplicate replacement, stable ties, exact-5 admission, forced-Team draw, budget equality/overshoot and repeated calls.

Parser/snapshot tests pin four Rules values, ToProtect, map bytes including absent defaults, House `-1`, archive self-reference, cooldown and Team suspension round trips/hash sensitivity.

Integration tests enter normal `commit_damage_events` for Building and ToProtect victims, assert native call ordering even on nullified/fatal hits, and verify per-receiver RNG/state visibility. A production-style `Simulation` test runs deterministic skirmish mode from damage through mission commence, attacker loss, Rescue anchor search/move and AreaGuard continuation.

Focused validation while working is `cargo test -p vera20k --lib <module/filter>`, after checking no other Cargo/rustc process owns the target directory. The phase-wide full `cargo test -p vera20k --lib` remains reserved for the end of the larger goal as required by `ENGINE.md`.

## Architectural Decisions

- Use the existing synchronous inline-hook authority bridge; do not create a deferred response queue.
- Keep one Team membership authority in `TeamScriptVm`; do not add `team_id` to GameEntity unless membership performance becomes a measured problem.
- Add a response-specific exact reachability method instead of changing the broader ZoneGrid API and its currently documented consumers in this slice.
- Preserve enum-shaped fire-error results instead of exposing a convenient boolean.
- Promote only mission behaviors reached by response-assigned ground units. Aircraft Rescue and TS-only fields remain proved exclusions.
- No new render/audio/UI dependency and no native inheritance emulation.

## Alternatives Considered

### Put the helper directly in `combat/mod.rs`

This avoids one module and hook method, but forces combat to own Team VM, ZoneGrid, playfield/session mode and mission continuation details. It enlarges an already central file, creates hidden world/combat coupling, and makes exact pure-helper tests harder. Rejected.

### Emit `BaseDefenseResponseEvent` and process it on the next world tick

This looks clean and avoids temporary authority sharing, but it changes same-receiver visibility, Team suspension timing, Scenario RNG order, fatal-hit behavior and repeated-call cooldown observation. Rejected for direct parity drift.

### Reuse generic `can_fire` and `ZoneGrid::can_reach`

This is the smallest patch, but both abstractions have deliberately different contracts: boolean fire legality collapses accepted non-5 errors, and general zone reachability lacks the asymmetric special admit and can widen distinct zones. Rejected because it silently changes responder membership.

## Autonomous Approval Record

Adversarial review asked three questions.

1. **Why approve this rather than defer Team/mission work?** Deferral would leave selected defenders assigned into missing or approximate behavior and would preserve no closed ordinary-player loop. The design keeps those dependencies inside the same mechanism without starting broader AI-team production.
2. **What could still make ordinary skirmish feel wrong?** Wrong responder choice, wrong target after attacker loss, wrong Rescue/AreaGuard movement, missing Team release, or RNG drift. Each is classified milestone-blocking above with an owning component/test; implementation must stop short of closure if any remains.
3. **What could create expensive later rework?** A second Team membership store, a boolean fire API, a general pathfinding semantic change, or a deferred response event. The chosen design explicitly avoids all four.

Decision: **APPROVED for autonomous implementation** under the active goal. Approval is conditional on the acceptance gate in the research report; no unresolved or approximate dependency may be relabeled complete.
