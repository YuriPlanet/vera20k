//! Render glue for the Movies & Credits children: movie list `0x129`, the
//! full-screen Play_Movie presentation and the Show_Credits roll.

use anyhow::Result;

use crate::app::frontend::credits_roll::CreditsRollSession;
use crate::app::frontend::fullscreen_movie::FullscreenMovie;
use crate::app::frontend::menu_page_render::{
    FamilyFrame, encode_family_page, family_art_ready, page_panel_state, page_tree,
};
use crate::app::frontend::shell_pass::{ShellComposition, TexturedDraw, encode_shell_pass};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::skirmish_shell_render::gui_paint::{
    GuiStyle, paint_gui_ids_into, paint_gui_styled_into,
};
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::shell_surface_present::SurfaceEffects;
use crate::render::shell_text::ShellTextDraw;
use crate::ui::movies_credits_shell::{
    FAMILY_LIST_ID, MOVIE_LIST_DIALOG, MOVIE_LIST_PAGE, MOVIE_LIST_PROMPT, MOVIE_LIST_TOOLTIP_KEY,
    find_family_list, movie_list_nodes,
};
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::menu_page;
use vera20k_gui::ListView;
use vera20k_gui::shell::list::ShellListGeometry;

/// The family page `dialog`'s list as its `List` node shows it, with its
/// geometry on the node's window.
pub(crate) fn family_list_view(
    shell_controller: &DialogController,
    dialog: crate::ui::shell::descriptor::DialogResourceId,
) -> Option<(ShellListGeometry, ListView)> {
    let gui = shell_controller.gui();
    let node = find_family_list(shell_controller, dialog)?;
    Some((gui.list_geometry(node)?, gui.list_view(node)?))
}

/// Whether the pointer is over the family page `dialog`'s list, its status
/// help's hover target. The page is the top dialog.
pub(crate) fn family_list_hovered(
    shell_controller: &DialogController,
    dialog: crate::ui::shell::descriptor::DialogResourceId,
) -> bool {
    let controller = shell_controller;
    controller.top_id() == Some(dialog)
        && find_family_list(controller, dialog)
            .is_some_and(|node| controller.gui().hovered() == Some(node))
}

/// Hover help of dialog `0x129` for its status line `0x695`.
fn movie_list_status_key(shell_controller: &DialogController) -> Option<&'static str> {
    let controller = shell_controller;
    if controller.top_id() != Some(MOVIE_LIST_PAGE.dialog) {
        return None;
    }
    if family_list_hovered(shell_controller, MOVIE_LIST_DIALOG) {
        return Some(MOVIE_LIST_TOOLTIP_KEY);
    }
    MOVIE_LIST_PAGE
        .button(menu_page::hovered_id(controller, MOVIE_LIST_PAGE.dialog)?)
        .map(|button| button.tooltip_key)
}

/// Paint dialog `0x129`. Returns `false` when the shell chrome is missing.
pub(crate) fn render_movie_list(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if !family_art_ready(
        renderer,
        frontend,
        process_assets,
        &persistence.options_profile,
    ) {
        return Ok(false);
    }
    let layout = crate::app::frontend::shell_layout::movie_list_layout(&renderer.gpu);
    let (page, dialog) = (&layout.page, MOVIE_LIST_DIALOG);
    let rect_of = |id| layout.rect_of(id);
    let tree = page_tree(
        &mut frontend.shell_controller,
        dialog,
        movie_list_nodes,
        rect_of,
        &[],
    );
    let status = |frontend: &FrontendState| {
        movie_list_status_key(&frontend.shell_controller).map(str::to_owned)
    };
    let kind = ShellSlideKind::MovieList;
    let panel = page_panel_state(
        frontend,
        process_assets,
        kind,
        true,
        status,
        page.status_help,
    );
    // The teardown slide covers every child with the dialog's own repaint
    // (`0x00622C4F`), and nothing repaints them before the dialog is gone:
    // the list box and the prompt stay blank.
    let leaving = panel.leaving;
    let title = MOVIE_LIST_PAGE.title_key;
    let mut frame = FamilyFrame::new("Movie List Shell", page, panel, title).on(
        &frontend.shell_controller,
        dialog,
        &tree,
    );
    frame.backdrop = true;
    if !leaving {
        // The list over its widget, its rows the movies' captions: only the
        // rows it shows are named.
        let controller = &frontend.shell_controller;
        let gui = controller.gui();
        let mut bar = None;
        if let Some(list) = frontend.movie_list.as_ref()
            && let Some(node) = find_family_list(controller, dialog)
            && let Some(geometry) = gui.list_geometry(node)
        {
            bar = geometry.scrollbar;
            let top = gui.list_view(node).map_or(0, |view| view.top);
            let shown = top..top + geometry.visible_rows;
            let rows_of = |id: i32| {
                (id == FAMILY_LIST_ID).then(|| {
                    let rows = list.iter().enumerate();
                    rows.map(|(row, movie)| match shown.contains(&row) {
                        true => (process_assets.csf_text(movie.label_key), u32::MAX),
                        false => (String::new(), u32::MAX),
                    })
                    .collect()
                })
            };
            let style = GuiStyle {
                rows_of: Some(&rows_of),
                ..Default::default()
            };
            let over = &mut frame.over;
            paint_gui_styled_into(
                over,
                &platform.ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                gui,
                node,
                page.scale,
                &[],
                style,
            );
        }
        frame.list_boxes.push((layout.list, bar));
        if let Some((gui, root)) = frame.buttons {
            let prompt = [MOVIE_LIST_PROMPT];
            let style = GuiStyle::default();
            let over = &mut frame.over;
            paint_gui_ids_into(
                over,
                &platform.ime,
                &renderer.bit_font,
                frontend,
                match_state,
                process_assets,
                gui,
                root,
                prompt,
                page.scale,
                style,
            );
        }
    }
    encode_family_page(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
        encoder,
        destination,
        frame,
    );
    Ok(true)
}

/// A cleared black frame: Play_Movie's closing clear (`arg1 == 1`) and the
/// end of Show_Credits, shown for the frame on which the presentation ends.
pub(crate) fn render_fullscreen_black(
    renderer: &RendererState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) {
    encode_shell_pass(
        renderer,
        encoder,
        destination,
        "Shell black",
        ShellComposition::default(),
    );
}

/// Paint the active full-screen movie on black, without a cursor.
pub(crate) fn render_fullscreen_movie(
    renderer: &RendererState,
    fullscreen_movie: &Option<FullscreenMovie>,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<()> {
    let Some(movie) = fullscreen_movie.as_ref() else {
        return Ok(());
    };
    let instance = movie.instance(
        renderer.gpu.config.width as i32,
        renderer.gpu.config.height as i32,
    );
    let draws = [TexturedDraw {
        texture: movie.surface().batch_texture(),
        instances: vec![instance],
    }];
    let subtitle: Vec<_> = movie
        .subtitle_text(
            &renderer.bit_font,
            renderer.gpu.config.width as i32,
            renderer.gpu.config.height as i32,
        )
        .into_iter()
        .collect();
    encode_shell_pass(
        renderer,
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
    window_active: bool,
    renderer: &RendererState,
    credits_roll: &mut Option<CreditsRollSession>,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<()> {
    let screen_w = renderer.gpu.config.width;
    let screen_h = renderer.gpu.config.height;
    let viewport = super::credits_roll::CreditsViewport::new(screen_w, screen_h);
    let active = window_active;
    let Some(session) = credits_roll.as_mut() else {
        return Ok(());
    };
    if active || session.last_drawn.is_empty() {
        session.last_drawn = session.roll.instances(&renderer.bit_font, viewport);
    }
    // Glyphs are clipped to the 520 px box (0x006211D0 sets the clip rect).
    let text = [ShellTextDraw {
        instances: session.last_drawn.clone(),
        scissor: viewport.scissor,
    }];
    encode_shell_pass(
        renderer,
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
    use crate::ui::main_menu_shell::RectPx;
    use vera20k_gui::shell::list::{ListMetrics, list_box_geometry};

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
