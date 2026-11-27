//! Descriptor-driven owner-draw shell paint pass (substrate Slice 3).
//!
//! ONE emitter for the front-end right-panel shells (main menu 0xE2, single
//! player 0x100). Consumes the ui::shell layout/controller outputs (button rects,
//! pressed/hover/enabled state) read-only and produces GPU draw lists
//! (`SpriteInstance` + `ShellTextDraw`). Lives in render/ because it emits GPU
//! types; ui/ must not depend on render/ (render -> ui is allowed). The caller
//! owns the render pass, the camera, the buffers, and the parent-compose order:
//! it submits the buffers this pass returns in the verified C8 sequence.
//!
//! Slice 5 adds `paint_modal_sprites`: the shared mode-2 SHP modal emitter
//! (PUDLGBGN frame 0 background + MNBTTN owner-draw type-3 buttons). It is
//! source-atlas-neutral — the caller passes the resolved chrome entries (the modal
//! art lives in the skirmish chrome atlas today). The boxes' texts are their
//! widgets' (`app::frontend::skirmish_shell_render::shell_box`).

use std::borrow::Cow;

use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::chrome_atlas::ChromeAtlasEntry;
use crate::render::main_menu_shell_chrome::{
    ListScrollArt, MainMenuShellChromeAtlas, MainMenuShellChromeEntry,
};
use crate::render::shell_text::ShellTextDraw;
use crate::render::shell_text_reveal::PathAReveal;
use crate::render::skirmish_shell_chrome::SkirmishShellChromeEntry;
use crate::ui::shell::geom::{RectPx, RightPanelRects, ShellAlign, ShellScale};
use crate::ui::shell::slide::{ColumnDraw, PanelArt, panel_art_origin};
use vera20k_gui::shell::list::{
    ListScrollPart, grip_tiles, scrollbar_arrow_origins, scrollbar_interior,
};

/// Parent background sits behind the movie in the Z stack. Greater depth =
/// farther back, so this must exceed `MOVIE_DEPTH`. 0xE2-only (the main-menu
/// MNSCRN parent bg); kept here so the whole compose order lives in one place.
pub const PARENT_BACKGROUND_DEPTH: f32 = 0.00098;
pub const MOVIE_DEPTH: f32 = 0.00095;
pub const CHROME_DEPTH: f32 = 0.00085;
/// Dialog-owned background art painted over the shell chrome but behind its
/// static children: campaign score's `SCRT` panel. Native WM_PAINT draws
/// `RightPanel__Draw` then `RadarBackground`; the score statics follow.
pub const DIALOG_BACKGROUND_DEPTH: f32 = 0.00084;
/// Image statics drawn over the right-panel chrome (the `0x71C` monitor).
pub const STATIC_IMAGE_DEPTH: f32 = 0.00083;
pub const BUTTON_DEPTH: f32 = 0.00080;
pub const TEXT_DEPTH: f32 = 0.00070;
/// The software cursor draws on top of everything else (smallest depth). The
/// original hides the OS cursor and blits the cursor SHP last.
pub const CURSOR_DEPTH: f32 = 0.00001;

/// Owner-draw label color when enabled (#FFFF00). Both shells.
pub const SHELL_TEXT_RGB_ENABLED: [f32; 3] = [1.0, 1.0, 0.0];
/// Owner-draw button caption color when disabled outside a suspended game:
/// the RGB `[0x00B0FA94]` (`0x00612F5F` -> `0x00613072`), which `0x0072A8C0`
/// sets to (0x48, 0, 0) (retail `rmg-steady.png`: RGB565 red 9). The column
/// buttons (owner-draw type 1, record `+0xB0`) keep their art: the disabled
/// black blend runs only for type 0 (`0x006135F3..0x0061361B`).
pub const SHELL_TEXT_RGB_DISABLED: [f32; 3] = [0x48 as f32 / 255.0, 0.0, 0.0];
/// One static or button label to paint. `rect` is already inset/sunk and `rgb`
/// already resolved (enabled vs disabled) by the caller-side label builder.
pub struct PaintLabel<'a> {
    pub text: Cow<'a, str>,
    pub rect: RectPx,
    pub align: ShellAlign,
    pub rgb: [f32; 3],
    /// Opt-in BITFONT Path-A reveal/tint. Ordinary labels keep this `None`.
    pub path_a_reveal: Option<PathAReveal>,
}

// --- Chrome atlas entry emitters, shared by every shell renderer ---

/// `entry` stretched to `size`, top-left at `(x, y)`, untinted and opaque.
fn entry_sprite(
    entry: ChromeAtlasEntry,
    x: f32,
    y: f32,
    size: [f32; 2],
    depth: f32,
) -> SpriteInstance {
    quad(
        [x, y],
        size,
        (entry.uv_origin, entry.uv_size),
        [1.0; 3],
        1.0,
        depth,
    )
}

/// One quad on the target: `size` pixels from `position`, sampling `uv`
/// (origin, size) in `tint` and `alpha` at `depth`. Every shell skin's
/// picture and solid fill (a white texel) is one, and so is each of the
/// HUD paint's fills and pictures (`HudPaint::build`).
pub fn quad(
    position: [f32; 2],
    size: [f32; 2],
    (uv_origin, uv_size): ([f32; 2], [f32; 2]),
    tint: [f32; 3],
    alpha: f32,
    depth: f32,
) -> SpriteInstance {
    SpriteInstance {
        position,
        size,
        uv_origin,
        uv_size,
        depth,
        tint,
        alpha,
        ..Default::default()
    }
}

/// One chrome atlas entry stretched to `size`, top-left at `(x, y)`.
pub fn push_entry_sized(
    out: &mut Vec<SpriteInstance>,
    entry: ChromeAtlasEntry,
    x: f32,
    y: f32,
    size: [f32; 2],
    depth: f32,
) {
    out.push(entry_sprite(entry, x, y, size, depth));
}

/// [`push_entry_sized`] at `alpha`.
pub fn push_entry_sized_alpha(
    out: &mut Vec<SpriteInstance>,
    entry: ChromeAtlasEntry,
    x: f32,
    y: f32,
    size: [f32; 2],
    depth: f32,
    alpha: f32,
) {
    out.push(SpriteInstance {
        alpha,
        ..entry_sprite(entry, x, y, size, depth)
    });
}

/// One chrome atlas entry stretched over `rect`.
pub fn push_entry(
    out: &mut Vec<SpriteInstance>,
    entry: ChromeAtlasEntry,
    rect: RectPx,
    depth: f32,
) {
    push_tinted_entry(out, entry, rect, [1.0, 1.0, 1.0], depth);
}

/// [`push_entry`] multiplied by `tint`.
pub fn push_tinted_entry(
    out: &mut Vec<SpriteInstance>,
    entry: ChromeAtlasEntry,
    rect: RectPx,
    tint: [f32; 3],
    depth: f32,
) {
    let (position, size) = (
        [rect.x as f32, rect.y as f32],
        [rect.w as f32, rect.h as f32],
    );
    out.push(quad(
        position,
        size,
        (entry.uv_origin, entry.uv_size),
        tint,
        1.0,
        depth,
    ));
}

/// One chrome atlas entry at its native size, top-left at `(x, y)`.
pub fn push_entry_native(
    out: &mut Vec<SpriteInstance>,
    entry: ChromeAtlasEntry,
    x: i32,
    y: i32,
    depth: f32,
) {
    push_entry_sized(out, entry, x as f32, y as f32, entry.pixel_size, depth);
}

/// One chrome atlas entry authored for the 800x600 dialog, placed at the
/// authored `(x, y)` and grown by `scale`: the point maps through the scale like
/// any authored rect, so it lands inside the centred dialog. Identity at
/// `RETAIL_SCALE`, so an unscaled page emits the same native-size
/// sprite at the same point. [`push_entry_grown`] takes a window-space point.
pub fn push_entry_scaled(
    out: &mut Vec<SpriteInstance>,
    entry: ChromeAtlasEntry,
    x: i32,
    y: i32,
    scale: ShellScale,
    depth: f32,
) {
    let [x, y] = scale.scale_point([x, y]);
    push_entry_grown(out, entry, x, y, scale, depth);
}

/// [`push_entry_scaled`] for art whose window-space top-left the caller
/// already holds, such as a point inside a scaled rect: only the native size
/// grows, so the point is not mapped a second time.
pub fn push_entry_grown(
    out: &mut Vec<SpriteInstance>,
    entry: ChromeAtlasEntry,
    x: i32,
    y: i32,
    scale: ShellScale,
    depth: f32,
) {
    let [w, h] = scale.art_size(entry.pixel_size[0], entry.pixel_size[1]);
    push_entry_sized(
        out,
        entry,
        x as f32,
        y as f32,
        [w.round(), h.round()],
        depth,
    );
}

/// The part of `entry`, drawn with its top-left at `entry_origin`, that lies
/// inside `rect`, 1:1.
pub fn push_entry_crop(
    out: &mut Vec<SpriteInstance>,
    entry: ChromeAtlasEntry,
    entry_origin: (i32, i32),
    rect: RectPx,
    depth: f32,
) {
    if let Some((rect, part)) = entry_crop(entry, entry_origin, rect) {
        push_entry(out, part, rect, depth);
    }
}

/// [`push_entry_crop`] for an entry and rect in the authored 800x600 dialog
/// space, with the result stretched over the device window. The sampled source
/// pixels stay aligned with the stretched background art, so a control's darker
/// under-patch still matches what lies behind it.
pub fn push_entry_crop_scaled(
    out: &mut Vec<SpriteInstance>,
    entry: ChromeAtlasEntry,
    entry_origin: (i32, i32),
    rect: RectPx,
    scale: ShellScale,
    depth: f32,
) {
    if let Some((rect, part)) = entry_crop(entry, entry_origin, rect) {
        push_entry(out, part, scale.scale_rect(rect), depth);
    }
}

/// The part of a backdrop whose whole canvas is scaled over `area` (a scaled
/// page's dialog rect): `rect`'s position in that area selects the texels, so an
/// inset drawn through this lines up with the scaled background it sits on.
/// [`push_entry_crop`] is the 1:1 form of the same idea.
pub fn push_entry_window_crop(
    out: &mut Vec<SpriteInstance>,
    entry: ChromeAtlasEntry,
    area: RectPx,
    rect: RectPx,
    depth: f32,
) {
    let (area_w, area_h) = (area.w.max(1), area.h.max(1));
    let left = rect.x.clamp(area.x, area.x + area_w);
    let top = rect.y.clamp(area.y, area.y + area_h);
    let right = (rect.x + rect.w).clamp(area.x, area.x + area_w);
    let bottom = (rect.y + rect.h).clamp(area.y, area.y + area_h);
    if right <= left || bottom <= top {
        return;
    }
    let u = |value: i32, span: i32| value as f32 / span as f32;
    let part = ChromeAtlasEntry {
        uv_origin: [
            entry.uv_origin[0] + u(left - area.x, area_w) * entry.uv_size[0],
            entry.uv_origin[1] + u(top - area.y, area_h) * entry.uv_size[1],
        ],
        uv_size: [
            u(right - left, area_w) * entry.uv_size[0],
            u(bottom - top, area_h) * entry.uv_size[1],
        ],
        ..entry
    };
    push_entry(
        out,
        part,
        RectPx::new(left, top, right - left, bottom - top),
        depth,
    );
}

/// The destination rect and atlas part of `entry` inside `rect`, both in the
/// space the caller places the entry in.
fn entry_crop(
    entry: ChromeAtlasEntry,
    entry_origin: (i32, i32),
    rect: RectPx,
) -> Option<(RectPx, ChromeAtlasEntry)> {
    // Clip the destination to the entry canvas, then map it to atlas UVs.
    let left = rect.x.max(entry_origin.0);
    let top = rect.y.max(entry_origin.1);
    let right = (rect.x + rect.w).min(entry_origin.0 + entry.pixel_size[0] as i32);
    let bottom = (rect.y + rect.h).min(entry_origin.1 + entry.pixel_size[1] as i32);
    if right <= left || bottom <= top {
        return None;
    }
    let u_per_px = entry.uv_size[0] / entry.pixel_size[0];
    let v_per_px = entry.uv_size[1] / entry.pixel_size[1];
    let part = ChromeAtlasEntry {
        uv_origin: [
            entry.uv_origin[0] + (left - entry_origin.0) as f32 * u_per_px,
            entry.uv_origin[1] + (top - entry_origin.1) as f32 * v_per_px,
        ],
        uv_size: [
            (right - left) as f32 * u_per_px,
            (bottom - top) as f32 * v_per_px,
        ],
        ..entry
    };
    Some((RectPx::new(left, top, right - left, bottom - top), part))
}

/// The top `rect.h / scale_y` source rows of `entry` across the whole of `rect`.
///
/// At `scale_y == 1` this is gamemd's 1:1 top-left crop, which the SDBTM cap
/// needs: the SHP is 168x65 native and the cap region is 23 px tall. On a
/// stretched panel the same 23 source rows fill the taller cap.
pub fn push_entry_top_clipped(
    out: &mut Vec<SpriteInstance>,
    mut entry: ChromeAtlasEntry,
    rect: RectPx,
    scale_y: f32,
    depth: f32,
) {
    let src_h = entry.pixel_size[1].round();
    if rect.w <= 0 || rect.h <= 0 || src_h <= 0.0 {
        return;
    }
    let source_h = (rect.h as f32 / scale_y).min(src_h).max(1.0);
    entry.uv_size[1] *= source_h / src_h;
    push_entry(out, entry, rect, depth);
}

/// `0x006BA3E0`: an opaque copy of `entry` into `rect` that starts the
/// source at the centred offset `max(0, (source - rect) / 2)` on each axis
/// and wraps past its end, so a smaller source tiles and a larger one shows
/// its middle.
///
/// `rect` is the device rect and `scale` stretches it: the tiling runs on the
/// authored numbers and each piece is emitted scaled, so a wider plaque grows
/// TROFM's gradient instead of repeating it. `RETAIL_SCALE` reproduces
/// the plain tiling.
pub fn push_entry_wrapped_centered(
    out: &mut Vec<SpriteInstance>,
    entry: ChromeAtlasEntry,
    rect: RectPx,
    scale: ShellScale,
    depth: f32,
) {
    let src_w = entry.pixel_size[0].round() as i32;
    let src_h = entry.pixel_size[1].round() as i32;
    if src_w <= 0 || src_h <= 0 {
        return;
    }
    let base = scale.base_rect(rect);
    let offset_x = ((src_w - base.w) / 2).max(0);
    let offset_y = ((src_h - base.h) / 2).max(0);
    let mut dy = 0;
    while dy < base.h {
        let sy = (offset_y + dy) % src_h;
        let h = (src_h - sy).min(base.h - dy);
        let mut dx = 0;
        while dx < base.w {
            let sx = (offset_x + dx) % src_w;
            let w = (src_w - sx).min(base.w - dx);
            let mut piece = entry;
            piece.uv_origin[0] += entry.uv_size[0] * sx as f32 / src_w as f32;
            piece.uv_origin[1] += entry.uv_size[1] * sy as f32 / src_h as f32;
            piece.uv_size[0] *= w as f32 / src_w as f32;
            piece.uv_size[1] *= h as f32 / src_h as f32;
            let piece_rect = scale.scale_rect(RectPx::new(base.x + dx, base.y + dy, w, h));
            push_entry(out, piece, piece_rect, depth);
            dx += w;
        }
        dy += h;
    }
}

/// `entry` whole, grown by the layout's `scale`, shrunk to fit `rect` where it
/// is larger, centred in it.
pub fn push_entry_fitted(
    out: &mut Vec<SpriteInstance>,
    entry: ChromeAtlasEntry,
    rect: RectPx,
    scale: ShellScale,
    depth: f32,
) {
    let [art_w, art_h] = scale.art_size(entry.pixel_size[0], entry.pixel_size[1]);
    if art_w <= 0.0 || art_h <= 0.0 || rect.w <= 0 || rect.h <= 0 {
        return;
    }
    let fit = (rect.w as f32 / art_w).min(rect.h as f32 / art_h).min(1.0);
    let (w, h) = ((art_w * fit).round(), (art_h * fit).round());
    let x = rect.x as f32 + ((rect.w as f32 - w) / 2.0).floor();
    let y = rect.y as f32 + ((rect.h as f32 - h) / 2.0).floor();
    push_entry_sized(out, entry, x, y, [w, h], depth);
}

/// Flag and icon art authored at its native size, grown by the layout's
/// stretch, centred in `rect` along an axis where it is smaller and clipped
/// to it. Identity on the retail layout.
pub fn push_flag_entry_clipped_centered(
    out: &mut Vec<SpriteInstance>,
    mut entry: ChromeAtlasEntry,
    rect: RectPx,
    scale: ShellScale,
    depth: f32,
) {
    let [art_w, art_h] = scale.art_size(entry.pixel_size[0], entry.pixel_size[1]);
    if art_w <= 0.0 || art_h <= 0.0 || rect.w <= 0 || rect.h <= 0 {
        return;
    }
    let (art_w_px, art_h_px) = (art_w.round() as i32, art_h.round() as i32);
    let draw_w = art_w.min(rect.w as f32);
    let draw_h = art_h.min(rect.h as f32);
    let x = if art_w_px < rect.w {
        (rect.x + (rect.w - art_w_px) / 2) as f32
    } else {
        rect.x as f32
    };
    let y = if art_h_px < rect.h {
        (rect.y + (rect.h - art_h_px) / 2) as f32
    } else {
        rect.y as f32
    };
    entry.uv_size[0] *= draw_w / art_w;
    entry.uv_size[1] *= draw_h / art_h;
    push_entry_sized(out, entry, x, y, [draw_w, draw_h], depth);
}

/// An owner-draw list scrollbar's art (`0x0061C690`) in `bar`, grown by
/// `scale`: the arrows at its ends, the pressed one's art while pressed,
/// then the grip middle tiled from `thumb`'s top with the top and bottom
/// caps drawn over it. Arrows and middle draw at `depths.0`, caps at `depths.1`.
pub fn push_list_scroll_art(
    out: &mut Vec<SpriteInstance>,
    art: &ListScrollArt,
    (bar, thumb, arrow_h): (RectPx, RectPx, i32),
    pressed: Option<ListScrollPart>,
    scale: ShellScale,
    (depth, cap_depth): (f32, f32),
) {
    for ((released, pressed_art, part), (x, y)) in [
        (art.up_released, art.up_pressed, ListScrollPart::Up),
        (art.down_released, art.down_pressed, ListScrollPart::Down),
    ]
    .into_iter()
    .zip(scrollbar_arrow_origins(bar, arrow_h))
    {
        let entry = if pressed == Some(part) {
            pressed_art.or(released)
        } else {
            released
        };
        if let Some(entry) = entry {
            push_entry_grown(out, entry, x, y, scale, depth);
        }
    }
    let x = scrollbar_interior(bar).x;
    if let Some(mid) = art.grip_mid {
        let [tile_w, tile_h] = scale.art_size(mid.pixel_size[0], mid.pixel_size[1]);
        let tile_h = tile_h.round() as i32;
        for (y, h) in grip_tiles(thumb, tile_h) {
            let part = ChromeAtlasEntry {
                uv_size: [mid.uv_size[0], mid.uv_size[1] * h as f32 / tile_h as f32],
                ..mid
            };
            push_entry_sized(out, part, x as f32, y as f32, [tile_w, h as f32], depth);
        }
    }
    if let Some(top) = art.grip_top {
        push_entry_grown(out, top, x, thumb.y, scale, cap_depth);
    }
    if let Some(bottom) = art.grip_bottom {
        let h = scale.art_size(bottom.pixel_size[0], bottom.pixel_size[1])[1].round() as i32;
        push_entry_grown(out, bottom, x, thumb.y + thumb.h - h, scale, cap_depth);
    }
}

/// Emit the right-panel chrome (SDTP top / SDBTNBKGD tile column / SDBTM bottom
/// clipped) + lower strip, in C8 order, at `CHROME_DEPTH`. Identical for
/// 0xE2/0x100. `lower_strip` is `None` for a (future) shell with no lower strip;
/// both current shells always pass `Some`.
pub fn paint_chrome(
    atlas: &MainMenuShellChromeAtlas,
    panel: RightPanelRects,
    lower_strip: Option<RectPx>,
    screen_w: i32,
) -> Vec<SpriteInstance> {
    let mut out = Vec::new();
    let column = [
        atlas.right_panel_top_sdtp,
        atlas.right_panel_tile_sdbtnbkgd,
        None,
        atlas.right_panel_bottom_sdbtm,
    ];
    push_right_panel_column(&mut out, panel, column, [CHROME_DEPTH; 4]);
    if let Some(strip) = lower_strip {
        let lower_strip_entry = if screen_w == 640 {
            atlas.lower_side_640_lwscrns
        } else {
            atlas.lower_side_large_lwscrnl
        };
        if let Some(entry) = lower_strip_entry {
            push_entry(&mut out, entry, strip, CHROME_DEPTH);
        }
    }
    out
}

/// `RightPanel__Draw`'s column (`0x0072E450`): the SDTP top cap, the
/// SDBTNBKGD tile rows with the SDBTNANM frame-10 `overlay` right-aligned
/// over each when given, and the SDBTM bottom cap's top rows, at `depths`.
pub fn push_right_panel_column(
    out: &mut Vec<SpriteInstance>,
    panel: RightPanelRects,
    [top, tile, overlay, bottom]: [Option<ChromeAtlasEntry>; 4],
    [top_depth, tile_depth, overlay_depth, bottom_depth]: [f32; 4],
) {
    if let Some(top) = top {
        push_entry(out, top, panel.top, top_depth);
    }
    if let Some(tile) = tile {
        for row in 0..panel.tile_count {
            push_entry(out, tile, panel.tile_row(row), tile_depth);
        }
    }
    if let Some(overlay) = overlay {
        let [w, h] = overlay.pixel_size.map(|side| side.round() as i32);
        for row in 0..panel.tile_count {
            push_entry(out, overlay, panel.row_cell(row, w, h), overlay_depth);
        }
    }
    if let Some(bottom) = bottom {
        push_entry_top_clipped(out, bottom, panel.bottom, panel.stretch().1, bottom_depth);
    }
}

/// A `+0xD5` dialog's top-panel shapes over [`paint_chrome`]'s SDTP, in
/// draw order (later on top): the parent paint's SDTP frame 1
/// (`0x0072E8C0`) when steady, or the slide engine's SDTP and SDWRNTMP
/// draws of one tick ([`crate::ui::shell::slide::SlideColumn::panel_art`]).
pub(crate) fn paint_top_panel(
    atlas: &MainMenuShellChromeAtlas,
    panel: RightPanelRects,
    art: &[PanelArt],
) -> Vec<SpriteInstance> {
    let mut out = Vec::new();
    let entry = |art| match art {
        PanelArt::Sdtp(frame) => atlas.top_panel.sdtp.get(frame).copied().flatten(),
        PanelArt::Warning(frame) => atlas.top_panel.warning.get(frame).copied().flatten(),
        // No main-menu-atlas dialog has a map button or score art.
        PanelArt::MapButton(_) | PanelArt::Scrt(_) | PanelArt::ScrBackdrop => None,
    };
    push_panel_art(&mut out, panel, art, entry, CHROME_DEPTH - 0.00001);
    out
}

/// The slide engine's top-panel draws of one tick (`0x006071E0`) in draw
/// order, later on top: each shape `entry` resolves, stretched with the
/// panel and placed by [`panel_art_origin`], from `depth` forward.
pub(crate) fn push_panel_art(
    out: &mut Vec<SpriteInstance>,
    panel: RightPanelRects,
    art: &[PanelArt],
    entry: impl Fn(PanelArt) -> Option<ChromeAtlasEntry>,
    depth: f32,
) {
    let (scale_x, scale_y) = panel.stretch();
    for (index, &art) in art.iter().enumerate() {
        let Some(entry) = entry(art) else {
            continue;
        };
        let w = (entry.pixel_size[0] * scale_x).round() as i32;
        let h = (entry.pixel_size[1] * scale_y).round().max(1.0) as i32;
        let (x, y) = panel_art_origin(panel, art, w, h);
        push_entry(
            out,
            entry,
            RectPx::new(x, y, w, h),
            depth - index as f32 * 1e-6,
        );
    }
}

/// The slide engine's SDBTNANM draws (`0x006071E0`): each frame right-aligned
/// over its tile row of [`paint_chrome`]'s column, in draw order (later on
/// top), in place of the buttons.
pub(crate) fn paint_slide_column(
    atlas: &MainMenuShellChromeAtlas,
    panel: RightPanelRects,
    draws: &[ColumnDraw],
) -> Vec<SpriteInstance> {
    let mut out = Vec::new();
    push_column_frames(
        &mut out,
        panel,
        &atlas.button_wave_frames,
        draws,
        BUTTON_DEPTH,
    );
    out
}

/// Each slide draw's SDBTNANM `frames` entry in the cell its native canvas
/// occupies on its tile row, scaled like the button authored there, from
/// `depth` forward.
pub(crate) fn push_column_frames(
    out: &mut Vec<SpriteInstance>,
    panel: RightPanelRects,
    frames: &[Option<ChromeAtlasEntry>],
    draws: &[ColumnDraw],
    depth: f32,
) {
    for (index, draw) in draws.iter().enumerate() {
        let Some(entry) = frames.get(draw.frame).copied().flatten() else {
            continue;
        };
        let [w, h] = entry.pixel_size.map(|side| side.round() as i32);
        let cell = panel.row_cell(draw.row as i32, w, h);
        push_entry(out, entry, cell, depth - index as f32 * 1e-7);
    }
}

/// A family page's column button: SDBTNANM `frame` fitted to its cell
/// (`rect`), right-anchored and vertically centred, so a stretched cell
/// stretches the art with it; at the authored 156x42 cell it is the native
/// top-left draw. `None` when the atlas lacks the frame.
pub fn column_button(
    atlas: &MainMenuShellChromeAtlas,
    rect: RectPx,
    frame: usize,
    depth: f32,
) -> Option<SpriteInstance> {
    let entry = (*atlas.button_wave_frames.get(frame)?)?;
    use crate::ui::shell::geom::{SDBTNANM_CELL_H, SDBTNANM_CELL_W_NARROW};
    let sx = rect.w as f32 / SDBTNANM_CELL_W_NARROW as f32;
    let sy = rect.h as f32 / SDBTNANM_CELL_H as f32;
    let [w, h] = [entry.pixel_size[0] * sx, entry.pixel_size[1] * sy];
    let x = rect.x as f32 + (rect.w as f32 - w);
    let y = rect.y as f32 + (rect.h as f32 - h) * 0.5;
    Some(entry_sprite(entry, x, y, [w, h], depth))
}

/// `RightPanel__Draw` (`0x0072E450`) with its overlay flag clear, as the empty
/// shell backdrop (`0x0072E820(0)`, painted by `0x0052FEC0` between dialogs)
/// draws it: every tile row closed (SDBTNANM frame 10), the state a slide-out
/// ends in.
pub(crate) fn shuttered_column(panel: RightPanelRects) -> Vec<ColumnDraw> {
    (0..panel.tile_count.max(0) as u32)
        .map(|row| ColumnDraw { row, frame: 10 })
        .collect()
}

/// Static `0x71C`: SDWRNANM `frame` in the static's window. Kind-4 paint
/// (`0x0061595E..0x0061597E`) centers the shape only along an axis where the
/// window is larger, and the window clips it.
///
/// `scale` is the layout's stretch: center and clip in authored pixels first,
/// then map the resulting rectangle with the static panel artwork. `RETAIL_SCALE`
/// reproduces the native 1:1 draw exactly.
pub fn paint_warning_monitor(
    atlas: &MainMenuShellChromeAtlas,
    monitor: RectPx,
    frame: usize,
    scale: ShellScale,
) -> Option<SpriteInstance> {
    let entry = *atlas.warning_monitor_frames.get(frame)?;
    paint_scaled_warning_monitor(entry, monitor, scale)
}

fn paint_scaled_warning_monitor(
    entry: MainMenuShellChromeEntry,
    monitor: RectPx,
    scale: ShellScale,
) -> Option<SpriteInstance> {
    // 0x0061595E..0x0061597E truncates the native half-gap before drawing.
    // Centre in authored pixels, then scale with the static panel artwork.
    // Re-centering a 92-wide shape in the scaled 93-wide window adds a pixel
    // at 2x, making the animation shift relative to the baked-in monitor.
    // Below 1x, rounded device edges cannot recover the authored window.
    // Keep the existing downscaled path rather than inventing lost pixels.
    if scale.factor() < 1.0 {
        return paint_kind4_static(entry, monitor, scale);
    }
    let native_window = scale.base_rect(monitor);
    let mut sprite =
        paint_kind4_static(entry, native_window, crate::ui::shell::geom::RETAIL_SCALE)?;
    let drawn = scale.scale_rect(RectPx::new(
        sprite.position[0] as i32,
        sprite.position[1] as i32,
        sprite.size[0] as i32,
        sprite.size[1] as i32,
    ));
    sprite.position = [drawn.x as f32, drawn.y as f32];
    sprite.size = [drawn.w as f32, drawn.h as f32];
    Some(sprite)
}

/// The native kind-4 STATIC's centre-and-clip placement of one SHP frame in
/// its control window. Shared by the warning monitor and campaign score art.
pub fn paint_kind4_static(
    entry: MainMenuShellChromeEntry,
    window: RectPx,
    scale: ShellScale,
) -> Option<SpriteInstance> {
    let shape_w = (entry.pixel_size[0] * scale.factor()).round() as i32;
    let shape_h = (entry.pixel_size[1] * scale.factor()).round() as i32;
    if shape_w <= 0 || shape_h <= 0 {
        return None;
    }
    let x = window.x
        + if window.w > shape_w {
            (window.w - shape_w) / 2
        } else {
            0
        };
    let y = window.y
        + if window.h > shape_h {
            (window.h - shape_h) / 2
        } else {
            0
        };
    let draw_w = shape_w.min(window.w);
    let draw_h = shape_h.min(window.h);
    let part = ChromeAtlasEntry {
        uv_size: [
            entry.uv_size[0] * draw_w as f32 / shape_w as f32,
            entry.uv_size[1] * draw_h as f32 / shape_h as f32,
        ],
        ..entry
    };
    let size = [draw_w as f32, draw_h as f32];
    Some(entry_sprite(
        part,
        x as f32,
        y as f32,
        size,
        STATIC_IMAGE_DEPTH,
    ))
}

/// One text draw per label at `TEXT_DEPTH`, the glyphs grown by the layout's
/// `scale`. Color / inset / sink are pre-applied by the caller into each
/// `PaintLabel`.
pub fn paint_labels_scaled(
    font: &BitFont,
    labels: &[PaintLabel<'_>],
    scale: f32,
) -> Vec<ShellTextDraw> {
    paint_labels_at_depth_scaled(font, labels, TEXT_DEPTH, scale)
}

/// [`paint_labels_scaled`] at a caller-chosen depth (a modal's front-most).
pub fn paint_labels_at_depth_scaled(
    font: &BitFont,
    labels: &[PaintLabel<'_>],
    depth: f32,
    scale: f32,
) -> Vec<ShellTextDraw> {
    labels
        .iter()
        .map(|label| {
            let text_rect = crate::render::shell_text::TextRect::of(label.rect);
            match label.path_a_reveal {
                Some(reveal) => crate::render::shell_text::draw_in_rect_path_a_scaled(
                    font,
                    label.text.as_ref(),
                    text_rect,
                    label.align,
                    [0.0, 0.0],
                    depth,
                    reveal,
                    scale,
                ),
                None => crate::render::shell_text::draw_in_rect_scaled(
                    font,
                    label.text.as_ref(),
                    text_rect,
                    label.rgb,
                    label.align,
                    [0.0, 0.0],
                    depth,
                    scale,
                ),
            }
        })
        .collect()
}

// --- Slice 5: mode-2 SHP modal emitter (PUDLGBGN + MNBTTN + labels) ---

/// Sprite entries supplied by a modal's button policy. These are semantic
/// roles, not numbered SHP frames; native type3 callers bind released0/held1.
/// Missing art draws nothing rather than panicking.
#[derive(Clone, Copy, Default)]
pub struct ModalButtonFrames {
    pub up: Option<SkirmishShellChromeEntry>,
    pub disabled: Option<SkirmishShellChromeEntry>,
    pub pressed: Option<SkirmishShellChromeEntry>,
}

impl ModalButtonFrames {
    /// Select the caller-supplied sprite role. Eligibility is handled here;
    /// native SHP indices and timer highlighting belong to the caller's policy.
    pub fn select(self, pressed: bool, enabled: bool) -> Option<SkirmishShellChromeEntry> {
        if !enabled {
            self.disabled
        } else if pressed {
            self.pressed
        } else {
            self.up
        }
    }
}

/// Back-to-front depths for the mode-2 modal's three layers (background behind
/// button behind text). The caller supplies them so the modal slots into its host
/// shell's existing Z budget as a blocking overlay.
#[derive(Clone, Copy)]
pub struct ModalDepths {
    pub background: f32,
    pub button: f32,
    pub text: f32,
}

/// Resolved draw lists for a mode-2 SHP modal.
pub struct ModalDraw {
    pub sprites: Vec<SpriteInstance>,
    pub text: Vec<ShellTextDraw>,
}

/// One owner-draw button to paint in a mode-2 modal: its control rect plus the
/// resolved press/enable state. The message-box family shares ONE MNBTTN frame set
/// across all its buttons (OK/Cancel/third), so the buttons differ only by rect and
/// state — `0xCE` passes one, `0x120` two, `0x121` three.
#[derive(Clone, Copy)]
pub struct ModalButton {
    pub rect: RectPx,
    pub pressed: bool,
    pub enabled: bool,
}

/// Emit the mode-2 modal sprites: selected PUDLGBG background at the dialog
/// top-left, clipped by the dialog client (621E90 -> 4AED70), then each MNBTTN
/// button centered on its control rect at the state-selected frame. Both grow
/// with the layout's `scale`, including message boxes over a fitted shell.
/// A missing background is the caller's concern — only present
/// entries are emitted here, so the solid-panel fallback stays with the host
/// caller.
pub fn paint_modal_sprites(
    background: Option<SkirmishShellChromeEntry>,
    button_frames: ModalButtonFrames,
    dialog: RectPx,
    buttons: &[ModalButton],
    scale: ShellScale,
    depths: ModalDepths,
) -> Vec<SpriteInstance> {
    let mut out = Vec::new();
    if let Some(mut bg) = background {
        // An oversized mounted SHP is cropped by the client, never squeezed.
        let grown = scale.art_size(bg.pixel_size[0], bg.pixel_size[1]);
        let size = [
            grown[0].min(dialog.w.max(0) as f32),
            grown[1].min(dialog.h.max(0) as f32),
        ];
        bg.uv_size = [
            bg.uv_size[0] * size[0] / grown[0],
            bg.uv_size[1] * size[1] / grown[1],
        ];
        push_entry_sized(
            &mut out,
            bg,
            dialog.x as f32,
            dialog.y as f32,
            size,
            depths.background,
        );
    }
    for button in buttons {
        if let Some(entry) = button_frames.select(button.pressed, button.enabled) {
            let [x, y] = modal_button_centered_position(button.rect, entry, scale);
            push_entry_sized(
                &mut out,
                entry,
                x,
                y,
                scale.art_size(entry.pixel_size[0], entry.pixel_size[1]),
                depths.button,
            );
        }
    }
    out
}

/// Center the button art, grown by `scale`, on its control rect (matches the
/// existing modal button placement: integer-halved gaps on each axis).
fn modal_button_centered_position(
    rect: RectPx,
    entry: SkirmishShellChromeEntry,
    scale: ShellScale,
) -> [f32; 2] {
    let [art_w, art_h] = scale.art_size(entry.pixel_size[0], entry.pixel_size[1]);
    let (art_w, art_h) = (art_w.round() as i32, art_h.round() as i32);
    [
        (rect.x + (rect.w - art_w) / 2) as f32,
        (rect.y + (rect.h - art_h) / 2) as f32,
    ]
}

#[cfg(all(test, tests_render_s_z))]
mod tests {
    use super::*;
    use crate::ui::shell::geom::RIGHT_PANEL_WIDTH;

    fn fake_entry(w: f32, h: f32) -> MainMenuShellChromeEntry {
        MainMenuShellChromeEntry {
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            pixel_size: [w, h],
        }
    }

    fn one_sprite(push: impl Fn(&mut Vec<SpriteInstance>)) -> SpriteInstance {
        let mut out = Vec::new();
        push(&mut out);
        out.pop().expect("one instance")
    }

    /// The panel's drift factor is read off its own top cap: 199 native rows at
    /// every retail screen size, the scaled row count on a responsive panel.
    #[test]
    fn panel_stretch_reads_the_layout_not_the_screen() {
        // 1024x768 is 1.28x the 800x600 art, but a retail panel is letterboxed
        // beside that scale, not scaled by it.
        let retail = crate::ui::shell::geom::right_panel_rects(1024, 768);
        assert_eq!(retail.stretch(), (1.0, 1.0));

        let double = retail.scaled(crate::ui::shell::geom::shell_scale(1600, 1200));
        assert_eq!(double.stretch(), (2.0, 2.0));

        // The shell keeps its shape, so a window only taller or only wider than
        // 800x600 leaves the panel at its native size.
        for (w, h) in [(800, 1200), (1600, 600)] {
            let panel = retail.scaled(crate::ui::shell::geom::shell_scale(w, h));
            assert_eq!(panel.stretch(), (1.0, 1.0), "{w}x{h}");
            assert_eq!(panel.tile.w, RIGHT_PANEL_WIDTH, "{w}x{h}");
        }
    }

    /// SDBTM is a 168x65 SHP whose bottom cap shows 23 rows. Stretching the cap
    /// must show the same 23 source rows over a taller rect, not reveal more of
    /// the SHP or keep the rect at 23 px.
    #[test]
    fn clipped_top_stretches_the_destination_and_keeps_the_source_rows() {
        let entry = fake_entry(168.0, 65.0);
        let retail = one_sprite(|out| {
            push_entry_top_clipped(
                out,
                entry,
                RectPx::new(632, 577, 168, 23),
                1.0,
                CHROME_DEPTH,
            )
        });
        assert_eq!(retail.size, [168.0, 23.0]);
        assert_eq!(retail.uv_size[1], 23.0 / 65.0);

        let stretched = one_sprite(|out| {
            push_entry_top_clipped(
                out,
                entry,
                RectPx::new(1264, 1154, 336, 46),
                2.0,
                CHROME_DEPTH,
            )
        });
        assert_eq!(stretched.size, [336.0, 46.0]);
        assert_eq!(stretched.uv_size, retail.uv_size);
        assert_eq!(stretched.uv_origin, retail.uv_origin);
    }

    #[test]
    fn warning_monitor_centers_before_integer_upscaling() {
        // Native 0x71C window and SDWRNANM canvas: x half-gap truncates to
        // zero, y half-gap is one (0x0061595E..0x0061597E).
        let window = RectPx::new(670, 47, 93, 55);
        let entry = fake_entry(92.0, 53.0);
        let retail =
            paint_scaled_warning_monitor(entry, window, crate::ui::shell::geom::RETAIL_SCALE)
                .unwrap();
        assert_eq!(retail.position, [670.0, 48.0]);
        let scale = crate::ui::shell::geom::shell_scale(1600, 1200);
        let scaled = paint_scaled_warning_monitor(entry, scale.scale_rect(window), scale).unwrap();
        assert_eq!(scaled.position, [1340.0, 96.0]);
        assert_eq!(scaled.size, [184.0, 106.0]);
    }

    #[test]
    fn warning_monitor_preserves_retail_640_and_downscaled_placement() {
        let entry = fake_entry(92.0, 53.0);
        let retail = paint_scaled_warning_monitor(
            entry,
            RectPx::new(510, 47, 93, 55),
            crate::ui::shell::geom::RETAIL_SCALE,
        )
        .unwrap();
        assert_eq!(retail.position, [510.0, 48.0]);
        let scale = crate::ui::shell::geom::shell_scale(400, 300);
        let window = scale.scale_rect(RectPx::new(670, 47, 93, 55));
        let old = paint_kind4_static(entry, window, scale).unwrap();
        let new = paint_scaled_warning_monitor(entry, window, scale).unwrap();
        assert_eq!(new.position, old.position);
        assert_eq!(new.size, old.size);
    }

    #[test]
    fn warning_monitor_edges_follow_fractional_letterboxed_panel_scaling() {
        let scale = crate::ui::shell::geom::shell_scale(1920, 1080);
        let window = scale.scale_rect(RectPx::new(670, 47, 93, 55));
        let sprite = paint_scaled_warning_monitor(fake_entry(92.0, 53.0), window, scale).unwrap();
        assert_eq!(sprite.position, [1446.0, 86.0]);
        // Round the authored right/bottom edges, not the size separately.
        assert_eq!(sprite.size, [166.0, 96.0]);
    }

    /// `RETAIL_SCALE` leaves the monitor at its native shape, centre-and-clip
    /// included; a stretched layout grows the shape with its window.
    #[test]
    fn warning_monitor_shape_follows_the_layout_scale() {
        let entry = fake_entry(62.0, 44.0);
        let window = RectPx::new(640, 30, 200, 200);
        let retail =
            paint_kind4_static(entry, window, crate::ui::shell::geom::RETAIL_SCALE).expect("shape");
        assert_eq!(retail.size, [62.0, 44.0]);
        // Integer-halved gaps: (200-62)/2 right, (200-44)/2 down.
        assert_eq!(retail.position, [709.0, 108.0]);

        let stretched = paint_kind4_static(
            entry,
            window,
            crate::ui::shell::geom::shell_scale(1600, 1200),
        )
        .expect("shape");
        // 2x on both axes, then re-centred in the same window.
        assert_eq!(stretched.size, [124.0, 88.0]);
        assert_eq!(stretched.position, [678.0, 86.0]);
    }

    #[test]
    fn campaign_score_emblem_uses_kind4_centering_and_clips_to_static_client() {
        let layout = crate::ui::campaign_score_shell::CampaignScoreLayout::compute(
            Default::default(),
            800,
            600,
            0,
        );
        let window = layout
            .control_rect(crate::ui::campaign_score_shell::SCORE_ART_REGION_STATIC)
            .expect("campaign score art static");
        // 0x00608500 sizes the 0x72B window to the SCRA frame exactly, so the
        // kind-4 draw fills it without centering or clipping.
        assert_eq!(window, RectPx::new(104, 142, 424, 220));
        let sprite = paint_kind4_static(
            fake_entry(424.0, 220.0),
            window,
            crate::ui::shell::geom::RETAIL_SCALE,
        )
        .expect("kind-4 SCRA frame");

        assert_eq!(sprite.position, [104.0, 142.0]);
        assert_eq!(sprite.size, [424.0, 220.0]);
        assert_eq!(sprite.uv_size, [1.0, 1.0]);
    }

    fn fake_skirmish_entry(
        uv_origin: [f32; 2],
        uv_size: [f32; 2],
        pixel_size: [f32; 2],
    ) -> SkirmishShellChromeEntry {
        SkirmishShellChromeEntry {
            uv_origin,
            uv_size,
            pixel_size,
        }
    }

    const MODAL_DEPTHS: ModalDepths = ModalDepths {
        background: 0.5,
        button: 0.4,
        text: 0.3,
    };

    #[test]
    fn modal_labels_grow_with_the_panel_and_button_art() {
        let font = crate::render::bit_font::tests::make_test_font(&[(b'A' as u16, 6)], 4);
        let draw = |scale: ShellScale| {
            let rect = scale.scale_rect(RectPx::new(175, 137, 451, 326));
            let sprites = paint_modal_sprites(
                Some(fake_skirmish_entry([0.0; 2], [1.0; 2], [451.0, 326.0])),
                ModalButtonFrames::default(),
                rect,
                &[],
                scale,
                MODAL_DEPTHS,
            );
            let label = PaintLabel {
                text: "A".into(),
                rect,
                align: ShellAlign::NONE,
                rgb: SHELL_TEXT_RGB_ENABLED,
                path_a_reveal: None,
            };
            let text =
                paint_labels_at_depth_scaled(&font, &[label], MODAL_DEPTHS.text, scale.factor());
            ModalDraw { sprites, text }
        };
        let retail = draw(crate::ui::shell::geom::RETAIL_SCALE);
        let fitted = draw(crate::ui::shell::geom::shell_scale(1600, 1200));
        assert_eq!(fitted.sprites[0].size, [902.0, 652.0]);
        let glyph = &retail.text[0].instances[0];
        let grown = &fitted.text[0].instances[0];
        assert_eq!(grown.size, glyph.size.map(|v| v * 2.0));
        assert_eq!(grown.position, glyph.position.map(|v| v * 2.0));
        assert_eq!(fitted.text[0].scissor.w, retail.text[0].scissor.w * 2);
    }

    #[test]
    fn progress_background_is_clipped_to_original_dialog_client() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/saved_game_layout.json",
        ))
        .unwrap();
        let row = &corpus["progress_background"][0];
        let dialog: [i32; 4] = serde_json::from_value(row["input"]["dialog_rect"].clone()).unwrap();
        let native = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["event"] == "draw")
            .unwrap();
        let clip: [i32; 4] = serde_json::from_value(native["clip"].clone()).unwrap();
        // An oversized mounted SHP must be cropped, never scaled into the
        // client; native draw's supplied clip is independent of its canvas.
        let background = fake_skirmish_entry([0.1, 0.2], [0.5, 0.4], [500.0, 400.0]);
        let draw = paint_modal_sprites(
            Some(background),
            ModalButtonFrames::default(),
            RectPx::new(dialog[0], dialog[1], dialog[2], dialog[3]),
            &[],
            crate::ui::shell::geom::RETAIL_SCALE,
            MODAL_DEPTHS,
        );
        assert_eq!(draw.len(), 1);
        assert_eq!(draw[0].size, [clip[2] as f32, clip[3] as f32]);
        assert_eq!(draw[0].position, [dialog[0] as f32, dialog[1] as f32]);
        assert_eq!(draw[0].uv_origin, background.uv_origin);
        for axis in 0..2 {
            assert!(
                (draw[0].uv_size[axis] / draw[0].size[axis]
                    - background.uv_size[axis] / background.pixel_size[axis])
                    .abs()
                    < 1e-7
            );
        }
    }

    /// Pressed modal draws the background at the dialog top-left and the MNBTTN
    /// supplied held sprite centered on the OK control.
    #[test]
    fn modal_sprites_use_pressed_frame_centered_on_control() {
        let bg = fake_skirmish_entry([0.1, 0.1], [0.2, 0.2], [200.0, 120.0]);
        let up = fake_skirmish_entry([0.3, 0.0], [0.05, 0.05], [80.0, 20.0]);
        let pressed_frame = fake_skirmish_entry([0.6, 0.0], [0.05, 0.05], [80.0, 20.0]);
        let frames = ModalButtonFrames {
            up: Some(up),
            disabled: None,
            pressed: Some(pressed_frame),
        };
        let dialog = RectPx::new(300, 200, 200, 120);
        let ok = RectPx::new(360, 290, 90, 30);

        let sprites = paint_modal_sprites(
            Some(bg),
            frames,
            dialog,
            &[ModalButton {
                rect: ok,
                pressed: true,
                enabled: true,
            }],
            crate::ui::shell::geom::RETAIL_SCALE,
            MODAL_DEPTHS,
        );
        assert_eq!(sprites.len(), 2);
        // Background: authored size at dialog top-left, background depth.
        assert_eq!(sprites[0].position, [300.0, 200.0]);
        assert_eq!(sprites[0].size, [200.0, 120.0]);
        assert_eq!(sprites[0].uv_origin, bg.uv_origin);
        assert_eq!(sprites[0].depth, MODAL_DEPTHS.background);
        // Button: PRESSED frame uv proves frame 2 selected; centered on the OK rect.
        assert_eq!(sprites[1].uv_origin, pressed_frame.uv_origin);
        assert_eq!(
            sprites[1].position,
            [(360 + (90 - 80) / 2) as f32, (290 + (30 - 20) / 2) as f32]
        );
        assert_eq!(sprites[1].size, [80.0, 20.0]);
        assert_eq!(sprites[1].depth, MODAL_DEPTHS.button);
    }

    /// A stretched dialog grows the panel and its buttons with the page, staying
    /// centred on the control rects it is drawn into.
    #[test]
    fn modal_sprites_grow_with_a_stretched_page() {
        let bg = fake_skirmish_entry([0.1, 0.1], [0.2, 0.2], [200.0, 120.0]);
        let up = fake_skirmish_entry([0.3, 0.0], [0.05, 0.05], [80.0, 20.0]);
        let frames = ModalButtonFrames {
            up: Some(up),
            disabled: None,
            pressed: None,
        };
        let sprites = paint_modal_sprites(
            Some(bg),
            frames,
            RectPx::new(600, 400, 400, 240),
            &[ModalButton {
                rect: RectPx::new(720, 580, 180, 60),
                pressed: false,
                enabled: true,
            }],
            crate::ui::shell::geom::shell_scale(1600, 1200),
            MODAL_DEPTHS,
        );
        assert_eq!(sprites[0].size, [400.0, 240.0]);
        assert_eq!(sprites[1].size, [160.0, 40.0]);
        assert_eq!(sprites[1].position, [730.0, 590.0]);
    }

    /// Two buttons (OK + Cancel) share the frame set; each resolves its own state.
    #[test]
    fn modal_sprites_draw_multiple_buttons() {
        let up = fake_skirmish_entry([0.3, 0.0], [0.05, 0.05], [80.0, 20.0]);
        let pressed_frame = fake_skirmish_entry([0.6, 0.0], [0.05, 0.05], [80.0, 20.0]);
        let frames = ModalButtonFrames {
            up: Some(up),
            disabled: None,
            pressed: Some(pressed_frame),
        };
        // OK pressed, Cancel idle.
        let sprites = paint_modal_sprites(
            None,
            frames,
            RectPx::new(0, 0, 10, 10),
            &[
                ModalButton {
                    rect: RectPx::new(100, 100, 90, 30),
                    pressed: true,
                    enabled: true,
                },
                ModalButton {
                    rect: RectPx::new(100, 140, 90, 30),
                    pressed: false,
                    enabled: true,
                },
            ],
            crate::ui::shell::geom::RETAIL_SCALE,
            MODAL_DEPTHS,
        );
        assert_eq!(sprites.len(), 2);
        assert_eq!(sprites[0].uv_origin, pressed_frame.uv_origin); // OK pressed
        assert_eq!(sprites[1].uv_origin, up.uv_origin); // Cancel idle
    }
}
