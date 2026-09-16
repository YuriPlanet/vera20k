# Scheduler-Backed AnimClass and Damage Fire Design

## Goal

Add the smallest generic, simulation-owned `AnimClass` runtime that preserves the verified live-object scheduling and lifecycle contract, then use building damage fire as its first production consumer.

Revision note: the targeted `AnimType.End` investigation in [ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md) supersedes older reports that considered only the later `End==-1` constructor/AI fallback. Normal `AnimTypeClass` art loading also resolves an omitted zero `End` from the SHP header before explicit `End=` is parsed.

## Architecture Context

Native YR keeps `AnimClass` instances in their own registry while revealed animations also participate in the shared `LogicClass` live-object vector. `LogicClass::PerTickUpdate` reloads the vector length after every object visit, so a child appended at the tail can be visited in the same pass. Concealment removes an entry by order-preserving compaction without repairing the current cursor. `AnimClass::Destroy` proceeds through uninit/conceal and deferred deletion.

Rust already has the important scheduler primitives:

- `ObjectSubstrate::logic` is the insertion-ordered `LogicVector`.
- `Simulation::for_each_live_object` implements tail-growth visibility and compacting-removal cursor behavior.
- `Simulation::uninit` and `flush_pending_delete` provide the two-phase death window for game entities.
- `AnimClassSpawnDescriptor` preserves verified constructor arguments.
- The app-side `AnimRuntime` mirrors part of the ordinary first-AI, timer, loop, `Next`, and trailer behavior, but it is not simulation-owned or scheduler-backed.

Current building damage fire is created and advanced after simulation in `app_building_anim::tick_damage_fire_overlays`. It stores a vector on `GameEntity`, always selects `ConditionYellow`, and renders directly from pixel offsets. Native creation instead occurs near the start of each `BuildingClass::Update`, stores eight `AnimClass*` slots, consumes the scenario RNG, and destroys each animation through its lifecycle path.

Relevant existing surfaces:

- `src/sim/world/substrate.rs`: object identity, live order, stores, pending deletion.
- `src/sim/world/mod.rs`: lifecycle APIs, tick spine, cache rebuild and state hash boundary.
- `src/sim/world/techno_ai.rs`: per-live-object category dispatch; the structure arm is currently empty.
- `src/sim/components.rs`: `AnimClassSpawnDescriptor`, `AnimRuntime`, `WorldEffect`, and transitional damage-fire overlay types.
- `src/sim/game_entity.rs`: building cached visual state and damage-fire overlays.
- `src/rules/art_data.rs`: parsed animation lifecycle, draw metadata, sounds, and building fire offsets.
- `src/app_building_anim.rs`: current damage-fire owner and reusable ordinary lifecycle logic.
- `src/app_instances/overlays.rs`: current damage-fire and world-effect instance generation.
- `src/sim/world/world_hash.rs` and `src/sim/snapshot.rs`: authoritative hashing and persistence versioning.

## Impact Analysis

### Direct changes

- Add a simulation-owned animation object type and a separate animation registry under `ObjectSubstrate`.
- Generalize live-object dispatch, membership checks, deferred deletion, save/load reconstruction, debug invariants, and hashing to cover both entity and animation registries.
- Add a distinct cached damage-fire state and eight fixed animation-ID slots to structures.
- Resolve native `AnimType.End`/`LoopEnd` from full SHP header counts plus explicit-key presence during initialization; do not use the renderer's unconditional half-count cache as gameplay metadata.
- Move the reusable ordinary `AnimRuntime` logic from the app layer into a sim module and drive it once per live-object visit.
- Make the structure live-object arm own damage-fire threshold transitions and creation.
- Render damage fires from live `AnimObject` state and remove the app-side damage-fire tick.
- Add ID-addressed animation start/stop sound events so audio cannot own simulation lifetime.
- Increment `SNAPSHOT_VERSION` once for the coordinated persisted-state change.

### Dependent behavior

- The scenario RNG stream moves because damage-fire draws occur during live-object dispatch instead of after `advance_tick`.
- A revealed animation changes `LogicVector` order and therefore subsequent same-pass visits.
- Animation destruction participates in the common pending-delete drain.
- Save/load and replay hashes must include the animation registry, slot IDs, cached state, and shared live order.
- The renderer and audio layer become read/consume-only clients of authoritative animation state.

### Explicit non-goals

- Do not replace all `WorldEffect` producers.
- Do not migrate the separate 21-slot building animation system.
- Do not implement bouncer physics, meteor impact, `BounceAnim`, or `ExpireAnim` in this slice.
- Do not add animations to `EntityCategory` or make them pretend to be `GameEntity` values.
- Do not bulk-migrate Techno, Bullet, movement, or combat phases into live-object dispatch.
- Do not claim full damage-fire parity while cross-object combat timing and framebuffer/depth equivalence remain uncertified.

### Known blockers outside this design

- Rust combat remains a staged phase after the current object-AI walk. Native damage occurring before or after a particular building's live visit can therefore differ in same-pass visibility until combat/projectile ownership migrates.
- The current renderer uses a normalized floating depth representation rather than a certified equivalent of native object submission and `AnimClass::DrawIt` ordering. This design preserves native inputs and removes render-owned state, but pixel parity still requires the named draw/depth checks.
- Exact downstream mixer timing for every `StopSound` branch remains partially unresolved. The event/identity contract must be correct now without claiming the mixer is certified.

## Chosen Approach

Add a separate `AnimStore` backed by `BTreeMap<u64, AnimObject>` beside `EntityStore`, while retaining one globally unique stable-ID namespace, one `LogicVector`, and one pending-delete queue.

This maps the native separation directly into Rust-native ownership:

```text
EntityStore ----\
                 +-- global stable IDs -- LogicVector -- per-object dispatch
AnimStore ------/                            |
                                             +-- PendingDelete
```

`LogicVector` remains `Vec<u64>`; it does not need a tagged handle because IDs cannot collide. Dispatch resolves an ID against the entity registry or animation registry. `EntityCategory` remains the four gameplay categories and does not acquire a presentation-only variant.

Damage fire is the first and only production integration in this design. The generic shell implements the verified ordinary non-bouncer lifecycle needed to avoid a damage-fire-specific second animation model: constructor/reveal, first-AI guard, native frame timers, loop byte, ping-pong/reverse, in-place `Next`, periodic trailer creation, normal destroy/unregister, and deferred deletion.

## Tiny-Detail Ledger

Each item is an implementation constraint, not optional polish.

| Constraint | Required behavior | Evidence |
|---|---|---|
| Registry versus scheduler | `AnimStore` owns animation storage; `LogicVector` alone owns live AI order. | [ANIMCLASS_AI_FIRST_SAFE_MIGRATION_SLICE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_AI_FIRST_SAFE_MIGRATION_SLICE_GHIDRA_REPORT.md), sections 3.1-3.2 |
| Shared identity | Entity and animation IDs come from one monotonic, non-reusing source. | Native object pointer identity; existing `ObjectSubstrate` stable-ID contract |
| Tail append | Reveal appends once at the live-vector tail; an appended child may receive a same-pass visit. | `LogicClass::PerTickUpdate @ 0x0055AFB0`; scheduler report section 3.1 |
| Compact removal | Conceal removes by stable order-preserving compaction; the cursor is not repaired. | remover `0x0055BAE0`; scheduler report section 3.5 |
| Separate registry lifetime | Construction inserts into the animation registry before reveal/live registration. | `AnimClass::Constructor @ 0x00421EA0` |
| Reveal before `Middle` | Normal zero-delay construction reveals/registers the animation before the final constructor-time `Middle` call. | `AnimClass::Constructor @ 0x00421EA0`; scheduler report section 3.2 |
| First-AI guard | The first visit clears the guard and returns before delay, timer, frame, loop, `Next`, or normal destroy. | `AnimClass::AI @ 0x00423AC0`; lifecycle report |
| Trailer position in visit | Trailer gating and child construction occur before the first-AI guard. | [ANIMCLASS_AI_TRAILERANIM_PERIODIC_SPAWNS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_AI_TRAILERANIM_PERIODIC_SPAWNS_GHIDRA_REPORT.md) |
| Rate conversion | Art `Rate > 0` becomes integer `900 / Rate`; `Rate <= 0` becomes zero and blocks normal advancement. | [TICK_AND_ANIMATION_SPEED_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TICK_AND_ANIMATION_SPEED_GHIDRA_REPORT.md), sections 3 and 6 |
| AnimType asset-load order | A fresh type starts with `End=0`, `LoopEnd=0`, and `Shadow=false`. `Shadow` is read first; the SHP loader then replaces zero `End` with signed header count at `+6`, halved only for `Shadow=yes`, and copies zero `LoopEnd` from that result. Explicit `End=` and `LoopEnd=` are read afterward and override the loaded values. | [ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md), sections 3.1-3.2 |
| Explicit-zero distinction | Omitted `End=` and explicit `End=0` are not equivalent: omission preserves the loader-derived value, while explicit zero resets it and suppresses damage-fire frame RNG. Parser/runtime metadata must retain key presence. | same report |
| Constructor `End=-1` fallback | Explicit `End=-1` is resolved from the signed SHP header during `AnimClass` construction, halved only for `Shadow`, before the damage-fire caller reads the type. | same report; `AnimClass::Constructor @ 0x00421EA0` |
| Frame clock | Animation AI uses native logic visits and the committed binary frame where specified, never render delta time. | [TICK_AND_ANIMATION_SPEED_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TICK_AND_ANIMATION_SPEED_GHIDRA_REPORT.md) |
| Loop width | Remaining loops are a byte; constructor multiplication wraps as a byte, values below 2 clamp to 1, and `0xFF` is infinite. | [ANIMCLASS_AI_LIFECYCLE_EXACT_SUBSET_RESWARM_20260527.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_AI_LIFECYCLE_EXACT_SUBSET_RESWARM_20260527.md) |
| `Next` identity | `Next` mutates the same animation object and stable ID; it is not a child spawn. | same lifecycle report, `Next` section |
| Constructor row | Damage fire uses `delay=0`, `loop=1`, `drawFlags=0x600`, `facing=0`, `z=0`. | [BUILDINGCLASS_DAMAGE_FIRE_SELECTOR_RNG_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_DAMAGE_FIRE_SELECTOR_RNG_GHIDRA_REPORT.md), section 2 |
| Threshold selector | `CanBeOccupied=false` selects `ConditionYellow`; true selects `ConditionRed`. | same selector report, sections 1-2 |
| Inclusive threshold | Active when health ratio is less than or equal to the selected threshold. | selector report, threshold comparison finding |
| Threshold arithmetic | Compare integer health against an exact precomputed integer cutoff for the selected threshold; do not divide or use `f32`/`f64` in simulation and do not reuse the lossy generic `x1000` shortcut without a proof. For stock YR, `ConditionYellow=50%` and `ConditionRed=25%` reduce exactly to `health*2 <= strength` and `health*4 <= strength`. | `ObjectClass::GetHealthRatio @ 0x005F5C60`; stock `rulesmd.ini`; project simulation rule |
| Cached transition | No spawn/clear occurs when computed and cached damage-fire states agree. The cached state changes even when creation yields no slots. | `BuildingClass::Update @ 0x0043FB20` |
| Empty type list | A zero `DamageFireTypes` count returns before RNG and slot scanning. | selector report, section 2 |
| Initial type RNG | With a nonempty type list, call scenario `RandomRanged(0,count-1)` before scanning the first slot. Equal bounds consume no draw. | selector report; `RandomRanged @ 0x0065C7E0` |
| Fixed slots | Store exactly eight `Option<AnimId>` slots, independent from the 21 normal building slots. | `Building+0x5C8..+0x5E4`; lifecycle report |
| Slot early return | Stop at the first sentinel/missing offset or occupied slot; never skip a hole or refill later slots. | selector report, slot-scan findings |
| Per-slot ordering | Construct (including delay-zero `Middle`/start effects), store the ID, write `zAdjust`, read the constructed type's resolved `End`, optionally select the start frame, then increment/wrap the type index. | `CreateDamageFireAnims @ 0x0043C0D0`; [ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md) |
| Frame RNG | For positive native-resolved signed `AnimType.End`, draw scenario `RandomRanged(0,End-1)`; equal bounds consume no draw. Non-positive `End` consumes none. Stock resolved values are derived as `FIRE01=30`, `FIRE02=64`, `FIRE03=30`, giving inclusive bounds `0..29`, `0..63`, `0..29`. | [ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md), sections 3.3-3.4 |
| Ranged raw-word consumption | `RandomRanged` uses mask/rejection: the stock initial `0..2` and FIRE01/FIRE03 `0..29` calls can consume multiple raw words; FIRE02 `0..63` consumes one. Tests compare the complete scenario RNG state, not an assumed one-word-per-call ledger. | same report; `RandomRanged @ 0x0065C7E0` |
| Type advancement | Advance/wrap the fire type only after a successful construction. | selector report, type-selection finding |
| Coordinate frame | Convert each isometric pixel offset to world/lepton coordinates, then add the building render coordinate using native signed conversion/rounding. | [BUILDINGCLASS_DAMAGE_FIRE_SELECTOR_RNG_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_DAMAGE_FIRE_SELECTOR_RNG_GHIDRA_REPORT.md); damage-fire trace |
| `zAdjust` | `(((offsetY - (foundationHeight + foundationWidth) * 15) * 3) >> 1) - 10`, clamped to at most zero, with signed shift semantics preserved. | selector report and [BUILDING_DAMAGE_FIRE_SLOT_RNG_ZADJUST_TRACE_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/BUILDING_DAMAGE_FIRE_SLOT_RNG_ZADJUST_TRACE_20260528.md) |
| Repair timing | Repair later in the same building update cannot clear fires until the next building visit. | [BUILDING_DAMAGEFIRE_SLOT_CLEAR_DESTROY_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_DAMAGEFIRE_SLOT_CLEAR_DESTROY_LIFECYCLE_GHIDRA_REPORT.md) |
| Cleanup ordering | For every occupied slot, call animation destroy/uninit and then null that slot, in slot order. Repeated clearing is a no-op. | same lifecycle report |
| Cleanup entry points | Threshold recovery, zero-health update, destruction effects, sell state 0, limbo, and destructor all clear the same slots. | same lifecycle report, lifecycle matrix |
| Owner change | Changing building owner neither clears nor reattaches damage-fire animations. | same lifecycle report, `ChangeOwner` finding |
| No owner attachment | Damage-fire animations are fixed-world objects, not `SetOwnerObject` children. | same lifecycle report |
| Sound ownership | Delay-zero constructor start/report sound occurs through animation start; destroy releases the active animation sound and may emit `StopSound`. Audio handles are keyed by animation ID and cannot control sim lifetime. | [ANIMATION_SOUNDS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMATION_SOUNDS_GHIDRA_REPORT.md); lifecycle destroy report |
| Deferred deletion | Destroyed animations leave live order before entering the shared pending-delete queue and remain registry-resolvable until the tail drain. | `AnimClass::Destroy`, `ObjectClass::UnInit`, existing substrate contract |
| Save/load | Store, slots, runtime fields, membership flags, shared live order, and RNG state round-trip. Pending delete is empty at supported save boundaries. | native-significant state plus current snapshot contract |
| Hashing | Every authoritative animation field and slot/order relationship changes `state_hash`; derived render caches do not. | project parity certification rules and `world_hash.rs` pattern |
| Retail metadata failure | Missing required stock AnimType or SHP metadata is initialization failure, not a silent one-frame runtime fallback. Native missing-SHP plus omitted `End` leaves `End=0` and consumes no damage-fire frame RNG; any accepted mod-data path must preserve that behavior rather than forcing one frame. Other malformed mod behavior stays `UNCHECKED`. | [ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md); sources-of-truth and verification discipline |

## Design

### Components

#### `AnimObject`

Add a focused sim module, expected at `src/sim/anim_class.rs`, containing the simulation-owned runtime. `AnimObject` contains only native-significant or lifecycle-required state:

- stable ID and current interned AnimType ID;
- signed world coordinate in leptons (`glam::IVec3` or an equivalent integer wrapper);
- constructor draw flags, instance `z_adjust`, and reverse state;
- constructor-observable signed `effective_end` and `effective_loop_end`, refreshed on in-place `Next` transitions;
- current frame, signed frame step, constructor delay, frame-delay reload/counter, loop byte, first-AI guard, inactive/alive state;
- live-vector membership byte;
- start-sound emission/active-sound state needed to produce ID-addressed audio edges.

Do not store screen coordinates, GPU handles, atlas entries, wall-clock elapsed milliseconds, or app audio handles in `AnimObject`.

The existing `AnimRuntime` logic should move into this sim module rather than be copied. App-only elapsed-millisecond state is deleted. Art configuration remains in the existing `RuleSet::art_registry`; do not add a parallel compiled animation catalog.

Extend the existing `AnimTypeRuntimeConfig` representation with the minimum presence information needed to distinguish omitted `End=`/`LoopEnd=` from explicit zero. During initialization, bind the full signed-compatible SHP header frame count to that existing configuration and compute the **post-ReadINI loaded values** in loader-then-INI order. Bind the asset counts into the canonical `ArtRegistry` before assigning the retained `RuleSet::art_registry` clone so app and sim cannot observe different records. These loaded values may still be `-1`; constructor/AI/`Next` resolution of `-1` happens when the `AnimObject` is initialized or changes type, producing its `effective_end`/`effective_loop_end` fields. Damage-fire creation reads the constructed object's effective value, matching the native caller-after-constructor observation without adding a mutable parallel type catalog. `AnimType+0x298`'s unconditional half-count behavior may be retained separately only if a verified consumer needs that distinct field; it must never stand in for `End`.

The atlas packs the full raw SHP frame range for these resolved types. Simulation alone applies `End`, `LoopEnd`, and `Shadow` semantics; packing extra raw frames is presentation availability, not gameplay metadata.

For this production slice, initialization must resolve the transitive `Next`/`TrailerAnim` closure reachable from `DamageFireTypes`. The resolved art registry is external metadata rebound during initialization/load; serialized `AnimObject` state keeps the current type ID and runtime fields but does not serialize asset bytes or duplicate the catalog.

#### `AnimStore`

Add `AnimStore` as a thin owner of `BTreeMap<u64, AnimObject>`, or use the map directly if no invariant-bearing wrapper is needed during implementation. It lives in `ObjectSubstrate` beside `EntityStore` and is serialized.

The global allocator currently named `next_stable_entity_id` becomes the object-ID allocator. Rename only the field and direct accessors required to make its broader authority honest; do not perform unrelated ID refactors.

#### Building damage-fire state

Add to `GameEntity`:

- `damage_fire_state_active: bool`, distinct from the existing yellow-health building visual gate;
- `damage_fire_slots: [Option<u64>; 8]`.

The existing `damage_fire_overlays` field and `DamageFireOverlays`/`DamageFireAnim` types are removed only after the renderer reads `AnimObject` values. Do not reuse `building_damage_state_active`: its current yellow-tier consumers have a different contract from the `CanBeOccupied`-selected damage-fire threshold.

#### Audio events

Extend the sim-to-app sound boundary with animation-ID-addressed start and stop/release events. The app may map `anim_id` to an active playback handle, but missing playback or an inaudible/camera-culled sound must not feed back into sim state.

#### Render view

Expose immutable animation iteration or lookup sufficient for the renderer to obtain type ID, world coordinate, effective frame, draw flags, layer inputs, and `zAdjust`. Rendering must not mutate animation state.

Damage-fire rendering may follow building slot IDs to preserve slot ownership and filter this first consumer. Do not broaden this slice into replacing every `WorldEffect` render path. Preserve the native integer inputs even where the current renderer still needs a later depth-certification change.

### Interfaces / Contracts

Expected focused lifecycle APIs on `Simulation`:

- `spawn_anim(descriptor, rules) -> AnimId`: construct registry state, reveal/register when native-eligible, then run delay-zero start/`Middle`-equivalent effects before returning.
- `visit_anim(anim_id, rules)`: execute one live-object AI visit, including trailer-before-guard, ordinary lifecycle and `Next`.
- `destroy_anim(anim_id)`: release/stop sound state, conceal from `LogicVector`, mark inactive/dying, and enqueue pending deletion.
- `clear_damage_fire_slots(building_id)`: destroy then null occupied slots from 0 through 7; idempotent.
- `update_building_damage_fire(building_id, rules)`: compute/cache the early transition and invoke creation/clear.

Names may change during planning to match local conventions, but authority and ordering must not.

Entity lifecycle APIs remain entity-specific where occupancy, house counts, radio links, and passengers are involved. Do not force animation teardown through entity-only cleanup. The shared mechanisms are stable identity, `LogicVector` removal, and pending deletion.

`flush_pending_delete` removes each queued ID from whichever registry owns it. Debug assertions require every live ID to resolve to exactly one registry, and require the corresponding membership byte to agree with `LogicVector`.

### Data Flow

#### Animation construction and scheduling

1. A verified producer builds an `AnimClassSpawnDescriptor` with an interned type and signed lepton coordinate.
2. `spawn_anim` allocates one global ID and inserts `AnimObject` into `AnimStore`, mirroring constructor registry insertion.
3. Constructor runtime fields are initialized from the presence-aware post-ReadINI record in `RuleSet::art_registry`, using native widths/conversions. Explicit `End=-1`/`LoopEnd=-1` resolve into the new object's effective fields at this constructor point before the caller can inspect them.
4. Reveal sets membership and appends the ID to `LogicVector` exactly once.
5. Delay-zero start/`Middle`-equivalent sound and state edges occur before the constructor returns.
6. `for_each_live_object` may reach the new tail ID later in the same pass.
7. The first visit clears the guard without advancing the frame.

#### Live dispatch

The existing object-AI walk resolves each live ID:

- if it belongs to `EntityStore`, use the existing four-category entity dispatch;
- otherwise, if it belongs to `AnimStore`, call `visit_anim`;
- otherwise, tolerate the absent ID defensively but fail debug invariants at the tick boundary.

Lookup order is safe because the ID namespace is globally unique. No `EntityCategory::Animation` arm is added.

#### Damage-fire creation

The structure arm calls `update_building_damage_fire` before later building/Techno work represented by that arm. To avoid mutable-borrow aliasing:

1. Read building health, type, foundation, current cached state, slot occupancy and art offsets into a small fixed spawn plan.
2. Release the building borrow.
3. Consume the initial type-selection RNG before checking the first slot, then process eligible slots in native order.
4. For each slot, call `spawn_anim`; after its constructor returns, immediately reborrow the building and store the returned ID in that slot.
5. Write the new `AnimObject`'s `zAdjust`, then read its `effective_end`; call `RandomRanged(0,End-1)` only when it is positive and store the result as the current frame.
6. Advance/wrap the fire type only after that successful construction, and update the building's cached state after creation/clear handling finishes.

The plan must use a fixed array/count, not a heap vector in the per-building hot path. It must recheck required invariants before storing an ID so a future same-pass mutation cannot attach it to the wrong building.

Native draws one initial type whenever the type list is nonempty, even if the first offset is sentinel or the first slot is occupied. Therefore the initial draw occurs before early-return slot checks.

#### Damage-fire cleanup

For each slot in ascending order:

1. Read the current ID.
2. If present, call `destroy_anim`.
3. Reborrow the building and set that slot to `None`.

This order matches destroy-then-null and remains safe if destroy mutates live order. Repeated cleanup sees null slots and does nothing.

#### Save/load and hashing

Serialize `AnimStore`, animation objects, building slots/state, the renamed global ID counter, and the existing `LogicVector`. Keep the pending-delete list skipped because supported save boundaries occur after its tail drain.

After load:

- restore the presence-aware art configuration and full SHP header counts through the existing cache-rebuild boundary; existing serialized animations retain their effective fields, while future construction/`Next` transitions derive new effective fields from the rebound configuration;
- rebuild both entity and animation membership bytes from serialized `LogicVector`;
- validate that every live ID resolves to exactly one registry and every slot ID resolves to an animation;
- do not recreate, restart, or rerandomize existing animations.

Extend `state_hash` explicitly; do not assume serde automatically covers manual hash logic. Bump `SNAPSHOT_VERSION` once after the full persisted shape lands.

### Error Handling

- Validate stock `DamageFireTypes`, art runtime definitions, full SHP header counts, and resolved `End` values during initialization. Return the existing application-level error type through initialization rather than silently inventing runtime defaults.
- This slice certifies integer threshold comparison for the loaded stock values `ConditionYellow=50%` and `ConditionRed=25%`. A non-stock threshold is accepted only if initialization derives and proves an integer cutoff equivalent to the verified native f32-parse/f64-percent and `GetHealthRatio` comparison; otherwise initialization rejects it instead of falling back to `x1000`. General non-stock threshold support remains `UNCHECKED`, not silently approximate.
- Never apply `max(1)` to gameplay `End`. A non-positive resolved value suppresses the frame-selection ranged call exactly as native does. Atlas code packs the full raw range (and may ensure frame zero for a missing/empty presentation asset), but that presentation requirement cannot mutate gameplay metadata or RNG behavior.
- Runtime producer functions operate on validated IDs. A missing ID caused by corrupted state is a deterministic no-op in release plus a debug assertion; it never consumes replacement RNG.
- Rust allocation failure follows Rust's process-level behavior; do not add a gamemd allocation-failure gameplay branch.
- Keep malformed mod offset/type behavior marked `UNCHECKED` unless a binary audit establishes it. Do not extrapolate from stock data.
- Audio or rendering failure is presentation-only and cannot delete, advance, or retain an `AnimObject`.

### Testing Strategy

#### Scheduler and lifecycle unit tests

- `anim_scheduler_tail_appended_child_gets_first_guard_visit_same_pass`
- `animclass_ai_first_visit_clears_guard_without_frame_advance`
- `animclass_self_destroy_compacts_live_vector_without_index_repair`
- `animclass_next_mutates_in_place_without_new_id`
- `animclass_trailer_spawns_before_parent_first_guard`
- `animclass_rate_zero_never_advances`
- `animclass_loopcount_ff_is_infinite`
- `anim_destroy_stays_resolvable_until_pending_delete_flush`

#### Damage-fire mechanism tests

- `animtype_omitted_end_resolves_from_full_shp_header`
- `animtype_explicit_end_zero_overrides_loaded_value`
- `animtype_explicit_positive_end_overrides_loaded_value`
- `animtype_explicit_minus_one_resolves_during_constructor`
- `animtype_shadow_alone_controls_loader_halving`
- `animtype_shp_count_sign_extends_i16`
- `animclass_zero_delay_reveals_before_middle_event`
- `damage_fire_threshold_uses_can_be_occupied_selector`
- `damage_fire_threshold_equality_is_active`
- `damage_fire_nonstock_threshold_requires_exact_cutoff_or_rejection`
- `damage_fire_empty_type_list_consumes_no_rng`
- `damage_fire_single_type_and_single_frame_consume_no_rng`
- `damage_fire_rng_consumes_initial_type_then_slot_frames`
- `damage_fire_stock_asset_bounds_resolve_to_30_64_30`
- `damage_fire_nonpositive_end_consumes_no_frame_rng`
- `damage_fire_slot_scan_stops_at_first_sentinel_or_occupied_slot`
- `damage_fire_type_advances_only_after_successful_construction`
- `damage_fire_world_conversion_and_z_adjust_match_fixture`
- `damage_fire_repair_crossing_threshold_clears_next_update`
- `damage_fire_slots_clear_idempotently_on_sell_death_limbo`
- `damage_fire_owner_change_preserves_unattached_slot_ids`

Use at least two stock fixtures:

- `GACNST`: not occupiable, yellow threshold, two contiguous offsets `(-24,-1)` and `(64,36)`.
- A stock `CanBeOccupied=yes` building: red threshold and at least one verified offset.

RNG tests compare complete logical scenario-RNG state before and after, not only selected values. They also assert Main and MapGen streams are unchanged.

The stock-bound test must derive counts from retail assets and merged art rather than hardcode them as implementation constants. Its expected oracle is `FIRE01=30`, `FIRE02=64`, `FIRE03=30`; a seeded two-slot fixture must prove the selected ranged bounds follow the initial type plus sequential wrap. Include rejection-producing seeds for `0..2` and `0..29`, plus a `0..63` case, so final RNG state proves mask/rejection consumption.

#### Persistence and hash tests

- animation runtime fields and slot IDs survive snapshot round-trip;
- restored live order is byte/order-identical and does not replay constructor effects;
- changing any native-significant animation field, including effective End/LoopEnd, changes `state_hash`;
- changing skipped render caches does not change `state_hash`;
- snapshot version is bumped exactly once.

#### Presentation tests

- the first rendered frame comes from the sim-owned current frame;
- stock `FIRE02` frame 63 is available to the atlas/render adapter when selected; the old unconditional 32-frame half-count ceiling is absent;
- fixed lepton coordinates project to the expected damage-fire pixels for the concrete `GACNST` fixture;
- `zAdjust` affects the render submission exactly once;
- start/stop events retain animation identity and do not duplicate across save/load or repeated cleanup.

These are regression/implementation checks, not a pixel-parity certificate. Pixel certification needs a named gamemd-derived framebuffer or exhaustive draw-order proof.

#### Verification sequence

1. Focused unit tests for the new anim module and scheduler.
2. Focused damage-fire tests.
3. Snapshot/hash tests.
4. Relevant render/audio instance tests.
5. One serial `cargo check -q`, after confirming no other session owns Cargo.

Format only edited Rust files with edition 2024 and inspect the diff for unrelated churn.

## Architectural Decisions

### Patterns followed

- Reuse `LogicVector` rather than introducing a second scheduler.
- Reuse the common monotonic identity and deferred-delete patterns.
- Keep deterministic storage in `BTreeMap` form and avoid an ECS.
- Keep `sim/` independent of render, UI, audio, and app modules.
- Use presence-aware merged art metadata and full retail SHP header counts to derive native-resolved `End`; do not hardcode animation facts or reuse an unconditional half-count cache.
- Preserve native behavior through Rust-native ownership: separate registries sharing explicit lifecycle services.

### Intentional deviations

- Native stores raw pointers; Rust stores stable IDs. The shared non-reusing namespace, ordered scheduler, slot ownership, and deferred deletion preserve the verified semantics without copying unsafe pointer architecture.
- Native may write an `End==-1` fallback into the shared `AnimTypeClass`; this scoped Rust translation stores the constructor-observable result on each `AnimObject`. Every scoped read occurs after that constructor/`Next` resolution, and the operation has no RNG or other side effect, so the per-object copy preserves all scoped downstream bytes and ordering without a second mutable type catalog. Broader consumers must revisit this choice if any verified path reads the shared type value before construction.
- Native sound objects/handles live inside engine objects. Rust emits ID-addressed sound edges to an app-owned mixer while retaining authoritative start/stop state in sim.
- Initial rendering remains an adapter over the current renderer. It consumes native-significant inputs but is not certified as an equivalent native draw traversal.

### Tech debt and follow-ups

- Current `WorldEffect`, garrison muzzle flash, parachute, and other animation-like paths remain separate transitional systems. Migrate them only from verified spawn families after this shell proves stable.
- Cross-object same-pass health/fire timing remains blocked on verified Bullet/Techno/combat migration.
- Renderer depth and `AnimClass::DrawIt` pixel equivalence remain a distinct parity task.
- Bouncer, impact, damage, crater, owner attachment, save/load type-pointer rebinding, and rare constructor branches remain later bounded AnimClass slices.

These follow-ups are explicit DRIFT/UNCHECKED boundaries, not reasons to downgrade their parity verdicts.

## Alternatives Considered

### One unified enum-based object store

Store entities and animations in a single `BTreeMap<u64, LogicObject>` enum. This makes dispatch explicit but forces almost every entity, occupancy, production, combat, house-count, and pathing caller through enum matching. It creates broad refactor risk without improving the verified behavior over separate registries plus shared IDs/order.

Rejected as unnecessary architectural churn.

### Add an animation variant to `GameEntity` / `EntityCategory`

This minimizes new storage code but falsely classifies `AnimClass` as a gameplay techno/entity category. It would contaminate exhaustive four-category dispatch, occupancy, ownership, visibility, production and rule lookups with exclusions.

Rejected as an architectural mismatch and future bug source.

### Upgrade `WorldEffect` or damage-fire overlays in place

Keep the retained vector and add more lifecycle fields. This cannot reproduce shared live-vector tail append, compacting removal, registry versus scheduler separation, fixed building slot handles, or the pending-delete window. It would create another temporary model that later needs replacement.

Rejected as known mechanism drift.

## Research Basis

- [docs/research/ANIMCLASS_AI_FIRST_SAFE_MIGRATION_SLICE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_AI_FIRST_SAFE_MIGRATION_SLICE_GHIDRA_REPORT.md)
- [docs/research/ANIMCLASS_AI_LIFECYCLE_EXACT_SUBSET_RESWARM_20260527.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_AI_LIFECYCLE_EXACT_SUBSET_RESWARM_20260527.md)
- [docs/research/ANIMCLASS_AI_TRAILERANIM_PERIODIC_SPAWNS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_AI_TRAILERANIM_PERIODIC_SPAWNS_GHIDRA_REPORT.md)
- [docs/research/TICK_AND_ANIMATION_SPEED_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TICK_AND_ANIMATION_SPEED_GHIDRA_REPORT.md)
- [docs/research/BUILDINGCLASS_DAMAGE_FIRE_SELECTOR_RNG_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_DAMAGE_FIRE_SELECTOR_RNG_GHIDRA_REPORT.md)
- [docs/research/BUILDING_DAMAGEFIRE_SLOT_CLEAR_DESTROY_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_DAMAGEFIRE_SLOT_CLEAR_DESTROY_LIFECYCLE_GHIDRA_REPORT.md)
- [docs/research/traces/BUILDING_DAMAGE_FIRE_SLOT_RNG_ZADJUST_TRACE_20260528.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/BUILDING_DAMAGE_FIRE_SLOT_RNG_ZADJUST_TRACE_20260528.md)
- [docs/research/ANIMCLASS_DRAWIT_ZADJUST_DEPTH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_DRAWIT_ZADJUST_DEPTH_GHIDRA_REPORT.md)
- `docs/research/ADVANCE_TICK_PHASE_PARTITION_NATIVE_SPINE_GHIDRA_REPORT.md`
- [docs/research/ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md)
- [docs/research/ANIMATION_SOUNDS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMATION_SOUNDS_GHIDRA_REPORT.md)
- [docs/research/CCINICLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CCINICLASS_GHIDRA_REPORT.md)

The targeted live-Ghidra investigation in [ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMTYPE_END_LOAD_DAMAGE_FIRE_FRAME_RNG_GHIDRA_REPORT.md) resolved the prior `End`/frame-bound contradiction. Its loader-order findings supersede omitted-`End` statements in [ANIMCLASS_AI_LIFECYCLE_EXACT_SUBSET_RESWARM_20260527.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_AI_LIFECYCLE_EXACT_SUBSET_RESWARM_20260527.md) and [TICK_AND_ANIMATION_SPEED_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TICK_AND_ANIMATION_SPEED_GHIDRA_REPORT.md) that considered only the later `End==-1` fallback. The design-review spot check of `AnimClass::Constructor @ 0x00421EA0` also corrected reveal/`Middle` ordering. No further live-Ghidra investigation is required before planning this scoped slice unless review exposes another contradiction.
