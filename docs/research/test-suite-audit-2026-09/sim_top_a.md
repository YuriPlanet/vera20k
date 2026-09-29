# Test audit: src/sim/*.rs (top level, names <= "intern.rs")

Scope: 49 files, 689 `#[test]` (2 `#[ignore]`: crew_survival_tests.rs retail_dustbowl_*).
Method: listed every test and read the bodies in the suspicious clusters and in samples of each large file. Callers were checked with grep outside the test code. No cargo runs.

Baseline notes:
- The global parity harness (src/sim/world/global_parity_harness_tests.rs header, lines 1-22) covers tanks, infantry, a harvester, a refinery and a war factory. It does not reach bridges, crates, deploy, cloak, capture, bombs or crew survival. So no test here can be deleted on the grounds that the harness catches its regression. The E2E category is therefore empty.
- Oracle-backed tests are kept and not flagged. Examples: nearby_raw_*, native_*_corpus, *_matches_the_original, original_*_rows, map_* in cell_rect_native_tests, set_difficulty_matches_the_original, survivor_direction_matches_the_original.
- 106 tests in this partition are named after work items (gsi_/ggi_). Almost all of them assert real native behaviour. Renaming them is optional. They are not deletion candidates unless listed below.

## TESTONLY (production never runs the code under test)

- src/sim/bridge_specs.rs::low_bridge_damage_step_{ignores_non_bridge_overlay,applies_rng_gate,atom_damage_bypasses_gate,maps_wood_family,maps_concrete_family_and_no_transition} (5) | TESTONLY | `low_bridge_overlay_damage_step_ra2` is `#[cfg(test)]` (bridge_specs.rs:99-100). It is an **RA2** port, not YR (module header lines 1-10 say "not yet fully wired") | H | delete, together with the test-only code below
- src/sim/bridge_specs.rs::low_bridge_selector_{rejects_non_bridge_overlay,uses_exact_anchor_policy} (2) | TESTONLY | `low_bridge_connected_section_selector_yr` is cfg(test) (bridge_specs.rs:157-158) | H | delete
- src/sim/bridge_specs.rs::zone_connection_{record_decodes_layout,match_uses_axis_aligned_segment_proximity,match_respects_skip_flag} (3) | TESTONLY | `zone_connection_matches_cell` is cfg(test) (:221). `decode_zone_connection_record` (:208) and `ZoneConnectionRecord` have no callers outside this file | H | delete
- src/sim/bridge_specs.rs::bridge_zone_policy_{turns_off_when_on_bridge_false,turns_off_when_bridge_bit_clear,matches_ra2_and_yr_fallback_split} (3) | TESTONLY | `get_cell_zone_id_bridge_policy_decision` is cfg(test) (:242). `BridgeZoneIdPolicyDecision` has no external use | H | delete
  - Test-only code that can go with these 13 tests: bridge_specs.rs:16-19 and 21-97 (types BridgeOverlayTriple, LowBridge*, ZoneConnectionRecord, BridgeZoneIdPolicy*), plus 99-340 (the 4 functions, decode, in_range/pattern/classify helpers, read_*_le). About 330 LOC. The module doc (lines 1-10) also needs rewriting.
- src/sim/anim_class.rs::long_tail_contract_tests::yr_long_tail_vectors | TESTONLY | three of its four asserts call cfg(test) `directional_tumble_frame`, `settled_bounce_frame` and `bounce_spawn_count` (anim_class.rs:177-198), which have no callers. The translucency assert is already covered by render/sprite_atlas_tests.rs:598 | H | delete, with anim_class.rs:177-198
- src/sim/anim_class.rs::tests::multiplayer_feedback_uses_sync_exempt_registry_without_global_id_or_logic_membership | TESTONLY | the only producer, `spawn_multiplayer_feedback_anim_at_world`, is cfg(test) (:1069-1148, comment "producer is not wired yet"). The production registry walk (`for_each_multiplayer_feedback_anim` :1179, lifecycle.rs:4364) therefore always iterates an empty store | H | delete the test, the spawner and the constants at :171-175. Treat the dormant registry fields (substrate.rs:150-187) as a follow-up
- src/sim/animation_tests.rs::test_sequence_is_prone_helper | TESTONLY | `sequence_is_prone` is cfg(test) and documented as a "temporary stance proxy" (animation.rs:128-140) | H | delete with the function
- src/sim/animation_tests.rs::test_tick_dying_entity_{skips_transitions,returns_finished_id,returns_id_on_finishing_visit} (3) | TESTONLY | they go through the cfg(test) wrapper `tick_animations` (animation.rs:535-552), which passes `tick_dying=true`. Production only calls `tick_animations_impl(..., false, ...)` (:584-593), and handles dying objects through `tick_dying_animation` (:600). So the dying branch in the impl (:351-406) is unreachable in production | H | delete; pass `false` in the wrapper and remove the `tick_dying` branch (~25 LOC)
- src/sim/components.rs::rocking_{default_is_neutral,active_angle_is_not_neutral,within_deadband_is_neutral,ship_rocking_is_not_neutral} (4) | TESTONLY | `RockingState::is_neutral` is cfg(test) (components.rs:951-957) | H | delete with is_neutral. rocking/rocking_tests.rs:555 also uses it (outside this partition)
- src/sim/house_strategy_tests.rs::native_entry_writers_change_only_their_owned_fields | TESTONLY | the writer it tests, `set_state_four`, is cfg(test) (house_state.rs:172-176). No production path writes mode 4 (Trigger action 9 and Team op 30 are not ported). Only `note_building_attack` is real, and other strategy tests cover it | M | delete; drop set_state_four and its use in snapshot.rs:4579
- src/sim/game_entity.rs::mission_shadow_tests::{derived_mission_idle_when_no_machine_active,passenger_derive_unchanged_placeholder,derived_mission_tracks_attack_target} (3) | TESTONLY | `derived_mission()` is cfg(test) (game_entity.rs:1521-1524). Production goes through `passive_acquire_mission` -> `derived_mission_with(!passively_acquired_target)`. The first two pin "legacy None placeholder" readings that their own comments call untraced | M | delete, or rewrite through passive_acquire_mission

## OBSOLETE

- src/sim/credit_income.rs::credit_income_state_affects_only_current_v135_hash_schema | OBSOLETE | uses the historical probe `state_hash_without_credit_income_v135` (cfg(test), world/world_hash.rs:659-666). hash_schema.rs:6 admits such probes "cannot reproduce old hash streams" | H | delete with the probe (and its entry at hash_schema.rs:419)
- src/sim/game_entity.rs::tests::gsi_05_10_pending_building_fire_serde_default_is_none | OBSOLETE | JSON missing-field default. Production snapshots are bincode (snapshot.rs:1000/1076) and never read GameEntity from JSON. ReplayLog JSON (replay.rs:742) carries only commands | M | delete
- src/sim/game_entity.rs::tests::gsi_13_06_body_frame_counter_serde_default_is_zero | OBSOLETE | same reason | M | delete
- src/sim/game_entity.rs::mission_shadow_tests::mission_round_trips_through_serde | OBSOLETE | JSON round trip, commented "Slice 6 un-skips the field". The bincode snapshot tests are the real persistence path | M | delete
- src/sim/house_strategy_tests.rs::non_bincode_missing_house_field_uses_native_constructor_defaults | OBSOLETE | the name itself says the path is non-bincode, and no production JSON load of HouseState exists | M | delete
- src/sim/components.rs::drive_locomotion_serde_defaults_missing_fields | OBSOLETE | `serde_json::from_str("{}")` compat default, same reasoning | M | delete
- (trim, not counted) src/sim/anim_class.rs::anim_store_saves_as_before_and_notes_every_hand_out | MIGRATION | the "Former" newtype equality (:3466-3486) pins the pre-refactor serialized shape. The touch-log half is real behaviour | M | keep the touch-log assertions and drop the Former comparison

## WRONG / misleading (keep or fix, report)

- src/sim/bounce.rs::gsi_05_14_axis_normalisation_associates_y_z_then_x | WRONG (trivially true) | its own comment says "They agree here by symmetry" (y == z), so the native-vs-decompiler assert cannot fail. The second half duplicates gsi_05_14_init_normalises_the_tumble_axis | H | delete, or rebuild with asymmetric inputs. Counted under DUP
- src/sim/economy.rs::purifier_bonus_credits_single_truncation_not_double | WRONG (evidence level) | presents "45 (gamemd)" as a "BLOCKER parity case", but the value is hand-calculated. No purifier/income oracle exists under tools/ (only refinery_dock) | M | keep as a Rust regression; relabel it or obtain a native golden
- src/sim/game_options.rs::stock_multiplayer_dialog_settings_match_hardcoded_defaults | WRONG-ish | asserts that constructor defaults equal a hand-copied stock INI. The contract requires defaults to come from the constructor, not from retail values. It does not use the production retail reader | L | delete, or replace with a retail-INI read test
- src/sim/game_entity.rs::passenger_derive_unchanged_placeholder | WRONG-ish | pins a placeholder its own comment calls untraced ("pin flips with the traced value later") | M | counted under TESTONLY
- src/sim/deploy_tests.rs and deploy.rs countdown tests | WRONG-ish (approximation) | deploy.rs:9-11 says the countdown "is a local approximation" of sequence-frame completion. Tests such as ggi_* pin that approximation | L | keep, but label them as regression tests of an approximation

## MIRROR

- src/sim/deploy.rs::tests::frames_to_ticks_{ggi_deploy,short_undeploy,zero} (3) | MIRROR | `frames_to_ticks` is the identity function `frames` (deploy.rs:50-52) | H | delete, and inline the identity
- src/sim/animation_tests.rs::test_default_infantry_has_all_sequences, test_default_building_has_stand_only (2) | MIRROR | restate the literal tables in rules/animation_sequence.rs:368-480 | H | delete
- src/sim/animation_tests.rs::test_animation_is_send_sync | MIRROR | compile-time trait check only | M | delete
- src/sim/animation_tests.rs::test_switch_resets_state, test_switch_noop_same_sequence (2) | MIRROR | restate `switch_to` (animation.rs:118-126); the test_tick_* tests exercise it | M | delete
- src/sim/entity_store.rs::tests::{test_deterministic_iteration_order,test_iter_sorted,test_values_sorted,test_sorted_after_mutation,test_empty_store,test_mutable_iteration_pattern,test_cross_entity_read_pattern} (7) | MIRROR | BTreeMap ordering and borrow-pattern documentation (entity_store.rs:620-740). test_cross_entity_read_pattern only snaps a facing | H | delete
- src/sim/entity_store.rs::tests::{test_insert_and_get,test_get_mut,test_remove} (3) | MIRROR | thin map wrapper that hundreds of sim tests exercise | M | delete
- src/sim/components.rs::{test_position_creation,nav_target_ref_has_cell_object_and_building_shapes,drive_locomotion_hash_changes_when_runtime_state_changes,c4_state_types_are_send_sync_copy} (4) | MIRROR | struct literal read back, enum constructors, `derive(Hash)`, trait bounds (components.rs:985-1085) | H | delete
- src/sim/components.rs::{drive_locomotion_default_is_inert,test_types_are_send_sync,drive_coord_cell_uses_center_leptons} (3) | MIRROR | Default derive, trait bounds, a one-line constructor | M | delete
- src/sim/economy.rs::economy_default_is_zeroed | MIRROR | Default derive (economy.rs:116) | H | delete
- src/sim/economy.rs::add_harvested_raw_adds_without_x5, apply_income_mult_identity_at_one (2) | MIRROR | a setter, and an identity already covered by the truncation test | M | delete
- src/sim/game_options.rs::build_off_ally_default_matches_yr_enabled | MIRROR | a Default value, already asserted by stock_multiplayer_... and absent_section_... | H | delete
- src/sim/game_options.rs::normalized_animation_formula_and_zero_boundary_are_exact | MIRROR | restates the formula `(x<<3)/(speed+1)` in the assert (game_options.rs:326-341). The table test next to it is the real check | H | delete
- src/sim/game_options.rs::stock_multiplayer_dialog_settings_match_hardcoded_defaults | MIRROR (see WRONG) | M | delete
- src/sim/game_entity.rs::tests::test_new_entity_defaults | MIRROR/TESTONLY | asserts the fields of the cfg(test) fixture `test_default` (game_entity.rs:1967) | H | delete
- src/sim/game_entity.rs::tests::test_is_alive | MIRROR | `health > 0` | H | delete
- src/sim/game_entity.rs::lifecycle_tests::lifecycle_authority_state_axes_are_independent | MIRROR | writes plain fields and reads them back (game_entity.rs:2180-2205) | H | delete
- src/sim/game_entity.rs::mission_shadow_tests::mission_defaults_to_idle_none | MIRROR | constructor defaults | M | delete
- src/sim/house_state.rs::house_ai_activation_latches_default_false | MIRROR | Default derive (house_state.rs:996) | H | delete
- src/sim/house_state.rs::native_difficulty_values_are_hardest_first | MIRROR | enum discriminants and from_native restated; the oracle test set_difficulty_matches_the_original exercises from_native | M | delete
- src/sim/intern.rs::interned_id_is_copy | MIRROR | a Copy derive | H | delete
- src/sim/intern.rs::different_strings_get_different_ids, get_returns_none_for_unknown (2) | MIRROR | trivial map behaviour. Keep serde_round_trip, because Deserialize is custom (intern.rs:128-140) | M | delete
- src/sim/infantry.rs::object_category_import_keeps_rules_fixture_infantry | MIRROR | parses a fixture and checks its category (infantry.rs:1114) | M | delete
- src/sim/estimated_health_tests.rs::reservations_are_initialized_and_hash_independently_of_actual_health | MIRROR | a per-field "changes hash" check | M | delete
- src/sim/anim_class.rs::tests::gsi_13_04_draw_and_terrain_attachment_state_roundtrip_and_hash | MIRROR | a bincode derive round trip plus a field-changes-hash check. anim_store_slots_scheduler_and_hash_roundtrip covers the store round trip | M | delete
- src/sim/deploy_tests.rs::snapshot_round_trip_mid_deploying | MIRROR | a serde derive round trip of one enum field | M | delete

## DUP

- src/sim/cell_rect.rs::playfield_bounds_from_map_header_{keeps_nearoref_native_values,clips_signed_local_before_margins,preserves_native_empty_and_small_results,caps_present_rect_at_native_margins} (4) | DUP | every input (80x58/[2,4,76,48], 80x80/[-5,-6,100,100], 0x0, 40x50/[0;4], 40x50/[5,6,40,50]) is a row of the oracle corpus that cell_rect_native_tests.rs::map_localsize_normalization_matches_native_executed_prefix checks (tools/spatial_oracle/map_queries.json "normalizations"). map/playfield.rs:272 also repeats two of them | H | delete
- src/sim/cell_rect.rs::get_cellclass_oob_returns_dummy_with_requested_coord | DUP | the oracle rows for (-1,0), (11,10) and other misses stamp the requested coord (map_lookup_matches_native_executable_pointer_and_dummy_effects) | H | delete
- src/sim/cell_rect.rs::{cell_index_uses_512_wide_stride_not_map_width,gsi_04_01_lookup_world_leptons_truncate_before_fallback,playfield_leptons_truncate_toward_zero} (3) | DUP | the oracle lookups include (512,0)->(0,1), (-1,1)->(511,0), 65536 wrap and world-lepton truncation rows, plus 92 world_playfield rows | M | delete
- src/sim/deploy_tests.rs::deploy_phase_advances_to_deployed | DUP | ggi_deploy_begins_decrementing_after_the_command_frame asserts the same transition exactly (deploy_tests.rs:1748) | H | delete
- src/sim/deploy_tests.rs::undeploy_phase_clears_to_none | DUP | same, via ggi_undeploy_begins_decrementing_after_the_command_frame (:1808) | H | delete
- src/sim/deploy_tests.rs::ggi_deploy_uses_art_frame_count | DUP | ggi_deploy_begins_decrementing... asserts ticks_remaining==15 right after dispatch | H | delete
- src/sim/deploy_tests.rs::ggi_undeploy_uses_art_frame_count | DUP | same, for undeploy (2) | H | delete
- src/sim/deploy_tests.rs::deploy_sound_emits_alongside_state_write | DUP | deploy_sound_emitted_on_phase_entry plus deploy_sound_suppressed_when_unset assert the same state and sound | M | delete
- src/sim/deploy.rs::tests::compute_anim_ticks_no_art_falls_back | DUP | deploy_tests.rs::sequence_less_infantry_falls_back_to_default_ticks | M | delete
- src/sim/deploy.rs::tests::compute_anim_ticks_uses_art_frames | DUP | ggi_*_begins_decrementing use the same GGI Deploy=300,15 / Undeploy=180,2 art | M | delete
- src/sim/crates.rs::tests::gsi_01_04_crate_minimum_lifts_single_human_count | DUP | the floored case in gsi_01_04_crate_count_applies_minimum_then_maximum plus the placement test (crates.rs:1330/1389) | M | delete
- src/sim/crates.rs::tests::gsi_01_04_crate_rules_read_all_three_image_keys | DUP | rules/crate_rules.rs::stock_crate_rules_section_parses_every_native_field and missing_section_retains_the_constructor_defaults | M | delete
- src/sim/find_nearby_cell.rs::find_nearby_same_tick_aliasing | DUP | a pure call made twice with the same arguments; find_nearby_selection_uses_frame_counter_modulo already pins frame-indexed selection | M | delete
- src/sim/find_nearby_cell.rs::find_nearby_selection_is_bit_identical_across_runs | DUP/trivial | reruns a deterministic function in the same process | M | delete
- src/sim/bounce.rs::gsi_05_14_axis_normalisation_associates_y_z_then_x | DUP/trivially true (see WRONG) | H | delete
- src/sim/animation_tests.rs::test_resolve_stand_facing_north, test_resolve_stand_facing_south (2) | DUP | test_resolve_all_8_facings asserts 0->7 and 128->3 | H | delete
- src/sim/animation_tests.rs::test_infantry_facing_slot_covers_every_byte | DUP | subsumed by the boundary and all_8 tests | M | delete
- src/sim/game_options.rs::build_off_ally_key_overrides_default | DUP | the same single-key override pattern as modded_numeric_and_bool_keys_override_defaults | M | merge into that test
- (not counted, L) src/sim/capture_manager_tests.rs::the_fate_table_walk | DUP? | hand-written boundaries that the 156-row oracle native_decide_unit_fate_corpus probably covers | L | verify the oracle's boundary rows, then delete

## CONSOLIDATE (table-driven; coverage preserved)

- src/sim/bridge_specs.rs::ramp_* (17, :1429-1573) | CONSOLIDATE/MIRROR | each test restates one match arm of `apply_ramp_transition` (:363-398) | H | one table test, saves 16
- src/sim/bridge_specs.rs::destruction_overlay_* (7, :1575-1637) | CONSOLIDATE/MIRROR | spot-check a table copied from the same statics (:424-455) | M | one test, saves 6
- src/sim/bridge_specs.rs::bridgehead_walk_* (11, :2370-2463) | CONSOLIDATE | the same shape `(lookup, start, axis) -> Option` | H | one table, saves 10
- src/sim/bridge_specs.rs::bridgehead_blow_up_row_* (7, :2465-2519) | CONSOLIDATE | the same shape | H | one table, saves 6
- src/sim/deploy_tests.rs::{move_silently_ignored_on_deployed,move_silently_ignored_on_deploying,move_silently_ignored_on_undeploying,attack_move_silently_ignored_on_deployed,enter_transport_silently_ignored_on_deployed} (5) | CONSOLIDATE | the same fixture and the same `!apply(...)` assert (:1231-1343) | H | one table over (phase, command), saves 4
- src/sim/deploy_tests.rs::deploy_mcv_rejects_{overlay_blocked,sloped,nonbuildable_land_type,live_bridge,pure_0x400_bridge_marker}_foundation_cell (5) | CONSOLIDATE | identical `(applied, remains, sound)` asserts (:627-697) | H | one table, saves 4
- src/sim/entity_store.rs::clear_radio_contacts_* (4) | CONSOLIDATE | the same fixture shape | M | one test, saves 3
- src/sim/animation_tests.rs::test_advance_{one_frame,loop_wraps_to_zero,hold_last_frame,transition_to,accumulates_native_frames,finished_does_nothing} (6) | CONSOLIDATE | one-line `advance_animation` cases | M | one table, saves 5
- src/sim/animation_tests.rs::test_resolve_{walk_facing_east_frame_3,non_directional,frame_index_wraps,facing_multiplier_differs_from_frame_count} (4) | CONSOLIDATE | `resolve_shp_frame` table | M | saves 3
- src/sim/economy.rs::purifier_bonus_* (4) | CONSOLIDATE | pure-function value tables | M | saves 3
- Total consolidation savings: 60 (H 40, M 20)

## MIGRATION

- src/sim/cell_rect.rs::passability_rect_shadow_agrees_with_pathgrid_on_plain_cells | MIGRATION | a "T2: passability shadow agreement" cross-check between CellRect and PathGrid from the migration (:2418-2451) | M | delete once PathGrid is no longer a competing authority
- L, verify first: cell_rect.rs::gsi_04_01_passability_path_grid_only_keeps_checked_projection and gsi_04_01_cellrect_pathgrid_only_required_zone_keeps_compatibility_projection pin the "legacy callers without authority" projection (cell_rect.rs:727-728). They become deletable when no production NearbyQuery or CellRect caller passes `resolved_terrain: None`. Not counted.

## Test-only or dead production code that becomes deletable (~600 LOC)
- bridge_specs.rs ~330 LOC (RA2 low-bridge step, YR selector, ZoneConnection decode/match, zone-id policy, their types and helpers)
- anim_class.rs ~110: tumble/bounce helpers (177-198), spawn_multiplayer_feedback_anim_at_world plus 2 consts. The multiplayer-feedback registry (substrate.rs:150-187, anim_class.rs:1179, lifecycle.rs:4364) has no production producer; it is a follow-up
- animation.rs ~55: sequence_is_prone, the tick_animations wrapper, the tick_dying branch
- world_hash.rs state_hash_without_credit_income_v135 (~5). capture_manager.rs:211-217 hash_before_mind_control is non-test production that serves only historical HashSchema::Before (world_hash.rs:2323)
- components.rs is_neutral (~7); house_state.rs set_state_four (~4); game_entity.rs derived_mission (~4); deploy.rs frames_to_ticks (identity)
- cloak_disguise.rs choose_default_mirage_disguise (cfg(test)). `can_open_still_disguise_gate` and `DisguiseRuntime::clear_techno` have no callers anywhere (non-cfg dead code, no tests)
