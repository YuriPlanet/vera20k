//! Aircraft DrawIt414687..4148C4's shadow placement, without temporary writes
//! to simulation height/Foot+6B6. The atlas owns the stencil; this caller owns
//! its ground anchor and locomotor-specific inputs. Executable controls:
//! tools/voxel_oracle/aircraft_shadow.py (original Fly/Rocket dispatch).

use crate::render::foot_depth::DepthCell;
use crate::rules::locomotor_type::LocomotorKind;
use crate::util::lepton::{BRIDGE_DECK_HEIGHT_LEPTONS, absolute_leptons_to_screen};
use crate::util::native_x87::adjust_for_z_standard;

pub(crate) struct AircraftShadowContext {
    pub xyz: [i32; 3],
    pub on_bridge: bool,
    pub locomotor: LocomotorKind,
    pub is_dropship: bool,
    pub primary_facing: u16,
    pub secondary_facing: u16,
}

#[derive(Debug)]
pub(crate) struct AircraftShadowPose {
    /// Absolute screen coordinates, including VERA's common +15 row bias.
    pub anchor: [f32; 2],
    /// Native temporary SetHeight's world Z, for the shared depth query.
    pub world_z: i32,
    pub facing: u8,
    pub slope: u8,
}

pub(crate) fn pose(
    context: AircraftShadowContext,
    mut lookup: impl FnMut([i16; 2]) -> DepthCell,
) -> Option<AircraftShadowPose> {
    let facing = match context.locomotor {
        LocomotorKind::Fly => context.secondary_facing,
        LocomotorKind::Rocket => context.primary_facing,
        _ => return None,
    };
    let [x, y, z] = context.xyz;
    let coord = [(x / 256) as i16, (y / 256) as i16];
    let cell = lookup(coord);
    let floor =
        crate::util::lepton::ground_height_leptons(cell.level as u8, cell.ramp, x, y).ok()?;
    let height = crate::sim::movement::ground_pose::height_at_z(z, floor, context.on_bridge);
    //414775..4147EC checks the neighboring bridge segment as well as the
    //current Cell's bridge flag. It uses the Cell's stored coordinate.
    let bridge_height = if height
        >= if context.on_bridge {
            0
        } else {
            BRIDGE_DECK_HEIGHT_LEPTONS
        }
        && cell.flags & 0x100 != 0
    {
        let delta = if cell.flags & 0x800 != 0 {
            [0, -1]
        } else {
            [-1, 0]
        };
        let neighbor = [
            cell.coord[0].wrapping_add(delta[0]),
            cell.coord[1].wrapping_add(delta[1]),
        ];
        if lookup(neighbor).flags & 0x100 != 0 {
            BRIDGE_DECK_HEIGHT_LEPTONS
        } else {
            0
        }
    } else {
        0
    };
    let (sx, mut sy) = absolute_leptons_to_screen(x, y, floor);
    sy -= adjust_for_z_standard(bridge_height) as f32;
    if context.locomotor == LocomotorKind::Rocket {
        //55A8C0 is called twice: before SetHeight by Aircraft, then after it
        //by Foot4DB0D0. Keep both integer conversions and their rounding.
        sy += adjust_for_z_standard(height) as f32;
        sy += adjust_for_z_standard(bridge_height) as f32;
    }
    Some(AircraftShadowPose {
        anchor: [sx, sy],
        world_z: floor
            .wrapping_add(bridge_height)
            .wrapping_add(if context.on_bridge {
                BRIDGE_DECK_HEIGHT_LEPTONS
            } else {
                0
            }),
        facing: crate::render::vxl_raster::voxel_facing_step_u16(facing) * 8,
        slope: if context.locomotor == LocomotorKind::Fly && context.is_dropship {
            0
        } else {
            cell.ramp
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_aircraft_caller_ground_climb_flight_descent_bridge_and_missile() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/voxel_oracle/aircraft_shadow.json",
        ))
        .unwrap();
        for case in fixture["callers"].as_array().unwrap() {
            let input = &case["input"];
            let xyz = std::array::from_fn(|i| input["xyz"][i].as_i64().unwrap() as i32);
            let at = [(xyz[0] / 256) as i16, (xyz[1] / 256) as i16];
            let result = pose(
                AircraftShadowContext {
                    xyz,
                    on_bridge: input["on_bridge"].as_bool().unwrap_or(false),
                    locomotor: if input["locomotor"] == "Fly" {
                        LocomotorKind::Fly
                    } else {
                        LocomotorKind::Rocket
                    },
                    is_dropship: input["dropship"].as_bool().unwrap_or(false),
                    primary_facing: input["primary"].as_u64().unwrap() as u16,
                    secondary_facing: input["secondary"].as_u64().unwrap() as u16,
                },
                |coord| DepthCell {
                    coord,
                    level: if coord == at {
                        input["level"].as_i64().unwrap_or(0) as i8
                    } else {
                        0
                    },
                    ramp: if coord == at {
                        input["slope"].as_u64().unwrap_or(0) as u8
                    } else {
                        0
                    },
                    flags: input[if coord == at {
                        "flags"
                    } else {
                        "neighbor_flags"
                    }]
                    .as_u64()
                    .unwrap_or(0) as u32,
                    ..Default::default()
                },
            )
            .unwrap();
            // Native camera [-320,200]; VERA camera Y includes +15 row bias.
            assert_eq!(
                [result.anchor[0] + 320.0, result.anchor[1] - 215.0],
                [
                    case["anchor"][0].as_i64().unwrap() as f32,
                    case["anchor"][1].as_i64().unwrap() as f32
                ],
                "{input}"
            );
            assert_eq!(
                result.world_z,
                case["temporary_xyz"][2].as_i64().unwrap() as i32,
                "{input}"
            );
            assert_eq!(case["restored_xyz"], input["xyz"]);
            assert_eq!(case["restored_6b6"], 1);
        }
    }
}
