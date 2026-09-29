# UI partition test audit (src/ui/**, src/bin/**)

Scope: 460 `#[test]` in src/ui (src/bin has 0). Static read + grep only (HEAD e83df3d).
No `#[ignore]` tests in src/ui. 6 tests early-return without retail INIs
(`retail_ranges_shell()` in skirmish_shell/state/tests.rs, `retail_rules_set_...` in trackbars.rs).

Native evidence available in-repo and used by UI tests (keep these):
`tools/storage_oracle/{shell_relayout,shell_help_keys,launcher_trackbar,shell_static_timers,
shell_slide_engine,in_game_shell_geometry,saved_game_layout,saved_scrollbar,owner_button_frame,...}.json`.
No Rust test runs `tools/exact_shell_ui_matrix` or `tools/shell_certification` (Python only), so no
shell-certification E2E exists in the default Rust suite. The only whole-dialog native comparisons are
the fixture-driven tests below; everything else is per-rect hand values.

Format: `path::test` | category | evidence | confidence | action

---

## TESTONLY (code has no non-test caller)

- `src/ui/shell/modal.rs::template_id_table` | TESTONLY | `ModalKind`/`template_id` have 0 refs outside modal.rs; only caller `build_message_box_descriptor` is itself test-only; module doc modal.rs:9-10 "nothing reads it yet" | H | delete (+ code)
- `src/ui/shell/modal.rs::count_rule_selects_template_by_populated_optional_slots` | TESTONLY | `message_box_kind` is `#[cfg(test)]` (modal.rs:94) | H | delete (+ code)
- `src/ui/shell/modal.rs::count_rule_treats_empty_string_as_absent` | TESTONLY | `slot_populated` `#[cfg(test)]` (modal.rs:106) | H | delete
- `src/ui/shell/modal.rs::result_convention_split` | TESTONLY | `result_convention` only read by a debug_assert inside test-only `build_message_box_descriptor` | H | delete (+ code)
- `src/ui/shell/modal.rs::message_box_control_result_mapping` | TESTONLY | `from_message_box_control` `#[cfg(test)]` (modal.rs:147) | H | delete (+ code)
- `src/ui/shell/modal.rs::quit_confirm_only_quits_on_ok` | TESTONLY | `quit_confirm_quits` `#[cfg(test)]` (modal.rs:161); production quits via `modal::control::OK` compare in app/shell_main_menu.rs:844 | H | delete (+ code)
- `src/ui/shell/modal.rs::message_box_descriptor_control_sets_match_count_rule` | TESTONLY | `build_message_box_descriptor`/`MessageBoxRects` 0 external refs | H | delete (+ code)
- `src/ui/shell/modal.rs::body_ok_descriptor_never_adds_cancel_or_third` | TESTONLY | same | H | delete
- `src/ui/shell/layout.rs::modal_centered_policy_skips_reanchor` | TESTONLY | `layout_pass` production callers: main_menu_shell/layout.rs:234 only (IncludeSetReanchor); the only ModalCentered descriptor builder is test-only modal.rs:189; test builds a synthetic descriptor (layout.rs:376) | H | delete; delete `RepositionPolicy::ModalCentered` arm (layout.rs:42-44) + `BgKind::ModalShp`
- `src/ui/shell/in_game_options.rs::baseline_layout_is_raw_dlu_to_pixel_per_control` | TESTONLY+OBSOLETE | bare `layout_pass` on the 0xBBB descriptor only called here (in_game_options.rs:502); production uses `layout_pass_in_game_options` (app/input/in_game_options.rs:92, app/frontend/skirmish_shell_render/in_game_options.rs:33); layout.rs:448 "Replaces the superseded 5a-i ... baseline" | H | delete; delete `RepositionPolicy::InGameOptions` arm in `layout_pass` (layout.rs:53-57)
- `src/ui/gadget/list.rs::retained_order_add_tail_head_after` | TESTONLY | `add_head`/`add_after` are `#[cfg(test)]` (list.rs:189,199) | H | delete (+ code)
- `src/ui/gadget/list.rs::add_after_missing_returns_none` | TESTONLY | same | H | delete
- `src/ui/gadget/list.rs::set_focus_steals_and_moves_keyboard_bit_g18` | TESTONLY | `set_focus` `#[cfg(test)]` (list.rs:255); production never sets `focus.keyboard` (only tick.rs:161 clears it) | H | delete (+ `set_focus`, `clear_focus`)
- `src/ui/gadget/list.rs::disable_forces_clear_focus_and_dirty_g18_g19` | TESTONLY | `set_enabled` (list.rs:282) has no caller outside this test (app `set_enabled` hits are tooltips/other types) | H | delete (+ `set_enabled`, `clear_focus`)
- `src/ui/gadget/focus.rs::clear_attached_list_only_clears_list` | TESTONLY | `clear_attached_list` `#[cfg(test)]` (focus.rs:44) | H | delete (+ code)
- `src/ui/gadget/list.rs::extract_by_id_and_clear` | TESTONLY (partial) | `extract_by_id` `#[cfg(test)]` (list.rs:220); `clear` is production | M | trim to the `clear` half or delete
- `src/ui/gadget/tick.rs::g10_keyboard_tier_and_g13_result` | TESTONLY (unreachable input) | production feeds only KEY_LMB/RMB_DOWN/UP or 0 (app/input/gadget_input.rs:412-423, 434) and never sets keyboard focus; relies on test-only `set_focus` (tick.rs:626) | M | delete with keyboard tier (tick.rs:232-240) or keep as dormant-native port
- `src/ui/gadget/tick.rs::g15_keyboard_flag_bypasses_bounds` | TESTONLY (unreachable input) | no production gadget mask contains FLAG_KEYBOARD 0x100 (gadget_input.rs CONTROL_FLAGS 0x11, SCROLL_FLAGS 0x55, CAMEO 0x19, regions 0x7F/0xDF) | M | same

## OBSOLETE / MIGRATION

- `src/ui/skirmish_shell/scroll.rs::unified_matches_combo_model_over_boundaries` | MIGRATION+MIRROR | "Verbatim reference copies of the pre-4E legacy math ... NEVER change" (scroll.rs:103-104); the three `legacy_*` fns are line-for-line the current `ScrollModel` bodies (scroll.rs:44-95) — a second implementation of prior Rust, not native | H | delete with ~60 LOC of legacy copies
- `src/ui/skirmish_shell/scroll.rs::unbounded_combo_never_needs_a_scrollbar` | MIRROR | restates `visible_rows` `cap==0` branch | M | delete
- `src/ui/skirmish_shell/scroll.rs::an_empty_combo_shows_no_scrollbar_and_its_thumb_would_fill_the_track` | MIRROR | pins a branch its own comment calls unreachable; re-derives `track_h.max(MIN)` inline | M | delete
- `src/ui/shell/geom.rs::right_panel_rects_byte_equal_to_pre_refactor_literals` | MIGRATION+DUP | "Values asserted by the three shells' existing suites pre-refactor" (geom.rs:277); identical numbers in skirmish_shell/layout.rs::right_panel_globals_match_research_modes (layout.rs:1347) and key_rects_match_1024x768; tile_count/panel origin also exercised natively by slide.rs::column_schedule_matches_the_executed_slide_engine | H | delete (keep one copy)
- `src/ui/shell/geom.rs::lower_strip_matches_pre_refactor_values` | MIGRATION | prior-Rust literals; 1024 case repeated in main_menu_shell/layout.rs::large_screen_offsets_movie_without_scaling | M | delete or fold
- `src/ui/skirmish_shell/state/random_map_setup.rs::gsi_04_02_dialog_open_is_rng_pure_and_generate_reroll_uses_process_main_only` | MIGRATION+MIRROR | `reroll_derived_for_generate` is a one-line wrapper over `derive_from_map_type` (random_map_setup.rs:368-374); test compares wrapper vs the same fn on a cloned RNG; `open` takes no RNG so the "rng pure" and scenario/mapgen asserts are trivially true | H | delete
- `src/ui/skirmish_shell/state/tests.rs::choose_map_action_bubbles_without_cycling_selected_map` | OBSOLETE | guards against removed map-cycling; `apply_action(ChooseMap)` is identity (hit_test.rs:374) | M | delete
- `src/ui/skirmish_shell/state/tests.rs::hit_test_ignores_combo_faces_after_owner_draw_buttons` | OBSOLETE | `hit_test` only checks 3 button rects (hit_test.rs:341-363); guards a legacy combo hit path that no longer exists | M | delete

## DUP / E2E (name of the stronger test that already fails on the same regression)

- `src/ui/skirmish_shell/state/tests.rs::choose_map_status_help_keys_use_verified_0x6b_mapping` | DUP | `choose_map_help_keys_match_the_native_table` (tests.rs:2531) compares the same 6 ids against the executed `0x006040B0` table; only extra assert (0x695→None) is also in the native fixture | H | delete; add 0x695 to the native loop
- `src/ui/skirmish_shell/state/tests.rs::trackbar_outside_thumb_click_remaps_value_and_keeps_capture` | DUP | identical press (rect.x+39=443, first_admitted_row=319) and identical asserts (7400, hold dragging:false, GenericClick) are the first half of `trackbar_rail_click_jumps_once_and_does_not_track_cursor` (tests.rs:1010) | H | delete
- `src/ui/skirmish_shell/state/tests.rs::battle_default_active_ai_starts_on_team_d` | DUP | `default_shell_tracks_native_slot_count` (tests.rs:1068) asserts the same active row and team 3 | H | delete
- `src/ui/skirmish_shell/layout.rs::skirmish_unit_count_trackbar_applies_0102_fixup_y_minus_one` | E2E | `trackbar_windows_match_the_executed_relayout` (layout.rs:1211) checks 0x50C against native relayout at 640/800/1024 (native = (404,340,129,22)) | H | delete
- `src/ui/skirmish_shell/layout.rs::skirmish_option_checkboxes_apply_0102_fixup_x_minus_one` | DUP | x=71/302 already asserted by `checkbox_rects_match_800x600_final_geometry`; 1024 layout equals 800 | H | delete
- `src/ui/main_menu_shell/layout.rs::title_rect_matches_enrolled_runtime_at_required_resolutions` | DUP | byte-identical asserts in `src/ui/shell/layout.rs::production_main_menu_title_uses_exact_runtime_rect_at_required_resolutions` (layout.rs:362) | H | delete one
- `src/ui/shell/geom.rs::snap_round_half_up_reproduces_main_menu_0xe2_stacked_cells` | DUP | `main_menu_shell/layout.rs::buttons_grid_snap_and_exit_special_case_800x600` asserts the same 5 rows through the production descriptor | H | delete
- `src/ui/shell/geom.rs::snap_biased_truncate_reproduces_single_player_0x100_native_cells` | DUP | `single_player_shell/layout.rs::key_rects_match_dialog_0x100_rows_at_800x600` (menu_page.rs:131 uses the same helper) | H | delete
- `src/ui/shell/geom.rs::snap_biased_truncate_reproduces_skirmish_0x102_narrow_cells` | DUP | `skirmish_shell/layout.rs::key_rects_match_800x600`; values also in native shell_relayout.json 0x617/0x5AA | H | delete
- `src/ui/shell/layout.rs::anchor_rules_reproduce_main_menu_rects_800x600` | DUP | synthetic hand copy of the 0xE2 descriptor; 0x683/0x3EE covered by main_menu layout tests, 0x694 by `production_main_menu_title_...`, 0x71C by `right_panel_statics_take_the_vertical_half_beyond_600` | H | delete
- `src/ui/shell/layout.rs::snap_and_bottom_row_track_supported_screen_sizes` | DUP | same rects via production layout in `main_menu_shell/layout.rs::key_rects_match_640x480_movie_choice` and `large_screen_buttons_sdbtnanm_cells_and_exit` | H | delete
- `src/ui/shell/layout.rs::in_game_options_ordinary_controls_centered_offset` | E2E | `in_game_options_ordinary_children_match_original_procedure` (layout.rs:469) checks 0x529 at 640/800/1024 against in_game_shell_geometry.json | H | delete
- `src/ui/shell/in_game_options_state.rs::mouse_x_maps_back_to_slider_stop` | E2E | `plain_192_rail_matches_original_thumb_and_partition_boundaries` (same rect, 209 native pointer samples incl. both clamps) | H | delete
- `src/ui/shell/trackbar.rs::trackbar_formula_matches_all_canonical_d5_thresholds` | E2E | hand thresholds for (180,0,{1,2,6}) and (128,50,10); all four geometries are in launcher_trackbar.json and every pointer x is compared by `launcher_trackbar_position_and_painted_thumb_match_original_instruction_goldens` | H | delete
- `src/ui/skirmish_shell/state/random_map_setup.rs::finishing_generate_unlocks_accept` | DUP | tail of `failed_generation_cannot_accept_or_save_old_or_partial_results` (begin+finish → Ok/Save enabled) | M | delete
- `src/ui/shell/controller.rs::push_over_base_pops_back_to_it` | DUP | `stack_push_pop_focus_restore` + `kbd_route_is_registration_order_and_pruned_on_pop` | M | delete
- `src/ui/main_menu_shell/state.rs::controller_release_must_match_pressed_button` | DUP | `shell/controller.rs::press_must_match_release` + `button_actions_preserve_return_codes` | M | delete
- `src/ui/skirmish_shell/state/tests.rs::launch_session_preserves_build_off_ally_default` | DUP | implied by `default_shell_options_use_launch_defaults` + `launch_session_packs_selected_map_and_enabled_slots` (copies build_off_ally) | M | delete
- `src/ui/gadget/button.rs::cameo_fires_on_press_strips_leftup_a2` | DUP (partial) | `tick.rs::cameo_fires_on_press_only_a2` covers press-fire and LEFTUP strip end-to-end; only right-press `|0x4000` is unique | M | fold right-press case into tick test
- `src/ui/gadget/button.rs::g22_row4_drag_off_cancels` | DUP | `tick.rs::g22_end_to_end_drag_off_cancels` | M | delete (or keep `is_on` assert in e2e)
- `src/ui/gadget/list.rs::remove_clears_focus_slots_g24` | DUP | `focus.rs::on_removed_clears_only_matching_slots` (remove = `focus.on_removed`), uses test-only `set_focus` | M | delete
- `src/ui/tooltips.rs::duration_auto_hide_re_arm` | DUP | `coordinates_refresh_visible_tip_and_restore_ordinary_delay` asserts the 10000 ms auto-hide boundary (10_199/10_200) | M | delete
- `src/ui/skirmish_shell/layout.rs::large_screen_offsets_without_scaling` | DUP | 156x42 / 144x112 sizes already asserted at 1024 by `key_rects_match_1024x768` | M | delete

## MIRROR

- `src/ui/skirmish_shell/state/tests.rs::status_help_keys_use_verified_stt_mappings` | MIRROR | hand copy of `status_help_key_for_hover` match (hit_test.rs:124-178); native table for dialog 0x102 (72 controls) exists in tools/storage_oracle/shell_help_keys.json but is unused for 0x102 | H | replace with fixture loop like `choose_map_help_keys_match_the_native_table` (also subsumes key half of `skirmish_status_help_includes_flag_and_right_panel_static_targets`)
- `src/ui/skirmish_shell/state/trackbars.rs::default_bounds_match_stock_constants` | MIRROR | `Default` == the same constants | H | delete
- `src/ui/main_menu_dialogs/options.rs::descriptor_is_complete_authoritative_layout_and_omits_dormant_603` | MIRROR | 17-row copy of `LAUNCHER_DESCRIPTOR` (options.rs:1588-1722) | H | delete (keep a 1-line "no 0x603" assert if wanted)
- `src/ui/main_menu_dialogs/options.rs::physical_frame_rounds_global_edges_before_subtraction_at_common_scales` | MIRROR+TESTONLY | re-derives `(edge*scale).round()` inline; entry point `PhysicalControlFrame::from_logical` is `#[cfg(test)]` (options.rs:611) | H | delete
- `src/ui/gadget/mod.rs::seed_constant_value` | MIRROR | `HIT_SEED_AREA == 1024*768` literal restated | H | delete
- `src/ui/shell/geom.rs::center_offset_equals_if_guard_form_at_boundaries` | MIRROR | inline `if s > b {(s-b)/2} else {0}` is the implementation | H | delete
- `src/ui/shell/geom.rs::mul_div_round_matches_round_half_up_muldiv_all_odd_dlu` | MIRROR | reference re-implementation, not native MulDiv output; DLU conversion covered natively by relayout fixture tests | M | delete
- `src/ui/skirmish_shell/state/tests.rs::ai_row_type_uses_verified_item_data_order` | MIRROR | restates `item_data` match | M | delete or back with native
- `src/ui/skirmish_shell/state/tests.rs::game_speed_visual_position_inverts_stored_value` | MIRROR | `MAX - x` (trackbars.rs:107-113) | M | delete
- `src/ui/shell/in_game_options_state.rs::slider_pos_inverts_speed_round_trip` | MIRROR | `MAX - x` (in_game_options_state.rs:74-81), second copy of the same inversion | M | delete
- `src/ui/skirmish_shell/state/tests.rs::default_shell_options_use_launch_defaults` | MIRROR | two Defaults compared field-by-field (drift guard for duplicated defaults) | M | fold into one defaults test
- `src/ui/skirmish_shell/state/tests.rs::player_name_focus_survives_dropdown_open_close_until_explicit_blur` | MIRROR/NOTATEST | "dropdown open/close" is two direct field writes (tests.rs:477-481), no dropdown code runs; only `blur_player_name_edit` is exercised | M | reduce to a blur test or delete
- `src/ui/skirmish_shell/state/player_name.rs::fresh_shell_starts_with_an_unreserved_random_position` | MIRROR | Default field | M | fold into defaults test
- `src/ui/skirmish_shell/state/random_map_setup.rs::open_keeps_an_existing_seed` | MIRROR | constructor copies field | M | delete
- `src/ui/skirmish_shell/state/random_map_setup.rs::map_type_entries_start_at_one_and_omit_archipelago` | MIRROR | const table restated | M | delete
- `src/ui/skirmish_shell/state/random_map_setup.rs::only_two_theaters_are_offered` | MIRROR | const table length | M | delete
- `src/ui/main_menu_dialogs/options.rs::exact_launcher_label_fallbacks_are_local_and_complete` | MIRROR | 31-row copy of `LAUNCHER_LABEL_SPECS`; fallbacks are English strings, not native | M | delete or keep only the lookup-order assert
- `src/ui/shell/pause_menu.rs::b5_resource_has_six_buttons_and_two_statics` | MIRROR | hand copy of resource ids/keys, not read from retail bytes | M | delete or replace with a resource-bytes oracle
- `src/ui/single_player_shell/state.rs::status_help_keys_match_dialog_0x100_control_mapping` | MIRROR | hand keys; native 0x100 table (8 controls) exists in shell_help_keys.json | M | replace with fixture loop
- `src/ui/pause_menu.rs::only_the_closed_state_lets_the_mission_run`, `::no_choice_leaves_the_modal_alone` | MIRROR | `is_open == !Closed`; `None → Stay` | M | delete

## CONSOLIDATE (coverage kept; savings = N-1)

- `src/ui/skirmish_shell/layout.rs` `key_rects_match_800x600`, `key_rects_match_1024x768`, `key_rects_match_640x480_formula`, `fixed_800_layout_centers_native_shell_without_rescaling`, `choose_map_modal_layout_matches_verified_0x6b_geometry`, `choose_map_modal_high_res_preserves_lists_and_offsets_shell_helpers` (6) | CONSOLIDATE | hand rects; shell_relayout.json has all 72 0x102 and 11 0x6B children at 3 resolutions but only 7 ids are compared | M | one table test over the fixture (save 5)
- `checkbox_rects_match_800x600_final_geometry` + `checkbox_rects_match_640x480_final_geometry` (skirmish layout.rs) | CONSOLIDATE | same values, loop the resolution | H | save 1
- `hit_test_start_choose_and_back`, `hit_test_uses_exclusive_bottom_right_edges`, `owner_draw_button_hit_test_returns_control_identity` (tests.rs) | CONSOLIDATE | same 3 rects; `hit_test` duplicates `hit_test_owner_draw_button`+`action_for_owner_draw_button` (hit_test.rs:316-363) | M | save 2
- `default_shell_tracks_native_slot_count` + `default_inactive_ai_rows_use_native_combo_defaults` (+ the MIRROR defaults above) | CONSOLIDATE | one default-state test | M | save 1
- `status_help_ai_row_state_uses_item_specific_stt`, `status_help_side_row_uses_item_specific_stt`, `status_help_color_row_uses_item_specific_stt_with_generic_miss_fallback` | CONSOLIDATE+MIRROR | three hand tables of one match | M | save 2
- random_map_setup.rs `ok_and_save_start_disabled`, `generate_and_cancel_start_enabled`, `load_and_delete_follow_saved_seed_availability_at_open` | CONSOLIDATE | one enable-matrix at open | H | save 2
- random_map_setup.rs `size_writes_both_axes` + `size_selection_reflects_the_width_axis` | CONSOLIDATE | same setter path | M | save 1
- in_game_options.rs `descriptor_carries_all_seventeen_0bbb_controls`, `control_kinds_match_template`, `descriptor_dlu_rects_match_verified_template`, `static_csf_keys_match_template`, `enabled_state_matches_template_default`, `visualdetails_triplet_hidden_rest_visible` (6) | CONSOLIDATE+MIRROR | hand copy of the 0xBBB template ("Verbatim from the live 0xBBB dialog resource template", in_game_options.rs:367) — not read from retail bytes; `bg_kind`/`slide_eligible` asserted but never read in production | M | one table test, ideally against resource bytes like saved_games.rs:210 (save 5)
- main_menu_dialogs.rs `exit_confirm_open_resolves_pinned_keys_to_fallbacks` + `exit_confirm_open_uses_csf_when_present` | CONSOLIDATE+MIRROR | constant == constant | M | save 1
- src/ui/pause_menu.rs remaining 6 transition tests | CONSOLIDATE | one-assert transition checks → one table | M | save 5
- main_menu_shell/layout.rs `key_rects_match_800x600` + `buttons_grid_snap_and_exit_special_case_800x600` | CONSOLIDATE | buttons[0]/[5] asserted twice | H | save 1
- main_menu_shell/layout.rs `large_screen_offsets_movie_without_scaling` + `large_screen_buttons_sdbtnanm_cells_and_exit` | CONSOLIDATE | right_panel/title/lower_strip already asserted elsewhere | M | save 1

## WRONG / unbacked parity claims (not deletions — correct or record residual)

- skirmish_shell/layout.rs `represented_0102_player_name_one_pixel_fixup_is_applied` (58,59,151,23), `checkbox_rects_match_*_final_geometry` (150x16 etc.), `color_combos_and_flags_do_not_right_anchor` (flag 48x20), `row_combo_rects_match_800x600_resource_geometry` (start 38x119), `option_label_static_rects_preserve_resource_positions` (90x16), `key_rects_*`/`choose_map_modal_*` map_preview (144x112) | WRONG (1 px) | the executed relayout (shell_relayout.json, 0x102 @800) gives 0x6A0 (58,59,152,24), 0x54E (71,286,151,17), 0x6DA (225,59,49,21), 0x6A3 (486,59,39,120), 0x699 (302,286,91,17), 0x468 (644,37,145,113): every one is +1 w/h. Only the combos' `dlu_rect` choice is documented (layout.rs:664 "face paint expects"); the rest are prior-Rust goldens named "verified"/"final geometry". Hit-tests use these rects, so right/bottom edge pixel differs from native | L | record a 1-px residual or compare against fixture-1 explicitly
- `src/ui/gadget/tick.rs::minimap_right_click_releases_sticky_with_0xdf` | WRONG (documented divergence) | gadget/mod.rs:75-80: native mask is 0x9F, Rust uses 0xDF "as a Rust-native divergence" | L | keep only if divergence stays accepted
- `src/ui/gadget/tick.rs::minimap_literal_0x9f_would_leave_capture_stuck` | NOTATEST | documents the rationale with a mask no production code uses | M | delete (comment already carries it)
- `src/ui/shell/slide.rs::modal_and_in_game_dialogs_do_not_slide` | WRONG (pins gap) | own comment: 0xBBB "is [in 0x0060C540's list], but has no slide here yet" | L | mark as residual, not expected behavior
- `src/ui/wol_shell.rs::right_panel_matches_the_executed_relayout` | unbacked claim | named "executed relayout" but hand values, no fixture | L | cite/produce evidence

## Test-only / dead production code (deletable with the tests above)

- src/ui/shell/modal.rs: `ModalKind`, `template_id`, `result_convention`, `ResultConvention`, `message_box_kind`, `slot_populated`, `MESSAGE_BOX_DISMISSED`, `from_message_box_control`, `quit_confirm_quits`, `MessageBoxRects`, `build_message_box_descriptor`, `includes_cancel/third`, `modal_control` ≈ 170 LOC (modal.rs:30-245 minus `ModalResult::options_persists` and `control`)
- src/ui/shell/layout.rs `layout_pass` ModalCentered + InGameOptions arms ≈ 20 LOC; `RepositionPolicy::ModalCentered`, `BgKind` (never read: no production reader of `.bg_kind`/`.slide_eligible`) ≈ 25 LOC
- src/ui/gadget: `add_head`, `add_after`, `extract_by_id`, `set_focus`, `clear_focus`, `set_enabled`, `clear_attached_list`, keyboard tier in `tick` ≈ 80 LOC
- src/ui/main_menu_dialogs/options.rs `PhysicalControlFrame::from_logical` ≈ 15 LOC
- src/ui/skirmish_shell: `SkirmishShellAction::SelectColor/SelectMap` + `ColorComboId` + their `apply_action` arms (never constructed; app/shell_skirmish.rs:336 ignores them; `(idx+1)%8` colour cycling is non-native) ≈ 30 LOC
- scroll.rs legacy copies (test code) ≈ 60 LOC
Total ≈ 400 LOC.

## Refactor leads observed (not test findings)

- Three copies of the right-panel Back-row formula: skirmish_shell/layout.rs:419 `back_rect`, shell/menu_page.rs:95 `back_rect`, shell/layout.rs `OwnerDrawButtonBottomRow` arm — why (644,535,156,42) is pinned by ~10 tests.
- Two press/release machines: shell/button.rs `ShellButtonInteraction` and shell/controller.rs `DialogController` both tested for "release must match press".
- Two identical speed inversions (skirmish trackbars.rs:107, in_game_options_state.rs:74).
- `hit_test` re-implements `hit_test_owner_draw_button` + `action_for_owner_draw_button`.
