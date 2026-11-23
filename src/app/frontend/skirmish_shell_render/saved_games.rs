//! The saved-file browser (driver `0x00558DD0`): its list, description edit
//! and column buttons paint from its `vera20k-gui` tree, then its message
//! box over them. In a match it is the Load/Save/Delete of saved games, with
//! the active-game parent and button art; outside, the seed browser over
//! `0x105`, VERA's Setups page and Single Player's Load `0xB7`.

use super::gui_paint::{ButtonArt, CellRow, GuiPaint, GuiStyle, paint_gui_with};
use super::in_game_shell::{InGamePaint, render_in_game_shell_frame};
use super::shell_box::{BoxLook, push_skirmish_box};
use super::{SHELL_LABEL_TEXT_RGB, SHELL_MODAL_PANEL_RGB};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_text::ShellTextDraw;
use crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::pause_menu::PauseMenuButtonState;
use crate::ui::shell::saved_games::{saved_game_layout, saved_game_title_label};
use crate::ui::skirmish_shell::saved_seed_browser::{ACTION, BACK, BROWSER_DIALOG, CONVERT, LIST};
use crate::ui::skirmish_shell::{SavedSeedBrowserState, SavedSeedLayout};
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};

/// Each row's description, date and time in the template's columns (the
/// `0x4A8` stops 2, 255 and 315; `0x00559A07`'s x200 marker is refused by
/// the setter `0x0061B0D4`), each cut by wide units to its column with the
/// literal three dots (`0x00619969`).
fn row_cells<I>(
    state: &AppState,
    layout: &SavedSeedLayout,
    browser: &SavedSeedBrowserState<I>,
) -> Vec<CellRow> {
    let font = &state.renderer.bit_font;
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

/// The browser's widgets from its tree: the list (its rows unless `rows` is
/// off, as the `0xB7` teardown slide blanks it), the description edit on its
/// plain sunken plate, and the column's buttons in `buttons`' art.
pub(crate) fn paint_saved_browser<I>(
    state: &AppState,
    atlas: Option<&SkirmishShellChromeAtlas>,
    layout: &SavedSeedLayout,
    browser: &SavedSeedBrowserState<I>,
    buttons: ButtonArt,
    rows: bool,
) -> GuiPaint {
    let ui = &browser.ui;
    let (Some(atlas), Some(root)) = (atlas, ui.root_in(BROWSER_DIALOG)) else {
        return GuiPaint::default();
    };
    let cells =
        |id: i32| (rows && id == i32::from(LIST)).then(|| row_cells(state, layout, browser));
    let caret = |node| ui.caret_visible(node);
    let style = GuiStyle {
        buttons,
        cells_of: Some(&cells),
        caret_of: Some(&caret),
        edit_fill: Some(SHELL_MODAL_PANEL_RGB),
        ..GuiStyle::default()
    };
    paint_gui_with(state, atlas, ui.gui(), root, layout.scale, &[], style)
}

/// The browser's message box, when one is up: its art after `sprites`, its
/// texts after `texts`, which give up the box's rect first.
pub(crate) fn push_saved_browser_box<I>(
    state: &AppState,
    atlas: Option<&SkirmishShellChromeAtlas>,
    layout: &SavedSeedLayout,
    browser: &SavedSeedBrowserState<I>,
    sprites: &mut Vec<SpriteInstance>,
    texts: &mut Vec<ShellTextDraw>,
) {
    let Some(prompt) = browser.prompt.as_ref() else {
        return;
    };
    let look = BoxLook::prompt(atlas.and_then(|atlas| atlas.mode_two_dialog_background(None)));
    let size = (layout.screen.w, layout.screen.h);
    let pressed = |id| browser.pressed(id);
    let texts = Some((texts, true));
    push_skirmish_box(
        state,
        atlas,
        size,
        &prompt.shell_box(),
        pressed,
        look,
        Some(sprites),
        texts,
    );
}

/// A pre-match browser over its page: the seed browser over `0x105` or
/// VERA's Setups page over `0x102`, its `title` centred over the column.
pub(super) fn seed_browser_paint<I>(
    state: &AppState,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SavedSeedLayout,
    browser: &SavedSeedBrowserState<I>,
    title: &str,
) -> GuiPaint {
    let mut paint = paint_saved_browser(
        state,
        Some(atlas),
        layout,
        browser,
        ButtonArt::PanelCentred,
        true,
    );
    super::text::push_text_draw(
        &mut paint.texts,
        state,
        title,
        super::text::rect_to_text_rect(layout.title),
        SHELL_LABEL_TEXT_RGB,
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
        super::SHELL_DROPDOWN_TEXT_DEPTH - 0.00012,
        layout.scale.factor(),
    );
    push_saved_browser_box(
        state,
        Some(atlas),
        layout,
        browser,
        &mut paint.controls,
        &mut paint.texts,
    );
    paint
}

pub(crate) fn render_saved_game_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let frame = {
        let mut paint = InGamePaint::new(state);
        let state = paint.state;
        let presentation = &state.match_state.match_presentation;
        let crate::ui::pause_menu::InGameMenuState::SavedGame(menu_mode) =
            presentation.in_game_menu
        else {
            return Ok(());
        };
        let browser = presentation.saved_game_browser.as_ref();
        let mode = browser.map_or(menu_mode, |browser| browser.mode);
        let layout = paint
            .shell
            .place(|w, h, shell, size| saved_game_layout(mode, w, h, shell, size));
        // 558F8A's hide40C condition is false in the active-game route. Both
        // resource statics use lowstylebits1: ownerdraw615A81 selects top/H_CENTER.
        let title = paint.label(browser.map_or(
            saved_game_title_label(mode),
            SavedSeedBrowserState::game_title_label,
        ));
        paint.text(&title, layout.title, ShellAlign::PANEL_TITLE);
        let prompt = paint.label(mode.prompt_label());
        paint.text(&prompt, layout.prompt, ShellAlign::H_CENTER);
        if let Some(key) = browser
            .filter(|b| b.is_naming_conversion())
            .and_then(SavedSeedBrowserState::hovered_help)
        {
            paint.text(&state.csf_text(key), layout.blank, ShellAlign::NONE);
        }
        // The column's buttons are the active game's SIDEBTTN art.
        if let Some(browser) = browser {
            let widgets = paint_saved_browser(
                state,
                state.frontend.skirmish_shell_chrome.as_ref(),
                &layout,
                browser,
                ButtonArt::Host,
                true,
            );
            paint.controls.extend(widgets.controls);
            paint.texts.extend(widgets.texts);
        }
        let convert = layout.convert.map(|rect| {
            (
                CONVERT,
                rect,
                ("VERA:ConvertSave", "Convert"),
                browser.is_some_and(SavedSeedBrowserState::convert_enabled),
            )
        });
        for (id, rect, label, enabled) in [
            Some((
                ACTION,
                layout.action,
                mode.action_label(),
                browser.is_some_and(SavedSeedBrowserState::action_enabled),
            )),
            convert,
            Some((BACK, layout.back, ("GUI:Back", "Back"), true)),
        ]
        .into_iter()
        .flatten()
        {
            // A confirmation drops the browser's press while open; its buttons
            // are painted by the common prompt renderer, not the underlying column.
            let pressed =
                browser.is_some_and(|browser| browser.prompt.is_none() && browser.pressed(id));
            let caption = paint.label(label);
            paint.owner_button(
                rect,
                &caption,
                PauseMenuButtonState {
                    pressed,
                    highlighted: false,
                    enabled,
                },
            );
        }
        // The box's clipping excludes all underlying text, the active-game
        // title and column labels included.
        if let Some(browser) = browser {
            push_saved_browser_box(
                state,
                state.frontend.skirmish_shell_chrome.as_ref(),
                &layout,
                browser,
                &mut paint.controls,
                &mut paint.texts,
            );
        }
        paint.finish("Active Saved Game Browser")
    };
    render_in_game_shell_frame(state, encoder, destination, frame)
}
