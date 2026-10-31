//! Copied LaserDraw lifetimes, updated by Logic550150 and drawn by Tactical
//! 550240 in reverse birth order. These are neither AbstractClass objects nor
//! attached source/target pointers. Source death does not detach a beam.

use crate::render::laser::{LaserDraw, LaserDrawColor};
use crate::sim::combat::laser::{LaserBirth, LaserColor};
use crate::sim::rng::MainRngDraws;
use crate::sim::timer::CdTimer;

struct Laser {
    birth: LaserBirth,
    /// Derived draw colors. Birth owns base outer/spread; each composite
    /// replaces its sampled outer once, before GPU upload and EBolt draws.
    color: LaserDrawColor,
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
    outer: [u8; 3],
    spread: [u8; 3],
    fade_end_bits: u32,
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
    /// fades=true, blinks=false and intensity start1. House RGB is bound
    /// here once; future source death/recoloring cannot change the copy.
    pub(crate) fn create(
        &mut self,
        mut birth: LaserBirth,
        house_rgb: impl FnOnce(crate::sim::intern::InternedId) -> [u8; 3],
    ) {
        let color = match birth.color {
            LaserColor::House(owner) => LaserDrawColor::House(house_rgb(owner)),
            LaserColor::Explicit { inner, outer, .. } => LaserDrawColor::Explicit {
                inner,
                outer: (outer != [0; 3]).then_some(outer),
            },
            LaserColor::HouseWithHalfOuter { owner, spread } => {
                // DiskAI4A7526..4A7578 resolves House56FC, halves the
                // outer RGB and retains weapon spread. Its +20 stays0,
                // so this is GeneralDraw, not the House Special route.
                let inner = house_rgb(owner);
                let outer = inner.map(|value| value >> 1);
                birth.color = LaserColor::Explicit {
                    inner,
                    outer,
                    spread,
                };
                LaserDrawColor::Explicit {
                    inner,
                    outer: (outer != [0; 3]).then_some(outer),
                }
            }
        };
        self.live.push(Laser {
            birth,
            color,
            age: 0,
            timer: CdTimer::started(birth.frame, 1),
        });
    }

    /// Tactical550240 visits reverse birth order on every composite,
    /// including repeated draws without Logic. General55048F..550550
    /// requests each channel's Main ranged draw before sampling detail or
    /// clipping. Equal zero bounds still make three requests, with no raw
    /// stream advance. Only this owner refreshes the derived color cache.
    pub(crate) fn composite(&mut self, main: &mut MainRngDraws) {
        for laser in self.live.iter_mut().rev() {
            if laser.birth.duration <= 0 {
                continue;
            }
            let LaserColor::Explicit { outer, spread, .. } = laser.birth.color else {
                continue;
            };
            if outer == [0; 3] {
                continue;
            }
            let sampled = std::array::from_fn(|channel| {
                let amount = i32::from(spread[channel]);
                (i32::from(outer[channel]) + main.ranged(-amount, amount)).clamp(0, 255) as u8
            });
            let LaserDrawColor::Explicit { outer, .. } = &mut laser.color else {
                unreachable!("resolved General birth retains its draw-color arm");
            };
            *outer = Some(sampled);
        }
    }

    /// 550150 visits backwards. At most one increment/restart per visit,
    /// including a skipped frame counter; it never catches up elapsed frames.
    /// Phobos `LaserDrawClass_Update550173` refreshes the tracked endpoints
    /// here whenever the world is available.
    pub(crate) fn update(
        &mut self,
        frame: i32,
        world: Option<(
            &crate::sim::world::Simulation,
            &crate::rules::ruleset::RuleSet,
        )>,
    ) {
        for laser in self.live.iter_mut().rev() {
            if laser.timer.expired(frame) {
                laser.age = laser.age.wrapping_add(1);
                laser.timer.start(frame, 1);
            }
            if let Some((sim, rules)) = world {
                laser.birth.update_tracking(sim, rules);
            }
        }
        self.live.retain(|laser| laser.age < laser.birth.duration);
    }

    pub(crate) fn draws(&self) -> impl Iterator<Item = LaserDraw> + '_ {
        self.live.iter().rev().map(|laser| LaserDraw {
            from: laser.birth.from,
            to: laser.birth.to,
            z_adjust: laser.birth.z_adjust,
            width: laser.birth.width,
            supported: laser.birth.supported,
            color: laser.color,
            age: laser.age,
            duration: laser.birth.duration,
            fade_end: laser.birth.fade_end,
        })
    }

    pub(crate) fn observations(&self) -> impl Iterator<Item = LaserObservation> + '_ {
        self.live.iter().map(|laser| {
            let (outer, spread) = match laser.birth.color {
                LaserColor::Explicit { outer, spread, .. } => (outer, spread),
                _ => ([0; 3], [0; 3]),
            };
            LaserObservation {
                birth_frame: laser.birth.frame,
                from: [laser.birth.from.x, laser.birth.from.y, laser.birth.from.z],
                to: [laser.birth.to.x, laser.birth.to.y, laser.birth.to.z],
                z_adjust: laser.birth.z_adjust,
                width: laser.birth.width,
                supported: laser.birth.supported,
                house_color: matches!(laser.birth.color, LaserColor::House(_)),
                rgb: laser.color.inner(),
                outer,
                spread,
                fade_end_bits: laser.birth.fade_end.bits(),
                duration: laser.birth.duration,
                age: laser.age,
                timer_start: laser.timer.start_frame(),
                timer_duration: laser.timer.duration(),
            }
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
