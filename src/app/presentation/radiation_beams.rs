//! RadBeam registry659110/6591B0, in reverse birth order. Its copied
//! endpoints have no detach links. Every Tactical composite draws once,
//! increments progress, and removes expired entries, even with no pixels; a
//! draw between composites shows the last one's lines ([`RadiationBeams::shown`]).
//! Kind1 beams are the straight copied modes; kind2 are SpawnRadEruption's.

use crate::render::radiation_beam::{BeamGeometry, eruption_cull_percent, segment_spacing};
use crate::render::surface_line::{SurfaceDraw, SurfaceLineViewport};
use crate::sim::combat::radiation_beam::{BeamHouseColor, RadiationBeamBirth};
use crate::sim::rng::MainRngDraws;

struct Beam {
    birth: RadiationBeamBirth,
    progress: i32,
    spacing: i32,
    geometry: Option<BeamGeometry>,
    /// RadBeam `+0x09`, set by the constructor659409. Only the kind2 cull
    /// clears it; a culled eruption keeps aging without drawing.
    active: bool,
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
    /// The camera `draws` are projected through.
    camera: [i32; 2],
    composites: Vec<BeamCompositeObservation>,
}

impl RadiationBeams {
    /// `house_rgb` resolves Phobos and Ares `Beam.IsHouseColor` to the firing
    /// House's laser or House colour before the copied birth is stored.
    pub(crate) fn create(
        &mut self,
        mut birth: RadiationBeamBirth,
        house_rgb: impl FnOnce(crate::sim::intern::InternedId, BeamHouseColor) -> [u8; 3],
    ) {
        if birth.house_color != BeamHouseColor::None
            && let Some(owner) = birth.house
        {
            birth.color = house_rgb(owner, birth.house_color);
        }
        // Allocate659189: the constructor's spacing is10 for kind1, else20.
        self.live.push(Beam {
            birth,
            progress: 0,
            spacing: if birth.eruption { 20 } else { 10 },
            geometry: None,
            active: true,
        });
    }

    /// `main` is process Main886B88: the kind2 cull draws from it.
    pub(crate) fn composite(
        &mut self,
        frame_rate: u32,
        viewport: SurfaceLineViewport,
        main: &MainRngDraws,
        mut visible: impl FnMut(&RadiationBeamBirth) -> bool,
    ) {
        self.draws.clear();
        self.composites.clear();
        self.camera = viewport.camera;
        if self.live.is_empty() {
            return;
        }
        // Ordinary PrepareSession52DABD seeds RequestedFPSA8B558=30.
        // Network timing Event4C807D is a separate producer not represented
        // here; display refresh and game speed are never substituted for it.
        let spacing = segment_spacing(frame_rate, 30);
        // 6591C1..6591F5 counts the kind2 beams and the active ones before
        // any of this pass's culls.
        let eruptions = self.live.iter().filter(|beam| beam.birth.eruption);
        let total = eruptions.clone().count() as i32;
        let active = eruptions.filter(|beam| beam.active).count() as i32;
        let cull = (active > 0).then(|| eruption_cull_percent(frame_rate, 30, total, active));
        for beam in self.live.iter_mut().rev() {
            let geometry = *beam
                .geometry
                .get_or_insert_with(|| BeamGeometry::new(beam.birth.from, beam.birth.to));
            let before = self.draws.len();
            let mut push = |line| self.draws.push(SurfaceDraw::Line(line));
            if !beam.birth.eruption {
                beam.spacing = spacing;
                if visible(&beam.birth) {
                    geometry.lines(beam.birth, beam.progress, spacing, viewport, &mut push);
                }
            } else {
                //6592D6..659301: one Main draw per active kind2 beam.
                if beam.active && cull.is_some_and(|cull| main.ranged(0, 99) < cull) {
                    beam.active = false;
                }
                if beam.active && visible(&beam.birth) {
                    geometry.eruption_lines(
                        beam.birth,
                        beam.progress,
                        beam.spacing,
                        viewport,
                        &mut push,
                    );
                }
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
    pub(crate) fn is_empty(&self) -> bool {
        self.live.is_empty()
    }
    /// The last composite's lines under the current camera.
    pub(crate) fn shown(&mut self, camera: [i32; 2]) -> &[SurfaceDraw] {
        if camera != self.camera {
            for draw in &mut self.draws {
                if let SurfaceDraw::Line(line) = draw {
                    line.follow_camera(self.camera, camera);
                }
            }
            self.camera = camera;
        }
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
