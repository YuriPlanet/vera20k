//! Render glue for the Movies & Credits children: movie list `0x129`, the
//! full-screen Play_Movie presentation and the Show_Credits roll.

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::shell_pass::{
    ShellComposition, TexturedDraw, encode_shell_pass, owner_draw_button_label_rect,
    software_cursor,
};
use crate::render::batch::SpriteInstance;
use crate::render::main_menu_shell_chrome::{MainMenuShellChromeAtlas, MainMenuShellChromeEntry};
use crate::render::shell_paint::{
    self, CHROME_DEPTH, CURSOR_DEPTH, PARENT_BACKGROUND_DEPTH, PaintButton, PaintLabel,
    SHELL_TEXT_RGB_ENABLED, push_entry_crop, push_entry_scaled, push_entry_window_crop,
};
use crate::render::shell_surface_present::SurfaceEffects;
use crate::render::shell_text::{ShellAlign, ShellTextDraw};
use crate::ui::main_menu_shell::RectPx;
use crate::ui::movies_credits_shell::{
    MOVIE_LIST_DIALOG, MOVIE_LIST_PAGE, MOVIE_LIST_PROMPT_KEY, MOVIE_LIST_TOOLTIP_KEY,
    MovieListLayout, find_family_list,
};
use crate::ui::shell::geom::ShellScale;
use crate::ui::shell::list::{
    ListFrameTone, ListMetrics, ListScrollPart, ShellListGeometry, list_frame_lines,
    scrollbar_edge_lines, scrollbar_interior,
};
use crate::ui::shell::menu_page;
use crate::ui::shell::static_reveal::Kind1RevealWindow;
use vera20k_gui::ListView;

/// Selected-row fill `0x0000FF` (RGB red).
const LIST_SELECTED_FILL: [u8; 3] = [0xFF, 0x00, 0x00];
/// Row text inset from the row's left edge.
const LIST_TEXT_INSET: i32 = 2;

const LIST_DEPTH: f32 = CHROME_DEPTH - 0.00002;
const LIST_FILL_DEPTH: f32 = CHROME_DEPTH - 0.00003;

fn rgb(color: [u8; 3]) -> [f32; 3] {
    color.map(|channel| f32::from(channel) / 255.0)
}

fn push_solid(
    out: &mut Vec<SpriteInstance>,
    atlas: &MainMenuShellChromeAtlas,
    rect: RectPx,
    color: [u8; 3],
    depth: f32,
) {
    let Some(white) = atlas.white_pixel else {
        return;
    };
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    shell_paint::push_tinted_entry(out, white, rect, rgb(color), depth);
}

/// The list's two frame rings around window `w` (`0x00619230`).
fn push_list_frame(out: &mut Vec<SpriteInstance>, atlas: &MainMenuShellChromeAtlas, w: RectPx) {
    for (line, tone) in list_frame_lines(w) {
        push_solid(out, atlas, line, tone.rgb(), LIST_DEPTH);
    }
}

/// A `BS_GROUPBOX` frame (`0x0061E700`, executed in the Westwood Online
/// research harness): the list's two rings, light over dark, with the top
/// edge eight pixels below the window top and the rings on the window's last
/// column and row. The insets are the authored ones, so a stretched window
/// keeps the caption's gap in proportion.
pub(crate) fn push_group_box(
    out: &mut Vec<SpriteInstance>,
    atlas: &MainMenuShellChromeAtlas,
    window: RectPx,
    scale: ShellScale,
) {
    push_list_frame(
        out,
        atlas,
        scale.derive_rect(window, |window| {
            RectPx::new(window.x + 1, window.y + 9, window.w - 3, window.h - 11)
        }),
    );
}

/// The list's darkened backing: everything inside the inner frame ring,
/// including the scrollbar track (translucency 0 at `0x00619230`).
fn list_interior(list: RectPx) -> RectPx {
    RectPx::new(list.x + 1, list.y + 1, list.w - 1, list.h - 1)
}

/// Scrollbar child of list `0x744`, measured on the retail 17-row list
/// (`list17-*.png`). Its left edge adds a light line at `bar.x` and a dark
/// line at `bar.x + 1` between the list's top and bottom rings, taking the
/// corner color where a line crosses a ring of the other tone. Inside, the
/// parent background shows without the list's darkening; 18x22 arrows sit
/// at `bar.x + 2` against the rings, and the grip middle is tiled from the
/// thumb top with the 2-row caps drawn over it.
fn push_list_scrollbar(
    out: &mut Vec<SpriteInstance>,
    atlas: &MainMenuShellChromeAtlas,
    backdrop: &FamilyBackdrop,
    geometry: &ShellListGeometry,
    pressed: Option<ListScrollPart>,
) {
    let (Some(bar), Some(thumb)) = (geometry.scrollbar, geometry.thumb) else {
        return;
    };
    let scale = backdrop.scale;
    for (line, tone) in scrollbar_edge_lines(bar) {
        // The corner pixels go over the lines they cross.
        let depth = if tone == ListFrameTone::Corner {
            LIST_DEPTH - 0.000005
        } else {
            LIST_DEPTH
        };
        push_solid(out, atlas, line, tone.rgb(), depth);
    }
    let interior = scrollbar_interior(bar);
    backdrop.push_pixels(out, backdrop.background, interior, LIST_FILL_DEPTH);
    shell_paint::push_list_scroll_art(
        out,
        &atlas.list_scroll,
        (bar, thumb, geometry.metrics.arrow_h),
        pressed,
        scale,
        (LIST_DEPTH - 0.00001, LIST_DEPTH - 0.00002),
    );
}

/// The family pages' parent background (`0x0060D20B`: MNSCRNL, MNSCRNS at
/// 640 wide) with its darkened copy for list interiors, at its origin: the
/// native canvas at the centered dialog origin on the retail layout, scaled
/// with the page while retaining the SHP canvas extent.
pub(crate) struct FamilyBackdrop {
    background: Option<MainMenuShellChromeEntry>,
    darkened: Option<MainMenuShellChromeEntry>,
    origin: (i32, i32),
    screen: (i32, i32),
    scale: ShellScale,
}

pub(crate) fn family_backdrop(
    atlas: &MainMenuShellChromeAtlas,
    screen_w: i32,
    screen_h: i32,
    scale: ShellScale,
) -> FamilyBackdrop {
    let (background, darkened) = if scale.is_retail() && screen_w == 640 {
        (
            atlas.parent_background_640_mnscrns,
            atlas.parent_background_640_mnscrns_list,
        )
    } else {
        (
            atlas.parent_background_large_mnscrnl,
            atlas.parent_background_large_mnscrnl_list,
        )
    };
    FamilyBackdrop {
        background,
        darkened,
        origin: if scale.is_retail() {
            crate::ui::shell::geom::dialog_origin(screen_w, screen_h)
        } else {
            (0, 0)
        },
        screen: (screen_w, screen_h),
        scale,
    }
}

impl FamilyBackdrop {
    /// The background whole at its origin: `0xE2`'s parent background, and
    /// the RA2TS area of a menu page while no movie is drawn.
    pub(crate) fn canvas(&self) -> Vec<SpriteInstance> {
        let mut out = Vec::new();
        if let Some(entry) = self.background {
            let (x, y) = self.origin;
            push_entry_scaled(&mut out, entry, x, y, self.scale, PARENT_BACKGROUND_DEPTH);
        }
        out
    }

    /// The backdrop's own pixels behind `rect`, taken with the mapping the
    /// backdrop is drawn with: 1:1 from the centered dialog canvas on the
    /// retail layout, and, on a scaled page, proportionally from the canvas
    /// scaled with the page, not stretched to its full extent. The list's darkened
    /// interior and scrollbar track use this same mapping.
    fn push_pixels(
        &self,
        out: &mut Vec<SpriteInstance>,
        entry: Option<MainMenuShellChromeEntry>,
        rect: RectPx,
        depth: f32,
    ) {
        let Some(entry) = entry else {
            return;
        };
        if self.scale.is_retail() {
            push_entry_crop(out, entry, self.origin, rect, depth);
        } else {
            // 0x0060D20B selects MNSCRNL; its retail SHP canvas is 632x568,
            // not the enclosing 800x600 dialog. Preserve that authored extent
            // when scaling, including the darkened list/scrollbar under-patches.
            let (w, h) = entry.native_size();
            let area = self.scale.scale_rect(RectPx::new(0, 0, w, h));
            push_entry_window_crop(out, entry, area, rect, depth);
        }
    }
}

pub(crate) fn push_family_backdrop(out: &mut Vec<SpriteInstance>, backdrop: &FamilyBackdrop) {
    let (screen_w, screen_h) = backdrop.screen;
    backdrop.push_pixels(
        out,
        backdrop.background,
        RectPx::new(0, 0, screen_w, screen_h),
        PARENT_BACKGROUND_DEPTH,
    );
}

/// The family page `dialog`'s list as its `List` node shows it, with its
/// geometry in window `list`.
pub(crate) fn family_list_view(
    state: &AppState,
    dialog: crate::ui::shell::descriptor::DialogId,
    list: RectPx,
    scale: ShellScale,
) -> Option<(ShellListGeometry, ListView)> {
    let controller = &state.frontend.shell_controller;
    let view = controller
        .gui()
        .list_view(find_family_list(controller, dialog)?)?;
    let rect = crate::ui::shell::list::list_box_rect(list);
    Some((view.geometry(rect, ListMetrics::scaled(scale)), view))
}

/// Whether the pointer is over the family page `dialog`'s list, its status
/// help's hover target. The page is the top dialog.
pub(crate) fn family_list_hovered(
    state: &AppState,
    dialog: crate::ui::shell::descriptor::DialogId,
) -> bool {
    let controller = &state.frontend.shell_controller;
    controller.top_id() == Some(dialog)
        && find_family_list(controller, dialog)
            .is_some_and(|node| controller.gui().hovered() == Some(node))
}

/// A family list box over the parent background: its darkened interior,
/// the two frame rings around `window`, the scrollbar when the rows
/// overflow, and the selected row's fill. Row text is the caller's.
pub(crate) fn push_family_list(
    out: &mut Vec<SpriteInstance>,
    atlas: &MainMenuShellChromeAtlas,
    backdrop: &FamilyBackdrop,
    window: RectPx,
    rows: Option<&(ShellListGeometry, ListView)>,
) {
    backdrop.push_pixels(
        out,
        backdrop.darkened,
        list_interior(window),
        LIST_FILL_DEPTH + 0.00001,
    );
    push_list_frame(out, atlas, window);
    let Some((geometry, view)) = rows else {
        return;
    };
    push_list_scrollbar(out, atlas, backdrop, geometry, view.pressed);
    if let Some(selected) = view.selected
        && let Some(visible) = selected.checked_sub(view.top)
        && visible < geometry.visible_rows
    {
        push_solid(
            out,
            atlas,
            geometry.row(visible),
            LIST_SELECTED_FILL,
            LIST_FILL_DEPTH,
        );
    }
}

/// Sprites and labels for dialog `0x129`.
fn movie_list_composition<'a>(
    state: &'a AppState,
    atlas: &MainMenuShellChromeAtlas,
    layout: &MovieListLayout,
    monitor_frame: Option<usize>,
    title_window: Option<Kind1RevealWindow>,
) -> (
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
    Vec<PaintLabel<'a>>,
) {
    let screen_w = layout.page.screen.w;
    let backdrop = family_backdrop(atlas, screen_w, layout.page.screen.h, layout.page.scale);
    let mut sprites = Vec::new();
    push_family_backdrop(&mut sprites, &backdrop);
    sprites.extend(shell_paint::paint_chrome(
        atlas,
        layout.page.right_panel,
        Some(layout.page.lower_strip),
        screen_w,
    ));
    sprites.extend(monitor_frame.and_then(|frame| {
        shell_paint::paint_warning_monitor(
            atlas,
            layout.page.warning_monitor,
            frame,
            layout.page.scale,
        )
    }));

    let exit_wave = crate::app::frontend::shell_transition::shell_exit_wave(
        state,
        crate::app::frontend::shell_transition::ShellSlideKind::MovieList,
    );
    // The teardown slide covers every child with the dialog's own repaint
    // (`0x00622C4F`), and nothing repaints them before the dialog is gone:
    // the list box and the prompt stay blank.
    let leaving = exit_wave.is_some();
    let list = state.frontend.movie_list.as_ref().filter(|_| !leaving);
    let view = list
        .and_then(|_| family_list_view(state, MOVIE_LIST_DIALOG, layout.list, layout.page.scale));
    let mut labels = Vec::new();
    if !leaving {
        push_family_list(&mut sprites, atlas, &backdrop, layout.list, view.as_ref());
    }
    if let (Some(list), Some((geometry, view))) = (list, view) {
        for (visible, index) in (view.top..list.len())
            .take(geometry.visible_rows)
            .enumerate()
        {
            let row = geometry.row(visible);
            labels.push(PaintLabel {
                text: state.csf_text(list[index].label_key).into(),
                rect: RectPx::new(
                    row.x + LIST_TEXT_INSET,
                    row.y,
                    row.w - LIST_TEXT_INSET,
                    row.h,
                ),
                align: ShellAlign::NONE,
                rgb: SHELL_TEXT_RGB_ENABLED,
                path_a_reveal: None,
            });
        }
    }

    let controller = &state.frontend.shell_controller;
    let active = controller.top_id() == Some(MOVIE_LIST_PAGE.dialog);
    let pressed = active
        .then(|| menu_page::pressed_id(controller, MOVIE_LIST_PAGE.dialog))
        .flatten();
    let hovered = active
        .then(|| menu_page::hovered_id(controller, MOVIE_LIST_PAGE.dialog))
        .flatten();
    let wave = exit_wave.or(state.frontend.shell_first_paint_slide.as_ref());
    // While a slide runs the engine draws the whole tile column in place of
    // the buttons (`0x006071E0`).
    let buttons: Vec<PaintButton> = match wave {
        Some(wave) => {
            sprites.extend(shell_paint::paint_slide_column(
                atlas,
                layout.page.right_panel,
                &wave.button_draws(),
            ));
            Vec::new()
        }
        None => layout
            .page
            .buttons
            .iter()
            .map(|button| PaintButton {
                rect: button.rect,
                pressed: pressed == Some(button.id),
                hovered: hovered == Some(button.id),
                enabled: true,
            })
            .collect(),
    };
    let button_sprites = shell_paint::paint_buttons(
        atlas,
        &buttons,
        crate::app::frontend::menu_page_render::MENU_PAGE_BUTTON_POLICY,
        crate::util::time::Instant::now(),
        None,
    );
    // The slide engine draws the button frames only; captions return with
    // the first ordinary paint after the entry slide.
    for button in layout.page.buttons.iter().filter(|_| wave.is_none()) {
        let Some(spec) = MOVIE_LIST_PAGE.button(button.id) else {
            continue;
        };
        labels.push(PaintLabel {
            text: state.csf_text(spec.csf_key).into(),
            rect: owner_draw_button_label_rect(button.rect, pressed == Some(button.id)),
            align: ShellAlign::BUTTON_CAPTION,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    if let Some(window) = title_window {
        labels.push(PaintLabel {
            text: state.csf_text(MOVIE_LIST_PAGE.title_key).into(),
            rect: layout.page.title,
            align: ShellAlign::PANEL_TITLE,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: Some(
                crate::app::frontend::main_menu_shell_render::shell_reveal_path_a(window),
            ),
        });
    }
    if !leaving {
        labels.push(PaintLabel {
            text: state.csf_text(MOVIE_LIST_PROMPT_KEY).into(),
            rect: layout.prompt,
            align: ShellAlign::H_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    (sprites, button_sprites, labels)
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
    if state.frontend.main_menu_shell_chrome.is_none() {
        return Ok(false);
    }
    let layout = crate::app::frontend::shell_layout::movie_list_layout(state);
    // The teardown slide starts with a full dialog repaint (`0x00622C4F`)
    // and pumps no messages until it ends: the statics stay blank and the
    // monitor window shows the right panel's own art.
    let leaving = crate::app::frontend::shell_transition::shell_exit_wave(
        state,
        crate::app::frontend::shell_transition::ShellSlideKind::MovieList,
    )
    .is_some();
    let monitor_frame = if leaving {
        None
    } else {
        crate::app::frontend::menu_page_render::paint_shell_monitor(state)
    };
    let (title_window, status_label) = if leaving {
        (None, None)
    } else {
        let title_window = state
            .frontend
            .shell_page_title
            .paint(crate::util::time::Instant::now());
        let status_text = movie_list_status_key(state)
            .map(|key| state.csf_text(key))
            .unwrap_or_default();
        let status_label = crate::app::frontend::menu_page_render::paint_shell_status_line(
            state,
            status_text,
            layout.page.status_help,
        );
        (title_window, status_label)
    };
    let Some(atlas) = state.frontend.main_menu_shell_chrome.as_ref() else {
        return Ok(false);
    };
    let (sprites, button_sprites, mut labels) =
        movie_list_composition(state, atlas, &layout, monitor_frame, title_window);
    labels.extend(status_label);
    let text = shell_paint::paint_labels_scaled(
        &state.renderer.bit_font,
        &labels,
        layout.page.scale.factor(),
    );
    let draws = [
        TexturedDraw {
            texture: &atlas.texture,
            instances: sprites,
        },
        TexturedDraw {
            texture: &atlas.texture,
            instances: button_sprites,
        },
    ];
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Movie List Shell",
        ShellComposition {
            draws: &draws,
            text: &text,
            cursor: software_cursor(state, CURSOR_DEPTH),
            effects: None,
            smooth_art: false,
        },
    );
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
    use crate::ui::shell::list::list_box_geometry;

    #[test]
    fn family_background_preserves_retail_canvas_when_scaled() {
        let entry = MainMenuShellChromeEntry {
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            pixel_size: [632.0, 568.0],
        };
        for (w, h) in [(1600, 1200), (1920, 1080), (640, 360)] {
            let scale = ShellScale::from_screen(w, h);
            let backdrop = FamilyBackdrop {
                background: Some(entry),
                darkened: Some(entry),
                origin: (0, 0),
                screen: (w, h),
                scale,
            };
            let mut sprites = Vec::new();
            push_family_backdrop(&mut sprites, &backdrop);
            let expected = scale.scale_rect(RectPx::new(0, 0, 632, 568));
            assert_eq!(sprites[0].position, [expected.x as f32, expected.y as f32]);
            assert_eq!(sprites[0].size, [expected.w as f32, expected.h as f32]);
            assert_eq!(sprites[0].uv_size, [1.0, 1.0]);
            let inset = scale.scale_rect(RectPx::new(158, 142, 316, 284));
            backdrop.push_pixels(&mut sprites, Some(entry), inset, LIST_FILL_DEPTH);
            let patch = &sprites[1];
            assert!((patch.uv_origin[0] - 0.25).abs() < 0.002);
            assert!((patch.uv_origin[1] - 0.25).abs() < 0.002);
            assert!((patch.uv_size[0] - 0.5).abs() < 0.002);
            assert!((patch.uv_size[1] - 0.5).abs() < 0.002);
        }
    }

    #[test]
    fn list_interior_and_rows_match_the_native_capture() {
        let list = RectPx::new(120, 128, 399, 304);
        // Selection fill measured at x 121..=518, y 129..=147.
        assert_eq!(list_interior(list), RectPx::new(121, 129, 398, 303));
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
