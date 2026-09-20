//! Building sell/repair logic: refund calculation, crew ejection, repair tick.
//!
//! Extracted from production_placement.rs for file-size limits.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::DestroyedGarrisonBuilding;
use crate::sim::components::{Health, Position};
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::movement;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::passenger::PassengerRole;
use crate::sim::pathfinding::cell_entry::{TerrainCheckResult, check_terrain};
use crate::sim::world::{
    PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, SimSoundEvent, Simulation,
    UninitContext,
};
use crate::util::lepton;

use super::production_queue::{credits_entry_for_owner, credits_for_owner};
use super::production_tech::foundation_dimensions;

/// RA2 sell refund: 50% of cost (integer percentage).
const SELL_REFUND_PERCENT: u32 = 50;

const SCATTER_DIRECTION_OFFSETS: [(i16, i16); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// Health as integer percentage (0–100).
fn health_percent(current: u16, max: u16) -> u32 {
    if max == 0 {
        return 100;
    }
    ((current as u32) * 100 / max as u32).min(100)
}

fn sell_refund_for_building(
    obj: &crate::rules::object_type::ObjectType,
    health: Option<Health>,
) -> i32 {
    let hp_pct: u32 = health
        .map(|hp| health_percent(hp.current, hp.max))
        .unwrap_or(100);
    // refund = cost * sell% * health% / 10000
    (obj.cost.max(0) as u64 * SELL_REFUND_PERCENT as u64 * hp_pct as u64 / 10000) as i32
}

/// Survivor divisor for the given owner's side, from `[General]` INI keys.
/// Uses HouseState.side_index (0=Allied, 1=Soviet, 2=Yuri) instead of
/// the old string-matching classify_owner_side hack.
fn survivor_divisor_for_owner(sim: &Simulation, rules: &RuleSet, owner: &str) -> i32 {
    let side = sim
        .interner
        .get(owner)
        .and_then(|id| sim.houses.get(&id))
        .map(|h| h.side_index)
        .unwrap_or(0);
    match side {
        1 => rules.general.soviet_survivor_divisor,
        2 => rules.general.third_survivor_divisor,
        _ => rules.general.allied_survivor_divisor,
    }
}

/// Compute survivor count using the RA2 formula: sell_refund / SurvivorDivisor.
///
/// The original engine divides the health-scaled sell refund by a per-side
/// divisor from `[General]`. Buildings at 0 HP produce no survivors. The
/// `Crewed=yes` flag must be set.
fn sell_survivor_limit(
    sim: &Simulation,
    obj: &crate::rules::object_type::ObjectType,
    health: Option<Health>,
    rules: &RuleSet,
    owner: &str,
) -> usize {
    if !obj.crewed {
        return 0;
    }
    let refund = sell_refund_for_building(obj, health);
    if refund <= 0 {
        return 0;
    }
    let divisor = survivor_divisor_for_owner(sim, rules, owner).max(1);
    (refund / divisor).max(0) as usize
}

fn sell_survivor_type(sim: &Simulation, rules: &RuleSet, owner: &str) -> Option<String> {
    let side = sim
        .interner
        .get(owner)
        .and_then(|id| sim.houses.get(&id))
        .map(|h| h.side_index)
        .unwrap_or(0);
    let mut preferred: Vec<&str> = match side {
        2 => vec!["INIT", "E2", "E1"],
        1 => vec!["E2", "E1", "INIT"],
        _ => vec!["E1", "E2", "INIT"],
    };
    preferred.extend(rules.infantry_ids.iter().map(String::as_str));

    preferred.into_iter().find_map(|id| {
        let obj = rules.object(id)?;
        if obj.category != ObjectCategory::Infantry {
            return None;
        }
        if !obj.owner.is_empty() && !obj.owner.iter().any(|h| h.eq_ignore_ascii_case(owner)) {
            return None;
        }
        Some(id.to_string())
    })
}

fn sell_survivor_positions(rx: u16, ry: u16, width: u16, height: u16) -> Vec<(u16, u16)> {
    let mut cells = Vec::new();
    let min_x = i32::from(rx) - 1;
    let max_x = i32::from(rx) + i32::from(width);
    let min_y = i32::from(ry) - 1;
    let max_y = i32::from(ry) + i32::from(height);

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if x < 0 || y < 0 {
                continue;
            }
            let inside_x = x >= i32::from(rx) && x < i32::from(rx) + i32::from(width);
            let inside_y = y >= i32::from(ry) && y < i32::from(ry) + i32::from(height);
            if inside_x && inside_y {
                continue;
            }
            cells.push((x as u16, y as u16));
        }
    }

    cells.sort_by_key(|&(cx, cy)| {
        let dx = i32::from(cx) - (i32::from(rx) + i32::from(width) - 1);
        let dy = i32::from(cy) - (i32::from(ry) + i32::from(height) - 1);
        let dist_sq = dx * dx + dy * dy;
        (dist_sq, cy, cx)
    });
    cells
}

fn eject_sell_survivors(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    building_type: &crate::rules::object_type::ObjectType,
    building_pos: Position,
    health: Option<Health>,
) -> usize {
    let Some(infantry_type) = sell_survivor_type(sim, rules, owner) else {
        return 0;
    };
    let survivor_limit = sell_survivor_limit(sim, building_type, health, rules, owner);
    if survivor_limit == 0 {
        return 0;
    }

    let (width, height) = foundation_dimensions(&building_type.foundation);
    let mut spawned = 0;
    for (spawn_rx, spawn_ry) in
        sell_survivor_positions(building_pos.rx, building_pos.ry, width, height)
            .into_iter()
            .take(survivor_limit)
    {
        if sim
            .spawn_object_at_height(
                &infantry_type,
                owner,
                spawn_rx,
                spawn_ry,
                64,
                building_pos.z,
                rules,
            )
            .is_some()
        {
            spawned += 1;
        }
    }
    spawned
}

/// Eject survivors from a crewed building destroyed in combat.
///
/// In the original RA2 engine, destroyed crewed buildings always eject at
/// least one infantry survivor regardless of the building's remaining HP (which
/// is 0). The survivor type is side-dependent (E1 for Allied, E2 for Soviet,
/// INIT for Yuri).
pub fn eject_destruction_survivors(
    sim: &mut Simulation,
    rules: &RuleSet,
    type_id: InternedId,
    owner: InternedId,
    rx: u16,
    ry: u16,
    z: u8,
) -> usize {
    let type_str = sim.interner.resolve(type_id);
    let owner_str = sim.interner.resolve(owner);
    let Some(obj) = rules.object(type_str) else {
        return 0;
    };
    if !obj.crewed {
        return 0;
    }
    let Some(infantry_type) = sell_survivor_type(sim, rules, owner_str) else {
        return 0;
    };
    // Clone owner string for spawn calls — rare path (building destruction only).
    let owner_owned = owner_str.to_string();
    let (width, height) = foundation_dimensions(&obj.foundation);
    // Always eject at least 1 survivor on destruction.
    let positions = sell_survivor_positions(rx, ry, width, height);
    let mut spawned = 0;
    for (spawn_rx, spawn_ry) in positions.into_iter().take(1) {
        if sim
            .spawn_object_at_height(
                &infantry_type,
                &owner_owned,
                spawn_rx,
                spawn_ry,
                64,
                z,
                rules,
            )
            .is_some()
        {
            spawned += 1;
        }
    }
    spawned
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GarrisonEjectMode {
    PlayerSell,
    DestructionNoExitRemove,
}

fn push_nonnegative_cell(cells: &mut Vec<(u16, u16)>, x: i32, y: i32) {
    if x >= 0 && y >= 0 && x <= i32::from(u16::MAX) && y <= i32::from(u16::MAX) {
        cells.push((x as u16, y as u16));
    }
}

fn garrison_sellbuilding_exit_cells(rx: u16, ry: u16, width: u16, height: u16) -> Vec<(u16, u16)> {
    let mut cells = Vec::new();
    if width == 0 || height == 0 {
        return cells;
    }

    let ox = i32::from(rx);
    let oy = i32::from(ry);
    let east = ox + i32::from(width);
    let west = ox - 1;
    let south = oy + i32::from(height);
    let north = oy - 1;

    for y in (north..=south).rev() {
        push_nonnegative_cell(&mut cells, east, y);
    }
    for x in (west..=east).rev() {
        push_nonnegative_cell(&mut cells, x, south);
    }
    for x in ox..=east {
        push_nonnegative_cell(&mut cells, x, north);
    }
    for y in oy..=south {
        push_nonnegative_cell(&mut cells, west, y);
    }

    cells
}

/// Closest current Rust stand-in for native `Can_Enter_Cell(cell,-1,-1,0,1) == 0`.
///
/// `SellBuilding` probes only occupant slot 0 while choosing the single exit
/// cell. Rust does not have the exact InfantryClass predicate bound here yet;
/// this uses the shared Can_Enter_Cell phase-1 terrain/sub-cell check with the
/// verified available inputs and leaves terrain-cost/layer details unchecked.
fn garrison_first_occupant_can_enter_cell(
    sim: &Simulation,
    first_occupant_id: u64,
    rx: u16,
    ry: u16,
) -> bool {
    garrison_infantry_can_enter_cell(sim, first_occupant_id, rx, ry, true)
}

fn garrison_infantry_can_enter_cell(
    sim: &Simulation,
    infantry_id: u64,
    rx: u16,
    ry: u16,
    require_inside_transport: bool,
) -> bool {
    let Some(infantry) = sim.substrate.entities.get(infantry_id) else {
        return false;
    };
    if !infantry.is_alive() {
        return false;
    }
    if require_inside_transport && !infantry.passenger_role.is_inside_transport() {
        return false;
    }

    matches!(
        check_terrain(
            (rx, ry),
            MovementLayer::Ground,
            infantry.category,
            None,
            None,
            &sim.substrate.occupancy,
        ),
        TerrainCheckResult::Clear
    )
}

fn choose_garrison_exit_cell(
    sim: &Simulation,
    rx: u16,
    ry: u16,
    width: u16,
    height: u16,
    passenger_ids: &[u64],
) -> Option<(u16, u16)> {
    let first_occupant_id = *passenger_ids.first()?;
    garrison_sellbuilding_exit_cells(rx, ry, width, height)
        .into_iter()
        .find(|&(cx, cy)| garrison_first_occupant_can_enter_cell(sim, first_occupant_id, cx, cy))
}

fn garrison_inside_foundation_fallback(rx: u16, ry: u16, width: u16, height: u16) -> (u16, u16) {
    (
        rx.saturating_add(width.saturating_sub(1)),
        ry.saturating_add(height.saturating_sub(1)),
    )
}

fn uninit_garrison_passenger_without_exit(
    sim: &mut Simulation,
    passenger_id: u64,
    context: UninitContext<'_>,
) {
    if let Some(pax) = sim.substrate.entities.get_mut(passenger_id) {
        pax.health.current = 0;
        pax.passenger_role = PassengerRole::None;
    }
    sim.uninit_with_context(passenger_id, context);
}

fn sellbuilding_direct_scatter_handoff(
    sim: &mut Simulation,
    rules: &RuleSet,
    passenger_id: u64,
    building_rx: u16,
    building_ry: u16,
    building_width: u16,
    building_height: u16,
) {
    let Some(pax) = sim.substrate.entities.get(passenger_id) else {
        return;
    };

    if pax.category != EntityCategory::Infantry
        || !pax.is_alive()
        || pax.dying
        || pax.passenger_role.is_inside_transport()
        || pax.locomotor.is_none()
    {
        return;
    }

    let target_rx = building_rx.saturating_add(building_width / 2);
    let target_ry = building_ry.saturating_add(building_height / 2);
    let base_dir = garrison_scatter_direction_index(
        i32::from(target_rx) - i32::from(pax.position.rx),
        i32::from(target_ry) - i32::from(pax.position.ry),
    );
    let start_cell = (pax.position.rx, pax.position.ry);
    let type_name = sim.interner.resolve(pax.type_ref()).to_string();
    // `FootClass::GetCurrentSpeed @ 0x004DB1A0`: a veteran garrison occupant
    // ejected by the sale scatters at its FASTER speed.
    let sell_obj = rules.object(&type_name);
    let speed = crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
        pax,
        sell_obj,
        sell_obj.map_or(4, |obj| obj.speed),
        rules.general.veteran_speed,
    );

    let jitter = sim.scatter_rng().next_range_u32_inclusive(0, 4) as i32 - 2;
    let start_dir = ((base_dir as i32 + jitter) & 7) as usize;
    let mut dest = None;
    for i in 0..8 {
        let (dx, dy) = SCATTER_DIRECTION_OFFSETS[(start_dir + i) & 7];
        let cx = i32::from(start_cell.0) + i32::from(dx);
        let cy = i32::from(start_cell.1) + i32::from(dy);
        if cx < 0 || cy < 0 {
            continue;
        }
        let candidate = (cx as u16, cy as u16);
        if garrison_infantry_can_enter_cell(sim, passenger_id, candidate.0, candidate.1, false) {
            dest = Some(candidate);
            break;
        }
    }

    if let Some(dest) = dest {
        let timing = movement::DestinationTiming::new(
            sim.session.binary_frame,
            sim.blockage_path_delay_ticks,
        );
        let _ = movement::issue_direct_move(
            &mut sim.substrate.entities,
            passenger_id,
            dest,
            speed,
            timing,
        );
    }
}

fn garrison_scatter_direction_index(dx: i32, dy: i32) -> usize {
    usize::from(movement::facing_from_delta(dx, dy) / 32) & 7
}

fn place_garrison_passenger_at_cell(
    sim: &mut Simulation,
    rules: &RuleSet,
    passenger_id: u64,
    owner_override: Option<InternedId>,
    rx: u16,
    ry: u16,
    z: u8,
    building_rx: u16,
    building_ry: u16,
    building_width: u16,
    building_height: u16,
    context: UninitContext<'_>,
) -> bool {
    // Owner transfer (if any) goes through the substrate chokepoint first, so
    // the by_owner index stays in sync; then take the mutable borrow for the
    // remaining field writes. change_owner is a no-op if the id is absent —
    // the get_mut below still guards absence.
    if let Some(owner) = owner_override {
        sim.change_owner_with_rules(passenger_id, owner, rules);
    }
    let Some(pax_sub_cell) = sim
        .substrate
        .entities
        .get(passenger_id)
        .map(|pax| pax.sub_cell)
    else {
        return false;
    };
    if let Some(pax) = sim.substrate.entities.get_mut(passenger_id) {
        pax.passenger_role = PassengerRole::None;
    }
    let (sub_x, sub_y) = lepton::subcell_lepton_offset(pax_sub_cell);
    let reveal = sim.try_reveal_entity_with_context(
        passenger_id,
        RevealRequest {
            position: RevealPosition {
                rx,
                ry,
                z,
                sub_x,
                sub_y,
            },
            // The selected exit/fallback is this caller's bounded admission
            // evidence; the exact native placement oracle remains separate.
            placement: PlacementEvidence::MarkSucceeded,
            logic_eligible: true,
        },
        context,
    );
    if !matches!(reveal, RevealOutcome::Revealed { .. }) {
        return false;
    }

    sellbuilding_direct_scatter_handoff(
        sim,
        rules,
        passenger_id,
        building_rx,
        building_ry,
        building_width,
        building_height,
    );

    true
}

fn eject_garrison_passengers_at_edges(
    sim: &mut Simulation,
    rules: &RuleSet,
    rx: u16,
    ry: u16,
    z: u8,
    width: u16,
    height: u16,
    passenger_ids: &[u64],
    owner_override: Option<InternedId>,
    mode: GarrisonEjectMode,
    uninit_context: UninitContext<'_>,
) -> usize {
    if passenger_ids.is_empty() || width == 0 || height == 0 {
        return 0;
    }

    let exit_cell = choose_garrison_exit_cell(sim, rx, ry, width, height, passenger_ids).or_else(
        || match mode {
            GarrisonEjectMode::PlayerSell => {
                Some(garrison_inside_foundation_fallback(rx, ry, width, height))
            }
            GarrisonEjectMode::DestructionNoExitRemove => None,
        },
    );
    let mut ejected: usize = 0;

    let Some((exit_rx, exit_ry)) = exit_cell else {
        for &pax_id in passenger_ids.iter().rev() {
            uninit_garrison_passenger_without_exit(sim, pax_id, uninit_context);
        }
        return 0;
    };

    // Iterate in reverse (LIFO); gamemd walks occupants high index to low.
    for &pax_id in passenger_ids.iter().rev() {
        if place_garrison_passenger_at_cell(
            sim,
            rules,
            pax_id,
            owner_override,
            exit_rx,
            exit_ry,
            z,
            rx,
            ry,
            width,
            height,
            uninit_context,
        ) {
            ejected += 1;
        }
    }

    ejected
}

/// Eject garrison occupants from a building being sold.
///
/// Matches gamemd `SellBuilding @ 0x00457DE0`: choose one exit coordinate
/// before the occupant loop, then Unlimbo occupants in LIFO order at that same
/// coordinate. If no edge exit is accepted, normal player sell falls back to the
/// southeast inside-foundation cell.
///
/// Returns the number of occupants successfully ejected.
fn eject_garrison_occupants(sim: &mut Simulation, rules: &RuleSet, building_id: u64) -> usize {
    // Snapshot building data before mutation.
    let (rx, ry, z, width, height, passenger_ids) = {
        let entity = match sim.substrate.entities.get(building_id) {
            Some(e) => e,
            None => return 0,
        };
        let cargo = match entity.passenger_role.cargo() {
            Some(c) if !c.is_empty() => c,
            _ => return 0,
        };
        let obj = match sim.object_type(entity.type_ref(), rules) {
            Some(o) => o,
            None => return 0,
        };
        let (fw, fh) = foundation_dimensions(&obj.foundation);
        (
            entity.position.rx,
            entity.position.ry,
            entity.position.z,
            fw,
            fh,
            cargo.passengers.clone(),
        )
    };

    let ejected = eject_garrison_passengers_at_edges(
        sim,
        rules,
        rx,
        ry,
        z,
        width,
        height,
        &passenger_ids,
        None,
        GarrisonEjectMode::PlayerSell,
        UninitContext::default(),
    );

    // Clear player-sell cargo only. Native SellBuilding is an ejection helper;
    // empty-garrison ownership reversion belongs to reconciliation/unload.
    if let Some(building) = sim.substrate.entities.get_mut(building_id) {
        if let Some(cargo) = building.passenger_role.cargo_mut() {
            cargo.clear_contents();
        }
    }

    ejected
}

/// Eject garrison occupants from a building destroyed in combat.
///
/// Verified gamemd evidence routes destroyed `CanBeOccupied` garrisons through
/// the same SellBuilding helper used by sell/abandon. Destruction callers use
/// the null no-exit branch: if no edge coordinate is accepted, occupants are
/// removed rather than parachuted or placed inside the foundation. The caller
/// invokes this while the building is still resolvable, before building UnInit,
/// so its cargo can be detached from generic transport-death recursion.
///
/// Returns the count of occupants successfully ejected (excludes those killed
/// when no edge cell can be used).
#[cfg(test)]
pub(crate) fn eject_destruction_garrison(
    sim: &mut Simulation,
    rules: &RuleSet,
    event: &DestroyedGarrisonBuilding,
) -> usize {
    eject_destruction_garrison_with_context(sim, rules, event, UninitContext::default())
}

pub(crate) fn eject_destruction_garrison_with_context(
    sim: &mut Simulation,
    rules: &RuleSet,
    event: &DestroyedGarrisonBuilding,
    uninit_context: UninitContext<'_>,
) -> usize {
    let passenger_ids = sim
        .substrate
        .entities
        .get_mut(event.building_id)
        .and_then(|building| building.passenger_role.cargo_mut())
        .map(|cargo| cargo.take_for_uninit())
        .unwrap_or_else(|| event.passenger_ids.clone());
    debug_assert_eq!(
        passenger_ids, event.passenger_ids,
        "destroyed-garrison cargo must be detached before building UnInit"
    );

    eject_garrison_passengers_at_edges(
        sim,
        rules,
        event.rx,
        event.ry,
        event.z,
        event.foundation_w,
        event.foundation_h,
        &passenger_ids,
        Some(event.owner),
        GarrisonEjectMode::DestructionNoExitRemove,
        uninit_context,
    )
}

/// Eject garrison occupants from a red-HP `CanBeOccupied` building.
///
/// Native `CheckAutoSellOrCivilian` calls the same `SellBuilding` occupant
/// helper when a garrisoned building is at red HP, but the building remains
/// alive and ownership reconciliation continues afterward.
pub(crate) fn eject_red_hp_garrison(
    sim: &mut Simulation,
    rules: &RuleSet,
    building_id: u64,
) -> usize {
    let (rx, ry, z, width, height, owner, passenger_ids) = {
        let Some(entity) = sim.substrate.entities.get(building_id) else {
            return 0;
        };
        let Some(cargo) = entity.passenger_role.cargo() else {
            return 0;
        };
        if cargo.is_empty() {
            return 0;
        }
        let Some(obj) = sim.object_type(entity.type_ref(), rules) else {
            return 0;
        };
        if !obj.can_be_occupied {
            return 0;
        }
        let (fw, fh) = foundation_dimensions(&obj.foundation);
        (
            entity.position.rx,
            entity.position.ry,
            entity.position.z,
            fw,
            fh,
            entity.owner(),
            cargo.passengers.clone(),
        )
    };

    let ejected = eject_garrison_passengers_at_edges(
        sim,
        rules,
        rx,
        ry,
        z,
        width,
        height,
        &passenger_ids,
        Some(owner),
        GarrisonEjectMode::DestructionNoExitRemove,
        UninitContext::default(),
    );

    if let Some(building) = sim.substrate.entities.get_mut(building_id) {
        if let Some(cargo) = building.passenger_role.cargo_mut() {
            cargo.clear_contents();
        }
    }

    ejected
}

/// Sell a building entity: refund part of its current value, eject crew, and despawn it.
///
/// Captured civilian `CanBeOccupied` garrisons use the same player-sell
/// transaction once they are owned by the seller: occupants eject through
/// the SellBuilding-style helper, then the building is removed/refunded.
/// Revert-to-civilian belongs to empty-garrison reconciliation, not player sell.
pub fn sell_building(sim: &mut Simulation, rules: &RuleSet, stable_id: u64) -> bool {
    let (owner_name, type_id, position, health) = {
        let Some(entity) = sim.substrate.entities.get(stable_id) else {
            return false;
        };
        if entity.category != EntityCategory::Structure {
            return false;
        }
        (
            sim.interner.resolve(entity.owner()).to_string(),
            sim.interner.resolve(entity.type_ref()).to_string(),
            entity.position.clone(),
            Some(entity.health),
        )
    };
    let Some(obj) = rules.object(&type_id) else {
        return false;
    };

    let refund = sell_refund_for_building(obj, health);
    let ejected = eject_sell_survivors(sim, rules, &owner_name, obj, position, health);
    // Eject garrison occupants alive before removing the building (gamemd SellBuilding).
    let garrison_ejected = eject_garrison_occupants(sim, rules, stable_id);
    let interrupted_miners =
        crate::sim::miner::interrupt_refinery_docked_miners(sim, rules, stable_id);
    // Eject a bunkered unit before the bunker is removed (gamemd UndockUnit: place
    // at the building cell, no sound/anim/Move). Must precede uninit so the unit
    // is revealed/placed before the despawn safety net would clear the link.
    if sim
        .substrate
        .entities
        .get(stable_id)
        .and_then(|b| b.bunker_occupant)
        .is_some()
    {
        crate::sim::docking::bunker_link::release_sell_destroy(sim, stable_id);
    }
    sim.set_building_light_active(stable_id, false);
    sim.uninit_with_context(stable_id, UninitContext::with_rules(rules));
    let owner_id = sim.interner.intern(&owner_name);
    // Refresh superweapon grants — sold building may have been providing a SW.
    if sim.session.game_options.super_weapons {
        crate::sim::superweapon::refresh_super_weapons_for_owner(sim, rules, owner_id);
    }
    if refund > 0 {
        *credits_entry_for_owner(sim, &owner_name) += refund;
    }
    // `BuildingClass::Sell 0x00449C99..0x00449CE5` (sell state 2, the
    // building is gone): `[this+0x6DD]` — the build-animation-complete flag,
    // set at `0x004467C9` (construction complete), cleared in sell state 0
    // and again at the end of sell state 1, so state 2 waits for the
    // sell-down animation — `[this+0x41A]` (owner is the local player) and
    // `UndeploysInto=` (`Type+0x408`) null → `PlayEVA("EVA_StructureSold")`.
    // A Construction Yard undeploys into its MCV instead and stays silent.
    // The app applies the local-owner half.
    if obj.undeploys_into.is_none() {
        sim.sound_events
            .push(SimSoundEvent::StructureSold { owner: owner_id });
    }
    log::info!(
        "Building {} sold by {}: refunded {} credits, ejected {} crew + {} garrison, undocked {} miners",
        type_id,
        owner_name,
        refund,
        ejected,
        garrison_ejected,
        interrupted_miners
    );
    true
}

/// Toggle repair mode on a building. If already repairing, stop. Otherwise start.
pub fn toggle_repair(sim: &mut Simulation, stable_id: u64) -> bool {
    let Some(entity) = sim.substrate.entities.get_mut(stable_id) else {
        return false;
    };
    if entity.category != EntityCategory::Structure {
        return false;
    }
    if entity.repairing {
        entity.repairing = false;
        log::info!("Repair stopped on entity {}", stable_id);
    } else {
        entity.repairing = true;
        log::info!("Repair started on entity {}", stable_id);
        // `BuildingClass::ToggleRepair @ 0x00446FF0`: once the flag is on
        // and `Health != Type.Strength` (`0x00447059`), the local owner
        // (`0x004470A4 CALL 0x0050B6F0`) gets the sidebar flash and
        // `PlayEVA("EVA_Repairing")` (`0x004470B7`). The app applies the
        // local-owner half.
        if entity.health.current != entity.health.max {
            let owner = entity.owner();
            sim.sound_events.push(SimSoundEvent::Repairing { owner });
        }
    }
    true
}

/// Repair cost: 25% of building cost spread across all HP.
const REPAIR_COST_PERCENT: u32 = 25;
/// HP healed per sim tick (at 15 Hz this is ~60 HP/sec).
const REPAIR_HP_PER_TICK: u16 = 4;

/// Run the `WasAttackedByEnemy` consumer from
/// `BuildingClass::UpdateRepairAndPower @ 0x00450630` in stable building order.
///
/// CurrentIQ is persisted per house because named scenario houses can carry a
/// lower `IQ=` than generated skirmish computer houses. The sale itself uses
/// the existing authoritative building-sale transaction.
fn tick_ai_low_credit_sell_decisions(sim: &mut Simulation, rules: &RuleSet) {
    let building_ids: Vec<u64> = sim
        .substrate
        .entities
        .values()
        .filter(|entity| entity.category == EntityCategory::Structure)
        .map(|entity| entity.stable_id())
        .collect();

    for stable_id in building_ids {
        let Some((owner, eligible_building)) =
            sim.substrate.entities.get(stable_id).map(|entity| {
                let mission = entity.mission.current().known();
                let below_red = entity.health.max != 0
                    && f64::from(entity.health.current) / f64::from(entity.health.max)
                        < f64::from(rules.general.condition_red);
                (
                    entity.owner(),
                    entity.is_active()
                        && !entity.lifecycle.in_limbo
                        && entity.was_attacked_by_enemy
                        && !matches!(
                            mission,
                            Some(MissionType::Selling | MissionType::Construction)
                        )
                        && below_red,
                )
            })
        else {
            continue;
        };
        if !eligible_building {
            continue;
        }

        let Some(house) = sim.houses.get(&owner) else {
            continue;
        };
        let house_iq = house.current_iq;
        if house_iq < rules.general.iq_repair_sell
            || house.economy.credits >= rules.general.credit_reserve
            || house_iq < rules.general.iq_sell_back
        {
            continue;
        }
        // Native draws inclusive RandomRanged(0, 0x32), then performs an
        // unsigned comparison against HouseClass TechLevel.
        let roll = sim.scenario_rng.next_range_u32_inclusive(0, 0x32);
        if roll >= house.tech_level as u32 {
            continue;
        }

        // The native vslot starts the Building sell/construction path. VERA's
        // existing building-sale authority completes that same gameplay
        // transaction synchronously; keeping it here preserves stable-ID and
        // RNG/credit visibility for the next building decision.
        let _ = sell_building(sim, rules, stable_id);
    }
}

/// Tick all repairing buildings: heal HP and deduct credits.
pub fn tick_repairs(sim: &mut Simulation, rules: &RuleSet) {
    // UpdateRepairAndPower evaluates the low-credit sale arm before its active
    // repair tick for the same building.
    tick_ai_low_credit_sell_decisions(sim, rules);
    // Collect snapshot of repairing structures.
    let actions: Vec<(u64, String, String, u16, u16)> = sim
        .substrate
        .entities
        .values()
        .filter(|e| {
            // A Dying building corpse (destroyed this tick, awaiting the end-of-
            // tick drain) must not be auto-repaired — no credits spent on a dead
            // building.
            !e.dying
                && e.repairing
                && e.category == EntityCategory::Structure
                && e.health.current < e.health.max
        })
        .map(|e| {
            (
                e.stable_id(),
                sim.interner.resolve(e.owner()).to_string(),
                sim.interner.resolve(e.type_ref()).to_string(),
                e.health.current,
                e.health.max,
            )
        })
        .collect();
    let mut stop_repairing: Vec<u64> = Vec::new();
    for (stable_id, owner, type_id, current_hp, max_hp) in actions {
        let cost_per_hp: i32 = rules
            .object(&type_id)
            .map(|obj| {
                // total_repair_cost = cost * 25 / 100, then / max_hp (ceiling division)
                let total_repair_cost: u32 = obj.cost.max(0) as u32 * REPAIR_COST_PERCENT / 100;
                total_repair_cost.div_ceil(max_hp.max(1) as u32).max(1) as i32
            })
            .unwrap_or(1);
        let credits = credits_for_owner(sim, &owner);
        if credits < cost_per_hp {
            stop_repairing.push(stable_id);
            continue;
        }
        let heal = REPAIR_HP_PER_TICK.min(max_hp - current_hp);
        if heal == 0 {
            stop_repairing.push(stable_id);
            continue;
        }
        *credits_entry_for_owner(sim, &owner) -= cost_per_hp * heal as i32;
        if let Some(entity) = sim.substrate.entities.get_mut(stable_id) {
            entity.health.current = (entity.health.current + heal).min(entity.health.max);
            entity.refresh_building_damage_state_gate(rules.general.condition_yellow_x1000);
            if entity.health.current >= entity.health.max {
                stop_repairing.push(stable_id);
            }
        }
    }
    for stable_id in stop_repairing {
        if let Some(entity) = sim.substrate.entities.get_mut(stable_id) {
            entity.repairing = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::occupancy::CellListInsertion;

    fn garrison_edge_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             0=E1\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=CAGAS01\n\
             [E1]\n\
             Name=GI\n\
             Cost=200\n\
             Strength=100\n\
             Armor=flak\n\
             Speed=4\n\
             Sight=5\n\
             TechLevel=1\n\
             Owner=Americans,Neutral\n\
             Occupier=yes\n\
             Size=1\n\
             [CAGAS01]\n\
             Name=GasStation\n\
             Cost=400\n\
             Strength=400\n\
             Armor=wood\n\
             Foundation=2x2\n\
             CanBeOccupied=yes\n\
             CanOccupyFire=yes\n\
             MaxNumberOccupants=5\n",
        );
        RuleSet::from_ini(&ini).expect("garrison edge rules should parse")
    }

    fn repair_damage_state_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n0=GAPOWR\n\n\
             [GAPOWR]\nStrength=100\nArmor=wood\nCost=800\n\n\
             [AudioVisual]\nConditionYellow=50%\n",
        );
        RuleSet::from_ini(&ini).expect("repair damage-state rules should parse")
    }

    fn insert_hidden_passenger_with_subcell(
        sim: &mut Simulation,
        stable_id: u64,
        transport_id: u64,
        owner: &str,
        sub_cell: Option<u8>,
    ) -> u64 {
        let mut pax = GameEntity::test_default(stable_id, "E1", owner, 0, 0);
        pax.category = EntityCategory::Infantry;
        pax.sub_cell = sub_cell;
        pax.owner = sim.interner.intern(owner);
        pax.type_ref = sim.interner.intern("E1");
        pax.passenger_role = PassengerRole::Inside { transport_id };
        sim.substrate.entities.insert(pax);
        stable_id
    }

    fn insert_hidden_passenger(
        sim: &mut Simulation,
        stable_id: u64,
        transport_id: u64,
        owner: &str,
    ) -> u64 {
        insert_hidden_passenger_with_subcell(sim, stable_id, transport_id, owner, Some(2))
    }

    fn insert_live_blocker(sim: &mut Simulation, stable_id: u64, rx: u16, ry: u16) {
        let mut blocker = GameEntity::test_default(stable_id, "BLOCKER", "Neutral", rx, ry);
        blocker.category = EntityCategory::Unit;
        sim.substrate.entities.insert(blocker);
        sim.substrate.occupancy.add(
            rx,
            ry,
            stable_id,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
    }

    fn insert_map_infantry(sim: &mut Simulation, stable_id: u64, rx: u16, ry: u16, sub_cell: u8) {
        let mut infantry = GameEntity::test_default(stable_id, "E1", "Neutral", rx, ry);
        infantry.category = EntityCategory::Infantry;
        infantry.sub_cell = Some(sub_cell);
        sim.substrate.entities.insert(infantry);
        sim.substrate.occupancy.add(
            rx,
            ry,
            stable_id,
            MovementLayer::Ground,
            Some(sub_cell),
            CellListInsertion::PrependNonBuilding,
        );
    }

    fn give_walk_locomotor(sim: &mut Simulation, stable_id: u64) {
        sim.substrate
            .entities
            .get_mut(stable_id)
            .expect("test entity should exist")
            .locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
    }

    fn block_all_garrison_exit_cells(sim: &mut Simulation, rx: u16, ry: u16, w: u16, h: u16) {
        for (idx, (block_rx, block_ry)) in garrison_sellbuilding_exit_cells(rx, ry, w, h)
            .into_iter()
            .enumerate()
        {
            insert_live_blocker(sim, 10_000 + idx as u64, block_rx, block_ry);
        }
    }

    #[test]
    fn building_repair_crossing_above_condition_yellow_clears_building_damage_state() {
        let rules = repair_damage_state_rules();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("GAPOWR");
        let mut building = GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            10,
            0,
            0,
            owner,
            Health {
                current: 49,
                max: 100,
            },
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        building.repairing = true;
        building.building_damage_state_active = true;
        sim.substrate.entities.insert(building);

        tick_repairs(&mut sim, &rules);

        let building = sim
            .substrate
            .entities
            .get(1)
            .expect("building should remain");
        assert_eq!(building.health.current, 53);
        assert!(!building.building_damage_state_active);
    }

    fn insert_captured_player_owned_garrison(
        sim: &mut Simulation,
        building_id: u64,
        passenger_id: u64,
    ) {
        let americans = sim.interner.intern("Americans");
        let neutral = sim.interner.intern("Neutral");

        let mut building = GameEntity::test_default(building_id, "CAGAS01", "Americans", 10, 10);
        building.category = EntityCategory::Structure;
        building.foundation = "2x2".to_string();
        building.owner = americans;
        building.type_ref = sim.interner.intern("CAGAS01");
        building.garrison_original_owner = Some(neutral);
        building.passenger_role = PassengerRole::Transport {
            cargo: crate::sim::passenger::PassengerCargo::new(5, 1),
        };
        if let Some(cargo) = building.passenger_role.cargo_mut() {
            assert!(cargo.board(passenger_id, 1));
        }
        sim.substrate.entities.insert(building);
        sim.reveal(building_id);
        sim.add_entity_occupancy(building_id);

        insert_hidden_passenger(sim, passenger_id, building_id, "Americans");
    }

    #[test]
    fn garrison_sellbuilding_scan_order_matches_gamemd_edges_2x2() {
        assert_eq!(
            garrison_sellbuilding_exit_cells(10, 10, 2, 2),
            vec![
                (12, 12),
                (12, 11),
                (12, 10),
                (12, 9),
                (12, 12),
                (11, 12),
                (10, 12),
                (9, 12),
                (10, 9),
                (11, 9),
                (12, 9),
                (9, 10),
                (9, 11),
                (9, 12),
            ]
        );
    }

    #[test]
    fn garrison_sellbuilding_scan_order_handles_map_edge_without_u16_wrap() {
        assert_eq!(
            garrison_sellbuilding_exit_cells(0, 0, 2, 2),
            vec![(2, 2), (2, 1), (2, 0), (2, 2), (1, 2), (0, 2)]
        );
    }

    #[test]
    fn garrison_exit_probe_uses_first_occupant_only() {
        let mut sim = Simulation::new();
        insert_hidden_passenger(&mut sim, 12, 10, "Neutral");

        assert_eq!(
            choose_garrison_exit_cell(&sim, 10, 10, 2, 2, &[11, 12]),
            None,
            "slot 0 drives the scan; a later valid passenger must not be probed"
        );

        insert_hidden_passenger(&mut sim, 11, 10, "Neutral");
        assert_eq!(
            choose_garrison_exit_cell(&sim, 10, 10, 2, 2, &[11, 12]),
            Some((12, 12))
        );
    }

    #[test]
    fn garrison_exit_probe_uses_infantry_subcell_entry_predicate() {
        let mut sim = Simulation::new();
        insert_hidden_passenger(&mut sim, 11, 10, "Neutral");
        insert_map_infantry(&mut sim, 100, 12, 12, 2);

        assert_eq!(
            choose_garrison_exit_cell(&sim, 10, 10, 2, 2, &[11]),
            Some((12, 12)),
            "one exterior infantry leaves a free sub-cell, so slot-0 Can_Enter_Cell accepts"
        );

        insert_map_infantry(&mut sim, 101, 12, 12, 3);
        insert_map_infantry(&mut sim, 102, 12, 12, 4);

        assert_eq!(
            choose_garrison_exit_cell(&sim, 10, 10, 2, 2, &[11]),
            Some((12, 11)),
            "full exterior infantry sub-cells reject the selected cell and continue the scan"
        );
    }

    #[test]
    fn captured_civilian_garrison_player_sell_removes_building_and_refunds() {
        let rules = garrison_edge_rules();
        let mut sim = Simulation::new();
        let building_id = 10;
        let passenger_id = 11;
        insert_captured_player_owned_garrison(&mut sim, building_id, passenger_id);

        let before = credits_for_owner(&sim, "Americans");

        assert!(sell_building(&mut sim, &rules, building_id));

        // Deferred-delete: drain at end-of-tick to free the sold building's slot.
        sim.flush_pending_delete();
        assert!(sim.substrate.entities.get(building_id).is_none());
        for cell in [(10, 10), (10, 11), (11, 10), (11, 11)] {
            assert!(
                !sim.substrate
                    .occupancy
                    .contains_entity(cell.0, cell.1, building_id),
                "sold building should clear foundation cell {cell:?}"
            );
        }
        sim.debug_assert_logic_membership_consistent();
        assert_eq!(credits_for_owner(&sim, "Americans") - before, 200);

        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("passenger should survive sell eject");
        assert!(matches!(passenger.passenger_role, PassengerRole::None));
        assert!(!passenger.dying);
        assert!(
            passenger.position.rx < 10
                || passenger.position.rx > 11
                || passenger.position.ry < 10
                || passenger.position.ry > 11,
            "passenger should be ejected outside the 2x2 foundation"
        );

        assert!(
            !sim.sound_events.iter().any(|event| {
                matches!(
                    event,
                    crate::sim::world::SimSoundEvent::StructureAbandoned { .. }
                )
            }),
            "player sell must not emit StructureAbandoned"
        );
    }

    #[test]
    fn sellbuilding_helper_ejects_without_owner_revert() {
        let rules = garrison_edge_rules();
        let mut sim = Simulation::new();
        let building_id = 20;
        let passenger_id = 21;
        insert_captured_player_owned_garrison(&mut sim, building_id, passenger_id);

        let americans = sim.interner.intern("Americans");
        let neutral = sim.interner.intern("Neutral");

        assert_eq!(eject_garrison_occupants(&mut sim, &rules, building_id), 1);

        let building = sim
            .substrate
            .entities
            .get(building_id)
            .expect("helper should not remove building");
        assert_eq!(
            building.owner, americans,
            "SellBuilding-style helper must not ChangeOwner"
        );
        assert_eq!(
            building.garrison_original_owner,
            Some(neutral),
            "helper must not consume reconciliation state during player-sell ejection"
        );
        assert!(
            building
                .passenger_role
                .cargo()
                .is_some_and(|cargo| cargo.is_empty()),
            "helper should clear building cargo"
        );

        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("passenger should remain");
        assert!(matches!(passenger.passenger_role, PassengerRole::None));
        assert!(!passenger.dying);
    }

    #[test]
    fn garrison_sellbuilding_reuses_single_exit_coord_for_all_lifo_occupants() {
        let rules = garrison_edge_rules();
        let mut sim = Simulation::new();
        let building_id = 10;
        let pax1 =
            insert_hidden_passenger_with_subcell(&mut sim, 11, building_id, "Neutral", Some(4));
        let pax2 =
            insert_hidden_passenger_with_subcell(&mut sim, 12, building_id, "Neutral", Some(3));
        let pax3 =
            insert_hidden_passenger_with_subcell(&mut sim, 13, building_id, "Neutral", Some(2));
        let owner = sim.interner.intern("Americans");
        let rng_before = sim.scenario_rng.state();

        let event = DestroyedGarrisonBuilding {
            building_id,
            type_id: sim.interner.intern("CAGAS01"),
            owner,
            rx: 10,
            ry: 10,
            z: 0,
            foundation_w: 2,
            foundation_h: 2,
            passenger_ids: vec![pax1, pax2, pax3],
        };

        assert_eq!(eject_destruction_garrison(&mut sim, &rules, &event), 3);
        assert_eq!(
            sim.scenario_rng.state(),
            rng_before,
            "test passengers have no locomotor, so Scatter gates must reject before RNG"
        );

        let checks = [(pax3, (12, 12)), (pax2, (12, 12)), (pax1, (12, 12))];

        for (pax_id, expected_cell) in checks {
            let pax = sim
                .substrate
                .entities
                .get(pax_id)
                .expect("passenger should remain");
            assert_eq!(
                (pax.position.rx, pax.position.ry),
                expected_cell,
                "destroyed garrison should reuse the one chosen SellBuilding exit coordinate"
            );
            assert!(
                pax.position.rx < 10
                    || pax.position.rx > 11
                    || pax.position.ry < 10
                    || pax.position.ry > 11,
                "destroyed garrison should not place passengers inside the foundation"
            );
            assert_eq!(
                pax.owner, owner,
                "destruction keeps death-time building owner"
            );
            assert!(matches!(pax.passenger_role, PassengerRole::None));
            assert!(!pax.dying);
            assert!(!pax.lifecycle.in_limbo);
            assert!(pax.lifecycle.cell_marked);
            assert!(pax.in_logic_vector);
        }

        let occupancy_ids: Vec<u64> = sim
            .substrate
            .occupancy
            .get(12, 12)
            .expect("chosen exit cell should be occupied")
            .iter_layer(crate::sim::movement::locomotor::MovementLayer::Ground)
            .map(|occupant| occupant.entity_id)
            .collect();
        assert_eq!(
            occupancy_ids,
            vec![pax1, pax2, pax3],
            "prepend insertion makes the final list expose LIFO placement order"
        );
    }

    #[test]
    fn garrison_destruction_detaches_cargo_before_building_uninit() {
        let rules = garrison_edge_rules();
        let mut sim = Simulation::new();
        let building_id = 60;
        let passenger_id = 61;
        insert_captured_player_owned_garrison(&mut sim, building_id, passenger_id);
        let owner = sim.interner.intern("Americans");
        let event = DestroyedGarrisonBuilding {
            building_id,
            type_id: sim.interner.intern("CAGAS01"),
            owner,
            rx: 10,
            ry: 10,
            z: 0,
            foundation_w: 2,
            foundation_h: 2,
            passenger_ids: vec![passenger_id],
        };

        assert_eq!(eject_destruction_garrison(&mut sim, &rules, &event), 1);
        assert!(
            sim.substrate
                .entities
                .get(building_id)
                .and_then(|building| building.passenger_role.cargo())
                .is_some_and(|cargo| cargo.is_empty()),
            "destroyed-garrison helper must detach cargo before building UnInit"
        );

        sim.uninit(building_id);
        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("detached occupant must survive the building UnInit");
        assert!(passenger.lifecycle.object_alive);
        assert!(passenger.is_alive());
        assert!(!passenger.lifecycle.in_limbo);
    }

    #[test]
    fn garrison_player_sell_no_exit_uses_inside_foundation_fallback() {
        let rules = garrison_edge_rules();
        let mut sim = Simulation::new();
        let building_id = 30;
        let passenger_id = 31;
        insert_captured_player_owned_garrison(&mut sim, building_id, passenger_id);
        block_all_garrison_exit_cells(&mut sim, 10, 10, 2, 2);
        if let Some(cargo) = sim
            .substrate
            .entities
            .get_mut(building_id)
            .and_then(|building| building.passenger_role.cargo_mut())
        {
            cargo.garrison_fire_index = 4;
        }
        let rng_before = sim.scenario_rng.state();

        assert_eq!(eject_garrison_occupants(&mut sim, &rules, building_id), 1);

        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("player-sell no-exit passenger should remain");
        assert_eq!((passenger.position.rx, passenger.position.ry), (11, 11));
        assert!(!passenger.dying);
        assert!(matches!(passenger.passenger_role, PassengerRole::None));
        assert_eq!(sim.scenario_rng.state(), rng_before);

        let cargo = sim
            .substrate
            .entities
            .get(building_id)
            .and_then(|building| building.passenger_role.cargo())
            .expect("building cargo remains present");
        assert!(cargo.is_empty());
        assert_eq!(cargo.garrison_fire_index, 0);
    }

    #[test]
    fn garrison_direct_scatter_uses_random_ranged_0_4_and_sets_destination() {
        let rules = garrison_edge_rules();
        let mut sim = Simulation::new();
        let building_id = 50;
        let passenger_id = insert_hidden_passenger(&mut sim, 51, building_id, "Neutral");
        give_walk_locomotor(&mut sim, passenger_id);
        let owner = sim.interner.intern("Americans");
        let mut expected_rng = sim.scenario_rng.clone();
        let _ = expected_rng.next_range_u32_inclusive(0, 4);

        let event = DestroyedGarrisonBuilding {
            building_id,
            type_id: sim.interner.intern("CAGAS01"),
            owner,
            rx: 10,
            ry: 10,
            z: 0,
            foundation_w: 2,
            foundation_h: 2,
            passenger_ids: vec![passenger_id],
        };

        assert_eq!(eject_destruction_garrison(&mut sim, &rules, &event), 1);
        assert_eq!(
            sim.scenario_rng.state(),
            expected_rng.state(),
            "direct Scatter must use scenario RandomRanged(0,4), not raw %8"
        );

        let passenger = sim.substrate.entities.get(passenger_id).unwrap();
        assert!(
            passenger.movement_target.is_some(),
            "successful direct Scatter should install a movement destination after RNG"
        );
        assert!(
            passenger.order_intent.is_none(),
            "Rust has no exact infantry MissionClass queue surface yet; do not fake mission 0xF as OrderIntent"
        );
    }

    #[test]
    fn garrison_destruction_no_exit_removes_without_rng_or_scatter() {
        let rules = garrison_edge_rules();
        let mut sim = Simulation::new();
        let building_id = 40;
        let passenger_id = insert_hidden_passenger(&mut sim, 41, building_id, "Neutral");
        let owner = sim.interner.intern("Americans");
        block_all_garrison_exit_cells(&mut sim, 10, 10, 2, 2);
        let rng_before = sim.scenario_rng.state();

        let event = DestroyedGarrisonBuilding {
            building_id,
            type_id: sim.interner.intern("CAGAS01"),
            owner,
            rx: 10,
            ry: 10,
            z: 0,
            foundation_w: 2,
            foundation_h: 2,
            passenger_ids: vec![passenger_id],
        };

        assert_eq!(eject_destruction_garrison(&mut sim, &rules, &event), 0);
        assert_eq!(sim.scenario_rng.state(), rng_before);

        let passenger = sim
            .substrate
            .entities
            .get(passenger_id)
            .expect("UnInit keeps the no-exit occupant resolvable until the drain");
        assert_eq!(passenger.health.current, 0);
        assert!(passenger.dying);
        assert!(matches!(passenger.passenger_role, PassengerRole::None));
        assert!(!passenger.lifecycle.object_alive);
        assert!(passenger.lifecycle.in_limbo);
        assert!(sim.substrate.pending_delete.contains(&passenger_id));

        sim.flush_pending_delete();
        assert!(sim.substrate.entities.get(passenger_id).is_none());
    }

    fn sell_eva_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n0=AMCV\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GAPOWR\n1=GACNST\n\n\
             [AMCV]\nStrength=450\nArmor=heavy\nSpeed=5\nDeploysInto=GACNST\n\n\
             [GAPOWR]\nStrength=100\nArmor=wood\nCost=800\n\n\
             [GACNST]\nStrength=1000\nArmor=wood\nCost=3000\nConstructionYard=yes\nUndeploysInto=AMCV\n",
        );
        RuleSet::from_ini(&ini).expect("sell eva rules should parse")
    }

    fn insert_structure(sim: &mut Simulation, id: u64, type_id: &str, owner: &str) {
        let mut entity = GameEntity::test_default(id, type_id, owner, 10, 10);
        entity.category = EntityCategory::Structure;
        entity.owner = sim.interner.intern(owner);
        entity.type_ref = sim.interner.intern(type_id);
        sim.substrate.entities.insert(entity);
    }

    fn sold_events(sim: &Simulation, owner: crate::sim::intern::InternedId) -> usize {
        sim.sound_events
            .iter()
            .filter(
                |event| matches!(event, SimSoundEvent::StructureSold { owner: o } if *o == owner),
            )
            .count()
    }

    /// `BuildingClass::Sell 0x00449CD1 MOV ECX,[EAX+0x408] ; TEST ; JNZ skip`:
    /// a type with `UndeploysInto=` never speaks.
    #[test]
    fn selling_announces_structure_sold_unless_the_type_undeploys() {
        let rules = sell_eva_rules();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        insert_structure(&mut sim, 1, "GAPOWR", "Americans");
        insert_structure(&mut sim, 2, "GACNST", "Americans");

        assert!(sell_building(&mut sim, &rules, 1));
        assert_eq!(
            sold_events(&sim, owner),
            1,
            "a power plant sale speaks once"
        );

        sim.sound_events.clear();
        assert!(sell_building(&mut sim, &rules, 2));
        assert_eq!(
            sold_events(&sim, owner),
            0,
            "a Construction Yard (UndeploysInto=) sale is silent"
        );
    }

    /// `BuildingClass::ToggleRepair 0x00447053..0x004470B7`: the line needs the
    /// flag to end up ON and `Health != Type.Strength`.
    #[test]
    fn repair_toggle_announces_only_when_switching_on_a_damaged_building() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        insert_structure(&mut sim, 1, "GAPOWR", "Americans");
        insert_structure(&mut sim, 2, "GAPOWR", "Americans");
        {
            let damaged = sim.substrate.entities.get_mut(1).expect("damaged plant");
            damaged.health = Health {
                current: 40,
                max: 100,
            };
            let intact = sim.substrate.entities.get_mut(2).expect("intact plant");
            intact.health = Health {
                current: 100,
                max: 100,
            };
        }
        let repairing = |sim: &Simulation| {
            sim.sound_events
                .iter()
                .filter(
                    |event| matches!(event, SimSoundEvent::Repairing { owner: o } if *o == owner),
                )
                .count()
        };

        assert!(toggle_repair(&mut sim, 1));
        assert_eq!(
            repairing(&sim),
            1,
            "switching repair on a damaged building speaks"
        );
        assert!(toggle_repair(&mut sim, 1));
        assert_eq!(repairing(&sim), 1, "switching it off is silent");
        assert!(toggle_repair(&mut sim, 2));
        assert_eq!(repairing(&sim), 1, "a building at full strength is silent");
    }
}
