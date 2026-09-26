//! Text draw helpers for the skirmish shell renderer.
//!
//! Owns localized labels, text rect conversion, modal/listbox text,
//! status help text, and start-marker number labels.

use crate::app::AppState;
use crate::app::frontend::main_menu_shell_render::shell_reveal_path_a;
use crate::app::loading::init::MapMenuEntry;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::shell_paint::{self, PaintLabel};
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw, TextRect};
use crate::skirmish_modes::mode_by_id;
use crate::ui::main_menu::SkirmishCountry;
use crate::ui::shell::modal::BodyOkLayout;
use crate::ui::shell::static_reveal::StaticPaint;
use crate::ui::skirmish_shell::{
    CHOOSE_MAP_TITLE_KEY, COMBO_DROPDOWN_ROW_H, COMBO_FACE_H, COMBO_TEXT_LEFT_INSET,
    ChooseMapModalButton, ChooseMapModalLayout, OwnerDrawButton, RandomMapSetupControl,
    RandomMapSetupLayout, RectPx, SETUP_COMBO_ROWS, SavedSeedLayout, SkirmishAiRowType,
    SkirmishCheckboxId, SkirmishComboId, SkirmishComboItem, SkirmishCountryChoice,
    SkirmishShellLayout, SkirmishShellOpponent, SkirmishShellState, SkirmishTrackbarId,
    checkbox_text_rect, combo_dropdown_content_rect, combo_dropdown_rect,
    combo_dropdown_visible_row_count, combo_enabled, combo_items, combo_text_rect,
    player_name_edit_text_rect, player_row_visible, random_map_setup_dropdown_rect,
    setup_combo_items, trackbar_value_text_rect, trackbar_visual_value,
};

use super::controls::trackbar_rect_for_id;
use super::{
    COMBODROPWIN_TEXT_INSET_X, COMBODROPWIN_TEXT_TRUNCATION_SCROLLBAR_RESERVE_PX,
    SHELL_CONTROL_TEXT_DEPTH, SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F,
    SHELL_DROPDOWN_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB,
};

pub(super) fn localized_label(state: &AppState, key: &str, fallback: &str) -> String {
    state
        .process_assets.csf
        .as_ref()
        .map(|csf| csf.text(key).into_owned())
        .unwrap_or_else(|| fallback.to_string())
}

pub(super) fn checkbox_label(id: SkirmishCheckboxId) -> (&'static str, &'static str) {
    match id {
        SkirmishCheckboxId::ShortGame0x54e => ("GUI:ShortGame", "Short Game"),
        SkirmishCheckboxId::McvRepacks0x693 => ("GUI:MCVRepacks", "MCV Repacks"),
        SkirmishCheckboxId::CratesAppear0x696 => ("GUI:CratesAppear", "Crates Appear"),
        SkirmishCheckboxId::SuperWeapons0x69a => ("GUI:SuperWeaponsAllowed", "Super Weapons"),
        SkirmishCheckboxId::BuildOffAlly0x69d => ("GUI:BuildOffAlly", "Build Off Ally"),
    }
}

fn team_label_spec(team: i32) -> (&'static str, &'static str) {
    match team {
        0 => ("LETTER_A", "A"),
        1 => ("LETTER_B", "B"),
        2 => ("LETTER_C", "C"),
        3 => ("LETTER_D", "D"),
        _ => ("GUI:NoneAsSymbols", "None"),
    }
}

pub(super) fn team_label(state: &AppState, team: i32) -> String {
    let (key, fallback) = team_label_spec(team);
    localized_label(state, key, fallback)
}

pub(super) fn combo_item_label(state: &AppState, item: SkirmishComboItem) -> String {
    match item {
        SkirmishComboItem::AiType(row_type) => {
            let (key, fallback) = row_type_label(row_type);
            localized_label(state, key, fallback)
        }
        SkirmishComboItem::Country(SkirmishCountryChoice::Random) => {
            localized_label(state, "GUI:RandomAsSymbols", "Random")
        }
        SkirmishComboItem::Country(SkirmishCountryChoice::Country(country)) => {
            country.label().to_string()
        }
        SkirmishComboItem::ColorSentinel(_) => {
            // The source-line immediate `0x20A` is not a string id; the
            // adjacent load supplies `GUI:RandomAsSymbols` to the color combo.
            localized_label(state, "GUI:RandomAsSymbols", "Random")
        }
        SkirmishComboItem::Color(_) => String::new(),
        SkirmishComboItem::Start(start) => match start {
            crate::ui::main_menu::StartPosition::Auto => {
                localized_label(state, "GUI:RandomAsSymbols", "Random")
            }
            crate::ui::main_menu::StartPosition::Position(idx) => (idx + 1).to_string(),
        },
        SkirmishComboItem::Team(team) => team_label(state, team),
    }
}

pub(super) fn country_choice_label(
    state: &AppState,
    random: bool,
    country: SkirmishCountry,
) -> String {
    if random {
        localized_label(state, "GUI:RandomAsSymbols", "Random")
    } else {
        country.label().to_string()
    }
}

pub(super) fn trackbar_display_value(shell: &SkirmishShellState, id: SkirmishTrackbarId) -> String {
    match id {
        SkirmishTrackbarId::GameSpeed0x529 => trackbar_visual_value(shell, id).to_string(),
        SkirmishTrackbarId::Credits0x511 => shell.starting_credits.to_string(),
        SkirmishTrackbarId::UnitCount0x50c => shell.unit_count.to_string(),
    }
}

pub(super) fn trackbar_value_text_color() -> [f32; 3] {
    SHELL_LABEL_TEXT_RGB
}

pub(super) fn trackbar_label(state: &AppState, id: SkirmishTrackbarId) -> String {
    match id {
        SkirmishTrackbarId::GameSpeed0x529 => localized_label(state, "GUI:GameSpeed", "Game Speed"),
        SkirmishTrackbarId::Credits0x511 => localized_label(state, "GUI:Credits", "Credits"),
        SkirmishTrackbarId::UnitCount0x50c => localized_label(state, "GUI:UnitCount", "Unit Count"),
    }
}

pub(super) fn trackbar_label_rect_for_id(
    layout: &SkirmishShellLayout,
    id: SkirmishTrackbarId,
) -> RectPx {
    match id {
        SkirmishTrackbarId::GameSpeed0x529 => layout.trackbar_labels.game_speed,
        SkirmishTrackbarId::Credits0x511 => layout.trackbar_labels.credits,
        SkirmishTrackbarId::UnitCount0x50c => layout.trackbar_labels.unit_count,
    }
}

pub(super) fn row_type_label(row_type: SkirmishAiRowType) -> (&'static str, &'static str) {
    row_type.label()
}

pub(super) fn push_button_label_draw(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    label: &str,
    rect: RectPx,
    pressed: bool,
    depth: f32,
) {
    let text_rect = button_text_rect(rect, pressed);
    push_text_draw(
        out,
        state,
        label,
        text_rect,
        button_label_color(),
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        depth,
    );
}

pub(super) fn button_label_color() -> [f32; 3] {
    SHELL_LABEL_TEXT_RGB
}

/// Retail provenance: disabled `OwnerDraw_Button_00612B70` replaces the normal
/// yellow label with its disabled color at `0x00612F5F`
/// ([`crate::render::shell_paint::SHELL_TEXT_RGB_DISABLED`]).
pub(super) fn button_label_color_for_disabled(disabled: bool) -> [f32; 3] {
    if disabled {
        crate::render::shell_paint::SHELL_TEXT_RGB_DISABLED
    } else {
        button_label_color()
    }
}

pub(super) fn combo_face_text_color(disabled: bool) -> [f32; 3] {
    if disabled {
        SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F
    } else {
        SHELL_LABEL_TEXT_RGB
    }
}

fn opponent_sibling_combo_text_color(opponent: &SkirmishShellOpponent) -> [f32; 3] {
    combo_face_text_color(!opponent.is_active())
}

pub(super) fn button_text_rect(rect: RectPx, pressed: bool) -> TextRect {
    let mut x = rect.x;
    let y = if pressed {
        x += 2;
        rect.y + 5
    } else {
        rect.y + 1
    };
    TextRect {
        x,
        y,
        w: (rect.x + rect.w - 2 - x).max(0) as u32,
        h: (rect.y + rect.h - y).max(0) as u32,
    }
}

fn button_label_rect_px(rect: RectPx, pressed: bool) -> RectPx {
    let rect = button_text_rect(rect, pressed);
    RectPx::new(rect.x, rect.y, rect.w as i32, rect.h as i32)
}

pub(super) const fn validation_modal_body_text_align() -> ShellAlign {
    ShellAlign::NONE
}

pub(super) fn push_text_draw(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    label: &str,
    text_rect: TextRect,
    color: [f32; 3],
    align: ShellAlign,
    depth: f32,
) {
    out.push(shell_text::draw_in_rect(
        &state.renderer.bit_font,
        label,
        text_rect,
        color,
        align,
        [0.0, 0.0],
        depth,
    ));
}

pub(super) fn rect_to_text_rect(rect: RectPx) -> TextRect {
    TextRect {
        x: rect.x,
        y: rect.y,
        w: rect.w.max(0) as u32,
        h: rect.h.max(0) as u32,
    }
}

pub(super) fn combo_dropdown_text_rect_for_current_renderer(
    content: RectPx,
    visible_row: usize,
) -> TextRect {
    // `ComboDropWin` draws from x+3 to the row right edge. The separate
    // client_width-20 value is only the caller-side fit limit.
    TextRect {
        x: content.x + COMBODROPWIN_TEXT_INSET_X,
        y: content.y + visible_row as i32 * COMBO_DROPDOWN_ROW_H,
        w: (content.w - COMBODROPWIN_TEXT_INSET_X).max(0) as u32,
        h: COMBO_DROPDOWN_ROW_H.max(0) as u32,
    }
}

pub(super) fn combo_dropdown_text_fit_width(content: RectPx) -> u32 {
    (content.w - COMBODROPWIN_TEXT_TRUNCATION_SCROLLBAR_RESERVE_PX).max(0) as u32
}

pub(super) fn combo_face_text_fit_width(rect: RectPx) -> u32 {
    combo_text_rect(rect).w.max(0) as u32
}

pub(super) fn combo_face_text_draw_rect(rect: RectPx) -> RectPx {
    RectPx::new(
        rect.x + COMBO_TEXT_LEFT_INSET,
        rect.y,
        (rect.w - COMBO_TEXT_LEFT_INSET).max(0),
        COMBO_FACE_H,
    )
}

pub(super) fn truncate_owner_draw_label<'a>(
    font: &BitFont,
    label: &'a str,
    fit_width: u32,
) -> std::borrow::Cow<'a, str> {
    let mut utf16: Vec<u16> = label.encode_utf16().collect();
    if owner_draw_utf16_text_width(font, &utf16) <= fit_width {
        return std::borrow::Cow::Borrowed(label);
    }

    while !utf16.is_empty() && owner_draw_utf16_text_width(font, &utf16) > fit_width {
        utf16.pop();
    }
    std::borrow::Cow::Owned(String::from_utf16_lossy(&utf16))
}

fn owner_draw_utf16_text_width(font: &BitFont, units: &[u16]) -> u32 {
    const TAB: u16 = b'\t' as u16;
    const CR: u16 = b'\r' as u16;
    const LF: u16 = b'\n' as u16;
    const SPACE: u16 = b' ' as u16;

    let mut x: u32 = 0;
    let mut count: u32 = 0;
    for unit in units.iter().copied() {
        match unit {
            0 => break,
            TAB => {
                let advanced = x + font.tab_width;
                x = advanced - ((advanced.saturating_sub(font.tab_origin)) % font.tab_width);
                continue;
            }
            CR | LF => continue,
            SPACE => {
                x += font.space_width;
                count += 1;
            }
            other => {
                let w = font
                    .glyphs
                    .get(&other)
                    .map(|g| g.pixel_width as u32)
                    .or_else(|| font.missing_glyph.as_ref().map(|g| g.pixel_width as u32));
                if let Some(w) = w {
                    x += w;
                    count += 1;
                }
            }
        }
    }
    x + count * font.char_spacing
}

pub(super) fn truncate_combo_dropdown_label<'a>(
    font: &BitFont,
    label: &'a str,
    fit_width: u32,
) -> std::borrow::Cow<'a, str> {
    truncate_owner_draw_label(font, label, fit_width)
}

pub(super) fn push_label_draw(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    label: &str,
    rect: RectPx,
    depth: f32,
) {
    let text_rect = TextRect {
        x: rect.x,
        y: rect.y,
        w: rect.w.max(0) as u32,
        h: rect.h.max(0) as u32,
    };
    push_text_draw(
        out,
        state,
        label,
        text_rect,
        SHELL_LABEL_TEXT_RGB,
        ShellAlign::V_CENTER,
        depth,
    );
}

fn rects_intersect(a: RectPx, b: RectPx) -> bool {
    a.w > 0
        && a.h > 0
        && b.w > 0
        && b.h > 0
        && a.x < b.x + b.w
        && a.x + a.w > b.x
        && a.y < b.y + b.h
        && a.y + a.h > b.y
}

fn text_covered_by_overlay(rect: RectPx, overlays: &[RectPx]) -> bool {
    overlays
        .iter()
        .any(|overlay| rects_intersect(rect, *overlay))
}

fn push_combo_face_label_draw(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    label: &str,
    rect: RectPx,
    covering_overlays: &[RectPx],
) {
    push_combo_face_label_draw_with_color(
        out,
        state,
        label,
        rect,
        SHELL_LABEL_TEXT_RGB,
        covering_overlays,
    );
}

fn push_combo_face_label_draw_with_color(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    label: &str,
    rect: RectPx,
    color: [f32; 3],
    covering_overlays: &[RectPx],
) {
    let text_rect = combo_face_text_draw_rect(rect);
    if text_covered_by_overlay(text_rect, covering_overlays) {
        return;
    }
    let fit_width = combo_face_text_fit_width(rect);
    let label = truncate_owner_draw_label(&state.renderer.bit_font, label, fit_width);
    push_text_draw(
        out,
        state,
        label.as_ref(),
        rect_to_text_rect(text_rect),
        color,
        ShellAlign::V_CENTER,
        SHELL_CONTROL_TEXT_DEPTH,
    );
}

/// The texts `0x102` holds for its heading, game type and map name statics
/// (`0x006AEC86`, `0x006AEC8D`; again after Use Map, `0x006ADA99`); its SHOW
/// completion hands them to [`crate::ui::skirmish_shell::SkirmishStatics`].
pub(crate) fn skirmish_right_panel_label_strings(state: &AppState) -> (String, String, String) {
    let shell = &state.frontend.skirmish_shell_state;
    let title = localized_label(state, "GUI:SkirmishGame", "Skirmish Game");
    let game_type = state
        .frontend.skirmish_modes
        .iter()
        .find(|mode| mode.id == shell.selected_mode_id)
        .map(|mode| localized_label(state, &mode.ui_name_key, &mode.ui_name_key))
        .unwrap_or_else(|| localized_label(state, "GUI:Battle", "Battle"));
    let map_label = state
        .frontend.scenario_catalog.shell_maps()
        .get(shell.selected_map_idx)
        .map(|map| map.display_name.clone())
        .unwrap_or_else(|| "None".to_string());
    (title, game_type, map_label)
}

/// A kind-1 static's label: the Path-A reveal of its text in its window
/// (kind-1 paint passes the window's horizontal alignment only,
/// `0x00615A81`).
pub(super) fn static_label(
    shown: StaticPaint<'_>,
    rect: RectPx,
    align: ShellAlign,
) -> PaintLabel<'static> {
    PaintLabel {
        text: shown.text.to_owned().into(),
        rect,
        align,
        rgb: SHELL_LABEL_TEXT_RGB,
        path_a_reveal: Some(shell_reveal_path_a(shown.window)),
    }
}

/// A status line's label, top-left in its window; nothing while it is
/// hidden or blank.
pub(super) fn status_line_label(
    shown: Option<StaticPaint<'_>>,
    rect: RectPx,
) -> Option<PaintLabel<'static>> {
    shown
        .filter(|shown| !shown.text.is_empty())
        .map(|shown| static_label(shown, rect, ShellAlign::NONE))
}

/// Paint `0x102`'s kind-1 statics for this recomposition. While a slide runs
/// it blits the top panel over the heading, game type and map name; the
/// status line keeps what it painted when the dialog showed.
pub(super) fn paint_skirmish_statics(
    state: &mut AppState,
    layout: &SkirmishShellLayout,
    sliding: bool,
) -> Vec<PaintLabel<'static>> {
    let now = std::time::Instant::now();
    let statics = &mut state.frontend.skirmish_shell_state.statics;
    let mut labels = Vec::with_capacity(4);
    if !sliding {
        let panel = statics.paint_right_panel(now);
        let text = &layout.right_panel_text;
        labels.extend(
            [
                (panel.heading, text.title),
                (panel.game_type, text.game_type),
                (panel.map_label, text.map_label),
            ]
            .into_iter()
            .filter_map(|(shown, rect)| {
                shown.map(|shown| static_label(shown, rect, ShellAlign::H_CENTER))
            }),
        );
    }
    labels.extend(status_line_label(
        statics.paint_status_line(now),
        layout.status_help,
    ));
    labels
}

pub(super) fn push_player_name_edit_text_draw(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    shell: &SkirmishShellState,
    layout: &SkirmishShellLayout,
) {
    let rect = player_name_edit_text_rect(layout.player_name);
    let scissor = shell_text::ScissorRect {
        x: rect.x.max(0) as u32,
        y: rect.y.max(0) as u32,
        w: rect.w.max(0) as u32,
        h: rect.h.max(0) as u32,
    };
    if shell.player_name_edit.text.is_empty() {
        out.push(ShellTextDraw {
            instances: Vec::new(),
            scissor,
        });
        return;
    }

    let line_y =
        rect.y as f32 + ((rect.h as f32 - state.renderer.bit_font.glyph_height()).max(0.0) / 2.0).floor();
    let instances = state.renderer.bit_font.build_text(
        &shell.player_name_edit.text,
        (rect.x - shell.player_name_edit.scroll_x.max(0)) as f32,
        line_y,
        1.0,
        SHELL_CONTROL_TEXT_DEPTH,
        SHELL_LABEL_TEXT_RGB,
        [0.0, 0.0],
    );
    out.push(ShellTextDraw { instances, scissor });
}

/// The dialog's text. While a slide runs (`sliding`) the right panel shows only
/// the slide engine's art (the captions are overdrawn every tick) and the
/// kind-1 statics (`statics`, from [`paint_skirmish_statics`]) stay hidden;
/// the left-side controls keep what they painted.
pub(super) fn build_shell_text_draws(
    state: &AppState,
    layout: &SkirmishShellLayout,
    shell: &SkirmishShellState,
    maps: &[MapMenuEntry],
    sliding: bool,
    statics: &[PaintLabel<'_>],
) -> (Vec<ShellTextDraw>, Vec<SpriteInstance>) {
    let mut shell_draws: Vec<ShellTextDraw> = Vec::new();
    let bare_instances: Vec<SpriteInstance> = Vec::new();
    let mut covering_overlays = Vec::new();
    if let Some(dropdown) = shell
        .open_combo_dropdown
        .and_then(|open| combo_dropdown_rect(shell, layout, maps, open.id))
    {
        covering_overlays.push(dropdown);
    }

    if !sliding {
        push_right_panel_text_draws(&mut shell_draws, state, layout, shell);
    }
    shell_draws.extend(shell_paint::paint_labels_at_depth(
        &state.renderer.bit_font,
        statics,
        SHELL_CONTROL_TEXT_DEPTH,
    ));

    for (key, fallback, rect) in [
        ("GUI:Players", "Players", layout.column_labels.players),
        ("GUI:Side", "Side", layout.column_labels.side),
        ("GUI:Color", "Color", layout.column_labels.color),
        ("GUI:StartPosition", "Start", layout.column_labels.start),
        ("GUI:Team", "Team", layout.column_labels.team),
    ] {
        let label = localized_label(state, key, fallback);
        push_label_draw(
            &mut shell_draws,
            state,
            &label,
            rect,
            SHELL_CONTROL_TEXT_DEPTH,
        );
    }

    push_player_name_edit_text_draw(&mut shell_draws, state, shell, layout);

    for checkbox in layout.checkboxes {
        let (key, fallback) = checkbox_label(checkbox.id);
        let label = localized_label(state, key, fallback);
        push_label_draw(
            &mut shell_draws,
            state,
            &label,
            checkbox_text_rect(checkbox.rect),
            SHELL_CONTROL_TEXT_DEPTH,
        );
    }

    for id in [
        SkirmishTrackbarId::GameSpeed0x529,
        SkirmishTrackbarId::Credits0x511,
        SkirmishTrackbarId::UnitCount0x50c,
    ] {
        let label = trackbar_label(state, id);
        push_label_draw(
            &mut shell_draws,
            state,
            &label,
            trackbar_label_rect_for_id(layout, id),
            SHELL_CONTROL_TEXT_DEPTH,
        );

        let value = trackbar_display_value(shell, id);
        push_text_draw(
            &mut shell_draws,
            state,
            &value,
            rect_to_text_rect(trackbar_value_text_rect(trackbar_rect_for_id(layout, id))),
            trackbar_value_text_color(),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            SHELL_CONTROL_TEXT_DEPTH,
        );
    }

    push_combo_face_label_draw(
        &mut shell_draws,
        state,
        &country_choice_label(state, shell.player_country_random, shell.player_country),
        layout.rows.side_combos[0],
        &covering_overlays,
    );
    if !shell.player_color_claimed {
        let random_color = localized_label(state, "GUI:RandomAsSymbols", "Random");
        push_combo_face_label_draw(
            &mut shell_draws,
            state,
            &random_color,
            layout.color_combos[0],
            &covering_overlays,
        );
    }
    push_combo_face_label_draw(
        &mut shell_draws,
        state,
        &combo_item_label(state, SkirmishComboItem::Start(shell.player_start_position)),
        layout.rows.start_combos[0],
        &covering_overlays,
    );
    push_combo_face_label_draw_with_color(
        &mut shell_draws,
        state,
        &team_label(state, shell.player_team),
        layout.rows.team_combos[0],
        // The local Team combo greys with the mode's AlliesAllowed gate; every
        // other local-row face is unconditionally live.
        combo_face_text_color(!combo_enabled(shell, maps, SkirmishComboId::Team(0))),
        &covering_overlays,
    );

    for (idx, opponent) in shell.opponents.iter().enumerate() {
        if idx >= layout.rows.ai_type_combos.len() {
            break;
        }
        let row = idx + 1;
        if !player_row_visible(shell, maps, row) {
            continue;
        }
        let (key, fallback) = row_type_label(opponent.row_type);
        let row_type = localized_label(state, key, fallback);
        push_combo_face_label_draw(
            &mut shell_draws,
            state,
            &row_type,
            layout.rows.ai_type_combos[idx],
            &covering_overlays,
        );
        let sibling_text_color = opponent_sibling_combo_text_color(opponent);
        push_combo_face_label_draw_with_color(
            &mut shell_draws,
            state,
            &country_choice_label(state, opponent.country_random, opponent.country),
            layout.rows.side_combos[row],
            sibling_text_color,
            &covering_overlays,
        );
        if !opponent.color_claimed {
            let random_color = localized_label(state, "GUI:RandomAsSymbols", "Random");
            push_combo_face_label_draw_with_color(
                &mut shell_draws,
                state,
                &random_color,
                layout.color_combos[row],
                sibling_text_color,
                &covering_overlays,
            );
        }
        push_combo_face_label_draw_with_color(
            &mut shell_draws,
            state,
            &combo_item_label(state, SkirmishComboItem::Start(opponent.start_position)),
            layout.rows.start_combos[row],
            sibling_text_color,
            &covering_overlays,
        );
        push_combo_face_label_draw_with_color(
            &mut shell_draws,
            state,
            &team_label(state, opponent.team),
            layout.rows.team_combos[row],
            // Row-active alone does not decide the Team face: the mode's
            // AlliesAllowed gate greys it as well.
            combo_face_text_color(!combo_enabled(shell, maps, SkirmishComboId::Team(row))),
            &covering_overlays,
        );
    }

    if let Some(open) = shell
        .open_combo_dropdown
        .filter(|open| combo_enabled(shell, maps, open.id))
    {
        if let Some(dropdown) = combo_dropdown_rect(shell, layout, maps, open.id) {
            let content =
                combo_dropdown_content_rect(shell, layout, maps, open.id).unwrap_or(dropdown);
            let visible_rows = combo_dropdown_visible_row_count(shell, maps, open.id);
            for (idx, item) in combo_items(shell, maps, open.id)
                .into_iter()
                .skip(open.top_index)
                .take(visible_rows)
                .enumerate()
            {
                let label = combo_item_label(state, item);
                if label.is_empty() {
                    continue;
                }
                let rect = combo_dropdown_text_rect_for_current_renderer(content, idx);
                let fit_width = combo_dropdown_text_fit_width(content);
                let label = truncate_combo_dropdown_label(&state.renderer.bit_font, &label, fit_width);
                push_text_draw(
                    &mut shell_draws,
                    state,
                    label.as_ref(),
                    rect,
                    SHELL_LABEL_TEXT_RGB,
                    ShellAlign::V_CENTER,
                    SHELL_DROPDOWN_TEXT_DEPTH,
                );
            }
        }
    }

    (shell_draws, bare_instances)
}

/// The right panel's button captions.
fn push_right_panel_text_draws(
    shell_draws: &mut Vec<ShellTextDraw>,
    state: &AppState,
    layout: &SkirmishShellLayout,
    shell: &SkirmishShellState,
) {
    let start = localized_label(state, "GUI:StartGame", "Start Game");
    let choose = localized_label(state, "GUI:ChooseMap", "Choose Map");
    let back = localized_label(state, "GUI:Back", "Back");

    for (label, rect, button) in [
        (
            start.as_str(),
            layout.start_button,
            OwnerDrawButton::StartGame0x617,
        ),
        (
            choose.as_str(),
            layout.choose_map_button,
            OwnerDrawButton::ChooseMap0x5aa,
        ),
        (
            back.as_str(),
            layout.back_button,
            OwnerDrawButton::Back0x5c0,
        ),
    ] {
        push_button_label_draw(
            shell_draws,
            state,
            label,
            rect,
            shell.pressed_owner_draw_button == Some(button),
            0.00041,
        );
    }
}

/// Text for the random-map setup dialog `0x105`.
///
/// The six row labels are `SS_LEFT` in the resource (style `0x50000200`), unlike
/// choose-map's headings which are `SS_CENTER` (`0x50000201`), so they are drawn
/// left-aligned and vertically centred.
///
/// The combo faces render no selection text yet: the item lists come from the
/// dialog's populate path and still need their string-table sources decoded.
/// The players trackbar sits below the five combo rows.
const PLAYERS_ROW: usize = 5;

pub(super) fn push_random_map_setup_modal_text_draws(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    layout: &RandomMapSetupLayout,
    // While a slide runs the column draws the right buttons without captions.
    sliding: bool,
) {
    let Some(modal) = state.frontend.skirmish_shell_state.random_map_setup_modal.as_ref() else {
        return;
    };

    // Row order: map type, time, theater, size, resources, players. The 0x405
    // label reads "Environment" even though the control writes the map type.
    for (row, (key, fallback)) in [
        ("GUI:Environment", "Environment"),
        ("GUI:TimeOfDay", "Time of Day"),
        ("GUI:Theater", "Theater"),
        ("GUI:MapSize", "Map Size"),
        ("GUI:Resources", "Resources"),
        ("GUI:Players", "Players"),
    ]
    .into_iter()
    .enumerate()
    {
        push_text_draw(
            out,
            state,
            &localized_label(state, key, fallback),
            rect_to_text_rect(layout.label_rects[row]),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00008,
        );
    }

    for (key, fallback, rect, control) in [
        (
            "GUI:SurpriseMe",
            "Surprise Me",
            layout.randomize,
            RandomMapSetupControl::Randomize0x621,
        ),
        (
            "GUI:PreviewMap",
            "Preview Map",
            layout.generate,
            RandomMapSetupControl::Generate0x620,
        ),
    ] {
        push_text_draw(
            out,
            state,
            &localized_label(state, key, fallback),
            rect_to_text_rect(rect),
            button_label_color_for_disabled(!modal.is_enabled(control)),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00009,
        );
    }
    // The column's owner-draw type-1 captions, as on `0x102` and `0x6B`.
    for (key, fallback, rect, control) in [
        (
            "GUI:UseMap",
            "Use Map",
            layout.use_map,
            RandomMapSetupControl::Ok0x6c5,
        ),
        (
            "GUI:LoadMap",
            "Load Map",
            layout.load,
            RandomMapSetupControl::Load0x6c2,
        ),
        (
            "GUI:SaveMap",
            "Save Map",
            layout.save,
            RandomMapSetupControl::Save0x6c3,
        ),
        (
            "GUI:DeleteMap",
            "Delete Map",
            layout.delete,
            RandomMapSetupControl::Delete0x6c4,
        ),
        (
            "GUI:Cancel",
            "Cancel",
            layout.cancel,
            RandomMapSetupControl::Cancel0x5c0,
        ),
    ]
    .into_iter()
    .filter(|_| !sliding)
    {
        push_text_draw(
            out,
            state,
            &localized_label(state, key, fallback),
            button_text_rect(rect, modal.pressed_control == Some(control)),
            button_label_color_for_disabled(!modal.is_enabled(control)),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00009,
        );
    }

    // Selected entry on each closed combo face. A value with no entry -- map
    // type 0, which the list omits -- leaves the face blank, as the original's
    // match-the-entry selection does.
    for (row, combo) in SETUP_COMBO_ROWS.iter().enumerate() {
        let items = setup_combo_items(*combo);
        let Some(selected) = modal.selected_item_index(*combo) else {
            continue;
        };
        let entry = items[selected];
        push_text_draw(
            out,
            state,
            &localized_label(state, entry.key, entry.fallback),
            rect_to_text_rect(combo_text_rect(layout.control_rects[row])),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00009,
        );
    }

    // The players trackbar carries its value in the plaque at its right end.
    push_text_draw(
        out,
        state,
        &modal.options.num_players.to_string(),
        rect_to_text_rect(trackbar_value_text_rect(layout.control_rects[PLAYERS_ROW])),
        SHELL_LABEL_TEXT_RGB,
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        SHELL_DROPDOWN_TEXT_DEPTH - 0.00009,
    );

    if modal.generating {
        push_text_draw(
            out,
            state,
            &localized_label(state, "GUI:WorkingPleaseWait", "Working, please wait..."),
            rect_to_text_rect(layout.progress_text),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.0001,
        );
    }

    // Matches the sprite pass: the open list is drawn over everything else.
    if let Some(combo) = modal.open_combo {
        let row = combo.row();
        let items = setup_combo_items(combo);
        let list = random_map_setup_dropdown_rect(layout, row, items.len());
        for (index, entry) in items.iter().enumerate() {
            let row_rect = RectPx::new(
                list.x + COMBO_TEXT_LEFT_INSET,
                list.y + COMBO_DROPDOWN_ROW_H * index as i32,
                list.w - COMBO_TEXT_LEFT_INSET,
                COMBO_DROPDOWN_ROW_H,
            );
            push_text_draw(
                out,
                state,
                &localized_label(state, entry.key, entry.fallback),
                rect_to_text_rect(row_rect),
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::V_CENTER,
                SHELL_DROPDOWN_TEXT_DEPTH - 0.00011,
            );
        }
    }
}

/// Text of Choose Map `0x6B`. The heading is the family kind-1 static (its
/// reveal starts when the entry slide ends); the status line is painted by
/// the caller. While a slide runs the column draws the buttons without
/// captions; the labels and lists (left-side children) still paint.
pub(super) fn push_choose_map_modal_text_draws(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    layout: &ChooseMapModalLayout,
    title: Option<crate::ui::shell::static_reveal::Kind1RevealWindow>,
    sliding: bool,
) {
    let Some(modal) = state.frontend.skirmish_shell_state.choose_map_modal.as_ref() else {
        return;
    };
    if let Some(window) = title {
        let heading = localized_label(state, CHOOSE_MAP_TITLE_KEY, "Choose Map");
        out.push(shell_text::draw_in_rect_path_a(
            &state.renderer.bit_font,
            &heading,
            rect_to_text_rect(layout.title),
            ShellAlign::H_CENTER,
            [0.0; 2],
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00008,
            crate::app::frontend::main_menu_shell_render::shell_reveal_path_a(window),
        ));
    }
    for (key, fallback, rect) in [
        (
            "GUI:SelectEngagement",
            "Select Engagement",
            layout.select_engagement,
        ),
        ("GUI:GameType", "Game Type", layout.game_type_heading),
        ("GUI:GameMap", "Game Map", layout.game_map_heading),
    ] {
        let label = localized_label(state, key, fallback);
        push_text_draw(
            out,
            state,
            &label,
            rect_to_text_rect(rect),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00008,
        );
    }
    if !sliding {
        for (label, rect, button) in [
            (
                localized_label(state, "GUI:UseMap", "Use Map"),
                layout.use_map_button,
                ChooseMapModalButton::UseMap0x6c5,
            ),
            (
                localized_label(state, "GUI:Cancel", "Cancel"),
                layout.cancel_button,
                ChooseMapModalButton::Cancel0x5c0,
            ),
            (
                localized_label(state, "GUI:CreateRandomMap", "Create Random Map"),
                layout.create_random_map_button,
                ChooseMapModalButton::CreateRandomMap0x583,
            ),
        ] {
            let disabled = !modal.button_enabled(button);
            push_text_draw(
                out,
                state,
                &label,
                rect_to_text_rect(rect),
                button_label_color_for_disabled(disabled),
                ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                SHELL_DROPDOWN_TEXT_DEPTH - 0.00009,
            );
        }
    }
    let geometry = modal.mode_geometry(layout);
    for (visible, mode_id) in modal
        .mode_rows()
        .iter()
        .skip(modal.mode_top_index)
        .take(geometry.visible_rows)
        .enumerate()
    {
        let Some(mode) = mode_by_id(&state.frontend.skirmish_modes, *mode_id) else {
            continue;
        };
        let label = localized_label(state, &mode.ui_name_key, &mode.ui_name_key);
        let row = geometry.row(visible);
        push_text_draw(
            out,
            state,
            &label,
            rect_to_text_rect(RectPx::new(row.x + 2, row.y, row.w - 2, row.h)),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00009,
        );
    }
    let geometry = modal.map_geometry(layout);
    for (visible, record_idx) in modal
        .filtered_record_indices
        .iter()
        .skip(modal.map_top_index)
        .take(geometry.visible_rows)
        .enumerate()
    {
        let Some(record) = state.frontend.scenario_catalog.records().get(*record_idx) else {
            continue;
        };
        let row = geometry.row(visible);
        push_text_draw(
            out,
            state,
            &record.display_name,
            rect_to_text_rect(RectPx::new(row.x + 2, row.y, row.w - 2, row.h)),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00009,
        );
    }
}

pub(super) fn push_validation_modal_text_draws(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    layout: &BodyOkLayout,
    pressed: bool,
) {
    let Some(modal) = state.frontend.skirmish_shell_state.validation_modal.as_ref() else {
        return;
    };
    let body = [PaintLabel {
        text: (&modal.message).into(),
        rect: layout.body,
        rgb: SHELL_LABEL_TEXT_RGB,
        align: validation_modal_body_text_align(),
        path_a_reveal: None,
    }];
    out.extend(shell_paint::paint_labels_at_depth(
        &state.renderer.bit_font,
        &body,
        SHELL_DROPDOWN_TEXT_DEPTH - 0.00012,
    ));
    let ok = [PaintLabel {
        text: (&modal.ok_button).into(),
        rect: button_label_rect_px(layout.ok, pressed),
        rgb: SHELL_LABEL_TEXT_RGB,
        align: ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        path_a_reveal: None,
    }];
    out.extend(shell_paint::paint_labels_at_depth(
        &state.renderer.bit_font,
        &ok,
        SHELL_DROPDOWN_TEXT_DEPTH - 0.00013,
    ));
}

pub(super) fn push_start_marker_labels(
    out: &mut Vec<SpriteInstance>,
    state: &AppState,
    projected_positions: &[(i32, i32)],
    depth: f32,
) {
    for (idx, &(x, y)) in projected_positions.iter().enumerate() {
        let label = (idx + 1).to_string();
        let (label_x, label_y) = start_marker_label_origin(x, y);
        out.extend(state.renderer.bit_font.build_text(
            &label,
            label_x as f32,
            label_y as f32,
            1.0,
            depth,
            start_marker_label_color(),
            [0.0, 0.0],
        ));
    }
}

pub(super) fn start_marker_label_origin(anchor_x: i32, anchor_y: i32) -> (i32, i32) {
    (anchor_x - 2, anchor_y - 6)
}

pub(super) fn start_marker_label_color() -> [f32; 3] {
    SHELL_LABEL_TEXT_RGB
}

pub(super) fn build_start_marker_label_instances(
    state: &AppState,
    projected_positions: &[(i32, i32)],
) -> Vec<SpriteInstance> {
    let mut instances = Vec::new();
    push_start_marker_labels(&mut instances, state, projected_positions, 0.00040);
    instances
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combo_face_text_under_open_dropdown_is_suppressed() {
        let dropdown = RectPx::new(23, 96, 149, 92);
        let covered_combo_text = RectPx::new(25, 123, 129, 24);
        let clear_combo_text = RectPx::new(25, 71, 129, 24);

        assert!(text_covered_by_overlay(covered_combo_text, &[dropdown]));
        assert!(!text_covered_by_overlay(clear_combo_text, &[dropdown]));
        assert!(!text_covered_by_overlay(covered_combo_text, &[]));
    }

    #[test]
    fn text_under_validation_dialog_is_suppressed_like_dropdown_text() {
        let dialog = RectPx::new(220, 239, 360, 122);
        let covered_combo_text = RectPx::new(260, 260, 100, 24);
        let clear_combo_text = RectPx::new(25, 71, 129, 24);

        assert!(text_covered_by_overlay(covered_combo_text, &[dialog]));
        assert!(!text_covered_by_overlay(clear_combo_text, &[dialog]));
    }

    #[test]
    fn adjacent_rects_do_not_count_as_dropdown_coverage() {
        let dropdown = RectPx::new(10, 20, 100, 50);
        let above = RectPx::new(10, 0, 100, 20);
        let below = RectPx::new(10, 70, 100, 20);

        assert!(!text_covered_by_overlay(above, &[dropdown]));
        assert!(!text_covered_by_overlay(below, &[dropdown]));
    }

    #[test]
    fn team_label_uses_native_sentinel_values() {
        assert_eq!(team_label_spec(-2), ("GUI:NoneAsSymbols", "None"));
        assert_eq!(team_label_spec(0), ("LETTER_A", "A"));
        assert_eq!(team_label_spec(1), ("LETTER_B", "B"));
        assert_eq!(team_label_spec(2), ("LETTER_C", "C"));
        assert_eq!(team_label_spec(3), ("LETTER_D", "D"));
    }

    #[test]
    fn trackbar_value_text_uses_normal_shell_yellow_source() {
        assert_eq!(trackbar_value_text_color(), SHELL_LABEL_TEXT_RGB);
    }

    #[test]
    fn combo_face_text_color_uses_disabled_source_for_inactive_siblings() {
        let shell = SkirmishShellState::default();

        assert_eq!(combo_face_text_color(false), SHELL_LABEL_TEXT_RGB);
        assert_eq!(
            combo_face_text_color(true),
            SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F
        );
        assert_eq!(
            opponent_sibling_combo_text_color(&shell.opponents[0]),
            SHELL_LABEL_TEXT_RGB
        );
        assert_eq!(
            opponent_sibling_combo_text_color(&shell.opponents[1]),
            SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F
        );
    }

    #[test]
    fn combodropwin_row_text_pretruncates_without_using_clip_width_as_rect_width() {
        let font = crate::render::bit_font::tests::make_test_font(
            &[
                (b'a' as u16, 6),
                (b'b' as u16, 6),
                (b'c' as u16, 6),
                (b'd' as u16, 6),
                (b' ' as u16, 4),
            ],
            4,
        );
        let content = RectPx::new(100, 50, 40, COMBO_DROPDOWN_ROW_H);

        let rect = combo_dropdown_text_rect_for_current_renderer(content, 0);
        assert_eq!(rect.x, 103);
        assert_eq!(rect.w, 37);
        assert_eq!(combo_dropdown_text_fit_width(content), 20);

        let label = truncate_combo_dropdown_label(&font, "ab cd", 20);
        assert_eq!(label.as_ref(), "ab ");
        assert!(font.text_width(label.as_ref()) <= 20);
    }

    #[test]
    fn owner_draw_label_truncates_ascii_by_utf16_code_unit() {
        let font = crate::render::bit_font::tests::make_test_font(
            &[
                (b'a' as u16, 6),
                (b'b' as u16, 6),
                (b'c' as u16, 6),
                (b'd' as u16, 6),
            ],
            4,
        );

        let label = truncate_owner_draw_label(&font, "abcd", 14);

        assert_eq!(label.as_ref(), "ab");
        assert!(font.text_width(label.as_ref()) <= 14);
    }

    #[test]
    fn owner_draw_label_truncates_latin1_by_utf16_code_unit() {
        let font = crate::render::bit_font::tests::make_test_font(
            &[(b'a' as u16, 6), (0x00e9, 8), (b'b' as u16, 6)],
            4,
        );

        let label = truncate_owner_draw_label(&font, "aéb", 16);

        assert_eq!(label.as_ref(), "aé");
        assert!(font.text_width(label.as_ref()) <= 16);
    }

    #[test]
    fn owner_draw_label_truncates_non_bmp_by_utf16_code_unit() {
        let font = crate::render::bit_font::tests::make_test_font(&[(b'a' as u16, 6)], 4);

        let label = truncate_combo_dropdown_label(&font, "a😀", 13);

        assert_eq!(label.as_ref(), "a\u{fffd}");
        assert_ne!(label.as_ref(), "a");
        assert!(font.text_width(label.as_ref()) <= 13);
    }

    #[test]
    fn collapsed_combo_face_text_pretruncates_to_arrow_reserved_width() {
        let font = crate::render::bit_font::tests::make_test_font(
            &[
                (b'a' as u16, 6),
                (b'b' as u16, 6),
                (b'c' as u16, 6),
                (b'd' as u16, 6),
                (b' ' as u16, 4),
            ],
            4,
        );
        let combo = RectPx::new(423, 59, 44, 24);

        let draw_rect = combo_face_text_draw_rect(combo);
        assert_eq!(draw_rect, RectPx::new(425, 59, 42, 24));
        assert_eq!(combo_face_text_fit_width(combo), 24);
        let label = truncate_owner_draw_label(&font, "ab cd", combo_face_text_fit_width(combo));

        assert_eq!(label.as_ref(), "ab ");
        assert!(font.text_width(label.as_ref()) <= combo_face_text_fit_width(combo));
    }
}

pub(super) fn push_saved_seed_modal_text_draws(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    layout: &SavedSeedLayout,
) {
    let Some(browser) = state.frontend.skirmish_shell_state.saved_seed_browser.as_ref() else {
        return;
    };
    let (title_key, title_fallback) = browser.mode.title_label();
    push_text_draw(
        out,
        state,
        &localized_label(state, title_key, title_fallback),
        rect_to_text_rect(layout.title),
        SHELL_LABEL_TEXT_RGB,
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        SHELL_DROPDOWN_TEXT_DEPTH - 0.00008,
    );
    // Ordinary pre-match saved seeds hide40C; the active saved-game caller
    // paints its resource prompt separately.
    push_saved_browser_contents_text(out, state, layout, browser);

    let (action_key, action_fallback) = browser.mode.action_label();
    for (key, fallback, rect) in [
        (action_key, action_fallback, layout.action),
        ("GUI:Back", "Back", layout.back),
    ] {
        push_text_draw(
            out,
            state,
            &localized_label(state, key, fallback),
            rect_to_text_rect(rect),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00012,
        );
    }
    push_saved_browser_prompt_text(out, state, layout, browser);
}

pub(super) fn push_saved_browser_contents_text<I>(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    layout: &SavedSeedLayout,
    browser: &crate::ui::skirmish_shell::SavedSeedBrowserState<I>,
) {
    let geometry = crate::ui::skirmish_shell::seed_list::SeedListGeometry::new(layout.list, browser.entries.len(), browser.top_index);
    let visible = geometry.visible_rows;
    for row in 0..visible {
        let Some(entry) = browser.entries.get(browser.top_index + row) else {
            break;
        };
        let rect = geometry.row(row);
        if rect.h <= 0 {
            continue;
        }
        let (date, time) = crate::util::native_file_time::format_file_time_parts(entry.last_write_time).unwrap_or_default();
        for (text, offset, width) in [
            (entry.description.display_text(), 2, 249), (date, 255, 56), (time, 315, 0),
        ] {
            let left = rect.x + offset;
            let available = (rect.x + rect.w - left).max(0);
            let width = if width == 0 { available } else { width.min(available) };
            if width <= 0 { continue; }
            // 0x00619969 trims by wide units and appends the literal three dots.
            let mut units: Vec<u16> = text.encode_utf16().collect();
            let mut label = text;
            if state.renderer.bit_font.text_width(&label) > width as u32 {
                while !units.is_empty() {
                    units.pop();
                    label = format!("{}...", String::from_utf16_lossy(&units));
                    if state.renderer.bit_font.text_width(&label) <= width as u32 { break; }
                }
            }
            push_text_draw(out, state, &label,
                rect_to_text_rect(RectPx::new(left, rect.y, width, rect.h)),
                SHELL_LABEL_TEXT_RGB, ShellAlign::NONE,
                SHELL_DROPDOWN_TEXT_DEPTH - 0.00011);
        }
        // Native559A07 attempts an x200 marker, but the4A8 setter61B0D4
        // rejects that absent column. These templates register only2/255/315.

    }

    if let Some(edit) = layout.name_edit {
        push_text_draw(
            out,
            state,
            &String::from_utf16_lossy(&browser.description_edit.units[browser.description_edit.first_visible_unit.min(browser.description_edit.units.len())..]),
            rect_to_text_rect(player_name_edit_text_rect(edit)),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00011,
        );
    }

}

/// Chrome is drawn before text in this renderer: cut a message box's
/// rectangle out of the text already queued so it cannot paint through the
/// box's panel.
pub(super) fn exclude_text_under(out: &mut Vec<ShellTextDraw>, dialog: RectPx) {
    let underlying = std::mem::take(out);
    for draw in underlying {
        let clip = draw.scissor;
        let x0 = clip.x as i32;
        let y0 = clip.y as i32;
        let x1 = x0 + clip.w as i32;
        let y1 = y0 + clip.h as i32;
        let cut_x0 = dialog.x.clamp(x0, x1);
        let cut_x1 = (dialog.x + dialog.w).clamp(x0, x1);
        let cut_y0 = dialog.y.clamp(y0, y1);
        let cut_y1 = (dialog.y + dialog.h).clamp(y0, y1);
        for (left, top, right, bottom) in [
            (x0, y0, x1, cut_y0),
            (x0, cut_y1, x1, y1),
            (x0, cut_y0, cut_x0, cut_y1),
            (cut_x1, cut_y0, x1, cut_y1),
        ] {
            if right > left && bottom > top {
                out.push(ShellTextDraw {
                    instances: draw.instances.clone(),
                    scissor: crate::render::shell_text::ScissorRect {
                        x: left as u32,
                        y: top as u32,
                        w: (right - left) as u32,
                        h: (bottom - top) as u32,
                    },
                });
            }
        }
    }
}

/// Use Map's eject box over the chooser: body `GUI:EjectAIPlayers`, OK and
/// Cancel.
pub(super) fn push_choose_map_eject_prompt_text(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    prompt: &crate::ui::skirmish_shell::EjectPrompt,
) {
    let layout = crate::ui::shell::modal::quit_confirm_layout(
        state.render_width() as i32,
        state.render_height() as i32,
    );
    exclude_text_under(out, layout.dialog);
    let pressed = |button| prompt.pressed == Some(button);
    let labels = [
        PaintLabel {
            text: localized_label(state, "GUI:EjectAIPlayers", "GUI:EjectAIPlayers").into(),
            rect: layout.body,
            rgb: SHELL_LABEL_TEXT_RGB,
            align: validation_modal_body_text_align(),
            path_a_reveal: None,
        },
        PaintLabel {
            text: localized_label(state, "GUI:Ok", "OK").into(),
            rect: button_label_rect_px(
                layout.ok,
                pressed(crate::ui::skirmish_shell::EjectPromptButton::Ok),
            ),
            rgb: SHELL_LABEL_TEXT_RGB,
            align: ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            path_a_reveal: None,
        },
        PaintLabel {
            text: localized_label(state, "GUI:Cancel", "Cancel").into(),
            rect: button_label_rect_px(
                layout.cancel,
                pressed(crate::ui::skirmish_shell::EjectPromptButton::Cancel),
            ),
            rgb: SHELL_LABEL_TEXT_RGB,
            align: ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            path_a_reveal: None,
        },
    ];
    out.extend(shell_paint::paint_labels_at_depth(
        &state.renderer.bit_font,
        &labels,
        SHELL_DROPDOWN_TEXT_DEPTH - 0.00013,
    ));
}

pub(super) fn push_saved_browser_prompt_text<I>(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    layout: &SavedSeedLayout,
    browser: &crate::ui::skirmish_shell::SavedSeedBrowserState<I>,
) {
    if let Some(prompt) = browser.prompt.as_ref() {
        let (dialog, body_rect, yes, no) =
            prompt.layout(layout.screen.w as u32, layout.screen.h as u32);
        exclude_text_under(out, dialog);
        let mut labels = vec![PaintLabel {
            text: prompt.body.as_str().into(), rect: body_rect, rgb: SHELL_LABEL_TEXT_RGB,
            align: validation_modal_body_text_align(), path_a_reveal: None,
        }];
        labels.push(PaintLabel {
            text: prompt.affirmative.as_str().into(), rect: button_label_rect_px(yes, browser.pressed_control == Some(crate::ui::skirmish_shell::SavedSeedControl::Action)), rgb: SHELL_LABEL_TEXT_RGB,
            align: ShellAlign::H_CENTER | ShellAlign::V_CENTER, path_a_reveal: None,
        });
        if let Some((rect, caption)) = no.zip(prompt.negative.as_ref()) {
            labels.push(PaintLabel {
                text: caption.as_str().into(), rect: button_label_rect_px(rect, browser.pressed_control == Some(crate::ui::skirmish_shell::SavedSeedControl::Back0x686)), rgb: SHELL_LABEL_TEXT_RGB,
                align: ShellAlign::H_CENTER | ShellAlign::V_CENTER, path_a_reveal: None,
            });
        }
        out.extend(shell_paint::paint_labels_at_depth(&state.renderer.bit_font, &labels,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00013));
    }

}
