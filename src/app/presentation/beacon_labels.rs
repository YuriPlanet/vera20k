//! The label box under a beacon: the tail of `BeaconClass::Draw
//! 0x00430250` (`0x004302FB..0x00430568`), after the beacon shape.
//!
//! A record with a label draws a box 32 px below the beacon's point: the
//! label measured in GAME.FNT at most 200 px wide (`MeasureText 0x00433CF0`),
//! plus 4 px on each side, centred on the point. The box is clipped to the
//! tactical view, filled black and outlined 1 px in the owning House's
//! colour (its ColorScheme `+0x308`, `HSV_To_RGB 0x00517440`, packed to the
//! surface format); the text is drawn 4 px inside in that colour.
//!
//! RESIDUAL: native draws each beacon's label right after its own shape, so
//! a later beacon's shape can cover an earlier label; VERA draws every
//! label after every shape. Trigger: two labelled beacons a few cells
//! apart. Frequency: rare. BitFont's byte `+0x41`, set to 1 before the text
//! (`0x004304E7`), is not modelled.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::render::batch::SpriteInstance;

/// `MeasureText(.., 200)` (`0x004303CE`) and DrawText's width (`0x0043054E`).
const WRAP_PX: u32 = 200;
/// The box top's drop below the point (`0x00430407`).
const DROP_PX: i32 = 0x20;
/// The box's padding on each side and the text's inset
/// (`0x004303F5`, `0x00430412`, `0x0043054B`, `0x00430553`).
const PAD_PX: i32 = 4;
/// `timeGetTime() % 500 < 250` (`0x0043032B..0x00430340`).
const CURSOR_PERIOD_MS: u64 = 500;
const CURSOR_BLANK_MS: u64 = 250;

/// The label as drawn (`0x0043030F..0x004303BF`). A selected beacon while
/// an edit is open keeps its trailing cursor `_`, blanked to a space for
/// the first half of each 500 ms; any other label loses a trailing `_`, or
/// failing that a trailing space.
pub(crate) fn shown_text(label: &[u16], selected: bool, editing: bool, now_ms: u64) -> Vec<u16> {
    let cursor = u16::from(b'_');
    let space = u16::from(b' ');
    let mut text = label.to_vec();
    if selected && editing {
        if now_ms % CURSOR_PERIOD_MS < CURSOR_BLANK_MS
            && let Some(last) = text.last_mut()
            && *last == cursor
        {
            *last = space;
        }
        return text;
    }
    if text
        .last()
        .is_some_and(|last| *last == cursor || *last == space)
    {
        text.pop();
    }
    text
}

/// The box (`0x004303EF..0x00430428`): `[x, y, width, height]` from the
/// beacon's point and the measured text. The half width truncates toward
/// zero (`CDQ; SUB; SAR`).
pub(crate) fn label_box(point: [i32; 2], measured: [i32; 2]) -> [i32; 4] {
    [
        point[0] - measured[0] / 2 - PAD_PX,
        point[1] + DROP_PX,
        measured[0] + 2 * PAD_PX,
        measured[1] + 2 * PAD_PX,
    ]
}

/// `Rect_Intersect 0x00421B60` of two `[x, y, width, height]` rectangles.
fn intersect(a: [i32; 4], b: [i32; 4]) -> Option<[i32; 4]> {
    let left = a[0].max(b[0]);
    let top = a[1].max(b[1]);
    let right = (a[0] + a[2]).min(b[0] + b[2]);
    let bottom = (a[1] + a[3]).min(b[1] + b[3]);
    (right > left && bottom > top).then_some([left, top, right - left, bottom - top])
}

fn quad(rect: [i32; 4], tint: [f32; 3], camera: [f32; 2]) -> SpriteInstance {
    SpriteInstance {
        position: [rect[0] as f32 + camera[0], rect[1] as f32 + camera[1]],
        size: [rect[2] as f32, rect[3] as f32],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        tint,
        alpha: 1.0,
        ..Default::default()
    }
}

/// Every visible labelled beacon's fill, outline and text quads, in the
/// frame's pixels plus the world camera the UI pass subtracts.
pub(crate) fn instances(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &MatchState,
    persistence: &PersistenceState,
) -> (
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
    Vec<SpriteInstance>,
) {
    let (mut fills, mut outlines, mut texts) = (Vec::new(), Vec::new(), Vec::new());
    let Some(runtime) = match_state.sim_runtime.runtime() else {
        return (fills, outlines, texts);
    };
    // The labels' built-in template (`health_templates`) may hide or move them.
    use crate::app::presentation::health_templates::{Mark, family};
    let Some(offset) = family(Mark::BeaconLabel) else {
        return (fills, outlines, texts);
    };
    let presentation = &match_state.match_presentation;
    let editing = presentation.message_list.editing();
    let now_ms = crate::app::input::tooltips::now_ms(diag, match_state);
    let font = &renderer.bit_font;
    let camera = [match_state.input.camera_x, match_state.input.camera_y];
    let zoom = match_state.input.zoom_level;
    let (left, top, width, height) = crate::app::input::camera::tactical_viewport_px(
        platform,
        renderer,
        &frontend.screen,
        &match_state.match_presentation,
        persistence,
    );
    let bounds = [left as i32, top as i32, width as i32, height as i32];
    let rgb565 = crate::render::native_surface_format::RGB565;
    for (house, _, beacon) in presentation.beacons.iter() {
        // 0x004302FB..0x00430309: a record with no label draws no box.
        if beacon.label().is_empty()
            || !crate::app::match_runtime::beacons::visible_to_player(
                &match_state.sim_runtime,
                house,
                beacon,
            )
        {
            continue;
        }
        let Some(&owner) = runtime.simulation.houses().order().get(house) else {
            continue;
        };
        let text = String::from_utf16_lossy(&shown_text(
            beacon.label(),
            beacon.selected,
            editing,
            now_ms,
        ));
        let layout = font.wrap_layout(&text, WRAP_PX);
        // MeasureText starts one line high, so an empty label is one line.
        let measured = if text.is_empty() {
            [0, font.cell_height() as i32]
        } else {
            [layout.width as i32, layout.height as i32]
        };
        let [x, y, z] = beacon.coord;
        let (wx, wy) = crate::util::lepton::absolute_leptons_to_screen(x, y, z);
        let point = [
            (((wx - camera[0]) * zoom) as i32).saturating_add(offset[0]),
            (((wy - camera[1]) * zoom) as i32).saturating_add(offset[1]),
        ];
        let rect = label_box(point, measured);
        let Some(clipped) = intersect(rect, bounds) else {
            continue;
        };
        let rgb = crate::app::input::messages::player_line_rgb(frontend, match_state, owner)
            .map(|channel| (channel * 255.0).round() as u8);
        let tint = rgb565
            .unpack_rgb8(rgb565.pack_rgb8(rgb))
            .map(|channel| f32::from(channel) / 255.0);
        // Fill_Rect (vt+0x14) black, then Draw_Rect (vt+0x58), both on the
        // clipped box (0x004304C8..0x004304E4).
        fills.push(quad(clipped, [0.0; 3], camera));
        let [cx, cy, cw, ch] = clipped;
        outlines.extend(
            [
                [cx, cy, cw, 1],
                [cx, cy + ch - 1, cw, 1],
                [cx, cy, 1, ch],
                [cx + cw - 1, cy, 1, ch],
            ]
            .map(|edge| quad(edge, tint, camera)),
        );
        let line_height = font.cell_height() as f32;
        for (index, span) in layout.lines.iter().enumerate() {
            texts.extend(font.build_text(
                &text[span.start_byte..span.end_byte],
                (rect[0] + PAD_PX) as f32,
                (rect[1] + PAD_PX) as f32 + index as f32 * line_height,
                1.0,
                0.0,
                tint,
                camera,
            ));
        }
    }
    (fills, outlines, texts)
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn the_cursor_blinks_only_on_the_edited_beacon_and_is_cut_elsewhere() {
        let typing = units("Go_");
        assert_eq!(shown_text(&typing, true, true, 1249), units("Go "));
        assert_eq!(shown_text(&typing, true, true, 1250), typing);
        // Not being edited: the cursor, or else Enter's trailing space, goes.
        assert_eq!(shown_text(&typing, true, false, 1249), units("Go"));
        assert_eq!(shown_text(&units("Go "), false, true, 0), units("Go"));
        assert_eq!(shown_text(&units("_"), false, false, 0), units(""));
    }

    #[test]
    fn the_box_hangs_32_px_below_the_point_with_4_px_padding() {
        assert_eq!(label_box([100, 50], [31, 12]), [100 - 15 - 4, 82, 39, 20]);
        assert_eq!(
            intersect([-5, 10, 20, 20], [0, 0, 640, 480]),
            Some([0, 10, 15, 20])
        );
        assert_eq!(intersect([700, 10, 20, 20], [0, 0, 640, 480]), None);
    }
}
