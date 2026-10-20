//! Modal sprite helpers for the skirmish shell renderer.
//!
//! Covers Choose Map and validation modal instance construction.

use crate::render::batch::SpriteInstance;
use crate::render::shell_paint;
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::ui::shell::geom::{LOWER_STRIP_H, ShellScale};
use crate::ui::shell::list::ListMetrics;
use crate::ui::shell::modal::BodyOkLayout;
use crate::ui::skirmish_shell::{
    ChooseMapModalButton, ChooseMapModalLayout, DropdownMetrics, PLAYERS_RANGE,
    RandomMapSetupControl, RandomMapSetupLayout, RandomMapSetupModalState, RectPx,
    SETUP_COMBO_ROWS, SavedSeedBrowserState, SavedSeedControl, SavedSeedLayout,
    SkirmishShellLayout, SkirmishShellState, combo_face_rect, random_map_setup_dropdown_rect,
    setup_combo_items,
};

use super::chrome::{
    common_shell_origin, push_button_30, push_entry_grown, push_lower_strip_instance,
    push_ownerdraw_two_pixel_bevel_frame, push_rect_outline, push_right_panel_base_instances,
    push_right_panel_button_shp, push_solid_rect,
};
use super::controls::{ControlPaint, paint_control};
use super::draw_order::{GenericBackgroundRole, generic_background_role};
use super::{
    OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
    OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
    SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE, SHELL_DROPDOWN_DEPTH,
    SHELL_LABEL_TEXT_RGB, SHELL_MODAL_BG_RGB, SHELL_MODAL_PANEL_RGB, SHELL_PARENT_BACKGROUND_DEPTH,
};

/// The five combo rows as hit-test controls, for the enabled/disabled lookup.
const SETUP_COMBO_CONTROLS: [RandomMapSetupControl; 5] = [
    RandomMapSetupControl::MapType0x405,
    RandomMapSetupControl::Time0x3ea,
    RandomMapSetupControl::Theater0x407,
    RandomMapSetupControl::Size0x406,
    RandomMapSetupControl::Resources0x408,
];
const PLAYERS_ROW: usize = 5;

const VALIDATION_MODAL_SPRITE_DEPTHS: shell_paint::ModalDepths = shell_paint::ModalDepths {
    background: SHELL_DROPDOWN_DEPTH - 0.00014,
    button: SHELL_DROPDOWN_DEPTH - 0.00016,
    text: 0.0,
};

/// Ordinary saved-load F0: 559D60 -> 623230, mode-2 paint 621E90. The original
/// copies its existing AlternateSurface backing, overlays the selected side's
/// PUDLGBG frame, and paints the plain left/top Static. It neither regenerates a
/// parent shell nor draws Cancel. Executed controls: saved_game_layout.json.
pub(crate) fn render_saved_game_loading(
    state: &mut crate::app::AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    use super::in_game_shell::{
        InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
    };
    use crate::ui::shell::modal::{body_ok_layout, build_saved_game_loading_descriptor, control};

    // An in-match load paints over the match frame at its source size and
    // zooms with it; a menu load paints over the menu at the window's size.
    let frame = (state.render_width(), state.render_height());
    let restored = state
        .renderer
        .shell_surface_presenter
        .restore_cursor_free_composition(
            &state.renderer.retail_screenshot_frame_cache,
            &state.renderer.gpu.device,
            encoder,
            frame,
            state.renderer.upscale_pass.as_ref(),
        );
    let art = match restored {
        Ok(true) => ShellFrameArt::Retained,
        Ok(false) => ShellFrameArt::Clear,
        Err(error) => {
            log::warn!("Saved-game progress has no compatible retained backing: {error:#}");
            ShellFrameArt::Clear
        }
    };
    let atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("loading shell chrome");
    // 559D60 shows F0 before 69BB40 clears Session+30D8. An active load thus
    // starts with its current side; a menu load starts with the neutral assets.
    let side = (state.frontend.screen == crate::ui::game_screen::GameScreen::InGame)
        .then(|| crate::app::presentation::sidebar_render::current_sidebar_theme(state));
    let layout = body_ok_layout(frame.0 as i32, frame.1 as i32);
    let descriptor = build_saved_game_loading_descriptor();
    let body = descriptor
        .controls
        .iter()
        .find(|child| child.id == control::BODY_STATIC && child.visible)
        .expect("F0 visible body Static");
    let labels = [shell_paint::PaintLabel {
        text: super::text::localized_label(
            state,
            body.csf_key.expect("F0 loading caption"),
            "Mission is loading. Please wait...",
        )
        .into(),
        rect: layout.body,
        rgb: SHELL_LABEL_TEXT_RGB,
        align: super::text::validation_modal_body_text_align(),
        path_a_reveal: None,
    }];
    let modal = shell_paint::paint_modal_shp(
        &state.renderer.bit_font,
        atlas.mode_two_dialog_background(side),
        shell_paint::ModalButtonFrames::default(),
        layout.dialog,
        &[],
        &labels,
        ShellScale::RETAIL,
        VALIDATION_MODAL_SPRITE_DEPTHS,
    );
    super::in_game_shell::render_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art: Vec::new(),
            controls: modal.sprites,
            texts: modal.text,
            label: "Saved Game Loading F0",
        },
        art,
        ShellFrameOverlay::default(),
        ShellFrameCursor::Hidden,
    )
}

/// Native owner-draw listboxes preserve a composed backing surface. On the
/// exact-800 chooser that backing is the Customize Battle artwork; the solid
/// interior exists only to keep the asset-missing fallback readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BackdropInteriorPaint {
    PreserveBacking,
    OpaqueFallback,
}

impl BackdropInteriorPaint {
    const fn paints_solid_fill(self) -> bool {
        matches!(self, Self::OpaqueFallback)
    }
}

const fn backdrop_interior(retail_background_available: bool) -> BackdropInteriorPaint {
    if retail_background_available {
        BackdropInteriorPaint::PreserveBacking
    } else {
        BackdropInteriorPaint::OpaqueFallback
    }
}

pub(super) fn choose_map_background_entry(
    atlas: &SkirmishShellChromeAtlas,
    layout: &ChooseMapModalLayout,
) -> Option<SkirmishShellChromeEntry> {
    // `0x0072E730` draws the small shape at 640 wide and the large one
    // (loaded only at exactly 800) elsewhere. A stretched chooser is authored at
    // 800, so it takes the large shape instead of falling back to a flat rect.
    match layout.authored_width() {
        640 => atlas.choose_map_background_640_customize_battle,
        800 => atlas.choose_map_background_800_customize_battle,
        _ => None,
    }
}

pub(super) fn shell_content_fallback_rect(layout: &SkirmishShellLayout) -> RectPx {
    let (origin_x, origin_y) = layout.shell_origin();
    let strip_h = (LOWER_STRIP_H as f32 * layout.scale.factor()).round() as i32;
    RectPx::new(
        origin_x,
        origin_y,
        (layout.right_panel.top.x - origin_x).max(0),
        (layout.shell_height() - strip_h).max(0),
    )
}

pub(super) fn push_choose_map_background_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    shell_layout: &SkirmishShellLayout,
    modal_layout: &ChooseMapModalLayout,
) -> BackdropInteriorPaint {
    let background = choose_map_background_entry(atlas, modal_layout);
    let interior = backdrop_interior(background.is_some());
    if let Some(background) = background {
        let (x, y) = modal_layout
            .scale
            .dialog_origin_or((modal_layout.screen.x, modal_layout.screen.y));
        push_entry_grown(
            out,
            background,
            x,
            y,
            modal_layout.scale,
            SHELL_PARENT_BACKGROUND_DEPTH,
        );
    } else {
        let fallback = shell_content_fallback_rect(shell_layout);
        push_solid_rect(
            out,
            atlas,
            fallback,
            SHELL_MODAL_BG_RGB,
            SHELL_DROPDOWN_DEPTH - 0.00008,
        );
        push_rect_outline(
            out,
            atlas,
            fallback,
            OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
            SHELL_DROPDOWN_DEPTH - 0.00009,
        );
    }
    interior
}

pub(super) fn push_choose_map_modal_control_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &ChooseMapModalLayout,
    interior: BackdropInteriorPaint,
    shell: &SkirmishShellState,
    steady: bool,
) {
    let Some(modal) = shell.choose_map_modal.as_ref() else {
        return;
    };
    // Both lists are the shell list subclass (`0x00618D40`).
    super::list::paint_list(
        out,
        atlas,
        modal.mode_geometry(layout),
        modal.mode_top_index,
        modal.selected_mode_row(),
        modal.mode_scroll.pressed_part(),
        interior.paints_solid_fill(),
        layout.scale,
    );
    super::list::paint_list(
        out,
        atlas,
        modal.map_geometry(layout),
        modal.map_top_index,
        modal.highlighted_filtered_index,
        modal.map_scroll.pressed_part(),
        interior.paints_solid_fill(),
        layout.scale,
    );
    // While a slide runs the column replaces the buttons and the preview
    // static is validate-only (`0x00606800`).
    if !steady {
        return;
    }
    // The modal's right-column buttons are the same owner-draw type-1 class as the
    // setup shell's Start/Choose/Back: SDBTNANM frame 2 idle, frame 4 pressed. They
    // share the right-panel SDBTNANM cell geometry, so draw them through the same
    // path rather than the gray 3-slice PCX (push_button_30).
    for (button, id) in [
        (layout.use_map_button, ChooseMapModalButton::UseMap0x6c5),
        (layout.cancel_button, ChooseMapModalButton::Cancel0x5c0),
        (
            layout.create_random_map_button,
            ChooseMapModalButton::CreateRandomMap0x583,
        ),
    ] {
        push_right_panel_button_shp(
            out,
            atlas,
            button,
            modal.pressed_button == Some(id),
            !modal.button_enabled(id),
            SHELL_DROPDOWN_DEPTH - 0.00011,
        );
    }
    push_rect_outline(
        out,
        atlas,
        layout.preview,
        OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
        SHELL_DROPDOWN_DEPTH - 0.00012,
    );
}

/// The empty backdrop `ShellMessageBox__Run` draws before its box
/// (`0x005D3514` -> `Shell__DrawEmptyBackdrop` `0x0052FEC0` ->
/// `0x0072E820`): the generic shell background and `RightPanel__Draw(0)`
/// (`0x0072E450`: top cap, tiles with every slot shuttered by SDBTNANM frame
/// 10, bottom cap, lower strip). The dialog behind does not show.
pub(super) fn push_message_box_backdrop_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
) {
    push_generic_shell_background_instances(out, atlas, layout);
    push_right_panel_base_instances(out, atlas, layout, true);
    push_lower_strip_instance(out, atlas, layout);
}

/// Use Map's two-button eject box.
pub(super) fn push_eject_box_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    screen: RectPx,
    prompt: crate::ui::skirmish_shell::EjectPrompt,
) {
    let boxed = crate::ui::shell::modal::quit_confirm_layout(screen.w, screen.h);
    let pressed = |button| prompt.pressed == Some(button);
    let buttons = [
        shell_paint::ModalButton {
            rect: boxed.ok,
            pressed: pressed(crate::ui::skirmish_shell::EjectPromptButton::Ok),
            enabled: true,
        },
        shell_paint::ModalButton {
            rect: boxed.cancel,
            pressed: pressed(crate::ui::skirmish_shell::EjectPromptButton::Cancel),
            enabled: true,
        },
    ];
    out.extend(shell_paint::paint_modal_sprites(
        atlas.mode_two_dialog_background(None),
        super::chrome::type3_button_frames(atlas),
        boxed.dialog,
        &buttons,
        ShellScale::RETAIL,
        VALIDATION_MODAL_SPRITE_DEPTHS,
    ));
}

/// The generic shell background (MNSCRNS/MNSCRNL through SHELL.PAL): the
/// random-map dialog `0x105`'s and the empty backdrop's
/// (`0x0072AB49..0x0072AB7E`), also selected for the RA2 skirmish skin.
pub(super) fn generic_shell_background_entry(
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
) -> Option<SkirmishShellChromeEntry> {
    match generic_background_role(layout) {
        GenericBackgroundRole::Mnscrns640 => atlas.generic_background_640_mnscrns_shell,
        GenericBackgroundRole::MnscrnlLarge => atlas.generic_background_large_mnscrnl_shell,
    }
}

pub(super) fn offline_setup_background_entry(
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
) -> Option<SkirmishShellChromeEntry> {
    match generic_background_role(layout) {
        GenericBackgroundRole::Mnscrns640 => atlas.offline_setup_backgrounds[0],
        GenericBackgroundRole::MnscrnlLarge => atlas.offline_setup_backgrounds[1],
    }
}

pub(super) fn push_generic_shell_background_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
) -> BackdropInteriorPaint {
    let background = generic_shell_background_entry(atlas, layout);
    let interior = backdrop_interior(background.is_some());
    if let Some(background) = background {
        let (x, y) = common_shell_origin(layout);
        push_entry_grown(
            out,
            background,
            x,
            y,
            layout.scale,
            SHELL_PARENT_BACKGROUND_DEPTH,
        );
    } else {
        let fallback = shell_content_fallback_rect(layout);
        push_solid_rect(
            out,
            atlas,
            fallback,
            SHELL_MODAL_BG_RGB,
            SHELL_DROPDOWN_DEPTH - 0.00008,
        );
        push_rect_outline(
            out,
            atlas,
            fallback,
            OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
            SHELL_DROPDOWN_DEPTH - 0.00009,
        );
    }
    interior
}

/// Build child-control sprite instances for random-map dialog `0x105`.
pub(super) fn push_random_map_setup_modal_control_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &RandomMapSetupLayout,
    interior: BackdropInteriorPaint,
    modal: &RandomMapSetupModalState,
    // The right column and the top panel paint only while no slide runs.
    steady: bool,
) {
    let chrome = atlas.control_chrome();
    // Rows 0..4 are combos; row 5 is the players trackbar. A collapsed combo
    // occupies only its face, not the dropdown extent the resource reserves.
    for (row, control) in SETUP_COMBO_ROWS.iter().enumerate() {
        let rect = layout.control_rects[row];
        let face = combo_face_rect(rect, layout.scale);
        // Native owner-draw controls preserve their composed background. The
        // solid plate is only needed when retail shell artwork is unavailable.
        if interior.paints_solid_fill() {
            push_solid_rect(
                out,
                atlas,
                face,
                SHELL_MODAL_PANEL_RGB,
                SHELL_DROPDOWN_DEPTH - 0.00009,
            );
        }
        push_ownerdraw_two_pixel_bevel_frame(out, atlas, face, SHELL_DROPDOWN_DEPTH - 0.000095);
        paint_control(
            out,
            &chrome,
            ControlPaint::Combo {
                rect: face,
                swatch: None,
                open: modal.open_combo == Some(*control),
                disabled: !modal.is_enabled(SETUP_COMBO_CONTROLS[row]),
            },
            layout.scale,
        );
    }
    let players = layout.control_rects[PLAYERS_ROW];
    paint_control(
        out,
        &chrome,
        ControlPaint::Trackbar {
            rect: players,
            thumb_left: PLAYERS_RANGE.thumb_left_in(
                modal.options.num_players,
                players,
                layout.scale,
            ),
            plaque: true,
        },
        layout.scale,
    );

    // Surprise Me and Generate are owner-draw type 3 for this dialog, so they
    // take the MNBTTN modal button art rather than the generic PCX slices --
    // the same art the message-box modals use. Native size, centred on the
    // control rect.
    let modal_button_frames = super::chrome::type3_button_frames(atlas);
    let action_buttons: Vec<shell_paint::ModalButton> = [
        (layout.randomize, RandomMapSetupControl::Randomize0x621),
        (layout.generate, RandomMapSetupControl::Generate0x620),
    ]
    .into_iter()
    .map(|(rect, control)| shell_paint::ModalButton {
        rect,
        pressed: modal.pressed_control == Some(control),
        enabled: modal.is_enabled(control),
    })
    .collect();
    if modal_button_frames.up.is_some() {
        out.extend(shell_paint::paint_modal_sprites(
            None,
            modal_button_frames,
            layout.dialog,
            &action_buttons,
            layout.scale,
            shell_paint::ModalDepths {
                background: SHELL_DROPDOWN_DEPTH - 0.00011,
                button: SHELL_DROPDOWN_DEPTH - 0.00011,
                text: 0.0,
            },
        ));
    } else {
        // MNBTTN.SHP missing from the install: fall back to the generic slices
        // rather than drawing nothing where a button belongs.
        for button in &action_buttons {
            push_button_30(
                out,
                atlas,
                button.rect,
                button.pressed,
                !button.enabled,
                SHELL_DROPDOWN_DEPTH - 0.00011,
            );
        }
    }

    for (rect, control) in [
        (layout.use_map, RandomMapSetupControl::Ok0x6c5),
        (layout.load, RandomMapSetupControl::Load0x6c2),
        (layout.save, RandomMapSetupControl::Save0x6c3),
        (layout.delete, RandomMapSetupControl::Delete0x6c4),
        (layout.cancel, RandomMapSetupControl::Cancel0x5c0),
    ]
    .into_iter()
    .filter(|_| steady)
    {
        push_right_panel_button_shp(
            out,
            atlas,
            rect,
            modal.pressed_control == Some(control),
            !modal.is_enabled(control),
            SHELL_DROPDOWN_DEPTH - 0.00011,
        );
    }

    // The preview `0x468` is an SS_BLACKRECT static in the top panel: black,
    // with no frame of its own (retail `rmg-steady.png`).

    // The progress widgets are hidden in the resource and shown only while the
    // synchronous generate block owns the dialog.
    if modal.generating {
        push_solid_rect(
            out,
            atlas,
            layout.progress_bar,
            SHELL_MODAL_PANEL_RGB,
            SHELL_DROPDOWN_DEPTH - 0.00013,
        );
        push_rect_outline(
            out,
            atlas,
            layout.progress_bar,
            OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
            SHELL_DROPDOWN_DEPTH - 0.00014,
        );
    }

    // An open list paints last so it covers the rows beneath it.
    if let Some(combo) = modal.open_combo {
        let row = combo.row();
        let items = setup_combo_items(combo);
        let list = random_map_setup_dropdown_rect(layout, row, items.len());
        let row_h = DropdownMetrics::scaled(layout.scale).row_h;
        push_solid_rect(
            out,
            atlas,
            list,
            SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE,
            SHELL_DROPDOWN_DEPTH - 0.00015,
        );
        push_rect_outline(
            out,
            atlas,
            list,
            OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
            SHELL_DROPDOWN_DEPTH - 0.00016,
        );
        if let Some(selected) = modal.selected_item_index(combo) {
            push_solid_rect(
                out,
                atlas,
                RectPx::new(list.x, list.y + row_h * selected as i32, list.w, row_h),
                OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
                SHELL_DROPDOWN_DEPTH - 0.00017,
            );
        }
    }
}

pub(super) fn push_validation_modal_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &BodyOkLayout,
    pressed: bool,
) {
    let frames = super::chrome::type3_button_frames(atlas);
    let button = shell_paint::ModalButton {
        rect: layout.ok,
        pressed,
        enabled: true,
    };
    if atlas.mode_two_dialog_background(None).is_none() {
        push_solid_rect(
            out,
            atlas,
            layout.dialog,
            SHELL_MODAL_PANEL_RGB,
            VALIDATION_MODAL_SPRITE_DEPTHS.background,
        );
        push_rect_outline(
            out,
            atlas,
            layout.dialog,
            OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
            SHELL_DROPDOWN_DEPTH - 0.00015,
        );
    }
    out.extend(shell_paint::paint_modal_sprites(
        atlas.mode_two_dialog_background(None),
        frames,
        layout.dialog,
        &[button],
        ShellScale::RETAIL,
        VALIDATION_MODAL_SPRITE_DEPTHS,
    ));
}

/// Build the sprite instances for the saved-seed browser.
///
/// It replaces the setup dialog on screen, so it redraws the same background
/// and right-column chrome rather than layering over it.
pub(super) fn push_saved_seed_modal_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    font: &crate::render::bit_font::BitFont,
    layout: &SavedSeedLayout,
    browser: &SavedSeedBrowserState,
    interior: BackdropInteriorPaint,
) {
    push_saved_browser_modal_instances(out, atlas, font, layout, browser, interior, true);
}

/// Shared native list/editor/prompt painting; active-game callers supply
/// SIDEBTTN from the side atlas and therefore omit the launcher column buttons.
pub(super) fn push_saved_browser_modal_instances<I: Clone + PartialEq>(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    font: &crate::render::bit_font::BitFont,
    layout: &SavedSeedLayout,
    browser: &SavedSeedBrowserState<I>,
    interior: BackdropInteriorPaint,
    column_buttons: bool,
) {
    use crate::ui::shell::list::{ListScrollPart, ShellListGeometry};
    let pressed = match browser.pressed_control {
        Some(SavedSeedControl::ScrollUp) => Some(ListScrollPart::Up),
        Some(SavedSeedControl::ScrollDown) => Some(ListScrollPart::Down),
        Some(SavedSeedControl::ScrollThumb) => Some(ListScrollPart::Thumb),
        Some(SavedSeedControl::ScrollTrack) => Some(ListScrollPart::Track),
        _ => None,
    };
    super::list::paint_list(
        out,
        atlas,
        ShellListGeometry::new(
            layout.list,
            browser.entries.len(),
            browser.top_index,
            ListMetrics::scaled(layout.scale),
        ),
        browser.top_index,
        browser.selected,
        pressed,
        interior.paints_solid_fill(),
        layout.scale,
    );
    // The name field is a plain sunken plate; Save is the only mode that has one.
    if let Some(edit) = layout.name_edit {
        super::controls::push_new_edit_instances(
            out,
            atlas,
            font,
            &browser.description_edit,
            super::controls::NewEditPaint {
                frame: edit,
                text: crate::ui::skirmish_shell::player_name_edit_text_rect(edit, layout.scale),
                background: Some(SHELL_MODAL_PANEL_RGB),
                frame_depth: SHELL_DROPDOWN_DEPTH - 0.00011,
                caret_depth: SHELL_DROPDOWN_DEPTH - 0.00012,
                caret_visible: browser.opened_at.elapsed().as_millis() % 2000 < 1000,
            },
            layout.scale,
        );
    }
    if column_buttons {
        for (rect, control) in [
            (layout.action, SavedSeedControl::Action),
            (layout.back, SavedSeedControl::Back0x686),
        ] {
            let disabled = control == SavedSeedControl::Action && !browser.action_enabled();
            push_right_panel_button_shp(
                out,
                atlas,
                rect,
                browser.pressed_control == Some(control),
                disabled,
                SHELL_DROPDOWN_DEPTH - 0.00012,
            );
        }
    }
    push_saved_browser_prompt_instances(out, atlas, layout, browser);
}

/// The browser's message box (PUDLGBGN and its buttons), when one is up.
pub(super) fn push_saved_browser_prompt_instances<I: Clone + PartialEq>(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SavedSeedLayout,
    browser: &SavedSeedBrowserState<I>,
) {
    if let Some(prompt) = browser.prompt.as_ref() {
        let (dialog, _, yes, no) = prompt.layout(layout.screen.w as u32, layout.screen.h as u32);
        let mut buttons = vec![shell_paint::ModalButton {
            rect: yes,
            pressed: browser.pressed_control == Some(SavedSeedControl::Action),
            enabled: true,
        }];
        if let Some(rect) = no {
            buttons.push(shell_paint::ModalButton {
                rect,
                pressed: browser.pressed_control == Some(SavedSeedControl::Back0x686),
                enabled: true,
            });
        }
        out.extend(shell_paint::paint_modal_sprites(
            atlas.mode_two_dialog_background(None),
            super::chrome::type3_button_frames(atlas),
            dialog,
            &buttons,
            ShellScale::RETAIL,
            VALIDATION_MODAL_SPRITE_DEPTHS,
        ));
    }
}
