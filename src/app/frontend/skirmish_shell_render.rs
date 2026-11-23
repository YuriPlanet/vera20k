//! Skirmish shell sprite construction and render pass.
//!
//! Part of the app layer: may depend on ui and render modules. Keeps the
//! `GameScreen::MainMenu` branch in the app facade small.

mod chrome;
mod controls;
mod draw_order;
mod game_options;
pub(crate) mod gui_paint;
#[cfg(test)]
use draw_order::{
    SkirmishShellDrawRole, choose_map_modal_semantic_draw_order,
    random_map_setup_semantic_draw_order, skirmish_shell_semantic_draw_order,
    validation_modal_semantic_draw_order,
};
mod in_game_shell;
pub(crate) use in_game_shell::{
    current_in_game_shell_layout, in_game_shell_cursor, in_game_shell_size,
    native_in_game_shell_active,
};
mod pause_menu;
pub(crate) use pause_menu::render_pause_menu_shell;
mod saved_games;
pub(crate) use saved_games::{paint_saved_browser, push_saved_browser_box, render_saved_game_shell};
mod launcher_options;
pub(crate) use launcher_options::{paint_options_settings, render_launcher_options};
mod modals;
mod network_failure;
pub(crate) use modals::render_saved_game_loading;
pub(crate) use network_failure::render_network_failure;
mod preview;
mod text;

pub(crate) use preview::{
    SkirmishPreviewTexture, is_random_map_sentinel_entry, start_marker_picture,
};
pub(super) use text::text_covered_by_overlay;

pub(crate) use text::{
    combo_item_label, skirmish_labels, skirmish_right_panel_label_strings,
    skirmish_selected_map_name,
};

/// The IME candidate anchor follows the search text and any active preedit.
pub(crate) fn choose_map_search_ime_area(
    state: &AppState,
    layout: &ChooseMapModalLayout,
    search: &str,
) -> crate::ui::shell::geom::RectPx {
    let display = text::map_search_display(state, search);
    text::map_search_caret_rect(
        &state.renderer.bit_font,
        &display,
        layout.map_search,
        layout.scale,
    )
    .unwrap_or(layout.map_search)
}

use crate::app::AppState;
use crate::app::frontend::shell_transition::ShellFrameWave;
use crate::app::loading::init::MapMenuEntry;
#[cfg(test)]
use crate::map::preview::PreviewSourceBounds;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::shell_text::ShellTextDraw;
use crate::render::shell_transition_pass::ShellRenderTarget;
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::skirmish_launch::LaunchCountry;
#[cfg(test)]
use crate::ui::shell::geom::ShellAlign;
#[cfg(test)]
use crate::ui::skirmish_shell::COMBO_DROPDOWN_ROW_H;
use crate::ui::skirmish_shell::{
    ChooseMapModalLayout, RandomMapSetupLayout, RectPx,
    SkirmishShellAction, SkirmishShellLayout, SkirmishShellState, player_row_visible,
};

use self::chrome::*;
use self::controls::*;
use self::draw_order::ShellDialogChromeProfile;
#[cfg(test)]
use self::draw_order::{
    GenericBackgroundRole, LowerStripRole, generic_background_role, lower_strip_role,
};
use self::modals::*;
use self::preview::*;
use self::text::*;

const PRESSED_BUTTON_CONTENT_OFFSET_Y: i32 = 2;
const START_MARKER_OFFSET_X: i32 = -9;
const START_MARKER_OFFSET_Y: i32 = -6;
const BUTTON_DISABLED_ALPHA: f32 = 0x80 as f32 / 255.0;
const SHELL_PARENT_BACKGROUND_DEPTH: f32 = 0.00090;
const SHELL_LOWER_STRIP_DEPTH: f32 = 0.00077;
const SHELL_PREVIEW_BACKDROP_DEPTH: f32 = 0.00059;
const SHELL_PREVIEW_SURFACE_DEPTH: f32 = 0.00058;
const SHELL_CONTROL_DEPTH: f32 = 0.00055;
const SHELL_CONTROL_TEXT_DEPTH: f32 = 0.00039;
const SHELL_SWATCH_DEPTH: f32 = 0.00054;
const SHELL_EDIT_FRAME_DEPTH: f32 = 0.00053;
const SHELL_EDIT_SELECTION_DEPTH: f32 = 0.00040;
const SHELL_EDIT_CARET_DEPTH: f32 = 0.00037;
const SHELL_DROPDOWN_DEPTH: f32 = 0.00034;
const SHELL_DROPDOWN_TEXT_DEPTH: f32 = 0.00029;
/// Software cursor draws on top of everything else on the shell (smallest
/// depth). The original hides the OS cursor and blits the cursor SHP last.
const SHELL_CURSOR_DEPTH: f32 = 0.00001;
const SHELL_LABEL_TEXT_RGB: [f32; 3] = [1.0, 1.0, 0.0];
const SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F: [f32; 3] = [0x9F as f32 / 255.0, 0.0, 0.0];
const COMBODROPWIN_TEXT_INSET_X: i32 = 3;
const COMBODROPWIN_TEXT_TRUNCATION_SCROLLBAR_RESERVE_PX: i32 = 20;
// Owner-draw packed colors are 0x00BBGGRR. Runtime DirectDraw conversion is
// display-format dependent; this renderer uses the decoded source RGB values.
const OWNERDRAW_BEVEL_LIGHT_RGB_FROM_PACKED_00C5BEA7: [f32; 3] = [
    0xA7 as f32 / 255.0,
    0xBE as f32 / 255.0,
    0xC5 as f32 / 255.0,
];
const OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68: [f32; 3] = [
    0x68 as f32 / 255.0,
    0x7A as f32 / 255.0,
    0x80 as f32 / 255.0,
];
const OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF: [f32; 3] = [1.0, 0.0, 0.0];
const SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE: [f32; 3] = [0.015, 0.024, 0.018];
const SHELL_SCROLLBAR_TRACK_RGB_PENDING_SCROLLBAR_SOURCE_CAPTURE: [f32; 3] = [0.035, 0.042, 0.034];
const SHELL_MODAL_BG_RGB: [f32; 3] = [0.020, 0.032, 0.025];
const SHELL_MODAL_PANEL_RGB: [f32; 3] = [0.050, 0.060, 0.044];

pub(crate) fn side_item_data_for_country(country: LaunchCountry) -> i32 {
    i32::from(country.raw_index())
}

pub(crate) fn flag_pcx_for_side_item_data(item_data: i32) -> Option<&'static str> {
    match item_data {
        -3 => Some("obsi.pcx"),
        -2 => Some("rani.pcx"),
        0 => Some("usai.pcx"),
        1 => Some("japi.pcx"),
        2 => Some("frai.pcx"),
        3 => Some("geri.pcx"),
        4 => Some("gbri.pcx"),
        5 => Some("djbi.pcx"),
        6 => Some("arbi.pcx"),
        7 => Some("lati.pcx"),
        8 => Some("rusi.pcx"),
        9 => Some("yrii.pcx"),
        _ => None,
    }
}

fn flag_name_for_country(country: LaunchCountry) -> Option<&'static str> {
    flag_pcx_for_side_item_data(side_item_data_for_country(country))
}

fn flag_name_for_country_choice<'a>(
    catalog: &'a crate::ui::skirmish_shell::CountryMenuCatalog,
    random: bool,
    country: LaunchCountry,
) -> Option<&'a str> {
    catalog.flag_for_choice(random, country).or_else(|| {
        if random {
            flag_pcx_for_side_item_data(-2)
        } else {
            flag_name_for_country(country)
        }
    })
}

fn flag_entry(atlas: &SkirmishShellChromeAtlas, label: &str) -> Option<SkirmishShellChromeEntry> {
    atlas
        .flags
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(label))
        .map(|(_, entry)| *entry)
}

/// Depth of `0x6B`'s slide column, with the steady chooser buttons.
const CHOOSE_MAP_COLUMN_DEPTH: f32 = SHELL_DROPDOWN_DEPTH - 0.00011;

/// The top panel and the column: while a slide runs, the art the engine
/// draws in their place (map button, top panel, warning display and the
/// whole tile column, `0x006071E0`), else the dialog's steady top.
fn push_panel_chrome(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
    wave: Option<&ShellFrameWave>,
    profile: ShellDialogChromeProfile,
    column_depth: f32,
) {
    match wave {
        Some(wave) => {
            push_slide_panel_art(out, atlas, layout, &wave.panel_art());
            push_slide_column(out, atlas, layout, &wave.button_draws(), column_depth);
        }
        None => push_steady_optional_chrome_instances(out, atlas, layout, profile),
    }
}

fn build_skirmish_shell_instances(
    atlas: &SkirmishShellChromeAtlas,
    font: &BitFont,
    layout: &SkirmishShellLayout,
    choose_map_layout: Option<&ChooseMapModalLayout>,
    // The seed browser covers `0x105`.
    seed_browser: bool,
    setup_layout: Option<&RandomMapSetupLayout>,
    // VERA's Setups page covers `0x102`.
    setups_layout: bool,
    shell: &SkirmishShellState,
    search_display: &str,
    maps: &[MapMenuEntry],
    wave: Option<&ShellFrameWave>,
    leaving: bool,
    // `0x102`'s widgets as the GUI paints them, and the computer rows'
    // scrollbar arrow its List draws pushed.
    // The random-map Advanced scrollbar's widget.
    (page, opponent_arrow, advanced_bar): (
        &[SpriteInstance],
        Option<vera20k_gui::shell::list::ListScrollPart>,
        Option<vera20k_gui::ScrollBarView>,
    ),
) -> Vec<SpriteInstance> {
    let mut instances = Vec::new();
    // Every page of the family starts with the right panel and the lower
    // strip; only `0x102` and the Setups over it shutter the panel's slots.
    let frame10 = choose_map_layout.is_none() && right_panel_frame10_overlay_active(shell);
    push_right_panel_base_instances(&mut instances, atlas, layout, frame10);
    push_lower_strip_instance(&mut instances, atlas, layout);

    if let Some(choose_map_layout) = choose_map_layout {
        // The setup dialog REPLACES the chooser rather than overlaying it: the
        // original hides/suspends the chooser while 0x105 is up. Drawing both
        // collides their listboxes, labels and button captions.
        // The browser paints over this from its tree.
        if seed_browser {
            push_choose_map_background_instances(
                &mut instances,
                atlas,
                layout,
                choose_map_layout,
                1.0,
            );
        } else if let (Some(modal), Some(setup_layout)) =
            (shell.random_map_setup_modal.as_ref(), setup_layout)
        {
            push_generic_shell_background_instances(&mut instances, atlas, layout);
            // `0x105`'s slides draw the top panel and the column like `0x6B`'s;
            // the slide-out repaint blanks every child.
            let profile = ShellDialogChromeProfile::RandomMapSetup0x105;
            push_panel_chrome(&mut instances, atlas, layout, wave, profile, CHOOSE_MAP_COLUMN_DEPTH);
            if !leaving {
                push_random_map_setup_modal_control_instances(&mut instances, atlas, setup_layout, modal, advanced_bar);
            }
        } else {
            let interior = push_choose_map_background_instances(
                &mut instances,
                atlas,
                layout,
                choose_map_layout,
                modals::CHOOSE_MAP_BACKGROUND_BRIGHTNESS,
            );
            // `0x6B`'s slides draw the top panel and the column like `0x102`'s
            // (no map button); the slide-out repaint blanks every child.
            let profile = ShellDialogChromeProfile::ChooseMap0x6b;
            push_panel_chrome(&mut instances, atlas, layout, wave, profile, CHOOSE_MAP_COLUMN_DEPTH);
            if !leaving {
                push_choose_map_modal_control_instances(
                    &mut instances,
                    atlas,
                    font,
                    choose_map_layout,
                    interior,
                    search_display,
                    wave.is_none(),
                );
            }
        }
        return instances;
    }

    // VERA's Setups page covers `0x102` as the seed browser covers `0x105`:
    // the panel, the strip, the page background, then the browser. The
    // panel keeps `0x102`'s top art, the map button framing the preview.
    if setups_layout {
        push_generic_shell_background_instances(&mut instances, atlas, layout);
        push_steady_optional_chrome_instances(
            &mut instances,
            atlas,
            layout,
            ShellDialogChromeProfile::SkirmishSetup0x102,
        );
        return instances;
    }

    // Dialog 0x102 selects its background by the persisted shell style. YR's
    // small shape is shared with RA2 but its context palette is not.
    if let Some(background) = offline_setup_background_entry(atlas, layout) {
        let (x, y) = layout
            .scale
            .dialog_origin_or((layout.screen.x, layout.screen.y));
        push_entry_grown(
            &mut instances,
            background,
            x,
            y,
            layout.scale,
            SHELL_PARENT_BACKGROUND_DEPTH,
        );
    }

    const RIGHT_PANEL_BUTTON_DEPTH: f32 = 0.00059;
    let profile = ShellDialogChromeProfile::SkirmishSetup0x102;
    push_panel_chrome(&mut instances, atlas, layout, wave, profile, RIGHT_PANEL_BUTTON_DEPTH);
    // The teardown slide repaints the dialog over every child and runs to its
    // end without dispatching messages (`0x00622C4F`, `0x006071E0`): only the
    // dialog's own paint and the button frames show.
    if leaving {
        return instances;
    }

    // The widgets, the name box among them, follow the parent background:
    // this batch has no depth test and paints in queue order.
    instances.extend_from_slice(page);
    push_opponent_scrollbar_instances(&mut instances, atlas, layout, shell, maps, opponent_arrow);

    if let Some(flag) = flag_name_for_country_choice(
        &shell.country_catalog,
        shell.player_country_random,
        shell.player_country,
    )
    .and_then(|name| flag_entry(atlas, name))
    {
        push_flag_entry_clipped_centered(
            &mut instances,
            flag,
            layout.flags[0],
            layout.scale,
            0.00057,
        );
    }
    for (slot, row, opponent) in shell.paged_opponents(layout.shown_rows) {
        if !player_row_visible(shell, maps, row) || !opponent.is_active() {
            continue;
        }
        let entry = flag_name_for_country_choice(
            &shell.country_catalog,
            opponent.country_random,
            opponent.country,
        )
        .and_then(|name| flag_entry(atlas, name));
        if let Some(flag) = entry {
            push_flag_entry_clipped_centered(
                &mut instances,
                flag,
                layout.flags[slot],
                layout.scale,
                0.00057,
            );
        }
    }
    instances
}

pub(crate) fn render_skirmish_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<SkirmishShellAction> {
    let color = state.renderer.shell_surface_presenter.source_render_view();
    let depth = state.renderer.depth_view.clone();
    let action = render_skirmish_shell_to_target(
        state,
        encoder,
        ShellRenderTarget {
            color: &color,
            depth: &depth,
        },
    )?;
    state
        .renderer
        .shell_surface_presenter
        .encode_present(encoder, destination);
    Ok(action)
}

pub(crate) fn render_skirmish_shell_to_target(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    target: ShellRenderTarget<'_>,
) -> anyhow::Result<SkirmishShellAction> {
    let chrome = state.frontend.skirmish_shell_chrome.take();
    let result = render_skirmish_shell_with_atlas(state, encoder, target, chrome.as_ref());
    state.frontend.skirmish_shell_chrome = chrome;
    result
}

fn render_skirmish_shell_with_atlas(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    target: ShellRenderTarget<'_>,
    atlas: Option<&SkirmishShellChromeAtlas>,
) -> anyhow::Result<SkirmishShellAction> {
    // Collect whatever the generator has produced before laying anything out,
    // so each in-progress preview lands on the next frame drawn and the frame
    // that clears "Working / Please Wait" is the one showing the finished map.
    // While a job is in flight the shell keeps asking for frames -- nothing else
    // is driving redraws, and without this the dialog would freeze on the first
    // "working" frame until the player moved the mouse.
    if crate::app::App::poll_random_map_generation(state)
        || state.frontend.random_map_generation.is_some()
    {
        state.platform.window.request_redraw();
    }
    let layout = crate::app::frontend::shell_layout::skirmish_layout(state);
    let choose_map_layout = state
        .frontend
        .skirmish_shell_state
        .choose_map_modal
        .as_ref()
        .map(|_| crate::app::frontend::shell_layout::choose_map_layout(state));
    let search_display = state
        .frontend
        .skirmish_shell_state
        .choose_map_modal
        .as_ref()
        .map(|modal| text::map_search_display(state, &modal.search))
        .unwrap_or_default();
    // `0x105` and the seed browser each replace the page beneath them, and both
    // the sprite pass and the text pass need the same rects: one layout per page
    // per frame, from the same seam the input handlers read.
    let seed_layout = state
        .frontend
        .skirmish_shell_state
        .saved_seed_browser
        .as_ref()
        .map(|browser| crate::app::frontend::shell_layout::saved_seed_layout(state, browser.mode));
    let setups_layout = state
        .frontend
        .skirmish_shell_state
        .setups_browser
        .as_ref()
        .map(|_| crate::app::frontend::shell_layout::setups_layout(state));
    let setup_layout = state
        .frontend
        .skirmish_shell_state
        .random_map_setup_modal
        .as_ref()
        .map(|_| crate::app::frontend::shell_layout::random_map_setup_layout(state));
    let validation = state.frontend.skirmish_shell_state.validation_modal.clone();
    // `ShellMessageBox__Run` draws the empty backdrop before its box
    // (`0x005D3514`): while a box is up the dialog behind does not show.
    let message_box = state
        .frontend
        .skirmish_shell_state
        .choose_map_modal
        .as_ref()
        .and_then(|modal| modal.message_box);
    let message_box_up = validation.is_some() || message_box.is_some();
    let action = SkirmishShellAction::None;

    let Some(atlas) = atlas else {
        clear_shell_target(state, encoder, target);
        return Ok(action);
    };

    let exit_wave = crate::app::frontend::shell_transition::skirmish_slide_target(
        &state.frontend.skirmish_shell_state,
    )
    .and_then(|kind| crate::app::frontend::shell_transition::shell_exit_wave(state, kind))
    .cloned();
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| state.frontend.shell_first_paint_slide.clone());
    // Every slide tick blits the top panel and the tile column from the
    // dialog's own surface (`0x00607EC8`, `0x00607EF1` with record `+0xD5`),
    // so no right-panel child shows while either slide runs.
    let sliding = wave.is_some();
    let setup_layout = setup_layout.as_ref();
    // While the setup dialog is up it owns the preview box, so the image goes in
    // its own 0x468 rect rather than the chooser's.
    let setup_preview_rect = setup_layout.map(|layout| layout.preview);
    let preview_rect = setup_preview_rect
        .or_else(|| choose_map_layout.as_ref().map(|layout| layout.preview))
        .unwrap_or(layout.map_preview);
    ensure_selected_preview_texture(state, preview_rect);
    let zoom_layout =
        ensure_zoom_texture(state, layout.scale, !sliding && !leaving && !message_box_up);
    let selected_entry = preview::preview_map_idx(state)
        .and_then(|index| state.frontend.scenario_catalog.shell_maps().get(index));
    // A generated preview carries its red squares (0x00641140) and also gets
    // the numbered markers from the last Generate's header: the setup dialog
    // paints them (0x00596ACD), and selecting the sentinel copies the header
    // back first (0x0058BB30 at 0x005E67ED/0x006AEE9C).
    // RESIDUAL: native also numbers its in-progress redraws, from the previous
    // header until 0x00594EE3 runs; VERA draws none while generating.
    let setup_modal = state
        .frontend
        .skirmish_shell_state
        .random_map_setup_modal
        .as_ref();
    let random_preview = setup_modal.is_some_and(|modal| !modal.generating)
        || (setup_modal.is_none() && selected_entry.is_some_and(is_random_map_sentinel_entry));
    let selected_preview_bounds = start_marker_bounds(
        random_preview.then(|| state.frontend.random_map_retention.generated_header()),
        selected_entry
            .filter(|_| setup_modal.is_none())
            .and_then(|entry| entry.preview_source_bounds.as_ref()),
    );
    let selected_preview_bounds = selected_preview_bounds.as_ref();
    let fitted_preview_rect = state
        .frontend
        .skirmish_preview_texture
        .as_ref()
        .map(|preview| aspect_fit_rect(preview_rect, preview.width, preview.height));
    let projected_start_positions = selected_preview_bounds
        .zip(fitted_preview_rect)
        .map(|(bounds, rect)| project_preview_start_positions(bounds, rect))
        .unwrap_or_default();
    let zoom = zoom_layout.as_ref().map(|zoom_layout| {
        zoom_draws(
            state,
            atlas,
            zoom_layout,
            selected_preview_bounds,
            layout.scale,
        )
    });
    // The map preview static and its markers sit in the top panel.
    let draw_start_marker_overlays = !sliding
        && !message_box_up
        && should_draw_start_marker_overlays(fitted_preview_rect, &projected_start_positions);
    let preview_instance = state
        .frontend
        .skirmish_preview_texture
        .as_ref()
        .filter(|_| !sliding && !message_box_up)
        .and_then(|preview| {
            build_preview_surface_instance(preview_rect, preview.width, preview.height)
        });
    let preview_buffer = preview_instance.as_ref().and_then(|instance| {
        state
            .renderer
            .batch_renderer
            .create_instance_buffer(&state.renderer.gpu, &[*instance])
    });
    let mut marker_instances = Vec::new();
    let mut marker_label_instances = Vec::new();
    if draw_start_marker_overlays {
        let (positions, scale) = (&projected_start_positions, layout.scale);
        push_start_marker_sprites(&mut marker_instances, atlas, positions, scale, 0.00056);
        push_start_marker_labels(&mut marker_label_instances, state, positions, scale, 0.00040);
    }
    let marker_buffer = state
        .renderer
        .batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &marker_instances);

    let parent_showing =
        !message_box_up && choose_map_layout.is_none() && setups_layout.is_none() && !leaving;
    // `0x102`'s widgets, synced to the session, through the shared painter;
    // an open list goes over every dialog text.
    let page_root = parent_showing
        .then(|| crate::app::App::sync_skirmish_page(state))
        .flatten();
    let page_paint = page_root.map_or_else(Default::default, |root| {
        let gui = state.frontend.shell_controller.gui();
        // While a slide runs the column's art covers the buttons.
        let buttons = match sliding {
            true => gui_paint::ButtonArt::Host,
            false => gui_paint::ButtonArt::Panel,
        };
        // The name box's caret holds steady while it has the focus.
        let caret_of = |node| gui.focus() == Some(node);
        // VERA's Quick Team Setup and Bulk Seat Actions: a greyed item or
        // heading in the disabled colour (packed `0x0000009F`).
        let labels = skirmish_labels(state);
        let menu_rows = |id: i32| {
            let rows = crate::ui::skirmish_shell::page::menu_rows(
                &state.frontend.skirmish_shell_state,
                state.frontend.scenario_catalog.shell_maps(),
                &labels,
                u16::try_from(id).ok()?,
            )?;
            let packed = |pickable| if pickable { u32::MAX } else { 0x0000_009F };
            Some(rows.into_iter().map(|(text, pickable)| (text, packed(pickable))).collect())
        };
        let style = gui_paint::GuiStyle {
            buttons,
            caret_of: Some(&caret_of),
            rows_of: Some(&menu_rows),
            skirmish_lists: true,
            ..Default::default()
        };
        gui_paint::paint_gui_with(state, atlas, gui, root, layout.scale, &[], style)
    });
    // The chooser, `0x105`, or the saved-file browser over either page.
    let shell = &state.frontend.skirmish_shell_state;
    let dialog_paint = match (choose_map_layout.as_ref(), seed_layout.as_ref()) {
        _ if message_box_up => None,
        (Some(_), Some(seed_layout)) => shell.saved_seed_browser.as_ref().map(|browser| {
            let (key, fallback) = browser.mode.title_label();
            let title = state.csf_label(key, fallback);
            saved_games::seed_browser_paint(state, atlas, seed_layout, browser, &title)
        }),
        (Some(_), None) if leaving => None,
        (Some(choose_map_layout), None) => match setup_layout {
            Some(setup_layout) => text::random_map_paint(state, atlas, setup_layout, sliding),
            None => text::choose_map_paint(state, atlas, choose_map_layout, sliding),
        },
        (None, _) => setups_layout.as_ref().zip(shell.setups_browser.as_ref()).map(
            |(setups_layout, browser)| {
                let title = crate::i18n::tr("VERA:SetupsTitle");
                saved_games::seed_browser_paint(state, atlas, setups_layout, browser, &title)
            },
        ),
    };
    let validation_ok_pressed = state.frontend.shell_controller.top_id()
        == Some(crate::ui::shell::descriptor::DialogResourceId(0x00CE))
        && crate::ui::shell::menu_page::pressed_id(
            &state.frontend.shell_controller,
            crate::ui::shell::descriptor::DialogResourceId(0x00CE),
        ) == Some(crate::ui::shell::modal::control::OK);
    let validation_pressed = |id| validation_ok_pressed && id == crate::ui::shell::modal::control::OK;
    let validation_look = BoxLook::skirmish(atlas.mode_two_dialog_background(None));
    let mut instances = if message_box_up {
        let mut instances = Vec::new();
        push_message_box_backdrop_instances(&mut instances, atlas, &layout);
        if let Some(validation) = validation.as_ref() {
            let size = shell_box::render_size(state);
            let out = Some(&mut instances);
            shell_box::push_skirmish_box(state, Some(atlas), size, validation, validation_pressed, validation_look, out, None);
        }
        if let Some(prompt) = message_box {
            shell_box::push_choose_map_prompt(state, Some(atlas), &prompt, Some(&mut instances), None);
        }
        instances
    } else {
        build_skirmish_shell_instances(
            atlas,
            &state.renderer.bit_font,
            &layout,
            choose_map_layout.as_ref(),
            seed_layout.is_some(),
            setup_layout,
            setups_layout.is_some(),
            &state.frontend.skirmish_shell_state,
            &search_display,
            state.frontend.scenario_catalog.shell_maps(),
            wave.as_ref(),
            leaving,
            (
                &page_paint.controls,
                crate::ui::skirmish_shell::page::opponent_arrow(&state.frontend.shell_controller),
                crate::ui::skirmish_shell::random_map_page::scrollbar_view(&state.frontend.shell_controller),
            ),
        )
    };
    // Choose Map's, `0x105`'s or a browser's widgets over their art.
    let mut dialog_paint = dialog_paint.unwrap_or_default();
    instances.append(&mut dialog_paint.controls);
    if preview_instance.is_some() {
        push_solid_rect(
            &mut instances,
            atlas,
            preview_rect,
            [0.0, 0.0, 0.0],
            SHELL_PREVIEW_BACKDROP_DEPTH,
        );
    }
    // ComboDropWin is a popup over the dialog: its frame and rows go after
    // every dialog text, which this unbuffered pass paints in queue order.
    // VERA's Game Options box takes the same place; the two never show at
    // once, as each takes every press while it is open.
    let mut dropdown_instances = page_paint.popup;
    let mut dropdown_text_draws = page_paint.popup_texts;
    dropdown_instances.append(&mut dialog_paint.popup);
    dropdown_text_draws.append(&mut dialog_paint.popup_texts);
    if parent_showing {
        game_options::push_game_options_box(
            &mut dropdown_instances,
            &mut dropdown_text_draws,
            state,
            atlas,
            &crate::ui::skirmish_shell::game_options::GameOptionsLayout::over(&layout),
        );
    }
    let parent_statics = if parent_showing {
        text::paint_skirmish_statics(state, &layout, sliding)
    } else {
        Vec::new()
    };
    let mut shell_draws = if parent_showing {
        build_shell_text_draws(
            state,
            &layout,
            &parent_statics,
            page_paint.texts,
        )
    } else {
        Vec::new()
    };
    shell_draws.append(&mut dialog_paint.texts);
    if let Some(prompt) = message_box {
        shell_box::push_choose_map_prompt(state, Some(atlas), &prompt, None, Some(&mut shell_draws));
    }
    if let Some(validation) = validation.as_ref() {
        let size = shell_box::render_size(state);
        let out = Some((&mut shell_draws, false));
        shell_box::push_skirmish_box(state, Some(atlas), size, validation, validation_pressed, validation_look, None, out);
    }
    let marker_label_buffer = state
        .renderer
        .batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &marker_label_instances);
    state.renderer.batch_renderer.update_camera(
        &state.renderer.gpu,
        state.render_width() as f32,
        state.render_height() as f32,
        0.0,
        0.0,
        1.0,
        crate::render::batch::DepthAxis::NONE,
    );

    let Some((buffer, count)) = state
        .renderer
        .batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &instances)
    else {
        clear_shell_target(state, encoder, target);
        return Ok(action);
    };
    let scissored_text_buffers: Vec<_> = shell_draws
        .iter()
        .map(|d| {
            state
                .renderer
                .batch_renderer
                .create_instance_buffer(&state.renderer.gpu, &d.instances)
        })
        .collect();
    let dropdown_buffer = state
        .renderer
        .batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &dropdown_instances);
    let dropdown_text_buffers: Vec<_> = dropdown_text_draws
        .iter()
        .map(|d| {
            state
                .renderer
                .batch_renderer
                .create_instance_buffer(&state.renderer.gpu, &d.instances)
        })
        .collect();
    let cursor_instances: Vec<SpriteInstance> =
        crate::app::frontend::shell_pass::software_cursor(state, SHELL_CURSOR_DEPTH)
            .map(|(_, instance)| instance)
            .into_iter()
            .collect();
    let cursor_buffer = state
        .renderer
        .batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &cursor_instances);
    let zoom_buffers = zoom.as_ref().map(|zoom| zoom.buffers(state));
    // Default-cursor frame-0 texture, borrowed for the duration of the pass.
    let cursor_texture = state
        .match_state
        .match_presentation
        .software_cursor
        .as_ref()
        .and_then(|cursor| cursor.get(crate::app::types::CursorId::Default))
        .and_then(|sequence| sequence.frames.first())
        .map(|frame| &frame.texture);

    // A game room's Choose Map is one of the online client's screens.
    let smooth_art = state.smooth_online_menus()
        && state.frontend.shell_route == crate::app::shell_route::ShellRoute::NetworkSetup;
    let mut pass = begin_shell_pass(encoder, target, "Skirmish Shell");
    state.renderer.batch_renderer.draw_chrome(
        &mut pass,
        &atlas.texture,
        &buffer,
        count,
        smooth_art,
    );
    if let (Some(preview), Some((buffer, count))) = (
        state.frontend.skirmish_preview_texture.as_ref(),
        preview_buffer.as_ref(),
    ) {
        state.renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            &preview.texture,
            buffer,
            *count,
        );
    }
    if let Some((buffer, count)) = marker_buffer.as_ref() {
        state.renderer.batch_renderer.draw_chrome(
            &mut pass,
            &atlas.texture,
            buffer,
            *count,
            smooth_art,
        );
    }
    if let Some((buffer, count)) = marker_label_buffer.as_ref() {
        state.renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            state.renderer.bit_font.atlas(),
            buffer,
            *count,
        );
    }
    draw_scissored_text(&mut pass, state, &shell_draws, &scissored_text_buffers);
    pass.set_scissor_rect(0, 0, state.render_width(), state.render_height());
    if let Some((buffer, count)) = dropdown_buffer.as_ref() {
        state.renderer.batch_renderer.draw_chrome(
            &mut pass,
            &atlas.texture,
            buffer,
            *count,
            smooth_art,
        );
    }
    draw_scissored_text(
        &mut pass,
        state,
        &dropdown_text_draws,
        &dropdown_text_buffers,
    );
    // Reset scissor to full render so any subsequent draws / passes aren't clipped.
    pass.set_scissor_rect(0, 0, state.render_width(), state.render_height());
    if let Some(buffers) = zoom_buffers.as_ref() {
        draw_zoom(&mut pass, state, atlas, buffers);
    }
    // Software cursor draws last, on top of all chrome/controls/modals.
    if let (Some((buffer, count)), Some(texture)) = (cursor_buffer.as_ref(), cursor_texture) {
        state
            .renderer
            .batch_renderer
            .draw_with_buffer_passthrough(&mut pass, texture, buffer, *count);
    }
    drop(pass);

    Ok(action)
}

/// Each draw under its own scissor; the caller resets the scissor after.
fn draw_scissored_text<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    state: &'a AppState,
    draws: &[ShellTextDraw],
    buffers: &'a [Option<(wgpu::Buffer, u32)>],
) {
    for (draw, buf) in draws.iter().zip(buffers) {
        let Some((buffer, count)) = buf.as_ref() else {
            continue;
        };
        pass.set_scissor_rect(
            draw.scissor.x,
            draw.scissor.y,
            draw.scissor.w,
            draw.scissor.h,
        );
        state.renderer.batch_renderer.draw_with_buffer_passthrough(
            pass,
            state.renderer.bit_font.atlas(),
            buffer,
            *count,
        );
    }
}

fn clear_shell_target(
    _state: &AppState,
    encoder: &mut wgpu::CommandEncoder,
    target: ShellRenderTarget<'_>,
) {
    begin_shell_pass(encoder, target, "Skirmish Shell Clear");
}

/// A pass over the shell target, its colour and depth cleared.
fn begin_shell_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    target: ShellRenderTarget<'_>,
    label: &str,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target.color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(crate::app::types::CLEAR_COLOR),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: target.depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn paint_map_dialog_statics(
    font: &BitFont,
    labels: &[crate::render::shell_paint::PaintLabel<'_>],
    scale: crate::ui::shell::geom::ShellScale,
) -> Vec<ShellTextDraw> {
    crate::render::shell_paint::paint_labels_at_depth_scaled(
        font,
        labels,
        SHELL_DROPDOWN_TEXT_DEPTH - 0.00008,
        scale.factor(),
    )
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::*;
    use crate::ui::skirmish_shell::compute_layout;

    #[test]
    fn map_dialog_statics_keep_page_scale_and_clipping() {
        use crate::ui::shell::static_reveal::{Kind1RevealWindow, StaticPaint};
        use crate::ui::shell::geom::fit_window;
        use crate::ui::skirmish_shell::{
            compute_choose_map_modal_layout, compute_random_map_setup_layout,
        };

        let font = crate::render::bit_font::tests::make_test_font(
            &[('字' as u16, 12), (u16::from(b'A'), 6)],
            4,
        );
        let draw = |width, height, random, sample: &str| {
            let (title, help, scale) = if random {
                let layout = fit_window(width, height, compute_random_map_setup_layout);
                (layout.title, layout.status_help, layout.scale)
            } else {
                let layout = fit_window(width, height, compute_choose_map_modal_layout);
                (layout.title, layout.status_help, layout.scale)
            };
            let shown = StaticPaint {
                text: sample,
                window: Kind1RevealWindow {
                    count: u32::MAX,
                    range: 0,
                },
            };
            let draws = paint_map_dialog_statics(
                &font,
                &[
                    text::static_label(shown, title, ShellAlign::PANEL_TITLE),
                    text::static_label(shown, help, ShellAlign::NONE),
                ],
                scale,
            );
            (draws, [title, help], scale.factor())
        };
        for (random, sample) in [true, false]
            .into_iter()
            .flat_map(|random| ["字".repeat(90), "A ".repeat(100)].map(|sample| (random, sample)))
        {
            let base = draw(800, 600, random, &sample).0;
            assert!(base.iter().all(|draw| !draw.instances.is_empty()));
            for (width, height) in [(800, 600), (1200, 900), (1600, 1200)] {
                let (fitted, rects, factor) = draw(width, height, random, &sample);
                for ((original, scaled), rect) in base.iter().zip(&fitted).zip(rects) {
                    assert_eq!(scaled.instances.len(), original.instances.len());
                    assert_eq!(
                        [
                            scaled.scissor.x,
                            scaled.scissor.y,
                            scaled.scissor.w,
                            scaled.scissor.h,
                        ],
                        [rect.x as u32, rect.y as u32, rect.w as u32, rect.h as u32],
                    );
                    for (a, b) in original.instances.iter().zip(&scaled.instances) {
                        assert_eq!(b.size, a.size.map(|v| v * factor));
                        assert_eq!(b.position, a.position.map(|v| v * factor));
                        assert_eq!(b.depth, a.depth);
                    }
                }
                let help = &fitted[1];
                assert!((help.instances[0].position[1] - rects[1].y as f32).abs() <= 0.5);
                assert!(help.scissor.y + help.scissor.h <= height);
            }
        }
    }

    fn test_entry(width: f32, height: f32) -> SkirmishShellChromeEntry {
        SkirmishShellChromeEntry {
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            pixel_size: [width, height],
        }
    }

    #[test]
    fn scrollbar_arrow_entry_uses_pressed_art_only_while_pressed() {
        let released = test_entry(20.0, 22.0);
        let pressed = test_entry(21.0, 23.0);

        assert_eq!(
            scrollbar_arrow_entry(Some(released), Some(pressed), false)
                .unwrap()
                .pixel_size,
            released.pixel_size
        );
        assert_eq!(
            scrollbar_arrow_entry(Some(released), Some(pressed), true)
                .unwrap()
                .pixel_size,
            pressed.pixel_size
        );
        assert_eq!(
            scrollbar_arrow_entry(Some(released), None, true)
                .unwrap()
                .pixel_size,
            released.pixel_size
        );
    }

    #[test]
    fn decode_preview_from_map_bytes_decodes_lazy_preview_pack() {
        let bytes = b"[Preview]\nSize=0,0,2,1\n[PreviewPack]\n1=CgAGABcBAgMEBQYRAAA=\n";
        let decoded = decode_preview_from_map_bytes(bytes, "packed.map").expect("decoded preview");

        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 1);
        assert_eq!(decoded.rgba, vec![1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    #[ignore = "requires the configured stock retail RA2/YR install"]
    fn official_pkt_backed_skirmish_map_preview_decodes_from_mix_and_fits_right_panel() {
        let config = crate::util::config::GameConfig::load().expect("game config");
        let mut assets = crate::assets::asset_manager::AssetManager::new(
            &config.paths.ra2_dir,
            crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        )
        .expect("assets");
        let records = crate::map::scenario_sources::list_skirmish_scenario_records_with_assets(
            &config.paths.ra2_dir,
            &mut assets,
            None,
        )
        .expect("scenario records");
        let record = records
            .iter()
            .find(|record| {
                matches!(
                    record.source,
                    crate::map::skirmish_scenarios::SkirmishScenarioSource::MissionsMdPkt
                ) && record.preview.has_packed_preview
            })
            .expect("official PKT-backed map with PreviewPack");
        let entry = record.to_map_menu_entry();

        let decoded = decode_preview_for_map_entry(
            &entry,
            Some(config.paths.ra2_dir.as_path()),
            Some(&assets),
        )
        .expect("official preview decoded");
        let preview_rect = compute_layout(800, 600).map_preview;
        let instance =
            build_preview_surface_instance(preview_rect, decoded.width, decoded.height).unwrap();

        assert_eq!(
            decoded.rgba.len(),
            (decoded.width * decoded.height * 4) as usize
        );
        assert!(instance.position[0] >= preview_rect.x as f32);
        assert!(instance.position[1] >= preview_rect.y as f32);
        assert!(
            instance.position[0] + instance.size[0] <= (preview_rect.x + preview_rect.w) as f32
        );
        assert!(
            instance.position[1] + instance.size[1] <= (preview_rect.y + preview_rect.h) as f32
        );
    }

    #[test]
    fn preview_backdrop_sits_behind_fitted_preview_surface() {
        const { assert!(SHELL_PREVIEW_BACKDROP_DEPTH > SHELL_PREVIEW_SURFACE_DEPTH) };
        const { assert!(SHELL_PREVIEW_BACKDROP_DEPTH < 0.00080) };
    }

    #[test]
    fn button_segments_tile_middle_and_keep_caps() {
        let rect = RectPx::new(644, 241, 156, 42);
        let segments = build_button_segments(rect, 7.0, 64.0, 10.0);
        let expected = [
            (ButtonPiece::Left, 644.0, 7.0),
            (ButtonPiece::Middle, 651.0, 64.0),
            (ButtonPiece::Middle, 715.0, 64.0),
            (ButtonPiece::Middle, 779.0, 18.0),
            (ButtonPiece::Right, 790.0, 10.0),
        ];

        assert_eq!(segments.len(), expected.len());
        for (segment, (piece, x, width)) in segments.iter().zip(expected) {
            assert_eq!(segment.piece, piece);
            assert_eq!(segment.x, x);
            assert_eq!(segment.width, width);
        }
    }

    #[test]
    fn final_middle_segment_clips_when_width_is_not_tile_multiple() {
        let rect = RectPx::new(0, 0, 162, 37);
        let segments = build_button_segments(rect, 8.0, 60.0, 8.0);
        let middle_segments: Vec<_> = segments
            .iter()
            .filter(|s| s.piece == ButtonPiece::Middle)
            .collect();
        assert!(middle_segments.len() > 1);
        let last_middle = middle_segments.last().unwrap();
        assert!(last_middle.width < 60.0);
        assert!(last_middle.uv_width_ratio < 1.0);
    }

    #[test]
    fn button_segment_sprites_keep_native_art_height() {
        let entry = SkirmishShellChromeEntry {
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            pixel_size: [8.0, 30.0],
        };
        let segment = ButtonSegment {
            piece: ButtonPiece::Left,
            x: 635.0,
            width: 8.0,
            uv_x_ratio: 0.0,
            uv_width_ratio: 1.0,
        };

        assert_eq!(button_segment_sprite_size(entry, segment), [8.0, 30.0]);
    }

    #[test]
    fn button_art_y_centers_native_height_and_offsets_pressed_enabled() {
        let rect = RectPx::new(644, 241, 156, 42);

        assert_eq!(button_art_y(rect, 30.0, false, false), 247.0);
        assert_eq!(button_art_y(rect, 30.0, true, false), 249.0);
        assert_eq!(button_art_y(rect, 30.0, true, true), 247.0);
    }

    #[test]
    fn button_middle_tile_uses_centered_source_phase_when_source_is_wider() {
        let rect = RectPx::new(10, 20, 40, 42);
        let segments = build_button_segments(rect, 7.0, 64.0, 10.0);
        let middle = segments
            .iter()
            .find(|segment| segment.piece == ButtonPiece::Middle)
            .unwrap();

        assert_eq!(middle.x, 17.0);
        assert_eq!(middle.width, 30.0);
        assert_f32_close(middle.uv_x_ratio, 17.0 / 64.0);
        assert_f32_close(middle.uv_width_ratio, 30.0 / 64.0);
    }

    #[test]
    fn pressed_buttons_select_down_skin_assets() {
        assert_eq!(
            button_piece_asset_names(false),
            ("bue_li30.pcx", "bue_mi30.pcx", "bue_ri30.pcx")
        );
        assert_eq!(
            button_piece_asset_names(true),
            ("bde_li30.pcx", "bde_mi30.pcx", "bde_ri30.pcx")
        );
    }

    #[test]
    fn right_panel_buttons_use_sdbtnanm_type1_frames() {
        assert_eq!(right_panel_button_sdbtnanm_frame_index(false, false), 2);
        assert_eq!(right_panel_button_sdbtnanm_frame_index(true, false), 4);
        assert_eq!(right_panel_button_sdbtnanm_frame_index(true, true), 2);
    }

    #[test]
    fn gsi_03_11_random_map_button_disabled_render_uses_idle_art_and_text() {
        assert_eq!(right_panel_button_sdbtnanm_frame_index(true, true), 2);
        assert_eq!(
            button_label_color_for_disabled(true),
            crate::render::shell_paint::SHELL_TEXT_RGB_DISABLED
        );
        assert_eq!(button_label_color_for_disabled(false), SHELL_LABEL_TEXT_RGB);
    }

    fn assert_f32_close(left: f32, right: f32) {
        assert!((left - right).abs() < 0.00001, "{left} != {right}");
    }

    #[test]
    fn flag_entry_draws_native_size_centered_when_smaller_than_rect() {
        let entry = SkirmishShellChromeEntry {
            uv_origin: [0.25, 0.5],
            uv_size: [0.5, 0.25],
            pixel_size: [20.0, 10.0],
        };
        let mut out = Vec::new();

        push_flag_entry_clipped_centered(
            &mut out,
            entry,
            RectPx::new(100, 50, 30, 16),
            crate::ui::shell::geom::RETAIL_SCALE,
            0.00057,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].position, [105.0, 53.0]);
        assert_eq!(out[0].size, [20.0, 10.0]);
        assert_eq!(out[0].uv_origin, entry.uv_origin);
        assert_eq!(out[0].uv_size, entry.uv_size);
    }

    #[test]
    fn a_stretched_page_grows_the_flag_art_with_its_cell() {
        let entry = SkirmishShellChromeEntry {
            uv_origin: [0.25, 0.5],
            uv_size: [0.5, 0.25],
            pixel_size: [20.0, 10.0],
        };
        let mut out = Vec::new();

        push_flag_entry_clipped_centered(
            &mut out,
            entry,
            RectPx::new(200, 100, 60, 32),
            crate::ui::shell::geom::shell_scale(1600, 1200),
            0.00057,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].size, [40.0, 20.0]);
        assert_eq!(out[0].position, [210.0, 106.0]);
        assert_eq!(out[0].uv_size, entry.uv_size);
    }

    #[test]
    fn flag_entry_centers_odd_delta_with_integer_truncation() {
        let entry = SkirmishShellChromeEntry {
            uv_origin: [0.25, 0.5],
            uv_size: [0.5, 0.25],
            pixel_size: [47.0, 14.0],
        };
        let mut out = Vec::new();

        push_flag_entry_clipped_centered(
            &mut out,
            entry,
            RectPx::new(100, 50, 48, 16),
            crate::ui::shell::geom::RETAIL_SCALE,
            0.00057,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].position, [100.0, 51.0]);
        assert_eq!(out[0].size, [47.0, 14.0]);
        assert_eq!(out[0].uv_origin, entry.uv_origin);
        assert_eq!(out[0].uv_size, entry.uv_size);
    }

    #[test]
    fn flag_entry_clips_uvs_without_fit_scaling_when_larger_than_rect() {
        let entry = SkirmishShellChromeEntry {
            uv_origin: [0.25, 0.5],
            uv_size: [0.5, 0.25],
            pixel_size: [20.0, 10.0],
        };
        let mut out = Vec::new();

        push_flag_entry_clipped_centered(
            &mut out,
            entry,
            RectPx::new(100, 50, 12, 6),
            crate::ui::shell::geom::RETAIL_SCALE,
            0.00057,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].position, [100.0, 50.0]);
        assert_eq!(out[0].size, [12.0, 6.0]);
        assert_eq!(out[0].uv_origin, entry.uv_origin);
        assert_f32_close(out[0].uv_size[0], 0.3);
        assert_f32_close(out[0].uv_size[1], 0.15);
    }

    #[test]
    fn sdbtm_bottom_cap_clips_top_source_rows_without_scaling() {
        let entry = SkirmishShellChromeEntry {
            uv_origin: [0.0, 0.0],
            uv_size: [0.84, 0.65],
            pixel_size: [168.0, 65.0],
        };
        let mut out = Vec::new();

        push_entry_top_clipped(
            &mut out,
            entry,
            RectPx::new(632, 577, 168, 23),
            1.0,
            0.00078,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].position, [632.0, 577.0]);
        assert_eq!(out[0].size, [168.0, 23.0]);
        assert_eq!(out[0].uv_origin, entry.uv_origin);
        assert_f32_close(out[0].uv_size[0], 0.84);
        assert_f32_close(out[0].uv_size[1], 0.23);

        // A cap twice as tall keeps the same 23 source rows and stretches them
        // over the whole cap, instead of cropping to a shorter band.
        let mut stretched = Vec::new();
        push_entry_top_clipped(
            &mut stretched,
            entry,
            RectPx::new(632, 1154, 336, 46),
            2.0,
            0.00078,
        );

        assert_eq!(stretched.len(), 1);
        assert_eq!(stretched[0].size, [336.0, 46.0]);
        assert_f32_close(stretched[0].uv_size[0], 0.84);
        assert_f32_close(stretched[0].uv_size[1], 0.23);
    }

    #[test]
    fn side_item_data_maps_to_verified_flag_pcxs() {
        assert_eq!(flag_pcx_for_side_item_data(-3), Some("obsi.pcx"));
        assert_eq!(flag_pcx_for_side_item_data(-2), Some("rani.pcx"));
        assert_eq!(flag_pcx_for_side_item_data(0), Some("usai.pcx"));
        assert_eq!(flag_pcx_for_side_item_data(1), Some("japi.pcx"));
        assert_eq!(flag_pcx_for_side_item_data(2), Some("frai.pcx"));
        assert_eq!(flag_pcx_for_side_item_data(3), Some("geri.pcx"));
        assert_eq!(flag_pcx_for_side_item_data(4), Some("gbri.pcx"));
        assert_eq!(flag_pcx_for_side_item_data(5), Some("djbi.pcx"));
        assert_eq!(flag_pcx_for_side_item_data(6), Some("arbi.pcx"));
        assert_eq!(flag_pcx_for_side_item_data(7), Some("lati.pcx"));
        assert_eq!(flag_pcx_for_side_item_data(8), Some("rusi.pcx"));
        assert_eq!(flag_pcx_for_side_item_data(9), Some("yrii.pcx"));
        assert_eq!(flag_pcx_for_side_item_data(10), None);
    }

    #[test]
    fn country_flags_preserve_side_item_data_order() {
        assert_eq!(side_item_data_for_country(LaunchCountry::Korea), 1);
        assert_eq!(
            flag_name_for_country(LaunchCountry::Korea),
            Some("japi.pcx")
        );
        assert_eq!(side_item_data_for_country(LaunchCountry::GreatBritain), 4);
        assert_eq!(
            flag_name_for_country(LaunchCountry::GreatBritain),
            Some("gbri.pcx")
        );
        assert_eq!(side_item_data_for_country(LaunchCountry::Cuba), 7);
        assert_eq!(flag_name_for_country(LaunchCountry::Cuba), Some("lati.pcx"));
    }

    #[test]
    fn skirmish_background_uses_ra2_small_and_large_shell_variants() {
        assert_eq!(
            generic_background_role(&compute_layout(640, 480)),
            GenericBackgroundRole::Mnscrns640
        );
        assert_eq!(
            generic_background_role(&compute_layout(800, 600)),
            GenericBackgroundRole::MnscrnlLarge
        );
        assert_eq!(
            generic_background_role(&compute_layout(1024, 768)),
            GenericBackgroundRole::MnscrnlLarge
        );
    }

    #[test]
    fn lower_strip_role_uses_only_verified_widths() {
        assert_eq!(
            lower_strip_role(&compute_layout(640, 480)),
            LowerStripRole::Lwscrns640
        );
        assert_eq!(
            lower_strip_role(&compute_layout(800, 600)),
            LowerStripRole::LwscrnlLarge
        );
        assert_eq!(
            lower_strip_role(&compute_layout(1024, 768)),
            LowerStripRole::LwscrnlLarge
        );
    }

    #[test]
    fn lower_strip_rect_uses_native_asset_size_at_screen_bottom() {
        let small = SkirmishShellChromeEntry {
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            pixel_size: [472.0, 32.0],
        };
        let large = SkirmishShellChromeEntry {
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            pixel_size: [632.0, 32.0],
        };

        assert_eq!(
            lower_strip_rect(&compute_layout(640, 480), small),
            RectPx::new(0, 448, 472, 32)
        );
        assert_eq!(
            lower_strip_rect(&compute_layout(800, 600), large),
            RectPx::new(0, 568, 632, 32)
        );
        assert_eq!(
            lower_strip_rect(&compute_layout(1024, 768), large),
            RectPx::new(112, 652, 632, 32)
        );
    }

    #[test]
    fn sdmpbtn_rect_matches_verified_minimap_button_position() {
        let entry = test_entry(156.0, 84.0);

        assert_eq!(
            sdmpbtn_rect(&compute_layout(800, 600), entry),
            RectPx::new(644, 157, 156, 84)
        );
        assert_eq!(
            sdmpbtn_rect(&compute_layout(1024, 768), entry),
            RectPx::new(756, 241, 156, 84)
        );
    }

    #[test]
    fn semantic_draw_order_records_verified_right_panel_sequence() {
        let layout = compute_layout(800, 600);
        let order = skirmish_shell_semantic_draw_order(&layout, true, false, false, 0);
        assert_eq!(order[0], SkirmishShellDrawRole::RightPanelTopSdtp);
        assert_eq!(
            &order[1..10],
            [SkirmishShellDrawRole::RightPanelTileSdbtnbkgd; 9]
        );
        assert_eq!(
            &order[10..19],
            [SkirmishShellDrawRole::RightPanelOverlaySdbtnanmFrame10; 9]
        );
        assert_eq!(order[19], SkirmishShellDrawRole::RightPanelBottomSdbtm);
        assert_eq!(order[20], SkirmishShellDrawRole::LowerSideLwscrnl);
        assert_eq!(
            order[21],
            SkirmishShellDrawRole::ParentBackgroundMnscrnlLarge
        );
        assert_eq!(
            order[22],
            SkirmishShellDrawRole::RightPanelTopHighlightSdtpFrame1
        );
        assert_eq!(order[23], SkirmishShellDrawRole::RightPanelMapButtonSdmpbtn);
        assert_eq!(&order[24..27], [SkirmishShellDrawRole::OwnerDrawButton; 3]);
        assert!(!order.contains(&SkirmishShellDrawRole::StartMarker));
        assert!(!order.contains(&SkirmishShellDrawRole::StartMarkerLabel));
    }

    #[test]
    fn standard_offline_first_paint_skips_sdbtnanm_frame10_overlay() {
        let shell = SkirmishShellState::default();
        let layout = compute_layout(800, 600);
        let order = skirmish_shell_semantic_draw_order(
            &layout,
            right_panel_frame10_overlay_active(&shell),
            false,
            false,
            0,
        );

        assert!(!right_panel_frame10_overlay_active(&shell));
        assert!(!order.contains(&SkirmishShellDrawRole::RightPanelOverlaySdbtnanmFrame10));
        assert!(order.contains(&SkirmishShellDrawRole::RightPanelBottomSdbtm));
        assert!(order.contains(&SkirmishShellDrawRole::RightPanelTopHighlightSdtpFrame1));
        assert!(order.contains(&SkirmishShellDrawRole::RightPanelMapButtonSdmpbtn));
    }

    #[test]
    fn semantic_draw_order_keeps_ra2_large_background_and_lower_strip_at_1024() {
        let order =
            skirmish_shell_semantic_draw_order(&compute_layout(1024, 768), false, false, false, 0);
        assert_eq!(order[0], SkirmishShellDrawRole::RightPanelTopSdtp);
        assert!(order.contains(&SkirmishShellDrawRole::LowerSideLwscrnl));
        assert!(order.contains(&SkirmishShellDrawRole::RightPanelTopHighlightSdtpFrame1));
        assert!(order.contains(&SkirmishShellDrawRole::RightPanelMapButtonSdmpbtn));
        assert!(!order.contains(&SkirmishShellDrawRole::ParentBackgroundMnscrns640));
        assert!(order.contains(&SkirmishShellDrawRole::ParentBackgroundMnscrnlLarge));
    }

    #[test]
    fn preview_markers_require_real_preview_surface() {
        let order =
            skirmish_shell_semantic_draw_order(&compute_layout(800, 600), false, false, false, 0);
        assert!(!order.contains(&SkirmishShellDrawRole::PreviewSurface));
        assert!(!order.contains(&SkirmishShellDrawRole::StartMarker));
        assert!(!order.contains(&SkirmishShellDrawRole::StartMarkerLabel));

        let order =
            skirmish_shell_semantic_draw_order(&compute_layout(800, 600), false, true, true, 0);
        assert!(order.contains(&SkirmishShellDrawRole::PreviewSurface));
        assert!(order.contains(&SkirmishShellDrawRole::StartMarker));
        assert!(order.contains(&SkirmishShellDrawRole::StartMarkerLabel));
    }

    #[test]
    fn decoded_preview_surface_does_not_imply_start_marker_overlays() {
        let order =
            skirmish_shell_semantic_draw_order(&compute_layout(800, 600), false, true, false, 0);
        assert!(order.contains(&SkirmishShellDrawRole::PreviewSurface));
        assert!(!order.contains(&SkirmishShellDrawRole::StartMarker));
        assert!(!order.contains(&SkirmishShellDrawRole::StartMarkerLabel));
    }

    #[test]
    fn choose_map_modal_semantic_draw_order_replaces_parent_shell() {
        let layout = compute_layout(800, 600);
        let order = choose_map_modal_semantic_draw_order(&layout, true);

        assert_eq!(order[0], SkirmishShellDrawRole::RightPanelTopSdtp);
        assert_eq!(
            &order[1..10],
            [SkirmishShellDrawRole::RightPanelTileSdbtnbkgd; 9]
        );
        assert_eq!(order[10], SkirmishShellDrawRole::RightPanelBottomSdbtm);
        assert_eq!(order[11], SkirmishShellDrawRole::LowerSideLwscrnl);
        assert_eq!(
            order[12],
            SkirmishShellDrawRole::ChooseMapBackgroundCustomizeBattle800
        );
        assert!(!order.contains(&SkirmishShellDrawRole::ChooseMapModalBackdrop));
        assert_eq!(
            order[13],
            SkirmishShellDrawRole::RightPanelTopHighlightSdtpFrame1
        );
        assert!(!order.contains(&SkirmishShellDrawRole::RightPanelMapButtonSdmpbtn));
        assert!(!order.contains(&SkirmishShellDrawRole::RightPanelOverlaySdbtnanmFrame10));
        let first_listbox = order
            .iter()
            .position(|role| *role == SkirmishShellDrawRole::ChooseMapListbox)
            .expect("Choose Map listbox role");
        assert!(first_listbox > 13);
        assert_eq!(
            &order[first_listbox..first_listbox + 2],
            [SkirmishShellDrawRole::ChooseMapListbox; 2]
        );
        assert_eq!(
            &order[first_listbox + 2..first_listbox + 5],
            [SkirmishShellDrawRole::ChooseMapOwnerDrawButton; 3]
        );
        assert_eq!(
            order[first_listbox + 5],
            SkirmishShellDrawRole::ChooseMapPreviewStatic
        );
        assert!(!order.contains(&SkirmishShellDrawRole::ParentBackgroundMnscrnlLarge));
        assert!(!order.contains(&SkirmishShellDrawRole::OwnerDrawButton));

        let fallback = choose_map_modal_semantic_draw_order(&layout, false);
        assert_eq!(fallback[12], SkirmishShellDrawRole::ChooseMapModalBackdrop);
        assert!(!fallback.contains(&SkirmishShellDrawRole::ChooseMapBackgroundCustomizeBattle800));
    }

    #[test]
    fn random_map_setup_semantic_draw_order_uses_generic_shell_profile() {
        let layout = compute_layout(800, 600);
        let order = random_map_setup_semantic_draw_order(&layout, true);
        let background = order
            .iter()
            .position(|role| *role == SkirmishShellDrawRole::RandomMapBackgroundMnscrnlLarge)
            .expect("RMG MNSCRNL role");
        let highlight = order
            .iter()
            .position(|role| *role == SkirmishShellDrawRole::RightPanelTopHighlightSdtpFrame1)
            .expect("RMG SDTP frame-1 role");
        let first_control = order
            .iter()
            .position(|role| *role == SkirmishShellDrawRole::RandomMapOptionControl)
            .expect("RMG option-control role");

        assert_eq!(order[0], SkirmishShellDrawRole::RightPanelTopSdtp);
        assert_eq!(order[10], SkirmishShellDrawRole::RightPanelBottomSdbtm);
        assert_eq!(order[11], SkirmishShellDrawRole::LowerSideLwscrnl);
        assert!(background < highlight);
        assert!(highlight < first_control);
        assert!(!order.contains(&SkirmishShellDrawRole::RightPanelMapButtonSdmpbtn));
        assert!(!order.contains(&SkirmishShellDrawRole::RightPanelOverlaySdbtnanmFrame10));
        assert!(!order.contains(&SkirmishShellDrawRole::ChooseMapBackgroundCustomizeBattle800));

        let fallback = random_map_setup_semantic_draw_order(&layout, false);
        assert!(fallback.contains(&SkirmishShellDrawRole::RandomMapModalBackdrop));
        assert!(!fallback.contains(&SkirmishShellDrawRole::RandomMapBackgroundMnscrnlLarge));
    }

    #[test]
    fn validation_modal_semantic_draw_order_is_blocking_overlay() {
        let order = validation_modal_semantic_draw_order();

        assert_eq!(
            order,
            vec![
                SkirmishShellDrawRole::ValidationModal,
                SkirmishShellDrawRole::ValidationModalButton
            ]
        );
        assert!(!order.contains(&SkirmishShellDrawRole::OwnerDrawButton));
        assert!(!order.contains(&SkirmishShellDrawRole::ChooseMapOwnerDrawButton));
    }

    #[test]
    fn randmap_sentinel_detection_uses_file_name_case_insensitive() {
        assert!(is_random_map_sentinel_file_name("RandMap.Sed"));
        assert!(is_random_map_sentinel_file_name("RandMap.SED"));
        assert!(is_random_map_sentinel_file_name("Maps\\RandMap.SED"));
        assert!(!is_random_map_sentinel_file_name("RandMap.img"));
        assert!(!is_random_map_sentinel_file_name("OtherMap.map"));
    }

    #[test]
    fn a_random_preview_numbers_the_last_generates_starts() {
        let mut header = crate::map::preview::ScenarioStartStaging {
            width: 40,
            height: 20,
            ..Default::default()
        };
        assert_eq!(
            start_marker_bounds(Some(header), None),
            None,
            "no Generate yet: count 0 draws nothing"
        );
        header.count = 1;
        header.pairs[0] = (10, 5);
        let bounds = start_marker_bounds(Some(header), None).expect("one staged start");
        assert_eq!(bounds.start_points.len(), 1);
        assert_eq!(
            start_marker_bounds(None, Some(&bounds)),
            Some(bounds),
            "an ordinary map keeps its own header"
        );
    }

    #[test]
    fn skirmish_preview_aspect_fit_uses_gamemd_integer_per_mille_truncation() {
        let layout = compute_layout(800, 600);
        let fitted = aspect_fit_rect(layout.map_preview, 138, 75);
        assert_eq!(fitted, RectPx::new(645, 54, 143, 78));
    }

    #[test]
    fn header_preview_points_project_with_integer_per_mille_math() {
        let bounds = PreviewSourceBounds {
            origin_x: 10,
            origin_y: 20,
            width: 100,
            height: 80,
            start_points: vec![
                crate::map::preview::PreviewStartPoint { x: 10, y: 20 },
                crate::map::preview::PreviewStartPoint { x: 60, y: 60 },
                crate::map::preview::PreviewStartPoint { x: 109, y: 99 },
            ],
        };
        let projected = project_preview_start_positions(&bounds, RectPx::new(644, 54, 144, 78));
        assert_eq!(projected, vec![(644, 54), (716, 93), (786, 130)]);
    }

    #[test]
    fn start_marker_instances_apply_verified_shape_offsets() {
        assert_eq!(
            start_marker_top_left(120, 70, crate::ui::shell::geom::RETAIL_SCALE),
            (111, 64)
        );
    }

    #[test]
    fn start_marker_overlays_use_destination_surface_clip_not_preview_rect() {
        let bounds = PreviewSourceBounds {
            origin_x: 0,
            origin_y: 0,
            width: 100,
            height: 100,
            start_points: vec![crate::map::preview::PreviewStartPoint { x: 140, y: 50 }],
        };
        let preview = RectPx::new(645, 54, 143, 78);

        let projected = project_preview_start_positions(&bounds, preview);

        assert_eq!(projected, vec![(845, 93)]);
        assert!(!preview.contains(projected[0].0, projected[0].1));
        assert_eq!(
            start_marker_top_left(projected[0].0, projected[0].1, crate::ui::shell::geom::RETAIL_SCALE),
            (836, 87)
        );
        assert_eq!(
            start_marker_label_origin(projected[0].0, projected[0].1, crate::ui::shell::geom::RETAIL_SCALE),
            (843, 87)
        );
    }

    #[test]
    fn start_marker_labels_use_startbut_overlay_origin_and_yellow_color() {
        assert_eq!(
            start_marker_label_origin(120, 70, crate::ui::shell::geom::RETAIL_SCALE),
            (118, 64)
        );
    }

    #[test]
    fn skirmish_dropdown_selected_row_fill_is_full_row() {
        use crate::ui::skirmish_shell::page::{self, Pointer};
        let layout = compute_layout(800, 600);
        let mut shell = SkirmishShellState::default();
        let mut ui = crate::ui::shell::controller::DialogController::default();
        let face = layout.color_combos[0];
        let labels = Default::default();
        page::pointer(&mut ui, &mut shell, &layout, &[], &labels, Pointer::Down, face.x + face.w - 2, face.y + 2);
        let list = ui.gui().combo_drop_down(ui.gui().popup().unwrap()).unwrap();
        let content = list.popup.content();
        let selected = list.popup.row_rect(list.hot_row.unwrap()).unwrap();

        assert_eq!(
            selected,
            RectPx::new(
                content.x,
                content.y + COMBO_DROPDOWN_ROW_H,
                content.w,
                COMBO_DROPDOWN_ROW_H
            )
        );
    }

    #[test]
    fn skirmish_dropdown_side_text_uses_row_rect_and_separate_fit_width() {
        let content = RectPx::new(287, 84, 97, 161);

        assert_eq!(
            combo_dropdown_text_fit_width(content, crate::ui::shell::geom::RETAIL_SCALE),
            77
        );
    }

    #[test]
    fn skirmish_dropdown_ownerdraw_source_colors_are_decoded_bbggrr() {
        assert_f32_close(
            OWNERDRAW_BEVEL_LIGHT_RGB_FROM_PACKED_00C5BEA7[0],
            0xA7 as f32 / 255.0,
        );
        assert_f32_close(
            OWNERDRAW_BEVEL_LIGHT_RGB_FROM_PACKED_00C5BEA7[1],
            0xBE as f32 / 255.0,
        );
        assert_f32_close(
            OWNERDRAW_BEVEL_LIGHT_RGB_FROM_PACKED_00C5BEA7[2],
            0xC5 as f32 / 255.0,
        );
        assert_eq!(
            OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
            [1.0, 0.0, 0.0]
        );
    }

    #[test]
    fn button_text_rect_follows_owner_draw_caller_contract() {
        let rect = RectPx::new(644, 241, 156, 42);
        let retail = crate::ui::shell::geom::RETAIL_SCALE;
        let released = super::text::button_label_rect_px(rect, false, retail);
        let pressed = super::text::button_label_rect_px(rect, true, retail);

        assert_eq!(
            (released.x, released.y, released.w, released.h),
            (644, 242, 154, 41)
        );
        assert_eq!(
            (pressed.x, pressed.y, pressed.w, pressed.h),
            (646, 246, 152, 37)
        );
    }
}
mod abort;
pub(crate) use abort::render_abort_shell;
mod diplomacy;
pub(crate) use diplomacy::render_diplomacy_shell;
mod restate;
pub(crate) use restate::render_restate_shell;
mod connection;
pub(crate) use connection::{connection_shell_drawable, render_connection_shell};

mod list;
pub(crate) use list::{paint_list, paint_list_frame};

mod sound;
pub(crate) use sound::render_sound_shell;

pub(crate) use chrome::type3_button_frames;

mod keyboard;
mod lan_lobby;
mod shell_box;
pub(crate) use shell_box::{BoxLook, paint_shell_box};
mod wol_buddy;
mod wol_lobby;
mod wol_login;
mod wol_page;
mod wol_quick_match;
pub(crate) use lan_lobby::render_lan_lobby;
pub(crate) use wol_buddy::paint_wol_buddy_children;
pub(crate) use wol_lobby::{
    WolLobbyPaint, paint_wol_card_countries, paint_wol_lobby_children, paint_wol_lobby_popup,
    paint_wol_player_card,
};
pub(crate) use wol_login::{paint_wol_account_form, paint_wol_login_children};
pub(crate) use wol_page::{
    in_match_page_draw, paint_wol_add_buddy, paint_wol_page_dialog, paint_wol_page_received,
    paint_wol_verify,
};
pub(crate) use wol_quick_match::paint_wol_quick_match_children;
mod coop_score;
mod multiplayer_setup;
pub(crate) use coop_score::render_coop_score;
pub(crate) use keyboard::render_keyboard_shell;
pub(crate) use multiplayer_setup::render_multiplayer_setup;
