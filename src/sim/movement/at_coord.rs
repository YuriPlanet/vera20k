//! Original locomotor Is_At_Coord projection, shared by live and snapshot callers.
//!
//! Drive4B4920 / Ship6A3F50 / Walk75CA80 / Hover517210. The caller supplies the
//! active instance's retained state in raw world leptons; this query never
//! reconstructs a committed head from a path or newly changed terrain.
//! Native comparisons: tools/spatial_oracle/locomotor_at_coord.{py,json}.

use super::drive_track::{raw_track_meta, raw_track_points, transform_track_point, turn_track_at};
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::DriveCoord;
use crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;

const NULL_COORD: DriveCoord = DriveCoord { x: 0, y: 0, z: 0 };

/// Native selectors are retained independently from the active curve object.
/// In particular, Is_At_Coord does not gate them on track_valid/IsOnTrack.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AtCoordTrack {
    pub turn_index: i32,
    pub cursor: i32,
    pub reversed: bool,
}

impl Default for AtCoordTrack {
    fn default() -> Self {
        Self {
            turn_index: -1,
            cursor: 0,
            reversed: false,
        }
    }
}

/// A value projection can be retained by a snapshot consumer. A live callback
/// instead constructs a fresh projection from the currently active instance.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AtCoordQuery {
    head: DriveCoord,
    handoff: Option<DriveCoord>,
    reject_null_head: bool,
}

impl AtCoordQuery {
    /// Fresh projection after a synchronous callback. Both the active instance
    /// and its retained head are reread; a path is not a substitute for either.
    pub(crate) fn from_entity(entity: &crate::sim::game_entity::GameEntity) -> Option<Self> {
        let locomotor = entity.locomotor.as_ref()?;
        let track = match locomotor.kind {
            LocomotorKind::Drive => entity.drive_locomotion.as_ref().map(|s| s.track),
            LocomotorKind::Ship => entity.ship_locomotion.as_ref().map(|s| s.track),
            _ => None,
        }
        .unwrap_or_default();
        let head = super::foot_coordinate::stored_head(entity);
        let current = super::foot_coordinate::current_coordinate(entity);
        Self::from_state(
            locomotor.kind,
            current,
            head,
            AtCoordTrack {
                turn_index: track.turn_index,
                cursor: track.cursor,
                reversed: track.reversed,
            },
        )
    }

    pub(crate) fn head_z(self) -> i32 {
        self.head.z
    }

    pub(crate) fn from_state(
        kind: LocomotorKind,
        current: DriveCoord,
        stored_head: Option<DriveCoord>,
        track: AtCoordTrack,
    ) -> Option<Self> {
        let track_family = matches!(kind, LocomotorKind::Drive | LocomotorKind::Ship);
        if !track_family && !matches!(kind, LocomotorKind::Walk | LocomotorKind::Hover) {
            // Active Fly/Jumpjet/Rocket/Teleport share the false leaf4B6630.
            // Dormant TS classes are deliberately not implemented here.
            return None;
        }
        let stored = stored_head.unwrap_or(NULL_COORD);
        let head = super::foot_coordinate::head_or_current(stored_head, current);
        let handoff = if track_family && head != NULL_COORD && !track.reversed {
            usize::try_from(track.turn_index)
                .ok()
                .filter(|&index| kind != LocomotorKind::Ship || index < 64)
                .and_then(turn_track_at)
                .filter(|turn| turn.normal_track != 0)
                .and_then(|turn| {
                    let index = raw_track_meta(turn.normal_track)?.occupation_handoff_point_index;
                    if index < 0 || track.cursor >= i32::from(index) {
                        return None;
                    }
                    let point = raw_track_points(turn.normal_track).get(index as usize)?;
                    let (x, y, _) =
                        transform_track_point(point.x, point.y, point.facing, turn.flags);
                    // Original transform4B4780/6A3DB0 uses the stored head XY,
                    // even if Head_To just fell back from NullCoord to current.
                    Some(DriveCoord {
                        x: stored.x.wrapping_add(i32::from(x)),
                        y: stored.y.wrapping_add(i32::from(y)),
                        z: current.z,
                    })
                })
        } else {
            None
        };
        Some(Self {
            head,
            handoff,
            reject_null_head: track_family,
        })
    }

    pub(crate) fn matches(self, probe: DriveCoord) -> bool {
        if self.reject_null_head && self.head == NULL_COORD {
            return false;
        }
        self.handoff
            .is_some_and(|point| matches_coord(point, probe))
            || matches_coord(self.head, probe)
    }

    /// The bridge peer snapshot retains these same native signed cell words;
    /// its separate live-height test supplies the Z comparison.
    pub(crate) fn cells(self) -> (Option<(i16, i16)>, (i16, i16)) {
        let cell = |point: DriveCoord| ((point.x / 256) as i16, (point.y / 256) as i16);
        (self.handoff.map(cell), cell(self.head))
    }
}

fn matches_coord(point: DriveCoord, probe: DriveCoord) -> bool {
    (point.x / 256) as i16 == (probe.x / 256) as i16
        && (point.y / 256) as i16 == (probe.y / 256) as i16
        && point.z.wrapping_sub(probe.z).wrapping_abs() <= GROUND_LEVEL_HEIGHT_LEPTONS
}

#[cfg(test)]
#[path = "at_coord_tests.rs"]
mod tests;
