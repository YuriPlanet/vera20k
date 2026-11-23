//! *(VERA)* Chores every bot does alike, as a
//! player would: repairing buildings, sending idle miners back to ore,
//! collecting crates (near home with the army, far away with scouts) and
//! putting spare MCVs to use.

use serde::{Deserialize, Serialize};

use super::view::{BotView, cell, dist2, is_structure};
use crate::map::entities::EntityCategory;
use crate::sim::command::Command;
use crate::sim::intern::InternedId;
use crate::sim::production::{BuildOption, ProductionCategory};

/// Turn repair on for this House's damaged buildings, as a player clicks
/// the wrench: a Construction Yard at any damage once 100 credits are left,
/// the rest below 70% from 300 credits, every damaged one from 2000. The
/// repair step stops by itself when the building is whole or the money runs
/// out (`production_repair`), so the order is sent again whenever a damaged
/// building is not repairing: remembering the buildings once sent left a
/// building damaged a second time unrepaired for good.
pub(crate) fn repair_buildings(view: &BotView<'_>, orders: &mut Vec<Command>) {
    let credits = view.credits();
    if credits < 100 {
        return;
    }
    for entity in view.own().filter(|e| is_structure(e)) {
        let id = entity.stable_id();
        let Some(ty) = view.object_type(entity.type_ref()) else {
            continue;
        };
        if view.repair_state(id) != Some((true, false)) {
            continue;
        }
        let low = entity.health.current() * 10 < ty.strength * 7;
        if ty.construction_yard || credits >= 2000 || (low && credits >= 300) {
            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "repair f{} {} health {}/{}",
                    view.frame(),
                    view.name(entity.type_ref()),
                    entity.health.current(),
                    ty.strength
                );
            }
            orders.push(Command::ToggleRepair { entity_id: id });
        }
    }
}

/// Send idle ore miners back to work, as a player clicks them onto ore. A
/// miner whose ore search finds nothing within `TiberiumLongScan=` parks on
/// Guard (`Mission_Harvest @ 0x0073E5E0` state 4); only a computer House's
/// Guard re-queues Harvest (`0x00740922`), so a bot's House, which stays
/// human, leaves it there for good. Every 3 seconds each parked one (Guard,
/// Area Guard, Sleep or nothing, standing still) is ordered to harvest the
/// nearest explored ore it can drive to, close to a refinery and away from
/// seen enemy buildings and armed units; miners sent at once spread over
/// different patches. Slave Miners (which deploy) are left to their bot.
/// `think_every`: the bot's thinking period, so one think in 90 frames tends.
pub(crate) fn tend_miners(
    view: &BotView<'_>,
    think_every: u32,
    health: &mut Vec<(u64, i32)>,
    orders: &mut Vec<Command>,
) {
    use crate::sim::mission::MissionType as M;
    chrono_miners_home(view, health, orders);
    if view.frame() % 90 >= think_every {
        return;
    }
    let mut idle = Vec::new();
    let mut heading = Vec::new();
    let mut refineries = Vec::new();
    for e in view.own() {
        let Some(t) = view.object_type(e.type_ref()) else {
            continue;
        };
        if is_structure(e) {
            if t.refinery {
                refineries.push(cell(e));
            }
            continue;
        }
        let parked = matches!(
            e.mission.effective().known(),
            None | Some(M::Guard | M::AreaGuard | M::Sleep)
        );
        if t.harvester && t.deploys_into.is_none() && parked && e.navigation.nav_com().is_none() {
            idle.push((e.stable_id(), cell(e), t.movement_zone));
        } else if t.harvester
            && t.deploys_into.is_none()
            && e.mission.effective().known() == Some(M::Harvest)
            && let Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) =
                e.navigation.nav_com()
        {
            // Its own ore search may pick ore in or behind an enemy base.
            heading.push((e.stable_id(), cell(e), t.movement_zone, (rx, ry)));
        }
    }
    let reach = Danger::seen(view, 2);
    for (id, at, mz, to) in heading {
        if view.has_ore(to.0, to.1) && (reach.at(to) || reach.on_way(at, to)) {
            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "miner f{} {id} at {at:?} heads into enemy reach at {to:?}",
                    view.frame()
                );
            }
            idle.push((id, at, mz));
        }
    }
    if idle.is_empty() {
        return;
    }
    let danger: Vec<(u16, u16)> = view
        .enemy_objects()
        .filter(|e| {
            is_structure(e)
                || view
                    .object_type(e.type_ref())
                    .is_some_and(|t| t.weapon_list.iter().any(Option::is_some))
        })
        .map(cell)
        .collect();
    let (w, h) = view.map_size();
    let mut ore = Vec::new();
    for y in (1..h).step_by(2) {
        for x in (1..w).step_by(2) {
            if view.has_ore(x, y)
                && danger.iter().all(|&d| dist2(d, (x, y)) > 12 * 12)
                && !reach.at((x, y))
            {
                ore.push((x, y));
            }
        }
    }
    let mut sent: Vec<(u16, u16)> = Vec::new();
    for (id, at, mz) in idle {
        let mut ranked: Vec<(i32, (u16, u16))> = ore
            .iter()
            .copied()
            .map(|c| {
                let home = refineries
                    .iter()
                    .map(|&r| dist2(r, c))
                    .min()
                    .unwrap_or(0)
                    .isqrt();
                let crowd = sent.iter().filter(|&&g| dist2(g, c) <= 5 * 5).count() as i32;
                (home * 2 + dist2(at, c).isqrt() + crowd * 10, c)
            })
            .collect();
        ranked.sort_unstable();
        // The best few the miner can drive to, the first whose way stays
        // clear of seen enemies' reach.
        let best = ranked
            .into_iter()
            .filter(|&(_, c)| view.can_reach(mz, at, c))
            .take(12)
            .find(|&(_, c)| !reach.on_way(at, c));
        if let Some((_, c)) = best {
            if log::log_enabled!(log::Level::Debug) {
                log::debug!("miner f{} {id} idle at {at:?} -> ore {c:?}", view.frame());
            }
            sent.push(c);
            orders.push(Command::clicked_mission(
                id,
                M::Harvest,
                crate::sim::combat::TargetKind::Cell(c.0, c.1),
            ));
        }
    }
}

/// Playtest, 2026-11-14: "teleport harvesters home as soon as they take
/// damage". Every think, a Teleporter miner (the Chrono Miner) whose health
/// dropped since the last think and that is more than 8 cells from its
/// nearest refinery is ordered into that refinery (`Enter`, as a player's
/// click on it), which a Teleporter warps to when it is that far
/// (`handle_return`, `ChronoHarvTooFarDistance`). `health`: each such
/// miner's health at the last think, saved with the bot.
fn chrono_miners_home(view: &BotView<'_>, health: &mut Vec<(u64, i32)>, orders: &mut Vec<Command>) {
    use crate::sim::mission::MissionType as M;
    let refineries: Vec<(u64, (u16, u16))> = view
        .own()
        .filter(|e| is_structure(e) && view.object_type(e.type_ref()).is_some_and(|t| t.refinery))
        .map(|e| (e.stable_id(), cell(e)))
        .collect();
    let mut now = Vec::new();
    for e in view.own() {
        if is_structure(e) || e.in_limbo() {
            continue;
        }
        let Some(t) = view.object_type(e.type_ref()) else {
            continue;
        };
        if !(t.harvester && t.teleporter) {
            continue;
        }
        let id = e.stable_id();
        let hp = e.health.current();
        now.push((id, hp));
        let hit = health.iter().any(|&(h, before)| h == id && hp < before);
        if !hit {
            continue;
        }
        let at = cell(e);
        let Some(&(refinery, r)) = refineries
            .iter()
            .min_by_key(|(rid, r)| (dist2(*r, at), *rid))
        else {
            continue;
        };
        if dist2(r, at) <= 8 * 8 || e.mission.effective().known() == Some(M::Enter) {
            continue;
        }
        if log::log_enabled!(log::Level::Debug) {
            log::debug!("miner f{} {id} hit at {at:?} -> home", view.frame());
        }
        orders.push(Command::clicked_mission(
            id,
            M::Enter,
            crate::sim::combat::TargetKind::Entity(refinery),
        ));
    }
    *health = now;
}

/// Units gathered around a crate before one steps on it: at most this many.
const CRATE_ESCORT: usize = 8;
/// A crate this far from home (cells) is gathered at by the army; farther
/// ones (anywhere on the map) are fetched by one runner.
const CRATE_RANGE: i32 = 40;
/// Units this far from the crate (cells) are not called to it.
const CRATE_CALL: i32 = 30;
/// Cells kept between a crate run and a seen enemy's reach.
const CRATE_MARGIN: i32 = 3;

/// Cells a garrisoned building reaches (GI-class occupants with the
/// building's range bonus).
const GARRISON_REACH: i32 = 8;

/// Where seen enemies can hit a ground unit: each armed enemy ground unit or
/// building with its longest ground-hitting reach (projectile `AG=`) plus a
/// margin, in cells. Built fresh each look from [`BotView`] only.
pub(crate) struct Danger {
    zones: Vec<((u16, u16), i32)>,
}

impl Danger {
    pub(crate) fn seen(view: &BotView<'_>, margin: i32) -> Self {
        Self::seen_but(view, margin, |_| false)
    }

    /// [`Self::seen`] without the enemies `skip` names (a raid's target).
    pub(crate) fn seen_but(
        view: &BotView<'_>,
        margin: i32,
        skip: impl Fn(&crate::sim::game_entity::GameEntity) -> bool,
    ) -> Self {
        let zones = view
            .enemy_objects()
            .filter(|e| e.category() != EntityCategory::Aircraft && !skip(e))
            .filter_map(|e| {
                let t = view.object_type(e.type_ref())?;
                if t.considered_aircraft {
                    return None;
                }
                // A garrisoned building fires its occupants' weapons; its
                // own type has none (a dog ran into one). Their reach with
                // the garrison bonus: GARRISON_REACH.
                if is_structure(e) && !e.passenger_role().occupants().is_empty() {
                    return Some((cell(e), GARRISON_REACH + margin));
                }
                let reach = t
                    .weapon_list
                    .iter()
                    .flatten()
                    .filter_map(|w| view.rules.weapon(w))
                    .filter(|w| {
                        w.projectile
                            .as_deref()
                            .and_then(|p| view.rules.projectile(p))
                            .is_none_or(|p| p.ag)
                    })
                    .map(|w| (w.range_leptons + 255) / 256)
                    .max()?;
                Some((cell(e), reach + margin))
            })
            .collect();
        Self { zones }
    }

    /// Whether cell `c` is within a seen enemy's reach.
    pub(crate) fn at(&self, c: (u16, u16)) -> bool {
        self.zones.iter().any(|&(z, r)| dist2(z, c) <= r * r)
    }

    /// Whether the straight way from `from` to `to` (every 2nd cell) passes
    /// within a seen enemy's reach. The path finder's way may bend; the
    /// straight line is what a player judges at a glance.
    pub(crate) fn on_way(&self, from: (u16, u16), to: (u16, u16)) -> bool {
        let dx = i32::from(to.0) - i32::from(from.0);
        let dy = i32::from(to.1) - i32::from(from.1);
        let steps = (dx.abs().max(dy.abs()) / 2).max(1);
        (0..=steps).any(|i| {
            let x = i32::from(from.0) + dx * i / steps;
            let y = i32::from(from.1) + dy * i / steps;
            self.at((x.max(0) as u16, y.max(0) as u16))
        })
    }
}

/// Seen enemy Terror Drones (or any vehicle whose weapon 0 carries a
/// Parasite warhead, `TechnoClass::Init_Managers @ 0x006F40FC`) this close
/// to an own or allied vehicle or building are shot down.
const DRONE_ALARM: i32 = 12;
/// Units this far from a drone are not sent after it.
const DRONE_CALL: i32 = 30;

/// Terror Drones, as a player handles them: an own vehicle with a drone
/// inside (`FootParasite::is_infested`) drives into the nearest own Service
/// Depot, whose repair forces the parasite out (`parasite.rs`, repair
/// `Receive_Radio 0x1C`); drones seen near own or allied vehicles and
/// buildings are shot by the two nearest of the bot's armed ground `units`
/// (id, cell) each. Returns the units it ordered, which the bot leaves alone
/// this think.
pub(crate) fn fight_drones(
    view: &BotView<'_>,
    think_every: u32,
    units: &[(u64, (u16, u16))],
    orders: &mut Vec<Command>,
) -> Vec<u64> {
    let frame = view.frame();
    let mut busy = Vec::new();
    let depots: Vec<(u64, (u16, u16))> = view
        .own()
        .filter(|e| {
            is_structure(e)
                && view
                    .object_type(e.type_ref())
                    .is_some_and(|t| t.unit_repair && !t.naval)
        })
        .map(|e| (e.stable_id(), cell(e)))
        .collect();
    // Own vehicles a drone eats before a depot could save them: the nearest
    // shooters finish them (playtest, 2026-11-14), which frees the drone sooner.
    let mut doomed: Vec<(u64, (u16, u16))> = Vec::new();
    for e in view.own() {
        if is_structure(e) || !e.parasite.is_infested() {
            continue;
        }
        let at = cell(e);
        let depot = depots
            .iter()
            .min_by_key(|(id, d)| (dist2(*d, at), *id))
            .copied();
        let saved = depot
            .is_some_and(|(_, d)| drone_frames_left(view, e) > depot_frames(view, e, dist2(d, at)));
        if !saved {
            busy.push(e.stable_id());
            doomed.push((e.stable_id(), at));
            continue;
        }
        let Some((depot, _)) = depot else {
            continue;
        };
        busy.push(e.stable_id());
        let going = e.navigation.nav_com()
            == Some(crate::sim::components::NavTargetRef::Object { id: depot });
        if !going {
            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "drone f{frame} {} infested at {at:?} -> depot",
                    e.stable_id()
                );
            }
            orders.push(Command::MoveToObject {
                entity_id: e.stable_id(),
                target_id: depot,
            });
        }
    }
    let mut drones: Vec<(u64, (u16, u16))> = view
        .enemy_objects()
        .filter(|e| e.category() == EntityCategory::Unit)
        .filter(|e| {
            view.object_type(e.type_ref()).is_some_and(|t| {
                crate::sim::combat::parasite::type_allocates_parasite(
                    view.rules,
                    t,
                    e.category(),
                    0,
                )
            })
        })
        .map(|e| (e.stable_id(), cell(e)))
        .collect();
    // A doomed vehicle is shot like a drone next to it.
    drones.extend(doomed.iter().copied());
    if drones.is_empty() {
        return busy;
    }
    let guarded: Vec<(u16, u16)> = view
        .own()
        .chain(view.allied_objects())
        .filter(|e| is_structure(e) || e.category() == EntityCategory::Unit)
        .map(cell)
        .collect();
    let mut shooters: Vec<(u64, (u16, u16))> = units
        .iter()
        .copied()
        .filter(|&(id, _)| !busy.contains(&id))
        .filter(|&(id, _)| {
            view.entity(id).is_some_and(|e| {
                e.category() != EntityCategory::Aircraft
                    && view.object_type(e.type_ref()).is_some_and(|t| {
                        !t.considered_aircraft
                            && !t.harvester
                            && t.weapon_list.iter().any(Option::is_some)
                            && !crate::sim::combat::parasite::type_allocates_parasite(
                                view.rules,
                                t,
                                e.category(),
                                0,
                            )
                    })
            })
        })
        .collect();
    for (drone, at) in drones {
        let own_victim = doomed.iter().any(|&(id, _)| id == drone);
        if !own_victim
            && !guarded
                .iter()
                .any(|&g| dist2(g, at) <= DRONE_ALARM * DRONE_ALARM)
        {
            continue;
        }
        shooters.sort_by_key(|&(id, c)| (dist2(c, at), id));
        let picked: Vec<(u64, (u16, u16))> = shooters
            .iter()
            .copied()
            .filter(|&(_, c)| dist2(c, at) <= DRONE_CALL * DRONE_CALL)
            .take(2)
            .collect();
        for (id, _) in picked {
            shooters.retain(|s| s.0 != id);
            busy.push(id);
            // At once when idle, else again every 45 frames: a drone is
            // fast, an order goes stale.
            let idle = view
                .entity(id)
                .is_some_and(|e| e.navigation.nav_com().is_none());
            if idle || frame % 45 < think_every {
                if log::log_enabled!(log::Level::Debug) {
                    log::debug!("drone f{frame} {id} shoots drone {drone} at {at:?}");
                }
                orders.push(Command::Attack {
                    attacker_id: id,
                    target_id: drone,
                });
            }
        }
    }
    busy
}

/// Frames until the drone inside `e` has eaten it: its strength over the
/// eater's primary weapon (DroneJump, Damage=50 every ROF=60 in retail),
/// armour ignored.
fn drone_frames_left(view: &BotView<'_>, e: &crate::sim::game_entity::GameEntity) -> i64 {
    let (damage, rof) = e
        .parasite
        .eater()
        .and_then(|id| view.entity(id))
        .and_then(|d| view.object_type(d.type_ref()))
        .and_then(|t| t.weapon_list.first().cloned().flatten())
        .and_then(|w| view.rules.weapon(&w).map(|w| (w.damage, w.rof)))
        .unwrap_or((50, 60));
    i64::from(e.health.current().max(0)) * i64::from(rof.max(1)) / i64::from(damage.max(1))
}

/// Frames for `e` to reach a depot `d2` (squared cells) away and have the
/// drone pushed out: the straight line times 1.5 for the path at its speed,
/// plus 3 seconds to dock.
fn depot_frames(view: &BotView<'_>, e: &crate::sim::game_entity::GameEntity, d2: i32) -> i64 {
    let speed = view.object_type(e.type_ref()).map_or(0, |t| {
        crate::util::fixed_math::ra2_speed_to_leptons_per_frame(t.speed)
    });
    if speed <= 0 {
        return i64::MAX / 4;
    }
    let cells = i64::from(d2.max(0).isqrt() + 1);
    cells * 256 * 3 / 2 / i64::from(speed) + 3 * 15
}

/// 16 directions on a circle of radius 100 (integer, deterministic).
const DIRS: [(i32, i32); 16] = [
    (100, 0),
    (92, 38),
    (71, 71),
    (38, 92),
    (0, 100),
    (-38, 92),
    (-71, 71),
    (-92, 38),
    (-100, 0),
    (-92, -38),
    (-71, -71),
    (-38, -92),
    (0, -100),
    (38, -92),
    (71, -71),
    (92, -38),
];

/// A bot's plain moves around seen enemies: a ground unit outside every seen
/// enemy's reach ordered to a cell outside it (going home, gathering, a
/// crate, a scout's goal) whose straight way crosses it drives to a waypoint
/// first from which both legs stay clear; with none, the order is dropped
/// and the unit stays where it is safe. Orders into reach (an attack) and
/// units already in a fight are left alone. A player does the same with a
/// waypoint click; the bot gives the next leg once the unit has arrived
/// (every bot re-orders an idle unit that is not where it wants it).
pub(crate) fn detour(view: &BotView<'_>, orders: &mut Vec<Command>) {
    let mut danger: Option<Danger> = None;
    let (w, h) = view.map_size();
    orders.retain_mut(|order| {
        let (id, to) = match order {
            Command::Move {
                entity_id,
                target_rx,
                target_ry,
            } => (*entity_id, (*target_rx, *target_ry)),
            Command::AttackMove {
                entity_id,
                target: None,
                destination: Some(crate::sim::combat::TargetKind::Cell(x, y)),
            } => (*entity_id, (*x, *y)),
            _ => return true,
        };
        let Some(unit) = view.entity(id).filter(|e| e.owner() == view.house) else {
            return true;
        };
        let Some(ty) = view.object_type(unit.type_ref()) else {
            return true;
        };
        if unit.category() == EntityCategory::Aircraft
            || ty.considered_aircraft
            || ty.jumpjet
            || is_structure(unit)
        {
            return true;
        }
        let from = cell(unit);
        if dist2(from, to) <= 4 * 4 {
            return true;
        }
        let danger = danger.get_or_insert_with(|| Danger::seen(view, 2));
        if danger.at(from) || danger.at(to) || !danger.on_way(from, to) {
            return true;
        }
        let mz = ty.movement_zone;
        let mut best: Option<(i32, (u16, u16))> = None;
        for r in [6, 10, 15, 22, 30] {
            for (dx, dy) in DIRS {
                let x = i32::from(from.0) + dx * r / 100;
                let y = i32::from(from.1) + dy * r / 100;
                if x < 1 || y < 1 || x >= i32::from(w) || y >= i32::from(h) {
                    continue;
                }
                let c = (x as u16, y as u16);
                let len = dist2(from, c).isqrt() + dist2(c, to).isqrt();
                if best.is_some_and(|(b, _)| len >= b)
                    || danger.on_way(from, c)
                    || danger.on_way(c, to)
                    || !view.can_reach(mz, from, c)
                {
                    continue;
                }
                best = Some((len, c));
            }
            if best.is_some() {
                break;
            }
        }
        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "detour f{} {} {from:?}->{to:?} via {:?}",
                view.frame(),
                view.name(unit.type_ref()),
                best.map(|b| b.1)
            );
        }
        match best {
            Some((_, c)) => {
                *order = Command::Move {
                    entity_id: id,
                    target_rx: c.0,
                    target_ry: c.1,
                };
                true
            }
            None => false,
        }
    });
}

/// Cells beyond a unit's longest ground reach within which an armed enemy
/// ground unit is attacked before anything else (`engage`).
const ENGAGE_MARGIN: i32 = 3;

/// The bots' orders as a player gives them to an army (playtest, 2026-11-12:
/// "use attack-move commands so units shoot back and in general don't go
/// past enemies - attack them - closest enemy units first"): an armed
/// ground unit's plain move becomes an attack-move (mission 29, the units
/// fight what they meet on the way), and one with a seen armed enemy ground
/// unit within its reach plus [`ENGAGE_MARGIN`] attacks the nearest such
/// unit instead of moving on or shooting a building. `errands` (scouts,
/// spies, repair trips ..) and unarmed units keep their orders.
pub(crate) fn engage(view: &BotView<'_>, errands: &[u64], orders: &mut [Command]) {
    use crate::sim::combat::TargetKind;
    let mut enemies: Option<Vec<((u16, u16), u64)>> = None;
    let mut buildings: Option<Vec<((u16, u16), u64, bool)>> = None;
    // Arena knob for baseline rounds; unset in every real match.
    let shoot_buildings = std::env::var("VG_ENGAGE_BUILDINGS").map_or(true, |v| v != "0");
    for order in orders.iter_mut() {
        let (id, destination, structure_target) = match order {
            Command::Move {
                entity_id,
                target_rx,
                target_ry,
            } => (*entity_id, Some((*target_rx, *target_ry)), false),
            Command::AttackMove {
                entity_id,
                target,
                destination,
            } => {
                let cell = match destination {
                    Some(TargetKind::Cell(x, y)) => Some((*x, *y)),
                    _ => None,
                };
                let on_building = match target {
                    Some(TargetKind::Entity(t)) => view.entity(*t).is_some_and(is_structure),
                    None => true,
                    _ => false,
                };
                (*entity_id, cell, on_building)
            }
            Command::Attack {
                attacker_id,
                target_id,
            } => (
                *attacker_id,
                None,
                view.entity(*target_id).is_some_and(is_structure),
            ),
            _ => continue,
        };
        if errands.contains(&id) {
            continue;
        }
        let Some(unit) = view.entity(id).filter(|e| e.owner() == view.house) else {
            continue;
        };
        if is_structure(unit)
            || unit.category() == EntityCategory::Aircraft
            || unit.seldom.mind_control.is_mind_controlled()
        {
            continue;
        }
        let Some(ty) = view.object_type(unit.type_ref()) else {
            continue;
        };
        if ty.considered_aircraft
            || ty.engineer
            || ty.harvester
            || ty.resource_gatherer
            || ty.deploys_into.is_some()
            || ty.infiltrate
        {
            continue;
        }
        // The longest ground-hitting reach, in cells.
        let Some(reach) = ty
            .weapon_list
            .iter()
            .flatten()
            .filter_map(|w| view.rules.weapon(w))
            .filter(|w| {
                w.projectile
                    .as_deref()
                    .and_then(|p| view.rules.projectile(p))
                    .is_none_or(|p| p.ag)
            })
            .map(|w| (w.range_leptons + 255) / 256)
            .max()
        else {
            continue;
        };
        let at = cell(unit);
        if structure_target || destination.is_some() {
            let enemies = enemies.get_or_insert_with(|| {
                view.enemy_objects()
                    .filter(|e| {
                        !is_structure(e)
                            && e.category() != EntityCategory::Aircraft
                            && view.object_type(e.type_ref()).is_some_and(|t| {
                                !t.considered_aircraft && t.weapon_list.iter().any(Option::is_some)
                            })
                    })
                    .map(|e| (cell(e), e.stable_id()))
                    .collect()
            });
            let near = (reach + ENGAGE_MARGIN).min(14);
            if let Some(&(_, enemy)) = enemies
                .iter()
                .filter(|(c, _)| dist2(*c, at) <= near * near)
                .min_by_key(|(c, e)| (dist2(*c, at), *e))
            {
                *order = Command::Attack {
                    attacker_id: id,
                    target_id: enemy,
                };
                continue;
            }
        }
        // Playtest, 2026-11-14: "the units go too far into the enemy base
        // without killing stuff. don't move further when enemies are still
        // in range". Attack-move passes buildings that do not shoot (the
        // threat scan takes armed objects only), so a move that starts with
        // an enemy building in the unit's own reach shoots it instead:
        // armed buildings and garrisons first, then the nearest.
        if shoot_buildings && destination.is_some() {
            let buildings = buildings.get_or_insert_with(|| {
                view.enemy_objects()
                    .filter(|e| is_structure(e) && !e.in_limbo())
                    .filter_map(|e| {
                        let t = view.object_type(e.type_ref())?;
                        if t.wall {
                            return None;
                        }
                        let armed = t.weapon_list.iter().any(Option::is_some)
                            || !e.passenger_role().occupants().is_empty();
                        Some((cell(e), e.stable_id(), armed))
                    })
                    .collect()
            });
            if let Some(&(_, enemy, _)) = buildings
                .iter()
                .filter(|(c, _, _)| dist2(*c, at) <= reach * reach)
                .min_by_key(|(c, e, armed)| (!*armed, dist2(*c, at), *e))
            {
                *order = Command::Attack {
                    attacker_id: id,
                    target_id: enemy,
                };
                continue;
            }
        }
        if let (Command::Move { .. }, Some((x, y))) = (&*order, destination) {
            *order = Command::AttackMove {
                entity_id: id,
                target: None,
                destination: Some(TargetKind::Cell(x, y)),
            };
        }
    }
}

/// Whether a seen enemy ground object (no wall) stands within own unit
/// `unit`'s longest ground-hitting reach.
pub(crate) fn enemy_in_reach(
    view: &BotView<'_>,
    unit: &crate::sim::game_entity::GameEntity,
) -> bool {
    let Some(reach) = view.object_type(unit.type_ref()).and_then(|ty| {
        ty.weapon_list
            .iter()
            .flatten()
            .filter_map(|w| view.rules.weapon(w))
            .filter(|w| {
                w.projectile
                    .as_deref()
                    .and_then(|p| view.rules.projectile(p))
                    .is_none_or(|p| p.ag)
            })
            .map(|w| (w.range_leptons + 255) / 256)
            .max()
    }) else {
        return false;
    };
    let at = cell(unit);
    view.enemy_objects().any(|e| {
        !e.in_limbo()
            && e.category() != EntityCategory::Aircraft
            && dist2(cell(e), at) <= reach * reach
            && view
                .object_type(e.type_ref())
                .is_some_and(|t| !t.wall && !t.considered_aircraft)
    })
}

/// Weapons at least this long (cells) keep their range against buildings
/// ([`RangeKeep`]); shorter ones walk up anyway.
const RANGE_KEEP_MIN: i32 = 5;
/// Frames a range walk may take before the attack goes in as ordered.
const RANGE_KEEP_FRAMES: u32 = 45 * 30;

/// Attacks on buildings as a player gives them to long-range units (playtest,
/// 2026-11-22: "Magnetrons: told to attack a building they drive closer to
/// it than their range needs. Better drive them just so close that they
/// reach it at their longest range, then attack. The same for Prism
/// Tanks"). An attack order from Approach (`Foot4D5690`, the engine's)
/// walks the unit to a ring point at range minus half a cell or nearer; so
/// a bot's attack on a building by a ground unit out of reach becomes a
/// move to the cell farthest from the building on the way to it from which
/// the unit's own weapon reaches it ([`BotView::in_range_from`]), and the
/// attack is given there. Every bot's orders pass through it, saved with
/// the bot's slot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RangeKeep {
    /// Units on their way to a stand cell: (unit, building, since frame).
    walks: Vec<(u64, u64, u32)>,
}

impl RangeKeep {
    /// After the bot's orders are final: an attack on a building out of
    /// reach becomes the walk to its stand cell; any other order for a
    /// walking unit ends its walk.
    pub(crate) fn rewrite(&mut self, view: &BotView<'_>, orders: &mut Vec<Command>) {
        let frame = view.frame();
        orders.retain_mut(|order| {
            let id = match order {
                Command::Attack { attacker_id, .. } => *attacker_id,
                Command::Move { entity_id, .. }
                | Command::AttackMove { entity_id, .. }
                | Command::Stop { entity_id }
                | Command::Guard { entity_id, .. } => {
                    let id = *entity_id;
                    self.walks.retain(|w| w.0 != id);
                    return true;
                }
                _ => return true,
            };
            let Command::Attack { target_id, .. } = *order else {
                return true;
            };
            let walking = self.walks.iter().any(|w| w.0 == id && w.1 == target_id);
            self.walks.retain(|w| w.0 != id);
            let (Some(unit), Some(target)) = (view.entity(id), view.entity(target_id)) else {
                return true;
            };
            let Some(stand) = stand_cell(view, unit, target) else {
                return true;
            };
            if walking && unit.navigation.nav_com().is_some() {
                // Still on the way there: the walk goes on.
                self.walks.push((id, target_id, frame));
                return false;
            }
            self.walks.push((id, target_id, frame));
            *order = Command::Move {
                entity_id: id,
                target_rx: stand.0,
                target_ry: stand.1,
            };
            true
        });
    }

    /// Every frame: a unit that reached its weapon's reach of the building
    /// (or stopped, or took too long) gets its attack.
    pub(crate) fn run(&mut self, view: &BotView<'_>, orders: &mut Vec<Command>) {
        let frame = view.frame();
        self.walks.retain(|&(id, target_id, since)| {
            let (Some(unit), Some(target)) = (view.entity(id), view.entity(target_id)) else {
                return false;
            };
            let arrived = view.in_range_from(unit, target, cell(unit));
            let stopped = unit.navigation.nav_com().is_none() && frame.saturating_sub(since) > 15;
            if arrived || stopped || frame.saturating_sub(since) > RANGE_KEEP_FRAMES {
                if log::log_enabled!(log::Level::Debug) {
                    log::debug!(
                        "range-keep f{frame} {} -> {} at {} cells, arrived={arrived}",
                        view.name(unit.type_ref()),
                        view.name(target.type_ref()),
                        dist2(cell(unit), cell(target)).isqrt()
                    );
                }
                orders.push(Command::Attack {
                    attacker_id: id,
                    target_id,
                });
                return false;
            }
            true
        });
    }
}

/// Where own ground unit `unit` stands to hit the seen enemy building
/// `target` at its longest reach: on the line from the building's middle
/// toward the unit, the farthest cell it can drive to from which its weapon
/// reaches. `None` when the unit already reaches it, is no long-range ground
/// unit, or no such cell is found.
fn stand_cell(
    view: &BotView<'_>,
    unit: &crate::sim::game_entity::GameEntity,
    target: &crate::sim::game_entity::GameEntity,
) -> Option<(u16, u16)> {
    if unit.owner() != view.house
        || !is_structure(target)
        || !view.is_enemy(target.owner())
        || is_structure(unit)
        || unit.category() == EntityCategory::Aircraft
        || unit.is_deployed()
        || unit.seldom.mind_control.is_mind_controlled()
    {
        return None;
    }
    let ty = view.object_type(unit.type_ref())?;
    let goal = view.object_type(target.type_ref())?;
    if goal.wall
        || ty.considered_aircraft
        || ty.jumpjet
        || ty.is_simple_deployer
        || ty.engineer
        || ty.infiltrate
    {
        return None;
    }
    let reach = ty
        .weapon_list
        .iter()
        .flatten()
        .filter_map(|w| view.rules.weapon(w))
        .map(|w| w.range_leptons / 256)
        .max()?;
    if reach < RANGE_KEEP_MIN {
        return None;
    }
    let from = cell(unit);
    if view.in_range_from(unit, target, from) {
        return None;
    }
    let (w, h) = goal.foundation.dimensions();
    let at = cell(target);
    let middle = (at.0 + u16::from(w) / 2, at.1 + u16::from(h) / 2);
    let (dx, dy) = (
        i32::from(from.0) - i32::from(middle.0),
        i32::from(from.1) - i32::from(middle.1),
    );
    let d = (dx * dx + dy * dy).isqrt();
    if d == 0 {
        return None;
    }
    let mz = ty.movement_zone;
    // From one cell beyond the weapon's reach plus the building's size
    // inward: the first cell that reaches is the farthest.
    let start = (reach + i32::from(w.max(h)) / 2 + 1).min(d);
    (1..=start).rev().find_map(|r| {
        let x = i32::from(middle.0) + dx * r / d;
        let y = i32::from(middle.1) + dy * r / d;
        if x < 1 || y < 1 {
            return None;
        }
        let c = (x as u16, y as u16);
        (view.in_playfield(c.0, c.1)
            && view.can_reach(mz, from, c)
            && view.in_range_from(unit, target, c))
        .then_some(c)
    })
}

/// The roaming Rocketeers, saved with the bot (playtest, 2026-11-14: "make our
/// bots constantly roam the map with 10-20 rocketeers (fewer early on) and
/// make them kill everything that doesn't have anti-air in the vicinity
/// (especially harvesters and incoming attack forces)").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RocketeerSquad {
    ids: Vec<u64>,
    /// The roaming point while nothing is to be hit, and since when.
    roam: Option<(u16, u16)>,
    roam_since: u32,
    /// Roaming points visited, for the next one.
    hops: u32,
}

/// What the squad is made of: the Allied Rocketeer; for the other sides
/// (playtest, 2026-11-14: "should be disks or siege choppers for the other
/// races (fewer though because they are more expensive and better)") the
/// Soviet Siege Chopper and Yuri's Floating Disc.
const ROCKETEER_TYPES: [&str; 3] = ["JUMPJET", "SCHP", "DISK"];
/// The Rocketeer's cost, against which the dearer types come fewer.
const ROCKETEER_COST: i32 = 600;
/// Anti-air reach margin the squad keeps, in cells.
const ROCKETEER_AA_MARGIN: i32 = 3;

impl RocketeerSquad {
    pub(crate) fn ids(&self) -> &[u64] {
        &self.ids
    }

    /// How many the squad should have: none before minute 4, then four,
    /// one more a minute, 10 by minute 10 and 20 from minute 20.
    fn wanted(view: &BotView<'_>) -> usize {
        let minutes = view.frame() / MINUTE;
        if minutes < 4 {
            0
        } else {
            (minutes as usize).min(20)
        }
    }

    /// With the barracks idle and money to spare: a missing Rocketeer
    /// (`keep` more stay with the bot's own jobs). `true` when queued.
    pub(crate) fn produce(
        &self,
        view: &BotView<'_>,
        options: &[BuildOption],
        keep: usize,
        orders: &mut Vec<Command>,
    ) -> bool {
        let Some(t) = options.iter().find(|o| {
            o.enabled
                && matches!(
                    o.queue_category,
                    ProductionCategory::Infantry | ProductionCategory::Vehicle
                )
                && ROCKETEER_TYPES.contains(&view.name(o.type_id))
        }) else {
            return false;
        };
        let factory = view.factory(t.queue_category);
        let queued = factory.as_ref().map_or(0, |f| {
            f.queue.iter().filter(|q| q.type_id == t.type_id).count()
                + usize::from(f.object.is_some_and(|o| o.type_id == t.type_id))
        });
        let busy = factory.map_or(0, |f| f.queue.len() + usize::from(f.object.is_some()));
        // The war factory never idles: a Chopper or Disc joins its queue
        // when none is in it.
        let busy = if t.queue_category == ProductionCategory::Vehicle {
            if queued > 0 || busy >= 4 { 1 } else { 0 }
        } else {
            busy
        };
        let have = view
            .own()
            .filter(|e| ROCKETEER_TYPES.contains(&view.name(e.type_ref())))
            .count()
            + queued;
        // Dearer types come fewer, at least one once the squad is wanted.
        let wanted = Self::wanted(view);
        let wanted = if t.queue_category == ProductionCategory::Infantry {
            keep + wanted
        } else if wanted == 0 {
            0
        } else {
            (wanted * ROCKETEER_COST as usize / t.cost.max(ROCKETEER_COST) as usize).max(1)
        };
        if busy > 0 || view.credits() < 1200.max(t.cost * 2) || have >= wanted {
            return false;
        }
        orders.push(Command::QueueProduction { type_id: t.type_id });
        true
    }

    /// Every 30 frames: the squad (own Rocketeers beyond the first `keep`,
    /// which stay with the bot) flies at the best seen target that no seen
    /// anti-air covers: miners first, then armed units within 30 cells of
    /// the own buildings (an attack coming in), then other units, then
    /// buildings; nearest the squad first. Nothing to hit: it roams between
    /// the start locations and the middle of the map, away from anti-air.
    /// Returns the squad.
    pub(crate) fn run(
        &mut self,
        view: &BotView<'_>,
        home: (u16, u16),
        keep: usize,
        think_every: u32,
        orders: &mut Vec<Command>,
    ) -> Vec<u64> {
        let frame = view.frame();
        let mut all: Vec<&crate::sim::game_entity::GameEntity> = view
            .own()
            .filter(|e| ROCKETEER_TYPES.contains(&view.name(e.type_ref())) && !e.in_limbo())
            .collect();
        all.sort_by_key(|e| e.stable_id());
        // `keep` Rocketeers stay with the bot; Siege Choppers and Discs all fly.
        let mut kept = 0;
        let squad: Vec<&crate::sim::game_entity::GameEntity> = all
            .into_iter()
            .filter(|e| {
                if kept < keep && view.name(e.type_ref()) == "JUMPJET" {
                    kept += 1;
                    false
                } else {
                    true
                }
            })
            .collect();
        self.ids = squad.iter().map(|e| e.stable_id()).collect();
        if squad.is_empty() || frame % 30 >= think_every {
            return self.ids.clone();
        }
        // Seen anti-air: what fires at aircraft, with its reach.
        let aa: Vec<((u16, u16), i32)> = view
            .enemy_objects()
            .filter_map(|e| {
                let t = view.object_type(e.type_ref())?;
                if t.powered && view.power_low(e.owner()) {
                    return None;
                }
                let reach = t
                    .weapon_list
                    .iter()
                    .flatten()
                    .filter_map(|w| view.rules.weapon(w))
                    .filter(|w| {
                        w.projectile
                            .as_deref()
                            .and_then(|p| view.rules.projectile(p))
                            .is_some_and(|p| p.aa)
                    })
                    .map(|w| (w.range_leptons + 255) / 256)
                    .max()?;
                Some((cell(e), reach + ROCKETEER_AA_MARGIN))
            })
            .collect();
        let covered = |c: (u16, u16)| aa.iter().any(|&(z, r)| dist2(z, c) <= r * r);
        let n = squad.len() as i32;
        let centre = (
            (squad.iter().map(|e| i32::from(cell(e).0)).sum::<i32>() / n) as u16,
            (squad.iter().map(|e| i32::from(cell(e).1)).sum::<i32>() / n) as u16,
        );
        let own_buildings: Vec<(u16, u16)> =
            view.own().filter(|e| is_structure(e)).map(cell).collect();
        let target = view
            .enemy_objects()
            .filter(|e| !e.in_limbo() && e.category() != EntityCategory::Aircraft)
            .filter_map(|e| {
                let t = view.object_type(e.type_ref())?;
                let c = cell(e);
                if covered(c) || t.wall {
                    return None;
                }
                let rank = if t.harvester || t.resource_gatherer {
                    0
                } else if !is_structure(e)
                    && t.weapon_list.iter().any(Option::is_some)
                    && own_buildings.iter().any(|&b| dist2(b, c) <= 30 * 30)
                {
                    1
                } else if !is_structure(e) {
                    2
                } else {
                    3
                };
                Some((rank, dist2(c, centre), e.stable_id(), c))
            })
            .min();
        if let Some((rank, _, id, c)) = target {
            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "rocketeers f{frame} {} on {id} rank {rank} at {c:?}",
                    squad.len()
                );
            }
            for e in &squad {
                let on_it = e
                    .attack_target
                    .target()
                    .is_some_and(|t| t == crate::sim::combat::TargetKind::Entity(id));
                if !on_it {
                    orders.push(Command::Attack {
                        attacker_id: e.stable_id(),
                        target_id: id,
                    });
                }
            }
            self.roam = None;
            return self.ids.clone();
        }
        // Roam: start locations and the map's middle in turn, skipping
        // what anti-air covers; a new point after arriving or a minute.
        let (w, h) = view.map_size();
        let mut points = view.start_locations();
        points.push((w / 2, h / 2));
        points.retain(|&p| !covered(p) && dist2(p, home) > 20 * 20);
        if points.is_empty() {
            points.push(home);
        }
        let arrived = self
            .roam
            .is_some_and(|r| dist2(r, centre) <= 5 * 5 || covered(r));
        if self.roam.is_none() || arrived || frame.saturating_sub(self.roam_since) > MINUTE {
            self.hops = self.hops.wrapping_add(1);
            self.roam = Some(points[self.hops as usize % points.len()]);
            self.roam_since = frame;
        }
        let Some(to) = self.roam else {
            return self.ids.clone();
        };
        for e in &squad {
            if e.navigation.nav_com().is_none() || frame == self.roam_since {
                orders.push(Command::Move {
                    entity_id: e.stable_id(),
                    target_rx: to.0,
                    target_ry: to.1,
                });
            }
        }
        self.ids.clone()
    }
}

/// Playtest, 2026-11-14: "for attacking, build a hit squad of 3x3 prism tanks,
/// keep them in that formation and move them one square towards the enemy
/// every time there's nothing from the enemy in any of their ranges
/// anymore". Saved with the bot; Allied bots only (the side that builds
/// `SREF`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrismSquad {
    ids: Vec<u64>,
    /// The formation's middle cell, and whether the squad stood formed.
    anchor: Option<(u16, u16)>,
    formed: bool,
    /// When the block last stepped.
    #[serde(default)]
    since: u32,
}

const PRISM_TYPE: &str = "SREF";
/// The squad: 3 by 3.
const PRISM_SQUAD: usize = 9;
/// Fewer left than this: back to the rally point to fill up.
const PRISM_REGROUP: usize = 5;

impl PrismSquad {
    pub(crate) fn ids(&self) -> &[u64] {
        &self.ids
    }

    /// Queue a Prism Tank when the squad lacks one, none is queued and
    /// money allows two. Never blocks the bot's own production.
    pub(crate) fn produce(
        &self,
        view: &BotView<'_>,
        options: &[BuildOption],
        orders: &mut Vec<Command>,
    ) -> bool {
        let Some(t) = options.iter().find(|o| {
            o.enabled
                && o.queue_category == ProductionCategory::Vehicle
                && view.name(o.type_id) == PRISM_TYPE
        }) else {
            return false;
        };
        let Some(f) = view.factory(ProductionCategory::Vehicle) else {
            return false;
        };
        let queued = f.queue.iter().any(|q| q.type_id == t.type_id)
            || f.object.is_some_and(|o| o.type_id == t.type_id);
        let have = view
            .own()
            .filter(|e| view.name(e.type_ref()) == PRISM_TYPE)
            .count();
        if queued || f.queue.len() >= 4 || have >= PRISM_SQUAD || view.credits() < t.cost * 2 {
            return false;
        }
        orders.push(Command::QueueProduction { type_id: t.type_id });
        true
    }

    /// Every 15 frames. Short of 9 (or down to under [`PRISM_REGROUP`]):
    /// the tanks gather in a 3x3 block 10 cells from home toward the map's
    /// middle. Formed: while any seen enemy ground object is in a member's
    /// reach, each member fires at the nearest one it reaches; with none,
    /// once every member stands on its slot, the block steps one cell
    /// toward the nearest seen enemy building (else an unexplored enemy
    /// start). Returns the squad.
    pub(crate) fn run(
        &mut self,
        view: &BotView<'_>,
        home: (u16, u16),
        think_every: u32,
        orders: &mut Vec<Command>,
    ) -> Vec<u64> {
        let frame = view.frame();
        let mut tanks: Vec<&crate::sim::game_entity::GameEntity> = view
            .own()
            .filter(|e| view.name(e.type_ref()) == PRISM_TYPE && !e.in_limbo())
            .collect();
        tanks.sort_by_key(|e| e.stable_id());
        tanks.truncate(PRISM_SQUAD);
        self.ids = tanks.iter().map(|e| e.stable_id()).collect();
        if tanks.is_empty() {
            self.anchor = None;
            self.formed = false;
            return Vec::new();
        }
        if frame % 15 >= think_every {
            return self.ids.clone();
        }
        let (w, h) = view.map_size();
        let short = if self.formed {
            tanks.len() < PRISM_REGROUP
        } else {
            tanks.len() < PRISM_SQUAD
        };
        if short {
            self.formed = false;
            self.anchor = Some(toward(home, (w / 2, h / 2), 10));
        }
        let anchor = *self
            .anchor
            .get_or_insert_with(|| toward(home, (w / 2, h / 2), 10));
        let slot = |i: usize, a: (u16, u16)| {
            (
                (i32::from(a.0) + (i % 3) as i32 - 1).clamp(1, i32::from(w) - 2) as u16,
                (i32::from(a.1) + (i / 3) as i32 - 1).clamp(1, i32::from(h) - 2) as u16,
            )
        };
        let move_to =
            |e: &crate::sim::game_entity::GameEntity, c: (u16, u16), orders: &mut Vec<Command>| {
                let going = e.navigation.nav_com()
                    == Some(crate::sim::components::NavTargetRef::Cell { rx: c.0, ry: c.1 });
                if cell(e) != c && !going {
                    orders.push(Command::Move {
                        entity_id: e.stable_id(),
                        target_rx: c.0,
                        target_ry: c.1,
                    });
                }
            };
        // On its slot: idle within 2 cells of it, or near it and the step
        // is 10 seconds old (a slot can be blocked); after 30 seconds the
        // block steps on regardless.
        let late = frame.saturating_sub(self.since) > 150;
        let stuck = self.formed && frame.saturating_sub(self.since) > 450;
        let on_slots = stuck
            || tanks.iter().enumerate().all(|(i, e)| {
                let d = dist2(cell(e), slot(i, anchor));
                (d <= 2 * 2 && e.navigation.nav_com().is_none()) || (late && d <= 3 * 3)
            });
        if !self.formed {
            if short || !on_slots {
                for (i, e) in tanks.iter().enumerate() {
                    move_to(e, slot(i, anchor), orders);
                }
                return self.ids.clone();
            }
            self.formed = true;
        }
        // Anything to shoot?
        let enemies: Vec<(u64, (u16, u16))> = view
            .enemy_objects()
            .filter(|e| !e.in_limbo() && e.category() != EntityCategory::Aircraft)
            .filter(|e| view.object_type(e.type_ref()).is_some_and(|t| !t.wall))
            .map(|e| (e.stable_id(), cell(e)))
            .collect();
        let mut firing = false;
        for e in &tanks {
            let reach = view
                .object_type(e.type_ref())
                .and_then(|t| {
                    t.weapon_list
                        .iter()
                        .flatten()
                        .filter_map(|w| view.rules.weapon(w))
                        .map(|w| (w.range_leptons + 255) / 256)
                        .max()
                })
                .unwrap_or(0);
            let at = cell(e);
            let Some(&(target, _)) = enemies
                .iter()
                .filter(|(_, c)| dist2(*c, at) <= reach * reach)
                .min_by_key(|(id, c)| (dist2(*c, at), *id))
            else {
                continue;
            };
            firing = true;
            let on_it = e
                .attack_target
                .target()
                .is_some_and(|t| t == crate::sim::combat::TargetKind::Entity(target));
            if !on_it {
                orders.push(Command::Attack {
                    attacker_id: e.stable_id(),
                    target_id: target,
                });
            }
        }
        if log::log_enabled!(log::Level::Debug) && frame % 90 < think_every {
            log::debug!(
                "prisms f{frame} {} at {anchor:?} firing {firing} on slots {on_slots}",
                tanks.len()
            );
        }
        if firing {
            return self.ids.clone();
        }
        if !on_slots {
            for (i, e) in tanks.iter().enumerate() {
                move_to(e, slot(i, anchor), orders);
            }
            return self.ids.clone();
        }
        // One cell toward the enemy.
        let goal = view
            .enemy_objects()
            .filter(|e| is_structure(e))
            .map(|e| (dist2(cell(e), anchor), cell(e)))
            .min()
            .map(|(_, c)| c)
            .or_else(|| {
                view.start_locations()
                    .into_iter()
                    .filter(|&s| dist2(s, home) > 20 * 20 && !view.explored(s.0, s.1))
                    .min_by_key(|&s| (dist2(s, anchor), s))
            });
        let Some(goal) = goal else {
            return self.ids.clone();
        };
        let step = (
            (i32::from(anchor.0) + (i32::from(goal.0) - i32::from(anchor.0)).signum()) as u16,
            (i32::from(anchor.1) + (i32::from(goal.1) - i32::from(anchor.1)).signum()) as u16,
        );
        if step == anchor {
            return self.ids.clone();
        }
        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "prisms f{frame} {} step {anchor:?} -> {step:?}",
                tanks.len()
            );
        }
        self.anchor = Some(step);
        self.since = frame;
        for (i, e) in tanks.iter().enumerate() {
            move_to(e, slot(i, step), orders);
        }
        self.ids.clone()
    }
}

/// A unit holds its ground in seen enemies' reach only with this much more
/// power around it than the enemies that reach it (percent).
const HOLD_RATIO: i64 = 150;
/// Cells around a unit whose own armed units and defences count with it.
const HOLD_NEAR: i32 = 6;
/// Within this many cells of the own Construction Yard units defend rather
/// than step back.
const HOLD_HOME: i32 = 12;

/// Playtest, 2026-11-14: "our bots are still very happy to send units somewhere
/// (harvesting, scouting, ...) where they just get killed. stay away from
/// enemy fire unless you're certain you can kill them". Every 15 frames an
/// own ground unit within a seen enemy's reach weighs the power there (cost
/// times health) of the enemies reaching it, defences and garrisons among
/// them, against its own and its armed neighbours' within [`HOLD_NEAR`]; with
/// less than [`HOLD_RATIO`] percent of theirs it walks back toward home
/// instead of whatever it was told (a plain move, so it does not stop to
/// trade shots). Units at home, errand units (their own escapes), miners,
/// Iron-Curtained units and units in a transport are left alone.
pub(crate) fn hold_back(view: &BotView<'_>, errands: &[u64], orders: &mut Vec<Command>) {
    let frame = view.frame();
    if frame % 15 != 0 {
        return;
    }
    let power = |e: &crate::sim::game_entity::GameEntity| -> i64 {
        let Some(t) = view.object_type(e.type_ref()) else {
            return 0;
        };
        let occupants = e.passenger_role().occupants();
        let base = if is_structure(e) && !occupants.is_empty() {
            occupants
                .ids()
                .iter()
                .filter_map(|&o| view.entity(o))
                .filter_map(|o| view.object_type(o.type_ref()))
                .map(|o| i64::from(o.cost.max(0)))
                .sum()
        } else {
            i64::from(t.cost.max(0))
        };
        base * i64::from(e.health.current().max(0)) / i64::from(t.strength.max(1))
    };
    let threats: Vec<((u16, u16), i32, i64)> = {
        let danger = Danger::seen(view, 1);
        view.enemy_objects()
            .filter(|e| e.category() != EntityCategory::Aircraft && !e.in_limbo())
            .filter_map(|e| {
                let c = cell(e);
                let r = danger
                    .zones
                    .iter()
                    .find(|(z, _)| *z == c)
                    .map(|&(_, r)| r)?;
                Some((c, r, power(e)))
            })
            .collect()
    };
    if threats.is_empty() {
        return;
    }
    let invulnerable = |e: &crate::sim::game_entity::GameEntity| {
        crate::sim::superweapon::invulnerability::is_invulnerable(e.invulnerability.as_ref(), frame)
    };
    let yard = view
        .own()
        .find(|e| {
            is_structure(e)
                && view
                    .object_type(e.type_ref())
                    .is_some_and(|t| t.construction_yard)
        })
        .map(cell);
    let mine: Vec<((u16, u16), i64)> = view
        .own()
        .filter(|e| !e.in_limbo() && e.category() != EntityCategory::Aircraft)
        .filter(|e| {
            view.object_type(e.type_ref())
                .is_some_and(|t| t.weapon_list.iter().any(Option::is_some))
                || !e.passenger_role().occupants().is_empty()
        })
        .map(|e| (cell(e), power(e)))
        .collect();
    let mut retreats: Vec<(u64, (u16, u16))> = Vec::new();
    for unit in view.own() {
        if is_structure(unit)
            || unit.in_limbo()
            || unit.category() == EntityCategory::Aircraft
            || errands.contains(&unit.stable_id())
            || invulnerable(unit)
        {
            continue;
        }
        let Some(ty) = view.object_type(unit.type_ref()) else {
            continue;
        };
        if ty.considered_aircraft
            || ty.jumpjet
            || ty.harvester
            || ty.resource_gatherer
            || ty.deploys_into.is_some()
        {
            continue;
        }
        let at = cell(unit);
        let Some(home) = yard else { break };
        if dist2(at, home) <= HOLD_HOME * HOLD_HOME {
            continue;
        }
        let reaching: Vec<&((u16, u16), i32, i64)> = threats
            .iter()
            .filter(|(c, r, _)| dist2(*c, at) <= r * r)
            .collect();
        if reaching.is_empty() {
            continue;
        }
        let theirs: i64 = reaching.iter().map(|t| t.2).sum();
        let ours: i64 = mine
            .iter()
            .filter(|(c, _)| dist2(*c, at) <= HOLD_NEAR * HOLD_NEAR)
            .map(|m| m.1)
            .sum();
        if ours * 100 >= theirs * HOLD_RATIO {
            continue;
        }
        let n = reaching.len() as i32;
        let centre = (
            (reaching.iter().map(|t| i32::from(t.0.0)).sum::<i32>() / n) as u16,
            (reaching.iter().map(|t| i32::from(t.0.1)).sum::<i32>() / n) as u16,
        );
        // Back toward home, or straight away from them when home is behind
        // them.
        let back = toward(at, home, 10);
        let to = if dist2(back, centre) > dist2(at, centre) {
            back
        } else {
            away_from(at, centre, 10, view.map_size())
        };
        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "hold back f{frame} {} {} at {at:?} ours {ours} theirs {theirs} -> {to:?}",
                view.name(unit.type_ref()),
                unit.stable_id()
            );
        }
        retreats.push((unit.stable_id(), to));
    }
    if retreats.is_empty() {
        return;
    }
    orders.retain(|o| !retreats.iter().any(|(id, _)| order_unit(o) == Some(*id)));
    for (id, to) in retreats {
        orders.push(Command::Move {
            entity_id: id,
            target_rx: to.0,
            target_ry: to.1,
        });
    }
}

/// The unit an order moves or fires, if it is one of those.
fn order_unit(order: &Command) -> Option<u64> {
    match order {
        Command::Move { entity_id, .. } | Command::AttackMove { entity_id, .. } => Some(*entity_id),
        Command::Attack { attacker_id, .. } => Some(*attacker_id),
        Command::CaptureBuilding { engineer_id, .. } => Some(*engineer_id),
        _ => None,
    }
}

/// A bot's crate run, saved with it. A crate's Armor, Firepower, Speed and
/// Veteran arms raise every object strictly within `[CrateRules]
/// CrateRadius=` of it (3 cells in retail; `crates::effects`, Armor
/// `0x00482D56`, Firepower `0x00483125`, Veteran `0x00482972`), so near home
/// the most valuable units nearby gather within 2 cells first, then the one
/// nearest steps on it. A crate anywhere else on the map is fetched by the
/// nearest unit alone. Never a crate, or a way to it, within a seen enemy's
/// reach ([`Danger`]); a run that becomes one is called off. Retail
/// `[Powerups]` gives no harmful arm any weight.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CrateHunt {
    /// The crate cell being gathered at.
    target: Option<(u16, u16)>,
    /// When the run started.
    since: u32,
    /// No new run before this frame (after one that gave up).
    rest_until: u32,
    /// A far crate's runner.
    #[serde(default)]
    runner: Option<u64>,
}

impl CrateHunt {
    /// The crate being gathered at, for spectator markers.
    /// A far crate's runner, on its errand.
    pub(crate) fn runner(&self) -> Option<u64> {
        self.runner
    }

    pub(crate) fn target(&self) -> Option<(u16, u16)> {
        self.target
    }

    fn give_up(&mut self, frame: u32, rest: u32) -> Vec<u64> {
        self.target = None;
        self.runner = None;
        self.rest_until = frame + rest;
        Vec::new()
    }

    /// Run the crate errand with the bot's `units` (id, cell), around
    /// `home`. Returns the units it ordered, which the bot leaves alone this
    /// think. Nothing while a seen armed enemy is within 20 cells of home.
    pub(crate) fn run(
        &mut self,
        view: &BotView<'_>,
        home: (u16, u16),
        units: &[(u64, (u16, u16))],
        orders: &mut Vec<Command>,
    ) -> Vec<u64> {
        let frame = view.frame();
        let crates = view.crates();
        if let Some(c) = self.target
            && !crates.contains(&c)
        {
            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "crate f{frame} {c:?} gone after {} frames",
                    frame - self.since
                );
            }
            self.target = None;
            self.runner = None;
        }
        let armed_enemies: Vec<(u16, u16)> = view
            .enemy_objects()
            .filter(|e| {
                view.object_type(e.type_ref())
                    .is_some_and(|t| t.weapon_list.iter().any(Option::is_some))
            })
            .map(cell)
            .collect();
        if armed_enemies.iter().any(|&e| dist2(e, home) <= 20 * 20) {
            self.target = None;
            self.runner = None;
            return Vec::new();
        }
        let danger = Danger::seen(view, CRATE_MARGIN);
        // Ground units that can pick a crate up, most valuable first.
        let mut walkers: Vec<(
            u64,
            (u16, u16),
            i32,
            crate::rules::locomotor_type::MovementZone,
        )> = units
            .iter()
            .filter_map(|&(id, at)| {
                let e = view.entity(id)?;
                let t = view.object_type(e.type_ref())?;
                let flies =
                    e.category() == EntityCategory::Aircraft || t.considered_aircraft || t.jumpjet;
                (!flies && !is_structure(e)).then_some((id, at, t.cost.max(0), t.movement_zone))
            })
            .collect();
        walkers.sort_by_key(|w| (std::cmp::Reverse(w.2), w.0));
        let can_go = |w: &(u64, (u16, u16), i32, _), c: (u16, u16)| {
            view.can_reach(w.3, w.1, c) && !danger.on_way(w.1, c)
        };
        let near = |c: (u16, u16)| {
            walkers
                .iter()
                .filter(move |w| dist2(w.1, c) <= CRATE_CALL * CRATE_CALL && can_go(w, c))
        };
        // The nearest unit that can go safely, for a far crate.
        let runner_for = |c: (u16, u16)| {
            walkers
                .iter()
                .filter(|w| can_go(w, c))
                .min_by_key(|w| (dist2(w.1, c), w.0))
                .copied()
        };
        let far = |c: (u16, u16)| dist2(c, home) > CRATE_RANGE * CRATE_RANGE;
        if self.target.is_none() {
            if frame < self.rest_until {
                return Vec::new();
            }
            self.target = crates
                .iter()
                .copied()
                .filter(|&c| !danger.at(c))
                .filter(|&c| {
                    if far(c) {
                        runner_for(c).is_some()
                    } else {
                        near(c).next().is_some()
                    }
                })
                .min_by_key(|&c| (dist2(c, home), c));
            self.since = frame;
            self.runner = None;
            if let Some(c) = self.target
                && log::log_enabled!(log::Level::Debug)
            {
                log::debug!("crate f{frame} run to {c:?} from home {home:?}");
            }
        }
        let Some(c) = self.target else {
            return Vec::new();
        };
        if danger.at(c) {
            return self.give_up(frame, 300);
        }
        let fresh = frame == self.since;
        if far(c) {
            // One runner, kept while its way stays clear.
            let kept = self
                .runner
                .and_then(|id| walkers.iter().find(|w| w.0 == id).copied())
                .filter(|w| !danger.on_way(w.1, c));
            let runner = match kept {
                Some(w) => w,
                None if self.runner.is_some() => return self.give_up(frame, 300),
                None => match runner_for(c) {
                    Some(w) => w,
                    None => return self.give_up(frame, 900),
                },
            };
            if frame.saturating_sub(self.since) > 45 * 180 {
                return self.give_up(frame, 900);
            }
            let moving = view
                .entity(runner.0)
                .is_some_and(|e| e.navigation.nav_com().is_some());
            if self.runner != Some(runner.0) || !moving {
                orders.push(Command::Move {
                    entity_id: runner.0,
                    target_rx: c.0,
                    target_ry: c.1,
                });
            }
            self.runner = Some(runner.0);
            return vec![runner.0];
        }
        let escort: Vec<_> = near(c).take(CRATE_ESCORT).copied().collect();
        if escort.is_empty() || frame.saturating_sub(self.since) > 1800 {
            return self.give_up(frame, 900);
        }
        let picker = escort
            .iter()
            .min_by_key(|w| (dist2(w.1, c), w.0))
            .map(|w| w.0);
        // Places around the crate, nearest first; never the crate's cell.
        let mut ring: Vec<(i32, i32)> = (-2..=2)
            .flat_map(|dy| (-2..=2).map(move |dx| (dx, dy)))
            .filter(|&d| d != (0, 0))
            .collect();
        ring.sort_by_key(|&(dx, dy)| (dx * dx + dy * dy, dy, dx));
        let placed = |at: (u16, u16)| {
            at != c
                && (i32::from(at.0) - i32::from(c.0)).abs() <= 2
                && (i32::from(at.1) - i32::from(c.1)).abs() <= 2
        };
        let mut ready = true;
        let mut slot = 0;
        for w in &escort {
            if Some(w.0) == picker {
                continue;
            }
            if placed(w.1) {
                continue;
            }
            ready = false;
            let moving = view
                .entity(w.0)
                .is_some_and(|e| e.navigation.nav_com().is_some());
            let (dx, dy) = ring[slot % ring.len()];
            slot += 1;
            if fresh || !moving {
                orders.push(Command::Move {
                    entity_id: w.0,
                    target_rx: (i32::from(c.0) + dx).max(1) as u16,
                    target_ry: (i32::from(c.1) + dy).max(1) as u16,
                });
            }
        }
        // Gathered, or 40 seconds on: the picker steps on the crate.
        if let Some(picker) = picker {
            let go = ready || frame.saturating_sub(self.since) > 600;
            let moving = view
                .entity(picker)
                .is_some_and(|e| e.navigation.nav_com().is_some());
            let at = escort.iter().find(|w| w.0 == picker).map_or(c, |w| w.1);
            if go && (!moving || fresh) {
                orders.push(Command::Move {
                    entity_id: picker,
                    target_rx: c.0,
                    target_ry: c.1,
                });
            } else if !go && !placed(at) && (fresh || !moving) {
                // Waits beside the crate meanwhile.
                orders.push(Command::Move {
                    entity_id: picker,
                    target_rx: (i32::from(c.0) + ring[slot % ring.len()].0).max(1) as u16,
                    target_ry: (i32::from(c.1) + ring[slot % ring.len()].1).max(1) as u16,
                });
            }
        }
        escort.iter().map(|w| w.0).collect()
    }
}

/// Scout dogs kept (2 at first, 3 from minute 3).
const SCOUT_DOGS: [usize; 2] = [2, 3];
/// Dogs built for scouting in all (lost ones are replaced up to this).
const SCOUT_DOGS_BUILT: u32 = 8;
/// Yuri has no dogs: its Initiates scout instead (playtest, 2026-11-14: "scout
/// with normal infantry for crates"); only Yuri builds them.
const DOG_TYPES: [&str; 3] = ["ADOG", "DOG", "INIT"];
/// The Chrono Legionnaire, once the Battle Lab allows it.
const CHRONO_TYPES: [&str; 1] = ["CLEG"];
/// A crate or goal is safe this far (cells) from every seen armed enemy
/// and enemy building.
const SCOUT_SAFE: i32 = 12;
/// A scout this near to one turns back home.
const SCOUT_FLEE: i32 = 9;
/// How long a place where a scout died or was hit stays avoided.
const SCOUT_SCAR_FRAMES: u32 = 4 * 45 * 60;
/// Cells kept from where a scout died or was hit.
const SCOUT_SCAR: i32 = 12;
/// Cells kept from an enemy's start position until it has been explored
/// (bases stand there before anything of them is seen).
const SCOUT_START: i32 = 24;
/// Frames a game minute.
const MINUTE: u32 = 45 * 60;

/// Crate scouts, saved with the bot (playtest, 2026-11-10: "2-3 dogs early and a
/// Chrono Legionnaire later with the only purpose to scout for and collect
/// crates without risking damage by going too close to the enemy"). Only in
/// matches with crates. The scouts explore the map (unexplored cells first),
/// step on every safe crate they see, keep [`SCOUT_SAFE`] cells from what
/// the bot has seen of the enemy and turn home when something armed comes
/// within [`SCOUT_FLEE`]. The bot leaves them to this errand.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CrateScouts {
    /// The scouts (stable ids).
    ids: Vec<u64>,
    /// Each scout's goal and when it was ordered there.
    goals: Vec<(u64, (u16, u16), u32)>,
    dogs_built: u32,
    chrono_built: u32,
    /// Each scout's cell and health at the last look.
    #[serde(default)]
    last: Vec<(u64, (u16, u16), i32)>,
    /// Where scouts died or were hit, and when: avoided for
    /// [`SCOUT_SCAR_FRAMES`] (playtest, 2026-11-11: "don't run them into range of
    /// enemies": most were killed by what the bot had not seen yet).
    #[serde(default)]
    scars: Vec<((u16, u16), u32)>,
}

impl CrateScouts {
    fn on(view: &BotView<'_>) -> bool {
        view.crates_enabled() || !view.crates().is_empty()
    }

    fn kind(view: &BotView<'_>, id: u64) -> Option<(bool, bool)> {
        let e = view.entity(id)?;
        let name = view.name(e.type_ref());
        Some((
            DOG_TYPES.iter().any(|n| n.eq_ignore_ascii_case(name)),
            CHRONO_TYPES.iter().any(|n| n.eq_ignore_ascii_case(name)),
        ))
    }

    /// Scout dogs alive (the bot's own dog counts leave them out).
    pub(crate) fn dogs(&self, view: &BotView<'_>) -> usize {
        self.ids
            .iter()
            .filter(|&&id| Self::kind(view, id).is_some_and(|k| k.0))
            .count()
    }

    /// The scouts: the bot gives them no other order.
    pub(crate) fn ids(&self) -> &[u64] {
        &self.ids
    }

    /// With the barracks idle: queue a scout that is missing. `true` when
    /// it did (the bot builds nothing else there this think).
    pub(crate) fn produce(
        &mut self,
        view: &BotView<'_>,
        options: &[BuildOption],
        orders: &mut Vec<Command>,
    ) -> bool {
        if !Self::on(view) {
            return false;
        }
        let busy = view
            .factory(ProductionCategory::Infantry)
            .map_or(0, |f| f.queue.len() + usize::from(f.object.is_some()));
        if busy > 0 {
            return false;
        }
        let pick = |names: &[&str]| {
            options
                .iter()
                .find(|o| {
                    o.enabled
                        && o.queue_category == ProductionCategory::Infantry
                        && names
                            .iter()
                            .any(|n| view.name(o.type_id).eq_ignore_ascii_case(n))
                })
                .map(|o| o.type_id)
        };
        let want_dogs = SCOUT_DOGS[usize::from(view.frame() >= 3 * MINUTE)];
        if self.dogs(view) < want_dogs
            && self.dogs_built < SCOUT_DOGS_BUILT
            && let Some(dog) = pick(&DOG_TYPES)
        {
            self.dogs_built += 1;
            orders.push(Command::QueueProduction { type_id: dog });
            return true;
        }
        let chrono = self
            .ids
            .iter()
            .any(|&id| Self::kind(view, id).is_some_and(|k| k.1));
        if !chrono
            && self.chrono_built < 3
            && view.credits() >= 1500
            && let Some(cleg) = pick(&CHRONO_TYPES)
        {
            self.chrono_built += 1;
            orders.push(Command::QueueProduction { type_id: cleg });
            return true;
        }
        false
    }

    /// Claim new dogs / the Chrono Legionnaire and send every scout on.
    /// `taken`: the crate the army's crate run gathers at (left to it).
    pub(crate) fn run(
        &mut self,
        view: &BotView<'_>,
        home: (u16, u16),
        taken: Option<(u16, u16)>,
        orders: &mut Vec<Command>,
    ) {
        let frame = view.frame();
        // A scout gone (died) or hurt since the last look marks the spot.
        for &(id, at, health) in &self.last {
            let now = view.entity(id).filter(|e| e.owner() == view.house);
            if now.is_none_or(|e| e.health.current() < health) {
                self.scars.push((now.map_or(at, cell), frame));
            }
        }
        self.scars
            .retain(|&(_, at)| frame.saturating_sub(at) < SCOUT_SCAR_FRAMES);
        if self.scars.len() > 32 {
            self.scars.drain(..self.scars.len() - 32);
        }
        self.ids
            .retain(|&id| view.entity(id).is_some_and(|e| e.owner() == view.house));
        self.last = self
            .ids
            .iter()
            .filter_map(|&id| view.entity(id).map(|e| (id, cell(e), e.health.current())))
            .collect();
        let ids = &self.ids;
        self.goals.retain(|g| ids.contains(&g.0));
        if !Self::on(view) {
            return;
        }
        // Claim: dogs up to the wanted count, one Chrono Legionnaire.
        let want_dogs = SCOUT_DOGS[usize::from(frame >= 3 * MINUTE)];
        let mut dogs = self.dogs(view);
        let mut chrono = self
            .ids
            .iter()
            .any(|&id| Self::kind(view, id).is_some_and(|k| k.1));
        let mut fresh: Vec<u64> = view
            .own()
            .filter(|e| !is_structure(e))
            .map(|e| e.stable_id())
            .filter(|id| !self.ids.contains(id))
            .collect();
        fresh.sort_unstable();
        for id in fresh {
            match Self::kind(view, id) {
                Some((true, _)) if dogs < want_dogs => {
                    dogs += 1;
                    self.ids.push(id);
                }
                Some((_, true)) if !chrono => {
                    chrono = true;
                    self.ids.push(id);
                }
                _ => {}
            }
        }
        // What the bot has seen of the enemy: armed objects and buildings.
        let danger: Vec<(u16, u16)> = view
            .enemy_objects()
            .filter(|e| {
                is_structure(e)
                    || view
                        .object_type(e.type_ref())
                        .is_some_and(|t| t.weapon_list.iter().any(Option::is_some))
            })
            .map(cell)
            .collect();
        // Also where scouts were hurt, and enemy starts not yet explored.
        let own_start = view
            .start_locations()
            .into_iter()
            .min_by_key(|&s| (dist2(s, home), s));
        let mut zones: Vec<((u16, u16), i32)> =
            self.scars.iter().map(|&(c, _)| (c, SCOUT_SCAR)).collect();
        zones.extend(
            view.start_locations()
                .into_iter()
                .filter(|&s| Some(s) != own_start && !view.explored(s.0, s.1))
                .map(|s| (s, SCOUT_START)),
        );
        let reach = Danger::seen(view, 4);
        let near_danger = |c: (u16, u16), d: i32| {
            danger.iter().any(|&e| dist2(e, c) <= d * d)
                || zones.iter().any(|&(z, r)| dist2(z, c) <= r * r)
                || reach.at(c)
        };
        // The straight way, every 2nd cell, stays out of flee range.
        let way_clear = |from: (u16, u16), to: (u16, u16)| {
            let dx = i32::from(to.0) - i32::from(from.0);
            let dy = i32::from(to.1) - i32::from(from.1);
            let steps = (dx.abs().max(dy.abs()) / 2).max(1);
            (1..=steps).all(|i| {
                let x = (i32::from(from.0) + dx * i / steps).max(0) as u16;
                let y = (i32::from(from.1) + dy * i / steps).max(0) as u16;
                !near_danger((x, y), SCOUT_FLEE)
            })
        };
        let crates = view.crates();
        let mut claimed: Vec<(u16, u16)> = taken.into_iter().collect();
        let (w, h) = view.map_size();
        // Where idle scouts wait for crates, by scout index (worked out
        // when first needed this look).
        let mut stations: Vec<(u16, u16)> = Vec::new();
        for (n, &id) in self.ids.clone().iter().enumerate() {
            let Some(e) = view.entity(id) else { continue };
            let Some(ty) = view.object_type(e.type_ref()) else {
                continue;
            };
            let at = cell(e);
            let mz = ty.movement_zone;
            let current = self.goals.iter().find(|g| g.0 == id).map(|g| (g.1, g.2));
            // Too near: away from the nearest danger (else home) at once,
            // and on to there unless that goal is unsafe itself.
            if near_danger(at, SCOUT_FLEE) {
                let fleeing = current.is_some_and(|(g, since)| {
                    !near_danger(g, SCOUT_SAFE) && frame.saturating_sub(since) < MINUTE / 2
                });
                if !fleeing {
                    let d = danger
                        .iter()
                        .copied()
                        .chain(zones.iter().map(|z| z.0))
                        .min_by_key(|&d| (dist2(d, at), d))
                        .unwrap_or(home);
                    let (dx, dy) = (
                        i32::from(at.0) - i32::from(d.0),
                        i32::from(at.1) - i32::from(d.1),
                    );
                    let len = (dx * dx + dy * dy).isqrt().max(1);
                    let away = (
                        (i32::from(at.0) + dx * 15 / len).clamp(1, i32::from(w) - 2) as u16,
                        (i32::from(at.1) + dy * 15 / len).clamp(1, i32::from(h) - 2) as u16,
                    );
                    let back = if !near_danger(away, SCOUT_SAFE) && view.can_reach(mz, at, away) {
                        away
                    } else {
                        toward(at, home, 15)
                    };
                    if log::log_enabled!(log::Level::Debug) {
                        log::debug!("scout f{frame} {id} flees from {at:?} to {back:?}");
                    }
                    self.set_goal(id, back, frame, orders);
                }
                continue;
            }
            // A safe crate in sight: the nearest scout goes.
            let target = crates
                .iter()
                .copied()
                .filter(|c| !claimed.contains(c) && !near_danger(*c, SCOUT_SAFE))
                .filter(|&c| way_clear(at, c) && view.can_reach(mz, at, c))
                .min_by_key(|&c| (dist2(c, at), c));
            if let Some(c) = target {
                claimed.push(c);
                if current.is_none_or(|(g, _)| g != c) {
                    if log::log_enabled!(log::Level::Debug) {
                        log::debug!(
                            "scout f{frame} {id} {} to crate {c:?} from {at:?}",
                            view.name(e.type_ref())
                        );
                    }
                    self.set_goal(id, c, frame, orders);
                }
                continue;
            }
            // Playtest, 2026-11-14: "collect crates when there is a safe one,
            // explore black where it is safe and then just wait for crates
            // spread across the map for minimal crate collection time".
            // A goal is kept while safe and not reached: an unexplored
            // spot until it is explored, a station for good.
            let others: Vec<(u16, u16)> = self
                .goals
                .iter()
                .filter(|g| g.0 != id)
                .map(|g| g.1)
                .collect();
            let keep = current.is_some_and(|(g, _)| {
                !near_danger(g, SCOUT_SAFE)
                    && way_clear(at, g)
                    && (!crates.contains(&g) || claimed.contains(&g))
                    && (!view.explored(g.0, g.1) || stations.get(n) == Some(&g))
            });
            if keep {
                continue;
            }
            // The nearest safe unexplored spot no other scout is heading for,
            // inside the playfield (playtest, 2026-11-14: "the scouts leave some
            // unexplored regions in the very north": the black cells
            // nearest there were the cell array's corners outside the map,
            // and the twelve tried were all unreachable). Black the scout
            // cannot stand on (water, cliffs) is seen from a reachable
            // cell up to 4 cells off.
            let mut black: Vec<(i32, (u16, u16))> = Vec::new();
            for y in (1..h.saturating_sub(1)).step_by(3) {
                for x in (1..w.saturating_sub(1)).step_by(3) {
                    if !view.explored(x, y)
                        && view.in_playfield(x, y)
                        && others.iter().all(|&o| dist2(o, (x, y)) > 10 * 10)
                    {
                        black.push((dist2(at, (x, y)), (x, y)));
                    }
                }
            }
            black.sort_unstable();
            let explore = black
                .into_iter()
                .map(|(_, c)| c)
                .filter(|&c| !near_danger(c, SCOUT_SAFE + 4))
                // Black this near is out of the scout's sight from here.
                .filter(|&c| dist2(c, at) > 4 * 4)
                .take(40)
                .find_map(|c| {
                    std::iter::once((0, 0))
                        .chain(DIRS.iter().map(|&(dx, dy)| (dx * 4 / 100, dy * 4 / 100)))
                        .filter_map(|(dx, dy)| {
                            let x = i32::from(c.0) + dx;
                            let y = i32::from(c.1) + dy;
                            (x > 0 && y > 0 && x < i32::from(w) - 1 && y < i32::from(h) - 1)
                                .then_some((x as u16, y as u16))
                        })
                        .find(|&s| {
                            view.in_playfield(s.0, s.1)
                                && way_clear(at, s)
                                && view.can_reach(mz, at, s)
                        })
                });
            let goal = explore.or_else(|| {
                // All safe black explored: this scout's station.
                if stations.is_empty() {
                    stations = scout_stations((w, h), self.ids.len(), home, |c| {
                        !near_danger(c, SCOUT_SAFE + 4) && view.can_reach(mz, at, c)
                    });
                }
                stations.get(n).copied().filter(|&c| way_clear(at, c))
            });
            match goal {
                Some(c) if current.is_none_or(|(g, _)| g != c) => {
                    self.set_goal(id, c, frame, orders);
                }
                Some(_) => {}
                // Nowhere safe to go: wait where it is.
                None => {}
            }
        }
    }

    fn set_goal(&mut self, id: u64, goal: (u16, u16), frame: u32, orders: &mut Vec<Command>) {
        self.goals.retain(|g| g.0 != id);
        self.goals.push((id, goal, frame));
        orders.push(Command::Move {
            entity_id: id,
            target_rx: goal.0,
            target_ry: goal.1,
        });
    }
}

/// `n` waiting places spread over the map for crate scouts: candidates on
/// an 8-cell grid that `ok` accepts, the first nearest the map's middle,
/// each next the one farthest from those taken and from `home` (whose own
/// units are near crates there anyway).
fn scout_stations(
    (w, h): (u16, u16),
    n: usize,
    home: (u16, u16),
    ok: impl Fn((u16, u16)) -> bool,
) -> Vec<(u16, u16)> {
    let mut candidates = Vec::new();
    for y in (4..h.saturating_sub(4)).step_by(8) {
        for x in (4..w.saturating_sub(4)).step_by(8) {
            if ok((x, y)) {
                candidates.push((x, y));
            }
        }
    }
    let mut out: Vec<(u16, u16)> = Vec::new();
    let middle = (w / 2, h / 2);
    while out.len() < n {
        let next = candidates
            .iter()
            .copied()
            .filter(|c| !out.contains(c))
            .max_by_key(|&c| {
                if out.is_empty() {
                    (-dist2(c, middle), c)
                } else {
                    let spread = out
                        .iter()
                        .chain(std::iter::once(&home))
                        .map(|&o| dist2(o, c))
                        .min()
                        .unwrap_or(0);
                    (spread, c)
                }
            });
        let Some(c) = next else { break };
        out.push(c);
    }
    out
}

fn mix(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^ (x >> 16)
}

/// Put MCVs beyond the bot's own one (`primary`; a crate's free unit or
/// `FreeMCV=`, a spare) to use: each deploys into another Construction Yard
/// near where it stands, after driving to within 20 cells of `home` if
/// farther, and away from seen armed enemies.
pub(crate) fn deploy_spare_mcvs(
    view: &BotView<'_>,
    primary: Option<u64>,
    home: (u16, u16),
    think_every: u32,
    orders: &mut Vec<Command>,
) {
    if view.frame() % 90 >= think_every {
        return;
    }
    let is_mcv = |e: &crate::sim::game_entity::GameEntity| {
        view.object_type(e.type_ref()).is_some_and(|t| {
            t.deploys_into
                .as_deref()
                .is_some_and(|into| view.rules.object(into).is_some_and(|b| b.construction_yard))
        })
    };
    let armed: Vec<(u16, u16)> = view
        .enemy_objects()
        .filter(|e| {
            !is_structure(e)
                && view
                    .object_type(e.type_ref())
                    .is_some_and(|t| t.weapon_list.iter().any(Option::is_some))
        })
        .map(cell)
        .collect();
    for mcv in view.own().filter(|e| !is_structure(e) && is_mcv(e)) {
        let id = mcv.stable_id();
        // A deploy order is the Unload mission; the MCV turns and unfolds
        // under it, and any order meanwhile would cancel it.
        let deploying =
            mcv.mission.effective().known() == Some(crate::sim::mission::MissionType::Unload);
        if Some(id) == primary || deploying || mcv.navigation.nav_com().is_some() {
            continue;
        }
        let at = cell(mcv);
        let Some(mz) = view.object_type(mcv.type_ref()).map(|t| t.movement_zone) else {
            continue;
        };
        let threatened = armed.iter().any(|&e| dist2(e, at) <= 10 * 10);
        if threatened || dist2(at, home) > 20 * 20 {
            let to = toward(home, at, 12);
            orders.push(Command::Move {
                entity_id: id,
                target_rx: to.0,
                target_ry: to.1,
            });
            continue;
        }
        // Rings out to 16 cells: next to a dense base or on a crate's spot
        // between cliffs the nearest free 4x4 can be well away.
        let site = (0..=16i32).find_map(|ring| {
            let mut sites: Vec<(u16, u16)> = Vec::new();
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs() != ring && dy.abs() != ring {
                        continue;
                    }
                    let (x, y) = (i32::from(at.0) + dx, i32::from(at.1) + dy);
                    if x < 2 || y < 2 {
                        continue;
                    }
                    let c = (x as u16, y as u16);
                    if view.can_deploy_at(mcv, c.0, c.1) && view.can_reach(mz, at, c) {
                        sites.push(c);
                    }
                }
            }
            sites.into_iter().min_by_key(|&c| (dist2(c, at), c))
        });
        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "spare mcv f{} {id} {} at {at:?} site {site:?} primary {primary:?} mission {:?}",
                view.frame(),
                view.name(mcv.type_ref()),
                mcv.mission.effective().known()
            );
        }
        match site {
            Some(c) if c == at => orders.push(Command::DeployMcv { entity_id: id }),
            Some(c) => orders.push(Command::Move {
                entity_id: id,
                target_rx: c.0,
                target_ry: c.1,
            }),
            None => {
                // Nowhere near: try again from closer to home.
                let to = toward(home, at, 10);
                orders.push(Command::Move {
                    entity_id: id,
                    target_rx: to.0,
                    target_ry: to.1,
                });
            }
        }
    }
}

/// Slave Miners (any miner that deploys into a refinery), saved with the
/// bot: an undeployed one drives to the richest explored ore patch near
/// home that no refinery serves, outside seen enemies' reach, and deploys
/// there (`TechnoClass` deploy, the Unload mission). playtest, 2026-11-11: "our
/// ais need to use harvesters of all races when received from a crate";
/// ordinary miners of every side go to ore through [`tend_miners`], a crate's
/// Slave Miner sat idle with every side but a Yuri bot that built its own.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlaveMiners {
    /// Each miner's ore goal and when it was chosen.
    goals: Vec<(u64, (u16, u16), u32)>,
    /// Deploy orders sent (miner, frame): one per 150 frames.
    deploys: Vec<(u64, u32)>,
}

impl SlaveMiners {
    /// Miners on their way to an ore goal.
    pub(crate) fn heading(&self) -> usize {
        self.goals.len()
    }

    pub(crate) fn run(
        &mut self,
        view: &BotView<'_>,
        home: (u16, u16),
        think_every: u32,
        orders: &mut Vec<Command>,
    ) {
        use crate::sim::mission::MissionType as M;
        let frame = view.frame();
        if frame % 90 >= think_every {
            return;
        }
        let mut refineries = Vec::new();
        let mut miners = Vec::new();
        for e in view.own() {
            let Some(t) = view.object_type(e.type_ref()) else {
                continue;
            };
            if is_structure(e) {
                if t.refinery {
                    refineries.push(cell(e));
                }
            } else if (t.harvester || t.resource_gatherer)
                && t.deploys_into
                    .as_deref()
                    .and_then(|b| view.rules.object(b))
                    .is_some_and(|b| b.refinery)
            {
                let busy = e.navigation.nav_com().is_some()
                    || e.mission.effective().known() == Some(M::Unload);
                miners.push((e.stable_id(), cell(e), t.movement_zone, busy));
            }
        }
        self.goals.retain(|g| miners.iter().any(|m| m.0 == g.0));
        self.deploys
            .retain(|&(id, at)| frame.saturating_sub(at) < 150 && miners.iter().any(|m| m.0 == id));
        if miners.is_empty() {
            return;
        }
        let danger = Danger::seen(view, 3);
        let (w, h) = view.map_size();
        let scan = |r: i32| {
            let mut ore = Vec::new();
            for y in (i32::from(home.1) - r..=i32::from(home.1) + r).step_by(2) {
                for x in (i32::from(home.0) - r..=i32::from(home.0) + r).step_by(2) {
                    if x < 1 || y < 1 || x >= i32::from(w) || y >= i32::from(h) {
                        continue;
                    }
                    let c = (x as u16, y as u16);
                    if view.has_ore(c.0, c.1) && !danger.at(c) {
                        ore.push(c);
                    }
                }
            }
            ore
        };
        let near = scan(40);
        // Nothing free near home (a crate's miner on a small or mined-out
        // patch sat idle): any explored ore on the map.
        let mut far: Option<Vec<(u16, u16)>> = None;
        for (id, at, mz, busy) in miners {
            let goal = match self.goals.iter().find(|g| g.0 == id) {
                Some(&(_, goal, since))
                    if frame.saturating_sub(since) < 2700 && !danger.at(goal) =>
                {
                    goal
                }
                _ => {
                    let taken: Vec<(u16, u16)> = refineries
                        .iter()
                        .copied()
                        .chain(self.goals.iter().filter(|g| g.0 != id).map(|g| g.1))
                        .collect();
                    let best = |ore: &[(u16, u16)]| {
                        ore.iter()
                            .copied()
                            .filter(|&o| taken.iter().all(|&t| dist2(t, o) > 8 * 8))
                            .filter(|&o| view.can_reach(mz, at, o) && !danger.on_way(at, o))
                            .map(|o| {
                                let n = ore.iter().filter(|&&p| dist2(p, o) <= 4 * 4).count();
                                let score = n as i32 * 3
                                    - dist2(o, home).isqrt() / 2
                                    - dist2(o, at).isqrt() / 2;
                                (score, std::cmp::Reverse(o))
                            })
                            .max()
                            .map(|(_, std::cmp::Reverse(o))| o)
                    };
                    let goal = best(&near).or_else(|| {
                        let far = far.get_or_insert_with(|| scan(i32::from(w.max(h))));
                        best(far)
                    });
                    let Some(goal) = goal else {
                        continue;
                    };
                    self.goals.retain(|g| g.0 != id);
                    self.goals.push((id, goal, frame));
                    goal
                }
            };
            if busy {
                continue;
            }
            if log::log_enabled!(log::Level::Debug) {
                log::debug!("slave miner f{frame} {id} at {at:?} goal {goal:?}");
            }
            if dist2(at, goal) > 3 * 3 {
                orders.push(Command::Move {
                    entity_id: id,
                    target_rx: goal.0,
                    target_ry: goal.1,
                });
            } else if view
                .entity(id)
                .is_some_and(|e| view.can_deploy_at(e, at.0, at.1))
            {
                if !self.deploys.iter().any(|d| d.0 == id) {
                    self.deploys.push((id, frame));
                    orders.push(Command::DeployMcv { entity_id: id });
                }
            } else {
                // A free spot nearby, by the frame and id (no RNG draw).
                let r = mix(frame ^ (id as u32).rotate_left(11));
                let dx = (r % 7) as i32 - 3;
                let dy = ((r >> 8) % 7) as i32 - 3;
                orders.push(Command::Move {
                    entity_id: id,
                    target_rx: (i32::from(at.0) + dx).max(1) as u16,
                    target_ry: (i32::from(at.1) + dy).max(1) as u16,
                });
            }
        }
    }
}

/// A Grinder at most this far (cells) from a Mastermind's spare captive
/// takes it; farther, the captive is shot.
const GRIND_RANGE: i32 = 35;

/// A Mastermind (`InfiniteMindControl=`) holding more captives than its
/// weapon's `Damage=` overloads and hurts itself (`CaptureManagerClass`
/// overload, `[CombatDamage] OverloadCount=`). playtest, 2026-11-12: past 3, the
/// bots get rid of the extra captives, into a Grinder when one is near
/// enough (its refund), else shot by own units nearby. The most valuable
/// captives stay. `busy` (captive, frame) keeps the order from repeating
/// every think; returns the captives being disposed of, which the bot gives
/// no other order.
pub(crate) fn mastermind_overload(
    view: &BotView<'_>,
    think_every: u32,
    busy: &mut Vec<(u64, u32)>,
    orders: &mut Vec<Command>,
) -> Vec<u64> {
    let frame = view.frame();
    busy.retain(|&(id, at)| {
        frame.saturating_sub(at) < 600 && view.entity(id).is_some_and(|e| e.owner() == view.house)
    });
    if frame % 15 >= think_every {
        return busy.iter().map(|b| b.0).collect();
    }
    let mut spare: Vec<u64> = Vec::new();
    for e in view.own() {
        let Some(manager) = e.capture_manager.as_deref() else {
            continue;
        };
        if !manager.is_infinite() || manager.count() <= manager.max_control() {
            continue;
        }
        let mut captives: Vec<(i32, u64)> = manager
            .victims()
            .filter_map(|id| {
                let v = view.entity(id).filter(|v| v.owner() == view.house)?;
                let cost = view.object_type(v.type_ref()).map_or(0, |t| t.cost);
                Some((cost, id))
            })
            .collect();
        captives.sort_unstable_by(|a, b| b.cmp(a));
        let keep = usize::try_from(manager.max_control().max(0)).unwrap_or(0);
        spare.extend(captives.into_iter().skip(keep).map(|c| c.1));
    }
    let grinders: Vec<(u64, (u16, u16))> = view
        .own()
        .filter(|e| is_structure(e) && view.object_type(e.type_ref()).is_some_and(|t| t.grinding))
        .map(|e| (e.stable_id(), cell(e)))
        .collect();
    for id in spare {
        if busy.iter().any(|b| b.0 == id) {
            continue;
        }
        let Some(victim) = view.entity(id) else {
            continue;
        };
        let at = cell(victim);
        let flies = victim.category() == EntityCategory::Aircraft
            || view
                .object_type(victim.type_ref())
                .is_some_and(|t| t.considered_aircraft);
        let grinder = grinders
            .iter()
            .filter(|_| !flies)
            .filter(|(_, g)| dist2(*g, at) <= GRIND_RANGE * GRIND_RANGE)
            .min_by_key(|(gid, g)| (dist2(*g, at), *gid));
        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "mastermind f{frame} spare captive {id} {} at {at:?} grinder {:?}",
                view.name(victim.type_ref()),
                grinder.map(|g| g.1)
            );
        }
        if let Some(&(grinder, _)) = grinder {
            busy.push((id, frame));
            orders.push(Command::MoveToObject {
                entity_id: id,
                target_id: grinder,
            });
            continue;
        }
        // Shot by the three nearest armed own units that are no captives.
        let mut shooters: Vec<(i32, u64)> = view
            .own()
            .filter(|e| {
                !is_structure(e)
                    && e.stable_id() != id
                    && !e.seldom.mind_control.is_mind_controlled()
                    && e.capture_manager.is_none()
                    && view
                        .object_type(e.type_ref())
                        .is_some_and(|t| t.weapon_list.iter().any(Option::is_some) && !t.engineer)
            })
            .map(|e| (dist2(cell(e), at), e.stable_id()))
            .filter(|&(d, _)| d <= 15 * 15)
            .collect();
        shooters.sort_unstable();
        if shooters.is_empty() {
            continue;
        }
        busy.push((id, frame));
        orders.push(Command::Stop { entity_id: id });
        for &(_, shooter) in shooters.iter().take(3) {
            orders.push(Command::Attack {
                attacker_id: shooter,
                target_id: id,
            });
        }
    }
    busy.iter().map(|b| b.0).collect()
}

/// Tech buildings farther than this (cells) from home count double the
/// distance when engineers pick their next one (all are taken in the end).
const TECH_REACH: i32 = 40;

/// Neutral tech buildings (oil derricks, hospitals, airports ..) and the
/// bot's own derricks, saved with the bot (playtest, 2026-11-12: "our ais should
/// capture and defend oil derricks"; "attack enemy oil derricks but keep
/// clear when they explode").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TechBuildings {
    /// Engineer -> the building it was sent to capture.
    sent: Vec<(u64, u64)>,
    /// Units sent at an enemy derrick or at enemies near an own one, and
    /// when (no repeated order within 10 seconds).
    ordered: Vec<(u64, u32)>,
    /// Enemy derricks shot at: since when, and the frame until which one
    /// that held out for a minute is left alone.
    #[serde(default)]
    shot_at: Vec<(u64, u32, u32)>,
}

impl TechBuildings {
    /// Units on a derrick errand.
    pub(crate) fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.ordered.iter().map(|o| o.0)
    }

    /// Whether `t` is an oil derrick (`ProduceCashAmount=`, a neutral tech
    /// building that pays its owner).
    fn derrick(t: &crate::rules::object_type::ObjectType) -> bool {
        t.produce_cash_amount > 0 && !t.construction_yard
    }

    /// Every 48 frames: engineers capture the neutral tech buildings within
    /// [`TECH_REACH`] cells of home whose cell and way are outside seen
    /// enemies' reach (one engineer each, at most two engineers built
    /// for it); own derricks with armed enemies near are defended by the
    /// nearest `free` units; enemy derricks within reach of `free` units
    /// are shot from at least [`DERRICK_SAFE`] cells, by units that reach
    /// that far; a derrick about to blow (any owner, under a third of its
    /// strength) is left by every own unit within [`DERRICK_SAFE`] cells.
    /// Returns the units it ordered.
    pub(crate) fn run(
        &mut self,
        view: &BotView<'_>,
        home: (u16, u16),
        think_every: u32,
        free: &[(u64, (u16, u16))],
        options: &[BuildOption],
        orders: &mut Vec<Command>,
    ) -> Vec<u64> {
        let frame = view.frame();
        self.ordered
            .retain(|&(id, at)| frame.saturating_sub(at) < 150 && view.entity(id).is_some());
        let mut used: Vec<u64> = self.ordered.iter().map(|o| o.0).collect();
        if frame % 48 >= think_every {
            return used;
        }
        let danger = Danger::seen(view, 2);

        // Capture.
        // Playtest, 2026-11-14: "constantly try to capture every oil derrick on
        // the map that isn't yours without moving engineers into enemy
        // fire"; later "keep capturing neutral airports (in fact, try to
        // capture all tech buildings)": every tech building (`TechLevel=-1`
        // ones nobody builds) anywhere, neutral or an enemy's, that is
        // not in seen enemies' reach; nearer first, a derrick or one with
        // a superweapon (the airport's paratroopers) counting a quarter of
        // its distance, one beyond [`TECH_REACH`] double.
        let neutrals: Vec<u64> = view.neutral_structures().map(|e| e.stable_id()).collect();
        let mut tech: Vec<(i32, u64)> = view
            .visible_objects()
            .filter(|e| is_structure(e))
            .filter_map(|e| {
                let t = view.object_type(e.type_ref())?;
                if !(t.capturable && t.needs_engineer && t.cost >= 0) {
                    return None;
                }
                let neutral = neutrals.contains(&e.stable_id());
                let tech = Self::derrick(t) || t.tech_level < 0;
                if !tech || !(neutral || view.is_enemy(e.owner())) {
                    return None;
                }
                let c = cell(e);
                let d = dist2(c, home);
                if danger.at(c) {
                    return None;
                }
                let prized = Self::derrick(t) || t.super_weapon.is_some();
                let d = if d > TECH_REACH * TECH_REACH {
                    d * 2
                } else {
                    d
                };
                Some((if prized { d / 4 } else { d }, e.stable_id()))
            })
            .collect();
        tech.sort_unstable();
        let engineers: Vec<(u64, (u16, u16))> = view
            .own()
            .filter(|e| {
                !is_structure(e) && view.object_type(e.type_ref()).is_some_and(|t| t.engineer)
            })
            .map(|e| (e.stable_id(), cell(e)))
            .collect();
        self.sent.retain(|(eng, b)| {
            engineers.iter().any(|e| e.0 == *eng) && tech.iter().any(|t| t.1 == *b)
        });
        for &(eng, at) in &engineers {
            if self.sent.iter().any(|(e, _)| *e == eng) {
                continue;
            }
            let Some(&(_, target)) = tech.iter().find(|(_, t)| {
                !self.sent.iter().any(|(_, b)| b == t)
                    && view.entity(*t).is_some_and(|b| !danger.on_way(at, cell(b)))
            }) else {
                break;
            };
            if log::log_enabled!(log::Level::Debug) {
                log::debug!("tech f{frame} engineer {eng} captures {target}");
            }
            self.sent.push((eng, target));
            orders.push(Command::CaptureBuilding {
                engineer_id: eng,
                target_building_id: target,
            });
        }
        let free_tech = tech
            .iter()
            .filter(|(_, t)| !self.sent.iter().any(|(_, b)| b == t))
            .count();
        let busy = view
            .factory(ProductionCategory::Infantry)
            .is_some_and(|f| f.object.is_some() || !f.queue.is_empty());
        let most = if view.credits() >= 3000 { 4 } else { 3 };
        if free_tech > 0 && engineers.len() < free_tech.min(most) && !busy && view.credits() >= 600
        {
            if let Some(engineer) = options.iter().find(|o| {
                o.enabled
                    && o.queue_category == ProductionCategory::Infantry
                    && view.object_type(o.type_id).is_some_and(|t| t.engineer)
            }) {
                orders.push(Command::QueueProduction {
                    type_id: engineer.type_id,
                });
            }
        }

        // Every derrick in sight: own, enemy, about to blow.
        let mut own_derricks = Vec::new();
        let mut enemy_derricks = Vec::new();
        let mut blowing = Vec::new();
        for e in view.visible_objects().filter(|e| is_structure(e)) {
            let Some(t) = view.object_type(e.type_ref()) else {
                continue;
            };
            if !Self::derrick(t) {
                continue;
            }
            let c = cell(e);
            if e.health.current() * 3 < t.strength {
                blowing.push(c);
            }
            if e.owner() == view.house {
                own_derricks.push(c);
            } else if view.is_enemy(e.owner()) {
                enemy_derricks.push((e.stable_id(), c, e.health.current() * 3 < t.strength));
            }
        }
        let unit_reach = |id: u64| -> i32 {
            view.entity(id)
                .and_then(|e| view.object_type(e.type_ref()))
                .and_then(|t| {
                    t.weapon_list
                        .iter()
                        .flatten()
                        .filter_map(|w| view.rules.weapon(w))
                        .map(|w| (w.range_leptons + 255) / 256)
                        .max()
                })
                .unwrap_or(0)
        };

        // Clear of a derrick about to blow.
        for &c in &blowing {
            for e in view.own().filter(|e| !is_structure(e)) {
                let at = cell(e);
                if dist2(at, c) > DERRICK_SAFE * DERRICK_SAFE || used.contains(&e.stable_id()) {
                    continue;
                }
                let away = away_from(at, c, DERRICK_SAFE + 3, view.map_size());
                if log::log_enabled!(log::Level::Debug) {
                    log::debug!("derrick f{frame} {} steps away from {c:?}", e.stable_id());
                }
                used.push(e.stable_id());
                self.ordered.push((e.stable_id(), frame));
                orders.push(Command::Move {
                    entity_id: e.stable_id(),
                    target_rx: away.0,
                    target_ry: away.1,
                });
            }
        }

        // Defend own derricks: armed enemies within 8 cells of one.
        for &d in &own_derricks {
            let Some(enemy) = view
                .enemy_objects()
                .filter(|e| {
                    !is_structure(e)
                        && view
                            .object_type(e.type_ref())
                            .is_some_and(|t| t.weapon_list.iter().any(Option::is_some))
                })
                .filter(|e| dist2(cell(e), d) <= 8 * 8)
                .min_by_key(|e| (dist2(cell(e), d), e.stable_id()))
            else {
                continue;
            };
            let mut near: Vec<(i32, u64)> = free
                .iter()
                .filter(|(id, at)| !used.contains(id) && dist2(*at, d) <= 30 * 30)
                .map(|&(id, at)| (dist2(at, d), id))
                .collect();
            near.sort_unstable();
            if log::log_enabled!(log::Level::Debug) && !near.is_empty() {
                log::debug!("derrick f{frame} defends own derrick at {d:?}");
            }
            for &(_, id) in near.iter().take(4) {
                used.push(id);
                self.ordered.push((id, frame));
                orders.push(Command::Attack {
                    attacker_id: id,
                    target_id: enemy.stable_id(),
                });
            }
        }

        // Shoot enemy derricks from afar (not one about to blow: it goes
        // by itself, and the units stand clear).
        self.shot_at.retain(|&(t, _, until)| {
            frame < until.max(1) || enemy_derricks.iter().any(|d| d.0 == t)
        });
        for &(target, c, low) in &enemy_derricks {
            // Not one an engineer goes to take.
            if low || danger.at(c) || self.sent.iter().any(|s| s.1 == target) {
                continue;
            }
            match self.shot_at.iter_mut().find(|s| s.0 == target) {
                Some(s) if frame < s.2 => continue,
                Some(s) if frame.saturating_sub(s.1) > 45 * 60 => {
                    // Held out for a minute: leave it for three.
                    s.1 = frame + 3 * 45 * 60;
                    s.2 = s.1;
                    continue;
                }
                Some(_) => {}
                None => self.shot_at.push((target, frame, 0)),
            }
            for &(id, at) in free {
                if used.contains(&id) || dist2(at, c) > 12 * 12 {
                    continue;
                }
                if unit_reach(id) < DERRICK_SAFE + 1 {
                    continue;
                }
                if log::log_enabled!(log::Level::Debug) {
                    log::debug!("derrick f{frame} {id} shoots enemy derrick at {c:?}");
                }
                used.push(id);
                self.ordered.push((id, frame));
                orders.push(Command::Attack {
                    attacker_id: id,
                    target_id: target,
                });
            }
        }
        used
    }
}

/// Cells kept from a derrick about to blow (its death explosion).
const DERRICK_SAFE: i32 = 5;

/// The cell `dist` cells from `at` straight away from `from`.
fn away_from(at: (u16, u16), from: (u16, u16), dist: i32, (w, h): (u16, u16)) -> (u16, u16) {
    let (dx, dy) = (
        i32::from(at.0) - i32::from(from.0),
        i32::from(at.1) - i32::from(from.1),
    );
    let (dx, dy) = if dx == 0 && dy == 0 { (1, 1) } else { (dx, dy) };
    let len = (dx * dx + dy * dy).isqrt().max(1);
    (
        (i32::from(from.0) + dx * dist / len).clamp(1, i32::from(w) - 2) as u16,
        (i32::from(from.1) + dy * dist / len).clamp(1, i32::from(h) - 2) as u16,
    )
}

/// Neutral buildings garrisoned, saved with the bot (playtest, 2026-11-11: "our
/// ais should try to garrison neutral buildings"; 2026-11-14: "garrison more
/// and more buildings if there are any. few GIs per building until all of
/// them are taken, then add more to those that ever attacked something.
/// don't get killed while going to the buildings").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Garrisons {
    /// Infantryman -> the building it walks into, and when it was sent.
    sent: Vec<(u64, u64, u32)>,
    /// Own garrisons that have had a target.
    #[serde(default)]
    fought: Vec<u64>,
    /// Occupier infantry the next round could use (for production).
    #[serde(default)]
    wanted: usize,
    /// Where paratroopers were dropped for garrisons, and when: buildings
    /// near one count as in reach for [`PARADROP_FRAMES`].
    #[serde(default)]
    drops: Vec<((u16, u16), u32)>,
}

/// How long a paradrop's buildings stay in the garrisons' reach.
const PARADROP_FRAMES: u32 = 2 * 45 * 60;
/// Cells around a paradrop whose empty buildings its soldiers take.
const PARADROP_SPREAD: i32 = 10;

/// Neutral garrisonable buildings within this many cells of home at the
/// start, growing by [`GARRISON_REACH_PER_MINUTE`] up to [`GARRISON_REACH_MAX`].
const GARRISON_RANGE: i32 = 25;
const GARRISON_REACH_PER_MINUTE: i32 = 4;
const GARRISON_REACH_MAX: i32 = 90;
/// Occupants an empty building gets first.
const GARRISON_FIRST: usize = 2;

impl Garrisons {
    /// Infantry walking into a building.
    pub(crate) fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.sent.iter().map(|s| s.0)
    }

    /// More occupier infantry the garrisons could take now (the bots build
    /// that many beyond their own soldiers).
    pub(crate) fn wanted(&self) -> usize {
        self.wanted
    }

    /// Where a paradrop (the tech airport's `ParaDropSpecial`, playtest,
    /// 2026-11-14: "use the paratroopers for garrison buildings") should
    /// land: the empty, intact, garrisonable neutral building anywhere on
    /// the map outside seen enemies' reach with the most such buildings
    /// within [`PARADROP_SPREAD`] cells, then nearest home. Remembered, so
    /// [`Self::run`] sends the soldiers that land into them.
    pub(crate) fn paradrop_target(
        &mut self,
        view: &BotView<'_>,
        home: (u16, u16),
    ) -> Option<(u16, u16)> {
        let danger = Danger::seen(view, 2);
        let empty: Vec<(u16, u16)> = view
            .neutral_structures()
            .filter(|e| {
                view.object_type(e.type_ref()).is_some_and(|t| {
                    t.can_be_occupied
                        && t.max_number_occupants > 0
                        && e.passenger_role().occupants().is_empty()
                        && e.health.current() * 2 >= t.strength
                })
            })
            .map(cell)
            .filter(|&c| !danger.at(c))
            .collect();
        let near = |c: (u16, u16)| {
            empty
                .iter()
                .filter(|&&o| dist2(o, c) <= PARADROP_SPREAD * PARADROP_SPREAD)
                .count()
        };
        let at = empty
            .iter()
            .copied()
            .max_by_key(|&c| (near(c), -dist2(c, home), c))?;
        self.drops.push((at, view.frame()));
        Some(at)
    }

    /// Every 90 frames: the empty civilian buildings that take occupants
    /// (`CanBeOccupied=`) within a reach that grows with the match, nearest
    /// home first, outside seen enemies' reach, get [`GARRISON_FIRST`] of
    /// the bot's spare occupier infantry (`Occupier=`: GI, Conscript,
    /// Initiate) from `free`, by the Capture click
    /// (`InfantryClass::PerCellProcess` garrisons on arrival). Once none is
    /// left empty, the own garrisons that have fired are filled up. Nobody
    /// is sent along a way through seen enemies' reach. Returns the
    /// infantry on its way in.
    pub(crate) fn run(
        &mut self,
        view: &BotView<'_>,
        home: (u16, u16),
        think_every: u32,
        free: &[(u64, (u16, u16))],
        orders: &mut Vec<Command>,
    ) -> Vec<u64> {
        let frame = view.frame();
        // Once inside, the infantryman is in no unit list: dropped here
        // after a minute either way.
        self.sent.retain(|&(id, b, at)| {
            frame.saturating_sub(at) < 45 * 60
                && view
                    .entity(id)
                    .is_some_and(|e| e.owner() == view.house && !e.in_limbo())
                && view.entity(b).is_some_and(|b| {
                    view.object_type(b.type_ref())
                        .is_none_or(|t| b.health.current() * 2 >= t.strength)
                })
        });
        let walking: Vec<u64> = self.sent.iter().map(|s| s.0).collect();
        if frame % 90 >= think_every {
            return walking;
        }
        let held: Vec<&crate::sim::game_entity::GameEntity> = view
            .own()
            .filter(|e| is_structure(e) && !e.passenger_role().occupants().is_empty())
            .collect();
        for e in &held {
            if e.attack_target.target().is_some() && !self.fought.contains(&e.stable_id()) {
                self.fought.push(e.stable_id());
            }
        }
        self.fought
            .retain(|&b| held.iter().any(|e| e.stable_id() == b));
        let danger = Danger::seen(view, 2);
        let minutes = (frame / MINUTE) as i32;
        let reach = (GARRISON_RANGE + GARRISON_REACH_PER_MINUTE * minutes).min(GARRISON_REACH_MAX);
        self.drops
            .retain(|&(_, at)| frame.saturating_sub(at) < PARADROP_FRAMES);
        let drops = self.drops.clone();
        let in_reach = |c: (u16, u16)| {
            dist2(c, home) <= reach * reach
                || drops
                    .iter()
                    .any(|&(d, _)| dist2(d, c) <= PARADROP_SPREAD * PARADROP_SPREAD)
        };
        let taken: Vec<u64> = self.sent.iter().map(|s| s.1).collect();
        let incoming = |b: u64| taken.iter().filter(|&&t| t == b).count();
        // (distance, building, cell, places left)
        let mut targets: Vec<(i32, u64, (u16, u16), usize)> = view
            .neutral_structures()
            .filter_map(|e| {
                let t = view.object_type(e.type_ref())?;
                let c = cell(e);
                (t.can_be_occupied
                    && t.max_number_occupants > 0
                    && e.passenger_role().occupants().is_empty()
                    // Below half strength nobody can enter (playtest, 2026-11-14).
                    && e.health.current() * 2 >= t.strength
                    && in_reach(c)
                    && !danger.at(c))
                .then(|| {
                    let left = GARRISON_FIRST
                        .min(t.max_number_occupants as usize)
                        .saturating_sub(incoming(e.stable_id()));
                    (dist2(c, home), e.stable_id(), c, left)
                })
            })
            .filter(|t| t.3 > 0)
            .collect();
        if targets.is_empty() {
            // Every building in reach is taken: the ones that fought fill up.
            targets = held
                .iter()
                .filter(|e| self.fought.contains(&e.stable_id()))
                .filter_map(|e| {
                    let t = view.object_type(e.type_ref())?;
                    let c = cell(e);
                    let inside = e.passenger_role().occupants().count().max(0) as usize;
                    let left = (t.max_number_occupants.max(0) as usize)
                        .saturating_sub(inside + incoming(e.stable_id()));
                    (left > 0 && !danger.at(c)).then(|| (dist2(c, home), e.stable_id(), c, left))
                })
                .collect();
        }
        targets.sort_unstable();
        self.wanted = targets.iter().map(|t| t.3).sum();
        let mut infantry: Vec<(u64, (u16, u16))> = free
            .iter()
            .copied()
            .filter(|(id, _)| !walking.contains(id))
            // A deployed GI on a post refuses the Capture click.
            .filter(|(id, _)| {
                view.entity(*id).is_some_and(|e| {
                    !e.is_deployed() && view.object_type(e.type_ref()).is_some_and(|t| t.occupier)
                })
            })
            .collect();
        let mut out = walking;
        for (_, building, c, left) in targets {
            infantry.sort_by_key(|&(id, at)| (dist2(at, c), id));
            let safe: Vec<(u64, (u16, u16))> = infantry
                .iter()
                .copied()
                .filter(|&(_, at)| !danger.on_way(at, c))
                .take(left)
                .collect();
            if safe.is_empty() {
                continue;
            }
            infantry.retain(|i| !safe.contains(i));
            for (id, _) in safe {
                if log::log_enabled!(log::Level::Debug) {
                    log::debug!("garrison f{frame} {id} into {building} at {c:?}");
                }
                self.sent.push((id, building, frame));
                self.wanted = self.wanted.saturating_sub(1);
                out.push(id);
                orders.push(Command::CaptureBuilding {
                    engineer_id: id,
                    target_building_id: building,
                });
            }
        }
        out
    }
}

/// The cell `dist` cells from `from` toward `to` (or `to` when nearer).
fn toward(from: (u16, u16), to: (u16, u16), dist: i32) -> (u16, u16) {
    let dx = i32::from(to.0) - i32::from(from.0);
    let dy = i32::from(to.1) - i32::from(from.1);
    let d = (dx * dx + dy * dy).isqrt();
    if d <= dist || d == 0 {
        return to;
    }
    (
        (i32::from(from.0) + dx * dist / d).max(1) as u16,
        (i32::from(from.1) + dy * dist / d).max(1) as u16,
    )
}

/// Spies alive at once.
const SPIES: usize = 2;
/// Spies built in all.
const SPIES_BUILT: u32 = 8;
/// A seen dog (or any `DetectDisguise=` object) this close to a spy's way or
/// target calls it off: dogs see through the disguise and kill spies.
const SPY_DOG_SAFE: i32 = 9;

/// A bot's spies, saved with it: from minute 4, up to [`SPIES`] `Agent=`
/// infantry walk into seen enemy buildings (the Capture click,
/// `BuildingClass::Infiltrate @ 0x004571E0`): the war factory and the
/// barracks once each (veteran vehicles / infantry), then refineries
/// (stolen money) and power plants (blackout). Never past a seen dog: a
/// target or way within [`SPY_DOG_SAFE`] cells of one is skipped, and a
/// spy with a dog that close steps away.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Spies {
    ids: Vec<u64>,
    /// Each spy's target building, where the spy last stood, and when it
    /// was ordered.
    goals: Vec<(u64, u64, (u16, u16), u32)>,
    /// The war factory (1) and barracks (2) arms done.
    done: Vec<u8>,
    built: u32,
    /// When each spy was first seen (it waits for an enemy soldier to
    /// copy for at most 3 minutes).
    #[serde(default)]
    born: Vec<(u64, u32)>,
}

impl Spies {
    fn is_spy(view: &BotView<'_>, e: &crate::sim::game_entity::GameEntity) -> bool {
        e.category() == EntityCategory::Infantry
            && view.object_type(e.type_ref()).is_some_and(|t| t.agent)
    }

    /// The spies: the bot gives them no other order.
    pub(crate) fn ids(&self) -> &[u64] {
        &self.ids
    }

    /// With the barracks idle: queue a missing spy. `true` when it did.
    pub(crate) fn produce(
        &mut self,
        view: &BotView<'_>,
        options: &[BuildOption],
        orders: &mut Vec<Command>,
    ) -> bool {
        if view.frame() < 4 * MINUTE
            || self.built >= SPIES_BUILT
            || view.credits() < 1000
            || view.own().filter(|e| Self::is_spy(view, e)).count() >= SPIES
            || view
                .factory(ProductionCategory::Infantry)
                .is_some_and(|f| f.object.is_some() || !f.queue.is_empty())
        {
            return false;
        }
        let Some(spy) = options.iter().find(|o| {
            o.enabled
                && o.queue_category == ProductionCategory::Infantry
                && view.object_type(o.type_id).is_some_and(|t| t.agent)
        }) else {
            return false;
        };
        self.built += 1;
        orders.push(Command::QueueProduction {
            type_id: spy.type_id,
        });
        true
    }

    /// Send every spy on.
    pub(crate) fn run(&mut self, view: &BotView<'_>, orders: &mut Vec<Command>) {
        let frame = view.frame();
        // A spy gone next to its target went in: a one-time arm is done.
        for &(id, target, at, _) in &self.goals {
            if view.entity(id).is_some() {
                continue;
            }
            let Some(b) = view.entity(target) else {
                continue;
            };
            let Some(t) = view.object_type(b.type_ref()) else {
                continue;
            };
            if dist2(at, cell(b)) <= 4 * 4 {
                use crate::rules::object_type::FactoryType as F;
                match t.factory_type() {
                    Some(F::UnitType) => self.done.push(1),
                    Some(F::InfantryType) => self.done.push(2),
                    _ => {}
                }
            }
        }
        self.ids = view
            .own()
            .filter(|e| Self::is_spy(view, e))
            .map(|e| e.stable_id())
            .collect();
        self.ids.sort_unstable();
        let ids = &self.ids;
        self.goals.retain(|g| ids.contains(&g.0));
        self.born.retain(|b| ids.contains(&b.0));
        if self.ids.is_empty() {
            return;
        }
        let dogs: Vec<(u16, u16)> = view
            .enemy_objects()
            .filter(|e| {
                !is_structure(e)
                    && view
                        .object_type(e.type_ref())
                        .is_some_and(|t| t.detect_disguise)
            })
            .map(cell)
            .collect();
        let near_dog = |c: (u16, u16)| {
            dogs.iter()
                .any(|&d| dist2(d, c) <= SPY_DOG_SAFE * SPY_DOG_SAFE)
        };
        let dog_on_way = |from: (u16, u16), to: (u16, u16)| {
            let dx = i32::from(to.0) - i32::from(from.0);
            let dy = i32::from(to.1) - i32::from(from.1);
            let steps = (dx.abs().max(dy.abs()) / 2).max(1);
            (0..=steps).any(|i| {
                near_dog((
                    (i32::from(from.0) + dx * i / steps).max(0) as u16,
                    (i32::from(from.1) + dy * i / steps).max(0) as u16,
                ))
            })
        };
        // Seen enemy buildings worth a spy, best arm first.
        let mut targets: Vec<(u8, u64, (u16, u16))> = view
            .enemy_objects()
            .filter(|e| is_structure(e))
            .filter_map(|e| {
                let t = view.object_type(e.type_ref())?;
                use crate::rules::object_type::FactoryType as F;
                let rank = match t.factory_type() {
                    Some(F::UnitType) if !self.done.contains(&1) => 0,
                    Some(F::InfantryType) if !self.done.contains(&2) => 1,
                    _ if t.refinery || t.storage > 0 => 2,
                    _ if t.power > 0 => 3,
                    _ => return None,
                };
                Some((rank, e.stable_id(), cell(e)))
            })
            .collect();
        targets.sort_unstable();
        let ids = self.ids.clone();
        for id in ids {
            let Some(spy) = view.entity(id) else { continue };
            let at = cell(spy);
            let goal = self.goals.iter().position(|g| g.0 == id);
            let target_at = goal.and_then(|g| view.entity(self.goals[g].1).map(cell));
            // A dog close: step away from it, unless the door is right here.
            if let Some(&dog) = dogs
                .iter()
                .filter(|&&d| dist2(d, at) <= SPY_DOG_SAFE * SPY_DOG_SAFE)
                .min_by_key(|&&d| dist2(d, at))
                && target_at.is_none_or(|t| dist2(t, at) > 3 * 3)
            {
                let away = |a: u16, d: u16| -> u16 {
                    let step = (i32::from(a) - i32::from(d)).signum() * 10;
                    (i32::from(a) + step).max(1) as u16
                };
                self.goals.retain(|g| g.0 != id);
                orders.push(Command::Move {
                    entity_id: id,
                    target_rx: away(at.0, dog.0),
                    target_ry: away(at.1, dog.1),
                });
                continue;
            }
            // Playtest, 2026-11-14: "a spy that didn't camouflage first". A spy
            // starts disguised as its own side's soldier (`0x006F41FE`),
            // which every enemy shoots: it first takes the look of the
            // nearest seen enemy soldier (its MakesDisguise shot,
            // `InfantryClass::DisguiseAs`), one no dog guards, within 40
            // cells. With none seen it waits, for 3 minutes at most.
            let own_look = spy
                .seldom
                .disguise
                .as_ref()
                .is_none_or(|d| !d.is_disguised() || d.house().is_none_or(|h| h == view.house));
            if own_look {
                let model = view
                    .enemy_objects()
                    .filter(|e| e.category() == EntityCategory::Infantry && !e.in_limbo())
                    .filter(|e| {
                        view.object_type(e.type_ref())
                            .is_some_and(|t| !t.detect_disguise && !t.agent)
                    })
                    .map(|e| (dist2(cell(e), at), e.stable_id(), cell(e)))
                    .filter(|&(d, _, c)| d <= 40 * 40 && !near_dog(c) && !dog_on_way(at, c))
                    .min();
                let born = self.born.iter().find(|b| b.0 == id).map(|b| b.1);
                let born = born.unwrap_or_else(|| {
                    self.born.push((id, frame));
                    frame
                });
                if let Some((_, model, _)) = model {
                    let on_it = spy
                        .attack_target
                        .target()
                        .is_some_and(|t| t == crate::sim::combat::TargetKind::Entity(model));
                    if !on_it {
                        orders.push(Command::Attack {
                            attacker_id: id,
                            target_id: model,
                        });
                    }
                    continue;
                }
                if frame.saturating_sub(born) < 3 * MINUTE {
                    continue;
                }
            }
            if let (Some(g), Some(t)) = (goal, target_at) {
                let (_, target, _, since) = self.goals[g];
                self.goals[g].2 = at;
                if near_dog(t) {
                    self.goals.remove(g);
                } else {
                    let moving = spy.navigation.nav_com().is_some();
                    if !moving && frame.saturating_sub(since) >= 150 {
                        self.goals[g].3 = frame;
                        orders.push(Command::CaptureBuilding {
                            engineer_id: id,
                            target_building_id: target,
                        });
                    }
                    continue;
                }
            }
            // A new target: the best arm, then the nearest; not one another
            // spy goes for.
            let taken: Vec<u64> = self.goals.iter().map(|g| g.1).collect();
            let pick = targets
                .iter()
                .filter(|(_, b, c)| !taken.contains(b) && !near_dog(*c) && !dog_on_way(at, *c))
                .min_by_key(|(rank, b, c)| (*rank, dist2(*c, at), *b));
            if let Some(&(_, building, _)) = pick {
                self.goals.retain(|g| g.0 != id);
                self.goals.push((id, building, at, frame));
                orders.push(Command::CaptureBuilding {
                    engineer_id: id,
                    target_building_id: building,
                });
            }
        }
    }
}

/// Wall segments a bot keeps per own base defense.
const WALLS_PER_DEFENSE: usize = 5;

/// An own base defense (armed `IsBaseDefense=` building, not a wall).
fn is_own_defense(view: &BotView<'_>, e: &crate::sim::game_entity::GameEntity) -> bool {
    is_structure(e)
        && view.object_type(e.type_ref()).is_some_and(|t| {
            t.is_base_defense && !t.wall && t.weapon_list.iter().any(Option::is_some)
        })
}

/// The wall a bot may build now: from minute 5 with $800 spare, while it
/// has fewer than [`WALLS_PER_DEFENSE`] segments a defense, the sidebar's
/// wall (`Wall=yes`) when it can start. The caller queues it when its
/// defenses are otherwise complete.
pub(crate) fn wall_to_build(view: &BotView<'_>, options: &[BuildOption]) -> Option<InternedId> {
    if view.frame() < 5 * MINUTE || view.credits() < 800 {
        return None;
    }
    // Placed walls are overlays: count those around the defenses.
    let (mw, mh) = view.map_size();
    let mut defenses = 0;
    let mut walls = 0;
    for e in view.own().filter(|e| is_own_defense(view, e)) {
        defenses += 1;
        let (x, y) = cell(e);
        let (w, h) = view
            .object_type(e.type_ref())
            .map_or((1, 1), |t| t.foundation.dimensions());
        for ry in y.saturating_sub(1)..=(y + h).min(mh - 1) {
            for rx in x.saturating_sub(1)..=(x + w).min(mw - 1) {
                walls += usize::from(view.own_wall_at(rx, ry));
            }
        }
    }
    if walls >= defenses * WALLS_PER_DEFENSE {
        return None;
    }
    let pick = options
        .iter()
        .find(|o| {
            o.enabled
                && matches!(
                    o.queue_category,
                    ProductionCategory::Defense | ProductionCategory::Building
                )
                && view.object_type(o.type_id).is_some_and(|t| t.wall)
        })
        .map(|o| o.type_id);
    pick
}

/// Where a finished wall segment goes: the ring of cells around an own base
/// defense, on its side facing `enemy`, the defense nearest the enemy
/// first; only cells the placement cursor accepts (it answers only for a
/// finished wall, so call this once one is ready).
pub(crate) fn wall_site(
    view: &BotView<'_>,
    wall: InternedId,
    enemy: (u16, u16),
) -> Option<(u16, u16)> {
    let name = view.name(wall);
    let mut defenses: Vec<((u16, u16), (i32, i32))> = view
        .own()
        .filter(|e| is_own_defense(view, e))
        .filter_map(|e| {
            let (w, h) = view.object_type(e.type_ref())?.foundation.dimensions();
            Some((cell(e), (i32::from(w), i32::from(h))))
        })
        .collect();
    defenses.sort_by_key(|&(at, _)| (dist2(at, enemy), at));
    for ((x, y), (w, h)) in defenses {
        let (x, y) = (i32::from(x), i32::from(y));
        // Doubled centre, so odd sizes stay integral.
        let (cx, cy) = (2 * x + w - 1, 2 * y + h - 1);
        let (ex, ey) = (2 * i32::from(enemy.0) - cx, 2 * i32::from(enemy.1) - cy);
        let mut ring: Vec<(i32, (u16, u16))> = Vec::new();
        for ry in y - 1..=y + h {
            for rx in x - 1..=x + w {
                if (x..x + w).contains(&rx) && (y..y + h).contains(&ry) || rx < 1 || ry < 1 {
                    continue;
                }
                let facing = (2 * rx - cx) * ex + (2 * ry - cy) * ey;
                if facing <= 0 {
                    continue;
                }
                ring.push((-facing, (rx as u16, ry as u16)));
            }
        }
        ring.sort_unstable();
        if let Some(&(_, c)) = ring.iter().find(|(_, c)| view.can_place(name, c.0, c.1)) {
            return Some(c);
        }
    }
    None
}

/// Selling as a player does when the money is gone: no miner left, none
/// being built, and less money than a new one costs. One building a 90
/// frames, the least needed first (labs, radar, depots, superweapons and
/// other extras, then defenses, then spare barracks and war factories);
/// never the Construction Yards, the last refinery, war factory or barracks,
/// or power plants.
pub(crate) fn sell_for_miner(
    view: &BotView<'_>,
    think_every: u32,
    options: &[BuildOption],
    orders: &mut Vec<Command>,
) {
    if view.frame() % 90 >= think_every {
        return;
    }
    let is_miner = |t: &crate::rules::object_type::ObjectType| {
        (t.harvester || t.resource_gatherer) && !is_structure_type(t)
    };
    let mut miners = 0;
    let mut buildings = Vec::new();
    for e in view.own() {
        let Some(t) = view.object_type(e.type_ref()) else {
            continue;
        };
        if is_structure(e) {
            buildings.push((e, t));
        } else if is_miner(t) || e.is_harvester() || t.slaved {
            miners += 1;
        }
    }
    if miners > 0 {
        return;
    }
    let building_miner = view.factory(ProductionCategory::Vehicle).is_some_and(|f| {
        f.object
            .is_some_and(|o| view.object_type(o.type_id).is_some_and(is_miner))
            || f.queue
                .iter()
                .any(|q| view.object_type(q.type_id).is_some_and(is_miner))
    });
    // A new miner: the cheapest the sidebar offers (refineries bring one).
    let price = options
        .iter()
        .filter(|o| o.queue_category == ProductionCategory::Vehicle)
        .filter_map(|o| view.object_type(o.type_id))
        .filter(|t| is_miner(t))
        .map(|t| t.cost)
        .min();
    let Some(price) = price else { return };
    if building_miner || view.credits() >= price {
        return;
    }
    use crate::rules::object_type::FactoryType as F;
    let count = |f: F| {
        buildings
            .iter()
            .filter(|(_, t)| t.factory_type() == Some(f) && !t.naval)
            .count()
    };
    let refineries = buildings.iter().filter(|(_, t)| t.refinery).count();
    let rank = |t: &crate::rules::object_type::ObjectType| -> Option<u8> {
        if t.construction_yard
            || t.wall
            || t.undeploys_into.is_some()
            || t.power > 0 && t.factory_type().is_none()
        {
            return None;
        }
        if t.refinery {
            return (refineries > 1).then_some(3);
        }
        match t.factory_type() {
            Some(F::UnitType) if !t.naval => return (count(F::UnitType) > 1).then_some(2),
            Some(F::InfantryType) => return (count(F::InfantryType) > 1).then_some(2),
            _ => {}
        }
        if t.is_base_defense {
            return Some(1);
        }
        Some(0)
    };
    let pick = buildings
        .iter()
        .filter_map(|(e, t)| Some((rank(t)?, std::cmp::Reverse(t.cost), e.stable_id())))
        .min();
    if let Some((_, _, id)) = pick {
        if log::log_enabled!(log::Level::Debug) {
            log::debug!("sell f{} {id}: no miner, ${}", view.frame(), view.credits());
        }
        orders.push(Command::SellBuilding { entity_id: id });
    }
}

fn is_structure_type(t: &crate::rules::object_type::ObjectType) -> bool {
    t.category == crate::rules::object_type::ObjectCategory::Building
}

#[cfg(test)]
#[path = "chores_tests.rs"]
mod tests;
