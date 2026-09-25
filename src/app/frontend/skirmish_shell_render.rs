//! Skirmish shell sprite construction and render pass.
//!
//! Part of the app layer: may depend on ui and render modules. Keeps the
//! `GameScreen::MainMenu` branch in the app facade small.

mod chrome;
mod controls;
// Retained as executable documentation of the native shell draw order; its
// items are exercised only by this file's tests since the F14 reexport
// narrowing.
#[cfg_attr(not(test), allow(dead_code))]
mod draw_order;
#[cfg(test)]
use draw_order::{
    SkirmishShellDrawRole, choose_map_modal_semantic_draw_order,
    random_map_setup_semantic_draw_order, skirmish_shell_semantic_draw_order,
    validation_modal_semantic_draw_order,
};
mod in_game_options;
mod in_game_shell;
pub(crate) use in_game_shell::{native_in_game_shell_active, current_in_game_shell_layout, render_in_game_options_shell};
mod pause_menu;
pub(crate) use pause_menu::render_pause_menu_shell;
mod saved_games;
pub(crate) use saved_games::{family_saved_game_overlays, render_saved_game_shell};
mod launcher_options;
pub(crate) use launcher_options::render_launcher_options;
mod modals;
mod preview;
mod text;

pub(crate) use preview::SkirmishPreviewTexture;
pub(crate) use text::skirmish_right_panel_label_strings;

use crate::app::AppState;
use crate::app::frontend::shell_transition::ShellFrameWave;
use crate::app::loading::init::MapMenuEntry;
#[cfg(test)]
use crate::map::preview::PreviewSourceBounds;
use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
#[cfg(test)]
use crate::render::shell_text::ShellAlign;
use crate::render::shell_transition_pass::ShellRenderTarget;
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::rules::color_scheme::ColorSchemeEntry;
use crate::skirmish_modes::SkirmishGameMode;
use crate::ui::main_menu::SkirmishCountry;
use crate::ui::shell::modal::{BodyOkLayout, body_ok_layout};
#[cfg(test)]
use crate::ui::skirmish_shell::{
    COMBO_DROPDOWN_ROW_H, SkirmishComboId, combo_dropdown_content_rect, player_name_edit_text_rect,
};
use crate::ui::skirmish_shell::{
    ChooseMapModalLayout, OwnerDrawButton, RectPx, SkirmishShellAction, SkirmishShellLayout,
    SkirmishShellState, compute_choose_map_modal_layout, compute_layout,
    compute_random_map_setup_layout, compute_saved_seed_layout, player_row_visible,
};

use self::chrome::*;
use self::controls::*;
use self::draw_order::ShellDialogChromeProfile;
#[cfg(test)]
use self::draw_order::{
    LowerStripRole, ParentBackgroundRole, lower_strip_role, parent_background_role,
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
// Owner-draw dark text color 0x00000C05 decoded as RGB; kept for regression
// tests that ensure Skirmish shell labels do not use this source accidentally.
#[cfg(test)]
const SHELL_BUTTON_TEXT_RGB_00000C05: [f32; 3] = [5.0 / 255.0, 12.0 / 255.0, 0.0];
const SHELL_LABEL_TEXT_RGB: [f32; 3] = [1.0, 1.0, 0.0];
const SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F: [f32; 3] = [0x9F as f32 / 255.0, 0.0, 0.0];
const RANDMAP_SENTINEL_FILE_NAME: &str = "RandMap.Sed";
const RANDMAP_PREVIEW_FILE_NAME: &str = "RandMap.img";
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellRenderMode {
    Visible,
    TransitionPreview,
}

#[cfg(test)]
fn shell_text_origin(
    rect: RectPx,
    text_w: u32,
    text_h: u32,
    flags: ShellAlign,
    y_offset: i32,
) -> (i32, i32) {
    let mut x = rect.x;
    if flags.contains(ShellAlign::H_CENTER) && text_w < rect.w as u32 {
        x += ((rect.w as u32 - text_w) / 2) as i32;
    } else if flags.contains(ShellAlign::H_RIGHT) && text_w < rect.w as u32 {
        x += (rect.w as u32 - text_w) as i32;
    }
    let mut y = rect.y + y_offset;
    if flags.contains(ShellAlign::V_CENTER) && text_h < rect.h as u32 {
        y += ((rect.h as u32 - text_h) / 2) as i32;
    }
    (x, y)
}

fn side_item_data_for_country(country: SkirmishCountry) -> i32 {
    match country {
        SkirmishCountry::America => 0,
        SkirmishCountry::Korea => 1,
        SkirmishCountry::France => 2,
        SkirmishCountry::Germany => 3,
        SkirmishCountry::GreatBritain => 4,
        SkirmishCountry::Libya => 5,
        SkirmishCountry::Iraq => 6,
        SkirmishCountry::Cuba => 7,
        SkirmishCountry::Russia => 8,
        SkirmishCountry::Yuri => 9,
    }
}

fn flag_pcx_for_side_item_data(item_data: i32) -> Option<&'static str> {
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

fn flag_name_for_country(country: SkirmishCountry) -> Option<&'static str> {
    flag_pcx_for_side_item_data(side_item_data_for_country(country))
}

fn flag_name_for_country_choice(random: bool, country: SkirmishCountry) -> Option<&'static str> {
    if random {
        flag_pcx_for_side_item_data(-2)
    } else {
        flag_name_for_country(country)
    }
}

fn flag_entry(atlas: &SkirmishShellChromeAtlas, label: &str) -> Option<SkirmishShellChromeEntry> {
    atlas
        .flags
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(label))
        .map(|(_, entry)| *entry)
}

fn build_skirmish_shell_instances(
    atlas: &SkirmishShellChromeAtlas,
    font: &BitFont,
    layout: &SkirmishShellLayout,
    choose_map_layout: Option<&ChooseMapModalLayout>,
    validation_layout: Option<&BodyOkLayout>,
    validation_ok_pressed: bool,
    shell: &SkirmishShellState,
    color_schemes: &[ColorSchemeEntry],
    maps: &[MapMenuEntry],
    modes: &[SkirmishGameMode],
    wave: Option<&ShellFrameWave>,
    leaving: bool,
) -> Vec<SpriteInstance> {
    let mut instances = Vec::new();

    if let Some(choose_map_layout) = choose_map_layout {
        // The setup dialog REPLACES the chooser rather than overlaying it: the
        // original hides/suspends the chooser while 0x105 is up. Drawing both
        // collides their listboxes, labels and button captions.
        if let Some(browser) = shell.saved_seed_browser.as_ref() {
            let seed_layout = compute_saved_seed_layout(
                browser.mode,
                choose_map_layout.screen.w as u32,
                choose_map_layout.screen.h as u32,
            );
            push_right_panel_base_instances(&mut instances, atlas, layout, false);
            push_lower_strip_instance(&mut instances, atlas, layout);
            let interior = push_choose_map_background_instances(&mut instances, atlas, layout, choose_map_layout);
            push_saved_seed_modal_instances(&mut instances, atlas, font, &seed_layout, browser, interior);
        } else if let Some(modal) = shell.random_map_setup_modal.as_ref() {
            let setup_layout = compute_random_map_setup_layout(
                choose_map_layout.screen.w as u32,
                choose_map_layout.screen.h as u32,
            );
            push_right_panel_base_instances(&mut instances, atlas, layout, false);
            push_lower_strip_instance(&mut instances, atlas, layout);
            let interior =
                push_random_map_setup_background_instances(&mut instances, atlas, layout);
            push_steady_optional_chrome_instances(
                &mut instances,
                atlas,
                layout,
                ShellDialogChromeProfile::RandomMapSetup0x105,
            );
            push_random_map_setup_modal_control_instances(
                &mut instances,
                atlas,
                &setup_layout,
                interior,
                modal,
            );
        } else {
            push_right_panel_base_instances(&mut instances, atlas, layout, false);
            push_lower_strip_instance(&mut instances, atlas, layout);
            let interior = push_choose_map_background_instances(
                &mut instances,
                atlas,
                layout,
                choose_map_layout,
            );
            push_steady_optional_chrome_instances(
                &mut instances,
                atlas,
                layout,
                ShellDialogChromeProfile::ChooseMap0x6b,
            );
            push_choose_map_modal_control_instances(
                &mut instances,
                atlas,
                choose_map_layout,
                interior,
                shell,
                modes,
            );
        }
        return instances;
    }

    push_right_panel_base_instances(
        &mut instances,
        atlas,
        layout,
        right_panel_frame10_overlay_active(shell),
    );

    // The teardown slide repaints the dialog over every child and runs to its
    // end without dispatching messages (`0x00622C4F`, `0x006071E0`): only the
    // dialog's own paint and the button frames show.
    if !leaving {
        push_player_name_edit_instances(&mut instances, atlas, font, layout, shell);
    }

    push_lower_strip_instance(&mut instances, atlas, layout);

    if let Some(background) = parent_background_entry(atlas, layout) {
        push_entry_native(
            &mut instances,
            background,
            layout.screen.x,
            layout.screen.y,
            SHELL_PARENT_BACKGROUND_DEPTH,
        );
    }

    const RIGHT_PANEL_BUTTON_DEPTH: f32 = 0.00059;
    match wave {
        // While either slide runs the engine draws the top-panel art (map
        // button, top panel, warning display) and the whole tile column in
        // place of the steady top, map button and buttons (`0x006071E0`).
        Some(wave) => {
            push_slide_panel_art(&mut instances, atlas, layout, &wave.panel_art());
            push_slide_column(
                &mut instances,
                atlas,
                layout,
                &wave.button_draws(),
                RIGHT_PANEL_BUTTON_DEPTH,
            );
        }
        None => {
            push_steady_optional_chrome_instances(
                &mut instances,
                atlas,
                layout,
                ShellDialogChromeProfile::SkirmishSetup0x102,
            );
            for (rect, button, disabled) in [
                (
                    layout.start_button,
                    OwnerDrawButton::StartGame0x617,
                    shell.validation_modal.is_some(),
                ),
                (
                    layout.choose_map_button,
                    OwnerDrawButton::ChooseMap0x5aa,
                    false,
                ),
                (layout.back_button, OwnerDrawButton::Back0x5c0, false),
            ] {
                push_right_panel_button_shp(
                    &mut instances,
                    atlas,
                    rect,
                    shell.pressed_owner_draw_button == Some(button),
                    disabled,
                    RIGHT_PANEL_BUTTON_DEPTH,
                );
            }
        }
    }
    if leaving {
        return instances;
    }

    push_combo_instances(&mut instances, atlas, color_schemes, layout, shell, maps);
    push_checkbox_instances(&mut instances, atlas, layout, shell);
    push_trackbar_instances(&mut instances, atlas, layout, shell);

    if let Some(flag) =
        flag_name_for_country_choice(shell.player_country_random, shell.player_country)
            .and_then(|name| flag_entry(atlas, name))
    {
        push_flag_entry_native_clipped_centered(&mut instances, flag, layout.flags[0], 0.00057);
    }
    for idx in 1..layout.flags.len() {
        if !player_row_visible(shell, maps, idx) {
            continue;
        }
        let entry = shell
            .opponents
            .get(idx - 1)
            .filter(|opponent| opponent.is_active())
            .and_then(|opponent| {
                flag_name_for_country_choice(opponent.country_random, opponent.country)
            })
            .and_then(|name| flag_entry(atlas, name));
        if let Some(flag) = entry {
            push_flag_entry_native_clipped_centered(
                &mut instances,
                flag,
                layout.flags[idx],
                0.00057,
            );
        }
    }

    push_dropdown_instances(&mut instances, atlas, color_schemes, layout, shell, maps);
    if shell.validation_modal.is_some() {
        if let Some(validation_layout) = validation_layout {
            push_validation_modal_instances(
                &mut instances,
                atlas,
                validation_layout,
                validation_ok_pressed,
            );
        }
    }
    instances
}

fn apply_owner_draw_button_paint_state(
    last_painted_pressed_button: &mut Option<OwnerDrawButton>,
    pressed: Option<OwnerDrawButton>,
    mode: ShellRenderMode,
) -> bool {
    if mode == ShellRenderMode::TransitionPreview {
        return false;
    }
    let play_sound = pressed.is_some() && last_painted_pressed_button.is_none();
    *last_painted_pressed_button = pressed;
    play_sound
}

fn update_owner_draw_button_paint_sound(state: &mut AppState, mode: ShellRenderMode) {
    let pressed = state.frontend.skirmish_shell_state.pressed_owner_draw_button;
    if apply_owner_draw_button_paint_state(
        &mut state.frontend.skirmish_shell_last_painted_pressed_button,
        pressed,
        mode,
    ) {
        crate::app::App::play_skirmish_shell_generic_click_sound(state);
    }
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
        ShellRenderMode::Visible,
    )?;
    state
        .renderer.shell_surface_presenter
        .encode_present(encoder, destination);
    Ok(action)
}


pub(crate) fn render_skirmish_shell_to_target(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    target: ShellRenderTarget<'_>,
    mode: ShellRenderMode,
) -> anyhow::Result<SkirmishShellAction> {
    let chrome = state.frontend.skirmish_shell_chrome.take();
    let result = render_skirmish_shell_with_atlas(state, encoder, target, chrome.as_ref(), mode);
    state.frontend.skirmish_shell_chrome = chrome;
    result
}

fn render_skirmish_shell_with_atlas(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    target: ShellRenderTarget<'_>,
    atlas: Option<&SkirmishShellChromeAtlas>,
    mode: ShellRenderMode,
) -> anyhow::Result<SkirmishShellAction> {
    // Collect whatever the generator has produced before laying anything out,
    // so each in-progress preview lands on the next frame drawn and the frame
    // that clears "Working / Please Wait" is the one showing the finished map.
    // While a job is in flight the shell keeps asking for frames -- nothing else
    // is driving redraws, and without this the dialog would freeze on the first
    // "working" frame until the player moved the mouse.
    if crate::app::App::poll_random_map_generation(state) {
        state.platform.window.request_redraw();
    } else if state.frontend.random_map_generation.is_some() {
        state.platform.window.request_redraw();
    }
    let layout = compute_layout(state.render_width(), state.render_height());
    let choose_map_layout = state
        .frontend.skirmish_shell_state
        .choose_map_modal
        .as_ref()
        .map(|_| compute_choose_map_modal_layout(state.render_width(), state.render_height()));
    let validation_layout = state
        .frontend.skirmish_shell_state
        .validation_modal
        .as_ref()
        .map(|_| body_ok_layout(state.render_width() as i32, state.render_height() as i32));
    let action = SkirmishShellAction::None;

    let Some(atlas) = atlas else {
        clear_shell_target(state, encoder, target);
        return Ok(action);
    };

    let exit_wave = crate::app::frontend::shell_transition::shell_exit_wave(
        state,
        crate::app::frontend::shell_transition::ShellSlideKind::Skirmish,
    )
    .cloned();
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| {
        (mode == ShellRenderMode::TransitionPreview)
            .then(|| state.frontend.shell_first_paint_slide.clone())
            .flatten()
    });
    // Every slide tick blits the top panel and the tile column from the
    // dialog's own surface (`0x00607EC8`, `0x00607EF1` with record `+0xD5`),
    // so no right-panel child shows while either slide runs.
    let sliding = wave.is_some();
    // No pressed-button click is painted while a slide draws the frames.
    let paint_mode = if leaving {
        ShellRenderMode::TransitionPreview
    } else {
        mode
    };
    update_owner_draw_button_paint_sound(state, paint_mode);
    ensure_selected_preview_texture(state);
    let selected_entry = state
        .frontend.scenario_catalog.shell_maps()
        .get(state.frontend.skirmish_shell_state.selected_map_idx);
    // Both the sentinel's RandMap.img and the setup dialog's generated image
    // already carry their start markers, so the overlay pass must not add them
    // a second time.
    let preview_has_baked_start_markers = selected_entry.is_some_and(is_random_map_sentinel_entry)
        || state.frontend.skirmish_shell_state.random_map_setup_modal.is_some();
    let selected_preview_bounds =
        selected_entry.and_then(|entry| entry.preview_source_bounds.as_ref());
    let setup_modal_open = state.frontend.skirmish_shell_state.random_map_setup_modal.is_some();
    // While the setup dialog is up it owns the preview box, so the image goes in
    // its own 0x468 rect rather than the chooser's.
    let setup_preview_rect = setup_modal_open.then(|| {
        compute_random_map_setup_layout(state.render_width(), state.render_height()).preview
    });
    let preview_rect = setup_preview_rect
        .or_else(|| choose_map_layout.as_ref().map(|layout| layout.preview))
        .unwrap_or(layout.map_preview);
    let fitted_preview_rect = state
        .frontend.skirmish_preview_texture
        .as_ref()
        .map(|preview| aspect_fit_rect(preview_rect, preview.width, preview.height));
    let projected_start_positions = selected_preview_bounds
        .zip(fitted_preview_rect)
        .map(|(bounds, rect)| project_preview_start_positions(bounds, rect))
        .unwrap_or_default();
    // The map preview static and its markers sit in the top panel.
    let draw_start_marker_overlays = !sliding
        && should_draw_start_marker_overlays(
            fitted_preview_rect,
            &projected_start_positions,
            preview_has_baked_start_markers,
        );
    let preview_instance = state
        .frontend
        .skirmish_preview_texture
        .as_ref()
        .filter(|_| !sliding)
        .and_then(|preview| {
            build_preview_surface_instance(preview_rect, preview.width, preview.height)
        });
    let preview_buffer = preview_instance.as_ref().and_then(|instance| {
        state
            .renderer.batch_renderer
            .create_instance_buffer(&state.renderer.gpu, &[*instance])
    });
    let marker_instances = if draw_start_marker_overlays {
        build_start_marker_instances(atlas, &projected_start_positions)
    } else {
        Vec::new()
    };
    let marker_buffer = state
        .renderer.batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &marker_instances);

    let color_schemes = state
        .rules()
        .map(|rules| rules.color_schemes.as_slice())
        .unwrap_or(&[]);
    let validation_ok_pressed = state.frontend.shell_controller.top_id()
        == Some(crate::ui::shell::descriptor::DialogId(0x00CE))
        && state.frontend.shell_controller.pressed() == Some(crate::ui::shell::modal::control::OK);
    let instances = build_skirmish_shell_instances(
        atlas,
        &state.renderer.bit_font,
        &layout,
        choose_map_layout.as_ref(),
        validation_layout.as_ref(),
        validation_ok_pressed,
        &state.frontend.skirmish_shell_state,
        color_schemes,
        state.frontend.scenario_catalog.shell_maps(),
        &state.frontend.skirmish_modes,
        wave.as_ref(),
        leaving,
    );
    let mut instances = instances;
    if preview_instance.is_some() {
        push_solid_rect(
            &mut instances,
            atlas,
            preview_rect,
            [0.0, 0.0, 0.0],
            SHELL_PREVIEW_BACKDROP_DEPTH,
        );
    }
    let (mut shell_draws, bare_text_instances) = if choose_map_layout.is_some() || leaving {
        (Vec::new(), Vec::new())
    } else {
        build_shell_text_draws(
            state,
            &layout,
            validation_layout.as_ref(),
            &state.frontend.skirmish_shell_state,
            state.frontend.scenario_catalog.shell_maps(),
            sliding,
        )
    };
    if let Some(choose_map_layout) = choose_map_layout.as_ref() {
        // Mirrors the sprite pass: the setup dialog replaces the chooser, so
        // only one of the two contributes text.
        if let Some(mode) = state
            .frontend.skirmish_shell_state
            .saved_seed_browser
            .as_ref()
            .map(|browser| browser.mode)
        {
            let seed_layout = compute_saved_seed_layout(
                mode,
                choose_map_layout.screen.w as u32,
                choose_map_layout.screen.h as u32,
            );
            push_saved_seed_modal_text_draws(&mut shell_draws, state, &seed_layout);
        } else if state.frontend.skirmish_shell_state.random_map_setup_modal.is_some() {
            let setup_layout = compute_random_map_setup_layout(
                choose_map_layout.screen.w as u32,
                choose_map_layout.screen.h as u32,
            );
            push_random_map_setup_modal_text_draws(&mut shell_draws, state, &setup_layout);
        } else {
            push_choose_map_modal_text_draws(&mut shell_draws, state, choose_map_layout);
        }
    }
    if let Some(validation_layout) = validation_layout.as_ref() {
        push_validation_modal_text_draws(
            &mut shell_draws,
            state,
            validation_layout,
            validation_ok_pressed,
        );
    }
    let marker_label_instances = if draw_start_marker_overlays {
        build_start_marker_label_instances(state, &projected_start_positions)
    } else {
        Vec::new()
    };
    let marker_label_buffer = state
        .renderer.batch_renderer
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
        .renderer.batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &instances)
    else {
        clear_shell_target(state, encoder, target);
        return Ok(action);
    };
    let bare_text_buffer = state
        .renderer.batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &bare_text_instances);
    let scissored_text_buffers: Vec<_> = shell_draws
        .iter()
        .map(|d| {
            state
                .renderer.batch_renderer
                .create_instance_buffer(&state.renderer.gpu, &d.instances)
        })
        .collect();
    let cursor_instances: Vec<SpriteInstance> =
        crate::app::frontend::shell_pass::software_cursor(state, SHELL_CURSOR_DEPTH)
            .map(|(_, instance)| instance)
            .into_iter()
            .collect();
    let cursor_buffer = state
        .renderer.batch_renderer
        .create_instance_buffer(&state.renderer.gpu, &cursor_instances);
    // Default-cursor frame-0 texture, borrowed for the duration of the pass.
    let cursor_texture = state
        .match_state.match_presentation.software_cursor
        .as_ref()
        .and_then(|cursor| cursor.get(crate::app::types::CursorId::Default))
        .and_then(|sequence| sequence.frames.first())
        .map(|frame| &frame.texture);

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Skirmish Shell"),
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
    });
    state
        .renderer.batch_renderer
        .draw_with_buffer_passthrough(&mut pass, &atlas.texture, &buffer, count);
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
        state.renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            &atlas.texture,
            buffer,
            *count,
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
    if let Some((buffer, count)) = bare_text_buffer.as_ref() {
        state.renderer.batch_renderer.draw_with_buffer_passthrough(
            &mut pass,
            state.renderer.bit_font.atlas(),
            buffer,
            *count,
        );
    }
    for (draw, buf) in shell_draws.iter().zip(scissored_text_buffers.iter()) {
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
            &mut pass,
            state.renderer.bit_font.atlas(),
            buffer,
            *count,
        );
    }
    // Reset scissor to full render so any subsequent draws / passes aren't clipped.
    pass.set_scissor_rect(0, 0, state.render_width(), state.render_height());
    // Software cursor draws last, on top of all chrome/controls/modals.
    if let (Some((buffer, count)), Some(texture)) = (cursor_buffer.as_ref(), cursor_texture) {
        state
            .renderer.batch_renderer
            .draw_with_buffer_passthrough(&mut pass, texture, buffer, *count);
    }
    drop(pass);

    Ok(action)
}

fn clear_shell_target(
    _state: &AppState,
    encoder: &mut wgpu::CommandEncoder,
    target: ShellRenderTarget<'_>,
) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Skirmish Shell Clear"),
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
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::skirmish_shell::{compute_fixed_800_layout, compute_layout};

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
    #[ignore]
    fn official_pkt_backed_skirmish_map_preview_decodes_from_mix_and_fits_right_panel() {
        let config = crate::util::config::GameConfig::load().expect("game config");
        let mut assets =
            crate::assets::asset_manager::AssetManager::new(&config.paths.ra2_dir).expect("assets");
        let records = crate::app::frontend::list_maps::list_skirmish_scenario_records_with_assets(
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
        assert!(SHELL_PREVIEW_BACKDROP_DEPTH > SHELL_PREVIEW_SURFACE_DEPTH);
        assert!(SHELL_PREVIEW_BACKDROP_DEPTH < 0.00080);
    }

    #[test]
    fn transition_preview_suppresses_owner_draw_paint_side_effects() {
        let mut last = None;
        let play_sound = apply_owner_draw_button_paint_state(
            &mut last,
            Some(OwnerDrawButton::StartGame0x617),
            ShellRenderMode::TransitionPreview,
        );

        assert!(!play_sound);
        assert_eq!(last, None);
    }

    #[test]
    fn visible_render_updates_owner_draw_paint_state_once() {
        let mut last = None;
        let play_sound = apply_owner_draw_button_paint_state(
            &mut last,
            Some(OwnerDrawButton::StartGame0x617),
            ShellRenderMode::Visible,
        );
        assert!(play_sound);
        assert_eq!(last, Some(OwnerDrawButton::StartGame0x617));

        let play_sound = apply_owner_draw_button_paint_state(
            &mut last,
            Some(OwnerDrawButton::StartGame0x617),
            ShellRenderMode::Visible,
        );
        assert!(!play_sound);
        assert_eq!(last, Some(OwnerDrawButton::StartGame0x617));
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
    fn gsi_03_11_random_map_button_disabled_render_uses_idle_art_alpha_and_text() {
        assert_eq!(right_panel_button_sdbtnanm_frame_index(true, true), 2);
        assert_f32_close(
            right_panel_button_sdbtnanm_alpha(true),
            BUTTON_DISABLED_ALPHA,
        );
        assert_f32_close(right_panel_button_sdbtnanm_alpha(false), 1.0);
        assert_eq!(
            button_label_color_for_disabled(true),
            SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F
        );
        assert_eq!(button_label_color_for_disabled(false), SHELL_LABEL_TEXT_RGB);
    }

    #[test]
    fn validation_modal_body_text_is_left_top_wrapped_not_centered() {
        assert_eq!(validation_modal_body_text_align(), ShellAlign::NONE);
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

        push_flag_entry_native_clipped_centered(
            &mut out,
            entry,
            RectPx::new(100, 50, 30, 16),
            0.00057,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].position, [105.0, 53.0]);
        assert_eq!(out[0].size, [20.0, 10.0]);
        assert_eq!(out[0].uv_origin, entry.uv_origin);
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

        push_flag_entry_native_clipped_centered(
            &mut out,
            entry,
            RectPx::new(100, 50, 48, 16),
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

        push_flag_entry_native_clipped_centered(
            &mut out,
            entry,
            RectPx::new(100, 50, 12, 6),
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

        push_entry_top_clipped_native(&mut out, entry, RectPx::new(632, 577, 168, 23), 0.00078);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].position, [632.0, 577.0]);
        assert_eq!(out[0].size, [168.0, 23.0]);
        assert_eq!(out[0].uv_origin, entry.uv_origin);
        assert_f32_close(out[0].uv_size[0], 0.84);
        assert_f32_close(out[0].uv_size[1], 0.23);
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
        assert_eq!(side_item_data_for_country(SkirmishCountry::Korea), 1);
        assert_eq!(
            flag_name_for_country(SkirmishCountry::Korea),
            Some("japi.pcx")
        );
        assert_eq!(side_item_data_for_country(SkirmishCountry::GreatBritain), 4);
        assert_eq!(
            flag_name_for_country(SkirmishCountry::GreatBritain),
            Some("gbri.pcx")
        );
        assert_eq!(side_item_data_for_country(SkirmishCountry::Cuba), 7);
        assert_eq!(
            flag_name_for_country(SkirmishCountry::Cuba),
            Some("lati.pcx")
        );
    }

    #[test]
    fn parent_background_role_uses_only_verified_widths() {
        assert_eq!(
            parent_background_role(&compute_layout(640, 480)),
            Some(ParentBackgroundRole::Mnscrns640)
        );
        assert_eq!(
            parent_background_role(&compute_layout(800, 600)),
            Some(ParentBackgroundRole::CoopGameSetup800)
        );
        assert_eq!(parent_background_role(&compute_layout(1024, 768)), None);
    }

    #[test]
    fn fixed_800_shell_uses_800_parent_background_when_centered() {
        let layout = compute_fixed_800_layout(1024, 768);

        assert_eq!(
            parent_background_role(&layout),
            Some(ParentBackgroundRole::CoopGameSetup800)
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
        assert_eq!(
            lower_strip_rect(&compute_fixed_800_layout(1280, 960), large),
            RectPx::new(240, 748, 632, 32)
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
            SkirmishShellDrawRole::ParentBackgroundCoopGameSetup800
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
    fn semantic_draw_order_keeps_1024_parent_blank_but_large_lower_strip() {
        let order =
            skirmish_shell_semantic_draw_order(&compute_layout(1024, 768), false, false, false, 0);
        assert_eq!(order[0], SkirmishShellDrawRole::RightPanelTopSdtp);
        assert!(order.contains(&SkirmishShellDrawRole::LowerSideLwscrnl));
        assert!(order.contains(&SkirmishShellDrawRole::RightPanelTopHighlightSdtpFrame1));
        assert!(order.contains(&SkirmishShellDrawRole::RightPanelMapButtonSdmpbtn));
        assert!(!order.contains(&SkirmishShellDrawRole::ParentBackgroundMnscrns640));
        assert!(!order.contains(&SkirmishShellDrawRole::ParentBackgroundCoopGameSetup800));
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
        assert!(!order.contains(&SkirmishShellDrawRole::ParentBackgroundCoopGameSetup800));
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
    fn parent_shell_status_text_is_blank_when_empty_and_present_when_set() {
        let mut shell = SkirmishShellState::default();
        assert_eq!(parent_shell_status_help_text(&shell), None);

        shell.status_help_text = "Start Game".to_string();
        assert_eq!(parent_shell_status_help_text(&shell), Some("Start Game"));
    }

    #[test]
    fn choose_map_modal_suppresses_parent_status_text_even_if_stale() {
        let mut shell = SkirmishShellState::default();
        shell.status_help_text = "Start Game".to_string();

        assert_eq!(choose_map_modal_parent_status_help_text(&shell), None);
        assert_eq!(
            choose_map_modal_status_help_text(&shell),
            Some("Start Game")
        );
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
    fn randmap_preview_suppresses_duplicate_start_marker_overlays() {
        let fitted = Some(RectPx::new(644, 54, 144, 78));
        let starts = [(700, 90)];

        assert!(should_draw_start_marker_overlays(fitted, &starts, false));
        assert!(!should_draw_start_marker_overlays(fitted, &starts, true));
        assert!(!should_draw_start_marker_overlays(fitted, &[], false));
        assert!(!should_draw_start_marker_overlays(None, &starts, false));
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
        assert_eq!(start_marker_top_left(120, 70), (111, 64));
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
            start_marker_top_left(projected[0].0, projected[0].1),
            (836, 87)
        );
        assert_eq!(
            start_marker_label_origin(projected[0].0, projected[0].1),
            (843, 87)
        );
    }

    #[test]
    fn start_marker_labels_use_startbut_overlay_origin_and_yellow_color() {
        assert_eq!(start_marker_label_origin(120, 70), (118, 64));
        assert_eq!(start_marker_label_color(), SHELL_LABEL_TEXT_RGB);
        assert_ne!(start_marker_label_color(), SHELL_BUTTON_TEXT_RGB_00000C05);
    }

    #[test]
    fn skirmish_dropdown_selected_row_fill_is_full_row() {
        let layout = compute_layout(800, 600);
        let shell = SkirmishShellState::default();
        let maps: [MapMenuEntry; 0] = [];
        let content =
            combo_dropdown_content_rect(&shell, &layout, &maps, SkirmishComboId::Color(0)).unwrap();

        let selected =
            dropdown_selected_row_rect(&shell, &maps, SkirmishComboId::Color(0), 0, content)
                .unwrap();

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

        let text = combo_dropdown_text_rect_for_current_renderer(content, 0);

        assert_eq!(text.x, content.x + COMBODROPWIN_TEXT_INSET_X);
        assert_eq!(text.y, content.y);
        assert_eq!(text.w, 94);
        assert_eq!(text.h, COMBO_DROPDOWN_ROW_H as u32);
        assert_eq!(combo_dropdown_text_fit_width(content), 77);
    }

    #[test]
    fn player_name_edit_text_rect_uses_verified_inset() {
        let layout = compute_layout(800, 600);
        let text = player_name_edit_text_rect(layout.player_name);

        assert_eq!(text, RectPx::new(61, 60, 147, 21));
    }

    #[test]
    fn player_name_caret_x_subtracts_horizontal_scroll() {
        let text = RectPx::new(61, 60, 147, 21);

        assert_eq!(player_name_caret_x_from_prefix_width(text, 0, 42), 103);
        assert_eq!(player_name_caret_x_from_prefix_width(text, 17, 42), 86);
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
    fn text_origin_centers_and_applies_pressed_offset() {
        let rect = RectPx::new(10, 20, 100, 30);
        assert_eq!(
            shell_text_origin(rect, 40, 10, ShellAlign::H_CENTER | ShellAlign::V_CENTER, 0),
            (40, 30)
        );
        assert_eq!(
            shell_text_origin(
                rect,
                40,
                10,
                ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                PRESSED_BUTTON_CONTENT_OFFSET_Y
            ),
            (40, 32)
        );
    }

    #[test]
    fn button_text_rect_follows_owner_draw_caller_contract() {
        let rect = RectPx::new(644, 241, 156, 42);
        let released = button_text_rect(rect, false);
        let pressed = button_text_rect(rect, true);

        assert_eq!(
            (released.x, released.y, released.w, released.h),
            (644, 242, 154, 41)
        );
        assert_eq!(
            (pressed.x, pressed.y, pressed.w, pressed.h),
            (646, 246, 152, 37)
        );
    }

    #[test]
    fn shell_text_colors_follow_verified_owner_draw_sources() {
        assert_eq!(
            SHELL_BUTTON_TEXT_RGB_00000C05,
            [5.0 / 255.0, 12.0 / 255.0, 0.0]
        );
        assert_eq!(SHELL_LABEL_TEXT_RGB, [1.0, 1.0, 0.0]);
        assert_eq!(
            SHELL_DISABLED_TEXT_RGB_FROM_PACKED_0000009F,
            [0x9F as f32 / 255.0, 0.0, 0.0]
        );
    }

    #[test]
    fn button_label_color_uses_owner_draw_button_yellow_source() {
        assert_eq!(button_label_color(), SHELL_LABEL_TEXT_RGB);
        assert_ne!(button_label_color(), SHELL_BUTTON_TEXT_RGB_00000C05);
    }

    #[test]
    fn text_origin_supports_left_and_right_alignment_flags() {
        let rect = RectPx::new(10, 20, 100, 30);
        assert_eq!(
            shell_text_origin(rect, 40, 10, ShellAlign::NONE, 0),
            (10, 20)
        );
        assert_eq!(
            shell_text_origin(rect, 40, 10, ShellAlign::H_RIGHT, 0),
            (70, 20)
        );
    }
}
mod abort;
pub(crate) use abort::render_abort_shell;

mod list;

mod sound;
pub(crate) use sound::render_sound_shell;

pub(crate) use chrome::type3_button_frames;

mod keyboard;
pub(crate) use keyboard::render_keyboard_shell;
