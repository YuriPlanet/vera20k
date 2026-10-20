//! Unit73C1D2..73C5C4's Jumpjet shadow suffix. Unlike Aircraft414687,
//! it keeps the live height and draws after the body. Original execution:
//! tools/voxel_oracle/aircraft_shadow.py --unit (unit_shadow.json).

use crate::render::foot_depth::DepthCell;
use crate::util::lepton::absolute_leptons_to_screen;
use crate::util::native_x87::adjust_for_z_standard;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnitShadowMask {
    None,
    Retained,
    Fresh,
}

pub(crate) struct UnitShadowContext {
    pub xyz: [i32; 3],
    pub on_bridge: bool,
    pub marked: bool,
    pub primary_facing: u16,
}

pub(crate) struct UnitShadowPose {
    pub anchor: [f32; 2],
    pub facing: u8,
    pub slope: u8,
    pub mask: UnitShadowMask,
    pub mask_offset: [i32; 2],
}

pub(crate) fn pose(
    context: UnitShadowContext,
    lookup: impl FnOnce([i16; 2]) -> DepthCell,
) -> Option<UnitShadowPose> {
    let [x, y, z] = context.xyz;
    let cell = lookup([(x / 256) as i16, (y / 256) as i16]);
    let floor =
        crate::util::lepton::ground_height_leptons(cell.level as u8, cell.ramp, x, y).ok()?;
    let height = crate::sim::movement::ground_pose::height_at_z(z, floor, context.on_bridge);
    // The inherited Shadow_Point55A8C0 is added by Foot4DB0D0. Preserve
    // the body's projection and the point's separate integer conversion.
    let (sx, sy) = absolute_leptons_to_screen(x, y, z);
    let point_y = adjust_for_z_standard(height);
    //73C423/73C468/73C472: high flight has no mask; positive low flight
    //sets key -1, while a grounded mask has its own high-bit cache entry.
    // Residual: 73C43F..73C457 takes that key change only for BalloonHover
    // (+D6A) or SpeedType Track/Wheel (+67C 1/2); other Jumpjets (stock
    // SCHP, SHAD, HIND) keep the plain cached key with the mask present.
    // Unported: those types take this path while grounded or in low flight,
    // likely a cached rather than fresh mask (a pixel-level difference).
    let high_flying = crate::sim::movement::air_movement::object_flight_predicate(
        context.marked,
        || height,
        true,
    );
    let mask = if high_flying {
        UnitShadowMask::None
    } else if height > 0 {
        UnitShadowMask::Fresh
    } else {
        UnitShadowMask::Retained
    };
    Some(UnitShadowPose {
        anchor: [sx, sy + point_y as f32],
        facing: crate::render::vxl_raster::voxel_facing_step_u16(context.primary_facing) * 8,
        slope: cell.ramp,
        mask,
        //73C4C8..73C4D5 adds one only on the positive-height arm. This
        //offset samples the elevated body surface; it does not move the quad.
        mask_offset: [0, point_y.wrapping_add(i32::from(height > 0))],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_unit_jumpjet_shadow_placement_and_mask_modes() {
        let fixture = crate::test_fixture::json("tools/voxel_oracle/unit_shadow.json");
        for case in fixture["callers"].as_array().unwrap() {
            if !case["draw_admitted"].as_bool().unwrap_or(false) {
                continue;
            }
            let input = &case["input"];
            let xyz = std::array::from_fn(|i| input["xyz"][i].as_i64().unwrap() as i32);
            let result = pose(
                UnitShadowContext {
                    xyz,
                    on_bridge: input["on_bridge"].as_bool().unwrap_or(false),
                    marked: input["marked"].as_bool().unwrap_or(true),
                    primary_facing: input["primary"].as_u64().unwrap() as u16,
                },
                |coord| DepthCell {
                    coord,
                    level: input["level"].as_i64().unwrap_or(0) as i8,
                    ramp: input["slope"].as_u64().unwrap_or(0) as u8,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                [result.anchor[0] + 320., result.anchor[1] - 215.],
                std::array::from_fn::<_, 2, _>(|i| case["anchor"][i].as_i64().unwrap() as f32),
                "{input}"
            );
            assert_eq!(
                result.mask_offset,
                std::array::from_fn(|i| case["renderer"]["mask_offset"][i].as_i64().unwrap() as i32),
                "{input}"
            );
            let key = case["cache_key"].as_u64().unwrap() as u32;
            assert_eq!(
                result.mask,
                if key == u32::MAX {
                    UnitShadowMask::Fresh
                } else if key & 0x8000_0000 != 0 {
                    UnitShadowMask::Retained
                } else {
                    UnitShadowMask::None
                },
                "{input}"
            );
            if key != u32::MAX {
                assert_eq!(result.slope, ((key >> 5) & 63) as u8, "{input}");
                assert_eq!(result.facing / 8, (key & 31) as u8, "{input}");
            }
        }
    }
}
