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
| Mover path facts | the crusher flag every move-order caller passed; facts now come from the mover (`from_snapshot`, `from_entity_without_wall_arm`), cell-entry contexts take the wall-arm key from `CrushCapability`. The wall arm on order and Drive tick searches is movement-ledger row I9b, a parity port, not a duplicate | #423 |
| Animation | `AnimStore`'s private 128-per-level Z scale: one frame, world leptons (104 per level), owner-attached anims follow the owner's actual height, sprites project from exact Z. Prerequisite for the muzzle flashes; snapshot 177 | #424 |
| Animation | weapon `Anim=` and occupant `OccupantAnim=` muzzle flashes on `AnimStore`, built at the sim's one fire coordinate (`combat/fire_coord.rs`, also the bullet origin and the report sound position). A building's FLH arm now starts from the building coordinate native uses (location minus 128), like its pixel arms. Deleted: the app's second AnimClass stepper, both flash lists and draw paths, the `f32` FLH transforms and pixel-offset math. Snapshot 178 | #425 |
| Animation | the parachute canopy: an owner-attached `AnimClass` built by the drop (`ObjectClass::Paradrop @ 0x005F5940`, canopy at `0x005F5A9D`) and wound down at landing (`0x005F3F9D`). Deleted: the app's third stepper (`chute_anim.rs`, `ParachuteAnim`) and `ParachuteRenderConfig`, a second parse of the `PARACH` AnimType's art. The app keeps only the canopy's placement on the body's sort key. Snapshot 179 | #426 |
| Move phases | the Jumpjet's `AirMovePhase` mirror: readers take the locomotor's own state field (`JumpjetRuntime::phase`, all seven native values) and the mirror is no longer written; the legacy VERA-only jumpjet physics with no caller left. Snapshot 180 | #427 |
| Dead code | items dead in both builds; superseded test-only duplicates (map-list funnels, `radiation_light_epoch`); test probes gated; the effect asset catalog trimmed to particle images, which changes the rules hash, so snapshot 176 | #421 |
| Dead code | second sweep, for what the compiler cannot see: `pub` items of the library and items behind `allow(dead_code)`. 90 suppressions removed and 30 put back where the reason is real (native enum values, GPU resource ownership, staged native ports, RNG stream-routing audit anchors); about 40 functions nothing references deleted, with the dead `movement/scatter.rs` module, a terrain render pipeline and instance buffer nothing drew with, the depth-stamp pipeline only a GPU test draws with (now built only there), and stale constants and imports; about 460 functions and constants that only tests call are `#[cfg(test)]`, so the production build no longer carries them. This is a one-time sweep, not a guard: the `dead_code` lint still cannot see an unreferenced `pub` item of the library, so a new one will not be reported. Unreferenced `pub` items kept on purpose: native-value vocabularies with a gap a deletion would hide (`FX_EMP`/`FX_MIRROR`, `REPLAY_FLAG_*`, the rocking constants `SNAP_BACK_RATE` and `APPLY_AREA_FORCE_FLOOR`, `TRACKBAR_WM_HSCROLL_MESSAGE`, the fixed-math `SIM_EPSILON`) and native-derived staged ports (cloak/disguise helpers, house base helpers, gas and smoke particle movers). Three modules only tests reach (`movement/track_speed_native`, `movement/track_fresh_dispatch`, `map/rmg/sqrt_table`) carry the gate on their `mod` line. Non-test warnings 100 to 18; the 17 that remain are fields only tests read and native enum values nothing constructs yet, left visible rather than suppressed | #428 |
| Per-mover world scans | the whole-world marker-peer snapshot and building entry-skip map every mover rebuilt every tick (and `track_entry.rs` per entry). The mover is lifted out of the store for its turn (`EntityStore::take_turn`), so the other entities are read live: peers by id from the cell lists `UpdateBridgePassability` walks, skips from the buildings on the queried cell's list. The mover's own facts are captured once as its turn begins. Debug builds still build both whole-world forms and compare every live read against them. Two of the three scans; the owner block sets are under Open | this PR |
| Loader funnels | `load_rules_with_merged_ini` composed the rules layers cold, a second path beside the match load's `NativeRulesProcessOwner::load_noncampaign_scenario`; it now runs startup selection and that rebuild, and `LoadedRules` is gone. `sprite_atlas.rs` kept a `cfg(test)` copy of the effect-name list that had drifted (no `Wake=`, no projectile images); production and tests call the one `collect_effect_names` | this PR |

Three per-mover world scans went with those: the per-frame dock sweep, the
whole-entity scan in `interrupt_refinery_docked_miners`, and the per-frame
building anim phase scan.

Ghidra: the MEGAMISSION comments at `0x004C72F8`, `0x004C733C`, `0x004C7342`
now name the radio BREAK (`vt+0x274` = `RadioClass::Transmit_Radio_ToFirst`,
read from the Unit vtable); the `AnimClass` Start/Middle labels were already
correct in the database and the stale source notes saying otherwise are gone.

## Independent audit, 2026-09-20

A read-only reviewer checked the rows above against main and accepted them. It
accepted the debris bouncer retention and rejected the others this file used
to carry: the movement branch named as an owner had already been squash-merged
as `527ac301` (identical tree), so no open lead is blocked by another session,
and the sim already computes an integer fire coordinate, so the muzzle flashes
are not blocked on a port. The goal is not done.

## Retained

**Animation: debris bouncers (accepted by the audit).** `MetallicDebris=` and `DebrisAnims=` types are
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

**`GroundMovePhase` (accepted by the #427 review).** A seven-value label
`update_locomotor_phases` recomputes at the end of every movement pass from
`path_blocked` and the mover's speed. Only two values are read: `Blocked` by the
Walk readiness producer and `Idle` by the piggyback end gate. It is not a pure
mirror: both readers see the value sampled at the end of the mover's last pass,
so replacing it with a live read of `path_blocked` moves Walk `Is_Moving_Now` by
a frame around a new order or a cleared block. What the native Walk predicate
reads there (the next-step coordinate) is the reconciliation movement rows
GSI-06.13 and GSI-06.14 own; removing the label is that port, not a fold.

**Jumpjet destination (accepted by the #427 review).** `movement_target`
and `JumpjetRuntime::destination` are two native fields, not two copies:
the Foot destination the order writes, and the locomotor's own cached
coordinate at `+0x40` that `Move_To` fills and `Process` flies toward. The
cruise host hands one to the other and clears the order on arrival.

**Staged native ports with no production caller yet (for the final audit to
confirm).**
`track_fresh_dispatch.rs`, `track_speed_native.rs`, the `load_object_lifecycle`
wall arm, `all_to_hunt_score_override`,
`projectile_slope_reflect_with_elasticity`, `advance_emergency_state`,
`snap_to_passable`, the engineer bridge-repair walker, the retail multiplayer
checksum: verified native work with golden or oracle tests. Deleting them
discards evidence-backed work; wiring each is a port. Their functions are
`#[cfg(test)]` since the second sweep (only tests reach them; a port removes the
gate), and so are the types only they use. Untested staged ports kept as they were: the House
base-centre helpers (`world/house_base.rs`, AI-deferred) and the gas and smoke
wind movers (need `WindDirection=`) behind their `allow(dead_code)`, and four
`pub` cloak helpers, `suppresses_ordinary_damage` and the wave
`color_mode`/`registration_bucket` pair (the only record of the native
wave-type mapping), which nothing references yet. The six unused `state_hash_without_*`
probes in `world_hash.rs` are replay-pin provenance.

## Open

**Owner block sets, the third per-mover world scan.**
`bump_crush::build_entity_block_sets` still walks every entity for each moving
object's turn (`prepare_movement_pass`, `refresh_owner_block_set_if_stale`, and
once more per Walk path search). A* and the blocked-tick handlers take the
result as whole sets through some fifty signatures, and the Drive selection
gate asks it at every cell crossing, so it cannot become a per-cell lookup the
way the other two did without changing what A* sees. Same lead, not done;
movement ledger row I2c carries the measurements.

**Scenario construction only tests call.** `runtime::construct_scenario`,
`construct_scenario_with_generated_inits`, `init_helpers::construct_app_scenario`
and `HeadlessTerrainBootstrap::construct_scenario` only sequence production
functions (`ScenarioBootstrapRng::into_simulation`, then
`populate_staged_scenario_with_generated_inits`), but
`build_headless_terrain_bootstrap` is a test-only terrain Fill funnel with its
own order, and all of them create the `Simulation` after Fill where production
stages it before, so what Fill does to the staged owner is untested on that
path.

## Production validation

Release build (`cargo build --release`), zero-interaction fixture maps through
`RA2_QUICKPLAY`, log read from `logs/ra2.log`:

- 2026-09-21, `minerloop.map` (Americans NAREFN + HARV): loads, 0 ERROR lines,
  full SearchOre, Harvest, ReturnToRefinery, Dock, deposit, SearchOre cycle
  (overlay-grid ore, radio-bus dock). The first such run failed to load any map
  (`required SHP for animation type [CAARAY_A]`); fixed in #422.
- 2026-09-21, after the live per-turn reads, same map: the miner drives onto
  the refinery pad through the live building-entry skips (bib cell and radio
  contact), Dock at (41,85), cargo 40 to 0, next cycle starts; 0 ERROR lines.
- 2026-09-21, `chronoloop.map` (the same map with GAREFN + CMIN): two full
  cycles; each ReturnToRefinery to Dock covers (38,88) to (41,85) inside about
  a second, which is the teleport relocation and with it the WarpOut
  `AnimStore` rows at both ends; cargo 20 to 0; 0 ERROR lines. The log has no
  line per constructed animation, so the run shows the warp path executing
  without error, not the pixels.

Not exercised in a release build: a superweapon invoke, a Lightning Storm, a
bridge collapse, a weapon muzzle flash, a paradrop. Each needs player input or
a second house, and the release binary has neither an input script nor a
quickplay opponent (by design: an empty AI house is defeated at once and ends
the session); desktop automation cannot reach a development executable. Their
production functions are exercised in the test profile by tests that go through
`Simulation`: `a_fired_shot_constructs_its_muzzle_anim_in_the_store`,
`commit_builds_the_flash_before_it_tears_the_firer_down`,
`a_drop_attaches_a_canopy_that_winds_down_at_landing_and_plays_out`,
`launch_constructs_the_invoke_anim_and_plays_its_report`,
`relocate_spawns_departure_and_arrival_warpout_rows`, the Lightning Storm bolt
tests and the bridge orchestrator's `BridgeExplosions=` tests. The known
release-only hazard class, a side effect inside `debug_assert!`, is denied by
clippy (`debug_assert_with_mut_call`), and clippy is clean on every merged PR.

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
- A building's Z has several readers that agree only on flat ground: damage
  fire anims use `object_world_z_leptons` (resamples terrain), slot anims use
  `position_world_coord` (`level * 104`), `movement_sound_world` ignores
  `exact_z_leptons`.
- A `MakeInfantry=` anim placed by a level byte on a bridge deck over a ramp
  cell sits at `(L+4) * 104`, under the ramp ground plus the 416-lepton deck
  test in `apply_anim_raw_occupation`, so it marks the ground occupation bits.
  The old 128 scale passed that test by accident. Rare; the producer's level
  byte is the coarse input.
- `tests/refinery_live_rules.rs` read the private `GameEntity::owner` and
  `type_ref` and did not compile; it uses the accessors since the second sweep,
  so all 32 integration targets and the binaries build. Integration tests are
  outside the `--lib` gate, so nothing runs it; it was compiled, not run.
- Combat death debris never binds a sprite (GSI-05.14); theaters other than
  TEMPERATE were not checked for unbound building animations.

## Other checkouts

- The separate `vera20k-engine-authority` checkout, branch
  `feature/persistent-facing-authority`: dirty facing, turret, walk-head
  and snapshot files. No open lead above touches them; its snapshot bump must
  land after 180.
