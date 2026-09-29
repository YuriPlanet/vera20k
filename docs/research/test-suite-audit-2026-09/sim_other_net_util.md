# Test audit: sim/{bridge_state,mission,docking,vision,superweapon,aircraft,rocking,radio,crates,map,passenger,team_script_vm,tiberium,transport_unload,projectile,substrate}, crate-root files, net/, util/

Read-only static audit at `e83df3de` (2026-09-28). No cargo runs.
Scanned: **1,086 `#[test]`** (sim partition 794, crate-root 83, net 30, util 176; 13 of them `#[ignore]`).

Format: `path::test` | category | evidence | confidence | action.

E2E coverage caveat: `global_parity_harness_tests.rs` exercises movement/combat/harvester only
(file header lines 1-21); it does not reach bridges, vision gap/spy-sat, superweapons, crates,
docking, passengers or net queue code, so it justifies almost nothing here.

---

## TESTONLY — code gated `#[cfg(test)]` or with no non-test caller

### High confidence (70)

- `src/util/fixed_math.rs` — 21 tests on `#[cfg(test)]` helpers never used in production:
  `test_dt_from_tick_ms`, `test_dt_zero` (dt_from_tick_ms :116), `test_fixed_sqrt_perfect_squares`,
  `test_fixed_sqrt_non_perfect`, `test_fixed_sqrt_small_values`, `test_fixed_sqrt_zero_and_negative` (fixed_sqrt :176),
  `test_fixed_clamp` (:134), `test_fixed_lerp` (:149), `test_fixed_max_min` (:156/:163), `test_sim_from_f64` (:100),
  `test_fixed_distance_sq` (:203, also calls fixed_sqrt), `test_fixed_abs` (:127),
  `test_ra2_speed_{zero_is_immobile,harvester,medium_tank,infantry,fast_unit,max,capped_above_100,one}` (8; ra2_speed_to_cells_per_second :345),
  `test_lepton_speed_is_256x_cell_speed` (ratio against the cfg(test) cells/s fn).
  grep: zero callers outside fixed_math.rs for all except `ra2_speed_to_cells_per_second`, which one test at `src/sim/spawn_manager_tests.rs:1286` uses (switch it to `ra2_speed_to_leptons_per_second`/256). | H | delete tests + the ~11 cfg(test) fns (~110 LOC).
- `src/util/ini_writer.rs` — 12 single-key tests (`replaces_value_preserving_other_keys_and_sections` … `appended_key_uses_crlf_even_in_lf_file`, :294-399) test `set_ini_value` (:28 `#[cfg(test)]`), a **separate second implementation**. Production calls only `set_ini_values` (options_profile.rs:14, skirmish_session.rs:83, rmg/options.rs:12, skirmish_persistence.rs:136). | H | delete `set_ini_value` (~87 LOC); move the 4 unique cases (CRLF kept on replace, comment lines, unterminated header, case-insensitive match) into one table test on `set_ini_values` → net −11.
- `src/util/lepton.rs::subcell_3_offsets_bottom_left`, `subcell_4_offsets_bottom_right`, `subcell_center_has_zero_offset` — `lepton_sub_to_screen_offset` (:245) and `SCREEN_{X,Y}_PER_LEPTON` (:40,:46) are cfg(test). | H | delete (3) + ~25 LOC.
- `src/util/direction_tables/dragon.rs::dragon_frame_table_equals_gamemd_dump`, `dragon_frame_index_formula` — `DRAGON_FRAME_TABLE`/`dragon_frame_index` have no consumer (only re-export at direction_tables/mod.rs:27; module doc says the cutover "is a later slice"). Second test also restates the formula. | H | delete with the table (~25 LOC), or keep the table test only when a consumer lands.
- `src/util/direction_tables/quantize.rs::opposite_dir_is_plus_or_minus_4`, `facing8_to_16_high_byte` — `opposite_dir`/`facing8_to_16` have zero callers (bridge_orchestrator.rs:791 `opposite_dir` is a local variable); both tests restate the one-line formula. | H | delete tests + fns.
- `src/sim/bridge_state/scan_tests.rs` (3: `cells_in_5x5_scan_*`) — `cells_in_5x5_scan` is `#[cfg(test)]` (mod.rs:1580) with no caller. | H | delete file + fn.
- `src/sim/bridge_state/tests.rs::render_state_byte_strips_healthy_variant` — `render_state_byte` is cfg(test) (mod.rs:135). | H | delete.
- `src/sim/bridge_state/tests.rs::test_seed_cell_grows_grid_to_fit`, `cell_mut_writes_visible_through_cell_read` — test the cfg(test) seeding helper `test_seed_cell` (mod.rs:922) and a trivial `cell_mut` setter/getter. | H | delete.
- `src/sim/vision/vision_tests.rs::test_merge_revealed_preserves_bits`, `test_merge_revealed_different_dimensions` — `merge_revealed_from` cfg(test) (vision/mod.rs:517); production reuses grids in place. | H | delete + fn.
- `src/sim/vision/vision_tests.rs::test_reset_explored_for_owner` — `reset_explored_for_owner` cfg(test) (mod.rs:1245). | H | delete test (fn still used as fixture by 2 shroud tests).
- `src/sim/vision/vision_tests.rs::test_shroud_edge_mask_{interior_cell,with_revealed_neighbors,at_grid_edge,ne_uses_correct_neighbor}` (4) — 4-bit `shroud_edge_mask` is cfg(test) (mod.rs:1308); production uses the 8-bit SHROUD.SHP mask right below it. Obsolete render helper. | H | delete tests + fn.
- `src/sim/mission/timer.rs::signed_dispatch_remaining_uses_wrapping_subtraction` — only asserts `remaining_if_pending` (cfg(test), timer.rs:70). | H | delete (other signed_dispatch tests keep `due()` coverage; drop their `remaining_if_pending` asserts and the fn).
- `src/sim/mission/concrete_effects.rs::production_concrete_effects_never_claim_partial_setter_coverage` — `UnavailableConcreteMissionEffects` is cfg(test) (concrete_effects.rs:68-75) although its doc calls it the "production boundary"; the name of the test is therefore false. | H | delete; fix stale doc.
- `src/sim/docking/building_dock.rs::depot_repair_response_maps_to_radio_codes` — `RepairResponse::radio_response` is cfg(test) (:140); opcode asserts duplicate `radio/mod.rs::response_codes_match_wire_opcodes`. | H | delete + fn.
- `src/sim/docking/bunker_link.rs::release_clear_plays_down_sound_but_does_not_reposition`, `release_clear_emits_one_walls_down_anim_event` — `release_clear` is cfg(test) and documented "legacy clear-only adapter has not been audited as a native teardown" (:296-301). | H | delete tests + fn (~18 LOC).
- `src/sim/aircraft/runtime_contract.rs::firing_branch_skips_base_update`, `normal_branch_preserves_tail_order` — `aircraft_update_steps` is cfg(test) (:25); `AircraftUpdateStep` has no production reader. The test checks a list the same fn builds. | H | delete tests, fn and enum (~50 LOC).
- `src/sim/map/bridge_occupancy_shadow.rs` (6: `list_layer_selected_by_on_bridge_byte`, `add_order_…`, `remove_walks_only_the_selected_layer`, `cross_removes_…`, `cross_step_off_deck_…`, `remove_cleans_up_fully_empty_cell`) — module header :13-19 "SHADOW ONLY — NOT authoritative … NOT wired into the tick, NOT serialized, NOT part of the state hash"; no reference outside `map/mod.rs:20`. Authoritative store is `sim::occupancy`. | H (also OBSOLETE) | delete module (269 LOC).
- `src/sim/map/bridge_topology.rs::is_bridge_tileset_distinct_from_structural_flag`, `is_wood_bridge_tileset_distinct_from_concrete_and_structural`, `is_low_bridge_requires_landtype10_and_tube_in_range`, `occupancy_bit_layer_inclusive_full_deck_and_clear_asymmetry`, `clear_occupation_no_structural_flag_required` — predicates are cfg(test) (bridge_topology.rs ~:190,:211,:225,:279; "SHADOW … only consumed by shadow tests" :273-275). Last two also duplicate each other. | H | delete tests + cfg(test) predicates (~70 LOC).

### Medium confidence (47)

- `src/net/lockstep.rs` — **all 28 tests**. `SynchronizedCommandQueue`, `FrameInfo`, `FrameInfoCompareGate`, `MultiplayerChecksumHistory`, `NetworkSendPolicy`, `CommandDispatchHouse`, `LockstepScheduler` have **zero references outside `src/net/`**. Production only calls `SynchronizedCommand::opaque(..).decode_for_simulation` (app/input/commands.rs:810,814,882) and queues through `sim.queue_command` (commands.rs:819) — a separate owner. Unintegrated native port (~600 LOC). | M | owner decision: integrate, or delete queue + tests (keep `SynchronizedCommand` + the 2 convergence tests). Minimum: merge `opcode_0c…`/`opcode_22…` (−1).
- `src/sim/vision/vision_tests.rs` gap tests that go through `apply_gap_generators` → `apply_gap_generator_sources_with_spy_sat` (vision/mod.rs:2194-2280, all cfg(test)). That fn **reimplements** the source-diff reconciliation; production uses `publish_gap_generator_event`/`materialize_gap_generator_sources` (:2306,:2365). 7 tests: `test_gap_generator_preserves_admitted_enemy_sight`, `gsi_04_18_hostile_gap_erases_map_knowledge_until_current_sight_returns`, `gsi_04_18_spy_sat_repeat_is_not_a_new_map_reveal_event`, `test_gap_generator_sets_gap_covered_flag`, `gap_radius_uses_strict_radius_plus_one_squared`, `gap_marks_friendly_viewer_as_fog_not_covered`, `gap_coverage_clears_when_no_generator_present` (2 more are DUP below). | M | retarget the footprint/friendly checks at the production publish path, then delete the test-only reconciliation (~75 LOC). The `shroud_current_sight_*` native-sequence tests use the same adapter and need the same retarget.
- `src/sim/vision/vision_tests.rs::test_owner_visibility_basic` — `OwnerVisibility::mark_visible` is cfg(test) (mod.rs:339). | M | delete.
- `src/sim/mission/authority.rs` — 6 tests on the cfg(test) `mission_override_exact_with_effects` (:1148), a **second Override_Mission transaction** (archive NavCom/TarCom, override_base, apply setters). Production overrides go through `override_entity_to_attack_target` in `override_mission_on_damage_response` / `mission_override_movement_blocker` (:892-1000). Tests: `override_target_unavailable_is_fieldwise_noop`, `override_destination_unavailable_is_fieldwise_noop`, `foot_override_provider_order_includes_same_identity_target_dispatch`, `override_transaction_traces_each_concrete_category_and_building_never_sets_nav`, `guarded_override_base_still_archives_and_runs_concrete_setters`, `blocked_aircraft_override_has_empty_trace_and_byte_identical_state`. | M | converge on one Override owner; if production stays, delete these 6 + ~90 LOC (Unavailable effects + test transaction).
- `src/sim/rocking/self_destruct.rs` (5 tests) — production passes `NoopSelfDestruct` (sim/world/mod.rs:6102), so detection has no observable effect; tests pin a stub hook. | M | collapse to 1 or delete until the C4 hook is ported.

## OBSOLETE

- `src/sim/crates/state.rs::crate_authority_every_raw_word_changes_v114_hash_only` — asserts the test-only historical projection `state_hash_without_crate_authority_v114` (world_hash.rs:639-640); "changes current hash" is already implied by snapshot/hash tests. | H | delete.
- `src/sim/vision/vision_tests.rs::shroud_current_sight_provenance_changes_future_conceal_and_hash` — lines 1959/1974 assert `state_hash_without_sustained_gap_sight_v142` (historical projection). Behavioral part is valuable. | H | trim the v142 asserts (no test deletion).
- (bridge_occupancy_shadow — counted under TESTONLY.)

## MIRROR — restates the implementation

### High (25)
- `src/util/fixed_math.rs::test_constants`, `test_new_constants` — constant == same literal; `SIM_TWO`/`SIM_1_5` have zero users outside the file. | H | delete tests + the 2 dead consts.
- `src/util/fixed_math.rs::test_facing_u16_consistent_with_u8` — `facing8_from_delta` is defined as `facing16 >> 8` (native_angle.rs:124). | H | delete.
- `src/util/lepton.rs::integer_and_fixed_leptons_per_cell_agree`, `leptons_per_cell_is_256` — const == const. | H | delete.
- `src/util/read_helpers.rs` (7: `test_read_u16_le` … `test_read_f32_le_negative`) — each fn is one `from_le_bytes` call (:20-65); tests assert std. Covered by every asset parser test (mix/shp/vxl/…). | H | delete all 7.
- `src/util/facing_table.rs::test_quarter_wave_checkpoints` — `QUARTER_SIN[n] == literal` from the same table. | H | delete.
- `src/sim/bridge_state/tests.rs::direction_offsets_match_compass`, `direction_opposite_is_idempotent`, `direction_opposite_pairs` — restate the `match` in `Direction::offset/opposite` (mod.rs:199-227). (The enum itself duplicates `util::direction` — refactor lead.) | H | delete.
- `src/sim/bridge_state/tests.rs::dispatch_path_is_state_machine` — restates a 4-arm match. | H | delete.
- `src/sim/bridge_state/tests.rs::bridge_state_getters_return_construction_values` — constructor arg → getter. | H | delete.
- `src/sim/mission/timer.rs::reversal_arithmetic_matches_gate_reverse` — the test performs the reversal arithmetic itself (`t.duration = total.saturating_sub(...)`); it tests `saturating_sub`, not production. | H | delete.
- `src/sim/docking/aircraft_dock.rs::airfield_docks_pad_assignment_is_deterministic` — two identical runs of a pure BTreeMap struct. `aircraft_ammo_new` — constructor fields. | H | delete (2).
- `src/sim/aircraft/paradrop_mission.rs::test_chebyshev_threshold_arithmetic` (`assert!(4*256 <= 1024)`), `test_opposite_edge_indices` (`(0u8+2)%4 == 2`) — no production code called at all. | H | delete (2).
- `src/sim/radio/mod.rs::payload_defaults_to_no_cell` — derive(Default). | H | delete.
- `src/sim/passenger.rs::test_cargo_new` — constructor fields. | H | delete.

### Medium (23)
- `src/util/fixed_math.rs::test_sim_conversions` — `fixed` crate conversions; uses cfg(test) `sim_from_i32`. | M
- `src/util/lepton.rs::subcell_lookup_returns_correct_positions` — restates the 5-arm match. | M
- `src/util/facing_table.rs::test_table_symmetry` | M
- `src/sim/bridge_state/tests.rs::anchor_span_iter_cells_skips_none` — Option flatten. | M
- `src/sim/mission/timer.rs::arm_and_reset_alias_defer` — alias methods. | M
- `src/sim/mission/leaf.rs::mission_leaf_accessors_do_not_cross_categories`, `mission_leaf_narrow_writers_preserve_other_raw_fields` (setter→getter, uses `*_raw_for_test` cfg(test) ctors), `mission_leaf_serde_round_trip_preserves_every_raw_field` (derive). | M (3)
- `src/sim/mission/state.rs::mission_state_serde_preserves_unknown_raw_selectors` — derive(Serialize) of raw i32 fields. | M
- `src/sim/docking/building_dock.rs::cell_distance_same`, `cell_distance_diagonal` — 3-line helper; note doc says "Manhattan" but code is Chebyshev (:298-302). | M (2) — fix doc.
- `src/sim/radio/mod.rs::message_codes_match_wire_opcodes`, `response_codes_match_wire_opcodes` — hand-typed discriminants re-asserted. `src/sim/radio/contacts.rs::default_is_one_empty_slot`. | M (3)
- `src/skirmish_modes.rs::parses_stock_mpmodesmd_roster`, `stock_mpmodes_do_not_include_siege_without_roster_row`, `team_game_has_must_ally`, `free_for_all_disables_allies`, `battle_and_free_for_all_allow_random_maps` — assert the cfg(test) fixture `stock_skirmish_modes` whose override flags are hand-authored (:197-211); `stock_contract_modes_match_selected_retail_roster_and_overrides` (:349) proves fixture == retail through the production reader (pre-merge with `VERA20K_REQUIRE_RETAIL_INI=1`). | M (5)
- `src/skirmish_launch.rs::yuri_country_uses_third_side`, `launch_mode_carries_selected_mpmode_data` — field copy / constant. | M (2)
- `src/sim/map/bridge_topology.rs::bridge_topology_predicates_match_pathcell` (restates `flags & CONST != 0`), `gsi_04_03b_deck_height_consts_resolve_to_verified_values` (const == literal). | M (2)

## DUP — same assertion already made elsewhere

### High (15)
- `src/util/fixed_math.rs::test_facing_cardinals`, `test_facing_zero_delta`, `test_facing_u16_cardinals`, `test_facing_u16_zero_delta` — `facing_from_delta_int[_u16]` are pure delegates (:287-297); identical values pinned by `direction_tables/native_angle.rs::cardinals_and_zero_use_the_native_65534_scale` (:154). | H (4)
- `src/util/fixed_math.rs::dir_to_cell_delta_quantized_directions`, `dir_to_cell_delta_rounds_to_nearest` — `dir_to_cell_delta` delegates to `direction::delta_from_facing` (:316); covered by `util/direction.rs::facing_quantization_matches_drive_locomotor_formula`. | H (2)
- `src/util/direction_tables/lepton.rs::lepton_is_cell_times_256` — both tables already pinned literally by `lepton_delta_table_equals_gamemd_dump` + `cell.rs::cell_delta_table_equals_gamemd_dump`. | H
- `src/util/flh_transform.rs::adjust_for_z_matches_retail_flh_fixtures` — `adjust_for_z_leptons` is a pass-through (:12-13); all 5 values are in `native_x87.rs::adjust_for_z_standard_matches_retail_fixtures_and_signed_edges`. | H
- `src/match_bootstrap.rs::accepted_match_tick_is_gated_without_receipt` — both asserts appear in `rust_l0_receipt_captures_each_rng_stream_exactly` and `accepted_tick_rejects_each_mismatched_receipt_field_and_nonzero_clock`. | H
- `src/sim/bridge_state/tests.rs::overlay_byte_round_trips_via_snapshot` — `bridge_runtime_state_snapshot_round_trip` already compares whole cells (incl. overlay_byte). `bridgehead_is_bridge_walkable_returns_true` — same asserts inside `bridgehead_survives_body_cell_collapse`. | H (2)
- `src/sim/docking/aircraft_dock.rs::airfield_docks_basic_reserve` — subsumed by `airfield_docks_four_pad_allocation_order` / `airfield_release_does_not_pin_freed_pad_index`. | H
- `src/sim/vision/vision_tests.rs::test_gap_covered_not_set_for_friendly`, `test_gap_generator_does_not_suppress_friendly` — same friendly-gap outcome as `gap_marks_friendly_viewer_as_fog_not_covered` (also via the test-only reconciliation). | H (2)
- `src/sim/projectile.rs::store_round_trips_through_snapshot_serialization` — `simulation_hash_and_save_preserve_pending_projectile` (next test) does full GameSnapshot save/load + hash equality. | H

### Medium (20)
- `src/util/fixed_math.rs::test_facing_diagonals` (loose ±1 of values `test_facing_u16_diagonals` pins exactly), `test_facing_quadrants`, `dir_to_cell_delta_round_trips_through_facing_from_delta` (±4 tolerance). | M (3)
- `src/util/lepton.rs::matching_lepton_projections_agree` ≈ `gsi_13_02_util_and_terrain_share_the_exact_absolute_projector`. | M
- `src/util/direction_tables/cell.rs::cell_delta_table_equals_gamemd_dump` ≈ `util/direction.rs::direction_ids_match_gamemd_compass_table` (`CELL_DELTAS` *is* `DIRECTION_DELTAS`, cell.rs:12). | M
- `src/sim/bridge_state/tests.rs::bridge_state_destroyable_flag_disabled`, `indestructible_bridge_outer_gate_is_clear` — both assert `!is_destroyable()` after `from_resolved_terrain(.., false, ..)`. | M (2)
- `src/sim/vision/vision_tests.rs::test_elevation_sight_bonus_z0_gives_no_bonus`, `test_elevation_sight_bonus_disabled_when_zero` — covered by `elevation_grants_no_sight_bonus_at_any_reachable_terrain_level` + `test_reveal_center_z_shift`; the latter's comment "would give large bonus if enabled" is wrong (16·104=1664 < 2000 → 0 steps). `test_dead_owner_keeps_revealed` ≈ `test_in_place_preserves_revealed`. | M (3)
- `src/sim/docking/aircraft_dock.rs::airfield_docks_four_pad_allocation_order`, `airfield_docks_release_preserves_other_reservations`, `airfield_docks_single_pad_helipad`, `airfield_docks_cleanup_dead` — reserve/release/no-FIFO semantics repeated across 6 tests; keep `airfield_release_does_not_pin_freed_pad_index`, `airfield_full_waiter_admitted_by_probe_not_fifo`, `dead_airfield_cleanup_removes_live_aircraft_reverse_entries`, `airfield_docks_idempotent_reserve`. | M (4)
- `src/sim/superweapon/invulnerability.rs::timer_remains_active_across_frame_wrap`, `zero_duration_never_active`, `negative_duration_never_active` — `is_invulnerable` is `CdTimer::remaining > 0` (:38-40); `sim/timer.rs` tests `signed_frame_subtraction_wraps`, `a_running_timer_with_no_or_negative_time_has_expired` cover it. | M (3)
- `src/sim/superweapon/iron_curtain.rs::ic_kills_infantry_in_grid` ⊂ `ic_infantry_kill_publishes_unit_lost_per_human_death`; `ic_protects_vehicles_in_grid` ⊂ `ic_affects_both_diagonals_and_center`. | M (2)
- `src/sim/passenger.rs::test_can_accept_when_full` ⊂ `test_board_and_count`. | M

## E2E — covered by a named, non-ignored stronger test
- `src/sim/rocking/rocking_tests.rs::convergence_decays_to_zero_over_time` — `integration_impulse_decays_to_neutral_over_60_ticks` drives the same stationary decay through `advance_tick`. | M

## MIGRATION / vacuous
- `src/util/flh_transform.rs::gsi_08_04_transform_rejects_an_out_of_range_step_rather_than_wrapping` — name says "rejects out-of-range"; the only assert is that an **in-range** (0,0) call returns `Some` (:201-219). | H — delete or write the real rejection case.
- `src/match_bootstrap.rs::correlation_exhaustion_fails_without_reading_seed` — clock never reaches the code under test, so `reads == 0` is trivially true; then the test reads the clock itself (:707-719). | H
- `src/sim/bridge_state/tests.rs::overlay_byte_populated_at_map_load` — no assertion ("field is reachable; type is u8", :875-884). | H
- `src/sim/map/bridge_topology.rs::service_gate_handle_is_bit_identical_to_pathfinding_gate` — compares `check_bridge_traversal` with itself (`use … check_bridge_traversal as bridge_traversal_gate`, :335). | H
- `src/architecture_guards.rs::no_root_app_modules_remain` — F12 migration finished; any `app_*` root import is already caught by `production_dependency_edges_match_frozen_ledger_inventory` (pseudo-root matches retired root app_* modules, header :9-10). | M
- Keep: `production_dependency_edges…`, `master_frame_adapters…`, `raw_ini_walks…`, `sim_names_no_upper_layer_root_even_in_tests` guard live CLAUDE.md invariants. `app_state_contains_only_named_owners` is a live ratchet (keep, L). Dead machinery: `FROZEN_EXCEPTIONS` is empty (:35), so its stale/ratchet branch is moot.

## CONSOLIDATE — table-driven merges (savings = N−1), ~70
- `src/match_bootstrap.rs` classify tests (12: `explicit_fixed_battle_session_is_accepted` … `team_none_is_concrete_and_accepted`, :495-615; `unresolved_choices_are_unverified` and `team_none_…` are DUPs of the others) → 1 table: −11. `rust_l0_rejects_nonzero_{tick,sim_time,binary_frame}` → 1: −2. | M/H
- `src/sim/bridge_state/tests.rs`: `damage_state_to_byte_{ns,ew}_axis` + `damage_state_from_byte_{ns,ew}_range` + round_trip → 1 table (−3); `body_collapse_from_{damaged,partial_a,partial_b}` already call one helper (−2); bridgehead-advance no-change cases `odd_h_ns`, `h_gt_4_ew`, `non_bridgehead_role`, `anchor_walk_failure`, `off_map` (−3); body-driver no-change `destroyed_anchor`, `bridgehead_cell`, `out_of_bounds` (−2). Also a ~60-line `ResolvedTerrainCell` literal is copy-pasted in fixtures (:1700-1760, :1880-1940). | M
- `src/sim/docking/building_dock.rs`: `dock_cell_for_{3x3,4x3,2x2,1x1}_foundation` (−3); 6 `depot_repair_*` (hand-computed cost=1000/15% fixture) (−5). | M
- `src/sim/docking/bunker_link.rs::release_normal_emits_one_walls_down_anim_event`, `release_sell_destroy_emits_no_anim_event` → fold into the release_normal / release_sell tests (−2). | M
- `src/sim/docking/pad_geometry.rs` 1×1 cases (`zero_offset`, `positive_offset`, `negative_offset_is_clamped`, `z_coord_does_not_affect_cell`) (−3). | L/M
- `src/sim/mission/timer.rs` MissionTimer basics (`unarmed_is_always_due`, `inclusive_due_boundary`, `defer_zero_is_due_next_check`, `clear_makes_due_again`, `elapsed_and_remaining`, `wraparound_delta_is_correct`) (−4); `src/sim/mission/state.rs::unknown_mission_id_round_trips…` + `high_bit_and_enum_idle…` (−1). | M
- `src/sim/superweapon/invulnerability.rs` remaining 3 (`none/fresh/expired`) → 1 (−2). | M
- `src/sim/aircraft/drop_payload.rs::test_v_pattern_*` (6, loose tolerances on a non-native sine table) → 1–2 (−4). | M
- `src/sim/rocking/rocking_tests.rs::ship_rocking_*` (5) → 1 (−4). | M
- `src/util/fixed_math.rs::test_int_distance_to_sim_*` (4, −3) and remaining `test_lepton_speed_*` (3, −2). | M
- `src/util/lepton.rs::cell_delta_to_lepton_dir_*` (3, −2). | M
- `src/util/native_trig.rs::gsi_08_04_{table_covers…,zero_step…,table_is_not_odd_symmetric,steps_32_apart_disagree}` (−3). | M
- `src/util/base64.rs` 7 RFC vectors → 1 (−6). | M
- `src/util/facing_table.rs::test_facing_{north,east,south,west,zero_speed}` (−4). | M
- L-only: `architecture_guards` two scanner self-tests (−1); `superweapon/psychic_reveal.rs::pr_reveals…`/`pr_does_not_reveal…` (−1).

## WRONG / questionable
- `src/sim/aircraft/idle_mode.rs` (4 tests) — module doc :3-5 "VERA's own tree, not a port of `AircraftClass::Enter_Idle_Mode @ 0x004176F0`" with a recorded DRIFT (:44-60). The tests pin invented behavior, not native. | L — keep only until ported; do not count as parity.
- No test in this partition presents hand-calculated values as native parity. Hand-calculated (not labeled parity): rocking unit tests, `depot_repair_*`, `pad_geometry`, `drop_payload` v-pattern (uses `facing_table`'s offline `round(sin·65536)` table, not the retail trig table in `util/native_trig.rs` — a possible non-native rounding source for `v_offset`).
- Stale or wrong text in otherwise-kept code: `building_dock.rs:298` doc "Manhattan" vs Chebyshev code; `concrete_effects.rs:66` "production boundary" on a cfg(test) type; `vision_tests.rs` comment in `test_elevation_sight_bonus_disabled_when_zero`.

## Partial trims (no test count; production code removable)
- `src/sim/projectile.rs::closed_collision_predicate_vectors_match_yr`, `burst_shrapnel_and_special_priority_match_closed_vectors` — `projectile_cell_obstacle` (:387) and `projectile_burst_plan` (:638) are cfg(test) and their pub types `ProjectileCellObstacle`/`ProjectileBurstPlan` have no production reader. Drop those asserts and ~60 LOC; keep the production-function asserts.
- `src/sim/mission/readiness.rs:890` asserts cfg(test) `base_ready_to_commence` (:128).
- `src/util/direction_tables/quantize.rs::muzzle_anim_8way_plus1_rotation` loop restates the formula (keep the first assert).

## Covered only by `#[ignore]` e2e (not in default suite; not a basis for deletion)
- `src/headless_scenario.rs`: `retail_headless_funnel_construction_is_deterministic_and_populated`, `retail_authored_terrain_anims_consume_the_final_bound_art_owner`, `retail_skirmish_computer_houses_build_their_teams`, `retail_yuri_teams_attack` (the last two are print-oriented diagnostics; doc :93 records an unexplained team disband).
- `src/sim/team_script_vm.rs::gsi_04_05_retail_aimd_installs_all_resolved_definitions_without_teams`.
- `src/sim/projectile/homing.rs` (3) and `src/sim/projectile/launch.rs` (5) "requires verified gamemd.exe math tables" oracle tests.

## Keep (sampled; native goldens, RNG order, regressions)
mission/{authority (non-override), readiness, verb}, bridge_state/{damage_dispatch, gap_restamp, occupants, ordinary_*, publication, ramp_repair, record_native, rim}_tests, crates.rs/crates/*, superweapon/{cell_receiver, lightning_storm, paradrop, mod}, transport_unload, tiberium, team_script_vm oracles, projectile native vectors, aircraft attack/release/approach oracles, rocking crash-spin oracles, native_x87, native_trig oracle tests, skirmish_cooperative, skirmish_persistence, rng_continuation, legacy_crt_rng.

## Refactor leads noticed (not test deletions)
- `sim::bridge_state::Direction` duplicates `util::direction::Ra2Direction` (8-way, offsets, opposite).
- Wrappers that CLAUDE.md forbids: `fixed_math::{facing_from_delta_int, facing_from_delta_int_u16, dir_to_cell_delta}`, `flh_transform::adjust_for_z_leptons`, `direction_tables::quantize::dir_from_facing8`.
- `rocking/impulse.rs::sqrt_approx` (SimFixed) versus the native `native_x87::sqrt_approx_f32`.
- Two Override_Mission transactions (mission/authority.rs); two gap-reconciliation paths (vision/mod.rs); two lockstep queues (net/lockstep.rs vs `sim.queue_command`).

## Tally
| Bucket | H | M |
|---|---|---|
| TESTONLY | 70 | 47 |
| OBSOLETE | 1 | – |
| MIRROR | 25 | 23 |
| DUP | 15 | 20 |
| E2E | – | 1 |
| MIGRATION/vacuous | 4 | 1 |
| **Removable** | **115** | **92** |
| CONSOLIDATE savings | ~70 (mostly M) | |
| WRONG (L, keep) | 4 | |

Removable in total: about 277 of 1,086 (25%): 115 H, 92 M and about 70 from consolidation. Counting only H and consolidation gives about 185 (17%).
Test-only production code that becomes deletable: about 800 LOC at H (fixed_math ~115, ini_writer ~87, bridge_occupancy_shadow ~155, bridge_topology ~70, vision ~50, lepton/dragon/quantize ~60, aircraft_update_steps ~50, projectile ~60, bridge_state ~35, bunker/building_dock ~30, mission ~50). About 700 LOC more at M: the net lockstep queue (~600), the test-only Override transaction (~90) and the gap reconciliation (~75).
