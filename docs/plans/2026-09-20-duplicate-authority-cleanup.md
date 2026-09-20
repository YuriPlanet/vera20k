# Duplicate-authority cleanup — audit and remaining work

Goal: every lead below resolved, or retained for an independently reviewed
reason, with production validation passing. Recording a lead does not resolve it.
Continues the whole-engine audit in
[2026-09-20-engine-authority-audit.md](2026-09-20-engine-authority-audit.md).

Replace this file on each update; do not append a diary.

## Resolved

| Lead | What went | PR |
| --- | --- | --- |
| Dead code | `bridge_re.rs`, the write-only building anim phase map, caller-less helpers | #416 |
| Radar events | the sim-side event queue; the client array owns all 17 types | #416 |
| Refinery dock contacts | the `dock_reservations` registry; the radio bus is the only record | #417 |
| Animation | teleport WarpOut, superweapon invoke and Lightning Storm on `AnimStore` | #418 |
| `resource_nodes` | the node map, the second growth algorithm, the miner authority switch, the test-only admission seam, and the state they left write-only | #419 |
| Animation | bridge `BridgeExplosions=` on `AnimStore`; the store's delayed start edge; the whole `WorldEffect` lane, which drew nothing | #420 |
| Map load | building animations bound tolerantly; the native anim file-name rule; ore twinkle and cliff-collapse roots | #422 |
| Dead code | items dead in both builds; superseded test-only duplicates (map-list funnels, `radiation_light_epoch`); test probes gated; the effect asset catalog trimmed to particle images | #421 |

Three per-mover world scans went with those: the per-frame dock sweep, the
whole-entity scan in `interrupt_refinery_docked_miners`, and the per-frame
building anim phase scan.

Ghidra: the MEGAMISSION comments at `0x004C72F8`, `0x004C733C`, `0x004C7342`
now name the radio BREAK (`vt+0x274` = `RadioClass::Transmit_Radio_ToFirst`,
read from the Unit vtable); the `AnimClass` Start/Middle labels were already
correct in the database and the stale source notes saying otherwise are gone.

## Independent audit, 2026-09-20

A read-only reviewer checked the rows above against main and accepted them. Of
the retentions this file used to carry it accepted one (debris bouncers) and
rejected the rest: the movement branch named as an owner had already been
squash-merged as `527ac301` (identical tree), so no open lead is blocked by
another session, and the sim already computes an integer fire coordinate, so the
muzzle flashes are not blocked on a port. The goal is not done.

## Retained, reason accepted

**Animation: debris bouncers.** `MetallicDebris=` and `DebrisAnims=` types are
native bouncers (`Bouncer=yes`, `RandomRate=`, `Damage=`, `ExpireAnim=`).
`AnimStore` has no bouncer arm: RESIDUAL M11b in `sim/anim_class.rs`. That is
a port of a native mechanism (constructor RNG fork, landing damage), not a
duplicate to fold, and it moves the lockstep stream. Nothing executes debris
today, so there is no second authority: VERA takes the gate and slot draws and
constructs nothing, in the bridge fallout and in combat deaths
(`combat/mod.rs`, GSI-05.14).
PRESERVE: the worktree `.claude/worktrees/phase6-audio-lane` (branch
`feature/phase6-anim-bounce-damage`, 0 commits ahead of main) holds about 1,650
uncommitted lines of parked bouncer/damage-arm work from 2026-09-03. Do not
delete it.

**Staged native ports with no production caller yet.**
`track_fresh_dispatch.rs`, `track_speed_native.rs`, the `load_object_lifecycle`
wall arm, `all_to_hunt_score_override`,
`projectile_slope_reflect_with_elasticity`, `advance_emergency_state`,
`snap_to_passable`: verified native work with golden or oracle tests. Deleting
them discards evidence-backed work; wiring each is a port. The six unused
`state_hash_without_*` probes in `world_hash.rs` are replay-pin provenance.

## Open

**Animation: muzzle flashes.** Garrison flashes (`components::AnimRuntime` +
`GarrisonMuzzleFlash`, stepped by a second AnimClass-like visit function in
`app/presentation/building_anim.rs` that diverges from the oracle-backed
`advance_anim_boundary`) and `WeaponMuzzleFlash`
(`app/presentation/fire_effects.rs`). The sim has the integer fire coordinate
(`combat/world_receiver.rs`, `native_flh_world_delta`); the app recomputes
another in `f32` from the body facing only. Move the flashes onto `AnimStore`
from the sim coordinate and delete the app stepper and the `f32` FLH.

**Animation: parachute.** `app/presentation/chute_anim.rs` +
`components::ParachuteAnim` is a third app-side stepper. Natively the chute is
an AnimClass attached to the object (`Object+0x88`); `AnimStore` supports owner
attachment.

**Hand-built mover path facts.** `MoverPathFacts::from_snapshot` is the single
derivation since PR #387. Two production sites in `movement_commands.rs` still
call `mover_path_facts_without_wall_arm` with independently derived
`mover_is_crusher`/`is_infantry`; closing them is ledger row I9b.

**Per-mover world scans.** `movement_tick.rs` rebuilds
`build_live_building_entry_skip_map` and `snapshot_bridge_marker_peers` (every
entity's remaining path) for every mover every tick, and `track_entry.rs`
rebuilds the skip map per entry: O(N^2) against the 20,000-unit target. The
snapshot exists because the mover holds the entity store mutably. Hoisting needs
incremental invalidation, because a mover's tick can rewrite other entities'
paths; `refresh_owner_block_set_if_stale` is the existing model.

**Move phases / Jumpjet destination.** `air_phase` is hashed and also mirrors
`JumpjetRuntime` (`jumpjet_cruise.rs::air_phase_for`); `movement_target` and
`JumpjetRuntime::destination` are both live in `world/jumpjet_cruise.rs`.

**Loader funnels only tests drive.** `construct_scenario`,
`construct_app_scenario`, `load_rules_with_merged_ini`, `LoadedRules`,
`HeadlessTerrainBootstrap`: oracle tests exercise these instead of the staged
production funnel. Also `sprite_atlas.rs` `collect_effect_names`, a `cfg(test)`
copy of the production list, so the atlas tests validate the copy.

## Production validation

Release build, `RA2_QUICKPLAY=minerloop.map`, 2026-09-21: loads, 0 ERROR lines,
full SearchOre, Harvest, ReturnToRefinery, Dock, deposit, SearchOre cycle
(overlay-grid ore, radio-bus dock). That run first failed to load any map
(`required SHP for animation type [CAARAY_A]`); fixed in #422. Still to run in
release: a chrono warp, a superweapon invoke, a bridge collapse.

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
- Combat death debris never binds a sprite (GSI-05.14); theaters other than
  TEMPERATE were not checked for unbound building animations.

## Other checkouts

- The separate `vera20k-engine-authority` checkout, branch
  `feature/persistent-facing-authority`: dirty facing, turret, walk-head
  and snapshot files. No open lead above touches them; its snapshot bump must
  land after 175.
