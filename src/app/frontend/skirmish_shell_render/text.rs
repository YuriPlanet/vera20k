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
use crate::skirmish_launch::{LaunchCountry, LaunchStartPosition};
use crate::skirmish_modes::mode_by_id;
use crate::ui::shell::geom::ShellScale;
use crate::ui::shell::list::ListMetrics;
use crate::ui::shell::modal::BodyOkLayout;
use crate::ui::shell::static_reveal::StaticPaint;
use crate::ui::shell::trackbar::value_text_rect;
use crate::ui::skirmish_shell::{
    COMBO_FACE_H, COMBO_TEXT_LEFT_INSET, ChooseMapModalButton, ChooseMapModalLayout,
    DropdownMetrics, OwnerDrawButton, RandomMapSetupControl, RandomMapSetupLayout, RectPx,
    SETUP_COMBO_ROWS, SavedSeedLayout, SkirmishAiRowType, SkirmishCheckboxId, SkirmishComboId,
    SkirmishComboItem, SkirmishCountryChoice, SkirmishShellLayout, SkirmishShellOpponent,
    SkirmishShellState, SkirmishTrackbarId, checkbox_text_rect, combo_dropdown_content_rect,
    combo_dropdown_rect, combo_dropdown_visible_row_count, combo_enabled, combo_items,
    combo_text_rect, player_name_edit_text_rect, player_row_visible,
    random_map_setup_dropdown_rect, setup_combo_items, trackbar_visual_value,
};

use super::controls::trackbar_rect_for_id;
use super::{
    COMBODROPWIN_TEXT_INSET_X, COMBODROPWIN_TEXT_TRUNCATION_SCROLLBAR_RESERVE_PX,
    SHELL_CONTROL_TEXT_DEPTH, SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F,
    SHELL_DROPDOWN_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB,
};

pub(super) fn localized_label(state: &AppState, key: &str, fallback: &str) -> String {
    state
        .process_assets
        .csf
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

pub(crate) fn combo_item_label(state: &AppState, item: SkirmishComboItem) -> String {
    match item {
        SkirmishComboItem::AiType(row_type) => {
            let (key, fallback) = row_type_label(row_type);
            localized_label(state, key, fallback)
        }
        SkirmishComboItem::Country(SkirmishCountryChoice::Random) => {
            localized_label(state, "GUI:RandomAsSymbols", "Random")
        }
        SkirmishComboItem::Country(SkirmishCountryChoice::Country(country)) => {
            country_label(state, country)
        }
        SkirmishComboItem::ColorSentinel(_) => {
            // The source-line immediate `0x20A` is not a string id; the
            // adjacent load supplies `GUI:RandomAsSymbols` to the color combo.
            localized_label(state, "GUI:RandomAsSymbols", "Random")
        }
        SkirmishComboItem::Color(_) => String::new(),
        SkirmishComboItem::Start(start) => match start {
            LaunchStartPosition::Auto => localized_label(state, "GUI:RandomAsSymbols", "Random"),
            LaunchStartPosition::Position(idx) => (idx + 1).to_string(),
        },
        SkirmishComboItem::Team(team) => team_label(state, team),
    }
}

pub(super) fn country_choice_label(
    state: &AppState,
    random: bool,
    country: LaunchCountry,
) -> String {
    if random {
        localized_label(state, "GUI:RandomEx", "Random")
    } else {
        country_label(state, country)
    }
}

/// Country UIName from the retained HouseType reader (511850), resolved by
/// the fixed CSF catalog as native4E3A00 uses it. A missing raw UIName remains
/// empty; the country catalog decides whether that row is eligible.
pub(super) fn country_label(state: &AppState, country: LaunchCountry) -> String {
    if let Some(rules) = state.rules() {
        let Some(id) = rules.country_name(country.index()) else {
            return String::new();
        };
        let (key, _) = rules.country_display_name_sources(id);
        return key
            .and_then(|key| {
                state
                    .process_assets
                    .csf
                    .as_ref()
                    .and_then(|csf| csf.get(key))
            })
            .unwrap_or("")
            .to_owned();
    }
    String::new()
}

pub(super) fn trackbar_display_value(shell: &SkirmishShellState, id: SkirmishTrackbarId) -> String {
    match id {
        SkirmishTrackbarId::GameSpeed0x529 => trackbar_visual_value(shell, id).to_string(),
        SkirmishTrackbarId::Credits0x511 => shell.credits().to_string(),
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
    scale: ShellScale,
) {
    let text_rect = button_text_rect(rect, pressed, scale);
    push_text_draw(
        out,
        state,
        label,
        text_rect,
        button_label_color(),
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        depth,
        scale.factor(),
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

/// The owner-draw button's caption rect: the `0x612B70` type-1 offsets stay
/// authored, so `scale` maps the button's device rect back for them and stretches
/// the result. `ShellScale::RETAIL` reproduces the plain offsets.
pub(super) fn button_text_rect(rect: RectPx, pressed: bool, scale: ShellScale) -> TextRect {
    rect_to_text_rect(button_label_rect_px(rect, pressed, scale))
}

fn button_label_rect_px(rect: RectPx, pressed: bool, scale: ShellScale) -> RectPx {
    scale.derive_rect(rect, |rect| {
        let mut x = rect.x;
        let y = if pressed {
            x += 2;
            rect.y + 5
        } else {
            rect.y + 1
        };
        RectPx::new(
            x,
            y,
            (rect.x + rect.w - 2 - x).max(0),
            (rect.y + rect.h - y).max(0),
        )
    })
}

pub(super) const fn validation_modal_body_text_align() -> ShellAlign {
    ShellAlign::NONE
}

/// Emit one caption. `scale` is the page's uniform stretch (
/// [`crate::ui::shell::geom::ShellScale::uniform`]): the wrap and centring run on
/// the authored numbers and the glyphs are emitted at `scale` x, so a stretched
/// page keeps gamemd's text metrics instead of re-wrapping a resized rect.
/// `1.0` is the retail path, byte-for-byte.
#[allow(clippy::too_many_arguments)]
pub(super) fn push_text_draw(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    label: &str,
    text_rect: TextRect,
    color: [f32; 3],
    align: ShellAlign,
    depth: f32,
    scale: f32,
) {
    out.push(shell_text::draw_in_rect_scaled(
        &state.renderer.bit_font,
        label,
        text_rect,
        color,
        align,
        [0.0, 0.0],
        depth,
        scale,
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
    scale: ShellScale,
) -> TextRect {
    // `ComboDropWin` draws from x+3 to the row right edge. The row is on the
    // grid the highlight and the pointer hit tests use, so the caption fills the
    // same band a click selects.
    let row = crate::ui::skirmish_shell::combo_dropdown_row_rect(content, scale, visible_row);
    let inset_x = (COMBODROPWIN_TEXT_INSET_X as f32 * scale.factor()).round() as i32;
    TextRect {
        x: row.x + inset_x,
        y: row.y,
        w: (row.w - inset_x).max(0) as u32,
        h: row.h.max(0) as u32,
    }
}

pub(super) fn combo_dropdown_text_fit_width(content: RectPx, scale: ShellScale) -> u32 {
    // As on the combo face: the truncation test runs on gamemd's glyph metrics,
    // so the stretched row's width goes back to the units the caption draws in.
    let reserve =
        (COMBODROPWIN_TEXT_TRUNCATION_SCROLLBAR_RESERVE_PX as f32 * scale.factor()) as i32;
    ((content.w - reserve).max(0) as f32 / scale.factor())
        .round()
        .max(0.0) as u32
}

/// A list row's text inset from its left edge, grown with the page.
fn list_text_inset(scale: ShellScale) -> i32 {
    (LIST_TEXT_INSET as f32 * scale.factor()).round() as i32
}

const LIST_TEXT_INSET: i32 = 2;

pub(super) fn combo_face_text_fit_width(rect: RectPx, scale: ShellScale) -> u32 {
    // The truncation test runs on gamemd's glyph metrics, so the stretched face's
    // width is taken back to the units the caption is centred in.
    let text = combo_text_rect(rect, scale);
    (text.w as f32 / scale.factor()).round().max(0.0) as u32
}

pub(super) fn combo_face_text_draw_rect(rect: RectPx, scale: ShellScale) -> RectPx {
    scale.derive_rect(rect, |rect| {
        RectPx::new(
            rect.x + COMBO_TEXT_LEFT_INSET,
            rect.y,
            (rect.w - COMBO_TEXT_LEFT_INSET).max(0),
            COMBO_FACE_H,
        )
    })
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
    scale: f32,
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
        scale,
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
    scale: ShellScale,
) {
    push_combo_face_label_draw_with_color(
        out,
        state,
        label,
        rect,
        SHELL_LABEL_TEXT_RGB,
        covering_overlays,
        scale,
    );
}

fn push_combo_face_label_draw_with_color(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    label: &str,
    rect: RectPx,
    color: [f32; 3],
    covering_overlays: &[RectPx],
    scale: ShellScale,
) {
    let text_rect = combo_face_text_draw_rect(rect, scale);
    if text_covered_by_overlay(text_rect, covering_overlays) {
        return;
    }
    let fit_width = combo_face_text_fit_width(rect, scale);
    let label = truncate_owner_draw_label(&state.renderer.bit_font, label, fit_width);
    push_text_draw(
        out,
        state,
        label.as_ref(),
        rect_to_text_rect(text_rect),
        color,
        ShellAlign::V_CENTER,
        SHELL_CONTROL_TEXT_DEPTH,
        scale.factor(),
    );
}

/// The texts `0x102` holds for its heading, game type and map name statics
/// (`0x006AEC86`, `0x006AEC8D`; again after Use Map, `0x006ADA99`); its SHOW
/// completion hands them to [`crate::ui::skirmish_shell::SkirmishStatics`].
pub(crate) fn skirmish_right_panel_label_strings(state: &AppState) -> (String, String, String) {
    let shell = &state.frontend.skirmish_shell_state;
    let title = localized_label(state, "GUI:SkirmishGame", "Skirmish Game");
    let game_type = state
        .frontend
        .skirmish_modes
        .iter()
        .find(|mode| mode.id == shell.selected_mode_id)
        .map(|mode| localized_label(state, &mode.ui_name_key, &mode.ui_name_key))
        .unwrap_or_else(|| localized_label(state, "GUI:Battle", "Battle"));
    let map_label = skirmish_selected_map_name(state)
        .unwrap_or("None")
        .to_string();
    (title, game_type, map_label)
}

/// The Skirmish selection's scenario description, Session+0xEA
/// (`0x00A8B322`): `MPGameOptions__SelectScenario` copies it from the chosen
/// list entry (`0x005E7D2C`) and `MPDialog__SetMapNameStatic` shows it.
pub(crate) fn skirmish_selected_map_name(state: &AppState) -> Option<&str> {
    state
        .frontend
        .scenario_catalog
        .shell_maps()
        .get(state.frontend.skirmish_shell_state.selected_map_idx)
        .map(|map| map.display_name.as_str())
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
    let shown = state
        .frontend
        .skirmish_shell_state
        .statics
        .paint(std::time::Instant::now(), sliding);
    let text = &layout.right_panel_text;
    [
        (shown.heading, text.title),
        (shown.game_type, text.game_type),
        (shown.map_label, text.map_label),
    ]
    .into_iter()
    .filter_map(|(shown, rect)| shown.map(|shown| static_label(shown, rect, ShellAlign::H_CENTER)))
    .chain(status_line_label(shown.status_line, layout.status_help))
    .collect()
}

/// A family dialog's heading and status line for this recomposition
/// ([`crate::ui::shell::static_reveal::DialogStatics::paint`]).
pub(super) fn dialog_statics_labels(
    statics: &mut crate::ui::shell::static_reveal::DialogStatics,
    title: RectPx,
    status_line: RectPx,
    sliding: bool,
) -> Vec<PaintLabel<'static>> {
    let shown = statics.paint(std::time::Instant::now(), sliding);
    shown
        .heading
        .map(|shown| static_label(shown, title, ShellAlign::H_CENTER))
        .into_iter()
        .chain(status_line_label(shown.status_line, status_line))
        .collect()
}

pub(super) fn push_player_name_edit_text_draw(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    shell: &SkirmishShellState,
    layout: &SkirmishShellLayout,
) {
    let scale = layout.scale.factor();
    let rect = player_name_edit_text_rect(layout.player_name, layout.scale);
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

    // The scroll offset is in authored glyph units, so it advances with the
    // scaled text.
    let line_y = rect.y as f32
        + ((rect.h as f32 - state.renderer.bit_font.glyph_height() * scale).max(0.0) / 2.0).floor();
    let instances = state.renderer.bit_font.build_text(
        &shell.player_name_edit.text,
        rect.x as f32 - (shell.player_name_edit.scroll_x.max(0) as f32 * scale),
        line_y,
        scale,
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
    let scale = layout.scale;
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
    shell_draws.extend(shell_paint::paint_labels_at_depth_scaled(
        &state.renderer.bit_font,
        statics,
        SHELL_CONTROL_TEXT_DEPTH,
        scale.factor(),
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
            scale.factor(),
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
            checkbox_text_rect(checkbox.rect, scale),
            SHELL_CONTROL_TEXT_DEPTH,
            scale.factor(),
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
            scale.factor(),
        );

        let value = trackbar_display_value(shell, id);
        push_text_draw(
            &mut shell_draws,
            state,
            &value,
            rect_to_text_rect(value_text_rect(trackbar_rect_for_id(layout, id), scale)),
            trackbar_value_text_color(),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            SHELL_CONTROL_TEXT_DEPTH,
            scale.factor(),
        );
    }

    push_combo_face_label_draw(
        &mut shell_draws,
        state,
        &country_choice_label(state, shell.player_country_random, shell.player_country),
        layout.rows.side_combos[0],
        &covering_overlays,
        scale,
    );
    if !shell.player_color_claimed {
        let random_color = localized_label(state, "GUI:RandomAsSymbols", "Random");
        push_combo_face_label_draw(
            &mut shell_draws,
            state,
            &random_color,
            layout.color_combos[0],
            &covering_overlays,
            scale,
        );
    }
    push_combo_face_label_draw(
        &mut shell_draws,
        state,
        &combo_item_label(state, SkirmishComboItem::Start(shell.player_start_position)),
        layout.rows.start_combos[0],
        &covering_overlays,
        scale,
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
        scale,
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
            scale,
        );
        let sibling_text_color = opponent_sibling_combo_text_color(opponent);
        push_combo_face_label_draw_with_color(
            &mut shell_draws,
            state,
            &country_choice_label(state, opponent.country_random, opponent.country),
            layout.rows.side_combos[row],
            sibling_text_color,
            &covering_overlays,
            scale,
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
                scale,
            );
        }
        push_combo_face_label_draw_with_color(
            &mut shell_draws,
            state,
            &combo_item_label(state, SkirmishComboItem::Start(opponent.start_position)),
            layout.rows.start_combos[row],
            sibling_text_color,
            &covering_overlays,
            scale,
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
            scale,
        );
    }

    if let Some(open) = shell
        .open_combo_dropdown
        .filter(|open| combo_enabled(shell, maps, open.id))
        && let Some(dropdown) = combo_dropdown_rect(shell, layout, maps, open.id)
    {
        let content = combo_dropdown_content_rect(shell, layout, maps, open.id).unwrap_or(dropdown);
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
            let rect = combo_dropdown_text_rect_for_current_renderer(content, idx, layout.scale);
            let fit_width = combo_dropdown_text_fit_width(content, layout.scale);
            let label = truncate_combo_dropdown_label(&state.renderer.bit_font, &label, fit_width);
            push_text_draw(
                &mut shell_draws,
                state,
                label.as_ref(),
                rect,
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::V_CENTER,
                SHELL_DROPDOWN_TEXT_DEPTH,
                // The rows stretch, so their captions grow with them.
                layout.scale.factor(),
            );
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
            layout.scale,
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
    let Some(modal) = state
        .frontend
        .skirmish_shell_state
        .random_map_setup_modal
        .as_ref()
    else {
        return;
    };
    let scale = layout.scale;

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
            scale.factor(),
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
            scale.factor(),
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
            button_text_rect(rect, modal.pressed_control == Some(control), scale),
            button_label_color_for_disabled(!modal.is_enabled(control)),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00009,
            scale.factor(),
        );
    }

    // An open list covers the faces and the trackbar under it; each ends in
    // the gap above a row, so a covered text is covered whole.
    let open_list: Vec<RectPx> = modal
        .open_combo
        .map(|combo| {
            random_map_setup_dropdown_rect(layout, combo.row(), setup_combo_items(combo).len())
        })
        .into_iter()
        .collect();

    // Selected entry on each closed combo face. A value with no entry -- map
    // type 0, which the list omits -- leaves the face blank, as the original's
    // match-the-entry selection does.
    for (row, combo) in SETUP_COMBO_ROWS.iter().enumerate() {
        let items = setup_combo_items(*combo);
        let Some(selected) = modal.selected_item_index(*combo) else {
            continue;
        };
        let rect = combo_text_rect(layout.control_rects[row], scale);
        if text_covered_by_overlay(rect, &open_list) {
            continue;
        }
        let entry = items[selected];
        push_text_draw(
            out,
            state,
            &localized_label(state, entry.key, entry.fallback),
            rect_to_text_rect(rect),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00009,
            scale.factor(),
        );
    }

    // The players trackbar carries its value in the plaque at its right end.
    let value_rect = value_text_rect(layout.control_rects[PLAYERS_ROW], scale);
    if !text_covered_by_overlay(value_rect, &open_list) {
        push_text_draw(
            out,
            state,
            &modal.options.num_players.to_string(),
            rect_to_text_rect(value_rect),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00009,
            scale.factor(),
        );
    }

    if modal.generating {
        push_text_draw(
            out,
            state,
            &localized_label(state, "GUI:WorkingPleaseWait", "Working, please wait..."),
            rect_to_text_rect(layout.progress_text),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.0001,
            scale.factor(),
        );
    }

    // Matches the sprite pass: the open list is drawn over everything else.
    if let Some(combo) = modal.open_combo {
        let row = combo.row();
        let items = setup_combo_items(combo);
        let list = random_map_setup_dropdown_rect(layout, row, items.len());
        let metrics = DropdownMetrics::scaled(scale);
        let inset = (COMBO_TEXT_LEFT_INSET as f32 * scale.factor()).round() as i32;
        for (index, entry) in items.iter().enumerate() {
            let row_rect = RectPx::new(
                list.x + inset,
                list.y + metrics.row_h * index as i32,
                list.w - inset,
                metrics.row_h,
            );
            push_text_draw(
                out,
                state,
                &localized_label(state, entry.key, entry.fallback),
                rect_to_text_rect(row_rect),
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::V_CENTER,
                SHELL_DROPDOWN_TEXT_DEPTH - 0.00011,
                scale.factor(),
            );
        }
    }
}

/// Text of Choose Map `0x6B` besides its heading and status line, which the
/// caller paints ([`dialog_statics_labels`]). While a slide runs the column
/// draws the buttons without captions; the labels and lists (left-side
/// children) still paint.
pub(super) fn push_choose_map_modal_text_draws(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    layout: &ChooseMapModalLayout,
    sliding: bool,
) {
    let Some(modal) = state
        .frontend
        .skirmish_shell_state
        .choose_map_modal
        .as_ref()
    else {
        return;
    };
    let scale = layout.scale;
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
            scale.factor(),
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
                scale.factor(),
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
            rect_to_text_rect(RectPx::new(
                row.x + list_text_inset(scale),
                row.y,
                row.w - list_text_inset(scale),
                row.h,
            )),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00009,
            scale.factor(),
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
            rect_to_text_rect(RectPx::new(
                row.x + list_text_inset(scale),
                row.y,
                row.w - list_text_inset(scale),
                row.h,
            )),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00009,
            scale.factor(),
        );
    }
}

pub(super) fn push_validation_modal_text_draws(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    modal: &crate::ui::skirmish_shell::SkirmishValidationModalState,
    layout: &BodyOkLayout,
    pressed: bool,
) {
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
        rect: button_label_rect_px(layout.ok, pressed, ShellScale::RETAIL),
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
    scale: ShellScale,
    depth: f32,
) {
    for (idx, &(x, y)) in projected_positions.iter().enumerate() {
        let label = (idx + 1).to_string();
        let (label_x, label_y) = start_marker_label_origin(x, y, scale);
        out.extend(state.renderer.bit_font.build_text(
            &label,
            label_x as f32,
            label_y as f32,
            scale.factor(),
            depth,
            start_marker_label_color(),
            [0.0, 0.0],
        ));
    }
}

/// The authored offset of the marker number from its anchor.
const START_MARKER_LABEL_OFFSET_X: i32 = -2;
const START_MARKER_LABEL_OFFSET_Y: i32 = -6;

/// The number sits on the marker's authored offset and draws at the preview's
/// glyph scale, so it stays with the marker it labels.
pub(super) fn start_marker_label_origin(
    anchor_x: i32,
    anchor_y: i32,
    scale: ShellScale,
) -> (i32, i32) {
    (
        anchor_x + (START_MARKER_LABEL_OFFSET_X as f32 * scale.factor()).round() as i32,
        anchor_y + (START_MARKER_LABEL_OFFSET_Y as f32 * scale.factor()).round() as i32,
    )
}

pub(super) fn start_marker_label_color() -> [f32; 3] {
    SHELL_LABEL_TEXT_RGB
}

pub(super) fn build_start_marker_label_instances(
    state: &AppState,
    projected_positions: &[(i32, i32)],
    scale: ShellScale,
) -> Vec<SpriteInstance> {
    let mut instances = Vec::new();
    push_start_marker_labels(&mut instances, state, projected_positions, scale, 0.00040);
    instances
}

pub(super) fn push_saved_seed_modal_text_draws(
    out: &mut Vec<ShellTextDraw>,
    state: &AppState,
    layout: &SavedSeedLayout,
) {
    let Some(browser) = state
        .frontend
        .skirmish_shell_state
        .saved_seed_browser
        .as_ref()
    else {
        return;
    };
    let (title_key, title_fallback) = browser.mode.title_label();
    let scale = layout.scale;
    push_text_draw(
        out,
        state,
        &localized_label(state, title_key, title_fallback),
        rect_to_text_rect(layout.title),
        SHELL_LABEL_TEXT_RGB,
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        SHELL_DROPDOWN_TEXT_DEPTH - 0.00008,
        scale.factor(),
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
            scale.factor(),
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
    let scale = layout.scale;
    let geometry = crate::ui::skirmish_shell::seed_list::SeedListGeometry::new(
        layout.list,
        browser.entries.len(),
        browser.top_index,
        ListMetrics::scaled(scale),
    );
    let visible = geometry.visible_rows;
    for row in 0..visible {
        let Some(entry) = browser.entries.get(browser.top_index + row) else {
            break;
        };
        let rect = geometry.row(row);
        if rect.h <= 0 {
            continue;
        }
        // The three columns are laid out in the template's authored pixels and
        // stretched once with the row, so their offsets keep their proportion.
        let base = scale.base_rect(rect);
        let (date, time) =
            crate::util::native_file_time::format_file_time_parts(entry.last_write_time)
                .unwrap_or_default();
        for (text, offset, width) in [
            (entry.description.display_text(), 2, 249),
            (date, 255, 56),
            (time, 315, 0),
        ] {
            let left = base.x + offset;
            let available = (base.x + base.w - left).max(0);
            let width = if width == 0 {
                available
            } else {
                width.min(available)
            };
            if width <= 0 {
                continue;
            }
            // 0x00619969 trims by wide units and appends the literal three dots.
            let mut units: Vec<u16> = text.encode_utf16().collect();
            let mut label = text;
            if state.renderer.bit_font.text_width(&label) > width as u32 {
                while !units.is_empty() {
                    units.pop();
                    label = format!("{}...", String::from_utf16_lossy(&units));
                    if state.renderer.bit_font.text_width(&label) <= width as u32 {
                        break;
                    }
                }
            }
            push_text_draw(
                out,
                state,
                &label,
                rect_to_text_rect(scale.scale_rect(RectPx::new(left, base.y, width, base.h))),
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::NONE,
                SHELL_DROPDOWN_TEXT_DEPTH - 0.00011,
                scale.factor(),
            );
        }
        // Native559A07 attempts an x200 marker, but the4A8 setter61B0D4
        // rejects that absent column. These templates register only2/255/315.
    }

    if let Some(edit) = layout.name_edit {
        push_text_draw(
            out,
            state,
            &String::from_utf16_lossy(
                &browser.description_edit.units[browser
                    .description_edit
                    .first_visible_unit
                    .min(browser.description_edit.units.len())..],
            ),
            rect_to_text_rect(player_name_edit_text_rect(edit, scale)),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00011,
            scale.factor(),
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
                ShellScale::RETAIL,
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
                ShellScale::RETAIL,
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
            text: prompt.body.as_str().into(),
            rect: body_rect,
            rgb: SHELL_LABEL_TEXT_RGB,
            align: validation_modal_body_text_align(),
            path_a_reveal: None,
        }];
        labels.push(PaintLabel {
            text: prompt.affirmative.as_str().into(),
            rect: button_label_rect_px(
                yes,
                browser.pressed_control
                    == Some(crate::ui::skirmish_shell::SavedSeedControl::Action),
                ShellScale::RETAIL,
            ),
            rgb: SHELL_LABEL_TEXT_RGB,
            align: ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            path_a_reveal: None,
        });
        if let Some((rect, caption)) = no.zip(prompt.negative.as_ref()) {
            labels.push(PaintLabel {
                text: caption.as_str().into(),
                rect: button_label_rect_px(
                    rect,
                    browser.pressed_control
                        == Some(crate::ui::skirmish_shell::SavedSeedControl::Back0x686),
                    ShellScale::RETAIL,
                ),
                rgb: SHELL_LABEL_TEXT_RGB,
                align: ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                path_a_reveal: None,
            });
        }
        out.extend(shell_paint::paint_labels_at_depth(
            &state.renderer.bit_font,
            &labels,
            SHELL_DROPDOWN_TEXT_DEPTH - 0.00013,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::skirmish_shell::COMBO_DROPDOWN_ROW_H;

    #[test]
    fn combo_face_text_under_open_dropdown_is_suppressed() {
        let dropdown = RectPx::new(23, 96, 149, 92);
        let covered_combo_text = RectPx::new(25, 123, 129, 24);
        let clear_combo_text = RectPx::new(25, 71, 129, 24);

        assert!(text_covered_by_overlay(covered_combo_text, &[dropdown]));
        assert!(!text_covered_by_overlay(clear_combo_text, &[dropdown]));
        assert!(!text_covered_by_overlay(covered_combo_text, &[]));
    }

    /// `0x105` hides a text an open list touches; that is exact only while
    /// every list ends in the gap between rows.
    #[test]
    fn every_random_map_list_covers_the_texts_under_it_whole() {
        for (width, height) in [(640, 480), (800, 600), (1024, 768)] {
            let layout = crate::ui::skirmish_shell::compute_random_map_setup_layout(width, height);
            // The Players window has the runtime growth (226 wide) the
            // combos do not have yet (225), so its value text runs one
            // column past the lists.
            let mut players_value =
                value_text_rect(layout.control_rects[PLAYERS_ROW], ShellScale::RETAIL);
            players_value.w -= 1;
            let texts: Vec<RectPx> = (0..PLAYERS_ROW)
                .map(|row| combo_text_rect(layout.control_rects[row], ShellScale::RETAIL))
                .chain([players_value])
                .collect();
            for combo in SETUP_COMBO_ROWS {
                let list = random_map_setup_dropdown_rect(
                    &layout,
                    combo.row(),
                    setup_combo_items(combo).len(),
                );
                for text in &texts {
                    let inside = text.x >= list.x
                        && text.y >= list.y
                        && text.x + text.w <= list.x + list.w
                        && text.y + text.h <= list.y + list.h;
                    assert!(
                        !text_covered_by_overlay(*text, &[list]) || inside,
                        "{width}x{height}: {combo:?}'s list {list:?} cuts {text:?}"
                    );
                }
            }
        }
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

        let rect = combo_dropdown_text_rect_for_current_renderer(content, 0, ShellScale::RETAIL);
        assert_eq!(rect.x, 103);
        assert_eq!(rect.w, 37);
        assert_eq!(
            combo_dropdown_text_fit_width(content, ShellScale::RETAIL),
            20
        );

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

        let draw_rect = combo_face_text_draw_rect(combo, ShellScale::RETAIL);
        assert_eq!(draw_rect, RectPx::new(425, 59, 42, 24));
        assert_eq!(combo_face_text_fit_width(combo, ShellScale::RETAIL), 24);
        let label = truncate_owner_draw_label(
            &font,
            "ab cd",
            combo_face_text_fit_width(combo, ShellScale::RETAIL),
        );

        assert_eq!(label.as_ref(), "ab ");
        assert!(
            font.text_width(label.as_ref()) <= combo_face_text_fit_width(combo, ShellScale::RETAIL)
        );
    }
}
