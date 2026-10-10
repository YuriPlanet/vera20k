//! Original LineTrail556C00 and Surface4BEAC0 ordered pixel producer.
//!
//! The integer three-axis walk includes repeated destination pixels when Z
//! dominates. Its output is consumed in order against the shared live Z and
//! RGB565 destination; it never writes Z. Native goldens: line_trail.json.

use crate::sim::projectile::ProjectileCoord;
use crate::util::native_x87::adjust_for_z_standard;

#[derive(Debug, Clone, Copy)]
pub(crate) struct LineTrailSegment {
    pub from: ProjectileCoord,
    pub to: ProjectileCoord,
    pub color: [u8; 3],
    pub strength: i32,
}

/// The actual Surface4BEAC0 boundary: points relative to the clip origin,
/// signed endpoint adjustments and one constant intensity for this segment.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProjectedLine {
    pub from: [i32; 2],
    pub to: [i32; 2],
    pub z_adjust: [i32; 2],
    pub strength: i32,
}

impl LineTrailSegment {
    /// VERA's absolute pixel frame carries the same +15 row bias as all world
    /// layers. Native556C00 passes the camera-relative projection unchanged;
    /// Surface4BEAC0 adds the clip origin. No motion interpolation enters history.
    pub(crate) fn project(self, camera: [i32; 2]) -> ProjectedLine {
        let project = |coord: ProjectileCoord| {
            let (x, y) = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
            [x as i32 - camera[0], y as i32 - camera[1]]
        };
        ProjectedLine {
            from: project(self.from),
            to: project(self.to),
            z_adjust: [
                (-2i32).wrapping_sub(adjust_for_z_standard(self.from.z)),
                (-2i32).wrapping_sub(adjust_for_z_standard(self.to.z)),
            ],
            strength: self.strength,
        }
    }
}

use super::surface_line::LinePixel;

/// Original4BEAC0 ->7BC2B0 clips XY first, then interpolates the endpoint Z
/// adjustments using truncated integer lengths (4C1B50). Presentation f64 is
/// bounded by native clipped controls, not a simulation arithmetic authority.
pub(crate) fn rasterize(
    line: ProjectedLine,
    clip: [i32; 4],
    z_origin_y: i32,
    emit: impl FnMut(LinePixel),
) {
    if line.strength >= 8 {
        super::surface_line::rasterize_z_clipped(
            line.from,
            line.to,
            line.z_adjust,
            clip,
            z_origin_y,
            emit,
        );
    }
}

#[cfg(test)]
#[path = "line_trail_tests.rs"]
mod tests;
