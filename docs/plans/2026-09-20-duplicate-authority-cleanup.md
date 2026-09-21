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
| Dead code | second sweep, for what the compiler cannot see: `pub` items of the library and items behind `allow(dead_code)`. 90 suppressions removed and 30 put back where the reason is real (native enum values, GPU resource ownership, staged native ports, RNG stream-routing audit anchors); about 40 functions nothing references deleted, with the dead `movement/scatter.rs` module, a terrain render pipeline and instance buffer nothing drew with, the depth-stamp pipeline only a GPU test draws with (now built only there), and stale constants and imports; about 460 functions and constants that only tests call are `#[cfg(test)]`, so the production build no longer carries them. This is a one-time sweep, not a guard: the `dead_code` lint still cannot see an unreferenced `pub` item of the library, so a new one will not be reported. Unreferenced `pub` items kept on purpose: native-value vocabularies with a gap a deletion would hide (`FX_EMP`/`FX_MIRROR`, `REPLAY_FLAG_*`, the rocking constants `SNAP_BACK_RATE` and `APPLY_AREA_FORCE_FLOOR`, `TRACKBAR_WM_HSCROLL_MESSAGE`, the fixed-math `SIM_EPSILON`) and native-derived staged ports (cloak/disguise helpers, house base helpers, gas and smoke particle movers). Three modules only tests reach (`movement/track_speed_native`, `movement/track_fresh_dispatch`, `map/rmg/sqrt_table`) carry the gate on their `mod` line. Non-test warnings 100 to 18: the crate summary line and the 17 that remain, which are fields only tests read and native enum values nothing constructs yet, left visible rather than suppressed | #428 |
| Per-mover world scans | the whole-world marker-peer snapshot and building entry-skip map every mover rebuilt every tick (and `track_entry.rs` per entry). The mover is lifted out of the store for its turn (`EntityStore::take_turn`), so the other entities are read live: peers by id from the cell lists `UpdateBridgePassability` walks, skips from the buildings on the queried cell's list. The mover's own facts are captured once as its turn begins. Debug builds still build both whole-world forms and compare every live read against them. Two of the three scans | #429 |
| Loader funnels | `load_rules_with_merged_ini` composed the rules layers cold, a second path beside the match load's `NativeRulesProcessOwner::load_noncampaign_scenario`; it now runs startup selection and that rebuild, and `LoadedRules` is gone. `sprite_atlas.rs` kept a `cfg(test)` copy of the effect-name list that had drifted (no `Wake=`, no projectile images); production and tests call the one `collect_effect_names` | #429 |
| Per-mover world scans | the third scan, the owner block sets every moving object's turn rebuilt from every entity. `EntityStore` logs each entity it hands out mutably (every route to a `&mut GameEntity` goes through it), and `movement/block_index.rs` keeps one shared record of where each entity sits plus each owner's sets, re-deriving only the logged entities and rewriting only the cells they touch, at the same two points the sets used to be rebuilt (pass preparation; a turn's start when occupancy moved), so a pass sees what a build of the whole world would give it. One rule (`contribution`) and one insert (`insert_unit`) serve the index and the whole-world build, which path searches outside a pass, tests and the debug-build comparison still use | #430 |
| Loader funnels | scenario construction only tests called, in an order production does not use: `runtime::construct_scenario`, `construct_scenario_with_generated_inits`, `init_helpers::construct_app_scenario`, `HeadlessTerrainBootstrap` with its own terrain Fill funnel, the bootstrap-side generated-construction replay wrapper, and the `ScenarioBootstrapRng::terrain_draws` and `replay_generated_construction_trace` probes with the unit tests that ran Fill and the replay on the bare bootstrap owner. Their users (two synthetic headless tests, the random-map launch snapshot behind `gsi_04_12`) now stage the one `Simulation` first, as the match load does (`into_stock_offline_staged_simulation` with the prefix bound to the native rules receipt, or `into_simulation` where there is no launch prefix), let Fill and the constructor replay draw from that owner, and populate it with the production functions; each asserts that Fill allocates the extent the descriptor announced. The random-map launch snapshot also gained the steps of the generated arm it had skipped (the `[Tubes]` and post-load particle-system native ids, the generator tail's tiberium queues and final germination) and no longer asks the post-map finalizer to rebuild the queues, which pinned the opposite of what a match load does. With that, the finalizer's rebuild branch (`tiberium_queues_preinitialized: false`, a near copy of `runtime::initialize_native_tiberium_queues`) had only unit tests left; the flag, the branch and the `tiberium_queues` output are deleted, and those tests build the queues with the production function before the post-map tail, as both load arms do | #431 |
| Per-mover world scans | found by the final audit: the blocker-neighbour plane was cached under a key that included the occupancy generation, so in traffic each moving object's turn rebuilt it from the whole map and every entity, and the resumed Walk path request built it uncached. It is now a sum kept current: a part no entity contributes to (terrain objects, the retained wall plane, under their own epochs) plus one source per marked object, taken out and put back from the store's touch log (wrapping byte counts, so exact). `EntityStore::dying_epoch` went with the old key | #432 |

Three whole-entity sweeps went with those (per frame, not per mover): the per-frame dock sweep, the
whole-entity scan in `interrupt_refinery_docked_miners`, and the per-frame
building anim phase scan.

Ghidra: the MEGAMISSION comments at `0x004C72F8`, `0x004C733C`, `0x004C7342`
now name the radio BREAK (`vt+0x274` = `RadioClass::Transmit_Radio_ToFirst`,
read from the Unit vtable); the `AnimClass` Start/Middle labels were already
correct in the database and the stale source notes saying otherwise are gone.
Two identities this work got wrong were wrong in its own source notes, not in
the database, and were corrected there: `0x005F5940` is labelled
`ObjectClass__Paradrop` (`ObjectClass__Unlimbo` is `0x005F4EC0`), read back
2026-09-21; `0x00ABDE88` is a Map-module scalar, not an animation global. No
other label was changed.

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

**The random-map launch snapshot (for the final audit to confirm).**
`MapLoadInitial::into_random_map_launch_snapshot` (`cfg(test)`, behind
`gsi_04_12`) is a second sequencing of the accepted-generated arm of
`load_map_from_initial`. It cannot call that function, which takes the GPU
context and interleaves presentation work; sharing the sequence means
extracting a GPU-free core out of the app loader, a change to production
loading that belongs to the random-map work, not a fold of duplicate state. It
now follows the match load's order with the match load's functions (staged
`Simulation`, bound prefix, native-id reservations, Fill, replay, populate,
generator tail, metadata, launch session, post-map finalizer). What it still
leaves out is named in its doc comment: the Team AI registry, the shared cell
dummy's Resize reconstruction and the theater registry publication. Its
`final_rng` and `post_map_output` are compared between two launch routes, not
certified equal to a match load's. The two probes left on
`ScenarioBootstrapRng` (`install_pre_fill_scenario_prefix_plan`,
`logical_states_for_test`) install the prefix through the same
`install_before_terrain` production uses and read cursors; their users stage
the `Simulation` next, and none runs Fill.

**The headless loader (for the final audit to confirm).**
`headless_scenario::load_with_launch` is a third sequencing of the load, for
authored maps without a GPU. Unlike the random-map snapshot it shares the
funnel: both it and the app call
`runtime::finalize_and_populate_staged_authored_scenario` and
`finalize_constructed_scenario`, and the steps before them are asset and rules
loading. The generated arm has no such shared funnel, which is the snapshot's
retention above.

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
`pub` cloak helpers, `suppresses_ordinary_damage`, the staged Fire_At damage
math and fire-gate vocabulary (`combat/damage/attacker.rs` `fire_damage`,
`fire_decision.rs` `FireDecision`, `base_defense_response.rs`
`ResponderPeekFireError`) and the wave
`color_mode`/`registration_bucket` pair (the only record of the native
wave-type mapping), which nothing references yet. The six unused `state_hash_without_*`
probes in `world_hash.rs` are replay-pin provenance.

## Open

Nothing.

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
- 2026-09-21, with the owner block index, both maps again: the miner docks
  and deposits, the chrono miner warps to the pad twice; 0 ERROR lines.
- 2026-09-21, main at #431 (the post-map finalizer without its queue flag):
  `minerloop.map` loads, builds the authored tiberium queues before the
  Technos (198 growth, 316 spread), docks and deposits; 0 ERROR lines.
- 2026-09-21, main at #432 plus the two quickplay switches below, release
  build. `combatfixture.map` (the miner fixture plus Americans MTNK x2 and E1,
  and a second house `Computer1` with HTNK x2, E2 and a power plant), run with
  `RA2_QUICKPLAY_OPPONENTS=1` and `RA2_QUICKPLAY_SCREENSHOT_AT=<ticks>`: both
  houses' objects spawn; the tanks turn their turrets and fire; the frames at
  ticks 50 and 55 show the `AnimStore` muzzle-flash animation at each Grizzly's
  barrel, and the GI's tracer; 21 frames, 0 ERROR lines. `combathunt.map` (the
  same with the opposing tanks on Hunt from six cells farther): both houses'
  units drive and path at once while the miner harvests, the Rhinos close to
  point-blank and fight; 10 frames over 800 ticks, 0 ERROR lines. The frames
  are game art, so they stay out of this public repository; the fixtures and
  switches reproduce them.

`RA2_QUICKPLAY_OPPONENTS=<n>` adds opponent houses (`Computer1`...) to the
quickplay sandbox so a fixture map can own objects by them (a house with no
building is defeated at once under the short-game rule, so give it one);
`RA2_QUICKPLAY_SCREENSHOT_AT=<tick,...>` takes the ordinary ScreenCapture
screenshot at those simulation ticks. With them, a fixture with pre-placed
hostile objects is a zero-interaction release scene, which the earlier text
here wrongly said could not be had.

Not exercised in a release build: a superweapon invoke and a Lightning Storm
(a charged superweapon needs a click on the sidebar and a target), a paradrop
(same), a bridge collapse (needs a force-fire order), a flying Jumpjet unit, a
crusher order over a wall, the bridge marker peers on a real bridge crossing, a
save and load at snapshot 180, the radar event ring, and the generated-map arm
of the load (`RA2_QUICKPLAY=<name>.sed` stops at "generated launch has no
accepted setup start staging", so a random map needs the skirmish shell). The
fixtures above can be extended to several of these (a Rocketeer, a walled
crusher path, units ordered across a bridge by a Hunt mission); the ones that
need a click cannot be driven without an input script, and desktop automation
cannot be granted for a development executable. Their production functions
are exercised in the test profile by tests that go through `Simulation`:
`a_fired_shot_constructs_its_muzzle_anim_in_the_store`,
`commit_builds_the_flash_before_it_tears_the_firer_down`,
`a_drop_attaches_a_canopy_that_winds_down_at_landing_and_plays_out`,
`launch_constructs_the_invoke_anim_and_plays_its_report`,
`relocate_spawns_departure_and_arrival_warpout_rows`, the Lightning Storm bolt
tests and the bridge orchestrator's `BridgeExplosions=` tests. The known
release-only hazard class, a side effect inside `debug_assert!`, is denied by
clippy (`debug_assert_with_mut_call = "deny"` in `Cargo.toml`), and the deny
lints pass on every merged PR.

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

### Rows of the 2026-09-20 discovery ledger this goal did not take

The goal named its leads. `2026-09-20-engine-authority-audit.md` lists more
duplicate state than that; the rows outside the named leads are recorded here
with what they cost, not resolved:

- **Economy purifier mirror.** `Simulation::refresh_economy_shadow` sweeps every
  entity once a frame to write `Economy::purifier_count`, which is hashed and
  saved and read by nothing; deposits count purifiers live
  (`miner_system::effective_purifier_count`). Trigger: every frame. Effect:
  none on play; one O(entities) sweep per frame and a dead hashed field.
  Retiring it moves the hash composition (a schema feature, snapshot bump and
  the three harness pins), which is why it is its own change.
- **Two aircraft dock state machines.** `docking/aircraft_dock.rs`
  (`tick_aircraft_docks`) handles aircraft with ammo and no `AircraftMission`;
  `aircraft::tick_aircraft_missions` handles those with one. The gate
  (`aircraft_mission.is_some()`) makes them disjoint, so no object has two
  authorities, but runtime-built Fly aircraft always get a mission
  (`world_spawn/construction.rs`), which leaves the legacy machine to
  map-authored aircraft and non-Fly types with ammo. Folding it is a migration
  of those cases onto the mission machine.
- Shell pointer mirrors, sidebar scroll, audio initialization, Drive/Ship
  track state, Object Z reconstruction, and movement without a locomotor
  runtime: not rechecked by this goal.

Smaller leftovers the final audit noted: `common.air_phase` is still hashed and
captured by piggyback for Jumpjets, where it is now constant; per-order path
searches build their own block sets and blocker plane instead of using the
pass cache; `apply_zoom` in `app/input/camera.rs` is kept by its own note for a
future binding; `HashSchema::Before(..)` branches in the production hasher are
reached by tests only (they are the provenance of the re-baselined pins).

## Other checkouts

- The separate `vera20k-engine-authority` checkout, branch
  `feature/persistent-facing-authority`: dirty facing, turret, walk-head
  and snapshot files. No open lead above touches them; its snapshot bump must
  land after 180.
