//! The Phobos left superweapon sidebar's drawing trees.
//!
//! Mirrors `SWColumnClass::Draw` (TopPCX/CenterPCX/BottomPCX then each
//! `SWButtonClass::Draw`) and `ToggleSWButtonClass::Draw`. Each lane is a
//! retained tree ([`PictureHud`] for the column art, the cameos and the
//! clocks, [`TextHud`] for the hover outlines and for the labels' strips and
//! words), whose nodes are the pieces in drawing order. Screen-fixed
//! coordinates, so callers draw these in the full-window pass.

use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::presentation::picture_hud::{PictureHud, covering};
use crate::app::presentation::sidebar_build::{
    CameoStatusLabels, OUTLINE_DEPTH, READY_PAD_X, READY_PAD_Y, build_gclock_stage_instance,
    place_canvas_crop_in_slot, rect_outline_edges, status_label_fit, superweapon_status_label,
};
use crate::app::presentation::text_hud::{TextHud, rgb, rounded};
use crate::app::renderer_state::RendererState;
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

/// The column's trees, kept from frame to frame, one for each lane.
#[derive(Default)]
pub(crate) struct SwSidebarHuds {
    /// TopPCX, CenterPCX and BottomPCX, from the column's own atlas.
    chrome: PictureHud,
    /// The buttons' cameos, from the cameo atlas.
    cameo: PictureHud,
    /// Their progress clocks, from the chrome atlas.
    gclock: PictureHud,
    /// The hovered toggle's and button's outlines.
    hover: TextHud,
    /// The labels' dark strips (its boxes) and words (its glyphs).
    labels: TextHud,
}

// Fixed-UI depths (smaller draws in front): text 0.00041, strip 0.00042,
// progress clock 0.00040, cameo 0.00045, column art 0.00048 — the same layers
// and order as the right sidebar, with `SWButtonClass::Draw`'s progress last.
const DEPTH_ART: f32 = 0.00048;
const DEPTH_CAMEO: f32 = 0.00045;
const DEPTH_CLOCK: f32 = 0.00040;
const DEPTH_STRIP: f32 = 0.00042;
const DEPTH_TEXT: f32 = 0.00041;

/// The column this frame, through `huds`: nothing while `shown` is false
/// (no sidebar). The labels' text draws at the font's raster of now.
pub(crate) fn build(
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    huds: &mut SwSidebarHuds,
    shown: bool,
    labels: CameoStatusLabels<'_>,
    ready_tint: [f32; 3],
) -> SwSidebarInstances {
    for pictures in [&mut huds.chrome, &mut huds.cameo, &mut huds.gclock] {
        pictures.begin();
    }
    huds.hover.begin();
    huds.labels.begin();
    if shown {
        draw(renderer, frontend, match_state, huds, labels, ready_tint);
    }
    let font = &renderer.bit_font;
    huds.hover.finish(font);
    huds.labels.finish(font);
    SwSidebarInstances {
        chrome: huds.chrome.finish().to_vec(),
        cameo: huds.cameo.finish().to_vec(),
        gclock: huds.gclock.finish().to_vec(),
        hover: huds.hover.boxes().copied().collect(),
        overlay: huds.labels.boxes().copied().collect(),
        text: huds.labels.glyphs().copied().collect(),
    }
}

/// The frame's next picture of `pictures`: `instance`, its node over the
/// whole pixels it covers.
fn picture(pictures: &mut PictureHud, instance: SpriteInstance) {
    let ([x, y], [w, h]) = (instance.position, instance.size);
    pictures.picture(covering(x, y, w, h), instance);
}

/// The hover outline of `rect` in `tint` as the frame's next boxes of `hud`,
/// as [`crate::app::presentation::sidebar_build::push_rect_outline`] draws it.
fn outline(hud: &mut TextHud, rect: Rect, tint: [f32; 3]) {
    for [x, y, w, h] in rect_outline_edges(rect) {
        hud.fill_tinted(
            covering(x, y, w, h),
            [x, y],
            [w, h],
            rounded(tint),
            OUTLINE_DEPTH,
            Some(tint),
        );
    }
}

fn draw(
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &MatchState,
    huds: &mut SwSidebarHuds,
    labels: CameoStatusLabels<'_>,
    ready_tint: [f32; 3],
) {
    let sw = &match_state.match_presentation.sw_sidebar;
    if !sw.ui.enabled {
        return;
    }
    let art = match_state.match_presentation.sw_sidebar_art.as_ref();
    // The cameo labels' colour of the layout (`[Cameos] LabelColor=`).
    let hover_tint = crate::render::sidebar_text::side_text_tint(
        match_state
            .match_presentation
            .sidebar_layout_spec
            .label_text,
    );
    let (cursor_x, cursor_y) = match_state.input.cursor_px();
    if let (Some(entry), Some(toggle)) = (
        crate::app::presentation::sw_sidebar::toggle_art(&match_state.match_presentation),
        sw.layout.toggle,
    ) {
        picture(&mut huds.chrome, sprite_art(entry, toggle, DEPTH_ART));
        // `ToggleSWButtonClass::Draw` outlines the hovered toggle too.
        if toggle.contains(cursor_x as f32, cursor_y as f32) {
            outline(&mut huds.hover, toggle, hover_tint);
        }
    }
    if !crate::app::presentation::sw_sidebar::expanded(&match_state.match_presentation) {
        return;
    }
    if let Some(rect) =
        crate::app::presentation::sw_sidebar::hovered_entry_rect(&match_state.match_presentation)
    {
        outline(&mut huds.hover, rect, hover_tint);
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
                picture(
                    &mut huds.chrome,
                    sprite_art(
                        entry,
                        Rect {
                            x: column_rect.x,
                            y: column_rect.y,
                            w: column_rect.w,
                            h: entry.pixel_size[1],
                        },
                        DEPTH_ART,
                    ),
                );
            }
            if let Some(entry) = art.entry(SwSidebarArtKind::Bottom) {
                picture(
                    &mut huds.chrome,
                    sprite_art(
                        entry,
                        Rect {
                            x: column_rect.x,
                            y: column_rect.y + column_rect.h - entry.pixel_size[1],
                            w: column_rect.w,
                            h: entry.pixel_size[1],
                        },
                        DEPTH_ART,
                    ),
                );
            }
            if let Some(entry) = art.entry(SwSidebarArtKind::Center) {
                // Each column's center strip repeats behind its buttons.
                let first = column.first_button;
                let end = first + column.button_count;
                for placed in sw.layout.entries.iter().take(end).skip(first) {
                    let half_gap = (center_height - 48.0) / 2.0;
                    picture(
                        &mut huds.chrome,
                        sprite_art(
                            entry,
                            Rect {
                                x: column.x as f32,
                                y: placed.rect.y - half_gap,
                                w: column.width as f32,
                                h: center_height,
                            },
                            DEPTH_ART,
                        ),
                    );
                }
            }
        }
    }

    let atlas = match_state.match_presentation.sidebar_cameo_atlas.as_ref();
    let chrome =
        crate::app::presentation::sidebar_render::current_sidebar_chrome(frontend, match_state);
    let gclock_frames: &[SidebarChromeEntry] = chrome
        .map(|atlas| atlas.gclock_frames.as_slice())
        .unwrap_or(&[]);
    let keys_enabled = crate::app::state::active_rules(frontend, match_state)
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
        picture(&mut huds.cameo, instance);
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
            picture(&mut huds.gclock, clock);
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
                match_state
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
            && renderer.bit_font.darken_texture().is_some()
        {
            // Native screen pixels, like `SWButtonClass::Draw` on
            // `DSurface::Composite`.
            let font = &renderer.bit_font;
            let fit = status_label_fit(font, text, slot.w, None);
            let text_w = font.fitted(fit, |font| font.text_width(text)) as f32;
            let strip_w = text_w + READY_PAD_X * 2.0;
            let strip_h = font.cell_height() + READY_PAD_Y * 2.0;
            let strip = [slot.x + (slot.w - strip_w) * 0.5, slot.y];
            let strip_size = [strip_w, strip_h.min(slot.h)];
            // The label's strip, in white as its lane draws it.
            huds.labels.fill(
                covering(strip[0], strip[1], strip_size[0], strip_size[1]),
                strip,
                strip_size,
                rgb([255; 3]),
                DEPTH_STRIP,
            );
            let text_x = slot.x + (slot.w - text_w) * 0.5;
            let text_y = slot.y + READY_PAD_Y;
            huds.labels.fitted_text(
                covering(text_x, text_y, text_w, font.cell_height()),
                text,
                [text_x, text_y],
                ready_tint,
                DEPTH_TEXT,
                fit,
            );
        }
    }
}

fn readiness_text<'a>(
    view: &crate::sim::superweapon::SuperWeaponView,
    labels: CameoStatusLabels<'a>,
) -> Option<&'a str> {
    superweapon_status_label(Some(&view.display_name), view.display, labels)
        .or(view.is_ready.then_some(labels.ready))
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
