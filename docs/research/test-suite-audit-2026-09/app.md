# src/app test audit (read-only)

Scope: 965 `#[test]` in `src/app/**` (25 `#[ignore]`: 13 GPU/retail, 11 retail-asset, 1 panic residual).
Method: listed every test (scratchpad `app_tests.txt`), read the bodies of every file with suspicious names, every
test ≤8 lines (≈170), every `#[cfg(test)]`-gated production item in `src/app`, and sampled the large files
(skirmish.rs, skirmish_shell_render.rs, pump/tests.rs, camera.rs, dispatch.rs, cursor.rs, hotkeys.rs, sim_tick.rs,
shell_capture.rs, tactical_capture/*). Verified callers with grep. Line numbers are at the audited checkout.

Confidence: H = safe to delete now; M = delete after the named small follow-up (fold one assertion, confirm one
native routine); L = judgement call.

---

## Totals (default suite, 965 tests)

| category | H | M | L |
|---|---|---|---|
| TESTONLY | 16 | 7 | – |
| MIRROR (incl. 2 OBSOLETE) | 31 | 20 | 9 |
| DUP | 20 | 11 | 2 |
| WRONG | 1 (+1 ignored) | 4 | – |
| CONSOLIDATE savings | 7 | 34 | 7 |
| **total removable** | **75** | **76** | **18** |

H+M = 151 (15.6%). Including L = 169 (17.5%). The 30–40% hypothesis is not supported for `src/app`: most app tests
are native-evidence behavior tests (hotkeys, camera, cursor, entity_pick item83, loading/pump, persistence, shell
layout), GPU/oracle goldens, or the only tests for sim/render owners that happen to live in app/ (relocate, not delete).

## TESTONLY — the tested code is `#[cfg(test)]` or has no production caller

- `frontend/skirmish_shell_render.rs::{semantic_draw_order_records_verified_right_panel_sequence (1397), semantic_draw_order_keeps_1024_parent_blank_but_large_lower_strip (1445), preview_markers_require_real_preview_surface (1457), decoded_preview_surface_does_not_imply_start_marker_overlays (1472), choose_map_modal_semantic_draw_order_replaces_parent_shell (1481), random_map_setup_semantic_draw_order_uses_generic_shell_profile (1529), validation_modal_semantic_draw_order_is_blocking_overlay (1560)}` (7) | TESTONLY | `SkirmishShellDrawRole` and all `*_semantic_draw_order`/`push_*_roles` fns are `#[cfg(test)]` (`skirmish_shell_render/draw_order.rs:47,111,135,148,179,198,224`). This is a second description of the paint order, not the draw path, so production reordering cannot fail these tests | H | delete the tests and ≈140 LOC of cfg(test) code in draw_order.rs
- `frontend/skirmish_shell_render.rs::standard_offline_first_paint_skips_sdbtnanm_frame10_overlay (1426)` | TESTONLY (partial) | uses the same cfg(test) order. Only `right_panel_frame10_overlay_active(&shell)` is production | M | keep that one assert (fold into a neighbour), delete the rest
- `frontend/skirmish_shell_render.rs::pressed_buttons_select_down_skin_assets (1150)` | TESTONLY | `button_piece_asset_names` is `#[cfg(test)]` (`skirmish_shell_render/chrome.rs:60`). Production resolves bue/bde in `render/skirmish_shell_chrome.rs:457-573` | H | delete test and fn
- `frontend/skirmish_shell_render.rs::{text_origin_centers_and_applies_pressed_offset (1728), text_origin_supports_left_and_right_alignment_flags (1782)}` (2) | TESTONLY | `shell_text_origin` is `#[cfg(test)]` (`skirmish_shell_render.rs:119`) with no production twin | H | delete tests and fn
- `presentation/instances/overlays.rs::gsi_04_10_render_visibility_distinguishes_unregistered_live_and_destroyed (1287)` | TESTONLY | `terrain_object_is_render_visible` is `#[cfg(test)]` (`overlays.rs:54`) and is a standalone reimplementation, not an adapter. No production reader of `terrain_object_cells` exists in presentation | H | delete test and ≈27 LOC fn
- `frontend/skirmish_session.rs::resolving_launch_copy_does_not_mutate_raw_random_fields (1509)` | TESTONLY | `resolve_launch_session` is `#[cfg(test)]` (`skirmish_session.rs:358`). Production resolves through `close_shell_transaction`. "Input not mutated" is guaranteed by the `&SkirmishLaunchSession` borrow | H | delete test and ≈35 LOC fn. Cross-partition: `skirmish_launch.rs:506 resolve_shell_random_assignments` then has only test callers
- `match_runtime/sim_tick.rs::session_mode_maps_writer_proofed_game_mode_values (1882)` | TESTONLY | `SessionMode::from_game_mode` is `#[cfg(test)]` (`sim_tick.rs:458`). `SessionMode` carries `cfg_attr(not(test), expect(dead_code))` (`:433-440`) | H | delete test and fn
- `match_runtime/sim_tick.rs::{only_network_modes_advance_behind_a_modal (1896), reentrancy_guard_blocks_advance_even_on_network (1926), service_only_blockers_prevent_network_modal_advance (1942), pumped_world_tick_freezes_offline_and_advances_on_network (1980)}` (4) | TESTONLY (dead branch) | `current_session_mode` always returns `Skirmish` (`sim_tick.rs:529-531`). The Lan/Wol/Campaign/Other variants and `TickLane::NetworkModal` exist only in tests. The pumped-world test loops the predicate inside the test body | M | collapse to one live-path assertion (Skirmish never advances behind a modal). Drop the network arms or move them to the future netcode PR. Also trim the Lan block of `sidebar_projection.rs::sidebar_credit_gate_matrix`
- `match_runtime/sim_tick.rs::{upsert_updates_existing_and_inserts_absent_coordinates (1628), upsert_empty_inputs (1649), gsi_04_09_render_handoff_replaces_regerminated_overlay_variant (1664)}` (3) | TESTONLY + DUP | `upsert_overlay_entries` is a `#[cfg(test)]` adapter. Its own comment says "contract tests moved with it" (`sim_tick.rs:1319-1321`). Covered by `presentation/overlay_index.rs::overlay_render_index_preserves_source_dynamic_tombstone_and_restore_order` (slot retention, re-occupation with a new id, append, counts) | H | delete tests and adapter
- `match_runtime/sim_tick.rs::{upsert_reuses_coordinate_within_candidate_list (1638), upsert_identical_existing_entries_is_a_noop (1657)}` (2) | TESTONLY + DUP | same adapter. The duplicate-in-batch and identical-entry `0` count are not in the overlay_index test | M | fold those two asserts into the overlay_index test, then delete

## MIRROR — restates the implementation (constant == literal, test-local copy, getter/default)

- `presentation/instances/particles.rs::{translucency_byte_to_alpha_table (153), fire_frame_uses_facing_band_times_end_state (172)}` (2) | MIRROR / second implementation | both define a local `fn alpha`/`fn fire_frame` inside the test and assert that copy. Production is at `particles.rs:82-83,105-112`, which neither test calls | H | delete
- `presentation/sidebar_gadgets.rs::{sw_ready_starts_defense_tab_flash, other_three_tabs_never_flash, auto_stop_on_condition_clear, repeat_start_during_active_does_not_resync_phase, sim_tick_delta_loop_iterates_correctly_under_catchup, phase_math_examples}` (6, 138-219) | MIRROR + DUP | the tests drive a test-local `orchestrate()` that copies `update_sidebar_gadget_state` (`sidebar_gadgets.rs:108-135` vs `30-82`), and `phase_math_examples` recomputes the formula inline. The `GadgetFlash` owner already tests start/stop/tick (`sidebar/gadget_flash.rs:244-341`) | H | delete all 6. If the phase formula needs a guard, extract `flash_start_args(frame)` and test that
- `diagnostics/debug_overlays.rs::{tint_colors_are_distinct (391), overlay_alpha_is_reasonable (397)}` (2) | MIRROR | constant != constant / constant in a range, on a dev overlay | H | delete
- `presentation/instances/bridges.rs::latin_square_value_at_xy (522)` | MIRROR + DUP | indexes the constant directly. `healthy_variant_zero_uses_latin_square_jitter (550)` covers the same (1,2)→3 through production | H | delete
- `presentation/instances/bridges.rs::latin_square_table_is_canonical_4x4 (637)` | MIRROR | compares the const to the same 16 literals (`bridges.rs:30`) | H | delete
- `presentation/instances/bridges.rs::shadow_ew_shift_constants_present (629)` | OBSOLETE | asserts `DX != 0 || DY != 0` "until visual diff (Task 17) resolves" −15 vs −45. The constant's doc says the question is closed at −15 (`bridges.rs:48-53`), so the assertion is trivially true | H | delete
- `presentation/instances/overlays.rs::anim_draw_bias_constant_matches_native (1570)` | MIRROR | `assert_eq!(ANIM_DRAW_DEPTH_BIAS_PX, -2)` | H | delete
- `match_runtime/sim_tick.rs::world_point_to_cell_forwards_bridge_lookup (1734)` | MIRROR | computes "expected" by calling `screen_to_cell_tactical_inverse` with the same arguments and applying the same round/max, which is the body of `world_point_to_cell` (`sim_tick.rs:1409-1435`) | H | delete
- `input/camera.rs::{shift_boosted_keyboard_scroll_truncates_to_52 (1880), ctrl_keyboard_scroll_overshoots_the_whole_map (1892)}` (2) | MIRROR | both recompute formulas over constants inside the test (`KEY_SCROLL_DISTANCE * MULT`, `cells << SHIFT`) and never call the keyboard-scroll code | H | delete, or replace with one call through the production scroll function
- `input/tooltips.rs::tip_text_inset_is_two_by_four (547)` | MIRROR + E2E | const == literal. Covered by `tooltip_quads_compensate_for_the_ui_camera (490)` (+2/+4 via real positions) and `tip_box_is_measured_text_plus_four_by_three (448)` | H | delete
- `input/tooltips.rs::sidebar_gadget_tip_labels_are_the_native_ones (555)` | MIRROR | seven constants each compared to their own literal | H | delete
- `presentation/render/mod.rs::game_render_counts_preserve_exact_emitted_lengths (404)` | MIRROR | `Vec::len` passthrough | H | delete
- `mod.rs::options_profile_startup_fields_survive_until_resumed (184)` | MIRROR | `App::new(x).startup_options == x` | H | delete
- `input/dispatch.rs::modifier_tests::{bare_key_bindings_require_no_modifier (2946), exact_modifier_sets_do_not_overlap (2970)}` (2) | MIRROR | trivial bool predicates. `only_ctrl/only_shift/only_alt` are used only by the dead branches listed under WRONG | H | delete, along with `only_*`
- `diagnostics/tactical_capture/evidence.rs::artifact_evidence_uses_the_observed_path_and_digest (395)` | MIRROR | field copy | H | delete
- `diagnostics/tactical_capture/evidence.rs::{adapter_evidence_preserves_exact_fields_with_stable_enum_names (412), render_counts_are_taken_from_the_production_output (460)}` (2) | MIRROR | field copies. The Debug enum names are the only schema content | M | keep one enum-name assert, delete the rest
- `loading/pump/tests.rs::both_native_loading_cadences_prepare_scenario_before_first_frame (1598)` | MIRROR | `prepares_scenario_before_first_frame` returns `true` for every variant (`pump.rs:164-168`), which makes the `else` arms at `pump.rs:817,1078` dead | H | delete. Consider deleting the fn and its dead branches
- `frontend/skirmish_shell_render.rs::shell_text_colors_follow_verified_owner_draw_sources (1763)` | MIRROR | three consts == their literals (one const is `#[cfg(test)]` only, `:87-88`) | H | delete
- `frontend/skirmish_shell_render.rs::button_label_color_uses_owner_draw_button_yellow_source (1776)`, `skirmish_shell_render/text.rs::trackbar_value_text_uses_normal_shell_yellow_source (1266)`, `skirmish_shell_render.rs::validation_modal_body_text_is_left_top_wrapped_not_centered (1179)` (3) | MIRROR | each is a constant-returning fn compared with that constant (`text.rs:117,164,212`) | H | delete
- `frontend/menu_page_render.rs::menu_page_policy_uses_native_art_without_mouse_hover_flash (486)` | MIRROR | fields of a const struct | H | delete
- `types.rs::game_speed_one_is_not_the_old_speed_two_or_options_three_calibration (206)` | OBSOLETE/MIRROR | pins against a retired calibration. `tps_for_game_speed` is a UI/debug readout (`types.rs:35-36`) | H | delete
- `types.rs::default_skirmish_speed_uses_verified_yr_stored_speed_one (200)` | MIRROR | const == 1, plus a debug-readout value | M | delete
- `types.rs::item83_type_select_outcomes_use_csf_keys_not_source_line_numbers (226)` | MIRROR | a getter returns literals. The CSF lookup is exercised by `input/messages.rs::item83_type_select_feedback_resolves_csf_into_one_real_silent_message_row` | M | delete
- `presentation/render/build_instances.rs::pixel_fx_uses_the_profile_detail_level_nonzero_gate (968)` | MIRROR | `x != 0` | M | delete
- `presentation/target_lines.rs::action_line_colors_are_palette_entries_eight_and_three (521)` | MIRROR | const == literal (palette provenance is in the comment only) | M | delete, or read PALETTE.PAL bytes instead
- `presentation/sidebar_text.rs::credits_depth_is_the_sidebar_text_layer (144)` | MIRROR | const == 0.00042, a Rust depth-lane value | M | delete. Depth is also asserted in `view_wrapper_places_credits_inside_the_sidebar_panel`
- `sidebar_projection.rs::sidebar_view_reads_are_pure (167)` | MIRROR | asserts that a `&self` getter returns the same pointer 8 times, which the borrow checker already guarantees | M | delete
- `persistence/options_profile.rs::profile_defaults_match_optionsclass (570)` | MIRROR | restates `Default` field by field | M | keep only if the values are later sourced from the ctor oracle. Otherwise delete
- `frontend/startup_options.rs::defaults_leave_the_screen_size_unset_and_audio_on (281)`, `loading/pump/tests.rs::loading_progress_standard_skirmish_initializes_one_lane_max_100 (1444)` (2) | MIRROR | Default/ctor restatement. `max_value()` is `#[cfg(test)]` (`pump.rs:70`) | M | delete
- `match_runtime/sound_dispatch.rs::the_damage_voice_roll_covers_the_native_range_at_the_native_rate (1479)` | MIRROR | the `roll < VOICE_FEEDBACK_PERCENT` gate is reimplemented in the test loop. The production gate is pinned by `the_damage_voice_speaks_on_a_roll_under_thirty_and_only_for_the_local_owner (1460)` | M | delete
- `frontend/skirmish_shell_render.rs::preview_backdrop_sits_behind_fitted_preview_surface (1035)`, `frontend/main_menu_shell_render.rs::parent_background_renders_behind_movie (1021)` (2) | MIRROR | compare two Rust depth constants | M | delete
- Source-text tests: `presentation/render/draw_passes.rs::{gsi_13_01_pixel_fx_is_last_tactical_write_before_screen_chrome (1033), gsi_13_01_pixel_fx_tail_remains_passthrough_and_tactically_scissored (1062), gsi_04_01_retained_sidebar_radar_subpass_ends_with_content_boundary (1075), gsi_04_01_tooltip_and_cursor_remain_after_the_retained_sidebar_surface (1123)}`, `frontend/shell_transition.rs::gsi_13_26_menu_page_first_paint_uses_same_presenter_entrypoint (1120)`, `frontend/menu_page_render.rs::gsi_13_26_menu_page_steady_frame_uses_rgb565_presenter_after_full_composition (519)` (6) | MIRROR/MIGRATION | `include_str!` of the module's own source, asserting substring offsets. They break on any rename and pass on behavior-preserving mistakes. They are also the only default-suite guard of pass order, because the pixel tests are `#[ignore]` GPU tests | M | replace with one draw-plan-order test (e.g. recorded batch-name sequence), then delete. Keep `loading/init.rs::literal_milestones_never_fall_back` (monotonic-literal invariant)
- Copied lookup tables: `match_runtime/eva_producers.rs::{ready_table_follows_the_type_index_jump_table (233), detected_table_follows_the_list_index_jump_table (287)}`, `input/cursor.rs::{maps_every_yr_active_action (1478), returns_none_for_ts_legacy_and_unknown (1521)}`, `frontend/skirmish_shell_render.rs::side_item_data_maps_to_verified_flag_pcxs (1277)`, `loading/composition.rs::country_key_table_contains_every_verified_load_brief_key (874)`, `frontend/shell_transition.rs::shell_kinds_map_to_their_dialog_ids (1094)`, `skirmish_shell_render/controls.rs::skirmish_swatches_match_the_native_fixed_table (714)` (8) | MIRROR | a second copy of a `match`/table. They catch typos only | L | keep one row per table as a smoke test, or validate against retail CSF/INI where a reader exists
- `presentation/target_lines.rs::selected_action_line_emits_endpoint_boxes (539)` | MIRROR/weak | `len() >= 18` | L | delete, or assert exact geometry

## DUP — the same assertion is already made by a stronger test (named)

- `presentation/instances/shp.rs::{civilian_empty_healthy_returns_0, civilian_empty_yellow_tier_returns_0, civilian_empty_red_tier_returns_1, civilian_occupied_healthy_bstate_formula_returns_2, civilian_occupied_yellow_tier_returns_2, civilian_occupied_red_tier_collapses_to_1, buildable_empty_healthy_returns_0, buildable_empty_yellow_tier_returns_1, buildable_occupied_healthy_returns_2, buildable_occupied_red_tier_returns_3, zero_over_zero_selects_damaged_body_frame, boundary_at_condition_red_inclusive}` (12, 971-1068) | DUP (hand-calculated) | all are covered by `completed_garrison_body_frames_match_native_oracle (1070)`: 54 native cases with occupants 0/8, tech −1/0/5 and health at every yellow/red boundary. `original_health_ratio_corpus_matches_completed_occupied_body_frame (1037)` covers 0/0, 25/100 and the signed/wide cases (103 native rows, `tools/spatial_oracle/health_ratio_predicates.json`) | H | delete
- `presentation/instances/shp.rs::building_frame_keeps_signed_health_and_live_strength_width (1057)` | DUP | the corpus already includes −1, 65536 and 2^31−1 widths | M | delete
- `input/selection_navigation.rs::{health_bands_include_thresholds_and_recheck_current_damage (270), health_bands_keep_signed_actual_and_live_strength_width (279)}` (2) | DUP (hand-calculated) | covered by `original_health_ratio_corpus_matches_navigation_category (238)` (boundaries 0/1/25/50 at 100, plus −1 and i32::MAX) | M | delete
- `presentation/target_lines.rs::selected_action_timer_expires_at_25_ticks (504)` | DUP | `start_timer_opens_the_window_without_a_command (529)` asserts the same 124/125 edge | H | delete
- `presentation/target_lines.rs::factory_rally_builder_emits_only_selected_local_eligible_structures (612)` | DUP/weak | asserts only `!is_empty()` on the same fixture as `disabling_unit_action_lines_does_not_disable_rally_lines (626)`. The "only" in the name is never checked | H | delete, or make it check exclusivity
- `presentation/selection_brackets.rs::emit_line_excludes_final_endpoint (534)` | DUP | `emit_line_canonicalises_right_to_left_segments (549)` asserts the rightward run is `[(0,0),(1,0),(2,0)]` | H | delete
- `loading/pump/tests.rs::{loading_progress_duplicate_milestones_do_not_redraw (1453), loading_progress_lower_milestone_does_not_redraw (1462), loading_progress_advancing_milestone_requests_redraw (1471)}` (3) | DUP | `loading_progress_suppresses_nonadvancing_raw_native_calls (1491)` covers lower, duplicate and advancing | H | delete
- `diagnostics/tactical_capture/profile.rs::sha256_matches_known_vectors (764)` | DUP | same two vectors as `util/sha256.rs::sha256_matches_known_vectors (217)`. `sha256_hex` here is a passthrough (`integrity.rs:172`) | H | delete
- `diagnostics/tactical_capture/integrity.rs::sha256_matches_known_vectors (305)` | DUP (partial) | first two asserts duplicate util. The chunked-streaming assert is unique | M | move the chunk assert to `util/sha256.rs`, then delete
- `frontend/skirmish.rs::skirmish_launch_does_not_use_country_hardcoded_mcv_for_parity_path (2540)` | DUP/OBSOLETE | guards against a retired hard-coded path. `skirmish_baseunit_selection_uses_rules_order (2506)` already proves selection is rules-driven | H | delete
- `frontend/skirmish.rs::gsi_09_01_ai_opening_grant_truncates_and_tolerates_missing_entries (1126)` | DUP (partial) | the 7×12345→864 chop is `sim/scenario_bootstrap.rs::native_ai_opening_grant_chops_where_round_to_nearest_differs (2624)` | M | fold the empty-list case into `gsi_09_01_post_map_init_grants...`, then delete
- `frontend/launch.rs::{a_cd_superstring_launches_instead_of_aborting (378), the_windowed_switch_launches_instead_of_aborting (392)}` (2) | DUP | `startup_options.rs::cd_matches_as_a_substring_but_win_matches_whole_token (317)` covers -CDROM and -win. `retail_cd_switch_is_a_global_interactive_launch_option (370)` covers routing | M | delete
- `diagnostics/shell_capture.rs::no_args_preserves_interactive_launch (1735)` | DUP | `frontend/launch.rs::no_args_still_delegate_to_interactive_shell_launch (362)` runs the outer production parser over the same case | M | delete
- `match_runtime/frame_pacer.rs::scenario_elapsed_clock_uninterrupted_wall_span_ignores_frame_activity (288)` | DUP | only `start` plus `elapsed_seconds`, as in `scenario_elapsed_clock_uses_retail_sixty_bucket_seconds (268)`. The "no hook" claim is untestable here | M | delete
- `input/tooltips.rs::tip_box_keeps_the_first_measure_when_it_already_fits (476)` | DUP | same path as `tip_box_is_measured_text_plus_four_by_three (448)` | M | delete
- `input/camera.rs::recalling_a_bookmark_centres_that_cell_in_the_tactical_viewport (1859)` | DUP | the bookmark is a plain `get`. The centring assertion is `centring_lands_a_unit_on_that_cell_at_the_tactical_centre (1385)` | M | delete
- `loading/init_helpers.rs::{map_ini_overrides_rules_values (942), map_without_overrides_leaves_rules_unchanged (963)}` (2) | DUP (misplaced rules-owner test) | exercises `RuleSet::from_rules_layers`, which is tested at the owner (`rules/ruleset.rs:6165,7494,7518`, `rules/native_processing_tests.rs`) | L | check the owner covers map-layer override, then delete

## WRONG — pins app-owned duplicate logic, a non-native fallback, or a known-wrong residual

- `input/dispatch.rs::control_group_tests::two_modifiers_match_no_binding (2795)` | WRONG (app duplicate of hotkey owner) | production always calls `control_group_press_action(KeyModifiers::default(), …)` (`dispatch.rs:2558-2559`), because modifiers are resolved by the hotkey owner into TeamCreate/TeamAddSelect/TeamCenter (`dispatch.rs:1495-1503`). The modifier arms (`dispatch.rs:2413-2425`) are dead duplicate decision logic, and the owner test is `hotkeys.rs::exact_modifiers_resolve_group_commands_and_reject_two_modifier_chords (908)` | H | delete the test and the modifier arms
- `input/dispatch.rs::control_group_tests::ctrl_assigns_shift_adds_alt_centers_bare_recalls (2771)` | WRONG (partial) | 3 of its 4 asserts hit the dead modifier arms | M | keep only the bare→Recall assert (fold into the double-tap table)
- `input/dispatch.rs::control_group_tests::{centroid_of_one_or_two_points_is_the_plain_mean (2915), centroid_of_three_or_more_drops_the_farthest_point (2926), centroid_of_nothing_is_nothing (2933)}` (3) | WRONG (duplicate, non-native) | `trimmed_centroid_leptons` (`dispatch.rs:2452-2485`, "tie-break UNVERIFIED", 2D, exact squared distance) duplicates the native port `camera.rs::selection_view_centre_leptons` (`camera.rs:960-1015`: 0x004AE290, 3D, native approximate root, strict-< first-wins, stacked quirk), which is tested at `camera.rs:2022-2109` | M (confirm the native control-group centre also routes through 0x004AE290) | route `center_camera_on_group` through `selection_view_centre_leptons`, then delete `trimmed_centroid_leptons` and these tests
- `input/cursor.rs::bridge_hut_repair_cursor_always_takes_the_overlay_branch (2373)` `#[ignore]` | WRONG/NOTATEST | the body is `panic!("unimplemented…")`, a residual recorded as an ignored test | H (not in the default suite) | move the residual text to a doc/issue and delete
- `presentation/instances/units.rs::gsi_13_10_vxl_selector_uses_unit_scalar_and_keeps_aircraft_compatibility_path (1967)` | WRONG (partial) | the aircraft assert pins the documented non-native "compatibility RGB path" (`units.rs:67-71`: "Until that term is represented, preserve this existing compatibility RGB path") | M | drop the aircraft assertion, keep Unit/Structure
- `presentation/instances/units.rs::vehicle_shadow_active_locomotor_selects_native_or_legacy_companion (1576)` | WRONG (partial) | the Teleport/Hover/slope rows pin `LEGACY_SHADOW_FRAME`, a "private atlas companion, never a native HVA frame" fallback (`render/unit_atlas.rs:50-52`) | L | keep the native-eligibility and cloak asserts, drop the legacy-frame rows or mark them as residual

## CONSOLIDATE — near-identical tests into one table (saving = N−1)

- `persistence/commands.rs::save_name_tests::{empty_returns_empty, strips_path_separators, strips_windows_reserved_chars, keeps_normal_chars, caps_at_64_chars, trims_whitespace}` (6→1) | save 5 | H
- `presentation/ui_overlays.rs::{selected_unit_draws_bracket_and_pips, hovered_unselected_unit_draws_pips_without_the_bracket, unselected_unhovered_unit_draws_nothing_regardless_of_damage}` (3→1 truth table) | save 2 | H
- `input/dispatch.rs::control_group_tests` double-tap/membership cases `{double_tap_inside_the_window_centers, double_tap_outside_the_window_recalls, a_different_slot_inside_the_window_recalls, an_extra_selected_unit_outside_the_group_recalls, a_group_member_left_unselected_recalls, an_empty_group_recalls_rather_than_centering}` (6→1) | save 5 | M
- `input/dispatch.rs::wheel_tests::{magnitude_never_scales_the_step, zero_delta_scrolls_up}` (2→1) | save 1 | M
- `input/cursor.rs::cursor_animation_tests::{a_new_cursor_shape_starts_at_frame_zero, animation_advances_one_frame_per_interval_and_never_skips, animation_wraps_at_the_end_of_the_sequence, changing_cursor_restarts_the_sequence, static_rows_never_advance}` (5→2) | save 3 | M
- `input/cursor.rs::{guard_feedback_maps_to_the_guard_area_reticle, harvest_feedback_maps_to_cursor_row_twenty_one, item82_allowed_and_blocked_scroll_feedback_use_directional_rows, bomb_cursor_rows}` (4→1 table over `cursor_id_for_feedback`) | save 3 | M
- `presentation/render_tests.rs::{test_ready_buildings_do_not_auto_arm_placement, test_invalid_armed_building_clears_when_not_ready, test_sw_armed_preserved_when_ready, test_sw_armed_cleared_when_not_ready, test_sw_armed_cleared_when_view_gone}` (5→1 over `sync_targeting_mode`) | save 4 | M
- `loading/pump/tests.rs::loading_progress_theater_ramp_*` (3→1) | save 2 | M
- `frontend/skirmish.rs::{skirmish_baseunit_vector_selects_side_matching_mcv, skirmish_baseunit_selection_uses_rules_order, skirmish_baseunit_selection_respects_required_and_forbidden_houses}` (3→1, after deleting the hard-coded-MCV test) | save 2 | M
- `diagnostics/dev_overlay.rs::frame_timer_*` (4→1, dev-only FrameTimer) | save 3 | M
- `diagnostics/shell_capture.rs` checkpoint round-trips `{slide_checkpoints_accept_only_shown_ticks, options_hover_checkpoints_round_trip_their_names, wol_checkpoints_round_trip, network_bounce_checkpoint_round_trips}` (4→1) | save 3 | M
- `diagnostics/shell_capture.rs` fail-closed args `{unsupported_resolution_fails_closed, duplicate_option_fails_closed, existing_output_directory_is_never_overwritten}` (3→1) | save 2 | M
- `diagnostics/shell_capture.rs` readiness `{active_wave_waits_without_weakening_identity_checks, first_ordinary_steady_frame_is_capture_ready, running_title_waits_for_the_retained_terminal_frame, wrong_movie_owner_is_invalid_not_waiting}` (4→1) | save 3 | M
- `frontend/skirmish_shell_render/text.rs::owner_draw_label_truncates_{ascii,latin1,non_bmp}_by_utf16_code_unit` (3→1) | save 2 | M
- `mod.rs::{gsi_01_01_noaudio_suppresses_music_and_sfx_output, gsi_01_01_default_startup_enables_music_and_sfx_output}` (2→1) | save 1 | M
- `input/camera.rs` centring `{centring_stays_on_the_tactical_centre_across_zoom → fold into centring_lands_a_unit…; cell_centre_shifts_half_a_tile… → fold into centring_target_is_the_tile_diamond_centre…}` plus octant `{nine_zone_direction_covers_all_eight_octants, touching_the_left_edge_high_up…}` | save 3 | L
- `frontend/skirmish_shell_render.rs::flag_entry_*` (3→1) and `skirmish_shell_render/controls.rs` swatch tests (3→1) | save 4 | L

## Relocate (not removable; tests of another owner living in app/)

- `frontend/skirmish.rs`: 54 of 57 tests `use crate::sim::scenario_bootstrap::*` (`skirmish.rs:62`) and test sim-owned launch bootstrap (gather, deficient starts, MCV spawn, starting units, alliances, AI credits). The file's own production code is only `house_color_map_for_launch_session`, `deployable_building_types`, `preregister_runtime_overlay_names` and `build_overlay_atlas_from_map`. These are the only tests for those sim functions (the sim owner has 26 different ones), so move them to `sim/`. Do not delete.
- `presentation/sidebar_text.rs`: 8 of 9 test `render::sidebar_text::build_credits_instances` (render-owned since F06, `sidebar_text.rs:21-25`). Move to render.
- `presentation/instances/shp.rs`: `building_frame_index` is an app wrapper around `sim::building_art::occupied_body_frame` (`shp.rs:902-918`), a caller-specific wrapper. The oracle tests belong at the sim owner.
- `presentation/render_tests.rs` (included from `render/mod.rs:401`): the click and box selection tests exercise `input::entity_pick`.

## NOTATEST
- None besides the ignored panic residual above. Assertion-free "no panic" checks such as `loading/pump/tests.rs::selected_generic_progress_uses_no_native_loader_metadata` still have closure panics as guards. Kept.

## Covered only by ignored e2e
- None of the removals rely on ignored tests. Note: the 13 `#[ignore]` GPU tests are the only pixel-level check of draw ordering, so the source-text order tests (MIRROR/M above) should be replaced, not simply dropped.

## Test-only production code that becomes deletable
- `frontend/skirmish_shell_render/draw_order.rs` cfg(test) role enum + 6 fns ≈140 LOC
- `presentation/instances/overlays.rs::terrain_object_is_render_visible` ≈27
- `frontend/skirmish_session.rs::resolve_launch_session` ≈35 (+ `skirmish_launch.rs::resolve_shell_random_assignments` ≈6, cross-partition)
- `frontend/skirmish_shell_render.rs::shell_text_origin` + cfg(test) colour const ≈22. `chrome.rs::button_piece_asset_names` ≈10
- `match_runtime/sim_tick.rs::upsert_overlay_entries` adapter ≈12, `SessionMode::from_game_mode` ≈10. If the network arm goes too, the Lan/Wol/Campaign/Other variants and the `TickLane::NetworkModal` plumbing add ≈40 more (M)
- `input/dispatch.rs`: dead modifier arms ≈12, `KeyModifiers::only_*` ≈12, `trimmed_centroid_leptons` ≈34 (replaced by the native port)
- `loading/pump.rs::prepares_scenario_before_first_frame` and its dead `else` arms ≈15
- Total ≈300–370 LOC
