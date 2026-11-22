//! Render glue for the Movies & Credits children: movie list `0x129`, the
//! full-screen Play_Movie presentation and the Show_Credits roll.

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::menu_page_render::{
    FamilyFrame, encode_family_page, family_art_ready, page_panel_state, page_tree,
};
use crate::app::frontend::shell_pass::{ShellComposition, TexturedDraw, encode_shell_pass};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint::{PaintLabel, SHELL_TEXT_RGB_ENABLED};
use crate::render::shell_surface_present::SurfaceEffects;
use crate::render::shell_text::ShellTextDraw;
use crate::ui::main_menu_shell::RectPx;
use crate::ui::movies_credits_shell::{
    MOVIE_LIST_DIALOG, MOVIE_LIST_PAGE, MOVIE_LIST_PROMPT_KEY, MOVIE_LIST_TOOLTIP_KEY,
    find_family_list,
};
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::{ShellAlign, ShellScale};
use crate::ui::shell::menu_page;
use vera20k_gui::ListView;
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};

/// The family page `dialog`'s list as its `List` node shows it, with its
/// geometry in window `list`.
pub(crate) fn family_list_view(
    state: &AppState,
    dialog: crate::ui::shell::descriptor::DialogResourceId,
    list: RectPx,
    scale: ShellScale,
) -> Option<(ShellListGeometry, ListView)> {
    let controller = &state.frontend.shell_controller;
    let view = controller
        .gui()
        .list_view(find_family_list(controller, dialog)?)?;
    let rect = vera20k_gui::shell::list::list_box_rect(list);
    Some((view.geometry(rect, ListMetrics::scaled(scale)), view))
}

/// Whether the pointer is over the family page `dialog`'s list, its status
/// help's hover target. The page is the top dialog.
pub(crate) fn family_list_hovered(
    state: &AppState,
    dialog: crate::ui::shell::descriptor::DialogResourceId,
) -> bool {
    let controller = &state.frontend.shell_controller;
    controller.top_id() == Some(dialog)
        && find_family_list(controller, dialog)
            .is_some_and(|node| controller.gui().hovered() == Some(node))
}

/// The family page `dialog`'s list in window `list` (`0x00619230` over the
/// parent background), its geometry and view; the rows' text is the host's.
pub(crate) fn paint_family_list(
    state: &AppState,
    dialog: DialogResourceId,
    list: RectPx,
    scale: ShellScale,
) -> Option<(Vec<SpriteInstance>, ShellListGeometry, ListView)> {
    let atlas = state.frontend.skirmish_shell_chrome.as_ref()?;
    let (geometry, view) = family_list_view(state, dialog, list, scale)?;
    let (top, selected, pressed) = (view.top, view.selected, view.pressed);
    let mut out = Vec::new();
    crate::app::frontend::skirmish_shell_render::paint_list(
        &mut out, atlas, geometry, top, selected, pressed, false, scale,
    );
    Some((out, geometry, view))
}

/// Hover help of dialog `0x129` for its status line `0x695`.
fn movie_list_status_key(state: &AppState) -> Option<&'static str> {
    let controller = &state.frontend.shell_controller;
    if controller.top_id() != Some(MOVIE_LIST_PAGE.dialog) {
        return None;
    }
    if family_list_hovered(state, MOVIE_LIST_DIALOG) {
        return Some(MOVIE_LIST_TOOLTIP_KEY);
    }
    MOVIE_LIST_PAGE
        .button(menu_page::hovered_id(controller, MOVIE_LIST_PAGE.dialog)?)
        .map(|button| button.tooltip_key)
}

/// Paint dialog `0x129`. Returns `false` when the shell chrome is missing.
pub(crate) fn render_movie_list(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if !family_art_ready(state) {
        return Ok(false);
    }
    let layout = crate::app::frontend::shell_layout::movie_list_layout(state);
    let (page, dialog) = (&layout.page, MOVIE_LIST_DIALOG);
    let rect_of = |id| page.button_rect(id);
    let tree = page_tree(state, dialog, || MOVIE_LIST_PAGE.nodes(), rect_of, &[]);
    let status = |state: &AppState| movie_list_status_key(state).map(str::to_owned);
    let kind = ShellSlideKind::MovieList;
    let panel = page_panel_state(state, kind, true, status, page.status_help);
    // The teardown slide covers every child with the dialog's own repaint
    // (`0x00622C4F`), and nothing repaints them before the dialog is gone:
    // the list box and the prompt stay blank.
    let leaving = panel.leaving;
    let title = MOVIE_LIST_PAGE.title_key;
    let mut frame =
        FamilyFrame::new("Movie List Shell", page, panel, title).on(state, dialog, &tree);
    frame.backdrop = true;
    if !leaving {
        let mut bar = None;
        if let Some(list) = state.frontend.movie_list.as_ref()
            && let Some((sprites, geometry, view)) =
                paint_family_list(state, dialog, layout.list, page.scale)
        {
            bar = geometry.scrollbar;
            frame.over.controls.extend(sprites);
            for (visible, movie) in list
                .iter()
                .skip(view.top)
                .take(geometry.visible_rows)
                .enumerate()
            {
                let row = geometry.row(visible);
                frame.labels.push(PaintLabel {
                    text: state.csf_text(movie.label_key).into(),
                    rect: RectPx::new(row.x + 2, row.y, row.w - 2, row.h),
                    align: ShellAlign::NONE,
                    rgb: SHELL_TEXT_RGB_ENABLED,
                    path_a_reveal: None,
                });
            }
        }
        frame.list_boxes.push((layout.list, bar));
        frame.labels.push(PaintLabel {
            text: state.csf_text(MOVIE_LIST_PROMPT_KEY).into(),
            rect: layout.prompt,
            align: ShellAlign::H_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    encode_family_page(state, encoder, destination, frame);
    Ok(true)
}

/// A cleared black frame: Play_Movie's closing clear (`arg1 == 1`) and the
/// end of Show_Credits, shown for the frame on which the presentation ends.
pub(crate) fn render_fullscreen_black(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) {
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Shell black",
        ShellComposition::default(),
    );
}

/// Paint the active full-screen movie on black, without a cursor.
pub(crate) fn render_fullscreen_movie(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<()> {
    let Some(movie) = state.frontend.fullscreen_movie.as_ref() else {
        return Ok(());
    };
    let instance = movie.instance(
        state.renderer.gpu.config.width as i32,
        state.renderer.gpu.config.height as i32,
    );
    let draws = [TexturedDraw {
        texture: movie.surface().batch_texture(),
        instances: vec![instance],
    }];
    let subtitle: Vec<_> = movie
        .subtitle_text(
            &state.renderer.bit_font,
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        )
        .into_iter()
        .collect();
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Play_Movie",
        ShellComposition {
            draws: &draws,
            text: &subtitle,
            ..ShellComposition::default()
        },
    );
    Ok(())
}

/// Paint the credits roll on black with the 16-bit band fade, no cursor.
pub(crate) fn render_credits_roll(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<()> {
    let screen_w = state.renderer.gpu.config.width;
    let screen_h = state.renderer.gpu.config.height;
    let viewport = super::credits_roll::CreditsViewport::new(screen_w, screen_h);
    let active = state.platform.window_active;
    let Some(session) = state.frontend.credits_roll.as_mut() else {
        return Ok(());
    };
    if active || session.last_drawn.is_empty() {
        session.last_drawn = session.roll.instances(&state.renderer.bit_font, viewport);
    }
    // Glyphs are clipped to the 520 px box (0x006211D0 sets the clip rect).
    let text = [ShellTextDraw {
        instances: session.last_drawn.clone(),
        scissor: viewport.scissor,
    }];
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Show_Credits",
        ShellComposition {
            text: &text,
            effects: Some(SurfaceEffects {
                fade_rows: viewport.fade_rows,
            }),
            ..ShellComposition::default()
        },
    );
    Ok(())
}

#[cfg(all(test, tests_app_frontend))]
mod tests {
    use super::*;
    use vera20k_gui::shell::list::list_box_geometry;

    #[test]
    fn list_rows_match_the_native_capture() {
        let list = RectPx::new(120, 128, 399, 304);
        // Selection fill measured at x 121..=518, y 129..=147.
        let state = crate::ui::movies_credits_shell::active_movie_entries(
            crate::ui::movies_credits_shell::MovieProgress::default(),
        );
        let geometry = |rows, top| list_box_geometry(list, rows, top, ListMetrics::RETAIL);
        let short = geometry(state.len(), 0);
        assert_eq!(short.row(0), RectPx::new(121, 129, 398, 19));
        assert_eq!(short.visible_rows, 15);
        assert!(short.scrollbar.is_none());
        // Retail list with all 17 movies (`list17-*.png`): bar columns
        // 499..=518, thumb rows 151..=352 at top 0 and 208..=409 at top 2.
        let rows = crate::ui::movies_credits_shell::active_movie_entries(
            crate::ui::movies_credits_shell::MovieProgress {
                soviet: 7,
                allied: 7,
            },
        );
        assert_eq!(rows.len(), 17);
        let full = geometry(rows.len(), 0);
        assert_eq!(full.scrollbar, Some(RectPx::new(499, 128, 20, 305)));
        assert_eq!(full.thumb, Some(RectPx::new(500, 151, 18, 202)));
        assert_eq!(full.row(0), RectPx::new(121, 129, 378, 19));
        assert_eq!(
            geometry(rows.len(), 2).thumb,
            Some(RectPx::new(500, 208, 18, 202))
        );
    }
}
