# Test audit — src/sim/world/** (static read, no cargo)

Scope: 1,138 `#[test]` items found by `#[test]` scan (the `assert_routes_scenario!` macro in
rng_routing_tests.rs expands to 4 tests but is counted once). 34 are `#[ignore]` (retail/RA2_DIR
e2e, diagnostics, known failures) and never run in the default suite.

Format: `path::test` | CATEGORY | evidence | confidence | action

Named non-ignored e2e used below:
- `global_parity_harness_tests.rs::global_skirmish_replay_is_deterministic_and_baseline_stable`
  (600-tick record/replay, per-tick hash equality, final hash + per-stream RNG pins; spawns via
  `spawn_from_map`; Move/AttackMove/Stop only, no production, no bridges).
- `bridge_parity_harness_tests.rs::bridge_crossing_replay_is_deterministic_and_baseline_stable`
  (200-tick high-bridge crossing, same shape).

---

## 1. OBSOLETE — hash-schema "historical projection" system (biggest structural find)

Evidence: `hash_schema.rs:1-38` header — projections "cannot reproduce old hash streams or certify
their golden values … do not promise snapshot loading compatibility or native parity".
Production rejects any non-current snapshot (`snapshot.rs:1151`, `SNAPSHOT_VERSION = 242`), and
`HashSchema::Before(u16)` / `BeforeBuildingPowerIntegration` are `#[cfg(test)]`
(`hash_schema.rs:40-48`); production only ever calls `state_hash_with_schema(Current)`
(`world_hash.rs:563-565`), so every `schema.includes(..)` is constant-true in production.

### 1a. Tests that exist only for projections — delete whole test (20)
- `hash_schema.rs::detached_track_absence_projection_ends_at_schema167` | OBSOLETE | asserts `Before(167)` mask only (`hash_schema.rs:307-312`) | H | delete with hash_schema.rs
- `hash_schema.rs::retired_tiberium_fold_ends_at_schema174` | OBSOLETE | mask only (`:314-321`) | H | delete
- `hash_schema.rs::historical_policies_preserve_original_positional_masks` | OBSOLETE+MIRROR | 114-line table of masks "frozen from the 25-argument calls before this refactor (main c1983f56)" (`:323-437`) | H | delete
- `world_hash.rs::prior_track_free_projection_keeps_retained_residual_and_short_state` | OBSOLETE | only `state_hash_without_track_authority_v167` (`world_hash.rs:3415-3437`) | H | delete
- `world_hash.rs::prior_projection_rejects_active_drive_instead_of_fabricating_absence` | OBSOLETE/TESTONLY | `should_panic` on test-only `assert_retired_track_projection_is_bounded` (`:3439-3443`, `:353`) | H | delete
- `world_hash.rs::prior_projection_rejects_active_ship_instead_of_fabricating_absence` | OBSOLETE/TESTONLY | same (`:3445-3449`) | H | delete
- `world_hash.rs::base_plan_center_affects_only_current_v111_hash_schema` | OBSOLETE | v111 probe; current half is a field-hash MIRROR (`:3740-3760`) | H | delete
- `world_hash.rs::house_harvester_no_ore_affects_only_current_v132_hash_schema` | OBSOLETE | v132 probe (`:3765-3786`) | H | delete
- `world_hash.rs::house_eva_advice_affects_only_current_v133_hash_schema` | OBSOLETE | v133 probe (`:3790-3818`) | H | delete
- `world_hash.rs::house_ai_activation_hash_field_order_preserves_v113_and_v112_streams` | OBSOLETE+MIRROR | re-hashes fields by hand to match `hash_house_ai_activation_fields(_, v112, v113)` bool arms (`:3894-3946`, fn at `:44-65`) | H | delete, drop the two bool params
- `world_hash.rs::gsi_04_01_real_cell_bridge_authority_is_current_schema_only` | OBSOLETE | pre-v28/v29 probes; current half = field-hash (`:319-344`) | H | delete
- `world_hash.rs::live_house_statistics_change_current_hash_and_preserve_older_projections` | OBSOLETE | `Before(227)` probe + field-hash (`:3522-3555`) | H | delete
- `world_hash.rs::bridge161_projects_dummy_only_and_keeps_real_owner_identity` | OBSOLETE | every assertion but one is `Before(161)` (`:5388-5419`) | H | delete
- `world_hash.rs::prism_support_state_folds_only_when_set` | OBSOLETE | `Before(225)` + field aliasing (`:5504-5556`) | H | delete
- `world_hash.rs::gsi_04_07_hashes_live_dummy_overlay_identity_and_state` | OBSOLETE | v114 probe + 2 field-hash asserts (`:232-256`) | H | delete
- `aircraft_deployment_tests.rs::mission_only_has_its_own_hash_fold` | OBSOLETE | `Before(189)` + one field-hash (`:108-126`) | H | delete
- `fly_height_tests.rs::fly_cruise_mode_hashes_separately_from_destination` | OBSOLETE | `Before(191)` + one field-hash (`:1108-1127`) | H | delete
- `production_shadow_tests.rs::legacy_progress_carry_removed_from_hash` (ignored) | OBSOLETE | `#[ignore = "P5D-REVIEW … no longer expressible"]`, comment "Human: confirm … or delete the test" (`:435-459`) | H | delete
- `world_hash.rs::retained_wall_neighbor_plane_changes_hash_with_identical_final_cells` | OBSOLETE+MIRROR | v115 probe + one field-hash (`:2938-2963`) | M | delete (or keep current half in consolidated coverage test)
- `world_hash.rs::retained_wall_neighbor_authority_mode_changes_hash_even_when_zero` | OBSOLETE+MIRROR | same (`:2966-2993`) | M | same

### 1b. Mixed tests — keep test, strip projection assertions (16, no count reduction)
- `global_parity_harness_tests.rs::global_skirmish_replay_…` — 17 projection pins (`:710-838`, `:910-924`) incl. `BeforeBuildingPowerIntegration` "full08" probe plus the 5 `assert!` fixture preconditions it needs (`:760-802`); ~12 `GLOBAL_HARNESS_FINAL_HASH_PRE_*` consts.
- `bridge_parity_harness_tests.rs::bridge_crossing_replay_…` — 12 pins (`:774-829`).
- `slice6_retask_tests.rs::replay_hash_stable_through_slice6` — 14 pins + `SLICE6_*_PRE_*` consts (`:128-178`, `:452-523`), and the ai_counter inversion (see WRONG §6).
- `world_hash.rs`: `gsi_04_03_hashes_dummy_level_slope_without_retained_projectile`, `gsi_04_05_base_plan_state_and_entity_facts_are_current_schema_hash_authority`, `house_ai_activation_hash_matches_native_direct_crc_fields_only` (keep its native-census AutoBaseBuilding exclusion), `foot_path_runtime_hash_and_snapshot_survive_without_movement_adapter`, `foot_scold_byte_survives_snapshot_and_preserves_prior_hash_projections`, `bridge161_hashes_each_active_and_stashed_payload_field`.
- `lifecycle_tests.rs::display_lifecycle_is_independent_of_logic_and_survives_production_save` (`:7859-7867`), `::unlimbo_levels_then_aims_the_barrel_elevation` (`:8198-8220`).
- `fly_height_tests.rs::fly_destination_is_hashed_and_persisted_in_active_and_stashed_runtime`, `::fly_landing_state_hashes_and_restores_active_and_stashed_instances`.
- `house_base.rs::factory_plant_discount_follows_unlimbo_capture_and_expiry` (`plant_fold` closure `:360-365`).
- `sinking_tests.rs::sinking_state_is_hashed_only_in_its_schema_and_survives_snapshot` (`:137`, `:146`).

Cross-partition users (for dedup by other auditors): `credit_income.rs::credit_income_state_affects_only_current_v135_hash_schema` (pure projection), `crates/state.rs::crate_authority_every_raw_word_changes_v114_hash_only`, `snapshot.rs::naval_build_const_order_and_membership_roundtrip_with_current_hash_only` (`:4401-4493`), `vision/vision_tests.rs::shroud_current_sight_provenance_changes_future_conceal_and_hash` (`:1959-1974`), `team_script_vm.rs:1539` (recruitment=false probe).

### 1c. Test-only production code deletable with §1 (~830 LOC)
- `hash_schema.rs` entire (439 LOC; `HashFeature` has no meaning once only `Current` exists).
- `world_hash.rs` production region: ~198 LOC `#[cfg(test)]` items/blocks (15 `state_hash_without_*/before_*` fns `:573-673`, `:1363-1367`; `assert_retired_track_projection_is_bounded` + `hash_retained_track_classes_before_167` `:353-447`; aircraft release/mission cfg(test) arms `:1596-1617`, etc.), ~109 LOC of never-taken legacy `else`/`!includes` branches, `hash_mission_com_before_v29` (`:489-534`, 47 LOC, called only under `!includes(Mission)`), and 127 `schema.includes(..)` gates collapse to straight-line folds; drop `schema` param from 8 fold fns.
- Helpers: `docking/aircraft_dock.rs:117 hash_before_pending_release` (cfg(test)), `capture_manager.rs:212 hash_before_mind_control`, `display_layers.rs:168 fold_hash_excluding` (cfg(test)), `ore_growth.rs:1462` `retired_scanner_fold` param, `team_script_vm.rs:1077-1081` `ai_teams`/`recruitment` params, `world_hash.rs:44` two bool params.

---

## 2. TESTONLY / MIRROR — test-only second implementation ("Checkpoint A")

- `techno_ai.rs::checkpoint_a_*` (15 tests, `:5209-6090`) | TESTONLY+MIRROR | every test asserts the output of `trace_cloned_ordinary_drive_host` (`:4897`), a test-only re-implementation of the Drive host control flow (Guard B/E, 6E0/6E1/6E2 bytes, dispatch timer, jitter, foot process gates) that emits `HostTraceEvent`s; no checkpoint test calls `object_ai_visit_one`/`advance_tick` (grep of `:4760-6100`). This is exactly "a second implementation of the same logic". Production behaviour is covered by `move_handler_rearms_from_the_authoritative_object_ai_host`, `move_handler_preserves_pending_timer_without_consuming_rng`, `move_handler_arrival_returns_one_frame_without_rng` (`:3599-3703`, real `object_ai_visit_one`) | H | delete 15 tests + ~1,480 LOC model (`:3339-3512` enums/structs/`stock_move_control`, `:4783-6091`); keep `ordinary_drive_host_sim`.

## 3. TESTONLY (other)
- `techno_ai.rs::unit_dispatch_attackmove_unreachable_for_units` | TESTONLY+MIRROR | `derived_mission` is `#[cfg(test)]` (`game_entity.rs:1521`); `derived_mission_with` has no AttackMove arm (`:1529-1565`) — trivially true | H | delete
- `production_shadow_tests.rs::factory_shadow_trace_order_matches_logic_vector` | TESTONLY/MIGRATION | `factory_shell_trace_order` is `#[cfg(test)]`, trace is `cfg(any(test, debug_assertions))` "P2 … no authoritative step … never hashed" scaffolding (`techno_ai.rs:1625-1705`) | H | delete test + ~70 LOC trace (`FactoryShellTrace`, `factory_shell_trace`, `debug_assert_factory_shell_trace`; its asserts are tautological by construction)
- `logic_vector.rs::forced_insert_failure_is_one_shot_and_non_mutating` | TESTONLY | tests the cfg(test) injection seam itself (`logic_vector.rs:16-19,41-44,88`) | H | delete (the lifecycle tests that use the seam stay)
- `lifecycle_tests.rs::lifecycle_authority_set_logic_order_for_test_synchronizes_all_membership_flags` | TESTONLY | tests `#[cfg(test)] set_logic_order_for_test` (`mod.rs:4186`) | M | delete (fixture helper)
- `world_tests.rs::derived_transit_separation_bound_is_inside_one_cell` | TESTONLY | validates test helper `derived_min_transit_separation_leptons` (`:9000-9025`) | M | delete or fold into helper `debug_assert`

## 4. MIGRATION — production/economy "shadow" scaffolding (production_shadow_tests.rs)
Header (`:1-7`) still states the shadow "leaves state_hash() bit-identical … SNAPSHOT_VERSION stays 17"; now the registry is authoritative/hashed (`:270-272`) and `refresh_production_shadow` is only `refresh_economy_shadow` (`mod.rs:4106-4108`), which only writes `purifier_count` over existing houses (`mod.rs:4074-4097`).
- `::economy_shadow_does_not_create_houses` | MIGRATION | loop is over `self.houses.iter_mut()` — trivially true | H | delete
- `::production_shadow_does_not_create_houses` | MIGRATION | same function | H | delete
- `::insertion_seq_stable_across_rebuild` | MIGRATION | "refresh no longer reconciles" (`:257-266`) — trivially true | H | delete
- `::factory_step_matches_legacy_shadow_holds` | MIGRATION | no assertion; names a non-existent `debug_assert_factory_step_matches_legacy` (`:664-681`); debug asserts run in every advance_tick anyway | H | delete
- `::factory_reconcile_seeds_zero_and_persists_progress` | MIGRATION | PERSIST half trivially true (refresh doesn't touch registry); SEED half goes through cfg(test) `test_enqueue_kernel` | M | delete
- `::purifier_refresh_preserves_house_wallet` | MIGRATION/MIRROR | function never writes credits | M | delete
- `world_tests.rs::current_rust_frame_call_order_is_preserved` | MIGRATION/DUP | "F07 characterization … before the SimRuntime extraction" (`:10325-10330`); `runtime_frame_call_order_matches_the_app_seam` asserts the same through `SimRuntime::advance_frame`, which calls `advance_app_frame` (`runtime.rs:212`) | M | delete
- Stale references (doc fix, not tests): `techno_ai.rs:1841-1845` cites removed `refresh_mission_shadow` and `object_ai_stage_commits_live_unit_mission`; `production_shadow_tests.rs:270,372` cite removed tests.

## 5. DUP / E2E
- `production_shadow_tests.rs::factory_step_order_matches_legacy_temporal_order` | DUP | same fixture/assert as `factory_insertion_seq_equals_front_enqueue_order` (Aircraft-then-Vehicle, sweep order) (`:582-658`) | H | delete
- `production_shadow_tests.rs::production_shadow_preserves_advance_tick_phase_order`, `techno_ai.rs::techno_ai_shell_preserves_advance_tick_phase_order`, `techno_ai.rs::unit_dispatch_preserves_advance_tick_phase_order` | DUP+E2E | three byte-identical bodies: empty `Simulation::new()`, 5 `advance_tick`, `run()==run()`; strictly weaker than global harness per-tick replay equality | H | delete all 3
- `rng_routing_tests.rs::determinism_both_streams_match_across_ticks` | E2E | empty-sim run-twice (`:340-363`); global harness pins per-stream + replay | M | delete
- `smudge_integration_tests.rs::smudge_state_hash_stable_across_advance_tick` | E2E/MIRROR | empty-tick run-twice (`:102-137`) | M | delete (keep the 5-cell sanity in roundtrip test)
- `mission_authoritative_tests.rs` (all 3: `mission_current_…`, `mission_timer_and_substate_…`, `mission_queued_and_suspended_…`) | DUP | subset of `world_hash.rs::every_mission_com_raw_field_changes_state_hash` (`:3117-3201`, all 9 raw fields) | H | delete file
- `world_tests.rs::mission_host_counter_changes_state_hash` | DUP | = `techno_ai.rs::object_ai_stage_ticks_every_live_object_counter` + ai_counter row of `every_mission_com_raw_field_…` | H | delete
- `smudge_integration_tests.rs::different_smudge_state_yields_different_hash` | DUP | = `world_hash.rs::hash_changes_when_smudge_placed` (`:4812`) | H | delete
- `rng_routing_tests.rs::each_stream_reproduces_gamemd_raw_sequence_seed_one` | DUP | `both_streams_seed_byte_identically` proves both streams == `SimRng::new(seed)`; `rng.rs::test_gamemd_raw_sequence_seed_one` pins the same 3 words | H | delete
- `world_tests.rs::test_bridge_damage_rebuilds_path_grid` | DUP | same scenario/asserts as `test_bridge_collapse_signals_pathgrid_refresh` (which also checks `state_changed`) (`:4296-4387`) | H | delete
- `world_orders_bridge_repair_tests.rs::stock_high_cabhut_no_overlay_fallback_collapses_bridge` | DUP | its seeder is literally `seed_hut_fallback_bridgehead_layout` (`:424-428`); same spawns/asserts as `c4_on_cabhut_bridgehead_fallback_collapses_bridge` | H | delete
- `world_orders_bridge_repair_tests.rs::stock_low_cabhut_no_overlay_fallback_collapses_bridge` | DUP | seeder = `seed_hut_pure_bridgehead_fallback_layout` (`:430-434`); same as `c4_on_cabhut_pure_bridgehead_fallback_uses_opposite_anchor_offset` | H | delete
- `substrate.rs::enter_order_counter_new_starts_at_one` | DUP | first assert of `enter_order_counter_next_returns_pre_increment_then_advances` | H | delete
- `world_tests.rs::test_spawn_vehicle_has_voxel_marker`, `::test_spawn_infantry_has_sprite_marker` | DUP | category fallback covered by `gsi_13_10_effective_art_metadata_precedes_complete_category_fallback` (`:4101-4140`) | M | delete
- `world_tests.rs::test_spawn_multiple_entities`, `::test_empty_entities_spawns_nothing`, `::test_stable_ids_are_assigned` | E2E/MIRROR | global harness spawns its roster via `spawn_from_map` (`global_parity_harness_tests.rs:474`) and asserts by ids 2/4/6 | M | delete
- `world_tests.rs::short_game_defeats_house_with_no_buildings_even_if_ordinary_units_remain`, `::short_game_keeps_house_alive_when_base_unit_remains`, `::long_game_keeps_house_alive_when_units_remain`, `::long_game_defeats_when_no_owned_objects_remain` | DUP | `house_defeat_tests.rs::native_defeat_gate_corpus` (21 Unicorn cases incl. short/normal game, BaseUnit, zero sum) (`:106-176`); world tests differ only by using `add_tracking` | M | delete or merge to one table test

## 6. WRONG (evidence contradicts or overstated)
- `slice6_retask_tests.rs::replay_hash_stable_through_slice6` — committed `SLICE6_BASELINE_HASH` is taken **after hand-decrementing ai_counter of actors 1 and 3** ("Invert only that established correction for the historical hash pins", `:380-410`): the pin hashes a state production never produces. | WRONG (synthetic pin) | H | re-pin `live_hash`, drop inversion + all projection pins; keep native paid-Walk rows (walk_paid_step.json) and replay equality.
- `rng_routing_tests.rs::scenario_stream_matches_gamemd_random_ranged_0_4`, `::main_stream_matches_gamemd_random_ranged_0_7` — "gamemd emitted values" were hand-derived by feeding raw draws through the Rust reading of the algorithm because Unicorn "times out" (`:190-205`); contract forbids hand-calculated parity goldens. Values duplicate `rng.rs::random_ranged_power_of_two_span_matches_gamemd_draw_stream`; RandomRanged is exercised by many Unicorn oracle corpora. | WRONG(label)+DUP | H on label / M on delete | delete (or relabel "Rust regression").
- `production_shadow_tests.rs::economy_shadow_does_not_change_state_hash` — asserts "economy shadow must not perturb the state hash", but `purifier_count` is hashed (`production_authoritative_hash_includes_factory_fields` mutates it and expects a hash change, `:311-322`); passes only because the fixture has no purifier. | WRONG/vacuous | H | delete
- `edge_cell.rs` legacy ground helper tests: `test_north_edge_picks_closest_to_target_x`, `test_west_…`, `test_east_…`, `test_south_edge_picks_closest_to_target_x_within_first_10`, `test_south_edge_target_outside_candidate_window_picks_nearest_candidate`, `test_no_passable_returns_none` (6) — pin `find_passable_at_edge`, which the file itself calls "the legacy ground helper" (`edge_cell.rs:3-8`); native Approach/Overfly use `FUN_004AA440` criterion 4 with no ground passability (`:66-74`) and "ten is capacity, not a cap" (`:342-356`) vs the helper's 10-cell cap. Still used by `aircraft/paradrop_mission.rs:227` carrier exit with an invented fallback chain. | WRONG (pins invented behaviour) | M | keep until carrier exit migrates to `find_paradrop_edge_cell`, then delete with helper (~75 LOC); meanwhile 6→1 table test.
- Ignored known-failure repros in `world_tests.rs`: `repro_group_move_of_eight_vehicles_to_one_cell`, `repro_group_move_short_range_traces_every_tick`, `head_on_pair_resolves_without_deadlock` — `#[ignore]` because production fails them; docs say "Native behaviour … is not established" (`:9197-9205`, `:9372-9380`, `:9888-9904`). | WRONG (ignored, unestablished expectation) | M | move residual text to an issue/ledger; delete tests (not in default suite).
- `world_tests.rs::diag_column_reservation_trace`, `::diag_short_range_group_reservation_trace` — `#[ignore = "diagnostic"]`, zero asserts, print-only | WRONG/tooling | H | delete or move to `src/bin`/scratch.

## 7. MIRROR — "field X changes hash" and trivial restatements
Trivially true (H, delete): `smudge_integration_tests.rs::same_seed_same_smudge_state_yields_same_hash`; `world_hash.rs::empty_particle_store_hashes_consistently`; `world_hash.rs::identical_overlay_bytes_hash_equal`; `rng_routing_tests.rs::rng_views_name_all_three_streams` (getter); `logic_vector.rs::snapshot_is_order_verbatim` (`snapshot()==as_slice().to_vec()`).

Field-coverage tests (M): 56 in `world_hash.rs` —
every_active_and_stashed_drive_ship_slope_field_changes_current_hash, state_hash_includes_mutable_playfield_authority, teleport_hash_projects_presence_phase_target_and_materialization_timer, rocket_hash_projects_complete_simulation_flight_runtime, gsi_04_12_raw_occupation_hash_distinguishes_byte_plane_and_coordinate, gsi_04_09_empty_overlay_raw_data_changes_state_hash, lifecycle_authority_each_axis_changes_state_hash, lifecycle_authority_pending_queue_order_and_length_change_state_hash, every_mission_com_raw_field_changes_state_hash, every_unit/infantry/aircraft_mission_leaf_field_changes_state_hash (3), building_mission_leaf_ready_latch_changes_state_hash, suspended_attack_target_presence_variant_and_payloads_change_state_hash, object_falling_byte_changes_state_hash, current_hash_covers_each_retained_track_progress_field, factory_rally_point_changes_state_hash, drive_locomotion_state_changes_state_hash, drive_accelerates_changes_state_hash, house_difficulty_changes_state_hash, house_rof_bias_changes_state_hash_at_its_difficulty, alternate_base_center_changes_state_hash_without_changing_primary_center, gsi_04_05_base_reservation_state_changes_world_hash, gsi_04_05_base_plan_state_and_entity_facts_are_current_schema_hash_authority, gsi_04_05_house_strategy_emergency_fields_each_change_world_hash, gsi_04_05_techno_base_defense_state_changes_world_hash, gsi_04_16_waypoint_edge_is_lockstep_hash_authority, particle_state_changes_hash, state_advance_counter_changes_hash, every_raw_spark_field_changes_the_state_hash, spark_coordinate_lifetime_and_delete_state_remain_hashed, terrain_spawners_included_in_state_hash, terrain_spawner_active_fields_change_state_hash, gsi_04_15_exact_z_and_live_tube_payload_are_fully_hashed, tunnel_and_drop_pod_runtime_change_the_lockstep_hash, live_radio_contacts_change_state_hash_per_mover, retained_slot_identity_flag_and_runtime_change_hash, every_gameplay_read_animation_field_changes_state_hash, infantry_fear_and_prone_change_hash, infantry_cell_entry_blocked_changes_hash_only_when_set, pending_infantry_fire_changes_hash, hash_changes_when_smudge_placed, overlay_byte_difference_changes_state_hash, bridgehead_anchor_class_difference_changes_state_hash, bridge_endpoint_record_kind_difference_changes_state_hash, native_frame_changes_state_hash, rocking_state/velocity/none_vs_default (3), c4_state_changes_hash, homing_state_presence/yaw (2), homing_object_and_cell_targets_hash_differently, per_entry_size_mapping_changes_hash_even_when_total_matches, bridge161_hashes_each_active_and_stashed_payload_field, aircraft_dock_indices_and_reservations_affect_simulation_hash.
Plus 4 outside world_hash: `fly_height_tests.rs::fly_hash_distinguishes_targets_and_native_flags`, `gsi_04_18_tests.rs::gsi_04_18_spy_sat_latch_and_persisted_fog_are_hash_authority`, `team_script_vm_tests.rs::team_member_order_is_hashed`, `production_shadow_tests.rs::production_authoritative_hash_includes_factory_fields`.
Rationale: one-per-field "mutate → hash differs" restates the fold list. Caveat: they are what gives snapshot "hash equal after load" tests teeth; global harness final pin catches removal of *unconditional* folds on state its scenario has, but not tagged/conditional folds or absent components (rocket, tube, c4, spark, homing, prism). | M | CONSOLIDATE into ~8 table-driven coverage tests (one per owner: mission, entity/lifecycle, locomotor, house/AI, particle/spark, bridge/overlay/occupancy, projectile/homing/c4, frame/misc) — saving ~52 (or better: exhaustive destructuring in fold fns, then delete all 60).
Other MIRROR (M): `rng_routing_tests.rs` `route_scatter_rng/route_bridge_rng/route_particle_rng/route_superweapon_rng` (macro, 4; accessors are `&mut self.scenario_rng`, `mod.rs:3056-3067`) → 1 loop; `drawing_scenario_leaves_main_untouched` + `drawing_main_leaves_scenario_untouched` (independent struct fields) → 1; `advancing_scenario_only_changes_state_hash`; `logic_vector.rs::serde_roundtrip_preserves_order`.

KEEP (behavioural hash properties): `tiberium_cells_are_hashed_by_cell_not_placement_order`, `gsi_04_12_raw_occupation_hash_is_insertion_order_independent`, `rocket_hash_excludes_explicit_render_only_pitch`, `homing_state_pitch_excluded_from_hash`, `diagnostic_total_sim_ms_does_not_change_state_hash`, `native_frame_*` (2 behavioural), `despawn_contact_cleanup_hash_matches_never_contacted_state`, `gsi_04_01_hashes_dummy_bridge_bits_without_retained_projectile`, `bridge_publication_hashes_full_dummy_flags_and_retained_anchor_coordinate`, rng `advancing_main_only_…`/`advancing_mapgen_only_…` (deliberate exclusions).

## 8. CONSOLIDATE (coverage preserved)
- `world_tests.rs::test_destroyed_bridge_snaps_unit_to_ground_{when_ground_exists,over_water_below,over_overlay_blocked,over_terrain_object_blocked}` — 4 copies differing only in one cell flag/height (`:4474-4691`) | H | 1 table test, save 3
- `world_tests.rs::test_bridge_collapse_signals_pathgrid_refresh` + `::test_bridge_collapse_clears_transition_flag` — same ion-cannon collapse fixture | M | save 1
- `bridge_orchestrator.rs::cabhut_seed_canonicalization_{shifts_edge_hit_forward,keeps_middle_hit,shifts_two_cells_in_backward}` | M | save 2
- `world_orders_bridge_repair_tests.rs::c4_on_cabhut_{low_terminal_overlay_0x65,high_terminal_overlay_0xe8}_uses_overlay_first_scan` | M | save 1; `::stock_cabhut_no_overlay_without_starter_is_noop` + `::c4_on_cabhut_fallback_rejects_anchor_or_direction_flags_alone` | M | save 1
- `techno_ai/mission_handlers.rs` Guard-arm tests (9: `chrono_miner_on_guard_…`×4, `ai_war_miner_on_guard_…`×3, `human_war_miner_…`, `war_miner_on_guard_beside_its_refinery_…`, `:2089-2278`) — same fixture helpers, one arm each | M | 1-2 table tests, save ~7
- `logic_vector.rs` basic container tests (4: register_appends…, unregister_preserves…, unregister_absent…, unregister_removes_only_first…) | M | save 3
- `substrate.rs` enter_order_counter remaining 2 | M | save 1
- `world_tests.rs::repro_two_moving_vehicles_pass_through_each_other` + `::repro_two_moving_vehicles_reservation_trace` — identical setup/commands (`:9560`, `:9772`) | M | save 1
- `techno_ai.rs::gsi_07_06_close_range_does_not_qualify_a_vehicle` into `gsi_07_06_attack_cadence_halves_only_for_qualifying_types` | L | save 1

## 9. Other observations
- Silent skips in default + pre-merge suite (RA2_DIR tables, not retail INI): `jumpjet_cruise.rs::production_cruise_flies_the_native_east_cruise_frames`, `::production_cruise_turns_from_the_body_facing` return early unless retail sine/atan tables match (`:872-884`) → effectively "covered only by ignored e2e"; should be `#[ignore]` or use committed tables.
- 32 retail `#[ignore]` e2e tests (bridge concrete/wood/middle/pavement, crash dustbowl, jumpjet rocketeer, engineer Hills/Shrapnel, building missions dustbowl, gattling) run nowhere by default; none of the deletions above rely on them.
- Source-grep architecture test `world_spawn/tests.rs::techno_constructor_raw_entity_constructor_is_world_spawn_only` (reads src/ tree) — a lint; could move to `tools/` (L).
- `world_orders_bridge_repair_tests.rs` `build_sim()` tests seed `BridgeRuntimeState` directly via `test_seed_cell`; `c4_on_cabhut_low_overlay_collapses_low_bridge` notes "The retired fixture populated only a runtime cache … no Recalc inputs" (`:879-883`) — the other ~10 `build_sim` C4 tests may rest on that retired fixture shape (L, needs owner review).

## Totals (partition, default-suite unless noted)
- H removable: OBSOLETE 18 (1 ignored) · TESTONLY 18 (15 checkpoint_a) · MIGRATION 4 · WRONG 3 (2 ignored diag) · DUP 14 · MIRROR 5 → **62**
- M removable: OBSOLETE 2 · TESTONLY 2 · MIGRATION 3 · WRONG 5 (3 ignored) · DUP 11 · MIRROR ~60 net after consolidating world_hash field tests (68 tests → ~8) → **~83**
- CONSOLIDATE savings: **~21**
- Total ≈ 166 of 1,138 ≈ 14.5% (H-only ≈ 5.4%). Test-code LOC removable ≈ 1,480 (checkpoint model) + ~2,500 in deleted tests; test-only production code ≈ 830 LOC (§1c) + ~70 LOC (factory shell trace).
