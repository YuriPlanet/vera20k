//! Shared ordering and destination bounds for one voxel unit's body and gun.
//!
//! Atlas pages select textures; the local part order and composite depth
//! rectangle stay attached to the entity on every frontend.

use crate::render::unit_atlas::UnitSpriteEntry;

/// `UnitClass__DrawVoxelBody @ 0x0073B470`, selector
/// `0x0073BC5B..0x0073BDAB`. Building draw43DA80 uses the same
/// rounded-quadrant order (tools/voxel_oracle/building_barrel.json).
pub(crate) fn native_turret_barrel_order<T: Copy>(
    turret_facing: u16,
    turret: T,
    barrel: T,
) -> [T; 2] {
    let facing_high = (turret_facing >> 8) as u8;
    if (32..160).contains(&facing_high) {
        [turret, barrel]
    } else {
        [barrel, turret]
    }
}

/// One parent raster has separate atlas storage and native drawing bounds.
/// Unit73BEA4's waterline and 73B140's split both consume the native union;
/// atlas padding must not move either boundary. Unsplit depth retains its
/// existing storage rectangle until that wider depth path is audited.
#[derive(Clone, Copy)]
pub(crate) struct CompositeDrawBounds {
    padded: [f32; 2],
    pub(crate) native: Option<[i32; 4]>,
}

impl CompositeDrawBounds {
    pub(crate) fn depth_rect(self, wants_split: bool) -> ([f32; 2], bool) {
        if wants_split && let Some([_, y, _, height]) = self.native {
            return ([y as f32, height as f32], true);
        }
        (self.padded, false)
    }
}

pub(crate) fn composite_draw_bounds(
    parts: impl IntoIterator<Item = (UnitSpriteEntry, [f32; 2])>,
) -> CompositeDrawBounds {
    let mut top = f32::INFINITY;
    let mut bottom = f32::NEG_INFINITY;
    let mut native = None;
    let mut all_native = true;
    for (entry, anchor) in parts {
        top = top.min(anchor[1] + entry.offset_y);
        bottom = bottom.max(anchor[1] + entry.offset_y + entry.pixel_size[1]);
        if let Some(mut bounds) = entry.native_draw_bounds {
            bounds[0] += anchor[0] as i32;
            bounds[1] += anchor[1] as i32;
            crate::render::vxl_raster::union_native_voxel_draw_bounds(&mut native, bounds);
        } else {
            all_native = false;
        }
    }
    CompositeDrawBounds {
        padded: if bottom > top {
            [top, bottom - top]
        } else {
            [0.0, 0.0]
        },
        native: all_native.then_some(native).flatten(),
    }
}

#[cfg(all(test, tests_render_s_z))]
mod tests {
    use super::*;

    #[test]
    fn unit_vxl_piece_order_matches_native_selector_for_every_facing() {
        for facing in 0..=u16::MAX {
            let native_quadrant = ((((u32::from(facing) >> 13) + 1) >> 1) & 3) as u8;
            let expected = if matches!(native_quadrant, 1 | 2) {
                ["turret", "barrel"]
            } else {
                ["barrel", "turret"]
            };
            assert_eq!(
                native_turret_barrel_order(facing, "turret", "barrel"),
                expected,
                "facing {facing:#06x}, native quadrant {native_quadrant}"
            );
        }
    }
}
