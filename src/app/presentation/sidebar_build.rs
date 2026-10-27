//! Sidebar sprite instance builders — slots, chrome, cameos, text, and placeholders.
//!
//! Builds the per-frame SpriteInstance vectors for each sidebar layer:
//! background rectangles, chrome art, cameo icons, text labels, progress bars.
//!
//! Split from `presentation::sidebar_render` to keep files under 400 lines.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::presentation::sidebar_render::current_sidebar_chrome;
use crate::render::batch::SpriteInstance;
use crate::render::sidebar_chrome::{SidebarChromeAtlas, SidebarChromeEntry};
use crate::ui::sidebar::power_bar_anim::PowerBarAnimState;
use crate::ui::sidebar::{
    Rect, SidebarChromeLayoutSpec, SidebarLayout, SidebarTabButton, SidebarView,
};

#[path = "sidebar_command_bar.rs"]
pub(crate) mod command_bar;

// ---------------------------------------------------------------------------
// Chrome art instances
// ---------------------------------------------------------------------------

pub(crate) fn build_sidebar_chrome_instances(
    state: &AppState,
    view: &SidebarView,
) -> Vec<SpriteInstance> {
    let Some(atlas) = current_sidebar_chrome(state) else {
        return Vec::new();
    };
    build_sidebar_chrome_instances_for_layout(
        atlas,
        state.match_state.match_presentation.sidebar_layout_spec,
        &view.layout,
        view,
        &view.tabs,
        &state.match_state.match_presentation.power_bar_anim,
        [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ],
        state.match_state.match_presentation.ui_scale,
        state.match_state.match_presentation.radar_anim.is_none(),
    )
}

/// The bottom command bar, which shares the chrome atlas but draws into a
/// surface of its own (`presentation::sidebar_surface`).
pub(crate) fn build_command_bar_instances(state: &AppState) -> Vec<SpriteInstance> {
    let mut instances = Vec::new();
    if let Some(atlas) = current_sidebar_chrome(state) {
        command_bar::append(state, atlas, &mut instances);
    }
    instances
}

pub fn build_sidebar_chrome_instances_for_layout(
    atlas: &SidebarChromeAtlas,
    spec: SidebarChromeLayoutSpec,
    layout: &SidebarLayout,
    view: &SidebarView,
    tabs: &[SidebarTabButton],
    power_bar_anim: &PowerBarAnimState,
    camera_offset: [f32; 2],
    ui_scale: f32,
    static_radar_fallback: bool,
) -> Vec<SpriteInstance> {
    let mut inst = Vec::new();
    let d = 0.00048;
    let s = ui_scale;
    let cx = layout.sidebar_x;
    // RadarClass::Draw 653100 and DrawCreditsSHPBackground 6D0E60.
    if let Some(top) = atlas.top_strip_sidebar {
        push_chrome(&mut inst, top, cx, 16.0, d, camera_offset, s);
    }
    if let Some(credits) = atlas.top_strip_thin {
        push_chrome(&mut inst, credits, cx, 0.0, d, camera_offset, s);
    }

    if static_radar_fallback {
        push_chrome(
            &mut inst,
            atlas.radar,
            cx,
            layout.radar_y,
            d,
            camera_offset,
            s,
        );
    }
    push_chrome(
        &mut inst,
        atlas.side1,
        cx,
        layout.side1_y,
        d,
        camera_offset,
        s,
    );
    let td = d - 0.00001;
    for tab_btn in tabs {
        let idx = tab_btn.tab.tab_index();
        let frame = tab_btn.frame_index as usize;
        // Fall back to frame 0 if the requested frame is missing in MIX.
        let entry = atlas.tab_frames[idx][frame].or(atlas.tab_frames[idx][0]);
        if let Some(e) = entry {
            push_chrome(
                &mut inst,
                e,
                tab_btn.rect.x,
                tab_btn.rect.y,
                td,
                camera_offset,
                s,
            );
        }
    }
    // 6A6C30: row count is native capacity; pixel advance is SIDE2's
    // loaded canvas height. SIDE3 and ADDON follow the last actual tile.
    for row in 0..layout.side2_tile_count {
        let y = layout.cameo_grid_top + row as f32 * atlas.side2.pixel_size[1];
        push_chrome(&mut inst, atlas.side2, cx, y, d, camera_offset, s);
    }
    push_chrome(
        &mut inst,
        atlas.side3,
        cx,
        layout.side3_y,
        d,
        camera_offset,
        s,
    );
    if let Some(addon) = atlas.unknown_top_housing {
        push_chrome(
            &mut inst,
            addon,
            cx,
            layout.side3_y + atlas.side3.pixel_size[1],
            d,
            camera_offset,
            s,
        );
    }

    // --- Sell / Repair buttons (inside the side1 area). ---
    // `shape_button_frame` (ShapeButtonClass::Draw 0x0069DEB0) picks the frame
    // through SidebarGadgetState::repair_frame / sell_frame.
    let btn_depth = d - 0.00002;
    let sell_frame = view.sell_button.frame_index as usize;
    if let Some(sell) = atlas.sell_frames[sell_frame].or(atlas.sell_frames[0]) {
        push_chrome(
            &mut inst,
            sell,
            view.sell_button.rect.x,
            view.sell_button.rect.y,
            btn_depth,
            camera_offset,
            s,
        );
    }
    let repair_frame = view.repair_button.frame_index as usize;
    if let Some(repair) = atlas.repair_frames[repair_frame].or(atlas.repair_frames[0]) {
        push_chrome(
            &mut inst,
            repair,
            view.repair_button.rect.x,
            view.repair_button.rect.y,
            btn_depth,
            camera_offset,
            s,
        );
    }

    // 653010/653100: draw and hit-test use the same loaded SHP canvas.
    for i in 0..2 {
        let button = &view.top_buttons[i];
        if let Some(entry) = atlas.top_button_frames[i][button.frame_index as usize] {
            push_chrome(
                &mut inst,
                entry,
                button.rect.x,
                button.rect.y,
                btn_depth,
                camera_offset,
                s,
            );
        }
    }

    // --- Strip-scroll pair (R-DN +page left, R-UP −page right) ---
    // 3-frame R-UP/R-DN convention (0 = idle, 1 = pressed, 2 = disabled),
    // selected by SidebarGadgetState::scroll_*_frame; frame-0 fallback only
    // covers art missing from the MIX, exactly like repair/sell.
    let scroll_down_rect = view.scroll_down_button.rect;
    let scroll_up_rect = view.scroll_up_button.rect;
    let down_frame = view.scroll_down_button.frame_index as usize;
    if let Some(e) = atlas.scroll_down_frames[down_frame].or(atlas.scroll_down_frames[0]) {
        push_chrome(
            &mut inst,
            e,
            scroll_down_rect.x,
            scroll_down_rect.y,
            btn_depth,
            camera_offset,
            s,
        );
    }
    let up_frame = view.scroll_up_button.frame_index as usize;
    if let Some(e) = atlas.scroll_up_frames[up_frame].or(atlas.scroll_up_frames[0]) {
        push_chrome(
            &mut inst,
            e,
            scroll_up_rect.x,
            scroll_up_rect.y,
            btn_depth,
            camera_offset,
            s,
        );
    }

    // --- Power bar meter (powerp.shp strips stacked from top) ---
    render_power_bar(
        &mut inst,
        atlas,
        spec,
        layout,
        power_bar_anim,
        camera_offset,
        d,
    );

    inst
}

/// Render the vertical power bar meter by stacking powerp.shp strip tiles.
///
/// Draws segments from top to bottom matching the original PowerClass::Draw_It:
///   Empty (top)  = unused bar space (frame 0)
///   Red          = deficit segments (frame 3)
///   Yellow       = balance indicator (frame 2)
///   Green        = surplus / consumed power (frame 1, with frame 4 blink)
///
/// Segment counts come from `PowerBarAnimState` which animates them
/// one-at-a-time toward their targets for a smooth sliding effect.
fn render_power_bar(
    inst: &mut Vec<SpriteInstance>,
    atlas: &SidebarChromeAtlas,
    spec: SidebarChromeLayoutSpec,
    layout: &SidebarLayout,
    anim: &PowerBarAnimState,
    camera_offset: [f32; 2],
    base_depth: f32,
) {
    let bar_x: f32 = layout.sidebar_x + spec.power_bar_x;
    let bar_top: f32 = 227.0;
    let tile_h: f32 = spec.power_bar_tile_height;

    if tile_h <= 0.0 || anim.max_segments() <= 0 {
        return;
    }

    let fill_depth: f32 = base_depth - 0.00002;

    anim.visit_draw_segments(|frame, row| {
        if let Some(entry) = atlas.powerp_frames[usize::from(frame)] {
            push_chrome_sized(
                inst,
                entry,
                bar_x,
                bar_top + row as f32 * tile_h,
                entry.pixel_size,
                fill_depth,
                camera_offset,
            );
        }
    });
}

fn push_chrome(
    instances: &mut Vec<SpriteInstance>,
    entry: crate::render::sidebar_chrome::SidebarChromeEntry,
    x: f32,
    y: f32,
    depth: f32,
    camera_offset: [f32; 2],
    scale: f32,
) {
    instances.push(SpriteInstance {
        position: [x + camera_offset[0], y + camera_offset[1]],
        size: [entry.pixel_size[0] * scale, entry.pixel_size[1] * scale],
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    });
}

fn push_chrome_sized(
    instances: &mut Vec<SpriteInstance>,
    entry: crate::render::sidebar_chrome::SidebarChromeEntry,
    x: f32,
    y: f32,
    size: [f32; 2],
    depth: f32,
    camera_offset: [f32; 2],
) {
    instances.push(SpriteInstance {
        position: [x + camera_offset[0], y + camera_offset[1]],
        size,
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    });
}

// ---------------------------------------------------------------------------
// Cameo icon instances
// ---------------------------------------------------------------------------

/// Horizontal padding around the ready text (each side, in native pixels).
pub(crate) const READY_PAD_X: f32 = 2.0;
/// Vertical padding around the ready text (each side, in native pixels).
pub(crate) const READY_PAD_Y: f32 = 1.0;

/// Horizontal padding for queue count badge (native pixels, matches ComputeTextRect x_pad=2).
const QUEUE_COUNT_PAD_X: f32 = 2.0;
/// Vertical padding for queue count badge (native pixels, matches ComputeTextRect y_pad=1).
const QUEUE_COUNT_PAD_Y: f32 = 1.0;

/// Compute the text scale for cameo overlay text (READY, queue count).
/// Uses full ui_scale so text stays proportional to the scaled cameos.
pub(crate) fn ready_text_scale(ui_scale: f32) -> f32 {
    ui_scale
}

/// The CSF labels shared by the cameo's single status strip and its glyphs.
#[derive(Clone, Copy)]
pub(crate) struct CameoStatusLabels<'a> {
    pub ready: &'a str,
    pub hold: &'a str,
    pub charging: &'a str,
    pub active: &'a str,
}

/// `SuperClass::NameReadiness6CC2B0` status label; `None` while it has no
/// status text. Shared by the right cameo strip and the left sidebar button.
pub(crate) fn superweapon_status_label<'a>(
    display: crate::sim::superweapon::SuperWeaponDisplay,
    labels: CameoStatusLabels<'a>,
) -> Option<&'a str> {
    use crate::sim::superweapon::SuperWeaponStatus;
    display.status.map(|status| match status {
        SuperWeaponStatus::Hold => labels.hold,
        SuperWeaponStatus::Ready => labels.ready,
        SuperWeaponStatus::Charging => labels.charging,
        SuperWeaponStatus::Active => labels.active,
    })
}

/// The same status selection sizes the dark strip and draws its text.
fn cameo_status_text<'a>(
    item: &crate::ui::sidebar::SidebarItem,
    labels: CameoStatusLabels<'a>,
) -> Option<&'a str> {
    if let Some(display) = item.super_weapon_display {
        return superweapon_status_label(display, labels);
    }
    if item.is_ready {
        Some(labels.ready)
    } else if item.is_on_hold {
        Some(labels.hold)
    } else {
        None
    }
}

fn shows_queue_badge(item: &crate::ui::sidebar::SidebarItem) -> bool {
    !matches!(
        item.queue_category,
        crate::sim::production::ProductionCategory::Building
            | crate::sim::production::ProductionCategory::Defense
    ) && (item.queued_count > 1 || (item.queued_count > 0 && !item.is_building_this_type))
}

/// Whether a cameo draws the build clock. gamemd's strip draw sets its clock
/// flag whenever the cameo's factory exists (`StripClass::Draw`,
/// `0x006A9812..0x006A981D`) and draws frame `progress + 1`, so a build shows
/// its clock from the frame it starts, before its first step.
fn shows_build_clock(item: &crate::ui::sidebar::SidebarItem) -> bool {
    // SuperClass::ShouldDrawProgress6CDE90 draws every drain phase, even
    // with raw Ready set or a stage-zero charge. Factory policy is separate.
    if let Some(display) = item.super_weapon_display {
        return display.draw_progress;
    }
    !item.is_ready && (item.is_building_this_type || item.is_on_hold || item.progress > 0.0)
}

/// Map an alpha-cropped source rectangle through its original canvas into a
/// sidebar slot. Rounding both crop edges from the shared canvas transform
/// keeps the base art and full-canvas overlays on the same pixel boundaries.
/// The camera offset is added after screen-space rounding so the shader can
/// subtract it without making fixed UI geometry depend on fractional panning.
pub(crate) fn place_canvas_crop_in_slot(
    slot: Rect,
    canvas_size: [f32; 2],
    crop_origin: [f32; 2],
    crop_size: [f32; 2],
    camera_offset: [f32; 2],
) -> Option<Rect> {
    let [canvas_w, canvas_h] = canvas_size;
    let [crop_w, crop_h] = crop_size;
    if canvas_w <= 0.0 || canvas_h <= 0.0 || crop_w <= 0.0 || crop_h <= 0.0 {
        return None;
    }

    let scale = (slot.w / canvas_w).min(slot.h / canvas_h);
    let canvas_x = slot.x + (slot.w - canvas_w * scale) * 0.5;
    let canvas_y = slot.y + (slot.h - canvas_h * scale) * 0.5;
    let left = (canvas_x + crop_origin[0] * scale).round();
    let top = (canvas_y + crop_origin[1] * scale).round();
    let right = (canvas_x + (crop_origin[0] + crop_w) * scale).round();
    let bottom = (canvas_y + (crop_origin[1] + crop_h) * scale).round();

    Some(Rect {
        x: left + camera_offset[0],
        y: top + camera_offset[1],
        w: right - left,
        h: bottom - top,
    })
}

/// Select and place the GCLOCK2 frame for one in-progress cameo.
pub(crate) fn build_gclock_instance(
    gclock_frames: &[SidebarChromeEntry],
    progress: f32,
    slot: Rect,
    camera_offset: [f32; 2],
) -> Option<SpriteInstance> {
    // Factory stage projection retains its existing clamp and rounding.
    let progress = progress.clamp(0.0, 1.0);
    let stage = (progress * f32::from(crate::sim::production::PRODUCTION_STEPS)).round() as i32;
    build_gclock_stage_instance(gclock_frames, stage, slot, camera_offset)
}

fn build_cameo_clock_instance(
    gclock_frames: &[SidebarChromeEntry],
    item: &crate::ui::sidebar::SidebarItem,
    slot: Rect,
    camera_offset: [f32; 2],
) -> Option<SpriteInstance> {
    if let Some(display) = item.super_weapon_display {
        build_gclock_stage_instance(gclock_frames, display.stage, slot, camera_offset)
    } else {
        build_gclock_instance(gclock_frames, item.progress, slot, camera_offset)
    }
}

pub(crate) fn build_gclock_stage_instance(
    gclock_frames: &[SidebarChromeEntry],
    stage: i32,
    slot: Rect,
    camera_offset: [f32; 2],
) -> Option<SpriteInstance> {
    let last_frame = gclock_frames.len().checked_sub(1)?;
    // StripClass::Draw6A9E97 adds one before SHP signed-frame admission.
    // Stage-1 selects frame0; lower frames refuse. Short art still clamps.
    let frame_index = usize::try_from(stage.wrapping_add(1)).ok()?.min(last_frame);
    let gclock_entry = &gclock_frames[frame_index];
    let gclock_rect = place_canvas_crop_in_slot(
        slot,
        gclock_entry.pixel_size,
        [0.0, 0.0],
        gclock_entry.pixel_size,
        camera_offset,
    )?;

    Some(SpriteInstance {
        position: [gclock_rect.x, gclock_rect.y],
        size: [gclock_rect.w, gclock_rect.h],
        uv_origin: gclock_entry.uv_origin,
        uv_size: gclock_entry.uv_size,
        depth: 0.00043,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        draw_state: crate::render::draw_state::DrawState::from_native_selector(0, 4),
        ..Default::default()
    })
}

#[derive(Default)]
pub(crate) struct SidebarCameoInstances {
    pub cameo: Vec<SpriteInstance>,
    pub unavailable: Vec<SpriteInstance>,
    pub flash: Vec<SpriteInstance>,
    pub gclock: Vec<SpriteInstance>,
    pub queue_overlay: Vec<SpriteInstance>,
    pub overlay: Vec<SpriteInstance>,
}

/// Original shape point is the cameo top-left; the caller clips at +60/+48
/// (StripDraw6A99C3..6A99E0). Do not fit the stock75x52 DARKEN canvas into
/// the cameo. Preserve frame origin, zero indices and the unscaled source UV.
pub(crate) fn build_darken_instance(
    mask: &crate::render::sidebar_chrome::SidebarDarkenMask,
    slot: Rect,
    camera_offset: [f32; 2],
    selector: u8,
) -> Option<SpriteInstance> {
    let [x, y] = mask.frame_origin;
    let width = mask.texture.width as f32;
    let height = mask.texture.height as f32;
    let size = [width.min(slot.w - x), height.min(slot.h - y)];
    if size[0] <= 0.0 || size[1] <= 0.0 {
        return None;
    }
    Some(SpriteInstance {
        position: [slot.x + x + camera_offset[0], slot.y + y + camera_offset[1]],
        size,
        uv_size: [size[0] / width, size[1] / height],
        tint: [1.0; 3],
        alpha: 1.0,
        draw_state: crate::render::draw_state::DrawState::from_native_selector(0, selector),
        ..Default::default()
    })
}

/// Base icons, native unavailable/flash/clock destination edits, then status
/// strip quads. Native status/badge text strips retain their separate owner.
pub(crate) fn build_sidebar_cameo_instances(
    state: &AppState,
    view: &SidebarView,
    labels: CameoStatusLabels<'_>,
) -> SidebarCameoInstances {
    let Some(atlas) = state
        .match_state
        .match_presentation
        .sidebar_cameo_atlas
        .as_ref()
    else {
        return SidebarCameoInstances::default();
    };
    let mut instances = Vec::new();
    let mut gclock_instances = Vec::new();
    let mut overlay_instances = Vec::new();
    let mut queue_overlay = Vec::new();
    let mut unavailable = Vec::new();
    let mut flash = Vec::new();
    let co = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ];
    let chrome = crate::app::presentation::sidebar_render::current_sidebar_chrome(state);
    let gclock_frames: &[SidebarChromeEntry] =
        chrome.map(|a| a.gclock_frames.as_slice()).unwrap_or(&[]);
    for item in &view.items {
        let entry = if item.is_superweapon {
            atlas.superweapon(&item.type_id)
        } else {
            atlas.cameo(&item.type_id, item.alt_cameo)
        };
        let Some(entry) = entry else {
            continue;
        };
        let slot = item.cameo_rect();
        let Some(cameo_rect) = place_canvas_crop_in_slot(
            slot,
            entry.canvas_size,
            entry.crop_origin,
            entry.pixel_size,
            co,
        ) else {
            continue;
        };
        instances.push(SpriteInstance {
            position: [cameo_rect.x, cameo_rect.y],
            size: [cameo_rect.w, cameo_rect.h],
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            depth: 0.00044,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        });
        if shows_build_clock(item)
            && let Some(instance) = build_cameo_clock_instance(gclock_frames, item, slot, co)
        {
            gclock_instances.push(instance);
        }

        if let Some(mask) = chrome.and_then(|atlas| atlas.darken.as_ref()) {
            // View owns admission: linked E2 clears BL even when its last
            // producer is Selling, while unlinked FLAKT can remain shaded.
            if !item.enabled
                && let Some(instance) = build_darken_instance(mask, slot, co, 1)
            {
                unavailable.push(instance);
            }
            if item.flash_darkened
                && let Some(instance) = build_darken_instance(mask, slot, co, 4)
            {
                flash.push(instance);
            }
        }

        let has_queue_badge = shows_queue_badge(item);

        // Dark strip overlay behind the cameo status text (alpha 0xAF).
        // When a queue badge is also present, the status strip shifts left.
        if let Some(status_text) = cameo_status_text(item, labels)
            && state.renderer.bit_font.darken_texture().is_some()
        {
            let s = state.match_state.match_presentation.ui_scale;
            let ts = ready_text_scale(s);
            let text_w = state.renderer.bit_font.text_width(status_text) as f32 * ts;
            let strip_w = text_w + READY_PAD_X * 2.0 * ts;
            // gamemd `ComputeTextRect` uses `cell_height + 2*y_pad` for the
            // strip height (cell_height includes the 1 px inter-line gap that
            // gamemd extends below the glyphs).
            let strip_h = (state.renderer.bit_font.cell_height() + READY_PAD_Y * 2.0) * ts;
            let strip_x = if has_queue_badge {
                slot.x + co[0]
            } else {
                slot.x + (slot.w - strip_w) * 0.5 + co[0]
            };
            overlay_instances.push(SpriteInstance {
                position: [strip_x, slot.y + co[1]],
                size: [strip_w, strip_h.min(slot.h)],
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                depth: 0.00043,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            });
        }

        // Dark strip overlay behind queue count badge (top-right, same alpha as Ready strip).
        // Original: ComputeTextRect(cameo_x+60, cameo_y+1, 0x242, x_pad=2, y_pad=1)
        // The dark rect extends x_pad (2px) past the cameo right edge.
        if has_queue_badge && state.renderer.bit_font.darken_texture().is_some() {
            let ts = ready_text_scale(state.match_state.match_presentation.ui_scale);
            let count_str = format!("{}", item.queued_count);
            let text_w = state.renderer.bit_font.text_width(&count_str) as f32 * ts;
            let strip_w = text_w + QUEUE_COUNT_PAD_X * 2.0 * ts;
            let strip_h = (state.renderer.bit_font.cell_height() + QUEUE_COUNT_PAD_Y * 2.0) * ts;
            // Right-align anchor at cameo right edge; strip extends x_pad past it.
            let strip_x = slot.x + slot.w - text_w - QUEUE_COUNT_PAD_X * ts;
            queue_overlay.push(SpriteInstance {
                position: [strip_x + co[0], slot.y + co[1]],
                size: [strip_w, strip_h.min(slot.h)],
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                depth: 0.00043,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            });
        }
    }
    SidebarCameoInstances {
        cameo: instances,
        unavailable,
        flash,
        gclock: gclock_instances,
        queue_overlay,
        overlay: overlay_instances,
    }
}

// ---------------------------------------------------------------------------
// Text label instances
// ---------------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct SidebarTextInstances {
    pub status: Vec<SpriteInstance>,
    pub queue: Vec<SpriteInstance>,
}

pub(crate) fn build_sidebar_text_instances(
    state: &AppState,
    view: &SidebarView,
    labels: CameoStatusLabels<'_>,
    ready_tint: [f32; 3],
) -> SidebarTextInstances {
    if state.renderer.bit_font.darken_texture().is_none() {
        // No retail FNT loaded; the native sidebar text path is unavailable.
        return SidebarTextInstances::default();
    }
    let s = state.match_state.match_presentation.ui_scale;
    let ts = ready_text_scale(s);
    let co = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ];
    let mut instances = Vec::new();
    let mut queue = Vec::new();

    for item in &view.items {
        let slot = item.rect;

        let has_queue_badge = shows_queue_badge(item);

        // Cameo status text ("Ready" / "On Hold") — at the top of the cameo.
        // When a queue badge is also shown, the status text shifts left to
        // avoid overlap (original: x = cameo_x+2, flags 0x42 vs centered
        // cameo_x+30, 0x142).
        if let Some(status_text) = cameo_status_text(item, labels) {
            let text_w = state.renderer.bit_font.text_width(status_text) as f32 * ts;
            let text_x = if has_queue_badge {
                slot.x + READY_PAD_X * ts
            } else {
                slot.x + (slot.w - text_w) * 0.5
            };
            // gamemd anchors text at `cameo_y + y_pad`; the strip extends
            // y_pad above and (cell_height - glyph_height + y_pad) below.
            let text_y = slot.y + READY_PAD_Y * ts;
            instances.extend(state.renderer.bit_font.build_text(
                status_text,
                text_x,
                text_y,
                ts,
                0.00042,
                ready_tint,
                co,
            ));
        }

        // Queue count badge — right-aligned at top-right of cameo.
        // Original: ComputeTextRect(cameo_x+60, cameo_y+1, 0x242, 2, 1)
        // 0x242 = right-align. Uses same side-dependent color as Ready text.
        if has_queue_badge {
            let count_str = format!("{}", item.queued_count);
            let text_w = state.renderer.bit_font.text_width(&count_str) as f32 * ts;
            // Right-align: text right edge at cameo right edge (anchor = cameo_x + 60).
            let text_x = slot.x + slot.w - text_w;
            let text_y = slot.y + QUEUE_COUNT_PAD_Y * ts;
            queue.extend(
                state
                    .renderer
                    .bit_font
                    .build_text(&count_str, text_x, text_y, ts, 0.00042, ready_tint, co),
            );
        }
    }
    SidebarTextInstances {
        status: instances,
        queue,
    }
}

#[cfg(test)]
mod tests {
    use super::{build_gclock_instance, place_canvas_crop_in_slot};
    use crate::render::sidebar_chrome::SidebarChromeEntry;
    use crate::ui::sidebar::Rect;

    #[test]
    fn test_canvas_crop_uses_shared_rounded_edges_with_camera_cancellation() {
        let slot = Rect {
            x: 100.1,
            y: 50.3,
            w: 75.0,
            h: 60.0,
        };
        let without_camera =
            place_canvas_crop_in_slot(slot, [60.0, 48.0], [3.0, 4.0], [54.0, 40.0], [0.0, 0.0])
                .unwrap();
        let with_camera =
            place_canvas_crop_in_slot(slot, [60.0, 48.0], [3.0, 4.0], [54.0, 40.0], [13.25, -7.5])
                .unwrap();

        assert_eq!(
            without_camera,
            Rect {
                x: 104.0,
                y: 55.0,
                w: 67.0,
                h: 50.0
            }
        );
        assert_eq!(with_camera.w, without_camera.w);
        assert_eq!(with_camera.h, without_camera.h);
        assert_eq!(with_camera.x - 13.25, without_camera.x);
        assert_eq!(with_camera.y + 7.5, without_camera.y);
        // 54 * 1.25 is 67.5; deriving size from the shared rounded edges
        // intentionally produces 67 rather than independently rounding to 68.
        assert_eq!(with_camera.w, 67.0);
    }

    #[test]
    fn gclock_selects_executed_native_frames_and_short_art_clamps() {
        let frames: Vec<_> = (0..55)
            .map(|frame| SidebarChromeEntry {
                uv_origin: [frame as f32, 0.0],
                uv_size: [1.0; 2],
                pixel_size: [60.0, 48.0],
            })
            .collect();
        let slot = Rect {
            x: 0.0,
            y: 0.0,
            w: 60.0,
            h: 48.0,
        };
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/sidebar_oracle/factory_sale_sidebar.json",
        ))
        .unwrap();
        let mut compared = 0;
        for control in native["controls"].as_array().unwrap() {
            for command in control["draw_commands"].as_array().into_iter().flatten() {
                if command["kind"] != "clock" {
                    continue;
                }
                let stage = control["prepared"]["factory"]["stage"].as_u64().unwrap() as u16;
                let progress =
                    f32::from(stage) / f32::from(crate::sim::production::PRODUCTION_STEPS);
                let instance = build_gclock_instance(&frames, progress, slot, [0.0; 2]).unwrap();
                let frame = command["frame"].as_u64().unwrap() as usize;
                assert_eq!(
                    instance.uv_origin, frames[frame].uv_origin,
                    "{}",
                    control["name"]
                );
                assert_eq!(
                    0x400 | instance.draw_state.native_selector_bits(),
                    command["flags"].as_u64().unwrap() as u32,
                );
                compared += 1;
            }
        }
        assert_eq!(compared, 4);
        let instance = build_gclock_instance(&frames[..3], 53.0 / 54.0, slot, [0.0; 2]).unwrap();
        assert_eq!(instance.uv_origin, frames[2].uv_origin);
    }

    /// The UI receives the simulation's derived native display, independently
    /// of raw Ready: drain phase2 still draws a clock and its own status.
    #[test]
    fn super_cameo_preserves_native_display_through_status_and_signed_clock() {
        use crate::sim::intern::StringInterner;
        use crate::sim::superweapon::{SuperWeaponDisplay, SuperWeaponStatus, SuperWeaponView};
        use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
        use crate::ui::sidebar::{self, SidebarChromeLayoutSpec, SidebarTab};
        let mut interner = StringInterner::new();
        let id = interner.intern("Drain");
        // Supplied display output at the client boundary. Core histories
        // cover the actual Grant/AI/click arithmetic that produces it.
        let project = |display, online| {
            let supers = [SuperWeaponView {
                type_id: id,
                use_ai_targeting: false,
                display_name: "Drain".into(),
                progress: 1.0,
                is_ready: true,
                is_online: online,
                should_flash_tab: false,
                can_fire: online,
                can_afford: true,
                constraints_blocked: false,
                #[cfg(feature = "ares")]
                ares_refusal: None,
                display,
            }];
            let cameos = sidebar::cameo_order::CameoStrips::layout_fixture(&[], &[], &supers);
            sidebar::build_sidebar_view_with_spec(
                SidebarChromeLayoutSpec::default(),
                800.0,
                600.0,
                SidebarTab::Defense,
                0,
                0,
                0,
                None,
                &[],
                &[],
                &[],
                None,
                &[],
                Some(&interner),
                &supers,
                &SidebarGadgetState::default(),
                None,
                None,
                None,
                None,
                [None; 2],
                &cameos,
            )
            .items
            .pop()
            .unwrap()
        };
        let labels = super::CameoStatusLabels {
            ready: "Ready",
            hold: "On Hold",
            charging: "Charging",
            active: "Firestorm On",
        };
        let display = SuperWeaponDisplay {
            stage: 0,
            draw_progress: true,
            status: Some(SuperWeaponStatus::Active),
        };
        let mut item = project(display, true);
        assert_eq!(item.super_weapon_display, Some(display));
        assert!(item.is_ready && super::shows_build_clock(&item));
        assert_eq!(
            super::cameo_status_text(&item, labels),
            Some("Firestorm On")
        );
        let frames: Vec<_> = (0..55)
            .map(|frame| SidebarChromeEntry {
                uv_origin: [frame as f32, 0.0],
                uv_size: [1.0; 2],
                pixel_size: [60.0, 48.0],
            })
            .collect();
        let slot = item.cameo_rect();
        let draw = |item: &sidebar::SidebarItem| {
            super::build_cameo_clock_instance(&frames, item, slot, [0.0; 2])
        };
        assert_eq!(
            draw(&item).unwrap().uv_origin,
            frames[1].uv_origin,
            "native integer stage wins over raw Ready and float progress"
        );
        item = project(
            SuperWeaponDisplay {
                status: Some(SuperWeaponStatus::Hold),
                ..display
            },
            false,
        );
        assert!(item.is_ready && item.is_on_hold);
        assert_eq!(super::cameo_status_text(&item, labels), Some("On Hold"));
        item = project(
            SuperWeaponDisplay {
                stage: -1,
                status: Some(SuperWeaponStatus::Charging),
                ..display
            },
            true,
        );
        assert_eq!(super::cameo_status_text(&item, labels), Some("Charging"));
        assert_eq!(draw(&item).unwrap().uv_origin, frames[0].uv_origin);
        item = project(
            SuperWeaponDisplay {
                stage: -2,
                status: None,
                ..display
            },
            true,
        );
        assert!(draw(&item).is_none());
        assert_eq!(super::cameo_status_text(&item, labels), None);
    }

    /// The cameo status slot carries `TXT_HOLD` for suspended production, the
    /// same way it carries `TXT_READY` for a placement-ready building. Before
    /// this, a held or unaffordable item looked identical to an idle one.
    #[test]
    fn cameo_status_text_covers_ready_and_hold() {
        use super::{CameoStatusLabels, cameo_status_text};
        let labels = CameoStatusLabels {
            ready: "Ready",
            hold: "On Hold",
            charging: "Charging",
            active: "Active",
        };

        let mut item = crate::ui::sidebar::SidebarItem {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 60.0,
                h: 48.0,
            },
            type_id: "GAPOWR".to_string(),
            display_name: "GAPOWR".to_string(),
            cost: Some(600),
            has_cameo_art: true,
            alt_cameo: false,
            flash_darkened: false,
            queue_category: crate::sim::production::ProductionCategory::Building,
            enabled: true,
            at_build_limit: false,
            progress: 0.5,
            queued_count: 1,
            is_building_this_type: false,
            is_ready: false,
            is_on_hold: false,
            is_armed: false,
            is_superweapon: false,
            super_weapon_section: None,
            super_weapon_display: None,
        };
        assert_eq!(cameo_status_text(&item, labels), None);

        item.is_on_hold = true;
        assert_eq!(cameo_status_text(&item, labels), Some("On Hold"));

        // Ready wins when a slot somehow reports both.
        item.is_ready = true;
        assert_eq!(cameo_status_text(&item, labels), Some("Ready"));
    }

    /// A build shows its clock from its start frame, at progress 0, and while
    /// it is on hold; a queued-only or ready cameo does not.
    #[test]
    fn build_clock_shows_from_the_start_of_a_build() {
        use super::shows_build_clock;

        let mut item = crate::ui::sidebar::SidebarItem {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 60.0,
                h: 48.0,
            },
            type_id: "MTNK".to_string(),
            display_name: "MTNK".to_string(),
            cost: Some(700),
            has_cameo_art: true,
            alt_cameo: false,
            flash_darkened: false,
            queue_category: crate::sim::production::ProductionCategory::Vehicle,
            enabled: true,
            at_build_limit: false,
            progress: 0.0,
            queued_count: 1,
            is_building_this_type: false,
            is_ready: false,
            is_on_hold: false,
            is_armed: false,
            is_superweapon: false,
            super_weapon_section: None,
            super_weapon_display: None,
        };
        assert!(!shows_build_clock(&item), "queued behind another build");
        item.is_building_this_type = true;
        assert!(shows_build_clock(&item), "started, no step yet");
        item.is_building_this_type = false;
        item.is_on_hold = true;
        assert!(shows_build_clock(&item), "on hold at progress 0");
        item.is_on_hold = false;
        item.is_ready = true;
        item.progress = 1.0;
        assert!(!shows_build_clock(&item), "ready");
    }
}
