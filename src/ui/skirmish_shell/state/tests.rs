//! Unit tests for the skirmish shell state modules.

use super::combos::apply_combo_selection as apply_combo_selection_for_test;
use super::*;
use crate::app::loading::init::MapMenuEntry;
use crate::map::briefing::BriefingSection;
use crate::map::preview::PreviewSection;
use crate::map::skirmish_scenarios::{
    SkirmishScenarioKind, SkirmishScenarioRecord, SkirmishScenarioSource,
};
use crate::map::waypoints::Waypoint;
use crate::rules::ini_parser::IniFile;
use crate::skirmish_launch::{
    LaunchCountry, LaunchStartPosition, LaunchTeam, LaunchValidationError, SKIRMISH_AI_SLOT_COUNT,
    SkirmishLaunchOptions,
};
use crate::skirmish_modes::stock_skirmish_modes;
use crate::ui::shell::trackbar::{THUMB_W, TrackbarHold, TrackbarRange};
use crate::ui::skirmish_shell::layout::compute_choose_map_modal_layout;
use crate::ui::skirmish_shell::{
    COMBO_DROPDOWN_ROW_H, COMBO_DROPDOWN_SCROLLBAR_BUTTON_H, COMBO_DROPDOWN_SCROLLBAR_W,
    COMBO_FACE_H, RectPx, checkbox_text_rect, compute_layout,
};

fn test_map_entry(name: &str) -> MapMenuEntry {
    test_map_entry_with_starts(name, 8)
}

fn test_map_entry_with_starts(name: &str, start_count: usize) -> MapMenuEntry {
    MapMenuEntry {
        file_name: name.to_string(),
        display_name: name.to_string(),
        author: None,
        briefing: BriefingSection::default(),
        preview: PreviewSection::default(),
        multiplayer_start_waypoints: (0..start_count)
            .map(|idx| Waypoint {
                index: idx as u32,
                rx: idx as u16,
                ry: idx as u16,
            })
            .collect(),
        player_capacity: i32::try_from(start_count).expect("test capacity fits i32"),
        preview_source_bounds: None,
    }
}

fn test_scenario_record(
    source_ordinal: usize,
    name: &str,
    game_modes: &str,
) -> SkirmishScenarioRecord {
    // Loose-map headers spell the mode-filter key `GameMode`, singular.
    let ini = IniFile::from_str(&format!("[Basic]\nName={name}\nGameMode={game_modes}\n"));
    SkirmishScenarioRecord::concrete_from_ini(
        source_ordinal,
        SkirmishScenarioSource::LooseYrm(format!("{name}.yrm")),
        &format!("{name}.yrm"),
        &ini,
    )
}

#[test]
fn hit_test_start_choose_and_back() {
    let layout = compute_layout(800, 600);
    assert_eq!(
        hit_test(
            &layout,
            layout.start_button.x + 1,
            layout.start_button.y + 1
        ),
        SkirmishShellAction::StartGame
    );
    assert_eq!(
        hit_test(
            &layout,
            layout.choose_map_button.x + 1,
            layout.choose_map_button.y + 1
        ),
        SkirmishShellAction::ChooseMap
    );
    assert_eq!(
        hit_test(&layout, layout.back_button.x + 1, layout.back_button.y + 1),
        SkirmishShellAction::BackOrExit
    );
}

#[test]
fn hit_test_uses_exclusive_bottom_right_edges() {
    let layout = compute_layout(800, 600);
    assert_eq!(
        hit_test(
            &layout,
            layout.start_button.x + layout.start_button.w,
            layout.start_button.y
        ),
        SkirmishShellAction::None
    );
    assert_eq!(
        hit_test(
            &layout,
            layout.back_button.x,
            layout.back_button.y + layout.back_button.h
        ),
        SkirmishShellAction::None
    );
}

#[test]
fn choose_map_action_bubbles_without_cycling_selected_map() {
    let maps = [test_map_entry("a.mmx"), test_map_entry("b.mmx")];
    let mut shell = SkirmishShellState::default();
    shell.selected_map_idx = 0;

    assert_eq!(
        apply_action(&mut shell, SkirmishShellAction::ChooseMap, &maps),
        SkirmishShellAction::ChooseMap
    );
    assert_eq!(shell.selected_map_idx, 0);
}

#[test]
fn choose_map_modal_open_filters_and_highlights_current_record() {
    let modes = stock_skirmish_modes();
    let records = vec![
        test_scenario_record(0, "first", "standard"),
        test_scenario_record(1, "second", "standard"),
        test_scenario_record(2, "team", "teamgame"),
    ];

    let layout = compute_choose_map_modal_layout(800, 600);
    let modal = ChooseMapModalState::open(1, Some(1), &modes, &records, &layout);

    assert_eq!(modal.selected_mode_id, 1);
    assert_eq!(modal.filtered_record_indices, vec![0, 1]);
    assert_eq!(modal.highlighted_filtered_index, Some(1));
    assert_eq!(modal.selected_record_index(), Some(1));
    assert_eq!(modal.pressed_button, None);
}

#[test]
fn choose_map_modal_select_mode_rebuilds_map_list_by_filter() {
    let modes = stock_skirmish_modes();
    let records = vec![
        test_scenario_record(0, "battle", "standard"),
        test_scenario_record(1, "team", "teamgame"),
        test_scenario_record(2, "duel", "duel"),
    ];
    let layout = compute_choose_map_modal_layout(800, 600);
    let mut modal = ChooseMapModalState::open(1, Some(0), &modes, &records, &layout);

    assert!(modal.select_mode(9, &modes, &records, &layout));

    assert_eq!(modal.selected_mode_id, 9);
    assert_eq!(modal.filtered_record_indices, vec![1]);
    assert_eq!(modal.selected_record_index(), Some(1));
    assert_eq!(modal.map_top_index, 0);
}

#[test]
fn choose_map_modal_cancel_restores_saved_selection_accept_uses_highlight() {
    let modes = stock_skirmish_modes();
    let records = vec![
        test_scenario_record(0, "first", "standard"),
        test_scenario_record(1, "second", "standard"),
    ];
    let layout = compute_choose_map_modal_layout(800, 600);
    let mut modal = ChooseMapModalState::open(1, Some(0), &modes, &records, &layout);

    modal.highlighted_filtered_index = Some(1);

    assert_eq!(
        modal.accept_selection(),
        Some(ChooseMapSelection {
            mode_id: 1,
            record_index: Some(1),
        })
    );
    assert_eq!(
        modal.cancel_selection(),
        ChooseMapSelection {
            mode_id: 1,
            record_index: Some(0),
        }
    );
}

#[test]
fn choose_map_modal_random_map_command_is_mode_gated_and_single_record() {
    let modes = stock_skirmish_modes();
    let mut records = vec![test_scenario_record(0, "battle", "standard")];
    let layout = compute_choose_map_modal_layout(800, 600);
    let mut modal = ChooseMapModalState::open(1, Some(0), &modes, &records, &layout);

    let first_random = modal
        .create_random_map(&mut records, &modes, "Random Map", 4, &layout)
        .expect("battle allows random maps");
    let second_random = modal
        .create_random_map(&mut records, &modes, "Random Map", 4, &layout)
        .expect("battle still allows random maps");

    assert_eq!(first_random, second_random);
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[first_random].kind,
        SkirmishScenarioKind::RandomMapSentinel
    );
    assert_eq!(modal.selected_record_index(), Some(first_random));

    assert!(modal.select_mode(9, &modes, &records, &layout));
    assert_eq!(
        modal.create_random_map(&mut records, &modes, "Nope", 4, &layout),
        None
    );
    assert_eq!(records.len(), 2);
}

#[test]
fn owner_draw_button_hit_test_returns_control_identity() {
    let layout = compute_layout(800, 600);
    assert_eq!(
        hit_test_owner_draw_button(
            &layout,
            layout.start_button.x + 1,
            layout.start_button.y + 1
        ),
        Some(OwnerDrawButton::StartGame0x617)
    );
    assert_eq!(
        hit_test_owner_draw_button(
            &layout,
            layout.choose_map_button.x + 1,
            layout.choose_map_button.y + 1
        ),
        Some(OwnerDrawButton::ChooseMap0x5aa)
    );
    assert_eq!(
        hit_test_owner_draw_button(&layout, layout.back_button.x + 1, layout.back_button.y + 1),
        Some(OwnerDrawButton::Back0x5c0)
    );
    assert_eq!(
        hit_test_owner_draw_button(
            &layout,
            layout.start_button.x + layout.start_button.w,
            layout.start_button.y
        ),
        None
    );
}

/// The first window row a press lands on (`y > bottom - 18`, `0x0061E505`).
fn first_admitted_row(rect: RectPx) -> i32 {
    rect.y + rect.h - 17
}

/// A Skirmish state with the slider ranges retail Rules gives through the
/// production reader, or `None` without the retail INIs.
fn retail_ranges_shell() -> Option<SkirmishShellState> {
    let rules = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini")?;
    let mut shell = SkirmishShellState::default();
    shell.trackbar_bounds = SkirmishTrackbarBounds::from_multiplayer_dialog_settings(&rules);
    Some(shell)
}

/// The screen x of the Credits thumb's left edge.
fn credits_thumb_x(shell: &SkirmishShellState, rect: RectPx) -> i32 {
    let range = shell
        .trackbar_bounds
        .range(SkirmishTrackbarId::Credits0x511);
    rect.x + range.thumb_left(shell.starting_credits, rect.w)
}

/// The original instructions' Credits samples for the runtime window
/// (`tools/storage_oracle/launcher_trackbar.py`): `(x, position)` pairs.
fn native_credits_pointers() -> Vec<(i32, i32)> {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tools/storage_oracle/launcher_trackbar.json"
    ))
    .unwrap();
    let geometry = golden["geometries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["width"] == 129 && g["maximum"] == 5000)
        .expect("runtime Credits fixture");
    geometry["pointers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["x"].as_i64().unwrap() as i32,
                p["position"].as_i64().unwrap() as i32,
            )
        })
        .collect()
}

#[test]
fn credits_presses_and_drags_follow_the_native_pointer_partition() {
    let Some(retail) = retail_ranges_shell() else {
        return;
    };
    // The native samples ran with retail Rules' Credits range.
    let range = retail
        .trackbar_bounds
        .range(SkirmishTrackbarId::Credits0x511);
    assert_eq!(range, TrackbarRange::new(5000, 10000, 100));
    let layout = compute_layout(800, 600);
    let rect = layout.trackbars.credits;
    let y = first_admitted_row(rect);
    let thumb = credits_thumb_x(&retail, rect) - rect.x;
    let samples = native_credits_pointers();
    for &(x, position) in &samples {
        let mut shell = retail.clone();
        let before = shell.starting_credits;
        handle_option_mouse_down(&mut shell, &layout, &[], rect.x + x, y);
        if !(0..rect.w).contains(&x) {
            assert_eq!(shell.starting_credits, before, "x={x} is off the window");
        } else if (thumb..thumb + THUMB_W).contains(&x) {
            assert_eq!(shell.starting_credits, before, "x={x} grabs the thumb");
        } else {
            assert_eq!(shell.starting_credits, range.min + position, "x={x}");
        }
    }
    // A grabbed thumb follows the pointer anywhere along the row.
    let mut shell = retail;
    handle_option_mouse_down(&mut shell, &layout, &[], rect.x + thumb, y);
    for &(x, position) in &samples {
        handle_option_mouse_move(&mut shell, &layout, &[], rect.x + x, y);
        assert_eq!(shell.starting_credits, range.min + position, "drag x={x}");
    }
}

#[test]
fn default_shell_options_use_launch_defaults() {
    let shell = SkirmishShellState::default();
    let options = SkirmishLaunchOptions::default();

    assert_eq!(shell.starting_credits, options.starting_credits);
    assert_eq!(shell.game_speed, options.game_speed);
    assert_eq!(shell.unit_count, options.unit_count);
    assert_eq!(shell.short_game, options.short_game);
    assert_eq!(shell.super_weapons, options.super_weapons);
    assert_eq!(shell.build_off_ally, options.build_off_ally);
    assert_eq!(shell.crates, options.crates);
    assert_eq!(shell.mcv_redeploy, options.mcv_redeploy);
}

#[test]
fn player_name_focus_selects_all_and_typing_replaces_default() {
    let mut shell = SkirmishShellState::default();

    assert!(focus_player_name_edit(&mut shell));
    assert_eq!(shell.player_name_edit.selection, Some((0, 6)));
    assert_eq!(shell.player_name_edit.caret, 6);

    assert!(insert_player_name_text(&mut shell, "Commander"));
    assert_eq!(shell.player_name_edit.text, "Commander");
    assert_eq!(shell.player_name_edit.selection, None);
    assert_eq!(shell.player_name_edit.caret, 9);
}

#[test]
fn player_name_insert_caps_at_nineteen_ascii_bytes() {
    let mut shell = SkirmishShellState::default();
    focus_player_name_edit(&mut shell);

    assert!(insert_player_name_text(
        &mut shell,
        "12345678901234567890extra"
    ));

    assert_eq!(shell.player_name_edit.text, "1234567890123456789");
    assert_eq!(
        shell.player_name_edit.text.chars().count(),
        PLAYER_NAME_EDIT_LIMIT_BYTES
    );
    assert_eq!(shell.player_name_edit.caret, PLAYER_NAME_EDIT_LIMIT_BYTES);
}

#[test]
fn player_name_crosses_only_the_platform_acp_boundary() {
    let input = "Aé€Ж😀";
    let expected = crate::util::native_string::acp_round_trip(input);
    let state = PlayerNameEditState::with_name(input);

    assert_eq!(state.text, expected);
}

#[test]
fn player_name_user_entry_stops_at_nineteen_acp_bytes() {
    let mut state = PlayerNameEditState::with_name("");
    assert!(state.insert_text(&"é".repeat(PLAYER_NAME_EDIT_LIMIT_BYTES)));

    assert!(
        crate::util::native_string::acp_encode(&state.text).len() <= PLAYER_NAME_EDIT_LIMIT_BYTES
    );
    assert!(
        crate::util::native_string::acp_encode(&(state.text.clone() + "é")).len()
            > PLAYER_NAME_EDIT_LIMIT_BYTES
    );
}

#[test]
fn player_name_enter_and_tab_do_not_insert_control_text() {
    let mut shell = SkirmishShellState::default();
    focus_player_name_edit(&mut shell);
    insert_player_name_text(&mut shell, "Ace");

    assert!(!insert_player_name_text(&mut shell, "\r\n\t"));
    assert_eq!(shell.player_name_edit.text, "Ace");
    assert!(!shell.player_name_edit.text.contains('\r'));
    assert!(!shell.player_name_edit.text.contains('\n'));
    assert!(!shell.player_name_edit.text.contains('\t'));
}

#[test]
fn player_name_backspace_and_delete_remove_selection_first() {
    let mut shell = SkirmishShellState::default();
    focus_player_name_edit(&mut shell);
    insert_player_name_text(&mut shell, "Alpha");
    shell.player_name_edit.selection = Some((1, 4));
    shell.player_name_edit.caret = 4;

    assert!(handle_player_name_backspace(&mut shell));
    assert_eq!(shell.player_name_edit.text, "Aa");
    assert_eq!(shell.player_name_edit.caret, 1);

    shell.player_name_edit.selection = Some((0, 1));
    shell.player_name_edit.caret = 1;
    assert!(handle_player_name_delete(&mut shell));
    assert_eq!(shell.player_name_edit.text, "a");
    assert_eq!(shell.player_name_edit.caret, 0);
}

#[test]
fn player_name_scroll_keeps_caret_visible_with_five_pixel_margin() {
    let mut shell = SkirmishShellState::default();
    shell.player_name_edit.text = "1234567890123456789".to_string();
    shell.player_name_edit.caret = PLAYER_NAME_EDIT_LIMIT_BYTES;

    assert!(update_player_name_scroll_for_caret(&mut shell, 50, 120));
    assert_eq!(shell.player_name_edit.scroll_x, 75);

    shell.player_name_edit.caret = 0;
    assert!(update_player_name_scroll_for_caret(&mut shell, 50, 0));
    assert_eq!(shell.player_name_edit.scroll_x, 0);
}

#[test]
fn player_name_tab_does_not_insert_control_text_and_leaves_edit_focus() {
    let mut shell = SkirmishShellState::default();
    focus_player_name_edit(&mut shell);
    insert_player_name_text(&mut shell, "Ace");

    assert!(handle_player_name_tab(&mut shell));
    assert!(!shell.player_name_edit.focused);
    assert_eq!(shell.player_name_edit.text, "Ace");
    assert!(!shell.player_name_edit.text.contains('\t'));
}

#[test]
fn player_name_focus_survives_status_hover_update() {
    let mut shell = SkirmishShellState::default();
    focus_player_name_edit(&mut shell);

    assert!(set_status_help_text(&mut shell, "Start Game"));

    assert!(shell.player_name_edit.focused);
    assert_eq!(shell.player_name_edit.selection, Some((0, 6)));
}

#[test]
fn player_name_focus_survives_dropdown_open_close_until_explicit_blur() {
    let mut shell = SkirmishShellState::default();
    focus_player_name_edit(&mut shell);

    shell.open_combo_dropdown = Some(OpenComboDropdown {
        id: SkirmishComboId::Side(0),
        top_index: 0,
    });
    shell.open_combo_dropdown = None;

    assert!(shell.player_name_edit.focused);
    assert!(blur_player_name_edit(&mut shell));
    assert!(!shell.player_name_edit.focused);
}

#[test]
fn hovered_shell_control_resolves_core_controls_and_dropdown_rows() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let maps = vec![test_map_entry("arena.map")];

    assert_eq!(
        hovered_shell_control(
            &layout,
            &shell,
            &maps,
            layout.start_button.x,
            layout.start_button.y
        ),
        Some(SkirmishHoverTarget::OwnerDrawButton(
            OwnerDrawButton::StartGame0x617
        ))
    );
    assert_eq!(
        hovered_shell_control(
            &layout,
            &shell,
            &maps,
            layout.player_name.x,
            layout.player_name.y
        ),
        Some(SkirmishHoverTarget::PlayerName0x6a0)
    );
    assert_eq!(
        hovered_shell_control(
            &layout,
            &shell,
            &maps,
            layout.status_help.x,
            layout.status_help.y
        ),
        Some(SkirmishHoverTarget::StatusHelp0x695)
    );
    assert_eq!(
        hovered_shell_control(
            &layout,
            &shell,
            &maps,
            layout.checkboxes[0].rect.x,
            layout.checkboxes[0].rect.y
        ),
        Some(SkirmishHoverTarget::Checkbox(
            SkirmishCheckboxId::ShortGame0x54e
        ))
    );
    assert_eq!(
        hovered_shell_control(
            &layout,
            &shell,
            &maps,
            layout.trackbars.credits.x,
            layout.trackbars.credits.y
        ),
        Some(SkirmishHoverTarget::Trackbar(
            SkirmishTrackbarId::Credits0x511
        ))
    );
    assert_eq!(
        hovered_shell_control(
            &layout,
            &shell,
            &maps,
            layout.rows.side_combos[0].x,
            layout.rows.side_combos[0].y
        ),
        Some(SkirmishHoverTarget::ComboFace(SkirmishComboId::Side(0)))
    );

    shell.open_combo_dropdown = Some(OpenComboDropdown {
        id: SkirmishComboId::AiType(0),
        top_index: 0,
    });
    let content =
        combo_dropdown_content_rect(&shell, &layout, &maps, SkirmishComboId::AiType(0)).unwrap();
    assert_eq!(
        hovered_shell_control(
            &layout,
            &shell,
            &maps,
            content.x + 1,
            content.y + COMBO_DROPDOWN_ROW_H + 1,
        ),
        Some(SkirmishHoverTarget::ComboItem {
            id: SkirmishComboId::AiType(0),
            item: SkirmishComboItem::AiType(SkirmishAiRowType::Easy),
        })
    );
}

#[test]
fn skirmish_status_help_includes_flag_and_right_panel_static_targets() {
    let layout = compute_layout(800, 600);
    let shell = SkirmishShellState::default();
    let maps = vec![test_map_entry("arena.map")];

    // Flag picture controls 0x6DA..0x6E1 -> STT:SkirmishPictureFlag.
    for flag in &layout.flags {
        let target = hovered_shell_control(&layout, &shell, &maps, flag.x, flag.y);
        assert_eq!(target, Some(SkirmishHoverTarget::FlagPicture));
        assert_eq!(
            status_help_key_for_hover(target.unwrap()),
            Some("STT:SkirmishPictureFlag")
        );
    }

    // Right-panel game-type label 0x6EC -> STT:SkirmishLabelGameType.
    let game_type = layout.right_panel_text.game_type;
    let game_type_target = hovered_shell_control(&layout, &shell, &maps, game_type.x, game_type.y);
    assert_eq!(
        game_type_target,
        Some(SkirmishHoverTarget::GameTypeLabel0x6ec)
    );
    assert_eq!(
        status_help_key_for_hover(game_type_target.unwrap()),
        Some("STT:SkirmishLabelGameType")
    );

    // Right-panel scenario/map label 0x5A8 -> STT:SkirmishLabelScenario.
    let map_label = layout.right_panel_text.map_label;
    let map_label_target = hovered_shell_control(&layout, &shell, &maps, map_label.x, map_label.y);
    assert_eq!(
        map_label_target,
        Some(SkirmishHoverTarget::ScenarioLabel0x5a8)
    );
    assert_eq!(
        status_help_key_for_hover(map_label_target.unwrap()),
        Some("STT:SkirmishLabelScenario")
    );
}

#[test]
fn hovered_shell_control_blocks_parent_targets_when_modal_owns_input() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let chooser = compute_choose_map_modal_layout(800, 600);
    shell.choose_map_modal = Some(ChooseMapModalState::open(1, None, &[], &[], &chooser));

    assert_eq!(
        hovered_shell_control(
            &layout,
            &shell,
            &[],
            layout.start_button.x,
            layout.start_button.y
        ),
        None
    );
}

#[test]
fn status_help_keys_use_verified_stt_mappings() {
    assert_eq!(
        status_help_key_for_hover(SkirmishHoverTarget::OwnerDrawButton(
            OwnerDrawButton::StartGame0x617
        )),
        Some("STT:SkirmishButtonStartGame")
    );
    assert_eq!(
        status_help_key_for_hover(SkirmishHoverTarget::PlayerName0x6a0),
        Some("STT:SkirmishEditPlayer")
    );
    assert_eq!(
        status_help_key_for_hover(SkirmishHoverTarget::StatusHelp0x695),
        None
    );
    assert_eq!(
        status_help_key_for_hover(SkirmishHoverTarget::Checkbox(
            SkirmishCheckboxId::ShortGame0x54e
        )),
        Some("STT:SkirmishCBoxShortGame")
    );
    assert_eq!(
        status_help_key_for_hover(SkirmishHoverTarget::Trackbar(
            SkirmishTrackbarId::Credits0x511
        )),
        Some("STT:SkirmishSliderCredits")
    );
    assert_eq!(
        status_help_key_for_hover(SkirmishHoverTarget::MapPreview0x468),
        Some("STT:SkirmishMapThumbnail")
    );
    assert_eq!(
        status_help_key_for_hover(SkirmishHoverTarget::ComboFace(SkirmishComboId::Side(0))),
        Some("STT:SkirmishComboCountry")
    );
    assert_eq!(
        status_help_key_for_hover(SkirmishHoverTarget::ComboFace(SkirmishComboId::Start(0))),
        Some("STT:HostComboStart")
    );
    assert_eq!(
        status_help_key_for_hover(SkirmishHoverTarget::ComboFace(SkirmishComboId::Team(0))),
        Some("STT:HostComboTeam")
    );
}

#[test]
fn status_help_ai_row_state_uses_item_specific_stt() {
    for (row_type, key) in [
        (SkirmishAiRowType::None, "STT:PlayerNone"),
        (SkirmishAiRowType::Easy, "STT:PlayerDumbAI"),
        (SkirmishAiRowType::Normal, "STT:PlayerSmartAI"),
        (SkirmishAiRowType::Hard, "STT:PlayerGeniusAI"),
    ] {
        assert_eq!(
            status_help_key_for_hover(SkirmishHoverTarget::ComboItem {
                id: SkirmishComboId::AiType(0),
                item: SkirmishComboItem::AiType(row_type),
            }),
            Some(key)
        );
    }
}

#[test]
fn status_help_side_row_uses_item_specific_stt() {
    for (country, key) in [
        (SkirmishCountryChoice::Random, "STT:PlayerSideRandom"),
        (
            SkirmishCountryChoice::Country(crate::ui::main_menu::SkirmishCountry::America),
            "STT:PlayerSideAmerica",
        ),
        (
            SkirmishCountryChoice::Country(crate::ui::main_menu::SkirmishCountry::GreatBritain),
            "STT:PlayerSideBritain",
        ),
        (
            SkirmishCountryChoice::Country(crate::ui::main_menu::SkirmishCountry::Yuri),
            "STT:PlayerSideYuriCountry",
        ),
    ] {
        assert_eq!(
            status_help_key_for_hover(SkirmishHoverTarget::ComboItem {
                id: SkirmishComboId::Side(0),
                item: SkirmishComboItem::Country(country),
            }),
            Some(key)
        );
    }
}

#[test]
fn status_help_color_row_uses_item_specific_stt_with_generic_miss_fallback() {
    for (item, key) in [
        (
            SkirmishComboItem::ColorSentinel(-2),
            "STT:PlayerColorRandom",
        ),
        (SkirmishComboItem::Color(0), "STT:PlayerColorGold"),
        (SkirmishComboItem::Color(5), "STT:PlayerColorSkyBlue"),
        (SkirmishComboItem::Color(7), "STT:PlayerColorPink"),
        (SkirmishComboItem::Color(8), "STT:PlayerColorObserver"),
    ] {
        assert_eq!(
            status_help_key_for_hover(SkirmishHoverTarget::ComboItem {
                id: SkirmishComboId::Color(0),
                item,
            }),
            Some(key)
        );
    }

    assert_eq!(
        status_help_key_for_hover(SkirmishHoverTarget::ComboItem {
            id: SkirmishComboId::Color(0),
            item: SkirmishComboItem::Color(9),
        }),
        Some("STT:SkirmishComboColor")
    );
    assert_eq!(
        status_help_key_for_hover(SkirmishHoverTarget::ComboFace(SkirmishComboId::Color(0))),
        Some("STT:SkirmishComboColor")
    );
}

#[test]
fn hovered_choose_map_modal_control_resolves_0x6b_status_targets() {
    let layout = compute_choose_map_modal_layout(800, 600);
    let modes = stock_skirmish_modes();
    let records = vec![test_scenario_record(0, "arena", "standard")];
    let modal = ChooseMapModalState::open(1, Some(0), &modes, &records, &layout);

    assert_eq!(
        hovered_choose_map_modal_control(
            &layout,
            &modal,
            layout.use_map_button.x,
            layout.use_map_button.y
        ),
        Some(ChooseMapHoverTarget::Button(
            ChooseMapModalButton::UseMap0x6c5
        ))
    );
    assert_eq!(
        hovered_choose_map_modal_control(&layout, &modal, layout.preview.x, layout.preview.y),
        Some(ChooseMapHoverTarget::Preview0x468)
    );
    assert_eq!(
        hovered_choose_map_modal_control(
            &layout,
            &modal,
            layout.status_help.x,
            layout.status_help.y
        ),
        Some(ChooseMapHoverTarget::StatusHelp0x695)
    );
    assert_eq!(
        hovered_choose_map_modal_control(
            &layout,
            &modal,
            layout.mode_list.x + 1,
            layout.mode_list.y + 1
        ),
        Some(ChooseMapHoverTarget::ModeListRow0x6eb { mode_id: 1 })
    );
    assert_eq!(
        hovered_choose_map_modal_control(
            &layout,
            &modal,
            layout.map_list.x + 1,
            layout.map_list.y + 1
        ),
        Some(ChooseMapHoverTarget::MapList0x553)
    );
}

#[test]
fn choose_map_status_help_keys_use_verified_0x6b_mapping() {
    assert_eq!(
        status_help_key_for_choose_map_hover(ChooseMapHoverTarget::ModeList0x6eb),
        Some("STT:ScenarioListGameType")
    );
    assert_eq!(
        status_help_key_for_choose_map_hover(ChooseMapHoverTarget::MapList0x553),
        Some("STT:ScenarioListMaps")
    );
    assert_eq!(
        status_help_key_for_choose_map_hover(ChooseMapHoverTarget::Preview0x468),
        Some("STT:ScenarioMapThumbnail")
    );
    assert_eq!(
        status_help_key_for_choose_map_hover(ChooseMapHoverTarget::Button(
            ChooseMapModalButton::UseMap0x6c5
        )),
        Some("STT:ScenarioButtonUseMap")
    );
    assert_eq!(
        status_help_key_for_choose_map_hover(ChooseMapHoverTarget::Button(
            ChooseMapModalButton::CreateRandomMap0x583
        )),
        Some("STT:ScenarioButtonRandom")
    );
    assert_eq!(
        status_help_key_for_choose_map_hover(ChooseMapHoverTarget::Button(
            ChooseMapModalButton::Cancel0x5c0
        )),
        Some("STT:ScenarioButtonCancel")
    );
    assert_eq!(
        status_help_key_for_choose_map_hover(ChooseMapHoverTarget::StatusHelp0x695),
        None
    );
}

#[test]
fn checkbox_icon_click_toggles_but_label_click_does_not() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let checkbox = layout.checkboxes[0];
    let initial = shell.short_game;

    assert_eq!(
        handle_option_mouse_down(&mut shell, &layout, &[], checkbox.rect.x, checkbox.rect.y),
        SkirmishShellAction::None
    );
    assert_eq!(shell.short_game, !initial);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiCheckboxSound]
    );

    let label = checkbox_text_rect(checkbox.rect);
    handle_option_mouse_down(&mut shell, &layout, &[], label.x, label.y + 1);
    assert_eq!(shell.short_game, !initial);
    assert!(shell.drain_pending_ui_sounds().is_empty());
}

#[test]
fn a_press_above_the_admitted_strip_only_holds_the_mouse() {
    let layout = compute_layout(800, 600);
    let rect = layout.trackbars.credits;
    for y in [rect.y, first_admitted_row(rect) - 1] {
        let mut shell = SkirmishShellState::default();
        handle_option_mouse_down(&mut shell, &layout, &[], rect.x + 39, y);
        handle_option_mouse_move(&mut shell, &layout, &[], rect.x, first_admitted_row(rect));
        assert_eq!(shell.starting_credits, CREDITS_MAX, "y={y}");
        assert_eq!(
            shell.trackbar_hold,
            Some(TrackbarHold {
                id: SkirmishTrackbarId::Credits0x511,
                dragging: false,
            })
        );
        assert!(shell.drain_pending_ui_sounds().is_empty());
    }
}

#[test]
fn trackbar_outside_thumb_click_remaps_value_and_keeps_capture() {
    let layout = compute_layout(800, 600);
    let Some(mut shell) = retail_ranges_shell() else {
        return;
    };
    let rect = layout.trackbars.credits;

    handle_option_mouse_down(
        &mut shell,
        &layout,
        &[],
        rect.x + 39,
        first_admitted_row(rect),
    );
    assert_eq!(shell.starting_credits, 7400);
    assert_eq!(
        shell.trackbar_hold,
        Some(TrackbarHold {
            id: SkirmishTrackbarId::Credits0x511,
            dragging: false,
        })
    );
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GenericClick]
    );
}

#[test]
fn a_zero_money_increment_slides_credits_in_ones() {
    // Every message turns a zero step into 1 (`0x0061DB94..0x0061DBAD`).
    let ini = IniFile::from_str("[MultiplayerDialogSettings]\nMoneyIncrement=0\n");
    let mut shell = SkirmishShellState::default();
    shell.trackbar_bounds = SkirmishTrackbarBounds::from_multiplayer_dialog_settings(&ini);
    let layout = compute_layout(800, 600);
    let rect = layout.trackbars.credits;
    handle_option_mouse_down(
        &mut shell,
        &layout,
        &[],
        rect.x + 39,
        first_admitted_row(rect),
    );
    let range = shell
        .trackbar_bounds
        .range(SkirmishTrackbarId::Credits0x511);
    assert_eq!(range.step, 1);
    assert_eq!(shell.starting_credits, range.value_at(39, rect.w));
    assert_eq!(shell.credits(), shell.starting_credits);
}

#[test]
fn the_game_starts_with_the_credits_the_slider_reads_back() {
    // `TBM_GETPOS` (`0x0061E4AD`) rounds down to MoneyIncrement; the thumb
    // keeps the saved value.
    let Some(mut shell) = retail_ranges_shell() else {
        return;
    };
    shell.starting_credits = 7549;
    assert_eq!(shell.credits(), 7500);
    assert_eq!(launch_settings(&shell).starting_credits, 7500);
}

#[test]
fn trackbar_thumb_hit_starts_drag() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let rect = layout.trackbars.credits;
    let thumb_x = credits_thumb_x(&shell, rect);
    let y = first_admitted_row(rect);

    handle_option_mouse_down(&mut shell, &layout, &[], thumb_x, y);
    assert_eq!(
        shell.trackbar_hold,
        Some(TrackbarHold {
            id: SkirmishTrackbarId::Credits0x511,
            dragging: true,
        })
    );
    assert!(shell.drain_pending_ui_sounds().is_empty());
}

#[test]
fn trackbar_mouse_move_updates_while_capture_active() {
    let layout = compute_layout(800, 600);
    let Some(mut shell) = retail_ranges_shell() else {
        return;
    };
    let rect = layout.trackbars.credits;
    let thumb_x = credits_thumb_x(&shell, rect);
    let y = first_admitted_row(rect);

    handle_option_mouse_down(&mut shell, &layout, &[], thumb_x, y);
    handle_option_mouse_move(&mut shell, &layout, &[], rect.x - 100, y);
    assert_eq!(shell.starting_credits, 5000);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GenericClick]
    );

    handle_option_mouse_up(&mut shell);
    handle_option_mouse_move(&mut shell, &layout, &[], rect.x + 1000, y);
    assert_eq!(shell.starting_credits, 5000);
    assert_eq!(shell.trackbar_hold, None);
    assert!(shell.drain_pending_ui_sounds().is_empty());
}

#[test]
fn trackbar_rail_click_jumps_once_and_does_not_track_cursor() {
    let layout = compute_layout(800, 600);
    let Some(mut shell) = retail_ranges_shell() else {
        return;
    };
    let rect = layout.trackbars.credits;

    handle_option_mouse_down(&mut shell, &layout, &[], 443, 319);
    // A rail click captures the pointer but is NOT a thumb grab, so the
    // following move must not retrack the value to the cursor.
    handle_option_mouse_move(&mut shell, &layout, &[], 470, 319);

    assert_eq!(shell.starting_credits, 7400);
    assert_eq!(
        shell.trackbar_hold,
        Some(TrackbarHold {
            id: SkirmishTrackbarId::Credits0x511,
            dragging: false,
        })
    );
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GenericClick]
    );

    handle_option_mouse_up(&mut shell);
    handle_option_mouse_move(&mut shell, &layout, &[], rect.x - 100, 319);
    assert_eq!(shell.starting_credits, 7400);
    assert_eq!(shell.trackbar_hold, None);
    assert!(shell.drain_pending_ui_sounds().is_empty());
}

#[test]
fn trackbar_repeated_drag_same_value_is_silent() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let rect = layout.trackbars.credits;
    let thumb_x = credits_thumb_x(&shell, rect);
    let y = first_admitted_row(rect);

    let max_value_x = thumb_x + THUMB_W / 2;
    handle_option_mouse_down(&mut shell, &layout, &[], max_value_x, y);
    handle_option_mouse_move(&mut shell, &layout, &[], max_value_x, y);

    assert_eq!(
        shell.starting_credits,
        SkirmishLaunchOptions::default().starting_credits
    );
    assert!(shell.drain_pending_ui_sounds().is_empty());
}

#[test]
fn game_speed_visual_position_inverts_stored_value() {
    assert_eq!(game_speed_visual_position(1), 5);
    assert_eq!(game_speed_from_visual_position(5), 1);
}

#[test]
fn launch_settings_preserves_current_load_contract() {
    let shell = SkirmishShellState::default();
    let settings = launch_settings(&shell);
    assert_eq!(settings.selected_map_idx, shell.selected_map_idx);
    assert_eq!(settings.starting_credits, shell.starting_credits);
    assert_eq!(settings.short_game, shell.short_game);
}

#[test]
fn default_shell_tracks_native_slot_count() {
    let shell = SkirmishShellState::default();
    assert_eq!(shell.opponents.len(), SKIRMISH_AI_SLOT_COUNT);
    assert_eq!(shell.opponents.iter().filter(|o| o.is_active()).count(), 1);
    assert_eq!(shell.player_team, -2);
    assert_eq!(shell.opponents[0].team, 3);
}

#[test]
fn ai_row_type_uses_verified_item_data_order() {
    assert_eq!(SkirmishAiRowType::None.item_data(), -1);
    assert_eq!(SkirmishAiRowType::Easy.item_data(), 2);
    assert_eq!(SkirmishAiRowType::Normal.item_data(), 1);
    assert_eq!(SkirmishAiRowType::Hard.item_data(), 0);
    assert!(!SkirmishAiRowType::None.is_active());
    assert!(SkirmishAiRowType::Hard.is_active());
}

#[test]
fn launch_session_packs_selected_map_and_enabled_slots() {
    let mut shell = SkirmishShellState::default();
    shell.selected_map_idx = 1;
    shell.player_name_edit.text = "Commander".to_string();
    shell.player_country = SkirmishCountry::Korea;
    shell.player_color_index = 3;
    shell.player_start_position = StartPosition::Position(2);
    shell.player_team = 0;
    shell.starting_credits = 7400;
    shell.unit_count = 4;
    shell.game_speed = game_speed_from_visual_position(3);
    shell.short_game = false;
    shell.super_weapons = false;
    shell.build_off_ally = false;
    shell.crates = false;
    shell.mcv_redeploy = false;
    shell.opponents[0].country = SkirmishCountry::Yuri;
    shell.opponents[0].color_index = 6;
    shell.opponents[0].start_position = StartPosition::Position(4);
    shell.opponents[0].team = 1;
    shell.opponents[0].row_type = SkirmishAiRowType::Hard;

    let maps = [test_map_entry("first.mmx"), test_map_entry("second.mmx")];
    let modes = stock_skirmish_modes();
    let session = launch_session(&shell, &maps, &modes).expect("session");

    assert_eq!(session.mode.id, 1);
    assert_eq!(session.selected_map_file.as_deref(), Some("second.mmx"));
    assert_eq!(session.player_name, "Commander");
    assert_eq!(session.local.country, LaunchCountry::Korea);
    assert_eq!(session.local.color_index, 3);
    assert_eq!(
        session.local.start_position,
        LaunchStartPosition::Position(2)
    );
    assert_eq!(session.local.team, LaunchTeam::Team(0));
    assert_eq!(session.opponents.len(), 1);
    assert_eq!(session.opponents[0].country, LaunchCountry::Yuri);
    assert_eq!(session.opponents[0].color_index, 6);
    assert_eq!(session.opponents[0].difficulty, AiDifficulty::Hard);
    assert_eq!(session.options.starting_credits, shell.starting_credits);
    assert_eq!(session.options.unit_count, shell.unit_count);
    assert_eq!(session.options.game_speed, shell.game_speed);
    assert_eq!(session.options.short_game, shell.short_game);
    assert_eq!(session.options.super_weapons, shell.super_weapons);
    assert_eq!(session.options.build_off_ally, shell.build_off_ally);
    assert_eq!(session.options.crates, shell.crates);
    assert_eq!(session.options.mcv_redeploy, shell.mcv_redeploy);
}

#[test]
fn launch_session_preserves_build_off_ally_default() {
    let shell = SkirmishShellState::default();
    let maps = [test_map_entry("map.mmx")];
    let modes = stock_skirmish_modes();
    let session = launch_session(&shell, &maps, &modes).expect("session");

    assert!(session.options.build_off_ally);
}

#[test]
fn launch_session_preserves_selected_team_game_mode_id() {
    let mut shell = SkirmishShellState {
        selected_mode_id: 9,
        ..Default::default()
    };
    let maps = [test_map_entry("team.yrm")];
    let modes = stock_skirmish_modes();
    repair_teams_for_selected_mode(&mut shell, &modes);

    let session = launch_session(&shell, &maps, &modes).expect("session");

    assert_eq!(session.mode.id, 9);
    assert_eq!(session.mode.ui_name_key, "GUI:TeamGame");
    assert!(session.mode.must_ally);
}

#[test]
fn launch_session_preserves_selected_ffa_mode_id() {
    let shell = SkirmishShellState {
        selected_mode_id: 2,
        ..Default::default()
    };
    let maps = [test_map_entry("ffa.yrm")];
    let modes = stock_skirmish_modes();

    let session = launch_session(&shell, &maps, &modes).expect("session");

    assert_eq!(session.mode.id, 2);
    assert_eq!(session.mode.ui_name_key, "GUI:FreeForAll");
    assert!(!session.mode.allies_allowed);
}

#[test]
fn launch_session_does_not_synthesize_unknown_mode() {
    let shell = SkirmishShellState {
        selected_mode_id: 99,
        ..Default::default()
    };
    let maps = [test_map_entry("map.mmx")];

    assert_eq!(
        launch_session(&shell, &maps, &stock_skirmish_modes()).unwrap_err(),
        LaunchValidationError::NoSelectedMode { mode_id: 99 }
    );
}

#[test]
fn launch_session_rejects_missing_map_and_bad_color() {
    let mut shell = SkirmishShellState::default();
    assert_eq!(
        launch_session(&shell, &[], &stock_skirmish_modes()).unwrap_err(),
        LaunchValidationError::NoSelectedMap
    );

    shell.player_color_index = HOUSE_COLOR_COUNT;
    let maps = [test_map_entry("map.mmx")];
    assert_eq!(
        launch_session(&shell, &maps, &stock_skirmish_modes()).unwrap_err(),
        LaunchValidationError::InvalidColorIndex {
            slot: 0,
            color_index: HOUSE_COLOR_COUNT,
        }
    );
}

#[test]
fn skirmish_launch_random_country_succeeds_and_flags_slot_for_resolution() {
    let mut shell = SkirmishShellState {
        player_country_random: true,
        ..Default::default()
    };
    let maps = [test_map_entry("map.mmx")];

    // A Random local country no longer blocks Start; the session is built with
    // the slot flagged so the concrete country is drawn during resolution.
    let session = launch_session(&shell, &maps, &stock_skirmish_modes())
        .expect("random local country must not block Start");
    assert!(session.local.country_random);

    shell.player_country_random = false;
    shell.opponents[0].country_random = true;

    let session = launch_session(&shell, &maps, &stock_skirmish_modes())
        .expect("random AI country must not block Start");
    assert!(!session.local.country_random);
    assert!(session.opponents[0].country_random);
}

#[test]
fn skirmish_concrete_country_color_launch_session_still_succeeds() {
    let mut shell = SkirmishShellState::default();
    shell.player_country_random = false;
    shell.player_country = SkirmishCountry::Germany;
    shell.player_color_index = 2;
    shell.opponents[0].country_random = false;
    shell.opponents[0].country = SkirmishCountry::Iraq;
    shell.opponents[0].color_index = 4;
    let maps = [test_map_entry("map.mmx")];

    let session = launch_session(&shell, &maps, &stock_skirmish_modes()).expect("session");

    assert_eq!(session.local.country, LaunchCountry::Germany);
    assert_eq!(session.local.color_index, 2);
    assert_eq!(session.opponents[0].country, LaunchCountry::Iraq);
    assert_eq!(session.opponents[0].color_index, 4);
}

#[test]
fn launch_session_rejects_map_capacity_overflow() {
    let mut shell = SkirmishShellState::default();
    shell.opponents[1].row_type = SkirmishAiRowType::Normal;
    let maps = [test_map_entry_with_starts("tiny.mmx", 2)];

    assert_eq!(
        launch_session(&shell, &maps, &stock_skirmish_modes()).unwrap_err(),
        LaunchValidationError::MapCapacityExceeded {
            capacity: 2,
            requested_players: 3,
        }
    );
}

#[test]
fn raw_pack_skips_start_only_validation_for_back_transaction() {
    let mut shell = SkirmishShellState::default();
    for opponent in &mut shell.opponents {
        opponent.row_type = SkirmishAiRowType::None;
    }
    let maps = [test_map_entry_with_starts("one-player.mmx", 1)];
    let modes = stock_skirmish_modes();

    assert_eq!(
        launch_session(&shell, &maps, &modes).unwrap_err(),
        LaunchValidationError::NoEnabledOpponent
    );
    let packed = pack_launch_session_without_start_validation(&shell, &maps, &modes)
        .expect("Back must pack controls without Start-only gates");
    assert!(packed.opponents.is_empty());
    assert_eq!(packed.selected_map_file.as_deref(), Some("one-player.mmx"));
}

#[test]
fn raw_pack_retains_sparse_ai_house_slots_before_compaction() {
    let mut shell = SkirmishShellState::default();
    for opponent in &mut shell.opponents {
        opponent.row_type = SkirmishAiRowType::None;
    }
    shell.opponents[1].row_type = SkirmishAiRowType::Normal;
    shell.opponents[4].row_type = SkirmishAiRowType::Hard;
    let maps = [test_map_entry("map.mmx")];
    let packed = pack_launch_session_without_start_validation(
        &shell,
        &maps,
        &stock_skirmish_modes(),
    )
    .expect("sparse raw AI slots pack before gameplay compaction");

    assert_eq!(packed.opponents.len(), 2);
    assert_eq!(
        packed
            .pre_fill_house_roster
            .ai_slots()
            .iter()
            .map(|slot| (slot.slot_index, slot.valid))
            .collect::<Vec<_>>(),
        vec![
            (0, false),
            (1, true),
            (2, false),
            (3, false),
            (4, true),
            (5, false),
            (6, false),
        ]
    );
    assert_eq!(packed.pre_fill_house_roster.required_start_count(), 3);
    assert_eq!(packed.pre_fill_house_roster.created_house_count(), 5);
}

#[test]
fn launch_session_rejects_no_active_opponents() {
    let mut shell = SkirmishShellState::default();
    for opponent in &mut shell.opponents {
        opponent.row_type = SkirmishAiRowType::None;
    }
    let maps = [test_map_entry("map.mmx")];

    assert_eq!(
        launch_session(&shell, &maps, &stock_skirmish_modes()).unwrap_err(),
        LaunchValidationError::NoEnabledOpponent
    );
}

#[test]
fn launch_session_rejects_same_explicit_team() {
    let mut shell = SkirmishShellState::default();
    shell.player_team = 0;
    shell.opponents[0].team = 0;
    let maps = [test_map_entry("map.mmx")];

    assert_eq!(
        launch_session(&shell, &maps, &stock_skirmish_modes()).unwrap_err(),
        LaunchValidationError::SameExplicitTeam { team: 0 }
    );
}

#[test]
fn dismiss_validation_modal_clears_only_when_open() {
    let mut shell = SkirmishShellState::default();

    assert!(!dismiss_validation_modal(&mut shell));

    shell.validation_modal = Some(SkirmishValidationModalState::new("body", "OK"));

    assert!(dismiss_validation_modal(&mut shell));
    assert!(shell.validation_modal.is_none());
    assert!(!dismiss_validation_modal(&mut shell));
}

#[test]
fn team_combo_uses_verified_item_data_values() {
    let shell = SkirmishShellState::default();
    let maps = [test_map_entry("map.mmx")];

    assert_eq!(
        combo_items(&shell, &maps, SkirmishComboId::Team(0)),
        vec![
            SkirmishComboItem::Team(-2),
            SkirmishComboItem::Team(0),
            SkirmishComboItem::Team(1),
            SkirmishComboItem::Team(2),
            SkirmishComboItem::Team(3),
        ]
    );
}

#[test]
fn team_game_must_ally_omits_team_none_combo_item() {
    let mut shell = SkirmishShellState {
        selected_mode_id: 9,
        ..Default::default()
    };
    repair_teams_for_selected_mode(&mut shell, &stock_skirmish_modes());
    let maps = [test_map_entry("map.mmx")];

    assert_eq!(
        combo_items(&shell, &maps, SkirmishComboId::Team(0)),
        vec![
            SkirmishComboItem::Team(0),
            SkirmishComboItem::Team(1),
            SkirmishComboItem::Team(2),
            SkirmishComboItem::Team(3),
        ]
    );
}

#[test]
fn ffa_combo_keeps_explicit_teams_despite_allies_disabled() {
    let mut shell = SkirmishShellState {
        selected_mode_id: 2,
        ..Default::default()
    };
    repair_teams_for_selected_mode(&mut shell, &stock_skirmish_modes());
    let maps = [test_map_entry("map.mmx")];

    assert_eq!(
        combo_items(&shell, &maps, SkirmishComboId::Team(0)),
        vec![
            SkirmishComboItem::Team(-2),
            SkirmishComboItem::Team(0),
            SkirmishComboItem::Team(1),
            SkirmishComboItem::Team(2),
            SkirmishComboItem::Team(3),
        ]
    );
}

#[test]
fn team_game_mode_repair_removes_team_none_from_local_and_ai() {
    let mut shell = SkirmishShellState {
        selected_mode_id: 9,
        ..Default::default()
    };
    shell.player_team = -2;
    shell.opponents[0].team = -2;

    repair_teams_for_selected_mode(&mut shell, &stock_skirmish_modes());

    assert_eq!(shell.player_team, 0);
    assert_eq!(shell.opponents[0].team, 3);
}

#[test]
fn inactive_ai_team_default_follows_allies_allowed() {
    let modes = stock_skirmish_modes();
    let mut battle = SkirmishShellState::default();
    battle.opponents[1].row_type = SkirmishAiRowType::None;
    repair_teams_for_selected_mode(&mut battle, &modes);

    let mut ffa = SkirmishShellState {
        selected_mode_id: 2,
        ..Default::default()
    };
    ffa.opponents[1].row_type = SkirmishAiRowType::None;
    repair_teams_for_selected_mode(&mut ffa, &modes);

    assert_eq!(battle.opponents[1].team, 3);
    assert_eq!(ffa.opponents[1].team, -2);
}

#[test]
fn default_inactive_ai_rows_use_native_combo_defaults() {
    let shell = SkirmishShellState::default();

    for opponent in shell.opponents.iter().skip(1) {
        assert_eq!(opponent.row_type, SkirmishAiRowType::None);
        assert!(!opponent.enabled);
        assert!(opponent.country_random);
        assert!(!opponent.color_claimed);
        assert_eq!(opponent.start_position, StartPosition::Auto);
        assert_eq!(opponent.team, 3);
    }
}

#[test]
fn accepted_map_shrink_resets_and_hides_complete_opponent_rows_atomically() {
    let maps = [
        test_map_entry_with_starts("eight.mmx", 8),
        test_map_entry_with_starts("two.mmx", 2),
    ];
    let mut shell = SkirmishShellState::default();
    shell.selected_map_idx = 0;
    for opponent in shell.opponents.iter_mut().take(4) {
        opponent.row_type = SkirmishAiRowType::Hard;
        opponent.enabled = true;
        opponent.country_random = false;
        opponent.color_claimed = true;
        opponent.start_position = StartPosition::Position(4);
        opponent.team = 1;
    }
    shell.open_combo_dropdown = Some(OpenComboDropdown {
        id: SkirmishComboId::Team(3),
        top_index: 0,
    });

    assert!(accept_selected_map(&mut shell, &maps, 1));

    assert_eq!(shell.selected_map_idx, 1);
    assert!(player_row_visible(&shell, &maps, 1));
    assert!(!player_row_visible(&shell, &maps, 2));
    assert!(shell.opponents[0].is_active());
    for opponent in shell.opponents.iter().skip(1) {
        assert_eq!(opponent.row_type, SkirmishAiRowType::None);
        assert!(!opponent.enabled);
        assert!(opponent.country_random);
        assert!(!opponent.color_claimed);
        assert_eq!(opponent.start_position, StartPosition::Auto);
        assert_eq!(opponent.team, 3);
    }
    assert!(shell.open_combo_dropdown.is_none());
}

#[test]
fn accepted_map_growth_reveals_rows_without_reactivating_hidden_state() {
    let maps = [
        test_map_entry_with_starts("two.mmx", 2),
        test_map_entry_with_starts("eight.mmx", 8),
    ];
    let mut shell = SkirmishShellState::default();
    shell.selected_map_idx = 0;
    initialize_rows_for_selected_map(&mut shell, &maps);

    assert!(accept_selected_map(&mut shell, &maps, 1));

    assert!(player_row_visible(&shell, &maps, 7));
    assert!(
        shell
            .opponents
            .iter()
            .skip(1)
            .all(|opponent| opponent.row_type == SkirmishAiRowType::None)
    );
}

#[test]
fn accepted_equal_capacity_map_does_not_rewrite_rows() {
    let maps = [
        test_map_entry_with_starts("first.mmx", 4),
        test_map_entry_with_starts("second.mmx", 4),
    ];
    let mut shell = SkirmishShellState::default();
    shell.opponents[2].row_type = SkirmishAiRowType::Normal;
    shell.opponents[2].enabled = true;
    shell.opponents[2].team = 2;

    assert!(accept_selected_map(&mut shell, &maps, 1));

    assert_eq!(shell.opponents[2].row_type, SkirmishAiRowType::Normal);
    assert_eq!(shell.opponents[2].team, 2);
}

#[test]
fn hidden_rows_are_excluded_from_combo_and_hover_input() {
    let maps = [test_map_entry_with_starts("two.mmx", 2)];
    let mut shell = SkirmishShellState::default();
    initialize_rows_for_selected_map(&mut shell, &maps);
    let layout = compute_layout(800, 600);
    let hidden_ai_type = layout.rows.ai_type_combos[1];

    assert!(!combo_enabled(&shell, &maps, SkirmishComboId::AiType(1)));
    assert_eq!(
        hovered_shell_control(&layout, &shell, &maps, hidden_ai_type.x, hidden_ai_type.y),
        None
    );
}

#[test]
fn negative_native_capacity_hides_every_ai_row_without_eight_player_fallback() {
    let mut map = test_map_entry_with_starts("negative.mmx", 0);
    map.player_capacity = -2;
    let maps = [map];
    let mut shell = SkirmishShellState::default();

    initialize_rows_for_selected_map(&mut shell, &maps);

    assert!(player_row_visible(&shell, &maps, 0));
    assert!(!player_row_visible(&shell, &maps, 1));
    assert!(
        shell
            .opponents
            .iter()
            .all(|opponent| opponent.row_type == SkirmishAiRowType::None)
    );
}

#[test]
fn ai_type_none_applies_inactive_combo_defaults() {
    let mut shell = SkirmishShellState::default();
    shell.opponents[0].row_type = SkirmishAiRowType::Hard;
    shell.opponents[0].enabled = true;
    shell.opponents[0].country_random = false;
    shell.opponents[0].country = SkirmishCountry::Yuri;
    shell.opponents[0].color_index = 4;
    shell.opponents[0].color_claimed = true;
    shell.opponents[0].start_position = StartPosition::Position(5);
    shell.opponents[0].team = 1;

    apply_combo_selection_for_test(
        &mut shell,
        SkirmishComboId::AiType(0),
        SkirmishComboItem::AiType(SkirmishAiRowType::None),
    );

    assert_eq!(shell.opponents[0].row_type, SkirmishAiRowType::None);
    assert!(!shell.opponents[0].enabled);
    assert!(shell.opponents[0].country_random);
    assert_eq!(shell.opponents[0].country, SkirmishCountry::Yuri);
    assert_eq!(shell.opponents[0].color_index, 4);
    assert!(!shell.opponents[0].color_claimed);
    assert_eq!(shell.opponents[0].start_position, StartPosition::Auto);
    assert_eq!(shell.opponents[0].team, 3);
}

#[test]
fn ai_type_none_uses_ffa_inactive_team_default() {
    let mut shell = SkirmishShellState {
        selected_mode_id: 2,
        ..Default::default()
    };
    repair_teams_for_selected_mode(&mut shell, &stock_skirmish_modes());
    shell.opponents[0].row_type = SkirmishAiRowType::Hard;
    shell.opponents[0].enabled = true;
    shell.opponents[0].team = 1;

    apply_combo_selection_for_test(
        &mut shell,
        SkirmishComboId::AiType(0),
        SkirmishComboItem::AiType(SkirmishAiRowType::None),
    );

    assert_eq!(shell.opponents[0].team, -2);
}

#[test]
fn mode_refresh_reselects_native_local_and_ai_team_defaults() {
    let mut shell = SkirmishShellState {
        selected_mode_id: 9,
        player_team: 1,
        ..Default::default()
    };
    shell.opponents[0].team = 2;

    repair_teams_for_selected_mode(&mut shell, &stock_skirmish_modes());

    assert_eq!(shell.player_team, 0);
    assert_eq!(shell.opponents[0].team, 3);

    shell.selected_mode_id = 2;
    shell.player_team = 2;
    repair_teams_for_selected_mode(&mut shell, &stock_skirmish_modes());

    assert_eq!(shell.player_team, -2);
    assert_eq!(shell.opponents[0].team, -2);
}

#[test]
fn ffa_mode_refresh_defaults_every_ai_team_to_none_and_disables_the_team_column() {
    let mut shell = SkirmishShellState {
        selected_mode_id: 2,
        ..Default::default()
    };
    for opponent in &mut shell.opponents {
        opponent.row_type = SkirmishAiRowType::Easy;
        opponent.team = 1;
    }
    repair_teams_for_selected_mode(&mut shell, &stock_skirmish_modes());
    let maps = [test_map_entry("ffa.yrm")];

    assert!(shell.opponents.iter().all(|opponent| opponent.team == -2));
    // Free For All is AlliesAllowed=no, so the native refresh greys every team
    // control including the local row's, even though the rows are all active.
    assert!(!combo_enabled(&shell, &maps, SkirmishComboId::Team(0)));
    for row in 1..=shell.opponents.len() {
        assert!(
            !combo_enabled(&shell, &maps, SkirmishComboId::Team(row)),
            "team combo for row {row} must be disabled in Free For All"
        );
    }
    // Only the team column is gated on the mode; the sibling combos still track
    // row-active state.
    assert!(combo_enabled(&shell, &maps, SkirmishComboId::Side(0)));
    assert!(combo_enabled(&shell, &maps, SkirmishComboId::Side(1)));
    assert!(combo_enabled(&shell, &maps, SkirmishComboId::AiType(0)));
}

#[test]
fn battle_mode_enables_the_team_column_for_local_and_active_rows_only() {
    let mut shell = SkirmishShellState {
        selected_mode_id: 1,
        ..Default::default()
    };
    shell.opponents[0].row_type = SkirmishAiRowType::Easy;
    shell.opponents[0].enabled = true;
    shell.opponents[1].row_type = SkirmishAiRowType::None;
    shell.opponents[1].enabled = false;
    repair_teams_for_selected_mode(&mut shell, &stock_skirmish_modes());
    let maps = [test_map_entry("battle.yrm")];

    // Battle is AlliesAllowed=yes: the local combo is live, and an AI row's
    // combo follows only whether that row is open.
    assert!(combo_enabled(&shell, &maps, SkirmishComboId::Team(0)));
    assert!(combo_enabled(&shell, &maps, SkirmishComboId::Team(1)));
    assert!(!combo_enabled(&shell, &maps, SkirmishComboId::Team(2)));
}

#[test]
fn disabled_team_combo_cannot_be_opened_or_reselected_in_free_for_all() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState {
        selected_mode_id: 2,
        ..Default::default()
    };
    shell.opponents[0].row_type = SkirmishAiRowType::Easy;
    shell.opponents[0].enabled = true;
    repair_teams_for_selected_mode(&mut shell, &stock_skirmish_modes());
    let maps = [test_map_entry("ffa.yrm")];

    for row in [0usize, 1] {
        let rect = layout.rows.team_combos[row];
        assert!(
            combo_dropdown_rect(&shell, &layout, &maps, SkirmishComboId::Team(row)).is_none(),
            "disabled team combo for row {row} must not offer a dropdown"
        );
        // A click on the arrow zone of a greyed combo neither opens the
        // dropdown nor plays the open sound.
        assert_eq!(
            handle_option_mouse_down(&mut shell, &layout, &maps, rect.x + rect.w - 1, rect.y + 1),
            SkirmishShellAction::None
        );
        assert!(shell.open_combo_dropdown.is_none());
        assert!(shell.drain_pending_ui_sounds().is_empty());
    }

    // Both rows stay on None, so nothing can ally in a mode that forbids it.
    assert_eq!(shell.player_team, -2);
    assert_eq!(shell.opponents[0].team, -2);
}

#[test]
fn battle_default_active_ai_starts_on_team_d() {
    let shell = SkirmishShellState::default();

    assert!(shell.opponents[0].is_active());
    assert_eq!(shell.opponents[0].team, 3);
}

#[test]
fn side_combo_exposes_random_country_and_verified_dropdown_cap() {
    let layout = compute_layout(800, 600);
    let shell = SkirmishShellState::default();
    let maps = [test_map_entry("map.mmx")];
    let items = combo_items(&shell, &maps, SkirmishComboId::Side(0));

    assert_eq!(
        items.first().copied(),
        Some(SkirmishComboItem::Country(SkirmishCountryChoice::Random))
    );
    assert_eq!(items.len(), SkirmishCountry::ALL.len() + 1);

    let dropdown = combo_dropdown_rect(&shell, &layout, &maps, SkirmishComboId::Side(0)).unwrap();
    assert_eq!(dropdown.y, layout.rows.side_combos[0].y + COMBO_FACE_H + 1);
    assert_eq!(dropdown.h, 7 * COMBO_DROPDOWN_ROW_H);
    assert_eq!(
        combo_dropdown_visible_row_count(&shell, &maps, SkirmishComboId::Side(0)),
        7
    );
    let content =
        combo_dropdown_content_rect(&shell, &layout, &maps, SkirmishComboId::Side(0)).unwrap();
    let scrollbar =
        combo_dropdown_scrollbar_rect(&shell, &layout, &maps, SkirmishComboId::Side(0)).unwrap();
    assert_eq!(content.w, dropdown.w - COMBO_DROPDOWN_SCROLLBAR_W);
    assert_eq!(scrollbar.x, dropdown.x + content.w);
    assert_eq!(scrollbar.h, dropdown.h);
}

#[test]
fn dropdown_wheel_is_inert_and_content_click_uses_top_index() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let maps = [test_map_entry("map.mmx")];
    let rect = layout.rows.side_combos[0];

    handle_option_mouse_down(&mut shell, &layout, &maps, rect.x + rect.w - 1, rect.y + 1);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboOpenSound]
    );
    // No verified retail wheel-scroll: the wheel neither consumes the event
    // nor moves the open dropdown's top index.
    assert!(!handle_option_mouse_wheel(&mut shell, &maps, -1.0));
    assert_eq!(shell.open_combo_dropdown.unwrap().top_index, 0);
    assert!(shell.drain_pending_ui_sounds().is_empty());

    let content =
        combo_dropdown_content_rect(&shell, &layout, &maps, SkirmishComboId::Side(0)).unwrap();
    handle_option_mouse_down(&mut shell, &layout, &maps, content.x + 2, content.y + 1);

    assert_eq!(
        selected_combo_item(&shell, SkirmishComboId::Side(0)),
        combo_items(&shell, &maps, SkirmishComboId::Side(0))
            .first()
            .copied()
    );
    assert_eq!(shell.open_combo_dropdown, None);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboCloseSound]
    );
}

#[test]
fn dropdown_scrollbar_arrows_step_and_drag_clamp_top_index() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let maps = [test_map_entry("map.mmx")];
    let rect = layout.rows.side_combos[0];

    handle_option_mouse_down(&mut shell, &layout, &maps, rect.x + rect.w - 1, rect.y + 1);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboOpenSound]
    );
    let scrollbar =
        combo_dropdown_scrollbar_rect(&shell, &layout, &maps, SkirmishComboId::Side(0)).unwrap();
    handle_option_mouse_down(
        &mut shell,
        &layout,
        &maps,
        scrollbar.x + 1,
        scrollbar.y + scrollbar.h - 1,
    );
    assert_eq!(shell.open_combo_dropdown.unwrap().top_index, 1);
    assert_eq!(
        shell.dropdown_scroll_press,
        Some(DropdownScrollbarPressState {
            id: SkirmishComboId::Side(0),
            part: DropdownScrollbarPart::DownArrow,
        })
    );
    assert!(shell.drain_pending_ui_sounds().is_empty());

    let thumb =
        combo_dropdown_scroll_thumb_rect(&shell, &layout, &maps, SkirmishComboId::Side(0)).unwrap();
    handle_option_mouse_down(&mut shell, &layout, &maps, thumb.x + 1, thumb.y + 1);
    assert_eq!(
        shell.dropdown_scroll_drag,
        Some(DropdownScrollDragState {
            id: SkirmishComboId::Side(0),
            grab_offset_y: 1,
        })
    );
    assert_eq!(
        shell.dropdown_scroll_press,
        Some(DropdownScrollbarPressState {
            id: SkirmishComboId::Side(0),
            part: DropdownScrollbarPart::Thumb,
        })
    );
    assert!(shell.drain_pending_ui_sounds().is_empty());
    handle_option_mouse_move(
        &mut shell,
        &layout,
        &maps,
        thumb.x + 1,
        scrollbar.y + scrollbar.h,
    );
    assert_eq!(
        shell.open_combo_dropdown.unwrap().top_index,
        combo_dropdown_max_top_index(&shell, &maps, SkirmishComboId::Side(0))
    );
    assert!(shell.drain_pending_ui_sounds().is_empty());
    handle_option_mouse_up(&mut shell);
    assert_eq!(shell.dropdown_scroll_drag, None);
    assert_eq!(shell.dropdown_scroll_press, None);
    assert!(shell.drain_pending_ui_sounds().is_empty());
}

#[test]
fn skirmish_side_dropdown_scrollbar_track_click_jumps_to_native_top_index() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let maps = [test_map_entry("map.mmx")];
    let rect = layout.rows.side_combos[0];

    handle_option_mouse_down(&mut shell, &layout, &maps, rect.x + rect.w - 1, rect.y + 1);
    shell.drain_pending_ui_sounds();
    let scrollbar =
        combo_dropdown_scrollbar_rect(&shell, &layout, &maps, SkirmishComboId::Side(0)).unwrap();
    let max_top = combo_dropdown_max_top_index(&shell, &maps, SkirmishComboId::Side(0));
    let click_y = scrollbar.y + scrollbar.h - COMBO_DROPDOWN_SCROLLBAR_BUTTON_H - 1;
    let selected_before = selected_combo_item(&shell, SkirmishComboId::Side(0));
    let expected = top_index_from_scrollbar_track_click(
        &shell,
        &layout,
        &maps,
        SkirmishComboId::Side(0),
        click_y,
    )
    .unwrap();

    handle_option_mouse_down(&mut shell, &layout, &maps, scrollbar.x + 1, click_y);

    assert!(expected > 1);
    assert!(expected <= max_top);
    assert_eq!(shell.open_combo_dropdown.unwrap().top_index, expected);
    assert_eq!(
        selected_combo_item(&shell, SkirmishComboId::Side(0)),
        selected_before
    );
    assert_eq!(
        shell.open_combo_dropdown.unwrap().id,
        SkirmishComboId::Side(0)
    );
    assert_eq!(
        shell.dropdown_scroll_press,
        Some(DropdownScrollbarPressState {
            id: SkirmishComboId::Side(0),
            part: DropdownScrollbarPart::Track,
        })
    );
    assert!(shell.drain_pending_ui_sounds().is_empty());
}

#[test]
fn selecting_random_country_updates_shell_choice_state() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let maps = [test_map_entry("map.mmx")];
    let rect = layout.rows.side_combos[0];

    handle_option_mouse_down(&mut shell, &layout, &maps, rect.x + rect.w - 1, rect.y + 1);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboOpenSound]
    );
    let dropdown = combo_dropdown_rect(&shell, &layout, &maps, SkirmishComboId::Side(0)).unwrap();
    handle_option_mouse_down(&mut shell, &layout, &maps, dropdown.x + 2, dropdown.y + 1);

    assert!(shell.player_country_random);
    assert_eq!(
        selected_combo_item(&shell, SkirmishComboId::Side(0)),
        Some(SkirmishComboItem::Country(SkirmishCountryChoice::Random))
    );
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboCloseSound]
    );
}

#[test]
fn hit_test_ignores_combo_faces_after_owner_draw_buttons() {
    let layout = compute_layout(800, 600);
    assert_eq!(
        hit_test(&layout, layout.color_combos[0].x, layout.color_combos[0].y),
        SkirmishShellAction::None
    );
    assert_eq!(
        hit_test(&layout, layout.color_combos[1].x, layout.color_combos[1].y),
        SkirmishShellAction::None
    );
}

#[test]
fn combo_arrow_opens_dropdown_and_selects_color_row() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    // Deactivate all AI rows so slot 0's filter sees no other claimants and
    // the dropdown maps row N → color N-1 as the test below assumes.
    for opponent in &mut shell.opponents {
        opponent.row_type = SkirmishAiRowType::None;
        opponent.color_claimed = false;
    }
    let maps = [test_map_entry("map.mmx")];
    let rect = layout.color_combos[0];

    handle_option_mouse_down(&mut shell, &layout, &maps, rect.x + rect.w - 1, rect.y + 1);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboOpenSound]
    );

    assert_eq!(
        shell.open_combo_dropdown,
        Some(OpenComboDropdown {
            id: SkirmishComboId::Color(0),
            top_index: 0
        })
    );

    let dropdown = combo_dropdown_rect(&shell, &layout, &maps, SkirmishComboId::Color(0)).unwrap();
    handle_option_mouse_down(
        &mut shell,
        &layout,
        &maps,
        dropdown.x + 2,
        dropdown.y + COMBO_DROPDOWN_ROW_H * 4 + 1,
    );

    assert_eq!(shell.player_color_index, 3);
    assert_eq!(shell.open_combo_dropdown, None);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboCloseSound]
    );
}

#[test]
fn skirmish_color_dropdown_normal_population_omits_initialized_row_8() {
    let mut shell = SkirmishShellState::default();
    shell.player_color_index = 8;
    // Deactivate all AI rows so the filter sees no other claimants and the
    // dropdown matches the historical unfiltered set.
    for opponent in &mut shell.opponents {
        opponent.row_type = SkirmishAiRowType::None;
        opponent.color_claimed = false;
    }
    let maps = [test_map_entry("map.mmx")];

    let items = combo_items(&shell, &maps, SkirmishComboId::Color(0));

    assert_eq!(items.first(), Some(&SkirmishComboItem::ColorSentinel(-2)));
    assert_eq!(
        &items[1..],
        &[
            SkirmishComboItem::Color(0),
            SkirmishComboItem::Color(1),
            SkirmishComboItem::Color(2),
            SkirmishComboItem::Color(3),
            SkirmishComboItem::Color(4),
            SkirmishComboItem::Color(5),
            SkirmishComboItem::Color(6),
            SkirmishComboItem::Color(7),
        ]
    );
    assert!(!items.contains(&SkirmishComboItem::Color(8)));
    assert_eq!(
        selected_combo_item(&shell, SkirmishComboId::Color(0)),
        Some(SkirmishComboItem::Color(7))
    );
    assert_eq!(
        selected_combo_item_index(&shell, &maps, SkirmishComboId::Color(0)),
        Some(8)
    );
}

#[test]
fn combo_outside_click_closes_with_close_sound() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let maps = [test_map_entry("map.mmx")];
    let rect = layout.rows.side_combos[0];

    handle_option_mouse_down(&mut shell, &layout, &maps, rect.x + rect.w - 1, rect.y + 1);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboOpenSound]
    );

    handle_option_mouse_down(&mut shell, &layout, &maps, 0, 0);

    assert_eq!(shell.open_combo_dropdown, None);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboCloseSound]
    );
}

#[test]
fn collapsed_combo_face_click_plays_open_sound_without_opening() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let maps = [test_map_entry("map.mmx")];
    let rect = layout.rows.side_combos[0];

    // Click the left/text portion of the collapsed face (not the arrow zone).
    handle_option_mouse_down(&mut shell, &layout, &maps, rect.x + 2, rect.y + 1);

    assert_eq!(shell.open_combo_dropdown, None);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboOpenSound]
    );

    // Click the rightmost arrow zone: open sound plays AND the dropdown opens.
    handle_option_mouse_down(&mut shell, &layout, &maps, rect.x + rect.w - 1, rect.y + 1);
    assert_eq!(
        shell.open_combo_dropdown.map(|open| open.id),
        Some(SkirmishComboId::Side(0))
    );
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboOpenSound]
    );
}

#[test]
fn switching_combos_closes_first_without_opening_second() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let maps = [test_map_entry("map.mmx")];
    let combo_a = layout.rows.side_combos[0];
    let combo_b = layout.rows.start_combos[0];

    handle_option_mouse_down(
        &mut shell,
        &layout,
        &maps,
        combo_a.x + combo_a.w - 1,
        combo_a.y + 1,
    );
    assert_eq!(
        shell.open_combo_dropdown.map(|open| open.id),
        Some(SkirmishComboId::Side(0))
    );
    shell.drain_pending_ui_sounds();

    // With combo A open, clicking combo B's arrow only closes A (captured popup
    // consumes the click); B does not open on this click.
    handle_option_mouse_down(
        &mut shell,
        &layout,
        &maps,
        combo_b.x + combo_b.w - 1,
        combo_b.y + 1,
    );
    assert_eq!(shell.open_combo_dropdown, None);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboCloseSound]
    );

    // A second click on B's arrow opens B with the open sound.
    handle_option_mouse_down(
        &mut shell,
        &layout,
        &maps,
        combo_b.x + combo_b.w - 1,
        combo_b.y + 1,
    );
    assert_eq!(
        shell.open_combo_dropdown.map(|open| open.id),
        Some(SkirmishComboId::Start(0))
    );
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboOpenSound]
    );
}

#[test]
fn start_dropdown_omits_starts_reserved_by_other_rows() {
    let mut shell = SkirmishShellState::default();
    shell.player_start_position = StartPosition::Position(0);
    shell.opponents[0].start_position = StartPosition::Position(1);
    let maps = [test_map_entry_with_starts("map.mmx", 4)];

    let items = combo_items(&shell, &maps, SkirmishComboId::Start(2));

    assert!(items.contains(&SkirmishComboItem::Start(StartPosition::Auto)));
    assert!(!items.contains(&SkirmishComboItem::Start(StartPosition::Position(0))));
    assert!(!items.contains(&SkirmishComboItem::Start(StartPosition::Position(1))));
    assert!(items.contains(&SkirmishComboItem::Start(StartPosition::Position(2))));
    assert!(items.contains(&SkirmishComboItem::Start(StartPosition::Position(3))));
}

#[test]
fn ai_type_dropdown_updates_active_row_state() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let maps = [test_map_entry("map.mmx")];
    let rect = layout.rows.ai_type_combos[1];

    handle_option_mouse_down(&mut shell, &layout, &maps, rect.x + rect.w - 1, rect.y + 1);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboOpenSound]
    );
    let dropdown = combo_dropdown_rect(&shell, &layout, &maps, SkirmishComboId::AiType(1)).unwrap();
    handle_option_mouse_down(
        &mut shell,
        &layout,
        &maps,
        dropdown.x + 2,
        dropdown.y + COMBO_DROPDOWN_ROW_H * 2 + 1,
    );

    assert_eq!(shell.opponents[1].row_type, SkirmishAiRowType::Normal);
    assert!(shell.opponents[1].enabled);
    assert_eq!(shell.opponents[1].difficulty, AiDifficulty::Normal);
    assert_eq!(
        shell.drain_pending_ui_sounds(),
        vec![SkirmishShellUiSound::GuiComboCloseSound]
    );
}

#[test]
fn inactive_ai_sibling_combo_does_not_open() {
    let layout = compute_layout(800, 600);
    let mut shell = SkirmishShellState::default();
    let maps = [test_map_entry("map.mmx")];
    let rect = layout.rows.side_combos[2];

    handle_option_mouse_down(&mut shell, &layout, &maps, rect.x + rect.w - 1, rect.y + 1);

    assert_eq!(shell.open_combo_dropdown, None);
}

#[test]
fn color_default_state_each_row_excludes_other_claimed_colors() {
    // Default state: every active row's filter excludes the other 7 default
    // colors and includes its own + sentinel.
    let mut shell = SkirmishShellState::default();
    // Activate every AI row so all 8 slots hold claims.
    for opponent in &mut shell.opponents {
        opponent.row_type = SkirmishAiRowType::Easy;
        opponent.color_claimed = true;
    }
    let maps = [test_map_entry("map.mmx")];

    for row in 0..SKIRMISH_AI_SLOT_COUNT + 1 {
        let items = combo_items(&shell, &maps, SkirmishComboId::Color(row));
        // Sentinel + exactly one color visible: this row's own.
        assert_eq!(items.len(), 2, "row {row} should see sentinel + self only");
        assert_eq!(items[0], SkirmishComboItem::ColorSentinel(-2));
        let expected_color = if row == 0 {
            shell.player_color_index
        } else {
            shell.opponents[row - 1].color_index
        };
        assert_eq!(items[1], SkirmishComboItem::Color(expected_color));
    }
}

#[test]
fn color_claim_excludes_color_from_other_rows_dropdown() {
    // Player claims color 4; AI row 1's filter loses color 4.
    let mut shell = SkirmishShellState::default();
    shell.player_color_index = 4;
    shell.player_color_claimed = true;
    // Deactivate AI row 1 so its own default claim doesn't confound the
    // assertion.
    shell.opponents[0].row_type = SkirmishAiRowType::None;
    shell.opponents[0].color_claimed = false;
    let maps = [test_map_entry("map.mmx")];

    let items = combo_items(&shell, &maps, SkirmishComboId::Color(1));

    assert!(items.contains(&SkirmishComboItem::ColorSentinel(-2)));
    assert!(!items.contains(&SkirmishComboItem::Color(4)));
    // Spot-check that other colors are still present.
    assert!(items.contains(&SkirmishComboItem::Color(0)));
    assert!(items.contains(&SkirmishComboItem::Color(7)));
}

#[test]
fn color_selection_evicts_prior_claimant() {
    // Player picks color 5 while AI row 1 already claimed color 5. After
    // the selection, AI row 1 must no longer claim 5, even though its
    // cached color_index can remain. The dropdown filter HIDES color 5
    // from slot 0 (since slot 1 owns it), so the test drives
    // apply_combo_selection directly — same entry point the mouse handler
    // uses, just bypassing the row-index lookup.
    let mut shell = SkirmishShellState::default();
    shell.player_color_index = 0;
    shell.player_color_claimed = true;
    shell.opponents[0].row_type = SkirmishAiRowType::Easy;
    shell.opponents[0].color_index = 5;
    shell.opponents[0].color_claimed = true;

    apply_combo_selection_for_test(
        &mut shell,
        SkirmishComboId::Color(0),
        SkirmishComboItem::Color(5),
    );

    assert_eq!(shell.player_color_index, 5);
    assert!(shell.player_color_claimed);
    assert!(
        !shell.opponents[0].color_claimed,
        "AI row 1 must have its claim evicted when the player takes its color"
    );
    // Cached color_index stays — the evicted row keeps its prior color in
    // case the user later re-picks from the dropdown.
    assert_eq!(shell.opponents[0].color_index, 5);
}

#[test]
fn sentinel_release_makes_color_available_to_other_rows() {
    // AI row 1 claims color 3; AI row 1 selects sentinel; AI row 2 can now
    // see color 3 in its dropdown.
    let mut shell = SkirmishShellState::default();
    shell.player_color_index = 0;
    shell.player_color_claimed = true;
    shell.opponents[0].row_type = SkirmishAiRowType::Easy;
    shell.opponents[0].color_index = 3;
    shell.opponents[0].color_claimed = true;
    shell.opponents[1].row_type = SkirmishAiRowType::Easy;
    shell.opponents[1].color_index = 6;
    shell.opponents[1].color_claimed = true;
    let maps = [test_map_entry("map.mmx")];

    let before = combo_items(&shell, &maps, SkirmishComboId::Color(2));
    assert!(!before.contains(&SkirmishComboItem::Color(3)));

    apply_combo_selection_for_test(
        &mut shell,
        SkirmishComboId::Color(1),
        SkirmishComboItem::ColorSentinel(-2),
    );

    assert!(!shell.opponents[0].color_claimed);
    assert_eq!(shell.opponents[0].color_index, 3, "cached color preserved");
    let after = combo_items(&shell, &maps, SkirmishComboId::Color(2));
    assert!(after.contains(&SkirmishComboItem::Color(3)));
}

#[test]
fn ai_type_none_releases_color() {
    // AI row 1 (Easy, color 4) → None. color_claimed clears and AI row 2's
    // filter regains color 4.
    let mut shell = SkirmishShellState::default();
    shell.player_color_index = 0;
    shell.player_color_claimed = true;
    shell.opponents[0].row_type = SkirmishAiRowType::Easy;
    shell.opponents[0].color_index = 4;
    shell.opponents[0].color_claimed = true;
    shell.opponents[1].row_type = SkirmishAiRowType::Easy;
    shell.opponents[1].color_index = 7;
    shell.opponents[1].color_claimed = true;
    let maps = [test_map_entry("map.mmx")];

    let before = combo_items(&shell, &maps, SkirmishComboId::Color(2));
    assert!(!before.contains(&SkirmishComboItem::Color(4)));

    apply_combo_selection_for_test(
        &mut shell,
        SkirmishComboId::AiType(0),
        SkirmishComboItem::AiType(SkirmishAiRowType::None),
    );

    assert!(!shell.opponents[0].color_claimed);
    let after = combo_items(&shell, &maps, SkirmishComboId::Color(2));
    assert!(after.contains(&SkirmishComboItem::Color(4)));
}

#[test]
fn ai_type_reactivate_does_not_auto_claim() {
    // AI row 1 starts None+color_claimed=false; switching to Easy must NOT
    // silently set color_claimed to true. Another row may have taken its
    // prior color during the deactivation gap.
    let mut shell = SkirmishShellState::default();
    shell.opponents[0].row_type = SkirmishAiRowType::None;
    shell.opponents[0].color_index = 4;
    shell.opponents[0].color_claimed = false;

    apply_combo_selection_for_test(
        &mut shell,
        SkirmishComboId::AiType(0),
        SkirmishComboItem::AiType(SkirmishAiRowType::Easy),
    );

    assert_eq!(shell.opponents[0].row_type, SkirmishAiRowType::Easy);
    assert!(shell.opponents[0].enabled);
    assert!(
        !shell.opponents[0].color_claimed,
        "AI row 1 reactivation must NOT auto-claim its cached color"
    );
    // Cached color_index preserved — user can re-pick if still available.
    assert_eq!(shell.opponents[0].color_index, 4);
}

#[test]
fn color_filter_keeps_self_selection_visible_per_row() {
    // Every active row sees its own claimed color in its own dropdown,
    // even though every other row's filter would exclude it.
    let mut shell = SkirmishShellState::default();
    shell.player_color_index = 2;
    shell.player_color_claimed = true;
    shell.opponents[0].row_type = SkirmishAiRowType::Easy;
    shell.opponents[0].color_index = 5;
    shell.opponents[0].color_claimed = true;
    let maps = [test_map_entry("map.mmx")];

    let player_items = combo_items(&shell, &maps, SkirmishComboId::Color(0));
    assert!(player_items.contains(&SkirmishComboItem::Color(2)));
    assert!(!player_items.contains(&SkirmishComboItem::Color(5)));

    let ai_items = combo_items(&shell, &maps, SkirmishComboId::Color(1));
    assert!(ai_items.contains(&SkirmishComboItem::Color(5)));
    assert!(!ai_items.contains(&SkirmishComboItem::Color(2)));
}

#[test]
fn random_color_remains_the_selected_visible_sentinel() {
    let mut shell = SkirmishShellState::default();
    shell.player_color_index = 3;
    shell.player_color_claimed = false;
    shell.opponents[0].color_index = 6;
    shell.opponents[0].color_claimed = false;
    let maps = [test_map_entry("map.mmx")];

    assert_eq!(
        selected_combo_item(&shell, SkirmishComboId::Color(0)),
        Some(SkirmishComboItem::ColorSentinel(-2))
    );
    assert_eq!(
        selected_combo_item(&shell, SkirmishComboId::Color(1)),
        Some(SkirmishComboItem::ColorSentinel(-2))
    );
    assert_eq!(
        selected_combo_item_index(&shell, &maps, SkirmishComboId::Color(0)),
        Some(0)
    );
}

#[test]
fn launch_session_carries_random_color_marker_and_cached_placeholder() {
    // The shell retains its cached concrete index behind the visible sentinel;
    // the launch copy carries both that placeholder and the raw Random marker.
    let mut shell = SkirmishShellState::default();
    shell.player_color_index = 3;
    shell.player_color_claimed = false;
    shell.opponents[0].row_type = SkirmishAiRowType::Easy;
    shell.opponents[0].color_index = 6;
    shell.opponents[0].color_claimed = false;
    let maps = [test_map_entry("map.mmx")];
    let modes = stock_skirmish_modes();

    let session = launch_session(&shell, &maps, &modes).expect("session");

    assert_eq!(session.local.color_index, 3);
    assert!(session.local.color_random);
    assert_eq!(session.opponents[0].color_index, 6);
    assert!(session.opponents[0].color_random);
}

#[test]
fn all_colors_claimed_activation_leaves_row_without_claim() {
    // Defensive Ledger #11. Claim all 8 colors across 8 active rows,
    // deactivate one, give its color to another slot, reactivate it.
    // Activating must NOT re-grab a color or steal another row's.
    let mut shell = SkirmishShellState::default();
    shell.player_color_index = 0;
    shell.player_color_claimed = true;
    for (idx, opponent) in shell.opponents.iter_mut().enumerate() {
        opponent.row_type = SkirmishAiRowType::Easy;
        opponent.color_index = idx + 1; // colors 1..7
        opponent.color_claimed = true;
    }

    // Deactivate AI row 1 (slot index 0) — its color 1 is released.
    apply_combo_selection_for_test(
        &mut shell,
        SkirmishComboId::AiType(0),
        SkirmishComboItem::AiType(SkirmishAiRowType::None),
    );
    // Another slot grabs color 1 before AI row 1 reactivates.
    apply_combo_selection_for_test(
        &mut shell,
        SkirmishComboId::Color(2),
        SkirmishComboItem::Color(1),
    );
    // Reactivate AI row 1.
    apply_combo_selection_for_test(
        &mut shell,
        SkirmishComboId::AiType(0),
        SkirmishComboItem::AiType(SkirmishAiRowType::Easy),
    );

    assert_eq!(shell.opponents[0].row_type, SkirmishAiRowType::Easy);
    assert!(
        !shell.opponents[0].color_claimed,
        "reactivation must not silently re-grab a color another row took"
    );
    assert!(
        shell.opponents[1].color_claimed && shell.opponents[1].color_index == 1,
        "the other row's claim on color 1 must be preserved"
    );
}

#[test]
fn player_name_programmatic_seed_is_not_subject_to_edit_limit() {
    let long = "X".repeat(PLAYER_NAME_EDIT_LIMIT_BYTES + 5);
    let edit = PlayerNameEditState::with_name(&long);
    assert_eq!(edit.text, long);
    assert_eq!(edit.caret, PLAYER_NAME_EDIT_LIMIT_BYTES + 5);
    assert!(!edit.focused);

    let edit = PlayerNameEditState::with_name("Commander");
    assert_eq!(edit.text, "Commander");
    assert_eq!(edit.caret, "Commander".chars().count());
}

/// The help table lookup `0x006040B0` executed for `dialog`'s `control`
/// (`tools/storage_oracle/shell_help_keys.py`).
fn native_help_key(dialog: u64, control: u64) -> Option<String> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tools/storage_oracle/shell_help_keys.json"
    ))
    .unwrap();
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["dialog_id"] == dialog && case["control_id"] == control)
        .unwrap_or_else(|| panic!("no help case for {dialog:#x}/{control:#x}"));
    case["help"].as_str().map(str::to_owned)
}

#[test]
fn random_map_setup_help_keys_match_the_native_table() {
    use crate::ui::skirmish_shell::RandomMapSetupControl as C;
    for (control, id) in [
        (C::MapType0x405, 0x405),
        (C::Time0x3ea, 0x3EA),
        (C::Theater0x407, 0x407),
        (C::Size0x406, 0x406),
        (C::Resources0x408, 0x408),
        (C::Players0x3eb, 0x3EB),
        (C::Randomize0x621, 0x621),
        (C::Generate0x620, 0x620),
        (C::Ok0x6c5, 0x6C5),
        (C::Load0x6c2, 0x6C2),
        (C::Save0x6c3, 0x6C3),
        (C::Delete0x6c4, 0x6C4),
        (C::Cancel0x5c0, 0x5C0),
    ] {
        assert_eq!(
            Some(status_help_key_for_random_map_setup(control).to_owned()),
            native_help_key(0x105, id),
            "{id:#x}"
        );
    }
}

#[test]
fn choose_map_help_keys_match_the_native_table() {
    for (target, id) in [
        (ChooseMapHoverTarget::ModeList0x6eb, 0x6EB),
        (ChooseMapHoverTarget::MapList0x553, 0x553),
        (ChooseMapHoverTarget::Preview0x468, 0x468),
        (
            ChooseMapHoverTarget::Button(ChooseMapModalButton::UseMap0x6c5),
            0x6C5,
        ),
        (
            ChooseMapHoverTarget::Button(ChooseMapModalButton::CreateRandomMap0x583),
            0x583,
        ),
        (
            ChooseMapHoverTarget::Button(ChooseMapModalButton::Cancel0x5c0),
            0x5C0,
        ),
    ] {
        assert_eq!(
            status_help_key_for_choose_map_hover(target).map(str::to_owned),
            native_help_key(0x6B, id),
            "{id:#x}"
        );
    }
}
