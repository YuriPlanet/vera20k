//! `IonBlastClass::DrawAll @ 0x0053D850`'s admission: the blasts whose
//! ripple the tactical draw edits into the terrain, and where.
//!
//! DrawAll visits the blasts last to first (`0x0053D8B6..0x0053D8D1`). A blast
//! draws only at detail level 2 (`0x00A8EB78`, `0x0053D588`) and when
//! `TacticalClass::CoordsToClient2 @ 0x006D2140` admits its coordinate
//! (`0x0053D5A3`). Its 512x256 frame is centred on that point
//! (`0x0053D600..0x0053D623`) and clipped to the tactical view less its bottom
//! 7 rows (`0x0053D5B0..0x0053D5E5`), and its rows' Z threshold starts from
//! the ZBuffer's row seed less `AdjustForZ(z)` (`0x0053D69D..0x0053D6AE`). The
//! renderer owns the pixels (`render::terrain_draw`'s IonBlast pass).

use crate::render::terrain_draw::IonBlastDraw;
use crate::sim::ion_blast_ripple::{FRAME_HEIGHT, FRAME_WIDTH};

/// The detail level the ripple draws at (`CMP EAX,2` at `0x0053D588`).
const RIPPLE_DETAIL_LEVEL: u32 = 2;
/// The rows the clip leaves off the tactical view's bottom (`0x0053D5D7`).
const CLIP_BOTTOM_ROWS: i32 = 7;
/// ZBuffer `+0x24`, the row seed the threshold starts from (`0x0053D6AA`).
/// `ZBuffer::Pan @ 0x007BCB50` moves it with each vertical scroll; every
/// blitter seeds from it, so VERA's fixed seed gives the same comparisons.
const ROW_SEED: i32 = crate::render::native_z::DEFAULT_Z;

/// DrawAll's draws for `blasts` (coordinate and frame, in vector order) seen
/// through `camera` over the tactical `viewport`, both in world pixels.
pub(super) fn ion_blast_draws(
    blasts: impl DoubleEndedIterator<Item = ([i32; 3], i32)>,
    camera: [f32; 2],
    viewport: [f32; 2],
    detail_level: u32,
) -> Vec<IonBlastDraw> {
    if detail_level != RIPPLE_DETAIL_LEVEL {
        return Vec::new();
    }
    let clip = [
        0,
        0,
        viewport[0] as i32,
        viewport[1] as i32 - CLIP_BOTTOM_ROWS,
    ];
    blasts
        .rev()
        .filter_map(|([x, y, z], frame)| {
            let (px, py) = crate::util::lepton::absolute_leptons_to_screen(x, y, z);
            if !super::instances::projection_admitted([px, py], camera, viewport) {
                return None;
            }
            let point = [
                px as i32 - camera[0].floor() as i32,
                py as i32 - camera[1].floor() as i32,
            ];
            Some(IonBlastDraw {
                origin: [
                    point[0] - FRAME_WIDTH as i32 / 2,
                    point[1] - FRAME_HEIGHT as i32 / 2,
                ],
                clip,
                frame: frame as u32,
                z_seed: ROW_SEED.wrapping_sub(crate::util::native_x87::adjust_for_z_standard(z))
                    as u16,
            })
        })
        .collect()
}

/// This frame's draws from the match's blasts, camera, zoom and options.
pub(super) fn draws(state: &crate::app::AppState) -> Vec<IonBlastDraw> {
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return Vec::new();
    };
    let input = &state.match_state.input;
    let (_, _, width, height) = crate::app::input::camera::tactical_viewport_px(state);
    ion_blast_draws(
        runtime.view().simulation().ion_blasts(),
        [input.camera_x, input.camera_y],
        [
            width as f32 / input.zoom_level,
            height as f32 / input.zoom_level,
        ],
        state
            .match_state
            .match_presentation
            .in_game_options
            .detail_level,
    )
}

#[cfg(test)]
#[path = "ion_blasts_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ion_blasts_gpu_tests.rs"]
mod gpu_tests;
