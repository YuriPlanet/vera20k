//! Sprite instances for the Phobos left superweapon sidebar.
//!
//! Mirrors `SWColumnClass::Draw` (TopPCX/CenterPCX/BottomPCX then each
//! `SWButtonClass::Draw`) and `ToggleSWButtonClass::Draw`. Screen-fixed
//! coordinates, so callers draw these in the full-window pass.

use crate::app::AppState;
use crate::app::presentation::sidebar_build::{
    CameoStatusLabels, READY_PAD_X, READY_PAD_Y, build_gclock_stage_instance,
    place_canvas_crop_in_slot, ready_text_scale, superweapon_status_label,
};
use crate::render::batch::SpriteInstance;
use crate::render::sidebar_chrome::SidebarChromeEntry;
use crate::render::sw_sidebar_art::SwSidebarArtKind;
use crate::ui::sidebar::Rect;

#[derive(Default)]
pub(crate) struct SwSidebarInstances {
    pub(crate) chrome: Vec<SpriteInstance>,
    pub(crate) cameo: Vec<SpriteInstance>,
    pub(crate) gclock: Vec<SpriteInstance>,
    pub(crate) hover: Vec<SpriteInstance>,
    pub(crate) overlay: Vec<SpriteInstance>,
    pub(crate) text: Vec<SpriteInstance>,
}

// Fixed-UI depths (smaller draws in front): text 0.00041, strip 0.00042,
// progress clock 0.00040, cameo 0.00045, column art 0.00048 — the same layers
// and order as the right sidebar, with `SWButtonClass::Draw`'s progress last.
const DEPTH_ART: f32 = 0.00048;
const DEPTH_CAMEO: f32 = 0.00045;
const DEPTH_CLOCK: f32 = 0.00040;
const DEPTH_STRIP: f32 = 0.00042;
const DEPTH_TEXT: f32 = 0.00041;

pub(crate) fn build(
    state: &AppState,
    labels: CameoStatusLabels<'_>,
    ready_tint: [f32; 3],
) -> SwSidebarInstances {
    let mut out = SwSidebarInstances::default();
    let sw = &state.match_state.match_presentation.sw_sidebar;
    if !sw.ui.enabled {
        return out;
    }
    let art = state.match_state.match_presentation.sw_sidebar_art.as_ref();
    let hover_tint = crate::render::sidebar_text::side_highlight_color(
        crate::app::presentation::sidebar_render::current_sidebar_theme(state),
    );
    let (cursor_x, cursor_y) = state.match_state.input.cursor_px();
    if let (Some(entry), Some(toggle)) = (
        crate::app::presentation::sw_sidebar::toggle_art(state),
        sw.layout.toggle,
    ) {
        out.chrome.push(sprite_art(entry, toggle, DEPTH_ART));
        // `ToggleSWButtonClass::Draw` outlines the hovered toggle too.
        if toggle.contains(cursor_x as f32, cursor_y as f32) {
            push_outline(&mut out.hover, toggle, hover_tint);
        }
    }
    if !sw.expanded {
        return out;
    }
    if let Some(rect) = crate::app::presentation::sw_sidebar::hovered_entry_rect(state) {
        push_outline(&mut out.hover, rect, hover_tint);
    }
    if let Some(art) = art {
        let center_height = sw.ui.cameo_height.max(48) as f32;
        for column in &sw.layout.columns {
            let column_rect = Rect {
                x: column.x as f32,
                y: column.y as f32,
                w: column.width as f32,
                h: column.height as f32,
            };
            if let Some(entry) = art.entry(SwSidebarArtKind::Top) {
                out.chrome.push(sprite_art(
                    entry,
                    Rect {
                        x: column_rect.x,
                        y: column_rect.y,
                        w: column_rect.w,
                        h: entry.pixel_size[1],
                    },
                    DEPTH_ART,
                ));
            }
            if let Some(entry) = art.entry(SwSidebarArtKind::Bottom) {
                out.chrome.push(sprite_art(
                    entry,
                    Rect {
                        x: column_rect.x,
                        y: column_rect.y + column_rect.h - entry.pixel_size[1],
                        w: column_rect.w,
                        h: entry.pixel_size[1],
                    },
                    DEPTH_ART,
                ));
            }
            if let Some(entry) = art.entry(SwSidebarArtKind::Center) {
                // Each column's center strip repeats behind its buttons.
                let first = column.first_button;
                let end = first + column.button_count;
                for placed in sw.layout.entries.iter().take(end).skip(first) {
                    let half_gap = (center_height - 48.0) / 2.0;
                    out.chrome.push(sprite_art(
                        entry,
                        Rect {
                            x: column.x as f32,
                            y: placed.rect.y - half_gap,
                            w: column.width as f32,
                            h: center_height,
                        },
                        DEPTH_ART,
                    ));
                }
            }
        }
    }

    let atlas = state
        .match_state
        .match_presentation
        .sidebar_cameo_atlas
        .as_ref();
    // Native screen pixels, like `SWButtonClass::Draw` on `DSurface::Composite`.
    let ts = ready_text_scale(1.0);
    let chrome = crate::app::presentation::sidebar_render::current_sidebar_chrome(state);
    let gclock_frames: &[SidebarChromeEntry] = chrome
        .map(|atlas| atlas.gclock_frames.as_slice())
        .unwrap_or(&[]);
    let keys_enabled = state
        .rules()
        .is_some_and(|rules| rules.sw_sidebar.keys_enabled);
    for entry in &sw.entries {
        let slot = entry.rect;
        // The shared atlas holds Ares `SidebarPCX` ahead of the SHP cameo,
        // as `SWButtonClass::Draw` selects them.
        let Some(cameo) = atlas.and_then(|atlas| atlas.superweapon(&entry.type_id)) else {
            continue;
        };
        let Some(rect) = place_canvas_crop_in_slot(
            slot,
            cameo.canvas_size,
            cameo.crop_origin,
            cameo.pixel_size,
            [0.0, 0.0],
        ) else {
            continue;
        };
        let mut instance = sprite_cameo(cameo, rect, DEPTH_CAMEO);
        // `SWButtonClass::Draw` darkens a ready superweapon its owner cannot
        // pay for, or one whose Ares targeting constraints refuse manual fire.
        // RESIDUAL: Phobos blits DARKEN.SHP with `BlitterFlags::Darken`; VERA
        // halves the cameo instead.
        if (entry.view.is_ready && !entry.view.can_afford) || entry.view.constraints_blocked {
            instance.tint = [0.5, 0.5, 0.5];
        }
        out.cameo.push(instance);
        if entry.view.display.draw_progress
            && let Some(mut clock) = build_gclock_stage_instance(
                gclock_frames,
                entry.view.display.stage,
                slot,
                [0.0, 0.0],
            )
        {
            // `SWButtonClass::Draw` blits the progress clock after the ready
            // text, so it covers it.
            clock.depth = DEPTH_CLOCK;
            out.gclock.push(clock);
        }

        let readiness = readiness_text(&entry.view, labels);
        // `SWButtonClass::Draw`'s `ready`: not on hold, and Ready by
        // ChargeDrainState for charge-drain types, i.e. NameReadiness's Ready.
        let hotkey = (keys_enabled
            && entry.view.display.status
                == Some(crate::sim::superweapon::SuperWeaponStatus::Ready)
            && entry.column == 0
            && entry.column_button < crate::rules::phobos_sw_sidebar::SUPER_WEAPON_SIDEBAR_KEYS)
            .then(|| {
                state
                    .match_state
                    .input
                    .hotkey_bindings
                    .first_key(crate::app::input::hotkeys::HotkeyCommand::FireTacticalSw(
                        entry.column_button,
                    ))
                    .map(crate::app::input::keyboard::key_name)
            })
            .flatten()
            .filter(|name| !name.is_empty());
        if let Some(text) = hotkey.as_deref().or(readiness)
            && state.renderer.bit_font.darken_texture().is_some()
        {
            let text_w = state.renderer.bit_font.text_width(text) as f32 * ts;
            let strip_w = text_w + READY_PAD_X * 2.0 * ts;
            let strip_h = (state.renderer.bit_font.cell_height() + READY_PAD_Y * 2.0) * ts;
            out.overlay.push(SpriteInstance {
                position: [slot.x + (slot.w - strip_w) * 0.5, slot.y],
                size: [strip_w, strip_h.min(slot.h)],
                uv_origin: [0.0, 0.0],
                uv_size: [1.0, 1.0],
                depth: DEPTH_STRIP,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            });
            let text_x = slot.x + (slot.w - text_w) * 0.5;
            out.text.extend(state.renderer.bit_font.build_text(
                text,
                text_x,
                slot.y + READY_PAD_Y * ts,
                ts,
                DEPTH_TEXT,
                ready_tint,
                [0.0, 0.0],
            ));
        }
    }
    out
}

/// `SWButtonClass::Draw`: a one-pixel `Drawing::Rect` in the tooltip colour
/// around the hovered gadget.
fn push_outline(out: &mut Vec<SpriteInstance>, rect: Rect, tint: [f32; 3]) {
    let border = 1.0;
    for (x, y, w, h) in [
        (rect.x, rect.y, rect.w, border),
        (rect.x, rect.y + rect.h - border, rect.w, border),
        (rect.x, rect.y, border, rect.h),
        (rect.x + rect.w - border, rect.y, border, rect.h),
    ] {
        out.push(SpriteInstance {
            position: [x, y],
            size: [w, h],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            depth: 0.00044,
            tint,
            alpha: 1.0,
            ..Default::default()
        });
    }
}

fn readiness_text<'a>(
    view: &crate::sim::superweapon::SuperWeaponView,
    labels: CameoStatusLabels<'a>,
) -> Option<&'a str> {
    superweapon_status_label(view.display, labels).or(view.is_ready.then_some(labels.ready))
}

fn sprite_cameo(
    entry: &crate::render::sidebar_cameo_atlas::SidebarCameoEntry,
    rect: Rect,
    depth: f32,
) -> SpriteInstance {
    SpriteInstance {
        position: [rect.x, rect.y],
        size: [rect.w, rect.h],
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    }
}

fn sprite_art(
    entry: &crate::render::sw_sidebar_art::SwSidebarArtEntry,
    rect: Rect,
    depth: f32,
) -> SpriteInstance {
    SpriteInstance {
        position: [rect.x, rect.y],
        size: [rect.w, rect.h],
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        depth,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    }
}
