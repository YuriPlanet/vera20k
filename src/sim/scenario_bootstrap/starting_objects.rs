//! Starting objects placed at each base: the MCV or Unholy/Ares base units,
//! the extra-unit budget and candidate lists, and the shared unlimbo-near-base
//! fallback search.

use super::{NativeStartBounds, NormalizedSkirmishSlot};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{PlacementEvidence, Simulation};
use crate::skirmish_launch::LaunchCountry;
use crate::util::direction::DIRECTION_DELTAS;

/// The Unlimbo facing of every starting object, the base unit and the extra
/// units alike: north. `MultiplayerGameMode::Create_Starting_Base_Unit @
/// 0x005D7030` pushes 0 at `0x005D708B`, and the fallback both callbacks share
/// (`Try_Unlimbo_Object_At_Or_Near_Cell @ 0x00688ED0`) pushes 0 at
/// `0x00688F90` (the seed cell) and `0x0068927F` (the ring cells).
pub const STARTING_MCV_FACING: u8 = 0;
pub const STARTING_MCV_FALLBACK_MAX_RADIUS: i32 = 31;
pub const STARTING_EXTRA_UNIT_FALLBACK_START_RADIUS: i32 = 4;

pub(super) fn place_starting_mcv(
    sim: &mut Simulation,
    mcv_type: &str,
    owner: &str,
    base_rx: u16,
    base_ry: u16,
    bounds: NativeStartBounds,
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Option<u64> {
    place_starting_object_near_base(
        sim,
        mcv_type,
        owner,
        base_rx,
        base_ry,
        STARTING_MCV_FACING,
        1,
        bounds,
        rules,
        resolved_terrain,
        overlay_registry,
    )
}

/// The first ring of Unholy Alliance's fallback placement (`PUSH 3` at
/// `0x005CB4F8`).
const UNHOLY_BASE_UNIT_FALLBACK_START_RADIUS: i32 = 3;

/// Unholy Alliance's base-unit callback (`0x005CB440`, the mode vtable's
/// `+0xC8` slot at `0x007EE8DC`), in place of the Battle one: the house gets
/// one unit of every `[General] BaseUnit=` type, last entry first
/// (`0x005CB44A..0x005CB453`, `0x005CB51A`), with no Owner test and no Bases
/// test. Entry 0 unlimbos at the base cell plus (3, 3), facing 0
/// (`0x005CB4B0..0x005CB4EA`). The other entries, and entry 0 when that
/// fails, go through `Try_Unlimbo_Object_At_Or_Near_Cell @ 0x00688ED0` at the
/// base cell from ring 3 (`0x005CB4F4..0x005CB506`). A unit that cannot be
/// built or finds no cell is deleted and the callback answers false, after
/// the remaining entries were still tried (`0x005CB472`, `0x005CB499`,
/// `0x005CB50F..0x005CB52B`). Returns the units placed and that answer.
///
/// The Scenario RNG takes one constructor draw per entry before its placement,
/// then the fallback's ring draws (one 0..7 direction per ring, four per
/// jitter probe).
///
/// A `BaseUnit=` name that is no vehicle type is skipped, as the Battle
/// lookup skips it: gamemd's list holds UnitTypes only
/// (`UnitTypeClass::FindOrAllocate @ 0x007480D0` allocates an empty one for
/// such a name), and VERA has no such type to build.
///
/// RESIDUAL: gamemd calls entry 0's Unlimbo directly, without the fallback's
/// playfield test (`0x00578460`); this admits that cell as the fallback
/// admits its seed cell. Trigger: an Unholy Alliance start whose base cell
/// plus (3, 3) is off the playfield. Effect: the first base unit takes a ring
/// cell where gamemd may put it on that cell. Frequency: rare; LAN and
/// online only.
pub(super) fn place_unholy_base_units(
    sim: &mut Simulation,
    owner: &str,
    base_rx: u16,
    base_ry: u16,
    bounds: NativeStartBounds,
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> (u32, bool) {
    let mut placed = 0;
    let mut complete = true;
    for (index, type_id) in rules.general.base_unit_types.iter().enumerate().rev() {
        if rules
            .object_in_category(crate::rules::object_type::ObjectCategory::Vehicle, type_id)
            .is_none()
        {
            continue;
        }
        let stable_id = if index == 0 {
            place_unholy_first_base_unit(
                sim,
                type_id,
                owner,
                base_rx,
                base_ry,
                bounds,
                rules,
                resolved_terrain,
                overlay_registry,
            )
        } else {
            place_starting_object_near_base(
                sim,
                type_id,
                owner,
                base_rx,
                base_ry,
                STARTING_MCV_FACING,
                UNHOLY_BASE_UNIT_FALLBACK_START_RADIUS,
                bounds,
                rules,
                resolved_terrain,
                overlay_registry,
            )
        };
        if stable_id.is_some() {
            placed += 1;
        } else {
            complete = false;
        }
    }
    (placed, complete)
}

/// Entry 0 of [`place_unholy_base_units`]: the base cell plus (3, 3), then
/// the shared fallback at the base cell.
fn place_unholy_first_base_unit(
    sim: &mut Simulation,
    type_id: &str,
    owner: &str,
    base_rx: u16,
    base_ry: u16,
    bounds: NativeStartBounds,
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Option<u64> {
    let (stable_id, category) = construct_starting_object(
        sim,
        type_id,
        owner,
        base_rx,
        base_ry,
        STARTING_MCV_FACING,
        rules,
        resolved_terrain,
    )?;
    // `ADD CX,3` / `ADD AX,3` at `0x005CB4B0..0x005CB4B4`, 16-bit.
    let (rx, ry) = (base_rx.wrapping_add(3), base_ry.wrapping_add(3));
    if unlimbo_starting_object_at(
        sim,
        stable_id,
        category,
        rx,
        ry,
        STARTING_MCV_FACING,
        rules,
        resolved_terrain,
        overlay_registry,
    ) {
        return Some(stable_id);
    }
    unlimbo_starting_object_near_base(
        sim,
        stable_id,
        category,
        base_rx,
        base_ry,
        STARTING_MCV_FACING,
        UNHOLY_BASE_UNIT_FALLBACK_START_RADIUS,
        bounds,
        rules,
        resolved_terrain,
        overlay_registry,
    )
}

/// Ares 3.0 MPGameMode_SpawnBaseUnit_BuildConst (1001F0B0, hook 0x5D7048,
/// in `MultiplayerGameMode::Create_Starting_Base_Unit @ 0x005D7030`): with
/// its country's `StartInMultiplayer.WithConst=` the house's base unit is the
/// first `[AI] BuildConst=` building it can expect to build
/// (HouseExt::FindBuildable, `ai_buildable::first_buildable_from_array`).
/// The yard is constructed and ForceMission(Guard)ed; a foundation over 2
/// cells in either axis moves BaseSpawnCell (`+0x5490`) one cell up-left;
/// a house no human controls then runs the AI half of a yard deploy
/// (`Simulation::refresh_ai_base_after_deploy`) at its base cell (the
/// alternate `+0x5494` unless empty, else BaseSpawnCell). The stock
/// placement (`0x005D707E`) then puts the yard at that cell, else near it.
/// `None` when the country has no WithConst; otherwise whether the yard
/// stands.
pub(super) fn ares_place_starting_construction_yard(
    sim: &mut Simulation,
    owner: &str,
    bounds: NativeStartBounds,
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Option<bool> {
    let owner_id = sim.interner.get(owner)?;
    let house = sim.houses.get(&owner_id)?;
    let country_name = sim.interner.resolve(house.country()?);
    if !rules.country_start_in_multiplayer_with_const(country_name) {
        return None;
    }
    let Some(yard) = crate::sim::ai_buildable::first_buildable_from_array(
        rules,
        rules.build_const_types(),
        country_name,
        house.side_index(),
        sim.session.game_options().super_weapons,
    ) else {
        log::error!(
            "House of country [{country_name}] cannot build anything from [General]BuildConst=."
        );
        return Some(false);
    };
    let (spawn_rx, spawn_ry) = house.base_center().unwrap_or((0, 0));
    let initial_z = resolved_terrain
        .cell(spawn_rx, spawn_ry)
        .map_or(0, |cell| cell.level);
    let Some(stable_id) = sim.construct_object_limbo_at_height(
        &yard.id,
        owner,
        spawn_rx,
        spawn_ry,
        STARTING_MCV_FACING,
        initial_z,
        rules,
        &mut crate::sim::world::FrameEffects::default(),
    ) else {
        return Some(false);
    };
    let _ = sim.substrate.mission_assign_exact(
        stable_id,
        crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard),
        sim.session.binary_frame(),
    );
    let (width, height) = yard.foundation.dimensions();
    let house = sim
        .houses
        .get_mut(&owner_id)
        .expect("launch house remains registered");
    if width > 2 || height > 2 {
        house.set_base_center((spawn_rx.wrapping_sub(1), spawn_ry.wrapping_sub(1)));
    }
    if !house.is_controlled_by_human(sim.session.rule_policy()) {
        let cell = house.base_origin();
        sim.refresh_ai_base_after_deploy(
            owner_id,
            cell,
            rules,
            overlay_registry,
            &mut crate::sim::world::FrameEffects::default(),
        );
    }
    let (rx, ry) = sim.houses[&owner_id].base_origin();
    Some(
        unlimbo_starting_object_near_base(
            sim,
            stable_id,
            crate::rules::object_type::ObjectCategory::Building,
            rx,
            ry,
            STARTING_MCV_FACING,
            1,
            bounds,
            rules,
            resolved_terrain,
            overlay_registry,
        )
        .is_some(),
    )
}

/// Ares 3.0 MPGameModeClass_CreateStartingUnits_BaseCenter (1001ED60, hook
/// 0x5D6F61), after a successful base-unit callback: each of the house's
/// construction yards (ConYards, `+0x54`) is QueueMission(Construction, 1)ed
/// and then, inside the placement scope (`0xA8E7AC`), Enter_Idle_Mode(0, 1)
/// queues Guard and the Idle body behind it. The yard opened during its
/// Unlimbo, before that Unlimbo's own Enter_Idle_Mode(1, 1) queued
/// Construction: a human's at its cell put (CellClass::AddContent's
/// DiscoveredBy(PlayerPtr), `0x0047E9DC`, outside a campaign), a computer's
/// at its owner discovery (`0x00440E76`, its current mission the forced
/// Guard). Opening set its ready byte, so the Construction commences here and
/// the first Update's Mission_Construction restarts the build-up. An emptied
/// BaseSpawnCell takes back its prior cell.
pub(super) fn ares_settle_starting_construction_yards(
    sim: &mut Simulation,
    owner: &str,
    saved_spawn_cell: Option<(u16, u16)>,
    rules: &RuleSet,
) {
    use crate::sim::mission::{MissionId, MissionType};
    let Some(owner_id) = sim.interner.get(owner) else {
        return;
    };
    let Some(house) = sim.houses.get(&owner_id) else {
        return;
    };
    let yards = house.build_const_order().to_vec();
    for id in yards {
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Construction),
            1,
            sim.session.binary_frame(),
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
        sim.with_object_placement_scope(|sim| sim.building_enter_idle_mode(id, false, rules));
        // The WithConst yard is unlimboed through the starting-object route
        // (`unlimbo_starting_object_near_base`), which — unlike the MCV
        // deploy's `unlimbo_constructed_parent` — leaves the body in
        // Construction. BuildingClass::Begin_Mode (`0x00447780`) applies a new
        // mode at once inside this scope (`0x004477A2..0x004477B0`), so the
        // Idle body is current here, not only queued.
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.initialize_building_idle_body(sim.session.binary_frame() as i32);
        }
    }
    let house = sim
        .houses
        .get_mut(&owner_id)
        .expect("launch house remains registered");
    if house.base_center().is_none_or(|cell| cell == (0, 0))
        && let Some(cell) = saved_spawn_cell
    {
        house.set_base_center(cell);
    }
}

pub(super) fn place_starting_object_near_base(
    sim: &mut Simulation,
    type_id: &str,
    owner: &str,
    base_rx: u16,
    base_ry: u16,
    facing: u8,
    start_radius: i32,
    bounds: NativeStartBounds,
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Option<u64> {
    let (stable_id, category) = construct_starting_object(
        sim,
        type_id,
        owner,
        base_rx,
        base_ry,
        facing,
        rules,
        resolved_terrain,
    )?;
    unlimbo_starting_object_near_base(
        sim,
        stable_id,
        category,
        base_rx,
        base_ry,
        facing,
        start_radius,
        bounds,
        rules,
        resolved_terrain,
        overlay_registry,
    )
}

/// One starting object constructed in limbo, with its category. Active
/// retail constructs the Techno before any Unlimbo, so its one constructor
/// draw precedes every placement-search draw and stays spent when every
/// attempt fails.
fn construct_starting_object(
    sim: &mut Simulation,
    type_id: &str,
    owner: &str,
    base_rx: u16,
    base_ry: u16,
    facing: u8,
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
) -> Option<(u64, crate::rules::object_type::ObjectCategory)> {
    let category = rules.object(type_id)?.category;
    let initial_z = resolved_terrain
        .cell(base_rx, base_ry)
        .map_or(0, |cell| cell.level);
    let stable_id = sim.construct_object_limbo_at_height(
        type_id,
        owner,
        base_rx,
        base_ry,
        facing,
        initial_z,
        rules,
        &mut crate::sim::world::FrameEffects::default(),
    )?;
    Some((stable_id, category))
}

/// The exact Unlimbo at the base cell, then the ring fallback
/// (`Try_Unlimbo_Object_At_Or_Near_Cell @ 0x00688ED0`), of one constructed
/// starting object; a total failure deletes it.
fn unlimbo_starting_object_near_base(
    sim: &mut Simulation,
    stable_id: u64,
    category: crate::rules::object_type::ObjectCategory,
    base_rx: u16,
    base_ry: u16,
    facing: u8,
    start_radius: i32,
    bounds: NativeStartBounds,
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Option<u64> {
    if unlimbo_starting_object_at(
        sim,
        stable_id,
        category,
        base_rx,
        base_ry,
        facing,
        rules,
        resolved_terrain,
        overlay_registry,
    ) {
        return Some(stable_id);
    }

    for radius in start_radius..=STARTING_MCV_FALLBACK_MAX_RADIUS {
        let start_direction = sim.scenario_rng.next_range_u32_inclusive(0, 7) as usize;
        for jitter_pass in 0..2 {
            for offset in 0..8 {
                let direction = (start_direction + offset) & 7;
                let (dx, dy) = DIRECTION_DELTAS[direction];
                let (mut rx, mut ry) = bounds.clamp(
                    i32::from(base_rx) + dx * radius,
                    i32::from(base_ry) + dy * radius,
                );

                if jitter_pass != 0 {
                    let x_amount = sim.scenario_rng.next_range_u32_inclusive(0, 1) as i32;
                    let x_sign = sim.scenario_rng.next_range_u32_inclusive(0, 99);
                    let y_amount = sim.scenario_rng.next_range_u32_inclusive(0, 1) as i32;
                    let y_sign = sim.scenario_rng.next_range_u32_inclusive(0, 99);
                    let jittered = bounds.clamp(
                        i32::from(rx) + if x_sign < 50 { x_amount } else { -x_amount },
                        i32::from(ry) + if y_sign < 50 { y_amount } else { -y_amount },
                    );
                    rx = jittered.0;
                    ry = jittered.1;
                }

                if (rx, ry) != (base_rx, base_ry)
                    && unlimbo_starting_object_at(
                        sim,
                        stable_id,
                        category,
                        rx,
                        ry,
                        facing,
                        rules,
                        resolved_terrain,
                        overlay_registry,
                    )
                {
                    return Some(stable_id);
                }
            }
        }
    }

    sim.discard_constructed_limbo(
        stable_id,
        rules,
        &mut crate::sim::world::FrameEffects::default(),
    );
    None
}

/// One starting-object Unlimbo at a cell the placement admits, at the cell's
/// level.
fn unlimbo_starting_object_at(
    sim: &mut Simulation,
    stable_id: u64,
    category: crate::rules::object_type::ObjectCategory,
    rx: u16,
    ry: u16,
    facing: u8,
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> bool {
    if !starting_object_cell_placeable(sim, resolved_terrain, rx, ry, category) {
        return false;
    }
    let z = resolved_terrain.cell(rx, ry).map_or(0, |cell| cell.level);
    sim.reveal_constructed_object_at_height_with_overlay_context(
        stable_id,
        rx,
        ry,
        facing,
        z,
        PlacementEvidence::EvaluateMark,
        rules,
        overlay_registry,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .is_some()
}

#[derive(Debug, Clone)]
pub struct StartingUnitCandidate {
    pub type_id: String,
    pub(crate) cost: i32,
}

pub const STARTING_EXTRA_UNIT_MAX_PLACEMENT_FAILURES: u8 = 20;

pub fn starting_unit_prefers_vehicle(spent: i32, budget: i32) -> bool {
    // gamemd 0x005D7337..0x005D7349 compares remaining budget against
    // trunc(initial_budget / 3). Expressed through `spent`, the strict
    // boundary is initial_budget - trunc(initial_budget / 3).
    spent < budget.wrapping_sub(budget / 3)
}

#[cfg(any(test, feature = "test-support"))]
pub fn seed_starting_extra_units(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    bounds: NativeStartBounds,
    unit_count: i32,
    initial_veteran: bool,
) -> u32 {
    seed_starting_extra_units_with_overlay_registry(
        sim,
        slots,
        rules,
        resolved_terrain,
        bounds,
        unit_count,
        initial_veteran,
        None,
    )
}

pub(super) fn seed_starting_extra_units_with_overlay_registry(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    bounds: NativeStartBounds,
    unit_count: i32,
    initial_veteran: bool,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> u32 {
    if unit_count <= 0 {
        return 0;
    }
    let ares = rules.ares_form();
    let budget = starting_unit_budget(
        rules,
        sim.houses.values().filter_map(|house| {
            house
                .country()
                .and_then(|name| rules.country_index(sim.interner.resolve(name)))
        }),
        sim.session.game_options().tech_level,
        unit_count,
    );
    if budget <= 0 {
        return 0;
    }
    let mut spawned = 0;

    for slot in slots {
        // 5D6EF0..F8 and5D6F4A..53 precede both starting-force callbacks.
        if slot.observer {
            continue;
        }
        let Some((base_center, tech_level, country_name)) =
            crate::sim::house_state::house_state_for_owner(
                &sim.houses,
                &slot.owner_name,
                &sim.interner,
            )
            .and_then(|house| {
                Some((
                    house.base_center()?,
                    house.tech_level(),
                    sim.interner.resolve(house.country()?),
                ))
            })
        else {
            continue;
        };
        if rules.country_multiplay_passive(country_name) {
            continue;
        }
        let Some(country) = rules.country_index(country_name) else {
            continue;
        };
        //5D70F0 compares the House's +1D4, not the global budget level.
        let candidates = starting_unit_candidates_for_country(
            rules,
            LaunchCountry::from_country_index(u32::from(country.0)),
            tech_level,
        );
        // 0x5D72AB refuses a house with neither kind of candidate. Ares'
        // empty `StartInMultiplayer.Types=` list instead returns success at
        // 0x5D743E (1001F220): nothing spawns and the budget is credited.
        let has_candidates = !(candidates.vehicles.is_empty() && candidates.infantry.is_empty());
        let refused = if ares {
            !has_candidates && !candidates.listed_empty
        } else {
            !has_candidates
        };
        if refused {
            continue;
        }
        let mut spent = 0i32;
        let mut placement_failures_left = STARTING_EXTRA_UNIT_MAX_PLACEMENT_FAILURES;
        while has_candidates && spent < budget && placement_failures_left != 0 {
            let prefer_vehicle = starting_unit_prefers_vehicle(spent, budget);
            // Ares 3.0 MPGameMode_SpawnStartingUnits_NoInfantry 1001F200
            // (hook 0x5D7337): without infantry candidates the house always
            // draws a vehicle (0x5D734F).
            let prefer_vehicle = prefer_vehicle || (ares && candidates.infantry.is_empty());
            let preferred = if prefer_vehicle {
                &candidates.vehicles
            } else {
                &candidates.infantry
            };
            let pool = if preferred.is_empty() {
                if prefer_vehicle {
                    &candidates.infantry
                } else {
                    break;
                }
            } else {
                preferred
            };
            let candidate_index =
                sim.scenario_rng
                    .next_range_u32_inclusive(0, pool.len() as u32 - 1) as usize;
            let candidate = &pool[candidate_index];
            let Some(stable_id) = place_starting_object_near_base(
                sim,
                &candidate.type_id,
                &slot.owner_name,
                base_center.0,
                base_center.1,
                STARTING_MCV_FACING,
                STARTING_EXTRA_UNIT_FALLBACK_START_RADIUS,
                bounds,
                rules,
                resolved_terrain,
                overlay_registry,
            ) else {
                placement_failures_left -= 1;
                continue;
            };

            // gamemd-derived: the starting-unit generator tests SpecialFlags
            // bit 9 (`InitialVeteran`, `TEST AH,0x2` on `[g_Scenario]` at
            // `0x005D73FD`) and calls `VeterancyStruct::SetElite(1) @
            // 0x007500B0` on the unit — 2.0f, ELITE, with no `Trainable=`
            // gate. Seeding the raw accumulator too is what keeps the unit
            // elite through its first kill instead of demoting it.
            if initial_veteran && let Some(entity) = sim.substrate.entities.get_mut(stable_id) {
                entity.veterancy.set_elite();
            }
            let mission = if slot.is_human {
                MissionType::Guard
            } else {
                MissionType::AreaGuard
            };
            let _ = sim.substrate.mission_assign_exact(
                stable_id,
                MissionId::from_known(mission),
                sim.session.binary_frame(),
            );
            spawned += 1;
            spent = spent.wrapping_add(candidate.cost);
        }
        // gamemd-derived: `MultiplayerGameMode__Generate_Starting_Units @
        // 0x005D6D80` hands each non-MultiplayPassive house its own copy of the
        // budget by reference; the extra-unit callback (`0x005D70F0`) subtracts
        // each placed unit's cost at `0x005D73EF..0x005D73F3`, and the caller
        // then reads the remainder at `0x005D6F91` and, when it is still > 0,
        // calls `HouseClass__Add_Credits @ 0x004F9950` (`0x005D6F99..0x005D6F9C`).
        // Human houses are included; the starting MCV is not charged.
        let remaining = budget.wrapping_sub(spent);
        if remaining > 0
            && let Some(house) = crate::sim::house_state::house_state_for_owner_mut(
                &mut sim.houses,
                &slot.owner_name,
                &sim.interner,
            )
            && !house.multiplay_passive()
        {
            house.economy_mut().add_credits(remaining);
        }
    }

    spawned
}

pub fn starting_unit_budget(
    rules: &RuleSet,
    countries: impl IntoIterator<Item = crate::rules::ruleset::CountryIdx>,
    tech_level: i32,
    unit_count: i32,
) -> i32 {
    if unit_count <= 0 {
        return 0;
    }
    if rules.ares_form() {
        ares_starting_unit_budget(rules, countries, tech_level, unit_count)
    } else {
        native_starting_unit_budget(rules, countries, tech_level, unit_count)
    }
}

/// Ares MPGameModeClass_CreateStartingUnits_UnitCost (1001EE10, hook
/// 0x5D6D9A), which replaces the average below: a set [General]
/// StartInMultiplayerUnitCost is the price of one unit, `UnitCount * cost`
/// in unsigned arithmetic. Unset, Ares averages a set of types: the Unit and
/// Infantry types of every `StartInMultiplayer.Types=` of a non-passive
/// country, then (AddTypes 1001E5C0, Unit array then Infantry array) each
/// AllowedToStartInMultiplayer type within the game's tech level that a
/// non-passive country without that list owns; every BaseUnit is then erased
/// from the set, the listed ones too. With no list it is the native average
/// (unsigned division aside).
fn ares_starting_unit_budget(
    rules: &RuleSet,
    countries: impl IntoIterator<Item = crate::rules::ruleset::CountryIdx>,
    tech_level: i32,
    unit_count: i32,
) -> i32 {
    use crate::rules::object_type::{ObjectCategory, ObjectType};
    // The std::set<TechnoTypeClass*>: each type once.
    fn insert<'a>(types: &mut Vec<&'a ObjectType>, object: &'a ObjectType) {
        if !types.iter().any(|known| std::ptr::eq(*known, object)) {
            types.push(object);
        }
    }
    if let Some(cost) = rules.ares_general().start_in_multiplayer_unit_cost() {
        return (unit_count as u32).wrapping_mul(cost as u32) as i32;
    }
    let mut owners = 0u32;
    let mut types = Vec::new();
    for country in countries {
        let Some(name) = rules.country_name(country) else {
            continue;
        };
        if rules.country_multiplay_passive(name) {
            continue;
        }
        let Some(listed) = rules.country_start_in_multiplayer_types(name) else {
            owners |= crate::sim::ai_buildable::country_bit(country);
            continue;
        };
        for reference in listed {
            if matches!(
                reference.category,
                ObjectCategory::Vehicle | ObjectCategory::Infantry
            ) && let Some(object) = rules.object_in_category(reference.category, &reference.id)
            {
                insert(&mut types, object);
            }
        }
    }
    for id in rules.vehicle_ids.iter().chain(&rules.infantry_ids) {
        let Some(object) = rules.object(id) else {
            continue;
        };
        if object.allowed_to_start_in_multiplayer
            && crate::sim::ai_buildable::owner_allows(object, owners, rules)
            && object.tech_level <= tech_level
        {
            insert(&mut types, object);
        }
    }
    types.retain(|object| {
        !rules
            .general
            .base_unit_types
            .iter()
            .any(|base_unit| base_unit.eq_ignore_ascii_case(&object.id))
    });
    let total = types
        .iter()
        .fold(0i32, |total, object| total.wrapping_add(object.cost));
    let count = (types.len() as u32).max(1);
    let cost = (count >> 1).wrapping_add(total as u32) / count;
    (unit_count as u32).wrapping_mul(cost) as i32
}

fn native_starting_unit_budget(
    rules: &RuleSet,
    countries: impl IntoIterator<Item = crate::rules::ruleset::CountryIdx>,
    tech_level: i32,
    unit_count: i32,
) -> i32 {
    // GenerateStartingUnits5D6D80:5D6DAF..5D6DCB ORs actual Country+B4
    // bits from non-passive Countries. Observer filtering is later; neither
    // ParentCountry nor production's Required/ForbiddenHouses enters this
    // budget or5D70F0's candidate lists. Executed controls:
    // tools/rules_oracle/session_country_starting_ownership.{json,md}.
    let country_mask = countries.into_iter().fold(0u32, |mask, country| {
        let Some(name) = rules.country_name(country) else {
            return mask;
        };
        if rules.country_multiplay_passive(name) {
            mask
        } else {
            mask | crate::sim::ai_buildable::country_bit(country)
        }
    });
    let mut total_cost: i32 = 0;
    let mut eligible_count: i32 = 0;
    for id in &rules.vehicle_ids {
        let Some(object) = rules.object(id) else {
            continue;
        };
        if !starting_unit_vehicle_allowed(rules, object, tech_level)
            || !crate::sim::ai_buildable::owner_allows(object, country_mask, rules)
        {
            continue;
        }
        eligible_count += 1i32;
        total_cost = total_cost.wrapping_add(object.cost);
    }
    for id in &rules.infantry_ids {
        let Some(object) = rules.object(id) else {
            continue;
        };
        if !starting_unit_infantry_allowed(object, tech_level)
            || !crate::sim::ai_buildable::owner_allows(object, country_mask, rules)
        {
            continue;
        }
        eligible_count += 1i32;
        total_cost = total_cost.wrapping_add(object.cost);
    }
    eligible_count = eligible_count.max(1);
    (eligible_count / 2)
        .wrapping_add(total_cost)
        .wrapping_div(eligible_count)
        .wrapping_mul(unit_count)
}

#[derive(Debug, Default)]
pub struct StartingUnitCandidates {
    pub vehicles: Vec<StartingUnitCandidate>,
    pub infantry: Vec<StartingUnitCandidate>,
    /// The country's `StartInMultiplayer.Types=` is set and empty, when Ares
    /// plays.
    pub listed_empty: bool,
}

pub fn starting_unit_candidates_for_country(
    rules: &RuleSet,
    country: LaunchCountry,
    tech_level: i32,
) -> StartingUnitCandidates {
    let Some(country_name) = rules.country_name(country.index()) else {
        return StartingUnitCandidates::default();
    };
    // Ares 3.0 MPGameMode_SpawnStartingUnits_Types 1001F220 (hook
    // 0x5D7163): a set `StartInMultiplayer.Types=` of the house's own
    // country replaces both scans. Its Unit and Infantry types are the
    // candidates in list order, without the AllowedToStartInMultiplayer,
    // Owner, tech-level or BaseUnit filters; other types are skipped.
    if rules.ares_form()
        && let Some(listed) = rules.country_start_in_multiplayer_types(country_name)
    {
        use crate::rules::object_type::ObjectCategory;
        let mut candidates = StartingUnitCandidates {
            listed_empty: listed.is_empty(),
            ..Default::default()
        };
        for reference in listed {
            let pool = match reference.category {
                ObjectCategory::Vehicle => &mut candidates.vehicles,
                ObjectCategory::Infantry => &mut candidates.infantry,
                _ => continue,
            };
            if let Some(object) = rules.object_in_category(reference.category, &reference.id) {
                pool.push(StartingUnitCandidate {
                    type_id: object.id.clone(),
                    cost: object.cost,
                });
            }
        }
        return candidates;
    }
    // GenerateStartingUnitsForHouse5D70F0 uses the actual Country+B4 bit,
    // unlike the MCV's separate505310 ParentCountry lookup. Reuse the one
    // shared Owner-mask decision; production admission is not called here.
    let country_mask = crate::sim::ai_buildable::country_bit(country.index());
    let vehicles = rules
        .vehicle_ids
        .iter()
        .filter_map(|id| {
            let object = rules.object(id)?;
            if !starting_unit_vehicle_allowed(rules, object, tech_level)
                || !crate::sim::ai_buildable::owner_allows(object, country_mask, rules)
            {
                return None;
            }
            Some(StartingUnitCandidate {
                type_id: id.clone(),
                cost: object.cost,
            })
        })
        .collect();
    let infantry = rules
        .infantry_ids
        .iter()
        .filter_map(|id| {
            let object = rules.object(id)?;
            if !starting_unit_infantry_allowed(object, tech_level)
                || !crate::sim::ai_buildable::owner_allows(object, country_mask, rules)
            {
                return None;
            }
            Some(StartingUnitCandidate {
                type_id: id.clone(),
                cost: object.cost,
            })
        })
        .collect();
    StartingUnitCandidates {
        vehicles,
        infantry,
        ..Default::default()
    }
}

fn starting_unit_vehicle_allowed(
    rules: &RuleSet,
    object: &crate::rules::object_type::ObjectType,
    tech_level: i32,
) -> bool {
    object.allowed_to_start_in_multiplayer
        && object.tech_level <= tech_level
        && !rules
            .general
            .base_unit_types
            .iter()
            .any(|base_unit| base_unit.eq_ignore_ascii_case(&object.id))
}

fn starting_unit_infantry_allowed(
    object: &crate::rules::object_type::ObjectType,
    tech_level: i32,
) -> bool {
    object.allowed_to_start_in_multiplayer && object.tech_level <= tech_level
}

fn starting_object_cell_placeable(
    sim: &Simulation,
    resolved_terrain: &ResolvedTerrainGrid,
    rx: u16,
    ry: u16,
    category: crate::rules::object_type::ObjectCategory,
) -> bool {
    // Acceptance is the retail isometric-diamond predicate: the mode's
    // starting-unit creator (vtable `+0xC8` @ `0x005D7030`) unlimbos at the
    // start coord, and its scan-place fallback @ `0x00688ED0` gates the seed
    // cell and every ring candidate with `MapClass::Is_Cell_In_Playfield(cell,
    // 1)`. The cell-array rect only clamps probe coordinates — it is never an
    // acceptance test.
    if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
        (i32::from(rx), i32::from(ry)),
        sim.playfield_bounds(),
        Some(resolved_terrain),
    ) {
        return false;
    }
    if let Some(occupancy) = sim.occupancy().get(rx, ry) {
        let compatible_infantry = category == crate::rules::object_type::ObjectCategory::Infantry
            && occupancy
                .occupants
                .first()
                .and_then(|occupant| sim.entities().get(occupant.entity_id))
                .is_some_and(|entity| entity.category() == EntityCategory::Infantry)
            && occupancy
                .occupants
                .iter()
                .filter(|occupant| occupant.sub_cell.is_some())
                .count()
                < 5;
        if !compatible_infantry {
            return false;
        }
    }
    let Some(cell) = resolved_terrain.cell(rx, ry) else {
        return false;
    };
    if cell.overlay_blocks || cell.terrain_object_blocks {
        return false;
    }
    if cell.has_bridge_deck && cell.bridge_walkable {
        return true;
    }
    cell.zone_type == crate::map::resolved_terrain::zone_class::GROUND
}

/// Battle5D7030 uses505310's Owner-only lookup, not production gates.
pub fn launch_mcv_type_for_country(country: LaunchCountry, rules: &RuleSet) -> Option<&str> {
    let name = rules.country_name(country.index())?;
    crate::sim::ai_buildable::first_owner_compatible(
        rules,
        &rules.general.base_unit_types,
        crate::rules::object_type::ObjectCategory::Vehicle,
        rules.country_owner_mask(name),
    )
    .map(|object| object.id.as_str())
}
