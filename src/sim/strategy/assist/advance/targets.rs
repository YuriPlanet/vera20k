//! Which enemy the push goes at.
//!
//! [`super::advance`] answers "does the army go" — the gate, the commit window,
//! the rest. This module answers the other question, "at what", and only that:
//! read the census, keep the structures, put the economy first, decide whether
//! this is a clean-up job, and pick the nearest target anybody can walk to.
//!
//! It was 136 lines of the middle of `advance`, between the gate and the
//! per-unit orders, so neither the gate nor the orders could be read without
//! reading it. The seam is the same one [`super::super::split-orders`] drew:
//! infrastructure versus decision. Here it is house versus world — the gate
//! looks inward at what the house has, this looks outward at what the enemy
//! has, and the two met in one function body.
//!
//! The geometry that choice is made of came with it: the reach test, the
//! structure that blocks a walk, the neutral door, the defence that covers a
//! candidate, and the ordered pick among the candidates anybody can reach.
//! They were the tail of `advance` rather than a second module, which made
//! the file read as gate, orders, geometry, tests — three subjects in the
//! order they were written rather than the order they matter in. They answer
//! one question between them, so they live where the question is asked.

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::{EnemyEntity, Observation};

use super::super::census::{Census, Unit};
use super::super::tracked::Tracker;
use super::cell_distance;
use super::{MOP_UP_OBJECTS, push_stalled, searching_cell};

/// The target one push goes at, and what the census looked like when it was
/// chosen.
pub(super) struct Chosen {
    /// The target object, or `0` when the target is a cell the army walks to
    /// because nothing hostile is known. `super::advance` reads the `0` as
    /// "searching" and orders an `AttackMove` to the ground instead of an
    /// `Attack` on an object.
    pub(super) entity_id: u64,
    pub(super) cell: (u16, u16),
    /// The reachable target before the blockers on the way to it were weighed.
    /// `super::advance` keeps it as the push's target for its own bookkeeping:
    /// the blocker is what the army shoots first, not where it is going.
    pub(super) nearest: (u64, (u16, u16)),
    /// Whether the target list was extended with enemy units, because the field
    /// is no longer a base. A clean-up army is allowed to lose to a garrison it
    /// cannot out-shoot, so `super::advance` reads this before it weighs a fight.
    pub(super) mopping_up: bool,
}

/// What the push should aim at, or why it aims at nothing.
pub(super) enum Answer {
    Chosen(Chosen),
    /// Nothing hostile is known and the nearest unseen ground is off the map.
    NowhereToLook,
    /// Something was chosen and then found unreachable from anywhere.
    NoTarget,
}

/// Pick the push's target.
///
/// `progress` is the `(tick, structures standing)` pair the stall check keeps;
/// it is handed back updated, because it is a house-level memory and not the
/// world's to hold. `fighters` is the fist: it decides whether a candidate is
/// walkable to, not whether it is worth walking to.
pub(super) fn choose(
    census: &Census,
    tracked: &Tracker,
    tick: u64,
    mut progress: (u64, usize),
    anchor: (u16, u16),
    fighters: &[&Unit],
    rules: &RuleSet,
    obs: &Observation<'_>,
) -> (Answer, (u64, usize)) {
    // Structures, not units: a base is what a house loses with, and a
    // defending unit walks into the push on its own. Refineries first — a
    // house that loses its unloading bays loses the war it is paying for,
    // and the opponent's wallet is the one number that decides the late game.
    //
    // A building an engineer or a spy is already walking into is left out: it
    // is being taken whole, and shooting it destroys the prize the walk was
    // paid for. Capturing outranks destroying wherever both are possible, so
    // the fist goes for something else while the walk is under way.
    let structures: Vec<(&ObjectType, EnemyEntity)> = census
        .enemies
        .iter()
        .filter_map(|enemy| {
            obs.object_type(rules, enemy.type_id)
                .filter(|ty| ty.category == ObjectCategory::Building)
                .map(|ty| (ty, *enemy))
        })
        .filter(|(_, enemy)| {
            !tracked.claimed_by_other(
                tick,
                crate::sim::strategy::assist::tracked::TrackedKind::Capture,
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
    if progress.1 == usize::MAX {
        progress = (tick, standing);
    }
    if standing < progress.1 {
        progress = (tick, standing);
    }
    let stalled = push_stalled(progress, tick, standing);
    // A field with no structure standing is a field army, whatever its size:
    // the army's job there is to shoot what it can see, not to walk to the
    // middle of the map and wait. The count and the stall are the other two
    // ways this opens -- a handful left, or a minute without a kill -- and this
    // is the one that was missing: measured on Death.mmx seed 10, the enemy
    // ended with ten objects and no building, and the fist marched to the map
    // centre for the rest of the match because ten is above MOP_UP_OBJECTS.
    let mopping_up =
        structures.is_empty() || census.enemies.len() <= MOP_UP_OBJECTS || stalled;
    if mopping_up {
        targets.extend(
            census
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
        let Some(cell) = searching_cell(obs, anchor) else {
            return (Answer::NowhereToLook, progress);
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
    let Some(nearest) = reachable_target(&targets, anchor, fighters, rules, obs) else {
        return (Answer::NoTarget, progress);
    };
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
    // A visible enemy nuclear/weather/psychic strike plot is not an ordinary
    // dispensable structure: if it is on this *same* assault lane, disable the
    // future superweapon before picking yet another nearby refinery. Do not
    // redirect the fist across the map: that would regress the proven v10
    // reachable/nearest target behaviour.
    let campaign_target = structures.iter()
        .filter(|(ty, enemy)| super::super::build::grants_a_strike(rules, ty)
            && obs.is_cell_visible(enemy.cell.0, enemy.cell.1)
            && strike_on_approach(anchor, nearest.cell, enemy.cell))
        .min_by_key(|(_, enemy)| (cell_distance(enemy.cell, anchor), enemy.entity_id))
        .map_or(nearest, |(_, enemy)| *enemy);
    let target = match blocking_structure(&structures, anchor, campaign_target) {
        Some(blocker) => covering_defense(&structures, blocker).unwrap_or(blocker),
        None => covering_defense(&structures, campaign_target).unwrap_or(campaign_target),
    };
    // A neutral building is nobody's, so it is in no census and no target list;
    // it is still closed ground the fist walks into and stops at. Sampling the
    // line for one turns "the whole army is jammed at a civilian block" into a
    // demolition order.
    let (entity_id, cell) = neutral_blocker(census, obs, rules, anchor, target.cell)
        .unwrap_or((target.entity_id, target.cell));
    (
        Answer::Chosen(Chosen {
            entity_id,
            cell,
            nearest: (nearest.entity_id, nearest.cell),
            mopping_up,
        }),
        progress,
    )
}
/// A threat is urgent when it lies within the next local push, not merely
/// anywhere on the map. The narrow distance condition protects v10's wins.
fn strike_on_approach(anchor: (u16, u16), chosen: (u16, u16),
    strike: (u16, u16)) -> bool {
    cell_distance(strike, chosen) <= 12
        && cell_distance(strike, anchor) <= cell_distance(chosen, anchor) + 6
}

#[cfg(test)]
mod strategic_threat_tests {
    use super::*;

    #[test]
    fn a_remote_strike_does_not_hijack_a_winning_attack_route() {
        assert!(strike_on_approach((2, 2), (15, 2), (18, 2)));
        assert!(!strike_on_approach((2, 2), (15, 2), (40, 2)));
        assert!(!strike_on_approach((2, 2), (15, 2), (15, 30)));
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
pub(super) fn blocking_structure(
    structures: &[(&ObjectType, EnemyEntity)],
    from: (u16, u16),
    target: EnemyEntity,
) -> Option<EnemyEntity> {
    structures
        .iter()
        .filter(|(_, enemy)| enemy.entity_id != target.entity_id)
        .filter(|(_, enemy)| {
            super::super::orders::path_crosses(&[enemy.cell], from, target.cell, BLOCKING_CELLS)
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
pub(super) fn neutral_blocker(
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
    // Never replace the enemy objective with a civilian prize or a distant
    // wall. The caller only needs help with a *nearby* physical blockage;
    // local enemy fire is handled by the per-unit combat micro first.
    if census.enemies.iter().any(|enemy| {
        obs.is_cell_visible(enemy.cell.0, enemy.cell.1)
            && cell_distance(enemy.cell, from) <= 8
    }) {
        return None;
    }
    census
        .neutrals
        .iter()
        .filter(|building| {
            cell_distance(building.cell, from) <= 4
                && super::super::orders::path_crosses(&[building.cell], from, to, BLOCKING_CELLS)
        })
        // A bunker is a fight, not a door. `can_be_occupied` alone is not
        // enough: a hospital an engineer may enter is still a wall to the
        // army. The pair that matters is the one that lets the enemy shoot
        // out of it.
        .filter(|building| {
            obs.object_type(rules, building.type_id).is_some_and(|ty| {
                // Oil derricks, hospitals and capturable tech structures are
                // objectives for engineers, NOT doors for tanks to destroy.
                // Non-wall decorations are navigated around by the pathfinder.
                ty.wall && !ty.capturable && !ty.can_be_occupied
            })
        })
        .min_by_key(|building| (cell_distance(building.cell, from), building.entity_id))
        .map(|building| (building.entity_id, building.cell))
}

/// How many candidate targets are asked whether anyone can walk to them.
///
/// The ordinary frame pays one zone-map lookup -- `reachable` short-circuits on
/// the first unit that can walk -- and the bounded scan is what keeps a frame
/// whose every candidate is cut off from that. Twelve is past the number of
/// structures a fist ever picks between: the nearest is nearly always reachable.
const REACHABLE_CANDIDATES: usize = 12;

/// The nearest candidate `can_reach` accepts, or the nearest one when it
/// accepts none.
///
/// Split out from [`reachable_target`] so the rule can be tested without a map:
/// the choice is "the first candidate somebody can walk to", and a fixture that
/// paints a zone map to prove it would be testing the zone map.
pub(super) fn pick_reachable(
    nearest_first: &[EnemyEntity],
    can_reach: impl Fn((u16, u16)) -> bool,
) -> Option<EnemyEntity> {
    nearest_first
        .iter()
        .find(|target| can_reach(target.cell))
        .copied()
        .or_else(|| nearest_first.first().copied())
}

/// The enemy structure nearest `from` that someone in the fist can walk to.
///
/// `orders::issue` refuses a walk the zone map cannot take, and it refuses it
/// per unit, so a fist aimed at an object across water stands at home while the
/// refusal repeats: measured on Death.mmx seed 24301, 187 refusals in one match,
/// every one of them the same `AttackMove`, and the army idle at the bell.
/// Nearest first, then the next, and the first candidate anybody can reach wins.
///
/// When nobody can reach anything the nearest is returned unchanged: that is
/// the state this cannot fix, and the caller still has to hold the army.
fn reachable_target(
    targets: &[EnemyEntity],
    from: (u16, u16),
    fighters: &[&Unit],
    rules: &RuleSet,
    obs: &Observation<'_>,
) -> Option<EnemyEntity> {
    let mut nearest_first: Vec<EnemyEntity> = targets.to_vec();
    nearest_first.sort_by_key(|target| {
        let dx = i32::from(target.cell.0) - i32::from(from.0);
        let dy = i32::from(target.cell.1) - i32::from(from.1);
        (dx * dx + dy * dy, target.entity_id)
    });
    let candidates: Vec<EnemyEntity> = nearest_first
        .iter()
        .take(REACHABLE_CANDIDATES)
        .copied()
        .collect();
    pick_reachable(&candidates, |cell| {
        fighters
            .iter()
            .any(|unit| super::super::orders::reachable(rules, obs, unit.entity_id, cell))
    })
    .or_else(|| nearest_first.into_iter().next())
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

