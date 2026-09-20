# Duplicate-authority cleanup — checkpoint

Goal: every lead below resolved, or retained for an independently reviewed
reason, with production validation passing. Recording a lead does not resolve it.
Continues the whole-engine audit in
[2026-09-20-engine-authority-audit.md](2026-09-20-engine-authority-audit.md).

Replace this file on each update; do not append a diary.

## State (2026-09-20)

Worktree `.claude/worktrees/refactor-candidates-review-3de5a3`.

| Lead | Status | Where |
| --- | --- | --- |
| Dead code (`bridge_re.rs`, write-only building anim phase map, caller-less helpers) | Merged | PR #416 |
| Radar events: sim queue vs client array | Merged | PR #416 |
| Refinery dock contacts: registry vs radio bus | Merged | PR #417 |
| Animation: teleport WarpOut, superweapon invoke, Lightning Storm | Merged | PR #418 |
| `resource_nodes` legacy ore map and the state it left write-only | Merged | PR #419 |
| Animation: bridge collapse `BridgeExplosions=`; AnimStore delayed start edge | In review | branch `feature/bridge-explosions-animstore` |
| Animation: `WorldEffect` lane | In review (deleted) | same branch |
| Animation: `MetallicDebris=` bouncers, muzzle flashes | Open, blocked | see below |
| Move phases / Jumpjet destination | Open, overlaps other owners | see below |
| Hand-built mover path facts | Owned elsewhere | see below |
| Per-mover world scans | Partly done | see below |

Validation results of each candidate are recorded on its commits. Windows only.

## Other owners — do not collide

- `feature/vera20k-yuris-revenge-movement-bc5040` (active Claude session) holds
  unmerged work across `movement_path.rs`, `movement/mod.rs`,
  `movement_tick.rs`, `movement_step.rs`, `pathfinding/core.rs`,
  `cell_entry.rs`, and large edits to `world/mod.rs` and `world/techno_ai.rs`.
  It owns the **mover path facts** lead (ledger row I9b:
  `mover_path_facts_without_wall_arm` at the two move-order sites).
- `C:\Users\enok\Documents\vera20k-engine-authority`, branch
  `feature/persistent-facing-authority` (Codex): uncommitted `facing_class.rs`,
  `turret.rs`, `walk_head.rs`, `snapshot.rs`. Its snapshot bump must land after
  175.

## Open leads — findings so far

**Animation.** `AnimStore`/`AnimClass` (`sim/anim_class.rs`) is the native
owner. Still outside it:

- `WorldEffect` is deleted. Its last producer, the bridge collapse
  `MetallicDebris=` spawn, pushed records whose `DBRIS*` sprites no atlas source
  ever loaded, so the lane drew nothing. The gate and slot draws stay; the
  debris is not constructed. Those types are native bouncers (`Bouncer=yes`,
  `RandomRate=`, `Damage=`), and AnimStore has no bouncer arm: RESIDUAL M11b in
  `sim/anim_class.rs`, a native port that moves the lockstep stream.
  PRESERVE: the worktree `.claude/worktrees/phase6-audio-lane` (branch
  `feature/phase6-anim-bounce-damage`, 0 commits ahead of main) holds about
  1,650 uncommitted lines of parked bouncer/damage-arm work from 2026-09-03
  across `anim_class.rs`, `bounce.rs`, `combat/mod.rs`, `world/mod.rs` and
  `snapshot.rs`. Ownership is unresolved; do not delete it.
- Garrison muzzle flashes (`components::AnimRuntime` + `GarrisonMuzzleFlash`,
  stepped in `app/presentation/building_anim.rs`) and `WeaponMuzzleFlash`
  (`fire_effects.rs`) are app-side and unhashed. Natively they are AnimClass
  objects built at the fire coordinate. The sim has no deterministic fire
  coordinate yet: FLH is resolved in the app in floats
  (`resolve_fire_origin_from_sim`), and `sim/projectile.rs` records FLH as an
  open residual (GSI-08.06/07). Moving them needs that port first.

**Move phases / Jumpjet.** For Jumpjets `air_phase` is a mirror written each
tick from the native state (`jumpjet_cruise.rs::air_phase_for`), and
`movement_target` is the order channel the cruise host turns into
`JumpjetRuntime::destination`. Consumers (`ready_producer.rs`, `piggyback.rs`,
`world_commands.rs`, MoveSound in `world/mod.rs`) serve Fly and Jumpjet alike.
Untangling this is the Foot destination migration and overlaps both owners
above.

**Per-mover world scans.** Removed so far: the per-frame dock sweep, the
whole-entity scan in `interrupt_refinery_docked_miners`, the per-frame
building anim phase scan. Remaining (movement territory):
`bump_crush::build_entity_block_sets`, `build_blocker_neighbor_counts`,
`movement_occupancy::build_live_building_entry_skip_map`,
`path_markers::snapshot_bridge_marker_peers`,
`sync_formation_speeds_after_live_pass`.

## Adjacent findings (not this goal's backlog)

- Slave Miner scan correction has no implementation: `SlaveMinerKickFrameDelay=`
  and `SlaveMinerScanCorrection=` are parsed and unread, so a deployed YAREFN
  never repositions toward closer ore. Visible in Yuri games once nearby ore runs
  out. The caller-less Rust sketch was removed; the native owner is unidentified.
- `[General] GrowthRate=` is parsed and unread; VERA's growth runs on the
  per-Tiberium `Growth=` timers. Whether gamemd reads it is UNCHECKED.

## Next safe action

Land the bridge explosion PR after its critic. Then the final whole-goal audit
with an independent review of every retained lead.
