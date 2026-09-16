# Core Engine Substrate TODO

Date: 2026-05-29

Purpose: keep the main missing foundational systems in one place. This is a
planning checklist, not a research report and not an implementation contract.
Use the linked research docs and contracts as evidence before editing Rust.

## Source Docs

- [docs/research/ENGINE_STATE_OVERVIEW.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ENGINE_STATE_OVERVIEW.md)
- [docs/research/SUBSTRATE_PARITY_LEDGER_20260529.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SUBSTRATE_PARITY_LEDGER_20260529.md)
- [docs/research/CORE_PRIMITIVE_PARITY_20260529.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CORE_PRIMITIVE_PARITY_20260529.md)
- [docs/research/TWO_RNG_STREAM_IMPLEMENTATION_CONTRACT_20260529.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TWO_RNG_STREAM_IMPLEMENTATION_CONTRACT_20260529.md)
- [docs/research/PERTICKUPDATE_FULL_ORDERING_LADDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PERTICKUPDATE_FULL_ORDERING_LADDER_GHIDRA_REPORT.md)
- [docs/research/2026-05-29-parity-gap-scan-shortlist.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/2026-05-29-parity-gap-scan-shortlist.md)
- `docs/plans/2026-05-29-native-tick-spine-contract.md`
- `docs/plans/TWO_STREAM_RNG_PLAN_20260529.md`
- `docs/plans/2026-05-28-foundational-scheduler-roadmap-todo.md`

## Framing

The issue is architectural parity, not just isolated feature bugs. VERA has
broad gameplay scaffolding, but the engine substrate is not yet gamemd-shaped:
ordering, timing, RNG routing, live object lifetime, hash/save-load behavior,
and same-tick consequences do not consistently match active YR `gamemd.exe`.

Useful shorthand:

> Close foundational engine-substrate parity before chasing more leaf features.

Translation rule:

> Rust-native structure, gamemd-native semantics.

## Big Missing Core Systems

### 1. Native tick spine / LogicClass scheduler

- [ ] Replace the current phase-split, sorted-entity-id tick shape with a
  native-shaped tick spine.
- [ ] Drive object AI from a live active-object vector with tail append,
  live-count reload, and compacting-removal skip semantics.
- [ ] Surround the object pass with the verified global subsystem ladder.
- [ ] Preserve the `Tactical -> all factories -> all houses` tail order.

Current problem: Rust still runs phased systems over sorted entity ids. gamemd
runs one live object vector with same-pass append/removal behavior, surrounded
by a specific global subsystem ladder.

### 2. Two RNG streams

- [ ] Add separate `g_MainRng` and Scenario RNG equivalents.
- [ ] Seed both streams from the same seed with identical initial state.
- [ ] Route every RNG consumer by verified gamemd stream identity.
- [ ] Preserve raw `Next` vs `RandomRanged` draw counts per caller.

Current problem: gamemd has `g_MainRng` and `ScenarioClass+0x218` as separate
streams. Rust currently has one stream. This breaks draw order and parity
across ore, scatter, combat, sounds, particles, bridges, and AI.

### 3. Object lifecycle and unregister discipline

- [ ] Centralize native-shaped reveal, conceal, limbo, unlimbo, uninit, delete,
  and pending-delete effects.
- [ ] Ensure all removal paths unregister from the live logic vector.
- [ ] Prevent direct `entities.remove()` paths from bypassing lifecycle cleanup.
- [ ] Keep synchronous vs deferred death differences per class.

Current problem: some Rust paths still remove entities directly instead of
going through a native-shaped conceal/unregister/uninit flow. That leaks ids
into logic order/hash and changes same-tick behavior.

### 4. Frame/timing model

- [ ] Separate native-frame timing from Rust app tick pacing.
- [ ] Audit all `binary_frame`, `tick`, `tick_ms`, timer, animation, ROF,
  movement, and modulo-gate consumers.
- [ ] Move high-frequency logic decisions onto the verified native-frame basis.
- [ ] Keep render/app pacing from changing sim-visible timing.

Current problem: gamemd logic is native-frame based. Rust has a 45 Hz tick model
plus `binary_frame`, and some systems use the wrong basis or wrong phase.

### 5. Authoritative combat/projectile/warhead pipeline

- [ ] Use authoritative live `BulletClass`-style projectile objects where
  gamemd does.
- [ ] Implement exact damage math, warhead dispatch, special effects, immunity
  gates, and AoE truncation order.
- [ ] Preserve projectile flight, impact timing, delayed detonation, child
  spawn, and same-pass effect ordering.
- [ ] Keep direct damage shortcuts only where gamemd actually has no projectile.

Current problem: damage, warhead effects, projectile flight, detonation timing,
and same-pass spawned effects are not yet a single gamemd-shaped pipeline.

### 6. Target acquisition / order cadence

- [ ] Implement native ring scans, early-return behavior, threat scoring, and
  tie-break order.
- [ ] Add native cadence timers such as normal targeting and guard-area delays.
- [ ] Preserve mission distinctions such as Guard, Area Guard, AttackMove, and
  anti-churn behavior.
- [ ] Route targeting-related RNG through the verified stream.

Current problem: Rust scans broadly and often. gamemd uses ring scans, threat
scoring, cadence timers, mission distinctions, and strict ordering.

### 7. Map/cell substrate

- [ ] Build native-compatible occupancy, `Can_Enter_Cell`, and `CellRect`
  validator surfaces.
- [ ] Represent cell blocker bytes, object lists, bridge state, zone records,
  reservation bits, and height/layer arguments separately where gamemd does.
- [ ] Make save/load rebuilds produce the same cell substrate as live gameplay.
- [ ] Avoid collapsing pathfinding, placement, occupancy, and nearby-cell checks
  into one boolean walkable grid.

Current problem: occupancy, `Can_Enter_Cell`, `CellRect` validators, bridge
state, zone records, and cell blocker bytes are not one native-compatible
substrate yet.

### 8. Save/load/hash/MP lockstep substrate

- [ ] Expand deterministic hash coverage to all authoritative gameplay state.
- [ ] Remove unstable hash inputs such as raw intern-order ids where content
  identity is what matters.
- [ ] Make save/load restoration match live active-vector, occupancy, RNG, and
  object lifecycle state.
- [ ] Build MP lockstep transport, seed handshake, command barrier, execution
  frame scheduling, and house-order command dispatch.

Current problem: hash coverage misses several authoritative fields. MP
transport is basically absent. Save/load rebuild paths can diverge from live
paths.

## Suggested Next Work

1. Produce or use the native tick spine implementation contract.
2. Land the two-RNG-stream split only with verified per-caller routing.
3. Tighten lifecycle/unregister paths before moving more behavior into the live
   scheduler.
4. Move combat/projectile/warhead work onto the native scheduler instead of
   strengthening the current direct-damage shortcut.
5. Treat map/cell substrate and save/load/hash/MP as parallel planning tracks,
   but integrate them only after their dependencies are clear.

## Reswarmable Topics

- `/re-swarm --handoff-plan native tick spine and LogicScheduler migration`
- `/re-swarm --handoff-plan object lifecycle unregister discipline and direct remove paths`
- `/re-swarm --handoff-plan target acquisition cadence and ring scan threat scoring`
- `/re-swarm --handoff-plan native CellRect Can_Enter_Cell occupancy substrate`
- `/re-swarm --handoff-plan save load hash MP lockstep substrate gaps`
