# Foundational Scheduler Roadmap TODO

Date: 2026-05-28

Purpose: capture the implementation roadmap that came out of the
`LogicClass` / `Simulation::advance_tick` swarm. This is a planning TODO, not a
new binary research report. Treat the linked research reports as the evidence.

## Evidence To Read First

- [docs/research/LOGICCLASS_LIVE_VECTOR_VS_RUST_ENTITY_PASSES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_LIVE_VECTOR_VS_RUST_ENTITY_PASSES_GHIDRA_REPORT.md)
- [docs/research/LOGICCLASS_GLOBAL_SUBSYSTEM_ORDER_0055AFB0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LOGICCLASS_GLOBAL_SUBSYSTEM_ORDER_0055AFB0_GHIDRA_REPORT.md)
- [docs/research/FRAME_COUNTER_PREINCREMENT_VS_RUST_BINARY_FRAME_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FRAME_COUNTER_PREINCREMENT_VS_RUST_BINARY_FRAME_GHIDRA_REPORT.md)
- [docs/research/FACTORY_HOUSE_AI_ORDER_VS_RUST_PRODUCTION_AI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FACTORY_HOUSE_AI_ORDER_VS_RUST_PRODUCTION_AI_GHIDRA_REPORT.md)
- [docs/research/SAME_TICK_SPAWNED_OBJECT_BEHAVIOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SAME_TICK_SPAWNED_OBJECT_BEHAVIOR_GHIDRA_REPORT.md)

## Core Problem

Current Rust has many systems implemented, but the simulation spine is not
native-equivalent. `Simulation::advance_tick` is a custom phase pipeline.
`gamemd.exe` uses a `LogicClass` tick spine with native global ordering, a live
active-object vector, old-frame-visible timing, and factory/house tail ordering.

The player-visible effect is same-frame drift: projectiles, anims, garrison
ownership, bridge engineers, ore growth, factory completion, defeat checks, and
superweapon readiness can occur a frame or phase early/late.

## Contract Stack To Create

- [x] Native Frame / Tick Contract — **done 2026-05-28**, see
  `2026-05-28-native-frame-tick-contract-design.md`. `binary_frame` now
  committed LATE in `advance_tick` (pre-increment-visible during the tick,
  mirroring Main_Tick). Classification: every `binary_frame` consumer is a
  relative/stored-start CDTimer (facing/turret/gate/miner-dock/ore-growth) —
  no absolute modulo gate exists, so the global shift is proven correct for
  all. Acceptance tests added (same-tick start/check, retarget boundary,
  discriminating gate-via-advance_tick capture-is-pre-increment). 45/15 rate
  question deferred (named DRIFT). Zero regressions (10 pre-existing
  movement/ore/production failures identical with/without the change).
  - Define old-frame-visible timing and late frame commit.
  - Classify `binary_frame`, `sim.tick`, and `tick_ms` consumers.
  - Acceptance tests: timer start/check in same update, modulo gate boundary,
    facing retarget boundary.

- [ ] LogicClass Scheduler Contract
  - Define active object list, membership state, registration, unregistration,
    tail append, live count reload, and compacting removal semantics.
  - Acceptance tests: append-during-pass ticks tail, duplicate registration is
    idempotent, self-unregister uses native compacting index behavior.

- [ ] ObjectClass Lifecycle Contract
  - Define reveal, conceal, limbo, unlimbo, uninit/delete, active-list
    registration, and cleanup ownership.
  - Must specify which transitions touch the scheduler.

- [ ] TechnoClass Shared State Contract
  - Define owner, health/death, targetability, cloak, EMP, iron curtain, force
    shield, temporal, mind control, gattling, and common notification effects.
  - Keep class-specific behavior out unless it changes shared lifecycle.

- [ ] Global Tick Spine Contract
  - Preserve native `0x0055AFB0` order:
    scenario/cell timers, tiberium growth/spread, bombs, teams, disk lasers,
    light/LaserDraw, LightningStorm, EMP, live object vector, tactical,
    factories, houses.

- [ ] Factory / House Tail Contract
  - Preserve `Tactical -> all factories -> all houses`.
  - Split global superweapon effects from per-house superweapon ready/charge.
  - Place defeat, AI choose, `AI_ManageProduction`, and `AI_ResumeProduction`
    in native house-tail order.

- [ ] Projectile / Anim Same-Tick Contract
  - Define BulletClass-style authoritative projectile objects.
  - Define AnimClass scheduling through the live object vector.
  - Acceptance tests: AAHeatSeeker2 same-pass bullet AI, first-AI anim guard,
    garrison muzzle flash first visible frame.

## Implementation Roadmap

- [ ] Add a `LogicScheduler` beside `EntityStore`.
  - `EntityStore` remains storage.
  - Scheduler owns active order and membership.
  - No broad rewrite at this stage.

- [x] Add native-frame timing primitives. **done 2026-05-28** (see Native
  Frame / Tick Contract above).
  - Expose a pre-increment frame value for native frame-counter consumers.
  - Commit the synthetic native frame late.
  - Do not globally add/subtract one frame without per-system proof.

- [ ] Route lifecycle transitions through scheduler-aware helpers.
  - Reveal/unlimbo/spawn can register active objects.
  - Conceal/despawn/delete can unregister active objects with native removal
    semantics.

- [ ] Migrate high-risk systems first.
  - Bullets/projectiles.
  - Anims/muzzle flashes.
  - Garrison/building owner reconciliation.
  - Bridge engineer repair/removal.
  - Miner/refinery/docking cross-object timing.
  - Factory completion and house production management.

- [ ] Rebuild `advance_tick` around the native tick spine.
  - Early native globals.
  - `LogicScheduler` live object pass.
  - Tactical.
  - FactoryClass-equivalent loop.
  - HouseClass-equivalent loop.
  - Late frame commit.

## Do Not Do

- Do not replace `EntityStore` wholesale.
- Do not recreate the C++ inheritance hierarchy directly.
- Do not treat `BTreeMap` stable-id iteration as active object order.
- Do not snapshot active candidates and call it scheduler parity.
- Do not force newly spawned logic objects to wait until the next tick by
  default.
- Do not use unordered removal for active-list semantics.
- Do not treat current `ai::tick_ai` as native TeamClass or HouseClass AI.
- Do not merge global LightningStorm/EMP timing with per-house superweapon
  ready/charge timing.

## Open Follow-Up Research

- [ ] `/re-swarm ObjectClass reveal conceal uninit LogicClass registration`
- [ ] `/re-swarm TechnoClass shared lifecycle cloak EMP IC force shield mind control`
- [ ] `/re-swarm FactoryClass array insertion order and simultaneous completions`
- [ ] `/re-swarm HouseClass Update production-management defeat superweapon ready`
- [ ] `/re-swarm BulletClass authoritative projectile object lifecycle`

