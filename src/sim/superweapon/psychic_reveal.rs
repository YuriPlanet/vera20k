//! The Psychic Reveal's launch: `SuperClass::Launch @ 0x006CC390` case 11
//! (`0x006CD70C..0x006CD7E7`).
//!
//! A charged Super (`+0x6F`) takes the target cell's GetCoords (vt+0x48,
//! `0x00486840`: its centre at its ground height, no bridge deck) and calls
//! `MapClass::RevealArea @ 0x005678E0` around it twice, with `[CombatDamage]
//! PsychicRevealRadius=` (`Rules+0xFEC`), the Super's house and no outline,
//! unreveal or line of sight: final 0 (`0x006CD773`), then final 1
//! (`0x006CD79C`) — [`Simulation::reveal_area`]. It then plays
//! `[AudioVisual] PsychicRevealActivateSound=` at the cell's coordinate
//! (`0x006CD7BF`). No EVA line, anim or radar event.
//!
//! RevealArea lifts the centre toward the map's north by the coordinate's
//! height, reveals nothing when the lifted centre is off the map `Size=`
//! diamond, skips cells off it, clamps the radius to 10, and writes the
//! Super's house's map and, under `[AudioVisual] AllyReveal=`, the map of
//! each house that house counts as allied.
//!
//! The local player's tail (`0x006CD7C4..0x006CD7E2`, then the shared
//! `0x006CD51E`) clears the selected Super and drops the queued
//! `EVA_PsychicRevealReady` line; the app does both from the launch event.
//!
//! Evidence: `tools/superweapon_oracle.py` section `psychic_launch` (Unicorn
//! on gamemd.exe: case 11, RevealArea a recorded stub), replayed in
//! `psychic_reveal_tests.rs`; RevealArea's walk and house gate against
//! `tools/spatial_oracle/reveal_area.py` in `vision/reveal_area_tests.rs`.
//!
//! Activation carries the original signed ground coordinate to the existing
//! retained PlayAtCoord owner; no structural deck or reveal-centre lift is
//! added. The later launch fact owns selection and Ready removal.
//!
//! RESIDUAL: a negative PsychicRevealRadius indexes before RevealArea's
//! count table natively; VERA reveals nothing. No retail value is negative.
//!
//! Ledger: no Scenario draws, timer writes or detach calls.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/vision, sim/world.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

#[cfg(test)]
#[path = "psychic_reveal_tests.rs"]
mod tests;

use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::sim::vision;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};

/// What [`launch`] called, in the oracle's terms (observation only).
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, PartialEq)]
enum Observed {
    /// RevealArea's pair of calls: the coordinate and the radius.
    RevealArea([i32; 3], u16),
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Observation only: what [`launch`] called on this thread while a test
    /// holds `Some`.
    static OBSERVED: std::cell::RefCell<Option<Vec<Observed>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(any(test, feature = "test-support"))]
fn observe(call: Observed) {
    OBSERVED.with(|log| {
        if let Some(log) = log.borrow_mut().as_mut() {
            log.push(call);
        }
    });
}

/// Launch case 11 for `owner`'s Super of type `sw_type` at (target_rx,
/// target_ry): see the module doc. Returns whether the Super was charged.
pub fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    target_rx: u16,
    target_ry: u16,
    sw_type: InternedId,
    frame_effects: FrameEffects<'_>,
) -> bool {
    if !super::is_charged(sim, owner, sw_type) {
        return false;
    }
    let ground = crate::sim::projectile::cell_ground_coord(
        sim.resolved_terrain.as_ref(),
        target_rx,
        target_ry,
    );
    let coord = [ground.x, ground.y, ground.z];
    let radius = rules
        .general
        .psychic_reveal_radius
        .clamp(0, i32::from(u16::MAX)) as u16;
    #[cfg(any(test, feature = "test-support"))]
    observe(Observed::RevealArea(coord, radius));
    let config = sim.sight_reveal_config(rules);
    let viewers = vision::direct_reveal_viewers(
        &sim.fog,
        sim.house_alliances.as_map(),
        owner,
        &config,
        &sim.interner,
    );
    for final_pass in [false, true] {
        let _ = sim.reveal_area(
            &viewers,
            (target_rx, target_ry),
            coord[2],
            radius,
            None,
            final_pass,
            &config,
            crate::sim::world::UninitContext::with_rules(rules),
            crate::sim::vision::MapRevealProjection::Transient,
            sim.session.current_house(),
        );
    }
    //6CD7A1..6CD7BF samples GetCoords again after RevealArea. A bridge does
    // not add its deck and the sound has no animation/reveal-centre lift.
    let crate::sim::projectile::ProjectileCoord { x, y, z } =
        crate::sim::projectile::cell_ground_coord(
            sim.resolved_terrain.as_ref(),
            target_rx,
            target_ry,
        );
    sim.sound_events.emit(SimSoundEvent::SuperWeaponActivation {
        sw_type,
        coordinate: DriveCoord { x, y, z },
    });
    frame_effects.flush_sound(sim, rules);
    sim.sound_events.emit(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx: target_rx,
        ry: target_ry,
    });
    true
}
