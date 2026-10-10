//! Copied LaserDraw lifetimes, updated by Logic550150 and drawn by Tactical
//! 550240 in reverse birth order. These are neither AbstractClass objects nor
//! attached source/target pointers. Source death does not detach a beam.

use crate::render::laser::LaserDraw;
use crate::sim::combat::laser::{LaserBirth, LaserColor};
use crate::sim::timer::CdTimer;

struct Laser {
    birth: LaserBirth,
    rgb: [u8; 3],
    age: i32,
    timer: CdTimer,
}

/// Read-only retained birth/lifetime data, in append order. Observing it does
/// not update the timer or call the draw/detail owners.
#[derive(Debug, serde::Serialize)]
pub(crate) struct LaserObservation {
    birth_frame: i32,
    from: [i32; 3],
    to: [i32; 3],
    z_adjust: i32,
    width: i32,
    supported: bool,
    house_color: bool,
    rgb: [u8; 3],
    duration: i32,
    age: i32,
    timer_start: i32,
    timer_duration: i32,
}

#[derive(Default)]
pub(crate) struct Lasers {
    live: Vec<Laser>,
}

impl Lasers {
    /// 54FE60 copies inputs once; ctor timer is {Frame,1}, step/rate1,
    /// fades=true, blinks=false and intensity endpoints1/0 for these callers.
    pub(crate) fn create(
        &mut self,
        birth: LaserBirth,
        house_rgb: impl FnOnce(crate::sim::intern::InternedId) -> [u8; 3],
    ) {
        let rgb = match birth.color {
            LaserColor::House(owner) => house_rgb(owner),
            LaserColor::Explicit { inner, .. } => inner,
        };
        self.live.push(Laser {
            birth,
            rgb,
            age: 0,
            timer: CdTimer::started(birth.frame, 1),
        });
    }

    /// 550150 visits backwards. At most one increment/restart per visit,
    /// including a skipped frame counter; it never catches up elapsed frames.
    pub(crate) fn update(&mut self, frame: i32) {
        for laser in self.live.iter_mut().rev() {
            if laser.timer.expired(frame) {
                laser.age = laser.age.wrapping_add(1);
                laser.timer.start(frame, 1);
            }
        }
        self.live.retain(|laser| laser.age < laser.birth.duration);
    }

    pub(crate) fn draws(&self) -> impl Iterator<Item = LaserDraw> + '_ {
        self.live.iter().rev().filter_map(|laser| {
            // RESIDUAL: non-house DrawBeam550260 consumes Main RNG for
            // spread. That separate caller path is not replaced by a client
            // RNG or this house-color DrawBeamSpecial5509F0 port.
            if !matches!(laser.birth.color, LaserColor::House(_)) {
                return None;
            }
            Some(LaserDraw {
                from: laser.birth.from,
                to: laser.birth.to,
                z_adjust: laser.birth.z_adjust,
                width: laser.birth.width,
                supported: laser.birth.supported,
                rgb: laser.rgb,
                age: laser.age,
                duration: laser.birth.duration,
            })
        })
    }

    pub(crate) fn observations(&self) -> impl Iterator<Item = LaserObservation> + '_ {
        self.live.iter().map(|laser| LaserObservation {
            birth_frame: laser.birth.frame,
            from: [laser.birth.from.x, laser.birth.from.y, laser.birth.from.z],
            to: [laser.birth.to.x, laser.birth.to.y, laser.birth.to.z],
            z_adjust: laser.birth.z_adjust,
            width: laser.birth.width,
            supported: laser.birth.supported,
            house_color: matches!(laser.birth.color, LaserColor::House(_)),
            rgb: laser.rgb,
            duration: laser.birth.duration,
            age: laser.age,
            timer_start: laser.timer.start_frame(),
            timer_duration: laser.timer.duration(),
        })
    }

    /// ScenarioLoad67E739 -> ClearScene6851F0 -> 534949 -> ClearAll550000.
    /// No laser data is serialized, resurrected, or reconstructed on restore.
    pub(crate) fn clear_on_load(&mut self) {
        self.live.clear();
    }
}

#[cfg(test)]
#[path = "laser_lifetime_tests.rs"]
mod tests;
