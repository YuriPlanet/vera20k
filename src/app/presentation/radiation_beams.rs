//! RadBeam kind1 registry659110/6591B0, in reverse birth order. Its copied
//! endpoints have no detach links. Every Tactical composite draws once,
//! increments progress, and removes expired entries, even with no pixels.

use crate::render::radiation_beam::{BeamGeometry, segment_spacing};
use crate::render::surface_line::{SurfaceDraw, SurfaceLineViewport};
use crate::sim::combat::radiation_beam::RadiationBeamBirth;

struct Beam {
    birth: RadiationBeamBirth,
    progress: i32,
    spacing: i32,
    geometry: Option<BeamGeometry>,
}

#[cfg(test)]
#[path = "radiation_beams_tests.rs"]
mod tests;

#[derive(Debug, serde::Serialize)]
pub(crate) struct BeamObservation {
    birth_frame: i32,
    from: [i32; 3],
    to: [i32; 3],
    z_adjust: i32,
    color: [u8; 3],
    duration: i32,
    amplitude_bits: u64,
    progress: i32,
    spacing: i32,
    length_bits: Option<u64>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct BeamCompositeObservation {
    birth_frame: i32,
    progress_before: i32,
    progress_after: i32,
    expired: bool,
    segments: usize,
}

#[derive(Default)]
pub(crate) struct RadiationBeams {
    live: Vec<Beam>,
    /// Derived last-composite work, rebuilt exactly once per composite.
    draws: Vec<SurfaceDraw>,
    composites: Vec<BeamCompositeObservation>,
}

impl RadiationBeams {
    /// `house_rgb` resolves Phobos `Beam.IsHouseColor` to the firing House's
    /// laser color before the copied birth is stored.
    pub(crate) fn create(
        &mut self,
        mut birth: RadiationBeamBirth,
        house_rgb: impl FnOnce(crate::sim::intern::InternedId) -> [u8; 3],
    ) {
        if birth.house_color
            && let Some(owner) = birth.house
        {
            birth.color = house_rgb(owner);
        }
        self.live.push(Beam {
            birth,
            progress: 0,
            spacing: 10,
            geometry: None,
        });
    }

    pub(crate) fn composite(
        &mut self,
        frame_rate: u32,
        viewport: SurfaceLineViewport,
        mut visible: impl FnMut(&RadiationBeamBirth) -> bool,
    ) {
        self.draws.clear();
        self.composites.clear();
        if self.live.is_empty() {
            return;
        }
        // Ordinary PrepareSession52DABD seeds RequestedFPSA8B558=30.
        // Network timing Event4C807D is a separate producer not represented
        // here; display refresh and game speed are never substituted for it.
        let spacing = segment_spacing(frame_rate, 30);
        for beam in self.live.iter_mut().rev() {
            let geometry = *beam
                .geometry
                .get_or_insert_with(|| BeamGeometry::new(beam.birth.from, beam.birth.to));
            let before = self.draws.len();
            beam.spacing = spacing;
            if visible(&beam.birth) {
                geometry.lines(beam.birth, beam.progress, spacing, viewport, |line| {
                    self.draws.push(SurfaceDraw::Line(line))
                });
            }
            let progress_before = beam.progress;
            beam.progress = beam.progress.wrapping_add(1);
            self.composites.push(BeamCompositeObservation {
                birth_frame: beam.birth.frame,
                progress_before,
                progress_after: beam.progress,
                expired: beam.progress >= beam.birth.duration,
                segments: self.draws.len() - before,
            });
        }
        self.live.retain(|beam| beam.progress < beam.birth.duration);
    }

    pub(crate) fn draws(&self) -> &[SurfaceDraw] {
        &self.draws
    }
    pub(crate) fn composites(&self) -> &[BeamCompositeObservation] {
        &self.composites
    }
    pub(crate) fn observation(&self) -> Vec<BeamObservation> {
        self.live
            .iter()
            .map(|beam| BeamObservation {
                birth_frame: beam.birth.frame,
                from: [beam.birth.from.x, beam.birth.from.y, beam.birth.from.z],
                to: [beam.birth.to.x, beam.birth.to.y, beam.birth.to.z],
                z_adjust: beam.birth.z_adjust,
                color: beam.birth.color,
                duration: beam.birth.duration,
                amplitude_bits: beam.birth.amplitude.bits(),
                progress: beam.progress,
                spacing: beam.spacing,
                length_bits: beam.geometry.map(|geometry| geometry.length().bits()),
            })
            .collect()
    }
}
