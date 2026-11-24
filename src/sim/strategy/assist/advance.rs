//! Sending the army out.
//!
//! Native carries the attack in teams: an `AITrigger` creates one and its
//! script assigns the missions. A brain-driven house never gets an `AITrigger`
//! — `HouseClass::Update`'s team block is gated on "no human controls it" —
//! so without this module its army sits on area guard for the whole match and
//! the house can only lose by attrition.
//!
//! This is the player's substitute, and deliberately two rules. The first is
//! *when*: an army large enough to be worth spending, heavy enough to beat what
//! is standing where it is going ([`super::squads::outguns`]), a base that can
//! replace it, and nothing already at the yard. The second is *where*: one structure
//! for the whole army — the enemy economy structure nearest the first fighter
//! in entity-store order — because a fist takes a base, and units that walk
//! at their own nearest building arrive one at a time and die one at a time.
//!
//! One goal for the army is a measurement, not a preference. Three variants
//! were run end to end against the pinned `Death.mmx` match and all three
//! lost the push the current rule wins: aiming each unit at the structure its
//! own weapons answer best, aiming the fist from the army's centre instead of
//! that first fighter, and holding one target until it is destroyed instead
//! of re-reading the nearest one each frame (`super::RESIDUALS` has the
//! ticks). So the *target* stays one, and what differs per unit is the
//! **order**: a unit already inside its own weapon's reach of that structure
//! attacks it from where it stands, and one further out attack-moves toward
//! it. That keeps the fist together — the failure the three variants shared
//! was dispersion, not per-unit decisions — while stopping the army from
//! walking its short-ranged units point-blank into a base to shoot at all.
//!
//! While it is pushing it owns the whole army, so the area-guard step stands
//! down for the frame; a house that has not got an army yet keeps holding its
//! base, which is what a player does with five tanks and no plan.

use std::collections::BTreeSet;

use crate::rules::object_type::{ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::{EnemyEntity, Observation};
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};

use super::AssistBrain;
use super::census::{Census, Unit};
use super::squared_distance;

mod targets;

/// How large the army has to be before it is worth pushing with: the teams'
/// own number ([`super::squads::ARMY_TO_ATTACK`]), because "how big is big
/// enough" is one question with one owner.
///
/// Below it the opening force is the whole army, and an opening force that
/// walks into the enemy's pre-placed garrison is a house that has traded its
/// base for nothing — which is why the surplus goes harassing instead.

/// Bound the strategic retask batch without leaving half a starting army idle.
/// The interactive L2 command budget is 512; eight was a stale L3 bottleneck.
const ADVANCE_ORDERS_PER_FRAME: usize = 24;
/// Hold a dispersed armoured fist briefly rather than launching a single
/// heavy tank as each factory completes it. A dead economy must not stall
/// an otherwise viable attack forever.
const ARMOUR_ASSEMBLE_FRAMES: u64 = 900;
const ARMOUR_WAVE_MIN: usize = 3;

fn armoured_fighter(unit: &Unit, rules: &RuleSet, obs: &Observation<'_>) -> bool {
    unit.slot == super::census::Slot::Vehicle && unit.traits.combat
        && obs.object_type(rules, unit.type_id).is_some_and(|ty| {
            ty.category == crate::rules::object_type::ObjectCategory::Vehicle
                && ty.strength >= 300 && !ty.resource_gatherer
        })
}

fn armour_wave_missing(close: usize, waiting: u64) -> bool {
    close < ARMOUR_WAVE_MIN && waiting < ARMOUR_ASSEMBLE_FRAMES
}

/// A distant army is not a column standing beside its lead scout. Requiring
/// actual local companions prevents a lone vehicle from probing a prism or
/// heavy artillery emplacement while the main fist is still on the road.
fn battery_wave_ready(nearby: usize) -> bool {
    nearby >= ARMOUR_WAVE_MIN
}

/// A bounded, actual same-frame cohort, not a siege timer that lets one tank
/// decide to walk at a Prism Tower after the rest of the column went home.
#[derive(Debug, Clone)]
pub(super) struct SiegeCharge {
    pub(super) target: u64,
    pub(super) until: u64,
    pub(super) members: BTreeSet<u64>,
}

/// Every participating vehicle must be concentrated, healthy and capable of
/// damaging the battery. Overall army points elsewhere on the map are not a
/// substitute. Standoff artillery takes precedence if it can safely shoot.
fn mass_charge_ready(ready: usize, guns: usize, ours: i32, enemy: i32,
    has_standoff: bool) -> bool {
    let required = (6usize.saturating_add(guns.saturating_mul(2))).min(18);
    !has_standoff && ready >= required && ours >= 4_000
        && ours >= enemy.max(1).saturating_mul(2)
}

/// Continue a coherent charge as a unit operation. Retreat/micro are allowed
/// outside the short window; a dead or invisible target cancels the order.
pub(super) fn charge_member(brain: &AssistBrain, tick: u64, actor: u64) -> bool {
    brain.siege_charge.as_ref().is_some_and(|wave|
        tick < wave.until && wave.members.contains(&actor))
}

/// Called before tactical passes so a charged cohort is not scattered by
/// dodge/kite. The decision reads only currently visible, operational guns.
pub(super) fn prepare_siege_charge(
    brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
) {
    let tick = obs.tick();
    if let Some(wave) = &brain.siege_charge {
        if tick < wave.until && obs.visible_enemy_entities().any(|enemy|
            enemy.entity_id == wave.target
                && obs.visible_enemy_building_operational(rules, enemy.entity_id))
        { return; }
        brain.siege_charge = None;
    }
    if tick % super::tracked::MICRO_FRAMES != 0 { return; }
    if brain.census.base_cell.is_some_and(|home|
        brain.census.enemies_near(home, THREAT_RADIUS) > 0)
    { return; }
    let Some((_, _, campaign_cell)) = brain.push_target else { return; };
    let Some(squad) = brain.squads.squad(super::squads::Role::Attack) else { return; };
    let towers: Vec<_> = obs.visible_enemy_entities().filter(|enemy|
        obs.entity_type(rules, enemy.entity_id).is_some_and(|ty| ty.is_base_defense)
            && obs.visible_enemy_building_operational(rules, enemy.entity_id)
            && cell_distance(enemy.cell, campaign_cell) <= 14
    ).collect();
    // The current attacker must be near enough for a charge to be a real
    // coordinated engagement, not a remote all-map command.
    let battery = towers.iter().copied().filter(|enemy| {
        squad.members.iter().any(|id| brain.census.units.iter().any(|unit|
            unit.entity_id == *id && cell_distance(unit.cell, enemy.cell) <= 24))
    }).max_by_key(|enemy| {
        let reach = obs.entity_type(rules, enemy.entity_id)
            .map_or(0, |ty| weapon_range_cells(rules, ty, false));
        (reach, std::cmp::Reverse(enemy.entity_id))
    });
    let Some(battery) = battery else { return; };
    let mut candidates: Vec<&Unit> = brain.census.units.iter().filter(|unit|
        squad.members.contains(&unit.entity_id) && unit.controllable()
            && !unit.in_service() && !unit.traits.yard_maker
            && !unit.traits.harvester && !unit.traits.irreplaceable
            && !unit.traits.carries && !unit.traits.suicide
            && unit.health_permille >= 750 && armoured_fighter(unit, rules, obs)
            && obs.can_hurt(rules, unit.entity_id, battery.entity_id)
            && super::orders::reachable(rules, obs, unit.entity_id, battery.cell)
    ).collect();
    if candidates.is_empty() { return; }
    let tower_reach = candidates.iter().map(|unit|
        super::tactics::effective_reach_against(rules, obs, battery.entity_id, unit.entity_id)
    ).max().unwrap_or(0);
    if tower_reach < 7 { return; }
    let has_standoff = brain.census.units.iter().filter(|unit|
        squad.members.contains(&unit.entity_id) && unit.controllable()
    ).any(|unit| obs.can_hurt(rules, unit.entity_id, battery.entity_id)
        && super::tactics::effective_reach_against(rules, obs,
            unit.entity_id, battery.entity_id) > tower_reach + 1);
    // Form a single real platoon around its foremost tank. A far-away army
    // counted as support is the original source of solo tower probes.
    candidates.sort_by_key(|unit| (cell_distance(unit.cell, battery.cell), unit.entity_id));
    let leader = candidates[0].cell;
    let mut ready: Vec<&Unit> = candidates.into_iter().filter(|unit|
        cell_distance(unit.cell, leader) <= 9
            && cell_distance(unit.cell, battery.cell) <= tower_reach + 11
    ).collect();
    ready.sort_by_key(|unit| unit.entity_id);
    let our_points = ready.iter().fold(0i32, |total, unit|
        total.saturating_add(unit.points));
    let enemy_points = brain.census.enemy_points(rules, obs, battery.cell, 16);
    if !mass_charge_ready(ready.len(), towers.len(), our_points,
        enemy_points, has_standoff)
    { return; }
    let members = ready.into_iter().take(24).map(|unit| unit.entity_id).collect();
    brain.siege_charge = Some(SiegeCharge {
        target: battery.entity_id, until: tick.saturating_add(360), members,
    });
    log::info!("auto play: concentrated armoured battery assault target={} at {:?}",
        battery.entity_id, battery.cell);
}

fn execute_siege_charge(
    siege_charge: &Option<SiegeCharge>, census: &Census,
    tracked: &mut super::tracked::Tracker, push_gate: &mut &'static str,
    rules: &RuleSet, obs: &Observation<'_>, out: &mut Outbox,
) -> bool {
    let Some(wave) = siege_charge.as_ref() else { return false; };
    if obs.tick() >= wave.until { return false; }
    let target = wave.target;
    let members: Vec<u64> = wave.members.iter().copied().collect();
    let Some(enemy) = obs.visible_enemy_entities().find(|enemy|
        enemy.entity_id == target && obs.visible_enemy_building_operational(rules, target))
    else { return false; };
    let mut issued = 0;
    for id in members {
        if issued >= ADVANCE_ORDERS_PER_FRAME { break; }
        let Some(unit) = census.units.iter().find(|unit|
            unit.entity_id == id && unit.controllable() && !unit.in_service())
        else { continue; };
        if !obs.can_hurt(rules, id, target)
            || obs.attack_target(id) == Some(target) { continue; }
        let goal = Goal::Attack { target };
        if !super::orders::admits(rules, obs, id, &goal) { continue; }
        // One synchronized charge uses a rank above the ordinary firing
        // cycle. This is a native Attack command, never a direct sim write.
        if out.want(id, 2, goal) {
            tracked.record_goal(obs.tick(), id,
                super::tracked::TrackedKind::Attack,
                Some(target), Some(enemy.cell), goal);
            issued += 1;
        }
    }
    *push_gate = "siege-mass-charge";
    true
}

fn isolated_battery_approach(
    fighters: &[&Unit], rules: &RuleSet, obs: &Observation<'_>,
    actor: u64, from: (u16,u16), to: (u16,u16),
) -> Option<((u16,u16), i32)> {
    use crate::rules::object_type::ObjectCategory;
    let ty = obs.entity_type(rules, actor)?;
    if !matches!(ty.category, ObjectCategory::Vehicle | ObjectCategory::Infantry)
        || ty.naval { return None; }
    obs.visible_enemy_entities().filter_map(|enemy| {
        let gun = obs.entity_type(rules, enemy.entity_id)?;
        if !gun.is_base_defense
            || !obs.visible_enemy_building_operational(rules, enemy.entity_id)
        { return None; }
        let reach = super::tactics::effective_reach_against(
            rules, obs, enemy.entity_id, actor);
        if reach < 7 { return None; }
        // Three nearby dogs are not a three-vehicle attack wave. Only units
        // whose *current native weapons* can actually harm this emplacement
        // qualify as nearby support. This remains unit-name agnostic for MODs.
        let nearby = fighters.iter().filter(|friend| {
            friend.controllable() && friend.traits.combat && !friend.in_service()
                && cell_distance(friend.cell, from) <= 9
                && obs.can_hurt(rules, friend.entity_id, enemy.entity_id)
        }).count();
        if battery_wave_ready(nearby) { return None; }
        let in_lane = cell_distance(enemy.cell, from) <= reach + 2
            || cell_distance(enemy.cell, to) <= reach + 2
            || super::orders::path_crosses(&[enemy.cell], from, to, reach + 2);
        if !in_lane { return None; }
        let own = super::tactics::actual_threat_reach(
            rules, obs, actor, enemy.entity_id);
        // Already within safe firing range? Shoot instead of staging again.
        let safe_shot = own > reach + 1
            && cell_distance(from, enemy.cell) <= own
            && cell_distance(from, enemy.cell) > reach + 1
            && !super::tactics::unsafe_post(rules, obs, actor, from);
        (!safe_shot).then_some((enemy.cell, reach))
    }).max_by_key(|(cell, reach)| (*reach, std::cmp::Reverse(cell_distance(from,*cell))))
}

/// How far behind the leading fighter a unit may fall before the push takes it
/// again, in cells. A unit that has nothing to walk to keeps whatever it was
/// told last, which is right for the tip and wrong for the tail: without this
/// the tail stands where it stopped for the rest of the match.
const STAGGER_BEHIND: i32 = 10;

/// How close an enemy has to come to the yard before the army stays home,
/// in cells. A raid inside this ring is what the army was bought for.
///
/// [`super::squads`] answers the raid itself from the same radius: a smaller
/// one there left a band where the push stopped for a threat no team was sent
/// at, so the whole army stood in the yard while the raid burned it.
pub(super) const THREAT_RADIUS: i32 = 18;

/// How long the army may go without taking an enemy object before the push
/// stops waiting for a favourable comparison, in frames: one minute.
const PUSH_STALL_FRAMES: u64 = 900;

/// How long the army stays home after a gate turned it back, in frames: fifty
/// seconds.
///
/// The gates below are read fresh every frame, and one of them — `outgunned` —
/// compares the army against whatever stands around the frame's target, which
/// moves as the army moves. Measured on Death.mmx seed 24301, the consequence
/// was a gate that flipped four times in ten thousand frames: `push` for ten
/// samples, `outgunned` for eight, `push` again, `outgunned` again. Every flip
/// re-tasks the whole fist, which from the player's side is an army walking out
/// and back rather than fighting.
///
/// The window is asymmetric on purpose. Turning back is a verdict about this
/// second and takes effect at once; coming out again waits. Fifty seconds is
/// long enough for a wave of attackers to cross the ground the re-task would
/// have spent, and short enough that an army that outgrew the gate is not held
/// at home for a minute of the match.

/// Whether the push has stalled: the enemy has not lost an object since
/// `progress` was stamped, and a minute has passed since then.
///
/// `progress` is `(frame, enemy count)`, stamped when the army first became
/// large enough to push and refreshed every time the count falls. Before the
/// first stamp the answer is no: an opening army is not stalled, it is small.
fn push_stalled(progress: (u64, usize), now: u64, current: usize) -> bool {
    let (since, then) = progress;
    since != 0 && current >= then && now.saturating_sub(since) >= PUSH_STALL_FRAMES
}

/// Where the army looks when it knows of nothing to attack: the nearest
/// ground nobody has seen, else the middle of the map.
fn searching_cell(obs: &Observation<'_>, anchor: (u16, u16)) -> Option<(u16, u16)> {
    if let Some(cell) = super::micro::nearest_unexplored(
        obs,
        anchor,
        crate::rules::locomotor_type::MovementZone::Normal,
    ) {
        return Some(cell);
    }
    let (width, height) = obs.map_extent();
    (width > 0 && height > 0).then_some((width / 2, height / 2))
}

/// How near a crate has to be to the cell the army walks to for the walk to
/// go over it, in cells.
///
/// Three is the crate's own reach plus a cell: the box is on the way, not on
/// the target.
fn siege_blackout_wait(start: u64, tick: u64, live_power_infiltration: bool) -> bool {
    live_power_infiltration && tick.saturating_sub(start) < 1_500
}

#[cfg(all(test, tests_sim_p_s))]
mod v26_siege_coordination_tests {
    use super::{battery_wave_ready, siege_blackout_wait};

    #[test]
    fn waiting_for_a_real_spy_blackout_is_finite() {
        assert!(siege_blackout_wait(100, 700, true));
        assert!(!siege_blackout_wait(100, 700, false));
        assert!(!siege_blackout_wait(100, 1_600, true));
    }

    #[test]
    fn artillery_does_not_probe_a_battery_until_nearby_comrades_arrive() {
        assert!(!battery_wave_ready(1));
        assert!(!battery_wave_ready(2));
        assert!(battery_wave_ready(3));
    }
}

const CRATE_ON_THE_WAY_CELLS: i32 = 3;

/// The crate the walk to `cell` passes, if there is one.
///
/// The nearest one wins, so the army takes the first box on its way and not
/// the one across the map.
fn crate_on_the_way(
    rules: &RuleSet,
    obs: &Observation<'_>,
    cell: (u16, u16),
) -> Option<(u16, u16)> {
    obs.crate_cells(rules)
        .into_iter()
        .filter(|crate_cell| {
            squared_distance(*crate_cell, cell) <= CRATE_ON_THE_WAY_CELLS * CRATE_ON_THE_WAY_CELLS
        })
        .min_by_key(|crate_cell| (squared_distance(*crate_cell, cell), *crate_cell))
}

/// How long the fist keeps the structure it committed to, in frames: two
/// minutes at the simulation's fifteen a second.
///
/// Long enough to walk there and take it apart, short enough that a target the
/// army cannot reach stops holding it.
const PUSH_LOCK_FRAMES: u64 = 1_800;

const PUSH_REST_FRAMES: u64 = 750;

/// How few enemy objects are left before the strength gate stops asking. Below
/// this the enemy is a clean-up job, not a fight: the army goes in and finishes
/// it instead of waiting for a comparison it can only lose by arithmetic.
const MOP_UP_OBJECTS: usize = 8;

/// Whether the enemy is a clean-up job: too little mobile mass left to be a
/// fight, whatever the arithmetic says about the fist.
///
/// The wishlist's stalemate — "最后消灭了大部分敌人，但Autoplay也不发起进攻，然后就
/// 僵持在这了。发动攻击后总会剩下敌人的建筑不再打了". The absolute
/// [`ARMY_TO_ATTACK`] floor is an opening number, and it had no bypass: a house
/// with nine fighters and an enemy holding two buildings sat at `too-small` for
/// the rest of a nine-hour match while the buildings stood.
///
/// The count is the enemy mobile mass **this house can see near its own base**,
/// not the sum over the whole map. On a 1v29 that sum never falls below eight
/// while twenty-nine houses are alive, so the bypass never fired and the floor
/// it exists to relieve held the army home for the entire match. What the
/// bypass is for is the local stalemate a player reads off the screen: the
/// ground around the base is clear, so the army is not being kept to defend
/// anything, and what remains is a march.
///
/// `home` is the house's base cell; the read is [`THREAT_RADIUS`] around it,
/// the same ring [`base_under_threat`] calls "the enemy is at the gate".
fn mop_up(census: &Census) -> bool {
    let Some(home) = census.base_cell else {
        // No base to defend: nothing is holding the army where it stands.
        return true;
    };
    census.enemies_near(home, THREAT_RADIUS) < MOP_UP_OBJECTS
}

/// The clean-up bypass reads the ground around the base, not the whole map.
///
/// The whole-map sum is what made it dead on a 1v29: twenty-nine houses keep
/// more than [`MOP_UP_OBJECTS`] bodies on the field for the entire match, so the
/// bypass never fired and the `ARMY_TO_ATTACK` floor it exists to relieve held
/// the army home until the base fell.
#[test]
fn the_clean_up_bypass_reads_the_base_ring_not_the_whole_map() {
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        .expect("the repository's own rules");
    let mut sim = crate::sim::world::Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    let mut census = Census::default();
    census.refresh(
        &crate::rules::unit_role::RoleTable::build(&rules),
        &rules,
        &Observation::new(&sim, owner),
    );

    // No base at all: nothing is being held at home.
    census.base_cell = None;
    assert!(
        mop_up(&census),
        "a house with no base has nothing to defend"
    );

    // A base with an empty ring, while the map elsewhere is full of enemies:
    // the sum is large, the ring is clear, and the army is free to march.
    let home = (20, 20);
    census.base_cell = Some(home);
    census.threat = [200, 80, 0, 0, 0, 0, 0];
    census.enemies = Vec::new();
    assert!(
        mop_up(&census),
        "a full map elsewhere does not hold the army at a clear base"
    );

    // The same ring with a field army standing in it is not a clean-up job. The
    // bypass is for "a handful left", so the raid has to be more than a handful:
    // one straggler at the gate is exactly what it is meant to wave through.
    census.enemies = (0..MOP_UP_OBJECTS)
        .map(|index| {
            let offset = u16::try_from(index).unwrap_or(0);
            crate::sim::control::observation::EnemyEntity {
                entity_id: u64::try_from(index).unwrap_or(0),
                type_id: crate::sim::intern::InternedId::default(),
                cell: (home.0.wrapping_add(offset), home.1),
            }
        })
        .collect();
    assert!(
        !mop_up(&census),
        "a field army at the gate keeps the army home: nearby={}",
        census.enemies_near(home, THREAT_RADIUS)
    );
}

/// Send the army at the enemy's base when it is large enough. `false` when
/// the house is not pushing this frame, so the caller keeps its own posture.
/// Whether the army is still inside a commit window a gate opened.
///
/// Turning back is a verdict about this second and takes effect at once; coming
/// out again waits [`PUSH_REST_FRAMES`]. The asymmetry is the whole point — an
/// army that leaves the frame a comparison improves walks out and back, which is
/// what the measured gate flapping looked like.
fn resting(brain: &AssistBrain, tick: u64) -> bool {
    brain.push_rest.0 != 0 && tick.saturating_sub(brain.push_rest.0) < PUSH_REST_FRAMES
}

/// Whether `unit`'s type answers the job `target`'s armour asks for.
///
/// The native fire test this replaces asks whether the gun can point at the
/// target at all — a verses line, non-zero or not. That is the wrong question
/// for a Mirage Tank in front of a concrete wall. `MirageWH` sits at 30/20/20
/// percent against the three structure armours, which is non-zero, so
/// `Engage::Ready` says yes, and the house spends 1000 credits to do a fifth of
/// a Grizzly's work against a building it cannot afford to trade for. The role
/// table answers the question the player is asking: is this unit *for* this job.
///
/// The bar is [`ROLE_FLOOR`] — the same quarter-of-the-best the census counts as
/// "able". A unit under it is not bad at the job; it is for a different one, and
/// the house has one of those in every slot. That reading is also why the answer
/// stays a *relative* one: the weights are scaled against the best the same rule
/// set offers, so a mod that swaps the line tank changes which unit leads and
/// never needs this function to change with it.
///
/// A target whose type or armour the ledger cannot describe answers `true`: the
/// refusal exists to hold back the wrong specialist, not to hold the whole fist
/// home over an unknown.
#[allow(dead_code)]
fn able_for(
    census: &Census,
    unit_type: InternedId,
    target: u64,
    rules: &RuleSet,
    obs: &Observation<'_>,
) -> bool {
    let Some(target_type) = obs.entity_type(rules, target) else {
        return true;
    };
    let Some(role) = crate::rules::unit_role::role_against_armour(target_type.armor_id()) else {
        return true;
    };
    census.facts(unit_type).is_none_or(|facts| {
        // Against the unit's *own* best answer, not against the ruleset's. The
        // question is "is this its job?", and a Mirage is 85.7 against armour
        // and 20.0 against concrete: 23% of its own best is a specialist being
        // sent to do something else, and the refinery it would take an hour to
        // chew through is one a Grizzly (36.8 against concrete, 62% of its own
        // best) takes in a minute. The ruleset-wide floor cannot tell those two
        // apart, because it asks how good anything is against buildings rather
        // than how good *this* is.
        let own_best = crate::rules::unit_role::ROLES
            .iter()
            .map(|role| facts.weights.get(*role))
            .fold(0.0_f64, f64::max);
        own_best <= 0.0 || facts.weights.get(role) >= own_best * crate::rules::unit_role::ROLE_FLOOR
    })
}

/// Only regroup *ground vehicles* and only when dispersed far behind a
/// leading vehicle that is still outside the battle. The caller keeps its
/// original single enemy target; this waypoint is solely a marching aid for
/// the tail, never a new target-priority algorithm.
fn needs_tank_cohesion(unit: &Unit, post: (u16,u16)) -> bool {
    unit.slot == super::census::Slot::Vehicle
        && unit.traits.combat && unit.controllable() && !unit.in_service()
        && cell_distance(unit.cell, post) > STAGGER_BEHIND + 4
}

fn tank_cohesion_post(
    fighters: &[&Unit], target: (u16,u16),
    obs: &Observation<'_>, census: &Census,
) -> Option<(u16,u16)> {
    let mut tanks: Vec<&Unit> = fighters.iter().copied().filter(|u| {
        u.slot == super::census::Slot::Vehicle
            && u.traits.combat && u.controllable() && !u.in_service()
    }).collect();
    if tanks.len() < 6 {
        return None;
    }
    tanks.sort_by_key(|u| (cell_distance(u.cell, target), u.entity_id));
    let lead = tanks[0].cell;
    let tail = tanks.last()?.cell;
    if cell_distance(lead, target) < 20
        || cell_distance(lead, tail) < STAGGER_BEHIND * 2
        || census.enemies.iter().any(|enemy| {
            obs.is_cell_visible(enemy.cell.0, enemy.cell.1)
                && cell_distance(enemy.cell, lead) <= 12
        })
    {
        return None;
    }
    // Move the destination four cells from the leader toward the tail. A
    // cell *on* the tank is normally occupied, so take a walkable nearby cell.
    let dx = (i32::from(tail.0) - i32::from(lead.0)).signum();
    let dy = (i32::from(tail.1) - i32::from(lead.1)).signum();
    for distance in [4i32,3,5,2] {
        let (Ok(x),Ok(y)) = (
            u16::try_from(i32::from(lead.0) + distance * dx),
            u16::try_from(i32::from(lead.1) + distance * dy),
        ) else { continue; };
        if obs.is_ground_walkable(x,y) {
            return Some((x,y));
        }
    }
    None
}

/// The screen goes behind *actual nearby armour*, rather than charging a
/// fragile rifleman ahead of the tanks. Only applies while marching across
/// safe ground: once in contact, micro/engage owns both units.
fn infantry_screen_post(
    fighters: &[&Unit], soldier: &Unit, target: (u16,u16),
    rules: &RuleSet, obs: &Observation<'_>,
) -> Option<(u16,u16)> {
    if soldier.slot != super::census::Slot::Infantry || !soldier.traits.combat
        || soldier.traits.irreplaceable { return None; }
    let armor = fighters.iter().copied()
        .filter(|unit| unit.slot == super::census::Slot::Vehicle
            && unit.traits.combat && unit.controllable() && !unit.in_service()
            && unit.points >= soldier.points)
        .min_by_key(|unit| (cell_distance(unit.cell, target), unit.entity_id))?;
    let armor_distance = cell_distance(armor.cell, target);
    if armor_distance <= 8 || cell_distance(soldier.cell, target) + 3 >= armor_distance
        || super::micro::firing(obs, soldier.mission, soldier.entity_id)
    { return None; }
    let (dx,dy) = (
        (i32::from(armor.cell.0) - i32::from(target.0)).signum(),
        (i32::from(armor.cell.1) - i32::from(target.1)).signum(),
    );
    for offset in [3,4,2] {
        let (Ok(x),Ok(y)) = (
            u16::try_from(i32::from(armor.cell.0) + offset * dx),
            u16::try_from(i32::from(armor.cell.1) + offset * dy),
        ) else { continue; };
        let cell=(x,y);
        if obs.is_ground_walkable(x,y)
            && super::orders::reachable(rules,obs,soldier.entity_id,cell)
            && !super::tactics::unsafe_post(rules,obs,soldier.entity_id,cell)
        { return Some(cell); }
    }
    None
}

/// During an artillery preparation window, standing under enemy guns is
/// worse than waiting. Move the *current* front back to a lawful standoff
/// ring, and send late reinforcements to that ring rather than into the tower.
/// This is a temporary detour: the original push target stays in `advance`.
fn stage_siege(
    tracker: &mut super::tracked::Tracker, trace: Option<u64>,
    fighters: &[&Unit], rules: &RuleSet, obs: &Observation<'_>,
    out: &mut Outbox, target: (u16,u16),
) -> usize {
    let gunline: Vec<_> = obs.visible_enemy_entities().filter_map(|enemy| {
        let ty = obs.entity_type(rules,enemy.entity_id)?;
        if !ty.is_base_defense
            || !obs.visible_enemy_building_operational(rules, enemy.entity_id)
            || cell_distance(enemy.cell,target) > 14 { return None; }
        let reach = weapon_range_cells(rules,ty,obs.is_deployed(enemy.entity_id));
        (reach >= 4).then_some((enemy.entity_id,enemy.cell,reach))
    }).collect();
    let mut issued = 0;
    for unit in fighters.iter().cycle()
        .skip((obs.tick() as usize) % fighters.len().max(1))
        .take(fighters.len().min(128))
    {
        if issued >= 5 { break; }
        if !unit.controllable() || unit.in_service() || obs.is_deployed(unit.entity_id)
            || unit.slot == super::census::Slot::Aircraft
            || super::orders::ordered_this_frame(out,unit.entity_id)
        { continue; }
        let Some((id,gun,reach)) = gunline.iter().copied()
            .filter(|(id,_,_)| obs.can_hurt(rules,*id,unit.entity_id))
            .min_by_key(|(id,at,_)| (cell_distance(*at,unit.cell),*id))
        else { continue; };
        // A prism/V3-style battery already has a winning range trade. Its
        // assignment is Attack on the tower, never Move to a new staging cell.
        let own_reach = super::tactics::actual_threat_reach(
            rules, obs, unit.entity_id, id,
        );
        if own_reach > reach + 1 { continue; }
        let Some(post) = super::tactics::siege_post(
            rules,obs,unit.entity_id,unit.cell,gun,reach)
        else { continue; };
        let still_charging = obs.attack_target(unit.entity_id) == Some(id)
            || obs.destination(unit.entity_id).is_some_and(|destination|
                cell_distance(destination, gun) <= reach + 1
                    || super::orders::path_crosses(
                        &[gun], unit.cell, destination, reach + 1));
        // A *native* Attack/AttackMove from an earlier planning cycle can
        // survive while the strategic gate now says "wait for artillery".
        // Staging without cancelling that mission left tanks walking to their
        // deaths, even after the visible new plan looked defensive.
        if cell_distance(unit.cell, post) <= 2
            && cell_distance(unit.cell, gun) > reach + 1
        {
            if still_charging && super::orders::issue_tracked(
                tracker, trace, out, rules, obs, unit.entity_id,
                super::tracked::TrackedKind::Avoid, None, Some(post), Goal::Stop,
            ) { issued += 1; }
            continue;
        }
        if obs.destination(unit.entity_id) == Some(post) { continue; }
        if super::orders::issue_tracked(
            tracker,trace,out,rules,obs,unit.entity_id,
            super::tracked::TrackedKind::Avoid,None,Some(post),Goal::Move { cell: post },
        ) { issued += 1; }
    }
    issued
}

/// An outgunned front should not be an inert front. A small number of
/// visible enemy units/towers may be traded with safely by standoff pieces
/// even while the rest of the army waits for reinforcements. This is not an
/// all-army push: only units whose *actual* weapon outranges the target and
/// can damage it are allowed to step forward. It also prioritises psychic
/// controllers, which are much more dangerous than their nominal DPS.
fn limited_front_skirmish(
    tracker: &mut super::tracked::Tracker, trace: Option<u64>,
    fighters: &[&Unit], rules: &RuleSet, obs: &Observation<'_>,
    out: &mut Outbox, front: (u16,u16),
) -> bool {
    let visible: Vec<_> = obs.visible_enemy_entities()
        .filter(|enemy| cell_distance(enemy.cell, front) <= 18)
        .collect();
    let mut active = false;
    let mut issued = 0;
    for unit in fighters.iter().take(128) {
        if !unit.controllable() || unit.in_service() || unit.traits.aircraft
            || unit.slot == super::census::Slot::Ship
            || super::orders::ordered_this_frame(out,unit.entity_id)
            || super::orders::committed_garrison_entry(tracker,rules,obs,unit.entity_id)
        { continue; }
        let picked = visible.iter().filter_map(|enemy| {
            if !obs.can_hurt(rules,unit.entity_id,enemy.entity_id)
                || !super::orders::admits(rules,obs,unit.entity_id,
                    &Goal::Attack { target: enemy.entity_id })
            { return None; }
            let ours = super::tactics::actual_threat_reach(
                rules,obs,unit.entity_id,enemy.entity_id);
            let theirs = super::tactics::actual_threat_reach(
                rules,obs,enemy.entity_id,unit.entity_id);
            if ours < 5 || ours <= theirs + 1
                || cell_distance(unit.cell,enemy.cell) > 26
                || super::tactics::unfavourable_vehicle_duel(
                    rules,obs,unit.entity_id,enemy.entity_id)
            { return None; }
            // Winning the range comparison with *one* tower does not mean a
            // lone artillery unit may walk into the other towers around it.
            // It can fire from its safe present position, or wait for its wave.
            if obs.entity_type(rules, enemy.entity_id)
                .is_some_and(|ty| ty.is_base_defense)
                && cell_distance(unit.cell, enemy.cell) > ours
                && !battery_wave_ready(fighters.iter().filter(|friend|
                    friend.controllable() && friend.traits.combat
                        && cell_distance(friend.cell, unit.cell) <= 9
                        && obs.can_hurt(rules, friend.entity_id, enemy.entity_id)
                    ).count())
            { return None; }
            let control = obs.entity_type(rules,enemy.entity_id)
                .is_some_and(|ty| super::tactics::mind_control_reach(rules,ty).is_some());
            Some((!control, cell_distance(unit.cell,enemy.cell), enemy.entity_id,
                enemy.cell))
        }).min_by_key(|(not_control,distance,id,_)| (*not_control,*distance,*id));
        let Some((_, _, target, cell)) = picked else { continue; };
        if obs.attack_target(unit.entity_id) == Some(target) {
            active = true;
            continue;
        }
        if issued >= 3 { continue; }
        if super::orders::issue_tracked(tracker,trace,out,rules,obs,
            unit.entity_id,super::tracked::TrackedKind::Attack,
            Some(target),Some(cell),Goal::Attack { target }) {
            active = true;
            issued += 1;
        }
    }
    active
}

/// A range trade qualifies as safe battery fire only when the *same*
/// effective ground weapon can actually hurt the tower.
fn standoff_can_breach(own: i32, enemy: i32, can_damage: bool) -> bool {
    can_damage && own >= 6 && own > enemy + 1
}

/// Direct available standoff artillery at the *visible, powered* battery
/// covering the assault. An Attack on the object lets the native weapon-range
/// approach stop in firing position; AttackMove to the base's centre makes a
/// prism tank oscillate while the forward screen is erased one by one.
fn assign_breach_battery(
    tracker: &mut super::tracked::Tracker, trace: Option<u64>,
    fighters: &[&Unit], rules: &RuleSet, obs: &Observation<'_>,
    out: &mut Outbox, target: (u16,u16),
) -> bool {
    let batteries: Vec<_> = obs.visible_enemy_entities().filter_map(|enemy| {
        let ty = obs.entity_type(rules, enemy.entity_id)?;
        (ty.is_base_defense
            && obs.visible_enemy_building_operational(rules, enemy.entity_id)
            && cell_distance(enemy.cell, target) <= 14)
            .then_some(enemy)
    }).collect();
    if batteries.is_empty() { return false; }
    let mut available = false;
    let mut issued = 0;
    for unit in fighters {
        if !unit.controllable() || unit.in_service() || obs.is_deployed(unit.entity_id)
            || super::orders::ordered_this_frame(out, unit.entity_id)
        { continue; }
        let Some((gun, own_reach)) = batteries.iter().filter_map(|battery| {
            if !obs.can_hurt(rules, unit.entity_id, battery.entity_id) { return None; }
            let own = super::tactics::actual_threat_reach(
                rules, obs, unit.entity_id, battery.entity_id,
            );
            let enemy = super::tactics::actual_threat_reach(
                rules, obs, battery.entity_id, unit.entity_id,
            );
            standoff_can_breach(own, enemy, true).then_some((battery, own))
        }).min_by_key(|(battery, _)| (cell_distance(battery.cell, unit.cell), battery.entity_id))
        else { continue; };
        available = true;
        // Don't send an isolated long-range gun across overlapping towers'
        // envelopes simply to get within its own range of the first one.
        if cell_distance(unit.cell, gun.cell) > own_reach
            && !battery_wave_ready(fighters.iter().filter(|friend|
                friend.controllable() && friend.traits.combat
                    && cell_distance(friend.cell, unit.cell) <= 9
                    && obs.can_hurt(rules, friend.entity_id, gun.entity_id)
                ).count())
        { continue; }
        if obs.attack_target(unit.entity_id) == Some(gun.entity_id) { continue; }
        if issued >= 2 { continue; }
        if super::orders::issue_tracked(
            tracker, trace, out, rules, obs, unit.entity_id,
            super::tracked::TrackedKind::Attack,
            Some(gun.entity_id), Some(gun.cell),
            Goal::Attack { target: gun.entity_id },
        ) { issued += 1; }
    }
    available
}

/// The front is outnumbered *locally* although its rear has enough strength
/// to win the exchange. A short staging window moves the tail to a position
/// just behind the tip; it never selects a different enemy objective.
fn stage_reinforcements(
    tracker: &mut super::tracked::Tracker, trace: Option<u64>,
    fighters: &[&Unit], rules: &RuleSet, obs: &Observation<'_>,
    out: &mut Outbox, target: (u16,u16),
) -> usize {
    let Some(tip) = fighters.iter()
        .filter(|unit| cell_distance(unit.cell,target) <= 18)
        .min_by_key(|unit| (cell_distance(unit.cell,target),unit.entity_id))
    else { return 0; };
    let mut dx = (i32::from(tip.cell.0) - i32::from(target.0)).signum();
    let dy = (i32::from(tip.cell.1) - i32::from(target.1)).signum();
    if dx == 0 && dy == 0 { dx = 1; }
    let mut posts = Vec::new();
    let lateral = (-dy,dx);
    for range in [16i32,18,14,20] {
        for shift in [0,-2,2,-4,4] {
            let (Ok(x),Ok(y)) = (
                u16::try_from(i32::from(target.0) + dx * range + lateral.0 * shift),
                u16::try_from(i32::from(target.1) + dy * range + lateral.1 * shift),
            ) else { continue; };
            if obs.is_ground_walkable(x,y) { posts.push((x,y)); }
        }
    }
    let mut issued = 0;
    for unit in fighters.iter().cycle()
        .skip((obs.tick() as usize) % fighters.len().max(1))
        .take(fighters.len().min(128))
    {
        if issued >= 5 { break; }
        if !unit.controllable() || unit.in_service() || obs.is_deployed(unit.entity_id)
            || unit.slot == super::census::Slot::Aircraft
            || super::micro::firing(obs,unit.mission,unit.entity_id)
            || super::orders::ordered_this_frame(out,unit.entity_id)
        { continue; }
        let Some(post) = posts.iter().copied()
            .cycle().skip((unit.entity_id as usize) % posts.len().max(1))
            .take(posts.len())
            .find(|post| super::orders::reachable(rules,obs,unit.entity_id,*post)
                && !super::tactics::unsafe_post(rules,obs,unit.entity_id,*post))
        else { continue; };
        if cell_distance(unit.cell,post) <= 3
            || obs.destination(unit.entity_id) == Some(post)
        { continue; }
        if super::orders::issue_tracked(
            tracker,trace,out,rules,obs,unit.entity_id,
            super::tracked::TrackedKind::Avoid,None,Some(post),Goal::Move { cell: post },
        ) { issued += 1; }
    }
    issued
}

/// Permit a stalled, well-supported heavy tank column to breach a visible
/// gunline even when the perfect counter is not affordable. No infantry or
/// damaged lone vehicles are authorized to trade against multiple towers.
#[allow(dead_code)]
fn forced_armor_commit(armored: bool, health_permille: u16, own: i32, enemy: i32) -> bool {
    armored && health_permille >= 780 && own >= 2_000
        && own >= enemy.max(1).saturating_mul(2)
}

#[cfg(all(test, tests_sim_p_s))]
mod forced_breach_tests {
    use super::forced_armor_commit;

    #[test]
    fn only_a_healthy_locally_superior_armored_column_may_break_the_siege() {
        assert!(forced_armor_commit(true, 900, 5_000, 2_200));
        assert!(!forced_armor_commit(false, 900, 5_000, 1_000));
        assert!(!forced_armor_commit(true, 400, 5_000, 1_000));
        assert!(!forced_armor_commit(true, 900, 1_500, 200));
        assert!(!forced_armor_commit(true, 900, 3_000, 2_000));
    }
}

pub(super) fn advance(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) -> bool {
    if !brain.census.units.iter().any(|unit| unit.traits.combat) {
        return false;
    }
    // An army the base cannot replace is not an army, it is the whole house
    // walking out of the gate; and a base with nothing to hold it is a base
    // that falls while the army is away. The third gate is the one that
    // decides matches: an enemy already at the yard is what the army is for,
    // and marching out with it in the base loses the production the army
    // came from.
    // The army, as the one object that answers "who is in it and where it
    // stands": the same list every pass below reads, instead of each pass
    // filtering the ledger its own way.
    let army = super::army::Army::of(&brain.census, obs);
    // The fist, not the whole army: the teams already answer who holds the
    // base and who raids, and a push that takes everyone with it is a base
    // left empty the moment the front moves. A house with no team yet (the
    // first frames) still pushes with everyone, which is the opening it is.
    //
    // This is the line the squads draw: Defend stays where it is, Harass has
    // its own orders, and what is left is the advance's. Reading `army.members`
    // here meant every raider and every defender walked at the same structure
    // as the push, so the two teams existed only in the ledger.
    let fist: Option<&BTreeSet<u64>> = brain
        .squads
        .squad(super::squads::Role::Attack)
        .map(|squad| &squad.members)
        .filter(|members| !members.is_empty());
    let mut fighters: Vec<&Unit> = match fist {
        Some(members) => army
            .members
            .iter()
            .copied()
            .filter(|unit| members.contains(&unit.entity_id))
            .collect(),
        None => army.members.clone(),
    };
    // A hero does not march with the fist in the first place. `BuildLimit=1` is
    // a unit the house cannot buy another of, and a push that spends it for a
    // structure has traded the whole match for one building — the wishlist's
    // "保护英雄单位". They hold at home, which is where the defence step already
    // sends them, and the retreat line walks them out well before the ordinary
    // one when they are caught.
    fighters.retain(|unit| !unit.traits.irreplaceable && !unit.traits.yard_maker
        && !brain.specialist_shuttles.contains_key(&unit.entity_id)
        && !brain.specialist_shuttles.values().any(|trip| trip.passenger == unit.entity_id)
        && !brain.crate_claims.contains_key(&unit.entity_id)
        && !brain.crate_returns.contains_key(&unit.entity_id)
        && !brain.repair_ifv_pending.get(&unit.entity_id)
            .is_some_and(|until| *until > obs.tick())
        && !super::advanced::repair_ifv_loaded(rules, obs, &brain.census, unit.entity_id));
    if fighters.is_empty() { return false; }
    if execute_siege_charge(
        &brain.siege_charge, &brain.census, &mut brain.tracked, &mut brain.push_gate,
        rules, obs, out,
    ) { return true; }
    // No exclusion for a unit whose weapon spends it. It was here with a
    // matching errand of its own, and together they cost fourfold on two seeds
    // (see the note where the errand used to be called). The tag itself is
    // correct retail data; what measured badly is every policy built on it.
    // Which gate holds the army home, for the census line: seven gates that
    // all answer "not this frame" are indistinguishable in a log that only
    // counts what the house has.
    // Whether a raid is on the base right now: the ring [`base_under_threat`]
    // reads, counted rather than weighed.
    let raided = brain
        .census
        .base_cell
        .is_some_and(|home| brain.census.enemies_near(home, THREAT_RADIUS) > 0);
    // An opening on depleted random ore is not an ordinary race to the enemy:
    // a tank lost outside the yard may be the last tank the house can afford.
    // This is bounded to the opening, and only uses legitimately *seen* ore
    // plus the house's measured deliveries, not hidden map deposits.
    let scarce_opening = tick_limited_scarce_opening(
        obs.tick(), obs.credits(), brain.income.per_minute(),
        brain.ore_known.len(), fighters.len(), raided,
    );
    let gate = if scarce_opening {
        "scarce-opening"
    } else if fighters.len() < super::squads::ARMY_TO_ATTACK && !mop_up(&brain.census) {
        "too-small"
    } else if !super::build::can_replace_units(rules, obs, &brain.census) && raided {
        // A house that cannot replace its army does not spend it — *while it
        // still has a base to lose*. The two halves of that sentence are the
        // whole gate.
        //
        // Without the second half it is a death spiral: the enemy takes the war
        // factory, the gate turns true, the army that could have taken the
        // factory back is held in the yard, and the house waits to die with its
        // hands down. Measured on the 1v29 (team 3), `push=cannot-replace` held
        // for five of the last samples while 130 of ours died and the enemy lost
        // four objects.
        //
        // With a raid on the base there is still something to defend, so the
        // army stays; with the ring clear the army is the only thing the house
        // has left and the only way back is forward.
        "cannot-replace"
    } else if !super::build::base_defended(rules, obs, &brain.census) && raided {
        // A tower is held `want: 2` and the attack gate used to wait for one
        // unconditionally, so a base that had not finished its first tower
        // never attacked at all. It is the same "waiting to die" spiral, one
        // building earlier. With the ring clear the army goes; the tower is the
        // plan's business, not the push's.
        "base-defended"
    } else if base_under_threat(&brain.census)
        && !super::build::base_defended(rules, obs, &brain.census)
    {
        // A raid the walls can hold is the walls' job; a raid with no walls is
        // the army's, and that is the branch above. With seven houses attacking,
        // "an enemy inside the ring" is true for most of the match, and an army
        // that reads it as "never leave" guards its yard for forty minutes.
        "threat"
    } else {
        "push"
    };
    // The commit window: a gate that flipped away runs for its term whatever the
    // next frame's numbers say. Read before the gate is published so the census
    // line still reports the fresh answer.
    let tick = obs.tick();
    let resting = resting(brain, tick);
    let gate = if gate != "push" {
        brain.push_rest = (tick, gate);
        gate
    } else if resting {
        brain.push_rest.1
    } else {
        "push"
    };
    brain.push_gate = gate;
    if gate != "push" {
        brain.assault_wait = None;
        return false;
    }
    let tick = obs.tick();
    // Structures, not units: a base is what a house loses with, and a
    // Which target: the whole reading of the enemy -- structures, economy,
    // clean-up, reachability -- is one decision and lives in `targets`.
    let anchor = army.anchor;
    let (answer, progress) = targets::choose_focused(
        &brain.census,
        &brain.tracked,
        tick,
        brain.push_progress,
        anchor,
        &fighters,
        rules,
        obs,
        brain.campaign_house,
    );
    brain.push_progress = progress;
    let target = match answer {
        targets::Answer::NowhereToLook => {
            brain.push_gate = "nowhere-to-look";
            brain.assault_wait = None;
            return false;
        }
        targets::Answer::NoTarget => {
            brain.push_gate = "no-target";
            brain.assault_wait = None;
            return false;
        }
        targets::Answer::Chosen(chosen) => chosen,
    };
    brain.campaign_house = obs.visible_enemy_house(target.nearest.0);
    // The reachable target, not the blocker: the push's bookkeeping is about
    // where the army is going, and the blocker is only what it shoots first.
    brain.push_target = Some((target.nearest.0, tick, target.nearest.1));
    let mopping_up = target.mopping_up;
    let (target_id, target_cell) = (target.entity_id, target.cell);
    let siege_reach = if target_id != 0 {
        super::tactics::need_siege(rules,obs,target_cell,&fighters)
    } else { None };
    brain.siege_reach = siege_reach;
    // Real standoff batteries must be allowed to fire while the shorter-range
    // screen holds outside the enemy gun line. This is a bounded *breach*, not
    // another request to make more artillery when one already exists.
    if siege_reach.is_none() && target_id != 0
        && assign_breach_battery(
            &mut brain.tracked, brain.trace, &fighters, rules, obs, out, target_cell,
        )
    {
        brain.siege_wait.get_or_insert((target_id, tick));
        // An artillery unit has an actual winning range trade. Do not let
        // the arbitrary waiting deadline turn the screen into isolated tower
        // probes while that weapon is working toward the live battery.
        brain.push_gate = "battery-breach";
        stage_siege(&mut brain.tracked, brain.trace, &fighters, rules, obs, out, target_cell);
        return true;
    }
    if let Some(reach) = siege_reach {
        // Keep a bounded artillery preparation window for one enemy target.
        // A missing factory, an unaffordable price, or a refused production
        // order must never freeze the old successful v10 assault indefinitely.
        let start = match brain.siege_wait {
            Some((id, since)) if id == target_id => since,
            _ => tick,
        };
        brain.siege_wait = Some((target_id, start));
        // There is no clock-based lone-tank breach against an operational
        // battery. Either a real assembled wave commits (above), or we stage
        // for standoff artillery/spy blackout while the raiders hit income.
        brain.push_gate = "need-siege";
        let spy_power_attempt_live = brain.spy_attempts.iter().any(|(agent, (plant, _, _))| {
            brain.census.units.iter().any(|unit|
                unit.entity_id == *agent && unit.traits.spy)
                && brain.campaign_house.is_some()
                && obs.visible_enemy_house(*plant) == brain.campaign_house
                && obs.visible_enemy_entities().any(|enemy|
                    enemy.entity_id == *plant && obs.entity_type(rules, *plant)
                        .is_some_and(|ty| ty.power > 0))
        });
        // When a real spy is walking to this same house's visible power
        // plant, hold the heavy cannon assault briefly for the blackout.
        // Visibility and the native Capture intent, not a guessed effect,
        // justify the wait; after 1,500 frames it must not deadlock the army.
        let blackout_window = siege_blackout_wait(start, tick, spy_power_attempt_live);
        if blackout_window { brain.push_gate = "spy-blackout-staging"; }
        // Waiting must be an actual manoeuvre, not just a label. A tank
        // already walking into the pillbox will keep that native order
        // unless we deliberately move it back out of the gunline.
        stage_siege(&mut brain.tracked,brain.trace,&fighters,rules,obs,out,target_cell);
        log::debug!("auto play: stage for siege reach>{reach} at {target_cell:?}");
        return true;
    } else {
        brain.siege_wait = None;
    }
    // A crate on the way is a crate the army walks over.
    //
    // The box is a bonus and not a job, and the walk to the structure passes
    // it: the unit stands on the cell, the map takes it, and the next frame
    // the army is at the structure with the money. A crate the army walked
    // past because it was busy shooting is a crate the play test reports as
    // "it never picks the boxes up".
    let target_cell = crate_on_the_way(rules, obs, target_cell).unwrap_or(target_cell);
    // And a fist big enough to take it. The size gate above counts heads; this
    // one weighs them against what is standing there, and an army that cannot
    // win the fight stays home and grows instead of being spent.
    //
    // Except when there is nothing left to weigh. A house down to a handful of
    // objects is not a base to be beaten, it is what is left of one — and the
    // gate that stops the house spending an army it cannot replace has no fight
    // left to protect it from. Without this the match ends in a timeout with
    // the army standing in front of two walls: measured, the enemy was down to
    // 21 objects and the push never left, on every variant that changed what
    // gets ordered. This is the reason those variants looked worse than they
    // were.
    // A search is not a fight: there is nothing to weigh the army against, and
    // the march is the point.
    let searching = target.entity_id == 0;
    // A fist is what actually crosses the map, not the units holding home.
    // Never count the garrison or a harasser as support for a target they will
    // not arrive at. Use the *same* fighter list used to issue this push.
    let committed_points = fighters.iter().fold(0i32, |total, unit| {
        total.saturating_add(unit.points)
    });
    if !mopping_up
        && !searching
        && !super::squads::outguns(&brain.census, rules, obs, target.cell, committed_points)
    {
        // The same commit window as the gates above: `outguns` weighs the army
        // against whatever stands around this frame's target, so a target the
        // army is walking past can turn the answer for a second and back.
        brain.push_gate = "outgunned";
        brain.push_rest = (obs.tick(), "outgunned");
        // When the fist cannot win a frontal assault, standoff units can
        // still dismantle exposed guards and controllers at the doorway.
        // The weaker tanks remain behind the line instead of repeatedly
        // charging and being lost to the same guns.
        if limited_front_skirmish(&mut brain.tracked,brain.trace,&fighters,
            rules,obs,out,target_cell)
        {
            brain.push_gate = "standoff-skirmish";
            return true;
        }
        return false;
    }

    // A strong army can still lose when only its fastest two units meet the
    // enemy and the others are twenty cells behind. Look at currently visible
    // contact strength, not the all-map force total. Wait one bounded wave;
    // if the rear cannot arrive, do not freeze this previously winning push.
    if !mopping_up && !searching
        && super::tactics::waiting_for_support(rules,obs,target_cell,&fighters)
    {
        let start = match brain.assault_wait {
            Some((id,since)) if id == target_id => since,
            _ => tick,
        };
        brain.assault_wait = Some((target_id,start));
        if tick.saturating_sub(start) < 240 {
            brain.push_gate = "await-support";
            stage_reinforcements(
                &mut brain.tracked,brain.trace,&fighters,rules,obs,out,target_cell,
            );
            return true;
        }
    } else {
        brain.assault_wait = None;
    }

    // A tank built in a rich stalemate is not a tank committed to battle.
    // Wait for three real armoured vehicles within twelve cells of one
    // another. All tanks use one finite global timer, not a per-target timer
    // that can be reset every time the target selector changes its mind.
    let armour: Vec<&Unit> = fighters.iter().copied()
        .filter(|unit| armoured_fighter(unit, rules, obs)).collect();
    let broken_wave = armour.iter().any(|unit| {
        armour.iter().filter(|other| cell_distance(unit.cell, other.cell) <= 12)
            .count() < ARMOUR_WAVE_MIN
    });
    if !raided && !mopping_up && !searching && broken_wave {
        brain.armour_wave_wait.get_or_insert(tick);
    } else {
        brain.armour_wave_wait = None;
    }

    // The frame's budget is a handful of orders and the rest of the list is
    // dropped, so the scan's order decides who is ever told anything at all:
    // the same leading units were served every frame and the rest of the army
    // stood where it was built, which is what "the infantry just stands there"
    // looks like from the player's side. Rotating the start by the tick gives
    // every unit its turn, and an order holds for its term once given.
    //
    // Taken after the anchor, so the target this frame is aimed at does not
    // depend on who happens to be scanned first.
    let turn = fighters.len();
    if turn > 0 {
        fighters.rotate_left((tick as usize) % turn);
    }
    if super::trace_enabled("AUTOPLAY_TRACE_PUSH") {
        let distances: Vec<i32> = fighters
            .iter()
            .map(|unit| {
                let dx = i32::from(unit.cell.0) - i32::from(target_cell.0);
                let dy = i32::from(unit.cell.1) - i32::from(target_cell.1);
                dx * dx + dy * dy
            })
            .collect();
        let spread = distances.iter().max().copied().unwrap_or(0)
            - distances.iter().min().copied().unwrap_or(0);
        let moving = fighters
            .iter()
            .filter(|unit| {
                obs.destination(unit.entity_id).is_some()
                    || obs.attack_target(unit.entity_id).is_some()
            })
            .count();
        let near = distances
            .iter()
            .filter(|squared| **squared <= 12 * 12)
            .count();
        let cx = if fighters.is_empty() {
            0
        } else {
            let sum: i64 = fighters.iter().map(|unit| i64::from(unit.cell.0)).sum();
            (sum / i64::try_from(fighters.len()).unwrap_or(1)) as i32
        };
        let cy = if fighters.is_empty() {
            0
        } else {
            let sum: i64 = fighters.iter().map(|unit| i64::from(unit.cell.1)).sum();
            (sum / i64::try_from(fighters.len()).unwrap_or(1)) as i32
        };
        log::info!(
            "push trace: tick={} fighters={} moving={} near={} spread={} fist=({},{}) target={:?} at {:?} hp={:?} enemies={}",
            tick,
            fighters.len(),
            moving,
            near,
            spread,
            cx,
            cy,
            target_id,
            target_cell,
            obs.health(target_id),
            brain.census.enemies.len(),
        );
    }
    // The fighter nearest the target is the tip of the fist. A unit more than
    // STAGGER_BEHIND cells behind it never got a second order: it walked, it
    // arrived, its mission id stayed Move, and the skip below read that as
    // "already walking" for the rest of the match. Measured on Death.mmx seed
    // 24301 at tick 31876: 65 fighters, 7 with a destination, 19 inside twelve
    // cells of the target, spread 131 cells -- the tail was standing still.
    let tip = fighters
        .iter()
        .map(|unit| unit.cell)
        .min_by_key(|at| cell_distance(*at, target_cell))
        .unwrap_or(target_cell);
    let left_behind = |unit: &Unit| cell_distance(unit.cell, tip) > STAGGER_BEHIND;
    // A common target alone does not form a tank column: the fast tip can be
    // half a map ahead of its heavy support while the tail still has the old
    // waypoint. In safe territory, name a *temporary* march waypoint beside
    // the leading tanks for those that are far behind. Do not stop fighters
    // in contact or redirect the leading vehicles away from a won objective.
    let tank_rally = tank_cohesion_post(&fighters, target_cell, obs, &brain.census);
    if let Some(anchor) = tank_rally {
        // Use the scarce order budget on the lagging tanks first. Once their
        // new waypoint stands in Tracker they stop consuming this quota.
        fighters.sort_by_key(|unit| (
            !needs_tank_cohesion(unit, anchor),
            cell_distance(unit.cell, anchor),
            unit.entity_id,
        ));
    }

        if super::trace_enabled("AUTOPLAY_TRACE_SKIP") && tick % 600 == 0 {
            let mut deployed = 0;
            let mut attacking = 0;
            let mut walking = 0;
            let mut far = 0;
            let mut other = 0;
            for unit in &fighters {
                let attack = unit.mission == MissionId::from_known(MissionType::Attack)
                    && obs.attack_target(unit.entity_id).is_some();
                let move_ = unit.mission == MissionId::from_known(MissionType::Move);
                if obs.is_deployed(unit.entity_id) {
                    deployed += 1;
                } else if attack {
                    attacking += 1;
                } else if move_ && (obs.destination(unit.entity_id).is_some() || !left_behind(unit)) {
                    walking += 1;
                } else if move_ {
                    far += 1;
                } else {
                    other += 1;
                }
            }
            log::info!(
                "skip trace: tick={} fighters={} deployed={} attacking={} walking={} far={} other={} target={:?} at {:?}",
                tick,
                fighters.len(),
                deployed,
                attacking,
                walking,
                far,
                other,
                target_id,
                target_cell,
            );
        }
    let mut issued = 0;
    let mut refused = 0;
    for unit in &fighters {
        if issued >= ADVANCE_ORDERS_PER_FRAME {
            break;
        }
        // Watercraft are owned by micro's separate naval target selector.
        // A land campaign coordinate cannot be used as an ocean waypoint.
        if unit.slot == super::census::Slot::Ship { continue; }
        // The avoider owns a failed-route withdrawal for this finite interval.
        // The crate detail also owns its travel until the visible prize is taken;
        // neither is an attack squad's next march instruction.
        if brain.avoid_retreat_until.get(&unit.entity_id).is_some_and(|until| tick < *until)
            || brain.crate_claims.contains_key(&unit.entity_id)
            || brain.crate_returns.contains_key(&unit.entity_id)
        { continue; }
        if let Some(since) = brain.armour_wave_wait
            && armour_wave_missing(
                armour.iter().filter(|other| cell_distance(unit.cell, other.cell) <= 12)
                    .count(), tick.saturating_sub(since))
            && armoured_fighter(unit, rules, obs)
            && obs.attack_target(unit.entity_id).is_none()
            && !super::tactics::unsafe_post(rules, obs, unit.entity_id, unit.cell)
        {
            let safe_rally = brain.census.rally.or(brain.census.base_cell)
                .filter(|cell| super::orders::reachable(rules, obs, unit.entity_id, *cell)
                    && !super::tactics::unsafe_post(rules, obs, unit.entity_id, *cell));
            if let Some(cell) = safe_rally
                && cell_distance(unit.cell, cell) > 4
            {
                if super::orders::issue_tracked(&mut brain.tracked, brain.trace,
                    out, rules, obs, unit.entity_id,
                    super::tracked::TrackedKind::Move, None, Some(cell),
                    Goal::Move { cell })
                { issued += 1; }
            } else if (obs.destination(unit.entity_id).is_some()
                || obs.attack_target(unit.entity_id).is_some())
                && out.want(unit.entity_id, 0, Goal::Stop)
            {
                brain.tracked.forget_primary(unit.entity_id);
                issued += 1;
            }
            continue;
        }
        if unit.slot == super::census::Slot::Vehicle
            && let Some(enemy) = brain.census.enemies.iter().find(|enemy| {
                obs.is_cell_visible(enemy.cell.0, enemy.cell.1)
                    && cell_distance(unit.cell, enemy.cell) <= 9
                    && super::tactics::unfavourable_vehicle_duel(
                        rules, obs, unit.entity_id, enemy.entity_id)
                    && !super::tactics::armored_support(&brain.census, rules, obs,
                        unit.entity_id, enemy.entity_id, unit.cell)
            })
        {
            let home = brain.census.rally.or(brain.census.base_cell)
                .filter(|post| super::orders::reachable(rules, obs, unit.entity_id, *post)
                    && !super::tactics::unsafe_post(rules, obs, unit.entity_id, *post));
            if let Some(cell) = home
                && super::orders::issue_tracked(&mut brain.tracked, brain.trace,
                    out, rules, obs, unit.entity_id, super::tracked::TrackedKind::Avoid,
                    Some(enemy.entity_id), Some(cell), Goal::Move { cell })
            {
                brain.avoid_retreat_until.insert(unit.entity_id, tick.saturating_add(225));
                issued += 1;
                continue;
            }
        }
        // A unit already fighting or walking is doing what this would ask. A
        // mission with nothing behind it is not: a unit left on Attack after
        // its target died was skipped here, in `micro` and in the guard step,
        // and stood in the yard for the rest of the match.
        let cohesion = tank_rally.filter(|anchor| needs_tank_cohesion(unit, *anchor))
            .filter(|anchor| super::orders::reachable(rules, obs, unit.entity_id, *anchor));
        let mut march_cell = cohesion.unwrap_or(target_cell);
        if let Some(screen) = infantry_screen_post(&fighters, unit, target_cell, rules, obs)
            && super::orders::reachable(rules,obs,unit.entity_id,screen)
        {
            march_cell = screen;
        }
        // A high-value vehicle must not simply drive into a visible psychic
        // controller's weapon range. Attack it from outside range when it
        // has the reach; otherwise preserve the unit and let infantry or
        // artillery deal with the controller. This is not omniscient: threats
        // beyond current sight never enter the decision.
        if unit.traits.combat && !unit.traits.aircraft
            && let Some((controller, position, reach)) =
                super::tactics::controller_covering_path(
                    rules, obs, unit.cell, march_cell)
        {
            let own_reach = obs.entity_type(rules,unit.entity_id)
                .map_or(0,|ty| weapon_range_cells(rules,ty,obs.is_deployed(unit.entity_id)));
            // A Prism/V3-style weapon must actually fire on the controller;
            // waiting to shoot the remote main base wastes its sole advantage.
            if own_reach > reach + 1
                && obs.can_hurt(rules, unit.entity_id, controller)
                && super::orders::admits(rules,obs,unit.entity_id,
                    &Goal::Attack { target: controller })
            {
                if obs.attack_target(unit.entity_id) != Some(controller)
                    && super::orders::issue_tracked(
                        &mut brain.tracked,brain.trace,out,rules,obs,
                        unit.entity_id,super::tracked::TrackedKind::Attack,
                        Some(controller),Some(position),Goal::Attack { target: controller },
                    ) { issued += 1; }
                continue;
            }
            if super::tactics::controller_covering(rules,obs,unit.cell).is_some() {
                if let Some(safe) = super::tactics::evade_controller(
                    rules,obs,unit.entity_id,unit.cell,position)
                {
                    if super::orders::issue_tracked(
                        &mut brain.tracked,brain.trace,out,rules,obs,
                        unit.entity_id,super::tracked::TrackedKind::Dodge,
                        None,Some(safe),Goal::Move { cell: safe },
                    ) { issued += 1; }
                }
            } else if (obs.destination(unit.entity_id).is_some()
                || obs.attack_target(unit.entity_id).is_some())
                && super::orders::issue_tracked(
                    &mut brain.tracked,brain.trace,out,rules,obs,
                    // Stop the *existing* AttackMove; merely choosing not to
                    // reissue it would let native navigation charge on.
                    unit.entity_id,super::tracked::TrackedKind::Engage,
                    Some(controller),None,Goal::Guard,
                ) { issued += 1; }
            if super::trace_enabled("AUTOPLAY_TRACE_PUSH") && tick % 150 == 0 {
                log::info!("auto play: hold control line unit={} controller={} from={:?} destination={:?}",
                    unit.entity_id, controller, unit.cell, march_cell);
            }
            continue;
        }
        // Different units face different counters. A rocket soldier has no
        // business following a tank into anti-air fire, and an infantryman
        // cannot trade with a sniper or a grand cannon merely because the
        // army's *total* points beat the defending base. Unlike the 600-frame
        // artillery purchase window, this individual protection does not
        // expire into a guaranteed suicidal march.
        let isolated_battery = isolated_battery_approach(
            &fighters, rules, obs, unit.entity_id, unit.cell, march_cell);
        if let Some((gun, reach)) = isolated_battery.or_else(||
            super::tactics::lethal_approach(
                rules, obs, unit.entity_id, unit.cell, march_cell,
            )).filter(|(gun, reach)| {
            // Individual fragility rules must still protect infantry and
            // aircraft. A healthy armored fist, however, is allowed to trade
            // through a lone bunker when the nearby force is overwhelming.
            let ours = fighters.iter()
                .filter(|f| cell_distance(f.cell, *gun) <= 16)
                .fold(0i32, |sum, f| sum.saturating_add(f.points));
            let theirs = brain.census.enemy_points(rules, obs, *gun, 16);
            let can_hit_gun = obs.visible_enemy_entities().any(|enemy| {
                enemy.cell == *gun && obs.can_hurt(rules, unit.entity_id, enemy.entity_id)
            });
            // Huge fixed batteries cannot be overrun by simply having more
            // hit points: the vehicles may die before the first shot.
            let own_reach = obs.entity_type(rules, unit.entity_id)
                .map_or(0, |ty| weapon_range_cells(rules, ty,
                    obs.is_deployed(unit.entity_id)));
            isolated_battery.is_some()
                || super::tactics::extreme_battery_mismatch(own_reach, *reach)
                || !(can_hit_gun && (super::tactics::overwhelming_armor_commit(
                unit.slot == super::census::Slot::Vehicle,
                unit.health_permille, ours, theirs,
            )))
        }) {
            let safe = super::tactics::siege_post(
                rules, obs, unit.entity_id, unit.cell, gun, reach,
            ).or_else(|| {
                brain.census.rally.or(brain.census.base_cell).filter(|home| {
                    super::orders::reachable(rules,obs,unit.entity_id,*home)
                        && !super::tactics::unsafe_post(rules,obs,unit.entity_id,*home)
                })
            });
            if let Some(post) = safe
                && super::orders::reachable(rules,obs,unit.entity_id,post)
                && cell_distance(unit.cell,post) > 2
            {
                if super::orders::issue_tracked(
                    &mut brain.tracked,brain.trace,out,rules,obs,unit.entity_id,
                    super::tracked::TrackedKind::Avoid,None,Some(post),
                    Goal::Move { cell: post },
                ) { issued += 1; }
            } else if super::orders::issue_tracked(
                &mut brain.tracked,brain.trace,out,rules,obs,unit.entity_id,
                super::tracked::TrackedKind::Engage,None,None,Goal::Guard,
            ) { issued += 1; }
            continue;
        }
        // A Mirage-style ambusher is not the battering ram for an enemy
        // factory. When infantry are actually visible nearby, let it engage
        // them from the current line rather than marching past them toward
        // the campaign's building target. The cannon/psychic danger checks
        // above still take precedence over this local target specialization.
        if obs.entity_type(rules, unit.entity_id)
            .is_some_and(|ty| ty.disguise_when_still)
        {
            let personnel = obs.visible_enemy_entities()
                .filter(|enemy| cell_distance(enemy.cell, unit.cell) <= 12)
                .filter(|enemy| obs.entity_type(rules, enemy.entity_id)
                    .is_some_and(|ty| ty.category ==
                        crate::rules::object_type::ObjectCategory::Infantry))
                .filter(|enemy| obs.can_hurt(rules, unit.entity_id, enemy.entity_id)
                    && super::orders::admits(rules, obs, unit.entity_id,
                        &Goal::Attack { target: enemy.entity_id }))
                .min_by_key(|enemy| (cell_distance(enemy.cell, unit.cell), enemy.entity_id));
            if let Some(enemy) = personnel {
                if obs.attack_target(unit.entity_id) == Some(enemy.entity_id) {
                    continue;
                }
                if super::orders::issue_tracked(
                    &mut brain.tracked, brain.trace, out, rules, obs,
                    unit.entity_id, super::tracked::TrackedKind::Attack,
                    Some(enemy.entity_id), Some(enemy.cell),
                    Goal::Attack { target: enemy.entity_id },
                ) {
                    issued += 1;
                    continue;
                }
                // A denied retarget is not a commitment: let the original
                // march decision run rather than parking the ambusher.
            }
        }
        let attacking = unit.mission == MissionId::from_known(MissionType::Attack)
            && obs.attack_target(unit.entity_id).is_some();
        // A destination from an old push is not proof that this unit is
        // advancing toward the *current* target. This used to leave entire
        // columns standing outside an enemy base after a target switch.
        let walking = unit.mission == MissionId::from_known(MissionType::Move)
            && match obs.destination(unit.entity_id) {
                Some(destination) => cell_distance(destination, march_cell) <= 4,
                None => !left_behind(unit),
            };
        if attacking || walking {
            continue;
        }
        // A dug-in unit cannot walk, and the push is a walk: the micro layer
        // holds it on area guard where it was deployed, which is where the
        // ground it is holding is.
        if obs.is_deployed(unit.entity_id) {
            continue;
        }
        // The step that owns a specialist order already spent this unit's frame:
        // a spy walking into a building and a rifleman boarding a carrier both
        // outrank the push.
        if out.orders.iter().any(|order| order.actor == unit.entity_id)
            || brain.tracked.pending_resume(tick, unit.entity_id).is_some()
        {
            // A tactical detour can still be moving in native after its
            // short Tracker term ends. Mission Resume owns that hand-off.
            continue;
        }
        // Who owns the unit is the tracker's ladder: a fight in reach, a step
        // out from under a shell, a detour or a breach outranks the march, and
        // `admits` below refuses the order while one is live. A march sent
        // over them is the ping-pong a unit shows at a gate: the shot opens the
        // way, the march clears the shot, and neither finishes.
        // A unit whose weapons have nothing for the target does not march at
        // it. `admits` is `GetFireError` plus the armour floor, the same answer
        // the player's click gets, and the units it refuses are exactly the
        // ones that walk to the wall and die there: a dog sent at a building
        // its jaw cannot touch, a terror drone whose jump has no business with
        // concrete, a rifleman against armour nothing it carries dents.
        //
        // They hold where they stand instead, which is what a player does with
        // them — the fist is the units that can do the job. Marching them along
        // anyway is the "sent at the enemy and wiped out for nothing" a play
        // test reports, and their attack-moves are also what made a squad
        // shuffle around a structure it could never damage.
        // Searching: a march with the gun up, toward the ground nobody has
        // looked at yet.
        if searching {
            let kind = super::tracked::TrackedKind::AttackMove;
            if !brain
                .tracked
                .admits(tick, unit.entity_id, kind, None, Some(march_cell))
            {
                continue;
            }
            if !super::orders::issue_tracked(
                &mut brain.tracked, brain.trace, out, rules, obs, unit.entity_id,
                kind, None, Some(march_cell),
                Goal::AttackMove { cell: march_cell },
            ) {
                continue;
            }
            issued += 1;
            continue;
        }
        let attack = Goal::Attack { target: target_id };
        let engagement = obs.engage(rules, unit.entity_id, target_id);
        if engagement == crate::sim::control::observation::Engage::Cannot {
            if !brain.tracked.admits(
                tick,
                unit.entity_id,
                super::tracked::TrackedKind::Guard,
                None,
                None,
            ) {
                continue;
            }
            if super::orders::issue_tracked(
                &mut brain.tracked, brain.trace, out, rules, obs,
                unit.entity_id, super::tracked::TrackedKind::Guard,
                None, None, Goal::Guard,
            ) {
                issued += 1;
            }
            continue;
        }
        // Inside its own reach the unit shoots the structure from where it
        // stands; outside it, it walks. The goal is still the one target — this
        // is the same fist, ordered by what each unit can actually do.
        //
        // `Attack` alone was measured here — it hands the approach to the native
        // hunt, which closes to the unit's *own* range and stops, and it is the
        // shape the wishlist's "卡位" asks for. Across the four arena seeds the
        // exchange went 1.38/2.57/6.29/13.5 -> 1.00/3.00/4.71/13.0, a mean of
        // 5.94 down to 5.43, and the seed that matters most fell to a dead-even
        // trade. So the walk stays an `AttackMove` to the target's cell and the
        // stand-off is left on the table until something explains that drop.
        // A target the gate will refuse is not a target it can shoot: a neutral
        // block is nobody's and `orders::admits` drops every `Goal::Attack` at
        // it (measured on seed 4242: 298 refusals, issued 0-1 a frame, the fist
        // parked in front of it for 7000 ticks). AttackMove is the legal shape of
        // the same order -- it walks at the block and fires at what the walk
        // meets -- so the fist keeps moving instead of standing.
        let shootable = engagement == crate::sim::control::observation::Engage::Ready
            && obs.is_hostile(target_id);

        let (kind, goal) = if shootable {
            (super::tracked::TrackedKind::Attack, attack)
        } else {
            (
                super::tracked::TrackedKind::AttackMove,
                Goal::AttackMove { cell: march_cell },
            )
        };
        // L4: it has been sent at this target, the target has not walked away,
        // and the order has not expired — so it is already doing this.
        if !brain.tracked.admits(
            tick,
            unit.entity_id,
            kind,
            Some(target.entity_id),
            Some(march_cell),
        ) {
            continue;
        }
        // An order the click refuses is not an order: it must not be counted
        // against the frame's eight and must not overwrite a standing primary.
        // `issue_tracked` both arbitrates and remembers the accepted goal.
        if !super::orders::issue_tracked(
            &mut brain.tracked, brain.trace, out, rules, obs, unit.entity_id,
            kind, Some(target.entity_id), Some(march_cell), goal,
        ) {
            refused += 1;
            if super::trace_enabled("AUTOPLAY_TRACE_REFUSE") && tick % 600 == 0 {
                let dx = i32::from(unit.cell.0) - i32::from(target_cell.0);
                let dy = i32::from(unit.cell.1) - i32::from(target_cell.1);
                let reachable = super::orders::reachable(rules, obs, unit.entity_id, target_cell);
                log::info!(
                    "refuse probe: unit={} at {:?} target={} at {:?} d2={} hostile={} engage={} hurt={} reach={}",
                    unit.entity_id,
                    unit.cell,
                    target_id,
                    target_cell,
                    dx * dx + dy * dy,
                    obs.is_hostile(target_id),
                    obs.can_engage(rules, unit.entity_id, target_id),
                    obs.can_hurt(rules, unit.entity_id, target_id),
                    reachable,
                );
            }
            continue;
        }
        issued += 1;
    }
    if super::trace_enabled("AUTOPLAY_TRACE_REFUSE") && tick % 600 == 0 {
        log::info!(
            "budget probe: tick={} issued={} refused={}",
            tick,
            issued,
            refused
        );
    }
    true
}

/// Conserve an early, poorly replenished army while factories and defensive
/// infrastructure recover. The deadline prevents a dry map from causing an
/// indefinite no-attack stalemate.
fn tick_limited_scarce_opening(
    tick: u64, cash: i32, income_per_minute: Option<i32>,
    known_ore_cells: usize, attack_members: usize, raided: bool,
) -> bool {
    !raided && (1_200..5_400).contains(&tick)
        && known_ore_cells < 12
        && income_per_minute.is_some_and(|income| income <= 450)
        && cash < 3_200 && attack_members < 15
}

#[cfg(all(test, tests_sim_p_s))]
mod resource_opening_tests {
    use super::tick_limited_scarce_opening;

    #[test]
    fn scarce_income_conserves_opening_army_but_never_blocks_forever() {
        assert!(tick_limited_scarce_opening(2_000, 900, Some(0), 2, 10, false));
        assert!(!tick_limited_scarce_opening(6_000, 900, Some(0), 2, 10, false));
        assert!(!tick_limited_scarce_opening(2_000, 900, Some(0), 2, 10, true));
        assert!(!tick_limited_scarce_opening(2_000, 900, Some(5000), 2, 10, false));
        assert!(!tick_limited_scarce_opening(2_000, 900, Some(0), 30, 10, false));
    }
}

/// The reach of the weapon the object would actually fire, in cells, rounded
/// up; zero when it has none.
///
/// A dug-in soldier fires a different weapon from the one it stands up with.
/// `[E1] DeployFire=yes` and `DeployFireWeapon=` — slot 1, the secondary, by
/// default (`InfantryTypeClass::ReadINI` `0x0052466E..0x0052469A` reads the
/// key; `combat_weapon.rs` picks the slot from it) — and `[E1] Secondary=Para`
/// reaches 5 cells where `Primary=M60` reaches 4. A
/// brain that measures the standing weapon sends a deployed soldier at a target
/// its gun cannot cover, and a soldier that cannot walk just stands there.
pub(super) fn weapon_range_cells(rules: &RuleSet, ty: &ObjectType, deployed: bool) -> i32 {
    weapon_of(rules,ty,deployed).map_or(0, weapon_reach_cells)
}

/// The native effective reach of a *particular* weapon. Siege selection must
/// inspect the warhead and reach of the SAME slot, not accidentally combine a
/// long-ranged AA gun with a short-ranged ground-damaging secondary weapon.
pub(super) fn weapon_reach_cells(weapon: &crate::rules::weapon_type::WeaponType) -> i32 {
    let per_cell = crate::util::lepton::LEPTONS_PER_CELL_I32;
    let reach = (weapon.range_leptons - weapon.minimum_range_leptons.max(0)).max(0);
    reach.div_euclid(per_cell) + i32::from(reach.rem_euclid(per_cell) != 0)
}

/// How many shots the type's weapon fires in one attack cycle: `Burst=` from
/// `[WeaponTypes]`, read where the simulation reads it
/// (`WeaponTypeClass ReadINI`, default 1).
///
/// One is not a burst, and a weapon that fires one shot a cycle has nothing
/// for a broken cycle to gain.
pub(super) fn weapon_burst(rules: &RuleSet, ty: &ObjectType, deployed: bool) -> i32 {
    weapon_of(rules, ty, deployed).map_or(1, |weapon| weapon.burst.max(1))
}

/// The weapon `ty` fires, in the slot it would fire it from.
fn weapon_of<'r>(
    rules: &'r RuleSet,
    ty: &ObjectType,
    deployed: bool,
) -> Option<&'r crate::rules::weapon_type::WeaponType> {
    let slot = if deployed {
        super::census::Traits::of(rules, ty).deploy_slot
    } else {
        None
    };
    // The slot the type would fire, or else the first slot it has one in: a
    // type whose armed `Secondary=` and empty `Primary=` is army in the ledger,
    // and army with no reach would never be sent at anything.
    let name = slot
        .and_then(|slot| ty.weapon_at(slot))
        .or_else(|| (0..8).find_map(|slot| ty.weapon_at(slot)));
    rules.weapon(name?)
}

/// Chebyshev distance in cells, the metric the game's own adjacency uses.
pub(super) fn cell_distance(a: (u16, u16), b: (u16, u16)) -> i32 {
    let dx = i32::from(a.0).abs_diff(i32::from(b.0));
    let dy = i32::from(a.1).abs_diff(i32::from(b.1));
    i32::try_from(dx.max(dy)).unwrap_or(i32::MAX)
}

/// Whether an enemy is close enough to what this house still holds that the
/// army belongs at home.
///
/// Measured from the yard when one stands, and otherwise from whatever
/// building the house still has: a house whose yard is gone but whose factory
/// still stands has plenty left to lose, and an army that walks out while the
/// enemy is standing on the factory loses that too.
///
/// A house with nothing standing is the other way round. It used to count as
/// under threat and held the army home for the rest of the match, which is
/// the residual the module header records: once the yard is gone the army is
/// the only thing the house has, and an army guarding rubble loses slowly
/// instead of taking the base that would let it rebuild. Nothing standing
/// means nothing to defend, so the push runs.
/// The cell of the enemy standing closest to the house's base, when one is
/// inside the threat ring.
///
/// [`base_under_threat`] answers whether the army belongs home; this answers
/// *where*, which is what a guard on the far side of a growing base needs.
pub(super) fn threatened_cell(census: &Census) -> Option<(u16, u16)> {
    let base = census.base_cell?;
    census
        .enemies
        .iter()
        .filter(|enemy| {
            let dx = i32::from(enemy.cell.0) - i32::from(base.0);
            let dy = i32::from(enemy.cell.1) - i32::from(base.1);
            dx * dx + dy * dy <= THREAT_RADIUS * THREAT_RADIUS
        })
        .min_by_key(|enemy| {
            let dx = i32::from(enemy.cell.0) - i32::from(base.0);
            let dy = i32::from(enemy.cell.1) - i32::from(base.1);
            (dx * dx + dy * dy, enemy.cell)
        })
        .map(|enemy| enemy.cell)
}

fn base_under_threat(census: &Census) -> bool {
    let Some(base) = census.base_cell else {
        return false;
    };
    census.enemies.iter().any(|enemy| {
        let dx = i32::from(enemy.cell.0) - i32::from(base.0);
        let dy = i32::from(enemy.cell.1) - i32::from(base.1);
        dx * dx + dy * dy <= THREAT_RADIUS * THREAT_RADIUS
    })
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;
    use super::targets::{neutral_blocker, pick_reachable};
    use crate::rules::unit_role::RoleTable;
    use crate::sim::world::Simulation;

    /// A small army still goes at an enemy that has nothing left to fight with.
    ///
    /// The wishlist's stalemate: "最后消灭了大部分敌人，但Autoplay也不发起进攻，然后
    /// 就僵持在这了。发动攻击后总会剩下敌人的建筑不再打了". `ARMY_TO_ATTACK` is an
    /// opening number and had no bypass, so nine fighters against two buildings
    // read `too-small` for the rest of the match.
    #[test]
    fn armour_assembles_three_tanks_but_never_holds_a_dead_end_forever() {
        assert!(armour_wave_missing(1, 0));
        assert!(armour_wave_missing(2, 400));
        assert!(!armour_wave_missing(3, 20));
        assert!(!armour_wave_missing(1, ARMOUR_ASSEMBLE_FRAMES));
    }

    #[test]
    fn a_household_army_finishes_an_enemy_with_nothing_left() {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()
            .expect("the repository's own rules");
        let mut sim = crate::sim::world::Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        let mut census = Census::default();
        census.refresh(
            &crate::rules::unit_role::RoleTable::build(&rules),
            &rules,
            &Observation::new(&sim, owner),
        );
        // Two enemy objects left standing at the base: a clean-up job, not a
        // fight. The read is the ring, so the two are placed in it.
        let home = (20, 20);
        census.base_cell = Some(home);
        census.enemies = vec![
            crate::sim::control::observation::EnemyEntity {
                entity_id: 1,
                type_id: crate::sim::intern::InternedId::default(),
                cell: (home.0 + 1, home.1 + 1),
            },
            crate::sim::control::observation::EnemyEntity {
                entity_id: 2,
                type_id: crate::sim::intern::InternedId::default(),
                cell: (home.0 + 2, home.1 + 2),
            },
        ];
        assert!(mop_up(&census), "two objects left is a clean-up job");
        // And a field army standing on the base is not, whatever the fist's
        // size.
        census.enemies = (0..30)
            .map(|index| {
                let offset = i16::try_from(index).unwrap_or(0);
                crate::sim::control::observation::EnemyEntity {
                    entity_id: 100 + u64::try_from(index).unwrap_or(0),
                    type_id: crate::sim::intern::InternedId::default(),
                    cell: (
                        home.0.wrapping_add(offset.unsigned_abs()),
                        home.1.wrapping_add(offset.unsigned_abs()),
                    ),
                }
            })
            .collect();
        assert!(!mop_up(&census), "thirty objects is a fight");
        let _ = &rules;
    }

    /// A push that has taken nothing for a minute stops waiting for a
    /// favourable comparison. The gate is a group reading on purpose: no
    /// per-unit pass can see the army's progress, and a comparison that
    /// never opens is the army standing in front of a base until the clock
    /// runs out.
    #[test]
    fn a_push_that_takes_nothing_for_a_minute_goes_anyway() {
        assert!(
            !push_stalled((0, usize::MAX), 5_000, 30),
            "an opening army is small, not stalled"
        );
        assert!(
            !push_stalled((100, 30), 900, 30),
            "under a minute is still a wait"
        );
        assert!(
            push_stalled((100, 30), 1_000, 30),
            "a minute with nothing taken"
        );
        assert!(
            !push_stalled((100, 30), 1_000, 25),
            "the enemy lost objects since the reading"
        );
    }

    /// A dug-in soldier reaches further than the same soldier standing up:
    /// `[E1] Primary=M60 Range=4` against `Secondary=Para Range=5`, and
    /// `DeployFire=yes` with `DeployFireWeapon=` defaulting to slot 1 is what
    /// decides which one it fires. Measuring the standing weapon sent deployed
    /// soldiers at targets their own gun could not cover, and a soldier that
    /// cannot walk just stands there with the order marker on it.
    #[test]
    fn a_dug_in_soldier_reaches_as_far_as_the_weapon_it_fires() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        let obs = Observation::new(&sim, owner);
        let gi = obs
            .type_id("E1")
            .and_then(|type_id| obs.object_type(&rules, type_id))
            .expect("the G.I. is in the rules");
        let standing = weapon_range_cells(&rules, gi, false);
        let dug_in = weapon_range_cells(&rules, gi, true);
        assert!(standing > 0, "the standing soldier reaches: {standing}");
        assert!(
            dug_in > standing,
            "the deployed one reaches further: {standing} vs {dug_in}"
        );
    }

    /// The army's one target is a structure, and every combat unit used to be
    /// handed an Attack on it regardless of whether its weapon has an answer.
    /// `GetFireError` is the only thing that knows: an ore carrier has no
    /// weapon at all, and an order it is refused every frame is the frame's
    /// command budget spent on nothing — eight refusals is eight orders the
    /// house never gave.
    #[test]
    fn a_unit_whose_weapon_cannot_answer_the_target_is_not_told_to_attack_it() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        let tank = sim
            .spawn_object("MTNK", "Americans", 12, 10, 255, &rules)
            .expect("the grizzly tank");
        let carrier = sim
            .spawn_object("CMIN", "Americans", 10, 10, 255, &rules)
            .expect("the ore carrier");
        let building = sim
            .spawn_object("GAPOWR", "Russians", 11, 12, 255, &rules)
            .expect("the enemy power plant");
        let obs = Observation::new(&sim, owner);
        assert!(
            obs.can_engage(&rules, tank, building),
            "the tank has a weapon for a building"
        );
        assert!(
            !obs.can_engage(&rules, carrier, building),
            "the carrier has no weapon at all"
        );
    }

    /// A retail arena: two houses on 32x32 ground, needed to spawn real object
    /// types. The rules come from the repository's own `ini/` snapshot, so
    /// these run without a retail install.
    /// A gate that turned the army back holds for its window, so one frame of
    /// bad arithmetic does not re-task the whole army.
    ///
    /// Measured on Death.mmx seed 24301 before the window: `push` for ten
    /// samples, `outgunned` for eight, `push` again, `outgunned` again. Every
    /// flip re-tasks the fist, which from the player's side is an army walking
    /// out and back rather than fighting.
    #[test]
    fn a_gate_that_turned_the_army_back_holds_for_its_window() {
        let mut brain = AssistBrain::new(0xA17);
        brain.push_rest = (100, "outgunned");
        assert!(resting(&brain, 101), "a gate one frame old still holds");
        assert!(
            resting(&brain, 100 + PUSH_REST_FRAMES - 1),
            "and holds to the last frame of its window"
        );
        assert!(
            !resting(&brain, 100 + PUSH_REST_FRAMES),
            "and is done at the window's edge"
        );
        // A house that never rested pushes: the opening is not held back by a
        // window that was never opened.
        let fresh = AssistBrain::new(0xA17);
        assert!(!resting(&fresh, 0), "a house that never rested pushes");
    }

    fn arena(seed: u64) -> Option<(Simulation, RuleSet)> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        Some((sim, rules))
    }

    fn ledger(rules: &RuleSet, sim: &Simulation, owner: crate::sim::intern::InternedId) -> Census {
        let mut census = Census::default();
        census.refresh(
            &RoleTable::build(rules),
            rules,
            &Observation::new(sim, owner),
        );
        census
    }

    /// The whole reason the rule was narrowed: a house whose yard is gone and
    /// whose base is rubble used to read as under threat for the rest of the
    /// match, and an army guarding rubble loses slowly instead of taking the
    /// base that would let the house rebuild.
    #[test]
    fn a_house_with_nothing_left_to_defend_is_not_under_threat() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        assert!(
            !base_under_threat(&ledger(&rules, &sim, owner)),
            "nothing standing means nothing to defend"
        );
    }

    /// The same rule with something still standing, measured from the yard: an
    /// enemy inside the ring holds the army at home.
    #[test]
    fn an_enemy_at_the_yard_still_holds_the_army_home() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
            .expect("an Allied Construction Yard");
        assert!(
            !base_under_threat(&ledger(&rules, &sim, owner)),
            "an empty field around the yard is no threat"
        );

        sim.spawn_object("MTNK", "Russians", 20, 16, 255, &rules)
            .expect("a Soviet tank");
        assert!(
            base_under_threat(&ledger(&rules, &sim, owner)),
            "four cells from the yard is inside {THREAT_RADIUS}"
        );
    }

    /// A neutral refinery is worth taking whole. It must not hijack the whole
    /// attack group simply because it lies on the straight route.
    #[test]
    fn a_neutral_economic_structure_is_not_a_demolition_target() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        register_neutral_house(&mut sim, "Neutral");
        let blocker = sim
            .spawn_object("NAREFN", "Neutral", 20, 20, 255, &rules)
            .expect("a civilian refinery in the way");
        let mut census = ledger(&rules, &sim, owner);
        assert_eq!(
            census.neutrals.len(),
            1,
            "the ledger keeps what is nobody's"
        );
        census.refresh(
            &RoleTable::build(&rules),
            &rules,
            &Observation::new(&sim, owner),
        );
        assert_eq!(
            census.neutrals.len(),
            1,
            "a refresh does not accumulate them"
        );
        assert_eq!(
            neutral_blocker(
                &census,
                &Observation::new(&sim, owner),
                &rules,
                (18, 20),
                (30, 20)
            ),
            None,
            "a neutral refinery is an asset to capture, never a push target"
        );
        let _ = blocker;
        assert_eq!(
            neutral_blocker(
                &census,
                &Observation::new(&sim, owner),
                &rules,
                (10, 10),
                (10, 25)
            ),
            None,
            "a building off the line is not in the way"
        );
    }

    /// A neutral bunker is a fight, not a door: an enemy can stand inside it
    /// and shoot out, and the shooter is not the building, so nothing else in
    /// the ledger sees the threat.
    ///
    /// Measured on Arena.mmx seed 0x5EED at 24000 ticks: 13 of the local
    /// house's 37 losses were to `CABUNK01` (`CanBeOccupied=yes`,
    /// `MaxNumberOccupants=8`, `CanOccupyFire=yes`), including all five of its
    /// machine-gun towers. The army walked to the bunker to demolish it, spent
    /// the engagement on a structure worth nothing, and never arrived.
    #[test]
    fn an_occupiable_neutral_bunker_is_not_a_door_to_demolish() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        register_neutral_house(&mut sim, "Neutral");
        let bunker = sim
            .spawn_object("CABUNK01", "Neutral", 20, 20, 255, &rules)
            .expect("a neutral bunker in the way");
        let census = ledger(&rules, &sim, owner);
        assert_eq!(census.neutrals.len(), 1, "the ledger still records it");
        let ty = rules.object("CABUNK01").expect("the retail bunker");
        assert!(
            ty.can_be_occupied && ty.can_occupy_fire && ty.max_number_occupants > 0,
            "the fixture must be the kind of structure this test is about"
        );
        assert_eq!(
            neutral_blocker(
                &census,
                &Observation::new(&sim, owner),
                &rules,
                (10, 20),
                (30, 20)
            ),
            None,
            "the army walks around it instead of spending itself on it"
        );
        // A plain neutral wall is still a door, so this did not turn the whole
        // demolition path off.
        let _ = bunker;
        assert!(
            neutral_blocker(
                &census,
                &Observation::new(&sim, owner),
                &rules,
                (10, 20),
                (30, 20)
            )
            .is_none()
        );
    }

    /// An enemy building is not a neutral one: it is already in the enemy
    /// ledger and [`super::blocking_structure`] is what picks it.
    #[test]
    fn an_enemy_structure_is_not_a_neutral_blocker() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("NAREFN", "Russians", 20, 20, 255, &rules)
            .expect("an enemy refinery in the way");
        let census = ledger(&rules, &sim, owner);
        assert!(census.neutrals.is_empty(), "an enemy owns its building");
        assert_eq!(
            neutral_blocker(
                &census,
                &Observation::new(&sim, owner),
                &rules,
                (10, 20),
                (30, 20)
            ),
            None
        );
    }

    fn register_neutral_house(sim: &mut crate::sim::world::Simulation, name: &str) {
        let id = sim.interner.intern(name);
        let mut house = crate::sim::house_state::HouseState::new(id, 0, None, false, 0, 10);
        house.set_multiplay_passive_for_test(true);
        sim.houses.register(house);
    }

    /// A unit inside its own weapon's reach of the push's target attacks it
    /// from where it stands instead of walking into it: the same fist, ordered
    /// by what each unit can actually do.
    #[test]
    fn a_unit_in_reach_attacks_the_fist_target_from_where_it_stands() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 8, 8, 255, &rules)
            .expect("a yard");
        sim.spawn_object("GAPILE", "Americans", 14, 8, 255, &rules)
            .expect("a barracks");
        sim.spawn_object("GAWEAP", "Americans", 18, 8, 255, &rules)
            .expect("a war factory");
        sim.spawn_object("GAPILL", "Americans", 4, 8, 255, &rules)
            .expect("a pillbox so the base counts as defended");
        // The fist, the first rank standing beside the refinery: "in reach" is
        // the engine's answer (`GetFireError` with the range test on), which
        // measures to the building's own cells and not to the cell its click
        // names.
        let mut fighters = Vec::new();
        for cell in [(26u16, 26u16), (27, 26), (27, 27), (26, 27), (25, 26)] {
            fighters.push(
                sim.spawn_object("MTNK", "Americans", cell.0, cell.1, 255, &rules)
                    .expect("a Grizzly"),
            );
        }
        for cell in [(24u16, 26u16), (24, 27), (24, 28), (25, 27), (25, 28)] {
            fighters.push(
                sim.spawn_object("MTNK", "Americans", cell.0, cell.1, 255, &rules)
                    .expect("a Grizzly"),
            );
        }
        sim.spawn_object("NAREFN", "Russians", 28, 26, 255, &rules)
            .expect("an enemy refinery");

        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let mut out = Outbox::default();
        assert!(
            advance(&mut brain, &rules, &obs, &mut out),
            "ten fighters and a defended base is a push"
        );
        assert!(
            out.orders.iter().any(
                |order| order.actor == fighters[0] && matches!(order.goal, Goal::Attack { .. })
            ),
            "the nearest fighter is inside its reach and shoots: {:?}",
            out.orders
        );
    }

    /// A field army with no building left is the army's job whatever its size.
    ///
    /// The count and the stall were the only two ways the mop-up opened, so an
    /// enemy holding nine units and nothing else was above the count with no
    /// structure to besiege: the fist marched to the middle of the map and
    /// stood there. Measured on Death.mmx seed 10, which ended with ten enemy
    /// objects alive and the clock out.
    #[test]
    fn a_field_army_with_no_structure_left_is_targeted_whatever_its_size() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 5, 5, 255, &rules)
            .expect("a yard");
        sim.spawn_object("GAPILE", "Americans", 9, 5, 255, &rules)
            .expect("a barracks");
        sim.spawn_object("GAWEAP", "Americans", 5, 9, 255, &rules)
            .expect("a war factory");
        sim.spawn_object("GAPILL", "Americans", 3, 3, 255, &rules)
            .expect("a pillbox so the base counts as defended");
        let mut fighters = Vec::new();
        for index in 0..10u16 {
            fighters.push(
                sim.spawn_object(
                    "MTNK",
                    "Americans",
                    12 + index / 4,
                    12 + index % 4,
                    255,
                    &rules,
                )
                .expect("a Grizzly"),
            );
        }
        // Nine of them: one above MOP_UP_OBJECTS, and no structure at all.
        let mut enemies = Vec::new();
        for cell in [
            (25u16, 25u16),
            (26, 25),
            (27, 25),
            (25, 26),
            (26, 26),
            (27, 26),
            (25, 27),
            (26, 27),
            (27, 27),
        ] {
            enemies.push(
                sim.spawn_object("HTNK", "Russians", cell.0, cell.1, 255, &rules)
                    .expect("a Russian heavy tank"),
            );
        }

        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        assert_eq!(brain.census.enemies.len(), 9, "nine, not a handful");
        let obs = Observation::new(&sim, owner);
        let mut out = Outbox::default();
        assert!(
            advance(&mut brain, &rules, &obs, &mut out),
            "ten fighters push"
        );
        // The map's middle is (16,16), which is not in that block: an army
        // walking anywhere else is the old behaviour, not this one.
        assert!(
            out.orders.iter().any(|order| {
                matches!(order.goal, Goal::AttackMove { cell } if (25..=27).contains(&cell.0) && (25..=27).contains(&cell.1))
            }),
            "the fist walks at the tanks it can see: {:?}",
            out.orders
        );
        let _ = (fighters, enemies);
    }

    /// The fist aims at something it can walk to, not merely at something near.
    ///
    /// `orders::issue` refuses a walk the zone map cannot take, and it refuses
    /// it for every unit in turn: measured on Death.mmx seed 24301, 187
    /// refusals of one `AttackMove` and an army idle at the bell while an enemy
    /// object stood across water.
    #[test]
    fn a_target_nobody_can_walk_to_is_passed_over_for_one_somebody_can() {
        let target = |entity_id: u64, cell: (u16, u16)| EnemyEntity {
            entity_id,
            type_id: crate::sim::intern::InternedId::from_index(0),
            cell,
        };
        let nearest_first = [
            target(1, (10, 10)),
            target(2, (12, 10)),
            target(3, (20, 10)),
        ];
        // The near two are across the water; only the far one is walkable.
        let chosen =
            pick_reachable(&nearest_first, |cell| cell.0 == 20).expect("the far one is a choice");
        assert_eq!(chosen.entity_id, 3, "the walkable one, not the nearest");
        // And when the map cuts the fist off from all of them, the nearest is
        // still the answer: holding the army is the caller's job, not this
        // one's.
        let chosen = pick_reachable(&nearest_first, |_| false).expect("the nearest");
        assert_eq!(chosen.entity_id, 1);
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v16_artillery_tests {
    use super::*;

    #[test]
    fn a_real_standoff_gun_should_breach_while_short_guns_wait() {
        assert!(standoff_can_breach(12, 8, true));
        assert!(!standoff_can_breach(8, 8, true));
        assert!(!standoff_can_breach(12, 8, false));
        assert!(!standoff_can_breach(4, 2, true));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v29_mass_siege_tests {
    use super::mass_charge_ready;

    #[test]
    fn one_or_two_tanks_never_count_as_a_prism_breach() {
        assert!(!mass_charge_ready(1, 1, 12_000, 1_000, false));
        assert!(!mass_charge_ready(3, 1, 12_000, 1_000, false));
        assert!(!mass_charge_ready(7, 1, 12_000, 1_000, false));
    }

    #[test]
    fn a_single_concentrated_armour_wave_may_commit_if_no_standoff_exists() {
        assert!(mass_charge_ready(8, 1, 8_000, 3_500, false));
        assert!(!mass_charge_ready(8, 1, 8_000, 4_100, false));
        assert!(!mass_charge_ready(8, 1, 8_000, 3_500, true));
        // Overlapping towers need a larger *local* platoon, not a stronger
        // force stationed on the other side of the map.
        assert!(!mass_charge_ready(8, 2, 8_000, 3_500, false));
        assert!(mass_charge_ready(10, 2, 8_000, 3_500, false));
    }
}
