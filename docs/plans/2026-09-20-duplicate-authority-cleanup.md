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
| Refinery dock contacts: registry vs radio bus | In review | PR #417, branch `feature/refinery-contact-authority` |
| Animation execution split four ways | Open | see below |
| `resource_nodes` legacy ore map | Open | see below |
| Move phases / Jumpjet destination | Open, blocked on other owners | see below |
| Hand-built mover path facts | Owned elsewhere | see below |
| Per-mover world scans | Partly done | see below |

Validation of the last merged candidate: `cargo test -p vera20k --lib` 9134
passed / 0 failed / 134 ignored; clippy exit 0. Windows only.

## Other owners — do not collide

- `feature/vera20k-yuris-revenge-movement-bc5040` (active Claude session) holds
  unmerged wall-arm work across `movement_path.rs`, `movement/mod.rs`,
  `movement_tick.rs`, `movement_step.rs`, `pathfinding/core.rs`,
  `cell_entry.rs`. It owns the **mover path facts** lead (ledger row I9b:
  `mover_path_facts_without_wall_arm` at the two move-order sites).
- `C:\Users\enok\Documents\vera20k-engine-authority`, branch
  `feature/persistent-facing-authority` (Codex): uncommitted `facing_class.rs`,
  `turret.rs`, `walk_head.rs`, `snapshot.rs`. It also bumps the snapshot to
  172, as PR #417 does; whichever lands second takes 173.

## Open leads — findings so far

**Animation.** `AnimStore`/`AnimClass` (`sim/anim_class.rs`) is the native
owner and already carries combat explosions and the 21 building slots. Still
outside it:

- `WorldEffect` (`components.rs`, ticked in `world/mod.rs`, drawn in
  `app/presentation/instances/overlays.rs`), nine producers:
  `teleport_movement.rs` (WarpOut), `bridge_orchestrator.rs` (three sites),
  `superweapon/{iron_curtain,genetic_converter,force_shield}.rs`,
  `superweapon/lightning_storm.rs` (two sites).
- Garrison muzzle flashes (`components::AnimRuntime`,
  `app/presentation/building_anim.rs`) and `WeaponMuzzleFlash`
  (`fire_effects.rs`).

Moving a producer onto `spawn_anim_at_world` is a behaviour change, not a
rename: the real constructor allocates a stable id, draws `RandomRate` from the
scenario stream, plays `Report=`/`StartSound=` and runs `Middle`
(`Scorch=`, `Crater=`, `SpawnsParticle=`). The bridge producers interleave their
own RNG draws with the spawn, so a deferred spawn would reorder the stream.
Migrate one producer family per increment with native evidence for type,
coordinate, delay and flags, and attribute any replay-pin movement.

**`resource_nodes`.** Production never seeds it
(`seed_resource_nodes_from_overlays` has no production caller) and miners read
only the overlay grid (`ResourceQueryAuthority::OverlayGrid`). What remains is
a test-only lane: `LegacyNodesForTests`, `tick_ore_growth`,
`compatibility_without_native_context` in `terrain_spawn.rs`,
`reduce_legacy_resource_node_for_tests`, the hash fold and the serialized
field. About 350 references; roughly 45 miner tests seed it through
`place_ore`. The global parity harness already uses the overlay grid, so its
pins should not move. Needs a shared overlay-grid ore fixture
(`tiberium/mod.rs` tests have the full 149-entry registry recipe).

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

## Next safe action

Land PR #417 after its critic. Then the animation lead, teleport WarpOut first
(it already carries a verified `AnimClassSpawnDescriptor`).
