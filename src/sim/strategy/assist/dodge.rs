//! Stepping out of the way of a shot that is already in the air.
//!
//! This is the one micro decision that has to be made inside a bullet's flight
//! time, so it runs on its own clock instead of the fifteen-frame micro cycle:
//! a shell crosses a cell in a handful of frames and a unit needs ten to twenty
//! to cross one itself. Everything below is arithmetic on the frame the
//! question is asked, and the only memory is the intent that stops the same
//! step being re-issued every frame ([`TrackedKind::Dodge`]).
//!
//! What can be dodged is what the bullet type says, not what the AI wishes:
//!
//! - `Inviso=` bullets are placed on the target and land the same frame
//!   (`BulletClass::Fire 0x004688B7`): `frames == 0`, no decision.
//! - `ROT >= 1` bullets re-read the live position every visit
//!   (`Projectile::tracks_target`): leaving the impact cell changes nothing, so
//!   they are ignored rather than strafed against.
//! - An ordinary `ROT < 1` bullet keeps the point it was fired at
//!   (`initial_target_position`): that frozen aim is the mechanic a step aside
//!   exploits, and it is why a target that has walked away is not there when
//!   the shell lands.
//!
//! The feasibility test is the physics the engine already has: the unit's own
//! `Speed=` converted exactly as `TechnoType ReadINI` does (`speed * 256 / 100`
//! leptons a frame), the bullet's current lepton speed, and the warhead's
//! `CellSpread=` as the radius the impact still hurts in. A step is ordered only
//! when the unit can leave that radius before the shot lands: a dodge that
//! arrives late is a wasted order and a unit out of position.

use std::collections::BTreeMap;

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::Observation;

use super::AssistBrain;
use super::tracked::TrackedKind;

/// Frames a reaction costs: the shot is seen on this frame and the walk starts
/// on the next, with one frame of slack for the order's round trip.
const REACTION_FRAMES: u32 = 2;

/// How far a step out of a blast may be: a `CellSpread=3` shell is a building's
/// problem, not a unit's, and a walk longer than this is the retreat's.
const MAX_STEP_CELLS: i32 = 4;

/// One shot as the dodge decision needs it: where it lands, when, and how far
/// its damage reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Shell {
    pub(super) impact: (i32, i32),
    pub(super) frames: u32,
    /// `CellSpread=` in whole cells: `0.4` hurts the cell it lands in, `2.0`
    /// hurts everything within two cells.
    pub(super) spread: i32,
}

/// How deep inside a blast `cell` is: `spread + 1` at the impact cell, zero
/// outside the radius.
///
/// Chebyshev on purpose: retail's spread test is a box, not a circle, so a
/// diagonal step has to clear it the same as a straight one.
pub(super) fn exposure(cell: (i32, i32), threat: &Shell) -> i32 {
    let dx = (cell.0 - threat.impact.0).abs();
    let dy = (cell.1 - threat.impact.1).abs();
    (threat.spread + 1 - dx.max(dy)).max(0)
}

/// The total exposure of `cell` to every shot in `threats`.
pub(super) fn exposure_to_all(cell: (i32, i32), threats: &[Shell]) -> i32 {
    threats.iter().map(|threat| exposure(cell, threat)).sum()
}

/// The nearest cell that leaves the most blast behind, or `None` when the unit
/// is already clear or every reachable cell is just as exposed.
///
/// The search radius is the widest blast's own: a `CellSpread=2` shell needs
/// three cells, and a step that only reaches one is not a dodge. Candidates are
/// ranked by `(exposure, distance)` and walked in a fixed order, so a farther
/// cell that is fully clear beats a nearer one that is half inside the blast,
/// and equal cells are decided by position rather than by hash order.
///
/// `extent` is the map's: a cell past the edge is not a place to stand, so it
/// is not a candidate. The shell does not move, and a unit on the last row
/// otherwise takes its step off the map — the order is refused at the door and
/// the unit stands in the blast it was told to leave.
pub(super) fn escape_cell(
    cell: (u16, u16),
    threats: &[Shell],
    extent: (u16, u16),
) -> Option<(u16, u16)> {
    let here = (i32::from(cell.0), i32::from(cell.1));
    let current = exposure_to_all(here, threats);
    if current == 0 {
        return None;
    }
    let widest = threats
        .iter()
        .map(|threat| threat.spread + 1)
        .max()
        .unwrap_or(1);
    let radius = widest.clamp(1, MAX_STEP_CELLS);
    let mut best: Option<((u16, u16), i32, i32)> = None;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx == 0 && dy == 0 {
                continue;
            }
            let (x, y) = (here.0 + dx, here.1 + dy);
            if x < 0 || y < 0 {
                continue;
            }
            let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
                continue;
            };
            if !super::orders::on_map((x, y), extent) {
                continue;
            }
            let exposure = exposure_to_all((i32::from(x), i32::from(y)), threats);
            let distance = dx.abs().max(dy.abs());
            if best.is_none_or(|(_, best_exposure, best_distance)| {
                (exposure, distance) < (best_exposure, best_distance)
            }) {
                best = Some(((x, y), exposure, distance));
            }
        }
    }
    best.filter(|(_, exposure, _)| *exposure < current)
        .map(|(cell, _, _)| cell)
}

/// Whether a unit moving `leptons_per_frame` can clear `cells` cells before
/// `frames` are up.
pub(super) fn can_clear(
    cells: u32,
    leptons_per_frame: i32,
    frames: u32,
    reaction_frames: u32,
) -> bool {
    if leptons_per_frame <= 0 || cells == 0 {
        return false;
    }
    let per_cell = crate::util::lepton::LEPTONS_PER_CELL_I32.max(1);
    let cells = i32::try_from(cells).unwrap_or(i32::MAX);
    let frames_needed = (cells * per_cell + leptons_per_frame - 1) / leptons_per_frame;
    let frames_needed = u32::try_from(frames_needed).unwrap_or(u32::MAX);
    frames_needed.saturating_add(reaction_frames) <= frames
}

/// Order every unit that a shell is about to land on to take one step.
///
/// One walk over the projectiles builds the per-target index; the units are then
/// read from the frame's ledger, so two hundred bullets in the air cost one pass
/// and not one pass per unit.
pub(super) fn run(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    let mut by_target: BTreeMap<u64, Vec<Shell>> = BTreeMap::new();
    // Guided shots, kept apart: a step does not make one miss, so a walker
    // takes the hit, but an airframe that leaves the launcher's range or the
    // missile's turn circle does outfly it. Air gets them; the ground does not.
    let mut homing: BTreeMap<u64, Vec<Shell>> = BTreeMap::new();
    for (target, shot) in obs.shots_in_flight() {
        let spread = obs.warhead_spread_cells(rules, shot.warhead);
        let threat = Shell {
            impact: (i32::from(shot.impact.0), i32::from(shot.impact.1)),
            frames: shot.frames,
            spread,
        };
        if shot.homing {
            homing.entry(target).or_default().push(threat);
        } else {
            by_target.entry(target).or_default().push(threat);
        }
    }
    if by_target.is_empty() && homing.is_empty() {
        return;
    }

    for unit in &brain.census.units {
        if !unit.traits.combat || !unit.controllable() {
            continue;
        }
        // A unit walking into a building or a depot keeps its destination: a
        // dodge `Move` over it drops the visit, and the mission does not
        // re-aim.
        if unit.in_service() {
            continue;
        }
        // An earlier pass already gave this unit the frame's order — a
        // garrison's Capture, a specialist's walk. A second one is a second
        // command, and the later of the two is the one the world takes.
        if super::orders::ordered_this_frame(out, unit.entity_id) {
            continue;
        }
        // A dug-in soldier cannot walk, and a position it was told to hold is
        // worth more than one shell.
        if obs.is_deployed(unit.entity_id) {
            continue;
        }
        let Some(ty) = obs.object_type(rules, unit.type_id) else {
            continue;
        };
        // An airframe dodges in the third dimension: it is not held to the
        // ground, a step costs it nothing, and a guided missile that has to
        // turn is a missile it can outfly. Everything else takes the guided
        // shot it cannot step out of.
        let airborne = ty.category == ObjectCategory::Aircraft
            || ty.movement_zone == crate::rules::locomotor_type::MovementZone::Fly;
        let threats: Vec<Shell> = match (by_target.get(&unit.entity_id), airborne) {
            (Some(list), _) => list.clone(),
            (None, true) => homing.get(&unit.entity_id).cloned().unwrap_or_default(),
            (None, false) => continue,
        };
        if threats.is_empty() {
            continue;
        }
        // A burst weapon mid-cycle takes the shell. The step is a Move and a
        // Move empties the burst, which is the rate-of-fire bug: the gun
        // comes back in three frames instead of a reload. Dodging is the pass
        // that would otherwise feed the loop every time anything shoots at
        // this unit, so for a burst weapon the cycle is finished first and
        // the shell is taken — the same trade a player makes who wants the
        // burst and not the exploit.
        if super::micro::firing(obs, unit.mission, unit.entity_id)
            && super::advance::weapon_burst(rules, ty, false) > 1
        {
            continue;
        }
        let Some(target) = escape_cell(unit.cell, &threats, obs.map_extent()) else {
            continue;
        };
        let here = (i32::from(unit.cell.0), i32::from(unit.cell.1));
        let Some(deadline) = threats
            .iter()
            .filter(|threat| exposure(here, threat) > 0)
            .map(|threat| threat.frames)
            .min()
        else {
            continue;
        };
        let cells = (i32::from(target.0) - here.0)
            .unsigned_abs()
            .max((i32::from(target.1) - here.1).unsigned_abs());
        let speed = crate::util::fixed_math::ra2_speed_to_leptons_per_frame(ty.speed);
        if !can_clear(cells, speed, deadline, REACTION_FRAMES) {
            continue;
        }
        if !brain
            .tracked
            .admits(tick, unit.entity_id, TrackedKind::Dodge, None, Some(target))
        {
            continue;
        }
        brain
            .tracked
            .record(tick, unit.entity_id, TrackedKind::Dodge, None, Some(target));
        super::orders::issue(
            brain,
            out,
            rules,
            obs,
            unit.entity_id,
            Goal::Move { cell: target },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(impact: (i32, i32), frames: u32, spread: i32) -> Shell {
        Shell {
            impact,
            frames,
            spread,
        }
    }

    /// A `CellSpread=0.4` cannon shell hurts the cell it lands in and nothing
    /// beside it: one cell away is already safe, which is what makes a step
    /// aside worth an order at all.
    #[test]
    fn a_small_blast_is_clear_one_cell_away() {
        let threat = shot((10, 10), 6, 0);
        assert_eq!(exposure((10, 10), &threat), 1);
        assert_eq!(exposure((11, 10), &threat), 0);
        assert_eq!(exposure((11, 11), &threat), 0);
    }

    /// `CellSpread=2` reaches two cells: the step has to leave both.
    #[test]
    fn a_wide_blast_needs_the_step_outside_it() {
        let threat = shot((10, 10), 6, 2);
        assert_eq!(exposure((12, 12), &threat), 1);
        assert_eq!(exposure((13, 10), &threat), 0);
        let escape = escape_cell((10, 10), &[threat], (60, 60)).expect("a step out of the blast");
        let escape = (i32::from(escape.0), i32::from(escape.1));
        assert_eq!(exposure_to_all(escape, &[threat]), 0);
        assert!(escape.0.abs_diff(10).max(escape.1.abs_diff(10)) >= 3);
    }

    /// A unit already in the clear is not ordered anywhere: a dodge pass that
    /// shuffles units standing outside the blast spends the frame on nothing.
    #[test]
    fn a_clear_cell_asks_for_no_step() {
        assert_eq!(escape_cell((1, 1), &[shot((10, 10), 6, 0)], (60, 60)), None);
    }

    /// Two shells, two impact points: the step goes where neither lands.
    #[test]
    fn the_step_leaves_every_impact() {
        let threats = [shot((10, 10), 6, 0), shot((11, 10), 6, 0)];
        let escape = escape_cell((10, 10), &threats, (60, 60)).expect("a step");
        let escape = (i32::from(escape.0), i32::from(escape.1));
        assert_eq!(exposure_to_all(escape, &threats), 0);
    }

    /// A shell landing beside a unit on the map's last cell leaves it one way
    /// out that is on the map: the steps past the edge are not candidates, so
    /// the pass takes the one that stays. Without them the clearest cell is
    /// past the edge, the door refuses the order, and the unit stands in the
    /// blast it was told to leave.
    #[test]
    fn a_step_at_the_edge_of_the_map_stays_on_it() {
        let threats = [shot((62, 62), 6, 3)];
        let escape = escape_cell((63, 63), &threats, (64, 64)).expect("a step out of the blast");
        assert!(
            escape.0 < 64 && escape.1 < 64,
            "{escape:?} is off the map, and every cell past the edge is"
        );
        let escape = (i32::from(escape.0), i32::from(escape.1));
        assert!(
            exposure_to_all(escape, &threats) < exposure_to_all((63, 63), &threats),
            "the step it takes still leaves the blast"
        );
    }

    /// The feasibility test is why this pass does not fire every frame: a shell
    /// landing in two frames is not dodgeable by anything, a Grizzly
    /// (`Speed=6` → 15 leptons a frame, 18 frames a cell) cannot clear a cell in
    /// ten frames, and it can in forty.
    #[test]
    fn only_a_step_that_arrives_in_time_is_ordered() {
        assert!(!can_clear(1, 15, 2, REACTION_FRAMES), "no time to move");
        assert!(
            !can_clear(1, 15, 10, REACTION_FRAMES),
            "not one cell's time"
        );
        assert!(can_clear(1, 15, 40, REACTION_FRAMES), "18 frames a cell");
        assert!(can_clear(2, 15, 40, REACTION_FRAMES), "two cells in forty");
        assert!(
            !can_clear(2, 15, 30, REACTION_FRAMES),
            "two cells is twice the walk"
        );
        assert!(!can_clear(1, 0, 400, 0), "an immobile object never clears");
    }

    /// The projection a dodge decision rests on: a shell in the air aimed at a
    /// unit is visible as where it will land, how long it has, and whether it
    /// follows. A bullet that follows reads as homing and is never dodged.
    #[test]
    fn a_shell_in_the_air_reads_as_a_landing_point_and_a_deadline() {
        use crate::sim::control::observation::Observation;
        use crate::sim::projectile::{
            ProjectileCollisionPolicy, ProjectileCoord, ProjectilePayload, ProjectileSpawn,
            ProjectileTarget, ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState,
            TargetExpiryPolicy,
        };

        let rules = match crate::rules::retail_ini_fixture::retail_unlayered_rules() {
            Some(rules) => rules,
            None => return,
        };
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        let tank = sim
            .spawn_object("MTNK", "Americans", 10, 10, 255, &rules)
            .expect("a Grizzly");

        let origin = ProjectileCoord::new(10 * 256 + 128, 20 * 256 + 128, 0);
        let aim = ProjectileCoord::new(10 * 256 + 128, 10 * 256 + 128, 0);
        let shot = |tracks_target: bool| ProjectileSpawn {
            native_unique_id: 0,
            line_trail: None,
            flat: false,
            source_id: 0,
            origin,
            target: ProjectileTarget::Entity(tank),
            initial_target_position: aim,
            payload: ProjectilePayload::new(
                40,
                crate::sim::intern::InternedId::from_index(0),
                None,
                false,
            ),
            speed_leptons_per_frame: 64,
            velocity: ProjectileVelocity::new(0, 0, 0),
            trajectory: ProjectileTrajectory::Straight,
            guidance: None,
            visual: ProjectileVisualState::new(0, 0, 0),
            arm_frames: 0,
            fuse_frames: None,
            ranged_fuse: false,
            tracks_target,
            target_expiry: TargetExpiryPolicy::Expire,
            collision: ProjectileCollisionPolicy::NONE,
        };
        let straight = sim.projectiles_mut_for_test().spawn(1, shot(false));
        let guided = sim.projectiles_mut_for_test().spawn(2, shot(true));
        assert_ne!(straight, guided, "two bullets, two ids");

        let obs = Observation::new(&sim, owner);
        let shots: Vec<_> = obs.incoming_shots(tank).collect();
        assert_eq!(shots.len(), 2, "both name the tank");
        let straight = shots
            .iter()
            .find(|shot| shot.projectile_id == straight)
            .expect("the straight one");
        assert_eq!(straight.impact, (10, 10), "it lands on the target's cell");
        assert!(!straight.homing, "an unguided shell keeps its aim");
        // Ten cells at 64 leptons a frame is 2560 / 64 = 40 frames.
        assert_eq!(straight.frames, 40, "ten cells of flight");
        assert!(
            shots
                .iter()
                .find(|shot| shot.projectile_id == guided)
                .is_some_and(|shot| shot.homing),
            "a guided one follows the tank"
        );
    }

    /// A shell forty frames out is a step's worth of warning and the pass takes
    /// it; one two frames out is a hit, and ordering a walk that arrives late
    /// would only take the unit out of position for nothing.
    #[test]
    fn the_pass_steps_out_only_when_the_walk_arrives_in_time() {
        use crate::rules::unit_role::RoleTable;
        use crate::sim::control::observation::Observation;
        use crate::sim::projectile::{
            ProjectileCollisionPolicy, ProjectileCoord, ProjectilePayload, ProjectileSpawn,
            ProjectileTarget, ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState,
            TargetExpiryPolicy,
        };

        let rules = match crate::rules::retail_ini_fixture::retail_unlayered_rules() {
            Some(rules) => rules,
            None => return,
        };
        for (origin_row, expected) in [(20_u16, true), (10, false)] {
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let owner = sim.interner.intern("Americans");
            let tank = sim
                .spawn_object("MTNK", "Americans", 10, 10, 255, &rules)
                .expect("a Grizzly");
            let spawn = ProjectileSpawn {
                native_unique_id: 0,
                line_trail: None,
                flat: false,
                source_id: 0,
                origin: ProjectileCoord::new(10 * 256 + 128, i32::from(origin_row) * 256 + 128, 0),
                target: ProjectileTarget::Entity(tank),
                initial_target_position: ProjectileCoord::new(10 * 256 + 128, 10 * 256 + 128, 0),
                payload: ProjectilePayload::new(
                    40,
                    crate::sim::intern::InternedId::from_index(0),
                    None,
                    false,
                ),
                speed_leptons_per_frame: 64,
                velocity: ProjectileVelocity::new(0, 0, 0),
                trajectory: ProjectileTrajectory::Straight,
                guidance: None,
                visual: ProjectileVisualState::new(0, 0, 0),
                arm_frames: 0,
                fuse_frames: None,
                ranged_fuse: false,
                tracks_target: false,
                target_expiry: TargetExpiryPolicy::Expire,
                collision: ProjectileCollisionPolicy::NONE,
            };
            sim.projectiles_mut_for_test().spawn(1, spawn);
            let obs = Observation::new(&sim, owner);
            let mut brain = AssistBrain::new(0xA17);
            let mut census = super::super::census::Census::default();
            census.refresh(&RoleTable::build(&rules), &rules, &obs);
            brain.census = census;
            let mut out = Outbox::default();
            run(&mut brain, &rules, &obs, &mut out);
            assert_eq!(
                out.orders.iter().any(|order| order.actor == tank),
                expected,
                "a shot {} cells out is {}",
                i32::from(origin_row) - 10,
                if expected { "a dodge" } else { "a hit" }
            );
        }
    }

    /// The conversion is the engine's own, not a second guess: `Speed=6` is
    /// `6 * 256 / 100 = 15` leptons a frame, so a 256-lepton cell is eighteen
    /// frames of walking and the reaction has to fit beside them.
    #[test]
    fn a_grizzly_needs_eighteen_frames_for_a_cell() {
        let speed = crate::util::fixed_math::ra2_speed_to_leptons_per_frame(6);
        assert_eq!(speed, 15);
        assert!(can_clear(1, speed, 18, 0), "a cell exactly");
        assert!(
            !can_clear(1, speed, 18, REACTION_FRAMES),
            "plus the reaction"
        );
        assert!(can_clear(1, speed, 20, REACTION_FRAMES));
    }
}
