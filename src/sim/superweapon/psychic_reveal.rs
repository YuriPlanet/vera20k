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
//! RESIDUAL: a negative `PsychicRevealRadius=` indexes before RevealArea's
//! count table natively; VERA reveals nothing. No retail value is negative.
//!
//! Ledger: no Scenario draws, timer writes or detach calls.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/vision, sim/world.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

#[cfg(all(test, tests_sim_superweapon))]
#[path = "psychic_reveal_tests.rs"]
mod tests;

use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::vision;
use crate::sim::world::{SimSoundEvent, Simulation};

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
        sim.house_alliances.as_map(),
        owner,
        &config,
        &sim.interner,
        &sim.fog_viewers(),
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
            crate::sim::world::UninitContext::new(rules),
            crate::sim::vision::MapRevealProjection::Transient,
            sim.session.current_house(),
        );
    }
    // `0x006CD717..0x006CD737`, `0x006CD7A1..0x006CD7BF`: the cue plays at
    // the cell's GetCoords, the reveal's own centre.
    sim.sound_events.emit(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        world: ground.into(),
    });
    true
}

/// Ares SW_PsychicReveal::Activate (10075150), hooked at 6CC390: on a
/// charged Super, project the target (including a bridge deck) to the ground
/// plane and map the eligible viewers' cells twice. The common Ares launch
/// owns presentation, charge and cleanup. No RNG or lasting sight source.
pub(super) fn ares_launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    sw: &crate::rules::superweapon_type::SuperWeaponType,
    target: (u16, u16),
) {
    if !super::is_charged(sim, owner, sw_type) {
        return;
    }
    let Some(metadata) = rules.super_weapon_metadata(&sw.id) else {
        return;
    };
    let (width, height) = metadata
        .launch()
        .range_or_radius(rules.general.psychic_reveal_radius.min(10));
    let center = vision::conceal_area_centre(super::deck_coords(sim, target));
    // 100751A1 also admits a radar spy through RadarVisibleTo (+0x54E4).
    // Its House state is the shared residual documented in sim::spy_infiltration.
    let local = sim.session.current_house();
    let viewers: Vec<_> = vision::direct_reveal_viewers(
        sim.house_alliances.as_map(),
        owner,
        &sim.sight_reveal_config(rules),
        &sim.interner,
        &sim.fog_viewers(),
    )
    .into_iter()
    .map(|viewer| (viewer, sim.reveal_notifies_viewer(viewer, local)))
    .collect();
    if viewers.is_empty() {
        return;
    }

    // RevealHelper 100692D5..10069315 and 10069480: competitive LAN/WOL
    // modes leave three map-relative cells hidden; cooperative vt+4 is true.
    let size = sim.map_size_diamond();
    let excluded = size
        .filter(|_| {
            sim.session.execution_topology().is_lockstep()
                && sim.session.rule_policy().ares_psychic_reveal_exclusions()
        })
        .map(|(w, h)| [(7, w + 5), (13, w + 11), (h + 13, w + h - 15)]);
    let mut context = crate::sim::world::UninitContext::new(rules);
    for final_pass in [false, true] {
        let reveal = |sim: &mut Simulation,
                      cell: (u16, u16),
                      context: &mut crate::sim::world::UninitContext<'_>| {
            let (x, y) = (i32::from(cell.0), i32::from(cell.1));
            if size.is_some_and(|(w, h)| {
                !crate::map::playfield::size_diamond_contains(w, h, (cell.0 as i16, cell.1 as i16))
            }) || excluded.is_some_and(|cells| cells.contains(&(x, y)))
            {
                return;
            }
            for &(viewer, notify) in &viewers {
                sim.apply_reveal_map_cell(
                    viewer,
                    cell,
                    final_pass,
                    vision::MapRevealProjection::Transient,
                    notify,
                    context.reborrow(),
                );
            }
        };
        if width < 0.0 {
            // 10075024..100750CB uses the shared Reset/Next allocated-cell
            // cursor, so discoveries may affect its next step.
            sim.for_each_allocated_map_cell(&mut context, reveal);
        } else {
            let (cx, cy) = (i32::from(center.0), i32::from(center.1));
            // 10074E40: a positive truncated width and height form a
            // rectangle. Otherwise 10074C90 visits a radius in row order,
            // and 10074DE4 excludes the exact circumference. Bound all
            // visits to stored cells, even for an infinite authored radius.
            let (left, top, right, bottom) = if height > 0 {
                if !(1.0..2147483648.0).contains(&width) {
                    return;
                }
                let w = width as i32;
                let left = cx - w / 2;
                let top = cy - height / 2;
                (left, top, left + w, top + height)
            } else {
                let radius = width.ceil() as i32;
                (
                    cx.saturating_sub(radius),
                    cy.saturating_sub(radius),
                    cx.saturating_add(radius).saturating_add(1),
                    cy.saturating_add(radius).saturating_add(1),
                )
            };
            let radius_sq = f64::from(width) * f64::from(width);
            for y in top.max(0)..bottom.min(i32::from(sim.fog.height())) {
                for x in left.max(0)..right.min(i32::from(sim.fog.width())) {
                    let Some(terrain) = sim.resolved_terrain.as_ref() else {
                        return;
                    };
                    let Some(index) = terrain.native_fixed_cell_index(x as i16, y as i16) else {
                        continue;
                    };
                    let (x, y) = terrain
                        .native_cell_coord(crate::map::cell_index::NativeCellIdentity::Real(index));
                    let dx = f64::from(i32::from(x) - cx);
                    let dy = f64::from(i32::from(y) - cy);
                    if height > 0 || dx * dx + dy * dy < radius_sq {
                        reveal(sim, (x as u16, y as u16), &mut context);
                    }
                }
            }
        }
    }
}
