//! Modal sprite helpers for the skirmish shell renderer.
//!
//! Covers Choose Map and validation modal instance construction.

use crate::render::batch::SpriteInstance;
use crate::render::shell_paint;
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::ui::shell::geom::LOWER_STRIP_H;
use crate::ui::shell::modal::BodyOkLayout;
use crate::ui::skirmish_shell::{
    COMBO_DROPDOWN_ROW_H, COMBO_FACE_H, ChooseMapModalButton, ChooseMapModalLayout,
    RandomMapSetupControl, RandomMapSetupLayout, RandomMapSetupModalState, RectPx,
    SETUP_COMBO_ROWS, SavedSeedBrowserState, SavedSeedControl, SavedSeedLayout,
    SkirmishShellLayout, SkirmishShellState, random_map_setup_dropdown_rect, setup_combo_items,
    trackbar_pixel_offset,
};

use super::chrome::{
    common_shell_origin, push_button_30, push_entry_native, push_lower_strip_instance,
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
/// Player-count trackbar bounds, matching the option normalizer's clamp.
const PLAYERS_MIN: i32 = 2;
const PLAYERS_MAX: i32 = 8;

const VALIDATION_MODAL_SPRITE_DEPTHS: shell_paint::ModalDepths = shell_paint::ModalDepths {
    background: SHELL_DROPDOWN_DEPTH - 0.00014,
    button: SHELL_DROPDOWN_DEPTH - 0.00016,
    text: 0.0,
};

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
    // (loaded only at exactly 800) elsewhere.
    match layout.screen.w {
        640 => atlas.choose_map_background_640_customize_battle,
        800 => atlas.choose_map_background_800_customize_battle,
        _ => None,
    }
}

pub(super) fn shell_content_fallback_rect(layout: &SkirmishShellLayout) -> RectPx {
    let (origin_x, origin_y) = common_shell_origin(layout);
    let shell_h = if layout.screen.h > 767 {
        600
    } else {
        layout.screen.h
    };
    RectPx::new(
        origin_x,
        origin_y,
        (layout.right_panel.top.x - origin_x).max(0),
        (shell_h - LOWER_STRIP_H).max(0),
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
        push_entry_native(
            out,
            background,
            modal_layout.screen.x,
            modal_layout.screen.y,
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
    );
    super::list::paint_list(
        out,
        atlas,
        modal.map_geometry(layout),
        modal.map_top_index,
        modal.highlighted_filtered_index,
        modal.map_scroll.pressed_part(),
        interior.paints_solid_fill(),
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
        atlas.validation_modal_background_pudlgbgn,
        super::chrome::type3_button_frames(atlas),
        boxed.dialog,
        &buttons,
        VALIDATION_MODAL_SPRITE_DEPTHS,
    ));
}

/// The generic shell background (MNSCRNS/MNSCRNL through SHELL.PAL): the
/// random-map dialog `0x105`'s and the empty backdrop's
/// (`0x0072AB49..0x0072AB7E`).
pub(super) fn generic_shell_background_entry(
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
) -> Option<SkirmishShellChromeEntry> {
    match generic_background_role(layout) {
        GenericBackgroundRole::Mnscrns640 => atlas.generic_background_640_mnscrns_shell,
        GenericBackgroundRole::MnscrnlLarge => atlas.generic_background_large_mnscrnl_shell,
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
        push_entry_native(out, background, x, y, SHELL_PARENT_BACKGROUND_DEPTH);
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
        let face = RectPx::new(rect.x, rect.y, rect.w, COMBO_FACE_H);
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
        );
    }
    let players = layout.control_rects[PLAYERS_ROW];
    paint_control(
        out,
        &chrome,
        ControlPaint::Trackbar {
            rect: players,
            thumb_px: trackbar_pixel_offset(
                modal.options.num_players,
                PLAYERS_MIN,
                PLAYERS_MAX,
                1,
                players,
            ),
        },
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
                RectPx::new(
                    list.x,
                    list.y + COMBO_DROPDOWN_ROW_H * selected as i32,
                    list.w,
                    COMBO_DROPDOWN_ROW_H,
                ),
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
    if atlas.validation_modal_background_pudlgbgn.is_none() {
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
        atlas.validation_modal_background_pudlgbgn,
        frames,
        layout.dialog,
        &[button],
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
    use crate::ui::shell::list::{ShellListGeometry,ListScrollPart};
    let pressed=match browser.pressed_control {
        Some(SavedSeedControl::ScrollUp)=>Some(ListScrollPart::Up),
        Some(SavedSeedControl::ScrollDown)=>Some(ListScrollPart::Down),
        Some(SavedSeedControl::ScrollThumb)=>Some(ListScrollPart::Thumb),
        Some(SavedSeedControl::ScrollTrack)=>Some(ListScrollPart::Track),
        _=>None,
    };
    super::list::paint_list(out,atlas,
        ShellListGeometry::new(layout.list,browser.entries.len(),browser.top_index),
        browser.top_index,browser.selected,pressed,interior.paints_solid_fill());
    // The name field is a plain sunken plate; Save is the only mode that has one.
    if let Some(edit) = layout.name_edit {
        push_solid_rect(
            out,
            atlas,
            edit,
            SHELL_MODAL_PANEL_RGB,
            SHELL_DROPDOWN_DEPTH - 0.00010,
        );
        push_ownerdraw_two_pixel_bevel_frame(out, atlas, edit, SHELL_DROPDOWN_DEPTH - 0.00011);
        if browser.description_edit.focused && browser.opened_at.elapsed().as_millis() % 2000 < 1000 {
            let field = crate::ui::skirmish_shell::player_name_edit_text_rect(edit);
            let editor = &browser.description_edit;
            let start = editor.first_visible_unit.min(editor.caret);
            let prefix = String::from_utf16_lossy(&editor.units[start..editor.caret]);
            let x = field.x + font.text_width(&prefix) as i32;
            if x < field.x + field.w {
                push_solid_rect(out, atlas, RectPx::new(x, field.y + 2, 2, (field.h - 4).max(1)),
                    SHELL_LABEL_TEXT_RGB, SHELL_DROPDOWN_DEPTH - 0.00012);
            }
        }

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
            rect: yes, pressed: browser.pressed_control == Some(SavedSeedControl::Action), enabled: true,
        }];
        if let Some(rect) = no {
            buttons.push(shell_paint::ModalButton {
                rect, pressed: browser.pressed_control == Some(SavedSeedControl::Back0x686), enabled: true,
            });
        }
        out.extend(shell_paint::paint_modal_sprites(
            atlas.validation_modal_background_pudlgbgn,
            super::chrome::type3_button_frames(atlas), dialog, &buttons, VALIDATION_MODAL_SPRITE_DEPTHS,
        ));
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_map_retail_art_preserves_listbox_backing() {
        let interior = backdrop_interior(true);

        assert_eq!(interior, BackdropInteriorPaint::PreserveBacking);
        assert!(!interior.paints_solid_fill());
    }

    #[test]
    fn choose_map_missing_art_uses_opaque_listbox_fallback() {
        let interior = backdrop_interior(false);

        assert_eq!(interior, BackdropInteriorPaint::OpaqueFallback);
        assert!(interior.paints_solid_fill());
    }

    #[test]
    fn choose_map_fallback_stops_before_800_rail_and_lower_strip() {
        let layout = crate::ui::skirmish_shell::compute_layout(800, 600);

        assert_eq!(
            shell_content_fallback_rect(&layout),
            RectPx::new(0, 0, 632, 568)
        );
    }

    #[test]
    fn choose_map_fallback_preserves_centered_1024_shell_geometry() {
        let layout = crate::ui::skirmish_shell::compute_layout(1024, 768);

        assert_eq!(
            shell_content_fallback_rect(&layout),
            RectPx::new(112, 84, 632, 568)
        );
    }

    #[test]
    fn random_map_retail_art_preserves_control_backing() {
        let interior = backdrop_interior(true);

        assert_eq!(interior, BackdropInteriorPaint::PreserveBacking);
        assert!(!interior.paints_solid_fill());
    }

    #[test]
    fn random_map_missing_art_uses_bounded_opaque_fallback() {
        let layout = crate::ui::skirmish_shell::compute_layout(800, 600);
        let interior = backdrop_interior(false);

        assert_eq!(interior, BackdropInteriorPaint::OpaqueFallback);
        assert!(interior.paints_solid_fill());
        assert_eq!(
            shell_content_fallback_rect(&layout),
            RectPx::new(0, 0, 632, 568)
        );
    }
}
