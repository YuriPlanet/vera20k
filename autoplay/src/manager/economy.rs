//! Economy: income, harvesters, refineries, the build order.
//!
//! Three decisions, all read off the profile rather than a type name: put the
//! maker down where it can stand, keep every harvester working, and raise the
//! next building the house needs. The need is a role — a refinery, a barracks,
//! a war factory, more power — and the building that satisfies it is the
//! cheapest buildable profile carrying that role.

use vera20k_core::rules::object_type::ObjectCategory;
use vera20k_core::sim::control::intent::Intent;
use vera20k_core::sim::control::placement::Placement;
use vera20k_core::sim::production::CanBuild;

use crate::job::{Proposal, Target};
use crate::manager::{Ctx, Demands, Manager};
use crate::profile::{Roles, UnitProfile};
use crate::registry::JobKind;

/// Power headroom under which the house buys another plant, in the same units
/// as power_balance.
const POWER_MARGIN: i32 = 20;

/// How many harvesters a refinery is given.
///
/// One truck on a refinery is the ceiling on the house's income, and the
/// ceiling is what the wallet hits: measured, five of the six baseline cells
/// sat at twelve credits with the wallet frozen for the last 3600 frames,
/// which is also why no second refinery ever landed.
const HARVESTERS_PER_REFINERY: usize = 3;

/// What the wallet keeps in hand before a second truck on a refinery is
/// bought.
const EXTRA_HARVESTER_MARGIN: i32 = 1_500;

#[derive(Debug, Default)]
pub struct Economy;

impl Manager for Economy {
    fn propose(&mut self, ctx: &Ctx<'_>, out: &mut Demands) {
        self.maker(ctx, out);
        self.harvesters(ctx, out);
        self.place(ctx, out);
        self.harvesters_for_refineries(ctx, out);
        Self::engineer(ctx, out);
        self.build(ctx, out);
    }
}

impl Economy {
    /// A maker that stands on ground that takes it is deployed before anything
    /// else is decided: a house with a yard has an economy, and a house with a
    /// maker on the ground has neither.
    fn maker(&self, ctx: &Ctx<'_>, out: &mut Demands) {
        for unit in ctx.obs.own_entities() {
            let Some(ty) = ctx.obs.object_type(ctx.rules, unit.type_id) else {
                continue;
            };
            let Some(profile) = ctx.profiles.get(&ty.id) else {
                continue;
            };
            if !profile.roles.contains(Roles::DEPLOYER) || !unit.orderable {
                continue;
            }
            if ctx.obs.is_deployed(unit.entity_id) || ty.deploys_into.is_none() {
                continue;
            }
            out.jobs.push(Proposal {
                kind: JobKind::Deploy,
                target: Target::None,
                priority: 95,
                actor: Some(unit.entity_id),
            });
        }
    }

    /// Every harvester that is not already harvesting gets one. The harvest
    /// mission finds its own ore, so the job names the unit's own cell.
    fn harvesters(&self, ctx: &Ctx<'_>, out: &mut Demands) {
        for unit in ctx.obs.own_entities() {
            let Some(ty) = ctx.obs.object_type(ctx.rules, unit.type_id) else {
                continue;
            };
            let Some(profile) = ctx.profiles.get(&ty.id) else {
                continue;
            };
            if !profile.roles.contains(Roles::HARVESTER) || !unit.orderable {
                continue;
            }
            if ctx.obs.inside_transport(unit.entity_id).is_some() {
                continue;
            }
            out.jobs.push(Proposal {
                kind: JobKind::Harvest,
                target: Target::Cell(unit.cell),
                priority: 10,
                actor: Some(unit.entity_id),
            });
        }
    }

    /// A finished building takes the first cell the native site search admits.
    fn place(&self, ctx: &Ctx<'_>, out: &mut Demands) {
        for ready in ctx.obs.ready_buildings() {
            let placement = Placement::new(ctx.obs, ctx.rules);
            // The search answers a different cell per attempt; one attempt that
            // finds nothing is a base that stays blocked, because a ready
            // building holds its factory until it is placed.
            let mut cell = None;
            for attempt in 0..8 {
                if let Some(found) = placement.cell_for(ready.type_id, attempt) {
                    cell = Some(found);
                    break;
                }
            }
            let Some(cell) = cell else {
                if ctx.tick % 600 == 0 {
                    log::info!("economy: no site for a ready building at {}", ctx.tick);
                }
                continue;
            };
            out.intents.push(Intent::PlaceBuilding {
                type_id: ready.type_id,
                cell,
            });
        }
    }

    /// The next building, in the order a house opens: a refinery, a barracks, a
    /// war factory, then power whenever the base is short of it.
    fn build(&self, ctx: &Ctx<'_>, out: &mut Demands) {
        if let Some((type_id, ready)) = ctx.obs.building_in_production() {
            if ctx.tick % 1200 == 0 {
                log::info!(
                    "economy: building in production at {}: {:?} ready={ready} credits={}",
                    ctx.tick,
                    ctx.obs
                        .object_type(ctx.rules, type_id)
                        .map_or("?", |ty| ty.id.as_str()),
                    ctx.obs.credits()
                );
            }
            return;
        }
        let mut refinery: Option<&UnitProfile> = None;
        let mut infantry_factory: Option<&UnitProfile> = None;
        let mut vehicle_factory: Option<&UnitProfile> = None;
        let mut power: Option<&UnitProfile> = None;
        let mut defense: Option<&UnitProfile> = None;
        // The technology buildings. Without them the house stops at a war
        // factory and four pillboxes: no laboratory, so no force shield and no
        // next tech tier; no purifier, so every truck earns less. Measured on
        // 1v7 easy, the enemy built `NATECH` on four of seven houses and fired
        // iron curtains (`IronCurtain launched at (72, 139) by Computer5`) while
        // this house owned nothing that answers.
        let mut laboratory: Option<&UnitProfile> = None;
        let mut purifier: Option<&UnitProfile> = None;
        let mut superweapon: Option<&UnitProfile> = None;
        // The radar sits between the war factory and the laboratory: `[GATECH]`
        // lists `Prerequisite=GAWEAP,RADAR,GACNST`, and a house without one has
        // every technology building refused on every term. Measured on 1v7 easy
        // before this existed: `GATECH refused by can_build` at every readout and
        // `lab=0` at the end of a 24000-tick match.
        let mut radar: Option<&UnitProfile> = None;
        for ty in ctx.rules.all_objects() {
            let Some(profile) = ctx.profiles.get(&ty.id) else {
                continue;
            };
            if profile.category != ObjectCategory::Building {
                continue;
            }
            // The build button's own answer: tech level, prerequisites, owner
            // limits. A policy that queues a type the house would refuse has
            // built nothing and blocked its own factory.
            if ctx.obs.can_build(ctx.rules, ty, false) != CanBuild::Yes {
                // Why a technology building is refused is not visible from the
                // outside: `CanBuild` answers `No` without a reason, and a house
                // that stops at four defences looks identical whether the
                // laboratory is unaffordable or impossible. One line per term is
                // the cheapest way to tell those apart.
                if ctx.tick % 1200 == 0
                    && (profile.roles.contains(Roles::LABORATORY)
                        || profile.roles.contains(Roles::ORE_PURIFIER))
                {
                    log::info!(
                        "economy: {} refused by can_build at {} cost={} power={} house_tech={}",
                        ty.id.as_str(),
                        ctx.tick,
                        profile.cost,
                        profile.power,
                        ctx.obs
                            .tech_level()
                            .map_or("?".to_string(), |level| level.to_string()),
                    );
                }
                continue;
            }
            if profile.roles.contains(Roles::LABORATORY) {
                laboratory = Self::cheaper(laboratory, profile);
            } else if profile.roles.contains(Roles::RADAR) {
                radar = Self::cheaper(radar, profile);
            } else if profile.roles.contains(Roles::ORE_PURIFIER) {
                purifier = Self::cheaper(purifier, profile);
            } else if profile.roles.contains(Roles::SUPERWEAPON) {
                // The launchers themselves. A laboratory declares the force
                // shield and is matched above, so "unlocks the tier" and "fires
                // something" stay two different buildings.
                superweapon = Self::cheaper(superweapon, profile);
            } else if profile.roles.contains(Roles::REFINERY) {
                refinery = Self::cheaper(refinery, profile);
            } else if profile.roles.contains(Roles::TRAINS_INFANTRY) {
                infantry_factory = Self::cheaper(infantry_factory, profile);
            } else if profile.roles.contains(Roles::BUILDS_VEHICLES) {
                vehicle_factory = Self::cheaper(vehicle_factory, profile);
            } else if profile.roles.contains(Roles::ARMED) && profile.cost <= 1200 {
                // A structure with a gun: the cheapest one the house can raise.
                defense = Self::cheaper(defense, profile);
            } else if profile.power > 0 {
                power = Self::cheaper(power, profile);
            }
        }

        let count = |role: Roles| {
            ctx.obs
                .own_entities()
                .filter(|unit| {
                    ctx.obs
                        .object_type(ctx.rules, unit.type_id)
                        .and_then(|ty| ctx.profiles.get(&ty.id))
                        .is_some_and(|profile| profile.roles.contains(role))
                })
                .count()
        };
        let has = |role: Roles| count(role) > 0;
        // A defences count is structures with a gun. ARMED alone counts every
        // soldier, so the base never bought one.
        let defenses = ctx
            .obs
            .own_entities()
            .filter(|unit| {
                ctx.obs
                    .object_type(ctx.rules, unit.type_id)
                    .and_then(|ty| ctx.profiles.get(&ty.id))
                    .is_some_and(|profile| {
                        profile.roles.contains(Roles::ARMED)
                            && profile.roles.contains(Roles::STRUCTURE)
                    })
            })
            .count();

        // Each need falls back to a plant: power is what the next building
        // usually waits on, and a plant is always cheaper than a stalled base.
        let want = if !has(Roles::REFINERY) {
            refinery.or(power)
        } else if !has(Roles::TRAINS_INFANTRY) {
            infantry_factory.or(power)
        } else if !has(Roles::BUILDS_VEHICLES) {
            vehicle_factory.or(power)
        } else if count(Roles::REFINERY) < 3
            && refinery.is_some_and(|want| Self::affordable(ctx, want))
        {
            refinery
        } else if defenses < 8 && defense.is_some_and(|want| Self::affordable(ctx, want)) {
            defense
        // The technology tier, after the base can defend itself and before the
        // house starts buying launchers. The radar comes first because it is what
        // the laboratory's prerequisite names, and the laboratory after it because
        // that is the gate: its own chain is what makes the purifier and the
        // launcher buildable, and it also carries the force shield.
        } else if !has(Roles::RADAR) && radar.is_some_and(|want| Self::affordable(ctx, want)) {
            radar
        } else if !has(Roles::LABORATORY)
            && laboratory.is_some_and(|want| Self::affordable(ctx, want))
        {
            laboratory
        } else if !has(Roles::ORE_PURIFIER)
            && purifier.is_some_and(|want| Self::affordable(ctx, want))
        {
            purifier
        } else if !has(Roles::SUPERWEAPON)
            && superweapon.is_some_and(|want| Self::affordable(ctx, want))
        {
            superweapon
        } else if ctx.obs.power_balance() < POWER_MARGIN {
            power
        } else {
            None
        };
        let Some(want) = want else {
            if ctx.tick % 1200 == 0 {
                // Why nothing is wanted is the question a reader of the log
                // actually asks: a house that stops at four defences has
                // candidates it cannot build or cannot reach.
                let tech_note = format!(
                    "lab={} purifier={} super={}",
                    count(Roles::LABORATORY),
                    count(Roles::ORE_PURIFIER),
                    count(Roles::SUPERWEAPON)
                );
                let candidates = format!(
                    "lab_cand={} purifier_cand={} super_cand={}",
                    laboratory.is_some(),
                    purifier.is_some(),
                    superweapon.is_some()
                );
                log::info!(
                    "economy: nothing to build at {} credits={} refineries={} defenses={} tech={} avail={} in_production={:?}",
                    ctx.tick,
                    ctx.obs.credits(),
                    count(Roles::REFINERY),
                    defenses,
                    tech_note,
                    candidates,
                    ctx.obs.building_in_production()
                );
            }
            return;
        };
        let Some(type_id) = ctx.obs.type_id(&want.id) else {
            return;
        };
        if ctx.obs.in_production(ctx.rules, type_id) {
            return;
        }
        // Starting a build the wallet cannot finish strands the factory: the
        // house pays as it builds, and a stall is a base that never grows.
        if !Self::affordable(ctx, want) {
            return;
        }
        out.intents.push(Intent::QueueProduction { type_id });
    }

    /// One harvester per refinery, replaced as they die. A refinery with no
    /// truck is the quietest way for a house to stop earning.
    fn harvesters_for_refineries(&self, ctx: &Ctx<'_>, out: &mut Demands) {
        let mut harvesters = 0;
        let mut refineries = 0;
        for unit in ctx.obs.own_entities() {
            if let Some(profile) = ctx
                .obs
                .object_type(ctx.rules, unit.type_id)
                .and_then(|ty| ctx.profiles.get(&ty.id))
            {
                harvesters += usize::from(profile.roles.contains(Roles::HARVESTER));
                refineries += usize::from(profile.roles.contains(Roles::REFINERY));
            }
        }
        let wanted = refineries * HARVESTERS_PER_REFINERY;
        if refineries == 0 || harvesters >= wanted {
            return;
        }
        let mut want: Option<&UnitProfile> = None;
        for ty in ctx.rules.all_objects() {
            let Some(profile) = ctx.profiles.get(&ty.id) else {
                continue;
            };
            if !profile.roles.contains(Roles::HARVESTER) {
                continue;
            }
            if ctx.obs.can_build(ctx.rules, ty, false) != CanBuild::Yes {
                continue;
            }
            want = Self::cheaper(want, profile);
        }
        let Some(want) = want else {
            return;
        };
        let Some(type_id) = ctx.obs.type_id(&want.id) else {
            return;
        };
        if ctx.obs.in_production(ctx.rules, type_id) {
            return;
        }
        // The first truck on a refinery is income and is bought as soon as the
        // wallet allows. The second is bought from the surplus: measured, the
        // two-truck rule raised the house's strength in four cells of six and
        // lowered it in two, and the two were the cells where the extra truck
        // took the money the army was about to spend.
        let margin = if harvesters >= refineries {
            EXTRA_HARVESTER_MARGIN
        } else {
            200
        };
        if ctx.obs.credits() < want.cost + margin {
            return;
        }
        out.intents.push(Intent::QueueProduction { type_id });
    }

    /// One engineer, so the house can take a building.
    ///
    /// The wish list calls taking an enemy construction yard the centrepiece:
    /// a yard is a production line the house did not have, and every prerequisite
    /// that names that side's yard is satisfied by holding one
    /// (`holds_a_captured_yard_of` in `can_build`). A capturer is also an engineer
    /// for a repair or a garrison, so one on the field does three jobs.
    ///
    /// One is enough until it is spent: the building receiver consumes the
    /// engineer that arrives, so a house that has none on the field and none in
    /// production buys the cheapest capturer it can build and then stops asking.
    fn engineer(ctx: &Ctx<'_>, out: &mut Demands) {
        let mut capturers = 0;
        for unit in ctx.obs.own_entities() {
            if let Some(profile) = ctx
                .obs
                .object_type(ctx.rules, unit.type_id)
                .and_then(|ty| ctx.profiles.get(&ty.id))
            {
                capturers += usize::from(
                    profile.roles.contains(Roles::ENGINEER) || profile.roles.contains(Roles::AGENT),
                );
            }
        }
        if capturers > 0 {
            return;
        }
        let mut want: Option<&UnitProfile> = None;
        for ty in ctx.rules.all_objects() {
            let Some(profile) = ctx.profiles.get(&ty.id) else {
                continue;
            };
            if !(profile.roles.contains(Roles::ENGINEER) || profile.roles.contains(Roles::AGENT)) {
                continue;
            }
            if ctx.obs.can_build(ctx.rules, ty, false) != CanBuild::Yes {
                continue;
            }
            want = Self::cheaper(want, profile);
        }
        let Some(want) = want else {
            return;
        };
        let Some(type_id) = ctx.obs.type_id(&want.id) else {
            return;
        };
        if ctx.obs.in_production(ctx.rules, type_id) {
            return;
        }
        // An engineer is not income and not the army: it is bought from the
        // surplus, the same margin the second truck uses, so a base under attack
        // buys soldiers first.
        if ctx.obs.credits() < want.cost + EXTRA_HARVESTER_MARGIN {
            return;
        }
        out.intents.push(Intent::QueueProduction { type_id });
    }
    fn affordable(ctx: &Ctx<'_>, profile: &UnitProfile) -> bool {
        ctx.obs.credits() >= profile.cost + 200
    }

    fn cheaper<'a>(
        current: Option<&'a UnitProfile>,
        candidate: &'a UnitProfile,
    ) -> Option<&'a UnitProfile> {
        match current {
            Some(best) if (best.cost, &best.id) <= (candidate.cost, &candidate.id) => Some(best),
            _ => Some(candidate),
        }
    }
}
