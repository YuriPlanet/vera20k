//! Keyboard A3: shared launcher/active frame and native control composition.
//!
//! Its controls and statics paint from its `vera20k-gui` nodes
//! ([`super::gui_paint`]), made current first every frame
//! ([`crate::app::input::keyboard::sync`]); the frame, the heading, the
//! front end's Back, the open category list and the status texts stay here.
use super::chrome::*;
#[cfg(test)]
use super::controls::{frame_segments, group_box_geometry, push_group_or_plain_frame};
use super::gui_paint::{ButtonArt, FrameArt, GuiPaint, GuiStyle, SideButtons};
use super::in_game_shell::{self, InGameShellFrame, ShellFrameArt, ShellFrameOverlay};
use super::text::rect_to_text_rect;
use super::{
    OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF, SHELL_CONTROL_DEPTH,
    SHELL_CONTROL_TEXT_DEPTH, SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE,
    SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::render::bit_font::BitFont;
use crate::render::shell_text_reveal::PathAReveal;
use crate::render::{
    shell_paint,
    shell_text::{self, ShellTextDraw},
};
use crate::ui::shell::geom::{RETAIL_SCALE, RectPx, ShellAlign, ShellScale};
use crate::ui::shell::keyboard::{
    CAPTURE_FRAME, CAPTURE_TEXT, DESCRIPTION_TEXT, KEYBOARD_PAGE, KeyboardButton, KeyboardControl,
    KeyboardParent, LABELS,
};
use crate::util::time::Instant;

pub(crate) fn render_keyboard_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    use crate::app::frontend::shell_transition::ShellSlideKind;
    let title_text = state.csf_label(KEYBOARD_PAGE.title_key, "Keyboard Options");
    let now = Instant::now();
    let active = state
        .frontend
        .keyboard_dialog
        .as_ref()
        .expect("active A3 state")
        .parent
        == KeyboardParent::GameControls;
    // The in-game A3 sits in the active-game shell's pixels.
    let shell = active
        .then(|| in_game_shell::current_in_game_shell_layout(state).expect("active A3 frame"));
    let (width, height) = shell.map_or(
        (state.render_width() as i32, state.render_height() as i32),
        |shell| (shell.width, shell.height),
    );
    // Retail slides the front-end page like its family: the teardown slide
    // repaints the dialog and pumps no messages until it ends, so every child
    // stays blank, and the entry slide suppresses the heading, status line
    // and Back (`0x00606800`) while the left-side controls paint. VERA opens
    // and closes the page in place, like a page of its Options window
    // (`shell_transition::moves_in_place`, `ShellExitThen::in_place`), so no
    // wave reaches this draw from Options; the handling stays with the
    // retail path it describes.
    let exit_wave = (!active)
        .then(|| {
            crate::app::frontend::shell_transition::shell_exit_wave(state, ShellSlideKind::Keyboard)
                .cloned()
        })
        .flatten();
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| {
        (!active)
            .then(|| state.frontend.shell_first_paint_slide.clone())
            .flatten()
    });
    let sliding = wave.is_some();
    let title = if active {
        let dialog = state
            .frontend
            .keyboard_dialog
            .as_mut()
            .expect("active A3 state");
        dialog.title.set_text(&title_text, now);
        dialog.title.start(now);
        dialog.title.paint(now)
    } else {
        // Heading and status line are the family statics: their reveals
        // start when the entry slide ends.
        (!sliding)
            .then(|| state.frontend.shell_page_title.paint(now))
            .flatten()
    };
    let status_label = if active || sliding {
        None
    } else {
        let help = state
            .frontend
            .keyboard_dialog
            .as_ref()
            .and_then(|dialog| crate::app::input::dialog_page::help(&dialog.ui))
            .map(|key| state.csf_label(key, ""))
            .unwrap_or_default();
        let window =
            crate::app::frontend::shell_layout::keyboard_layout(state, width, height, None).footer;
        crate::app::frontend::menu_page_render::paint_shell_status_line(state, help, window)
    };
    let layout = crate::app::input::keyboard::sync(state).expect("active A3 geometry");
    let dialog = state
        .frontend
        .keyboard_dialog
        .as_ref()
        .expect("active A3 state");
    let is_pressed = |button: KeyboardButton| {
        crate::app::input::dialog_page::pressed(&dialog.ui, KeyboardControl::Button(button).id())
    };
    let atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("A3 control art");
    // In game Back is a node on the right panel's type-2 art.
    let mut side = None;
    let mut art = if active {
        let sidebar = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
            .expect("active A3 side art");
        let shell = shell.expect("active A3 frame");
        side = Some(SideButtons {
            frames: sidebar.in_game_shell.buttons,
            theme: crate::app::presentation::sidebar_render::current_sidebar_theme(state),
            clip: RectPx::new(0, 0, width, height),
        });
        in_game_shell::background_instances(sidebar, shell.layout, width, height)
    } else {
        super::launcher_options::launcher_background_instances(
            atlas,
            width as u32,
            height as u32,
            layout.scale,
        )
    };
    if let Some(wave) = wave.as_ref() {
        push_slide_column(
            &mut art,
            atlas,
            &crate::ui::skirmish_shell::compute_layout_on(
                layout.scale,
                width as u32,
                height as u32,
            ),
            &wave.button_draws(),
            SHELL_CONTROL_DEPTH,
        );
    } else if !active {
        push_right_panel_button_shp(
            &mut art,
            atlas,
            layout.back,
            is_pressed(KeyboardButton::Back),
            false,
            SHELL_CONTROL_DEPTH,
        );
    }
    if leaving {
        in_game_shell::render_shell_frame(
            state,
            encoder,
            destination,
            InGameShellFrame {
                art,
                controls: Vec::new(),
                texts: Vec::new(),
                label: "Keyboard A3",
            },
            ShellFrameArt::Launcher,
            ShellFrameOverlay::default(),
            in_game_shell::ShellFrameCursor::Visible,
        )?;
        state.platform.window.request_redraw();
        return Ok(());
    }
    let font = &state.renderer.bit_font;
    let glyph_scale = layout.scale.factor();
    let text = |font: &BitFont,
                value: &str,
                rect: RectPx,
                align: ShellAlign,
                depth: f32|
     -> ShellTextDraw {
        shell_text::draw_in_rect_scaled(
            font,
            value,
            rect_to_text_rect(rect),
            SHELL_LABEL_TEXT_RGB,
            align,
            [0.0; 2],
            depth,
            glyph_scale,
        )
    };
    let mut paint = GuiPaint {
        side: art,
        ..Default::default()
    };
    if let Some(window) = title {
        paint.texts.push(shell_text::draw_in_rect_path_a_scaled(
            font,
            &title_text,
            rect_to_text_rect(layout.title),
            ShellAlign::PANEL_TITLE,
            [0.0; 2],
            SHELL_CONTROL_TEXT_DEPTH,
            PathAReveal {
                count: window.count,
                range: window.range,
                base_rgb: [255, 255, 0],
                highlight_rgb: [255; 3],
            },
            glyph_scale,
        ));
    }
    // The front end's Back is its family's SDBTNANM art above, captioned
    // here; the entry slide's column stands in for it.
    if !active && !sliding {
        let rect =
            super::pause_menu::button_text_rect(layout.back, is_pressed(KeyboardButton::Back));
        paint.texts.push(text(
            font,
            &state.csf_label("GUI:Back", "Back"),
            rect,
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }
    // The painter's closures read the state while the frame is built.
    let shared: &AppState = state;
    let text_of = |id: i32| -> Option<String> {
        let (key, fallback) = match KeyboardControl::from_id(u16::try_from(id).ok()?)? {
            KeyboardControl::Button(KeyboardButton::Back) => ("GUI:Back", "Back"),
            KeyboardControl::Button(KeyboardButton::Assign) => ("GUI:Assign", "Assign"),
            KeyboardControl::Button(KeyboardButton::ResetAll) => ("GUI:ResetAll", "Reset All"),
            KeyboardControl::Description => ("GUI:Description", "Description"),
            _ => return None,
        };
        Some(shared.csf_label(key, fallback))
    };
    let back = i32::from(KeyboardControl::Button(KeyboardButton::Back).id());
    let art_of = |id: i32| {
        (id == back).then_some(if active {
            ButtonArt::Side
        } else {
            ButtonArt::Host
        })
    };
    let description = i32::from(KeyboardControl::Description.id());
    let frame_of = |id: i32| match id {
        id if id == description => Some(FrameArt::Group),
        id if id == i32::from(CAPTURE_FRAME) => Some(FrameArt::Plain { outer_light: true }),
        _ => None,
    };
    let style = GuiStyle {
        buttons: ButtonArt::ModalType2,
        art_of: Some(&art_of),
        text_of: Some(&text_of),
        frame_of: Some(&frame_of),
        side,
        ..Default::default()
    };
    let root = dialog.ui.root_in(KEYBOARD_PAGE.dialog);
    let gui = dialog.ui.gui();
    let mut paint_ids = |ids: &[u16], scale: ShellScale| {
        let nodes = ids
            .iter()
            .filter_map(|id| gui.find_id(root?, i32::from(*id)));
        for node in nodes {
            super::gui_paint::paint_gui_into(
                &mut paint,
                shared,
                Some(atlas),
                gui,
                node,
                scale,
                &[],
                style,
            );
        }
    };
    let control = |control: KeyboardControl| control.id();
    let button = |button: KeyboardButton| control(KeyboardControl::Button(button));
    // The draw order of the hand-made frame: the combo face, the list, the
    // group box, the capture field's frame (never grown with the page), the
    // MNBTTN buttons, then the statics.
    paint_ids(
        &[
            control(KeyboardControl::Category),
            control(KeyboardControl::Commands),
            control(KeyboardControl::Description),
        ],
        layout.scale,
    );
    paint_ids(&[CAPTURE_FRAME], RETAIL_SCALE);
    paint_ids(
        &[
            button(KeyboardButton::Back),
            button(KeyboardButton::Assign),
            button(KeyboardButton::ResetAll),
        ],
        layout.scale,
    );
    paint_ids(&LABELS.map(|label| label.0), layout.scale);
    paint_ids(
        &[
            DESCRIPTION_TEXT,
            control(KeyboardControl::CurrentShortcut),
            control(KeyboardControl::CurrentOwner),
            CAPTURE_TEXT,
            control(KeyboardControl::Error),
        ],
        layout.scale,
    );
    // The open category list is this page's own (below); the painter's
    // lanes for it are dropped.
    let GuiPaint {
        side: art,
        controls,
        mut texts,
        ..
    } = paint;
    if let Some(key) = crate::app::input::dialog_page::help(&dialog.ui).filter(|_| active) {
        texts.push(text(
            font,
            &state.csf_label(key, ""),
            layout.footer,
            ShellAlign::NONE,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }
    // The status line is a family static like the heading: it scales with the
    // page, as `paint_labels_scaled` does for every other family page.
    texts.extend(shell_paint::paint_labels_scaled(
        font,
        &status_label.into_iter().collect::<Vec<_>>(),
        layout.scale.factor(),
    ));
    let mut overlay = ShellFrameOverlay::default();
    if dialog.category_open() {
        let popup = layout.category_popup(dialog.categories.len());
        push_solid_rect(
            &mut overlay.controls,
            atlas,
            popup,
            SHELL_DROPDOWN_BG_RGB_PENDING_COMBODROPWIN_SOURCE_CAPTURE,
            SHELL_DROPDOWN_DEPTH,
        );
        let row_h = vera20k_gui::shell::list::ListMetrics::combo(layout.scale).row_h;
        for (i, category) in dialog.categories.iter().enumerate() {
            let row = RectPx::new(
                popup.x + 2,
                popup.y + 2 + i as i32 * row_h,
                popup.w - 4,
                row_h,
            );
            if dialog.category_hot_row().or(Some(dialog.category)) == Some(i) {
                push_solid_rect(
                    &mut overlay.controls,
                    atlas,
                    row,
                    OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
                    SHELL_DROPDOWN_DEPTH - 0.00001,
                );
            }
            overlay.texts.push(text(
                font,
                category,
                RectPx::new(row.x + 3, row.y, row.w - 6, row.h),
                ShellAlign::V_CENTER,
                SHELL_DROPDOWN_TEXT_DEPTH,
            ));
        }
        push_ownerdraw_two_pixel_bevel_frame(
            &mut overlay.controls,
            atlas,
            popup,
            SHELL_DROPDOWN_DEPTH - 0.00002,
        );
    }
    in_game_shell::render_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls,
            texts,
            label: "Keyboard A3",
        },
        if active {
            ShellFrameArt::Sidebar
        } else {
            ShellFrameArt::Launcher
        },
        overlay,
        in_game_shell::ShellFrameCursor::Visible,
    )?;
    state.platform.window.request_redraw();
    Ok(())
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::*;

    #[test]
    fn native_group_caption_gap_and_bevel_polarity_are_preserved() {
        //61E700:207px group at95,159;17px font moves its top edge down8.
        let segments = frame_segments(RectPx::new(95, 167, 207, 146), Some(66), false);
        assert_eq!(
            segments[0],
            (RectPx::new(95, 167, 9, 1), [0x80, 0x7a, 0x68])
        );
        assert_eq!(
            segments[1],
            (RectPx::new(173, 167, 129, 1), [0x80, 0x7a, 0x68])
        );
        assert_eq!(
            segments[7],
            (RectPx::new(96, 168, 8, 1), [0xc5, 0xbe, 0xa7])
        );
        assert!(segments.contains(&(RectPx::new(301, 167, 1, 1), [0xa2, 0x9c, 0x87])));
    }

    #[test]
    fn stretched_group_caption_and_frame_keep_native_geometry() {
        use crate::render::skirmish_shell_chrome::SkirmishShellChromeEntry;

        let scale = crate::ui::shell::geom::shell_scale(1600, 1200);
        let group = scale.scale_rect(RectPx::new(95, 159, 207, 154));
        let (frame, caption) = group_box_geometry(group, 17, scale);
        assert_eq!(caption, RectPx::new(210, 318, 394, 34));
        assert_eq!(frame, RectPx::new(190, 334, 414, 292));
        let white = SkirmishShellChromeEntry {
            uv_origin: [0.0; 2],
            uv_size: [1.0; 2],
            pixel_size: [1.0; 2],
        };
        let mut sprites = Vec::new();
        push_group_or_plain_frame(&mut sprites, Some(white), frame, Some(66), false, scale);
        assert_eq!(sprites.len(), 14);
        assert_eq!(sprites[0].position, [190.0, 334.0]);
        assert_eq!(sprites[0].size, [18.0, 2.0]);
        assert_eq!(sprites[1].position, [346.0, 334.0]);
        assert_eq!(sprites[1].size, [258.0, 2.0]);
        assert_eq!(sprites[7].position, [192.0, 336.0]);
        assert_eq!(sprites[7].size, [16.0, 2.0]);
    }

    #[test]
    fn hotkey_frame_includes_native_extra_pixel_before_border_expansion() {
        //61ED86..88:186x23 window becomes187x24 before two outside rings.
        let segments = frame_segments(RectPx::new(94, 396, 191, 28), None, true);
        assert_eq!(
            segments[0],
            (RectPx::new(94, 396, 190, 1), [0xc5, 0xbe, 0xa7])
        );
        assert!(segments.contains(&(RectPx::new(284, 396, 1, 1), [0xa2, 0x9c, 0x87])));
        assert!(segments.contains(&(RectPx::new(94, 423, 1, 1), [0xa2, 0x9c, 0x87])));
    }
}
