//! Text draw helpers for the skirmish shell renderer.
//!
//! Owns localized labels, text rect conversion, modal/listbox text,
//! status help text, and start-marker number labels.

use crate::app::frontend::main_menu_shell_render::shell_reveal_path_a;
use crate::i18n::{tr, tr_count, tr_fill};
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::shell_paint::{self, PaintLabel};
use crate::render::shell_text::{ShellTextDraw, TextRect};
use crate::skirmish_launch::{LaunchCountry, LaunchStartPosition};
use crate::ui::shell::geom::{ShellAlign, ShellScale};
use crate::ui::shell::static_reveal::StaticPaint;
use crate::ui::skirmish_shell::{
    COMBO_FACE_H, COMBO_TEXT_LEFT_INSET, ChooseMapModalLayout, RandomMapSetupLayout, RectPx,
    SkirmishAiRowType, SkirmishCheckboxId, SkirmishComboId, SkirmishComboItem,
    SkirmishCountryChoice, SkirmishShellLayout, combo_text_rect, random_map_setup_dropdown_rect,
};

use super::gui_paint::{ButtonArt, FrameArt, GuiStyle, HostRows, paint_gui_with};
use super::{
    COMBODROPWIN_TEXT_TRUNCATION_SCROLLBAR_RESERVE_PX, SHELL_CONTROL_TEXT_DEPTH,
    SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F, SHELL_LABEL_TEXT_RGB,
};
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::assets::csf_file::CsfFile;
use crate::render::hud_paint::HudPaint;
use crate::ui::skirmish_shell::SkirmishShellState;

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

pub(super) fn team_label(process_assets: &ProcessAssets, team: i32) -> String {
    room_team_label(team).unwrap_or_else(|| {
        let (key, fallback) = team_label_spec(team);
        process_assets.csf_label(key, fallback)
    })
}

/// A VERA room's teams past gamemd's A to D, as the lobby names them: the
/// alphabet's next letters.
fn room_team_label(team: i32) -> Option<String> {
    let team = u32::try_from(team)
        .ok()
        .filter(|team| *team >= u32::from(crate::skirmish_launch::NATIVE_TEAMS))?;
    Some(
        char::from_u32(u32::from(b'A') + team)
            .filter(char::is_ascii_uppercase)
            .map_or_else(|| (team + 1).to_string(), String::from),
    )
}

pub(crate) fn combo_item_label(
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    item: SkirmishComboItem,
) -> String {
    match item {
        SkirmishComboItem::AiType(row_type) => {
            let (key, fallback) = row_type.label();
            process_assets.csf_label(key, fallback)
        }
        SkirmishComboItem::Country(SkirmishCountryChoice::Random) => {
            process_assets.csf_label("GUI:RandomAsSymbols", "Random")
        }
        SkirmishComboItem::Country(SkirmishCountryChoice::Country(country)) => {
            country_label(frontend, match_state, &process_assets.csf, country)
        }
        SkirmishComboItem::ColorSentinel(_) => {
            // The source-line immediate `0x20A` is not a string id; the
            // adjacent load supplies `GUI:RandomAsSymbols` to the color combo.
            process_assets.csf_label("GUI:RandomAsSymbols", "Random")
        }
        SkirmishComboItem::Color(_) => String::new(),
        SkirmishComboItem::Start(start) => match start {
            LaunchStartPosition::Auto => process_assets.csf_label("GUI:RandomAsSymbols", "Random"),
            LaunchStartPosition::Position(idx) => (idx + 1).to_string(),
        },
        SkirmishComboItem::Team(team) => team_label(process_assets, team),
        SkirmishComboItem::TeamTool { tool, refusal } => {
            crate::ui::team_tools::team_tool_text(tool, refusal)
        }
        SkirmishComboItem::TeamHeading => crate::ui::team_tools::presets_heading(),
        SkirmishComboItem::SeatAction { action, .. } => {
            crate::ui::team_tools::seat_action_text(action)
        }
    }
}

/// Country UIName from the retained HouseType reader (511850), resolved by
/// the fixed CSF catalog as native4E3A00 uses it. A missing raw UIName remains
/// empty; the country catalog decides whether that row is eligible.
pub(super) fn country_label(
    frontend: &FrontendState,
    match_state: &MatchState,
    csf: &Option<CsfFile>,
    country: LaunchCountry,
) -> String {
    if let Some(rules) = crate::app::state::active_rules(frontend, match_state) {
        let Some(id) = rules.country_name(country.index()) else {
            return String::new();
        };
        let (key, _) = rules.country_display_name_sources(id);
        return key
            .and_then(|key| csf.as_ref().and_then(|csf| csf.get(key)))
            .unwrap_or("")
            .to_owned();
    }
    String::new()
}

/// Retail provenance: disabled `OwnerDraw_Button_00612B70` replaces the normal
/// yellow label with its disabled color at `0x00612F5F`
/// ([`crate::render::shell_paint::SHELL_TEXT_RGB_DISABLED`]).
pub(super) fn button_label_color_for_disabled(disabled: bool) -> [f32; 3] {
    if disabled {
        crate::render::shell_paint::SHELL_TEXT_RGB_DISABLED
    } else {
        SHELL_LABEL_TEXT_RGB
    }
}

pub(super) fn combo_face_text_color(disabled: bool) -> [f32; 3] {
    if disabled {
        SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F
    } else {
        SHELL_LABEL_TEXT_RGB
    }
}

/// The owner-draw button's caption rect: the `0x612B70` type-1 offsets stay
/// authored, so `scale` maps the button's device rect back for them and stretches
/// the result. `RETAIL_SCALE` reproduces the plain offsets.
pub(super) fn button_label_rect_px(rect: RectPx, pressed: bool, scale: ShellScale) -> RectPx {
    scale.derive_rect(rect, |rect| {
        crate::app::frontend::shell_pass::owner_draw_button_label_rect(rect, pressed)
    })
}

/// [`TextRect::of`], under the name `gui_paint` calls it by.
pub(super) fn rect_to_text_rect(rect: RectPx) -> TextRect {
    TextRect::of(rect)
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

/// An owner-drawn label in `fit_width`: squeezed until it fits
/// ([`BitFont::fit_line`]) and cut only as far as the tightest squeeze still
/// leaves over. `draw` gets the text while the font draws at that squeeze.
pub(super) fn fit_owner_draw_label<R>(
    font: &BitFont,
    label: &str,
    fit_width: u32,
    draw: impl FnOnce(&str) -> R,
) -> R {
    font.fitted(font.fit_line(label, fit_width), |font| {
        draw(&font.truncate_to_width(label, fit_width))
    })
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

pub(in crate::app::frontend) fn text_covered_by_overlay(rect: RectPx, overlays: &[RectPx]) -> bool {
    overlays
        .iter()
        .any(|overlay| rects_intersect(rect, *overlay))
}

/// The texts `0x102` holds for its heading, game type and map name statics
/// (`0x006AEC86`, `0x006AEC8D`; again after Use Map, `0x006ADA99`); its SHOW
/// completion hands them to [`crate::ui::skirmish_shell::SkirmishStatics`].
pub(crate) fn skirmish_right_panel_label_strings(
    frontend: &FrontendState,
    process_assets: &ProcessAssets,
) -> (String, String, String) {
    let shell = &frontend.skirmish_shell_state;
    let title = process_assets.csf_label("GUI:SkirmishGame", "Skirmish Game");
    let game_type = shell
        .selected_mode
        .resolve(&frontend.skirmish_modes)
        .map(|mode| process_assets.csf_label(&mode.ui_name_key, &mode.ui_name_key))
        .unwrap_or_else(|| process_assets.csf_label("GUI:Battle", "Battle"));
    // VERA: with no map files the list is empty and no map is selected. The
    // online setup dialogs' map name static `0x5A8` has the template caption
    // `GUI:None` (`tools/storage_oracle/wol_shell.json`), which the LAN and
    // online setup also show with no map.
    let map_label = skirmish_selected_map_name(frontend).map_or_else(
        || process_assets.csf_label("GUI:None", "None"),
        str::to_owned,
    );
    (title, game_type, map_label)
}

/// The Skirmish selection's scenario description, Session+0xEA
/// (`0x00A8B322`): `MPGameOptions__SelectScenario` copies it from the chosen
/// list entry (`0x005E7D2C`) and `MPDialog__SetMapNameStatic` shows it.
pub(crate) fn skirmish_selected_map_name(frontend: &FrontendState) -> Option<&str> {
    frontend
        .scenario_catalog
        .shell_maps()
        .get(frontend.skirmish_shell_state.selected_map_idx)
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

/// A status line's label, top-left in its window and running on along the
/// bar (`status_text_rect`); nothing while it is hidden or blank.
pub(super) fn status_line_label(
    shown: Option<StaticPaint<'_>>,
    window: RectPx,
) -> Option<PaintLabel<'static>> {
    let rect = crate::ui::shell::layout::status_text_rect(window);
    shown
        .filter(|shown| !shown.text.is_empty())
        .map(|shown| static_label(shown, rect, ShellAlign::NONE))
}

/// Paint `0x102`'s kind-1 statics for this recomposition. While a slide runs
/// it blits the top panel over the heading, game type and map name; the
/// status line keeps what it painted when the dialog showed.
pub(super) fn paint_skirmish_statics(
    skirmish_shell_state: &mut SkirmishShellState,
    layout: &SkirmishShellLayout,
    sliding: bool,
) -> Vec<PaintLabel<'static>> {
    let shown = skirmish_shell_state
        .statics
        .paint(crate::util::time::Instant::now(), sliding);
    use crate::ui::skirmish_shell::{GAME_TYPE_STATIC, MAP_LABEL_STATIC, TITLE_STATIC};
    [
        (shown.heading, layout.static_rect(TITLE_STATIC)),
        (shown.game_type, layout.static_rect(GAME_TYPE_STATIC)),
        (shown.map_label, layout.static_rect(MAP_LABEL_STATIC)),
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
    let shown = statics.paint(crate::util::time::Instant::now(), sliding);
    shown
        .heading
        .map(|shown| static_label(shown, title, ShellAlign::PANEL_TITLE))
        .into_iter()
        .chain(status_line_label(shown.status_line, status_line))
        .collect()
}

/// The dialog's text: the kind-1 statics (`statics`, from
/// [`paint_skirmish_statics`]) and the widgets' texts, the name box's among
/// them (`widgets`, [`super::gui_paint`]).
pub(super) fn build_shell_text_draws(
    bit_font: &BitFont,
    layout: &SkirmishShellLayout,
    statics: &[PaintLabel<'_>],
    widgets: Vec<ShellTextDraw>,
) -> Vec<ShellTextDraw> {
    let mut shell_draws = shell_paint::paint_labels_at_depth_scaled(
        bit_font,
        statics,
        SHELL_CONTROL_TEXT_DEPTH,
        layout.scale.factor(),
    );
    shell_draws.extend(widgets);
    shell_draws
}

/// The combo entries the page's labels answer for: every item a row's list can
/// draw, the country catalog, and the two menus. The Team items come from the
/// same owner as the Team combos ([`crate::ui::skirmish_shell::team_items`]),
/// so a map past gamemd's eight starts labels E..O instead of leaving them
/// blank.
fn label_items(
    shell: &crate::ui::skirmish_shell::SkirmishShellState,
    maps: &[crate::map::scenario_menu::MapMenuEntry],
) -> Vec<SkirmishComboItem> {
    use crate::skirmish_launch::SKIRMISH_MAX_SLOT_COUNT;
    let ai = [
        SkirmishAiRowType::None,
        SkirmishAiRowType::Easy,
        SkirmishAiRowType::Normal,
        SkirmishAiRowType::Hard,
    ]
    .map(SkirmishComboItem::AiType);
    let starts = std::iter::once(LaunchStartPosition::Auto)
        .chain((0..SKIRMISH_MAX_SLOT_COUNT as u8).map(LaunchStartPosition::Position))
        .map(SkirmishComboItem::Start);
    let teams = crate::ui::skirmish_shell::team_items(shell, maps);
    // VERA's Quick Team Setup and Bulk Seat Actions: their items as the
    // rows stand now, a greyed one with why.
    let menus = [SkirmishComboId::TeamMenu, SkirmishComboId::SeatMenu]
        .into_iter()
        .flat_map(|menu| crate::ui::skirmish_shell::combo_items(shell, maps, menu));
    ai.into_iter()
        .chain(shell.country_catalog.items().iter().copied())
        .chain([SkirmishComboItem::ColorSentinel(-2)])
        .chain(starts)
        .chain(teams)
        .chain(menus)
        .collect()
}

/// The words `0x102`'s widgets show ([`crate::ui::skirmish_shell::page::sync`]).
pub(crate) fn skirmish_labels(
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
) -> crate::ui::skirmish_shell::page::SkirmishLabels {
    let shell = &frontend.skirmish_shell_state;
    let maps = frontend.scenario_catalog.shell_maps();
    let items = label_items(shell, maps)
        .into_iter()
        .map(|item| {
            (
                item,
                combo_item_label(frontend, match_state, process_assets, item),
            )
        })
        .collect();
    crate::ui::skirmish_shell::page::SkirmishLabels {
        items,
        random_country: process_assets.csf_label("GUI:RandomEx", "Random"),
        menus: [
            crate::ui::team_tools::TEAM_MENU_KEY,
            crate::ui::team_tools::SEAT_MENU_KEY,
        ]
        .map(tr),
        // VERA captions the map button "Choose Map", as its room does
        // (`SetupDialog::button_label`): retail's "Customize Battle" reads as
        // the settings Game Options keeps.
        buttons: [
            ("GUI:StartGame", "Start Game"),
            (
                crate::ui::skirmish_shell::game_options::GAME_OPTIONS_LABEL,
                "Game Options",
            ),
            ("VERA:ChooseMap", ""),
            ("VERA:Setups", ""),
            ("GUI:Back", "Back"),
        ]
        .map(|(key, fallback)| process_assets.csf_label(key, fallback)),
        headings: [
            ("GUI:Players", "Players"),
            ("GUI:Side", "Side"),
            ("GUI:Color", "Color"),
            ("GUI:StartPosition", "Start"),
            ("GUI:Team", "Team"),
            ("GUI:GameSpeed", "Game Speed"),
            ("GUI:Credits", "Credits"),
            ("GUI:UnitCount", "Unit Count"),
        ]
        .map(|(key, fallback)| process_assets.csf_label(key, fallback)),
        checkboxes: [
            SkirmishCheckboxId::ShortGame0x54e,
            SkirmishCheckboxId::McvRepacks0x693,
            SkirmishCheckboxId::CratesAppear0x696,
            SkirmishCheckboxId::SuperWeapons0x69a,
            SkirmishCheckboxId::BuildOffAlly0x69d,
        ]
        .map(|id| {
            let (key, fallback) = checkbox_label(id);
            process_assets.csf_label(key, fallback)
        }),
    }
}

/// The random-map dialog `0x105`'s widgets
/// ([`crate::ui::skirmish_shell::random_map_page`]) with their words, an
/// open list over the dialog, then its heading and status line.
pub(super) fn random_map_paint(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: &crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas,
    layout: &RandomMapSetupLayout,
    sliding: bool,
) -> Option<HudPaint> {
    use crate::ui::skirmish_shell::random_map_page;
    use crate::ui::skirmish_shell::{RandomMapSetupControl as C, SetupCombo};
    // DrawFill (0x00643470) uses the DarkRed scheme for the dialog outline.
    let progress_outline = {
        use crate::rules::color_scheme::{hsv_to_rgb, scheme_entry_by_name, scheme_hsv_by_entry};
        crate::app::state::active_rules(frontend, match_state).and_then(|rules| {
            let schemes = &rules.color_schemes;
            let index = scheme_entry_by_name(schemes, "DarkRed")?;
            Some(hsv_to_rgb(scheme_hsv_by_entry(schemes, index)?).map(|v| f32::from(v) / 255.0))
        })
    };
    let modal = frontend
        .skirmish_shell_state
        .random_map_setup_modal
        .as_mut()?;
    // The heading and status line, revealed from the entry slide's end.
    let statics = dialog_statics_labels(
        &mut modal.statics,
        layout.title,
        layout.status_help,
        sliding,
    );
    let modal = &*modal;
    let controller = &mut frontend.shell_controller;
    let root = random_map_page::sync_page(controller, modal, layout)?;
    let csf = process_assets.csf.as_ref();
    random_map_page::show_texts(controller, modal, sliding, &|key, fallback| {
        crate::i18n::label(csf, key, fallback)
    });
    let modal = frontend
        .skirmish_shell_state
        .random_map_setup_modal
        .as_ref()?;
    let combo = |id: i32| {
        let control = match id {
            0x405 => C::MapType0x405,
            0x3EA => C::Time0x3ea,
            0x407 => C::Theater0x407,
            0x406 => C::Size0x406,
            0x408 => C::Resources0x408,
            _ => return None,
        };
        SetupCombo::from_control(control)
    };
    let rows_of = |id| {
        let items = modal.combo_items(combo(id)?).iter();
        Some(
            items
                .map(|item| (process_assets.csf_label(item.key, item.fallback), u32::MAX))
                .collect(),
        )
    };
    // Surprise Me and Generate are owner-draw type 3 (MNBTTN), captions
    // centred on the whole button; VERA's Extras
    // rows are framed plates; Ares' Urban Areas button shows as its box.
    let art_of = |id: i32| match id {
        0x621 | 0x620 => Some(ButtonArt::ModalCentred),
        5000 => Some(ButtonArt::CheckboxFace),
        0x7420..0x7427 => Some(match modal.extras_row((id - 0x7420) as usize) {
            Some(_) => ButtonArt::Plate,
            None => ButtonArt::Host,
        }),
        _ => None,
    };
    let frame_of = |id: i32| {
        if id != i32::from(RandomMapSetupLayout::PROGRESS_BAR) {
            return None;
        }
        let outline = progress_outline;
        Some(match atlas.random_map_progress {
            Some(shape) => FrameArt::RandomMapProgress {
                shape,
                outline,
                percent: modal.progress_percent,
            },
            None => FrameArt::Progress {
                rgb: outline.unwrap_or(SHELL_LABEL_TEXT_RGB),
                done: u64::from(modal.progress_percent),
                total: 100,
                depth: super::SHELL_DROPDOWN_DEPTH - 0.00014,
            },
        })
    };
    let style = GuiStyle {
        // The right column shows while no slide runs.
        buttons: if sliding {
            ButtonArt::Host
        } else {
            ButtonArt::Panel
        },
        art_of: Some(&art_of),
        rows_of: Some(&rows_of),
        combos: super::gui_paint::ComboArt::Framed,
        frame_of: Some(&frame_of),
        ..Default::default()
    };
    let gui = frontend.shell_controller.gui();
    let mut paint = paint_gui_with(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        atlas,
        gui,
        root,
        layout.scale,
        &[],
        style,
    );
    // The open list never covered Ares' Urban Areas caption: it still paints
    // over a list that reaches it.
    let (open, _) = random_map_page::view(&frontend.shell_controller, modal);
    let list = open.map(|combo| {
        random_map_setup_dropdown_rect(layout, combo.row(), modal.combo_items(combo).len())
    });
    if let Some(urban) = gui.find_id(root, random_map_page::URBAN_CAPTION.into())
        && gui
            .tree()
            .screen_rect(urban)
            .is_ok_and(|r| text_covered_by_overlay(r, list.as_slice()))
    {
        let over = paint_gui_with(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            atlas,
            gui,
            urban,
            layout.scale,
            &[],
            style,
        );
        paint.popup_texts.extend(over.texts);
    }
    paint.texts.extend(super::paint_map_dialog_statics(
        bit_font,
        &statics,
        layout.scale,
    ));
    Some(paint)
}

/// Choose Map `0x6B`'s widgets
/// ([`crate::ui::skirmish_shell::choose_map_page`]) with their words and
/// rows, the search box's text, then its heading and status line.
pub(super) fn choose_map_paint(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &mut FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: &crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas,
    layout: &ChooseMapModalLayout,
    sliding: bool,
) -> Option<HudPaint> {
    use crate::ui::skirmish_shell::choose_map_page;
    let modal = frontend.skirmish_shell_state.choose_map_modal.as_mut()?;
    // The heading and status line, revealed from the entry slide's end; a
    // hidden chooser shows neither.
    let statics = match modal.is_hidden() {
        true => Vec::new(),
        false => dialog_statics_labels(
            &mut modal.statics,
            layout.title,
            layout.status_help,
            sliding,
        ),
    };
    let modal = &*modal;
    let controller = &mut frontend.shell_controller;
    let root = choose_map_page::sync_page(controller, modal, layout)?;
    let csf = process_assets.csf.as_ref();
    let label = |key: &str, fallback: &str| crate::i18n::label(csf, key, fallback);
    choose_map_page::show_texts(
        controller,
        modal,
        frontend.scenario_catalog.records(),
        sliding,
        &label,
    );
    let frontend = &*frontend;
    let modal = frontend.skirmish_shell_state.choose_map_modal.as_ref()?;
    let rows_of = |id| {
        Some(choose_map_rows(
            frontend,
            process_assets,
            modal,
            layout,
            id == 0x6EB,
        ))
    };
    // VERA's search line over the map list shows the search with its
    // composition, or names itself while empty; an empty list says why.
    // The line takes its text once a frame: it moves in, not a copy.
    let search = map_search_display(ime, &modal.search);
    let empty = search.is_empty();
    let search = std::cell::Cell::new(Some(search));
    let text_of = |id: i32| {
        (id == i32::from(choose_map_page::MAP_SEARCH))
            .then(|| search.take())
            .flatten()
    };
    let hint_of = |id: i32| {
        (id == i32::from(choose_map_page::MAP_SEARCH) && empty).then(|| {
            (
                tr("VERA:MapSearchHint"),
                choose_map_page::MAP_SEARCH_INSET.x,
            )
        })
    };
    let edit_face_of = |id: i32| {
        (id == i32::from(choose_map_page::MAP_SEARCH))
            .then_some(super::gui_paint::EditFace::SearchLine)
    };
    let empty_of = |id: i32| {
        (id == i32::from(choose_map_page::MAP_LIST))
            .then(|| choose_map_empty_text(frontend, modal))
            .flatten()
    };
    // Native owner-draw lists keep the composed backing; only the
    // asset-missing fallback fills the search line.
    let backed = super::modals::choose_map_background_entry(atlas, layout).is_some();
    let style = GuiStyle {
        // The right column shows while no slide runs.
        buttons: if sliding {
            ButtonArt::Host
        } else {
            ButtonArt::Panel
        },
        rows_of: Some(&rows_of),
        text_of: Some(&text_of),
        hint_of: Some(&hint_of),
        edit_face_of: Some(&edit_face_of),
        edit_fill: (!backed).then_some(super::SHELL_MODAL_PANEL_RGB),
        empty_of: Some(&empty_of),
        ..Default::default()
    };
    let gui = frontend.shell_controller.gui();
    let mut paint = paint_gui_with(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        atlas,
        gui,
        root,
        layout.scale,
        &[],
        style,
    );
    paint.texts.extend(super::paint_map_dialog_statics(
        bit_font,
        &statics,
        layout.scale,
    ));
    Some(paint)
}

/// The chooser's game-type rows (`modes`) or map rows by their names; only
/// the rows the list shows are named.
fn choose_map_rows(
    frontend: &FrontendState,
    process_assets: &ProcessAssets,
    modal: &crate::ui::skirmish_shell::ChooseMapModalState,
    layout: &ChooseMapModalLayout,
    modes: bool,
) -> HostRows {
    let (count, top, shown) = match modes {
        true => (
            modal.mode_rows().len(),
            modal.mode_top_index,
            modal.mode_geometry(layout).visible_rows,
        ),
        false => (
            modal.map_row_count(),
            modal.map_top_index,
            modal.map_geometry(layout).visible_rows,
        ),
    };
    let name = |row: usize| -> Option<String> {
        if modes {
            let chosen = modal.mode_rows().get(row)?;
            return Some(if chosen.id == crate::ui::skirmish_shell::ONLINE_MAPS_ROW {
                tr("VERA:OnlineMaps")
            } else if let Some(key) = chosen.online_category_label() {
                format!("  {}", tr(key))
            } else if chosen.id == crate::ui::skirmish_shell::RANDOM_MAPS_ROW {
                tr("VERA:RandomMaps")
            } else if chosen.id == crate::ui::skirmish_shell::FAVORITES_ROW {
                tr("VERA:Favorites")
            } else {
                let mode = chosen.resolve(&frontend.skirmish_modes)?;
                process_assets.csf_label(&mode.ui_name_key, &mode.ui_name_key)
            });
        }
        // The Random Maps folder lists its saved seeds by description.
        if let Some(seed) = modal.saved_seed_at(row) {
            return Some(seed.description.display_text());
        }
        if let Some(labels) = &modal.row_labels {
            return labels.get(row).cloned();
        }
        let record = frontend
            .scenario_catalog
            .records()
            .get(*modal.filtered_record_indices.get(row)?)?;
        // Starred maps carry a star, except under Favorites, where every
        // map is one.
        Some(
            match !modal.favorites_selected() && modal.is_favorite(record) {
                true => format!("* {}", record.display_name),
                false => record.display_name.clone(),
            },
        )
    };
    (0..count)
        .map(|row| {
            let shown = (top..top + shown).contains(&row);
            (
                shown.then(|| name(row)).flatten().unwrap_or_default(),
                u32::MAX,
            )
        })
        .collect()
}

/// VERA: why an empty Online Maps or Favorites list is empty, which it
/// shows down the list.
fn choose_map_empty_text(
    frontend: &FrontendState,
    modal: &crate::ui::skirmish_shell::ChooseMapModalState,
) -> Option<String> {
    if modal.map_row_count() > 0 {
        None
    } else if modal.random_maps_selected() && modal.search.is_empty() {
        Some(tr("VERA:NoRandomMaps"))
    } else if modal.online_maps_selected() {
        Some(crate::app::map_library::empty_online_maps_text(frontend))
    } else if modal.favorites_selected() && modal.search.is_empty() {
        Some(tr(if modal.favorites.is_empty() {
            "VERA:NoFavorites"
        } else {
            "VERA:NoFavoritesHere"
        }))
    } else {
        None
    }
}

/// The committed query plus the currently composed IME text, for display
/// only; filtering still uses the committed query.
pub(super) fn map_search_display(ime: &ImeState, search: &str) -> String {
    #[cfg(not(target_os = "android"))]
    let composition = ime.composition(crate::app::input::ime::ImeTarget::ChooseMapSearch);
    // Android: the search line is the focused GUI edit (`ImeTarget::Gui`).
    #[cfg(target_os = "android")]
    let composition = ime.open_composition();
    let composition = composition
        .map(|(text, _)| String::from_utf16_lossy(text))
        .unwrap_or_default();
    format!("{search}{composition}")
}

/// Where the search caret goes: after the text, inside the box. `None`
/// while the box is empty and shows its hint.
pub(super) fn map_search_caret_rect(
    font: &BitFont,
    search: &str,
    map_search: RectPx,
    scale: ShellScale,
) -> Option<RectPx> {
    if search.is_empty() {
        return None;
    }
    let inset = crate::ui::skirmish_shell::choose_map_page::MAP_SEARCH_INSET;
    Some(super::gui_paint::search_line_tail(font, search, map_search, inset, scale).1)
}

pub(super) fn push_start_marker_labels(
    out: &mut Vec<SpriteInstance>,
    bit_font: &BitFont,
    projected_positions: &[(i32, i32)],
    scale: ShellScale,
    depth: f32,
) {
    for (idx, &(x, y)) in projected_positions.iter().enumerate() {
        let label = (idx + 1).to_string();
        let (label_x, label_y) = start_marker_label_origin(x, y, scale);
        out.extend(bit_font.build_text(
            &label,
            label_x as f32,
            label_y as f32,
            scale.factor(),
            depth,
            SHELL_LABEL_TEXT_RGB,
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

/// The body and first button of Use Map's box. The download and random-map
/// failure texts are VERA's own; the retail string table has none.
pub(super) fn choose_map_prompt_labels(
    frontend: &FrontendState,
    process_assets: &ProcessAssets,
    prompt: &crate::ui::skirmish_shell::ChooseMapPrompt,
) -> (String, String) {
    use crate::ui::skirmish_shell::ChooseMapPromptKind;
    let record = prompt
        .selection
        .record_index
        .and_then(|index| frontend.scenario_catalog.records().get(index));
    let online = record.and_then(|record| {
        let map = frontend.map_library.online_map(record)?;
        Some((map.name.as_str(), record.player_capacity, map.byte_length))
    });
    // The map's name fills in last, so a `{...}` in it stays as written.
    let body = match (prompt.kind, online) {
        // A downloaded map's row is the saved file's, no longer the library's.
        (ChooseMapPromptKind::Downloaded, _) => match record {
            Some(record) => tr_fill("VERA:MapDownloaded", &[("name", &record.display_name)]),
            None => tr("VERA:MapDownloadedUnnamed"),
        },
        (ChooseMapPromptKind::Download, Some((name, players, bytes))) => tr_count(
            "VERA:DownloadMap",
            players,
            &[("size", &bytes.div_ceil(1024)), ("name", &name)],
        ),
        (ChooseMapPromptKind::Downloading, Some((name, ..))) => {
            match crate::app::map_library::download_progress(frontend) {
                Some((received, length)) => tr_fill(
                    "VERA:DownloadingMapPercent",
                    &[
                        ("percent", &(received.min(length) * 100 / length.max(1))),
                        ("name", &name),
                    ],
                ),
                None => tr_fill("VERA:DownloadingMap", &[("name", &name)]),
            }
        }
        (ChooseMapPromptKind::DownloadFailed, Some((name, ..))) => {
            tr_fill("VERA:MapDownloadFailed", &[("name", &name)])
        }
        (ChooseMapPromptKind::SaveFailed, Some((name, ..))) => {
            tr_fill("VERA:MapSaveFailed", &[("name", &name)])
        }
        (ChooseMapPromptKind::RandomMapFailed, _) => tr("VERA:RandomMapSaveFailed"),
        _ => process_assets.csf_label("GUI:EjectAIPlayers", "GUI:EjectAIPlayers"),
    };
    let ok = match prompt.kind {
        ChooseMapPromptKind::EjectAi
        | ChooseMapPromptKind::Downloaded
        | ChooseMapPromptKind::RandomMapFailed => process_assets.csf_label("GUI:Ok", "OK"),
        _ => tr("VERA:Download"),
    };
    (body, ok)
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::*;
    use crate::ui::skirmish_shell::COMBO_DROPDOWN_ROW_H;

    /// `0x105` hides a text an open list touches; that is exact only while
    /// every list ends in the gap between rows.
    #[test]
    fn every_random_map_list_covers_the_texts_under_it_whole() {
        for (width, height) in [(640, 480), (800, 600), (1024, 768)] {
            let layout = crate::ui::skirmish_shell::compute_random_map_setup_layout(width, height);
            // The Players window has the runtime growth (226 wide) the
            // combos do not have yet (225), so its value text runs one
            // column past the lists.
            let mut players_value = vera20k_gui::shell::trackbar::value_text_rect(
                layout.control_rects[5],
                crate::ui::shell::geom::RETAIL_SCALE,
            );
            players_value.w -= 1;
            let texts: Vec<RectPx> = (0..5)
                .map(|row| {
                    combo_text_rect(
                        layout.control_rects[row],
                        crate::ui::shell::geom::RETAIL_SCALE,
                    )
                })
                .chain([players_value])
                .collect();
            for combo in crate::ui::skirmish_shell::SETUP_COMBO_ROWS {
                let list = random_map_setup_dropdown_rect(
                    &layout,
                    combo.row(),
                    crate::ui::skirmish_shell::setup_combo_items(combo, false).len(),
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
    fn room_teams_past_d_take_the_lobby_letters() {
        assert_eq!(room_team_label(-2), None);
        assert_eq!(room_team_label(3), None);
        assert_eq!(room_team_label(4).as_deref(), Some("E"));
        assert_eq!(room_team_label(14).as_deref(), Some("O"));
    }

    fn map_entry_with_starts(start_count: usize) -> crate::map::scenario_menu::MapMenuEntry {
        crate::map::scenario_menu::MapMenuEntry {
            file_name: "map.mmx".to_string(),
            display_name: "map.mmx".to_string(),
            author: None,
            preview: Default::default(),
            multiplayer_start_waypoints: (0..start_count)
                .map(|index| crate::map::waypoints::Waypoint {
                    index: index as u32,
                    rx: index as u16,
                    ry: index as u16,
                })
                .collect(),
            player_capacity: start_count as i32,
            preview_source_bounds: None,
        }
    }

    #[test]
    fn team_labels_cover_every_item_the_team_combo_offers() {
        let shell = crate::ui::skirmish_shell::SkirmishShellState::default();
        let maps = [map_entry_with_starts(13)];
        let offered =
            crate::ui::skirmish_shell::combo_items(&shell, &maps, SkirmishComboId::Team(0));
        assert!(
            offered.contains(&SkirmishComboItem::Team(14)),
            "a map past gamemd's eight starts runs teams to O"
        );
        let labels = label_items(&shell, &maps);
        let teams: Vec<_> = labels
            .into_iter()
            .filter(|item| matches!(item, SkirmishComboItem::Team(_)))
            .collect();
        assert_eq!(
            teams, offered,
            "the Team column's labels must name every item its list offers"
        );
    }

    #[test]
    fn combo_face_text_color_uses_disabled_source_for_inactive_siblings() {
        let shell = crate::ui::skirmish_shell::SkirmishShellState::default();

        assert_eq!(combo_face_text_color(false), SHELL_LABEL_TEXT_RGB);
        assert_eq!(
            combo_face_text_color(true),
            SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F
        );
        // A sibling face greys with its row's combo (`gui_paint` paints a
        // disabled combo's caption in the disabled source).
        let sibling = |row| {
            let side = crate::ui::skirmish_shell::SkirmishComboId::Side(row);
            combo_face_text_color(!crate::ui::skirmish_shell::combo_enabled(&shell, &[], side))
        };
        assert_eq!(sibling(1), SHELL_LABEL_TEXT_RGB);
        assert_eq!(sibling(2), SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F);
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

        assert_eq!(
            combo_dropdown_text_fit_width(content, crate::ui::shell::geom::RETAIL_SCALE),
            20
        );

        let label = fit_owner_draw_label(&font, "ab cd", 20, str::to_owned);
        assert_eq!(label, "ab ");
        assert!(font.text_width(&label) <= 20);
    }

    /// VERA's own: a combo caption too wide for its face is squeezed whole,
    /// and loses its end only where the tightest squeeze is still too wide.
    #[test]
    fn an_owner_drawn_label_is_squeezed_before_it_is_cut() {
        let font = crate::render::bit_font::tests::make_vector_test_font();
        let label = "[Ŕåñðöɱ ~~~]";
        let natural = font.text_width(label);
        let squeezed = fit_owner_draw_label(&font, label, natural * 9 / 10, str::to_owned);
        assert_eq!(squeezed, label);
        let cut = fit_owner_draw_label(&font, label, natural / 3, str::to_owned);
        assert!(label.starts_with(&cut) && cut.len() < label.len(), "{cut}");
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

        let label = font.truncate_to_width("abcd", 14);

        assert_eq!(label.as_ref(), "ab");
        assert!(font.text_width(label.as_ref()) <= 14);
    }

    /// A search longer than its box keeps its end and the caret inside the
    /// box; an empty box shows its hint and no caret.
    #[test]
    fn a_long_map_search_scrolls_to_keep_its_end_and_caret_in_the_box() {
        let font = crate::render::bit_font::tests::make_test_font(&[(b'a' as u16, 6)], 4);
        let field = RectPx::new(338, 119, 196, 17);
        let long = "a".repeat(40);
        // 196 less two 2 px insets and the 2 px caret leaves 190: 27 of the
        // font's 7 px advances.
        let inset = crate::ui::skirmish_shell::choose_map_page::MAP_SEARCH_INSET;
        let scale = crate::ui::shell::geom::RETAIL_SCALE;
        let (tail, _) =
            super::super::gui_paint::search_line_tail(&font, &long, field, inset, scale);
        assert_eq!(tail, &long[13..]);
        let caret = map_search_caret_rect(&font, &long, field, scale).unwrap();
        assert_eq!(caret, RectPx::new(529, 121, 2, 13));
        assert!(caret.x + caret.w <= field.x + field.w - 2);
        assert_eq!(
            map_search_caret_rect(&font, "", field, crate::ui::shell::geom::RETAIL_SCALE),
            None
        );
    }

    /// VERA: Choose Map credits a map's author, and draws nothing for a map
    /// whose `Author` is missing or blank.
    #[test]
    fn owner_draw_label_truncates_latin1_by_utf16_code_unit() {
        let font = crate::render::bit_font::tests::make_test_font(
            &[(b'a' as u16, 6), (0x00e9, 8), (b'b' as u16, 6)],
            4,
        );

        let label = font.truncate_to_width("aéb", 16);

        assert_eq!(label.as_ref(), "aé");
        assert!(font.text_width(label.as_ref()) <= 16);
    }

    #[test]
    fn owner_draw_label_truncates_non_bmp_by_utf16_code_unit() {
        let font = crate::render::bit_font::tests::make_test_font(&[(b'a' as u16, 6)], 4);

        let label = fit_owner_draw_label(&font, "a😀", 13, str::to_owned);

        assert_eq!(label, "a\u{fffd}");
        assert_ne!(label, "a");
        assert!(font.text_width(&label) <= 13);
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

        let draw_rect = combo_face_text_draw_rect(combo, crate::ui::shell::geom::RETAIL_SCALE);
        assert_eq!(draw_rect, RectPx::new(425, 59, 42, 24));
        assert_eq!(
            combo_face_text_fit_width(combo, crate::ui::shell::geom::RETAIL_SCALE),
            24
        );
        let label = font.truncate_to_width(
            "ab cd",
            combo_face_text_fit_width(combo, crate::ui::shell::geom::RETAIL_SCALE),
        );

        assert_eq!(label.as_ref(), "ab ");
        assert!(
            font.text_width(label.as_ref())
                <= combo_face_text_fit_width(combo, crate::ui::shell::geom::RETAIL_SCALE)
        );
    }
}
