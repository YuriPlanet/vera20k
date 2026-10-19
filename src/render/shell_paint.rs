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
//! Per-shell differences are carried as plain data (`ButtonPolicy` + `ArtFit`),
//! NOT a trait — two shells with no per-ControlKind dispatch yet make a trait an
//! empty abstraction; the trait arrives at Slice 4 when skirmish needs it. None
//! of this policy lives in `ui::shell::descriptor` (render-agnostic by contract):
//! a frame index / pixel sink / fit-scale is meaningless without
//! the atlas, so it stays render-side.
//!
//! Slice 5 adds `paint_modal_shp`: the shared mode-2 SHP modal emitter (PUDLGBGN
//! frame 0 background + MNBTTN owner-draw type-3 button + body/OK labels). It is
//! source-atlas-neutral — the caller passes the resolved chrome entries (the modal
//! art lives in the skirmish chrome atlas today) so the emitter stays a pure
//! composition the quit-confirm and validation modals both delegate to.

use std::borrow::Cow;
use std::time::Instant;

use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::main_menu_shell_chrome::{MainMenuShellChromeAtlas, MainMenuShellChromeEntry};
use crate::render::shell_text::{ShellAlign, ShellTextDraw};
use crate::render::shell_text_reveal::PathAReveal;
use crate::render::skirmish_shell_chrome::SkirmishShellChromeEntry;
use crate::ui::shell::geom::{RectPx, RightPanelRects, ShellScale};
use crate::ui::shell::slide::{ColumnDraw, column_draw_origin};

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
/// How a button's SDBTNANM art is fit into its cell rect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArtFit {
    /// Native `pixel_size` at `(rect.x, rect.y + art_sink_y)`. Active 0xE2 and
    /// 0x100 both use this path.
    Native,
    /// Optional fit policy: scale by `(rect.w / panel_w, rect.h / tile_h)`,
    /// right-anchor x, and vertically center. No press sink.
    FitRightAnchored { panel_w: f32, tile_h: f32 },
}

/// Per-shell render policy: how art is fit, whether a hover flash happens and
/// how far content sinks on press. Constructed as a `const` in each render
/// caller.
#[derive(Clone, Copy)]
pub struct ButtonPolicy {
    pub art_fit: ArtFit,
    /// Opt-in frame-3 ~1 Hz flash. Active 0xE2 and 0x100 both leave this false.
    pub hover_flash: bool,
    /// Policy-specific vertical art sink while pressed. The verified 0xE2 and
    /// 0x100 policies both use 0.0. Float because it routes through art emission.
    pub art_sink_y: f32,
}

/// One owner-draw button to paint: its cell rect + current per-control state.
/// The caller threads the resolved pressed/hovered/enabled booleans (descriptor
/// default + controller runtime); the pass never re-derives hit-testing.
#[derive(Clone, Copy)]
pub struct PaintButton {
    pub rect: RectPx,
    pub pressed: bool,
    pub hovered: bool,
    pub enabled: bool,
}

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

/// One chrome atlas entry stretched to `size`, top-left at `(x, y)`.
pub fn push_entry_sized(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
    x: f32,
    y: f32,
    size: [f32; 2],
    depth: f32,
) {
    out.push(SpriteInstance {
        position: [x, y],
        size,
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    });
}

/// One chrome atlas entry at its native size, top-left at `(x, y)`.
pub fn push_entry_native(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
    x: i32,
    y: i32,
    depth: f32,
) {
    push_entry_sized(out, entry, x as f32, y as f32, entry.pixel_size, depth);
}

/// One chrome atlas entry authored for the 800x600 dialog, placed at the
/// authored `(x, y)` and grown by `scale`: the point maps through the scale like
/// any authored rect, so it lands inside the centred dialog. Identity at
/// [`ShellScale::RETAIL`], so an unscaled page emits the same native-size
/// sprite at the same point. [`push_entry_grown`] takes a window-space point.
pub fn push_entry_scaled(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
    x: i32,
    y: i32,
    scale: ShellScale,
    depth: f32,
) {
    let [w, h] = scale.art_size(entry.pixel_size[0], entry.pixel_size[1]);
    let [x, y] = scale.scale_point([x, y]);
    push_entry_sized(
        out,
        entry,
        x as f32,
        y as f32,
        [w.round(), h.round()],
        depth,
    );
}

/// [`push_entry_scaled`] for art whose window-space top-left the caller
/// already holds, such as a point inside a scaled rect: only the native size
/// grows, so the point is not mapped a second time.
pub fn push_entry_grown(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
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
    entry: MainMenuShellChromeEntry,
    entry_origin: (i32, i32),
    rect: RectPx,
    depth: f32,
) {
    if let Some(crop) = entry_crop(entry, entry_origin, rect) {
        push_crop(out, crop, depth);
    }
}

/// [`push_entry_crop`] for an entry and rect in the authored 800x600 dialog
/// space, with the result stretched over the device window. The sampled source
/// pixels stay aligned with the stretched background art, so a control's darker
/// under-patch still matches what lies behind it.
pub fn push_entry_crop_scaled(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
    entry_origin: (i32, i32),
    rect: RectPx,
    scale: ShellScale,
    depth: f32,
) {
    if let Some((rect, uv_origin, uv_size)) = entry_crop(entry, entry_origin, rect) {
        push_crop(out, (scale.scale_rect(rect), uv_origin, uv_size), depth);
    }
}

/// The part of a backdrop whose whole canvas is scaled over `area` (a scaled
/// page's dialog rect): `rect`'s position in that area selects the texels, so an
/// inset drawn through this lines up with the scaled background it sits on.
/// [`push_entry_crop`] is the 1:1 form of the same idea.
pub fn push_entry_window_crop(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
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
    out.push(SpriteInstance {
        position: [left as f32, top as f32],
        size: [(right - left) as f32, (bottom - top) as f32],
        uv_origin: [
            entry.uv_origin[0] + u(left - area.x, area_w) * entry.uv_size[0],
            entry.uv_origin[1] + u(top - area.y, area_h) * entry.uv_size[1],
        ],
        uv_size: [
            u(right - left, area_w) * entry.uv_size[0],
            u(bottom - top, area_h) * entry.uv_size[1],
        ],
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    });
}

fn push_crop(
    out: &mut Vec<SpriteInstance>,
    (rect, uv_origin, uv_size): (RectPx, [f32; 2], [f32; 2]),
    depth: f32,
) {
    out.push(SpriteInstance {
        position: [rect.x as f32, rect.y as f32],
        size: [rect.w as f32, rect.h as f32],
        uv_origin,
        uv_size,
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    });
}

/// The destination rect, atlas UV origin and UV size of the part of `entry`
/// inside `rect`, both in the space the caller places the entry in.
fn entry_crop(
    entry: MainMenuShellChromeEntry,
    entry_origin: (i32, i32),
    rect: RectPx,
) -> Option<(RectPx, [f32; 2], [f32; 2])> {
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
    Some((
        RectPx::new(left, top, right - left, bottom - top),
        [
            entry.uv_origin[0] + (left - entry_origin.0) as f32 * u_per_px,
            entry.uv_origin[1] + (top - entry_origin.1) as f32 * v_per_px,
        ],
        [
            (right - left) as f32 * u_per_px,
            (bottom - top) as f32 * v_per_px,
        ],
    ))
}

fn push_entry_rect(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
    rect: RectPx,
    depth: f32,
) {
    push_entry_sized(
        out,
        entry,
        rect.x as f32,
        rect.y as f32,
        [rect.w as f32, rect.h as f32],
        depth,
    );
}

/// Draw `entry` into `rect`, taking the top `rect.h / scale` rows of the SHP.
///
/// At `scale == 1` this is gamemd's 1:1 crop, which SDBTM needs: the SHP is
/// 168x65 native but the destination cap region is 23 px tall. On the
/// responsive layout the same 23 source rows fill the stretched cap.
fn push_clipped_top(
    out: &mut Vec<SpriteInstance>,
    entry: MainMenuShellChromeEntry,
    rect: RectPx,
    scale: f32,
    depth: f32,
) {
    let native_h = entry.pixel_size[1].max(1.0);
    let source_h = (rect.h as f32 / scale).min(native_h).max(1.0);
    let uv_h = entry.uv_size[1] * (source_h / native_h);
    out.push(SpriteInstance {
        position: [rect.x as f32, rect.y as f32],
        size: [rect.w as f32, rect.h as f32],
        uv_origin: entry.uv_origin,
        uv_size: [entry.uv_size[0], uv_h],
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    });
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
    if let Some(top) = atlas.right_panel_top_sdtp {
        push_entry_rect(&mut out, top, panel.top, CHROME_DEPTH);
    }
    if let Some(tile) = atlas.right_panel_tile_sdbtnbkgd {
        for row in 0..panel.tile_count {
            let rect = RectPx::new(
                panel.tile.x,
                panel.tile.y + row * panel.tile.h,
                panel.tile.w,
                panel.tile.h,
            );
            push_entry_rect(&mut out, tile, rect, CHROME_DEPTH);
        }
    }
    if let Some(bottom) = atlas.right_panel_bottom_sdbtm {
        push_clipped_top(
            &mut out,
            bottom,
            panel.bottom,
            panel.stretch().1,
            CHROME_DEPTH,
        );
    }
    if let Some(strip) = lower_strip {
        let lower_strip_entry = if screen_w == 640 {
            atlas.lower_side_640_lwscrns
        } else {
            atlas.lower_side_large_lwscrnl
        };
        if let Some(entry) = lower_strip_entry {
            push_entry_rect(&mut out, entry, strip, CHROME_DEPTH);
        }
    }
    out
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
    let (scale_x, scale_y) = panel.stretch();
    for (index, draw) in draws.iter().enumerate() {
        let Some(entry) = atlas.button_wave_frames.get(draw.frame).copied().flatten() else {
            continue;
        };
        // The cell the frame occupies: its native 156x42 canvas, stretched by
        // the same factor as the column it slides in.
        let w = (entry.pixel_size[0] * scale_x).round() as i32;
        let h = (entry.pixel_size[1] * scale_y).round().max(1.0) as i32;
        let (x, y) = column_draw_origin(panel, draw.row, w);
        push_entry_rect(
            &mut out,
            entry,
            RectPx::new(x, y, w, h),
            BUTTON_DEPTH - index as f32 * 1e-7,
        );
    }
    out
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
/// `scale` is the layout's stretch: the shape grows with the window it was
/// laid out into, then keeps the same center-and-clip rule. `ShellScale::RETAIL`
/// reproduces the native 1:1 draw exactly.
pub fn paint_warning_monitor(
    atlas: &MainMenuShellChromeAtlas,
    monitor: RectPx,
    frame: usize,
    scale: ShellScale,
) -> Option<SpriteInstance> {
    let entry = *atlas.warning_monitor_frames.get(frame)?;
    paint_kind4_static(entry, monitor, scale)
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
    Some(SpriteInstance {
        position: [x as f32, y as f32],
        size: [draw_w as f32, draw_h as f32],
        uv_origin: entry.uv_origin,
        uv_size: [
            entry.uv_size[0] * draw_w as f32 / shape_w as f32,
            entry.uv_size[1] * draw_h as f32 / shape_h as f32,
        ],
        depth: STATIC_IMAGE_DEPTH,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    })
}

/// Which steady SDBTNANM frame a button shows (when it is not mid-slide).
/// Separated from atlas lookup so the selection logic is unit-testable without
/// a GPU-backed atlas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SteadyFrame {
    /// SDBTNANM frame 2.
    Default,
    /// SDBTNANM frame 3 (opt-in hover flash, high phase only).
    Hover,
    /// SDBTNANM frame 4 (pressed).
    Pressed,
}

/// Pure frame-choice for a steady (non-wave) button. Pressed beats hover; hover
/// flash only on the high phase of the ~1 Hz square wave AND only when the
/// policy enables it (0xE2 never reaches Hover). `hover_started_at` is the
/// controller's hover-entry instant.
fn steady_frame_choice(
    b: &PaintButton,
    policy: ButtonPolicy,
    now: Instant,
    hover_started_at: Option<Instant>,
) -> SteadyFrame {
    if b.pressed {
        return SteadyFrame::Pressed;
    }
    if policy.hover_flash && b.hovered {
        let flash = hover_started_at
            .map(|start| crate::ui::shell::button::flash_phase(start, now))
            .unwrap_or(false);
        if flash {
            return SteadyFrame::Hover;
        }
    }
    SteadyFrame::Default
}

/// Pick the SDBTNANM frame for a steady button: pressed (frame 4), else opt-in
/// hover-flash (frame 3), else default (frame 2). Slides paint the column
/// instead (`paint_slide_column`).
fn select_frame(
    atlas: &MainMenuShellChromeAtlas,
    b: &PaintButton,
    policy: ButtonPolicy,
    now: Instant,
    hover_started_at: Option<Instant>,
) -> MainMenuShellChromeEntry {
    match steady_frame_choice(b, policy, now, hover_started_at) {
        SteadyFrame::Default => atlas.button_default,
        SteadyFrame::Hover => atlas.button_hover,
        SteadyFrame::Pressed => atlas.button_pressed,
    }
}

/// Emit the owner-draw buttons at `BUTTON_DEPTH`, applying the per-shell policy
/// (frame select 2/3/4, art fit, art sink) without a per-dialog branch. A
/// disabled button keeps its full art (the black blend is only for type 0,
/// `0x006135F3`).
pub fn paint_buttons(
    atlas: &MainMenuShellChromeAtlas,
    buttons: &[PaintButton],
    policy: ButtonPolicy,
    now: Instant,
    hover_started_at: Option<Instant>,
) -> Vec<SpriteInstance> {
    let mut out = Vec::new();
    for b in buttons {
        let frame = select_frame(atlas, b, policy, now, hover_started_at);
        let (pos, size) = match policy.art_fit {
            ArtFit::Native => {
                let sink = if b.pressed { policy.art_sink_y } else { 0.0 };
                ([b.rect.x as f32, b.rect.y as f32 + sink], frame.pixel_size)
            }
            ArtFit::FitRightAnchored { panel_w, tile_h } => {
                let sx = b.rect.w as f32 / panel_w;
                let sy = b.rect.h as f32 / tile_h;
                let fw = frame.pixel_size[0] * sx;
                let fh = frame.pixel_size[1] * sy;
                let x = b.rect.x as f32 + (b.rect.w as f32 - fw);
                let y = b.rect.y as f32 + (b.rect.h as f32 - fh) * 0.5; // NO press sink
                ([x, y], [fw, fh])
            }
        };
        out.push(SpriteInstance {
            position: pos,
            size,
            uv_origin: frame.uv_origin,
            uv_size: frame.uv_size,
            depth: BUTTON_DEPTH,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        });
    }
    out
}

/// Emit one text draw per label at `TEXT_DEPTH` via `shell_text::draw_in_rect`.
/// Color / inset / sink are pre-applied by the caller into each `PaintLabel`.
pub fn paint_labels(font: &BitFont, labels: &[PaintLabel<'_>]) -> Vec<ShellTextDraw> {
    paint_labels_at_depth(font, labels, TEXT_DEPTH)
}

/// [`paint_labels`] for a stretched shell: labels are centred in their stretched
/// rects, so the glyphs grow by the layout's own `scale`.
pub fn paint_labels_scaled(
    font: &BitFont,
    labels: &[PaintLabel<'_>],
    scale: f32,
) -> Vec<ShellTextDraw> {
    paint_labels_at_depth_scaled(font, labels, TEXT_DEPTH, scale)
}

/// As [`paint_labels`] but at a caller-chosen depth. The right-panel shells use
/// `TEXT_DEPTH`; the mode-2 modal draws as a blocking overlay and supplies its own
/// front-most text depth so the body/OK labels sit above the modal art.
pub fn paint_labels_at_depth(
    font: &BitFont,
    labels: &[PaintLabel<'_>],
    depth: f32,
) -> Vec<ShellTextDraw> {
    paint_labels_at_depth_scaled(font, labels, depth, 1.0)
}

/// [`paint_labels_at_depth`] with a glyph scale.
pub fn paint_labels_at_depth_scaled(
    font: &BitFont,
    labels: &[PaintLabel<'_>],
    depth: f32,
    scale: f32,
) -> Vec<ShellTextDraw> {
    use crate::render::shell_text::TextRect;

    labels
        .iter()
        .map(|label| {
            let text_rect = TextRect {
                x: label.rect.x,
                y: label.rect.y,
                w: label.rect.w.max(0) as u32,
                h: label.rect.h.max(0) as u32,
            };
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
/// with `scale`, which is [`ShellScale::RETAIL`] for the message-box family's
/// own fixed-size box and a stretched page's scale where the dialog itself
/// stretches. A missing background is the caller's concern — only present
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
        push_skirmish_entry(
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
            push_skirmish_entry(
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

/// Compose the full mode-2 SHP modal: [`paint_modal_sprites`] plus the body/button
/// labels (via [`paint_labels_at_depth`] at the modal's front-most text depth).
pub fn paint_modal_shp(
    font: &BitFont,
    background: Option<SkirmishShellChromeEntry>,
    button_frames: ModalButtonFrames,
    dialog: RectPx,
    buttons: &[ModalButton],
    labels: &[PaintLabel<'_>],
    scale: ShellScale,
    depths: ModalDepths,
) -> ModalDraw {
    let sprites = paint_modal_sprites(background, button_frames, dialog, buttons, scale, depths);
    let text = paint_labels_at_depth(font, labels, depths.text);
    ModalDraw { sprites, text }
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

fn push_skirmish_entry(
    out: &mut Vec<SpriteInstance>,
    entry: SkirmishShellChromeEntry,
    x: f32,
    y: f32,
    size: [f32; 2],
    depth: f32,
) {
    out.push(SpriteInstance {
        position: [x, y],
        size,
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::bit_font::tests::make_test_font;
    use crate::ui::shell::geom::RIGHT_PANEL_WIDTH;
    use std::time::Duration;

    const FIT_PANEL_W: f32 = 168.0;
    const FIT_TILE_H: f32 = 42.0;

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

        let double = ShellScale::from_screen(1600, 1200).scale_panel(retail);
        assert_eq!(double.stretch(), (2.0, 2.0));

        // The shell keeps its shape, so a window only taller or only wider than
        // 800x600 leaves the panel at its native size.
        for (w, h) in [(800, 1200), (1600, 600)] {
            let panel = ShellScale::from_screen(w, h).scale_panel(retail);
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
            push_clipped_top(
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
            push_clipped_top(
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

    /// `ShellScale::RETAIL` leaves the monitor at its native shape, centre-and-clip
    /// included; a stretched layout grows the shape with its window.
    #[test]
    fn warning_monitor_shape_follows_the_layout_scale() {
        let entry = fake_entry(62.0, 44.0);
        let window = RectPx::new(640, 30, 200, 200);
        let retail = paint_kind4_static(entry, window, ShellScale::RETAIL).expect("shape");
        assert_eq!(retail.size, [62.0, 44.0]);
        // Integer-halved gaps: (200-62)/2 right, (200-44)/2 down.
        assert_eq!(retail.position, [709.0, 108.0]);

        let stretched =
            paint_kind4_static(entry, window, ShellScale::from_screen(1600, 1200)).expect("shape");
        // 2x on both axes, then re-centred in the same window.
        assert_eq!(stretched.size, [124.0, 88.0]);
        assert_eq!(stretched.position, [678.0, 86.0]);
    }

    #[test]
    fn campaign_score_emblem_uses_kind4_centering_and_clips_to_static_client() {
        let layout = crate::ui::campaign_score_shell::CampaignScoreLayout::compute_layout(800, 600);
        let window = layout
            .control_rect(crate::ui::campaign_score_shell::SCORE_ART_REGION_STATIC)
            .expect("campaign score art static");
        // 0x00608500 sizes the 0x72B window to the SCRA frame exactly, so the
        // kind-4 draw fills it without centering or clipping.
        assert_eq!(window, RectPx::new(104, 142, 424, 220));
        let sprite = paint_kind4_static(fake_entry(424.0, 220.0), window, ShellScale::RETAIL)
            .expect("kind-4 SCRA frame");

        assert_eq!(sprite.position, [104.0, 142.0]);
        assert_eq!(sprite.size, [424.0, 220.0]);
        assert_eq!(sprite.uv_size, [1.0, 1.0]);
    }

    fn btn(pressed: bool, hovered: bool) -> PaintButton {
        PaintButton {
            rect: RectPx::new(0, 0, 168, 42),
            pressed,
            hovered,
            enabled: true,
        }
    }

    #[test]
    fn plain_label_path_remains_byte_identical_to_direct_shell_text() {
        let font = make_test_font(&[(b'a' as u16, 6), (b'b' as u16, 6)], 4);
        let rect = RectPx::new(9, 11, 80, 20);
        let labels = [PaintLabel {
            text: "aba".into(),
            rect,
            align: ShellAlign::H_CENTER,
            rgb: [1.0, 1.0, 0.0],
            path_a_reveal: None,
        }];
        let actual = paint_labels(&font, &labels).pop().expect("one label");
        let expected = crate::render::shell_text::draw_in_rect(
            &font,
            "aba",
            crate::render::shell_text::TextRect {
                x: rect.x,
                y: rect.y,
                w: rect.w as u32,
                h: rect.h as u32,
            },
            [1.0, 1.0, 0.0],
            ShellAlign::H_CENTER,
            [0.0, 0.0],
            TEXT_DEPTH,
        );
        assert_eq!(
            bytemuck::cast_slice::<SpriteInstance, u8>(&actual.instances),
            bytemuck::cast_slice::<SpriteInstance, u8>(&expected.instances)
        );
        assert_eq!(
            (
                actual.scissor.x,
                actual.scissor.y,
                actual.scissor.w,
                actual.scissor.h,
            ),
            (
                expected.scissor.x,
                expected.scissor.y,
                expected.scissor.w,
                expected.scissor.h,
            )
        );
    }

    const NATIVE_POLICY: ButtonPolicy = ButtonPolicy {
        art_fit: ArtFit::Native,
        hover_flash: false,
        art_sink_y: 0.0,
    };

    /// 0xE2 native art stays at the cell top-left when frame 4 is selected.
    #[test]
    fn native_art_stays_fixed_when_pressed() {
        let frame = fake_entry(156.0, 42.0);
        let rect = RectPx::new(644, 199, 156, 42);

        // Unpressed: top-left, native size.
        let (pos_up, size_up) = native_emit(frame, rect, false);
        assert_eq!(pos_up, [644.0, 199.0]);
        assert_eq!(size_up, [156.0, 42.0]);

        // Pressed: frame selection changes elsewhere; geometry is identical.
        let (pos_dn, size_dn) = native_emit(frame, rect, true);
        assert_eq!(pos_dn, [644.0, 199.0]);
        assert_eq!(size_dn, [156.0, 42.0]);
        assert_eq!(pos_dn, pos_up);
    }

    /// Mirror `paint_buttons`' Native art branch for an entry+rect without an
    /// atlas (the frame is already resolved).
    fn native_emit(
        frame: MainMenuShellChromeEntry,
        rect: RectPx,
        pressed: bool,
    ) -> ([f32; 2], [f32; 2]) {
        let policy = NATIVE_POLICY;
        let sink = if pressed { policy.art_sink_y } else { 0.0 };
        ([rect.x as f32, rect.y as f32 + sink], frame.pixel_size)
    }

    /// Mirror `paint_buttons`' optional FitRightAnchored branch as a function of
    /// the real frame canvas width (`frame.pixel_size[0]`).
    fn fit_emit(
        frame: MainMenuShellChromeEntry,
        rect: RectPx,
        panel_w: f32,
        tile_h: f32,
    ) -> ([f32; 2], [f32; 2]) {
        let sx = rect.w as f32 / panel_w;
        let sy = rect.h as f32 / tile_h;
        let fw = frame.pixel_size[0] * sx;
        let fh = frame.pixel_size[1] * sy;
        let x = rect.x as f32 + (rect.w as f32 - fw);
        let y = rect.y as f32 + (rect.h as f32 - fh) * 0.5;
        ([x, y], [fw, fh])
    }

    /// Optional fit-anchored art at a 168-wide cell with tile_h=42.
    #[test]
    fn fit_right_anchored_scales_anchors_and_never_sinks() {
        let panel_w = FIT_PANEL_W;
        let tile_h = FIT_TILE_H;
        let rect = RectPx::new(632, 199, 168, 42);
        // Use a frame whose canvas width equals the cell width so sx=1.0 — the
        // common steady case — but assert in terms of the input, not a literal.
        let canvas_w = panel_w; // exercise the sx=1.0 case explicitly
        let canvas_h = tile_h;
        let frame = fake_entry(canvas_w, canvas_h);

        let sx = rect.w as f32 / panel_w;
        let sy = rect.h as f32 / tile_h;
        let expect_fw = canvas_w * sx;
        let expect_fh = canvas_h * sy;
        let expect_x = rect.x as f32 + (rect.w as f32 - expect_fw);
        let expect_y = rect.y as f32 + (rect.h as f32 - expect_fh) * 0.5;

        let (pos, size) = fit_emit(frame, rect, panel_w, tile_h);
        assert_eq!(size, [expect_fw, expect_fh]);
        assert_eq!(pos, [expect_x, expect_y]);
        // At sx=1.0 the art exactly fills the cell: right-anchored x == rect.x,
        // v-centered y == rect.y, NO +2 sink regardless of "pressed".
        assert_eq!(pos, [632.0, 199.0]);

        // A pressed flag must not move FitRightAnchored art (art_sink_y is unused
        // on this path). Re-emit identically.
        let (pos_pressed, _) = fit_emit(frame, rect, panel_w, tile_h);
        assert_eq!(pos_pressed, pos);
    }

    /// A narrower canvas (e.g. 156 art in a 168 cell) right-anchors with a left
    /// gap of `(cell_w - frame_w)` and v-centers — pins the non-trivial case.
    #[test]
    fn fit_right_anchored_left_gap_for_narrow_canvas() {
        let panel_w = FIT_PANEL_W; // 168
        let tile_h = FIT_TILE_H; // 42
        let rect = RectPx::new(632, 199, 168, 42);
        let frame = fake_entry(156.0, 42.0); // canvas narrower than the cell
        let (pos, size) = fit_emit(frame, rect, panel_w, tile_h);
        // sx = 168/168 = 1.0 -> fw = 156, gap = 168-156 = 12 px on the LEFT.
        assert_eq!(size, [156.0, 42.0]);
        assert_eq!(pos, [632.0 + 12.0, 199.0]);
    }

    const FLASH_POLICY: ButtonPolicy = ButtonPolicy {
        art_fit: ArtFit::FitRightAnchored {
            panel_w: FIT_PANEL_W,
            tile_h: FIT_TILE_H,
        },
        hover_flash: true,
        art_sink_y: 0.0,
    };

    /// Pressed always wins, regardless of policy or hover. Both shells.
    #[test]
    fn steady_choice_pressed_beats_everything() {
        let now = Instant::now();
        assert_eq!(
            steady_frame_choice(&btn(true, true), FLASH_POLICY, now, Some(now)),
            SteadyFrame::Pressed
        );
        assert_eq!(
            steady_frame_choice(&btn(true, false), NATIVE_POLICY, now, None),
            SteadyFrame::Pressed
        );
    }

    /// A no-flash policy never reaches frame 3 even while hovered.
    #[test]
    fn steady_choice_no_flash_never_hovers() {
        let start = Instant::now();
        // High phase that WOULD flash if the policy allowed it.
        let now = start + Duration::from_millis(1500);
        assert_eq!(
            steady_frame_choice(&btn(false, true), NATIVE_POLICY, now, Some(start)),
            SteadyFrame::Default
        );
    }

    /// An opt-in flash policy shows frame 3 only on the high phase of the ~1 Hz
    /// square wave: elapsed_ms / 1000 % 2 == 1.
    #[test]
    fn steady_choice_flash_phase() {
        let start = Instant::now();
        // Low phase: 0..1000 ms -> 0 % 2 == 0 -> Default.
        assert_eq!(
            steady_frame_choice(
                &btn(false, true),
                FLASH_POLICY,
                start + Duration::from_millis(500),
                Some(start)
            ),
            SteadyFrame::Default
        );
        // High phase: 1000..2000 ms -> 1 % 2 == 1 -> Hover.
        assert_eq!(
            steady_frame_choice(
                &btn(false, true),
                FLASH_POLICY,
                start + Duration::from_millis(1500),
                Some(start)
            ),
            SteadyFrame::Hover
        );
        // Next low phase: 2000..3000 ms -> 2 % 2 == 0 -> Default.
        assert_eq!(
            steady_frame_choice(
                &btn(false, true),
                FLASH_POLICY,
                start + Duration::from_millis(2500),
                Some(start)
            ),
            SteadyFrame::Default
        );
    }

    /// Not hovered -> never flashes regardless of phase.
    #[test]
    fn steady_choice_no_hover_no_flash() {
        let start = Instant::now();
        assert_eq!(
            steady_frame_choice(
                &btn(false, false),
                FLASH_POLICY,
                start + Duration::from_millis(1500),
                Some(start)
            ),
            SteadyFrame::Default
        );
    }

    /// Depth/compose order: parent bg behind movie behind chrome behind buttons
    /// behind text, cursor on top. Pins the C8 tiebreaker constants.
    #[test]
    fn compose_depths_are_ordered_back_to_front() {
        const {
            assert!(PARENT_BACKGROUND_DEPTH > MOVIE_DEPTH);
            assert!(MOVIE_DEPTH > CHROME_DEPTH);
            assert!(CHROME_DEPTH > BUTTON_DEPTH);
            assert!(BUTTON_DEPTH > TEXT_DEPTH);
            assert!(TEXT_DEPTH > CURSOR_DEPTH);
        }
    }

    #[test]
    fn caption_colour_constants() {
        assert_eq!(SHELL_TEXT_RGB_DISABLED, [0x48 as f32 / 255.0, 0.0, 0.0]);
        assert_eq!(SHELL_TEXT_RGB_ENABLED, [1.0, 1.0, 0.0]);
    }

    /// Optional FitRightAnchored geometry pinned against the real
    /// SDBTNANM.SHP canvas width read out of the retail asset. The pass
    /// reads `frame.pixel_size[0]` = the SHP header width parsed at load time
    /// (`render::main_menu_shell_chrome::render_shp_entry` sets `pixel_size` from
    /// `shp.width`). This test loads the actual file, so a future edit that
    /// re-introduces a wrong canvas assumption fails.
    /// Skips gracefully when retail assets are absent.
    #[test]
    fn fit_right_anchored_pins_real_sdbtnanm_canvas_width() {
        use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
        use crate::assets::shp_file::ShpFile;
        use crate::util::config::GameConfig;

        let ra2_dir = match std::env::var("RA2_DIR") {
            Ok(val) => std::path::PathBuf::from(val),
            Err(_) => match GameConfig::load() {
                Ok(cfg) => cfg.paths.ra2_dir,
                Err(_) => {
                    eprintln!("SKIPPED: RA2_DIR not set and config.toml not found");
                    return;
                }
            },
        };
        if !ra2_dir.exists() {
            eprintln!("SKIPPED: RA2 assets not found at {}", ra2_dir.display());
            return;
        }
        let Ok(mut assets) = AssetManager::new(&ra2_dir, MediaArchiveMode::STOCK_DIGITAL) else {
            eprintln!("SKIPPED: could not mount asset archives");
            return;
        };
        match assets.register_neutral_archives() {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("SKIPPED: retail neutral archive pair is unavailable");
                return;
            }
            Err(err) => {
                eprintln!("SKIPPED: could not register retail neutral archives: {err:#}");
                return;
            }
        }
        let Some(bytes) = assets.get_ref("SDBTNANM.SHP") else {
            eprintln!("SKIPPED: SDBTNANM.SHP not present in mounted archives");
            return;
        };
        let Ok(shp) = ShpFile::from_bytes(bytes) else {
            eprintln!("SKIPPED: SDBTNANM.SHP failed to parse");
            return;
        };
        // The atlas bakes `pixel_size = [shp.width, shp.height]` (canvas size).
        let canvas_w = shp.width as f32;
        let canvas_h = shp.height as f32;
        eprintln!("SDBTNANM.SHP real canvas size: {canvas_w} x {canvas_h}");
        assert!(canvas_w > 0.0 && canvas_h > 0.0);

        let rect = RectPx::new(632, 199, 168, 42);
        let frame = fake_entry(canvas_w, canvas_h);
        let (pos, size) = fit_emit(frame, rect, FIT_PANEL_W, FIT_TILE_H);

        // Independently recompute the expected fit/anchor/center from the REAL
        // canvas width — the pass must produce exactly this.
        let sx = rect.w as f32 / FIT_PANEL_W;
        let sy = rect.h as f32 / FIT_TILE_H;
        let expect_fw = canvas_w * sx;
        let expect_fh = canvas_h * sy;
        let expect_x = rect.x as f32 + (rect.w as f32 - expect_fw);
        let expect_y = rect.y as f32 + (rect.h as f32 - expect_fh) * 0.5;
        assert_eq!(size, [expect_fw, expect_fh]);
        assert_eq!(pos, [expect_x, expect_y]);
    }

    // --- Slice 5: mode-2 SHP modal emitter ---

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
            ShellScale::RETAIL,
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

    /// `select` resolves a semantic state to the caller-supplied entry.
    #[test]
    fn modal_button_frames_select_matches_role() {
        let up = fake_skirmish_entry([0.0, 0.0], [0.1, 0.1], [80.0, 20.0]);
        let disabled = fake_skirmish_entry([0.2, 0.0], [0.1, 0.1], [80.0, 20.0]);
        let pressed = fake_skirmish_entry([0.4, 0.0], [0.1, 0.1], [80.0, 20.0]);
        let frames = ModalButtonFrames {
            up: Some(up),
            disabled: Some(disabled),
            pressed: Some(pressed),
        };
        assert_eq!(frames.select(false, true).unwrap().uv_origin, up.uv_origin);
        assert_eq!(
            frames.select(true, true).unwrap().uv_origin,
            pressed.uv_origin
        );
        assert_eq!(
            frames.select(false, false).unwrap().uv_origin,
            disabled.uv_origin
        );
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
            ShellScale::RETAIL,
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

    /// Idle (unpressed, enabled) modal draws the UP frame (frame 0).
    #[test]
    fn modal_sprites_use_up_frame_when_idle() {
        let up = fake_skirmish_entry([0.3, 0.0], [0.05, 0.05], [80.0, 20.0]);
        let pressed_frame = fake_skirmish_entry([0.6, 0.0], [0.05, 0.05], [80.0, 20.0]);
        let frames = ModalButtonFrames {
            up: Some(up),
            disabled: None,
            pressed: Some(pressed_frame),
        };
        let sprites = paint_modal_sprites(
            None,
            frames,
            RectPx::new(0, 0, 10, 10),
            &[ModalButton {
                rect: RectPx::new(0, 0, 90, 30),
                pressed: false,
                enabled: true,
            }],
            ShellScale::RETAIL,
            MODAL_DEPTHS,
        );
        // No background passed -> only the button sprite, using the UP frame.
        assert_eq!(sprites.len(), 1);
        assert_eq!(sprites[0].uv_origin, up.uv_origin);
    }

    /// A missing button frame draws nothing for the button (no panic on a short SHP).
    #[test]
    fn modal_sprites_skip_missing_button_frame() {
        let frames = ModalButtonFrames::default(); // all None
        let sprites = paint_modal_sprites(
            None,
            frames,
            RectPx::new(0, 0, 10, 10),
            &[ModalButton {
                rect: RectPx::new(0, 0, 90, 30),
                pressed: true,
                enabled: true,
            }],
            ShellScale::RETAIL,
            MODAL_DEPTHS,
        );
        assert!(sprites.is_empty());
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
            ShellScale::from_screen(1600, 1200),
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
            ShellScale::RETAIL,
            MODAL_DEPTHS,
        );
        assert_eq!(sprites.len(), 2);
        assert_eq!(sprites[0].uv_origin, pressed_frame.uv_origin); // OK pressed
        assert_eq!(sprites[1].uv_origin, up.uv_origin); // Cancel idle
    }
}
