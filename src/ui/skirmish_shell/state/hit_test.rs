//! Hit testing, hover targets, status-help keys, and action application for the skirmish shell.

use crate::map::scenario_menu::MapMenuEntry;
use crate::skirmish_launch::SKIRMISH_PLAYER_SLOT_COUNT;

use super::super::layout::{
    COMBO_DROPDOWN_ROW_H, ChooseMapModalButton, ChooseMapModalLayout, ColorComboId,
    RandomMapSetupControl, RectPx, SKIRMISH_AI_ROW_COUNT, SkirmishCheckboxId, SkirmishShellLayout,
    SkirmishTrackbarId, combo_face_rect,
};
use super::trackbars::{trackbar_ids, trackbar_rect};
use super::{
    ChooseMapHoverTarget, ChooseMapModalState, OwnerDrawButton, SkirmishAiRowType, SkirmishComboId,
    SkirmishComboItem, SkirmishCountryChoice, SkirmishHoverTarget, SkirmishShellAction,
    SkirmishShellState, accept_selected_map, combo_dropdown_content_rect,
    combo_dropdown_visible_row_count, combo_items, combo_rect, player_row_visible,
};

fn hover_open_combo_item(
    state: &SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    x: i32,
    y: i32,
) -> Option<SkirmishHoverTarget> {
    let open = state.open_combo_dropdown?;
    let content = combo_dropdown_content_rect(state, layout, maps, open.id)?;
    if !content.contains(x, y) {
        return None;
    }

    let visible_row = ((y - content.y) / COMBO_DROPDOWN_ROW_H).max(0) as usize;
    if visible_row >= combo_dropdown_visible_row_count(state, maps, open.id) {
        return None;
    }
    let item_index = open.top_index + visible_row;
    let item = combo_items(state, maps, open.id).get(item_index).copied()?;
    Some(SkirmishHoverTarget::ComboItem { id: open.id, item })
}

pub fn hovered_shell_control(
    layout: &SkirmishShellLayout,
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    x: i32,
    y: i32,
) -> Option<SkirmishHoverTarget> {
    if state.choose_map_modal.is_some() || state.validation_modal.is_some() {
        return None;
    }

    if let Some(target) = hover_open_combo_item(state, layout, maps, x, y) {
        return Some(target);
    }

    if layout.status_help.contains(x, y) {
        return Some(SkirmishHoverTarget::StatusHelp0x695);
    }
    if layout.player_name.contains(x, y) {
        return Some(SkirmishHoverTarget::PlayerName0x6a0);
    }
    if let Some(button) = hit_test_owner_draw_button(layout, x, y) {
        return Some(SkirmishHoverTarget::OwnerDrawButton(button));
    }
    if layout.map_preview.contains(x, y) {
        return Some(SkirmishHoverTarget::MapPreview0x468);
    }
    for checkbox in layout.checkboxes {
        if checkbox.rect.contains(x, y) {
            return Some(SkirmishHoverTarget::Checkbox(checkbox.id));
        }
    }
    for id in trackbar_ids() {
        if trackbar_rect(layout, id).contains(x, y) {
            return Some(SkirmishHoverTarget::Trackbar(id));
        }
    }
    for row in 0..SKIRMISH_PLAYER_SLOT_COUNT {
        if !player_row_visible(state, maps, row) {
            continue;
        }
        for id in [
            SkirmishComboId::Side(row),
            SkirmishComboId::Color(row),
            SkirmishComboId::Start(row),
            SkirmishComboId::Team(row),
        ] {
            if combo_rect(layout, id).is_some_and(|rect| combo_face_rect(rect).contains(x, y)) {
                return Some(SkirmishHoverTarget::ComboFace(id));
            }
        }
    }
    for row in 0..SKIRMISH_AI_ROW_COUNT {
        if !player_row_visible(state, maps, row + 1) {
            continue;
        }
        let id = SkirmishComboId::AiType(row);
        if combo_rect(layout, id).is_some_and(|rect| combo_face_rect(rect).contains(x, y)) {
            return Some(SkirmishHoverTarget::ComboFace(id));
        }
    }

    // Static (non-interactive) controls are tested last: they do not overlap
    // the interactive widgets above, so order does not change any result, and
    // gamemd does not restrict 0x102 status help to interactive widgets.
    for (row, flag) in layout.flags.iter().enumerate() {
        if !player_row_visible(state, maps, row) {
            continue;
        }
        if flag.contains(x, y) {
            return Some(SkirmishHoverTarget::FlagPicture);
        }
    }
    if layout.right_panel_text.game_type.contains(x, y) {
        return Some(SkirmishHoverTarget::GameTypeLabel0x6ec);
    }
    if layout.right_panel_text.map_label.contains(x, y) {
        return Some(SkirmishHoverTarget::ScenarioLabel0x5a8);
    }

    None
}

pub fn status_help_key_for_hover(target: SkirmishHoverTarget) -> Option<&'static str> {
    match target {
        SkirmishHoverTarget::StatusHelp0x695 => None,
        SkirmishHoverTarget::PlayerName0x6a0 => Some("STT:SkirmishEditPlayer"),
        SkirmishHoverTarget::MapPreview0x468 => Some("STT:SkirmishMapThumbnail"),
        SkirmishHoverTarget::OwnerDrawButton(OwnerDrawButton::StartGame0x617) => {
            Some("STT:SkirmishButtonStartGame")
        }
        SkirmishHoverTarget::OwnerDrawButton(OwnerDrawButton::ChooseMap0x5aa) => {
            Some("STT:SkirmishButtonChooseMap")
        }
        SkirmishHoverTarget::OwnerDrawButton(OwnerDrawButton::Back0x5c0) => {
            Some("STT:SkirmishButtonBack")
        }
        SkirmishHoverTarget::Checkbox(SkirmishCheckboxId::ShortGame0x54e) => {
            Some("STT:SkirmishCBoxShortGame")
        }
        SkirmishHoverTarget::Checkbox(SkirmishCheckboxId::McvRepacks0x693) => {
            Some("STT:SkirmishCBoxRedeploys")
        }
        SkirmishHoverTarget::Checkbox(SkirmishCheckboxId::CratesAppear0x696) => {
            Some("STT:SkirmishCBoxCrates")
        }
        SkirmishHoverTarget::Checkbox(SkirmishCheckboxId::SuperWeapons0x69a) => {
            Some("STT:SkirmishCBoxSWAllowed")
        }
        SkirmishHoverTarget::Checkbox(SkirmishCheckboxId::BuildOffAlly0x69d) => {
            Some("STT:SkirmishCBoxBuildOffAlly")
        }
        SkirmishHoverTarget::Trackbar(SkirmishTrackbarId::GameSpeed0x529) => {
            Some("STT:SkirmishSliderSpeed")
        }
        SkirmishHoverTarget::Trackbar(SkirmishTrackbarId::Credits0x511) => {
            Some("STT:SkirmishSliderCredits")
        }
        SkirmishHoverTarget::Trackbar(SkirmishTrackbarId::UnitCount0x50c) => {
            Some("STT:SkirmishSliderUnit")
        }
        SkirmishHoverTarget::FlagPicture => Some("STT:SkirmishPictureFlag"),
        SkirmishHoverTarget::GameTypeLabel0x6ec => Some("STT:SkirmishLabelGameType"),
        SkirmishHoverTarget::ScenarioLabel0x5a8 => Some("STT:SkirmishLabelScenario"),
        SkirmishHoverTarget::ComboFace(id) => status_help_key_for_combo(id),
        SkirmishHoverTarget::ComboItem {
            id: SkirmishComboId::AiType(_),
            item: SkirmishComboItem::AiType(row_type),
        } => status_help_key_for_ai_row_type(row_type),
        SkirmishHoverTarget::ComboItem {
            id: id @ SkirmishComboId::Side(_),
            item: SkirmishComboItem::Country(country),
        } => status_help_key_for_side_item(country).or_else(|| status_help_key_for_combo(id)),
        SkirmishHoverTarget::ComboItem {
            id: id @ SkirmishComboId::Color(_),
            item,
        } => status_help_key_for_color_item(item).or_else(|| status_help_key_for_combo(id)),
        SkirmishHoverTarget::ComboItem { id, .. } => status_help_key_for_combo(id),
    }
}

pub fn hovered_choose_map_modal_control(
    layout: &ChooseMapModalLayout,
    modal: &ChooseMapModalState,
    x: i32,
    y: i32,
) -> Option<ChooseMapHoverTarget> {
    if layout.status_help.contains(x, y) {
        return Some(ChooseMapHoverTarget::StatusHelp0x695);
    }
    if let Some(button) = super::super::layout::choose_map_modal_button_at(layout, x, y) {
        return Some(ChooseMapHoverTarget::Button(button));
    }
    if layout.preview.contains(x, y) {
        return Some(ChooseMapHoverTarget::Preview0x468);
    }
    if layout.mode_list.contains(x, y) {
        let rows = modal.mode_rows();
        if let Some(row) =
            modal
                .mode_geometry(layout)
                .row_at(rows.len(), modal.mode_top_index, x, y)
        {
            return Some(ChooseMapHoverTarget::ModeListRow0x6eb { mode_id: rows[row] });
        }
        return Some(ChooseMapHoverTarget::ModeList0x6eb);
    }
    if layout.map_list.contains(x, y) {
        return Some(ChooseMapHoverTarget::MapList0x553);
    }
    None
}

pub fn status_help_key_for_choose_map_hover(target: ChooseMapHoverTarget) -> Option<&'static str> {
    match target {
        ChooseMapHoverTarget::StatusHelp0x695 => None,
        ChooseMapHoverTarget::ModeList0x6eb | ChooseMapHoverTarget::ModeListRow0x6eb { .. } => {
            Some("STT:ScenarioListGameType")
        }
        ChooseMapHoverTarget::MapList0x553 => Some("STT:ScenarioListMaps"),
        ChooseMapHoverTarget::Preview0x468 => Some("STT:ScenarioMapThumbnail"),
        ChooseMapHoverTarget::Button(ChooseMapModalButton::UseMap0x6c5) => {
            Some("STT:ScenarioButtonUseMap")
        }
        ChooseMapHoverTarget::Button(ChooseMapModalButton::CreateRandomMap0x583) => {
            Some("STT:ScenarioButtonRandom")
        }
        ChooseMapHoverTarget::Button(ChooseMapModalButton::Cancel0x5c0) => {
            Some("STT:ScenarioButtonCancel")
        }
    }
}

/// The random-map dialog `0x105`'s status help: the table lookup
/// `0x006040B0` for its controls (`tools/storage_oracle/shell_help_keys.py`).
pub fn status_help_key_for_random_map_setup(control: RandomMapSetupControl) -> &'static str {
    use RandomMapSetupControl as C;
    match control {
        C::MapType0x405 => "STT:GenerateCBoxEnvironment",
        C::Time0x3ea => "STT:GenerateCBoxTime",
        C::Theater0x407 => "STT:GenerateCBoxTheater",
        C::Size0x406 => "STT:GenerateCBoxMapSize",
        C::Resources0x408 => "STT:GenerateCBoxResources",
        C::Players0x3eb => "STT:GenerateSliderNumPlayers",
        C::Randomize0x621 => "STT:GenerateButtonSurprise",
        C::Generate0x620 => "STT:GenerateButtonPreview",
        C::Ok0x6c5 => "STT:GenerateButtonUseMap",
        C::Load0x6c2 => "STT:GenerateButtonLoadMap",
        C::Save0x6c3 => "STT:GenerateButtonSaveMap",
        C::Delete0x6c4 => "STT:GenerateButtonDeleteMap",
        C::Cancel0x5c0 => "STT:GenerateButtonCancel",
    }
}

fn status_help_key_for_combo(id: SkirmishComboId) -> Option<&'static str> {
    match id {
        SkirmishComboId::AiType(_) => Some("STT:SkirmishComboAIPlayer"),
        SkirmishComboId::Side(_) => Some("STT:SkirmishComboCountry"),
        SkirmishComboId::Color(_) => Some("STT:SkirmishComboColor"),
        SkirmishComboId::Start(_) => Some("STT:HostComboStart"),
        SkirmishComboId::Team(_) => Some("STT:HostComboTeam"),
    }
}

fn status_help_key_for_ai_row_type(row_type: SkirmishAiRowType) -> Option<&'static str> {
    match row_type {
        SkirmishAiRowType::None => Some("STT:PlayerNone"),
        SkirmishAiRowType::Easy => Some("STT:PlayerDumbAI"),
        SkirmishAiRowType::Normal => Some("STT:PlayerSmartAI"),
        SkirmishAiRowType::Hard => Some("STT:PlayerGeniusAI"),
    }
}

fn status_help_key_for_side_item(country: SkirmishCountryChoice) -> Option<&'static str> {
    match country {
        SkirmishCountryChoice::Random => Some("STT:PlayerSideRandom"),
        SkirmishCountryChoice::Country(country) => match country {
            crate::ui::main_menu::SkirmishCountry::America => Some("STT:PlayerSideAmerica"),
            crate::ui::main_menu::SkirmishCountry::Korea => Some("STT:PlayerSideKorea"),
            crate::ui::main_menu::SkirmishCountry::France => Some("STT:PlayerSideFrance"),
            crate::ui::main_menu::SkirmishCountry::Germany => Some("STT:PlayerSideGermany"),
            crate::ui::main_menu::SkirmishCountry::GreatBritain => Some("STT:PlayerSideBritain"),
            crate::ui::main_menu::SkirmishCountry::Libya => Some("STT:PlayerSideLibya"),
            crate::ui::main_menu::SkirmishCountry::Iraq => Some("STT:PlayerSideIraq"),
            crate::ui::main_menu::SkirmishCountry::Cuba => Some("STT:PlayerSideCuba"),
            crate::ui::main_menu::SkirmishCountry::Russia => Some("STT:PlayerSideRussia"),
            crate::ui::main_menu::SkirmishCountry::Yuri => Some("STT:PlayerSideYuriCountry"),
        },
    }
}

fn status_help_key_for_color_item(item: SkirmishComboItem) -> Option<&'static str> {
    match item {
        SkirmishComboItem::ColorSentinel(-2) => Some("STT:PlayerColorRandom"),
        SkirmishComboItem::Color(0) => Some("STT:PlayerColorGold"),
        SkirmishComboItem::Color(1) => Some("STT:PlayerColorRed"),
        SkirmishComboItem::Color(2) => Some("STT:PlayerColorBlue"),
        SkirmishComboItem::Color(3) => Some("STT:PlayerColorGreen"),
        SkirmishComboItem::Color(4) => Some("STT:PlayerColorOrange"),
        SkirmishComboItem::Color(5) => Some("STT:PlayerColorSkyBlue"),
        SkirmishComboItem::Color(6) => Some("STT:PlayerColorPurple"),
        SkirmishComboItem::Color(7) => Some("STT:PlayerColorPink"),
        SkirmishComboItem::Color(8) => Some("STT:PlayerColorObserver"),
        _ => None,
    }
}

fn hit_rect(rect: RectPx, x: i32, y: i32, action: SkirmishShellAction) -> SkirmishShellAction {
    if rect.contains(x, y) {
        action
    } else {
        SkirmishShellAction::None
    }
}

pub fn action_for_owner_draw_button(button: OwnerDrawButton) -> SkirmishShellAction {
    match button {
        OwnerDrawButton::StartGame0x617 => SkirmishShellAction::StartGame,
        OwnerDrawButton::ChooseMap0x5aa => SkirmishShellAction::ChooseMap,
        OwnerDrawButton::Back0x5c0 => SkirmishShellAction::BackOrExit,
    }
}

pub fn hit_test_owner_draw_button(
    layout: &SkirmishShellLayout,
    x: i32,
    y: i32,
) -> Option<OwnerDrawButton> {
    if layout.start_button.contains(x, y) {
        return Some(OwnerDrawButton::StartGame0x617);
    }
    if layout.choose_map_button.contains(x, y) {
        return Some(OwnerDrawButton::ChooseMap0x5aa);
    }
    if layout.back_button.contains(x, y) {
        return Some(OwnerDrawButton::Back0x5c0);
    }
    None
}

pub fn hit_test(layout: &SkirmishShellLayout, x: i32, y: i32) -> SkirmishShellAction {
    let start = hit_rect(layout.start_button, x, y, SkirmishShellAction::StartGame);
    if start != SkirmishShellAction::None {
        return start;
    }

    let choose = hit_rect(
        layout.choose_map_button,
        x,
        y,
        SkirmishShellAction::ChooseMap,
    );
    if choose != SkirmishShellAction::None {
        return choose;
    }

    let back = hit_rect(layout.back_button, x, y, SkirmishShellAction::BackOrExit);
    if back != SkirmishShellAction::None {
        return back;
    }

    SkirmishShellAction::None
}

pub fn apply_action(
    state: &mut SkirmishShellState,
    action: SkirmishShellAction,
    maps: &[MapMenuEntry],
) -> SkirmishShellAction {
    match action {
        SkirmishShellAction::None => SkirmishShellAction::None,
        SkirmishShellAction::StartGame => SkirmishShellAction::StartGame,
        SkirmishShellAction::BackOrExit => SkirmishShellAction::BackOrExit,
        SkirmishShellAction::ChooseMap => SkirmishShellAction::ChooseMap,
        SkirmishShellAction::SelectMap(idx) => {
            accept_selected_map(state, maps, idx);
            SkirmishShellAction::None
        }
        SkirmishShellAction::SelectColor(target) => {
            match target {
                ColorComboId::Player => {
                    state.player_color_index = (state.player_color_index + 1) % 8;
                }
                ColorComboId::Ai(idx) => {
                    let visible = player_row_visible(state, maps, idx + 1);
                    if visible && let Some(opponent) = state.opponents.get_mut(idx) {
                        opponent.color_index = (opponent.color_index + 1) % 8;
                    }
                }
            }
            SkirmishShellAction::None
        }
    }
}
