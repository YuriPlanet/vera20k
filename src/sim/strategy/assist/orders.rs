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

use std::collections::BTreeSet;

use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::command::Command;
use crate::sim::control::brain::Outbox;
use crate::sim::control::intent::Intent;
use crate::sim::control::observation::{EnemyEntity, Observation};
use crate::sim::control::{Goal, MicroKind, MicroParams};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::strategy::assist::AssistBrain;

use super::squared_distance;
use super::tracked::TrackedKind;

mod common;

/// The admission half, re-exported so every caller keeps the path it had.
pub(in crate::sim::strategy::assist) use common::{
    admits, already_attacking, issue, on_map, ordered_this_frame, reachable,
};

/// How many new unit orders one frame may carry. The board drops everything
/// past its own per-frame budget, so asking for more would only spend the frame
/// on orders that never leave; this keeps the surviving orders to the newest
/// units first, in storage order.
const FORCE_ORDERS_PER_FRAME: usize = 8;

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
/// A garrisoned rifleman is in limbo: `order_actor_admits` refuses it, and no
/// step here brings it back out, so every unit sent in leaves the army for
/// good. Measured on Death.mmx (America, seed 24301, 36000 ticks) the uncapped
/// rule put 28-40 riflemen into `Mission::Capture` and out of the playfield,
/// which is where the 56-unit gap between the old `fighters=99` and the 50
/// units the brain could actually move came from. Retail's own base-defence
/// teams hold a handful of structures; the rest of the army stays outside.
const GARRISON_MAX: usize = 4;

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
    if !brain
        .census
        .units
        .iter()
        .any(|unit| walks_into_buildings_flags(&unit.traits))
    {
        return;
    }
    let engineer_targets: Vec<EnemyEntity> = obs.capturable_buildings(rules).collect();
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
        let landed = brain
            .capturable_last
            .iter()
            .filter(|id| !now.contains(*id))
            .filter(|id| owned.contains(*id))
            .count();
        for _ in 0..landed {
            brain.engineers.note_captured();
        }
        brain.capturable_last = now;
    }
    // Which of those belong to an enemy, and where that enemy's units stand.
    // `capturable_buildings` is every `Capturable=` building the house can see —
    // the map's own oil derricks and hospitals included — and the nearest of
    // them was how an engineer walked straight into a base: the derrick inside
    // the enemy's yard is near, and the engineer that goes for it is a 500-credit
    // gift. Buildings do not guard anything, so only units count as pressure.
    let hostile: BTreeSet<u64> = obs
        .enemy_entities()
        .map(|entity| entity.entity_id)
        .collect();
    let guards: Vec<(u16, u16)> = obs
        .enemy_entities()
        .filter(|entity| {
            obs.object_type(rules, entity.type_id)
                .is_some_and(|ty| ty.category != ObjectCategory::Building)
        })
        .map(|entity| entity.cell)
        .collect();
    // Where the enemy's own buildings stand. A walk that passes through their
    // base is a walk through the guns that defend it, whatever the target's own
    // clearance says: the engineer that went *through* a construction yard to
    // reach a neutral derrick was reading a straight line that said "clear".
    let structures: Vec<(u16, u16)> = obs
        .enemy_entities()
        .filter(|entity| {
            obs.object_type(rules, entity.type_id)
                .is_some_and(|ty| ty.category == ObjectCategory::Building)
        })
        .map(|entity| entity.cell)
        .collect();
    // What can see a spy coming. The retail rules mark no `Detector=` key on any
    // type, and the dog's own section (`[DOG] NotHuman=yes`, `Natural=yes`)
    // carries nothing about spies either: the reveal is engine behaviour, and the
    // only retail-marked pair for it is the dog's. A unit whose weapon warhead
    // mind-controls is on the list for the worse reason — a spy that is taken is
    // a spy the enemy now owns.
    let watchers: Vec<(u16, u16)> = obs
        .enemy_entities()
        .filter(|entity| {
            obs.object_type(rules, entity.type_id).is_some_and(|ty| {
                super::census::Traits::of(rules, ty).scout || mind_controls(rules, ty)
            })
        })
        .map(|entity| entity.cell)
        .collect();
    let spy_targets: Vec<(EnemyEntity, u8)> = obs
        .spyable_buildings(rules)
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
        .filter(|(target, _)| !spy_route_blocked(obs, rules, target.cell, &watchers))
        .collect();
    // The receipts from spies already sent. A spy that reached its building is
    // gone from the ledger, and that disappearance is the only signal there is:
    // the infiltration itself lives in Ares' building extension data, which this
    // brain does not read. A spy that died on the way is indistinguishable from
    // one that landed, so a class can be marked done by a spy that never
    // arrived — the cost is one missed unlock, never a wasted walk.
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
            brain.infiltrated.insert(code);
        }
    }
    let mut issued = 0;
    for unit in &brain.census.units {
        if issued >= FORCE_ORDERS_PER_FRAME {
            break;
        }
        if !unit.orderable || unit.hand || ordered_this_frame(out, unit.entity_id) {
            continue;
        }
        let Some(ty) = obs.object_type(rules, unit.type_id) else {
            continue;
        };
        if !walks_into_buildings(rules, ty) {
            continue;
        }
        // A spy takes what is worth most; anyone else takes what is nearest.
        let chosen = if ty.agent {
            spy_targets
                .iter()
                .max_by_key(|(target, value)| (*value, -squared_distance(unit.cell, target.cell)))
                .map(|(target, _)| *target)
        } else {
            // One building takes one engineer. Without this both were sent at
            // the same derrick, the first took it, and the second arrived at a
            // building that was already ours and stood there.
            let free: Vec<EnemyEntity> = engineer_targets
                .iter()
                .copied()
                // Only the buildings this engineer's click takes: an ally's
                // plant answers Repair, and a `Capturable=` wall that is not
                // `Repairable=` answers nothing at all. The list itself is the
                // type-level candidate set; the click is per engineer.
                .filter(|target| obs.can_take_building(rules, unit.entity_id, target.entity_id))
                .filter(|target| {
                    !brain.tracked.claimed_by_other(
                        tick,
                        TrackedKind::Capture,
                        target.entity_id,
                        unit.entity_id,
                    )
                })
                .collect();
            engineer_preference(
                rules,
                obs,
                &free,
                &hostile,
                &guards,
                &structures,
                unit.cell,
                brain.census.refineries < DERRICK_WORTH_REFINERIES,
            )
        };
        let Some(target) = chosen else {
            continue;
        };
        // The walk is re-decided as it goes, not committed for the whole term.
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
        if brain.tracked.is_claimed(tick, unit.entity_id) {
            let close = guards.iter().any(|cell| {
                squared_distance(*cell, unit.cell) <= CAPTURE_CLEAR_CELLS * CAPTURE_CLEAR_CELLS
            });
            let crossed = tick % CAPTURE_RECHECK_FRAMES == 0
                && path_crosses(&guards, unit.cell, target.cell, PATH_CLEAR_CELLS);
            if close || crossed {
                brain.tracked.void(unit.entity_id, TrackedKind::Capture);
                log::debug!(
                    "auto play: engineer {} drops the walk, the ground turned bad",
                    unit.entity_id
                );
                continue;
            }
        }
        // L4: it has been told to take this building, and the walk is not
        // over, so it is not told again.
        if !brain.tracked.admits(
            tick,
            unit.entity_id,
            TrackedKind::Capture,
            Some(target.entity_id),
            Some(target.cell),
        ) {
            continue;
        }
        brain.tracked.record(
            tick,
            unit.entity_id,
            TrackedKind::Capture,
            Some(target.entity_id),
            Some(target.cell),
        );
        issue(
            brain,
            out,
            rules,
            obs,
            unit.entity_id,
            Goal::Capture {
                target: target.entity_id,
            },
        );
        // A spy that walks into a barracks, a war factory or a lab has bought
        // the unlock, and the walk is not worth repeating. Remember which class
        // this one was sent to: the unit is consumed on arrival, so its
        // disappearance is the receipt.
        if ty.agent
            && let Some(code) = obs
                .object_type(rules, target.type_id)
                .and_then(|target_ty| spy_class_code(rules, target_ty))
        {
            brain.spy_errand.insert(unit.entity_id, code);
        }
        issued += 1;
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
    let tick = obs.tick();
    if !brain
        .census
        .units
        .iter()
        .any(|unit| unit.traits.suicide && unit.orderable && !unit.hand)
    {
        return;
    }
    // What the house would rather not catch in its own blast.
    let own: Vec<(u16, u16)> = brain
        .census
        .units
        .iter()
        .filter(|unit| !unit.traits.suicide)
        .map(|unit| unit.cell)
        .collect();
    // The enemy's structures, which is what a one-shot body is worth spending on.
    let targets: Vec<EnemyEntity> = obs
        .enemy_entities()
        .filter(|enemy| {
            obs.object_type(rules, enemy.type_id)
                .is_some_and(|ty| ty.category == ObjectCategory::Building)
        })
        .collect();
    if targets.is_empty() {
        return;
    }
    let mut issued = 0;
    for unit in &brain.census.units {
        if issued >= FORCE_ORDERS_PER_FRAME {
            break;
        }
        if !unit.traits.suicide || !unit.orderable || unit.hand {
            continue;
        }
        if ordered_this_frame(out, unit.entity_id) {
            continue;
        }
        let Some(ty) = obs.object_type(rules, unit.type_id) else {
            continue;
        };
        // What it costs, in the census's own currency.
        let cost = super::census::fight_points(ty, 1000);
        let reach = super::advance::weapon_range_cells(rules, ty, false).max(1);
        let chosen = targets
            .iter()
            .filter_map(|target| {
                let value = obs
                    .object_type(rules, target.type_id)
                    .map_or(0, |target_ty| super::census::fight_points(target_ty, 1000));
                // Only a trade: the blast has to be worth more than the body.
                // Equal is not a trade — the house pays the walk as well.
                if value <= cost {
                    return None;
                }
                // Nothing of the house's inside the blast. `SPLASH_CELLS` is a
                // stand-in for the warhead's `CellSpread=`, taken from the
                // dearest of them: a bomb truck's is 8 and the desolator's
                // eruption is 10, so the check is the pessimistic one.
                let nearby = own.iter().any(|cell| {
                    squared_distance(*cell, target.cell) <= SPLASH_CELLS * SPLASH_CELLS
                });
                if nearby {
                    return None;
                }
                Some((
                    std::cmp::Reverse(value),
                    squared_distance(unit.cell, target.cell),
                    target.entity_id,
                    target.cell,
                ))
            })
            .min_by_key(|(value, walk, id, _)| (*value, *walk, *id))
            .map(|(_, _, id, cell)| (id, cell));
        let Some((target, target_cell)) = chosen else {
            continue;
        };
        if !brain.tracked.admits(
            tick,
            unit.entity_id,
            TrackedKind::Attack,
            Some(target),
            None,
        ) {
            continue;
        }
        brain.tracked.record(
            tick,
            unit.entity_id,
            TrackedKind::Attack,
            Some(target),
            None,
        );
        // Out of reach it walks; in reach the world fires the bomb on the next
        // frame the gun is ready, and the unit is consumed by its own attack.
        let goal = if super::advance::cell_distance(unit.cell, target_cell) <= reach {
            Goal::Attack { target }
        } else {
            Goal::AttackMove { cell: target_cell }
        };
        issue(brain, out, rules, obs, unit.entity_id, goal);
        issued += 1;
    }
}

/// How near the house's own units have to stand to a blast for it not to be
/// worth firing, in cells.
///
/// The retail warheads that reach past their target are `TerrorBombWH 2`,
/// `DemobombWH 8` and `RadEruptionWarhead 10`; the widest of them is the one
/// that has to be safe, because the unit that carries it cannot know which it
/// will fire.
const SPLASH_CELLS: i32 = 10;

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
        if !ty.construction_yard {
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
        if in_the_base {
            continue;
        }
        if !obs.can_repack_yard(rules, unit.entity_id) {
            continue;
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
        .filter(|site| within(home_cells(brain), site.cell, HOME_REACH_CELLS))
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
        let Some(target) = nearest_site(&huts, unit.cell) else {
            continue;
        };
        if !brain.tracked.admits(
            tick,
            unit.entity_id,
            TrackedKind::Capture,
            Some(target),
            None,
        ) {
            continue;
        }
        brain.tracked.record(
            tick,
            unit.entity_id,
            TrackedKind::Capture,
            Some(target),
            None,
        );
        issue(
            brain,
            out,
            rules,
            obs,
            unit.entity_id,
            Goal::Capture { target },
        );
        issued += 1;
    }
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
    let anchors: Vec<(u16, u16)> = home_cells(brain)
        .chain(
            brain
                .census
                .units
                .iter()
                .filter(|unit| unit.traits.combat)
                .map(|unit| unit.cell),
        )
        .collect();
    let billets: Vec<_> = obs
        .occupiable_buildings(rules)
        .filter(|billet| within(anchors.iter().copied(), billet.cell, HOME_REACH_CELLS))
        .collect();
    if billets.is_empty() {
        return;
    }
    // The count is exact, not a guess from the census: a garrisoned unit is a
    // passenger of the building it walked into (`Observation::inside_transport`),
    // so a rifleman still in his factory's limbo is not mistaken for one.
    let mut garrisoned = brain
        .census
        .units
        .iter()
        .filter(|unit| unit.traits.occupier && obs.inside_transport(unit.entity_id).is_some())
        .count();
    let mut issued = 0;
    for unit in &brain.census.units {
        if issued >= FORCE_ORDERS_PER_FRAME || garrisoned >= GARRISON_MAX {
            break;
        }
        if !unit.traits.occupier
            || !unit.orderable
            || unit.hand
            || holds_or_fights(unit.mission)
            || ordered_this_frame(out, unit.entity_id)
        {
            continue;
        }
        let Some(billet) = billets
            .iter()
            .filter(|billet| squared_distance(unit.cell, billet.cell) <= HOME_REACH_CELLS.pow(2))
            .filter(|billet| obs.can_garrison(rules, unit.entity_id, billet.building_id))
            .min_by_key(|billet| squared_distance(unit.cell, billet.cell))
        else {
            continue;
        };
        // The garrison is the one that had no memory at all: it was told to
        // walk into the same building every frame, and a unit being told to
        // enter what it is already entering is a unit that never arrives.
        if !brain.tracked.admits(
            tick,
            unit.entity_id,
            TrackedKind::Capture,
            Some(billet.building_id),
            Some(billet.cell),
        ) {
            continue;
        }
        brain.tracked.record(
            tick,
            unit.entity_id,
            TrackedKind::Capture,
            Some(billet.building_id),
            Some(billet.cell),
        );
        if issue(
            brain,
            out,
            rules,
            obs,
            unit.entity_id,
            Goal::Capture {
                target: billet.building_id,
            },
        ) {
            issued += 1;
            garrisoned += 1;
        }
    }
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
fn infiltration_value(rules: &RuleSet, ty: &ObjectType, free: i32) -> u8 {
    // The refinery first, and above everything else. A spy in a bay takes the
    // enemy's own bank, which is the one infiltration that pays for the walk in
    // the same minute it lands; a radar or a lab is worth more in kind but only
    // once the house has an army to use it with. It used to share value 3 with
    // the radar, and the spy walked to whichever came first.
    if ty.resource_destination {
        // The refinery first, and above everything else — until the house is
        // rich enough that the enemy's bank changes nothing. A spy is a walk
        // into a base that does not screen for one, and the veteran classes are
        // a permanent unlock where a theft is a one-off number.
        if free >= SPY_RICH_CREDITS { 2 } else { 5 }
    } else if rules.build_power_types.iter().any(|name| *name == ty.id)
        || ty.super_weapon.is_some()
        || ty.super_weapon2.is_some()
    {
        4
    } else if ty.radar {
        3
    } else if rules.build_weapons_types.iter().any(|name| *name == ty.id)
        || rules.build_barracks_types.iter().any(|name| *name == ty.id)
    {
        2
    } else if rules.build_tech_types.iter().any(|name| *name == ty.id) {
        // The lab is the walk worth taking: infiltrating it is what hands the
        // house the other side's unit (`sim::spy_infiltration`'s `Tech`), which
        // is a type the wallet cannot buy. It used to rank last here, so a spy
        // walked past the lab to a barracks.
        3
    } else {
        1
    }
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
                    guarded,
                    std::cmp::Reverse(capture_value(rules, ty, short_of_income)),
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

/// What a building is worth to the house that captures it, in the same
/// categories the rules write down. [`infiltration_value`] is the spy's answer
/// to the same question through `Infiltrate=`; this is the engineer's through
/// `Engineer=`, and the two differ because the prizes differ: a spy walks out
/// with an effect, an engineer walks out owning the building.
///
/// The ranking is what `HouseClass::Can_Build` reads, not a list of names: a
/// captured building is a building the house holds, so a lab unlocks what a lab
/// unlocks and a yard unlocks a side. The map's own buildings unlock nothing
/// but they pay, and an oil derrick nobody is standing next to is the capture
/// an engineer is most likely to come back from.
fn capture_value(rules: &RuleSet, ty: &ObjectType, short_of_income: bool) -> u8 {
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

/// The nearest of `(id, cell)` sites, ties broken by id so the answer does not
/// depend on the order the entity store happens to yield.
fn nearest_site(sites: &[(u64, (u16, u16))], from: (u16, u16)) -> Option<u64> {
    sites
        .iter()
        .min_by_key(|(id, cell)| (squared_distance(from, *cell), *id))
        .map(|(id, _)| *id)
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
        if ordered_this_frame(out, unit.entity_id) {
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
pub(super) fn deploy(brain: &mut AssistBrain, obs: &Observation<'_>, out: &mut Outbox) {
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
    let wanted: Vec<(u64, i32)> = brain
        .census
        .units
        .iter()
        .filter(|unit| unit.traits.deployer && !unit.hand && unit.orderable)
        .map(|unit| {
            (
                unit.entity_id,
                nearest_distance(&brain.census.enemies, unit.cell),
            )
        })
        .collect();
    let mut issued = 0;
    for (entity_id, distance) in wanted {
        if issued >= DEPLOYS_PER_FRAME {
            break;
        }
        let deployed = obs.is_deployed(entity_id);
        let Some(down) = pose_to_take(deployed, distance) else {
            continue;
        };
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
    let harvest = MissionId::from_known(MissionType::Harvest);
    let unload = MissionId::from_known(MissionType::Unload);
    let idle: Vec<(u64, (u16, u16))> = brain
        .census
        .units
        .iter()
        .filter(|unit| {
            unit.traits.harvester
                && !unit.hand
                && unit.orderable
                && !unit.in_service()
                && unit.mission != harvest
                && unit.mission != unload
        })
        .map(|unit| (unit.entity_id, unit.cell))
        .collect();
    // Where the ore actually is, read on a term: the field does not move and a
    // scan of the ring around the base is not free.
    let extent = obs.map_extent();
    let anchor = brain.census.base_cell;
    if let Some(home) = anchor {
        let stale = brain
            .ore_scan
            .as_ref()
            .is_none_or(|(read_at, _)| read_at.saturating_add(super::ore::SCAN_TERM) <= tick);
        if stale {
            let cells = super::ore::cells_near(rules, obs, home, extent);
            brain.ore_scan = Some((tick, cells));
        }
    }
    let fields: &[(u16, u16)] = brain
        .ore_scan
        .as_ref()
        .map(|(_, cells)| cells.as_slice())
        .unwrap_or(&[]);
    // What the trucks must not be sent into. Ore inside an enemy base is the
    // wishlist's "the harvester walks into the enemy base to mine": the field is
    // real and the trip is a donation.
    let space = super::terrain::Space::read(obs, rules, &brain.census.home_cells, anchor);
    for (id, cell) in idle {
        if ordered_this_frame(out, id) {
            continue;
        }
        // The best field is the nearest ore the house may work: close, and not
        // under an enemy gun. Standing on ore already is its own answer, and the
        // cell the unit holds is the one field the scan can prove it is on.
        let target = if super::ore::at(rules, obs, cell) {
            Some(cell)
        } else {
            super::ore::nearest(
                cell,
                &fields
                    .iter()
                    .copied()
                    .filter(|field| space.ground_at(*field).buildable())
                    .collect::<Vec<_>>(),
            )
        };
        let Some(target) = target else {
            continue;
        };
        // No cell on the record: the click names the unit, not a place, and a
        // cell that is the unit's own position changes every frame the unit
        // walks — which would make the intent look stale and re-send it every
        // frame.
        if !brain
            .tracked
            .admits(tick, id, TrackedKind::Harvest, None, None)
        {
            continue;
        }
        if issue(brain, out, rules, obs, id, Goal::Harvest { cell }) {
            brain
                .tracked
                .record(tick, id, TrackedKind::Harvest, None, None);
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

/// How hurt a building may be before it is sold, in permille.
///
/// A captured plant at a sliver of health is a repair bill the house pays under
/// fire; the refund is most of what it cost and the ground is not worth the
/// wrench.
const HOLDLESS_HEALTH: i32 = 250;

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
        if ty.category != ObjectCategory::Building || buildable.contains(&unit.type_id) {
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
        let hurt = unit.health_permille <= HOLDLESS_HEALTH as u16;
        // "Exposed" alone is not a reason. A building inside the house's own
        // walls is defensible while an enemy is at the gate, and selling it then
        // is the windfall the enemy was after. Only a building that is also out
        // of reach counts, and it still has to be far or hurt to go.
        let abandoned = far && !space.ground_at(unit.cell).buildable();
        if !(far || hurt) {
            continue;
        }
        // A captured yard is not sold, it is packed. It is the one building that
        // is worth more as a unit: the maker drives home and becomes a second
        // base, while the refund is a fraction of what a second yard is worth.
        if ty.build_const_eligible() {
            log::info!(
                "auto play: pack the captured yard {} at {:?} for the drive home",
                ty.id,
                unit.cell
            );
            out.intent(Intent::UndeployBuilding {
                entity_id: unit.entity_id,
            });
            brain.sale_at = tick.saturating_add(SALE_GAP_FRAMES);
            return;
        }
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
    if rules.build_barracks_types.iter().any(|name| *name == ty.id) {
        Some(1)
    } else if rules.build_weapons_types.iter().any(|name| *name == ty.id) {
        Some(2)
    } else if rules.build_tech_types.iter().any(|name| *name == ty.id) {
        Some(3)
    } else {
        None
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

/// The wallet above which a spy is not sent for the enemy's bank.
///
/// A theft is a one-off number; the veteran classes are a permanent unlock.
/// Past this the house does not need the number.
const SPY_RICH_CREDITS: i32 = 20_000;

/// How near a spy's target a watcher may stand before the walk is refused, in
/// cells: the reveal range plus a step, since the spy is seen on approach.
const SPY_WATCH_CELLS: i32 = 6;

/// How near a crate has to be for it to be a detour, in cells.
///
/// A crate next to the unit is a bonus it takes on its way past, and any
/// unit may take it. A crate further away is a job, and the house does not
/// give a unit one: measured, a fighter that walked thirty cells for a box
/// turned a win into a timeout.
const CRATE_SEEK_CELLS: i32 = 8;

/// Send an idle scout to a nearby crate it can actually reach.
///
/// Last of the steps that move a unit, and only for a unit with nothing else
/// to do: a crate is a bonus, not a job, and a scout pulled off a fight or a
/// walk for one has lost more than the crate is worth. The pickup is the walk
/// — the map takes the crate when the unit enters the cell — so a Move is the
/// whole order.
pub(super) fn crates(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let tick = obs.tick();
    let cells: Vec<(u16, u16)> = obs.crate_cells(rules);
    if cells.is_empty() {
        return;
    }
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
    let mut detail: Vec<(u64, (u16, u16), MovementZone, i32)> = brain
        .census
        .units
        .iter()
        .filter(|unit| {
            (unit.traits.scout || unit.traits.combat)
                && !unit.hand
                && unit.orderable
                && !unit.in_service()
                && !holds_or_fights(unit.mission)
        })
        .filter_map(|unit| {
            let ty = obs.object_type(rules, unit.type_id);
            let zone = ty.map_or(MovementZone::Infantry, |ty| ty.movement_zone);
            let speed = ty.map_or(0, |ty| i32::from(ty.speed));
            Some((unit.entity_id, unit.cell, zone, speed))
        })
        .filter(|(_, _, _, speed)| *speed >= CRATE_RUNNER_SPEED)
        .collect();
    // Quickest first, so the detail is the same units every frame rather than
    // whichever the store yields first.
    detail.sort_by_key(|(id, _, _, speed)| (-*speed, *id));
    let idle: Vec<(u64, (u16, u16), MovementZone, bool, i32)> = detail
        .into_iter()
        .take(CRATE_RUNNERS)
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
    crates.sort_by_key(|cell| (squared_distance(anchor, *cell), *cell));
    let mut sent: BTreeSet<u64> = BTreeSet::new();
    for cell in crates {
        // The nearest idle unit that can walk there. The unit's own movement
        // zone: a tank and a dog reach different ground, and asking with the
        // infantry's zone for everybody sends vehicles at cells they cannot
        // walk to.
        //
        // One range, and it is the detour a unit takes on its way past. A
        // crate across the map is a walk, and the house that walks across
        // the map for it is a house that is not fighting: measured, sending a
        // fighter thirty cells for a box turned a win into a timeout.
        let reachable = |unit: &(u64, (u16, u16), MovementZone, bool, i32), range: i32| {
            squared_distance(unit.1, cell) <= range * range && obs.can_reach(unit.2, unit.1, cell)
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
        let picked = idle
            .iter()
            .filter(|(id, _, _, _, _)| !sent.contains(id))
            .filter(|unit| reachable(unit, CRATE_SEEK_CELLS))
            .min_by_key(|unit| (arrival(unit), unit.0))
            .copied();
        let Some((id, from, zone, _, _)) = picked else {
            continue;
        };
        sent.insert(id);
        if ordered_this_frame(out, id) {
            continue;
        }
        if !brain
            .tracked
            .admits(tick, id, TrackedKind::Move, None, Some(cell))
        {
            continue;
        }
        if issue(brain, out, rules, obs, id, Goal::Move { cell }) {
            brain
                .tracked
                .record(tick, id, TrackedKind::Move, None, Some(cell));
            log::info!(
                "auto play: unit {id} goes for the crate at {cell:?} taken={}",
                obs.ground_object_cells(rules).contains(&cell)
            );
        }
    }
}

/// How fast a unit has to be before it belongs to the crate detail, in the
/// rules' `Speed=`. Below this a unit is a fighter first and a courier never:
/// the walk is the whole cost of a crate, and a slow body on it is a body off
/// the line for nothing.
const CRATE_RUNNER_SPEED: i32 = 6;

/// How many units the crate detail holds at once.
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
pub(super) fn repairs(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let mut issued = 0;
    let mut whole: Vec<u64> = Vec::new();
    let mut hurt: Vec<u64> = Vec::new();
    for entity in obs.own_entities() {
        let Some(ty) = obs.object_type(rules, entity.type_id) else {
            continue;
        };
        if ty.category != ObjectCategory::Building {
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
            hurt.push(entity.entity_id);
        }
    }
    // A building is worth the wrench once it has stood whole: a structure still
    // going up is at a fraction of its strength by definition, and healing it
    // would spend the house's credits on the build it has already paid for.
    for entity_id in &whole {
        brain.repaired.insert(*entity_id);
        brain.repairing.remove(entity_id);
    }
    for entity_id in hurt {
        if issued >= FORCE_ORDERS_PER_FRAME {
            break;
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
        brain
            .repairing
            .insert(entity_id, obs.tick().saturating_add(REPAIR_REARM_FRAMES));
        out.command(Command::ToggleRepair { entity_id });
        issued += 1;
    }
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
    // The depot when the house has one, the base's anchor otherwise.
    let depot = brain.census.repair_building;
    // With no bay, the walk ends beside the base: the yard's own cell is its
    // foundation, and a Move to it is a walk that never arrives.
    let post = brain
        .census
        .repair_cell
        .or(brain.census.rally)
        .or(brain.census.base_cell);
    // Collected before anything is written: the routine takes the unit, and
    // the ledger is borrowed while it is read.
    let retreating: Vec<(u64, (u16, u16), MissionId)> = brain
        .census
        .units
        .iter()
        .filter(|unit| unit.traits.combat && !unit.hand && unit.orderable)
        .filter(|unit| unit.health_permille <= retreat_line(&unit.traits))
        // A hurt unit in a fight the house is winning stays in it. The
        // comparison is the push's own, with a stricter margin: the push may
        // wait for a better moment, this unit cannot, and a third of the
        // attackers walking home mid-exchange is what makes a push a series
        // of small fights that each one can be lost.
        .filter(|unit| {
            !super::squads::winning_fight(
                &brain.census,
                rules,
                obs,
                unit.cell,
                RETREAT_SUPERIORITY_PERMILLE,
            )
        })
        .map(|unit| (unit.entity_id, unit.cell, unit.mission))
        .collect();
    for (entity_id, cell, mission) in retreating {
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

/// Drive what a transport carries: the Transport pilot's dispatch.
///
/// Boarding is deliberately not here. It was measured before (won 25636 became
/// 34897) and needs its own comparison; what this pass does is the other half
/// — a carrier that already has passengers takes them where the army is going
/// and opens, instead of standing where it was built for the whole match.
pub(super) fn transports(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    let Some(deliver) = delivery_cell(brain) else {
        return;
    };
    let tick = obs.tick();
    let carriers: Vec<(u64, (u16, u16), i32)> =
        brain
            .census
            .units
            .iter()
            .filter(|unit| unit.orderable && !unit.hand && !unit.in_service())
            .filter_map(|unit| {
                let ty = obs.object_type(rules, unit.type_id)?;
                (super::pilots::Pilot::of(rules, ty) == super::pilots::Pilot::Transport)
                    .then_some((unit.entity_id, unit.cell, ty.passengers))
            })
            .collect();
    for (entity_id, cell, _) in carriers.iter().copied() {
        let step = carrying_step(
            obs.passengers(entity_id),
            super::threats::distance(cell, deliver),
        );
        let (kind, goal) = match step {
            Carrying::Empty => continue,
            Carrying::Advance => (TrackedKind::Move, Goal::Move { cell: deliver }),
            Carrying::Unload => (TrackedKind::Enter, Goal::Unload),
        };
        if !brain
            .tracked
            .admits(tick, entity_id, kind, None, Some(deliver))
        {
            continue;
        }
        brain
            .tracked
            .record(tick, entity_id, kind, None, Some(deliver));
        if !issue(brain, out, rules, obs, entity_id, goal) {
            brain.tracked.void(entity_id, kind);
        }
    }
    // An empty carrier asks one idle rifleman aboard; next frame it is loaded
    // and the loop above takes it to the front.
    board_idle(brain, rules, obs, out, &carriers, tick);
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
        if capacity <= 0 || obs.passengers(carrier) != 0 {
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
        if !brain.tracked.admits(
            tick,
            soldier_id,
            TrackedKind::Enter,
            Some(carrier),
            Some(cell),
        ) {
            continue;
        }
        brain.tracked.record(
            tick,
            soldier_id,
            TrackedKind::Enter,
            Some(carrier),
            Some(cell),
        );
        log::debug!("auto play: {soldier_id} boards {carrier} at {cell:?}");
        if issue(
            brain,
            out,
            rules,
            obs,
            soldier_id,
            Goal::Enter { target: carrier },
        ) {
            return true;
        }
        brain.tracked.void(soldier_id, TrackedKind::Enter);
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
/// Only powers aimed at an enemy cell are fired. The Iron Curtain, the Force
/// Shield and the Chrono pair are spent on the house's *own* units and
/// buildings, and firing them at the enemy burns the charge for nothing; they
/// belong to a defensive decision this brain does not make yet.
pub(super) fn supers(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    if !obs.super_weapons() {
        return;
    }
    let ready = obs.ready_super_weapons(rules);
    if ready.is_empty() {
        return;
    }
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
        if !spent_on_the_enemy(super_weapon.kind) {
            continue;
        }
        // Where it lands is the power's own answer, not one cell for all of
        // them: a paradrop is soldiers, and soldiers dropped into the enemy's
        // own base land inside everything that stands there.
        let Some(cell) = landing_cell(brain, rules, obs, aim_of(super_weapon.kind)) else {
            continue;
        };
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
}

/// What a power is for, by its kind: `Type=` in the rules.
fn aim_of(kind: SuperWeaponKind) -> Aim {
    match kind {
        SuperWeaponKind::ParaDrop
        | SuperWeaponKind::AmerParaDrop
        | SuperWeaponKind::UnitDelivery => Aim::Rally,
        SuperWeaponKind::SpyPlane | SuperWeaponKind::PsychicReveal => Aim::Recon,
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
        Aim::Enemy => enemy_structure_cell(brain, rules, obs),
        // Dropped where the army is, so they arrive as a squad rather than as
        // eight soldiers walking out of the enemy's own base one at a time.
        // With no army yet, the base: they are defenders until there is one.
        Aim::Rally => brain
            .census
            .units
            .iter()
            .find(|unit| unit.traits.combat)
            .map(|unit| unit.cell)
            .or(brain.census.base_cell),
        // What the house cannot see. The shroud has no projection yet, so this
        // is the enemy's base for now — the one place worth looking — and it
        // belongs to the exploration layer when that has a target source.
        Aim::Recon => enemy_structure_cell(brain, rules, obs),
    }
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
        | SuperWeaponKind::HunterSeeker => true,
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
