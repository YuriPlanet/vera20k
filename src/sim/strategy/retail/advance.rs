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

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::{EnemyEntity, Observation};
use crate::sim::mission::{MissionId, MissionType};

use super::RetailBrain;
use super::census::{Census, Unit};
use super::squared_distance;

/// How large the army has to be before it is worth pushing with: the teams'
/// own number ([`super::squads::ARMY_TO_ATTACK`]), because "how big is big
/// enough" is one question with one owner.
///
/// Below it the opening force is the whole army, and an opening force that
/// walks into the enemy's pre-placed garrison is a house that has traded its
/// base for nothing — which is why the surplus goes harassing instead.

/// How many units one frame may re-task. The board's per-frame budget drops
/// everything past its own limit, so a larger number here would only spend the
/// frame on orders that never leave.
const ADVANCE_ORDERS_PER_FRAME: usize = 8;

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

/// How few enemy objects are left before the strength gate stops asking. Below
/// this the enemy is a clean-up job, not a fight: the army goes in and finishes
/// it instead of waiting for a comparison it can only lose by arithmetic.
const MOP_UP_OBJECTS: usize = 8;

/// Whether the fist is too small for what it can see, in a multi-house match.
///
/// Only 1vN asks this. In a duel the enemy's visible mass is the whole enemy, and
/// the gates above already answer it; with several houses, each of them is only
/// part of what will arrive, and the absolute `ARMY_TO_ATTACK` floor was written
/// for one opponent.
///
/// The comparison is the fist against the enemy's visible mobile mass — the same
/// `threat` vector the composition reads, summed — and the bar is two thirds: a
/// push that is not at least that goes out as a raid, not as the army. Below
/// [`MOP_UP_OBJECTS`] the caller is mopping up and this does not apply.
fn outnumbered(census: &Census, fighters: usize) -> bool {
    if census.enemy_houses <= 1 {
        return false;
    }
    let theirs: usize = census.threat.iter().map(|count| *count as usize).sum();
    theirs > 0 && fighters * 3 < theirs * 2
}

/// The 1vN bar is about what is visible, not about a fixed count.
#[test]
fn a_small_fist_does_not_push_into_several_houses() {
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

    // A duel: the gate has nothing to say, whatever the arithmetic.
    census.enemy_houses = 1;
    census.threat = [10, 10, 0, 0, 0, 0, 0];
    assert!(!outnumbered(&census, 12), "a duel is not 1vN");

    // Three houses and forty enemy bodies: twelve is a raid, not an army.
    census.enemy_houses = 3;
    assert!(
        outnumbered(&census, 12),
        "twelve against forty is walking into several houses' guns"
    );
    // The same houses and thirty of ours: the bar is two thirds, and that
    // clears it.
    assert!(!outnumbered(&census, 30), "thirty against forty is a push");
    // And with nothing visible the comparison is undefined, not a refusal.
    census.threat = [0; 7];
    assert!(
        !outnumbered(&census, 4),
        "no visible enemy is not outnumbering"
    );
}

/// Send the army at the enemy's base when it is large enough. `false` when
/// the house is not pushing this frame, so the caller keeps its own posture.
pub(super) fn advance(
    brain: &mut RetailBrain,
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
    fighters.retain(|unit| !unit.traits.irreplaceable);
    // Which gate holds the army home, for the census line: seven gates that
    // all answer "not this frame" are indistinguishable in a log that only
    // counts what the house has.
    let gate = if fighters.len() < super::squads::ARMY_TO_ATTACK {
        "too-small"
    } else if !super::build::can_replace_units(rules, obs, &brain.census) {
        "cannot-replace"
    } else if !super::build::base_defended(rules, obs, &brain.census) {
        "base-defended"
    } else if base_under_threat(&brain.census)
        && !super::build::base_defended(rules, obs, &brain.census)
    {
        // A raid the walls can hold is the walls' job; a raid with no walls is
        // the army's, and that is the branch above. With seven houses attacking,
        // "an enemy inside the ring" is true for most of the match, and an army
        // that reads it as "never leave" guards its yard for forty minutes.
        "threat"
    } else if outnumbered(&brain.census, fighters.len()) {
        // 1vN. The absolute floor is an opening number, and a fist that clears it
        // is still walking into several houses' worth of guns: measured in a
        // 1v7, the push traded its army for a fraction of theirs and then had
        // nothing to hold the base with. The bar rises with what is actually
        // visible, so a bigger enemy needs a bigger fist, and mopping up (few
        // objects left) is the branch that bypasses it.
        "outnumbered"
    } else {
        "push"
    };
    brain.push_gate = gate;
    if gate != "push" {
        return false;
    }
    let tick = obs.tick();
    // Structures, not units: a base is what a house loses with, and a
    // defending unit walks into the push on its own. Refineries first — a
    // house that loses its unloading bays loses the war it is paying for,
    // and the opponent's wallet is the one number that decides the late game.
    //
    // A building an engineer or a spy is already walking into is left out: it
    // is being taken whole, and shooting it destroys the prize the walk was
    // paid for. Capturing outranks destroying wherever both are possible, so
    // the fist goes for something else while the walk is under way.
    let structures: Vec<(&ObjectType, EnemyEntity)> = brain
        .census
        .enemies
        .iter()
        .filter_map(|enemy| {
            obs.object_type(rules, enemy.type_id)
                .filter(|ty| ty.category == ObjectCategory::Building)
                .map(|ty| (ty, *enemy))
        })
        .filter(|(_, enemy)| {
            !brain.tracked.claimed_by_other(
                tick,
                super::tracked::TrackedKind::Capture,
                enemy.entity_id,
                0,
            )
        })
        .collect();
    let economy: Vec<EnemyEntity> = structures
        .iter()
        .filter(|(ty, _)| ty.resource_destination)
        .map(|(_, enemy)| *enemy)
        .collect();
    let mut targets: Vec<EnemyEntity> = if economy.is_empty() {
        structures.iter().map(|(_, enemy)| *enemy).collect()
    } else {
        economy
    };
    // Clean-up targets whatever is left. A house down to a handful of objects
    // is not a base: the army that only ever aims at structures stands in
    // front of the last two units until the match times out — measured, a
    // timeout at 36000 with two enemies alive. Units join the list only here,
    // so an ordinary push still aims at what a base is.
    // A push that has not taken an enemy **structure** for a minute is not
    // waiting for a better moment: the comparison holding it home can be held
    // forever by a base the army is strong enough to trade into. Past the
    // stall the army goes, the way a player stops waiting for the perfect
    // engagement. This is a group-level reading on purpose — it is the army's
    // progress, not any one unit's orders, and no per-unit pass can see it.
    //
    // The count is structures, not objects: a field trade that never reaches
    // the base keeps the object count falling and would reset this forever,
    // which is the stalemate it is meant to catch.
    let standing = structures.len();
    if brain.push_progress.1 == usize::MAX {
        brain.push_progress = (tick, standing);
    }
    if standing < brain.push_progress.1 {
        brain.push_progress = (tick, standing);
    }
    let stalled = push_stalled(brain.push_progress, tick, standing);
    let mopping_up = brain.census.enemies.len() <= MOP_UP_OBJECTS || stalled;
    if mopping_up {
        targets.extend(
            brain
                .census
                .enemies
                .iter()
                .filter(|enemy| {
                    !structures
                        .iter()
                        .any(|(_, structure)| structure.entity_id == enemy.entity_id)
                })
                .copied(),
        );
    }
    if targets.is_empty() {
        // Nothing hostile is known: the army goes to look instead of
        // standing. That is what a player does after the ground in front of
        // them is clear, and what "everyone waits until the next target
        // appears" is missing. The cell is the nearest ground nobody has
        // seen yet, so the march is also the scouting pass.
        let Some(cell) = searching_cell(obs, army.anchor) else {
            brain.push_gate = "nowhere-to-look";
            return false;
        };
        targets.push(EnemyEntity {
            entity_id: 0,
            type_id: crate::sim::intern::InternedId::from_index(0),
            cell,
        });
    }

    // One target for the whole army, not the nearest for each unit: units
    // pointed at their own nearest structure arrive one at a time and are
    // killed one at a time. A fist is what takes a base.
    //
    // The target is re-read every frame, and that is deliberate: committing
    // the fist to one structure for a term was measured on the pinned 1v1 and
    // turned its win into a timeout — a structure the army cannot reach held
    // it for the whole term while the nearest one moved with the army.
    let anchor = army.anchor;
    let tick = obs.tick();
    let Some(nearest) = nearest_target(&targets, anchor) else {
        brain.push_gate = "no-target";
        return false;
    };
    brain.push_target = Some((nearest.entity_id, tick, nearest.cell));
    // The enemy's defenses before its economy while they cover the target: a
    // tesla coil or a pillbox in range of the structure the fist walks at is
    // what kills the fist before it fires a shot, and a spy's blackout only
    // buys the army the window in which those guns are dark. `Primary=` is the
    // rules' own answer to "does this structure shoot", so a mod's defense is
    // covered without a name list here.
    // A structure standing on the way is the first thing to shoot: a wall and a
    // building are the same answer to a walk — closed ground — and an army that
    // stops at one and does nothing never arrives. Clearing the way costs one
    // target and unblocks the whole fist; the target behind it is what the army
    // comes back for.
    let target = match blocking_structure(&structures, anchor, nearest) {
        Some(blocker) => covering_defense(&structures, blocker).unwrap_or(blocker),
        None => covering_defense(&structures, nearest).unwrap_or(nearest),
    };
    // A neutral building is nobody's, so it is in no census and no target list;
    // it is still closed ground the fist walks into and stops at. Sampling the
    // line for one turns "the whole army is jammed at a civilian block" into a
    // demolition order.
    let (target_id, target_cell) = neutral_blocker(&brain.census, obs, rules, anchor, target.cell)
        .unwrap_or((target.entity_id, target.cell));
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
    if !mopping_up && !searching && !super::squads::outguns(&brain.census, rules, obs, target.cell)
    {
        brain.push_gate = "outgunned";
        return false;
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
    let mut issued = 0;
    for unit in &fighters {
        if issued >= ADVANCE_ORDERS_PER_FRAME {
            break;
        }
        // A unit already fighting or walking is doing what this would ask. A
        // mission with nothing behind it is not: a unit left on Attack after
        // its target died was skipped here, in `micro` and in the guard step,
        // and stood in the yard for the rest of the match.
        let attacking = unit.mission == MissionId::from_known(MissionType::Attack)
            && obs.attack_target(unit.entity_id).is_some();
        if attacking || unit.mission == MissionId::from_known(MissionType::Move) {
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
        if out.orders.iter().any(|order| order.actor == unit.entity_id) {
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
                .admits(tick, unit.entity_id, kind, None, Some(target_cell))
            {
                continue;
            }
            brain
                .tracked
                .record(tick, unit.entity_id, kind, None, Some(target_cell));
            if !super::orders::issue(
                brain,
                out,
                rules,
                obs,
                unit.entity_id,
                Goal::AttackMove { cell: target_cell },
            ) {
                brain.tracked.void(unit.entity_id, kind);
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
            brain.tracked.record(
                tick,
                unit.entity_id,
                super::tracked::TrackedKind::Guard,
                None,
                None,
            );
            if super::orders::issue(brain, out, rules, obs, unit.entity_id, Goal::Guard) {
                issued += 1;
            }
            continue;
        }
        // Inside its own reach the unit shoots the structure from where it
        // stands; outside it, it walks. The goal is still the one target — this
        // is the same fist, ordered by what each unit can actually do.
        let (kind, goal) = if engagement == crate::sim::control::observation::Engage::Ready {
            (super::tracked::TrackedKind::Attack, attack)
        } else {
            (
                super::tracked::TrackedKind::AttackMove,
                Goal::AttackMove { cell: target_cell },
            )
        };
        // L4: it has been sent at this target, the target has not walked away,
        // and the order has not expired — so it is already doing this.
        if !brain.tracked.admits(
            tick,
            unit.entity_id,
            kind,
            Some(target.entity_id),
            Some(target.cell),
        ) {
            continue;
        }
        brain.tracked.record(
            tick,
            unit.entity_id,
            kind,
            Some(target.entity_id),
            Some(target.cell),
        );
        // An order the click refuses is not an order: it must not be counted
        // against the frame's eight, and it must not be remembered as if it had
        // been given. Eight refusals a frame is eight orders the house never
        // gave, and a record of one holds the unit for its whole term.
        if !super::orders::issue(brain, out, rules, obs, unit.entity_id, goal) {
            brain.tracked.void(unit.entity_id, kind);
            continue;
        }
        issued += 1;
    }
    true
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
    let Some(weapon) = weapon_of(rules, ty, deployed) else {
        return 0;
    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::unit_role::RoleTable;
    use crate::sim::world::Simulation;

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

    /// A nobody's building on the walk is a target: it is closed ground, it is
    /// in no enemy list, and the ledger's own neutral collection is what names
    /// it.
    #[test]
    fn a_neutral_structure_on_the_walk_is_named_as_the_blocker() {
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
                (10, 20),
                (30, 20)
            )
            .map(|(id, _)| id),
            Some(blocker),
            "the structure on the line is the one to shoot"
        );
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

        let mut brain = RetailBrain::new(0xA17);
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
}

/// How near an enemy's own defense has to stand to the structure the fist is
/// walking at before the defense is the thing to kill first, in cells: far
/// enough to cover the approach, near enough that it is that structure's gun
/// and not another base's.
const DEFENSE_CLEAR_CELLS: i32 = 8;

/// The nearest enemy structure with a weapon that covers `target`, if any.
///
/// A defense is what the fist has to walk through, and the economy building
/// behind it is only reachable once the gun is gone. `Primary=` (or any other
/// weapon slot) is the rules' own answer to "does this structure shoot", which
/// is the data's statement of intent: `[GAPILL]` and `[ATESLA]` carry one, a
/// refinery and a lab do not. Aircraft and units are not in the list — this is
/// the structure the fist is going to, and a defending tank walks into the
/// fight on its own.
fn covering_defense(
    structures: &[(&ObjectType, EnemyEntity)],
    target: EnemyEntity,
) -> Option<EnemyEntity> {
    let clear = DEFENSE_CLEAR_CELLS * DEFENSE_CLEAR_CELLS;
    structures
        .iter()
        .filter(|(ty, enemy)| {
            enemy.entity_id != target.entity_id && (0..8).any(|slot| ty.weapon_at(slot).is_some())
        })
        .filter(|(_, enemy)| {
            let dx = i32::from(enemy.cell.0) - i32::from(target.cell.0);
            let dy = i32::from(enemy.cell.1) - i32::from(target.cell.1);
            dx * dx + dy * dy <= clear
        })
        .min_by_key(|(_, enemy)| {
            let dx = i32::from(enemy.cell.0) - i32::from(target.cell.0);
            let dy = i32::from(enemy.cell.1) - i32::from(target.cell.1);
            (dx * dx + dy * dy, enemy.entity_id)
        })
        .map(|(_, enemy)| *enemy)
}

/// How near a structure has to stand to the straight line from the fist to its
/// target to block it, in cells: one wall's thickness plus the gap a unit needs
/// to squeeze past.
const BLOCKING_CELLS: i32 = 2;

/// The enemy structure standing on the way to `target`, nearest the fist first.
///
/// Tested against the straight line rather than the pathfinder: the question is
/// "what is between us and it", and the answer only has to be good enough to
/// pick the one thing to shoot. A structure *behind* the target does not count —
/// `path_crosses` clamps the projection to the segment's ends.
fn blocking_structure(
    structures: &[(&ObjectType, EnemyEntity)],
    from: (u16, u16),
    target: EnemyEntity,
) -> Option<EnemyEntity> {
    structures
        .iter()
        .filter(|(_, enemy)| enemy.entity_id != target.entity_id)
        .filter(|(_, enemy)| {
            super::orders::path_crosses(&[enemy.cell], from, target.cell, BLOCKING_CELLS)
        })
        .min_by_key(|(_, enemy)| (cell_distance(enemy.cell, from), enemy.entity_id))
        .map(|(_, enemy)| *enemy)
}

/// How far along the walk the neutral scan looks, in cells.
const NEUTRAL_SCAN_CELLS: i32 = 24;

/// The first building on the straight line from `from` to `to` that belongs to
/// nobody, nearest the army first — but only one the army may treat as a door.
///
/// A neutral structure is closed ground like any other, and the army that walks
/// into one and stops is the army that never arrives. It is not in
/// `census.enemies` — nobody owns it — so the ledger keeps it in
/// `census.neutrals` (`Observation::neutral_structures`) and the line is tested
/// against those, not against every cell of the map.
///
/// **A building an enemy can be inside is not a door.** `CanBeOccupied=yes` with
/// `CanOccupyFire=yes` and `MaxNumberOccupants=` above zero is a bunker: it
/// shoots back through whoever is standing in it, and the shooter is not the
/// building, so it never appears in `census.enemies` either. Demolishing one is
/// the worst trade on the map — the army walks into its guns, spends the
/// engagement on a structure worth nothing, and does not arrive.
///
/// Measured on Arena.mmx seed 0x5EED at 24000 ticks with the death ledger on:
/// **13 of the local house's 37 losses were to `CABUNK01`**, a neutral concrete
/// bunker (`CanBeOccupied=yes`, `MaxNumberOccupants=8`, `CanOccupyFire=yes`),
/// including all five of its machine-gun towers. That is 35% of the army spent
/// on map decoration, and the wish list reads it from the other side — its own
/// complaint is that auto play likes demolishing neutral buildings too much.
///
/// So an occupiable structure is left out of the demolition list and the walk
/// goes around it, which is what `avoidance` already does for anything it can
/// see shooting. A bunker nobody is inside shoots nothing, and the line passes
/// it as harmless ground.
fn neutral_blocker(
    census: &Census,
    obs: &Observation<'_>,
    rules: &RuleSet,
    from: (u16, u16),
    to: (u16, u16),
) -> Option<(u64, (u16, u16))> {
    let span = i32::from(from.0)
        .abs_diff(i32::from(to.0))
        .max(i32::from(from.1).abs_diff(i32::from(to.1)));
    if i32::try_from(span).unwrap_or(i32::MAX) < 2 {
        return None;
    }
    census
        .neutrals
        .iter()
        .filter(|building| {
            cell_distance(building.cell, from) <= NEUTRAL_SCAN_CELLS
                && super::orders::path_crosses(&[building.cell], from, to, BLOCKING_CELLS)
        })
        // A bunker is a fight, not a door. `can_be_occupied` alone is not
        // enough: a hospital an engineer may enter is still a wall to the
        // army. The pair that matters is the one that lets the enemy shoot
        // out of it.
        .filter(|building| {
            !obs.object_type(rules, building.type_id).is_some_and(|ty| {
                ty.can_be_occupied && ty.can_occupy_fire && ty.max_number_occupants > 0
            })
        })
        .min_by_key(|building| (cell_distance(building.cell, from), building.entity_id))
        .map(|building| (building.entity_id, building.cell))
}

/// The enemy structure nearest `from`, ties going to the earlier one in
/// storage order. Integer cells, so the choice is deterministic.
fn nearest_target(targets: &[EnemyEntity], from: (u16, u16)) -> Option<EnemyEntity> {
    let mut best: Option<(i32, EnemyEntity)> = None;
    for target in targets {
        let dx = i32::from(target.cell.0) - i32::from(from.0);
        let dy = i32::from(target.cell.1) - i32::from(from.1);
        let distance = dx * dx + dy * dy;
        if best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, *target));
        }
    }
    best.map(|(_, target)| target)
}
