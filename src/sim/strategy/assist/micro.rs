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
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::{EnemyEntity, Observation};
use crate::sim::mission::{MissionId, MissionType};

use super::AssistBrain;
use super::squads::Role;
use super::squared_distance;
use super::tracked::TrackedKind;
use crate::sim::control::Goal;

/// How many engagements one frame may start. The board drops what is past its
/// own budget, and a frame that spends everything on the first squad it looked
/// at has none left for the squad under attack.
const MICRO_ORDERS_PER_FRAME: usize = 8;

/// How far past its own reach a unit will still take a fight, in cells. One:
/// the cell it is about to step into, so a closing hostile is met rather than
/// walked into.
const ENGAGE_MARGIN_CELLS: i32 = 1;

/// One unit's share of this cycle.
struct Work {
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
    // Snapshotted: the ledger and the teams are borrowed while they are read,
    // and the intent table is written below.
    let mut work = collect(brain, rules, obs, tick);
    // Eight orders a frame and the board drops the rest, so the scan's order
    // decides who is ever served: the same leading units were re-engaged every
    // frame and the rest of the army stood next to an enemy it never answered.
    // Rotating the cycle by the tick gives every unit its turn, and an engage
    // order holds for its term once given.
    let turn = work.len();
    if turn > 0 {
        work.rotate_left((tick as usize) % turn);
    }
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
    let aimed_at: std::collections::BTreeMap<u64, u64> = brain
        .census
        .enemies
        .iter()
        .filter_map(|enemy| {
            obs.attack_target(enemy.entity_id)
                .map(|t| (enemy.entity_id, t))
        })
        .collect();
    let mut issued = 0;
    for item in work {
        if issued >= MICRO_ORDERS_PER_FRAME {
            break;
        }
        // A step out of a shell's landing point is this frame's answer for the
        // unit; an engage order on top of it would walk it back into the blast.
        if super::orders::ordered_this_frame(out, item.entity_id) {
            continue;
        }
        // What is in reach now outranks where the unit was going: this is the
        // whole reason the layer exists. "In reach" is reach and weapon
        // together, so a dog walks on past the tank to the infantry behind it
        // instead of being held on a target it cannot hurt.
        if let Some(enemy) = in_reach(
            brain.census.enemies.iter().map(|enemy| {
                (
                    enemy.entity_id,
                    enemy.cell,
                    structures.contains(&enemy.entity_id),
                    aimed_at.get(&enemy.entity_id) == Some(&item.entity_id),
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
                super::orders::admits(rules, obs, item.entity_id, &Goal::Attack { target })
                    && obs.can_hurt(rules, item.entity_id, target)
            },
            |target| prefer(rules, obs, item.preference, target),
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
        // Nobody's building in the way. It is in no enemy list — nobody owns
        // it — so nothing in this layer would ever shoot it, and the unit
        // stands at its wall for the whole order, which is the "the infantry
        // just stands there" report. What is cleared is what the unit is up
        // against, and what stands inside the yard, where it blocks the
        // factories' own lanes; a village the army walks past is not a
        // demolition order.
        if let Some((blocker, _)) = neutral_in_the_way(brain, rules, obs, item.cell) {
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
            // The fist is the advance step's; this layer does not walk it.
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
                let cell = match enemy_economy(rules, obs, &brain.census, item.cell) {
                    Some((_, cell)) => cell,
                    None => {
                        let Some(cell) = nearest_unexplored(obs, item.cell, item.zone) else {
                            continue;
                        };
                        cell
                    }
                };
                // Attack-move, not attack: the walk is the point, and what
                // stands in the way on the way is dealt with by the engage
                // branch above.
                if super::orders::issue_tracked(
                    &mut brain.tracked,
                    brain.trace,
                    out,
                    rules,
                    obs,
                    item.entity_id,
                    TrackedKind::AttackMove,
                    None,
                    Some(cell),
                    Goal::AttackMove { cell },
                ) {
                    issued += 1;
                }
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
                // A scout walks the leg it is on. The nearest unexplored cell
                // changes sides as the dog moves — two cells at the same
                // distance, and the tie-break follows the dog — so re-deciding
                // every frame is a dog pacing between them instead of walking
                // anywhere. It is re-aimed when the leg is finished, which is
                // what the destination going away means.
                if obs.destination(item.entity_id).is_some() {
                    continue;
                }
                let Some(cell) = nearest_unexplored(obs, item.cell, item.zone).or_else(|| {
                    // The fallback is a hostile, and the reachability question
                    // the shroud grid answered above has to be asked here too:
                    // a dog sent at an enemy behind a neutral wall walks into
                    // the wall and the order is refused every frame.
                    nearest(&brain.census.enemies, item.cell)
                        .map(|(_, cell)| cell)
                        .filter(|cell| obs.can_reach(item.zone, item.cell, *cell))
                }) else {
                    continue;
                };
                if super::orders::issue_tracked(
                    &mut brain.tracked,
                    brain.trace,
                    out,
                    rules,
                    obs,
                    item.entity_id,
                    TrackedKind::Move,
                    None,
                    Some(cell),
                    Goal::Move { cell },
                ) {
                    issued += 1;
                }
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

/// Every team member that may be told anything, with what the cycle needs to
/// know about it.
fn collect(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    tick: u64,
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
            if !squad.members.contains(&unit.entity_id) || !unit.controllable() {
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
            let fighting = unit.mission == MissionId::from_known(MissionType::Attack)
                && obs.attack_target(unit.entity_id).is_some();
            if fighting || unit.in_service() {
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
            let reach = ty.map_or(0, |ty| {
                super::advance::weapon_range_cells(rules, ty, deployed)
            });
            work.push(Work {
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
fn in_reach(
    enemies: impl Iterator<Item = (u64, (u16, u16), bool, bool)>,
    from: (u16, u16),
    reach: i32,
    answers: impl Fn(u64) -> bool,
    rank: impl Fn(u64) -> u8,
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
) -> u8 {
    let category = obs.entity_type(rules, target).map(|ty| ty.category);
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

/// How near a neutral building has to stand to a unit for it to be what the
/// unit is up against, in cells: the cell it stands on and the one behind it,
/// which is where a foundation's near edge is.
const NEUTRAL_BLOCK_CELLS: i32 = 2;

/// How far from the base's centre a neutral building still counts as standing
/// in the yard, in cells: the outline the wall plan encloses
/// ([`super::walls::BASE_WALL_RADIUS`]).
const NEUTRAL_YARD_CELLS: i32 = 12;

/// How near a unit has to be to a neutral building inside the yard to be the
/// one that answers it, in cells. Everything in the base does not converge on
/// one civilian block: the closest units clear it, the rest keep their posts.
const NEUTRAL_YARD_REACH: i32 = 10;

/// The neutral building `unit` should clear out of its way: the one it is
/// standing against, or one standing in the yard that it is near enough to
/// answer.
///
/// One owner for the question, because the two cases are the same rule seen
/// from two sides: nobody's building where the house's own units have to walk.
fn neutral_in_the_way(
    brain: &AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    unit: (u16, u16),
) -> Option<(u64, (u16, u16))> {
    // Only what no click can use is cleared. A `CanBeOccupied=` house is a post
    // the house's own riflemen can hold, and a `Capturable=` derrick is the
    // income the engineer was sent for; both are worth more standing than the
    // walk around them costs, and shelling either is the capture undone. A
    // fence, a wreck or a decoration answers no click, and that is what stands
    // in the way. The one thing this does *not* decide is a strongpoint that
    // truly seals the road: that is the walk's question and
    // [`super::avoidance`] answers it at the cell the unit cannot enter.
    let clears = |building: &EnemyEntity| {
        obs.object_type(rules, building.type_id)
            .is_some_and(|ty| !ty.can_be_occupied && !ty.capturable)
    };
    if let Some(against) = brain
        .census
        .neutral_within(unit, NEUTRAL_BLOCK_CELLS, clears)
    {
        return Some(against);
    }
    let base = brain.census.base_cell?;
    brain
        .census
        .neutral_within(base, NEUTRAL_YARD_CELLS, clears)
        .filter(|(_, cell)| {
            squared_distance(*cell, unit) <= NEUTRAL_YARD_REACH * NEUTRAL_YARD_REACH
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
    let (width, height) = obs.map_extent();
    let zones = obs.has_zone_grid();
    let mut reach: Option<(i32, u16, u16)> = None;
    let mut walk: Option<(i32, u16, u16)> = None;
    let mut any: Option<(i32, u16, u16)> = None;
    let mut ry = 0;
    while ry < height {
        let mut rx = 0;
        while rx < width {
            if !obs.is_cell_revealed(rx, ry) {
                let distance = squared_distance((rx, ry), from);
                if any.is_none_or(|(closest, _, _)| distance < closest) {
                    any = Some((distance, rx, ry));
                }
                let open = obs.is_ground_walkable(rx, ry);
                if open && walk.is_none_or(|(closest, _, _)| distance < closest) {
                    walk = Some((distance, rx, ry));
                }
                if open
                    && zones
                    && obs.can_reach(zone, from, (rx, ry))
                    && reach.is_none_or(|(closest, _, _)| distance < closest)
                {
                    reach = Some((distance, rx, ry));
                }
            }
            rx = rx.saturating_add(RECON_STEP_CELLS);
        }
        ry = ry.saturating_add(RECON_STEP_CELLS);
    }
    if zones {
        return reach.map(|(_, rx, ry)| (rx, ry));
    }
    walk.or(any).map(|(_, rx, ry)| (rx, ry))
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
                    target
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
                |target| prefer(&rules, &obs, super::super::pilots::Preference::Air, target)
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
