# Duplicate-authority cleanup — final audit

Goal: every lead below resolved, or retained for an independently reviewed
reason, with production validation passing. Recording a lead does not resolve it.
Continues the whole-engine audit in
[2026-09-20-engine-authority-audit.md](2026-09-20-engine-authority-audit.md).

Replace this file on each update; do not append a diary.

## Resolved (2026-09-20)

| Lead | What went | PR |
| --- | --- | --- |
| Dead code | `bridge_re.rs`, the write-only building anim phase map, caller-less helpers | #416 |
| Radar events | the sim-side event queue; the client array owns all 17 types | #416 |
| Refinery dock contacts | the `dock_reservations` registry; the radio bus is the only record | #417 |
| Animation | teleport WarpOut, superweapon invoke and Lightning Storm on `AnimStore` | #418 |
| `resource_nodes` | the node map, the second growth algorithm, the miner authority switch, the test-only admission seam, and the state they left write-only | #419 |
| Animation | bridge `BridgeExplosions=` on `AnimStore`; the store's delayed start edge; the whole `WorldEffect` lane, which drew nothing | #420 |
| Dead code | items dead in both builds; the effect asset catalog trimmed to particle images | `feature/dead-code-sweep` |

Three per-mover world scans went with those: the per-frame dock sweep, the
whole-entity scan in `interrupt_refinery_docked_miners`, and the per-frame
building anim phase scan.

Ghidra: the MEGAMISSION comments at `0x004C72F8`, `0x004C733C`, `0x004C7342`
now name the radio BREAK (`vt+0x274` = `RadioClass::Transmit_Radio_ToFirst`,
read from the Unit vtable); the `AnimClass` Start/Middle labels were already
correct in the database and the stale source notes saying otherwise are gone.

## Retained, with reasons

Each reason below names what would have to be true to pick the lead up.

**Animation: debris bouncers.** `MetallicDebris=` and `DebrisAnims=` types are
native bouncers (`Bouncer=yes`, `RandomRate=`, `Damage=`, `ExpireAnim=`).
`AnimStore` has no bouncer arm: RESIDUAL M11b in `sim/anim_class.rs`. That is
a port of a native mechanism (constructor RNG fork, landing damage), not a
duplicate to fold, and it moves the lockstep stream. VERA takes the gate and
slot draws and constructs nothing; no debris sprite was ever bound or drawn, in
the bridge fallout or in combat deaths (`combat/mod.rs`, GSI-05.14).
PRESERVE: the worktree `.claude/worktrees/phase6-audio-lane` (branch
`feature/phase6-anim-bounce-damage`, 0 commits ahead of main) holds about 1,650
uncommitted lines of parked bouncer/damage-arm work from 2026-09-03. Ownership
is unresolved; do not delete it.

**Animation: muzzle flashes.** Garrison flashes (`components::AnimRuntime` +
`GarrisonMuzzleFlash`, stepped by a second AnimClass-like visit function in
`app/presentation/building_anim.rs`) and `WeaponMuzzleFlash`
(`app/presentation/fire_effects.rs`) are app-side and unhashed. Natively they
are AnimClass objects built at the fire coordinate. The sim has no deterministic
fire coordinate: FLH and the garrison port offset are resolved in the app in
`f32` (`resolve_fire_origin_from_sim`), and `sim/projectile.rs` records FLH as
an open residual (GSI-08.06/07). Moving the flashes first needs that port; doing
it with app-side floats would put presentation math into hashed state.

**Hand-built mover path facts.** `mover_path_facts_without_wall_arm` has two
callers in `movement_commands.rs`. The active session on
`feature/vera20k-yuris-revenge-movement-bc5040` owns them (ledger row I9b); its
branch is 15 commits ahead of main across 19 files, ten of them in
`sim/movement/`.

**Per-mover world scans.** `build_entity_block_sets`,
`build_live_building_entry_skip_map` and `snapshot_bridge_marker_peers` are
invoked per mover from `movement_tick.rs` (and `track_entry.rs`). The helper
files are untouched by the movement owner, but every call site sits in
`movement_tick.rs`, which that branch changes by +113/-16. Hoisting the scans
means restructuring those call sites. This is a cost problem, not a second
authority.

**Move phases / Jumpjet destination.** For Jumpjets `air_phase` is a mirror
written each tick from the native state (`jumpjet_cruise.rs::air_phase_for`),
and `movement_target` is the order channel the cruise host turns into
`JumpjetRuntime::destination`. `AirMovePhase` has 124 references in 23 files,
`GroundMovePhase` 51 in 18, nearly all under `sim/movement/`. Untangling them is
the Foot destination migration and overlaps the movement owner and the Codex
facing branch (`C:\Users\enok\Documents\vera20k-engine-authority`, 11 dirty
files).

**Dead code that only tests reach.** After the sweep the non-test build still
reports about 100 unused items. Every one is referenced by tests:
- staged native ports with their own golden or oracle tests and no production
  caller yet (for example `projectile_slope_reflect_with_elasticity`,
  `advance_emergency_state`, `snap_to_passable`, the `load_object_lifecycle`
  wall arm, `track_speed_native.rs`). Deleting them discards verified work;
  wiring each is a port of its mechanism.
- test-facing probes and accessors (`native_processing` trace accessors,
  `entity_pick` snapshot functions, `HeadlessTerrainBootstrap`).
- older scenario/rules load funnels that only oracle tests still drive
  (`construct_scenario`, `construct_app_scenario`,
  `load_rules_with_merged_ini`, `LoadedRules`). These are a genuine "tests
  exercise a parallel path" debt in the loader; moving those tests onto the
  staged production funnel belongs to the load/bootstrap track.
- six unused `state_hash_without_*` probes in `world_hash.rs`: replay-pin
  provenance kept by the harness maintainers.
- `movement_tick.rs` and `track_*` items: the movement owner's.

## Adjacent findings (not this goal's backlog)

- Slave Miner scan correction has no implementation: `SlaveMinerKickFrameDelay=`
  and `SlaveMinerScanCorrection=` are parsed and unread, so a deployed YAREFN
  never repositions toward closer ore. Visible in Yuri games once nearby ore runs
  out. The caller-less Rust sketch was removed; the native owner is unidentified.
- `[General] GrowthRate=` is parsed and unread; VERA's growth runs on the
  per-Tiberium `Growth=` timers. Whether gamemd reads it is UNCHECKED.
- Bridge collapse explosions leave no `Scorch=`/`Crater=` and skip one native
  draw per hut-walker explosion, because the store does not run
  `AnimClass::Middle` (recorded in `bridge_orchestrator.rs`).

## Other owners

- `feature/vera20k-yuris-revenge-movement-bc5040` (active Claude session).
- `C:\Users\enok\Documents\vera20k-engine-authority`, branch
  `feature/persistent-facing-authority` (Codex). Its snapshot bump must land
  after 175.
