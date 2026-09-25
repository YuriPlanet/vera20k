//! Render glue for the Movies & Credits children: movie list `0x129`, the
//! full-screen Play_Movie presentation and the Show_Credits roll.

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::shell_pass::{
    ShellComposition, TexturedDraw, encode_shell_pass, owner_draw_button_label_rect, resolve_csf,
    software_cursor,
};
use crate::render::batch::SpriteInstance;
use crate::render::main_menu_shell_chrome::{MainMenuShellChromeAtlas, MainMenuShellChromeEntry};
use crate::render::shell_paint::{
    self, CHROME_DEPTH, CURSOR_DEPTH, PARENT_BACKGROUND_DEPTH, PaintButton, PaintLabel,
    SHELL_TEXT_RGB_ENABLED,
};
use crate::render::shell_surface_present::SurfaceEffects;
use crate::render::shell_text::{ShellAlign, ShellTextDraw};
use crate::ui::main_menu_shell::RectPx;
use crate::ui::movies_credits_shell::{
    MOVIE_LIST_CONTROL, MOVIE_LIST_PAGE, MOVIE_LIST_PROMPT_KEY, MOVIE_LIST_TOOLTIP_KEY,
    MovieListLayout, compute_movie_list_layout,
};
use crate::ui::shell::list::{ListScrollPart, ShellListGeometry};
use crate::ui::shell::static_reveal::Kind1RevealWindow;

/// Owner-draw ListBox frame colors (`0x00619230`), as RGB: light
/// `0xC5BEA7`, dark `0x807A68`, and their average `0xA29C87` at the two
/// corners where they meet.
const LIST_FRAME_LIGHT: [u8; 3] = [0xA7, 0xBE, 0xC5];
const LIST_FRAME_DARK: [u8; 3] = [0x68, 0x7A, 0x80];
const LIST_FRAME_CORNER: [u8; 3] = [0x87, 0x9C, 0xA2];
/// Selected-row fill `0x0000FF` (RGB red).
const LIST_SELECTED_FILL: [u8; 3] = [0xFF, 0x00, 0x00];
/// Row text inset from the row's left edge.
const LIST_TEXT_INSET: i32 = 2;

const LIST_DEPTH: f32 = CHROME_DEPTH - 0.00002;
const LIST_FILL_DEPTH: f32 = CHROME_DEPTH - 0.00003;

fn rgb(color: [u8; 3]) -> [f32; 3] {
    color.map(|channel| f32::from(channel) / 255.0)
}

fn push_entry_crop(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
    entry_origin: (i32, i32),
    rect: RectPx,
    depth: f32,
) {
    // Clip the destination to the entry canvas, then map it to atlas UVs.
    let left = rect.x.max(entry_origin.0);
    let top = rect.y.max(entry_origin.1);
    let right = (rect.x + rect.w).min(entry_origin.0 + entry.pixel_size[0] as i32);
    let bottom = (rect.y + rect.h).min(entry_origin.1 + entry.pixel_size[1] as i32);
    if right <= left || bottom <= top {
        return;
    }
    let u_per_px = entry.uv_size[0] / entry.pixel_size[0];
    let v_per_px = entry.uv_size[1] / entry.pixel_size[1];
    out.push(SpriteInstance {
        position: [left as f32, top as f32],
        size: [(right - left) as f32, (bottom - top) as f32],
        uv_origin: [
            entry.uv_origin[0] + (left - entry_origin.0) as f32 * u_per_px,
            entry.uv_origin[1] + (top - entry_origin.1) as f32 * v_per_px,
        ],
        uv_size: [
            (right - left) as f32 * u_per_px,
            (bottom - top) as f32 * v_per_px,
        ],
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    });
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
    out.push(SpriteInstance {
        position: [rect.x as f32, rect.y as f32],
        size: [rect.w as f32, rect.h as f32],
        uv_origin: white.uv_origin,
        uv_size: white.uv_size,
        depth,
        tint: rgb(color),
        alpha: 1.0,
        ..Default::default()
    });
}

/// The two offset frame rings around list window `w`, as measured in the
/// native capture. With `R = x + w` and `B = y + h`, the outer ring spans
/// `x-1..=R+1` by `y-1..=B+1` (light top/left, dark bottom/right) and the
/// inner ring spans `x..=R` by `y..=B` with the colors swapped. Each ring's
/// top-right and bottom-left corner takes the average color.
fn push_list_frame(out: &mut Vec<SpriteInstance>, atlas: &MainMenuShellChromeAtlas, w: RectPx) {
    let (left, top) = (w.x, w.y);
    let (right, bottom) = (w.x + w.w, w.y + w.h);
    for (ring, top_left, bottom_right) in [
        (1, LIST_FRAME_LIGHT, LIST_FRAME_DARK),
        (0, LIST_FRAME_DARK, LIST_FRAME_LIGHT),
    ] {
        let (x0, y0) = (left - ring, top - ring);
        let (x1, y1) = (right + ring, bottom + ring);
        // Top edge x0..x1-1, corner at x1; left edge y0..y1-1, corner at y1.
        push_solid(
            out,
            atlas,
            RectPx::new(x0, y0, x1 - x0, 1),
            top_left,
            LIST_DEPTH,
        );
        push_solid(
            out,
            atlas,
            RectPx::new(x0, y0, 1, y1 - y0),
            top_left,
            LIST_DEPTH,
        );
        push_solid(
            out,
            atlas,
            RectPx::new(x1, y0, 1, 1),
            LIST_FRAME_CORNER,
            LIST_DEPTH,
        );
        push_solid(
            out,
            atlas,
            RectPx::new(x0, y1, 1, 1),
            LIST_FRAME_CORNER,
            LIST_DEPTH,
        );
        // Right edge y0+1..=y1, bottom edge x0+1..=x1.
        push_solid(
            out,
            atlas,
            RectPx::new(x1, y0 + 1, 1, y1 - y0),
            bottom_right,
            LIST_DEPTH,
        );
        push_solid(
            out,
            atlas,
            RectPx::new(x0 + 1, y1, x1 - x0, 1),
            bottom_right,
            LIST_DEPTH,
        );
    }
}

/// A `BS_GROUPBOX` frame (`0x0061E700`, executed in the Westwood Online
/// research harness): the list's two rings, light over dark, with the top
/// edge eight pixels below the window top and the rings on the window's last
/// column and row.
pub(crate) fn push_group_box(
    out: &mut Vec<SpriteInstance>,
    atlas: &MainMenuShellChromeAtlas,
    window: RectPx,
) {
    push_list_frame(
        out,
        atlas,
        RectPx::new(window.x + 1, window.y + 9, window.w - 3, window.h - 11),
    );
}

/// List interior (`x+1, y+1, w-1, h-1`) and the rows it holds.
/// One atlas entry at its native size.
fn push_entry_native(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
    x: i32,
    y: i32,
    depth: f32,
) {
    out.push(SpriteInstance {
        position: [x as f32, y as f32],
        size: entry.pixel_size,
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    });
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
    backdrop: Option<(MainMenuShellChromeEntry, (i32, i32))>,
    bar: RectPx,
    thumb: RectPx,
    pressed: Option<ListScrollPart>,
) {
    let (top, bottom) = (bar.y, bar.y + bar.h - 1);
    push_solid(
        out,
        atlas,
        RectPx::new(bar.x, top, 1, bottom - top),
        LIST_FRAME_LIGHT,
        LIST_DEPTH,
    );
    push_solid(
        out,
        atlas,
        RectPx::new(bar.x + 1, top - 1, 1, bottom - top + 1),
        LIST_FRAME_DARK,
        LIST_DEPTH,
    );
    for (x, y) in [(bar.x, top), (bar.x + 1, top - 1), (bar.x + 1, bottom)] {
        push_solid(
            out,
            atlas,
            RectPx::new(x, y, 1, 1),
            LIST_FRAME_CORNER,
            LIST_DEPTH - 0.000005,
        );
    }
    let interior = RectPx::new(bar.x + 2, bar.y + 1, bar.w - 2, bar.h - 2);
    if let Some((entry, origin)) = backdrop {
        push_entry_crop(out, entry, origin, interior, LIST_FILL_DEPTH);
    }
    let art = &atlas.list_scroll;
    let x = interior.x;
    let arrow_h = art
        .down_released
        .map_or(22, |entry| entry.pixel_size[1].round() as i32);
    for (released, pressed_art, part, y) in [
        (
            art.up_released,
            art.up_pressed,
            ListScrollPart::Up,
            interior.y,
        ),
        (
            art.down_released,
            art.down_pressed,
            ListScrollPart::Down,
            interior.y + interior.h - arrow_h,
        ),
    ] {
        let entry = if pressed == Some(part) {
            pressed_art.or(released)
        } else {
            released
        };
        if let Some(entry) = entry {
            push_entry_native(out, entry, x, y, LIST_DEPTH - 0.00001);
        }
    }
    if let Some(mid) = art.grip_mid {
        let tile_h = mid.pixel_size[1].round() as i32;
        let mut y = thumb.y;
        while tile_h > 0 && y < thumb.y + thumb.h {
            let h = tile_h.min(thumb.y + thumb.h - y);
            out.push(SpriteInstance {
                position: [x as f32, y as f32],
                size: [mid.pixel_size[0], h as f32],
                uv_origin: mid.uv_origin,
                uv_size: [mid.uv_size[0], mid.uv_size[1] * h as f32 / tile_h as f32],
                depth: LIST_DEPTH - 0.00001,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            });
            y += tile_h;
        }
    }
    if let Some(top) = art.grip_top {
        push_entry_native(out, top, x, thumb.y, LIST_DEPTH - 0.00002);
    }
    if let Some(bottom) = art.grip_bottom {
        let h = bottom.pixel_size[1].round() as i32;
        push_entry_native(out, bottom, x, thumb.y + thumb.h - h, LIST_DEPTH - 0.00002);
    }
}

/// The family pages' parent background (`0x0060D20B`: MNSCRNL, MNSCRNS at
/// 640 wide) with its darkened copy for list interiors, at its origin.
pub(crate) struct FamilyBackdrop {
    background: Option<MainMenuShellChromeEntry>,
    darkened: Option<MainMenuShellChromeEntry>,
    origin: (i32, i32),
}

pub(crate) fn family_backdrop(
    atlas: &MainMenuShellChromeAtlas,
    screen_w: i32,
    screen_h: i32,
) -> FamilyBackdrop {
    let (background, darkened) = if screen_w == 640 {
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
        origin: crate::app::frontend::main_menu_shell_render::shell_background_origin(
            screen_w, screen_h,
        ),
    }
}

pub(crate) fn push_family_backdrop(out: &mut Vec<SpriteInstance>, backdrop: &FamilyBackdrop) {
    if let Some(entry) = backdrop.background {
        let (x, y) = backdrop.origin;
        push_entry_crop(
            out,
            entry,
            backdrop.origin,
            RectPx::new(x, y, entry.pixel_size[0] as i32, entry.pixel_size[1] as i32),
            PARENT_BACKGROUND_DEPTH,
        );
    }
}

/// The rows a family list shows, when it has any state to paint.
pub(crate) struct FamilyListRows<'a> {
    pub geometry: &'a ShellListGeometry,
    pub top: usize,
    pub selected: Option<usize>,
    pub pressed: Option<ListScrollPart>,
}

/// A family list box over the parent background: its darkened interior,
/// the two frame rings around `window`, the scrollbar when the rows
/// overflow, and the selected row's fill. Row text is the caller's.
pub(crate) fn push_family_list(
    out: &mut Vec<SpriteInstance>,
    atlas: &MainMenuShellChromeAtlas,
    backdrop: &FamilyBackdrop,
    window: RectPx,
    rows: Option<FamilyListRows<'_>>,
) {
    if let Some(entry) = backdrop.darkened {
        push_entry_crop(
            out,
            entry,
            backdrop.origin,
            list_interior(window),
            LIST_FILL_DEPTH + 0.00001,
        );
    }
    push_list_frame(out, atlas, window);
    let Some(rows) = rows else {
        return;
    };
    let geometry = rows.geometry;
    if let (Some(bar), Some(thumb)) = (geometry.scrollbar, geometry.thumb) {
        push_list_scrollbar(
            out,
            atlas,
            backdrop.background.map(|entry| (entry, backdrop.origin)),
            bar,
            thumb,
            rows.pressed,
        );
    }
    if let Some(selected) = rows.selected
        && let Some(visible) = selected.checked_sub(rows.top)
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
    let backdrop = family_backdrop(atlas, screen_w, layout.page.screen.h);
    let mut sprites = Vec::new();
    push_family_backdrop(&mut sprites, &backdrop);
    sprites.extend(shell_paint::paint_chrome(
        atlas,
        layout.page.right_panel,
        Some(layout.page.lower_strip),
        screen_w,
    ));
    sprites.extend(monitor_frame.and_then(|frame| {
        shell_paint::paint_warning_monitor(atlas, layout.page.warning_monitor, frame)
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
    let mut labels = Vec::new();
    if !leaving {
        let geometry = list.map(|list| list.geometry(layout.list));
        push_family_list(
            &mut sprites,
            atlas,
            &backdrop,
            layout.list,
            geometry
                .as_ref()
                .zip(list)
                .map(|(geometry, list)| FamilyListRows {
                    geometry,
                    top: list.top,
                    selected: list.selected,
                    pressed: list.scroll.pressed_part(),
                }),
        );
    }
    if let Some(list) = list {
        let geometry = list.geometry(layout.list);
        for (visible, index) in (list.top..list.rows.len())
            .take(geometry.visible_rows)
            .enumerate()
        {
            let row = geometry.row(visible);
            labels.push(PaintLabel {
                text: resolve_csf(state, list.rows[index].label_key),
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
    let pressed = active.then(|| controller.pressed()).flatten();
    let hovered = active.then(|| controller.hovered()).flatten();
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
        std::time::Instant::now(),
        None,
    );
    // The slide engine draws the button frames only; captions return with
    // the first ordinary paint after the entry slide.
    for button in layout.page.buttons.iter().filter(|_| wave.is_none()) {
        let Some(spec) = MOVIE_LIST_PAGE.button(button.id) else {
            continue;
        };
        labels.push(PaintLabel {
            text: resolve_csf(state, spec.csf_key),
            rect: owner_draw_button_label_rect(button.rect, pressed == Some(button.id)),
            align: ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0),
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    if let Some(window) = title_window {
        labels.push(PaintLabel {
            text: resolve_csf(state, MOVIE_LIST_PAGE.title_key),
            rect: layout.page.title,
            align: ShellAlign::H_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: Some(
                crate::app::frontend::main_menu_shell_render::shell_reveal_path_a(window),
            ),
        });
    }
    if !leaving {
        labels.push(PaintLabel {
            text: resolve_csf(state, MOVIE_LIST_PROMPT_KEY),
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
    match controller.hovered()? {
        MOVIE_LIST_CONTROL => Some(MOVIE_LIST_TOOLTIP_KEY),
        id => MOVIE_LIST_PAGE.button(id).map(|button| button.tooltip_key),
    }
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
    let layout = compute_movie_list_layout(
        state.renderer.gpu.config.width,
        state.renderer.gpu.config.height,
    );
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
            .paint(std::time::Instant::now());
        let status_text = movie_list_status_key(state)
            .map(|key| resolve_csf(state, key).into_owned())
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
    let text = shell_paint::paint_labels(&state.renderer.bit_font, &labels);
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
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Play_Movie",
        ShellComposition {
            draws: &draws,
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
    let active = state.platform.window_active;
    let Some(session) = state.frontend.credits_roll.as_mut() else {
        return Ok(());
    };
    if active || session.last_drawn.is_empty() {
        session.last_drawn = session
            .roll
            .instances(&state.renderer.bit_font, screen_w as i32);
    }
    // Glyphs are clipped to the 520 px box (0x006211D0 sets the clip rect).
    let box_x = crate::app::frontend::credits_roll::credits_box_x(screen_w as i32).max(0) as u32;
    let text = [ShellTextDraw {
        instances: session.last_drawn.clone(),
        scissor: crate::render::shell_text::ScissorRect {
            x: box_x,
            y: 0,
            w: (crate::app::frontend::credits_roll::CREDITS_BOX_WIDTH as u32)
                .min(screen_w.saturating_sub(box_x)),
            h: screen_h,
        },
    }];
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Show_Credits",
        ShellComposition {
            text: &text,
            effects: Some(SurfaceEffects {
                fade_rows: crate::app::frontend::credits_roll::CREDITS_FADE_ROWS,
            }),
            ..ShellComposition::default()
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_interior_and_rows_match_the_native_capture() {
        let list = RectPx::new(120, 128, 399, 304);
        // Selection fill measured at x 121..=518, y 129..=147.
        assert_eq!(list_interior(list), RectPx::new(121, 129, 398, 303));
        let mut state = crate::ui::movies_credits_shell::MovieListState::open(
            crate::ui::movies_credits_shell::MovieProgress::default(),
            -1,
        );
        let short = state.geometry(list);
        assert_eq!(short.row(0), RectPx::new(121, 129, 398, 19));
        assert_eq!(short.visible_rows, 15);
        assert!(short.scrollbar.is_none());
        // Retail list with all 17 movies (`list17-*.png`): bar columns
        // 499..=518, thumb rows 151..=352 at top 0 and 208..=409 at top 2.
        state.rows = crate::ui::movies_credits_shell::active_movie_entries(
            crate::ui::movies_credits_shell::MovieProgress {
                soviet: 7,
                allied: 7,
            },
        );
        assert_eq!(state.rows.len(), 17);
        let full = state.geometry(list);
        assert_eq!(full.scrollbar, Some(RectPx::new(499, 128, 20, 305)));
        assert_eq!(full.thumb, Some(RectPx::new(500, 151, 18, 202)));
        assert_eq!(full.row(0), RectPx::new(121, 129, 378, 19));
        state.top = 2;
        assert_eq!(
            state.geometry(list).thumb,
            Some(RectPx::new(500, 208, 18, 202))
        );
    }
}
