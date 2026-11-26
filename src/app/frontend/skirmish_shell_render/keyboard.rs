//! Keyboard A3: shared launcher/active frame and native control composition.
//!
//! Its heading, controls, statics, Back, open category list and in-game
//! status line paint from its `vera20k-gui` nodes ([`super::gui_paint`]),
//! made current first every frame ([`crate::app::input::keyboard::sync`]);
//! the front end's status line is the family static every page shares, from
//! the panel's own widgets
//! ([`crate::app::frontend::menu_page_render::paint_panel_statics`]); the
//! frame stays here.
#[cfg(test)]
use super::controls::{frame_segments, group_box_geometry, push_group_or_plain_frame};
use super::gui_paint::{ButtonArt, ComboArt, FrameArt, GuiStyle, HostRows, SideButtons};
use super::in_game_shell::{self, InGameShellFrame, ShellFrameArt, ShellFrameOverlay};
use crate::app::AppState;
use crate::app::frontend::menu_page_render::{PanelStatic, paint_panel_statics};
use crate::render::hud_paint::{HudPaint, Material};
use crate::render::shell_text_reveal::PathAReveal;
use crate::ui::shell::geom::{RETAIL_SCALE, RectPx, ShellScale};
use crate::ui::shell::in_game_shell::{FOOTER, TITLE};
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
    // VERA opens and closes the front-end page in place, like a page of its
    // Options window (`shell_transition::moves_in_place`, `exits_in_place`):
    // no slide reaches this draw.
    let title = if active {
        let dialog = state
            .frontend
            .keyboard_dialog
            .as_mut()
            .expect("active A3 state");
        // The heading's text is resolved as the page opens
        // (`input::keyboard::refresh_title`).
        dialog.title.start(now);
        dialog.title.paint(now)
    } else {
        // Heading and status line are the family statics.
        state.frontend.shell_page_title.paint(now)
    };
    let layout = crate::app::input::keyboard::sync(state).expect("active A3 geometry");
    let status_label = if active {
        None
    } else {
        let help = state
            .frontend
            .keyboard_dialog
            .as_ref()
            .and_then(|dialog| crate::app::input::dialog_page::help(&dialog.ui, None))
            .map(|key| state.csf_label(key, ""))
            .unwrap_or_default();
        crate::app::frontend::menu_page_render::paint_shell_status_line(
            &mut state.frontend.shell_status_line,
            help,
            layout.footer,
        )
    };
    let dialog = state
        .frontend
        .keyboard_dialog
        .as_ref()
        .expect("active A3 state");
    let atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("A3 control art");
    // In game Back is a node on the right panel's type-2 art.
    let mut side = None;
    let art = if active {
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
    let mut paint = HudPaint::default();
    // The painter's closures read the state while the frame is built.
    let shared: &AppState = state;
    let text_of = |id: i32| -> Option<String> {
        let id = u16::try_from(id).ok()?;
        let (key, fallback) = match id {
            // The heading shows once its reveal starts: then its node's own
            // `Text` (`input::keyboard::sync`), borrowed; blank before.
            TITLE => return title.is_none().then(String::new),
            // In game the status line shows the hovered control's help; the
            // front end's is the family static.
            FOOTER => {
                let key = crate::app::input::dialog_page::help(&dialog.ui, None)?;
                return active.then(|| shared.csf_label(key, ""));
            }
            _ => match KeyboardControl::from_id(id)? {
                KeyboardControl::Button(KeyboardButton::Back) => ("GUI:Back", "Back"),
                KeyboardControl::Button(KeyboardButton::Assign) => ("GUI:Assign", "Assign"),
                KeyboardControl::Button(KeyboardButton::ResetAll) => ("GUI:ResetAll", "Reset All"),
                KeyboardControl::Description => ("GUI:Description", "Description"),
                _ => return None,
            },
        };
        Some(shared.csf_label(key, fallback))
    };
    let reveal_of = |id: i32| -> Option<PathAReveal> {
        let window = title.filter(|_| id == i32::from(TITLE))?;
        Some(PathAReveal {
            count: window.count,
            range: window.range,
            base_rgb: [255, 255, 0],
            highlight_rgb: [255; 3],
        })
    };
    let category = i32::from(KeyboardControl::Category.id());
    // The open list's rows are the categories.
    let rows_of = |id: i32| -> Option<HostRows> {
        (id == category && dialog.category_open()).then(|| {
            let rows = dialog.categories.iter();
            rows.map(|row| (row.clone(), u32::MAX)).collect()
        })
    };
    // The front end's Back is its family's SDBTNANM art with the type-2
    // caption.
    let back = i32::from(KeyboardControl::Button(KeyboardButton::Back).id());
    let art_of = |id: i32| {
        (id == back).then_some(if active {
            ButtonArt::Side
        } else {
            ButtonArt::PanelType2
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
        rows_of: Some(&rows_of),
        frame_of: Some(&frame_of),
        reveal_of: Some(&reveal_of),
        combos: ComboArt::Beveled,
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
                &shared.platform.ime,
                &shared.renderer.bit_font,
                &shared.frontend,
                &shared.match_state,
                &shared.process_assets,
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
    // The draw order of the hand-made frame: the heading, the combo face,
    // the list, the group box, the capture field's frame (never grown with
    // the page), the buttons, the statics, then the status line.
    paint_ids(&[TITLE], layout.scale);
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
            FOOTER,
        ],
        layout.scale,
    );
    // The type-2 Back over the art; the open category list paints over
    // every control and text.
    let mut art = art;
    paint.drain_lane(Material::Side, &mut art);
    let popup = paint.take_lane(Material::Popup);
    let controls = paint.take_lane(Material::Controls);
    let (mut texts, popup_texts) = (paint.texts, paint.popup_texts);
    // The status line is a family static like the heading: it scales with the
    // page, as on every other family page.
    let status = status_label.map(PanelStatic::status);
    texts.extend(paint_panel_statics(
        &shared.platform.ime,
        &shared.renderer.bit_font,
        &shared.frontend,
        &shared.match_state,
        &shared.process_assets,
        [None, status],
        layout.scale,
    ));
    let overlay = ShellFrameOverlay {
        controls: popup,
        texts: popup_texts,
        ..Default::default()
    };
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
