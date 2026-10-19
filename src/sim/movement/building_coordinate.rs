//! Building447E90 (+4C), GetDockCoord447B20 (+A8), GetExitCoords44F640 (+B4).
//! Native receivers: tools/spatial_oracle/building_navigation_coordinate.*;
//! ExitCoords: tools/spatial_oracle/anytown_damage/unit_unlimbo.*.
use crate::rules::object_type::ObjectType;
use crate::sim::components::DriveCoord;
use crate::sim::radio::Contacts;

///The one foundation exit-track point shared by Building ClearBib449540
///and Mission_Unload44DCDD..44DD38. Type+ED4 element10 supplies both signed
///cell words; native subtracts one from X, centres XY and sets requested Z0.
///This is distinct from the configured ExitCoord used by object Unlimbo.
pub(crate) fn factory_exit_track_coordinate(
    current: DriveCoord,
    object: &ObjectType,
) -> DriveCoord {
    let (dx, dy) = crate::sim::docking::building_dock::foundation_exit_pair(&object.foundation, 10)
        .expect("every native foundation exit row retains30 pair slots");
    let x = ((current.x / 256) as i16)
        .wrapping_add(dx as i16)
        .wrapping_sub(1);
    let y = ((current.y / 256) as i16).wrapping_add(dy as i16);
    DriveCoord {
        x: i32::from(x) * 256 + 128,
        y: i32::from(y) * 256 + 128,
        z: 0,
    }
}

/// A war factory's exit Cell: the Cell of its Location (`+0x9C`, signed
/// truncation) plus (4, 1). Mission_Unload sends a non-Drive product there
/// (`0x0044DF1C`), and Unit Assign_Destination's WeaponsFactory arm lets only
/// that destination through (`0x00742D98..0x00742DFD`).
pub(crate) fn war_factory_exit_cell(location: DriveCoord) -> (u16, u16) {
    (
        ((location.x / 256) as i16).wrapping_add(4) as u16,
        ((location.y / 256) as i16).wrapping_add(1) as u16,
    )
}

/// Read the destination's type, physical coordinate, center and sparse contact
/// slots at the call boundary. The requester coordinate is read only for Bunker;
/// ordinary targets and uncontacted docks do not need an approach direction.
pub(super) fn navigation_coordinate(
    current: DriveCoord,
    center: DriveCoord,
    object: &ObjectType,
    contacts: &Contacts,
    requester: Option<u64>,
    requester_coordinate: impl FnOnce() -> Result<DriveCoord, String>,
) -> Result<DriveCoord, String> {
    //447E90 chooses +A8 only for these three flags. A refinery alone still
    // returns +48, even though GetDockCoord has a refinery-specific arm.
    if !(object.helipad || object.unit_repair || object.bunker) {
        return Ok(center);
    }
    dock_coordinate(
        current,
        center,
        object,
        contacts,
        requester,
        requester_coordinate,
    )
}

/// `BuildingClass::GetDockCoord @ 0x00447B20` (vtable +0xA8), which the
/// DOCKING receiver (`0x0043C91B`) and Per_Cell_Process (`0x0073A3B1`) call
/// directly for a refinery, outside the +4C flag gate above.
pub(crate) fn dock_coordinate(
    current: DriveCoord,
    center: DriveCoord,
    object: &ObjectType,
    contacts: &Contacts,
    requester: Option<u64>,
    requester_coordinate: impl FnOnce() -> Result<DriveCoord, String>,
) -> Result<DriveCoord, String> {
    //447B2D: Weeder+16BC, NOT Shipyard. ReadBool4604C1 uses81AC50="Weeder".
    if object.weeder {
        return Ok(DriveCoord {
            x: i32::from(((current.x / 256) as i16).wrapping_add(2)) * 256 + 128,
            y: i32::from(((current.y / 256) as i16).wrapping_add(1)) * 256 + 128,
            z: current.z,
        });
    }
    //447B9E: Refinery+16BB (ReadBool460A67, literal81AA5C).
    if object.refinery {
        return Ok(DriveCoord {
            x: center.x.wrapping_add(128),
            ..center
        });
    }
    if object.bunker && requester.is_some() {
        let target = requester_coordinate()?;
        let facing = crate::util::direction_tables::facing16_between(
            [center.x, center.y],
            [target.x, target.y],
        );
        //447C44 rounds the full DirStruct to a byte before quadrant selection.
        // A high-byte truncation or sign-only quadrant differs near the axes.
        let direction = crate::util::direction_tables::round_facing16_to_8(facing);
        return Ok(DriveCoord {
            x: center
                .x
                .wrapping_add(if direction < 128 { 128 } else { -128 }),
            y: center.y.wrapping_add(if (64..192).contains(&direction) {
                128
            } else {
                -128
            }),
            z: center.z,
        });
    }
    if !(object.helipad || object.unit_repair) {
        return Ok(center);
    }
    let slot = match object.number_of_docks {
        0 => None,
        1 => Some(0),
        count => requester
            .and_then(|id| contacts.find_slot(id))
            .filter(|&slot| (slot as i64) < i64::from(count)),
    };
    // The native type allocation zero-initializes undeclared offsets. VERA's
    // art owner omits an all-zero array; read that representation as zero,
    // without a competing default dock position or inferred slot reservation.
    let (x, y, z) = slot
        .and_then(|slot| object.pads.get(slot))
        .map_or((0, 0, 0), |pad| pad.lepton_offset);
    Ok(DriveCoord {
        x: center.x.wrapping_add(x),
        y: center.y.wrapping_add(y),
        z: center.z.wrapping_add(z),
    })
}

/// BuildingClass::GetExitCoords44F640, virtual+B4. A nonempty ExitCoord adds
/// to physical Location at44F678..44F6AA. The constructor's missing vector and
/// an explicit all-zero vector call the existing GetCoords+48 owner at44F6BC.
/// Native execution: anytown_damage/unit_unlimbo's factory_exit_coordinate_rows.
pub(crate) fn exit_coordinate(
    current: DriveCoord,
    object: &ObjectType,
    centre: impl FnOnce() -> DriveCoord,
) -> DriveCoord {
    let Some((x, y, z)) = object.exit_coord.filter(|&coord| coord != (0, 0, 0)) else {
        return centre();
    };
    DriveCoord {
        x: current.x.wrapping_add(x),
        y: current.y.wrapping_add(y),
        z: current.z.wrapping_add(z),
    }
}

#[cfg(test)]
#[path = "building_coordinate_tests.rs"]
mod tests;
