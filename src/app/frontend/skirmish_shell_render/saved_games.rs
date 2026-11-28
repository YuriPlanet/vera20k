//! The saved-file browser (driver `0x00558DD0`): its list, description edit
//! and column buttons paint from its `vera20k-gui` tree, then its message
//! box over them. In a match it is the Load/Save/Delete of saved games, with
//! the active-game parent and button art; outside, the seed browser over
//! `0x105`, VERA's Setups page and Single Player's Load `0xB7`.

use super::gui_paint::{ButtonArt, CellRow, GuiStyle, paint_gui_into};
use super::in_game_shell::{InGamePaint, render_in_game_shell_frame};
use super::shell_box::{BoxLook, push_skirmish_box, skirmish_parts};
use super::{SHELL_LABEL_TEXT_RGB, SHELL_MODAL_PANEL_RGB};
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, Material};
use crate::render::shell_text::ShellTextDraw;
use crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::saved_games::saved_game_layout;
use crate::ui::sidebar::SidebarTheme;
use crate::ui::skirmish_shell::saved_seed_browser::{
    ACTION, BACK, BROWSER_DIALOG, CONVERT, LIST, PROMPT, PROMPT_DIALOG, TITLE,
};
use crate::ui::skirmish_shell::{SavedSeedBrowserState, SavedSeedLayout, SavedSeedMode};
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};

/// Each row's description, date and time in the template's columns (the
/// `0x4A8` stops 2, 255 and 315; `0x00559A07`'s x200 marker is refused by
/// the setter `0x0061B0D4`), each cut by wide units to its column with the
/// literal three dots (`0x00619969`).
fn row_cells<I>(
    bit_font: &BitFont,
    layout: &SavedSeedLayout,
    browser: &SavedSeedBrowserState<I>,
) -> Vec<CellRow> {
    let font = bit_font;
    let metrics = ListMetrics::scaled(layout.scale);
    let geometry = ShellListGeometry::new(layout.list, browser.entries.len(), 0, metrics);
    let row_w = layout.scale.base_rect(geometry.row(0)).w;
    let fit = |text: String, width: i32| {
        let mut units: Vec<u16> = text.encode_utf16().collect();
        let mut label = text;
        while font.text_width(&label) > width.max(0) as u32 && !units.is_empty() {
            units.pop();
            label = format!("{}...", String::from_utf16_lossy(&units));
        }
        label
    };
    let file_time = crate::util::native_file_time::format_file_time_parts;
    let cells = |entry: &crate::ui::skirmish_shell::SavedSeedBrowserRow<I>| {
        let (date, time) = file_time(entry.last_write_time).unwrap_or_default();
        let mut row = CellRow::default();
        for (text, x, w) in [
            (entry.description.display_text(), 2, 249),
            (date, 255, 56),
            (time, 315, 0),
        ] {
            let w = match w {
                0 => row_w - x,
                w => w.min(row_w - x),
            };
            if w <= 0 {
                continue;
            }
            row.text(x, w, &fit(text, w), ShellAlign::NONE, SHELL_LABEL_TEXT_RGB);
        }
        row
    };
    browser.entries.iter().map(cells).collect()
}

/// The browser's widgets from its tree: the active game's heading and
/// prompt, the list (its rows unless `rows` is off, as the `0xB7` teardown
/// slide blanks it), the description edit on its plain sunken plate, and
/// the column's buttons in `buttons`' art. Without `atlas` only the texts
/// paint.
pub(crate) fn paint_saved_browser<I>(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: Option<&SkirmishShellChromeAtlas>,
    layout: &SavedSeedLayout,
    browser: &SavedSeedBrowserState<I>,
    buttons: ButtonArt,
    rows: bool,
) -> HudPaint {
    let ui = &browser.ui;
    let mut paint = HudPaint::default();
    let Some(root) = ui.root_in(BROWSER_DIALOG) else {
        return paint;
    };
    let cells =
        |id: i32| (rows && id == i32::from(LIST)).then(|| row_cells(bit_font, layout, browser));
    let caret = |node| ui.caret_visible(node);
    // 558F8A's hide40C condition is false in the active-game route. Both
    // resource statics use lowstylebits1: ownerdraw615A81 selects top/H_CENTER.
    let text_of = |id: i32| {
        let label = match u16::try_from(id).ok()? {
            TITLE => browser.game_title_label(),
            PROMPT => browser.mode.prompt_label(),
            _ => return None,
        };
        browser
            .statics
            .then(|| process_assets.csf_label(label.0, label.1))
    };
    let style = GuiStyle {
        buttons,
        text_of: Some(&text_of),
        cells_of: Some(&cells),
        caret_of: Some(&caret),
        edit_fill: Some(SHELL_MODAL_PANEL_RGB),
        ..GuiStyle::default()
    };
    paint_gui_into(
        &mut paint,
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        atlas,
        ui.gui(),
        root,
        layout.scale,
        &[],
        style,
    );
    paint
}

/// The browser's message box, when one is up: its art after `sprites`, its
/// texts after `texts`, which give up the box's rect first.
pub(crate) fn push_saved_browser_box<I>(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: Option<&SkirmishShellChromeAtlas>,
    layout: &SavedSeedLayout,
    browser: &SavedSeedBrowserState<I>,
    side: Option<SidebarTheme>,
    sprites: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
) {
    let Some(prompt) = browser.prompt.as_ref() else {
        return;
    };
    let look = BoxLook::prompt(atlas.and_then(|atlas| atlas.mode_two_dialog_background(side)));
    let shell_box = prompt.shell_box();
    let parts = skirmish_parts(&shell_box, (layout.screen.w, layout.screen.h));
    let pressed = |id| browser.pressed(id);
    let texts = (texts, true);
    // Its widgets, which the browser's input put on it.
    let tree = super::controller_tree(&browser.ui, PROMPT_DIALOG);
    push_skirmish_box(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        atlas,
        tree,
        &shell_box,
        &parts,
        pressed,
        look,
        sprites,
        texts,
    );
}

/// A pre-match browser over its page: the seed browser over `0x105` or
/// VERA's Setups page over `0x102`, its `title` centred over the column.
pub(super) fn seed_browser_paint<I>(
    ime: &ImeState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SavedSeedLayout,
    browser: &SavedSeedBrowserState<I>,
    title: &str,
) -> HudPaint {
    let mut paint = paint_saved_browser(
        ime,
        &renderer.bit_font,
        frontend,
        match_state,
        process_assets,
        Some(atlas),
        layout,
        browser,
        ButtonArt::PanelCentred,
        true,
    );
    paint
        .texts
        .push(crate::render::shell_text::draw_in_rect_scaled(
            &renderer.bit_font,
            title,
            crate::render::shell_text::TextRect::of(layout.title),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            [0.0, 0.0],
            super::SHELL_DROPDOWN_TEXT_DEPTH - 0.00012,
            layout.scale.factor(),
        ));
    paint.draw(Material::Controls, |sprites, texts| {
        push_saved_browser_box(
            ime,
            &renderer.bit_font,
            frontend,
            match_state,
            process_assets,
            Some(atlas),
            layout,
            browser,
            None,
            sprites,
            texts,
        )
    });
    paint
}

pub(crate) fn render_saved_game_shell(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let frame = {
        let mut paint = InGamePaint::new(platform, renderer, frontend, match_state, persistence);
        let (ime, bit_font) = (&platform.ime, &renderer.bit_font);
        let presentation = &match_state.match_presentation;
        let crate::ui::pause_menu::InGameMenuState::SavedGame(_) = presentation.in_game_menu else {
            return Ok(());
        };
        // The browser is open for as long as the menu shows it.
        let Some(browser) = presentation.saved_game_browser.as_ref() else {
            return Ok(());
        };
        let layout = paint
            .shell
            .place(|w, h, shell, size| saved_game_layout(browser.mode, w, h, shell, size));
        // The heading and the prompt, then the widgets; the column's buttons
        // are the active game's SIDEBTTN art.
        let widgets = paint_saved_browser(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            frontend.skirmish_shell_chrome.as_ref(),
            &layout,
            browser,
            ButtonArt::Host,
            true,
        );
        paint
            .controls
            .extend(widgets.lane(Material::Controls).copied());
        paint.texts.extend(widgets.texts);
        // A conversion's naming step explains the hovered control in the
        // blank band.
        if let Some(key) = Some(browser)
            .filter(|b| b.is_naming_conversion())
            .and_then(SavedSeedBrowserState::hovered_help)
        {
            paint
                .texts
                .push(crate::render::shell_text::draw_in_rect_scaled(
                    bit_font,
                    &process_assets.csf_text(key),
                    crate::render::shell_text::TextRect::of(layout.blank),
                    SHELL_LABEL_TEXT_RGB,
                    ShellAlign::NONE,
                    [0.0, 0.0],
                    super::SHELL_CONTROL_TEXT_DEPTH,
                    1.0,
                ));
        }
        // Action, Convert and Back from their nodes on the side's type-2 art.
        // They keep the pushed look while they hold the press, on the pointer
        // or not; a confirmation drops the browser's press while open (its
        // buttons are the common prompt renderer's).
        let text_of = |id: i32| -> Option<String> {
            match u16::try_from(id).ok()? {
                ACTION => {
                    let (key, fallback) = browser.mode.action_label();
                    Some(process_assets.csf_label(key, fallback))
                }
                CONVERT => Some(process_assets.csf_label("VERA:ConvertSave", "Convert")),
                BACK => Some(process_assets.csf_label("GUI:Back", "Back")),
                _ => None,
            }
        };
        let enabled_of = |id: i32| -> Option<bool> {
            match u16::try_from(id).ok()? {
                ACTION => Some(browser.action_enabled()),
                CONVERT => Some(browser.convert_enabled()),
                BACK => Some(true),
                _ => None,
            }
        };
        let pressed_of = |id: i32| -> Option<bool> {
            let id = u16::try_from(id).ok()?;
            matches!(id, ACTION | CONVERT | BACK)
                .then(|| browser.prompt.is_none() && browser.pressed(id))
        };
        let style = GuiStyle {
            buttons: ButtonArt::Side,
            side: Some(paint.side_buttons()),
            text_of: Some(&text_of),
            enabled_of: Some(&enabled_of),
            pressed_of: Some(&pressed_of),
            ..GuiStyle::default()
        };
        if let Some(root) = browser.ui.root_in(BROWSER_DIALOG) {
            paint.nodes_in(
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                browser.ui.gui(),
                root,
                &[ACTION, CONVERT, BACK],
                style,
            );
        }
        // game.exe's mode-2 paint (0x005FF1F5) draws the player's side
        // PUDLGBG art while a game runs; classic RA2's Save and Delete
        // prompts show it.
        let prompt_side = (crate::util::ra2_launch::enabled()
            && matches!(browser.mode, SavedSeedMode::Save | SavedSeedMode::Delete))
        .then_some(paint.theme);
        // The box's clipping excludes all underlying text, the active-game
        // title and column labels included.
        push_saved_browser_box(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            frontend.skirmish_shell_chrome.as_ref(),
            &layout,
            browser,
            prompt_side,
            &mut paint.controls,
            &mut paint.texts,
        );
        paint.finish("Active Saved Game Browser")
    };
    render_in_game_shell_frame(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        encoder,
        destination,
        frame,
    )
}
