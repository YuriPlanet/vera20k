//! Abilities: what a unit does with, rather than to, a structure.
//!
//! One file per manager; see `manager/mod.rs`. This one owns the jobs that need
//! a click on a building instead of a click on a cell: capture, garrison,
//! infiltration and transport. The engine already owns every mechanism —
//! `sim::mission::capture` walks the engineer and `sim::movement::per_cell` hands
//! the arrival to the building receiver (`0x00519948`) — so what is missing here
//! is only the decision of *when* and *which*.
//!
//! Nothing in this file names a type. A capturer is any unit whose profile
//! carries [`Roles::ENGINEER`] or [`Roles::AGENT`], and a building worth taking is
//! one the house does not already own.

use super::{Ctx, Demands, Manager};
use crate::job::{Proposal, Target};
use crate::profile::Roles;
use crate::registry::JobKind;

/// How far a capturer will walk for a structure, in cells.
///
/// The wish list asks for capture "with the least loss", which on this map means
/// the engineer reaches the building it is sent to: a walk across the whole map
/// is a walk through whatever the enemy has between. Measured earlier in the
/// project, a march covers about thirty cells before it stops, so a capture
/// farther than a march is one the engineer never arrives at.
const CAPTURE_REACH_CELLS: i32 = 20;

/// The priority a capture runs at.
///
/// Above the assault's sixty, and that is the whole point. An engineer carries a
/// sidearm, so the profile calls it `ARMED` and the military manager proposes it
/// for an assault; a measured run had the engineer spend four thousand frames
/// walking to an attack-move cell — `job=Assault target=Cell((160, 92))
/// standing=4110` — before a building was ever offered to it. A capture is worth
/// more than one more rifle in the line, because a yard is a production line, so
/// it outranks the line's work.
const CAPTURE_PRIORITY: i64 = 75;

/// How close an armed enemy may be to the engineer's route before the house
/// calls the walk unsafe, in cells.
///
/// A rifleman's sight is five and its reach four, so three keeps the walk clear
/// of a unit that can see and shoot it without the engineer having to win a fight
/// it cannot. Wider than the sight on purpose: the walk is a straight line the
/// policy chose, not the path the engine will take, so the buffer absorbs the
/// difference between them.
const THREAT_BUFFER_CELLS: i32 = 3;

/// How far a rifleman will walk to hold a civilian building, in cells.
///
/// Shorter than the capture reach on purpose: holding a building is a defensive
/// act and the house gains a gun that cannot be shot back at, not a production
/// line, so it is worth a short walk and not a march.
#[allow(dead_code)] // parked with `garrison`, disabled at the call site in `propose`
const GARRISON_REACH_CELLS: i32 = 20;

/// What a garrison runs at.
///
/// Below the assault's sixty: a rifleman already in the line is doing more than
/// one walking into a block, and the 1v7 lane has shown twice that pulling even
/// one soldier out of the column costs the base.
#[allow(dead_code)] // parked with `garrison`, disabled at the call site in `propose`
const GARRISON_PRIORITY: i64 = 25;

/// How many enemies may be in sight before the house stops garrisoning.
///
/// Holding a civilian block is a quiet-time act: it costs a rifleman from the
/// line and pays back a gun that cannot be shot at. Measured, priority alone is
/// not enough — at priority 30 the job fired once on the 1v7 lane and the lane
/// collapsed from own 84 to 17, because the rifleman it took was a defend post
/// the base needed. With the gate, a lane where thirteen to nineteen enemies are
/// visible never proposes a garrison at all, and a quiet lane still does.
#[allow(dead_code)] // parked with `garrison`, disabled at the call site in `propose`
const GARRISON_QUIET_ENEMIES: usize = 3;

/// Capture, in the order a house should want it.
///
/// The wish list calls taking an enemy construction yard the most important thing
/// in the whole feature set: a yard is a production line the house did not have,
/// and `[GATECH]`-style prerequisites are satisfied by holding the side's yard
/// (`holds_a_captured_yard_of` in `can_build`). A war factory and a refinery are
/// next because both pay immediately; a power plant is worth taking for the
/// headroom, and a defence only when it is close enough to walk to.
///
/// The weights are the ordering, not a score: a yard is always worth more than a
/// refinery at the same distance, and distance only breaks ties inside one rank.
fn capture_rank(roles: Roles) -> i64 {
    if roles.contains(Roles::BUILDS_VEHICLES) {
        80
    } else if roles.contains(Roles::REFINERY) {
        70
    } else if roles.contains(Roles::TRAINS_INFANTRY) {
        60
    } else if roles.contains(Roles::SUPERWEAPON) {
        55
    } else if roles.contains(Roles::LABORATORY) {
        50
    } else if roles.contains(Roles::ARMED) {
        40
    } else {
        30
    }
}

#[derive(Debug, Default)]
pub struct Abilities {
    /// The building each capturer has been sent to.
    ///
    /// Remembered, not re-chosen every term: a target recomputed each term is an
    /// engineer that walks one step, sees the building is still there, and is
    /// pointed at whatever the scan likes next. The same failure the scouts had
    /// (status.md, six attempts) and the reason `keep_captures` exists.
    capturing: std::collections::BTreeMap<u64, u64>,
    /// The building each occupier is walking into, remembered for the same
    /// reason: an infantryman re-aimed every term stops a step outside the door.
    #[allow(dead_code)] // written only by the parked `garrison`
    garrisoning: std::collections::BTreeMap<u64, u64>,
}

impl Manager for Abilities {
    fn propose(&mut self, ctx: &Ctx<'_>, out: &mut Demands) {
        self.capture(ctx, out);
        // Disabled: measured, seven garrison jobs on the 1v7 lane collapse the base
        // (own 84 -> 17) because every infantryman in the column is load-bearing.
        // The code stays for the lane where the army has slack; see plan.md D-batch.
        // self.garrison(ctx, out);
    }
}

impl Abilities {
    /// Send an engineer at a structure worth taking.
    ///
    /// The wish list's centrepiece: taking an enemy construction yard gives the
    /// house that side's production line. A house with no capturer does nothing
    /// here and loses nothing by it.
    fn capture(&mut self, ctx: &Ctx<'_>, out: &mut Demands) {
        // Capturers the house still has. A taken engineer is consumed by the
        // building receiver, so a unit that is gone drops out of this set and its
        // remembered target is forgotten with it.
        let capturers = self.capturers(ctx);
        let targets = self.capture_targets(ctx);
        if capturers.is_empty() || targets.is_empty() {
            self.capturing.clear();
            return;
        }
        // A capture is remembered until the engineer is gone, not until the
        // building leaves sight. `visible_enemy_entities` is L2 and a building the
        // capturer is walking towards can drop out of sight on the way — the
        // engineer then lost its job on the very term it needed it, which is what
        // `job=Capture ... standing=15` was: one term, then gone.
        //
        // The neutral list is unfogged by design (the wish list says every neutral
        // building is visible from the start), so the enemy half is the only one
        // that can blink.
        let alive: std::collections::BTreeSet<u64> =
            capturers.iter().map(|unit| unit.entity_id).collect();
        self.capturing.retain(|unit, _| alive.contains(unit));

        // One capturer per building: two engineers sent to the same yard are the
        // second one standing at a building that is already the house's.
        let taken: std::collections::BTreeSet<u64> = self.capturing.values().copied().collect();
        let free: Vec<_> = capturers
            .iter()
            .filter(|unit| !self.capturing.contains_key(&unit.entity_id))
            .copied()
            .collect();
        let unclaimed: Vec<_> = targets
            .iter()
            .filter(|target| !taken.contains(&target.entity_id))
            .copied()
            .collect();

        // Nearest building first, and that is not a preference: measured, a
        // capture sent across the map never arrived — the `capture:` line never
        // fired in a 24000-tick match and the engineer vanished mid-walk — while
        // a house whose capturer is sent at a building it already stands beside
        // arrives. The wish list asks for capture "with the least loss", and the
        // shortest walk is the one with the least of it.
        let mut unclaimed = unclaimed;
        unclaimed.sort_by_key(|target| {
            let nearest = capturers
                .iter()
                .map(|unit| cell_distance(unit.cell, target.cell))
                .min()
                .unwrap_or(i32::MAX);
            (nearest, target.entity_id)
        });

        for (unit, target) in free.iter().zip(unclaimed.iter()) {
            // The building has to be worth the walk: an engineer sent forty cells
            // through the enemy's ground arrives dead or not at all, and the keep
            // rule is that a unit the house owns is not thrown away for nothing.
            if cell_distance(unit.cell, target.cell) > CAPTURE_REACH_CELLS {
                continue;
            }
            // And the route has to be clear of what would kill it on the way. An
            // engineer has no weapon worth the name and cannot outrun anything:
            // sending it into ground the house can see the enemy standing on is
            // sending it to die, and the wish list's own words are "least loss".
            // Measured before this check: one engineer was bought and sent on a
            // 4000-frame walk that ended with the job dropped and no building
            // taken.
            if !Self::route_is_clear(ctx, unit.cell, target.cell) {
                continue;
            }
            self.capturing.insert(unit.entity_id, target.entity_id);
            out.jobs.push(Proposal {
                kind: JobKind::Capture,
                target: Target::Object(target.entity_id),
                priority: CAPTURE_PRIORITY + capture_rank(self.roles_of(ctx, target.type_id)),
                actor: Some(unit.entity_id),
            });
        }

        // Every capturer already on its way keeps the building it was given, and
        // says so every term. The assignment drops a job no manager proposes —
        // `keep_scouts` in the military manager exists for exactly this reason —
        // so a capture proposed once lives one term and then vanishes. Measured:
        // `job=Capture ... standing=15`, one term and gone, and the engineer
        // reassigned to a defend post while it was still walking.
        for (unit, target) in &self.capturing {
            out.jobs.push(Proposal {
                kind: JobKind::Capture,
                target: Target::Object(*target),
                priority: CAPTURE_PRIORITY,
                actor: Some(*unit),
            });
        }
    }

    /// Whether the straight walk from `from` to `to` passes near anything the
    /// house can see that would kill an engineer.
    ///
    /// The route is the straight line, not the pathfinder's answer, because the
    /// decision has to be cheap: this runs for every capturer every term, and a
    /// full path per candidate is the kind of cost that makes a policy slow
    /// without making it right. The line is what an engineer walks when nothing
    /// blocks it, and it is the closest thing to "what will it meet".
    ///
    /// An enemy counts when it is armed and within [`THREAT_BUFFER_CELLS`] of any
    /// cell on that line. A building does not count: a pillbox is a real danger
    /// but its gun's reach is a weapon fact this manager does not own, and
    /// counting every structure would refuse every route on a map where the
    /// enemy's base is a wall of buildings.
    fn route_is_clear(ctx: &Ctx<'_>, from: (u16, u16), to: (u16, u16)) -> bool {
        let threats: Vec<(u16, u16)> = ctx
            .obs
            .visible_enemy_entities()
            .filter(|enemy| {
                ctx.obs
                    .object_type(ctx.rules, enemy.type_id)
                    .and_then(|ty| ctx.profiles.get(&ty.id))
                    .is_some_and(|profile| {
                        profile.roles.contains(Roles::ARMED)
                            && !profile.roles.contains(Roles::STRUCTURE)
                    })
            })
            .map(|enemy| enemy.cell)
            .collect();
        if threats.is_empty() {
            return true;
        }
        // Sample the line in cells: a step per cell is the resolution the engine
        // moves on, so a threat the line passes within a buffer of is a threat the
        // walk passes.
        let steps = cell_distance(from, to).max(1);
        for step in 0..=steps {
            let t = f64::from(step) / f64::from(steps);
            let x = f64::from(from.0) + (f64::from(to.0) - f64::from(from.0)) * t;
            let y = f64::from(from.1) + (f64::from(to.1) - f64::from(from.1)) * t;
            let cell = (x.round() as u16, y.round() as u16);
            if threats
                .iter()
                .any(|threat| cell_distance(cell, *threat) <= THREAT_BUFFER_CELLS)
            {
                return false;
            }
        }
        true
    }

    /// The units that can take a building: an engineer or an agent.
    fn capturers(&self, ctx: &Ctx<'_>) -> Vec<vera20k_core::sim::control::observation::OwnEntity> {
        let mut capturers = Vec::new();
        for unit in ctx.obs.own_entities() {
            let Some(profile) = ctx
                .obs
                .object_type(ctx.rules, unit.type_id)
                .and_then(|ty| ctx.profiles.get(&ty.id))
            else {
                continue;
            };
            if !(profile.roles.contains(Roles::ENGINEER) || profile.roles.contains(Roles::AGENT)) {
                continue;
            }
            if !unit.orderable || ctx.obs.inside_transport(unit.entity_id).is_some() {
                continue;
            }
            capturers.push(unit);
        }
        capturers.sort_by_key(|unit| unit.entity_id);
        capturers
    }

    /// The buildings worth taking: the enemy's and the map's neutrals.
    ///
    /// Both come from the projection rather than from the rules, because a policy
    /// that aimed at a building the player could not see would be using
    /// information outside L0/L2, which spec §2.5 forbids. `neutral_structures`
    /// is L0 by definition — the wish list says every neutral building is visible
    /// from the start — and `visible_enemy_entities` is L2.
    /// Whether any capturer the house owns could take this building.
    fn capturers_can_take_any(ctx: &Ctx<'_>, building: u64) -> bool {
        ctx.obs
            .own_entities()
            .filter(|unit| {
                ctx.obs
                    .object_type(ctx.rules, unit.type_id)
                    .and_then(|ty| ctx.profiles.get(&ty.id))
                    .is_some_and(|profile| {
                        profile.roles.contains(crate::profile::Roles::ENGINEER)
                            || profile.roles.contains(crate::profile::Roles::AGENT)
                    })
            })
            .any(|unit| {
                ctx.obs
                    .can_take_building(ctx.rules, unit.entity_id, building)
            })
    }

    fn capture_targets(
        &self,
        ctx: &Ctx<'_>,
    ) -> Vec<vera20k_core::sim::control::observation::EnemyEntity> {
        let mut targets: Vec<_> = ctx
            .obs
            .visible_enemy_entities()
            .chain(ctx.obs.neutral_structures())
            // A building an engineer cannot take is not a target, and offering one
            // costs a whole errand.
            //
            // Measured on Arena.mmx seed 0x5EED at 24000 ticks: thirty-six
            // `Capture` proposals for one engineer, one engineer bought, **zero**
            // buildings taken, and the unit line showed `MissionId(8)`/`(2)`/`(5)`
            // alternating around `(89, 37)` — the engineer never left the
            // neighbourhood of its own yard. `Goal::Capture` is admitted by
            // `Observation::can_take_building`, and a target that fails that test
            // never becomes an order, so the manager re-proposed an errand the
            // engine would not accept on every term.
            //
            // The same predicate the order path uses answers it up front.
            .filter(|target| Self::capturers_can_take_any(ctx, target.entity_id))
            .collect();
        targets.sort_by_key(|target| target.entity_id);
        targets.dedup_by_key(|target| target.entity_id);
        targets
    }

    /// The roles a type carries, from the policy's own profile table.
    /// Put a rifleman inside a building it can hold.
    ///
    /// The wish list's neutral-building section: 可驻扎中立建筑应该分析其重要性,
    /// and 可占领中立建筑是重点设施, 派兵保护. A garrisoned structure is the
    /// cheapest defence the game offers — a rifleman inside a civilian block
    /// shoots harder and cannot be shot back at — so a house that ignores it is
    /// leaving free defence on the map.
    ///
    /// Both halves of the question already exist as projection calls:
    /// `can_garrison` is the engine's `BuildingClass::CanBeOccupiedBy` for the
    /// pair, and the building is any neutral structure the wish list says is
    /// visible from the start. What was missing was the decision.
    #[allow(dead_code)] // disabled at the call site in `propose`; measured and kept
    fn garrison(&mut self, ctx: &Ctx<'_>, out: &mut Demands) {
        // Quiet time only. A garrison takes a rifleman out of the line for a gun
        // that cannot be shot back at: a good trade when nothing is attacking, a
        // fatal one when the base is under fire. Measured, the job fired once on
        // the 1v7 lane at priority 30 and the lane went from own 84 to 17.
        if ctx.obs.visible_enemy_entities().count() > GARRISON_QUIET_ENEMIES {
            self.garrisoning.clear();
            return;
        }
        let occupiers = self.occupiers(ctx);
        let buildings = self.neutral_buildings(ctx);
        if occupiers.is_empty() || buildings.is_empty() {
            self.garrisoning.clear();
            return;
        }
        let alive: std::collections::BTreeSet<u64> =
            occupiers.iter().map(|unit| unit.entity_id).collect();
        self.garrisoning.retain(|unit, _| alive.contains(unit));
        let taken: std::collections::BTreeSet<u64> = self.garrisoning.values().copied().collect();

        // Nearest building first, for the same reason capture sorts by distance:
        // a walk across the map is a walk the rifleman does not finish.
        let mut unclaimed: Vec<_> = buildings
            .iter()
            .filter(|building| !taken.contains(&building.entity_id))
            .copied()
            .collect();
        unclaimed.sort_by_key(|building| {
            let nearest = occupiers
                .iter()
                .map(|unit| cell_distance(unit.cell, building.cell))
                .min()
                .unwrap_or(i32::MAX);
            (nearest, building.entity_id)
        });

        let free: Vec<_> = occupiers
            .iter()
            .filter(|unit| !self.garrisoning.contains_key(&unit.entity_id))
            .copied()
            .collect();
        for unit in &free {
            let Some(building) = unclaimed.iter().copied().find(|building| {
                cell_distance(unit.cell, building.cell) <= GARRISON_REACH_CELLS
                    && ctx
                        .obs
                        .can_garrison(ctx.rules, unit.entity_id, building.entity_id)
            }) else {
                continue;
            };
            self.garrisoning.insert(unit.entity_id, building.entity_id);
            out.jobs.push(Proposal {
                kind: JobKind::Garrison,
                target: Target::Object(building.entity_id),
                priority: GARRISON_PRIORITY,
                actor: Some(unit.entity_id),
            });
        }

        // Everything already walking keeps its building and says so every term:
        // a job no manager proposes is dropped by the assignment, which is the
        // lesson every remembered job in this crate carries.
        for (unit, building) in &self.garrisoning {
            out.jobs.push(Proposal {
                kind: JobKind::Garrison,
                target: Target::Object(*building),
                priority: GARRISON_PRIORITY,
                actor: Some(*unit),
            });
        }
    }

    /// The infantry that can hold a building: a unit carrying `OCCUPIER`.
    #[allow(dead_code)] // called only by the parked `garrison`
    fn occupiers(&self, ctx: &Ctx<'_>) -> Vec<vera20k_core::sim::control::observation::OwnEntity> {
        let mut occupiers = Vec::new();
        for unit in ctx.obs.own_entities() {
            let Some(profile) = ctx
                .obs
                .object_type(ctx.rules, unit.type_id)
                .and_then(|ty| ctx.profiles.get(&ty.id))
            else {
                continue;
            };
            if !profile.roles.contains(Roles::OCCUPIER) {
                continue;
            }
            if !unit.orderable || ctx.obs.inside_transport(unit.entity_id).is_some() {
                continue;
            }
            occupiers.push(unit);
        }
        occupiers.sort_by_key(|unit| unit.entity_id);
        occupiers
    }

    /// The structures on the map that nothing owns, and that the house can see.
    ///
    /// `neutral_structures` is unfogged by design — the wish list says every
    /// neutral building is visible from the start — so a civilian block is a
    /// candidate wherever it stands.
    #[allow(dead_code)] // called only by the parked `garrison`
    fn neutral_buildings(
        &self,
        ctx: &Ctx<'_>,
    ) -> Vec<vera20k_core::sim::control::observation::EnemyEntity> {
        let mut buildings: Vec<_> = ctx.obs.neutral_structures().collect();
        buildings.sort_by_key(|building| building.entity_id);
        buildings.dedup_by_key(|building| building.entity_id);
        buildings
    }

    fn roles_of(&self, ctx: &Ctx<'_>, type_id: vera20k_core::sim::intern::InternedId) -> Roles {
        ctx.obs
            .object_type(ctx.rules, type_id)
            .and_then(|ty| ctx.profiles.get(&ty.id))
            .map_or(Roles::default(), |profile| profile.roles)
    }
}

/// Chebyshev distance in cells: how far a walk is on this grid.
fn cell_distance(a: (u16, u16), b: (u16, u16)) -> i32 {
    i32::from(a.0.abs_diff(b.0)).max(i32::from(a.1.abs_diff(b.1)))
}
