//! Walking around a fight that cannot be won.
//!
//! [`super::dodge`] steps out of a shell already in the air and [`super::kite`]
//! spends a reload outside the enemy's reach; both take it for granted that the
//! unit is standing where it should be. This pass owns the question underneath
//! them: **is the ground ahead worth walking onto?**
//!
//! Two things make a unit walk into its own grave, and neither is a target
//! choice. A defence structure outranges and outweighs a lone soldier, so the
//! march to the enemy's base crosses its guns one unit at a time; and a group of
//! hostiles that outweighs what stands here turns the same march into a trade
//! the house loses. Both look identical from the outside — units walking
//! straight into fire — and both are decided *before* a shot is fired, which is
//! why no pass that starts from a shot in the air can fix them.
//!
//! So the test is a local one, in the same currency the rest of the brain
//! compares armies in ([`fight_points`]): the hit points of what this unit can
//! see near it against the hit points of what can see it. With the local fight
//! lost ([`outmatched`]) the unit does not walk in; it is routed around the
//! threat instead. That is the whole rule, and it is deliberately local: the
//! army's own push is the `outguns` gate's business ([`super::squads`]), and a
//! column that outnumbers the guns walks straight through them.
//!
//! The route is two legs of pure geometry, re-derived from where the unit
//! stands every time it is asked — never remembered:
//!
//! 1. **out**: while the unit is inside the threat's band, walk sideways to
//!    the band's edge. `CellSpread` and weapon reach are boxes in this game
//!    (Chebyshev, as [`super::kite`] measures them), so leaving the band is one
//!    straight walk that cannot be interrupted by the thing being avoided.
//! 2. **past**: once outside the band, walk forward past the threat at the
//!    clearance already gained.
//!
//! The other thing that stops a walk is not a gun at all, and it is the one a
//! player never thinks about because a player sees it: **a wall across the
//! road**. A `Wall=yes` overlay is ground a walker cannot enter, so a march
//! ordered across it is a march that never arrives — the objective on the far
//! side is unreachable and the unit spends the rest of the order standing in
//! front of it. Retail has an AI answer here only because the player has one:
//! the same road is opened the same two ways, **around** ([`way_around`]) when
//! the barrier is a segment, and **through** when it is a ring — the
//! force-fire click on the cell, the one the garage door in a base is opened
//! with.
//!
//! Because the leg is a function of the unit's own position, the pass has no
//! state machine to fall out of: the leg changes when the position does, and
//! when the objective stops being covered by the threat the pass says nothing
//! at all and the unit's own march is left alone.
//!
//! What it applies to is every walking unit, not just the ones with a gun: an
//! engineer on its way to a derrick and a dog sent to look are routed around a
//! pillbox exactly like a rifleman. Fighters stand their ground — a unit on
//! `Attack`, `Guard` or `AreaGuard` is not walking anywhere, and a dug-in
//! position is a position worth holding — and a unit whose capture, garrison or
//! deploy routine owns it is that routine's.
//!
//! Determinism: the sweep is bounded and rotates the ledger (the units at the
//! end of it would otherwise never be looked at), squares are integer
//! comparisons, the side of the detour is read from the unit's own position,
//! and ties break by entity id.

use std::collections::BTreeMap;

use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::Observation;
use crate::sim::mission::{MissionId, MissionType};

use super::AssistBrain;
use super::census::{Census, fight_points};
use super::tracked::TrackedKind;

/// How many detours one frame may hand out. The board drops what is past its
/// own budget, and the pass below a threat-ridden front would otherwise spend
/// the whole frame on the first few units it looked at.
const AVOID_ORDERS_PER_FRAME: usize = 8;

/// How much of the army one frame may examine, so a large army does not turn
/// this pass into a per-frame cost. The sweep rotates (modulo the ledger), so
/// the units at the end of it are looked at on the next frames rather than
/// never.
const AVOID_SCAN_PER_FRAME: usize = 96;

/// How far away an enemy's guns are considered at all, in cells: far enough
/// that a threat the walk is heading into is named before the unit is inside
/// it, since the detour's own corner sits one cell past the reach.
const LOOK_CELLS: i32 = 24;

/// The neighbourhood the local fight is weighed in, in cells: close enough that
/// the units counted are the ones that could actually join this fight.
const BALANCE_CELLS: i32 = 12;

/// How much of the enemy's hit points the house needs here to walk in.
///
/// Eight tenths: the attacker pays for the walk and arrives under the
/// defender's guns, so parity is already a losing trade. Below it the local
/// fight is one the unit goes around rather than into.
const FIGHT_PERMILLE: i32 = 800;

/// How far outside the threat's reach the detour keeps the walk, in cells. One
/// is the cheapest clearance that is definitely outside a box measured in whole
/// cells.
const CORNER_MARGIN: i32 = 1;

/// How far to either side a walk looks for a way past a wall, in cells.
///
/// A barrier wider than this is a ring around a base, not a segment across a
/// road: the walk goes through it instead of around it. Six is a two-cell
/// building's own arc, which is the widest thing a player walks around rather
/// than shoots; a wall past six either side encloses something.
const WALL_WAY_CELLS: i32 = 6;
/// A route that failed to find a safe detour for six simulated seconds gives
/// up on this objective rather than losing a scout to repeated side changes.
const DETOUR_GIVE_UP_FRAMES: u64 = 90;
const DETOUR_REPLAN_FRAMES: u64 = 450;

use super::threats::{BUCKET_CELLS, Threat, ThreatIndex, bucket_of, distance};

/// Whether a fight worth `ours` hit points against `theirs` is one to walk
/// into. Nothing to fight is always one to walk into.
pub(super) fn outmatched(ours: i32, theirs: i32) -> bool {
    if theirs <= 0 {
        return false;
    }
    ours.max(0).saturating_mul(1_000) < theirs.saturating_mul(FIGHT_PERMILLE)
}

/// The type of the thing that owns the gun standing on the road, when the
/// ledger still knows it.
///
/// The threat index carries the entity and its gun's reach, not the type; the
/// type is what says whether one shot of that gun takes this unit off the
/// field.
fn blocker_shooter<'a>(
    census: &Census,
    obs: &Observation<'_>,
    rules: &'a RuleSet,
    blocker: &Threat,
) -> Option<&'a ObjectType> {
    let type_id = census
        .enemies
        .iter()
        .find(|enemy| enemy.entity_id == blocker.entity_id)?
        .type_id;
    obs.object_type(rules, type_id)
}

/// Whether one shot from `shooter` takes `points` of `victim` off the field.
///
/// The arithmetic is the engine's own: the weapon's `Damage=` against the
/// victim's armour through the warhead's `Verses`, the same product
/// [`Observation::can_hurt`] reads to decide a shot is worth taking. What it
/// answers here is not "will this hurt" but "is there a second shot" — a tesla
/// coil against a dog, an engineer or a rifleman.
pub(super) fn kills_outright(
    rules: &RuleSet,
    shooter: &ObjectType,
    victim: &ObjectType,
    points: i32,
) -> bool {
    if points <= 0 {
        return false;
    }
    let armour = victim.armor_id();
    (0..shooter.weapon_record_count()).any(|slot| {
        let Some(name) = shooter.weapon_at(slot) else {
            return false;
        };
        let Some(weapon) = rules.weapon(name) else {
            return false;
        };
        if weapon.never_use {
            return false;
        }
        let Some(warhead) = weapon
            .warhead
            .as_deref()
            .and_then(|name| rules.warhead(name))
        else {
            return false;
        };
        let verses = warhead.interaction(armour).multiplier();
        let one_shot = f64::from(weapon.damage) * f64::from_bits(verses.bits());
        one_shot >= f64::from(points)
    })
}

/// Whether `objective` is somewhere *past* `blocker` rather than the thing
/// being attacked.
///
/// A target inside the guns is the army's decision, not this pass's: the push
/// gate ([`super::squads::outguns`]) already decided whether the house can take
/// it, and a unit told to attack a building does not get to circle it forever.
/// Two cells of slack cover a target standing beside the structure — the wall
/// next to the pillbox is still the pillbox's fight.
pub(super) fn passes_by(objective: (u16, u16), blocker: &Threat) -> bool {
    distance(objective, blocker.cell) > blocker.reach + 2
}

/// Whether this unit's frame belongs to the pass: only a walk has a route to
/// change, and a dug-in unit cannot walk at all.
///
/// Everything else here is a decision this pass does not own — a fighter that
/// has a target keeps it (`Attack`), a held post is held (`Guard`), a capture
/// or a garrison is its own step's, and a harvester is on an errand.
pub(super) fn walking(mission: MissionId, deployed: bool) -> bool {
    !deployed
        && (mission == MissionId::from_known(MissionType::Move)
            || mission == MissionId::from_known(MissionType::AttackMove))
}

/// One step from `from` towards `to`, eight ways.
pub(super) fn step_toward(from: (u16, u16), to: (u16, u16)) -> (u16, u16) {
    let dx = i32::from(to.0) - i32::from(from.0);
    let dy = i32::from(to.1) - i32::from(from.1);
    let step = |value: u16, delta: i32| -> u16 {
        let moved = i32::from(value) + delta.signum();
        u16::try_from(moved.max(0)).unwrap_or(value)
    };
    (step(from.0, dx), step(from.1, dy))
}

/// The cell one leg of the detour around `threat` ends on, on `side`.
///
/// `side` is the lateral direction (`-1` or `1`) the walk goes around on. The
/// two legs are [`detour_cell`]'s whole content: inside the threat's band the
/// lateral walk is the answer, and outside it the forward walk is. `None` when
/// the leg leaves the map — the caller tries the other side.
pub(super) fn detour_cell(
    unit: (u16, u16),
    objective: (u16, u16),
    threat: &Threat,
    extent: (u16, u16),
    side: i32,
) -> Option<(u16, u16)> {
    let dx = i32::from(objective.0) - i32::from(unit.0);
    let dy = i32::from(objective.1) - i32::from(unit.1);
    // Which axis the walk is mostly along decides which axis is "forward": the
    // detour steps off that line, and going around a box means going around its
    // short side.
    let across_x = dx.abs() >= dy.abs();
    let forward_sign = if (if across_x { dx } else { dy }) < 0 {
        -1
    } else {
        1
    };
    // The coordinates of the walk's two axes, forward first.
    let (unit_forward, unit_lateral, threat_forward, threat_lateral) = if across_x {
        (
            i32::from(unit.0),
            i32::from(unit.1),
            i32::from(threat.cell.0),
            i32::from(threat.cell.1),
        )
    } else {
        (
            i32::from(unit.1),
            i32::from(unit.0),
            i32::from(threat.cell.1),
            i32::from(threat.cell.0),
        )
    };
    let clearance = threat.reach + CORNER_MARGIN;
    let (forward, lateral) = if (unit_lateral - threat_lateral).abs() <= threat.reach {
        // Leg one: the unit is inside the band of cells the guns cover
        // sideways, whatever its forward progress. Leave it at the edge.
        (unit_forward, threat_lateral + side * clearance)
    } else {
        // Leg two: outside the band, so the walk past the threat keeps the
        // clearance it already has.
        (threat_forward + forward_sign * clearance, unit_lateral)
    };
    let (x, y) = if across_x {
        (forward, lateral)
    } else {
        (lateral, forward)
    };
    let (x, y) = (u16::try_from(x).ok()?, u16::try_from(y).ok()?);
    (x < extent.0 && y < extent.1).then_some((x, y))
}

/// Whether a walker of this movement zone gets through under its own power
/// rather than through what this pass opens.
///
/// These are the zones the passability matrix already gives rows of their own
/// (`crate::sim::pathfinding::passability`): `Water` and `WaterBeach` are the
/// water rows, and the three amphibious ones take the beach as well as the
/// land. A wall is not what closes the ground to such a unit, and a shot is
/// not what opens it again.
fn walks_its_own_way(zone: MovementZone) -> bool {
    matches!(
        zone,
        MovementZone::Water
            | MovementZone::WaterBeach
            | MovementZone::Amphibious
            | MovementZone::AmphibiousCrusher
            | MovementZone::AmphibiousDestroyer
    )
}

/// The cell the walk turns onto when the ground ahead is closed: the first
/// free cell to `side` of the blocked one, at the same point along the walk.
///
/// `free` is the navigation projection's own answer, the one the pathfinder
/// walks on, so the question is asked in the terms the walk will be answered
/// in. The lateral axis is the one the walk is *not* mostly along, because
/// going around a barrier means crossing its short side, the same choice
/// [`detour_cell`] makes.
///
/// `None` when the barrier runs past [`WALL_WAY_CELLS`] that way — the caller
/// tries the other side, and then shoots.
pub(super) fn way_around(
    blocked: (u16, u16),
    lateral_is_x: bool,
    side: i32,
    free: impl Fn(u16, u16) -> bool,
) -> Option<(u16, u16)> {
    for step in 1..=WALL_WAY_CELLS {
        let (x, y) = if lateral_is_x {
            (i32::from(blocked.0) + side * step, i32::from(blocked.1))
        } else {
            (i32::from(blocked.0), i32::from(blocked.1) + side * step)
        };
        let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
            return None;
        };
        if free(x, y) {
            return Some((x, y));
        }
    }
    None
}

/// The side the walk goes around on: the side of the threat the unit already
/// stands on, so two frames cannot pick opposite sides. A unit exactly on the
/// threat's line takes the fixed `-1`, which is the only tie-break that does
/// not depend on anything but the position.
pub(super) fn detour_side(unit: (u16, u16), objective: (u16, u16), threat: &Threat) -> i32 {
    let dx = i32::from(objective.0) - i32::from(unit.0);
    let dy = i32::from(objective.1) - i32::from(unit.1);
    let lateral = if dx.abs() >= dy.abs() {
        i32::from(unit.1) - i32::from(threat.cell.1)
    } else {
        i32::from(unit.0) - i32::from(threat.cell.0)
    };
    match lateral.cmp(&0) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Equal => -1,
    }
}

/// The nearest threat whose guns cover `cell`; ties by entity id, so two
/// frames of the same situation name the same blocker.
pub(super) fn blocker_at(threats: &[Threat], cell: (u16, u16)) -> Option<&Threat> {
    threats
        .iter()
        .filter(|threat| threat.covers(cell))
        .min_by_key(|threat| (distance(threat.cell, cell), threat.entity_id))
}

/// The house's own fighting weight, bucketed the same way.
#[derive(Default)]
struct FriendIndex {
    buckets: BTreeMap<(u16, u16), Vec<((u16, u16), i32)>>,
}

impl FriendIndex {
    fn insert(&mut self, cell: (u16, u16), points: i32) {
        if points > 0 {
            self.buckets
                .entry(bucket_of(cell))
                .or_default()
                .push((cell, points));
        }
    }

    /// What the house has within `radius` of `cell`, in hit points.
    fn sum_within(&self, cell: (u16, u16), radius: i32) -> i32 {
        let span = i32::from(BUCKET_CELLS).max(1);
        let reach = u16::try_from((radius + span - 1) / span).unwrap_or(1);
        let (bx, by) = bucket_of(cell);
        let mut total = 0i32;
        for y in by.saturating_sub(reach)..=by.saturating_add(reach) {
            for x in bx.saturating_sub(reach)..=bx.saturating_add(reach) {
                let Some(bucket) = self.buckets.get(&(x, y)) else {
                    continue;
                };
                for (at, points) in bucket {
                    if distance(cell, *at) <= radius {
                        total = total.saturating_add(*points);
                    }
                }
            }
        }
        total
    }
}

/// A geometric detour is not safe merely because its final cell is clear:
/// the direct leg may pass through a *second* visible turret's fire. Check
/// sample cells using only the same currently observed threat ledger.
fn safe_leg_from_other_guns(
    index: &ThreatIndex, from: (u16, u16), to: (u16, u16), ignore: u64,
) -> bool {
    let steps = distance(from, to);
    if steps > 48 { return false; }
    if steps == 0 { return true; }
    (1..=steps).all(|step| {
        let x = i32::from(from.0) +
            (i32::from(to.0) - i32::from(from.0)) * step / steps;
        let y = i32::from(from.1) +
            (i32::from(to.1) - i32::from(from.1)) * step / steps;
        let cell = (x as u16, y as u16);
        !index.covering(cell, LOOK_CELLS).iter()
            .any(|threat| threat.entity_id != ignore)
    })
}

/// After the lateral leg, the pathfinder must still turn towards the goal.
/// Reject a corner whose first few forward steps feed a neighbouring gun.
fn safe_exit_from_other_guns(
    index: &ThreatIndex, corner: (u16, u16), objective: (u16, u16), ignore: u64,
) -> bool {
    let mut current = corner;
    for _ in 0..4 {
        if current == objective { break; }
        current = step_toward(current, objective);
        if index.covering(current, LOOK_CELLS).iter()
            .any(|threat| threat.entity_id != ignore)
        { return false; }
    }
    true
}

/// Route every walking unit around the fights it would lose on the way.
pub(super) fn run(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    brain.avoid_retreat_until.retain(|_, until| *until > tick);
    // A capture mission is a native Capture walk, not necessarily a Move
    // mission. The old blocked-walker pass ignored these engineers entirely.
    // Search only the visible approach to their confirmed capture objective;
    // a nearby escort may open the wall while the engineer stays protected.
    request_capture_wall_breaches(brain, rules, obs);
    answer_breaches(brain, rules, obs, out, tick);
    if brain.census.enemies.is_empty() {
        return;
    }
    let mut index = ThreatIndex::default();
    for enemy in &brain.census.enemies {
        let Some(ty) = obs.object_type(rules, enemy.type_id) else {
            continue;
        };
        // A type that cannot shoot is not a reason to walk around anything,
        // which is also what keeps an enemy base's economy out of the count.
        let points = fight_points(ty, 1_000);
        let reach = super::advance::weapon_range_cells(rules, ty, false);
        if points <= 0 || reach <= 0 {
            continue;
        }
        index.insert(Threat {
            entity_id: enemy.entity_id,
            cell: enemy.cell,
            reach,
            points,
        });
    }
    if index.is_empty() {
        return;
    }
    let mut friends = FriendIndex::default();
    for unit in &brain.census.units {
        friends.insert(unit.cell, unit.points);
    }
    // Walls an unarmed unit ran into this frame. They are opened by this
    // house's own guns, not by the engineer that found them.
    let mut breaches: Vec<(u16, u16)> = Vec::new();
    // What each team is walking at, as a cell: the only reading this pass has
    // of which way is "forward" for a unit. A unit no team holds aims at what
    // it is already shooting, and failing both at the nearest hostile.
    let mut squad_targets: BTreeMap<u64, (u16, u16)> = BTreeMap::new();
    for squad in brain.squads.all() {
        let Some(target) = squad.target else {
            continue;
        };
        let Some(enemy) = brain
            .census
            .enemies
            .iter()
            .find(|enemy| enemy.entity_id == target)
        else {
            continue;
        };
        for member in &squad.members {
            squad_targets.insert(*member, enemy.cell);
        }
    }
    let extent = obs.map_extent();
    let house = obs.house();
    // What a walker can stand on, in the projection the pathfinder walks on.
    // Without a projection every cell answers closed, which is a map made of
    // walls rather than a map with walls in it, so the whole question is not
    // asked until the projection stands.
    let grid = obs.has_ground_grid();
    let walkable = move |x: u16, y: u16| !grid || obs.is_ground_walkable(x, y);
    let total = brain.census.units.len();
    let start = if total == 0 {
        0
    } else {
        brain.avoid_cursor % total
    };
    let scanned = total.min(AVOID_SCAN_PER_FRAME);
    let mut issued = 0;
    for offset in 0..scanned {
        if issued >= AVOID_ORDERS_PER_FRAME {
            break;
        }
        let unit = &brain.census.units[(start + offset) % total];
        // A player's unit, an object another object drives, and a unit a
        // routine owns are not this pass's. The spawned ones are the report
        // this pass earned: a `Move` around a "wall" the cargo plane could not
        // walk was an order that ended the paradrop's own route.
        if super::advance::charge_member(brain, tick, unit.entity_id)
            || !unit.controllable() || brain.tracked.held(tick, unit.entity_id)
            || brain.avoid_retreat_until.contains_key(&unit.entity_id) {
            continue;
        }
        // The civilian house's entrance is a high-priority primary task.
        // Do not invalidate a confirmed garrison order with a direct retreat
        // or detour, even though this pass can bypass issue_tracked.
        if super::orders::committed_garrison_entry(
            &brain.tracked, rules, obs, unit.entity_id)
        { continue; }
        if !walking(unit.mission, obs.is_deployed(unit.entity_id)) {
            continue;
        }
        // An earlier pass gave it this frame's order.
        if super::orders::ordered_this_frame(out, unit.entity_id) {
            continue;
        }
        let Some(ty) = obs.object_type(rules, unit.type_id) else {
            continue;
        };
        if ty.category == ObjectCategory::Aircraft {
            continue;
        }
        // A spy whose *enemy-facing* disguise is valid need not weave around
        // every ordinary tower: those detours caused an endless perimeter
        // dance. Capture's watcher recheck still aborts on dogs, parasites,
        // psychic weapons or a disguise which ceases to fool this enemy.
        if ty.agent
            && let Some(Goal::Capture { target }) = brain.tracked.primary_goal(unit.entity_id)
            && obs.disguised_to_visible_enemy(unit.entity_id, target)
        {
            continue;
        }
        let Some(objective) = objective(brain, obs, unit.entity_id, unit.cell, &squad_targets)
        else {
            continue;
        };
        if objective == unit.cell {
            continue;
        }
        // What the unit is about to walk into: one step along the line to its
        // objective. A threat behind it is not a reason to turn around.
        let heading = step_toward(unit.cell, objective);
        // The wall across the road, before any of the shooting: it is not a
        // gun, and a march ordered across it is a march that never arrives —
        // the objective behind it is unreachable, whatever the guns say.
        //
        // The movement zone is the seam this pass will be read through when
        // water moves: the projection's passability matrix already carries
        // `Water`, `WaterBeach` and the three amphibious rows, so a zone that
        // can move on water has its own way through and is not one whose road
        // needs opening. Today the projection has no water *layer*, so such a
        // cell answers closed exactly like a wall and this pass would shell
        // the sea; the zone is what tells the two apart.
        if walks_its_own_way(ty.movement_zone) {
            continue;
        }
        if !walkable(heading.0, heading.1) {
            // Around it first. A segment across a road is walked around the
            // way a player walks around it, and a shot spent on a wall that
            // could have been walked is a shot spent for nothing.
            let across = Threat {
                entity_id: 0,
                cell: heading,
                reach: 0,
                points: 0,
            };
            let side = detour_side(unit.cell, objective, &across);
            let lateral_is_x = (i32::from(objective.0) - i32::from(unit.cell.0)).abs()
                < (i32::from(objective.1) - i32::from(unit.cell.1)).abs();
            let mut around = None;
            for side in [side, -side] {
                if let Some(cell) = way_around(heading, lateral_is_x, side, &walkable) {
                    around = Some(cell);
                    break;
                }
            }
            if let Some(cell) = around {
                if super::orders::issue_tracked(
                    &mut brain.tracked,
                    brain.trace,
                    out,
                    rules,
                    obs,
                    unit.entity_id,
                    TrackedKind::Avoid,
                    None,
                    Some(cell),
                    Goal::Move { cell },
                ) {
                    log::debug!(
                        "auto play: avoid {} around the wall at {:?} by {:?}",
                        unit.entity_id,
                        heading,
                        cell
                    );
                    issued += 1;
                }
                continue;
            }
            // Six cells either side and still closed: it is a ring or a wall,
            // not a segment, and the way through is the way a player opens
            // one — a shot at whatever is standing there. Three things have
            // to be true first, and they are all about the thing and not
            // about the map: the unit has a gun worth it, there is open
            // ground on the far side (a cliff six cells deep is not opened
            // by shooting at it), and the thing is not this house's own.
            let past = step_toward(heading, objective);
            let armed =
                super::advance::weapon_range_cells(rules, ty, obs.is_deployed(unit.entity_id)) > 0;
            if !walkable(past.0, past.1) {
                continue;
            }
            if !armed {
                // No gun, so the wall is not this unit's to open — an engineer
                // walks at a gate and stands there. The cell is left in the
                // request list instead of being walked into for the rest of the
                // term; an armed unit of the house answers it below.
                if breaches.len() < BREACH_REQUESTS_PER_FRAME {
                    breaches.push(heading);
                }
                continue;
            }
            // What stands there, and what a shot at it is called. A wall is
            // an overlay and not an object, so it is opened by the forced
            // click on the cell; a structure is an object, and it is opened
            // by the ordinary attack click on it. The one that is there is
            // the one that is shot.
            //
            // A structure is skipped when an engineer is already walking
            // into it: a derrick sealing a canyon is a road, but a derrick
            // the capture step has claimed is a refinery the house has not
            // paid for, and shelling it is the capture undone.
            let blocker = if obs.wall_at(rules, heading.0, heading.1)
                && obs.wall_owner_at(heading.0, heading.1) != Some(house)
            {
                Some(Goal::AttackCell { cell: heading })
            } else {
                obs.structure_at(rules, heading)
                    .filter(|(id, mine)| {
                        !mine && !brain.tracked.claimed_by_other(
                            tick, TrackedKind::Capture, *id, unit.entity_id)
                            && !brain.census.enemies.iter().any(|enemy| {
                                enemy.entity_id == *id && super::orders::engineer_claimed(
                                    rules, obs, &brain.census, &brain.tracked, *enemy)
                            })
                    })
                    .map(|(id, _)| Goal::Attack { target: id })
            };
            let Some(goal) = blocker else {
                continue;
            };
            let claim = match goal {
                Goal::AttackCell { cell } => Some(cell),
                Goal::Attack { .. } => Some(heading),
                _ => None,
            };
            if !brain
                .tracked
                .admits(tick, unit.entity_id, TrackedKind::Breach, None, claim)
            {
                continue;
            }
            brain
                .tracked
                .record(tick, unit.entity_id, TrackedKind::Breach, None, claim);
            log::debug!("auto play: breach {} through {goal:?}", unit.entity_id);
            if !super::orders::issue(brain, out, rules, obs, unit.entity_id, goal) {
                brain.tracked.void(unit.entity_id, TrackedKind::Breach);
                continue;
            }
            issued += 1;
            continue;
        }
        let threats = index.near(unit.cell, LOOK_CELLS);
        if threats.is_empty() {
            continue;
        }
        // A gun covering the next cell is a blocker to a unit that cannot
        // answer it. The walkers this is for are the ones with no gun at all —
        // an engineer on its way to a derrick, a harvester on its way home —
        // and for them the friends standing nearby are not what keeps them
        // alive: the round does. So the covering gun is the thing to walk
        // around, and the balance below is not asked.
        let armed =
            super::advance::weapon_range_cells(rules, ty, obs.is_deployed(unit.entity_id)) > 0;
        let Some(blocker) = blocker_at(&threats, heading)
            .or_else(|| {
                (!armed)
                    .then(|| threats.iter().find(|threat| threat.covers(heading)))
                    .flatten()
            })
            .copied()
        else {
            continue;
        };
        // Two different questions, and the walk answers one of them alone.
        //
        // The objective inside those guns is the army's decision, not this
        // pass's: circling it would be a unit that never arrives, so the
        // balance below decides whether the push can take it.
        //
        // The objective *past* those guns is a march, and a march is not a
        // battle. A column that weighs "how many of us are there" against a
        // defense and walks in anyway pays for the crossing in units, and the
        // ones that arrive are the ones the tower did not shoot — the report
        // of a push that crossed a tesla's ring and reached the base with
        // half of it. So a route that only passes by is rerouted whatever the
        // numbers say, and only the target itself is weighed.
        let passing = passes_by(objective, &blocker);
        // The local fight, in hit points: what stands with this unit against
        // what can shoot at where it is walking.
        let ours = friends.sum_within(unit.cell, BALANCE_CELLS);
        let theirs = threats
            .iter()
            .filter(|threat| distance(threat.cell, heading) <= BALANCE_CELLS)
            .fold(0i32, |total, threat| total.saturating_add(threat.points));
        // Numbers do not help the unit that dies on the first shot. A defense
        // whose gun takes this unit off the field in one hit is not a fight to
        // be weighed against how many friends stand nearby — which is how a
        // dog or an engineer walked into a tesla coil beside an army that was
        // winning the neighbourhood.
        let one_shot = blocker_shooter(&brain.census, obs, rules, &blocker)
            .is_some_and(|shooter| kills_outright(rules, shooter, ty, unit.points));
        // A healthy, armored column that massively outguns a minor bunker
        // must be allowed to press the attack, even when that gun is merely
        // *along* the route. The old unconditional `passing` branch made an
        // entire tank fist sidestep forever around one pillbox.
        let overwhelming_armor = unit.traits.combat
            && unit.slot == super::census::Slot::Vehicle
            && unit.health_permille >= 650
            && ours >= theirs.saturating_mul(3)
            && !one_shot;
        if overwhelming_armor || (!passing && armed && !outmatched(ours, theirs) && !one_shot) {
            continue;
        }
        // Two legs, two sides: the unit's own side first, the other only when
        // the first leaves the map or another gun covers it.
        let side = detour_side(unit.cell, objective, &blocker);
        let mut chosen = None;
        for side in [side, -side] {
            let Some(cell) = detour_cell(unit.cell, objective, &blocker, extent, side) else {
                continue;
            };
            if obs.is_ground_walkable(cell.0, cell.1)
                && obs.can_reach(ty.movement_zone, unit.cell, cell)
                && safe_leg_from_other_guns(&index, unit.cell, cell, blocker.entity_id)
                && safe_exit_from_other_guns(&index, cell, objective, blocker.entity_id)
                && index.covering(cell, LOOK_CELLS).is_empty()
            {
                chosen = Some(cell);
                break;
            }
        }
        let Some(cell) = chosen else {
            let blocked_since = brain.avoid_deadends.entry(unit.entity_id)
                .or_insert((objective, tick));
            if blocked_since.0 != objective { *blocked_since = (objective, tick); }
            if tick.saturating_sub(blocked_since.1) >= DETOUR_GIVE_UP_FRAMES {
                // Pull back only to reachable, observed-safe ground. Stop if
                // there is no such post: don't "retreat" through another gun.
                let home = brain.census.rally.or(brain.census.base_cell);
                let safe = home.filter(|post| {
                    obs.can_reach(ty.movement_zone, unit.cell, *post)
                        && !super::tactics::unsafe_post(rules, obs, unit.entity_id, *post)
                        && index.covering(*post, LOOK_CELLS).is_empty()
                });
                let goal = safe.map_or(Goal::Stop, |cell| Goal::Move { cell });
                if out.want(unit.entity_id, 0, goal) {
                    brain.tracked.forget_primary(unit.entity_id);
                    brain.avoid_retreat_until.insert(unit.entity_id,
                        tick.saturating_add(DETOUR_REPLAN_FRAMES));
                    log::info!("auto play: {} abandons unsafe route {:?}; fallback {:?}",
                        unit.entity_id, objective, goal);
                }
                brain.avoid_deadends.remove(&unit.entity_id);
            }
            continue;
        };
        brain.avoid_deadends.remove(&unit.entity_id);
        if super::orders::issue_tracked(
            &mut brain.tracked,
            brain.trace,
            out,
            rules,
            obs,
            unit.entity_id,
            TrackedKind::Avoid,
            None,
            Some(cell),
            Goal::Move { cell },
        ) {
            log::debug!(
                "auto play: avoid {} around {} by {:?} ({} vs {} hp)",
                unit.entity_id,
                blocker.entity_id,
                cell,
                ours,
                theirs
            );
            issued += 1;
        }
    }
    if total > 0 {
        brain.avoid_cursor = (start + scanned) % total;
    }
    if !breaches.is_empty() {
        remember_breaches(brain, tick, &breaches);
    }
    answer_breaches(brain, rules, obs, out, tick);
    // A walk the map refused is the other half of the same question: the gate
    // above answers a unit already walking into a wall, this answers the one
    // whose order never took.
    answer_blocked(brain, rules, obs, out, tick);
}

/// How long a refused destination is answered for, in frames: twenty seconds.
const BLOCKED_FRAMES: u64 = 300;

/// How far along the line to the destination the first closed cell is looked
/// for, in cells.
const BLOCKED_SCAN_CELLS: i32 = 24;

/// Answer a walk the map refused.
///
/// `orders::issue` refuses a destination it cannot reach and leaves the unit
/// with no mission at all — and every pass here reads missions, so the unit is
/// invisible to all of them. That is the unit parked in front of a gate after
/// it shot the neutral building beside it: the building fell, the walk was
/// refused, and nothing ever asked what stood in the way.
///
/// The refused cell is recorded by the gate that refused it; this pass walks
/// the line to it, finds the first cell it cannot enter, and clicks that cell
/// the way a player does: a forced cell fire for an overlay (a wall, a gate,
/// a barricade — anything the overlay grid holds), an attack on a structure.
/// Terrain that no click can open is dropped, so a cliff does not become a
/// permanent demolition order.
fn answer_blocked(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
    tick: u64,
) {
    if !obs.has_ground_grid() {
        return;
    }
    // This frame's refusals join the ones still unanswered from earlier
    // frames: the gate that opened may need a second shot, and a request that
    // lived one frame would be a request the unit has to earn again.
    let fresh = std::mem::take(&mut out.blocked);
    for (unit, cell, at) in fresh {
        brain.blocked_goals.insert(unit, (cell, at));
    }
    if brain.blocked_goals.is_empty() {
        return;
    }
    brain
        .blocked_goals
        .retain(|_, (_, at)| tick.saturating_sub(*at) < BLOCKED_FRAMES);
    let blocked: Vec<(u64, (u16, u16))> = brain
        .blocked_goals
        .iter()
        .map(|(id, (cell, _))| (*id, *cell))
        .collect();
    for (entity_id, destination) in blocked {
        let Some(unit) = brain
            .census
            .units
            .iter()
            .find(|unit| unit.entity_id == entity_id)
        else {
            brain.blocked_goals.remove(&entity_id);
            continue;
        };
        if !unit.controllable() || unit.in_service() {
            continue;
        }
        let Some(blocker) = first_closed_cell(obs, unit.cell, destination) else {
            brain.blocked_goals.remove(&entity_id);
            continue;
        };
        // A walker with no gun cannot open what is in its way. Sending it the
        // click anyway is an order the engine refuses every frame, which is
        // the engineer standing at a neutral wall for the rest of the match.
        // Ask the house's guns instead, the same way the walking pass does,
        // and leave the engineer out of it until the wall is gone.
        let armed = obs.object_type(rules, unit.type_id).is_some_and(|ty| {
            super::advance::weapon_range_cells(rules, ty, obs.is_deployed(entity_id)) > 0
        });
        if !armed {
            brain
                .breaches
                .push((blocker, tick.saturating_add(BREACH_REQUEST_FRAMES)));
            brain.blocked_goals.remove(&entity_id);
            brain.tracked.refuse(tick, entity_id, TrackedKind::Capture);
            continue;
        }
        let goal = if obs.overlay_at(blocker.0, blocker.1) {
            Goal::AttackCell { cell: blocker }
        } else if let Some((target, true)) = obs.structure_at(rules, blocker) {
            let _ = target;
            // The house's own building is not the thing in the way of its own
            // unit's walk: drop the record rather than shell the base.
            brain.blocked_goals.remove(&entity_id);
            continue;
        } else if let Some((target, false)) = obs.structure_at(rules, blocker) {
            Goal::Attack { target }
        } else {
            // Closed ground with nothing to click: terrain, not an obstacle.
            brain.blocked_goals.remove(&entity_id);
            continue;
        };
        if !brain
            .tracked
            .admits(tick, entity_id, TrackedKind::Breach, None, Some(blocker))
        {
            continue;
        }
        brain
            .tracked
            .record(tick, entity_id, TrackedKind::Breach, None, Some(blocker));
        log::debug!(
            "auto play: {} blocked at {:?} on the way to {:?}; clicking {:?}",
            entity_id,
            blocker,
            destination,
            goal
        );
        if super::orders::issue(brain, out, rules, obs, entity_id, goal) {
            brain.blocked_goals.remove(&entity_id);
        } else {
            brain.tracked.void(entity_id, TrackedKind::Breach);
        }
    }
}

/// The first cell on the straight line from `from` to `to` that the ground
/// projection will not let this unit enter.
fn first_closed_cell(
    obs: &Observation<'_>,
    from: (u16, u16),
    to: (u16, u16),
) -> Option<(u16, u16)> {
    let mut cell = from;
    for _ in 0..BLOCKED_SCAN_CELLS {
        if cell == to {
            return None;
        }
        cell = step_toward(cell, to);
        if cell == to {
            // The destination itself is closed to *walking* (a building's own
            // cell is), but the click that matters is the one before it.
            return None;
        }
        if !obs.is_ground_walkable(cell.0, cell.1) {
            return Some(cell);
        }
    }
    None
}

/// How many wall cells one frame may ask the house's guns to open, in cells.
const BREACH_REQUESTS_PER_FRAME: usize = 2;

/// How far from the wall a unit may stand and be the one that opens it, in
/// cells: close enough that the shot is worth the walk.
const BREACH_REACH_CELLS: i32 = 10;

/// Give each wall an unarmed unit ran into to the nearest armed unit of the
/// house — the engineer's escort, in effect.
///
/// The engineer itself is left where it stopped: it has no gun for a wall, and
/// the capture step re-issues once the wall is gone. The helper is a normal
/// `AttackCell` force-fire on the wall's cell, which is the same shot
/// [`super::avoidance`] orders when the gun is the walker's own.
/// How long a request for the house's guns stands, in frames: twenty seconds.
///
/// Long enough that it survives the frames in which every armed unit nearby is
/// busy with its own fight, short enough that a gate nobody can open is not
/// asked for forever.
const BREACH_REQUEST_FRAMES: u64 = 300;

/// How many requests one frame may hand out. A gate is answered in one frame
/// when the guns are there; the cap keeps a wall of requests from spending the
/// whole frame's order budget.
const BREACH_ANSWERS_PER_FRAME: usize = 4;

/// How many outstanding requests the brain holds. A handful of gates and walls
/// at a time; the requests that matter are the ones the army is walking into.
const BREACH_REQUESTS_HELD: usize = 8;

fn request_capture_wall_breaches(
    brain: &mut AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
) {
    let mut requested = Vec::new();
    for engineer in &brain.census.units {
        if requested.len() >= BREACH_REQUESTS_PER_FRAME { break; }
        if !engineer.traits.engineer || !engineer.controllable() || engineer.in_service() {
            continue;
        }
        let Some(Goal::Capture { target }) = brain.tracked.primary_goal(engineer.entity_id)
        else { continue; };
        let Some((destination, _)) = obs.object_cell_and_type(target) else { continue; };
        if !obs.is_cell_visible(destination.0, destination.1)
            || !obs.can_take_building(rules, engineer.entity_id, target) {
            continue;
        }
        let mut sample = engineer.cell;
        for _ in 0..32 {
            if sample == destination { break; }
            sample = step_toward(sample, destination);
            if !obs.is_cell_visible(sample.0, sample.1) { break; }
            if obs.wall_at(rules, sample.0, sample.1)
                && obs.wall_owner_at(sample.0, sample.1) != Some(obs.house())
                && distance(engineer.cell, sample) <= 16
            {
                requested.push(sample);
                break;
            }
        }
    }
    if !requested.is_empty() {
        remember_breaches(brain, obs.tick(), &requested);
    }
}

/// Add this frame's closed cells to the brain's standing requests.
fn remember_breaches(brain: &mut AssistBrain, tick: u64, breaches: &[(u16, u16)]) {
    let until = tick.saturating_add(BREACH_REQUEST_FRAMES);
    for cell in breaches.iter().copied() {
        if let Some((_, expires)) = brain.breaches.iter_mut().find(|(at, _)| *at == cell) {
            *expires = (*expires).max(until);
        } else if brain.breaches.len() < BREACH_REQUESTS_HELD {
            brain.breaches.push((cell, until));
        }
    }
}

fn answer_breaches(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
    tick: u64,
) {
    // Expired requests go first: the gate that stood here two minutes ago is a
    // cell some unit is now walking through.
    brain.breaches.retain(|(_, until)| tick < *until);
    if brain.breaches.is_empty() {
        return;
    }
    // Snapshotted before the tracked table is written: the ledger is borrowed
    // for the search and the table is not part of it.
    let helpers: Vec<(u64, (u16, u16))> = brain
        .census
        .units
        .iter()
        .filter(|unit| unit.traits.combat && unit.controllable() && !unit.in_service()
            && obs.entity_type(rules, unit.entity_id).is_some_and(|ty|
                super::build::armament(rules, ty).anti_ground))
        .map(|unit| (unit.entity_id, unit.cell))
        .collect();
    if helpers.is_empty() {
        return;
    }
    let pending: Vec<(u16, u16)> = brain.breaches.iter().map(|(cell, _)| *cell).collect();
    let mut answered: Vec<(u16, u16)> = Vec::new();
    for cell in pending.iter().copied().take(BREACH_ANSWERS_PER_FRAME) {
        let Some(&(helper, _)) = helpers
            .iter()
            .filter(|(_, from)| distance(*from, cell) <= BREACH_REACH_CELLS)
            .min_by_key(|(id, from)| (super::squared_distance(*from, cell), *id))
        else {
            continue;
        };
        let Some(ty) = brain
            .census
            .units
            .iter()
            .find(|unit| unit.entity_id == helper)
            .and_then(|unit| obs.object_type(rules, unit.type_id))
        else {
            continue;
        };
        if super::advance::weapon_range_cells(rules, ty, obs.is_deployed(helper)) <= 0 {
            continue;
        }
        if super::orders::ordered_this_frame(out, helper) {
            continue;
        }
        if !brain
            .tracked
            .admits(tick, helper, TrackedKind::Avoid, None, Some(cell))
        {
            continue;
        }
        // What stands there decides how it is opened: a wall is an overlay,
        // and the forced click on the cell is the shot; a structure (a gate, a
        // shed) is an object, and a cell click on it is an order the engine
        // answers with nothing. The house's own structure is never opened.
        let goal = match obs.structure_at(rules, cell) {
            Some((_, true)) => continue,
            Some((id, false)) => {
                if brain.census.enemies.iter().any(|enemy| {
                    enemy.entity_id == id && super::orders::engineer_claimed(
                        rules, obs, &brain.census, &brain.tracked, *enemy)
                }) { continue; }
                Goal::Attack { target: id }
            },
            None => Goal::AttackCell { cell },
        };
        brain
            .tracked
            .record(tick, helper, TrackedKind::Avoid, None, Some(cell));
        log::debug!("auto play: {helper} opens {goal:?} at {cell:?} for the house's engineers");
        if super::orders::issue(brain, out, rules, obs, helper, goal) {
            // The engineer's own path is clear once the wall is gone; nothing
            // else is ordered here. The request is kept until the issue is
            // accepted, so a helper whose own order preempted the shot does
            // not make the engineer walk into the gate again.
            answered.push(cell);
        }
    }
    brain.breaches.retain(|(cell, _)| !answered.contains(cell));
}

/// Where the unit is walking: what its team is aimed at, else what it is
/// already shooting, else the nearest hostile. `None` when the house can see
/// nothing at all.
fn objective(
    brain: &AssistBrain,
    obs: &Observation<'_>,
    entity_id: u64,
    from: (u16, u16),
    squad_targets: &BTreeMap<u64, (u16, u16)>,
) -> Option<(u16, u16)> {
    let enemies = &brain.census.enemies;
    squad_targets
        .get(&entity_id)
        .copied()
        .or_else(|| {
            obs.attack_target(entity_id).and_then(|target| {
                enemies
                    .iter()
                    .find(|enemy| enemy.entity_id == target)
                    .map(|enemy| enemy.cell)
            })
        })
        .or_else(|| {
            enemies
                .iter()
                .min_by_key(|enemy| (super::squared_distance(enemy.cell, from), enemy.entity_id))
                .map(|enemy| enemy.cell)
        })
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn a_second_turret_blocks_a_seemingly_clear_side_route() {
        let mut threats = ThreatIndex::new();
        threats.insert(Threat::gun(1, (16, 10), 5));
        assert!(safe_leg_from_other_guns(&threats, (8,10), (14,10), 1));
        threats.insert(Threat::gun(2, (11, 12), 2));
        assert!(!safe_leg_from_other_guns(&threats, (8,10), (14,10), 1));
    }

    #[test]
    fn a_corner_with_a_second_gun_ahead_is_not_a_safe_exit() {
        let mut threats = ThreatIndex::new();
        threats.insert(Threat::gun(1, (16,10), 5));
        threats.insert(Threat::gun(2, (16,18), 2));
        assert!(!safe_exit_from_other_guns(&threats, (12,18), (24,18), 1));
        assert!(safe_exit_from_other_guns(&threats, (12,22), (24,22), 1));
    }

    /// A defence structure's guns, as the pass weighs them: `[GAPILL]`
    /// `Strength=400` with a five-cell `Vulcan`.
    fn pillbox(cell: (u16, u16)) -> Threat {
        Threat {
            entity_id: 7,
            cell,
            reach: 5,
            points: 400,
        }
    }

    /// A rifleman cannot take a pillbox and walks around it: inside the band
    /// the first leg goes sideways to the edge of the guns, not forward into
    /// them.
    #[test]
    fn a_lone_unit_leaves_the_guns_sideways_before_walking_on() {
        let unit = (10, 10);
        let objective = (24, 10);
        let threat = pillbox((16, 10));
        let cell = detour_cell(
            unit,
            objective,
            &threat,
            (60, 60),
            detour_side(unit, objective, &threat),
        )
        .expect("a leg out of the band");
        assert!(
            !threat.covers(cell),
            "{cell:?} is still inside the guns at {:?}",
            threat.cell
        );
        assert_eq!(
            cell.0, unit.0,
            "the first leg does not advance while still in the band"
        );
        assert!(
            cell.1.abs_diff(unit.1) >= 6,
            "six cells to clear a five-cell reach"
        );
    }

    /// Outside the band, the second leg walks past the threat at the clearance
    /// already gained instead of cutting back in front of it.
    #[test]
    fn past_the_band_the_walk_goes_forward_again() {
        let unit = (10, 4);
        let objective = (24, 10);
        let threat = pillbox((16, 10));
        let cell = detour_cell(
            unit,
            objective,
            &threat,
            (60, 60),
            detour_side(unit, objective, &threat),
        )
        .expect("a leg past the threat");
        assert!(!threat.covers(cell), "{cell:?} is inside the guns");
        assert!(cell.0 > unit.0, "past the threat, towards the objective");
        assert_eq!(cell.1, unit.1, "the clearance is kept");
        assert!(
            distance(cell, objective) < distance(unit, objective),
            "the detour makes progress"
        );
    }

    /// A target inside the guns is the army's decision: circling it would be a
    /// unit that never arrives, and the push gate already weighed the fight.
    #[test]
    fn a_target_inside_the_guns_is_not_routed_around() {
        let threat = pillbox((16, 10));
        assert!(!passes_by((16, 10), &threat), "the pillbox itself");
        assert!(!passes_by((18, 12), &threat), "the structure beside it");
        assert!(passes_by((24, 10), &threat), "a base behind it");
    }

    /// Ten conscripts are not ten tanks: the comparison is in hit points, and
    /// only a local deficit turns a march into a detour.
    #[test]
    fn only_a_lost_local_fight_is_walked_around() {
        assert!(outmatched(100, 400), "one rifleman against a pillbox");
        assert!(outmatched(300, 400), "one tank against a pillbox");
        assert!(!outmatched(800, 400), "two tanks against the same pillbox");
        assert!(!outmatched(0, 0), "nothing to fight");
    }

    /// Only a walk is rerouted: a fighter keeps its target, a held post is
    /// held, and a dug-in unit cannot walk out of anything.
    #[test]
    fn only_a_walk_is_rerouted() {
        assert!(walking(MissionId::from_known(MissionType::Move), false));
        assert!(walking(
            MissionId::from_known(MissionType::AttackMove),
            false
        ));
        assert!(
            !walking(MissionId::from_known(MissionType::Attack), false),
            "the fighter's target outranks the route"
        );
        assert!(
            !walking(MissionId::from_known(MissionType::Guard), false),
            "a held post is worth holding"
        );
        assert!(
            !walking(MissionId::from_known(MissionType::Move), true),
            "a dug-in unit cannot walk"
        );
    }

    /// The blocker is the nearest gun covering the next step; a threat the walk
    /// is not entering is not a reason to turn.
    #[test]
    fn the_nearest_gun_on_the_way_is_the_one_that_blocks() {
        let threats = [
            pillbox((16, 10)),
            Threat {
                entity_id: 9,
                ..pillbox((18, 10))
            },
        ];
        assert_eq!(
            blocker_at(&threats, (11, 10)).map(|threat| threat.entity_id),
            Some(7),
            "the nearer gun"
        );
        assert_eq!(
            blocker_at(&threats, (10, 30)),
            None,
            "a step that enters no reach"
        );
    }

    /// A short wall across the walk is walked around it: the corner is the
    /// first free cell to one side of the blocked one, at the same point along
    /// the walk, on whichever side the unit looks.
    #[test]
    fn a_short_wall_is_walked_around() {
        // A three-cell north-south wall at x=16: a walk east crosses it at
        // (16,10), so the lateral axis is y and the corner is (16,8) or
        // (16,12).
        let wall = |x: u16, y: u16| x == 16 && (9..=11).contains(&y);
        let free = |x: u16, y: u16| !wall(x, y);
        assert_eq!(way_around((16, 10), false, -1, &free), Some((16, 8)));
        assert_eq!(way_around((16, 10), false, 1, &free), Some((16, 12)));
        // The same wall crossed by a walk south: the lateral axis is x then.
        let wall = |x: u16, y: u16| y == 10 && (13..=19).contains(&x);
        let free = |x: u16, y: u16| !wall(x, y);
        assert_eq!(way_around((16, 10), true, -1, &free), Some((12, 10)));
        assert_eq!(way_around((16, 10), true, 1, &free), Some((20, 10)));
    }

    /// A wall that runs six cells or more to either side is a ring around a
    /// base, not a segment across a road: the walk finds no way around it, and
    /// the pass shoots its way through instead.
    #[test]
    fn a_ring_is_not_walked_around() {
        let wall = |x: u16, y: u16| x == 16 && (4..=16).contains(&y);
        let free = |x: u16, y: u16| !wall(x, y);
        assert_eq!(way_around((16, 10), false, -1, &free), None);
        assert_eq!(way_around((16, 10), false, 1, &free), None);
    }

    /// The side is read from where the unit stands, so two frames cannot pick
    /// opposite ways around the same threat.
    #[test]
    fn the_detour_keeps_to_the_side_the_unit_is_already_on() {
        let threat = pillbox((16, 10));
        assert_eq!(
            detour_side((10, 8), (24, 10), &threat),
            -1,
            "above the line"
        );
        assert_eq!(detour_side((10, 13), (24, 10), &threat), 1, "below it");
        assert_eq!(detour_side((10, 10), (24, 10), &threat), -1, "on it: fixed");
    }

    /// A request for the house's guns outlives the frame that made it.
    ///
    /// The helper that answers it is one unit with its own order, so a request
    /// that lives one frame is one the engineer has to run into the gate again
    /// for — the gate that takes a squad's whole crossing to open.
    #[test]
    fn a_breach_request_outlives_the_frame_that_made_it() {
        let mut brain = AssistBrain::new(0xA17);
        remember_breaches(&mut brain, 100, &[(10, 10)]);
        assert_eq!(
            brain.breaches,
            vec![((10, 10), 100 + BREACH_REQUEST_FRAMES)],
            "the cell is held until its own expiry"
        );
        // The same cell again refreshes one request instead of stacking two.
        remember_breaches(&mut brain, 200, &[(10, 10)]);
        assert_eq!(brain.breaches.len(), 1, "one cell, one request");
        assert_eq!(brain.breaches[0].1, 200 + BREACH_REQUEST_FRAMES);
        // And the table does not grow without bound.
        let many: Vec<(u16, u16)> = (0..20u16).map(|x| (x, 4)).collect();
        remember_breaches(&mut brain, 300, &many);
        assert_eq!(brain.breaches.len(), BREACH_REQUESTS_HELD);
    }
}
