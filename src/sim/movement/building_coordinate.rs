//! Building447E90 (+4C), GetDockCoord447B20 (+A8) and their live inputs.
//! Full native receiver corpus: tools/spatial_oracle/building_navigation_coordinate.*.
use crate::rules::object_type::ObjectType;
use crate::sim::components::DriveCoord;
use crate::sim::radio::Contacts;

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
        let direction = ((u32::from(facing) >> 7) + 1) >> 1 & 0xFF;
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

#[cfg(test)]
#[path = "building_coordinate_tests.rs"]
mod tests;
