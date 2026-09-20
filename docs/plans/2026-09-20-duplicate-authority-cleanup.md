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
| `resource_nodes` legacy ore map and the state it left write-only | In review | branch `feature/retire-resource-nodes` |
| Animation: bridge collapse producers, `WorldEffect` itself, muzzle flashes | Open | see below |
| Move phases / Jumpjet destination | Open, overlaps other owners | see below |
| Hand-built mover path facts | Owned elsewhere | see below |
| Per-mover world scans | Partly done | see below |

Validation of the `resource_nodes` candidate: `cargo test -p vera20k --lib`
and `cargo clippy -p vera20k --lib` results are recorded on its commits.
Windows only.

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
  174.

## Open leads — findings so far

**Animation.** `AnimStore`/`AnimClass` (`sim/anim_class.rs`) is the native
owner. Still outside it:

- `WorldEffect` (`components.rs`, ticked in `world/mod.rs`, drawn in
  `app/presentation/instances/overlays.rs`): the three
  `bridge_orchestrator.rs` collapse producers. They interleave their own RNG
  draws with the spawn, so moving them reorders the scenario stream and moves
  the replay pins; that needs its own increment with causal attribution.
  MetallicDebris are native bouncers and AnimStore has no bouncer arm yet
  (owned by the unmerged `feature/phase6-anim-bounce-damage`), so `WorldEffect`
  cannot be deleted before that lands.
- Garrison muzzle flashes (`components::AnimRuntime`,
  `app/presentation/building_anim.rs`) and `WeaponMuzzleFlash`
  (`fire_effects.rs`).

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

Land the `resource_nodes` PR after its critic re-review. Then the bridge
collapse producers, with a per-producer replay-pin attribution.
