//! The army's direct orders, for the units no team has taken: an idle combat
//! unit goes on area guard, an engineer walks into the bridge hut whose span is
//! down or into a building it may capture, a spy walks into the building worth
//! most, an `Occupier=` rifleman garrisons a building near the base, and an
//! idle scout detours for a crate it can walk to.
//!
//! Each is a click the player already has, issued as the same command: the
//! Capture click (engineers, spies, bridge huts, garrisons) and the Enter click
//! (docking a vehicle into a depot). Nothing here decides what a click *does* —
//! `InfantryClass::PerCellProcess`, `BuildingClass::CanBeOccupiedBy` and the
//! entry queries own that.
//!
//! Native teams carry the attack. An `AITrigger` creates a team, its script
//! assigns missions, and `team_script_vm` writes them onto the members. Those
//! missions have no player equivalent, and a team's script already runs for a
//! brain-driven house, so this module only covers **the units no team has
//! taken**: an idle one goes on area guard where it stands, and the sim's own
//! Guard mission decides what it engages and where it returns to. That is the
//! native posture for a unit no team has taken — it sits, it does not charge,
//! and it is the sim, not this module, that picks its fight.
//!
//! Sending every idle unit at the nearest hostile instead is what a play test
//! loses with: the opening squad walks into the enemy's own pre-placed force
//! and dies, the plan loses the units it was going to recruit, and the base
//! keeps spending on replacements it cannot afford.
//!
//! That leaves the offensive half to the teams, which is where the retail AI
//! puts it: a hostile the enemy owns is what an `AITrigger` needs, and the
//! enemy search that sets it runs in the house's own Strategy tick
//! (`sim::house_strategy`), which the brain asks for.
//!
//! Every order here passes through [`super::tracked`], which remembers what
//! each unit has been told and refuses to say it again until the intent has
//! expired, has changed, or its target has moved. That is what keeps a toggle
//! from undoing itself and a walk-in from being restarted every frame — and
//! what keeps the whole per-frame budget from going on one army's standing
//! orders.
//!
//! Every order that names an object also passes through [`admits`], which asks
//! the simulation whether this unit can act on that object at all: the same
//! `GetFireError` / `What_Action_OnObject` answer the player's click gets. The
//! brain aims units at what the house can see, not at what the unit can do, so
//! without that door a dog is walked to a tank its jaw cannot touch and an
//! engineer to a building that answers Repair — both refused on arrival, both
//! read as a stuck unit.

use std::collections::{BTreeMap,BTreeSet};

use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::command::Command;
use crate::sim::control::brain::Outbox;
use crate::sim::control::intent::Intent;
use crate::sim::production::CanBuild;
use crate::sim::control::observation::{EnemyEntity, Observation};
use crate::sim::control::{Goal, MicroKind, MicroParams};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::strategy::assist::AssistBrain;

use super::advance::cell_distance;
use super::squared_distance;
use super::tracked::TrackedKind;
use super::tasks::JobKind;

mod common;

/// The admission half, re-exported so every caller keeps the path it had.
pub(in crate::sim::strategy::assist) use common::{
    admits, already_attacking, committed_garrison_entry, issue, issue_tracked,
    on_map, ordered_this_frame, reachable,
};

/// Bound the work of a single task per frame, not the total units controlled.
/// The interactive host accepts 512 commands per frame; eight here was a
/// separate L3 choke point. With ten starting infantry near the MCV, two were
/// untouched until the next task refresh and could obstruct its foundation.
/// A moderate cap still avoids flooding a 20k-unit match with low-value orders.
const FORCE_ORDERS_PER_FRAME: usize = 24;

/// The intents that mean a unit is already on its way somewhere: the advance
/// step owns those, and a guard here would pull it back out of the push.
const PUSHING: &[TrackedKind] = &[
    TrackedKind::Attack,
    TrackedKind::AttackMove,
    TrackedKind::Move,
];

/// How near one of the house's own buildings a bridge or a garrison has to be
/// for the policy to spend a unit on it, in cells. A garrison is only worth
/// holding where the base is, and a bridge nobody crosses is an engineer spent
/// for nothing.
const HOME_REACH_CELLS: i32 = 12;

/// How many of the house's soldiers may be inside buildings at once.
///
/// This home reserve still leaves mobile guards outside. Front-line houses
/// add their actual open seats to the ceiling, up to a separate bounded cap.
/// A garrisoned rifleman is in limbo: `order_actor_admits` refuses it, and no
/// step here brings it back out, so every unit sent in leaves the army for
/// good. Measured on Death.mmx (America, seed 24301, 36000 ticks) the uncapped
/// rule put 28-40 riflemen into `Mission::Capture` and out of the playfield,
/// which is where the 56-unit gap between the old `fighters=99` and the 50
/// units the brain could actually move came from. Retail's own base-defence
/// teams hold a handful of structures; the rest of the army stays outside.
const GARRISON_MAX: usize = 10;

// Why there is no boarding step here. Wiring one — an idle rifleman climbing
// into his own empty carrier, a correct Enter click (`Goal::Enter` and
// `can_enter_transport` both answered as expected) — cost the reference match
// nine thousand ticks: `won at 25636` became `won at 34897`, and with the army's
// strength count corrected to skip units the gate would refuse, the match
// stopped winning inside its 36000-tick cap. Whether the carrier loses value
// once loaded or the boarded unit is simply not where the push needs it is not
// established, so the step stays out and recorded rather than guessed at.

/// Every unit that walks into an enemy building takes one it may: the Capture
/// click, which an engineer's capture and a spy's infiltration both are.
///
/// Natively the capture mission starts from the same order path the player's
/// click uses; a brain-driven house never receives that click, so the brain
/// issues the order. The two units differ only in what the building's own
/// per-cell handler does when they arrive — `InfantryClass::PerCellProcess`
/// captures it for an `Engineer=` and infiltrates it for an `Infiltrate=` — so
/// one order serves both, and neither unit would otherwise do anything: a spy
/// carries no weapon worth ranking and an engineer carries none at all.
///
/// What they walk at differs, because the click's own admission does: a spy
/// needs `Spyable=` (see [`Observation::spyable_buildings`]) while an engineer
/// needs `Capturable=`. And a spy's target is picked by what the building is
/// worth, not by what is nearest — walking the house's only spy into the
/// derrick next door is how the feature is wasted.
pub(super) fn captures(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    // A dog can discover a cash refinery long before we have a spy. Remember
    // the *observed cell* even if no engineer/spy exists yet; otherwise the
    // early return below loses the only reconnaissance that could later guide
    // a spy through shroud. A remembered cell is never authority to click a
    // hidden entity -- it can only be a Move/reconnaissance destination.
    // Store only real sightings. Full-map spyable_buildings is a native
    // entity query, not permission to learn a hidden building's position.
    // Close agent missions only on observable disappearance, not on the
    // assumption that a spy made it inside. The next spy may try the same
    // money target after a bounded cooldown; one raid is never a permanent
    // 'already infiltrated' flag for a Storage building.
    let alive: BTreeSet<u64> = brain.census.units.iter()
        .map(|unit| unit.entity_id).collect();
    let finished: Vec<(u64,u64)> = brain.spy_attempts.iter()
        .filter(|(agent,_)| !alive.contains(agent))
        .map(|(agent,(building,_,_))| (*agent,*building)).collect();
    for (agent, building) in finished {
        brain.spy_attempts.remove(&agent);
        brain.spy_still.remove(&agent);
        brain.spy_target_retry.insert(building, tick.saturating_add(450));
        log::debug!("auto play: spy {agent} disappeared at target {building}; outcome unconfirmed, retry permitted later");
    }
    brain.spy_target_retry.retain(|_,until| *until > tick);
    brain.specialist_risk_after.retain(|_, until| *until > tick);
    brain.spy_kind_recent.retain(|_, when|
        tick.saturating_sub(*when) < 2_400);
    brain.spy_still.retain(|agent,_| alive.contains(agent));
    brain.spy_uniform_retry.retain(|agent,_| alive.contains(agent));
    brain.captured_assets.retain(|id,_| obs.own_entities().any(|e| e.entity_id == *id));
    let visible_spy_cells: BTreeSet<(u16, u16)> = obs.spyable_buildings(rules)
        .filter(|e| obs.is_cell_visible(e.cell.0, e.cell.1))
        .map(|e| e.cell).collect();
    for &cell in &visible_spy_cells { brain.spy_sites.insert(cell, tick); }
    brain.spy_sites.retain(|cell, since| {
        tick.saturating_sub(*since) < SPY_SITE_MEMORY_FRAMES
            && (!obs.is_cell_visible(cell.0, cell.1) || visible_spy_cells.contains(cell))
    });
    // Always reconcile observed ownership *before* checking for available
    // engineers. The successful engineer is consumed on entering the building;
    // there may be no engineer alive on the very frame its capture appears.
    // Neutral structures are public map objectives (v2 requirement). Enemy
    // buildings are not: an enemy-owned factory in unexplored fog cannot be
    // chosen for an engineer simply because the native list knows it exists.
    let passive_cover:BTreeSet<_>=obs.neutral_structures()
        .filter(|site|obs.is_cell_visible(site.cell.0,site.cell.1))
        .filter(|site|obs.object_type(rules,site.type_id).is_some_and(|ty|ty.can_be_occupied))
        .map(|site|site.entity_id).collect();
    let engineer_targets: Vec<EnemyEntity> = if obs.is_campaign() {
        obs.campaign_engineer_sites(rules).filter_map(|(site,repair)|(!repair).then_some(site)).collect()
    } else {
        obs.capturable_buildings(rules).filter(|target|
            obs.is_cell_visible(target.cell.0,target.cell.1)
                && !passive_cover.contains(&target.entity_id)).collect()
    };
    // Book the captures that landed since the last look. A building that was
    // capturable and is not any more either changed hands or was destroyed, and
    // the house owning it now is what tells the two apart. Pairing the two
    // readings is the only way the ratio in `EngineerLedger` learns that an
    // engineer's errand worked: the unit itself is consumed by success and by
    // death alike, so its disappearance says nothing.
    {
        let now: BTreeSet<u64> = engineer_targets
            .iter()
            .map(|target| target.entity_id)
            .collect();
        let owned: BTreeSet<u64> = obs.own_entities().map(|unit| unit.entity_id).collect();
        let landed: Vec<u64> = brain
            .capturable_last
            .iter()
            .filter(|id| !now.contains(*id))
            .filter(|id| owned.contains(*id))
            .copied().collect();
        for id in landed {
            brain.engineers.note_captured();
            brain.captured_assets.insert(id, tick);
            log::debug!("auto play: engineer captured building {id} at tick {tick}");
        }
        brain.capturable_last = now;
    }
    if !brain.census.units.iter().any(|unit|
        walks_into_buildings_flags(&unit.traits)) { return; }
    // Which of those belong to an enemy, and where that enemy's units stand.
    // `capturable_buildings` is every `Capturable=` building the house can see —
    // the map's own oil derricks and hospitals included — and the nearest of
    // them was how an engineer walked straight into a base: the derrick inside
    // the enemy's yard is near, and the engineer that goes for it is a 500-credit
    // gift. Buildings do not guard anything, so only units count as pressure.
    let hostile: BTreeSet<u64> = obs
        .visible_enemy_entities()
        .map(|entity| entity.entity_id)
        .collect();
    let guards: Vec<(u16, u16)> = obs
        .visible_enemy_entities()
        .filter(|entity| {
            obs.object_type(rules, entity.type_id)
                .is_some_and(|ty| ty.category != ObjectCategory::Building)
        })
        .map(|entity| entity.cell)
        .collect();
    // Only real, currently operational gun emplacements obstruct a safe
    // engineer approach. A *defenceless* captured construction yard, factory
    // or tech lab must not count as a gun and forever exclude itself or the
    // abandoned factory standing behind it from all engineer missions.
    let site_threats: Vec<_> = obs.visible_enemy_entities().collect();
    // Destination survivability is independent of the claimant. Evaluate it
    // once per candidate instead of repeating the building/weapon scan for
    // every engineer. The final arbiter still checks the live assignment.
    let eligible_engineer_targets: Vec<_> = engineer_targets.iter().copied()
        .filter(|target| super::asset_ops::capture_opportunity(brain,rules,obs,target,&site_threats))
        .collect();
    let structures: Vec<(u16, u16)> = obs.visible_enemy_entities()
        .filter(|entity| obs.object_type(rules, entity.type_id).is_some_and(|ty| {
            ty.category == ObjectCategory::Building && ty.is_base_defense
                && obs.visible_enemy_building_operational(rules, entity.entity_id)
        }))
        .map(|entity| entity.cell).collect();
    // What can see a spy coming. The retail rules mark no `Detector=` key on any
    // type, and the dog's own section (`[DOG] NotHuman=yes`, `Natural=yes`)
    // carries nothing about spies either: the reveal is engine behaviour, and the
    // only retail-marked pair for it is the dog's. A unit whose weapon warhead
    // mind-controls is on the list for the worse reason — a spy that is taken is
    // a spy the enemy now owns.
    let watchers: Vec<(u16, u16)> = obs
        .visible_enemy_entities()
        .filter(|entity| {
            obs.object_type(rules, entity.type_id).is_some_and(|ty| {
                super::census::Traits::of(rules, ty).scout
                    || mind_controls(rules, ty)
                    || super::kite::carries_parasite(rules, ty)
            })
        })
        .map(|entity| entity.cell)
        .collect();
    // The *disguise* specimen is decided before the infiltration-route veto.
    // A dog beside a refinery makes the infiltration unsafe, but that must
    // not prevent a remote MakeupKit shot at a Conscript elsewhere on the map.
    let disguise_sites: Vec<(EnemyEntity, u8)> = obs
        .spyable_buildings(rules)
        .filter(|target| obs.is_cell_visible(target.cell.0, target.cell.1))
        .filter_map(|target| {
            let ty = obs.object_type(rules, target.type_id)?;
            // A class the house has already unlocked is not worth a second spy:
            // one barracks infiltration gives veteran infantry for the match,
            // one war factory gives veteran vehicles, one lab gives the tech.
            // The walk is spent on money, power, radar and the superweapon
            // instead, where the effect is per building.
            if !spy_class_open(rules, ty, &brain.infiltrated) {
                return None;
            }
            Some((target, infiltration_value(rules, ty, obs.credits())))
        })
        .collect();
    let spy_targets: Vec<(EnemyEntity, u8)> = disguise_sites.iter().copied()
        .filter(|(target, _)| !spy_route_blocked(obs, rules, target.cell, &watchers))
        // An engineer already claiming this prize should take ownership,
        // rather than have a spy enter it at the same moment.
        .filter(|(target, _)| !engineer_claimed(
            rules, obs, &brain.census, &brain.tracked, *target))
        // A second spy should choose another live target briefly after the
        // first disappears, but never mark money/power as one-time effects.
        .filter(|(target, _)| !brain.spy_target_retry.contains_key(&target.entity_id))
        .collect();
    // A missing spy is no proof of success: dogs, turrets and capture can
    // all remove it before infiltration. Only permanent house effects confirm
    // the one-time veterancy and stolen-tech classes. Money and blackout are
    // temporary effects, so neither is falsely marked as a lasting unlock.
    if !brain.spy_errand.is_empty() {
        let alive: std::collections::BTreeSet<u64> = brain
            .census
            .units
            .iter()
            .map(|unit| unit.entity_id)
            .collect();
        let done: Vec<(u64, u8)> = brain
            .spy_errand
            .iter()
            .filter(|(unit, _)| !alive.contains(*unit))
            .map(|(unit, code)| (*unit, *code))
            .collect();
        for (unit, _) in &done {
            brain.spy_errand.remove(unit);
        }
        for (_, code) in done {
            // The spy may have been killed by a dog. Only the actual house
            // veterans/stolen-tech flags are a receipt, not disappearance.
            if obs.spy_class_effect_observed(code) {
                brain.infiltrated.insert(code);
            }
        }
    }
    let mut issued = 0;
    for unit in &brain.census.units {
        if issued >= FORCE_ORDERS_PER_FRAME {
            break;
        }
        if !unit.controllable() || ordered_this_frame(out, unit.entity_id)
            || brain.specialist_shuttles.values().any(|task|
                task.passenger == unit.entity_id) {
            continue;
        }
        let Some(ty) = obs.object_type(rules, unit.type_id) else {
            continue;
        };
        if !walks_into_buildings(rules, ty) {
            continue;
        }
        if ty.engineer
            && (brain.tracked.held(tick,unit.entity_id)
                || brain.campaign_repairs.values().any(|(actor,_,_)|*actor==unit.entity_id))
        {continue;}
        if ty.agent {
            // An immobilized spy with a valid old Capture intent is no longer
            // making progress. Release that intent and the site briefly so a
            // new approach or different building can be considered.
            let state = brain.spy_still.entry(unit.entity_id)
                .or_insert((unit.cell, tick));
            if state.0 != unit.cell {
                *state = (unit.cell, tick);
            } else if tick.saturating_sub(state.1) >= 300
                && let Some((building, _, _)) = brain.spy_attempts.remove(&unit.entity_id)
            {
                brain.tracked.tasks.release(unit.entity_id);
                brain.spy_target_retry.insert(building, tick.saturating_add(360));
                brain.tracked.void(unit.entity_id, TrackedKind::Capture);
                brain.tracked.forget_primary(unit.entity_id);
                state.1 = tick;
                log::debug!("auto play: spy {} stalled; reconsider infiltration target {}",
                    unit.entity_id, building);
            }
        }
        // Retail [SPY]'s MakeupKit carries MakesDisguise: the same ordinary
        // Attack click the player uses makes the spy wear the *victim's* enemy
        // house uniform. Use the native fire/detonation path; never edit
        // Simulation disguise state. Only click an actually visible enemy
        // infantryman from the house whose building we intend to infiltrate.
        if ty.agent && has_disguise_weapon(rules, ty) {
            // Retail MakeupKit has Range=-2: its native shot changes clothes
            // without walking to the specimen. The old 18-cell and straight
            // path safety checks excluded exactly the useful distant soldiers
            // before L2 ever saw Attack. Modded positive-range kits retain a
            // conservative approach check; no teleport or sim-state write.
            let remote = disguise_shot_is_remote(rules, ty);
            let desired = disguise_sites.iter()
                .filter(|(site, _)| obs.can_take_building(
                    rules, unit.entity_id, site.entity_id))
                .filter(|(site, _)| !obs.disguised_to_visible_enemy(
                    unit.entity_id, site.entity_id))
                .max_by_key(|(site, value)| (spy_target_score(
                    brain, rules, obs, site, *value, tick),
                    -squared_distance(unit.cell, site.cell)));
            // A spy can change uniform *before* the dog discovers a refinery.
            // Requiring a currently visible Spyable building to fire the
            // MakeupKit was a needless dependency: enemy infantry can be
            // visible long before their factory is. When a site is visible,
            // insist on that house's uniform; otherwise take any safe visible
            // hostile infantry specimen and save the long walk for later.
            let uniform = obs.visible_enemy_entities()
                .filter(|enemy| desired.map_or(true, |(site, _)|
                    obs.visible_enemy_same_owner(enemy.entity_id, site.entity_id)))
                .filter(|enemy| obs.entity_type(rules, enemy.entity_id)
                    .is_some_and(|inf| inf.category == ObjectCategory::Infantry
                        && !inf.agent && !mind_controls(rules, inf)
                        && !super::kite::carries_parasite(rules, inf)
                        && !super::census::Traits::of(rules, inf).scout))
                .filter(|enemy| !obs.disguised_to_visible_enemy(
                    unit.entity_id, enemy.entity_id))
                .filter(|enemy| remote
                    || (squared_distance(unit.cell, enemy.cell) <= 18 * 18
                        && !path_crosses(&watchers, unit.cell, enemy.cell,
                            SPY_WATCH_CELLS)
                        && !spy_gun_crossfire_except(rules, obs,
                            unit.entity_id, unit.cell, enemy.cell, Some(enemy.entity_id))))
                // A legal fire error is necessary, but does not by itself
                // prove that the projectile hit or the uniform changed.
                .filter(|enemy| admits(rules, obs, unit.entity_id,
                    &Goal::Attack { target: enemy.entity_id }))
                .min_by_key(|enemy| (squared_distance(unit.cell, enemy.cell), enemy.entity_id));
            if let Some(infantry) = uniform {
                if obs.attack_target(unit.entity_id) == Some(infantry.entity_id) {
                    continue;
                }
                if brain.spy_uniform_retry.get(&unit.entity_id)
                    .is_some_and(|(specimen, until)|
                        *specimen == infantry.entity_id && tick < *until)
                {
                    continue;
                }
                if issue_tracked(&mut brain.tracked, brain.trace, out,
                    rules, obs, unit.entity_id, TrackedKind::Engage,
                    Some(infantry.entity_id), Some(infantry.cell),
                    Goal::Attack { target: infantry.entity_id }) {
                    brain.spy_uniform_retry.insert(unit.entity_id,
                        (infantry.entity_id, tick.saturating_add(75)));
                    log::info!("auto play: spy {} issued disguise Attack on enemy infantry {} (remote={remote}) targeted site={:?}",
                        unit.entity_id, infantry.entity_id,
                        desired.map(|(site, _)| site.entity_id));
                    issued += 1;
                    continue;
                }
            } else if tick % 150 == 0 {
                log::info!("auto play: spy {} no legal uniform specimen (remote={remote}, site={:?}); awaiting visibility",
                    unit.entity_id, desired.map(|(site, _)| site.entity_id));
            }
        }
        // A spy takes what is worth most; anyone else takes what is nearest.
        let chosen = if ty.agent {
            let reserved = brain.tracked.tasks.get(unit.entity_id)
                .filter(|job| job.kind == JobKind::Spy).and_then(|job|
                    spy_targets.iter().find(|(target, _)| Some(target.entity_id) == job.target
                        && obs.can_take_building(rules, unit.entity_id, target.entity_id))
                    .map(|(target, _)| *target));
            reserved.or_else(|| spy_targets
                .iter()
                .filter(|(target, _)| obs.can_take_building(
                    rules, unit.entity_id, target.entity_id))
                .filter(|(target, _)| !brain.tracked.claimed_by_other(
                    tick, TrackedKind::Capture, target.entity_id, unit.entity_id))
                .filter(|(target, _)| !brain.tracked.tasks.reserved(
                    JobKind::Spy, target.entity_id, unit.entity_id))
                .filter(|(target, _)| !brain.specialist_risk_after
                    .get(&(unit.entity_id, target.entity_id))
                    .is_some_and(|until| *until > tick))
                // A safe target cell is not enough if the shortest approach
                // runs through a dog or a mind-controller. Keep the spy for
                // another opportunity instead of consuming it for nothing.
                .filter(|(target, _)| !path_crosses(
                    &watchers, unit.cell, target.cell, SPY_WATCH_CELLS))
                // The engine seeds PermaDisguise wearing our own house, not
                // necessarily a uniform that fools *this* enemy. Until the
                // native command path can change uniform, treat unmasked spies
                // like fragile infantry: a turret-covered approach waits.
                .filter(|(target, _)| {
                    obs.disguised_to_visible_enemy(unit.entity_id, target.entity_id)
                        || !spy_gun_crossfire(rules, obs, unit.entity_id,
                            unit.cell, target.cell)
                })
                .max_by_key(|(target, value)| (spy_target_score(
                    brain, rules, obs, target, *value, tick),
                    -squared_distance(unit.cell, target.cell)))
                .map(|(target, _)| *target))
        } else {
            // One building takes one engineer. Without this both were sent at
            // the same derrick, the first took it, and the second arrived at a
            // building that was already ours and stood there.
            let approach = super::safety::envelopes(rules,obs,unit.entity_id,&site_threats,true);
            let free: Vec<EnemyEntity> = eligible_engineer_targets
                .iter()
                .copied()
                // Only the buildings this engineer's click takes: an ally's
                // plant answers Repair, and a `Capturable=` wall that is not
                // `Repairable=` answers nothing at all. The list itself is the
                // type-level candidate set; the click is per engineer.
                .filter(|target| obs.can_take_building(rules, unit.entity_id, target.entity_id))
                // Respect V37's recovery window even in the legacy fallback.
                // Only the original claimant may continue an active capture.
                .filter(|target| brain.asset_ops.permits_capture(
                    unit.entity_id, target.entity_id))
                .filter(|target| super::safety::safe_leg(&approach,unit.cell,target.cell,false)
                    && super::safety::specialist_approach_safe(rules,obs,unit.entity_id,target.entity_id))
                // A visible neutral factory across an impassable cliff is not
                // an engineer mission. Evaluate the building entrance, not
                // its blocked footprint cell.
                .filter(|target| engineer_building_approachable(
                    rules,obs,ty.movement_zone,unit.cell,target))
                .filter(|target| {
                    !brain.tracked.claimed_by_other(
                        tick,
                        TrackedKind::Capture,
                        target.entity_id,
                        unit.entity_id,
                    ) && !brain.tracked.tasks.reserved(
                        JobKind::Engineer, target.entity_id, unit.entity_id)
                        && !brain.specialist_risk_after
                            .get(&(unit.entity_id, target.entity_id))
                            .is_some_and(|until| *until > tick)
                })
                .collect();
            brain.tracked.tasks.get(unit.entity_id)
                .filter(|job| job.kind == JobKind::Engineer)
                .and_then(|job| free.iter().find(|target| Some(target.entity_id) == job.target)
                    .copied())
                // V37: an operation retains its high-value objective across
                // frames and engineers. It still passes this exact same native
                // Capture, path/guard and TaskLedger admission pipeline.
                .or_else(|| brain.asset_ops.preferred_target(
                    rules, obs, unit.entity_id, unit.cell, &free, &guards, &structures,
                    brain.joint_ops.prize(), &brain.tracked,
                ))
                .or_else(|| engineer_preference(
                    rules, obs, &free, &hostile, &guards, &structures,
                    unit.cell, brain.census.refineries < DERRICK_WORTH_REFINERIES,
                ))
        };
        let Some(target) = chosen else {
            // A spy may have been produced *after* the dog last saw the
            // refinery. Last sighting is not authority to click an invisible
            // building: walk towards that remembered site, then ask the native
            // Capture admission once it comes into real sight again.
            if ty.agent {
                if let Some(post) = spy_recon_post(brain, rules, obs, unit.cell, ty.movement_zone) {
                    let _ = issue_tracked(
                        &mut brain.tracked, brain.trace, out, rules, obs,
                        unit.entity_id, TrackedKind::Move, None, Some(post),
                        Goal::Move { cell: post },
                    );
                }
            }
            continue;
        };
        // Capture safety applies to *new* and continuing walks alike. After
        // a dangerous walk is aborted, the next frame must not immediately
        // send the same engineer into the same tank again.
        //
        // `TrackedKind::Capture`'s window is 300 frames — the length of a walk
        // across the map. An engineer that stepped out towards a clear derrick
        // and then met a tank halfway was locked in for five seconds with
        // nothing re-reading the ground: the wishlist's "工程师开始绕远路，最后
        // 还是送死，完全不会识别威胁单位". The window is right for "do not ask
        // again", and wrong for "do not look again", so the distance test runs
        // here every frame the walk is live.
        //
        // The route test, which walks a line, runs only every
        // [`CAPTURE_RECHECK_FRAMES`]. Gating the whole check on that modulus was
        // measured and fired zero times in four matches: it is one frame in
        // thirty, so the gate was reading the clock rather than the ground.
        // Ordinary riflemen do not reveal a disguised spy. Engineers must
        // avoid every hostile rifle; spies avoid the actual detector/psychic
        // watchers only. Applying the engineer's guard list here aborted every
        // cash theft as soon as a refinery came into a normal garrison's view.
        let wearing_enemy_uniform = ty.agent
            && obs.disguised_to_visible_enemy(unit.entity_id, target.entity_id);
        let route_threats = capture_route_threats(wearing_enemy_uniform, &guards, &watchers);
        // The aim is not 'ignore towers' but 'ignore towers that truly regard
        // this spy as friendly'. A native disguise rejected by the observer
        // makes all regular weapons dangerous along the route.
        let guns_unsafe = if ty.engineer {
            !super::safety::specialist_approach_safe(rules,obs,unit.entity_id,target.entity_id)
        } else { (!ty.agent || !wearing_enemy_uniform)
            && spy_gun_crossfire(rules, obs, unit.entity_id, unit.cell, target.cell) };
        let close = route_threats.iter().any(|cell| {
            squared_distance(*cell, unit.cell) <= CAPTURE_CLEAR_CELLS * CAPTURE_CLEAR_CELLS
        });
        let crossed = tick % CAPTURE_RECHECK_FRAMES == 0
            && path_crosses(route_threats, unit.cell, target.cell, PATH_CLEAR_CELLS);
        if (!ty.engineer && (close || crossed)) || guns_unsafe {
            brain.tracked.tasks.release(unit.entity_id);
            brain.specialist_risk_after.insert((unit.entity_id, target.entity_id),
                tick.saturating_add(240));
            if ty.engineer {
                brain.asset_ops.aborted(tick,unit.entity_id,target.entity_id);
                brain.tracked.hold(unit.entity_id,tick.saturating_add(90));
            }
            brain.tracked.void(unit.entity_id, TrackedKind::Capture);
            // The walk is no longer safe: it is an abort, not a
            // temporary dodge that should later restore Capture. Clear
            // its strategic primary so a bodyguard also stands down.
            brain.tracked.forget_primary(unit.entity_id);
            // Cancel an already accepted native Capture: forgetting the L4
            // intent alone does not stop a specialist walking into gunfire.
            if !out.ordered_this_frame(unit.entity_id) {
                let _ = out.want(unit.entity_id, 0, Goal::Stop);
            }
            log::debug!(
                "auto play: specialist {} drops the walk, the ground turned bad",
                unit.entity_id
            );
            continue;
        }
        // Reserve this unit only if its capture click passed the map, actor
        // and frame arbiter. A failed click is not a successful spy errand.
        if !issue_tracked(
            &mut brain.tracked, brain.trace, out, rules, obs,
            unit.entity_id, TrackedKind::Capture,
            Some(target.entity_id), Some(target.cell),
            Goal::Capture { target: target.entity_id },
        ) {
            continue;
        }
        brain.tracked.tasks.claim(tick, unit.entity_id,
            if ty.agent { JobKind::Spy } else { JobKind::Engineer },
            Some(target.entity_id), Some(target.cell), unit.cell);
        if !ty.agent {
            // Submitted is not ownership; V37 waits for the actual owned
            // entity observation before celebrating or investing in its tech.
            brain.asset_ops.submitted(tick, unit.entity_id, target.entity_id);
        }
        // Remember the requested one-time infiltration class until the spy
        // disappears, then check the *actual* house effect before crediting it.
        // Death on the route must never count as a successful unlock.
        if ty.agent {
            brain.spy_attempts.insert(unit.entity_id,
                (target.entity_id, target.cell, tick));
            if let Some(target_ty) = obs.object_type(rules, target.type_id) {
                brain.spy_kind_recent.insert(spy_purpose(rules, target_ty), tick);
            }
            if let Some(code) = obs.object_type(rules, target.type_id)
                .and_then(|target_ty| spy_class_code(rules, target_ty))
            {
                brain.spy_errand.insert(unit.entity_id, code);
            }
        }
        issued += 1;
    }
}

/// Revalidate the *entire remaining path* of each live specialist mission.
/// An old native Capture keeps walking even if a new tower is discovered.
/// Merely removing the tracked intent does not stop it; this pass must replace
/// the dangerous native mission with a safe extraction or Stop order.
/// Only visible enemies contribute to the veto, so fog remains honest.
pub(super) fn supervise_specialists(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    let tick = obs.tick();
    let jobs: Vec<_> = brain.tracked.tasks.entries()
        .filter(|(_, job)| matches!(job.kind, JobKind::Engineer | JobKind::Spy | JobKind::Garrison))
        .collect();
    for (actor, job) in jobs.into_iter().take(FORCE_ORDERS_PER_FRAME) {
        let Some(unit) = brain.census.units.iter()
            .find(|unit| unit.entity_id == actor && unit.controllable()) else { continue };
        let Some(to) = job.cell else { continue };
        let disguised = job.kind == JobKind::Spy
            && job.target.is_some_and(|id| obs.disguised_to_visible_enemy(actor, id));
        // Engineer and garrison entries use the same active-weapon envelope
        // as new orders and final review. In particular an offline tower or
        // a non-damaging weapon is not a new reason to cancel the walk.
        let watched = if job.kind==JobKind::Engineer {
            job.target.is_none_or(|target|
                !super::safety::specialist_approach_safe(rules,obs,actor,target)
                || !obs.can_take_building(rules,actor,target)
                || !super::site_risk::capture_site_safe(&brain.census,rules,obs,target,
                    &obs.visible_enemy_entities().collect::<Vec<_>>()))
        } else if job.kind==JobKind::Garrison {
            job.target.is_none_or(|target|
                !super::safety::garrison_approach_safe(rules,obs,actor,target))
        } else { obs.visible_enemy_entities().any(|enemy| {
            let Some(ty) = obs.entity_type(rules, enemy.entity_id) else { return false };
            let watcher = super::census::Traits::of(rules, ty).scout
                || mind_controls(rules, ty)
                || super::kite::carries_parasite(rules, ty);
            if job.kind == JobKind::Spy && disguised {
                watcher && path_crosses(&[enemy.cell], unit.cell, to, SPY_WATCH_CELLS)
            } else {
                (watcher && job.kind == JobKind::Spy
                    && path_crosses(&[enemy.cell], unit.cell, to, SPY_WATCH_CELLS))
                    || (obs.can_hurt(rules, enemy.entity_id, actor)
                        && super::tactics::effective_reach_against(
                            rules, obs, enemy.entity_id, actor) > 0
                        && path_crosses(&[enemy.cell], unit.cell, to,
                            super::tactics::effective_reach_against(
                                rules, obs, enemy.entity_id, actor).saturating_add(1)))
            }
        }) };
        if !watched { continue; }
        brain.tracked.tasks.release(actor);
        brain.tracked.void(actor, TrackedKind::Capture);
        brain.tracked.forget_primary(actor);
        if let Some(target) = job.target {
            brain.specialist_risk_after.insert((actor, target), tick.saturating_add(240));
            if job.kind == JobKind::Engineer {
                brain.asset_ops.aborted(tick,actor,target);
                brain.tracked.cancel_capture_purchase(tick,target);
                brain.safety.abandon(actor);
                brain.tracked.hold(actor,tick.saturating_add(90));
            }
            if job.kind == JobKind::Spy {
                brain.spy_target_retry.insert(target, tick.saturating_add(180));
                brain.spy_attempts.remove(&actor);
            }
        }
        // Prefer a reachable nearby cell outside current visible weapons.
        // An unactionable Stop is the fallback, never a blind walk through the
        // same defence to a distant home anchor.
        let zone = obs.entity_type(rules, actor).map(|ty| ty.movement_zone);
        let visible:Vec<_>=obs.visible_enemy_entities().collect();
        let guns=super::safety::envelopes(rules,obs,actor,&visible,true);
        let exposed=!super::safety::safe_leg(&guns,unit.cell,unit.cell,false);
        // Do not manufacture an eight-cell retreat merely because the prize
        // vanished into fog or a garrison filled. Stop at the safe current
        // position and let scouting/new legal assignments take over.
        let safe = exposed.then(|| [(0, -8), (8, 0), (0, 8), (-8, 0),
            (-6, -6), (6, -6), (6, 6), (-6, 6),
            (0, -12), (12, 0), (0, 12), (-12, 0)]
            .into_iter().filter_map(|(dx, dy)| {
                let x = u16::try_from(i32::from(unit.cell.0) + dx).ok()?;
                let y = u16::try_from(i32::from(unit.cell.1) + dy).ok()?;
                let cell = (x, y);
                (obs.is_cell_in_playfield(x, y) && obs.is_ground_walkable(x, y)
                    && zone.is_some_and(|zone| obs.can_reach(zone, unit.cell, cell))
                    && super::safety::safe_leg(&guns,unit.cell,cell,true)
                    && super::safety::safe_leg(&guns,cell,cell,false))
                    .then_some(cell)
            }).min_by_key(|cell| (squared_distance(unit.cell, *cell), *cell))).flatten();
        let command = safe.map_or(Goal::Stop, |cell| Goal::Move { cell });
        if out.want(actor, 0, command) {
            log::info!("auto play: {:?} {actor} aborts unsafe capture {:?}, extracts to {:?}",
                job.kind, job.target, safe);
        }
    }
}

/// Bring a long-running job back after its native Move/Capture was interrupted.
/// The task survives the tactical interruption, but reissue is bounded and
/// requires a missing destination and an observable still-valid goal.
pub(super) fn resume_specialist_jobs(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    let tick = obs.tick();
    let jobs: Vec<_> = brain.tracked.tasks.entries().collect();
    let mut resumed = 0;
    for (actor, job) in jobs {
        if resumed >= FORCE_ORDERS_PER_FRAME { break; }
        if job.kind == JobKind::Yard || out.ordered_this_frame(actor)
            || !brain.tracked.tasks.needs_retry(tick, actor, obs.destination(actor))
            || brain.tracked.live_kind(tick, actor)
                .is_some_and(|kind| kind.temporary())
        { continue; }
        let Some(unit) = brain.census.units.iter().find(|unit|
            unit.entity_id == actor && unit.controllable()) else { continue };
        // Capture uses an object navigation target, not a cell. Its native
        // destination may therefore be None for the *entire valid mission*.
        // Do not reset a living Capture mission every 75 frames; if the unit
        // truly stops, reissue even when it has reached the building door.
        if job.kind != JobKind::Crate && unit.in_service() { continue; }
        let (kind, target, goal) = match job.kind {
            JobKind::Crate => {
                let Some(cell) = job.cell else { continue };
                if squared_distance(unit.cell, cell) > 2 * 2
                    && obs.is_cell_visible(cell.0, cell.1)
                    && !obs.visible_crate_at(rules, cell) { continue; }
                (TrackedKind::Move, None, Goal::Move { cell })
            }
            JobKind::Engineer | JobKind::Spy | JobKind::Garrison => {
                let Some(target) = job.target else { continue };
                if obs.observed_object_cell(target).is_none()
                    || (obs.is_hostile(target) && !obs.visible_enemy_entities()
                        .any(|enemy|enemy.entity_id==target)) {continue;}
                let repairing=brain.campaign_repairs.get(&target)
                    .is_some_and(|(owner,_,_)|*owner==actor);
                if job.kind==JobKind::Engineer && ((!repairing && !brain.asset_ops.permits_capture(actor,target))
                    || !obs.can_take_building(rules,actor,target)
                    || !super::safety::specialist_approach_safe(rules,obs,actor,target)) {continue;}
                (TrackedKind::Capture, Some(target), Goal::Capture { target })
            }
            JobKind::Recycle => {
                let Some(target) = job.target else { continue };
                if !obs.controlled_by_own_controller(actor)
                    || !obs.can_enter_grinder(rules, actor, target)
                { continue; }
                (TrackedKind::Enter, Some(target), Goal::Grind { target })
            }
            JobKind::Yard => continue,
        };
        brain.tracked.void(actor, kind);
        if issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
            actor, kind, target, job.cell, goal)
        {
            brain.tracked.tasks.claim(tick, actor, job.kind, job.target,
                job.cell, unit.cell);
            resumed += 1;
        }
    }
}

/// The attack was accepted on an earlier frame, before an engineer booked
/// the objective. Filtering future target choices alone does not stop that
/// existing native Attack mission. Issue one urgent Stop; the normal target
/// selectors are already prevented from acquiring this engineer's prize.
pub(super) fn protect_engineer_prizes(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    let mut stopped = 0;
    for unit in &brain.census.units {
        if stopped >= FORCE_ORDERS_PER_FRAME { break; }
        if !unit.controllable() || !unit.traits.combat
            || ordered_this_frame(out, unit.entity_id) { continue; }
        let Some(target) = obs.attack_target(unit.entity_id) else { continue; };
        let Some(enemy) = brain.census.enemies.iter()
            .find(|enemy| enemy.entity_id == target) else { continue; };
        if !reclamation_reserved(rules, obs, &brain.census, &brain.tracked, *enemy) {
            continue;
        }
        // L2's per-frame priority also prevents the later micro and advance
        // passes from replacing this emergency ceasefire with a new Attack.
        if out.want(unit.entity_id, 0, Goal::Stop) {
            brain.tracked.void(unit.entity_id, TrackedKind::Attack);
            brain.tracked.void(unit.entity_id, TrackedKind::Engage);
            brain.tracked.forget_primary(unit.entity_id);
            stopped += 1;
            log::info!("auto play: cease fire on engineer prize {} by {}",
                target, unit.entity_id);
        }
    }
}

/// Final capture-prize arbitration: ordinary combat proposals must not
/// undo an engineering reservation later in the same frame.
/// Passive tech structures are not demolition targets. A passive building
/// with a genuine live attack can still be answered in self-defence, except
/// during an active capture/repair appointment. Ordinary walls are unchanged.
pub(super) fn protected_neutral(rules:&RuleSet,obs:&Observation<'_>,id:u64)->bool {
    obs.visible_passive_structure(id) && !obs.is_hostile(id)
        && obs.entity_type(rules,id).is_some_and(|ty|
            ty.capturable || ty.can_be_occupied || strategic_public_asset(ty))
}

pub(super) fn review_capture_prizes(brain:&mut AssistBrain,rules:&RuleSet,
    obs:&Observation<'_>,out:&mut Outbox) {
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let mut protected:BTreeMap<u64,(u16,u16)>=BTreeMap::new();
    // The enemy iterator excludes passive neutrals. Include both sources;
    // also protect a just-captured building from an obsolete native attack.
    for e in obs.neutral_structures().filter(|e|obs.is_cell_visible(e.cell.0,e.cell.1)) {
        if protected_neutral(rules,obs,e.entity_id)
            || reclamation_reserved(rules,obs,&brain.census,&brain.tracked,e)
            || brain.cover_detail.contains_key(&e.entity_id) {
            protected.insert(e.entity_id,e.cell);
        }
    }
    for e in &visible {
        if reclamation_reserved(rules,obs,&brain.census,&brain.tracked,*e) {
            protected.insert(e.entity_id,e.cell);
        }
    }
    // Same-frame Capture orders may not yet be reflected in the ledger.
    for order in &out.orders {
        if let Goal::Capture{target}=order.goal {
            if obs.entity_type(rules,order.actor).is_some_and(|ty|ty.engineer)
                && obs.entity_type(rules,target).is_some_and(|ty|ty.category==ObjectCategory::Building)
            {
                if let Some(cell)=obs.observed_object_cell(target) {
                    protected.insert(target,cell);
                }
            }
        }
    }
    let active_engineer_targets:BTreeSet<_>=brain.census.units.iter()
        .filter(|u|u.traits.engineer && u.controllable()
            && u.mission.known()==Some(MissionType::Capture))
        .filter_map(|u|obs.own_destination_object(u.entity_id)).collect();
    let unsafe_attack=|actor:u64,goal:Goal| {
        let (target,at)=match goal {
            Goal::Attack{target}=>{
                if protected.contains_key(&target) || active_engineer_targets.contains(&target) {
                    return true;
                }
                // Ownership may have changed while the native attack was
                // walking. Preserve actual native healing and neutral
                // vehicle-control, not an obsolete ordinary attack.
                if obs.owns_object(target) {
                    return !super::advanced::field_heal_compatible(rules,obs,actor,target);
                }
                (Some(target),obs.observed_object_cell(target))
            },
            Goal::AttackCell{cell}=>(None,Some(cell)),
            _=>return false,
        };
        let Some(at)=at else{return false;};
        // Direct and area shots around an engineer's prize need a separate
        // collateral check. Use actual current weapon records; don't apply
        // an IFV's possible passenger modes to the empty chassis.
        let Some(ty)=obs.entity_type(rules,actor) else{return false;};
        let spread=(0..ty.weapon_record_count()).filter_map(|slot|
            obs.observed_actor_weapon(rules,actor,slot))
            .filter_map(|w|{
                if w.never_use {return None;}
                let head=if w.spawner {
                    let family=rules.missile_spawn.family_of(ty.spawns.as_deref()?)?;
                    let missile=rules.missile_spawn.params(family);
                    if missile.damage<=0 {return None;}
                    rules.warhead(&missile.warhead)?
                }else{
                    if w.damage<=0 {return None;}
                    rules.warhead(w.warhead.as_deref()?)?
                };
                Some(head.cell_spread_f64.ceil().clamp(0.0,16.0) as i32)
            }).max().unwrap_or(0);
        // Do not prevent all defence near a neutral. Only stop a proposed
        // splash whose estimated impact overlaps protected property.
        protected.iter().any(|(id,cell)|Some(*id)!=target
            && cell_distance(at,*cell)<=spread
            && (spread>0 || target.is_none()))
    };
    let mut blocked=BTreeSet::new();
    out.orders.retain(|order|{
        if obs.owned_by_hand(order.actor) || !unsafe_attack(order.actor,order.goal) {return true;}
        blocked.insert(order.actor);false
    });
    // V63 only revisited actors whose *new* order was vetoed. An old native
    // Attack can otherwise continue to destroy the building for minutes.
    for u in &brain.census.units {
        let id=u.entity_id;
        if !u.controllable() || u.in_service() || !u.traits.combat {continue;}
        if obs.attack_target(id).is_some_and(|target|unsafe_attack(id,Goal::Attack{target}))
            && !out.orders.iter().any(|o|o.actor==id) {
            blocked.insert(id);
            out.orders.push(crate::sim::control::intent::Order{actor:id,goal:Goal::Stop});
        }
    }
    for id in blocked {
        brain.tracked.void(id,TrackedKind::Attack);
        brain.tracked.void(id,TrackedKind::Engage);
        brain.tracked.forget_primary(id);
        log::debug!("auto play: v64_capture_fire_veto tick={} actor={id}",obs.tick());
    }
}

/// Only a spy *effectively disguised to that enemy* may ignore ordinary
/// guards. Both an unmasked spy and an engineer avoid visible hostiles.
/// Detector, parasite, and control coverage is handled separately.
fn capture_route_threats<'a>(
    spy: bool, general_guards: &'a [(u16, u16)],
    spy_watchers: &'a [(u16, u16)],
) -> &'a [(u16, u16)] {
    if spy { spy_watchers } else { general_guards }
}

/// Does the straight-line approach to an enemy spy target cross an active
/// hostile weapon's envelope? Used only when the real per-observer disguise
/// query is false; an actually masked spy can walk past ordinary guns but
/// still avoids the separate detector/parasite/psychic watcher set.
fn spy_gun_crossfire(
    rules: &RuleSet, obs: &Observation<'_>, spy: u64,
    from: (u16, u16), to: (u16, u16),
) -> bool {
    spy_gun_crossfire_except(rules, obs, spy, from, to, None)
}

/// The soldier used as a disguise specimen is an unavoidable close-range
/// encounter, not *by itself* proof that no specimen is approachable. Other
/// currently visible hostile fire still blocks the attempted disguise.
fn spy_gun_crossfire_except(
    rules: &RuleSet, obs: &Observation<'_>, spy: u64,
    from: (u16, u16), to: (u16, u16), spared: Option<u64>,
) -> bool {
    obs.visible_enemy_entities().filter(|enemy| Some(enemy.entity_id) != spared).any(|enemy| {
        let Some(ty) = obs.object_type(rules, enemy.type_id) else {
            return false;
        };
        if !obs.can_hurt(rules, enemy.entity_id, spy) {
            return false;
        }
        let reach = super::advance::weapon_range_cells(
            rules, ty, obs.is_deployed(enemy.entity_id));
        reach > 0 && path_crosses(&[enemy.cell], from, to, reach.saturating_add(1))
    })
}

/// A fogged spy target is an old sighting, *not* an entity we may click.
/// Stage a spy several cells short of its last seen building, so the ordinary
/// visibility-gated Capture logic gets another chance. The fog may still be
/// stale: never issue an object-target command here.
fn spy_recon_post(
    brain: &AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
    from: (u16, u16), zone: MovementZone,
) -> Option<(u16, u16)> {
    let sites: Vec<(u16, u16)> = brain.spy_sites.keys().copied()
        .filter(|cell| !obs.is_cell_visible(cell.0, cell.1)).collect();
    sites.into_iter().flat_map(|site| {
        // A nearby approach avoids blindly walking to the building's centre.
        [(5,0), (-5,0), (0,5), (0,-5)].into_iter().filter_map(move |(dx,dy)| {
            let x = u16::try_from(i32::from(site.0) + dx).ok()?;
            let y = u16::try_from(i32::from(site.1) + dy).ok()?;
            Some((x,y))
        })
    })
    .filter(|cell| obs.is_cell_in_playfield(cell.0, cell.1)
        && obs.is_ground_walkable(cell.0, cell.1)
        && (!obs.has_zone_grid() || obs.can_reach(zone, from, *cell)))
    .filter(|cell| !obs.visible_enemy_entities().any(|enemy| {
        obs.entity_type(rules, enemy.entity_id)
            .is_some_and(|ty| super::census::Traits::of(rules, ty).scout
                || mind_controls(rules, ty) || super::kite::carries_parasite(rules, ty))
            && squared_distance(enemy.cell, *cell) <= SPY_WATCH_CELLS * SPY_WATCH_CELLS
    }))
    .min_by_key(|cell| (squared_distance(from, *cell), *cell))
}

/// A valid explosive order can become unsafe while its carrier walks: our
/// own infantry may move into the blast ring, the target may change owners,
/// or the building may be destroyed before the bomb arrives. Cancel that
/// *existing* native Attack rather than merely refusing to issue a new one.
/// A Stop is safety supervision, not evidence that detonation was avoided.
pub(super) fn supervise_hazardous_attacks(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let visible: std::collections::BTreeSet<u64> = obs.visible_enemy_entities()
        .map(|enemy| enemy.entity_id).collect();
    let mut cancelled = 0usize;
    for unit in &brain.census.units {
        if cancelled >= 4 { break; }
        if !unit.controllable() || ordered_this_frame(out,unit.entity_id) { continue; }
        if !matches!(super::doctrine::actor_duty(rules,obs,unit.entity_id),
            Some(super::doctrine::Duty::Disposable | super::doctrine::Duty::BombPlanter))
        { continue; }
        let Some(target) = obs.attack_target(unit.entity_id) else { continue; };
        if visible.contains(&target) && obs.is_hostile(target)
            && super::doctrine::target_ok(rules,obs,unit.entity_id,target)
        { continue; }
        if issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
            unit.entity_id,TrackedKind::Avoid,None,None,Goal::Stop)
        { cancelled += 1; }
    }
}

/// Spend a unit whose weapon spends it on something worth more than it costs.
///
/// The wishlist's "恐怖分子，自爆卡车，疯狂伊万，辐射工兵…有特殊攻击能力且存在
/// 误伤". A `Suicide=yes` primary is not a rifle with a shorter range: firing it
/// removes the unit, so the ordinary rules are all wrong for it — it does not
/// advance with the fist, it does not trade at a third of a health bar, and it
/// does not kite out of a shell, because there is nothing left to save.
///
/// Two things make the errand worth running at all:
///
/// * **Worth.** The blast has to be aimed at something dearer than the body.
///   Measured against `points`, the same currency the census weighs both sides
///   in, so a bomb truck sent at a construction yard is a trade and one sent at
///   a wall is not.
/// * **Splash.** `DemobombWH CellSpread=8` does not read the owner list. A
///   target with the house's own units inside the blast radius is not a target:
///   the wishlist's "误伤", and the reason the second half of this step looks at
///   where the blast would land rather than only at what it would hit.
pub(super) fn suicide_errand(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let targets: Vec<EnemyEntity> = obs.visible_enemy_entities().take(128).collect();
    let mut issued = 0;
    for unit in &brain.census.units {
        if issued >= 3 { break; }
        if !unit.traits.suicide || !unit.controllable() || unit.in_service()
            || ordered_this_frame(out,unit.entity_id) { continue; }
        let chosen = targets.iter().filter(|enemy|
            super::doctrine::target_ok(rules,obs,unit.entity_id,enemy.entity_id)
                && obs.can_engage(rules,unit.entity_id,enemy.entity_id)
                && super::advance::cell_distance(unit.cell,enemy.cell) <= 30)
            .min_by_key(|enemy| (
                squared_distance(unit.cell,enemy.cell),
                std::cmp::Reverse(obs.entity_type(rules,enemy.entity_id)
                    .map_or(0,|ty|ty.cost)),enemy.entity_id));
        let Some(target) = chosen else { continue; };
        // Native Attack retains one explicit victim while walking into firing
        // range. AttackMove would autonomously select an unsafe target on the
        // way, and its blast may take our own squad with it.
        if issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
            unit.entity_id,TrackedKind::Attack,Some(target.entity_id),Some(target.cell),
            Goal::Attack { target: target.entity_id }) { issued += 1; }
    }
}

/// A Crazy-Ivan-type weapon attaches a delayed bomb, rather than performing a
/// normal rifle attack. Never let general-purpose micro or guard march this
/// specialist into friendly troops. Require a currently visible, valuable
/// target with a safety margin around the eventual bomb site; move only via
/// the engine's explicit targeted Attack, not an autonomous AttackMove.
pub(super) fn bomb_errand(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let targets: Vec<EnemyEntity> = obs.visible_enemy_entities().take(128).collect();
    let mut issued = 0;
    for unit in &brain.census.units {
        if issued >= 2 { break; }
        if !unit.controllable() || unit.in_service()
            || ordered_this_frame(out,unit.entity_id)
            || super::doctrine::actor_duty(rules,obs,unit.entity_id)
                != Some(super::doctrine::Duty::BombPlanter)
        { continue; }
        let chosen = targets.iter().filter(|enemy|
            super::advance::cell_distance(unit.cell,enemy.cell) <= 12
                && super::doctrine::target_ok(rules,obs,unit.entity_id,enemy.entity_id)
                && obs.can_engage(rules,unit.entity_id,enemy.entity_id)
                && !super::tactics::unsafe_post(rules,obs,unit.entity_id,enemy.cell))
            .min_by_key(|enemy| (squared_distance(unit.cell,enemy.cell),enemy.entity_id));
        if let Some(target) = chosen
            && issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
                unit.entity_id,TrackedKind::Attack,Some(target.entity_id),Some(target.cell),
                Goal::Attack { target: target.entity_id })
        { issued += 1; }
    }
}

/// Pack a captured yard and drive it home.
///
/// The wishlist's "占领敌方建造场以后，若敌方建造场可以打包回MCV并安全撤离，应当
/// 打包撤离到己方基地". A captured yard stands where the enemy built it, which is
/// by definition not where this house's base is: it is a second yard the enemy
/// can shell at leisure, and it cannot be sold (its type is `BuildConst=`
/// eligible, so [`sell_hard_to_hold`] refuses it by name).
///
/// Three things have to hold before the pack is worth the trip:
///
/// * **The house keeps a yard.** Packing the last one is how a base packs itself
///   up under fire — the wishlist's "主基地车会收缩".
/// * **The receiver allows it.** `can_repack_yard` is the deploy-facing gate, not
///   a rule restated here; it is what refuses a mind-controlled yard and an
///   option that turns the deploy button off.
/// * **It is not already home.** A yard inside the base's own ring is a second
///   yard doing its job; packing it to move it a few cells is a churn.
pub(super) fn pack_captured_yard(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    let Some(home) = brain.census.base_cell else {
        return;
    };
    // Packing the house's only yard leaves it with none: the wishlist's "主基地
    // 车会收缩", which a contested reading of the map made routine.
    if brain.census.yards <= 1 {
        return;
    }
    let mut issued = 0;
    for unit in &brain.census.units {
        if issued >= FORCE_ORDERS_PER_FRAME {
            break;
        }
        let Some(ty) = obs.object_type(rules, unit.type_id) else {
            continue;
        };
        if !ty.construction_yard || obs.owned_by_hand(unit.entity_id) {
            // A grouped yard belongs to the player in non-group mode (and
            // vice versa). Do not mark an undelivered pack as committed.
            continue;
        }
        // A yard standing among the house's *other* buildings is the base, not a
        // prize. Its own cell is in `home_cells` like every building's, so it is
        // excluded first; `census.base_cell` cannot tell the two apart when there
        // are two yards, because the ledger names whichever it saw last.
        let in_the_base = brain.census.home_cells.iter().any(|cell| {
            *cell != unit.cell
                && squared_distance(*cell, unit.cell) <= HOME_REACH_CELLS * HOME_REACH_CELLS
        });
        let foreign_prize=brain.captured_assets.contains_key(&unit.entity_id)
            && super::build::foreign_yard(rules,obs,ty)
            && squared_distance(unit.cell,home)>14*14
            && brain.census.yard_cells.iter().any(|cell|*cell!=unit.cell
                && squared_distance(*cell,home)<=14*14);
        let recall=foreign_prize && ty.undeploys_into.as_deref().and_then(|name|rules.object(name))
            .is_some_and(|maker| {
                let visible:Vec<_>=obs.visible_enemy_entities().collect();
                let Some(rear)=brain.census.rally else {return false;};
                campaign_building_perimeter(rules,obs,unit.entity_id).into_iter().any(|from|
                    obs.can_reach(maker.movement_zone,from,rear)
                    && super::site_risk::prospective_ground_leg(rules,obs,maker,&visible,from,rear))
            });
        if in_the_base && !recall {
            continue;
        }
        // Distance is not evidence of danger. A captured enemy yard is now
        // our forward base when the surrounding enemy has been cleared; the
        // old unconditional pack and the second-yard deployer undid each
        // other after their shared cooldown, creating an endless MCV walk.
        let attackers = brain.census.enemies.iter().filter(|enemy| {
            obs.is_cell_visible(enemy.cell.0, enemy.cell.1)
                && squared_distance(enemy.cell, unit.cell) <= 14 * 14
        }).count();
        if !recall && !should_pack_captured_yard(unit.health_permille, attackers) {
            continue;
        }
        if !obs.can_repack_yard(rules, unit.entity_id) {
            continue;
        }
        if recall {
            log::debug!("auto play: v61_foreign_yard_recall tick={tick} yard={} home={home:?}",unit.entity_id);
        }
        // The other half of the commitment: this yard and the maker it becomes
        // are one object, so the term the deploy wrote is the term this reads.
        if super::expand::yard_committed(brain, tick, unit.entity_id) {
            continue;
        }
        if !brain
            .tracked
            .admits(tick, unit.entity_id, TrackedKind::Deploy, None, None)
        {
            continue;
        }
        brain
            .tracked
            .record(tick, unit.entity_id, TrackedKind::Deploy, None, None);
        brain.yard_commit.insert(unit.entity_id, tick);
        log::info!(
            "auto play: yard {} at {:?} packs and comes home",
            ty.id,
            unit.cell
        );
        out.command(Command::UndeployBuilding {
            entity_id: unit.entity_id,
        });
        issued += 1;
    }
}

/// Packing is an emergency salvage action, not ordinary exploration.
/// A single passing scout is not a reason to uproot a forward base; we
/// require both material damage and more than one visible attacker.
fn should_pack_captured_yard(health_permille: u16, attackers: usize) -> bool {
    (health_permille <= 750 && attackers >= 2)
        || (health_permille <= 400 && attackers >= 1)
}

/// Send an engineer into a `BridgeRepairHut=` whose span is down.
///
/// Walking in is the only thing that rebuilds a bridge: the hut carries no
/// timer and no `Capturable=`, and `BridgeRepairHut` is retail's only consumer
/// ([`Observation::bridge_repair_huts`]). The engineer is consumed by it, which
/// is also retail's price, so this only pays for a bridge inside the house's
/// own reach.
pub(super) fn bridges(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    if !brain.census.units.iter().any(|unit| unit.traits.engineer) {
        return;
    }
    let huts: Vec<(u64, (u16, u16))> = obs
        .bridge_repair_huts(rules)
        .filter(|site| obs.is_cell_visible(site.cell.0, site.cell.1))
        .filter(|site| {
            within(home_cells(brain), site.cell, 60)
                || brain.census.units.iter().any(|unit| unit.controllable()
                    && squared_distance(unit.cell, site.cell) <= 35 * 35)
        })
        .filter(|site| !obs.visible_enemy_entities().any(|enemy|
            squared_distance(enemy.cell, site.cell) <= 8 * 8))
        .map(|site| (site.building_id, site.cell))
        .collect();
    if huts.is_empty() {
        return;
    }
    let mut issued = 0;
    for unit in &brain.census.units {
        if issued >= FORCE_ORDERS_PER_FRAME {
            break;
        }
        let Some(ty) = obs.object_type(rules, unit.type_id) else {
            continue;
        };
        if !ty.engineer
            || !unit.orderable
            || unit.hand
            || ordered_this_frame(out, unit.entity_id)
            || holds_or_fights(unit.mission)
        {
            continue;
        }
        let Some(target) = huts.iter().filter(|(target,_)|
            super::safety::specialist_approach_safe(rules,obs,unit.entity_id,*target))
            .min_by_key(|(target,cell)| (squared_distance(unit.cell,*cell),*target))
            .map(|(target,_)| *target) else {
            continue;
        };
        if issue_tracked(
            &mut brain.tracked, brain.trace, out, rules, obs,
            unit.entity_id, TrackedKind::Capture, Some(target), None,
            Goal::Capture { target },
        ) {
            issued += 1;
        }
    }
}

/// Dense, *previously discovered* mineral fields are worth a forward
/// foothold even when no soldier has marched beside their central buildings.
/// Count ore once into 8-cell buckets, then evaluate each garrison with only
/// nine lookups: a giant procedural map cannot afford to scan every ore cell
/// for every building on every brain tick.
fn ore_garrison_buckets(
    known: &std::collections::BTreeMap<(u16, u16), u8>,
) -> std::collections::BTreeMap<(u16, u16), usize> {
    let mut buckets = std::collections::BTreeMap::new();
    for &(x, y) in known.keys() {
        *buckets.entry((x / 8, y / 8)).or_insert(0) += 1;
    }
    buckets
}

fn ore_garrison_score(
    buckets: &std::collections::BTreeMap<(u16, u16), usize>,
    cell: (u16, u16),
) -> usize {
    let (cx, cy) = (i32::from(cell.0 / 8), i32::from(cell.1 / 8));
    (-1..=1).flat_map(|dx| (-1..=1).filter_map(move |dy| {
        let (Ok(x), Ok(y)) = (u16::try_from(cx + dx), u16::try_from(cy + dy))
            else { return None; };
        buckets.get(&(x, y)).copied()
    })).sum()
}

/// At least a meaningful ore cluster, not a single isolated tile.
const ORE_GARRISON_MIN_CELLS: usize = 10;
const ORE_GARRISON_REACH_CELLS: i32 = 40;
/// Front-line cover is a deliberate objective, not merely a house a soldier
/// happened to walk past on a Move mission.
const FRONT_GARRISON_REACH_CELLS: i32 = 32;
/// Surplus occupiers can establish map-wide outposts without stripping the
/// mobile home screen. Only currently observed, reachable buildings qualify.
const MAP_GARRISON_REACH_CELLS: i32 = 28;
/// Reserve the opening's occupiers for forward mineral chokes and discovered
/// enemy approaches. Filling ten home buildings before scouting a rich seam
/// permanently strands the only riflemen that could contest it.
const HOME_GARRISON_OPENING_CAP: usize = 3;

fn garrison_priority(enemy_front: bool, ore: usize, forward: bool) -> u8 {
    if enemy_front { 0 }
    else if ore >= ORE_GARRISON_MIN_CELLS { 1 }
    else if forward { 2 }
    else { 3 }
}

/// A Capture click targets the building's *occupied* cell. Zone connectivity
/// at that coordinate often reports false even when an infantryman can reach
/// the entrance. Require reachable, walkable ground adjacent to the footprint
/// instead, including larger MOD foundations. This only rejects impossible
/// walks; the native Capture action still decides final admission.
pub(super) fn building_approachable(
    obs: &Observation<'_>, zone: MovementZone,
    from: (u16, u16), building: (u16, u16),
) -> bool {
    if !obs.has_zone_grid() { return true; }
    (1..=4).any(|ring: i32| {
        (-ring..=ring).any(|dy| (-ring..=ring).any(|dx| {
            if dx.abs().max(dy.abs()) != ring { return false; }
            let (Ok(x), Ok(y)) = (
                u16::try_from(i32::from(building.0) + dx),
                u16::try_from(i32::from(building.1) + dy),
            ) else { return false; };
            obs.is_cell_in_playfield(x, y) && obs.is_ground_walkable(x, y)
                && obs.can_reach(zone, from, (x, y))
        }))
    })
}

/// Native footprint perimeter, rather than an arbitrary radius across a
/// river. If the zone map is missing, do not invent a remote campaign route.
pub(super) fn campaign_building_approachable(rules:&RuleSet,obs:&Observation<'_>,
    zone:MovementZone,from:(u16,u16),target:u64)->bool {
    if zone==MovementZone::Fly {return true;}
    if !obs.has_zone_grid() {return false;}
    campaign_building_perimeter(rules,obs,target).into_iter()
        .any(|cell|obs.can_reach(zone,from,cell))
}

fn campaign_building_perimeter(rules:&RuleSet,obs:&Observation<'_>,
    target:u64)->Vec<(u16,u16)> {
    let Some(cell)=obs.observed_object_cell(target) else {return Vec::new();};
    let Some(ty)=obs.entity_type(rules,target) else {return Vec::new();};
    let (w,h)=ty.foundation.dimensions();
    let w=i32::from(w.max(1));
    let h=i32::from(h.max(1));
    let mut result=Vec::new();
    for dy in -1..=h {
        for dx in -1..=w {
            if dx>=0 && dx<w && dy>=0 && dy<h {continue;}
            let (Ok(x),Ok(y))=(u16::try_from(i32::from(cell.0)+dx),
                u16::try_from(i32::from(cell.1)+dy)) else {continue;};
            if obs.is_cell_in_playfield(x,y) && obs.is_ground_walkable(x,y) {result.push((x,y));}
        }
    }
    result
}

pub(super) fn engineer_building_approachable(rules:&RuleSet,obs:&Observation<'_>,
    zone:MovementZone,from:(u16,u16),site:&EnemyEntity)->bool {
    if obs.is_campaign() {
        campaign_building_approachable(rules,obs,zone,from,site.entity_id)
    } else {building_approachable(obs,zone,from,site.cell)}
}

/// An engineer purchase requires a real reachable producer-side approach.
pub(super) fn engineer_recruitment_route(rules:&RuleSet,obs:&Observation<'_>,
    engineer:&ObjectType,site:&EnemyEntity,visible:&[EnemyEntity])->bool {
    if !obs.is_cell_visible(site.cell.0,site.cell.1) {return false;}
    let Some(producer)=obs.current_producer(rules,engineer) else {return false;};
    campaign_building_perimeter(rules,obs,producer).into_iter().any(|from|
        campaign_building_approachable(rules,obs,engineer.movement_zone,from,site.entity_id)
        && super::site_risk::prospective_ground_leg(rules,obs,engineer,visible,from,site.cell))
}

fn valuable_cover(brain:&AssistBrain,rules:&RuleSet,obs:&Observation<'_>,
    cell:(u16,u16),visible:&[EnemyEntity])->bool {
    within(home_cells(brain),cell,12)
        || ore_garrison_score(&ore_garrison_buckets(&brain.ore_known),cell)>=ORE_GARRISON_MIN_CELLS
        || obs.own_entities().any(|e|squared_distance(e.cell,cell)<=10*10
            && obs.object_type(rules,e.type_id).is_some_and(strategic_public_asset))
        || obs.neutral_structures().any(|e|obs.is_cell_visible(e.cell.0,e.cell.1)
            && squared_distance(e.cell,cell)<=10*10
            && obs.object_type(rules,e.type_id).is_some_and(strategic_public_asset))
        || visible.iter().any(|e|squared_distance(e.cell,cell)<=14*14)
}

/// The repair soldier must be able to stand at the actual foundation
/// entrance safely BEFORE the engineer is consumed. Future admission to
/// red cover is not asserted: CanBeOccupiedBy is checked after restoration.
fn cover_partner(brain:&AssistBrain,rules:&RuleSet,obs:&Observation<'_>,
    site:&EnemyEntity,visible:&[EnemyEntity])->Option<(u64,(u16,u16))> {
    if !valuable_cover(brain,rules,obs,site.cell,visible)
        || brain.census.available_fighters()<if obs.is_campaign(){2}else{4}
    {return None;}
    let posts=campaign_building_perimeter(rules,obs,site.entity_id);
    brain.census.units.iter().filter(|u|u.traits.occupier && !u.traits.engineer
        && brain.census.available(u) && u.health_permille>=700
        && !obs.own_elite(u.entity_id) && !obs.controlled_by_own_controller(u.entity_id)
        && brain.tracked.tasks.get(u.entity_id).is_none()
        && !brain.tracked.held(obs.tick(),u.entity_id)
        && !brain.joint_ops.reserves(u.entity_id)
        && !brain.cover_detail.values().any(|(id,_,_)|*id==u.entity_id)
        && obs.attack_target(u.entity_id).is_none()
        && squared_distance(u.cell,site.cell)<=28*28)
        .filter_map(|u|{
            let ty=obs.object_type(rules,u.type_id)?;
            let guns=super::safety::envelopes(rules,obs,u.entity_id,visible,true);
            let post=posts.iter().copied().filter(|p|obs.can_reach(ty.movement_zone,u.cell,*p)
                && super::safety::safe_leg(&guns,u.cell,*p,false)
                && super::safety::safe_leg(&guns,*p,site.cell,false))
                .min_by_key(|p|(squared_distance(u.cell,*p),*p))?;
            Some((squared_distance(u.cell,post),u.entity_id,post))
        }).min().map(|(_,id,post)|(id,post))
}

pub(super) fn campaign_repair_needed(brain:&AssistBrain,rules:&RuleSet,
    obs:&Observation<'_>,site:&EnemyEntity,visible:&[EnemyEntity])->bool {
    if brain.engineer_repair_after.get(&site.entity_id).is_some_and(|end|obs.tick()<*end)
        || brain.campaign_repairs.contains_key(&site.entity_id)
        || !super::site_risk::capture_site_safe(&brain.census,rules,obs,site.entity_id,visible)
    {return false;}
    // A valuable, unclaimed and currently safe strategic capture takes
    // priority over consuming an engineer on a house.
    if obs.capturable_buildings(rules).any(|target|
        obs.is_cell_visible(target.cell.0,target.cell.1)
        && obs.object_type(rules,target.type_id).is_some_and(strategic_public_asset)
        && brain.asset_ops.purchase_ready(target.entity_id)
        && brain.tracked.tasks.pending_at(JobKind::Engineer,target.entity_id)==0
        && super::asset_ops::capture_opportunity(brain,rules,obs,&target,visible)
        && brain.census.units.iter().any(|u|u.traits.engineer && u.controllable()
            && !u.in_service() && obs.can_take_building(rules,u.entity_id,target.entity_id)
            && super::safety::specialist_approach_safe(rules,obs,u.entity_id,target.entity_id)))
    {return false;}
    brain.cover_detail.contains_key(&site.entity_id)
        || cover_partner(brain,rules,obs,site,visible).is_some()
}

/// A bounded purchase opportunity, not permission to enter a hidden or
/// occupied building. Check only known ore/our captured strategic assets,
/// actual open neutral/owned seats, and a plausible reachable barracks exit.
/// The new soldier still goes through native CanBeOccupiedBy and final review.
pub(super) fn garrison_recruitment_site(brain:&AssistBrain,rules:&RuleSet,
    obs:&Observation<'_>,soldier:&ObjectType)->Option<u64> {
    let own:Vec<_>=obs.own_entities().filter(|o|o.orderable).collect();
    let neutral:BTreeSet<_>=obs.neutral_structures()
        .filter(|o|obs.is_cell_visible(o.cell.0,o.cell.1))
        .map(|o|o.entity_id).collect();
    let assets:Vec<_>=own.iter().filter(|o|brain.captured_assets.contains_key(&o.entity_id)
        || obs.object_type(rules,o.type_id).is_some_and(strategic_public_asset))
        .map(|o|o.cell).collect();
    let ore=ore_garrison_buckets(&brain.ore_known);
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let producer=obs.current_producer(rules,soldier)?;
    let home=obs.observed_object_cell(producer)?;
    let mut candidates:Vec<_>=obs.occupiable_buildings(rules).filter(|b|
        obs.is_cell_visible(b.cell.0,b.cell.1)
            && (obs.owns_object(b.building_id) || neutral.contains(&b.building_id))
            && obs.entity_type(rules,b.building_id).is_some_and(|ty|
                obs.health(b.building_id).is_some_and(|hp|
                    f64::from(hp)>f64::from(ty.strength)*f64::from(rules.general.condition_red)))
            && squared_distance(home,b.cell)<=40*40
            && b.room as usize>brain.tracked.tasks.pending_at(JobKind::Garrison,b.building_id)
            && brain.garrison_release_after.get(&b.building_id).is_none_or(|until|obs.tick()>=*until)
            && (within(assets.iter().copied(),b.cell,10)
                || valuable_cover(brain,rules,obs,b.cell,&visible)
                || ore_garrison_score(&ore,b.cell)>=ORE_GARRISON_MIN_CELLS)
    ).collect();
    candidates.sort_unstable_by_key(|b|(!within(assets.iter().copied(),b.cell,10),
        obs.passengers(b.building_id).min(2),squared_distance(home,b.cell),b.building_id));
    candidates.truncate(12);
    for billet in candidates {
        for producer in own.iter().filter(|o|o.entity_id==producer)
        {
            for ring in [2i32,4] {
                for (dx,dy) in [(ring,0),(-ring,0),(0,ring),(0,-ring),(ring,ring),(-ring,ring),(ring,-ring),(-ring,-ring)] {
                    let (Ok(x),Ok(y))=(u16::try_from(i32::from(producer.cell.0)+dx),
                        u16::try_from(i32::from(producer.cell.1)+dy)) else {continue;};
                    let from=(x,y);
                    if obs.is_cell_in_playfield(x,y) && (!obs.has_ground_grid() || obs.is_ground_walkable(x,y))
                        && squared_distance(from,billet.cell)<=40*40
                        && building_approachable(obs,soldier.movement_zone,from,billet.cell)
                        && super::site_risk::prospective_ground_leg(rules,obs,soldier,&visible,from,billet.cell)
                    {return Some(billet.building_id);}
                }
            }
        }
    }
    None
}

/// Idle `Occupier=` infantry garrison a building near the house's own base.
///
/// `BuildingClass::CanBeOccupiedBy` decides whether this infantryman may walk
/// in, so a full building, a red one and a stranger's are all refused by the
/// owner rather than by a second rule here.
///
/// The order is the Capture click, not Enter. A player's right click on a
/// building that answers action 9 is `FootClass::ClickedAction`'s `Capture8`
/// (`0x004D7716..0x004D773A`) — the same command an engineer's capture and a
/// spy's infiltration are, and the one `InfantryClass::PerCellProcess` admits
/// for an `Occupier=` (`0x00519CB1..0x00519D4E`). Enter is the *dock* mission:
/// its receiver asks the building for `DOCK_NOW`, and a `CanBeOccupied=`
/// structure answers that request nowhere (`radio::receive`'s
/// `building_dock_now`), so a rifleman sent with it walks to the wall and
/// stands there — the garrison that never garrisons.
pub(super) fn garrison(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    // Where the army stands counts as home for this. A soldier walking into a
    // building on the way is how a push holds the ground it takes, and a
    // garrison that only ever answers inside the base is a garrison that never
    // leaves it — the base fills up and the army walks past every building it
    // could have held the street from.
    // Defensive structures around the enemy's observed base are strategic
    // garrison objectives even if no friendly soldier has reached them yet.
    let enemy_buildings: Vec<(u16, u16)> = obs.visible_enemy_entities()
        .filter(|enemy| obs.object_type(rules, enemy.type_id)
            .is_some_and(|ty| ty.category == ObjectCategory::Building))
        .map(|enemy| enemy.cell).collect();
    let occupier_cells: Vec<(u16, u16)> = brain.census.units.iter()
        .filter(|unit| unit.traits.occupier && unit.controllable()
            && obs.inside_transport(unit.entity_id).is_none())
        .map(|unit| unit.cell).collect();
    let anchors:Vec<_>=home_cells(brain).collect();
    let enemy_cells:Vec<_>=obs.visible_enemy_entities().map(|e|e.cell).collect();
    let ore_buckets = ore_garrison_buckets(&brain.ore_known);
    let asset_cells:Vec<_>=obs.own_entities().filter(|own|
        obs.object_type(rules,own.type_id).is_some_and(strategic_public_asset))
        .map(|own|own.cell).chain(obs.neutral_structures()
            .filter(|e|obs.is_cell_visible(e.cell.0,e.cell.1)
                && obs.object_type(rules,e.type_id).is_some_and(strategic_public_asset))
            .map(|e|e.cell)).chain(obs.visible_enemy_entities()
            .filter(|e|obs.object_type(rules,e.type_id).is_some_and(strategic_public_asset))
            .map(|e|e.cell)).collect();
    let billets: Vec<_> = obs
        .occupiable_buildings(rules)
        // Occupancy and ownership can change inside fog. A remembered house
        // is not permission to query today's private seat count; once a
        // Capture order is running, the task ledger can finish the trip.
        .filter(|billet| obs.is_cell_visible(billet.cell.0, billet.cell.1))
        .filter(|billet| brain.garrison_release_after.get(&billet.building_id)
            .is_none_or(|until|tick>=*until))
        .filter(|billet| within(asset_cells.iter().copied(),billet.cell,8)
            || within(anchors.iter().copied(), billet.cell, HOME_REACH_CELLS)
            || ore_garrison_score(&ore_buckets, billet.cell) >= ORE_GARRISON_MIN_CELLS
            || enemy_buildings.iter().any(|enemy|
                squared_distance(*enemy, billet.cell) <= 18 * 18)
            || enemy_cells.iter().any(|enemy|
                squared_distance(*enemy,billet.cell)<=12*12))
        .collect();
    if billets.is_empty() {
        return;
    }
    // Do not give up a central ore chokepoint just because the initial
    // riflemen spawned on the map's edge. This value is independent of the
    // order in which occupiable buildings were enumerated.
    let ore_sites: std::collections::BTreeMap<u64, usize> = billets.iter()
        .map(|billet| (billet.building_id,
            ore_garrison_score(&ore_buckets, billet.cell)))
        .filter(|(_, score)| *score >= ORE_GARRISON_MIN_CELLS)
        .collect();
    // The count is exact, not a guess from the census: a garrisoned unit is a
    // passenger of the building it walked into (`Observation::inside_transport`),
    // so a rifleman still in his factory's limbo is not mistaken for one.
    let mut garrisoned = brain
        .census
        .units
        .iter()
        .filter(|unit| unit.traits.occupier && obs.inside_transport(unit.entity_id)
            .is_some_and(|carrier| obs.entity_type(rules, carrier)
                .is_some_and(|ty| ty.category == ObjectCategory::Building)))
        .count();
    // Ordinary home garrisons must not occupy the entire budget that a
    // paradrop needs for the houses immediately outside the enemy perimeter.
    // This is calculated once. Re-scanning every visible enemy for every
    // soldier × building combination becomes quadratic in large games.
    let enemy_front_ids: BTreeSet<u64> = billets.iter().filter(|billet| {
        // OccupyWeapon is a different weapon from the walking rifleman's
        // Primary. Use its real ground range and native armor interaction.
        brain.census.units.iter().filter(|u|u.traits.occupier && u.controllable())
            .filter_map(|u|obs.object_type(rules,u.type_id))
            .filter_map(|ty|ty.occupy_weapon.as_deref().and_then(|name|rules.weapon(name)))
            .any(|weapon| {
                let reach=super::siege_counter::max_range_cells(weapon).max(1);
                weapon.damage>0 && rules.weapon_projectile(weapon).is_some_and(|p|p.ag)
                    && enemy_cells.iter().any(|cell|
                        squared_distance(*cell,billet.cell)<=(reach+3).pow(2))
            })
    })
        .map(|billet| billet.building_id).collect();
    let front_ids: BTreeSet<u64> = billets.iter().filter(|billet| {
        // Explicitly value houses screening a *visible* enemy production line
        // even before the main army reaches it. Do not require a friendly
        // occupier already standing by the building.
        (enemy_front_ids.contains(&billet.building_id)
            && brain.census.base_cell.is_some_and(|home|
                squared_distance(home, billet.cell) >= 8 * 8))
        || (brain.census.base_cell.is_some_and(|home|
            squared_distance(home, billet.cell) >= 18 * 18)
            && (enemy_cells.iter().any(|enemy|
                squared_distance(*enemy, billet.cell) <= 20 * 20)
                || ore_sites.contains_key(&billet.building_id)
                || occupier_cells.iter().any(|cell|
                    squared_distance(*cell, billet.cell) <= 7 * 7)))
    }).map(|billet| billet.building_id).collect();
    let mut front_ids=front_ids;
    front_ids.extend(billets.iter().filter(|b|within(asset_cells.iter().copied(),b.cell,8))
        .map(|b|b.building_id));
    let restored_cover:BTreeSet<_>=billets.iter().filter(|b|
        brain.campaign_restored_cover.get(&b.building_id).is_some_and(|end|tick<*end))
        .map(|b|b.building_id).collect();
    front_ids.extend(restored_cover.iter().copied());
    let front_available = !front_ids.is_empty();
    // Garrison capacity belongs to *buildings*, not a global population cap.
    // Count active Capture missions against the empty seats before arrival.
    let mut pending_by_site:std::collections::BTreeMap<u64,BTreeSet<u64>>=
        std::collections::BTreeMap::new();
    for unit in brain.census.units.iter().filter(|u|u.traits.occupier
        && obs.inside_transport(u.entity_id).is_none()) {
        let id=unit.entity_id;
        // Union the durable reservation and actual native entry. Counting
        // max of two different sets can oversubscribe a building's seats.
        let job=brain.tracked.tasks.get(id).filter(|j|j.kind==JobKind::Garrison)
            .and_then(|j|j.target);
        let native=if unit.mission.known()==Some(MissionType::Capture) {
            obs.own_destination_object(id)
        } else {None};
        for site in [job,native].into_iter().flatten() {
            pending_by_site.entry(site).or_default().insert(id);
        }
    }
    let pending_actors:BTreeSet<u64>=pending_by_site.values().flatten().copied().collect();
    garrisoned+=pending_actors.len();
    let mut room_left: std::collections::BTreeMap<u64, usize> = billets.iter()
        .map(|billet| {
            let pending=pending_by_site.get(&billet.building_id).map_or(0,BTreeSet::len);
            (billet.building_id, (billet.room as usize).saturating_sub(pending))
        }).collect();
    let front_room = billets.iter()
        .filter(|billet| front_ids.contains(&billet.building_id))
        .map(|billet| billet.room as usize).sum();
    let spare_occupiers = occupier_cells.len();
    let solvent=obs.credits()>=4500 || (obs.credits()>=2000
        && brain.income.per_minute().is_some_and(|rate|rate>=900));
    let mobile_floor=if obs.is_campaign() && solvent {1}
        else if solvent {2}else{3};
    let limit = garrison_budget(front_available, front_room)
        + spare_occupiers.saturating_sub(8).min(24)
        + if solvent {billets.len().min(8)}else{0};
    let mut issued = 0;
    let mut available_mobiles = brain.census.units.iter()
        .filter(|unit| brain.census.available(unit)
            && brain.tracked.tasks.get(unit.entity_id).is_none()
            && !pending_actors.contains(&unit.entity_id)
            && !ordered_this_frame(out,unit.entity_id)
            && obs.inside_transport(unit.entity_id).is_none()).count();
    // Count existing home occupants and already accepted home Capture tasks.
    // A global population limit alone is insufficient: the first three home
    // buildings used to consume every available rifleman before a mineral
    // choke entered vision, and a later front-room bonus could not un-garrison
    // those soldiers.
    let home = brain.census.base_cell;
    let is_home = |cell: (u16, u16)| home.is_some_and(|base|
        squared_distance(cell, base) <= HOME_REACH_CELLS * HOME_REACH_CELLS);
    let mut home_placed = brain.census.units.iter().filter(|unit|
        unit.traits.occupier && obs.inside_transport(unit.entity_id)
            .and_then(|id| obs.object_cell_and_type(id))
            .is_some_and(|(cell, _)| is_home(cell))
    ).count();
    home_placed += billets.iter().filter(|billet| is_home(billet.cell))
        .map(|billet| pending_by_site.get(&billet.building_id).map_or(0,BTreeSet::len))
        .sum::<usize>();
    // Choose *which soldiers get a turn* by strategic objective, not entity
    // storage order. The nearest home soldier no longer takes the frame's
    // order budget before the soldier beside the contested ore field.
    let mut personnel: Vec<_> = brain.census.units.iter().filter(|unit|
        unit.traits.occupier && unit.controllable()).collect();
    personnel.sort_unstable_by_key(|unit| billets.iter().filter_map(|billet| {
        let ore = ore_sites.get(&billet.building_id).copied().unwrap_or(0);
        let front = front_ids.contains(&billet.building_id);
        let reach = if ore >= ORE_GARRISON_MIN_CELLS { ORE_GARRISON_REACH_CELLS }
            else if front { FRONT_GARRISON_REACH_CELLS }
            else { HOME_REACH_CELLS };
        let distance = squared_distance(unit.cell, billet.cell);
        (distance <= reach * reach).then_some((
            garrison_priority(enemy_front_ids.contains(&billet.building_id),ore,front),
            distance, unit.entity_id))
    }).min().unwrap_or((4, i32::MAX, unit.entity_id)));
    for unit in personnel {
        if issued >= FORCE_ORDERS_PER_FRAME || garrisoned >= limit {
            break;
        }
        let near_front = billets.iter().any(|billet| {
            front_ids.contains(&billet.building_id)
                && squared_distance(unit.cell, billet.cell) <=
                    (if ore_sites.contains_key(&billet.building_id) {
                        ORE_GARRISON_REACH_CELLS * ORE_GARRISON_REACH_CELLS
                    } else { FRONT_GARRISON_REACH_CELLS * FRONT_GARRISON_REACH_CELLS })
        });
        // An infantryman walking past empty cover is not committed to a live
        // firefight; use the nearby building without stealing an actively firing unit.
        let cover_nearby = billets.iter().any(|billet| {
            squared_distance(unit.cell, billet.cell) <= 6 * 6
                && room_left.get(&billet.building_id).copied().unwrap_or(0) > 0
        });
        if !unit.traits.occupier
            || !brain.census.available(unit)
            || brain.cover_detail.values().any(|(id,_,_)|*id==unit.entity_id)
            || (obs.own_elite(unit.entity_id) && !near_front)
            || unit.health_permille<700
            || brain.tracked.tasks.get(unit.entity_id).is_some()
            || pending_actors.contains(&unit.entity_id)
            || brain.joint_ops.reserves(unit.entity_id)
            || brain.asset_guards.contains_key(&unit.entity_id)
            || brain.outpost_guards.contains_key(&unit.entity_id)
            || (holds_or_fights(unit.mission)
                && !may_garrison_from_mission(
                    unit.mission, near_front || cover_nearby,
                    super::micro::firing(obs, unit.mission, unit.entity_id),
                ))
            || ordered_this_frame(out, unit.entity_id)
        {
            continue;
        }
        let visible:Vec<_>=obs.visible_enemy_entities().collect();
        let approach=super::safety::envelopes(rules,obs,unit.entity_id,&visible,true);
        let Some(billet) = billets
            .iter()
            .filter(|billet| brain.specialist_risk_after.get(&(unit.entity_id,billet.building_id))
                .is_none_or(|until|tick>=*until))
            .filter(|billet| super::safety::safe_leg(&approach,unit.cell,billet.cell,false))
            .filter(|billet| squared_distance(unit.cell, billet.cell) <=
                (if ore_sites.contains_key(&billet.building_id)
                    || (solvent && within(asset_cells.iter().copied(),billet.cell,8)) {
                    ORE_GARRISON_REACH_CELLS * ORE_GARRISON_REACH_CELLS
                } else if front_ids.contains(&billet.building_id) {
                    FRONT_GARRISON_REACH_CELLS * FRONT_GARRISON_REACH_CELLS
                } else if spare_occupiers > 8 {
                    MAP_GARRISON_REACH_CELLS * MAP_GARRISON_REACH_CELLS
                } else { HOME_REACH_CELLS.pow(2) }))
            .filter(|billet| room_left.get(&billet.building_id).copied().unwrap_or(0) > 0)
            .filter(|billet| !brain.cover_detail.contains_key(&billet.building_id))
            .filter(|billet| !brain.director.under_attack_now()
                || (squared_distance(unit.cell,billet.cell)<=8*8
                    && (is_home(billet.cell) || front_ids.contains(&billet.building_id))))
            .filter(|billet| {
                let ore = ore_sites.get(&billet.building_id).copied().unwrap_or(0);
                let important = garrison_priority(
                    enemy_front_ids.contains(&billet.building_id), ore,
                    front_ids.contains(&billet.building_id)) < 3;
                important || !is_home(billet.cell)
                    || home_placed < HOME_GARRISON_OPENING_CAP
                        + spare_occupiers.saturating_sub(4).min(8)
                        + if solvent {4}else{0}
            })
            .filter(|billet| {
                // Preserve a minimal mobile base defense while sending the
                // paradropped/front-line infantry to available cover.
                // A distant outpost is optional; unlike a defended mineral
                // choke or an active front, it needs a larger mobile reserve.
                if restored_cover.contains(&billet.building_id) {
                    available_mobiles>if obs.is_campaign(){1}else{3}
                }
                else if front_ids.contains(&billet.building_id) { available_mobiles > mobile_floor }
                else if ore_sites.contains_key(&billet.building_id) { available_mobiles > mobile_floor }
                else if squared_distance(unit.cell, billet.cell) > HOME_REACH_CELLS.pow(2) {
                    available_mobiles > if solvent {mobile_floor+2}else{8}
                } else { available_mobiles > mobile_floor }
            })
            .filter(|billet| obs.can_garrison(rules, unit.entity_id, billet.building_id)
                && obs.entity_type(rules, unit.entity_id).is_some_and(|ty|
                    building_approachable(obs, ty.movement_zone, unit.cell, billet.cell)))
            .min_by_key(|billet| {
                // Rich central cover first, then other defended approach
                // lanes, then ordinary home houses. The final distance is a
                // tie-break, not the whole strategy for a mineral chokepoint.
                let ore = ore_sites.get(&billet.building_id).copied().unwrap_or(0);
                let priority = garrison_priority(
                    enemy_front_ids.contains(&billet.building_id), ore,
                    front_ids.contains(&billet.building_id));
                let assigned=obs.passengers(billet.building_id) as usize
                    + pending_by_site.get(&billet.building_id).map_or(0,BTreeSet::len)
                    + (billet.room as usize).saturating_sub(
                        pending_by_site.get(&billet.building_id).map_or(0,BTreeSet::len)
                            + room_left.get(&billet.building_id).copied().unwrap_or(0));
                (priority, assigned.min(2), usize::MAX - ore,
                    squared_distance(unit.cell, billet.cell), billet.building_id)
            })
        else {
            continue;
        };
        // The garrison is the one that had no memory at all: it was told to
        // walk into the same building every frame, and a unit being told to
        // enter what it is already entering is a unit that never arrives.
        if issue_tracked(
            &mut brain.tracked, brain.trace, out, rules, obs,
            unit.entity_id, TrackedKind::Capture,
            Some(billet.building_id), Some(billet.cell),
            Goal::Capture { target: billet.building_id },
        ) {
            brain.tracked.tasks.claim(tick, unit.entity_id,
                JobKind::Garrison, Some(billet.building_id), Some(billet.cell), unit.cell);
            if super::trace_enabled("AUTOPLAY_V56_TRACE") {
                log::info!("auto play: v56_garrison_proposal tick={tick} actor={} target={} asset_cover={}",
                    unit.entity_id,billet.building_id,within(asset_cells.iter().copied(),billet.cell,8));
            }
            issued += 1;
            garrisoned += 1;
            available_mobiles = available_mobiles.saturating_sub(1);
            if is_home(billet.cell) {
                home_placed += 1;
            }
            if let Some(room) = room_left.get_mut(&billet.building_id) {
                *room = room.saturating_sub(1);
            }
        }
    }
}

/// A paradropped occupier may still have its initial Attack mission while
/// running toward a front line. Borrow it for a nearby building only before
/// it is actually firing; never pull it out of an active exchange.
fn may_garrison_from_mission(mission: MissionId, near_front: bool, firing: bool) -> bool {
    near_front && (mission == MissionId::from_known(MissionType::Move)
        || (mission == MissionId::from_known(MissionType::Attack) && !firing))
}

/// Keep a small home contingent while front-line infantry fill the actual
/// remaining seats, subject to the shared order budget.
fn garrison_budget(front_available: bool, front_room: usize) -> usize {
    // The home slots are a *base* reserve, not a global population cap.
    // A city block near the enemy can be filled to its own real capacity.
    GARRISON_MAX + if front_available { front_room.min(32) } else { 0 }
}

/// What a spy gets out of a building, in retail's own categories.
///
/// The order follows `AIMD.INI`'s own spy teams — `General Spy vs Soviet
/// Power` / `vs Allied Power` before `vs Allied War` / `vs Soviet War`, and
/// `vs Sov Tech` after them (`ini/aimd.ini:1058-1158`) — and the categories are
/// the rules' `BuildCat=` lists, so a mod that moves a building keeps its
/// weight. A power plant blacks the enemy out, a refinery hands over its ore
/// and a radar its map; a barracks or a war factory makes its army veteran; a
/// lab is worth the walk and little more.
/// Match the *native* cash arm in `sim::spy_infiltration`: Radar, positive
/// Power, BuildTech and SuperWeapon take precedence over Storage. A refinery
/// icon alone does not prove that spying it will steal anything.
fn cash_spy_target(rules: &RuleSet, ty: &ObjectType) -> bool {
    ty.storage > 0 && !ty.radar && ty.power <= 0
        && !rules.is_build_tech(&ty.id)
        && ty.super_weapon.is_none() && ty.super_weapon2.is_none()
}

fn infiltration_value(rules: &RuleSet, ty: &ObjectType, free: i32) -> u8 {
    // Native `Simulation::infiltrated_by` checks Radar, positive Power,
    // BuildTech, super, Storage and Factory in exactly this order. A factory
    // gives veteran units, while a lab grants actually stolen technology.
    if ty.radar { 3 }
    else if ty.power > 0 { 6 }
    else if rules.is_build_tech(&ty.id) { 10 }
    else if ty.super_weapon.is_some() || ty.super_weapon2.is_some() { 5 }
    else if cash_spy_target(rules, ty) { if free < 3_000 { 9 } else { 8 } }
    else if matches!(ty.factory_type(),
        Some(crate::rules::object_type::FactoryType::InfantryType
            | crate::rules::object_type::FactoryType::UnitType)) { 8 }
    else { 1 }
}

/// Semantic infiltration *effect*, not a structure name. Permanent academy /
/// laboratory unlocks use their actual house-effect class; repeatable effects
/// have distinct tags to let several spies diversify their assignments.
fn spy_purpose(rules: &RuleSet, ty: &ObjectType) -> u8 {
    if let Some(class) = spy_class_code(rules, ty) { return class; }
    if ty.radar { 20 }
    else if ty.power > 0 { 21 }
    else if ty.super_weapon.is_some() || ty.super_weapon2.is_some() { 22 }
    else if cash_spy_target(rules, ty) { 23 }
    else { 24 }
}

/// Give a real, defenseless breach a purpose: steal useful tech/production
/// experience while engineers take buildings. On a dangerous, powered defense
/// line blackout remains the decisive supporting action. An *attempt* only
/// adds a short diversity cost, never marks a benefit as acquired.
fn spy_context_value(
    base: u8, purpose: u8, recent: Option<u64>, tick: u64,
    open_breach: bool, needed_blackout: bool,
) -> i32 {
    let mut score = i32::from(base);
    if open_breach && (1..=5).contains(&purpose) {
        score += 4;
    }
    if needed_blackout && purpose == 21 {
        score += 9;
    }
    if recent.is_some_and(|when| tick.saturating_sub(when) < 1_800)
        && !needed_blackout
    {
        score -= 5;
    }
    score
}

fn spy_open_breach(
    brain: &AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
    at: (u16, u16),
) -> bool {
    // This is not a claim that fog-hidden defenders have vanished: the cell
    // must be visible and an actual nearby escort must hold the approach.
    obs.is_cell_visible(at.0, at.1)
        && brain.census.units.iter().filter(|unit| {
            unit.traits.combat && unit.controllable()
                && squared_distance(unit.cell, at) <= 14 * 14
        }).count() >= 3
        && !obs.visible_enemy_entities().any(|enemy| {
            squared_distance(enemy.cell, at) <= 12 * 12
                && obs.entity_type(rules, enemy.entity_id).is_some_and(|ty| {
                    if ty.category == ObjectCategory::Building {
                        ty.is_base_defense && obs.visible_enemy_building_operational(
                            rules, enemy.entity_id)
                    } else {
                        (0..ty.weapon_record_count()).any(|slot| {
                            ty.weapon_at(slot).and_then(|name| rules.weapon(name))
                                .is_some_and(|weapon| !weapon.never_use
                                    && weapon.damage > 0)
                        })
                    }
                })
        })
}

fn spy_target_score(
    brain: &AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
    target: &EnemyEntity, base: u8, tick: u64,
) -> i32 {
    let Some(ty) = obs.object_type(rules, target.type_id) else {
        return i32::from(base);
    };
    let purpose = spy_purpose(rules, ty);
    // The power plant is valuable during an actual battery siege, not just
    // because there was a stalled push long ago. A vanished or unpowered
    // defensive grid is no reason to repeat a blackout instead of stealing
    // factory experience or a lab unlock.
    let needed_blackout = purpose == 21 && brain.siege_wait.is_some()
        && obs.visible_enemy_house(target.entity_id) == brain.campaign_house
        && brain.campaign_house.is_some()
        && obs.visible_enemy_entities().any(|enemy| {
            obs.visible_enemy_same_owner(enemy.entity_id, target.entity_id)
                && obs.entity_type(rules, enemy.entity_id)
                    .is_some_and(|ty| ty.is_base_defense)
                && obs.visible_enemy_building_operational(rules, enemy.entity_id)
        });
    let open_breach = (1..=5).contains(&purpose)
        && spy_open_breach(brain, rules, obs, target.cell);
    spy_context_value(base, purpose,
        brain.spy_kind_recent.get(&purpose).copied(), tick,
        open_breach, needed_blackout)
}

/// The cells of the house's own buildings: the yard, the plants, the factories,
/// the defenses. A bridge or a garrison is only worth a unit near one of them.
fn home_cells(brain: &AssistBrain) -> impl Iterator<Item = (u16, u16)> + '_ {
    brain.census.home_cells.iter().copied()
}

/// Whether a unit's type is one the Capture click reaches: an engineer's by
/// `Engineer=` and a spy's by `Infiltrate=`.
fn walks_into_buildings_flags(traits: &super::census::Traits) -> bool {
    traits.engineer || traits.infiltrator
}

/// Whether any of `cells` is within `reach` cells (Chebyshev-independent, all
/// orders in this file use squared Euclidean distance) of `cell`.
fn within(cells: impl Iterator<Item = (u16, u16)>, cell: (u16, u16), reach: i32) -> bool {
    cells
        .into_iter()
        .any(|other| squared_distance(cell, other) <= reach * reach)
}

/// How many frames an engineer's walk goes before the ground under it is read
/// again, in frames: one second.
///
/// The `Capture` intent's own term is the whole walk (300 frames), which is the
/// right window for "do not issue the same order again" and the wrong one for
/// "do not look again". A tank that arrived halfway through a five-second walk
/// had nothing to interrupt it. One second is short enough to turn around and
/// long enough not to re-plan every frame.
const CAPTURE_RECHECK_FRAMES: u64 = 30;

/// How near an enemy unit has to stand to a capture target for the walk to look
/// like a gift rather than a job, in cells: the range of the weapons that shoot
/// infantry, near enough.
const CAPTURE_CLEAR_CELLS: i32 = 6;

/// How many bays the house has to hold before the oil derrick stops being the
/// capture that pays for the army.
///
/// A derrick is a refinery the house did not build. Two bays is where the
/// harvesters stop queueing at the one there is, and from there the enemy's
/// tech is worth more than another trickle.
const DERRICK_WORTH_REFINERIES: usize = 2;

/// How near an enemy unit has to stand to the engineer's walk for the walk to
/// look like a gift rather than a job, in cells: the range of the weapons that
/// shoot infantry, near enough.
const PATH_CLEAR_CELLS: i32 = 3;

/// How near an enemy building has to stand to the engineer's walk for the walk
/// to cross the base that owns it, in cells: one wall's thickness plus the gun
/// behind it — a building is not just itself, it is the ground its neighbours
/// cover.
const BASE_SKIRT_CELLS: i32 = 4;

/// Whether one of `cells` stands on the line from `from` to `to`, within
/// `radius` cells of it.
///
/// The target's own clearance is not the question the walk asks: an engineer
/// killed half-way there never arrives, and the building it was going to take
/// was never guarded at all.
pub(super) fn path_crosses(
    cells: &[(u16, u16)],
    from: (u16, u16),
    to: (u16, u16),
    radius: i32,
) -> bool {
    let clear = i64::from(radius * radius);
    let (x0, y0) = (i64::from(from.0), i64::from(from.1));
    let (x1, y1) = (i64::from(to.0), i64::from(to.1));
    let (dx, dy) = (x1 - x0, y1 - y0);
    let length = dx * dx + dy * dy;
    cells.iter().any(|(gx, gy)| {
        // The destination is where the walk ends, not something it crosses: the
        // building being taken is not cover it has to get past. A guard posted
        // *on* it is the target-clearance question, asked separately.
        if (*gx, *gy) == to {
            return false;
        }
        let (px, py) = (i64::from(*gx), i64::from(*gy));
        // The guard projected onto the walk, clamped to its two ends: a guard
        // behind the engineer or past the building is not on the way.
        let t = if length == 0 {
            0
        } else {
            (((px - x0) * dx + (py - y0) * dy) * 1024 / length).clamp(0, 1024)
        };
        let (cx, cy) = (x0 + dx * t / 1024, y0 + dy * t / 1024);
        let (ox, oy) = (px - cx, py - cy);
        ox * ox + oy * oy <= clear
    })
}

/// Which building an engineer should walk into.
///
/// In the order a player weighs it: the map's own building before the enemy's
/// (an oil derrick is income with no defence and no owner to answer), a
/// walk no gun covers before one that crosses fire, then what the building is
/// worth, and the nearer before the further. Ties by id, so the answer does
/// not depend on the order the entity store yields.
fn engineer_preference(
    rules: &RuleSet,
    obs: &Observation<'_>,
    targets: &[EnemyEntity],
    hostile: &BTreeSet<u64>,
    guards: &[(u16, u16)],
    structures: &[(u16, u16)],
    from: (u16, u16),
    short_of_income: bool,
) -> Option<EnemyEntity> {
    let clear = CAPTURE_CLEAR_CELLS * CAPTURE_CLEAR_CELLS;
    targets
        .iter()
        .filter_map(|target| {
            let ty = obs.object_type(rules, target.type_id)?;
            let owned = u8::from(hostile.contains(&target.entity_id));
            // A target inside a guard's reach, or one whose walk crosses a
            // guard's, is not a job but a gift: the engineer is worth more than
            // the walk, and a capture that never happens still costs the unit.
            // It is ranked, not refused — with nothing clear on offer, the walk
            // with the least fire on it is still the walk to take.
            let guarded = u8::from(
                guards
                    .iter()
                    .any(|cell| squared_distance(*cell, target.cell) <= clear)
                    || path_crosses(guards, from, target.cell, PATH_CLEAR_CELLS)
                    // A neutral derrick behind the enemy's yard is not closer
                    // than the same derrick in front of it: walking through a
                    // base is walking through its guns, and the base's guns are
                    // its buildings as much as the units standing among them.
                    || path_crosses(structures, from, target.cell, BASE_SKIRT_CELLS),
            );
            // Civilian economic objectives are not a suicide mission. If a
            // visible guard covers the route or prize, wait until the army or
            // escort clears it. Ordinary hostile captures retain their
            // existing risk ranking (and its measured tactical trade-offs).
            if guarded != 0 && owned != 0 {
                // A hostile lab/yard is valuable, but not valuable enough to
                // throw an unarmed engineer through a defended base. Wait for
                // the main army or escort to clear the route first.
                return None;
            }
            // **Clear ground first, then what it is worth, then the walk.**
            //
            // The risk is the first key because an engineer that walks into a
            // guard is both a wasted 500 credits and a capture that never
            // happens: a guarded target is only taken when nothing else is on
            // offer. That is also what makes the opening right — early on the
            // enemy's own buildings sit inside a base with its tanks in it,
            // and the derrick beside the base does not, so the engineer takes
            // the derrick and the economy it is, which is the capture that
            // pays for the next three.
            //
            // Then the worth: a captured Battle Lab or Construction Yard is
            // the enemy's tech — `HouseClass::Can_Build @ 0x004F7870` reads
            // the buildings the house holds, so a lab that is ours opens its
            // tree to us and a yard hands over the side. Nearness is last:
            // the nearest `Capturable=` building used to win every time, and
            // the nearest one is whatever the sweep of the map saw first.
            Some((
                (
                    // Clear ground first: an engineer that walks into a guard
                    // is a 500-credit gift and a wasted cycle, and no capture
                    // is worth that. The worth decides between two targets
                    // equally safe, and the walk between two equally good.
                    if owned==0 {0} else {guarded},
                    std::cmp::Reverse(capture_value(rules, ty, short_of_income)),
                    // An enemy-occupied oil well, hospital or civilian tech
                    // asset is still an economic/strategic map objective.
                    // Ownership must not turn it into a low-value target.
                    u8::from(!strategic_public_asset(ty)),
                    owned,
                    squared_distance(from, target.cell),
                    target.entity_id,
                ),
                *target,
            ))
        })
        .min_by_key(|(key, _)| *key)
        .map(|(_, target)| target)
}

/// Public infrastructure remains strategically useful after another house
/// captures it. Use rules data rather than remembered neutral ownership: an
/// oil derrick still has ProduceCashAmount after the enemy takes it.
pub(super) fn strategic_public_asset(ty: &ObjectType) -> bool {
    ty.capturable && (ty.produce_cash_amount > 0 || ty.hospital
        || ty.unit_repair || ty.refinery || ty.radar
        || ty.super_weapon.is_some() || ty.super_weapon2.is_some())
}

/// Preserve a visible strategic public building from our own army's fire
/// when an engineer can actually take it in the near future. The native
/// action can be Damage for a healthy hostile building; it is still a legal
/// engineering operation and must not turn into a tank-demolition order.
///
/// Do not protect indefinitely a distant asset with no available engineer:
/// the ordinary push must remain able to finish an enemy base.
/// An engineer with a live primary Capture owns this building even when a
/// momentary dodge has replaced its latest tactical intent. This is the same
/// claim that the production/capture layers use, now visible to *all* guns.
/// A claimant must still exist and be legally able to capture the target;
/// an expired, killed or manually controlled engineer cannot freeze a match.
pub(super) fn engineer_claimed(
    rules: &RuleSet, obs: &Observation<'_>, census: &super::census::Census,
    tracked: &super::tracked::Tracker, target: EnemyEntity,
) -> bool {
    if !obs.is_cell_visible(target.cell.0, target.cell.1)
        || obs.object_type(rules, target.type_id).is_none_or(|ty| !ty.capturable)
    { return false; }
    census.units.iter().any(|unit| {
        unit.traits.engineer && unit.controllable()
            && ((unit.mission.known()==Some(MissionType::Capture)
                && obs.own_destination_object(unit.entity_id)==Some(target.entity_id))
                || (tracked.tasks.get(unit.entity_id).is_some_and(|job|
                    job.kind==JobKind::Engineer && job.target==Some(target.entity_id))
                    && obs.can_take_building(rules, unit.entity_id, target.entity_id)
                    && super::safety::specialist_approach_safe(rules,obs,unit.entity_id,target.entity_id)))
    })
}

pub(super) fn reclamation_reserved(
    rules: &RuleSet, obs: &Observation<'_>,
    census: &super::census::Census, tracked: &super::tracked::Tracker,
    target: EnemyEntity,
) -> bool {
    // Protect *every* rule-capturable enemy building with an active engineer
    // mission: enemy-owned civilian tech, factories and captured laboratories
    // included. The previous public-only filter allowed friendly tanks to
    // destroy a lab while its engineer was already inside the last approach.
    if engineer_claimed(rules, obs, census, tracked, target) { return true; }
    let Some(ty) = obs.object_type(rules, target.type_id) else { return false; };
    if !ty.capturable || !obs.is_cell_visible(target.cell.0, target.cell.1) {
        return false;
    }
    // A defenceless foreign yard, barracks, factory or battle lab is a new
    // production tree, not disposable rubble. Protect strategic capturable
    // buildings BEFORE an engineer happens to acquire a Capture intent.
    // This also protects harmless enemy-owned former neutral tech structures.
    let value = built_capture_value(rules, ty);
    let worth_capturing = strategic_public_asset(ty) || value >= 3
        || obs.easy_capture_candidate(rules,target.entity_id)
        || obs.is_campaign() && ty.capturable;
    if !worth_capturing || !tracked.capture_purchase_live(obs.tick(),target.entity_id) {
        return false;
    }
    // A hypothetical future capture is not an indefinite ceasefire. The old
    // fallback protected this prize whenever *any* other enemy existed, even
    // if nobody could buy/reach an engineer. Preserve a real active claim
    // above, or a nearby free engineer with a currently safe legal approach.
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    if !super::site_risk::capture_site_safe(census,rules,obs,target.entity_id,&visible) {
        return false;
    }
    if tracked.capture_purchase_live(obs.tick(),target.entity_id) {
        return super::build::all_class_types(rules,obs,ObjectCategory::Infantry)
            .any(|(engineer,_)|engineer.engineer
                && obs.can_build(rules,engineer,false)!=CanBuild::No
                && obs.has_producer(rules,engineer)
                && engineer_recruitment_route(rules,obs,engineer,&target,&visible));
    }
    // A free engineer is not yet a reservation. Orders::captures executes
    // before the normal attack planner, so a legal live assignment will
    // already have claimed its actual target. No indefinite global ceasefire.
    false

}

/// What a building is worth to the house that captures it, in the same
/// categories the rules write down. [`infiltration_value`] is the spy's answer
/// to the same question through `Infiltrate=`; this is the engineer's through
/// `Engineer=`, and the two differ because the prizes differ: a spy walks out
/// with an effect, an engineer walks out owning the building.
///
/// The ranking is what `HouseClass::Can_Build` reads, not a list of names: a
/// captured building is a building the house holds, so a lab unlocks what a lab
/// unlocks and a yard unlocks a side. The map's own buildings unlock nothing
/// but they pay. Abandoned capturable yards and factories outrank another
/// derrick when engineers can reach them safely, because they unlock tech.
fn capture_value(rules: &RuleSet, ty: &ObjectType, short_of_income: bool) -> u8 {
    // A captured yard or production building is an entire additional tech
    // branch, especially when it belongs to another side. A rich player
    // should not send every available engineer to yet another oil derrick
    // while an abandoned, undefended Construction Yard stands beside it.
    if ty.build_const_eligible() { return 10; }
    if ty.factory_type().is_some() { return 9; }
    if rules.build_tech_types.iter().any(|name| *name == ty.id) { return 8; }
    if strategic_public_asset(ty) { return if short_of_income { 8 } else { 7 }; }
    let built = built_capture_value(rules, ty);
    if built > 0 {
        return built;
    }
    if short_of_income {
        // A building in none of the lists, so one this house could never have
        // built itself — the oil derrick above all — for as long as the house
        // still has fewer bays than the derrick stands in for.
        //
        // while the house still has fewer bays than the derrick stands in for.
        //
        // A derrick is a refinery the house did not build: `[GAOREP]
        // Prerequisite=GATECH,PROC,GACNST` in the retail data, so the credits
        // it pays are credits that did not have to pass the tech tree, and
        // the opening is exactly when the house cannot afford to pass it. So
        // for those few minutes it outranks what the enemy built, including
        // the bays and the plants of the base holding it: two credits a second
        // at home is what pays for the army that takes the lab afterwards.
        // Once the bays stand, the enemy's tech is the prize again. The six
        // is the point: while the bays are missing the derrick is placed
        // above the tech, not beside it, because it is the credits that pay
        // for the army that walks into the lab afterwards.
        6
    } else {
        // Income the house never built a bay for, and — because nobody
        // defends it — the capture an engineer is most likely to walk home
        // from.
        3
    }
}

/// What a building this house could have built itself is worth: zero for one
/// in none of the build lists, which is what makes it the map's own.
///
/// The ranking is what `HouseClass::Can_Build @ 0x004F7870` reads, not a list
/// of names: a captured building is a building the house holds, so a lab
/// unlocks what a lab unlocks and a yard hands over a side. Zero is the
/// derrick's answer, and the derrick is an income the tree never offered.
fn built_capture_value(rules: &RuleSet, ty: &ObjectType) -> u8 {
    if ty.build_const_eligible()
        || rules.build_tech_types.iter().any(|name| *name == ty.id)
        || ty.super_weapon.is_some()
        || ty.super_weapon2.is_some()
    {
        5
    } else if rules.build_weapons_types.iter().any(|name| *name == ty.id) || ty.resource_destination
    {
        4
    } else if rules.build_barracks_types.iter().any(|name| *name == ty.id) || ty.radar {
        3
    } else if rules.build_power_types.iter().any(|name| *name == ty.id) {
        2
    } else {
        0
    }
}

/// Put every idle combat unit no team has taken on area guard where it stands.
pub(super) fn forces(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    // Forget the units the house no longer has, so the memory stays bounded by
    // the army and a recycled id cannot inherit an old order.
    let ids: BTreeSet<u64> = brain
        .census
        .units
        .iter()
        .map(|unit| unit.entity_id)
        .collect();
    brain.tracked.retain(&ids);
    if !brain.census.units.iter().any(|unit| unit.traits.combat) {
        return;
    }
    // The army's holding list: one definition of "who guards the base", from
    // the same object that answers "who attacks". It is wider than the fist,
    // and that width is the lesson from wiring it to the fist's members: the
    // guards went with the spawned units and the match stopped finishing.
    // When the base is under attack the guards converge on the attack instead of
    // holding the cell they were built beside.
    //
    // The user's report: "基地挨打的时候有很多士兵还是在发呆，因为基地会越来越大，
    // 所以发呆的越来越多". A base grows past the guard's reach, so a soldier on the
    // far side of it never sees the fight at the gate and `Guard` keeps him where
    // he stands for the whole match. `base_under_threat` is already the house's
    // answer for "the enemy is close enough that the army belongs at home", so
    // the same answer aims them.
    let threatened = super::advance::threatened_cell(&brain.census);
    let guards = super::army::Army::guards(&brain.census);
    let mut issued = 0;
    for unit in &guards {
        if issued >= FORCE_ORDERS_PER_FRAME {
            break;
        }
        // A unit already given this frame's order — a step out of a shell — is
        // not told to guard where it was standing.
        if ordered_this_frame(out, unit.entity_id)
            || unit.traits.suicide
            || super::doctrine::actor_duty(rules,obs,unit.entity_id)
                == Some(super::doctrine::Duty::BombPlanter)
            || brain.joint_ops.reserves(unit.entity_id)
            || brain.crate_claims.contains_key(&unit.entity_id)
            || brain.crate_returns.contains_key(&unit.entity_id)
        {
            continue;
        }
        // A unit that is already holding or fighting is doing exactly this, and
        // its mission owns it: a second Guard would cancel the fight.
        if holds_or_fights(unit.mission) {
            continue;
        }
        // The advance step owns a unit it is pushing: a guard here would pull
        // it back out of the push.
        if brain.tracked.stands(tick, unit.entity_id, PUSHING) {
            continue;
        }
        if !brain
            .tracked
            .admits(tick, unit.entity_id, TrackedKind::Guard, None, None)
        {
            continue;
        }
        brain
            .tracked
            .record(tick, unit.entity_id, TrackedKind::Guard, None, None);
        let goal = match threatened {
            Some(cell) => Goal::GuardAt { cell },
            None => Goal::Guard,
        };
        issue(brain, out, rules, obs, unit.entity_id, goal);
        issued += 1;
    }
}

/// How near a hostile has to be before a soldier takes its deployed position,
/// and how far it has to be before it gives the position up, in cells.
///
/// Deploying is a weapon change and a movement stop — `DeployFire=` swaps the
/// primary for the deployed one — so it is worth taking when the fight has
/// arrived, not when it is a rumour across the map.
///
/// The two numbers are deliberately different. One radius for both made the
/// soldier sit and stand at almost the same distance, and `6` sat right on top
/// of the reach the deployed gun covers, so a hostile pacing between five and
/// seven cells had it kneeling and rising forever — and a soldier mid-animation
/// is a soldier with its gun silent, which is why a dug-in position sometimes
/// shot and sometimes did not. The gap is the dead band: nothing inside
/// [`DEPLOY_RANGE_CELLS`] changes the pose until it is outside
/// [`UNDEPLOY_RANGE_CELLS`].
const DEPLOY_RANGE_CELLS: i32 = 6;
const UNDEPLOY_RANGE_CELLS: i32 = 10;

/// How many toggles one frame carries. The D key is a click like any other,
/// and a frame that spends its budget on soldiers standing up has none left
/// for the orders that move them.
const DEPLOYS_PER_FRAME: usize = 4;

/// How long a soldier is left alone after the D key is [`TrackedKind::term`]'s
/// answer now: the deploy animation, and the command's round trip.

/// The D key on the soldiers that fight better deployed: `DeployFire=yes`.
///
/// `[E1]` and `[GGI]` carry it with `Deployer=yes`, and what it buys is the
/// deployed weapon — a GI's sandbag posture, a Guardian GI's missiles — at the
/// price of not moving. A player presses D when the enemy is in front of the
/// soldier, which is what the range above is.
///
/// The command toggles (`None → Deploying`, `Deployed → Undeploying`, and a
/// no-op mid-transition), so it is only issued on the edge: a soldier that is
/// already down stays down, and one that is standing stays up. Issuing it
/// every frame would walk them in and out of the pose, and a deployed soldier
/// that never stands up is a soldier that never takes ground.
pub(super) fn deploy(brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox) {
    let tick = obs.tick();
    // Forget the soldiers the house no longer has, so a recycled id cannot
    // inherit another soldier's pose.
    let live: BTreeSet<u64> = brain
        .census
        .units
        .iter()
        .map(|unit| unit.entity_id)
        .collect();
    // What each lost unit was last told to do. A unit the house no longer has is
    // a death, and this is the only honest "what was it doing when it died" —
    // the wishlist's "送死", counted by the order it was carrying rather than
    // reconstructed from a per-type diff.
    let lost = brain.tracked.retain(&live);
    for (id, kind) in &lost {
        let what = kind.map_or("none".to_owned(), |kind| format!("{kind:?}"));
        log::info!("auto play: lost {id} while carrying {what}");
    }
    // Collected before anything is written: the toggle has to be recorded
    // against the soldier, and the ledger is borrowed while it is read.
    // An old sighting cannot justify activating a permanent area hazard.
    // Only truly visible enemy positions may trigger an infantry deploy.
    let visible: Vec<EnemyEntity> = obs.visible_enemy_entities().take(256).collect();
    let wanted: Vec<(u64, i32)> = brain
        .census
        .units
        .iter()
        .filter(|unit| unit.traits.deployer && !unit.hand && unit.orderable)
        .map(|unit| {
            (
                unit.entity_id,
                nearest_distance(&visible, unit.cell),
            )
        })
        .collect();
    let mut issued = 0;
    for (entity_id, distance) in wanted {
        if issued >= DEPLOYS_PER_FRAME {
            break;
        }
        let deployed = obs.is_deployed(entity_id);
        let radiation = obs.entity_type(rules,entity_id)
            .is_some_and(|ty| super::doctrine::radiation_deploy_radius(rules,ty) > 0);
        let unsafe_radiation = radiation
            && obs.object_cell_and_type(entity_id).is_some_and(|(cell,_)|
                !super::doctrine::radiation_deploy_safe(rules,obs,entity_id,cell));
        // Undeploy immediately when allies enter the affected area; refuse to
        // turn a safe infantry screen into an irradiated friendly formation.
        // Residual radiation can linger: prevent exposure before deployment.
        let down = if unsafe_radiation {
            if deployed { Some(false) } else { None }
        } else { pose_to_take(deployed, distance) };
        let Some(down) = down else { continue; };
        log::debug!(
            "auto play: deploy {} -> {} nearest={}",
            entity_id,
            if down { "dug in" } else { "standing" },
            distance
        );
        // The toggle's own term is the deploy animation, and `Is_Deployed`
        // only follows it — so until then the soldier still reads as standing
        // and would be toggled straight back up. L4 refuses the second press.
        if !brain
            .tracked
            .admits(tick, entity_id, TrackedKind::Deploy, None, None)
        {
            continue;
        }
        out.command(Command::ToggleInfantryDeploy { entity_id });
        brain
            .tracked
            .record(tick, entity_id, TrackedKind::Deploy, None, None);
        issued += 1;
    }
}

/// The pose the soldier should take, or `None` for "leave it as it is".
///
/// Down when the fight is here, up only once it is clearly gone, and nothing in
/// between changes anything: that gap is the dead band. One radius for both
/// made a hostile pacing between the two kneel and rise the soldier forever, and
/// a soldier mid-animation is a soldier whose gun is silent.
fn pose_to_take(deployed: bool, distance: i32) -> Option<bool> {
    let down = distance <= DEPLOY_RANGE_CELLS * DEPLOY_RANGE_CELLS;
    let up = distance > UNDEPLOY_RANGE_CELLS * UNDEPLOY_RANGE_CELLS;
    match (deployed, down, up) {
        (false, true, _) => Some(true),
        (true, _, true) => Some(false),
        _ => None,
    }
}

/// How far the nearest hostile is from `cell`, as a squared cell distance so
/// the square root is not needed; `i32::MAX` when there is none.
fn nearest_distance(enemies: &[EnemyEntity], cell: (u16, u16)) -> i32 {
    enemies
        .iter()
        .map(|enemy| squared_distance(enemy.cell, cell))
        .min()
        .unwrap_or(i32::MAX)
}

/// Whether the object is already walking or fighting, so re-tasking it would
/// cancel something.
///
/// `Move` is here because a unit the house is still walking somewhere is not
/// idle, and replacing that move with a guard would stop it half-way. `Attack`
/// is here because a second order would cancel the fight.
///
/// `AreaGuard` is deliberately **not** here, and used to be. Every unit a
/// scenario spawns for the player starts on area guard, and treating that as
/// "busy" left the whole opening force standing in the yard for the whole
/// match: the brain never gave it a job because the mission it was born with
/// said it had one. A guard is the posture to *replace*, not a reason to leave
/// a unit alone — the intent table ([`super::tracked`]) is what stops it being
/// re-issued every frame.
fn holds_or_fights(mission: MissionId) -> bool {
    [MissionType::Attack, MissionType::Move]
        .into_iter()
        .any(|kind| mission == MissionId::from_known(kind))
}

/// A harvester is part of the economy, not a disposable scout. Only a real,
/// *currently visible* enemy able to damage the miner permits interrupting
/// the native Harvest/Return/Unload cycle. This reads the same fog-filtered
/// enemy census as every other tactical decision.
const MINER_DANGER_CELLS: i32 = 8;
const MINER_FIELD_SAFETY_CELLS: i32 = 10;
const MINER_ESCAPE_FRAMES: u64 = 210;
const MINER_ESCAPE_PER_FRAME: usize = 2;

fn miner_in_danger(
    rules: &RuleSet, obs: &Observation<'_>,
    enemy_id: u64, enemy_cell: (u16,u16), miner_id: u64,
    miner_cell: (u16,u16),
) -> bool {
    obs.is_cell_visible(enemy_cell.0, enemy_cell.1)
        && squared_distance(enemy_cell, miner_cell) <= MINER_DANGER_CELLS * MINER_DANGER_CELLS
        && obs.can_hurt(rules, enemy_id, miner_id)
}

/// Find a safe *walkable* staging cell beside a refinery. The refinery's
/// footprint itself is an invalid Move destination; clicking it to evade an
/// enemy causes a miner to collide with its own bay forever.
fn safe_miner_post(
    obs: &Observation<'_>, zone: MovementZone,
    miner: (u16,u16), home: &[(u16,u16)],
    visible_threats: &[(u16,u16)],
) -> Option<(u16,u16)> {
    let mut posts = Vec::new();
    for &anchor in home {
        for ring in 3i32..=6 {
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs().max(dy.abs()) != ring { continue; }
                    let (Ok(x),Ok(y)) = (
                        u16::try_from(i32::from(anchor.0)+dx),
                        u16::try_from(i32::from(anchor.1)+dy),
                    ) else { continue; };
                    let cell = (x,y);
                    if obs.is_ground_walkable(x,y)
                        && obs.can_reach(zone, miner, cell)
                        && visible_threats.iter().all(|&enemy| {
                            squared_distance(enemy,cell) > MINER_DANGER_CELLS * MINER_DANGER_CELLS
                        })
                    {
                        posts.push(cell);
                    }
                }
            }
        }
    }
    posts.into_iter().min_by_key(|&cell| (squared_distance(cell,miner), cell))
}

/// Store sightings rather than following hidden enemy IDs. When the old
/// position is observed clear, that evidence immediately removes the site.
pub(super) fn observe_mining_territory(brain:&mut AssistBrain,rules:&RuleSet,obs:&Observation<'_>) {
    let now=obs.tick();
    if now%30!=0 {return;}
    let mut current=BTreeSet::new();
    for e in obs.visible_enemy_entities() {
        let Some(ty)=obs.object_type(rules,e.type_id) else {continue;};
        if ty.category!=ObjectCategory::Building {continue;}
        let infrastructure=ty.build_const_eligible() || ty.factory_type().is_some()
            || ty.resource_destination || ty.power>0;
        let armed=super::advance::weapon_range_cells(rules,ty,false);
        if !infrastructure && armed==0 {continue;}
        let radius=if infrastructure {14}else{armed.clamp(6,18)+2};
        current.insert(e.entity_id);
        brain.mining_hostile_sites.insert(e.entity_id,(e.cell,radius,now));
    }
    brain.mining_hostile_sites.retain(|id,(cell,_,seen)|
        current.contains(id) || (!obs.is_cell_visible(cell.0,cell.1)
            && now.saturating_sub(*seen)<9000));
}

pub(super) fn hostile_mining_ground(brain:&AssistBrain,cell:(u16,u16))->bool {
    brain.mining_hostile_sites.values().any(|(at,radius,_)|
        cell_distance(*at,cell)<=*radius)
        || brain.mine_incidents.values().any(|(at,_)|cell_distance(*at,cell)<=8)
}

fn mining_leg_safe(brain:&AssistBrain,obs:&Observation<'_>,zone:MovementZone,
    from:(u16,u16),to:(u16,u16),guns:&[super::safety::Envelope])->bool {
    obs.is_cell_in_playfield(to.0,to.1) && obs.can_reach(zone,from,to)
        && !hostile_mining_ground(brain,to)
        && super::safety::safe_leg(guns,from,to,true)
        && brain.mining_hostile_sites.values().all(|(at,radius,_)|
            if cell_distance(from,*at)<=*radius {
                cell_distance(to,*at)>cell_distance(from,*at)
            } else {!super::tactics::path_enters_control_box(from,to,*at,*radius)})
}

/// Park without giving the native Harvest scanner permission to seek an
/// alternate seam. Distinct posts stay off ore and refinery entrances.
fn miner_standby_post(brain:&AssistBrain,rules:&RuleSet,obs:&Observation<'_>,
    id:u64,from:(u16,u16),zone:MovementZone,guns:&[super::safety::Envelope])
    ->Option<(u16,u16)> {
    let valid=|p:(u16,u16)|obs.is_cell_in_playfield(p.0,p.1)
        && obs.is_ground_walkable(p.0,p.1)
        && !(obs.is_cell_visible(p.0,p.1) && super::ore::at(rules,obs,p))
        && !brain.ore_known.contains_key(&p)
        && mining_leg_safe(brain,obs,zone,from,p,guns);
    if let Some(p)=brain.miner_standby.get(&id).copied().filter(|p|valid(*p)) {return Some(p);}
    let mut best=None;
    for at in brain.census.refinery_cells.iter().copied().chain(brain.census.base_cell) {
        for r in [6i32,9,12] {
            for (dx,dy) in [(r,0),(-r,0),(0,r),(0,-r),(r,r),(-r,r),(r,-r),(-r,-r)] {
                let (Ok(x),Ok(y))=(u16::try_from(i32::from(at.0)+dx),
                    u16::try_from(i32::from(at.1)+dy)) else {continue;};
                let p=(x,y);
                if !valid(p) {continue;}
                let occupied=brain.census.units.iter().filter(|u|u.entity_id!=id
                    && cell_distance(u.cell,p)<=2).count();
                let claimed=brain.miner_standby.iter().filter(|(other,cell)|
                    **other!=id && cell_distance(**cell,p)<=3).count();
                let score=(occupied+claimed*3,cell_distance(from,p),p);
                if best.is_none_or(|old|score<old) {best=Some(score);}
            }
        }
    }
    best.map(|(_,_,p)|p)
}

fn stand_miner_down(brain:&mut AssistBrain,rules:&RuleSet,obs:&Observation<'_>,
    out:&mut Outbox,id:u64,cell:(u16,u16),zone:MovementZone,
    guns:&[super::safety::Envelope]) {
    let now=obs.tick();
    brain.miner_journeys.remove(&id);
    brain.miner_idle_since.remove(&id);
    brain.tracked.cancel_for_safety(id);
    let post=miner_standby_post(brain,rules,obs,id,cell,zone,guns);
    let goal=if let Some(post)=post {
        let first=brain.miner_standby.insert(id,post).is_none();
        if first {log::info!("auto play: v63_miner_standby tick={now} actor={id} post={post:?}");}
        if cell_distance(cell,post)>2 && obs.destination(id)==Some(post) {return;}
        if cell_distance(cell,post)>2 {Goal::Move{cell:post}} else {Goal::Stop}
    } else {
        if brain.miner_standby.insert(id,cell).is_none() {
            log::info!("auto play: v63_miner_standby tick={now} actor={id} no_safe_post=true");
        }
        Goal::Stop
    };
    if matches!(goal,Goal::Stop) && obs.destination(id).is_none()
        && obs.attack_target(id).is_none()
        && brain.census.units.iter().any(|u|u.entity_id==id
            && matches!(u.mission.known(),Some(MissionType::Guard|MissionType::Sleep)))
    {return;}
    if admits(rules,obs,id,&goal) {
        let _=out.want(id,0,goal);
    }
}

/// Interrupt only threatened miners, at most two per frame. The temporary
/// Avoid command uses the normal order gate; deliberately forget the old
/// Harvest primary so Mission Resume does not send it straight back to the
/// dangerous ore. Harvest is selected anew after the safety cooldown.
pub(super) fn harvester_safety(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    let now = obs.tick();
    brain.miner_avoid_until.retain(|_,until| *until > now);
    if brain.census.refinery_cells.is_empty() {
        return;
    }
    let visible: Vec<_> = brain.census.enemies.iter()
        .filter(|enemy| obs.is_cell_visible(enemy.cell.0, enemy.cell.1))
        .map(|enemy| (enemy.entity_id,enemy.cell))
        .collect();
    if visible.is_empty() { return; }
    let mut issued = 0;
    for unit in &brain.census.units {
        if issued >= MINER_ESCAPE_PER_FRAME { break; }
        let id = unit.entity_id;
        if !unit.traits.harvester || !unit.controllable() || unit.in_service()
            || obs.is_deployed(id) || brain.miner_avoid_until.contains_key(&id)
            || out.ordered_this_frame(id)
        {
            continue;
        }
        if !visible.iter().any(|(enemy_id,enemy_cell)| {
            miner_in_danger(rules, obs, *enemy_id, *enemy_cell, id, unit.cell)
        }) { continue; }
        let Some(ty) = obs.object_type(rules,unit.type_id) else { continue; };
        let enemy_cells: Vec<_> = visible.iter().map(|(_,cell)| *cell).collect();
        let seen:Vec<_>=obs.visible_enemy_entities().collect();
        let guns=super::safety::envelopes(rules,obs,id,&seen,true);
        let Some(post)=miner_standby_post(brain,rules,obs,id,unit.cell,ty.movement_zone,&guns)
            .or_else(||safe_miner_post(obs,ty.movement_zone,unit.cell,
                &brain.census.refinery_cells,&enemy_cells).filter(|post|
                    mining_leg_safe(brain,obs,ty.movement_zone,unit.cell,*post,&guns)))
        else {continue;};
        if issue_tracked(
            &mut brain.tracked, brain.trace, out, rules, obs,
            id, TrackedKind::Avoid, None, Some(post), Goal::Move { cell: post },
        ) {
            brain.tracked.forget_primary(id);
            brain.miner_avoid_until.insert(id, now.saturating_add(MINER_ESCAPE_FRAMES));
            issued += 1;
            log::debug!("auto play: harvester {id} flees visible threat toward {post:?}");
        }
    }
}

/// An Enslaves= mobile producer is a refinery that must be placed *beside*
/// ore, not a normal harvester. This is the rule-driven version of the Yuri
/// Slave Miner: the deployed type has ResourceDestination= and spawns the
/// actual worker infantry. The original UnitClass::Mission_Harvest (0x73E5E9)
/// only services its slave manager; it does not walk the chassis to ore.
fn mobile_slave_refinery<'r>(rules: &'r RuleSet, ty: &ObjectType) -> Option<&'r ObjectType> {
    if ty.category != ObjectCategory::Vehicle || !ty.resource_gatherer
        || ty.enslaves.is_none() { return None; }
    let building = rules.object(ty.deploys_into.as_deref()?)?;
    (building.resource_destination && building.enslaves.is_some()).then_some(building)
}

/// A depleted deployed slave refinery may move; do not pack a productive one
/// to chase a slightly richer patch. `distance` is to another known seam.
fn slave_relocation_worthwhile(local: u32, remote: u32, distance: i32, scan: i32) -> bool {
    local < 6 && remote >= 20 && remote > local.saturating_mul(3)
        && distance > scan
}

/// Find a truly deployable and reachable post near an *observed* ore cluster.
/// The slave scan is read from the active rules, allowing mods to change the
/// radius. A rich ore patch's centre may itself be covered by ore and unable
/// to accept the mine's building footprint; search its fringe instead.
fn slave_work_site(
    brain: &AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
    miner: u64, from: (u16,u16), ty: &ObjectType,
) -> Option<(u16,u16)> {
    let building = mobile_slave_refinery(rules, ty)?;
    let scan = (rules.general.slave_miner_slave_scan / 256).clamp(4, 20);
    let space = super::terrain::Space::read(
        obs, rules, &brain.census.home_cells, brain.census.base_cell);
    let patches = brain.ore_scan.as_ref()?.patches.as_slice();
    // Prefer substantial, reachable deposits, but do not send a miner across
    // the map for an exhausted two-tile vein. A candidate's eventual footprint
    // and the whole route are checked against native movement/admission.
    let mut prospects: Vec<_> = patches.iter().copied()
        .filter(|p| p.stock >= 6
            && super::advance::cell_distance(from, p.cell) <= super::ore::REMOTE_WORK_CELLS
            && space.ground_at(p.cell).buildable()
            && !hostile_mining_ground(brain,p.cell))
        .collect();
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let guns=super::safety::envelopes(rules,obs,miner,&visible,true);
    prospects.sort_by_key(|p| (super::advance::cell_distance(from,p.cell)/12,
        std::cmp::Reverse(p.stock),super::advance::cell_distance(from,p.cell),p.cell));
    for patch in prospects.into_iter().take(20) {
        // Do not stack several worker factories on a small seam that already
        // has a working Enslaves= refinery. Large fields may support more.
        let serviced = brain.census.units.iter().any(|unit| {
            obs.object_type(rules, unit.type_id).is_some_and(|t|
                t.category == ObjectCategory::Building && t.resource_destination
                    && t.enslaves.is_some()
                    && super::advance::cell_distance(unit.cell, patch.cell) <= scan)
        });
        if serviced && patch.stock < 80 { continue; }
        // Another mobile mine already has a committed post at this seam.
        // Do not send a second one into the same foundation and then blame
        // native deployment when the first one unfolds there.
        if brain.slave_sites.iter().any(|(id, (site, _, _))|
            *id != miner && super::advance::cell_distance(*site, patch.cell) <= scan)
        { continue; }
        for ring in 0i32..=scan {
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs().max(dy.abs()) != ring { continue; }
                    let (Ok(x),Ok(y)) = (
                        u16::try_from(i32::from(patch.cell.0) + dx),
                        u16::try_from(i32::from(patch.cell.1) + dy),
                    ) else { continue; };
                    let site = (x,y);
                    if brain.slave_rejected.contains_key(&site)
                        || !obs.is_cell_in_playfield(x,y)
                        || !space.ground_at(site).buildable()
                        || !obs.is_ground_walkable(x,y)
                        || brain.census.enemies.iter().any(|enemy|
                            obs.is_cell_visible(enemy.cell.0,enemy.cell.1)
                            && super::squared_distance(enemy.cell,site) <= 12*12)
                        || !obs.deploy_admits_at(rules, miner, building, site)
                        || !mining_leg_safe(brain,obs,ty.movement_zone,from,site,&guns)
                    { continue; }
                    return Some(site);
                }
            }
        }
    }
    None
}

/// Two-stage economic mission: Move to a legal slave-refinery plot, then
/// Deploy. Distinguish it from both MCV expansion and normal ore Harvest.
/// Keep the destination sticky while driving; abort a genuinely stalled or
/// newly blocked plot instead of selecting a different one every frame.
pub(super) fn slave_miners(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    if !obs.production_enabled() || !obs.forces_enabled() { return; }
    let now = obs.tick();
    brain.slave_rejected.retain(|_,until| *until > now);
    let mobiles: Vec<_> = brain.census.units.iter()
        .filter(|unit| unit.orderable && unit.controllable() && !unit.in_service())
        .filter(|unit| obs.object_type(rules, unit.type_id)
            .is_some_and(|ty| mobile_slave_refinery(rules, ty).is_some()))
        .map(|unit| (unit.entity_id, unit.cell, unit.type_id))
        .collect();
    brain.slave_sites.retain(|id,_| mobiles.iter().any(|unit| unit.0 == *id));
    for (id, cell, type_id) in mobiles.into_iter().take(3) {
        if out.ordered_this_frame(id)
            || brain.miner_avoid_until.get(&id).is_some_and(|until| *until > now)
        { continue; }
        let Some(ty) = obs.object_type(rules, type_id) else { continue; };
        let Some(building) = mobile_slave_refinery(rules, ty) else { continue; };
        if let Some((site, last, progress)) = brain.slave_sites.get(&id).copied() {
            let visible:Vec<_>=obs.visible_enemy_entities().collect();
            let guns=super::safety::envelopes(rules,obs,id,&visible,true);
            if !mining_leg_safe(brain,obs,ty.movement_zone,cell,site,&guns) {
                brain.slave_sites.remove(&id);
                brain.slave_rejected.insert(site,now.saturating_add(1200));
                stand_miner_down(brain,rules,obs,out,id,cell,ty.movement_zone,&guns);
                continue;
            }
            if cell == site && obs.deploy_site_admits(rules, id, building) {
                if issue(brain, out, rules, obs, id, Goal::Deploy) {
                    log::info!("auto play: slave refinery {id} deploys at {site:?}");
                }
                // Native transformation is the confirmation; it removes this
                // vehicle from the mobile list on the following frame.
                continue;
            }
            let progress = if cell != last { now } else { progress };
            if now.saturating_sub(progress) >= 300
                || !obs.can_reach(ty.movement_zone, cell, site)
                || (cell == site && !obs.deploy_site_admits(rules, id, building)
                    && now.saturating_sub(progress) >= 150)
            {
                brain.slave_sites.remove(&id);
                brain.slave_rejected.insert(site, now.saturating_add(1200));
                log::debug!("auto play: slave refinery {id} rejects stalled site {site:?}");
            } else {
                brain.slave_sites.insert(id, (site, cell, progress));
                if obs.destination(id) == Some(site) { continue; }
                if obs.destination(id).is_some() { continue; }
                // Reissue only when the native mission has actually stopped.
                issue(brain, out, rules, obs, id, Goal::Move { cell: site });
                continue;
            }
        }
        // Another task may have just moved the miner out of observed danger.
        if obs.destination(id).is_some() { continue; }
        if let Some(site) = slave_work_site(brain, rules, obs, id, cell, ty) {
            let goal = if site == cell { Goal::Deploy } else { Goal::Move { cell: site } };
            if issue(brain, out, rules, obs, id, goal) {
                brain.slave_sites.insert(id, (site, cell, now));
                log::info!("auto play: slave refinery {id} assigned safe mineral site {site:?}");
            }
        }
    }
    // A depleted *deployed* slave mine should be allowed to move to a new
    // known safe patch. The native can_repack gate is final authority; never
    // pack another type of refinery, a still productive mine, or a structure
    // held by the player's control groups.
    if now < brain.slave_pack_after { return; }
    let scan = (rules.general.slave_miner_slave_scan / 256).clamp(4, 20);
    for unit in &brain.census.units {
        if !unit.orderable || !unit.controllable() { continue; }
        let Some(ty) = obs.object_type(rules, unit.type_id) else { continue; };
        if ty.category != ObjectCategory::Building || !ty.resource_destination
            || ty.enslaves.is_none()
            || !ty.undeploys_into.as_deref()
                .and_then(|name| rules.object(name))
                .is_some_and(|mobile| mobile_slave_refinery(rules, mobile).is_some())
            || !obs.can_repack_yard(rules, unit.entity_id)
        { continue; }
        let local_stock = brain.ore_known.iter()
            .filter(|(cell,_)| super::advance::cell_distance(**cell,unit.cell) <= scan)
            .map(|(_,stage)| u32::from(*stage)+1).sum::<u32>();
        let useful_remote = brain.ore_scan.as_ref().is_some_and(|survey|
            survey.patches.iter().any(|p|
                slave_relocation_worthwhile(local_stock,p.stock,
                    super::advance::cell_distance(unit.cell,p.cell),scan)
                && !hostile_mining_ground(brain,p.cell)
                && !brain.census.enemies.iter().any(|enemy|
                    obs.is_cell_visible(enemy.cell.0,enemy.cell.1)
                    && super::squared_distance(enemy.cell,p.cell) <= 12*12)));
        if !useful_remote { continue; }
        out.intent(Intent::UndeployBuilding { entity_id: unit.entity_id });
        brain.slave_pack_after = now.saturating_add(1200);
        log::info!("auto play: depleted slave refinery {} at {:?} relocates toward known ore",
            unit.entity_id,unit.cell);
        break;
    }
}

/// Put an idle harvester back on the ore.
///
/// The miner's state machine (`SearchOre → Harvest → ReturnToRefinery → Dock`)
/// is driven by the simulation once the object is on `Harvest`, but a
/// harvester that rolled out of a war factory can sit on its birth posture
/// and the machine never starts. Every harvester this brain owns that is not
/// already harvesting is given the click the player would give it; the field
/// search is the mission's job, so the cell is the unit's own.
pub(super) fn harvesters(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    if tick%15!=0 {return;}
    brain.miner_rejected.retain(|_, (_, until)| *until > tick);
    // Bound long-lived state even if many crate miners die or change owner.
    if tick % 300 == 0 {
        let miners: BTreeSet<u64> = brain.census.units.iter()
            .filter(|unit| unit.traits.harvester && unit.controllable())
            .map(|unit| unit.entity_id).collect();
        brain.miner_journeys.retain(|id, _| miners.contains(id));
        brain.miner_idle_since.retain(|id, _| miners.contains(id));
        brain.miner_standby.retain(|id,_|miners.contains(id));
    }
    let harvest = MissionId::from_known(MissionType::Harvest);
    let unload = MissionId::from_known(MissionType::Unload);
    let idle: Vec<(u64, (u16, u16), crate::sim::intern::InternedId, MissionId)> = brain
        .census
        .units
        .iter()
        .filter(|unit| {
            unit.traits.harvester
                && !unit.hand
                && unit.orderable
                && !unit.in_service()
                // An Enslaves= chassis is a mobile *refinery*, not a
                // conventional ore truck. It must deploy beside ore and let
                // its spawned workers harvest. Harvest on it merely executes
                // the native slave-manager prologue, never drives to the seam.
                && obs.object_type(rules, unit.type_id)
                    .is_some_and(|ty| ty.enslaves.is_none()
                        && ty.category == ObjectCategory::Vehicle)
                && unit.mission != unload
        })
        .map(|unit| (unit.entity_id, unit.cell, unit.type_id, unit.mission))
        .collect();
    // The spending decision already surveyed the same ore and counted its
    // density. One snapshot for purchase and dispatch, not two stale answers.
    let anchor = brain.census.base_cell;
    let patches: Vec<_> = brain.ore_scan.as_ref()
        .map(|survey| survey.patches.clone()).unwrap_or_default();
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    // What the trucks must not be sent into. Ore inside an enemy base is the
    // wishlist's "the harvester walks into the enemy base to mine": the field is
    // real and the trip is a donation.
    let space = super::terrain::Space::read(obs, rules, &brain.census.home_cells, anchor);
    for (id, cell, type_id, mission) in idle {
        if ordered_this_frame(out, id)
            || brain.miner_avoid_until.get(&id).is_some_and(|until| *until > tick)
        {
            continue;
        }
        let Some(zone) = obs.object_type(rules, type_id).map(|ty| ty.movement_zone)
            else { continue; };
        let guns=super::safety::envelopes(rules,obs,id,&visible,true);
        let safe=|field:(u16,u16)|space.ground_at(field).buildable()
            && mining_leg_safe(brain,obs,zone,cell,field,&guns)
            && !brain.miner_rejected.get(&id).is_some_and(|(blocked,end)|
                *blocked==field && tick<*end)
            && (!obs.is_cell_visible(field.0,field.1) || super::ore::at(rules,obs,field));
        let bay_distance=|field:(u16,u16)|brain.census.refinery_cells.iter()
            .map(|bay|cell_distance(*bay,field)).min().unwrap_or(i32::MAX/4);
        let local=patches.iter().filter(|patch|bay_distance(patch.cell)<=24
            && safe(patch.cell))
            .min_by_key(|patch|(bay_distance(patch.cell)+cell_distance(cell,patch.cell),
                std::cmp::Reverse(patch.stock),patch.cell)).map(|p|p.cell);
        let target=if obs.is_cell_visible(cell.0,cell.1) && super::ore::at(rules,obs,cell)
            && safe(cell) && (bay_distance(cell)<=24 || local.is_none()) {Some(cell)}
            else {local.or_else(||super::ore::best_patch(cell,&patches,
                super::ore::REMOTE_WORK_CELLS,safe))};
        // A loaded truck returning to its own bay is not searching for ore.
        // Preserve native unloading; never replace it with another expedition.
        let returning=obs.miner_cargo_bales(id).is_some_and(|bales|bales>0)
            && obs.own_destination_object(id).is_some_and(|bay|
                obs.owns_object(bay) && obs.entity_type(rules,bay)
                    .is_some_and(|ty|ty.resource_destination)
                    && obs.observed_object_cell(bay).is_some_and(|at|
                        !hostile_mining_ground(brain,at)
                        && super::safety::safe_leg(&guns,cell,at,true)));
        if returning {brain.miner_standby.remove(&id);continue;}
        let destination=obs.destination(id);
        let unsafe_native=destination.is_some_and(|dest|
            !mining_leg_safe(brain,obs,zone,cell,dest,&guns)
            || (local.is_some() && bay_distance(dest)>32
                && !obs.miner_cargo_bales(id).is_some_and(|bales|bales>0)));
        let unsafe_trip=brain.miner_journeys.get(&id).is_some_and(|(dest,_,_)|
            !mining_leg_safe(brain,obs,zone,cell,*dest,&guns)
            || (obs.is_cell_visible(dest.0,dest.1) && !super::ore::at(rules,obs,*dest)));
        if target.is_none() || unsafe_native || unsafe_trip {
            // The native Harvest mission can silently pick an alternate
            // deposit. Cancel an unsafe approach even when already moving.
            stand_miner_down(brain,rules,obs,out,id,cell,zone,&guns);
            continue;
        }
        if let Some(post)=brain.miner_standby.remove(&id) {
            if destination==Some(post) {
                brain.tracked.cancel_for_safety(id);
                let _=out.want(id,0,Goal::Stop);
                continue;
            }
        }
        if mission == harvest && !brain.miner_journeys.contains_key(&id) {
            // Native Harvest is already doing useful work if it is walking,
            // carrying ore or has known ore nearby. Do not disrupt it simply
            // because a distant high-density seam was discovered. When native
            // keeps its Harvest mission but the empty miner stands on a spent
            // field for 300 frames, it needs a deliberate new expedition.
            let native_scan = (rules.general.tiberium_long_scan / 256).max(1);
            let nearby_ore = patches.iter().any(|patch|
                super::advance::cell_distance(cell, patch.cell) <= native_scan / 2
                    && mining_leg_safe(brain,obs,zone,cell,patch.cell,&guns));
            if obs.destination(id).is_some()
                || obs.miner_cargo_bales(id).is_some_and(|bales| bales > 0)
                || nearby_ore
            {
                brain.miner_idle_since.remove(&id);
                continue;
            }
            let entry = brain.miner_idle_since.entry(id).or_insert((cell, tick));
            if entry.0 != cell { *entry = (cell, tick); }
            if tick.saturating_sub(entry.1) < 300 { continue; }
            brain.miner_idle_since.remove(&id);
            log::info!("auto play: harvester {id} idle on empty known field; surveying remote ore");
        } else {
            brain.miner_idle_since.remove(&id);
        }
        // Native Harvest scans only its own LongScan radius (YR: 48 cells),
        // regardless of the cell named by MegaMission. If the rich seam is
        // beyond the scan, a miner must physically drive near it FIRST.
        // Maintain that trip across frames and only then start native Harvest.
        if let Some((target, last_cell, last_progress)) = brain.miner_journeys.get(&id).copied() {
            if cell_distance(cell, target) <= 3 {
                brain.miner_journeys.remove(&id);
                if issue(brain, out, rules, obs, id, Goal::Harvest { cell: target }) {
                    brain.tracked.record(tick, id, TrackedKind::Harvest, None, None);
                }
                continue;
            }
            let progress = if cell != last_cell { tick } else { last_progress };
            if tick.saturating_sub(progress) >= 300
                || !obs.can_reach(zone, cell, target)
                || !brain.ore_known.contains_key(&target)
            {
                brain.miner_journeys.remove(&id);
                brain.miner_rejected.insert(id, (target, tick.saturating_add(900)));
                log::debug!("auto play: miner {id} abandons stalled ore journey to {target:?}");
            } else {
                brain.miner_journeys.insert(id, (target, cell, progress));
                if obs.destination(id) == Some(target) { continue; }
                if obs.destination(id).is_some() { continue; }
                // Navigation reached a temporary halt; retry at most after
                // actual progress, without continually restarting its mission.
                if issue(brain, out, rules, obs, id, Goal::Move { cell: target }) {
                    brain.tracked.record(tick, id, TrackedKind::Move, None, Some(target));
                }
                continue;
            }
        }
        // Do not commandeer a miner currently executing another legitimate
        // movement (e.g. a tactical retreat) and treat it as a fresh ore hunt.
        if obs.destination(id).is_some() { continue; }
        let Some(target) = target else { continue; };
        brain.miner_standby.remove(&id);
        // Commit the remote trip as an ordinary, real Move. A 100-cell ore
        // field cannot be harvested by repeatedly clicking Harvest from home.
        let native_scan = (rules.general.tiberium_long_scan / 256).max(1);
        if cell_distance(cell, target) > native_scan / 2 {
            if issue(brain, out, rules, obs, id, Goal::Move { cell: target }) {
                brain.miner_journeys.insert(id, (target, cell, tick));
                brain.tracked.record(tick, id, TrackedKind::Move, None, Some(target));
                log::debug!("auto play: miner {id} dispatched to known ore seam {target:?}");
            }
            continue;
        }
        if !brain.tracked.admits(tick, id, TrackedKind::Harvest, None, None) {
            continue;
        }
        if issue(brain, out, rules, obs, id, Goal::Harvest { cell: target }) {
            brain.tracked.record(tick, id, TrackedKind::Harvest, None, None);
        }
    }
}

/// How far from the house's own base a captured building may stand before it
/// is sold rather than held, in cells.
///
/// Past this the building is behind the enemy's line: the house cannot walk a
/// wrench or a defender to it, and every frame it stands is a frame it can be
/// taken back. Inside it the same building is worth keeping and garrisoning.
const HOLDLESS_CELLS: i32 = 36;

/// A passive refund is not a defense. Never sell a remote or weak building
/// solely because it is far from the first base: a captured refinery or lab
/// can be the entire reason this house is still alive. Only a building that
/// is nearly destroyed *during a currently visible attack* may be considered.
const HOLDLESS_HEALTH: u16 = 120;
const SALE_THREAT_CELLS: i32 = 10;

/// Sell the buildings this house cannot hold.
///
/// A captured building is a building the house holds but did not make, and the
/// two facts that matter are that the yard cannot replace it and that it is
/// standing where the house cannot defend it. Held, it is worth keeping: it is
/// power, income or a factory the enemy paid for. Behind the enemy's line it is
/// a hostage, and the refund is worth more than the building.
///
/// The test for "did not make it" is the house's own build list: a type this
/// house's yards could produce is one the house built, and a type they could not
/// is one the house took. The yards themselves are never sold — they are the
/// house — and a yard maker building is left alone, because it is worth packing
/// and driving home rather than refunding.
pub(super) fn sell_hard_to_hold(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    if tick < brain.sale_at {
        return;
    }
    let Some(home) = brain.census.base_cell else {
        return;
    };
    // What this house's yards could make: the yard itself is `BuildConst=` and
    // is in this list, so it is excluded by name below as well.
    let buildable: BTreeSet<crate::sim::intern::InternedId> =
        super::build::class_types(rules, obs, ObjectCategory::Building)
            .map(|(_, id)| id)
            .collect();
    let space = super::terrain::Space::read(obs, rules, &brain.census.home_cells, Some(home));
    for unit in &brain.census.units {
        let Some(ty) = obs.object_type(rules, unit.type_id) else {
            continue;
        };
        if ty.category != ObjectCategory::Building
            || buildable.contains(&unit.type_id)
            || obs.owned_by_hand(unit.entity_id)
        {
            continue;
        }
        // A sale the world did not show is not a sale that is about to happen.
        // The refund path has no answer for a type the house's own yards cannot
        // produce, so the same captured building qualified again every
        // `SALE_GAP_FRAMES` for the whole match. Written off after
        // [`SALE_ATTEMPTS`], and said once.
        match brain.sale_attempts.get(&unit.entity_id).copied() {
            Some(attempts) if attempts >= SALE_ATTEMPTS => continue,
            Some(attempts) => {
                log::info!(
                    "auto play: {} at {:?} did not sell (attempt {})",
                    ty.id,
                    unit.cell,
                    attempts + 1
                );
            }
            None => {}
        }
        // The yard: the house, not a hostage. And a maker building is worth
        // packing and driving home, which is a different errand.
        if super::expand::makes_a_yard(rules, ty) {
            continue;
        }
        // A Construction Yard is never sold and never packed while it is the
        // house's only one, whatever its cell reads as. An enemy at the gate
        // makes every cell near it Contested, and reading that as "cannot be
        // held" is how a base packs itself up under fire — the wishlist's
        // "主基地车会收缩".
        if ty.construction_yard
            && (brain.census.yards <= 1 || brain.census.base_cell == Some(unit.cell))
        {
            continue;
        }
        let far = squared_distance(home, unit.cell) > HOLDLESS_CELLS * HOLDLESS_CELLS;
        let hurt = unit.health_permille <= HOLDLESS_HEALTH;
        let threatened = brain.census.enemies.iter().any(|enemy| {
            obs.is_cell_visible(enemy.cell.0, enemy.cell.1)
                && squared_distance(enemy.cell, unit.cell)
                    <= SALE_THREAT_CELLS * SALE_THREAT_CELLS
        });
        // The build-list membership is not capture provenance. Be maximally
        // conservative: never liquidate a source of money, power, production,
        // radar, tech or a Construction Yard, even if it came from the enemy.
        // A distant, unthreatened trophy is an asset, not a hostage.
        let strategic = ty.build_const_eligible()
            || ty.construction_yard
            || ty.refinery
            || ty.produce_cash_amount != 0
            || ty.factory_type().is_some()
            || ty.unit_repair
            || ty.ore_purifier
            || ty.cloning
            || ty.factory_plant
            || ty.power > 0
            || ty.radar
            || ty.spy_sat
            || ty.super_weapon.is_some()
            || ty.super_weapon2.is_some()
            || super::build::earns_its_keep(rules, ty);
        if !may_sell_captured(far, hurt, threatened, strategic) {
            continue;
        }
        let abandoned = !space.ground_at(unit.cell).buildable();
        log::info!(
            "auto play: sell {} at {:?} (far={far} hurt={hurt} abandoned={abandoned})",
            ty.id,
            unit.cell
        );
        out.intent(Intent::SellBuilding {
            entity_id: unit.entity_id,
        });
        *brain.sale_attempts.entry(unit.entity_id).or_default() += 1;
        // One at a time: a dozen refunds in a frame is a windfall, and the next
        // one is a frame away.
        brain.sale_at = tick.saturating_add(SALE_GAP_FRAMES);
        return;
    }
}

/// Sale requires all positive evidence: distant, dying, under direct enemy
/// attack, and not important to continued play. It never fires from a stale
/// strategic memory or simply from the absence of friendly units nearby.
fn may_sell_captured(far: bool, dying: bool, threatened: bool, strategic: bool) -> bool {
    far && dying && threatened && !strategic
}

/// How many frames between two sales.
const SALE_GAP_FRAMES: u64 = 60;

/// How many times one building may be sold before it is written off.
///
/// Three, because the refund is not instant: a sale the world accepts takes the
/// building down over a few frames, and a step that gave up on the first frame
/// would write off a sale that was already working. Three gaps is a sale that
/// plainly did not take.
const SALE_ATTEMPTS: u32 = 3;

/// Whether a type's weapon carries a mind-control warhead.
///
/// The warhead query the wishlist asked for: `Primary=`/`Secondary=` name a
/// weapon, the weapon names a warhead, and the warhead carries the flags. Retail
/// marks nothing at unit level for this — a Yuri clone and a rifleman differ only
/// in what their weapons point at — so this is the only place the answer exists.
fn mind_controls(rules: &RuleSet, ty: &ObjectType) -> bool {
    (0..2).any(|slot| {
        ty.weapon_at(slot)
            .and_then(|name| rules.weapon(name))
            .and_then(|weapon| weapon.warhead.as_deref())
            .and_then(|name| rules.warhead(name))
            .is_some_and(|warhead| warhead.mind_control)
    })
}

/// The infiltration classes that unlock something once per match: a barracks
/// gives veteran infantry, a war factory veteran vehicles, a lab the other
/// side's tech. Any one of them is enough; a second spy into the same class
/// buys nothing.
fn spy_class_code(rules: &RuleSet, ty: &ObjectType) -> Option<u8> {
    // Mirror the native arm selection, including its higher-priority power
    // and shroud effects. A stolen lab unlock belongs to its *own* side:
    // stealing Allied tech must not mark Soviet/Yuri lab targets exhausted.
    if ty.radar || ty.power > 0 { return None; }
    if rules.is_build_tech(&ty.id) {
        return Some(match ty.ai_base_planning_side { 0 => 3, 1 => 4, _ => 5 });
    }
    if ty.super_weapon.is_some() || ty.super_weapon2.is_some() || ty.storage > 0 { return None; }
    match ty.factory_type() {
        Some(crate::rules::object_type::FactoryType::InfantryType) => Some(1),
        Some(crate::rules::object_type::FactoryType::UnitType) => Some(2),
        _ => None,
    }
}

/// Whether this class is still worth a spy.
fn spy_class_open(
    rules: &RuleSet,
    ty: &ObjectType,
    taken: &std::collections::BTreeSet<u8>,
) -> bool {
    spy_class_code(rules, ty).is_none_or(|code| !taken.contains(&code))
}

/// Whether an enemy can see or take a spy on its way to `cell`.
///
/// A spy is revealed before it arrives, and a spy taken by mind control is worse
/// than one shot: it walks into the enemy's own base afterwards.
fn has_disguise_weapon(rules: &RuleSet, ty: &ObjectType) -> bool {
    (0..ty.weapon_record_count()).any(|slot| {
        ty.weapon_at(slot).and_then(|name| rules.weapon(name))
            .filter(|weapon| !weapon.never_use)
            .and_then(|weapon| weapon.warhead.as_deref()
                .and_then(|name| rules.warhead(name)))
            .is_some_and(|head| head.makes_disguise)
    })
}

/// A negative range is the original MakeupKit's special fire-from-anywhere
/// convention. This is an *attack* through L2, not a move toward the specimen.
fn disguise_shot_is_remote(rules: &RuleSet, ty: &ObjectType) -> bool {
    (0..ty.weapon_record_count()).any(|slot| {
        ty.weapon_at(slot).and_then(|name| rules.weapon(name))
            .filter(|weapon| !weapon.never_use && weapon.range_leptons < 0)
            .and_then(|weapon| weapon.warhead.as_deref()
                .and_then(|name| rules.warhead(name)))
            .is_some_and(|head| head.makes_disguise)
    })
}

fn spy_route_blocked(
    obs: &Observation<'_>,
    rules: &RuleSet,
    cell: (u16, u16),
    watchers: &[(u16, u16)],
) -> bool {
    watchers
        .iter()
        .any(|watcher| squared_distance(*watcher, cell) <= SPY_WATCH_CELLS * SPY_WATCH_CELLS)
}

/// How near a spy's target a watcher may stand before the walk is refused, in
/// cells: the reveal range plus a step, since the spy is seen on approach.
const SPY_WATCH_CELLS: i32 = 6;
/// Strategic memory lasts longer than live combat sightings, but the next
/// Capture still requires current visual confirmation of the actual building.
const SPY_SITE_MEMORY_FRAMES: u64 = 7_200;

/// How near a crate has to be for it to be a detour, in cells.
///
/// A crate next to the unit is a bonus it takes on its way past, and any
/// unit may take it. A crate further away is a job, and the house does not
/// give a unit one: measured, a fighter that walked thirty cells for a box
/// turned a win into a timeout.
const CRATE_SEEK_CELLS: i32 = 8;
/// Crates are transient overlays. A sighting behind fog is a *lead*, not a
/// guaranteed box: the random crate may expire before the runner gets there.
/// Keep the lead briefly, but give currently visible boxes first refusal.
const CRATE_MEMORY_FRAMES: u64 = 360;
/// Never lend a fast scout to a vanished box for a whole regeneration cycle.
const CRATE_CLAIM_FRAMES: u64 = 600;

fn crate_collector_type(ty:&ObjectType)->bool {
    matches!(ty.category,ObjectCategory::Infantry|ObjectCategory::Vehicle)
        && ty.speed>0 && !ty.jumpjet && !ty.balloon_hover
        && ty.movement_zone!=MovementZone::Fly
        && !matches!(ty.locomotor,crate::rules::locomotor_type::LocomotorKind::Fly
            |crate::rules::locomotor_type::LocomotorKind::Jumpjet
            |crate::rules::locomotor_type::LocomotorKind::Rocket
            |crate::rules::locomotor_type::LocomotorKind::Teleport)
        && !ty.spawned && !ty.engineer && !ty.agent && !ty.resource_gatherer
}

fn crate_sighting_fresh(now: u64, last_seen: u64) -> bool {
    now.saturating_sub(last_seen) <= CRATE_MEMORY_FRAMES
}

/// Give an available scout a visible crate collection errand.
///
/// Scout exploration may be interrupted for a valuable crate; ordinary
/// combat units are borrowed only when idle and nearby. The pickup is the
/// walk itself, and a claimed box must not attract another unit.
pub(super) fn crates(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    let visible_cells: BTreeSet<(u16, u16)> = obs.crate_cells(rules)
        .into_iter().filter(|cell| obs.is_cell_visible(cell.0, cell.1))
        .collect();
    for &cell in &visible_cells {
        brain.crate_sightings.insert(cell, tick);
    }
    // Do not peek at crate slots behind fog to determine whether a sighting
    // still exists. Confirm disappearance only where we have current vision.
    brain.crate_sightings.retain(|cell, seen| {
        crate_sighting_fresh(tick, *seen)
            && (!obs.is_cell_visible(cell.0, cell.1)
                || obs.visible_crate_at(rules, *cell))
    });
    let cells: Vec<(u16, u16)> = brain.crate_sightings.keys().copied().collect();
    // A collection run must *end*. An idle scout that picked up a crate at
    // the front was otherwise immediately made an independent assault member,
    // where one dog or fast IFV attacked a base alone and died. Keep a brief
    // regroup order, never a permanent exclusion from the army.
    let live: BTreeSet<u64> = brain.census.units.iter()
        .map(|unit| unit.entity_id).collect();
    brain.crate_returns.retain(|id, (post, until)| {
        // A collector may turn into a crate-spawned yard maker without changing
        // its entity ID. Its old return-to-the-fist order is no longer valid.
        live.contains(id) && tick < *until
            && !brain.census.units.iter().any(|unit|
                unit.entity_id == *id && unit.traits.yard_maker)
            && brain.census.units.iter().any(|unit| unit.entity_id == *id
                && unit.controllable() && !unit.in_service()
                && obs.object_type(rules,unit.type_id).is_some_and(crate_collector_type)
                && squared_distance(unit.cell, *post) > 4 * 4)
    });
    brain.crate_retry_after.retain(|(id,_),until|live.contains(id) && tick<*until);
    let air_contacts:Vec<_>=obs.visible_enemy_entities().filter(|e|
        obs.object_type(rules,e.type_id).is_some_and(|ty|
            ty.strength>=500 && (ty.category==ObjectCategory::Aircraft
                || ty.balloon_hover || (ty.jumpjet && !obs.is_deployed(e.entity_id)))))
        .collect();
    let census=&brain.census;
    let air_needed=|u:&super::census::Unit|census.mobile_aa.contains(&u.entity_id)
        && air_contacts.iter().any(|e|super::advance::cell_distance(u.cell,e.cell)<=32
            || census.base_cell.is_some_and(|home|
                super::advance::cell_distance(home,e.cell)<=44));
    let invalid:Vec<_>=brain.crate_claims.iter().filter_map(|(&id,&(cell,_))|{
        let u=brain.census.units.iter().find(|u|u.entity_id==id)?;
        let valid=obs.object_type(rules,u.type_id).is_some_and(|ty|
            crate_collector_type(ty) && obs.can_reach(ty.movement_zone,u.cell,cell));
        (!valid || !u.controllable() || u.in_service() || air_needed(u))
            .then_some((id,cell,u.controllable(),u.in_service()))
    }).collect();
    for (id,cell,controllable,in_service) in invalid {
        brain.crate_claims.remove(&id);
        brain.crate_returns.remove(&id);
        brain.tracked.tasks.release(id);
        brain.tracked.void(id,TrackedKind::Move);
        brain.tracked.forget_primary(id);
        brain.crate_retry_after.insert((id,cell),tick.saturating_add(900));
        if controllable && !in_service && obs.destination(id)==Some(cell) && !out.ordered_this_frame(id) {
            let _=out.want(id,2,Goal::Stop);
        }
        log::info!("auto play: v60_crate_release tick={tick} actor={id} cell={cell:?} reason=invalid-collector");
    }
    let finished: Vec<u64> = brain.crate_claims.iter()
        .filter(|(id, (cell, until))| !live.contains(id)
            || brain.census.units.iter().any(|unit|
                unit.entity_id == **id && unit.traits.yard_maker)
            || *until <= tick || !cells.contains(cell)
            || !brain.tracked.tasks.active(**id, JobKind::Crate))
        .map(|(id, _)| *id).collect();
    for id in finished {
        if let Some((cell,_))=brain.crate_claims.remove(&id) {
            if cells.contains(&cell) {
                brain.crate_retry_after.insert((id,cell),tick.saturating_add(900));
                if brain.census.units.iter().any(|u|u.entity_id==id && u.controllable() && !u.in_service())
                    && obs.destination(id)==Some(cell) && !out.ordered_this_frame(id) {
                    let _=out.want(id,2,Goal::Stop);
                }
                log::info!("auto play: v60_crate_release tick={tick} actor={id} cell={cell:?} reason=no-result");
            }
        }
        brain.tracked.tasks.release(id);
        brain.tracked.void(id, TrackedKind::Move);
        brain.tracked.forget_primary(id);
        // Returning alone to a home 100 cells away may be more dangerous
        // than the original box errand. Regroup with a *nearby friendly
        // formation* first; fall back to the base when nobody is close.
        if let Some(unit) = brain.census.units.iter().find(|unit|
            unit.entity_id == id && unit.controllable() && !unit.in_service()
                && !unit.traits.yard_maker && obs.object_type(rules,unit.type_id)
                    .is_some_and(crate_collector_type)) {
            let nearby = brain.census.units.iter()
                .filter(|other| other.entity_id != id && other.traits.combat
                    && other.controllable() && !other.in_service()
                    && !brain.crate_claims.contains_key(&other.entity_id)
                    && !brain.crate_returns.contains_key(&other.entity_id)
                    && squared_distance(unit.cell, other.cell) <= 26 * 26)
                .min_by_key(|other| (squared_distance(unit.cell, other.cell), other.entity_id))
                .map(|other| other.cell);
            let post = nearby.filter(|cell| super::orders::reachable(rules, obs, id, *cell)
                && !super::tactics::unsafe_post(rules, obs, id, *cell))
                .or(brain.census.rally.or(brain.census.base_cell)
                    .filter(|cell| super::orders::reachable(rules, obs, id, *cell)
                        && !super::tactics::unsafe_post(rules, obs, id, *cell)));
            if let Some(post) = post
                && squared_distance(unit.cell, post) > 4 * 4
            {
                brain.crate_returns.insert(id, (post, tick.saturating_add(1_200)));
                log::debug!("auto play: crate courier {id} regroups at {post:?}");
            }
        }
    }
    for (&id, &(post, _)) in &brain.crate_returns {
        if out.ordered_this_frame(id) || obs.destination(id) == Some(post) { continue; }
        if issue_tracked(&mut brain.tracked, brain.trace, out, rules, obs,
            id, TrackedKind::Avoid, None, Some(post), Goal::Move { cell: post }) {
            log::debug!("auto play: crate courier {id} returning to {post:?}");
        }
    }
    if cells.is_empty() { return; }
    // A crate is a bonus and not a job, and the unit that takes it is one
    // with nothing else to do: not a unit the push owns, not a unit that is
    // already walking somewhere, not a unit that is fighting.
    // The detail: fast bodies whose job this is. A crate is a bonus, and the unit
    // that takes it should be one the house can spare for thirty seconds, not
    // whichever fighter happens to be standing nearest — a Grizzly pulled off the
    // line for a box is a tank the push does not have.
    //
    // The wishlist's "捡箱子单独拉出一个线程": the detail is a standing
    // assignment of the quickest idle-capable types, and only its members collect.
    // Speed is the rules' own `Speed=`, so a mod's fast unit joins it and a slow
    // one does not, with no type named here.
    let scouts: BTreeSet<u64> = brain.census.units.iter()
        .filter(|unit| unit.traits.scout).map(|unit| unit.entity_id).collect();
    let attackers: BTreeSet<u64> = brain.squads.squad(super::squads::Role::Attack)
        .map(|squad| squad.members.iter().copied().collect()).unwrap_or_default();
    let mut detail: Vec<(u64, (u16, u16), MovementZone, i32)> = brain
        .census
        .units
        .iter()
        .filter(|unit| {
            (unit.traits.scout || unit.traits.combat)
                && !unit.traits.yard_maker && !unit.traits.irreplaceable
                && unit.health_permille>=600
                && !brain.census.factory_handoffs.contains_key(&unit.entity_id)
                && !brain.tracked.held(tick,unit.entity_id)
                && brain.tracked.tasks.get(unit.entity_id).is_none()
                && !brain.asset_guards.contains_key(&unit.entity_id)
                && !brain.outpost_guards.contains_key(&unit.entity_id)
                && !brain.specialist_shuttles.contains_key(&unit.entity_id)
                && !brain.specialist_shuttles.values().any(|trip|trip.passenger==unit.entity_id)
                && !super::advanced::repair_ifv_loaded(rules,obs,&brain.census,unit.entity_id)
                && !air_needed(unit)
                && !(brain.director.under_attack_now() && brain.census.mobile_aa.contains(&unit.entity_id))
                && unit.controllable()
                && !unit.in_service()
                // A scout on an ordinary exploration Move is still the
                // dedicated crate runner. Combat units may only be borrowed
                // when idle: never interrupt a tank's attack for a box.
                && (unit.traits.scout || !holds_or_fights(unit.mission)
                    || (obs.attack_target(unit.entity_id).is_none()
                        && obs.destination(unit.entity_id).is_none())
                    // A box at a soldier's feet is not a remote expedition.
                    // Borrow a unit already walking to a *different* goal,
                    // but never interrupt an actual exchange of fire.
                    || (obs.attack_target(unit.entity_id).is_none()
                        && !super::micro::firing(obs, unit.mission, unit.entity_id)
                        && cells.iter().any(|cell|
                            squared_distance(unit.cell, *cell) <= 2 * 2)))
                && !brain.crate_returns.contains_key(&unit.entity_id)
                && !brain.joint_ops.reserves(unit.entity_id)
        })
        .filter_map(|unit| {
            let ty=obs.object_type(rules,unit.type_id)?;
            if !crate_collector_type(ty) || (obs.own_elite(unit.entity_id)
                && !cells.iter().any(|cell|squared_distance(unit.cell,*cell)<=2*2))
            {return None;}
            let zone=ty.movement_zone;
            let speed=i32::from(ty.speed);
            Some((unit.entity_id, unit.cell, zone, speed))
        })
        // Nearby crates belong to *any* mobile spare infantryman, even a
        // Speed=4 GI. The remote detail still uses the fast-runner threshold.
        .filter(|(_, cell, _, speed)| *speed >= CRATE_RUNNER_SPEED
            || cells.iter().any(|crate_cell|
                squared_distance(*cell, *crate_cell) <= 3 * 3))
        .collect();
    // Quickest first, so the detail is the same units every frame rather than
    // whichever the store yields first.
    detail.sort_by_key(|(id, _, _, speed)| {
        (if scouts.contains(id) { 0 } else { 1 }, -*speed, *id)
    });
    // Do not take the first four globally *before* checking distance.
    // They may all be on the other side of the map; the fifth scout could
    // stand on the crate. Cap simultaneous claims, not candidates examined.
    let idle: Vec<(u64, (u16, u16), MovementZone, bool, i32)> = detail
        .into_iter()
        .map(|(id, cell, zone, speed)| (id, cell, zone, false, speed))
        .collect();
    if idle.is_empty() {
        return;
    }
    // The crates, nearest the house first: the box closest to home is the
    // one a player sends for first, and the one further out is a walk only
    // worth it once the near one is taken.
    let anchor = brain
        .census
        .rally
        .or(brain.census.base_cell)
        .unwrap_or((0, 0));
    let mut crates = cells;
    // A box at a soldier's feet must not wait behind four distant jobs simply
    // because those boxes are closer to the old home anchor.
    crates.sort_by_key(|cell| {
        let closest = idle.iter().map(|(_, from, _, _, speed)|
            squared_distance(*from, *cell) / (*speed).max(1)).min()
            .unwrap_or(i32::MAX);
        let adjacent = idle.iter().any(|(_, from, _, _, _)|
            squared_distance(*from, *cell) <= 3 * 3);
        // A visible box is known to exist NOW. A remembered box may already
        // have been replaced by the next random crate wave. If a box appears
        // under a nearby unit, that immediate opportunity still comes first.
        (!adjacent, !visible_cells.contains(cell), closest,
            squared_distance(anchor, *cell), *cell)
    });
    let visible_threats:Vec<_>=obs.visible_enemy_entities().collect();
    let mut sent: BTreeSet<u64> = BTreeSet::new();
    for cell in crates {
        // Two extra slots are reserved for boxes appearing *under our feet*.
        // Otherwise four distant claimants can lock out a free soldier who
        // could collect a fresh trigger crate in the next two steps.
        let immediately_reachable = idle.iter().any(|(_, from, _, _, _)|
            squared_distance(*from, cell) <= 3 * 3);
        let cap = CRATE_RUNNERS + if immediately_reachable { 2 } else { 0 };
        if !crate_runner_available(brain.crate_claims.len(), cap) {
            continue;
        }
        // Each site is a firsthand or time-limited remembered sighting.
        // Never read the current hidden global crate slots at assignment time.
        // The nearest available scout across the map, or an idle nearby
        // fighter. The unit's own movement
        // zone: a tank and a dog reach different ground, and asking with the
        // infantry's zone for everybody sends vehicles at cells they cannot
        // walk to.
        //
        // One range, and it is the detour a unit takes on its way past. A
        // crate across the map is a walk, and the house that walks across
        // the map for it is a house that is not fighting: measured, sending a
        // fighter thirty cells for a box turned a win into a timeout.
        let reachable = |unit: &(u64, (u16, u16), MovementZone, bool, i32), range: i32| {
            // Dedicated scouts can cross the entire *known* map for a prize.
            // Combat units remain limited to their short detour range.
            let remote_runner = remote_crate_runner(
                scouts.contains(&unit.0), unit.4, attackers.contains(&unit.0));
            crate_runner_in_range(remote_runner,
                squared_distance(unit.1, cell), range)
                && obs.can_reach(unit.2, unit.1, cell)
                && super::safety::safe_leg(
                    &super::safety::envelopes(rules,obs,unit.0,&visible_threats,true),
                    unit.1,cell,false)
        };
        // The soonest arrival, not the nearest body. The walk is the entire cost
        // of a crate, so the question is who gets there first per unit of army
        // taken off the line — a dog four cells out beats a Grizzly one cell
        // out, and a fast IFV beats both a slow one and a rifleman. Distance
        // over the type's own `Speed=` is that comparison in one integer, and it
        // is scale-free: a mod that doubles every speed changes nothing here.
        //
        // The detour range itself stays as it was. Measured, a *fighter* sent
        // thirty cells for a box turned a win into a timeout, and that is a
        // statement about the range, not about which unit takes it.
        let arrival = |unit: &(u64, (u16, u16), MovementZone, bool, i32)| {
            let speed = unit.4.max(1);
            squared_distance(unit.1, cell) / speed
        };
        if brain.crate_claims.values().any(|(claimed,until)|
            *claimed == cell && *until > tick)
        { continue; }
        let picked = idle
            .iter()
            .filter(|(id, _, _, _, _)| !sent.contains(id)
                && !brain.crate_claims.contains_key(id)
                && !brain.crate_retry_after.contains_key(&(*id,cell))
                && !out.ordered_this_frame(*id)
                && brain.tracked.admits(tick, *id, TrackedKind::Move,
                    None, Some(cell)))
            .filter(|unit| reachable(unit, CRATE_SEEK_CELLS))
            .min_by_key(|unit| (arrival(unit), unit.0))
            .copied();
        let Some((id, from, zone, _, _)) = picked else {
            continue;
        };
        if issue_tracked(
            &mut brain.tracked, brain.trace, out, rules, obs,
            id, TrackedKind::Move, None, Some(cell), Goal::Move { cell },
        ) {
            sent.insert(id);
            brain.crate_claims.insert(id,(cell,tick.saturating_add(CRATE_CLAIM_FRAMES)));
            brain.tracked.tasks.claim(tick, id, JobKind::Crate, None, Some(cell), from);
            log::info!(
                "auto play: unit {id} goes for the crate at {cell:?} taken={}",
                obs.ground_object_cells(rules).contains(&cell)
            );
        }
    }
}

/// When planning collectors, all eligible candidates must be considered
/// before applying the *simultaneous assignment* cap. Cap candidates first and
/// a fast dog on the wrong side of the map can hide a nearby reachable dog.
fn crate_runner_available(claimed: usize, cap: usize) -> bool {
    claimed < cap
}

/// How fast a unit has to be before it belongs to the crate detail, in the
/// rules' `Speed=`. Below this a unit is a fighter first and a courier never:
/// the walk is the whole cost of a crate, and a slow body on it is a body off
/// the line for nothing.
const CRATE_RUNNER_SPEED: i32 = 6;

/// How many units the crate detail holds at once.
/// The dedicated scout detail travels to already-seen crates across the
/// entire reachable map; ordinary fighters are constrained to short detours.
/// Visibility and zone connectivity are checked separately at assignment.
fn crate_runner_in_range(remote: bool, distance_squared: i32, near: i32) -> bool {
    remote || distance_squared <= near.saturating_mul(near)
}

fn remote_crate_runner(scout: bool, speed: i32, in_attack: bool) -> bool {
    scout || (speed >= 9 && !in_attack)
}

const CRATE_RUNNERS: usize = 4;

/// The health a building is worth the wrench at, in permille of its full
/// strength.
///
/// Below nine tenths: a house that waits for a red bar repairs the building
/// after it has already lost the fight it was holding, and a house that repairs
/// a scratch spends credits it cannot spare. The repair costs the damage, so
/// the threshold is where a scratch becomes a wound.
const REPAIR_HEALTH_PERMILLE: i32 = 900;

/// How long a building is left with the wrench the house has already switched
/// on, in frames: a minute (900 frames at 15 a second). A repair takes minutes,
/// so a click that never took is worth asking again after a quarter of one.
const REPAIR_REARM_FRAMES: u64 = 900;

/// Turn the wrench on the house's own damaged buildings.
///
/// `Command::ToggleRepair` is the Display REPAIR15 click
/// (`Display BandBox_LeftUp4AC0E7`): the building spends the house's credits to
/// heal over time and switches itself off at full strength. It is a toggle, so
/// asking twice turns a repair *off* — the brain therefore remembers the
/// buildings it has already switched on, and forgets one only when it is whole
/// again. Nothing about the click is per-frame, so nothing here is either.
fn repair_wrench_allowed(ty: &ObjectType) -> bool {
    ty.category == ObjectCategory::Building && ty.repairable && !ty.can_be_occupied
}

pub(super) fn repairs(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    budget: &super::plan::Budget,
    out: &mut Outbox,
) {
    let mut budget = *budget;
    let mut issued = 0;
    let mut whole: Vec<u64> = Vec::new();
    let mut hurt: Vec<(bool,i32,u64,i32)> = Vec::new();
    for entity in obs.own_entities() {
        let Some(ty) = obs.object_type(rules, entity.type_id) else {
            continue;
        };
        // An occupied civilian building is maintained with an engineer, not
        // a native sidebar wrench. The toggle is refused or wastes orders.
        if !repair_wrench_allowed(ty) {
            continue;
        }
        let Some(full) = obs
            .max_health(rules, entity.type_id)
            .filter(|full| *full > 0)
        else {
            continue;
        };
        let permille =
            i32::try_from(i64::from(entity.health.max(0)) * 1_000 / i64::from(full)).unwrap_or(0);
        if permille >= 1_000 {
            whole.push(entity.entity_id);
        } else if permille < REPAIR_HEALTH_PERMILLE && brain.repaired.contains(&entity.entity_id) {
            // Repair actual infrastructure before incidental towers. Never
            // start a repair on an already active native toggle.
            if let Some((false,cost)) = obs.own_repair_quote(rules,entity.entity_id) {
                let core = ty.resource_destination || ty.factory_type().is_some() || ty.power > 0;
                hurt.push((!core,permille,entity.entity_id,cost));
            }
        }
    }
    // A building is worth the wrench once it has stood whole: a structure still
    // going up is at a fraction of its strength by definition, and healing it
    // would spend the house's credits on the build it has already paid for.
    for entity_id in &whole {
        brain.repaired.insert(*entity_id);
        brain.repairing.remove(entity_id);
    }
    hurt.sort_unstable();
    for (optional,_,entity_id,cost) in hurt {
        if issued >= FORCE_ORDERS_PER_FRAME {
            break;
        }
        if obs.owned_by_hand(entity_id) {
            // A denied wrench is a toggle that never happened. Leave the
            // repair cooldown untouched until this building enters scope.
            continue;
        }
        // The memory has a term. A click that was refused — the budget, the
        // domain, the gate — leaves nothing behind, and a house that remembers
        // a repair it never started never asks again, which is why some
        // buildings healed and others never did.
        if brain
            .repairing
            .get(&entity_id)
            .is_some_and(|until| obs.tick() < *until)
        {
            continue;
        }
        if optional && brain.campaign.economy_resting() { continue; }
        if !budget.try_commit(cost) { continue; }
        brain
            .repairing
            .insert(entity_id, obs.tick().saturating_add(REPAIR_REARM_FRAMES));
        out.command(Command::ToggleRepair { entity_id });
        issued += 1;
    }
}

pub(super) fn observe_campaign_repairs(brain:&mut AssistBrain,rules:&RuleSet,
    obs:&Observation<'_>) {
    let now=obs.tick();
    brain.engineer_repair_after.retain(|_,end|now<*end);
    brain.campaign_restored_cover.retain(|_,end|now<*end);
    let mut repaired=Vec::new();
    let tasks=&brain.tracked.tasks;
    brain.campaign_repairs.retain(|target,(actor,before,until)| {
        if !obs.owns_object(*actor) && obs.observed_object_cell(*target).is_some()
            && !obs.is_hostile(*target) && obs.health(*target).is_some_and(|hp|
                hp>*before && obs.entity_type(rules,*target).is_some_and(|ty|hp>=ty.strength))
        {
            repaired.push(*target);
            return false;
        }
        now<*until && (!obs.owns_object(*actor) || tasks.get(*actor)
            .is_some_and(|job|job.kind==JobKind::Engineer && job.target==Some(*target)))
    });
    for id in repaired {
        brain.engineers.note_repaired();
        brain.engineer_repair_after.insert(id,now.saturating_add(900));
        brain.campaign_restored_cover.insert(id,now.saturating_add(900));
        log::info!("auto play: v59_garrison_repair_observed tick={now} target={id}");
    }
}

fn repair_campaign_cover(brain:&mut AssistBrain,rules:&RuleSet,obs:&Observation<'_>,out:&mut Outbox) {
    let now=obs.tick();
    if brain.cover_detail.is_empty() && !brain.census.units.iter().any(|u|
        u.traits.engineer && u.controllable() && !u.in_service()
        && !ordered_this_frame(out,u.entity_id) && !brain.tracked.held(now,u.entity_id))
    {return;}
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let active:Vec<_>=brain.cover_detail.iter().map(|(id,detail)|(*id,*detail)).collect();
    for (site,(actor,post,deadline)) in active {
        let soldier=brain.census.units.iter().find(|u|u.entity_id==actor).copied();
        if obs.inside_transport(actor)==Some(site) && obs.owns_object(actor) {
            brain.cover_detail.remove(&site);
            log::info!("auto play: v62_cover_occupied tick={now} actor={actor} target={site}");
            continue;
        }
        let valid=soldier.is_some_and(|u|u.controllable() && u.health_permille>=600
            && !u.in_service() && obs.observed_object_cell(site).is_some()
            && !obs.is_hostile(site)
            && !brain.safety.reserved(rules,obs,&u)
            && super::safety::safe_attack_leg(rules,obs,&u,post,&visible)
            && reachable(rules,obs,actor,post));
        // A native Capture mission is in_service, but it is exactly the
        // final phase of this operation, not a conflicting task.
        let entering=soldier.is_some_and(|u|u.controllable()
            && brain.tracked.tasks.get(actor).is_some_and(|job|
                job.kind==JobKind::Garrison && job.target==Some(site))
            && super::safety::garrison_approach_safe(rules,obs,actor,site));
        if now>=deadline || (!valid && !entering) {
            brain.cover_detail.remove(&site);
            brain.engineer_repair_after.insert(site,now.saturating_add(600));
            if let Some((engineer,_,_))=brain.campaign_repairs.remove(&site) {
                brain.tracked.cancel_for_safety(engineer);
                brain.safety.abandon(engineer);
                brain.tracked.hold(engineer,now.saturating_add(90));
                if obs.owns_object(engineer) {let _=out.want(engineer,0,Goal::Stop);}
            }
            if soldier.is_some_and(|u|u.controllable()) {
                brain.tracked.cancel_for_safety(actor);
                brain.safety.abandon(actor);
                let _=out.want(actor,0,Goal::Stop);
            }
            continue;
        }
        if entering {
            if obs.own_destination_object(actor)!=Some(site) && now%30==actor%30
                && !ordered_this_frame(out,actor) {
                let _=issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
                    actor,TrackedKind::Capture,Some(site),obs.observed_object_cell(site),
                    Goal::Capture{target:site});
            }
            continue;
        }
        let Some(u)=soldier else {continue;};
        if obs.can_garrison(rules,actor,site)
            && super::safety::garrison_approach_safe(rules,obs,actor,site) {
            if !ordered_this_frame(out,actor) && out.want(actor,0,Goal::Capture{target:site}) {
                brain.tracked.cancel_for_safety(actor);
                brain.tracked.tasks.claim(now,actor,JobKind::Garrison,Some(site),
                    obs.observed_object_cell(site),u.cell);
                brain.tracked.record_goal(now,actor,TrackedKind::Capture,Some(site),
                    obs.observed_object_cell(site),Goal::Capture{target:site});
                log::info!("auto play: v62_cover_entry tick={now} actor={actor} target={site}");
            }
            continue;
        }
        if squared_distance(u.cell,post)>2*2 {
            if !ordered_this_frame(out,actor) && obs.destination(actor)!=Some(post)
                && admits(rules,obs,actor,&Goal::Move{cell:post}) {
                let _=out.want(actor,0,Goal::Move{cell:post});
            }
            brain.tracked.hold(actor,now.saturating_add(2));
            continue;
        }
        // Wait just outside the foundation. Do not give the soldier a
        // speculative Capture before native red-health admission opens.
        brain.tracked.hold(actor,now.saturating_add(2));
    }
    let engineers:Vec<_>=brain.census.units.iter().copied().filter(|u|
        u.traits.engineer && u.controllable() && !u.in_service()
        && !ordered_this_frame(out,u.entity_id) && !brain.tracked.held(now,u.entity_id)
        && brain.tracked.tasks.get(u.entity_id).is_none()
        && !brain.specialist_shuttles.values().any(|t|t.passenger==u.entity_id)).collect();
    if engineers.is_empty(){return;}
    let mut sites:Vec<_>=obs.campaign_engineer_sites(rules).filter_map(|(site,repair)|
        (repair && campaign_repair_needed(brain,rules,obs,&site,&visible)).then_some(site)).collect();
    sites.sort_by_key(|site|(!brain.cover_detail.contains_key(&site.entity_id),
        brain.census.base_cell.map_or(0,|home|squared_distance(home,site.cell)),site.entity_id));
    for site in sites {
        let id=site.entity_id;
        let Some(hp)=obs.health(id) else {continue;};
        let Some(engineer)=engineers.iter().filter(|u|obs.can_take_building(rules,u.entity_id,id)
            && super::safety::specialist_approach_safe(rules,obs,u.entity_id,id))
            .min_by_key(|u|(squared_distance(u.cell,site.cell),u.entity_id)) else {continue;};
        let (actor,post,_)=if let Some(plan)=brain.cover_detail.get(&id).copied(){plan}
            else {
                if !brain.cover_detail.is_empty(){continue;}
                let Some((actor,post))=cover_partner(brain,rules,obs,&site,&visible) else {continue;};
                let plan=(actor,post,now.saturating_add(1800));
                brain.cover_detail.insert(id,plan);
                brain.census.unavailable.insert(actor);
                brain.tracked.cancel_for_safety(actor);
                log::info!("auto play: v62_cover_pair tick={now} soldier={actor} target={id}");
                plan
            };
        if obs.observed_object_cell(actor).is_none_or(|cell|squared_distance(cell,post)>2*2) {
            if !ordered_this_frame(out,actor) && admits(rules,obs,actor,&Goal::Move{cell:post}) {
                let _=out.want(actor,0,Goal::Move{cell:post});
            }
            brain.tracked.hold(actor,now.saturating_add(2));
            break;
        }
        if issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,engineer.entity_id,
            TrackedKind::Capture,Some(id),Some(site.cell),Goal::Capture{target:id}) {
            brain.tracked.tasks.claim(now,engineer.entity_id,JobKind::Engineer,Some(id),Some(site.cell),engineer.cell);
            brain.engineer_repair_after.insert(id,now.saturating_add(450));
            brain.campaign_repairs.insert(id,(engineer.entity_id,hp,now.saturating_add(1800)));
            log::info!("auto play: v62_cover_repair tick={now} engineer={} soldier={actor} target={id}",engineer.entity_id);
            break;
        }
    }
}

/// Occupied civilian buildings have no ordinary repair-wrench path. The
/// native engineer's `Repair(true)` cursor issues `CaptureBuilding`, consumes
/// the engineer and restores the structure. Reserve that costly action for
/// valuable, seriously damaged positions rather than every scratched house.
pub(super) fn repair_garrisons(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
) {
    if brain.director.under_attack_now() && !obs.is_campaign() && brain.cover_detail.is_empty() {return;}
    // A finite specialist is spent only on useful, reachable cover. Normal
    // capture has already run; actual ownership/health determines outcomes.
    repair_campaign_cover(brain,rules,obs,out);
}

/// Whether a unit's click on an enemy building is a Capture click: the two
/// flags the per-cell handler branches on, an engineer's by `Engineer=` and a
/// spy's by `Infiltrate=` (which `Engineer=` also implies, per
/// `InfantryTypeClass::ReadINI` `0x0052466E..0x0052469A`).
pub(super) fn walks_into_buildings(rules: &RuleSet, ty: &ObjectType) -> bool {
    super::census::Traits::of(rules, ty).captures
}

/// The health fraction at which a fighting unit belongs to the retreat routine
/// rather than to the brain's own orders.
///
/// A unit the sim's own mission owns fights to the death, which is the wrong
/// trade for a house that pays for every replacement: a third of its strength
/// left is the point where a player walks it home. The routine hands the unit
/// back as soon as it is healthy again, so the threshold is the whole rule.
const RETREAT_HEALTH_PERMILLE: u16 = 300;

/// The health a unit pulls out at, which for a hero is most of it.
///
/// The wishlist's "保护英雄单位": `BuildLimit=1` means there is one of these and
/// the house cannot buy another, so the trade that is worth it for a rifleman —
/// a third of a health bar for a structure — is not worth it for the one unit
/// the house can never replace. A hero leaves at [`HERO_RETREAT_PERMILLE`], well
/// before the ordinary line, and the same routine walks it home.
fn retreat_line(traits: &super::census::Traits) -> u16 {
    if traits.irreplaceable {
        HERO_RETREAT_PERMILLE
    } else {
        RETREAT_HEALTH_PERMILLE
    }
}

/// Where a hero turns back, in permille of its full strength.
///
/// Three quarters: a Tanya at 200 strength leaves at 150, which is one rifleman
/// burst past the point a player would have walked her out. The exchange has to
/// be very good before a cap of one is worth spending.
const HERO_RETREAT_PERMILLE: u16 = 750;

/// How much the house's army has to outweigh what stands around a hurt unit
/// before the unit stays in the fight, in permille.
///
/// Half again is a clear trade, not a close one: the retreat is for the
/// exchange being lost, and a unit that pulls out of a fight the house is
/// winning is the "拉扯" that makes a push a string of small fights instead
/// of one. The margin is stricter than the push's own (1.2), because the
/// push may wait for a better moment and this unit cannot.
const RETREAT_SUPERIORITY_PERMILLE: i32 = 1_500;

/// Hand the house's hurt fighters back to a depot, or home when it has none.
///
/// With a `UnitRepair=` building standing, the retreat is the click a player
/// makes on it: Enter on the building (`FootClass::ClickedAction`'s depot arm).
/// The Enter mission asks the bay for `CAN_DOCK`, walks the unit onto the pad,
/// and the `DOCK_NOW` that follows hands both sides their service missions
/// (`radio::receive`'s `building_dock_now` queues `Repair` on the building and
/// `Sleep` on the unit), so the bay heals the unit and pays for it. Walking at
/// the depot's *cell* is not that click and cannot be: the pad is inside the
/// foundation, a Move to it has no arrival, and the unit stood at the wall
/// hurt for the rest of the match — which is what this step used to do.
///
/// With no depot, the unit is a managed routine's: it walks to the base's own
/// anchor so it breaks off rather than freezing where the fight left it. That
/// is the one place the brain uses a routine, asked through `Outbox::attach`
/// because the table of attached objects is L2's. Asking again for a unit the
/// table already holds is free: `MicroTable::attach` refuses a second attach,
/// so no bookkeeping is kept here and a routine that hands an object back is
/// picked up again the next frame its condition holds.
///
/// Either way a unit that has arrived is held out of the push, and is held
/// again every frame: it is still under the line the retreat was called at.
pub(super) fn managed(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    tick: u64,
    out: &mut Outbox,
) {

    // Ground casualties now keep actor-scoped recovery ownership; the legacy
    // managed routine remains for other movement classes.
    super::safety::recover(brain, rules, obs, tick, out);
    // Collected before anything is written: the routine takes the unit, and
    // the ledger is borrowed while it is read.
    let retreating: Vec<(u64, (u16, u16), MissionId, super::census::Slot)> = brain
        .census
        .units
        .iter()
        .filter(|unit| !unit.hand && unit.orderable && !unit.in_service())
        .filter(|unit| {
            (unit.traits.combat && (unit.health_permille <= retreat_line(&unit.traits)
                || super::tactics::exposed_casualty(rules,obs,unit)))
                || ((unit.traits.combat || unit.traits.harvester)
                    && obs.has_parasite(unit.entity_id)
                    && !brain.census.repair_sites.is_empty())
        })
        .filter(|unit| !super::safety::ground_recovery_unit(rules,obs,unit))
        // A hurt unit in a fight the house is winning stays in it. The
        // comparison is the push's own, with a stricter margin: the push may
        // wait for a better moment, this unit cannot, and a third of the
        // attackers walking home mid-exchange is what makes a push a series
        // of small fights that each one can be lost.
        .filter(|unit| {
            obs.has_parasite(unit.entity_id)
                || !super::squads::winning_fight(
                    &brain.census,
                    rules,
                    obs,
                    unit.cell,
                    RETREAT_SUPERIORITY_PERMILLE,
                )
        })
        .map(|unit| (unit.entity_id, unit.cell, unit.mission, unit.slot))
        .collect();
    for (entity_id, cell, mission, slot) in retreating {
        // The bay this unit walks to: the nearest one the house owns, so two
        // service depots share the work instead of every hurt unit crossing the
        // base to the first. With no bay the walk ends beside the base — the
        // yard's own cell is its foundation, and a Move to it is a walk that
        // never arrives.
        // UnitRepair is a vehicle bay, not an infantry hospital. Walking a
        // wounded soldier to its pad leaves a refused Enter intent; infantry
        // instead use the existing safe-home retreat routine.
        let site = (slot == super::census::Slot::Vehicle)
            .then(|| brain.census.nearest_repair_site(cell)).flatten();
        let depot = site.map(|(_, entity)| entity);
        let post = site
            .map(|(cell, _)| cell)
            .or(brain.census.rally)
            .or(brain.census.base_cell);
        // Standing on the post is the end of the walk. The army does not get the
        // unit back yet — it is still under the line the retreat was called at —
        // but it does not need a routine either: attaching one only to have it
        // answer `Done` costs an attach and a hand-back every frame, which is
        // how a hurt unit parked on the depot became the log's own subject. The
        // hold is what keeps the push from spending it again. On a depot the
        // same cell is the pad the bay is already servicing.
        if Some(cell) == post {
            brain
                .tracked
                .hold(entity_id, tick.saturating_add(HOLD_FRAMES));
            continue;
        }
        if let Some(depot) = depot {
            // The bay owns the unit once it is in. A depot whose pad is offset
            // from its own cell keeps the vehicle off the post while the repair
            // runs, so the cell is not enough: `Enter` is the visit, and the
            // docking `Sleep` and the building's `Repair` are the service. A
            // second Enter on top of a running repair is a new visit queued over
            // the paid one.
            if mission == MissionId::from_known(MissionType::Enter)
                || mission == MissionId::from_known(MissionType::Sleep)
                || mission == MissionId::from_known(MissionType::Repair)
            {
                brain
                    .tracked
                    .hold(entity_id, tick.saturating_add(HOLD_FRAMES));
                continue;
            }
            // The same click again while the previous one is still walking is
            // what restarts the walk: the intent table is the throttle.
            if brain
                .tracked
                .admits(tick, entity_id, TrackedKind::Enter, Some(depot), Some(cell))
            {
                brain
                    .tracked
                    .record(tick, entity_id, TrackedKind::Enter, Some(depot), Some(cell));
                issue(
                    brain,
                    out,
                    rules,
                    obs,
                    entity_id,
                    Goal::Enter { target: depot },
                );
            }
            brain
                .tracked
                .hold(entity_id, tick.saturating_add(HOLD_FRAMES));
            continue;
        }
        out.attach(
            entity_id,
            MicroKind::Retreat,
            MicroParams {
                health_floor: RETREAT_HEALTH_PERMILLE,
                standoff_cells: 4,
                radius_cells: 10,
                hold_frames: HOLD_FRAMES as u32,
            },
            post,
        );
        // The routine owns the unit while it walks it out, so the layers above
        // leave it alone rather than ordering it back into the fight.
        brain
            .tracked
            .hold(entity_id, tick.saturating_add(HOLD_FRAMES));
    }
}

/// How long a routine keeps a unit it has taken, in frames: the retreat's own
/// `hold_frames`.
const HOLD_FRAMES: u64 = 15;

/// How long a fired super waits before this brain tries it again, in frames.
///
/// The charge itself is the real gate — a launched super is not ready until it
/// recharges — so this only covers the refusal case, where the click did not
/// take (no warhead for the type, the house's wallet for a paid launch) and the
/// button stays lit. Forty seconds (600 frames at 15 a second): long enough
/// not to spend the frame budget on a refused command, short enough to catch a
/// refusal that clears.
const SUPER_RETRY_FRAMES: u64 = 600;

/// How near the delivery cell a loaded transport has to be before it opens.
const UNLOAD_CELLS: i32 = 4;

/// What a carrier does with what it is carrying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Carrying {
    /// Empty: nothing to carry, and boarding is its own decision.
    Empty,
    /// Loaded and far from where the army is going: walk there.
    Advance,
    /// Loaded and close enough: open.
    Unload,
}

/// The one answer the transport pass is built on, so the rule is testable
/// without a match: an empty carrier does nothing, a loaded one walks to the
/// delivery cell and opens inside [`UNLOAD_CELLS`] of it.
fn carrying_step(passengers: usize, distance: i32) -> Carrying {
    if passengers == 0 {
        Carrying::Empty
    } else if distance <= UNLOAD_CELLS {
        Carrying::Unload
    } else {
        Carrying::Advance
    }
}

/// One deliberate ferry for an engineer/spy. The voyage has a bounded
/// lease; it does not borrow a tank from the attacking fist and it does not
/// inspect objects hidden by the fog. A ferry is not the generic troop
/// transport which drops soldiers at the first enemy base.
#[derive(Debug, Clone, Copy)]
pub(super) struct SpecialistShuttle {
    pub(super) passenger: u64,
    target: u64,
    target_cell: (u16,u16),
    drop: (u16,u16),
    origin: (u16,u16),
    until: u64,
    boarded: bool,
    returning:bool,
    last_carrier:(u16,u16),
    last_passenger:(u16,u16),
    progress:u64,
}

const SPECIALIST_FERRY_LEASE: u64 = 3_600;
const SPECIALIST_FERRY_ORDERS: usize = 2;

fn ferry_lease_active(now: u64, until: u64, loaded: bool) -> bool {
    now < until || (loaded && now < until.saturating_add(1_200))
}

#[cfg(all(test, tests_sim_p_s))]
mod specialist_ferry_tests {
    use super::*;

    #[test]
    fn v65_retail_transport_modes_are_derived_from_rules() {
        let Some(rules)=crate::rules::retail_ini_fixture::retail_unlayered_rules() else{return;};
        if let Some(ty)=rules.object("LCRF") {
            assert_eq!(ferry_mode(&rules,ty),Some(FerryMode::Sea));
        }
        if let Some(ty)=rules.object("SHAD") {
            assert_eq!(ferry_mode(&rules,ty),Some(FerryMode::Air));
        }
        if let Some(ty)=rules.object("FV") {
            assert_eq!(ferry_mode(&rules,ty),None);
        }
    }

    #[test]
    fn a_loaded_ferry_gets_a_bounded_return_window() {
        assert!(ferry_lease_active(100, 900, false));
        assert!(!ferry_lease_active(900, 900, false));
        assert!(ferry_lease_active(900, 900, true));
        assert!(ferry_lease_active(2_099, 900, true));
        assert!(!ferry_lease_active(2_100, 900, true));
    }

    #[test]
    fn a_missing_or_refused_passenger_must_not_keep_a_permanent_ticket() {
        assert!(!ferry_lease_active(6_000, 1_000, false));
        assert!(!ferry_lease_active(6_000, 1_000, true));
    }
}


/// A transport has a ground landing requirement even if it crosses water.
/// Retail hover transports unload on land; pure water hulls must not be
/// treated as hovercraft. Armed open-topped/IFV platforms have another owner.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) enum FerryMode { Land, Sea, Air }

pub(super) fn ferry_mode(rules:&RuleSet,ty:&ObjectType)->Option<FerryMode> {
    if ty.passengers<=0 || ty.gunner || ty.open_topped || ty.spawned
        || ty.airport_bound || ty.speed<=0
        || super::pilots::Pilot::of(rules,ty)!=super::pilots::Pilot::Transport
        || !matches!(ty.category,ObjectCategory::Vehicle|ObjectCategory::Aircraft) {return None;}
    if ty.movement_zone==MovementZone::Fly || ty.jumpjet || ty.balloon_hover
        || ty.category==ObjectCategory::Aircraft {return Some(FerryMode::Air);}
    if matches!(ty.movement_zone,MovementZone::Amphibious|MovementZone::AmphibiousCrusher
        |MovementZone::AmphibiousDestroyer|MovementZone::WaterBeach) {return Some(FerryMode::Sea);}
    if ty.naval || ty.movement_zone==MovementZone::Water {return None;}
    Some(FerryMode::Land)
}

fn ferry_reachable(obs:&Observation<'_>,ty:&ObjectType,mode:FerryMode,
    from:(u16,u16),to:(u16,u16))->bool {
    obs.is_cell_in_playfield(to.0,to.1) && obs.is_ground_walkable(to.0,to.1)
        && (mode==FerryMode::Air || obs.can_reach(ty.movement_zone,from,to))
}

/// Candidate landing cells are revealed, dry, with a walkable exit for
/// cargo. The hovercraft does NOT unload in water and let native search
/// choose an arbitrary bank. Routes are conservative straight-leg checks,
/// not omniscient knowledge of native navigation.
fn ferry_posts(obs:&Observation<'_>,near:(u16,u16),rings:&[i32])->Vec<(u16,u16)> {
    let mut cells=Vec::new();
    for &r in rings {
        for (dx,dy) in [(r,0),(-r,0),(0,r),(0,-r),(r,r),(-r,r),(r,-r),(-r,-r)] {
            let (Ok(x),Ok(y))=(u16::try_from(i32::from(near.0)+dx),
                u16::try_from(i32::from(near.1)+dy)) else{continue;};
            if obs.is_cell_revealed(x,y) && obs.is_cell_in_playfield(x,y)
                && obs.is_ground_walkable(x,y) && !cells.contains(&(x,y)) {
                cells.push((x,y));
            }
        }
    }
    cells
}

fn ferry_leg(rules:&RuleSet,obs:&Observation<'_>,id:u64,
    visible:&[EnemyEntity],from:(u16,u16),to:(u16,u16))->bool {
    super::safety::safe_leg(&super::safety::envelopes(rules,obs,id,visible,true),from,to,false)
}

pub(super) fn prospective_ferry_route(rules:&RuleSet,obs:&Observation<'_>,
    boat:&ObjectType,person:&ObjectType,factory:(u16,u16),start:(u16,u16),
    site:EnemyEntity,visible:&[EnemyEntity])->bool {
    let Some(mode)=ferry_mode(rules,boat) else{return false;};
    let entries=ferry_posts(obs,factory,&[2,4,6,8,10]);
    let pickups=ferry_posts(obs,start,&[0,2,4,6]);
    let drops=ferry_posts(obs,site.cell,&[6,8,10,12]);
    pickups.into_iter().filter(|p|obs.can_reach(person.movement_zone,start,*p))
        .take(8).any(|pickup|{
            entries.iter().any(|entry|ferry_reachable(obs,boat,mode,*entry,pickup))
                && super::site_risk::prospective_ground_leg(rules,obs,person,visible,start,pickup)
                && drops.iter().any(|drop|obs.is_cell_visible(drop.0,drop.1)
                    && ferry_reachable(obs,boat,mode,pickup,*drop)
                    && building_approachable(obs,person.movement_zone,*drop,site.cell)
                    && super::site_risk::prospective_ground_leg(rules,obs,boat,visible,pickup,*drop)
                    && super::site_risk::prospective_ground_leg(rules,obs,person,visible,*drop,site.cell))
        })
}

fn ferry_destination(rules:&RuleSet,obs:&Observation<'_>,passenger:u64,
    carrier:u64,from:(u16,u16),site:EnemyEntity,capture:bool,
    visible:&[EnemyEntity])->Option<((u16,u16),(u16,u16))> {
    let person=obs.entity_type(rules,passenger)?;
    let boat=obs.entity_type(rules,carrier)?;
    let mode=ferry_mode(rules,boat)?;
    let start=obs.object_cell_and_type(passenger)?.0;
    let walking=building_approachable(obs,person.movement_zone,start,site.cell);
    let direct=cell_distance(start,site.cell);
    if direct<20 || (mode==FerryMode::Land && (!walking || boat.speed<=person.speed))
        || (walking && direct<36) {return None;}
    let mut pickups=ferry_posts(obs,start,&[0,2,4,6,8]);
    pickups.retain(|p|ferry_reachable(obs,boat,mode,from,*p)
        && obs.can_reach(person.movement_zone,start,*p)
        && ferry_leg(rules,obs,passenger,visible,start,*p)
        && ferry_leg(rules,obs,carrier,visible,from,*p));
    pickups.sort_by_key(|p|(cell_distance(start,*p)+cell_distance(from,*p),*p));
    for pickup in pickups.into_iter().take(4) {
        let mut drops=ferry_posts(obs,site.cell,&[6,8,10,12]);
        drops.retain(|p|obs.is_cell_visible(p.0,p.1)
            && ferry_reachable(obs,boat,mode,pickup,*p)
            && ferry_leg(rules,obs,carrier,visible,pickup,*p)
            && ferry_leg(rules,obs,passenger,visible,*p,site.cell)
            && building_approachable(obs,person.movement_zone,*p,site.cell)
            && !visible.iter().any(|e|cell_distance(e.cell,*p)<=6
                && obs.can_hurt(rules,e.entity_id,passenger)));
        // A ground ferry must actually save time even after walking to the
        // pickup and waiting for the carrier. Do not taxi a faster tank.
        drops.retain(|p|!walking ||
            (i64::from(cell_distance(start,pickup)+cell_distance(*p,site.cell)+6)
                *i64::from(boat.speed)
                + i64::from(cell_distance(from,pickup)+cell_distance(pickup,*p))
                *i64::from(person.speed)) * 4
                < i64::from(direct)*i64::from(boat.speed)*3);
        if !capture {
            // A lone combat passenger is only reinforcement, not an
            // amphibious invasion. Require existing safe local allies.
            drops.retain(|p|obs.own_entities().filter(|u|u.orderable
                && u.entity_id!=passenger && u.entity_id!=carrier
                && cell_distance(u.cell,*p)<=12
                && obs.object_type(rules,u.type_id).is_some_and(|ty|
                    !ty.naval && !ty.jumpjet && !ty.balloon_hover
                    && matches!(ty.category,ObjectCategory::Infantry|ObjectCategory::Vehicle)
                    && is_combat_unit(rules,ty))).take(2).count()>=2);
        }
        if let Some(drop)=drops.into_iter().min_by_key(|p|
            (cell_distance(*p,site.cell)+cell_distance(pickup,*p)/4,*p)) {
            return Some((pickup,drop));
        }
    }
    None
}

/// One task owner for land, hovercraft and air ferries. Plan a pickup and
/// destination BEFORE boarding, watch actual cargo, and return if the
/// destination becomes unsafe. An expired loaded ticket is not released
/// to arbitrary generic transport movement.
pub(super) fn specialist_transports(
    brain:&mut AssistBrain,rules:&RuleSet,obs:&Observation<'_>,out:&mut Outbox,
) {
    let now=obs.tick();
    if now%15!=0 {return;}
    if brain.specialist_shuttles.is_empty() && now%60!=0 {return;}
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let active:Vec<_>=brain.specialist_shuttles.iter().map(|(id,p)|(*id,*p)).collect();
    for (carrier,mut plan) in active {
        if !obs.owns_object(carrier) || obs.owned_by_hand(carrier)
            || obs.health(carrier).is_none_or(|hp|hp<=0) {
            brain.specialist_shuttles.remove(&carrier);continue;
        }
        let loaded=obs.inside_transport(plan.passenger)==Some(carrier);
        let Some(boat)=obs.entity_type(rules,carrier) else{continue;};
        let Some(mode)=ferry_mode(rules,boat) else{brain.specialist_shuttles.remove(&carrier);continue;};
        let Some((cell,_))=obs.object_cell_and_type(carrier) else{continue;};
        if plan.boarded && !loaded {
            brain.specialist_shuttles.remove(&carrier);
            brain.tracked.cancel_for_safety(plan.passenger);
            if obs.owns_object(plan.passenger) && !obs.owned_by_hand(plan.passenger)
                && obs.health(plan.passenger).is_some_and(|hp|hp>0) {
                log::info!("auto play: v65_ferry_delivered tick={now} carrier={carrier} passenger={} target={} returned={}",
                    plan.passenger,plan.target,plan.returning);
                if !plan.returning && obs.entity_type(rules,plan.passenger).is_some_and(|ty|ty.engineer)
                    && brain.asset_ops.permits_capture(plan.passenger,plan.target)
                    && obs.observed_object_cell(plan.target).is_some()
                    && obs.can_take_building(rules,plan.passenger,plan.target)
                    && super::safety::specialist_approach_safe(rules,obs,plan.passenger,plan.target)
                {
                    if let Some((at,_))=obs.object_cell_and_type(plan.passenger) {
                        if issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
                            plan.passenger,TrackedKind::Capture,Some(plan.target),Some(plan.target_cell),
                            Goal::Capture{target:plan.target}) {
                            brain.tracked.tasks.claim(now,plan.passenger,JobKind::Engineer,
                                Some(plan.target),Some(plan.target_cell),at);
                            brain.asset_ops.submitted(now,plan.passenger,plan.target);
                        }
                    }
                }
            }
            continue;
        }
        if !obs.owns_object(plan.passenger) || obs.owned_by_hand(plan.passenger)
            || obs.health(plan.passenger).is_none_or(|hp|hp<=0) {
            brain.specialist_shuttles.remove(&carrier);continue;
        }
        let person_cell=obs.object_cell_and_type(plan.passenger).map(|v|v.0).unwrap_or(cell);
        if cell!=plan.last_carrier || person_cell!=plan.last_passenger || loaded!=plan.boarded {
            plan.progress=now;plan.last_carrier=cell;plan.last_passenger=person_cell;
        }
        let target_live=obs.observed_object_cell(plan.target).is_some()
            && !obs.owns_object(plan.target);
        let safe_drop=obs.is_cell_visible(plan.drop.0,plan.drop.1)
            && ferry_reachable(obs,boat,mode,cell,plan.drop)
            && ferry_leg(rules,obs,carrier,&visible,cell,plan.drop)
            && ferry_leg(rules,obs,plan.passenger,&visible,plan.drop,plan.target_cell);
        let stalled=now.saturating_sub(plan.progress)>=450;
        if now>=plan.until || !target_live || !safe_drop || stalled {
            if !loaded {
                brain.specialist_shuttles.remove(&carrier);
                brain.tracked.cancel_for_safety(plan.passenger);
                brain.tracked.cancel_for_safety(carrier);
                // A cancelled native Enter/Move still belongs to this ticket.
                if !ordered_this_frame(out,plan.passenger) {
                    let _=out.want(plan.passenger,0,Goal::Stop);
                }
                if !ordered_this_frame(out,carrier) {let _=out.want(carrier,0,Goal::Stop);}
                brain.ferry_retry.insert(carrier,now.saturating_add(600));
                continue;
            }
            plan.returning=true;
        }
        if loaded {
            plan.boarded=true;
            let drop=if !plan.returning {Some(plan.drop)}else{
                std::iter::once(plan.origin).chain(ferry_posts(obs,cell,&[0,2,4,6,8]))
                    .filter(|p|ferry_reachable(obs,boat,mode,cell,*p)
                        && ferry_leg(rules,obs,carrier,&visible,cell,*p)
                        && !hostile_mining_ground(brain,*p)
                        && !visible.iter().any(|e|cell_distance(e.cell,*p)<=8
                            && obs.can_hurt(rules,e.entity_id,plan.passenger)))
                    .min_by_key(|p|(cell_distance(*p,plan.origin),*p))
            };
            if let Some(drop)=drop {
                if !ordered_this_frame(out,carrier) {
                    let near=cell_distance(cell,drop)<=1 && obs.is_ground_walkable(cell.0,cell.1);
                    if near && brain.census.units.iter().any(|u|u.entity_id==carrier
                        && u.mission.known()==Some(MissionType::Unload)) {
                        // Native unloading owns its animation and tether.
                    }else if near || obs.destination(carrier)!=Some(drop) {
                        let goal=if near{Goal::Unload}else{Goal::Move{cell:drop}};
                        let _=issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
                            carrier,if near{TrackedKind::Enter}else{TrackedKind::Move},
                            None,Some(drop),goal);
                    }
                }
            }else if !ordered_this_frame(out,carrier) {
                let _=out.want(carrier,0,Goal::Stop);
            }
            // After a failed return, retry from the next real observation;
            // never drop a live cargo ticket solely because a clock expired.
            if stalled && now%450==0 {
                log::info!("auto play: v65_ferry_return_wait tick={now} carrier={carrier}");
            }
        }else {
            let Some(person)=obs.entity_type(rules,plan.passenger) else{continue;};
            if !ferry_reachable(obs,boat,mode,cell,plan.origin)
                || !ferry_leg(rules,obs,carrier,&visible,cell,plan.origin)
                || !obs.can_reach(person.movement_zone,person_cell,plan.origin)
                || !ferry_leg(rules,obs,plan.passenger,&visible,person_cell,plan.origin) {
                plan.until=now;brain.specialist_shuttles.insert(carrier,plan);continue;
            }
            if cell_distance(cell,plan.origin)>1 {
                if !ordered_this_frame(out,carrier) && obs.destination(carrier)!=Some(plan.origin) {
                    let _=issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,carrier,
                        TrackedKind::Move,None,Some(plan.origin),Goal::Move{cell:plan.origin});
                }
            }else if !ordered_this_frame(out,plan.passenger)
                && obs.can_enter_transport(rules,plan.passenger,carrier)
                && obs.own_destination_object(plan.passenger)!=Some(carrier) {
                let _=issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
                    plan.passenger,TrackedKind::Enter,Some(carrier),Some(cell),Goal::Enter{target:carrier});
            }
        }
        brain.specialist_shuttles.insert(carrier,plan);
    }
    brain.ferry_retry.retain(|id,end|obs.owns_object(*id)&&now<*end);
    if now%60!=0 || brain.specialist_shuttles.len()>=2
        || brain.director.under_attack_now() || brain.director.recovering_under_fire() {return;}
    let mut sites:Vec<_>=obs.capturable_buildings(rules)
        .filter(|e|obs.is_cell_visible(e.cell.0,e.cell.1)
            && brain.asset_ops.permits_capture(0,e.entity_id)
            && super::asset_ops::capture_opportunity(brain,rules,obs,e,&visible))
        .collect();
    let home=brain.census.base_cell.unwrap_or((0,0));
    sites.sort_by_key(|e|(std::cmp::Reverse(super::asset_ops::capture_priority(
        rules,obs,e,home,brain.census.refineries<2)),e.entity_id));
    sites.truncate(12);
    let mut objectives:Vec<_>=sites.into_iter().map(|e|(e,true)).collect();
    if let Some((target,_,_))=brain.push_target {
        if let Some(e)=visible.iter().find(|e|e.entity_id==target
            && !reclamation_reserved(rules,obs,&brain.census,&brain.tracked,**e)) {
            objectives.push((*e,false));
        }
    }
    let carriers:Vec<_>=brain.census.units.iter().filter(|u|u.controllable()
        && !u.in_service() && u.health_permille>=700
        && !brain.specialist_shuttles.contains_key(&u.entity_id)
        && !brain.ferry_retry.contains_key(&u.entity_id)
        && obs.passengers(u.entity_id)==0 && !ordered_this_frame(out,u.entity_id)
        && !brain.tracked.held(now,u.entity_id)
        && !brain.census.factory_handoffs.contains_key(&u.entity_id)
        && obs.entity_type(rules,u.entity_id).is_some_and(|ty|ferry_mode(rules,ty).is_some()))
        .take(8).copied().collect();
    for carrier in carriers {
        let id=carrier.entity_id;
        let mut passengers:Vec<_>=brain.census.units.iter().filter(|u|u.entity_id!=id
            && u.controllable() && !u.in_service() && u.health_permille>=700
            && !u.traits.harvester && !u.traits.yard_maker && !u.traits.aircraft
            && !u.traits.irreplaceable && !u.traits.carries && !u.traits.suicide
            && (u.traits.engineer || (brain.census.main_available(u)
                && !brain.census.mobile_aa.contains(&u.entity_id)))
            && !brain.specialist_shuttles.values().any(|p|p.passenger==u.entity_id)
            && !brain.tracked.held(now,u.entity_id)
            && brain.tracked.tasks.get(u.entity_id).is_none()
            && !brain.joint_ops.reserves(u.entity_id)
            && !ordered_this_frame(out,u.entity_id)
            && obs.inside_transport(u.entity_id).is_none()
            && obs.attack_target(u.entity_id).is_none()
            && cell_distance(u.cell,carrier.cell)<=24
            && obs.can_enter_transport(rules,u.entity_id,id)).collect();
        passengers.sort_by_key(|u|(!u.traits.engineer,cell_distance(u.cell,carrier.cell),u.entity_id));
        let passengers:Vec<_>=passengers.into_iter().take(6).copied().collect();
        for u in passengers {
            for &(site,capture) in &objectives {
                if capture!=u.traits.engineer || (capture && !obs.can_take_building(rules,u.entity_id,site.entity_id))
                    || (!capture && !obs.can_hurt(rules,u.entity_id,site.entity_id))
                    || brain.specialist_shuttles.values().any(|p|p.target==site.entity_id)
                {continue;}
                let Some((origin,drop))=ferry_destination(rules,obs,u.entity_id,id,
                    carrier.cell,site,capture,&visible) else{continue;};
                brain.specialist_shuttles.insert(id,SpecialistShuttle{
                    passenger:u.entity_id,target:site.entity_id,target_cell:site.cell,drop,origin,
                    until:now.saturating_add(SPECIALIST_FERRY_LEASE),boarded:false,returning:false,
                    last_carrier:carrier.cell,last_passenger:u.cell,progress:now,
                });
                brain.census.unavailable.insert(id);
                brain.census.unavailable.insert(u.entity_id);
                brain.tracked.cancel_for_safety(u.entity_id);
                log::info!("auto play: v65_ferry_plan tick={now} carrier={id} passenger={} target={} pickup={origin:?} drop={drop:?}",
                    u.entity_id,site.entity_id);
                return;
            }
        }
    }
}

/// Recover already-loaded unassigned carriers (e.g. after save/load).
/// Do not board random idle infantry or drive to the lexicographically
/// first enemy cell. Actual cargo is unloaded on a safe local ground post.
pub(super) fn transports(brain:&mut AssistBrain,rules:&RuleSet,obs:&Observation<'_>,out:&mut Outbox) {
    if obs.tick()%30!=0 {return;}
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let carriers:Vec<_>=brain.census.units.iter().filter(|u|u.controllable() && !u.in_service()
        && !brain.specialist_shuttles.contains_key(&u.entity_id)
        && obs.passengers(u.entity_id)>0 && !ordered_this_frame(out,u.entity_id))
        .take(4).copied().collect();
    for u in carriers {
        let id=u.entity_id;
        let Some(ty)=obs.entity_type(rules,id) else{continue;};
        let Some(mode)=ferry_mode(rules,ty) else{continue;};
        if u.mission.known()==Some(MissionType::Unload) {continue;}
        let cargo:Vec<_>=obs.own_entities().filter(|p|obs.inside_transport(p.entity_id)==Some(id)).collect();
        if cargo.is_empty() || cargo.iter().any(|p|obs.owned_by_hand(p.entity_id)) {continue;}
        let post=ferry_posts(obs,u.cell,&[0,2,4,6,8,12]).into_iter().filter(|p|
            ferry_reachable(obs,ty,mode,u.cell,*p) && ferry_leg(rules,obs,id,&visible,u.cell,*p)
            && !hostile_mining_ground(brain,*p)
            && !visible.iter().any(|e|cell_distance(e.cell,*p)<=9
                && cargo.iter().any(|passenger|obs.can_hurt(rules,e.entity_id,passenger.entity_id))))
            .min_by_key(|p|(cell_distance(u.cell,*p),*p));
        if let Some(post)=post {
            if cell_distance(u.cell,post)<=1 && obs.is_ground_walkable(u.cell.0,u.cell.1) {
                let _=issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,id,
                    TrackedKind::Enter,None,Some(post),Goal::Unload);
            }else if obs.destination(id)!=Some(post) {
                let _=issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,id,
                    TrackedKind::Move,None,Some(post),Goal::Move{cell:post});
            }
        }
    }
}

/// A bounded naval commander. Use actual weapon answers and safe water
/// firing positions instead of sailing to the ground object's coordinate.
pub(super) fn naval_operations(brain:&mut AssistBrain,rules:&RuleSet,
    obs:&Observation<'_>,out:&mut Outbox) {
    if obs.tick()%15!=0 {return;}
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let mut assigned=Vec::new();
    let ships:Vec<_>=brain.census.units.iter().filter(|u|u.slot==super::census::Slot::Ship
        && brain.census.available(u) && u.health_permille>=600
        && !u.in_service() && !brain.specialist_shuttles.contains_key(&u.entity_id)
        && !brain.tracked.held(obs.tick(),u.entity_id)).take(16).copied().collect();
    for u in ships {
        let id=u.entity_id;
        if ordered_this_frame(out,id) || obs.passengers(id)>0 {continue;}
        let Some(ty)=obs.entity_type(rules,id) else{continue;};
        if ferry_mode(rules,ty).is_some(){continue;}
        let mut targets:Vec<_>=visible.iter().filter_map(|e|{
            if !obs.can_hurt(rules,id,e.entity_id) || !admits(rules,obs,id,&Goal::Attack{target:e.entity_id})
                || obs.visible_enemy_invulnerable(e.entity_id)
                || reclamation_reserved(rules,obs,&brain.census,&brain.tracked,*e)
                || cell_distance(u.cell,e.cell)>48 {return None;}
            let enemy=obs.object_type(rules,e.type_id)?;
            let reach=super::tactics::actual_threat_reach(rules,obs,id,e.entity_id)
                .max(if enemy.category==ObjectCategory::Building {
                    super::composition::siege_range(rules,ty)
                }else{0});
            let ready=obs.engage(rules,id,e.entity_id)==crate::sim::control::observation::Engage::Ready;
            if reach<=0 && !ready{return None;}
            let priority=if enemy.naval {0}else if enemy.is_base_defense {1}
                else if enemy.category==ObjectCategory::Building {2}else{3};
            Some((!ready,priority,cell_distance(u.cell,e.cell),e.entity_id,*e,reach))
        }).collect();
        targets.sort_by_key(|(ready,priority,distance,id,_,_)|(*ready,*priority,*distance,*id));
        let guns=super::safety::envelopes(rules,obs,id,&visible,true);
        for (not_ready,_,_,target,e,reach) in targets.into_iter().take(8) {
            if !not_ready {
                if obs.attack_target(id)==Some(target){break;}
                if issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,id,
                    TrackedKind::Attack,Some(target),Some(e.cell),Goal::Attack{target}) {
                    assigned.push((id,u.cell));break;
                }
                continue;
            }
            let mut best=None;
            for r in [(reach-1).clamp(2,24),(reach-3).clamp(1,22)] {
                for (dx,dy) in [(r,0),(-r,0),(0,r),(0,-r),(r*2/3,r*2/3),
                    (-r*2/3,r*2/3),(r*2/3,-r*2/3),(-r*2/3,-r*2/3)] {
                    let (Ok(x),Ok(y))=(u16::try_from(i32::from(e.cell.0)+dx),
                        u16::try_from(i32::from(e.cell.1)+dy)) else{continue;};
                    let post=(x,y);
                    if !obs.known_water_cell(x,y) || !obs.can_reach(ty.movement_zone,u.cell,post)
                        || !super::safety::safe_leg(&guns,u.cell,post,false) {continue;}
                    let crowd=assigned.iter().filter(|(_,p)|cell_distance(*p,post)<=2).count();
                    let score=(crowd,cell_distance(u.cell,post),post);
                    if best.is_none_or(|old|score<old){best=Some(score);}
                }
            }
            let Some((_,_,post))=best else{continue;};
            if obs.destination(id)==Some(post){break;}
            if issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,id,
                TrackedKind::Move,None,Some(post),Goal::Move{cell:post}) {
                assigned.push((id,post));
                log::debug!("auto play: v65_naval_post tick={} actor={id} target={target} post={post:?}",obs.tick());
                break;
            }
        }
        if assigned.len()>=4{break;}
    }
}

/// How near an idle rifleman has to be to an empty carrier to be asked aboard.
const BOARD_CELLS: i32 = 3;

/// Whether an empty carrier with room asks a rifleman standing this close
/// aboard. One line, so the rule is testable without a match.
fn wants_board(capacity: i32, passengers: usize, distance: i32) -> bool {
    capacity > 0 && passengers == 0 && distance <= BOARD_CELLS
}

/// Ask an idle soldier near an empty carrier aboard, one a frame.
///
/// This is the half of Transport measured harmful in 2026-10-28 (won 25636
/// became 34897): a general boarding step sent units away from where they were
/// needed. What is different here is who asks and who is asked - only an empty
/// carrier within BOARD_CELLS, only a soldier with no order and no specialist
/// job, one a frame - and the pass around it already delivers what it carries.
/// It is still a behaviour change, so it is measured on the pinned seed rather
/// than assumed.
fn board_idle(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
    carriers: &[(u64, (u16, u16), i32)],
    tick: u64,
) -> bool {
    for (carrier, cell, capacity) in carriers.iter().copied() {
        if brain.specialist_shuttles.contains_key(&carrier) { continue; }
        if capacity <= 0 || obs.passengers(carrier) != 0
            || brain.tracked.pending_enterers(tick, carrier).any(|passenger| {
                obs.inside_transport(passenger) != Some(carrier)
            })
        {
            continue;
        }
        let Some(soldier) = brain
            .census
            .units
            .iter()
            .filter(|unit| unit.entity_id != carrier)
            .filter(|unit| unit.orderable && !unit.hand && !unit.in_service())
            .filter(|unit| unit.traits.combat && !unit.traits.engineer && !unit.traits.infiltrator)
            .filter(|unit| !brain.tracked.held(tick, unit.entity_id))
            .filter(|unit| {
                obs.object_type(rules, unit.type_id).is_some_and(|ty| {
                    // A rifleman is the Deploy pilot in the table (a G.I. digs
                    // in), and he is exactly who a carrier should ask.
                    matches!(
                        super::pilots::Pilot::of(rules, ty),
                        super::pilots::Pilot::Engage | super::pilots::Pilot::Deploy
                    )
                })
            })
            .filter(|unit| {
                wants_board(
                    capacity,
                    obs.passengers(carrier),
                    super::threats::distance(unit.cell, cell),
                )
            })
            .min_by_key(|unit| (super::squared_distance(unit.cell, cell), unit.entity_id))
        else {
            continue;
        };
        let soldier_id = soldier.entity_id;
        if !obs.can_enter_transport(rules, soldier_id, carrier) {
            continue;
        }
        if issue_tracked(
            &mut brain.tracked, brain.trace, out, rules, obs,
            soldier_id, TrackedKind::Enter, Some(carrier), Some(cell),
            Goal::Enter { target: carrier },
        ) {
            log::debug!("auto play: {soldier_id} boards {carrier} at {cell:?}");
            return true;
        }
    }
    false
}

/// Where a carrier should open: the push's target when there is one, else the
/// nearest hostile the house knows about.
fn delivery_cell(brain: &AssistBrain) -> Option<(u16, u16)> {
    brain
        .census
        .enemies
        .iter()
        .map(|enemy| enemy.cell)
        .min_by_key(|cell| (cell.0, cell.1))
}

#[cfg(all(test, tests_sim_p_s))]
mod transport_tests {
    use super::*;

    #[test]
    fn a_carrier_walks_to_the_delivery_and_opens_there() {
        assert_eq!(carrying_step(0, 2), Carrying::Empty);
        assert_eq!(carrying_step(2, 30), Carrying::Advance);
        assert_eq!(carrying_step(2, 3), Carrying::Unload);
    }

    /// An empty carrier with room asks a soldier standing on it aboard, and
    /// nobody else: a loaded carrier, one with no seats, and a soldier across
    /// the yard are all no.
    #[test]
    fn boarding_is_only_for_an_empty_carrier_beside_a_soldier() {
        assert!(wants_board(5, 0, 2));
        assert!(!wants_board(5, 1, 2), "already carrying someone");
        assert!(!wants_board(0, 0, 2), "no seats");
        assert!(!wants_board(5, 0, 4), "too far to ask");
    }
}

/// Fire the house's charged supers at the enemy, the way a player clicks the
/// button and then the target.
///
/// Native has this decision (`AI_TryFireSW @ 0x005098F0`) but gates it on the
/// house not being human (`is_controlled_by_human`), and a brain-driven house
/// is a human's: without this step it never fires one. The decision is the
/// native one's in shape — for each charged super, in the rules' order, find a
/// target — with the target read off the ledger instead of the native value
/// table: the enemy's Construction Yard when one stands, otherwise the first
/// enemy structure.
///
/// Enemy-targeting powers strike remembered hostile structures; support
/// powers pick a relevant friendly target only when a current threat exists.
/// Two-click Chronosphere transportation remains disabled until source and
/// destination can be committed as one recoverable mission.
/// A two-click Chronosphere mission, retained until the engine confirms the
/// first click by recording its source. Expiry prevents a rejected or invalid
/// second click from monopolising abilities indefinitely.
#[derive(Debug, Clone, Copy)]
pub(super) struct ChronoTrip {
    pub(super) source_type: crate::sim::intern::InternedId,
    pub(super) warp_type: crate::sim::intern::InternedId,
    source: (u16, u16),
    destination: (u16, u16),
    earliest: u64,
    deadline: u64,
    pub(super) destination_sent_at: Option<u64>,
}

fn chrono_partner<'r>(
    rules: &'r RuleSet, source_name: &str,
) -> Option<&'r str> {
    let source = rules.super_weapon(source_name)?;
    if source.kind != SuperWeaponKind::ChronoSphere
        || !source.pre_click || source.post_click
    { return None; }
    if rules.ares_form() {
        let warp_name = crate::sim::superweapon::ares_chrono_warp_for(rules, source)?;
        let warp = rules.super_weapon(warp_name)?;
        // Ares paid two-stage pairs require an explicit sequence ticket; do
        // not send two unbound Launch intents and pretend the warp succeeded.
        if crate::sim::superweapon::paid_pair_source(rules, warp).is_some() {
            return None;
        }
        return Some(warp_name);
    }
    let index = i32::try_from(rules.super_weapon_order.iter()
        .position(|name| name == source_name)?).ok()?;
    rules.super_weapon_order.iter().find(|name| rules.super_weapon(name)
        .is_some_and(|dest| dest.kind == SuperWeaponKind::ChronoWarp
            && dest.post_click && dest.pre_dependent == index))
        .map(String::as_str)
}

/// A teleporting assault is only useful if the 3x3 source contains a real
/// armoured group and the destination has a safe landing area near a known
/// enemy asset. Do not select harvesters, MCVs or unarmed transports.
fn chrono_assault_cells(
    brain: &AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
) -> Option<((u16,u16), (u16,u16))> {
    use super::census::Slot;
    let (group, source) = brain.census.units.iter()
        .filter(|unit| unit.slot == Slot::Vehicle && unit.controllable()
            && unit.traits.combat && !unit.traits.harvester && !unit.traits.yard_maker
            && !unit.in_service())
        .filter_map(|leader| {
            let group: Vec<_> = brain.census.units.iter()
                .filter(|mate| mate.slot == Slot::Vehicle && mate.controllable()
                    && mate.traits.combat && !mate.traits.harvester && !mate.traits.yard_maker
                    && super::advance::cell_distance(mate.cell, leader.cell) <= 1)
                .collect();
            (group.len() >= 2).then_some((group.len(), group.iter()
                .map(|unit| unit.points.max(0)).sum::<i32>(), leader.cell))
        })
        .max_by_key(|(n, points, cell)| (*n, *points, std::cmp::Reverse(*cell)))
        .map(|(n,_,cell)| (n,cell))?;
    if group < 2 { return None; }
    let target = brain.census.enemies.iter()
        .filter(|enemy| obs.is_cell_visible(enemy.cell.0, enemy.cell.1))
        .filter_map(|enemy| {
            let ty = obs.object_type(rules, enemy.type_id)?;
            (ty.category == ObjectCategory::Building
                && (ty.build_const_eligible() || ty.resource_destination || ty.is_base_defense))
                .then_some((enemy.cell, (ty.build_const_eligible(), ty.resource_destination,
                    ty.is_base_defense, ty.strength)))
        })
        .max_by_key(|(cell, weight)| (*weight, std::cmp::Reverse(*cell)))?.0;
    let offsets = [(-7,0),(7,0),(0,-7),(0,7),(-6,-5),(-6,5),(6,-5),(6,5)];
    let destination = offsets.into_iter().filter_map(|(dx,dy)| {
        let (Ok(x), Ok(y)) = (
            u16::try_from(i32::from(target.0)+dx),
            u16::try_from(i32::from(target.1)+dy),
        ) else { return None; };
        let cell = (x,y);
        if !obs.is_cell_in_playfield(x,y) || !obs.is_ground_walkable(x,y)
            || super::advance::cell_distance(source, cell) <= 18
        { return None; }
        let danger = obs.visible_enemy_entities().filter(|enemy| {
            super::advance::cell_distance(enemy.cell,cell) <= 6
        }).count();
        // A clear landing zone is part of the attack plan, not a gamble on
        // teleporting an entire heavy tank column into a friendly-fire blast.
        (danger <= 1).then_some((danger,cell))
    }).min_by_key(|(danger,cell)| (*danger,*cell))?.1;
    Some((source,destination))
}

fn complete_chrono_trip(
    brain: &mut AssistBrain, obs: &Observation<'_>, out: &mut Outbox,
) -> bool {
    let Some(mut trip) = brain.chrono_trip else { return false; };
    if obs.tick() > trip.deadline {
        brain.chrono_trip = None;
        return false;
    }
    if let Some(sent) = trip.destination_sent_at {
        // A post-click command is a single shot. Don't keep repeating it
        // every 30 ticks just because the first Super retains chrono_cell.
        // ControlBoard will independently report an L2 refusal for retry.
        if obs.tick().saturating_sub(sent) >= 3 {
            brain.chrono_trip = None;
            return false;
        }
        return true;
    }
    // A PreClick is supposed to *consume source readiness*. Thus requiring
    // the source to appear in ready_super_weapons here silently kills every
    // legitimate two-stage mission after the first native click.
    if obs.tick() >= trip.earliest
        && obs.chrono_source_cell(trip.source_type) == Some(trip.source)
        && brain.super_at.get(&trip.warp_type).is_none_or(|at| obs.tick() >= *at)
    {
        out.intent(Intent::LaunchSuperWeapon {
            sw_type_id: trip.warp_type, cell: trip.destination,
        });
        trip.destination_sent_at = Some(obs.tick());
        brain.chrono_trip = Some(trip);
        brain.super_at.insert(trip.warp_type, obs.tick().saturating_add(30));
    }
    true
}

pub(super) fn supers(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    // PostClick is allowed even though the first PreClick consumed the only
    // ready superweapon. Check the pending pair before the empty-ready gate.
    if complete_chrono_trip(brain, obs, out) {
        return;
    }
    let ready = obs.ready_super_weapons(rules);
    if ready.is_empty() { return; }
    let tick = obs.tick();
    let mut fired = 0;
    for name in &rules.super_weapon_order {
        if fired >= SUPERS_PER_FRAME {
            break;
        }
        let Some(type_id) = obs.type_id(name) else {
            continue;
        };
        if !ready.contains(&type_id) {
            continue;
        }
        if brain.super_at.get(&type_id).is_some_and(|at| tick < *at) {
            continue;
        }
        let Some(super_weapon) = rules.super_weapon(name) else {
            continue;
        };
        // The lobby's SuperWeapons checkbox only disables powers explicitly
        // marked disableable by the rules. American paradrops and some other
        // support powers remain legal even when offensive supers are off.
        if !auto_power_supported(super_weapon.kind)
            || !allowed_by_shell(obs.super_weapons(), super_weapon.disableable_from_shell)
        {
            continue;
        }
        // Ares Money.Amount is a real wallet transaction (negative = cost),
        // not a harmless activation. Never spend the credits promised to the
        // income/yard recovery plan on a support power.
        if !super_launch_affordable(
            super_weapon.money_amount, obs.credits(),
            brain.yard_reserve.max(if brain.director.recovering_income() { 3_000 } else { 750 }),
        ) {
            continue;
        }
        // Chronosphere is a source click followed by a post-click Warp. Do
        // not treat it as an ordinary single-target superweapon.
        if super_weapon.kind == SuperWeaponKind::ChronoSphere {
            let Some(warp_name) = chrono_partner(rules, name) else { continue; };
            let Some(warp_type) = obs.type_id(warp_name) else { continue; };
            if brain.siege_reach.is_none() && brain.census.enemy_strike_sites == 0 {
                continue;
            }
            let Some((source,destination)) = chrono_assault_cells(brain, rules, obs) else {
                continue;
            };
            brain.chrono_trip = Some(ChronoTrip {
                source_type: type_id, warp_type, source, destination,
                earliest: tick.saturating_add(1), deadline: tick.saturating_add(120),
                destination_sent_at: None,
            });
            brain.super_at.insert(type_id, tick.saturating_add(SUPER_RETRY_FRAMES));
            out.intent(Intent::LaunchSuperWeapon { sw_type_id: type_id, cell: source });
            log::info!("auto play: chrono assault from {source:?} to {destination:?}");
            return;
        }
        // Where it lands is the power's own answer, not one cell for all of
        // them: a paradrop is soldiers, and soldiers dropped into the enemy's
        // own base land inside everything that stands there.
        let cell=match super_weapon.kind {
            SuperWeaponKind::MultiMissile | SuperWeaponKind::LightningStorm
            | SuperWeaponKind::PsychicDominator if !rules.ares() && !rules.ares_form() =>
                damaging_strike_cell(brain,rules,obs,super_weapon.kind),
            _=>landing_cell(brain,rules,obs,aim_of(super_weapon.kind)),
        };
        let Some(cell)=cell else {
            brain.super_at.insert(type_id,tick.saturating_add(90));
            continue;
        };
        // Force Shield blacks out its own house. Saving one tower must not
        // shut down every powered tower, radar and production line. Only an
        // actual emergency over valuable infrastructure can justify that cost.
        if super_weapon.kind == SuperWeaponKind::ForceShield
            && rules.general.force_shield_blackout_duration > 0
            && !shield_blackout_worth_it(brain, rules, obs, cell)
        {
            continue;
        }
        log::info!("auto play: fire {name} at {cell:?}");
        brain
            .super_at
            .insert(type_id, tick.saturating_add(SUPER_RETRY_FRAMES));
        out.intent(Intent::LaunchSuperWeapon {
            sw_type_id: type_id,
            cell,
        });
        fired += 1;
    }
}

/// Negative Money.Amount is a debit; positive amounts award money. Maintain a
/// reserve even when the native command would be accepted, so a flashy launch
/// cannot bankrupt the one remaining harvester or the construction yard.
fn super_launch_affordable(money_amount: i32, credits: i32, reserve: i32) -> bool {
    // No debit means no economic opportunity cost. In particular, never
    // suppress a free American paradrop while saving for a refinery.
    if money_amount >= 0 { return true; }
    let debit = money_amount.saturating_neg();
    credits.saturating_sub(debit) >= reserve
}

/// A shield that cuts all house power is an emergency last resort. Protect
/// the home yard (or *last* refinery), not every new foreign building we have
/// just captured: trading the entire army's AA and powered towers for a remote
/// prize is normally far worse than losing the prize.
fn shield_blackout_worth_it(
    brain: &AssistBrain, rules: &RuleSet, obs: &Observation<'_>, cell: (u16, u16),
) -> bool {
    let radius = rules.general.force_shield_radius.max(1);
    let Some(home) = brain.census.base_cell else { return false; };
    obs.own_entities().any(|building| {
        let Some(ty) = obs.entity_type(rules, building.entity_id) else {
            return false;
        };
        if ty.category != ObjectCategory::Building
            || !(ty.construction_yard
                || (ty.refinery && brain.census.refineries <= 1))
            || squared_distance(building.cell, home) > 12 * 12
            || super::squared_distance(building.cell, cell) > radius * radius
        {
            return false;
        }
        let incoming: i32 = obs.incoming_shots(building.entity_id)
            .fold(0i32, |sum, shot| sum.saturating_add(shot.damage.max(0)));
        let attackers = obs.visible_enemy_entities()
            .filter(|enemy| obs.attack_target(enemy.entity_id) == Some(building.entity_id))
            .count();
        shield_emergency(building.health, ty.strength, incoming, attackers)
    })
}

fn shield_emergency(health: i32, strength: i32, incoming: i32, attackers: usize) -> bool {
    // Multiple attackers alone are NOT an emergency: a blackout disables the
    // very defensive towers that could stop them. Only lethal shots in flight
    // or a critically damaged home asset under sustained fire merit a shield.
    health > 0 && (incoming >= health
        || attackers >= 2 && incoming > 0
            && i64::from(health) * 100 <= i64::from(strength.max(1)) * 20)
}

fn allowed_by_shell(super_weapons_enabled: bool, disableable_from_shell: bool) -> bool {
    super_weapons_enabled || !disableable_from_shell
}

/// How many powers one frame fires.
///
/// One. Everything that is ready was being launched on the same frame, so a
/// house that had charged a missile and a paradrop together spent both on the
/// same second and had nothing when the fight it was saving them for arrived.
const SUPERS_PER_FRAME: usize = 1;

/// Where a power of this kind belongs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Aim {
    /// Where the enemy's base is: a missile, a storm, a dominator.
    Enemy,
    /// Where the house's own army stands: a paradrop lands soldiers, and they
    /// have to form up before they walk into anything.
    Rally,
    /// Where the house cannot see: a spy plane, a psychic reveal.
    Recon,
    /// An important building is actually under attack now.
    Shield,
    /// A fighting vehicle is in the middle of a current battle.
    Curtain,
    /// Enemy infantry worth converting without catching friendly infantry.
    Genetic,
}

/// What a power is for, by its kind: `Type=` in the rules.
fn aim_of(kind: SuperWeaponKind) -> Aim {
    match kind {
        SuperWeaponKind::ParaDrop
        | SuperWeaponKind::AmerParaDrop
        | SuperWeaponKind::UnitDelivery => Aim::Rally,
        SuperWeaponKind::SpyPlane | SuperWeaponKind::PsychicReveal => Aim::Recon,
        SuperWeaponKind::ForceShield => Aim::Shield,
        SuperWeaponKind::IronCurtain => Aim::Curtain,
        SuperWeaponKind::GeneticConverter => Aim::Genetic,
        _ => Aim::Enemy,
    }
}

/// The cell an aim resolves to this frame.
fn landing_cell(
    brain: &AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    aim: Aim,
) -> Option<(u16, u16)> {
    match aim {
        Aim::Enemy => siege_battery_cell(brain, rules, obs)
            .or_else(|| enemy_structure_cell(brain, rules, obs)),
        // Dropped where the army is, so they arrive as a squad rather than as
        // eight soldiers walking out of the enemy's own base one at a time.
        // With no army yet, the base: they are defenders until there is one.
        Aim::Rally => safe_paradrop_cell(brain, obs),
        // Enemy structures are already known; reveal a *new* part of the map
        // first. The fog is this player's own information, not all entities.
        Aim::Recon => recon_cell(obs, brain.census.base_cell)
            .or_else(|| enemy_structure_cell(brain, rules, obs)),
        Aim::Shield => threatened_building(brain, obs, rules),
        Aim::Curtain => engaged_vehicle(brain, rules, obs),
        Aim::Genetic => genetic_cluster(rules, obs),
    }
}

/// A direct hit against an immune building, or a tiny scratch against a
/// durable facility, is not equivalent to destroying a power plant. Compare
/// current health, the actual strike warhead and economic/military value.
fn strike_value(cost:i32,importance:i32,hp:i32,damage:i32)->i64 {
    if hp<=0 || damage<=0 {return 0;}
    let value=i64::from(cost.max(100)).saturating_mul(i64::from(importance.max(1)));
    let fraction=i64::from(damage.min(hp))*100/i64::from(hp);
    if fraction<20 {return 0;}
    if damage>=hp {value*3}
    else {value*fraction/100}
}

fn damaging_strike_cell(brain:&AssistBrain,rules:&RuleSet,obs:&Observation<'_>,
    kind:SuperWeaponKind)->Option<(u16,u16)> {
    let (damage,warhead,scatter)=match kind {
        SuperWeaponKind::MultiMissile=>{
            let weapon=rules.weapon("NukePayload")?;
            (weapon.damage,weapon.warhead.as_deref()?,0)
        },
        SuperWeaponKind::LightningStorm=>(rules.general.lightning_damage,
            rules.general.lightning_warhead.as_str(),(rules.general.lightning_cell_spread/2).clamp(0,32)),
        SuperWeaponKind::PsychicDominator=>(rules.general.dominator_damage,
            rules.general.dominator_warhead.as_str(),0),
        _=>return None,
    };
    let head=rules.warhead(warhead)?;
    let spread=head.cell_spread_f64.ceil().clamp(1.0,32.0) as i32;
    let radius=(spread+scatter+1).max(if kind==SuperWeaponKind::PsychicDominator {
        rules.general.dominator_capture_range.clamp(0,32)+1
    }else{1});
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    let protected:BTreeSet<_>=visible.iter().filter(|e|
        protected_neutral(rules,obs,e.entity_id)
        || reclamation_reserved(rules,obs,&brain.census,&brain.tracked,**e))
        .map(|e|e.entity_id).collect();
    let neutral_prizes:Vec<_>=obs.neutral_structures()
        .filter(|e|obs.is_cell_visible(e.cell.0,e.cell.1)
            && (protected_neutral(rules,obs,e.entity_id)
                || reclamation_reserved(rules,obs,&brain.census,&brain.tracked,*e)
                || brain.cover_detail.contains_key(&e.entity_id)))
        .map(|e|e.cell).collect();
    let mut buildings=Vec::new();
    for e in &visible {
        let Some(ty)=obs.object_type(rules,e.type_id) else{continue;};
        if ty.category!=ObjectCategory::Building || ty.immune
            || obs.visible_enemy_invulnerable(e.entity_id)
            || protected.contains(&e.entity_id) {continue;}
        let Some(hp)=obs.health(e.entity_id) else{continue;};
        let predicted=match obs.visible_strike_building_damage(rules,e.entity_id,damage,warhead) {
            Some(value)=>value,
            // A stock rule-only lower-confidence estimate is useful when
            // a build enables an unsupported receiver feature. Do not
            // claim native damage or a guaranteed kill in this fallback.
            None=>{
                let mult=f64::from_bits(head.interaction(ty.armor_id()).multiplier().bits());
                if !mult.is_finite() || mult<=0.0 {continue;}
                (f64::from(damage)*mult*0.7).clamp(0.0,f64::from(i32::MAX)) as i32
            }
        };
        let importance=if ty.power>0 {5}
            else if ty.weapons_factory || ty.factory_type().is_some() {5}
            else if ty.resource_destination {4}
            else if ty.is_base_defense && (brain.siege_reach.is_some()
                || brain.mine_incidents.contains_key(&e.entity_id)) {5}
            else if ty.build_const_eligible() {3}
            else {2};
        let score=strike_value(obs.cost_of(rules,ty),importance,hp,predicted);
        if score>0 {buildings.push((e.cell,score,e.entity_id));}
    }
    let own:Vec<_>=obs.own_entities().filter(|e|e.health>0
        && obs.inside_transport(e.entity_id).is_none()).collect();
    let mut best=None;
    for &(center,direct,id) in &buildings {
        if !obs.is_cell_in_playfield(center.0,center.1){continue;}
        // Never deliberately centre a strike on our own productive base or
        // an engineer's reserved capture. Use the larger storm warning
        // radius for friendly risk, not a promised destruction radius.
        if own.iter().any(|e|cell_distance(e.cell,center)<=radius
            && obs.object_type(rules,e.type_id).is_some_and(|ty|
                ty.category==ObjectCategory::Building || ty.engineer || ty.resource_gatherer))
            || visible.iter().any(|e|cell_distance(e.cell,center)<=radius
                && protected.contains(&e.entity_id))
            || neutral_prizes.iter().any(|at|cell_distance(*at,center)<=radius)
            {continue;}
        let friendly=own.iter().filter(|e|cell_distance(e.cell,center)<=radius)
            .fold(0i64,|sum,e|sum+obs.object_type(rules,e.type_id)
                .map_or(0,|ty|i64::from(obs.cost_of(rules,ty).max(100))*3));
        // Neighbour value is only an approximate bonus. Never count every
        // building in the nominal spread as a direct, lethal centre hit.
        let collateral=buildings.iter().filter(|(_,_,other)|*other!=id)
            .filter(|(cell,_,_)|cell_distance(*cell,center)<=spread)
            .fold(0i64,|sum,(_,score,_)|sum+score/5);
        let score=direct+collateral-friendly;
        if score<=0 {continue;}
        let rank=(score,std::cmp::Reverse(id),center);
        if best.is_none_or(|old|rank>old){best=Some(rank);}
    }
    if let Some((score,_,cell))=best {
        log::info!("auto play: v63_strike_target tick={} kind={kind:?} cell={cell:?} score={score}",obs.tick());
        Some(cell)
    }else{None}
}

/// Place airborne reinforcements on passable *friendly staging ground*, not
/// on the coordinate of the first fighter (which may be an aircraft, in
/// limbo, or fighting inside the enemy base). Prefer the attack squad's
/// rearward safe ground, then the base's established muster cell.
fn safe_paradrop_cell(brain: &AssistBrain, obs: &Observation<'_>) -> Option<(u16,u16)> {
    let safe = |cell: (u16,u16)| {
        obs.is_cell_in_playfield(cell.0,cell.1)
            && (!obs.has_ground_grid() || obs.is_ground_walkable(cell.0,cell.1))
            && !brain.census.enemies.iter().any(|enemy| {
                obs.is_cell_visible(enemy.cell.0,enemy.cell.1)
                    && squared_distance(enemy.cell,cell) <= 10 * 10
            })
    };
    let attack = brain.squads.squad(super::squads::Role::Attack);
    let on_front = brain.census.units.iter()
        .filter(|unit| unit.slot != super::census::Slot::Aircraft
            && unit.traits.combat && unit.controllable())
        .filter(|unit| attack.is_some_and(|squad| squad.members.contains(&unit.entity_id)))
        .filter_map(|unit| {
            super::census::rally_near(obs,unit.cell,&brain.census.home_cells)
        })
        .find(|cell| safe(*cell));
    on_front
        .or_else(|| brain.census.rally.filter(|cell| safe(*cell)))
        .or_else(|| brain.census.base_cell.and_then(|home| {
            super::census::rally_near(obs,home,&brain.census.home_cells)
                .filter(|cell| safe(*cell))
        }))
}

/// Only single-click powers with a safe, currently meaningful target are
/// autonomous. Chronosphere/ChronoWarp need a paired source/destination
/// transaction, so firing either as a single click would consume the charge.
fn auto_power_supported(kind: SuperWeaponKind) -> bool {
    spent_on_the_enemy(kind)
        || matches!(kind, SuperWeaponKind::ForceShield | SuperWeaponKind::IronCurtain
            | SuperWeaponKind::ChronoSphere)
}

/// Pick one still-shrouded *walkable* sample, farthest from home. Spy planes
/// need not be driven there, but using real terrain avoids repeatedly
/// revealing the padded fog outside the map. Stable integer ordering avoids
/// replay divergence and no live enemy coordinates are read through fog.
fn recon_cell(obs: &Observation<'_>, home: Option<(u16, u16)>) -> Option<(u16, u16)> {
    let (width, height) = obs.map_extent();
    let home = home.unwrap_or((width / 2, height / 2));
    let mut best: Option<(i32, (u16, u16))> = None;
    for y in (0..height).step_by(8) {
        for x in (0..width).step_by(8) {
            if obs.is_cell_revealed(x, y) || !obs.is_ground_walkable(x, y) {
                continue;
            }
            let cell = (x, y);
            let score = squared_distance(home, cell);
            if best.is_none_or(|(value, current)| (score, cell) > (value, current)) {
                best = Some((score, cell));
            }
        }
    }
    best.map(|(_, cell)| cell)
}

/// A mutation bomb converts infantry, not tank armour. Prefer visible enemy
/// infantry in groups, never on top of our own infantry. One fleeing soldier
/// is not worth consuming a long recharge or endangering our own front line.
fn genetic_cluster(rules: &RuleSet, obs: &Observation<'_>) -> Option<(u16,u16)> {
    let infantry: Vec<_> = obs.visible_enemy_entities()
        .filter(|unit| obs.entity_type(rules, unit.entity_id)
            .is_some_and(|ty| ty.category == ObjectCategory::Infantry))
        .map(|unit| unit.cell).collect();
    infantry.iter().copied().filter_map(|center| {
        let enemies = infantry.iter()
            .filter(|other| super::advance::cell_distance(**other,center) <= 3).count();
        if enemies < 3 { return None; }
        let friendlies = obs.own_entities()
            .filter(|unit| obs.object_type(rules,unit.type_id)
                .is_some_and(|ty| ty.category == ObjectCategory::Infantry))
            .filter(|unit| super::advance::cell_distance(unit.cell, center) <= 5).count();
        (friendlies == 0).then_some((enemies,center))
    }).max_by_key(|(n,cell)| (*n, std::cmp::Reverse(*cell)))
        .map(|(_,cell)| cell)
}

/// The Force Shield goes over the threatened *structure*, never an arbitrary
/// empty base location or an enemy. Otherwise it stays charged.
fn threatened_building(brain: &AssistBrain, obs: &Observation<'_>, rules: &RuleSet)
    -> Option<(u16, u16)>
{
    obs.own_entities()
        .filter_map(|entity| {
            let ty = obs.object_type(rules, entity.type_id)?;
            if ty.category != ObjectCategory::Building
                || !(ty.construction_yard
                    || ty.refinery && brain.census.refineries <= 1)
                || !brain.census.base_cell.is_some_and(|home|
                    squared_distance(entity.cell, home) <= 12 * 12)
            {
                return None;
            }
            let incoming: i32 = obs.incoming_shots(entity.entity_id)
                .fold(0i32, |sum, shot| sum.saturating_add(shot.damage.max(0)));
            let attackers = obs.visible_enemy_entities()
                .filter(|enemy| obs.attack_target(enemy.entity_id) == Some(entity.entity_id))
                .count();
            shield_emergency(entity.health, ty.strength, incoming, attackers).then_some((
                (ty.construction_yard, incoming, -entity.health),
                entity.cell,
            ))
        })
        .max_by_key(|(weight, cell)| (*weight, *cell))
        .map(|(_, cell)| cell)
}

/// The Iron Curtain is for a combat vehicle actually facing an enemy. With
/// no battle in progress there is no reason to burn its charge on a tank in
/// the garage. Exclude MCVs, transports and workers via census role traits.
fn engaged_vehicle(
    brain: &AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
) -> Option<(u16, u16)> {
    brain.census.units.iter()
        .filter(|unit| unit.orderable && unit.traits.combat)
        .filter(|unit| obs.object_type(rules, unit.type_id)
            .is_some_and(|ty| ty.category == ObjectCategory::Vehicle))
        .filter_map(|unit| {
            let distance = obs.visible_enemy_entities()
                .map(|enemy| squared_distance(unit.cell, enemy.cell))
                .min()?;
            (distance <= 10 * 10).then_some((
                (unit.points, -(unit.health_permille as i32), -distance),
                unit.cell,
            ))
        })
        .max_by_key(|(weight, cell)| (*weight, *cell))
        .map(|(_, cell)| cell)
}

/// Whether a power is aimed at an enemy cell rather than held for the house's
/// own force.
fn spent_on_the_enemy(kind: SuperWeaponKind) -> bool {
    match kind {
        SuperWeaponKind::MultiMissile
        | SuperWeaponKind::LightningStorm
        | SuperWeaponKind::ParaDrop
        | SuperWeaponKind::AmerParaDrop
        | SuperWeaponKind::SpyPlane
        | SuperWeaponKind::PsychicReveal
        | SuperWeaponKind::PsychicDominator
        | SuperWeaponKind::GeneticConverter
        | SuperWeaponKind::GenericWarhead
        | SuperWeaponKind::UnitDelivery
        // Ares' seeker picks its own target among the enemy's
        // (`AiTargetingMode::HunterSeeker`, `HouseMask::ENEMIES`), so a cell
        // of theirs is still what it is fired at.
        | SuperWeaponKind::HunterSeeker
        // A VERA power: one ion blast at the cell it is fired at, so the
        // target is an enemy's.
        | SuperWeaponKind::IonCannon => true,
        // Spent on the house's own units and buildings, or on nothing.
        SuperWeaponKind::IronCurtain
        | SuperWeaponKind::ChronoSphere
        | SuperWeaponKind::ChronoWarp
        | SuperWeaponKind::ForceShield
        | SuperWeaponKind::NoEffect => false,
        // Ares powers AI_TryFireSW leaves to team scripts
        // ([`super::super::super::super::superweapon::ai_fire::AiFireArm::None`]):
        // Protect is an Iron Curtain over the house's own force, Battery and
        // Firestorm spend on its own power, SonarPulse and EMPulse act from the
        // cannon's own cell, and DropPod is a paradrop, which `aim_of` sites on
        // the army. None of them is aimed at an enemy cell here, so the policy
        // does not fire them rather than invent a target the ported AI does not.
        SuperWeaponKind::Protect
        | SuperWeaponKind::Battery
        | SuperWeaponKind::SonarPulse
        | SuperWeaponKind::EMPulse
        | SuperWeaponKind::Firestorm
        | SuperWeaponKind::DropPod => false,
    }
}

/// When artillery has pinned our main force, a charged strike is a way to
/// break the fortified line without driving short-range units into it. This
/// uses only current visible defenders near the actual campaign objective;
/// an old enemy sighting is not permission to aim a new missile precisely.
fn siege_battery_cell(
    brain: &AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
) -> Option<(u16, u16)> {
    let center = brain.push_target?.2;
    brain.siege_reach?;
    brain.census.enemies.iter().filter_map(|enemy| {
        let ty = obs.object_type(rules, enemy.type_id)?;
        if !ty.is_base_defense || !obs.is_cell_visible(enemy.cell.0, enemy.cell.1)
            || !obs.visible_enemy_building_operational(rules, enemy.entity_id)
            || super::advance::cell_distance(enemy.cell, center) > 14
        { return None; }
        Some((super::advance::weapon_range_cells(rules, ty,
            obs.is_deployed(enemy.entity_id)),ty.strength, enemy.entity_id,enemy.cell))
    }).max_by_key(|(reach,health,id,_)| (*reach,*health,*id))
        .map(|(_,_,_,cell)| cell)
}

/// The cell a super is fired at: the enemy Construction Yard when one stands,
/// otherwise the first enemy structure in storage order.
///
/// The yard is native's own most-valued target (`AI_FindBestRallyTarget`'s
/// value table puts a Construction Yard at 100 against a power plant's 60 and a
/// tank's 2), and it is the cell a single missile decides a match at.
fn enemy_structure_cell(
    brain: &AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
) -> Option<(u16, u16)> {
    let mut first = None;
    for enemy in &brain.census.enemies {
        let Some(ty) = obs.object_type(rules, enemy.type_id) else {
            continue;
        };
        if ty.category != ObjectCategory::Building {
            continue;
        }
        if ty.build_const_eligible() {
            return Some(enemy.cell);
        }
        first.get_or_insert(enemy.cell);
    }
    first
}

/// Whether an object is one these orders step around: an infantry, vehicle or
/// aircraft that is neither a gatherer nor a Construction Yard maker.
/// Buildings and terrain are not.
pub(super) fn is_combat_unit(rules: &RuleSet, ty: &ObjectType) -> bool {
    super::census::Traits::of(rules, ty).combat
}

#[cfg(all(test, tests_sim_p_s))]
mod tests;

#[cfg(all(test, tests_sim_p_s))]
mod v14_crate_runner_tests {
    use super::*;
    #[test]
    fn limit_active_crate_claims_not_potential_candidates() {
        assert!(crate_runner_available(0, CRATE_RUNNERS));
        assert!(crate_runner_available(1, CRATE_RUNNERS));
        assert!(crate_runner_available(3, CRATE_RUNNERS));
        assert!(!crate_runner_available(4, CRATE_RUNNERS));
        assert!(!crate_runner_available(5, CRATE_RUNNERS));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v15_superweapon_tests {
    use super::*;

    #[test]
    fn v63_strike_prefers_real_destruction_to_an_expensive_minor_scratch() {
        assert_eq!(strike_value(5000,2,2000,0),0);
        assert_eq!(strike_value(5000,2,2000,300),0);
        assert!(strike_value(800,5,600,900)>strike_value(5000,2,2000,600));
    }

    #[test]
    fn two_stage_chrono_source_is_autonomous_but_warp_is_not_a_first_click() {
        assert!(auto_power_supported(SuperWeaponKind::ChronoSphere));
        assert!(!auto_power_supported(SuperWeaponKind::ChronoWarp));
    }

    #[test]
    fn support_and_genetic_powers_never_target_enemy_buildings_by_default() {
        assert_eq!(aim_of(SuperWeaponKind::GeneticConverter), Aim::Genetic);
        assert_eq!(aim_of(SuperWeaponKind::IronCurtain), Aim::Curtain);
        assert_eq!(aim_of(SuperWeaponKind::ForceShield), Aim::Shield);
        assert_eq!(aim_of(SuperWeaponKind::AmerParaDrop), Aim::Rally);
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v16_objective_tests {
    use super::*;

    #[test]
    fn money_spies_are_worth_sending_even_when_our_wallet_is_rich() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let Some(refinery) = rules.object("GAREFN") else { return; };
        assert!(refinery.storage > 0, "refinery is a native money-spy target");
        assert!(cash_spy_target(&rules, refinery));
        assert_eq!(infiltration_value(&rules, refinery, 25_000), 8);
        assert_eq!(infiltration_value(&rules, refinery, 2_000), 9);
        if let Some(power) = rules.object("GAPOWR") {
            assert!(!cash_spy_target(&rules, power));
        }
    }

    #[test]
    fn occupied_public_infrastructure_remains_a_capture_priority() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let Some(oil) = rules.object("CAOILD") else { return; };
        assert!(strategic_public_asset(oil));
        assert!(capture_value(&rules, oil, false) >= 7);
        assert!(capture_value(&rules, oil, true) >= 8);
    }

    #[test]
    fn abandoned_yards_and_factories_outweigh_one_more_oil_well() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let (Some(yard), Some(factory), Some(oil)) = (
            rules.object("NACNST"), rules.object("GAWEAP"), rules.object("CAOILD"),
        ) else { return; };
        assert!(yard.build_const_eligible());
        assert!(factory.factory_type().is_some());
        assert!(capture_value(&rules, yard, false) > capture_value(&rules, oil, false));
        assert!(capture_value(&rules, factory, false) > capture_value(&rules, oil, false));
    }

    #[test]
    fn forward_garrison_does_not_exhaust_the_home_garrison_quota() {
        assert_eq!(garrison_budget(false, 0), GARRISON_MAX);
        assert_eq!(garrison_budget(true, 4), GARRISON_MAX + 4);
        let attack = MissionId::from_known(MissionType::Attack);
        let move_mission = MissionId::from_known(MissionType::Move);
        assert!(may_garrison_from_mission(attack, true, false));
        assert!(may_garrison_from_mission(move_mission, true, false));
        assert!(!may_garrison_from_mission(attack, true, true));
        assert!(!may_garrison_from_mission(attack, false, false));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v18_execution_tests {
    use super::*;

    #[test]
    fn repair_toggle_excludes_houses_that_need_an_engineer() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let Some(plant) = rules.object("GAPOWR") else { return; };
        assert!(repair_wrench_allowed(plant));
        if let Some(house) = rules.all_objects().find(|obj| obj.can_be_occupied) {
            assert!(!repair_wrench_allowed(house));
        }
    }

    #[test]
    fn only_a_truly_disguised_spy_ignores_ordinary_soldiers() {
        let guards = [(10, 10), (15, 15)];
        let watchers = [(15, 15)];
        assert_eq!(capture_route_threats(false, &guards, &watchers), &guards);
        assert_eq!(capture_route_threats(true, &guards, &watchers), &watchers);
        assert!(!path_crosses(capture_route_threats(true, &guards, &watchers),
            (0, 10), (20, 10), 2));
        assert!(path_crosses(capture_route_threats(false, &guards, &watchers),
            (0, 10), (20, 10), 2));
    }

    #[test]
    fn v60_aircraft_and_jumpjets_are_not_crate_collectors() {
        let Some(rules)=crate::rules::retail_ini_fixture::retail_unlayered_rules() else {return;};
        assert!(!crate_collector_type(rules.object("ZEP").expect("Kirov")));
        assert!(!crate_collector_type(rules.object("ORCA").expect("Harrier")));
        assert!(crate_collector_type(rules.object("MTNK").expect("Grizzly")));
        assert!(!crate_collector_type(rules.object("ENGINEER").expect("Engineer")));
    }

    #[test]
    fn fast_reserve_collects_remote_crates_but_attack_tanks_do_not() {
        assert!(remote_crate_runner(true, 6, false));
        assert!(remote_crate_runner(false, 9, false));
        assert!(!remote_crate_runner(false, 8, false));
        assert!(!remote_crate_runner(false, 9, true));
    }

    #[test]
    fn dedicated_scouts_can_pursue_crates_across_the_seen_map() {
        assert!(crate_runner_in_range(true, 120 * 120, CRATE_SEEK_CELLS));
        assert!(!crate_runner_in_range(false, 120 * 120, CRATE_SEEK_CELLS));
        assert!(crate_runner_in_range(false, 4 * 4, CRATE_SEEK_CELLS));
    }

    #[test]
    fn a_box_seen_one_wave_ago_does_not_hold_a_courier_forever() {
        assert!(crate_sighting_fresh(300, 1));
        assert!(!crate_sighting_fresh(CRATE_MEMORY_FRAMES + 2, 1));
        assert!(CRATE_CLAIM_FRAMES < 1_800);
    }

    #[test]
    fn forward_houses_take_all_their_seats_with_a_bounded_detail() {
        assert_eq!(garrison_budget(false, 30), GARRISON_MAX);
        assert_eq!(garrison_budget(true, 12), GARRISON_MAX + 12);
        assert_eq!(garrison_budget(true, 100), GARRISON_MAX + 32);
    }

    #[test]
    fn both_sides_can_capture_a_public_oil_asset() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let Some(oil) = rules.object("CAOILD") else { return; };
        assert!(strategic_public_asset(oil));
        assert!(capture_value(&rules, oil, true) > 0);
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v26_spy_effect_priority_tests {
    use super::*;

    #[test]
    fn tech_and_unit_experience_beats_repeat_rich_refinery_spying() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
            else { return; };
        let Some(money) = rules.object("GAREFN") else { return; };
        let Some(lab) = rules.object("GATECH") else { return; };
        let Some(barracks) = rules.object("GAPILE") else { return; };
        let Some(factory) = rules.object("GAWEAP") else { return; };
        assert!(cash_spy_target(&rules, money));
        assert_eq!(infiltration_value(&rules, money, 25_000), 8,
            "existing V16 money-spy priority must not regress");
        assert!(rules.is_build_tech(&lab.id));
        assert!(infiltration_value(&rules, lab, 25_000)
            > infiltration_value(&rules, money, 25_000));
        assert_eq!(spy_class_code(&rules, barracks), Some(1));
        assert_eq!(spy_class_code(&rules, factory), Some(2));
        assert_eq!(spy_class_code(&rules, lab), Some(3));
        assert!(infiltration_value(&rules, barracks, 25_000) >= 8);
        assert!(infiltration_value(&rules, factory, 25_000) >= 8);
    }

    #[test]
    fn power_sabotage_is_repeatable_and_has_a_real_power_effect() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
            else { return; };
        let Some(power) = rules.object("GAPOWR") else { return; };
        assert!(power.power > 0);
        assert_eq!(spy_class_code(&rules, power), None,
            "a temporary blackout cannot permanently exhaust a spy class");
        assert!(infiltration_value(&rules, power, 25_000) > 1);
    }

    #[test]
    fn successful_breach_prefers_new_capabilities_over_repeatable_cash() {
        // A laboratory and a barracks provide persistent capabilities; their
        // value should rise after escorts have secured a real approach, even
        // though their original rule-based target weights are unchanged.
        let cash = spy_context_value(8, 23, None, 10_000, true, false);
        let lab = spy_context_value(10, 3, None, 10_000, true, false);
        let barracks = spy_context_value(8, 1, None, 10_000, true, false);
        assert!(lab > cash);
        assert!(barracks > cash);
    }

    #[test]
    fn diversifies_attempts_without_marking_them_as_success() {
        let first_cash = spy_context_value(8, 23, None, 2_000, false, false);
        let repeated_cash = spy_context_value(8, 23, Some(1_900), 2_000, false, false);
        let factory = spy_context_value(8, 2, None, 2_000, false, false);
        assert!(first_cash > repeated_cash);
        assert!(factory > repeated_cash);
        // Emergency sabotage of an *active* battery should still win when
        // the same target's power has been attacked recently.
        assert!(spy_context_value(6, 21, Some(1_900), 2_000, false, true)
            > factory);
        // A recent *attempt* expires; only native observed house effects are
        // allowed to make a one-time unlock permanently exhausted.
        assert_eq!(spy_context_value(8, 23, Some(1_000), 3_000, false, false), 8);
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v22_spy_disguise_rules_tests {
    use super::*;

    #[test]
    fn spy_attack_uses_a_real_makes_disguise_weapon_not_a_simulation_cheat() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
            else { return; };
        let spy = rules.object("SPY").expect("retail spy exists");
        let infantry = rules.object("E1").expect("retail infantry exists");
        assert!(has_disguise_weapon(&rules, spy),
            "the spy should disguise by a native attack/detonation");
        assert!(!has_disguise_weapon(&rules, infantry),
            "ordinary infantry must not borrow the disguise exception");
        assert!(disguise_shot_is_remote(&rules, spy),
            "retail MakeupKit has a negative range: approaching infantry to fire is wrong");
        assert!(!disguise_shot_is_remote(&rules, infantry));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v23_garrison_ore_priority_tests {
    use super::{ore_garrison_buckets, ore_garrison_score, ORE_GARRISON_MIN_CELLS};
    use std::collections::BTreeMap;

    #[test]
    fn center_mineral_garrison_beats_edge_house_without_hidden_ore() {
        let known: BTreeMap<(u16, u16), u8> = (0..16)
            .map(|i| ((50 + i % 4, 50 + i / 4), 1))
            .collect();
        let buckets = ore_garrison_buckets(&known);
        assert!(ore_garrison_score(&buckets, (53, 53)) >= ORE_GARRISON_MIN_CELLS);
        assert_eq!(ore_garrison_score(&buckets, (4, 4)), 0);
        assert_eq!(ore_garrison_score(&ore_garrison_buckets(&BTreeMap::new()),
            (53, 53)), 0);
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v23_initial_army_and_crate_tests {
    use super::{crate_runner_available, FORCE_ORDERS_PER_FRAME, CRATE_RUNNERS};

    #[test]
    fn opening_ten_infantry_are_not_truncated_by_the_old_eight_order_cap() {
        assert!(FORCE_ORDERS_PER_FRAME >= 10);
    }

    #[test]
    fn immediate_crates_get_two_extra_claim_slots_over_distant_ones() {
        assert!(!crate_runner_available(CRATE_RUNNERS, CRATE_RUNNERS));
        assert!(crate_runner_available(CRATE_RUNNERS, CRATE_RUNNERS + 2));
        assert!(!crate_runner_available(CRATE_RUNNERS + 2, CRATE_RUNNERS + 2));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v28_slave_miner_strategy_tests {
    use super::*;

    #[test]
    fn retail_slave_miner_is_a_mobile_refinery_not_a_normal_ore_truck() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
            else { return; };
        let mobile = rules.object("SMIN").expect("standard slave miner");
        let deployed = mobile_slave_refinery(&rules, mobile)
            .expect("Enslaves= and DeploysInto= give the true refinery lifecycle");
        assert!(deployed.resource_destination);
        assert_eq!(deployed.id, "YAREFN");
        let normal = rules.object("CMIN").expect("standard chrono miner");
        assert!(mobile_slave_refinery(&rules, normal).is_none());
    }

    #[test]
    fn a_working_slave_mine_does_not_pack_for_minor_gain() {
        assert!(!slave_relocation_worthwhile(12, 100, 40, 14));
        assert!(!slave_relocation_worthwhile(0, 8, 40, 14));
        assert!(!slave_relocation_worthwhile(0, 100, 8, 14));
        assert!(slave_relocation_worthwhile(0, 80, 60, 14));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v41_defense_regression_tests {
    use super::*;

    #[test]
    fn field_houses_beat_home_houses_and_opening_stays_mobile() {
        assert_eq!(garrison_priority(true, 0, false), 0);
        assert_eq!(garrison_priority(false, ORE_GARRISON_MIN_CELLS, false), 1);
        assert_eq!(garrison_priority(false, 0, true), 2);
        assert_eq!(garrison_priority(false, 0, false), 3);
        assert_eq!(HOME_GARRISON_OPENING_CAP, 1);
    }

    #[test]
    fn force_shield_blackout_requires_actual_home_emergency() {
        assert!(!shield_emergency(400, 500, 40, 2));
        assert!(!shield_emergency(400, 500, 0, 12));
        assert!(shield_emergency(80, 500, 40, 2));
        assert!(shield_emergency(80, 500, 80, 1));
    }
}
