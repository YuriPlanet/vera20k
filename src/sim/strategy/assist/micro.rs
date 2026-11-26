//! What the army does *this second*: the layer between a team's target and a
//! unit's mission.
//!
//! The teams above decide where the army goes. What they do not decide, and
//! what a player decides continuously, is what each unit shoots on the way:
//! an army told to walk at a base and nothing else walks past the tanks
//! sitting between it and that base, because the fight was never its order —
//! the walk was. That is the whole of "the push is dense and blind", and it is
//! not fixed by choosing a better target: it is fixed by asking, every second
//! and per unit, what is in reach *now*.
//!
//! So the cycle here is short ([`super::tracked::MICRO_FRAMES`], a second) and
//! its answers override a march rather than being overridden by one: a unit
//! whose reach already holds a hostile is told to shoot it, and the march is
//! re-decided when the shooting is over. The intent it records is
//! [`TrackedKind::Engage`], whose term is one cycle — long enough that the
//! order is not re-sent every frame, short enough that a unit which has
//! finished fighting is the march's again inside a second.
//!
//! The rest of the layer is what each role does when nothing is in reach: a
//! defender walks at the raid, a scout walks where it can see what the enemy is
//! doing. The fist is left alone — the advance step owns it.
//!
//! Determinism: the ledger's own order, squared integer distances, id
//! tie-breaks.

use std::collections::BTreeSet;

use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::{EnemyEntity, Engage, Observation};
use crate::sim::mission::{MissionId, MissionType};

use super::AssistBrain;
use super::squads::Role;
use super::squared_distance;
use super::tracked::TrackedKind;
use crate::sim::control::Goal;

/// How many engagements this pass may begin in one frame. Round-robin work
/// ordering and per-actor terms keep this bounded without an eight-unit cliff.
const MICRO_ORDERS_PER_FRAME: usize = 24;

/// How far past its own reach a unit will still take a fight, in cells. One:
/// the cell it is about to step into, so a closing hostile is met rather than
/// walked into.
const ENGAGE_MARGIN_CELLS: i32 = 1;

/// One unit's share of this cycle.
struct Work {
    unit: super::census::Unit,
    entity_id: u64,
    role: Role,
    cell: (u16, u16),
    /// What its weapon reaches, in cells; zero means it cannot shoot at all.
    reach: i32,
    /// What its team is aimed at, when the team is aimed at something.
    squad_target: Option<(u64, (u16, u16))>,
    /// Whether it is dug in. A deployed soldier cannot walk, so every order
    /// that is a walk is an order it cannot obey.
    deployed: bool,
    /// Its movement zone: the reachability question is asked per zone, so a
    /// boat's map is not a dog's.
    zone: MovementZone,
    /// How many shots its weapon fires a cycle: the number that decides whether
    /// interrupting it is a thing worth doing.
    burst: i32,
    /// What this unit runs: the capability table's answer, not a flag each
    /// pass re-derives.
    pilot: super::pilots::Pilot,
    /// What its pilot goes for first when several hostiles are in reach.
    preference: super::pilots::Preference,
}

pub(super) fn run(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    emergency_raid_retreat(brain, rules, obs, out);
    // Snapshot these as shared borrows: the outer squad() borrow and the inner
    // census/tracked reads must not move `brain` into the FnMut closure.
    let units = &brain.census.units;
    let available = |unit: &super::census::Unit| brain.census.available(unit);
    let tasked = |id: u64| brain.tracked.tasks.get(id).is_none();
    let raiders: Vec<super::census::Unit> = brain.squads.squad(Role::Harass)
        .into_iter().flat_map(|team| units.iter().filter(move |unit|
            team.members.contains(&unit.entity_id) && available(unit)
                && unit.health_permille>=700
                && tasked(unit.entity_id)).copied()).collect();
    let rebuilding = brain.director.recovering_under_fire() || brain.mobilization.active() || brain.campaign.economy_resting();
    let raid_target = if rebuilding { None } else {
        mining_raid_objective(rules, obs, brain, &raiders)
    };
    if let Some((id, cell)) = raid_target {
        let since = brain.harass_focus.filter(|(old, until)|
            *old == id && tick < *until);
        brain.harass_focus = Some(since.unwrap_or((id, tick.saturating_add(900))));
        brain.harass_site = Some((cell, tick.saturating_add(900)));
    } else {
        brain.harass_focus = None;
        if rebuilding || brain.harass_site.is_some_and(|(_, until)| tick >= until) {
            brain.harass_site = None;
        }
    }
    // Snapshotted: the ledger and the teams are borrowed while they are read,
    // and the intent table is written below.
    // Which hostiles in the ledger are buildings. A unit's guns reach both, and
    // the nearest one is a wall as often as a soldier — a unit that shells the
    // wall while a tank closes has its answer backwards.
    let structures: BTreeSet<u64> = brain
        .census
        .enemies
        .iter()
        .filter(|enemy| {
            obs.object_type(rules, enemy.type_id)
                .is_some_and(|ty| ty.category == ObjectCategory::Building)
        })
        .map(|enemy| enemy.entity_id)
        .collect();
    // Who each hostile is shooting, read once for the whole army: one query per
    // enemy rather than one per enemy and unit. A unit that is being shot at
    // answers the gun that is shooting it before the wall it was shelling —
    // the "attacked the base while a tank killed it" report.
    // Historic sightings are useful to pick a destination, but not a live
    // firing target. Every prioritisation below uses *current* vision only.
    let visible_enemies: BTreeSet<u64> = obs.visible_enemy_entities()
        .map(|enemy| enemy.entity_id).collect();
    let aimed_at: std::collections::BTreeMap<u64, u64> = brain
        .census
        .enemies
        .iter()
        .filter(|enemy| visible_enemies.contains(&enemy.entity_id))
        .filter_map(|enemy| {
            obs.attack_target(enemy.entity_id)
                .map(|t| (enemy.entity_id, t))
        })
        .collect();
    // Kill the visible controller to release captured friends instead of
    // destroying those friends first. Never chase an invisible controller.
    let liberators: BTreeSet<u64> = brain.census.enemies.iter()
        .filter_map(|enemy| obs.visible_liberation_target(enemy.entity_id))
        .collect();
    let controllers: BTreeSet<u64> = obs.visible_enemy_entities()
        .filter(|enemy| super::tactics::actor_control_reach(rules,obs,enemy.entity_id).is_some()
            || obs.entity_type(rules,enemy.entity_id).is_some_and(|ty|
                super::tactics::anti_infantry_standoff(rules,ty)))
        .map(|enemy| enemy.entity_id).collect();
    // An enemy shooting an ore carrier, a refinery, or the yard is a real
    // economic emergency. The spare guns in reach should focus that raider
    // rather than independently choosing nearer scenery. An attacker shooting
    // *this* fighter still wins: the first key in `in_reach` owns self-defence.
    let economic_assets: BTreeSet<u64> = brain.census.units.iter()
        .filter(|unit| unit.traits.harvester || unit.traits.refinery || unit.traits.yard)
        .map(|unit| unit.entity_id)
        .collect();
    let economic_raiders: BTreeSet<u64> = aimed_at.iter()
        .filter_map(|(enemy, target)| economic_assets.contains(target).then_some(*enemy))
        .chain(obs.visible_enemy_entities().filter(|enemy|
            super::convergence::mobile_threat(rules,obs,enemy)
                && super::convergence::near_income(&brain.census,enemy.cell))
            .map(|enemy| enemy.entity_id)).collect();
    // Existing friendly fire is evidence that a durable enemy is worth
    // finishing. Support that engagement, rather than scattering the spare
    // tanks among different targets. Don't waste three guns on a nearly dead
    // rifleman, and don't use last-known coordinates as current fire orders.
    let mut friendly_fire = std::collections::BTreeMap::<u64,usize>::new();
    for unit in brain.census.units.iter().filter(|unit| brain.census.available(unit)) {
        if let Some(target) = obs.attack_target(unit.entity_id)
            && visible_enemies.contains(&target)
            && !obs.visible_enemy_invulnerable(target)
            && obs.engage(rules,unit.entity_id,target)==Engage::Ready
            && obs.can_hurt(rules,unit.entity_id,target)
        {
            *friendly_fire.entry(target).or_default() += 1;
        }
    }
    let focus_targets: BTreeSet<u64> = friendly_fire.into_iter()
        .filter(|(_,count)| *count >= 2)
        .filter_map(|(enemy,_)| {
            let ty = obs.entity_type(rules,enemy)?;
            (ty.category != ObjectCategory::Infantry
                && obs.health(enemy).is_some_and(|hp| hp.saturating_mul(2) > ty.strength))
                .then_some(enemy)
        }).collect();
    let mut work=collect(brain,rules,obs,tick,&visible_enemies,&aimed_at,
        &economic_raiders,&controllers,&liberators);
    let turn=work.len();
    if turn>0 {work.rotate_left((tick as usize)%turn);}
    let mut issued = 0;
    for item in work {
        if issued >= MICRO_ORDERS_PER_FRAME {
            break;
        }
        let rebuilding_squad = rebuilding && matches!(item.role, Role::Attack | Role::Harass);
        // A step out of a shell's landing point is this frame's answer for the
        // unit; an engage order on top of it would walk it back into the blast.
        // A courier is not a new solo raiding squad after collecting its box.
        // Only allow immediate self-defence here, never a proactive target.
        // A courier's delivery leg is its primary mission; do not give a
        // passenger ferry a disposable combat assignment on the way in.
        if brain.specialist_shuttles.contains_key(&item.entity_id)
            || brain.specialist_shuttles.values().any(|trip|
                trip.passenger == item.entity_id) { continue; }
        if (brain.crate_claims.contains_key(&item.entity_id)
                || brain.crate_returns.contains_key(&item.entity_id))
            && !brain.census.enemies.iter().any(|enemy| {
                obs.is_cell_visible(enemy.cell.0, enemy.cell.1)
                    && obs.attack_target(enemy.entity_id) == Some(item.entity_id)
            })
        { continue; }
        if super::advance::charge_member(brain, tick, item.entity_id)
            || super::orders::ordered_this_frame(out, item.entity_id)
            || brain.avoid_retreat_until.get(&item.entity_id)
                .is_some_and(|until| tick < *until)
        { continue; }
        // An exposed light vehicle must break an already losing native Attack
        // mission before it can be sent into another duel. A nearby armored
        // escort may change the trade; a lone IFV should save itself instead.
        if let Some(enemy) = obs.attack_target(item.entity_id)
            && visible_enemies.contains(&enemy)
            && super::tactics::unfavourable_vehicle_duel(rules, obs, item.entity_id, enemy)
            && !super::tactics::armored_support(&brain.census, rules, obs,
                item.entity_id, enemy, item.cell)
        {
            let home = brain.census.rally.or(brain.census.base_cell)
                .filter(|cell| super::orders::reachable(rules, obs, item.entity_id, *cell)
                    && !super::tactics::unsafe_post(rules, obs, item.entity_id, *cell));
            if let Some(cell) = home
                && super::orders::issue_tracked(&mut brain.tracked, brain.trace,
                    out, rules, obs, item.entity_id, super::tracked::TrackedKind::Avoid,
                    None, Some(cell), Goal::Move { cell })
            {
                brain.avoid_retreat_until.insert(item.entity_id, tick.saturating_add(225));
                issued += 1;
                continue;
            }
        }
        // A single mind-controller can turn a whole squad against itself.
        // Current visibility only: long-range guns may concentrate on the
        // controller, but short-range infantry/vehicles must not chase some
        // nearer rifleman and wander into the controller's range.
        if !item.deployed
            && !super::tactics::psychic_immune(rules,obs,item.entity_id)
            && obs.entity_type(rules, item.entity_id).is_some_and(|ty|
                ty.category != ObjectCategory::Aircraft && !ty.naval)
            && let Some((controller, position, enemy_reach)) =
                super::tactics::controller_covering(rules, obs, item.cell)
        {
            if super::tactics::actual_threat_reach(rules,obs,item.entity_id,controller)>enemy_reach+1
                && obs.engage(rules,item.entity_id,controller)==Engage::Ready
                && super::orders::admits(rules, obs, item.entity_id,
                    &Goal::Attack { target: controller })
            {
                // Keep the native shot/reload cycle, rather than refresh
                // the same target every time this tactical branch runs.
                if obs.attack_target(item.entity_id)==Some(controller)
                    && obs.engage(rules,item.entity_id,controller)==Engage::Ready
                { continue; }
                if super::orders::issue_tracked(&mut brain.tracked,
                    brain.trace, out, rules, obs, item.entity_id,
                    TrackedKind::Engage, Some(controller), Some(position),
                    Goal::Attack { target: controller }) {
                    issued += 1;
                }
                continue;
            }
            if let Some(cell) = super::tactics::evade_controller(
                rules, obs, item.entity_id, item.cell, position)
            {
                if super::orders::issue_tracked(&mut brain.tracked, brain.trace,
                    out, rules, obs, item.entity_id, TrackedKind::Dodge,
                    None, Some(cell), Goal::Move { cell }) {
                    brain.avoid_retreat_until.insert(item.entity_id,
                        tick.saturating_add(150));
                    issued += 1;
                }
            }
            continue;
        }
        // What is in reach now outranks where the unit was going: this is the
        // whole reason the layer exists. "In reach" is reach and weapon
        // together, so a dog walks on past the tank to the infantry behind it
        // instead of being held on a target it cannot hurt.
        let personnel_specialist=obs.entity_type(rules,item.entity_id)
            .is_some_and(|ty|ty.disguise_when_still);
        if let Some(enemy) = in_reach_ready(
            brain.census.enemies.iter()
                // A visible controller of our stolen unit is the legitimate
                // target. Do not spend this cycle killing the captured tank.
                .filter(|enemy| visible_enemies.contains(&enemy.entity_id))
                .filter(|enemy| !rebuilding_squad
                    || aimed_at.get(&enemy.entity_id) == Some(&item.entity_id)
                    || economic_raiders.contains(&enemy.entity_id)
                    || brain.census.base_cell.is_some_and(|home| {
                        let radius = super::advance::THREAT_RADIUS;
                        squared_distance(home, enemy.cell) <= radius * radius
                    }))
                .filter(|enemy| !super::orders::reclamation_reserved(
                    rules, obs, &brain.census, &brain.tracked, **enemy))
                .filter(|enemy| obs.visible_liberation_target(enemy.entity_id).is_none())
                .map(|enemy| {
                (
                    enemy.entity_id,
                    enemy.cell,
                    structures.contains(&enemy.entity_id),
                    aimed_at.get(&enemy.entity_id) == Some(&item.entity_id)
                        && obs.engage(rules,enemy.entity_id,item.entity_id)==Engage::Ready
                        && obs.can_hurt(rules,enemy.entity_id,item.entity_id),
                )
            }),
            item.cell,
            item.reach,
            |target| {
                // Can it hurt it at all? A dog beside a tank can reach it
                // and fire at it and never damage it; the answer is the
                // armour floor the native target scan applies, not the fire
                // error. Without this a dog spent the match on a tank while
                // the infantry behind it walked past.
                // During long-term strategic reserve, do not let the micro
                // pass undo the whole campaign by engaging an undefended
                // building with one lonely tank. Mobile enemies and any gun
                // currently firing at this actor remain legitimate defence.
                (!brain.campaign.holding_fist() || item.role != Role::Attack
                    || !structures.contains(&target)
                    || aimed_at.get(&target) == Some(&item.entity_id))
                    // The long-term siege plan is not allowed to fall apart
                    // because micro sees a nearby operational prism tower.
                    // Charge units are handled above by the wave commander.
                    && (item.role != Role::Attack
                        || !super::siege_counter::avoid_solo_tower_attack(
                            rules, obs, item.entity_id, target)
                        || super::safety::supported_ready_fire(
                            brain,rules,obs,&item.unit,Goal::Attack {target}))
                    && !obs.visible_enemy_invulnerable(target)
                    && !super::advanced::protect_controlled_friend(brain,rules,obs,item.entity_id,target)
                    && super::orders::admits(rules, obs, item.entity_id, &Goal::Attack { target })
                    && obs.can_hurt(rules, item.entity_id, target)
                    && (!super::tactics::unfavourable_vehicle_duel(
                        rules, obs, item.entity_id, target)
                        || aimed_at.get(&target) == Some(&item.entity_id)
                        || super::tactics::armored_support(&brain.census, rules, obs,
                            item.entity_id, target, item.cell))
            },
            |target| (target_rank(
                prefer(rules,obs,item.preference,target,personnel_specialist)
                    .saturating_add(super::doctrine::preference_penalty(
                        rules,obs,item.entity_id,target)),
                liberators.contains(&target) || controllers.contains(&target),
                economic_raiders.contains(&target),
                focus_targets.contains(&target),
            ),structures.contains(&target),
                super::tactics::shots_to_finish(rules,obs,item.entity_id,target)),
            |target|obs.engage(rules,item.entity_id,target)==Engage::Ready,
        ) {
            // A tank beside a soldier flattens him instead of trading shots:
            // `Crusher=` on the tank, `Crushable=` on the infantry, and the
            // walk onto his cell is the crush. One cell only — the step the
            // next frame would take anyway — so a miss costs a cell of ground
            // rather than a march, and the native approach arm is a computer
            // house's, which is why nothing else was doing this.
            if item.pilot == super::pilots::Pilot::Crush
                && !item.deployed
                && crush_adjacent(item.cell, enemy.1)
                && obs
                    .entity_type(rules, enemy.0)
                    .is_some_and(|ty| ty.category == ObjectCategory::Infantry)
                && obs.crushes(rules, item.entity_id, enemy.0)
                && brain.tracked.admits(
                    tick,
                    item.entity_id,
                    TrackedKind::Engage,
                    Some(enemy.0),
                    Some(enemy.1),
                )
            {
                // The crush is this unit's fight for the micro term: recorded
                // as the engage it is, so the next frame does not read the
                // empty target the walk leaves behind as "no fight here" and
                // send the tank back — the attack/move loop a crush that
                // landed used to start.
                brain.tracked.record(
                    tick,
                    item.entity_id,
                    TrackedKind::Engage,
                    Some(enemy.0),
                    Some(enemy.1),
                );
                if issue(
                    brain,
                    out,
                    rules,
                    obs,
                    item.entity_id,
                    Goal::Move { cell: enemy.1 },
                    item.reach,
                    item.deployed,
                ) {
                    issued += 1;
                    continue;
                }
                brain.tracked.void(item.entity_id, TrackedKind::Engage);
            }
            // An attack on the target it already holds is not an order.
            //
            // The Engage cycle re-sends the attack every frame its term allows,
            // and re-sending it is `Mission_Attack`'s commence with the same
            // Target: in retail that is the famous rate-of-fire bug — a turreted
            // vehicle, the IFV above all, whose passenger's launcher is re-armed
            // by the reassign rather than by the shot, so a unit being pulled
            // back and forth fires a rocket every cycle instead of every ROF.
            // Asking the object what it holds costs nothing and takes the cycle
            // out of the firing path entirely.
            if obs.attack_target(item.entity_id) == Some(enemy.0) {
                continue;
            }
            if !brain.tracked.admits(
                tick,
                item.entity_id,
                TrackedKind::Engage,
                Some(enemy.0),
                Some(enemy.1),
            ) {
                continue;
            }
            brain.tracked.record(
                tick,
                item.entity_id,
                TrackedKind::Engage,
                Some(enemy.0),
                Some(enemy.1),
            );
            if !issue(
                brain,
                out,
                rules,
                obs,
                item.entity_id,
                Goal::Attack { target: enemy.0 },
                item.reach,
                item.deployed,
            ) {
                brain.tracked.void(item.entity_id, TrackedKind::Engage);
                continue;
            }
            issued += 1;
            continue;
        }
        // A vehicle can be shelling a factory while a tank shoots it from
        // beyond its own range. Even if no hostile is *in* our gun range yet,
        // the attacker's effective range (not the IFV's range) proves that
        // continuing to shell the building is a losing commitment.
        if !item.deployed && obs.attack_target(item.entity_id).is_some_and(|id| {
            obs.entity_type(rules,id).is_some_and(|ty| ty.category == ObjectCategory::Building)
        }) {
            let overmatched = obs.visible_enemy_entities().find_map(|enemy| {
                if aimed_at.get(&enemy.entity_id) != Some(&item.entity_id)
                    || !obs.can_hurt(rules,enemy.entity_id,item.entity_id) { return None; }
                let range = super::tactics::actual_threat_reach(
                    rules,obs,enemy.entity_id,item.entity_id);
                (range > item.reach + 1).then_some((enemy.cell,range))
            });
            if let Some((gun,reach)) = overmatched {
                let safe = super::tactics::siege_post(
                    rules,obs,item.entity_id,item.cell,gun,reach)
                    .or_else(|| brain.census.rally.or(brain.census.base_cell)
                        .filter(|cell| !super::tactics::unsafe_post(
                            rules,obs,item.entity_id,*cell)));
                if let Some(post) = safe
                    && super::orders::reachable(rules,obs,item.entity_id,post)
                    && super::advance::cell_distance(item.cell,post) > 2
                    && super::orders::issue_tracked(
                        &mut brain.tracked,brain.trace,out,rules,obs,
                        item.entity_id,TrackedKind::Avoid,None,Some(post),
                        Goal::Move { cell: post },
                    )
                {
                    issued += 1;
                    continue;
                }
            }
        }
        // The main fist is not a free pool of solo demolition crews during
        // campaign accumulation. Local *mobile* threats and self-defence were
        // handled above; any further voluntary combat/march belongs to the
        // persistent campaign's safe formation/staging operation below.
        // The Harass team is unaffected and keeps cutting enemy income.
        if rebuilding_squad || (brain.campaign.holding_fist() && item.role == Role::Attack) {
            continue;
        }
        // Nobody's building in the way. It is in no enemy list — nobody owns
        // it — so nothing in this layer would ever shoot it, and the unit
        // stands at its wall for the whole order, which is the "the infantry
        // just stands there" report. What is cleared is what the unit is up
        // against, and what stands inside the yard, where it blocks the
        // factories' own lanes; a village the army walks past is not a
        // demolition order.
        if let Some((blocker, _)) = neutral_in_the_way(brain, rules, obs, item.entity_id, item.cell)
            && !brain.census.enemies.iter().any(|enemy| enemy.entity_id == blocker
                && super::orders::engineer_claimed(
                    rules, obs, &brain.census, &brain.tracked, *enemy))
        {
            if super::orders::admits(
                rules,
                obs,
                item.entity_id,
                &Goal::Attack { target: blocker },
            ) && !super::orders::already_attacking(obs, item.entity_id, blocker)
                && brain.tracked.admits(
                    tick,
                    item.entity_id,
                    TrackedKind::Engage,
                    Some(blocker),
                    None,
                )
            {
                brain.tracked.record(
                    tick,
                    item.entity_id,
                    TrackedKind::Engage,
                    Some(blocker),
                    None,
                );
                if issue(
                    brain,
                    out,
                    rules,
                    obs,
                    item.entity_id,
                    Goal::Attack { target: blocker },
                    item.reach,
                    item.deployed,
                ) {
                    issued += 1;
                    continue;
                }
                brain.tracked.void(item.entity_id, TrackedKind::Engage);
            }
        }
        // A deployed soldier is not given anything else. Nothing in reach means
        // there is nothing to shoot; an AreaGuard here would be a fresh
        // `Queue_Mission`, and `Assign_Target_Command` clears the target on it
        // — which, for a unit whose deploy state *is* the guard state, is the
        // deploy/undeploy cycle the player sees as twitching while the order
        // markers pile up. The dug-in position already is the order.
        if item.deployed {
            continue;
        }
        // Nothing in reach: the role says where to be.
        match item.role {
            // Ships need a *water* battle, not the main land army's march
            // target. Keep their own nearest visible reachable naval foe;
            // coastal targets may be attacked only within the ship's present
            // weapon reach, without a blind order to cross dry land.
            Role::Attack if obs.entity_type(rules,item.entity_id)
                .is_some_and(|ty|ty.naval) => {
                // Ready fire is handled above; distant ship movement belongs
                // to the explicit water-post commander, not a land advance.
                continue;
            }
            // The land fist is the advance step's; it should not pull ships
            // through a factory or order an ocean fleet into a city street.
            Role::Attack => continue,
            Role::Defend => {
                let Some((target, cell)) = item.squad_target else {
                    continue;
                };
                // A team is aimed at a structure more often than not, and a dog
                // has nothing for one: the units that can act on it go, this
                // one holds its guard instead of standing at the wall it cannot
                // touch for the whole term.
                if !super::orders::admits(rules, obs, item.entity_id, &Goal::Attack { target }) {
                    continue;
                }
                if super::orders::issue_tracked(
                    &mut brain.tracked,
                    brain.trace,
                    out,
                    rules,
                    obs,
                    item.entity_id,
                    TrackedKind::Attack,
                    Some(target),
                    Some(cell),
                    Goal::Attack { target },
                ) {
                    issued += 1;
                }
            }
            // Go at what pays for the enemy's army. An army too small to take
            // a base is not an army that should be standing in its own yard:
            // the ore is the one thing the enemy cannot replace quickly, and
            // a harvester is worth the tank that was going to be bought with
            // the load it is carrying.
            Role::Harass => {
                // A raid with nothing to raid has not looked yet. The
                // economy is in no ledger the house can see until somebody
                // has been there, so the raiders are the eyes: they walk at
                // the nearest ground the shroud still covers, and the ore
                // field they find is the raid's next target. Standing in the
                // base because the enemy's ore is unknown is the "it never
                // scouts" a play test reports, and it is the same walk a
                // player makes with the first few tanks.
                if brain.avoid_retreat_until.get(&item.entity_id)
                    .is_some_and(|until| tick < *until)
                { continue; }
                if let Some((victim, cell)) = raid_target {
                    if safe_mining_raid_leg(rules, obs, item.entity_id, item.cell, cell)
                        && super::orders::admits(rules, obs, item.entity_id,
                            &Goal::Attack { target: victim })
                    {
                        // Never commit the lead vehicle alone: two real
                        // mobile raiders must have made it to the same front.
                        let gathered = raiders.iter().filter(|friend|
                            super::advance::cell_distance(friend.cell, item.cell) <= 9
                                && obs.can_hurt(rules,friend.entity_id,victim)
                                && obs.entity_type(rules,victim).is_some_and(|ty|
                                    raid_target_reachable(rules,obs,friend.entity_id,ty,cell))
                                && safe_mining_raid_leg(rules, obs,
                                    friend.entity_id, friend.cell, cell)
                        ).count();
                        let free_shot=obs.engage(rules,item.entity_id,victim)
                            ==crate::sim::control::observation::Engage::Ready
                            && super::tactics::actual_threat_reach(rules,obs,victim,item.entity_id)==0;
                        if gathered >= 2 || free_shot {
                            if obs.attack_target(item.entity_id) != Some(victim)
                                && super::orders::issue_tracked(
                                    &mut brain.tracked, brain.trace, out, rules, obs,
                                    item.entity_id, TrackedKind::Attack,
                                    Some(victim), Some(cell), Goal::Attack { target: victim },
                                ) { issued += 1; }
                            continue;
                        }
                        if let Some(post) = raiders.iter()
                            .filter(|friend| friend.entity_id != item.entity_id)
                            .min_by_key(|friend| super::squared_distance(friend.cell,item.cell))
                            .map(|friend| friend.cell)
                            .filter(|post| super::orders::reachable(rules,obs,
                                item.entity_id,*post)
                                && safe_mining_raid_leg(rules,obs,
                                    item.entity_id,item.cell,*post))
                        {
                            if super::orders::issue_tracked(
                                &mut brain.tracked,brain.trace,out,rules,obs,
                                item.entity_id,TrackedKind::Move,None,Some(post),
                                Goal::Move { cell: post },
                            ) { issued += 1; }
                        }
                        continue;
                    }
                }
                // Keep a *recently observed* mining corridor on the raiders'
                // route while the moving miner vanishes into fog or transforms
                // into a deployed enslaving refinery. This is only a Move to
                // a known location; attacking a hidden entity remains illegal.
                if let Some((site, until)) = brain.harass_site
                    && tick < until
                    && super::advance::cell_distance(item.cell, site) > 7
                    && super::orders::reachable(rules, obs, item.entity_id, site)
                    && safe_mining_raid_leg(rules, obs, item.entity_id, item.cell, site)
                {
                    if super::orders::issue_tracked(
                        &mut brain.tracked, brain.trace, out, rules, obs,
                        item.entity_id, TrackedKind::Move, None, Some(site),
                        Goal::Move { cell: site },
                    ) { issued += 1; }
                    continue;
                }
                // No vulnerable *visible* miner: explore a legal, safe
                // frontier. Never blindly attack-move into old enemy fire.
                let Some(cell) = nearest_unexplored(obs, item.cell, item.zone)
                    .filter(|cell| safe_mining_raid_leg(
                        rules, obs, item.entity_id, item.cell, *cell))
                else { continue; };
                if super::orders::issue_tracked(
                    &mut brain.tracked, brain.trace, out, rules, obs,
                    item.entity_id, TrackedKind::Move,
                    None, Some(cell), Goal::Move { cell },
                ) { issued += 1; }
            }
            // A scout is sent to look, not to fight: a walk, so it does not
            // stop to bite every tank it passes on the way to the map.
            //
            // Where it is sent is the part that was missing — it was walked at
            // the nearest *known* hostile, which is following the enemy around
            // a map the house has already seen, not looking at the part it
            // has not. The shroud's own grid is the target source now, and a
            // known hostile is only the fallback once there is nothing left to
            // find.
            Role::Scout => {
                // The reconnaissance director owns this scout's *persistent*
                // destination. It divides unrevealed ground among scouts and
                // sends them to safe observation posts around stale strategic
                // sightings. Do not fall back to chasing a hidden enemy id.
                if brain.crate_claims.contains_key(&item.entity_id)
                    || brain.crate_returns.contains_key(&item.entity_id) { continue; }
                let Some(cell) = brain.recon.destination(item.entity_id) else {
                    continue;
                };
                // The native nav target confirms the command is already on
                // its way. A fresh assignment may replace an obsolete walk.
                if obs.destination(item.entity_id) == Some(cell) { continue; }
                if super::orders::issue_tracked(
                    &mut brain.tracked, brain.trace, out, rules, obs,
                    item.entity_id, TrackedKind::Move,
                    None, Some(cell), Goal::Move { cell },
                ) { issued += 1; }
            }
        }
    }
}

/// Whether one step from `here` lands on `victim`'s cell, which is what a
/// crush is: the tank is beside him, not a march away.
fn crush_adjacent(here: (u16, u16), victim: (u16, u16)) -> bool {
    here.0.abs_diff(victim.0).max(here.1.abs_diff(victim.1)) <= 1
}

/// Whether this unit's gun is talking right now: it holds a target and the
/// mission that fires it.
///
/// What it is for is the other side of the coin. Retail's burst state belongs
/// to the object, not to the target: `Assign_Target @ 0x006FCF5B` empties the
/// burst index on the one assignment that changes *to null*, and `GetROF
/// 0x004C7520` answers 3–5 frames for a shot inside a burst against the whole
/// reload after the last one. So a walk, a guard or a stop handed to a unit
/// mid-burst does not pause the burst — it rewinds it, and the shot after it
/// is a burst's first shot arriving in three frames instead of a full
/// reload's worth. Attack, move, attack is a loop that fires faster than the
/// weapon reloads, and it is the loop a brain that micromanages runs by
/// accident; it is also the one a player with an IFV runs on purpose.
///
/// The simulation keeps it, because retail has it. What this is for is keeping
/// the brain out of it: a pass that would move this unit asks first, and a
/// burst weapon's cycle is left to finish.
pub(super) fn firing(obs: &Observation<'_>, mission: MissionId, entity_id: u64) -> bool {
    mission == MissionId::from_known(MissionType::Attack) && obs.attack_target(entity_id).is_some()
}

/// Issue one order and say so.
///
/// The brain is the only thing that can explain why a soldier did what it did,
/// and "it sometimes shot and sometimes did not" is not a symptom anyone can
/// chase from a screenshot: the order, the target and the reach it was measured
/// with have to be on the record. One line per order at debug.
///
/// The order itself goes through [`super::orders::issue`], the one door that
/// asks the simulation whether the unit can act on the target at all.
fn issue(
    brain: &AssistBrain,
    out: &mut Outbox,
    rules: &RuleSet,
    obs: &Observation<'_>,
    entity_id: u64,
    goal: Goal,
    reach: i32,
    deployed: bool,
) -> bool {
    log::debug!(
        "auto play: micro {} {}{} reach={}",
        entity_id,
        match goal {
            Goal::Attack { target } => format!("attack {target}"),
            Goal::AttackMove { cell } => format!("attack-move {:?}", cell),
            Goal::Move { cell } => format!("move {:?}", cell),
            Goal::Guard => "guard".to_owned(),
            _ => format!("{goal:?}"),
        },
        if deployed { " deployed" } else { "" },
        reach
    );
    super::orders::issue(brain, out, rules, obs, entity_id, goal)
}

/// Keep a lawful hostile engagement unless there is a *reachable*, visible
/// attacker threatening this unit. An attack on neutral scenery has no such
/// ownership, so the next micro cycle may switch targets immediately.
fn preserve_existing_attack(fighting: bool, hostile_target: bool, under_fire: bool) -> bool {
    fighting && hostile_target && !under_fire
}

/// Every team member that may be told anything, with what the cycle needs to
/// know about it.
fn collect(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    tick: u64,
    visible:&BTreeSet<u64>,
    aimed_at:&std::collections::BTreeMap<u64,u64>,
    economic_raiders:&BTreeSet<u64>,
    controllers:&BTreeSet<u64>,
    liberators:&BTreeSet<u64>,
) -> Vec<Work> {
    let mut work = Vec::new();
    for squad in brain.squads.all() {
        let squad_target = squad.target.and_then(|target| {
            brain
                .census
                .enemies
                .iter()
                .find(|enemy| enemy.entity_id == target)
                .map(|enemy| (target, enemy.cell))
        });
        for unit in &brain.census.units {
            if !squad.members.contains(&unit.entity_id) || !unit.controllable()
                || unit.traits.yard_maker
                || super::advanced::field_healer(rules,obs,unit.entity_id)
                || obs.entity_type(rules,unit.entity_id).is_some_and(|ty| matches!(
                    super::doctrine::duty(rules,ty),
                    super::doctrine::Duty::Disposable |
                        super::doctrine::Duty::BombPlanter)) {
                continue;
            }
            // A unit already mid-fight is told nothing: its mission owns it,
            // and re-ordering it is what makes the fight stutter. The same is
            // true of a unit on its way into a building or a depot.
            //
            // Mid-fight means *holding a target*. A unit whose mission is
            // still Attack with nothing to shoot is a leftover the mission
            // engine kept, and every pass that reads the mission as "busy"
            // leaves it standing there for the rest of the match — the army
            // the play test reports as "a lot of units doing nothing".
            let current_target = obs.attack_target(unit.entity_id);
            let fighting=unit.mission==MissionId::from_known(MissionType::Attack)
                && current_target.is_some_and(|target|
                    visible.contains(&target) && !obs.visible_enemy_invulnerable(target)
                    && obs.engage(rules,unit.entity_id,target)==Engage::Ready
                    && obs.can_hurt(rules,unit.entity_id,target));
            let deployed = obs.is_deployed(unit.entity_id);
            let reach = super::tactics::actor_reach(rules,obs,unit.entity_id);
            let under_fire = brain.census.enemies.iter().any(|enemy| {
                visible.contains(&enemy.entity_id)
                    && aimed_at.get(&enemy.entity_id)==Some(&unit.entity_id)
                    && obs.engage(rules,enemy.entity_id,unit.entity_id)==Engage::Ready
                    && obs.can_hurt(rules,enemy.entity_id,unit.entity_id)
            });
            // Preserve an ongoing attack unless the unit is *being hit* by a
            // reachable hostile. An attack on a neutral wall is not such a
            // commitment: the hostile taking shots at us must come first.
            let losing_duel = current_target.is_some_and(|enemy| {
                super::tactics::unfavourable_vehicle_duel(rules, obs, unit.entity_id, enemy)
                    && !super::tactics::armored_support(&brain.census, rules, obs,
                        unit.entity_id, enemy, unit.cell)
            });
            // A Mirage-like ambusher has an anti-personnel weapon. A nearby
            // rifleman is worth switching to even while it is shelling a
            // building: otherwise the tank keeps shooting a concrete wall
            // while the enemy infantry burns its escort. Read the actual
            // disguise-when-still rule and current, visible infantry only.
            let nearby_personnel = current_target.is_some_and(|target|
                obs.entity_type(rules, target).is_some_and(|ty|
                    ty.category == ObjectCategory::Building))
                && obs.entity_type(rules, unit.entity_id)
                    .is_some_and(|ty| ty.disguise_when_still)
                && brain.census.enemies.iter().any(|enemy| {
                    obs.is_cell_visible(enemy.cell.0, enemy.cell.1)
                        && obs.entity_type(rules, enemy.entity_id)
                            .is_some_and(|ty| ty.category == ObjectCategory::Infantry)
                        && squared_distance(unit.cell, enemy.cell) <= (reach + 1).pow(2)
                        && obs.can_hurt(rules, unit.entity_id, enemy.entity_id)
                });
            let building_target=current_target.is_some_and(|id|
                obs.entity_type(rules,id).is_some_and(|ty|ty.category==ObjectCategory::Building));
            let priority_contact=building_target && brain.census.enemies.iter().any(|enemy|
                visible.contains(&enemy.entity_id) && Some(enemy.entity_id)!=current_target
                && (economic_raiders.contains(&enemy.entity_id)
                    || controllers.contains(&enemy.entity_id)
                    || liberators.contains(&enemy.entity_id))
                && squared_distance(unit.cell,enemy.cell)<=(reach+1).pow(2)
                && !obs.visible_enemy_invulnerable(enemy.entity_id)
                && obs.engage(rules,unit.entity_id,enemy.entity_id)==Engage::Ready
                && obs.can_hurt(rules,unit.entity_id,enemy.entity_id));
            if (preserve_existing_attack(fighting,
                current_target.is_some_and(|id| obs.is_hostile(id)), under_fire)
                && !losing_duel && !nearby_personnel && !priority_contact) || unit.in_service()
            {
                continue;
            }
            // A unit a routine has taken is that routine's: the retreat owns
            // it while it walks the unit out.
            if brain.tracked.held(tick, unit.entity_id) {
                continue;
            }
            // An order the world does not show is an order that did not take;
            // the brain reads the mission against every intent before any layer
            // runs (`AssistBrain::void_orders_the_world_does_not_show`), so the
            // walk that was refused, found no path or was interrupted is asked
            // again now rather than on its 450-frame term.
            let deployed = obs.is_deployed(unit.entity_id);
            let ty = obs.object_type(rules, unit.type_id);
            let pilot = ty.map_or(super::pilots::Pilot::Structure, |ty| {
                super::pilots::Pilot::of(rules, ty)
            });
            let reach = super::tactics::actor_reach(rules,obs,unit.entity_id);
            work.push(Work {
                unit:*unit,
                entity_id: unit.entity_id,
                role: squad.role,
                cell: unit.cell,
                reach,
                squad_target,
                deployed,
                zone: ty.map_or(MovementZone::Normal, |ty| ty.movement_zone),
                burst: ty.map_or(1, |ty| super::advance::weapon_burst(rules, ty, deployed)),
                pilot: pilot,
                preference: pilot.preference(),
            });
        }
    }
    work
}

/// The nearest hostile inside `reach` this unit's weapon can act on, as
/// `(id, cell)`; ties by id.
///
/// `answers` is [`Observation::can_engage`], asked per candidate rather than of
/// the nearest one: the nearest hostile is not always a hostile the unit can
/// do anything about. A dog beside a tank is beside a target its parasite
/// warhead cannot touch, and the infantry one cell further off is what it can
/// actually take — so the reach test and the weapon test are the same search,
/// and a dog with only tanks in reach is told nothing at all.
#[cfg(all(test, tests_sim_p_s))]
fn in_reach(
    enemies: impl Iterator<Item=(u64,(u16,u16),bool,bool)>,
    from:(u16,u16),reach:i32,answers:impl Fn(u64)->bool,rank:impl Fn(u64)->u8,
)->Option<(u64,(u16,u16))> {
    in_reach_ready(enemies,from,reach,answers,rank,|_|true)
}

fn in_reach_ready<R:Ord>(
    enemies: impl Iterator<Item = (u64, (u16, u16), bool, bool)>,
    from: (u16, u16),
    reach: i32,
    answers: impl Fn(u64) -> bool,
    rank: impl Fn(u64) -> R,
    ready: impl Fn(u64) -> bool,
) -> Option<(u64, (u16, u16))> {
    if reach <= 0 {
        return None;
    }
    let reach = (reach + ENGAGE_MARGIN_CELLS).max(1);
    enemies
        .filter(|(_, cell, _, _)| squared_distance(*cell, from) <= reach * reach)
        .filter(|(id, _, _, _)| answers(*id))
        // The gun pointed at this unit is the first answer, whatever else is
        // in reach: a unit that answers the crowd in front of it while a tank
        // behind it fires is a unit that dies to the one it ignored. Then a
        // mobile hostile outranks a structure — the one that shoots back is
        // the one that kills the unit while it shells a wall. Within a kind
        // the nearest wins, ties by id, so the answer does not depend on the
        // ledger's order.
        .min_by_key(|(id, cell, structure, shooting_me)| {
            (
                !*shooting_me,
                !ready(*id),
                rank(*id),
                *structure,
                squared_distance(*cell, from),
                *id,
            )
        })
        .map(|(id, cell, _, _)| (id, cell))
}

/// Where a pilot's preference ranks a target: 0 is what it goes for first.
///
/// The one thing a capability changes about an ordinary fight. A parasite goes
/// for what it can eat, an IFV's own rocket for what flies; everything else
/// takes the reach order the pass already had.
fn prefer(
    rules: &RuleSet,
    obs: &Observation<'_>,
    preference: super::pilots::Preference,
    target: u64,
    personnel_specialist: bool,
) -> u8 {
    let category = obs.entity_type(rules, target).map(|ty| ty.category);
    if personnel_specialist {
        return mirage_target_preference(category);
    }
    match preference {
        super::pilots::Preference::Any => 0,
        super::pilots::Preference::Armour => u8::from(!matches!(
            category,
            Some(ObjectCategory::Vehicle) | Some(ObjectCategory::Aircraft)
        )),
        super::pilots::Preference::Air => {
            u8::from(!matches!(category, Some(ObjectCategory::Aircraft)))
        }
    }
}

/// Mirage-style disguise tanks are ambushers, not demolition artillery.
/// Prefer living infantry, then mobile armor, and leave masonry to artillery.
/// Self-defence, controller liberation and economic raiders still have higher
/// precedence in `in_reach`/`target_rank`.
fn mirage_target_preference(category: Option<ObjectCategory>) -> u8 {
    match category {
        Some(ObjectCategory::Infantry) => 0,
        Some(ObjectCategory::Vehicle | ObjectCategory::Aircraft) => 1,
        Some(ObjectCategory::Building) => 3,
        _ => 2,
    }
}

/// One shared priority for special threats. Liberating captured allies
/// outranks an ordinary raid; attacking the house's income chain outranks
/// generic role preferences. Direct self-defense still wins in `in_reach`.
fn target_rank(
    preference: u8, liberation: bool, economic_raid: bool, allied_focus: bool,
) -> u8 {
    if liberation { 0 }
    else if economic_raid { 1 }
    else if allied_focus { 2 }
    else { preference.saturating_add(3) }
}

/// How near a neutral building has to stand to a unit for it to be what the
/// unit is up against, in cells: the cell it stands on and the one behind it,
/// which is where a foundation's near edge is.
const NEUTRAL_BLOCK_CELLS: i32 = 2;

/// Only a physical wall *on the current march* may be demolished, and
/// only when a weapon-bearing unit is already blocked at that wall. A neutral
/// building near the home base is not automatically a military objective.
/// This distinction protects civilian structures that engineers can occupy
/// and prevents a low-value fence from diverting the main army for minutes.
fn neutral_in_the_way(
    brain: &AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    actor: u64,
    cell: (u16, u16),
) -> Option<(u64, (u16, u16))> {
    let destination = obs.destination(actor)?;
    brain.census.neutral_within(cell, NEUTRAL_BLOCK_CELLS, |building| {
        obs.object_type(rules, building.type_id).is_some_and(|ty| {
            ty.wall && !ty.can_be_occupied && !ty.capturable
        })
    }).filter(|(_, barrier)| {
        super::orders::path_crosses(&[*barrier], cell, destination, 1)
    })
}

/// How coarse a look the scout takes at the map, in cells.
///
/// A scout does not need every cell: it needs to be sent somewhere the house
/// has not been, and six cells is close enough that nothing worth finding
/// hides between two samples — and far enough apart that looking over a whole
/// map costs a hundred asks rather than four thousand.
const RECON_STEP_CELLS: u16 = 6;

/// What is worth raiding, nearest first: an ore carrier before the bay it
/// unloads into, because the carrier is worth the load it is carrying and the
/// bay is a building that can be rebuilt.
#[allow(dead_code)]
fn enemy_economy(
    rules: &RuleSet,
    obs: &Observation<'_>,
    census: &crate::sim::strategy::assist::census::Census,
    from: (u16, u16),
) -> Option<(u64, (u16, u16))> {
    let mut best: Option<(u8, i32, u64, (u16, u16))> = None;
    for enemy in &census.enemies {
        let Some(ty) = obs.object_type(rules, enemy.type_id) else {
            continue;
        };
        // The same tags the house's own ledger reads, not the two keys this
        // used to test on its own: `Harvester=` and `ResourceGatherer=` are
        // both a mod's way of saying the same thing, and `Traits` is where the
        // brain decides which key it means.
        let traits = super::census::Traits::of(rules, ty);
        let rank = if traits.harvester {
            0
        } else if traits.refinery {
            1
        } else {
            continue;
        };
        let distance = squared_distance(enemy.cell, from);
        if best.is_none_or(|(best_rank, best_distance, _, _)| {
            (rank, distance) < (best_rank, best_distance)
        }) {
            best = Some((rank, distance, enemy.entity_id, enemy.cell));
        }
    }
    best.map(|(_, _, id, cell)| (id, cell))
}


/// The value of a mining raid is denying deliveries, not trading our own
/// expensive units under the opponent's fixed defences. Every test below is
/// grounded in a currently visible weapon, its real reach and a native path.
fn safe_mining_raid_leg(
    rules: &RuleSet, obs: &Observation<'_>, actor: u64,
    from: (u16, u16), to: (u16, u16),
) -> bool {
    if super::tactics::controller_covering_path(rules, obs, from, to).is_some() {
        return false;
    }
    let visible:Vec<_>=obs.visible_enemy_entities().collect();
    if !super::safety::safe_leg(&super::safety::envelopes(rules,obs,actor,&visible,false),from,to,false) {
        return false;
    }
    !visible.iter().any(|defender| {
        let Some(ty) = obs.entity_type(rules, defender.entity_id) else { return false; };
        if !super::siege_counter::dangerous_battery(rules, ty)
            || !obs.visible_enemy_building_operational(
                rules, defender.entity_id)
        { return false; }
        let reach = super::tactics::effective_reach_against(
            rules, obs, defender.entity_id, actor,
        );
        reach > 0 && (super::advance::cell_distance(defender.cell, from) <= reach + 1
            || super::advance::cell_distance(defender.cell, to) <= reach + 1
            || super::orders::path_crosses(&[defender.cell], from, to, reach + 1))
    })
}

/// A small mobile raiding group can exhaust the enemy economy without needing
/// a costly frontal siege. Target only *currently visible* harvesters or their
/// unloading bays, and require more than a single fighter to have a safe
/// native attack route. The target is shared across the squad.
pub(super) fn idle_miner_pressure(brain:&mut AssistBrain,rules:&RuleSet,
    obs:&Observation<'_>,out:&mut Outbox) {
    let now=obs.tick();
    if now%30!=0 || brain.director.under_attack_now()
        || brain.campaign.economy_resting() {return;}
    let candidates:Vec<_>=brain.census.units.iter().copied().filter(|u|
        brain.census.main_available(u) && u.health_permille>=750
        && u.slot==super::census::Slot::Vehicle && !u.traits.irreplaceable
        && !brain.census.mobile_aa.contains(&u.entity_id)
        && brain.tracked.tasks.get(u.entity_id).is_none()
        && !super::advance::siege_member(brain,now,u.entity_id)
        && !brain.squads.squad(Role::Defend).is_some_and(|s|s.members.contains(&u.entity_id))
        && !out.ordered_this_frame(u.entity_id)
        && obs.destination(u.entity_id).is_none() && obs.attack_target(u.entity_id).is_none())
        .collect();
    let mut issued=0;
    for enemy in obs.visible_enemy_entities() {
        if issued>=2 {break;}
        let Some(ty)=obs.object_type(rules,enemy.type_id) else {continue;};
        if ty.category!=ObjectCategory::Vehicle || !ty.resource_gatherer
            || obs.visible_enemy_invulnerable(enemy.entity_id) {continue;}
        let local:Vec<_>=candidates.iter().filter(|u|
            !out.ordered_this_frame(u.entity_id)
            && super::advance::cell_distance(u.cell,enemy.cell)<=12
            && obs.can_hurt(rules,u.entity_id,enemy.entity_id)
            && raid_target_reachable(rules,obs,u.entity_id,ty,enemy.cell)
            && safe_mining_raid_leg(rules,obs,u.entity_id,u.cell,enemy.cell))
            .collect();
        if local.len()<2 {continue;}
        let Some(lead)=local.iter().copied().find(|u|local.iter().any(|v|
            v.entity_id!=u.entity_id && super::advance::cell_distance(u.cell,v.cell)<=8))
            else {continue;};
        let pair:Vec<_>=local.iter().copied().filter(|u|
            super::advance::cell_distance(u.cell,lead.cell)<=8).take(2).collect();
        if pair.len()<2 {continue;}
        let strength=pair.iter().map(|u|u.points).fold(0i32,i32::saturating_add);
        if brain.census.enemy_points(rules,obs,enemy.cell,10)>strength.saturating_mul(2)/3
        {continue;}
        for u in pair {
            if super::orders::issue_tracked(&mut brain.tracked,brain.trace,out,rules,obs,
                u.entity_id,TrackedKind::Attack,Some(enemy.entity_id),Some(enemy.cell),
                Goal::Attack{target:enemy.entity_id}) {
                issued+=1;
                log::debug!("auto play: v60_miner_pressure tick={now} actor={} target={}",u.entity_id,enemy.entity_id);
            }
        }
    }
}

fn mining_raid_objective(
    rules: &RuleSet, obs: &Observation<'_>, brain: &AssistBrain,
    actors: &[super::census::Unit],
) -> Option<(u64, (u16, u16))> {
    if actors.len() < 2 { return None; }
    let center = {
        let count = i64::try_from(actors.len()).unwrap_or(1).max(1);
        let x = actors.iter().map(|u| i64::from(u.cell.0)).sum::<i64>() / count;
        let y = actors.iter().map(|u| i64::from(u.cell.1)).sum::<i64>() / count;
        (u16::try_from(x).unwrap_or(0), u16::try_from(y).unwrap_or(0))
    };
    let ours = actors.iter().fold(0i32, |total, unit|
        total.saturating_add(unit.points));
    let until = brain.harass_focus;
    obs.visible_enemy_entities().filter_map(|enemy| {
        let ty = obs.entity_type(rules, enemy.entity_id)?;
        let traits = super::census::Traits::of(rules, ty);
        // A deployed slave mine is not an ordinary refinery: it *is* both the
        // workforce and the output point, and destroying it severs the whole
        // slave-harvesting economy. Match the retail `Enslaves=` property in
        // both mobile (`SMIN`) and deployed (`YAREFN`) forms, not their IDs.
        let rank = mining_objective_rank(ty, traits.harvester, traits.refinery)
            .or_else(|| peripheral_raid_rank(ty))?;
        if obs.visible_enemy_invulnerable(enemy.entity_id)
            || super::orders::reclamation_reserved(rules,obs,&brain.census,&brain.tracked,enemy)
        {return None;}
        let safe:Vec<_>=actors.iter().filter(|unit|
            super::advance::cell_distance(unit.cell,enemy.cell)<=28
                && super::orders::admits(rules,obs,unit.entity_id,&Goal::Attack {target:enemy.entity_id})
                && obs.can_hurt(rules, unit.entity_id, enemy.entity_id)
                && raid_target_reachable(rules, obs, unit.entity_id, ty, enemy.cell)
                && safe_mining_raid_leg(rules, obs,
                    unit.entity_id, unit.cell, enemy.cell)
        ).collect();
        // The distant or blocked members cannot finance the local exchange.
        let local=safe.iter().filter(|u|safe.iter().any(|v|u.entity_id!=v.entity_id
            && super::advance::cell_distance(u.cell,v.cell)<=9))
            .fold(0i32,|sum,u|sum.saturating_add(u.points));
        if safe.len() < 2 || local<=0 || brain.census.enemy_points(rules, obs, enemy.cell, 10)
            > local.min(ours).saturating_mul(2) / 3
        { return None; }
        let held = until.is_some_and(|(id, limit)| id == enemy.entity_id
            && obs.tick() < limit);
        Some((!held, rank, super::squared_distance(center, enemy.cell),
            enemy.entity_id, enemy.cell))
    }).min_by_key(|(not_held, rank, distance, id, _)|
        (*rank >= 4, *not_held, *rank, *distance, *id))
        .map(|(_, _, _, id, cell)| (id, cell))
}

/// Both retail SMIN and YAREFN set ResourceGatherer *and*
/// ResourceDestination. The native object category is the distinction:
/// the parked mine's slaves and output point are one stationary economy,
/// whereas the mobile vehicle can still escape and relocate.
fn mining_objective_rank(ty: &ObjectType, harvester: bool, refinery: bool) -> Option<u8> {
    if ty.enslaves.is_some() && ty.category == ObjectCategory::Building { Some(0) }
    else if ty.enslaves.is_some() && ty.category == ObjectCategory::Vehicle { Some(1) }
    else if harvester { Some(2) }
    else if refinery { Some(3) }
    else { None }
}

/// A raiding group with no accessible mine may hit exposed power and
/// production instead of walking aimlessly along the map edge. Fixed armed
/// structures are excluded here; their actual envelope is checked separately.
fn peripheral_raid_rank(ty:&ObjectType)->Option<u8> {
    if ty.category!=ObjectCategory::Building || ty.is_base_defense || ty.weapon_record_count()>0 {
        return None;
    }
    if ty.power>0 {Some(4)}
    else if ty.weapons_factory || ty.factory_type().is_some() {Some(5)}
    else {None}
}

/// The centre of a deployed mine occupies ground. A zone query to that exact
/// square can fail although a tank can legally approach its *perimeter* to
/// attack it. Preserve the regular connected-zone check for moving miners.
fn raid_target_reachable(
    rules: &RuleSet, obs: &Observation<'_>, actor: u64,
    target: &ObjectType, target_cell: (u16,u16),
) -> bool {
    if target.category != ObjectCategory::Building {
        return super::orders::reachable(rules, obs, actor, target_cell);
    }
    let Some((from, type_id)) = obs.object_cell_and_type(actor) else { return false; };
    let Some(unit) = obs.object_type(rules, type_id) else { return false; };
    super::orders::building_approachable(obs,unit.movement_zone,from,target_cell)
}

/// Never chase a harvester all the way into a turret's coverage. The most
/// valuable raid is one that can leave and be repeated.
fn emergency_raid_retreat(
    brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>, out: &mut Outbox,
) {
    let Some(team) = brain.squads.squad(Role::Harass) else { return; };
    let ids: Vec<u64> = team.members.iter().copied().collect();
    for id in ids {
        let Some(unit) = brain.census.units.iter().find(|unit|
            unit.entity_id == id && unit.controllable() && !unit.in_service())
        else { continue; };
        let objective = obs.attack_target(id)
            .and_then(|target| obs.visible_enemy_entities().find(|enemy|
                enemy.entity_id == target).map(|enemy| enemy.cell))
            .or_else(|| obs.destination(id));
        // A harasser that has *already* wandered under an operational tower
        // must retreat even when its native Attack target has disappeared.
        // Previously, no destination meant no retreat and the micro combat
        // pass could turn it into a solo sacrifice against the tower.
        let standing_safe = safe_mining_raid_leg(rules, obs,
            id, unit.cell, unit.cell);
        let pursuit_safe = objective.is_none_or(|to|
            safe_mining_raid_leg(rules, obs, id, unit.cell, to));
        if standing_safe && pursuit_safe { continue; }
        let Some(home) = brain.census.rally.or(brain.census.base_cell) else { continue; };
        if !super::orders::reachable(rules, obs, id, home)
            || super::tactics::unsafe_post(rules, obs, id, home)
        { continue; }
        let order = Goal::Move { cell: home };
        // A real emergency fallback must replace a native Attack still
        // chasing the moving victim. Simply skipping the next order leaves
        // the old mission and the raider dies under the battery.
        if super::orders::admits(rules, obs, id, &order)
            && out.want(id, 2, order)
        {
            brain.tracked.forget_primary(id);
            brain.harass_focus = None;
            brain.avoid_retreat_until.insert(id, obs.tick().saturating_add(330));
            log::debug!("auto play: raid member {id} withdraws from visible battery");
        }
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v29_raid_policy_tests {
    use super::safe_mining_raid_leg;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::control::observation::Observation;

    #[test]
    fn a_clear_retreat_path_is_not_an_automatic_combat_order() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let mut sim = crate::sim::world::Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        let obs = Observation::new(&sim, owner);
        assert!(safe_mining_raid_leg(&rules, &obs, 10, (2, 2), (5, 5)));
    }
}

/// The nearest cell the house has not revealed, sampled on that grid.
///
/// The nearest unrevealed sample the unit can actually get to, in that order:
///
/// **reachable** — one the zone map says this unit's movement zone can get to
/// at all. This is the one that was missing, and it is why a scout stood
/// still: a cell can be perfectly good ground and still sit behind a wall,
/// across a cliff or on an island, and a walk ordered to it is a walk that
/// never arrives. The island is the water case, and it is answered by the
/// zone and not by a water test: the movement zone is what makes a boat's
/// reachable map a different map from a dog's and a hovercraft's a third,
/// with no list of who can swim.
///
/// `None` when there is nothing unrevealed this unit could get to. That is
/// not a failure, it is the answer the caller needs: the scout's fallback is
/// the nearest hostile it can see, and a scout that walks at an island is
/// worse than one that goes and looks at the enemy. Only a map with no
/// navigation projection at all falls back to the open ground and then to
/// the nearest cell regardless — there, no cell answers either question.
pub(super) fn nearest_unexplored(
    obs: &Observation<'_>,
    from: (u16, u16),
    zone: MovementZone,
) -> Option<(u16, u16)> {
    let (width,height)=obs.map_extent();
    let ground_bound=!matches!(zone,MovementZone::Fly|MovementZone::Water|MovementZone::WaterBeach
        |MovementZone::Amphibious|MovementZone::AmphibiousCrusher|MovementZone::AmphibiousDestroyer);
    // Dense scans are a periodic last resort, not a new per-frame full
    // map traversal after the game has already revealed everything.
    let steps:&[usize]=if obs.tick()%120==0 {
        &[RECON_STEP_CELLS as usize,2,1]
    }else{&[RECON_STEP_CELLS as usize]};
    for &step in steps {
        let mut best=None;
        for y in (0..height).step_by(step) {
            for x in (0..width).step_by(step) {
                let cell=(x,y);
                if obs.is_cell_revealed(x,y) || !obs.is_cell_in_playfield(x,y)
                    || (ground_bound && obs.has_ground_grid() && !obs.is_ground_walkable(x,y))
                    || !obs.can_reach(zone,from,cell) {continue;}
                let score=(squared_distance(cell,from),cell);
                if best.is_none_or(|old|score<old){best=Some(score);}
            }
        }
        if let Some((_,cell))=best{return Some(cell);}
        // Once the map is known, the dedicated periodic sector sweep owns
        // revisits. Do not invent a map-centre or unreachable fallback.
    }
    None
}

/// The nearest hostile of any kind, as `(id, cell)`; ties by id.
fn nearest(
    enemies: &[crate::sim::control::observation::EnemyEntity],
    from: (u16, u16),
) -> Option<(u64, (u16, u16))> {
    enemies
        .iter()
        .min_by_key(|enemy| (squared_distance(enemy.cell, from), enemy.entity_id))
        .map(|enemy| (enemy.entity_id, enemy.cell))
}

#[cfg(all(test, tests_sim_p_s))]
mod v13_priority_tests {
    use super::*;

    #[test]
    fn liberate_and_protect_income_before_preference_targets() {
        assert_eq!(target_rank(1,true,false,true),0);
        assert_eq!(target_rank(1,false,true,true),1);
        assert_eq!(target_rank(1,false,false,true),2);
        assert_eq!(target_rank(0,false,false,false),3);
        assert_eq!(target_rank(1,false,false,false),4);
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    use crate::sim::control::observation::EnemyEntity;
    use crate::sim::intern::InternedId;

    /// What a unit is told to shoot is what its own weapon reaches, and the
    /// margin is the cell it is about to step into — not the whole map, and
    /// not nothing.
    #[test]
    fn what_is_in_reach_is_what_the_unit_can_shoot() {
        let enemies = [(42u64, (22u16, 20u16), false, false)];
        assert_eq!(
            in_reach(enemies.into_iter(), (20, 20), 5, |_| true, |_| 0),
            Some((42, (22, 20))),
            "two cells away, and the weapon reaches five"
        );
        let far = [(43u64, (40u16, 20u16), false, false)];
        assert_eq!(
            in_reach(far.into_iter(), (20, 20), 1, |_| true, |_| 0),
            None,
            "twenty cells away is out of a one-cell reach"
        );
        assert_eq!(
            in_reach(enemies.into_iter(), (20, 20), 0, |_| true, |_| 0),
            None,
            "a unit with no weapon engages nothing"
        );
    }

    /// A pilot's preference is what its capability is for: a parasite goes
    /// for a vehicle even when a soldier is nearer, and an IFV without a
    /// passenger goes for what flies. Both are answers from the capability
    /// table, not from a name list in the target loop.
    #[test]
    fn a_pilots_preference_changes_what_it_shoots_first() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        let gi = sim
            .spawn_object("E1", "Russians", 21, 20, 255, &rules)
            .expect("a conscript");
        let tank = sim
            .spawn_object("MTNK", "Russians", 23, 20, 255, &rules)
            .expect("a Rhino");
        let orca = sim
            .spawn_object("ORCA", "Americans", 24, 20, 255, &rules)
            .expect("an aircraft");
        let obs = Observation::new(&sim, owner);
        let enemies = [
            (gi, (21u16, 20u16), false, false),
            (tank, (23, 20), false, false),
        ];
        assert_eq!(
            in_reach(
                enemies.into_iter(),
                (20, 20),
                6,
                |_| true,
                |target| prefer(
                    &rules,
                    &obs,
                    super::super::pilots::Preference::Armour,
                    target,
                    false
                )
            ),
            Some((tank, (23, 20))),
            "the parasite goes for the vehicle, not the nearer soldier"
        );
        let mixed = [
            (tank, (21u16, 20u16), false, false),
            (orca, (24, 20), false, false),
        ];
        assert_eq!(
            in_reach(
                mixed.into_iter(),
                (20, 20),
                6,
                |_| true,
                |target| prefer(&rules, &obs, super::super::pilots::Preference::Air, target, false)
            ),
            Some((orca, (24, 20))),
            "the gunner goes for what flies"
        );
    }

    /// The gun pointed at this unit is answered first, even when something
    /// else is nearer: a unit that shells the crowd in front of it while a
    /// tank behind it fires is a unit that dies to the one it ignored.
    #[test]
    fn the_gun_pointed_at_this_unit_is_the_first_answer() {
        // 7 is three cells away and shooting; 8 is next to the unit and is not.
        let enemies = [
            (7u64, (23u16, 20u16), false, true),
            (8u64, (21u16, 20u16), false, false),
        ];
        assert_eq!(
            in_reach(enemies.into_iter(), (20, 20), 5, |_| true, |_| 0),
            Some((7, (23, 20))),
            "the one shooting this unit, not the nearest one"
        );
    }

    /// A mobile hostile outranks a wall at the same distance: the one that
    /// shoots back is the one that kills a unit busy shelling a building.
    #[test]
    fn a_real_shot_beats_a_nominally_preferred_pursuit() {
        let targets=[(7u64,(23u16,20u16),false,false),(8,(22,20),true,false)];
        assert_eq!(in_reach_ready(targets.into_iter(),(20,20),5,|_|true,
            |id|if id==7 {0}else{3},|id|id==8),Some((8,(22,20))));
    }

    #[test]
    fn a_mobile_hostile_outranks_a_structure_in_reach() {
        // 8 is the building at two cells, 9 the rifleman at three.
        let enemies = [
            (8u64, (22u16, 20u16), true, false),
            (9u64, (23u16, 20u16), false, false),
        ];
        assert_eq!(
            in_reach(enemies.into_iter(), (20, 20), 4, |_| true, |_| 0),
            Some((9, (23, 20))),
            "the soldier, not the closer building"
        );
        assert_eq!(
            in_reach(
                enemies.into_iter(),
                (20, 20),
                4,
                |target| target == 8,
                |_| 0
            ),
            Some((8, (22, 20))),
            "the building is still taken when nothing else can be hurt"
        );
    }

    /// The nearest hostile is not the answer when the unit cannot act on it.
    /// A dog beside a tank and one cell from a rifleman goes for the rifleman:
    /// the weapon test is part of the search, not a filter applied to its
    /// result — otherwise the nearer tank would hide the soldier behind it.
    #[test]
    fn the_search_skips_what_the_weapon_cannot_take() {
        // 8 is the tank at one cell, 9 the rifleman at two.
        let enemies = [
            (8u64, (21u16, 20u16), false, false),
            (9u64, (22u16, 20u16), false, false),
        ];
        let can_hurt = |target: u64| target == 9;
        assert_eq!(
            in_reach(enemies.into_iter(), (20, 20), 2, can_hurt, |_| 0),
            Some((9, (22, 20))),
            "the rifleman the jaw can take, not the nearer tank"
        );
        assert_eq!(
            in_reach(enemies.into_iter(), (20, 20), 2, |_| false, |_| 0),
            None,
            "nothing it can act on is nothing to do"
        );
    }

    /// The nearest hostile is the one a scout is sent to look at.
    #[test]
    fn a_scout_is_sent_to_the_nearest_thing_it_can_see() {
        let enemies = vec![
            EnemyEntity {
                entity_id: 7,
                type_id: InternedId::default(),
                cell: (40, 40),
            },
            EnemyEntity {
                entity_id: 9,
                type_id: InternedId::default(),
                cell: (21, 20),
            },
        ];
        assert_eq!(nearest(&enemies, (20, 20)), Some((9, (21, 20))));
    }

    /// A scout is not sent to look at a place it cannot get to.
    ///
    /// The map is cut in two by open water: the scout stands on the bank it
    /// has already seen, and the only thing left to look at is the far bank,
    /// which a walker has no way to reach. Asking only "is the cell open and
    /// is it dark" sent the scout to walk into the sea, and it stood at the
    /// water's edge until the order was taken away from it.
    ///
    /// The movement zone is the seam this is asked through, and that is what
    /// makes it the seam for the sea: the same question for a dog and a
    /// hovercraft gives different answers, with nobody keeping a list of what
    /// can swim.
    #[test]
    fn a_scout_is_not_sent_where_it_cannot_get_to() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
        let owner = sim.interner.intern("Americans");
        let size = 32u16;
        // A two-cell stretch of open water down the map: the far bank is an
        // island to anything that cannot cross it.
        crate::sim::arena_fixture::flood(
            &mut sim,
            &rules,
            size,
            (18u16..20).flat_map(|rx| (0u16..size).map(move |ry| (rx, ry))),
        );
        sim.rebuild_dynamic_navigation(&rules);
        // The home bank has been seen; everything past the water has not.
        let mut fog = crate::sim::vision::FogState::new(size, size);
        fog.reveal_cells_for_owner(
            owner,
            (0u16..18).flat_map(|rx| (0u16..size).map(move |ry| (rx, ry))),
        );
        sim.fog = fog;
        let obs = Observation::new(&sim, owner);
        assert_eq!(
            nearest_unexplored(&obs, (1, 16), MovementZone::Infantry),
            None,
            "the far bank is not somewhere a dog can go and look"
        );
    }

    /// A tank beside a rifleman flattens him: `[MTNK] Crusher=yes` against
    /// `[E1]`'s default `Crushable=yes`. The IFV cannot (`Crusher=no`), and the
    /// native approach arm is the computer house's, so the brain has to ask the
    /// world before it drives a tank anywhere.
    #[test]
    fn a_crusher_flattens_infantry_and_an_ifv_cannot() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        let tank = sim
            .spawn_object("MTNK", "Americans", 20, 20, 255, &rules)
            .expect("a Grizzly");
        let gi = sim
            .spawn_object("E1", "Russians", 21, 20, 255, &rules)
            .expect("a conscript");
        let ifv = sim
            .spawn_object("FV", "Americans", 24, 20, 255, &rules)
            .expect("an IFV");
        let other = sim
            .spawn_object("E1", "Russians", 25, 20, 255, &rules)
            .expect("a conscript");
        let obs = Observation::new(&sim, owner);
        assert!(obs.crushes(&rules, tank, gi), "a Grizzly crushes a soldier");
        assert!(!obs.crushes(&rules, ifv, other), "an IFV has no Crusher=");
        assert!(crush_adjacent((20, 20), (21, 20)));
        assert!(!crush_adjacent((20, 20), (22, 20)));
    }

    /// A dug-in Guardian GI still shoots: `[GGI] DeployFire=yes` with the
    /// missile in slot 1 is the deployed weapon. The brain measures a
    /// deployed soldier's reach through that slot, and a zero there is an
    /// order path with nothing to ask for — the deployed unit that stands in
    /// front of a tank and never fires.
    #[test]
    fn a_deployed_guardian_gi_has_a_reach_to_measure() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let obs = Observation::new(&sim, crate::sim::intern::InternedId::from_index(0));
        let ty = obs
            .type_id("GGI")
            .and_then(|type_id| obs.object_type(&rules, type_id))
            .expect("the Guardian GI is in the rules");
        let standing = super::super::advance::weapon_range_cells(&rules, ty, false);
        let dug_in = super::super::advance::weapon_range_cells(&rules, ty, true);
        assert!(standing > 0, "the standing soldier reaches: {standing}");
        assert!(
            dug_in > 0,
            "the dug-in one has the missile to fire: {dug_in}"
        );
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v26_personnel_ambush_tests {
    use super::*;

    #[test]
    fn mirage_tanks_prioritize_infantry_above_masonry() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
            else { return; };
        let Some(mirage) = rules.object("MGTK") else { return; };
        assert!(mirage.disguise_when_still);
        assert!(mirage_target_preference(Some(ObjectCategory::Infantry))
            < mirage_target_preference(Some(ObjectCategory::Building)));
        assert!(mirage_target_preference(Some(ObjectCategory::Vehicle))
            < mirage_target_preference(Some(ObjectCategory::Building)));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod priority_regression_tests {
    use super::*;

    #[test]
    fn neutral_wall_attack_does_not_lock_out_enemy_response() {
        assert!(!preserve_existing_attack(true,false,false));
        assert!(!preserve_existing_attack(true,true,true));
        assert!(preserve_existing_attack(true,true,false));
        assert!(!preserve_existing_attack(false,true,false));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v41_slaveminer_raid_tests {
    use super::*;

    #[test]
    fn peripheral_raids_prefer_unarmed_power_and_production_not_towers() {
        let Some(rules)=crate::rules::retail_ini_fixture::retail_unlayered_rules() else {return;};
        assert_eq!(peripheral_raid_rank(rules.object("GAPOWR").unwrap()),Some(4));
        assert_eq!(peripheral_raid_rank(rules.object("GAWEAP").unwrap()),Some(5));
        assert_eq!(peripheral_raid_rank(rules.object("GTGCAN").unwrap()),None);
        assert_eq!(peripheral_raid_rank(rules.object("ATESLA").unwrap()),None);
    }

    #[test]
    fn deployed_slave_mine_precedes_mobile_miner_and_normal_refinery() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let deployed = rules.object("YAREFN").expect("Yuri deployed slave mine");
        let mobile = rules.object("SMIN").expect("Yuri mobile slave miner");
        assert!(deployed.enslaves.is_some() && mobile.enslaves.is_some());
        assert_eq!(mining_objective_rank(deployed, true, true), Some(0));
        assert_eq!(mining_objective_rank(mobile, true, true), Some(1));
        let refinery = rules.object("GAREFN").expect("normal refinery");
        assert_eq!(mining_objective_rank(refinery, false, true), Some(3));
    }
}
