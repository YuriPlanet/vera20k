//! Scenario-start crate placement — gamemd `ScenarioClass::Post_Map_Init`.
//!
//! When the lobby "Crates" option is on (stock default), the scenario clamps the
//! player count between `[CrateRules] CrateMinimum` and `CrateMaximum` and calls
//! the map's random-cell crate placer exactly that many times. Each placer call
//! first looks for a free entry in the map's 256-entry crate-slot array and
//! returns immediately — spending no draws — if there is none. Otherwise it walks
//! a bounded retry loop; every attempt draws a random X then a random Y inside
//! the active Map rectangle, snaps the result to a nearby passable cell, and
//! submits the destination to the crate-specific native Mark transaction.
//!
//! The drawn cell decides the snap movement: water uses *float*, anything else
//! uses *track*. After snapping, the destination cell independently chooses
//! `WaterCrateImg` or `WoodCrateImg` from its own land type.
//!
//! This module owns startup placement and its persistent slot/timer result.
//! The [`runtime`] submodule owns the live slot clear, the identity-specific
//! overlay removal, and the per-tick `CrateRegen` scan. [`pickup`] owns outcome
//! selection; pickup effects and their movement integration remain unfinished.
//!
//! ## Dependency rules
//! Part of `sim/` — depends on `rules/`, `map/` grid types and other `sim/`
//! modules only. Never on render/, ui/, sidebar/, audio/, net/.

mod pickup;
mod runtime;
mod speed;
mod state;

pub(crate) use runtime::tick_crate_regeneration;
pub use state::{CrateAuthority, CrateSlot};

use crate::map::bridge_facts::{
    BRIDGE_FLAG_STRUCTURAL, BridgeFlagStamp, BridgeStampFamily, BridgeStampSlot,
    high_bridge_stamp_for_overlay,
};
use crate::map::lighting::LightingProfileUnits;
#[cfg(test)]
use crate::map::lighting::ParsedLightingProfiles;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::crate_rules::CrateRules;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::sim::cell_rect::cell_is_in_playfield_height_aware;
use crate::sim::find_nearby_cell::{
    NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell,
};
use crate::sim::occupancy::OBJECT_OCCUPATION_BIT;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

/// Retry budget inside one placer call. Every attempt spends its two draws
/// whether or not the cell works out; after this many failures the call gives up
/// and no crate is placed.
const MAX_PLACEMENT_ATTEMPTS: u32 = 1000;

/// Native hard cap supplied to the nearby-passable-cell search.
const CRATE_SNAP_RADIUS_CAP: u16 = 32;

/// Outcome of one scenario-start crate pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CratePlacement {
    /// Signed attempts selected by native minimum/human/maximum comparisons.
    pub requested: i32,
    /// Accepted calls, including timed ghosts whose overlay Mark failed.
    pub accepted: u32,
    /// Accepted calls that installed a visible overlay.
    pub visible: u32,
}

/// `min(max(CrateMinimum, player_count), CrateMaximum)`.
///
/// gamemd applies the floor first and the ceiling second, so a `CrateMaximum`
/// below `CrateMinimum` wins — the clamp is not symmetric and must not be
/// rewritten as a single `clamp()` call with the arguments in rules order.
pub fn scenario_start_crate_count(rules: &CrateRules, player_count: i32) -> i32 {
    rules.maximum.min(rules.minimum.max(player_count))
}

/// The human seat count the crate clamp is applied to.
///
/// gamemd's pregame setup assigns the crate-count global straight from the value
/// it logs as "Pregame setup for N players", and the sibling `Post_Map_Init`
/// budget gate adds the AI count to that same value — so it is human seats only.
/// In a 1v3 skirmish this is 1, not 4. Passive houses (Neutral, Special) are
/// never seats.
pub fn human_player_count(sim: &Simulation) -> i32 {
    i32::try_from(
        sim.houses
            .values()
            .filter(|house| house.is_human && !house.multiplay_passive)
            .count(),
    )
    .unwrap_or(i32::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OneCrateResult {
    HardRejected,
    AcceptedGhost,
    AcceptedVisible,
}

/// Native allocation, Unlimbo, and Mark failures all collapse to the same
/// gameplay result after the two hard prechecks: an accepted timed ghost.
/// Production invariants select `None`; focused tests inject the other cases
/// without importing gamemd's object allocator into simulation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
enum ForcedPostPrecheckFailure {
    #[default]
    None,
    Allocation,
    Unlimbo,
    Mark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcceptedCellResult {
    Ghost,
    Visible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CrateRandomFrame {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    radius_cap: u16,
}

/// Place the scenario-start crates.
///
/// `player_count` is the lobby *human* player count. gamemd clamps a session
/// global that the pregame-setup routine assigns from the same value it logs as
/// "Pregame setup for N players", and the sibling budget gate in `Post_Map_Init`
/// adds the AI count to it separately — so it counts human seats only, not
/// human + AI. Callers pass VERA's human, non-passive house count.
///
/// ## RNG
/// Both coordinates are drawn from the scenario stream — the placer loads the
/// scenario instance pointer and adds `0x218` before each `RandomRanged` call,
/// the same member `Gather_Start_Positions` binds and the one VERA models as
/// `scenario_rng`. Failed attempts spend X then Y. A successful overlay spends
/// one additional `RandomRanged(0, 0x7fff_fffe)` draw for its crate slot timer.
#[cfg(test)]
pub fn place_scenario_start_crates(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    path_grid: Option<&PathGrid>,
    player_count: i32,
) -> CratePlacement {
    place_scenario_start_crates_with_lighting(
        sim,
        rules,
        overlay_registry,
        path_grid,
        player_count,
        ParsedLightingProfiles::default().normal,
    )
}

/// Production entry carrying the already parsed ordinary ScenarioClass
/// lighting profile. `OverlayClass::Mark` copies each target CellClass's
/// `+0x10A` value into a spawned CellAnim after construction. The runtime
/// regeneration rung reaches the same Mark path every tick, so the shared
/// helpers below are no longer scenario-start-only.
pub(crate) fn place_scenario_start_crates_with_lighting(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    path_grid: Option<&PathGrid>,
    player_count: i32,
    lighting_profile: LightingProfileUnits,
) -> CratePlacement {
    place_scenario_start_crates_with_failure(
        sim,
        rules,
        overlay_registry,
        path_grid,
        player_count,
        lighting_profile,
        ForcedPostPrecheckFailure::None,
    )
}

fn place_scenario_start_crates_with_failure(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    path_grid: Option<&PathGrid>,
    player_count: i32,
    lighting_profile: LightingProfileUnits,
    forced_failure: ForcedPostPrecheckFailure,
) -> CratePlacement {
    if !sim.session.game_options.crates {
        return CratePlacement {
            requested: 0,
            accepted: 0,
            visible: 0,
        };
    }
    let requested = scenario_start_crate_count(&rules.crate_rules, player_count);
    if requested <= 0 {
        return CratePlacement {
            requested,
            accepted: 0,
            visible: 0,
        };
    }

    let mut accepted = 0u32;
    let mut visible = 0u32;
    for _ in 0..requested {
        match place_one_random_crate(
            sim,
            rules,
            overlay_registry,
            path_grid,
            lighting_profile,
            forced_failure,
        ) {
            OneCrateResult::HardRejected => {}
            OneCrateResult::AcceptedGhost => accepted = accepted.wrapping_add(1),
            OneCrateResult::AcceptedVisible => {
                accepted = accepted.wrapping_add(1);
                visible = visible.wrapping_add(1);
            }
        }
    }
    if visible != 0 {
        // Post_Map_Init publishes navigation before crate placement. Native
        // Mark mutates live CellClass land/zone/bridge state synchronously;
        // refresh Rust's derived bridge and path projections once after the
        // ordered crate batch, without introducing another RNG boundary.
        if let (Some(bridge_state), Some(terrain), Some(bounds), Some(size_height)) = (
            sim.bridge_state.as_ref(),
            sim.resolved_terrain.as_ref(),
            sim.playfield_bounds,
            sim.playfield_size_height,
        ) {
            let destroyable = bridge_state.is_destroyable();
            let bridge_strength = bridge_state.bridge_strength();
            sim.bridge_state = Some(
                crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain_with_map_size(
                    terrain,
                    destroyable,
                    bridge_strength,
                    (bounds.base, size_height),
                ),
            );
        }
        let _ = sim.rebuild_dynamic_navigation(rules);
    }
    log::info!(
        "Scenario-start crates: requested {requested}, accepted {accepted}, visible {visible}"
    );
    CratePlacement {
        requested,
        accepted,
        visible,
    }
}

/// Native water-vs-land classification, evaluated independently at the drawn
/// cell for movement and at the snapped destination for image selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrateSurface {
    Land,
    Water,
}

/// One `MapClass__PlaceCrateAtRandomCell @ 0x0056BD40` call.
fn place_one_random_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    path_grid: Option<&PathGrid>,
    lighting_profile: LightingProfileUnits,
    forced_failure: ForcedPostPrecheckFailure,
) -> OneCrateResult {
    let Some(slot_index) = sim.crate_authority.first_empty_index() else {
        // The native full-table return precedes every random draw.
        return OneCrateResult::HardRejected;
    };
    let Some(frame) = crate_random_frame(sim) else {
        return OneCrateResult::HardRejected;
    };
    for _ in 0..MAX_PLACEMENT_ATTEMPTS {
        let drawn = draw_crate_candidate(&mut sim.scenario_rng, frame);

        // The drawn cell's own land type selects the snap, before any snapping
        // happens. A draw outside the grid takes the land branch, matching the
        // native fallback cell.
        let movement_surface = crate_surface_at_packed(sim, drawn);

        let Some(cell) =
            snap_to_passable_with_radius(sim, path_grid, drawn, movement_surface, frame.radius_cap)
        else {
            continue;
        };
        if cell == (0, 0)
            || !cell_is_in_playfield_height_aware(
                (i32::from(cell.0), i32::from(cell.1)),
                sim.playfield_bounds,
                sim.resolved_terrain.as_ref(),
            )
        {
            continue;
        }
        // A cell that already carries ore, a wall, a bridge piece or another
        // crate cannot take one.
        if sim
            .overlay_grid
            .as_ref()
            .is_some_and(|grid| grid.cell(cell.0, cell.1).overlay_id.is_some())
        {
            continue;
        }
        let accepted = validate_and_stamp_candidate_with_rules_and_lighting(
            sim,
            rules,
            overlay_registry,
            cell,
            lighting_profile,
            forced_failure,
        );

        // `CrateSlot__PlaceOverlayAndInitTimer @ 0x004A17C0`: accepted ghosts
        // and visible overlays both write the packed coordinate before drawing
        // and installing start/aux/duration timer words.
        {
            let slot = sim.crate_authority.slot_mut(slot_index);
            slot.cell_x = cell.0 as i16;
            slot.cell_y = cell.1 as i16;
        }
        let timer_draw = sim.scenario_rng.next_range_u32_inclusive(0, 0x7fff_fffe);
        let (start_frame, aux, duration) = state::crate_timer_words(
            rules.crate_rules.regen,
            timer_draw,
            sim.session.binary_frame as i32,
        );
        let slot = sim.crate_authority.slot_mut(slot_index);
        slot.start_frame = start_frame;
        slot.aux = aux;
        slot.duration = duration;
        return match accepted {
            AcceptedCellResult::Ghost => OneCrateResult::AcceptedGhost,
            AcceptedCellResult::Visible => OneCrateResult::AcceptedVisible,
        };
    }
    OneCrateResult::HardRejected
}

/// Active Map rectangle used by random placement. `MapClass` constructs it as
/// `(1, 1, SizeW + SizeH - 1, SizeW + SizeH - 1)`; LocalSize and the canonical
/// Rust cell-array extent do not own these draws.
fn crate_random_frame(sim: &Simulation) -> Option<CrateRandomFrame> {
    let size_width = sim.playfield_bounds?.base;
    let size_height = sim.playfield_size_height?;
    let size_sum = size_width.wrapping_add(size_height);
    let width = size_sum.wrapping_sub(1);
    Some(CrateRandomFrame {
        left: 1,
        top: 1,
        width,
        height: width,
        // Native forwards min(SizeW + SizeH, 32) as a signed loop cap. A
        // nonpositive cap scans no FNPC rings, but the X/Y draws above still
        // occur. Zero is the Rust-native representation of that empty scan.
        radius_cap: size_sum.clamp(0, i32::from(CRATE_SNAP_RADIUS_CAP)) as u16,
    })
}

/// Signed `Random__RandomRanged` projection used by the crate rectangle.
/// Reversed endpoints are compared and swapped as signed dwords before the
/// shared native mask/rejection sampler sees their nonnegative span.
fn crate_random_ranged_i32(rng: &mut crate::sim::rng::SimRng, low: i32, high: i32) -> i32 {
    let (lo, hi) = if low <= high {
        (low, high)
    } else {
        (high, low)
    };
    let span = (i64::from(hi) - i64::from(lo)) as u32;
    lo.wrapping_add(rng.next_range_u32_inclusive(0, span) as i32)
}

/// `MapClass__PlaceCrateAtRandomCell @ 0x0056BD8B..0x0056BDD3` always draws
/// X then Y, adds the signed rectangle origin with dword wrapping, and stores
/// each result through a 16-bit word before the later MOVSX reads it back.
fn draw_crate_candidate(rng: &mut crate::sim::rng::SimRng, frame: CrateRandomFrame) -> (i32, i32) {
    let x = frame
        .left
        .wrapping_add(crate_random_ranged_i32(rng, 0, frame.width.wrapping_sub(1)));
    let y = frame.top.wrapping_add(crate_random_ranged_i32(
        rng,
        0,
        frame.height.wrapping_sub(1),
    ));
    (x as i16 as i32, y as i16 as i32)
}

#[cfg(test)]
fn validate_and_stamp_candidate(
    sim: &mut Simulation,
    rules: &CrateRules,
    overlay_registry: &OverlayTypeRegistry,
    cell: (u16, u16),
    forced_failure: ForcedPostPrecheckFailure,
) -> AcceptedCellResult {
    validate_and_stamp_candidate_inner(
        sim,
        rules,
        None,
        overlay_registry,
        cell,
        ParsedLightingProfiles::default().normal,
        forced_failure,
    )
}

#[cfg(test)]
fn validate_and_stamp_candidate_with_rules(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    cell: (u16, u16),
    forced_failure: ForcedPostPrecheckFailure,
) -> AcceptedCellResult {
    validate_and_stamp_candidate_with_rules_and_lighting(
        sim,
        rules,
        overlay_registry,
        cell,
        ParsedLightingProfiles::default().normal,
        forced_failure,
    )
}

fn validate_and_stamp_candidate_with_rules_and_lighting(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    cell: (u16, u16),
    lighting_profile: LightingProfileUnits,
    forced_failure: ForcedPostPrecheckFailure,
) -> AcceptedCellResult {
    validate_and_stamp_candidate_inner(
        sim,
        &rules.crate_rules,
        Some(rules),
        overlay_registry,
        cell,
        lighting_profile,
        forced_failure,
    )
}

fn validate_and_stamp_candidate_inner(
    sim: &mut Simulation,
    rules: &CrateRules,
    full_rules: Option<&RuleSet>,
    overlay_registry: &OverlayTypeRegistry,
    cell: (u16, u16),
    lighting_profile: LightingProfileUnits,
    forced_failure: ForcedPostPrecheckFailure,
) -> AcceptedCellResult {
    let water_id = rules
        .water_crate_img
        .as_deref()
        .and_then(|name| overlay_registry.id_for_name(name));
    let wood_id = rules
        .wood_crate_img
        .as_deref()
        .and_then(|name| overlay_registry.id_for_name(name));
    let crate_id = rules
        .crate_img
        .as_deref()
        .and_then(|name| overlay_registry.id_for_name(name));
    let selected_id = match crate_surface_at(sim, cell) {
        CrateSurface::Water => water_id,
        CrateSurface::Land => wood_id,
    };
    let Some(selected_id) = selected_id else {
        return AcceptedCellResult::Ghost;
    };

    if forced_failure != ForcedPostPrecheckFailure::None {
        return AcceptedCellResult::Ghost;
    }

    let Some(selected_flags) = overlay_registry.flags(selected_id).cloned() else {
        return AcceptedCellResult::Ghost;
    };
    // `OverlayClass::OverlayClass @ 0x005FC380` performs the TerrainClass scan
    // before calling Unlimbo. A hit skips Unlimbo, so Mark never reaches its
    // slope, bridge, CellAnim, or common RecalcAttributes paths.
    if sim.production.terrain_object_cells.contains_key(&cell) {
        return AcceptedCellResult::Ghost;
    }
    let Some(slope_type) = sim
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .map(|terrain_cell| terrain_cell.slope_type)
    else {
        return AcceptedCellResult::Ghost;
    };
    // `OverlayClass::Mark @ 0x005FC5E0..0x005FC5F4`: every derived branch,
    // including the hard-coded bridge and TS-legacy identities, shares this
    // gate before any overlay write or branch-local Scenario draw.
    if slope_type > 4 && selected_id != 0xB2 {
        return AcceptedCellResult::Ghost;
    }

    // The four high-anchor identities call their bridge setter first, then
    // continue through Mark's ordinary precedence. The setters write raw data
    // on anchor/F1/F2/opposite even before any overlay identity exists.
    let high_bridge_data = high_bridge_stamp_for_overlay(selected_id).map(|(family, direction)| {
        let stamp = BridgeFlagStamp::new(cell, direction, true);
        let data = if direction == 0 { 0 } else { 9 };
        apply_high_bridge_crate_setter(sim, stamp, family, data);
        data
    });

    // GSI-18.01 is explicitly excluded by the Phase 14 contract: these two
    // hard-coded branches are the TS veins/veinhole carryover. Preserve crate
    // admission/timer behavior but do not port their legacy world mutation.
    if matches!(selected_id, 0x7E | 0xA7) {
        recalc_real_crate_mark_cell(sim, overlay_registry, cell);
        return AcceptedCellResult::Ghost;
    }

    if selected_flags.land == LandType::Railroad {
        let wrote = write_real_crate_mark_fields(sim, overlay_registry, cell, selected_id, 0);
        recalc_real_crate_mark_cell(sim, overlay_registry, cell);
        return if wrote {
            AcceptedCellResult::Visible
        } else {
            AcceptedCellResult::Ghost
        };
    }

    if selected_flags.wall {
        // `Cell_passability_building_placement @ 0x0047C620` receives
        // `(cell, 1, 0, 0)` here. The crate caller already proved allocated,
        // in-playfield, and overlay-empty; with the null object argument the
        // surviving admission is Track speed, not terrain-object/occupation.
        let wall_passes = sim
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(cell.0, cell.1))
            .is_some_and(|terrain_cell| terrain_cell.speed_costs.track != Some(0));
        if !wall_passes {
            return AcceptedCellResult::Ghost;
        }
        let wrote = write_real_crate_mark_fields(sim, overlay_registry, cell, selected_id, 0);
        if wrote && let Some(grid) = sim.overlay_grid.as_mut() {
            crate::sim::overlay_grid::refresh_wall_connectivity_after_placement(
                grid,
                overlay_registry,
                sim.resolved_terrain.as_mut(),
                cell.0,
                cell.1,
            );
        }
        recalc_real_crate_mark_cell(sim, overlay_registry, cell);
        return if wrote {
            AcceptedCellResult::Visible
        } else {
            AcceptedCellResult::Ghost
        };
    }

    if let Some(spec) = LowBridgeCrateSpec::for_trigger(selected_id) {
        let visible = execute_low_bridge_crate_mark(sim, overlay_registry, cell, spec);
        return if visible {
            AcceptedCellResult::Visible
        } else {
            AcceptedCellResult::Ghost
        };
    }

    // `OverlayClass__Mark @ 0x005FC570` compares live Rules pointers, with
    // Water first. Numeric registry identity is the Rust-native pointer alias.
    let mark_speed = if Some(selected_id) == water_id {
        SpeedType::Float
    } else if Some(selected_id) == crate_id || Some(selected_id) == wood_id {
        SpeedType::Track
    } else {
        return AcceptedCellResult::Ghost;
    };

    // `OverlayClass::Mark @ 0x005FCFE7..0x005FD003` forces passability true
    // for the four high-anchor IDs after their setter. This is an explicit
    // identity bypass, not an inferred consequence of selecting deck facts;
    // even a nonzero deck occupation byte does not reject the ordinary tail.
    if high_bridge_data.is_none() {
        let Some((bridge_layer_selected, selected_speed_cost)) = sim
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(cell.0, cell.1))
            .map(|terrain_cell| {
                (
                    terrain_cell.bridge_facts.raw_flags & BRIDGE_FLAG_STRUCTURAL != 0,
                    terrain_cell.speed_costs.cost_for_speed_type(mark_speed),
                )
            })
        else {
            return AcceptedCellResult::Ghost;
        };
        let selected_occupation = if bridge_layer_selected {
            sim.substrate.raw_cell_occupation.deck_bits(cell.0, cell.1)
        } else {
            sim.substrate
                .raw_cell_occupation
                .ground_bits(cell.0, cell.1)
        };
        // `CellClass::CheckCellPassability @ 0x004834A0` applies both native
        // occupation filters; their intersection is exactly bit 0x40. Other
        // raw occupation bits do not reject this Mark admission.
        if selected_occupation & OBJECT_OCCUPATION_BIT != 0 {
            if let Some(full_rules) = full_rules {
                spawn_crate_cell_anim(
                    sim,
                    full_rules,
                    overlay_registry,
                    cell,
                    selected_flags.cell_anim.as_deref(),
                    lighting_profile,
                );
            }
            recalc_real_crate_mark_cell(sim, overlay_registry, cell);
            return AcceptedCellResult::Ghost;
        }
        if !bridge_layer_selected && selected_speed_cost == Some(0) {
            if let Some(full_rules) = full_rules {
                spawn_crate_cell_anim(
                    sim,
                    full_rules,
                    overlay_registry,
                    cell,
                    selected_flags.cell_anim.as_deref(),
                    lighting_profile,
                );
            }
            recalc_real_crate_mark_cell(sim, overlay_registry, cell);
            return AcceptedCellResult::Ghost;
        }
    }

    let wrote = write_real_crate_mark_fields(
        sim,
        overlay_registry,
        cell,
        selected_id,
        high_bridge_data.unwrap_or(0),
    );
    if wrote && selected_flags.land == LandType::Road {
        set_crate_mark_data(sim, cell, 1);
        if let Some(full_rules) = full_rules {
            spread_cell_germinate_without_randomization(
                sim,
                full_rules,
                overlay_registry,
                (cell.0 as i16, cell.1 as i16),
            );
        }
    }
    if wrote && selected_flags.crate_type {
        set_crate_mark_data(sim, cell, u8::MAX);
    }
    if let Some(full_rules) = full_rules {
        spawn_crate_cell_anim(
            sim,
            full_rules,
            overlay_registry,
            cell,
            selected_flags.cell_anim.as_deref(),
            lighting_profile,
        );
    }
    recalc_real_crate_mark_cell(sim, overlay_registry, cell);
    if wrote
        && sim
            .overlay_grid
            .as_ref()
            .is_some_and(|grid| grid.cell(cell.0, cell.1).overlay_id.is_some())
    {
        AcceptedCellResult::Visible
    } else {
        AcceptedCellResult::Ghost
    }
}

#[derive(Debug, Clone, Copy)]
struct LowBridgeCrateSpec {
    start_offset: (i16, i16),
    row_direction: u8,
    join_direction: u8,
    fixed_id: u8,
    opposite_id: u8,
    body_base: u8,
}

impl LowBridgeCrateSpec {
    /// Dense active-YR runtime IDs from the literal tables read by
    /// `OverlayClass::Mark @ 0x005FC790..0x005FD1FA`.
    fn for_trigger(id: u8) -> Option<Self> {
        Some(match id {
            0x7A => Self::new((0, -1), 4, 6, 0x5C, 0x5E, 0x4A),
            0x7B => Self::new((0, -1), 4, 2, 0x5E, 0x5C, 0x4A),
            0x7C => Self::new((-1, 0), 2, 4, 0x60, 0x62, 0x53),
            0x7D => Self::new((-1, 0), 2, 0, 0x62, 0x60, 0x53),
            0xE9 => Self::new((0, -1), 4, 6, 0xDF, 0xE1, 0xCD),
            0xEA => Self::new((0, -1), 4, 2, 0xE1, 0xDF, 0xCD),
            0xEB => Self::new((-1, 0), 2, 4, 0xE3, 0xE5, 0xD6),
            0xEC => Self::new((-1, 0), 2, 0, 0xE5, 0xE3, 0xD6),
            _ => return None,
        })
    }

    const fn new(
        start_offset: (i16, i16),
        row_direction: u8,
        join_direction: u8,
        fixed_id: u8,
        opposite_id: u8,
        body_base: u8,
    ) -> Self {
        Self {
            start_offset,
            row_direction,
            join_direction,
            fixed_id,
            opposite_id,
            body_base,
        }
    }
}

#[derive(Debug, Clone)]
enum CrateMarkCellRef {
    Real(u16, u16),
    Dummy(crate::map::resolved_terrain::SharedCellDummy),
}

fn resolve_crate_mark_cell(sim: &Simulation, cell: (i16, i16)) -> CrateMarkCellRef {
    let x = i32::from(cell.0);
    let y = i32::from(cell.1);
    if let Some((rx, ry)) = crate::map::cell_index::canonical_cell_coord(x, y)
        && sim
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(rx, ry))
            .is_some()
        && sim
            .overlay_grid
            .as_ref()
            .is_some_and(|grid| rx < grid.width() && ry < grid.height())
    {
        return CrateMarkCellRef::Real(rx, ry);
    }
    let dummy = sim.effective_shared_cell_dummy();
    dummy.stamp_coord(x, y);
    CrateMarkCellRef::Dummy(dummy)
}

fn read_crate_mark_fields(sim: &Simulation, cell: (i16, i16)) -> (Option<u8>, u8) {
    match resolve_crate_mark_cell(sim, cell) {
        CrateMarkCellRef::Real(rx, ry) => {
            let cell = sim
                .overlay_grid
                .as_ref()
                .expect("real crate Mark cell requires OverlayGrid")
                .cell(rx, ry);
            (cell.overlay_id, cell.overlay_data)
        }
        CrateMarkCellRef::Dummy(dummy) => dummy.overlay_fields(),
    }
}

fn write_crate_mark_fields(
    sim: &mut Simulation,
    registry: &OverlayTypeRegistry,
    cell: (i16, i16),
    overlay_id: u8,
    overlay_data: u8,
) -> bool {
    match resolve_crate_mark_cell(sim, cell) {
        CrateMarkCellRef::Real(rx, ry) => {
            write_real_crate_mark_fields(sim, registry, (rx, ry), overlay_id, overlay_data)
        }
        CrateMarkCellRef::Dummy(dummy) => {
            dummy.set_overlay_fields(Some(overlay_id), overlay_data);
            false
        }
    }
}

fn write_crate_mark_raw_data(sim: &mut Simulation, cell: (i16, i16), overlay_data: u8) {
    match resolve_crate_mark_cell(sim, cell) {
        CrateMarkCellRef::Real(rx, ry) => {
            let (Some(grid), Some(terrain)) =
                (sim.overlay_grid.as_mut(), sim.resolved_terrain.as_mut())
            else {
                return;
            };
            let _ = grid.write_crate_mark_data_field(terrain, rx, ry, overlay_data);
        }
        CrateMarkCellRef::Dummy(dummy) => {
            let (overlay_id, _) = dummy.overlay_fields();
            dummy.set_overlay_fields(overlay_id, overlay_data);
        }
    }
}

fn apply_high_bridge_crate_setter(
    sim: &mut Simulation,
    stamp: BridgeFlagStamp,
    family: BridgeStampFamily,
    overlay_data: u8,
) {
    let Some(slots) = stamp.slots() else {
        return;
    };
    // `SetBridgeDirection_NESW/NWSE @ 0x0047E040/0x0047E470` write the
    // data byte on exactly these four visits. Perform those writes before the
    // flag transaction so the shared dummy ends on the setter's final visit.
    for (slot, coord) in slots {
        if !matches!(
            slot,
            BridgeStampSlot::Anchor
                | BridgeStampSlot::Forward1
                | BridgeStampSlot::Forward2
                | BridgeStampSlot::Opposite
        ) {
            continue;
        }
        let Some((x, y)) = coord else {
            continue;
        };
        write_crate_mark_raw_data(sim, (x as i16, y as i16), overlay_data);
    }
    sim.apply_runtime_bridge_mark_stamp(stamp, family);
}

fn write_real_crate_mark_fields(
    sim: &mut Simulation,
    registry: &OverlayTypeRegistry,
    cell: (u16, u16),
    overlay_id: u8,
    overlay_data: u8,
) -> bool {
    let (Some(grid), Some(terrain)) = (sim.overlay_grid.as_mut(), sim.resolved_terrain.as_mut())
    else {
        return false;
    };
    grid.write_crate_mark_fields(terrain, registry, cell.0, cell.1, overlay_id, overlay_data)
}

fn recalc_real_crate_mark_cell(
    sim: &mut Simulation,
    registry: &OverlayTypeRegistry,
    cell: (u16, u16),
) {
    let (Some(grid), Some(terrain)) = (sim.overlay_grid.as_mut(), sim.resolved_terrain.as_mut())
    else {
        return;
    };
    grid.recalculate_runtime_cell(
        terrain,
        registry,
        cell,
        crate::sim::overlay_grid::NavigationPublication::NextPathReader,
    );
}

fn set_crate_mark_data(sim: &mut Simulation, cell: (u16, u16), data: u8) {
    let wrote = if let Some(grid) = sim.overlay_grid.as_mut() {
        let target = grid.cell_mut(cell.0, cell.1);
        if target.overlay_id.is_some() {
            target.overlay_data = data;
            true
        } else {
            false
        }
    } else {
        false
    };
    if wrote && let Some(terrain) = sim.resolved_terrain.as_mut() {
        let _ = terrain.set_runtime_overlay_bridge_state_byte(cell.0, cell.1, data);
    }
}

fn packed_step(cell: (i16, i16), direction: u8) -> (i16, i16) {
    let (dx, dy) = crate::util::direction::direction_delta(direction)
        .expect("low bridge tables use cardinal directions");
    (
        cell.0.wrapping_add(dx as i16),
        cell.1.wrapping_add(dy as i16),
    )
}

fn packed_offset(cell: (i16, i16), offset: (i16, i16)) -> (i16, i16) {
    (cell.0.wrapping_add(offset.0), cell.1.wrapping_add(offset.1))
}

/// `MapClass::In_Bounds @ 0x00568300` — the active diamond test against
/// `MapClass+0xF4` (size width) and `MapClass+0xF8` (size height). The low
/// bridge search and `CrateSlot__RemoveCrateOverlayFromCell @ 0x004A1AA0` both
/// call it before touching a CellClass.
fn map_cell_in_bounds(sim: &Simulation, cell: (i16, i16)) -> bool {
    let (Some(bounds), Some(height)) = (sim.playfield_bounds, sim.playfield_size_height) else {
        return false;
    };
    let x = i32::from(cell.0);
    let y = i32::from(cell.1);
    let sum = x.wrapping_add(y);
    let width = bounds.base;
    width < sum
        && x.wrapping_sub(y) < width
        && y.wrapping_sub(x) < width
        && sum <= width.wrapping_add(height.wrapping_mul(2))
}

fn execute_low_bridge_crate_mark(
    sim: &mut Simulation,
    registry: &OverlayTypeRegistry,
    origin: (u16, u16),
    spec: LowBridgeCrateSpec,
) -> bool {
    let origin = (origin.0 as i16, origin.1 as i16);
    let row_start = packed_offset(origin, spec.start_offset);
    let mut probe = row_start;
    let mut row_clear = true;
    for _ in 0..3 {
        let (overlay_id, _) = read_crate_mark_fields(sim, probe);
        row_clear &= overlay_id.is_none();
        probe = packed_step(probe, spec.row_direction);
    }

    if row_clear {
        let mut target = row_start;
        for data in 0..3u8 {
            let _ = write_crate_mark_fields(sim, registry, target, spec.fixed_id, data);
            if let CrateMarkCellRef::Real(rx, ry) = resolve_crate_mark_cell(sim, target) {
                recalc_real_crate_mark_cell(sim, registry, (rx, ry));
            }
            target = packed_step(target, spec.row_direction);
        }

        let mut search = packed_step(origin, spec.join_direction);
        let mut found = None;
        while map_cell_in_bounds(sim, search) {
            let fields = read_crate_mark_fields(sim, search);
            if fields == (Some(spec.opposite_id), 1) {
                found = Some(search);
                break;
            }
            search = packed_step(search, spec.join_direction);
        }

        if let Some(found) = found {
            let reverse = spec.join_direction.wrapping_sub(4) & 7;
            let mut work = packed_step(found, reverse);
            let dx = i32::from(work.0).wrapping_sub(i32::from(row_start.0));
            let dy = i32::from(work.1).wrapping_sub(i32::from(row_start.1));
            let length = dx.wrapping_abs().max(dy.wrapping_abs());
            let cross_offsets: [(i16, i16); 3] = if matches!(reverse, 0 | 4) {
                [(-1, 0), (0, 0), (1, 0)]
            } else {
                [(0, -1), (0, 0), (0, 1)]
            };
            for _ in 0..length {
                for (data, offset) in cross_offsets.into_iter().enumerate() {
                    let target = packed_offset(work, offset);
                    let variant = (sim.scenario_rng.next_u32() & 3) as u8;
                    let overlay_id = spec.body_base.wrapping_add(variant);
                    let _ = write_crate_mark_fields(sim, registry, target, overlay_id, data as u8);
                    if let CrateMarkCellRef::Real(rx, ry) = resolve_crate_mark_cell(sim, target) {
                        recalc_real_crate_mark_cell(sim, registry, (rx, ry));
                    }
                }
                work = packed_step(work, reverse);
            }
        }
    }

    let origin_real = (origin.0 as u16, origin.1 as u16);
    recalc_real_crate_mark_cell(sim, registry, origin_real);
    sim.overlay_grid
        .as_ref()
        .is_some_and(|grid| grid.cell(origin_real.0, origin_real.1).overlay_id.is_some())
}

/// Crate Mark seam of `CellClass::SpreadCellGerminate @ 0x004818E0` (argument
/// 0), called synchronously by `OverlayClass::Mark @ 0x005FD0EC` for a Land-5
/// overlay. The receiver and every neighbour resolve through the crate Mark
/// real-or-dummy lookup; the shared helper in `sim::tiberium_germinate` owns
/// the native neighbour order, table, and modulo.
fn spread_cell_germinate_without_randomization(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    cell: (i16, i16),
) {
    let (receiver_id, _) = read_crate_mark_fields(sim, cell);
    let germinated = {
        let view: &Simulation = sim;
        crate::sim::tiberium_germinate::spread_cell_germinate_without_randomization(
            &rules.tiberium_types,
            registry,
            receiver_id,
            cell,
            |neighbor| read_crate_mark_fields(view, neighbor),
        )
    };
    let Some(germinated) = germinated else {
        return;
    };
    match resolve_crate_mark_cell(sim, cell) {
        CrateMarkCellRef::Real(rx, ry) => set_crate_mark_data(sim, (rx, ry), germinated.density),
        CrateMarkCellRef::Dummy(dummy) => dummy.set_overlay_fields(receiver_id, germinated.density),
    }
}

fn spawn_crate_cell_anim(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    cell: (u16, u16),
    cell_anim: Option<&str>,
    lighting_profile: LightingProfileUnits,
) {
    let Some(cell_anim) = cell_anim else {
        return;
    };
    let center_x = i32::from(cell.0).wrapping_mul(256).wrapping_add(128);
    let center_y = i32::from(cell.1).wrapping_mul(256).wrapping_add(128);
    let (ground_z, level) = sim
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .and_then(|terrain_cell| {
            crate::util::lepton::ground_height_leptons(
                terrain_cell.level,
                terrain_cell.slope_type,
                center_x,
                center_y,
            )
            .ok()
            .map(|ground_z| (ground_z, terrain_cell.level))
        })
        .unwrap_or((0, 0));
    // `OverlayClass::Mark @ 0x005FD1B9..0x005FD1F4` calls
    // `CellClass::GetTiberiumType` with ECX = the target CellClass. Only a
    // successfully installed tiberium identity enters the palette/+0xFC
    // post-write block; a failed or non-tiberium Mark leaves constructor zero.
    let tiberium_type = sim
        .overlay_grid
        .as_ref()
        .and_then(|grid| grid.cell(cell.0, cell.1).overlay_id)
        .and_then(|overlay_id| {
            overlay_registry.tiberium_type_for_overlay(&rules.tiberium_types, overlay_id)
        })
        .and_then(|type_id| rules.tiberium_types.get(type_id));
    let remap_color = tiberium_type
        .and_then(|tiberium| tiberium.color.as_deref())
        .and_then(|color| {
            crate::rules::color_scheme::scheme_entry_by_name(&rules.color_schemes, color)
        })
        .and_then(|entry| u8::try_from(entry).ok())
        .map(crate::rules::house_colors::HouseColorIndex);
    let z_adjust = tiberium_type
        .map(|_| crate::map::lighting::cell_ground_z_adjust(lighting_profile, level))
        .unwrap_or(0);
    let type_name = sim.interner.intern(cell_anim);
    let mut descriptor = crate::sim::components::AnimClassSpawnDescriptor::new(
        type_name,
        cell.0,
        cell.1,
        SimFixed::from_num(0),
        SimFixed::from_num(0),
        0,
    );
    descriptor.delay = 0;
    descriptor.loop_count = 1;
    descriptor.draw_flags = 0x600;
    descriptor.z_adjust = 0;
    descriptor.reverse = false;
    let anim_id = sim
        .spawn_anim_at_world(
            rules,
            descriptor,
            crate::sim::anim_class::AnimWorldCoord {
                x: center_x.wrapping_add(0x180),
                y: center_y.wrapping_add(0x180),
                z: ground_z,
            },
        )
        .unwrap_or_else(|error| {
            panic!("crate CellAnim [{cell_anim}] must be bound before OverlayClass::Mark: {error}")
        });
    assert!(
        sim.set_cell_anim_draw_authority(anim_id, remap_color, z_adjust),
        "crate CellAnim {anim_id} disappeared before OverlayClass post-constructor writes"
    );
}

fn crate_surface_at(sim: &Simulation, cell: (u16, u16)) -> CrateSurface {
    if sim
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .is_some_and(|cell| cell.yr_cell_land_type == LandType::Water.as_index())
    {
        CrateSurface::Water
    } else {
        CrateSurface::Land
    }
}

fn crate_surface_at_packed(sim: &Simulation, cell: (i32, i32)) -> CrateSurface {
    let Some((rx, ry)) = crate::map::cell_index::canonical_cell_coord(cell.0, cell.1) else {
        return CrateSurface::Land;
    };
    crate_surface_at(sim, (rx, ry))
}

/// Snap a drawn cell onto a nearby passable cell of the matching surface.
#[cfg(test)]
fn snap_to_passable(
    sim: &Simulation,
    path_grid: Option<&PathGrid>,
    drawn: (i32, i32),
    surface: CrateSurface,
) -> Option<(u16, u16)> {
    let radius_cap = crate_random_frame(sim)?.radius_cap;
    snap_to_passable_with_radius(sim, path_grid, drawn, surface, radius_cap)
}

fn snap_to_passable_with_radius(
    sim: &Simulation,
    path_grid: Option<&PathGrid>,
    drawn: (i32, i32),
    surface: CrateSurface,
    radius_cap: u16,
) -> Option<(u16, u16)> {
    let query = NearbyQuery {
        native_cells: None,
        raw_occupation: None,
        passability: PassabilityArgs {
            // Verified: the placer passes native speed type 5 (float) when the
            // drawn cell is water and 1 (track) otherwise.
            speed_type: match surface {
                CrateSurface::Water => SpeedType::Float,
                CrateSurface::Land => SpeedType::Track,
            },
            required_zone_id: None,
            movement_zone: MovementZone::Normal,
            bridge_aware_zone: false,
        },
        footprint: NearbyFootprint::SINGLE,
        // gamemd-derived: every FNPC candidate path, including crate placement,
        // calls Is_Cell_In_Playfield_CellClass(cell, 1) @ 0x00578540 before
        // CellRect passability (caller PlaceCrateAtRandomCell @ 0x0056BD40).
        anchor_gate: NearbyAnchorGate::NativeHeightAware,
        allow_bridge_cells: true,
        check_height: false,
        check_occupancy: false,
        radius_cap,
        // gamemd-derived: PlaceCrateAtRandomCell @ 0x0056BD40 initializes
        // this reference to the engine zero cell. FNPC @ 0x0056DC20 treats
        // that zero sentinel as "no target" and selects by live frame modulo.
        target_cell: None,
        path_grid,
        resolved_terrain: sim.resolved_terrain.as_ref(),
        overlay_grid: sim.overlay_grid.as_ref(),
        occupancy: Some(&sim.substrate.occupancy),
        entities: Some(&sim.substrate.entities),
        zone_grid: sim.zone_grid.as_ref(),
        playfield_bounds: sim.playfield_bounds,
    };
    find_nearby_passable_cell(drawn, &query, sim.session.binary_frame)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fmt::Write as _;

    use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
    use crate::map::overlay_types::OverlayTypeRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::cell_rect::PlayfieldBounds;
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::sim::pathfinding::PathGrid;

    const MAP: u16 = 40;

    pub(crate) fn crate_registry() -> OverlayTypeRegistry {
        let ini = IniFile::from_str(
            "[OverlayTypes]\n0=TIB01\n1=SILVER\n2=WOOD\n3=WATER\n\
             [TIB01]\nTiberium=yes\n[SILVER]\nCrate=yes\n\
             [WOOD]\nCrate=yes\n[WATER]\nCrate=yes\n",
        );
        OverlayTypeRegistry::from_ini(&ini, None)
    }

    pub(crate) fn crate_ruleset(extra: &str) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [CrateRules]\nCrateImg=SILVER\nWoodCrateImg=WOOD\nWaterCrateImg=WATER\n{extra}",
        ));
        RuleSet::from_ini(&ini).expect("rules")
    }

    pub(crate) fn crate_ruleset_with_images(
        wood: &str,
        common: &str,
        water: &str,
        extra: &str,
    ) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [CrateRules]\nCrateImg={common}\nWoodCrateImg={wood}\nWaterCrateImg={water}\n{extra}",
        ));
        RuleSet::from_ini(&ini).expect("rules")
    }

    fn crate_registry_with_raw_b2() -> OverlayTypeRegistry {
        let mut ini_text = String::from("[OverlayTypes]\n");
        for index in 0..=0xB2u16 {
            let name = if index == 0xB2 {
                "STEEPCRATE".to_owned()
            } else {
                format!("OV{index:03}")
            };
            writeln!(&mut ini_text, "{index}={name}").expect("string write");
        }
        ini_text.push_str("[STEEPCRATE]\nCrate=yes\n");
        OverlayTypeRegistry::from_ini(&IniFile::from_str(&ini_text), None)
    }

    fn dense_registry(last: u8, overrides: &[(u8, &str, &str)]) -> OverlayTypeRegistry {
        let mut names: Vec<String> = (0..=last).map(|id| format!("OV{id:03}")).collect();
        for &(id, name, _) in overrides {
            names[usize::from(id)] = name.to_owned();
        }
        let mut ini_text = String::from("[OverlayTypes]\n");
        for (id, name) in names.iter().enumerate() {
            writeln!(&mut ini_text, "{id}={name}").unwrap();
        }
        for (id, name) in names.iter().enumerate() {
            let override_section = overrides.iter().find_map(|(candidate, _, section)| {
                (usize::from(*candidate) == id).then_some(*section)
            });
            let default_low_bridge = crate::map::overlay_types::is_bridge_overlay_index(id as u8)
                && !crate::map::overlay_types::is_high_bridge_index(id as u8);
            let section = override_section.or(default_low_bridge.then_some("Land=Road\n"));
            if let Some(section) = section {
                writeln!(&mut ini_text, "[{name}]").unwrap();
                ini_text.push_str(section);
            }
        }
        OverlayTypeRegistry::from_ini(&IniFile::from_str(&ini_text), None)
    }

    /// `seed` positions the scenario cursor; the placer draws from there.
    pub(crate) fn sim_with_grid(seed: u64) -> Simulation {
        let mut sim = Simulation::new();
        sim.session.seed = seed;
        sim.scenario_rng = crate::sim::rng::SimRng::new(seed);
        sim.session.map_width = MAP;
        sim.session.map_height = MAP;
        // Production installs MapClass authority before Post_Map_Init reaches
        // crate placement. Keep this focused fixture intentionally broad while
        // exercising the same required mode-one gate.
        sim.playfield_bounds = Some(PlayfieldBounds {
            base: 20,
            off_fc: -128,
            off_100: -128,
            off_104: 256,
            off_108: 256,
        });
        sim.playfield_size_height = Some(20);
        sim.session.game_options.crates = true;
        sim.overlay_grid = Some(OverlayGrid::new(MAP, MAP));
        sim.resolved_terrain = Some(uniform_terrain(false));
        sim
    }

    fn cells_with_overlay(
        sim: &Simulation,
        registry: &OverlayTypeRegistry,
        name: &str,
    ) -> Vec<(u16, u16)> {
        let id = registry.id_for_name(name).expect("overlay type");
        let grid = sim.overlay_grid.as_ref().expect("overlay grid");
        let mut cells = Vec::new();
        for ry in 0..grid.height() {
            for rx in 0..grid.width() {
                if grid.cell(rx, ry).overlay_id == Some(id) {
                    cells.push((rx, ry));
                }
            }
        }
        cells
    }

    pub(crate) fn crate_cells(sim: &Simulation, registry: &OverlayTypeRegistry) -> Vec<(u16, u16)> {
        cells_with_overlay(sim, registry, "WOOD")
    }

    fn first_random_cell(sim: &Simulation) -> ((u16, u16), crate::sim::rng::SimRng) {
        let frame = crate_random_frame(sim).expect("valid crate random frame");
        let mut replay = sim.scenario_rng.clone();
        let cell =
            (
                frame.left.wrapping_add(
                    replay.next_range_u32_inclusive(0, (frame.width - 1) as u32) as i32,
                ) as u16,
                frame.top.wrapping_add(
                    replay.next_range_u32_inclusive(0, (frame.height - 1) as u32) as i32,
                ) as u16,
            );
        (cell, replay)
    }

    #[test]
    fn gsi_01_04_crate_count_applies_minimum_then_maximum() {
        let rules = CrateRules {
            minimum: 1,
            maximum: 255,
            ..CrateRules::default()
        };
        // Stock shape: one crate per player, never below CrateMinimum.
        assert_eq!(scenario_start_crate_count(&rules, 0), 1);
        assert_eq!(scenario_start_crate_count(&rules, 1), 1);
        assert_eq!(scenario_start_crate_count(&rules, 4), 4);
        assert_eq!(scenario_start_crate_count(&rules, 900), 255);

        let floored = CrateRules {
            minimum: 6,
            maximum: 255,
            ..CrateRules::default()
        };
        assert_eq!(scenario_start_crate_count(&floored, 2), 6);

        // The ceiling is applied after the floor, so a maximum under the
        // minimum wins outright.
        let inverted = CrateRules {
            minimum: 10,
            maximum: 3,
            ..CrateRules::default()
        };
        assert_eq!(scenario_start_crate_count(&inverted, 1), 3);

        let signed = CrateRules {
            minimum: -9,
            maximum: -3,
            ..CrateRules::default()
        };
        assert_eq!(scenario_start_crate_count(&signed, -7), -7);
        assert_eq!(scenario_start_crate_count(&signed, 4), -3);

        let uncapped_slot_count = CrateRules {
            minimum: 300,
            maximum: i32::MAX,
            ..CrateRules::default()
        };
        assert_eq!(scenario_start_crate_count(&uncapped_slot_count, 1), 300);
    }

    #[test]
    fn gsi_01_04_crate_rules_read_all_three_image_keys() {
        let rules = crate_ruleset("CrateMinimum=2\nCrateMaximum=9\n");
        assert_eq!(rules.crate_rules.minimum, 2);
        assert_eq!(rules.crate_rules.maximum, 9);
        assert_eq!(rules.crate_rules.crate_img.as_deref(), Some("SILVER"));
        assert_eq!(rules.crate_rules.wood_crate_img.as_deref(), Some("WOOD"));
        assert_eq!(rules.crate_rules.water_crate_img.as_deref(), Some("WATER"));

        let defaults = crate_ruleset("");
        assert_eq!(defaults.crate_rules.minimum, 1);
        assert_eq!(defaults.crate_rules.maximum, 255);
    }

    #[test]
    fn gsi_01_04_crate_placement_respects_clamped_count_and_distinct_cells() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=3\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0x1234_5678);

        // Five players, ceiling 3.
        let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 5);

        assert_eq!(result.requested, 3);
        assert_eq!((result.accepted, result.visible), (3, 3));
        let cells = crate_cells(&sim, &registry);
        assert_eq!(cells.len(), 3);
        let unique: BTreeSet<(u16, u16)> = cells.iter().copied().collect();
        assert_eq!(unique.len(), 3, "no two crates share a cell");
        for (rx, ry) in cells {
            assert!(rx < MAP && ry < MAP, "crate inside the map rectangle");
        }
    }

    #[test]
    fn gsi_01_04_crate_minimum_lifts_single_human_count() {
        let rules = crate_ruleset("CrateMinimum=4\nCrateMaximum=255\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(99);

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 1);

        assert_eq!(result.requested, 4);
        assert_eq!((result.accepted, result.visible), (4, 4));
        assert_eq!(crate_cells(&sim, &registry).len(), 4);
    }

    #[test]
    fn gsi_01_04_crate_disabled_spends_no_rng_and_places_nothing() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=255\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(7);
        sim.session.game_options.crates = false;
        let rng_before = sim.scenario_rng.state();

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 4);

        assert_eq!(
            result,
            CratePlacement {
                requested: 0,
                accepted: 0,
                visible: 0,
            }
        );
        assert!(crate_cells(&sim, &registry).is_empty());
        assert_eq!(sim.scenario_rng.state(), rng_before);
    }

    #[test]
    fn scenario_start_crate_missing_map_size_authority_invents_no_rectangle() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0xBAD5_1E00);
        sim.playfield_size_height = None;
        let before = sim.scenario_rng.state();

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 1);

        assert_eq!((result.accepted, result.visible), (0, 0));
        assert_eq!(sim.scenario_rng.state(), before);
        assert!(sim.crate_authority.slots()[0].is_empty());
    }

    /// A successful request spends X, Y, then the crate-slot timer draw.
    #[test]
    fn gsi_01_04_crate_success_spends_x_y_then_timer() {
        let rules = crate_ruleset("CrateMinimum=3\nCrateMaximum=255\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        let mut first = sim_with_grid(0xABCD);
        let mut replay = first.scenario_rng.clone();
        place_scenario_start_crates(&mut first, &rules, &registry, Some(&grid), 3);

        // Three successful first attempts: X, Y, timer for each crate.
        for _ in 0..3 {
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1));
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1));
            replay.next_range_u32_inclusive(0, 0x7fff_fffe);
        }
        assert_eq!(
            first.scenario_rng.state(),
            replay.state(),
            "successful crates must leave the cursor after X, Y, timer"
        );

        let mut same_cursor = sim_with_grid(0xABCD);
        place_scenario_start_crates(&mut same_cursor, &rules, &registry, Some(&grid), 3);
        let mut other_cursor = sim_with_grid(0x1111);
        place_scenario_start_crates(&mut other_cursor, &rules, &registry, Some(&grid), 3);

        assert_eq!(
            crate_cells(&first, &registry),
            crate_cells(&same_cursor, &registry),
            "the same scenario cursor must scatter crates identically"
        );
        assert_ne!(
            crate_cells(&first, &registry),
            crate_cells(&other_cursor, &registry),
            "a different scenario cursor must scatter crates differently"
        );
    }

    #[test]
    fn gsi_01_04_crate_failed_attempt_spends_only_x_y_before_success_timer() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0xA11C_E);
        let mut replay = sim.scenario_rng.clone();

        let first = (
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
        );
        let second = (
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
        );
        assert_ne!(first, second, "fixture needs a distinct second attempt");
        replay.next_range_u32_inclusive(0, 0x7fff_fffe);

        let blocker = registry.id_for_name("TIB01").expect("overlay type");
        sim.overlay_grid
            .as_mut()
            .expect("overlay grid")
            .place_overlay(first.0, first.1, blocker, 0);

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 1);

        assert_eq!((result.accepted, result.visible), (1, 1));
        assert_eq!(crate_cells(&sim, &registry), vec![second]);
        assert_eq!(sim.scenario_rng.state(), replay.state());
    }

    #[test]
    fn gsi_01_04_crate_second_request_sees_first_overlay_immediately() {
        let rules = crate_ruleset("CrateMinimum=2\nCrateMaximum=2\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        // Seed 1671 yields (37,4), timer, (37,4), then retry (5,8).
        let mut sim = sim_with_grid(1671);
        let mut replay = sim.scenario_rng.clone();
        let first = (
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
        );
        assert_eq!(first, (37, 4));
        replay.next_range_u32_inclusive(0, 0x7fff_fffe);
        assert_eq!(
            (
                replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
                replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            ),
            first
        );
        let retry = (
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
        );
        assert_eq!(retry, (5, 8));
        replay.next_range_u32_inclusive(0, 0x7fff_fffe);

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 2);

        assert_eq!((result.accepted, result.visible), (2, 2));
        assert_eq!(
            crate_cells(&sim, &registry)
                .into_iter()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([first, retry])
        );
        assert_eq!(sim.scenario_rng.state(), replay.state());
    }

    #[test]
    fn gsi_01_04_crate_one_thousand_failures_spend_exactly_two_thousand_draws() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0xF411_ED);
        // Empty diamond: every FNPC candidate fails the mandatory anchor gate
        // (and the independent final playfield gate would reject it too).
        sim.playfield_bounds = Some(PlayfieldBounds {
            base: 20,
            off_fc: 0,
            off_100: 0,
            off_104: 0,
            off_108: 0,
        });
        let mut replay = sim.scenario_rng.clone();
        for _ in 0..MAX_PLACEMENT_ATTEMPTS {
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1));
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1));
        }

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 1);

        assert_eq!((result.accepted, result.visible), (0, 0));
        assert!(crate_cells(&sim, &registry).is_empty());
        assert_eq!(sim.crate_authority.slots()[0], CrateSlot::default());
        assert_eq!(sim.scenario_rng.state(), replay.state());
    }

    #[test]
    fn gsi_01_04_crate_full_slot_array_stops_before_more_rng() {
        let rules = crate_ruleset("CrateMinimum=300\nCrateMaximum=300\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0x5107_0256);
        let mut replay = sim.scenario_rng.clone();
        let mut occupied = BTreeSet::new();
        while occupied.len() < state::CRATE_SLOT_CAPACITY {
            let cell = (
                replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
                replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            );
            if occupied.insert(cell) {
                replay.next_range_u32_inclusive(0, 0x7fff_fffe);
            }
        }

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 300);

        assert_eq!(result.requested, 300);
        assert_eq!(
            (result.accepted, result.visible),
            (
                state::CRATE_SLOT_CAPACITY as u32,
                state::CRATE_SLOT_CAPACITY as u32,
            )
        );
        assert_eq!(sim.scenario_rng.state(), replay.state());
    }

    #[test]
    fn scenario_start_crate_preexisting_full_slot_table_spends_no_rng() {
        let rules = crate_ruleset("CrateMinimum=4\nCrateMaximum=4\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0x5107_F011);
        for index in 0..state::CRATE_SLOT_CAPACITY {
            sim.crate_authority.slot_mut(index).cell_x = (index as i16).wrapping_add(1);
        }
        let before = sim.scenario_rng.state();

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 1);

        assert_eq!(result.requested, 4);
        assert_eq!((result.accepted, result.visible), (0, 0));
        assert_eq!(sim.scenario_rng.state(), before);
    }

    #[test]
    fn scenario_start_crate_object_ground_occupation_accepts_a_timed_ghost() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0x47C5_50);
        let mut replay = sim.scenario_rng.clone();
        let rx = replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16;
        let ry = replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16;
        replay.next_range_u32_inclusive(0, 0x7fff_fffe);
        sim.substrate
            .raw_cell_occupation
            .mark_ground(rx, ry, OBJECT_OCCUPATION_BIT);

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 1);

        assert_eq!(result.requested, 1);
        assert_eq!((result.accepted, result.visible), (1, 0));
        assert!(crate_cells(&sim, &registry).is_empty());
        assert_eq!(
            sim.overlay_grid
                .as_ref()
                .expect("overlay grid")
                .cell(rx, ry)
                .overlay_id,
            None,
            "a post-precheck Mark failure leaves the Cell unchanged",
        );
        assert_eq!(sim.scenario_rng.state(), replay.state());
        let slot = sim.crate_authority.slots()[0];
        assert_eq!((slot.cell_x, slot.cell_y), (rx as i16, ry as i16));
        assert_eq!(slot.start_frame, sim.session.binary_frame as i32);
        assert_eq!(
            slot.aux, 0x40d1_9400,
            "constructor regen 10 owns upper=18000"
        );
        assert!(slot.duration > 0);
    }

    #[test]
    fn scenario_start_crate_only_object_ground_occupation_bit_ghosts() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        for bit in [1, 2, 4, 8, 16, 32, 64, 128] {
            let mut sim = sim_with_grid(0x0CC0_0000 + u64::from(bit));
            let (cell, _) = first_random_cell(&sim);
            sim.substrate
                .raw_cell_occupation
                .mark_ground(cell.0, cell.1, bit);
            let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 1);
            assert_eq!(
                (result.accepted, result.visible),
                (1, u32::from(bit != OBJECT_OCCUPATION_BIT)),
                "only raw occupation bit 0x40 may reject Mark; tested {bit:#04x}"
            );
            assert!(sim.crate_authority.slots()[0].cell_x != 0);
            assert_eq!(
                crate_cells(&sim, &registry).contains(&cell),
                bit != OBJECT_OCCUPATION_BIT,
                "non-object occupation bits must retain the visible crate"
            );
        }
    }

    #[test]
    fn scenario_start_crate_null_image_and_terrain_object_are_accepted_ghosts() {
        let null_rules = crate_ruleset_with_images(
            "none",
            "SILVER",
            "WATER",
            "CrateMinimum=1\nCrateMaximum=1\nCrateRegen=3\n",
        );
        let ordinary_rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\nCrateRegen=3\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        let mut null_sim = sim_with_grid(0xA011_0001);
        null_sim.session.binary_frame = u32::MAX;
        let (null_cell, mut null_replay) = first_random_cell(&null_sim);
        let timer_draw = null_replay.next_range_u32_inclusive(0, 0x7fff_fffe);
        let expected_timer = state::crate_timer_words(
            null_rules.crate_rules.regen,
            timer_draw,
            null_sim.session.binary_frame as i32,
        );
        let null_result =
            place_scenario_start_crates(&mut null_sim, &null_rules, &registry, Some(&grid), 1);
        assert_eq!((null_result.accepted, null_result.visible), (1, 0));
        assert!(
            null_sim
                .overlay_grid
                .as_ref()
                .unwrap()
                .iter_occupied()
                .next()
                .is_none()
        );
        let null_slot = null_sim.crate_authority.slots()[0];
        assert_eq!(
            (null_slot.cell_x, null_slot.cell_y),
            (null_cell.0 as i16, null_cell.1 as i16)
        );
        assert_eq!(
            (null_slot.start_frame, null_slot.aux, null_slot.duration),
            expected_timer
        );
        assert_eq!(null_sim.scenario_rng.state(), null_replay.state());

        let mut terrain_sim = sim_with_grid(0xA011_0002);
        let (cell, _) = first_random_cell(&terrain_sim);
        terrain_sim.production.terrain_object_cells.insert(cell, 77);
        let terrain_result = place_scenario_start_crates(
            &mut terrain_sim,
            &ordinary_rules,
            &registry,
            Some(&grid),
            1,
        );
        assert_eq!((terrain_result.accepted, terrain_result.visible), (1, 0));
        assert_eq!(
            (
                terrain_sim.crate_authority.slots()[0].cell_x,
                terrain_sim.crate_authority.slots()[0].cell_y
            ),
            (cell.0 as i16, cell.1 as i16)
        );
    }

    #[test]
    fn crate_placement_forced_post_precheck_failures_are_timed_ghosts() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\nCrateRegen=3\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        for failure in [
            ForcedPostPrecheckFailure::Allocation,
            ForcedPostPrecheckFailure::Unlimbo,
            ForcedPostPrecheckFailure::Mark,
        ] {
            let mut sim = sim_with_grid(0xFA11_0000 + failure as u64);
            let result = place_scenario_start_crates_with_failure(
                &mut sim,
                &rules,
                &registry,
                Some(&grid),
                1,
                ParsedLightingProfiles::default().normal,
                failure,
            );
            assert_eq!((result.accepted, result.visible), (1, 0));
            let slot = sim.crate_authority.slots()[0];
            assert!(!slot.is_empty());
            assert_eq!(slot.aux, 0x40b5_1800);
            assert!(
                sim.overlay_grid
                    .as_ref()
                    .unwrap()
                    .iter_occupied()
                    .next()
                    .is_none()
            );
        }
    }

    #[test]
    fn gsi_01_04_crate_water_track_zero_commits_and_spends_timer_draw() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0xF10A_7000);
        sim.resolved_terrain = Some(uniform_terrain(true));
        for cell in &mut sim.resolved_terrain.as_mut().expect("terrain").cells {
            cell.speed_costs.track = Some(0);
            cell.speed_costs.float = Some(100);
        }
        let mut replay = sim.scenario_rng.clone();
        let destination = (
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
        );
        replay.next_range_u32_inclusive(0, 0x7fff_fffe);

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 1);

        assert_eq!((result.accepted, result.visible), (1, 1));
        assert_eq!(
            cells_with_overlay(&sim, &registry, "WATER"),
            vec![destination]
        );
        assert_eq!(sim.scenario_rng.state(), replay.state());
    }

    #[test]
    fn crate_placement_mark_uses_water_first_numeric_identity_alias() {
        let rules =
            crate_ruleset_with_images("WOOD", "SILVER", "WOOD", "CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let mut sim = sim_with_grid(0xA11A_5001);
        let cell = (7, 9);
        let terrain_cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        terrain_cell.speed_costs.track = Some(0);
        terrain_cell.speed_costs.float = Some(100);

        let result = validate_and_stamp_candidate(
            &mut sim,
            &rules.crate_rules,
            &registry,
            cell,
            ForcedPostPrecheckFailure::None,
        );

        assert_eq!(result, AcceptedCellResult::Visible);
        assert_eq!(
            sim.overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .overlay_id,
            registry.id_for_name("WOOD")
        );

        let mut zero_float = sim_with_grid(0xA11A_5002);
        let zero_cell = zero_float
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        zero_cell.speed_costs.track = Some(100);
        zero_cell.speed_costs.float = Some(0);
        assert_eq!(
            validate_and_stamp_candidate(
                &mut zero_float,
                &rules.crate_rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost,
            "the aliased selected identity must use Water/Float, not Wood/Track"
        );
    }

    #[test]
    fn crate_placement_bridge_selects_full_deck_byte_and_bypasses_speed_zero() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let cell = (8, 11);

        let mut visible = sim_with_grid(0xB21D_0001);
        let terrain_cell = visible
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        terrain_cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        terrain_cell.speed_costs.track = Some(0);
        visible
            .substrate
            .raw_cell_occupation
            .mark_ground(cell.0, cell.1, 0x80);
        assert_eq!(
            validate_and_stamp_candidate(
                &mut visible,
                &rules.crate_rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible,
            "structural bridge chooses empty deck and bypasses ground speed zero"
        );

        let mut non_object_deck = sim_with_grid(0xB21D_0002);
        let terrain_cell = non_object_deck
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        terrain_cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        terrain_cell.speed_costs.track = Some(100);
        non_object_deck
            .substrate
            .raw_cell_occupation
            .mark_deck(cell.0, cell.1, 0x01);
        assert_eq!(
            validate_and_stamp_candidate(
                &mut non_object_deck,
                &rules.crate_rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible,
            "a selected deck byte without object bit 0x40 passes Mark"
        );

        let mut object_deck = sim_with_grid(0xB21D_0003);
        let terrain_cell = object_deck
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap();
        terrain_cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        terrain_cell.speed_costs.track = Some(100);
        object_deck
            .substrate
            .raw_cell_occupation
            .mark_deck(cell.0, cell.1, OBJECT_OCCUPATION_BIT);
        assert_eq!(
            validate_and_stamp_candidate(
                &mut object_deck,
                &rules.crate_rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost,
            "selected deck object bit 0x40 rejects Mark"
        );
    }

    #[test]
    fn crate_placement_steep_slope_uses_exact_raw_b2_exception() {
        let ordinary_rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let ordinary_registry = crate_registry();
        let cell = (12, 12);
        let mut ordinary = sim_with_grid(0xB200_0001);
        ordinary
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap()
            .slope_type = 5;
        assert_eq!(
            validate_and_stamp_candidate(
                &mut ordinary,
                &ordinary_rules.crate_rules,
                &ordinary_registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );

        let b2_registry = crate_registry_with_raw_b2();
        assert_eq!(b2_registry.id_for_name("STEEPCRATE"), Some(0xB2));
        let b2_rules = CrateRules {
            wood_crate_img: Some("STEEPCRATE".to_owned()),
            crate_img: Some("STEEPCRATE".to_owned()),
            water_crate_img: Some("STEEPCRATE".to_owned()),
            ..CrateRules::default()
        };
        let mut exempt = sim_with_grid(0xB200_0002);
        exempt
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap()
            .slope_type = u8::MAX;
        assert_eq!(
            validate_and_stamp_candidate(
                &mut exempt,
                &b2_rules,
                &b2_registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
    }

    #[test]
    fn place_crate_overlay_preserves_wall_owner_and_publishes_one_dirty_cell() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let mut sim = sim_with_grid(0xD117_0001);
        let cell = (6, 13);
        let owner = sim.interner.intern("Owner");
        let grid = sim.overlay_grid.as_mut().unwrap();
        grid.cell_mut(cell.0, cell.1).wall_owner = Some(owner);
        assert!(grid.take_dirty_cells().is_empty());

        assert_eq!(
            validate_and_stamp_candidate(
                &mut sim,
                &rules.crate_rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        let grid = sim.overlay_grid.as_mut().unwrap();
        let written = grid.cell(cell.0, cell.1);
        assert_eq!(written.overlay_id, registry.id_for_name("WOOD"));
        assert_eq!(written.overlay_data, u8::MAX);
        assert_eq!(written.wall_owner, Some(owner));
        assert_eq!(grid.take_dirty_cells(), vec![cell]);
    }

    #[test]
    fn configured_noncrate_images_keep_native_mark_zero_and_road_data() {
        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str(
                "[OverlayTypes]\n0=PLAIN\n1=ROADBOX\n[PLAIN]\n[ROADBOX]\nLand=Road\n",
            ),
            None,
        );
        for (name, expected_data) in [("PLAIN", 0), ("ROADBOX", 1)] {
            let rules = CrateRules {
                wood_crate_img: Some(name.to_owned()),
                crate_img: Some(name.to_owned()),
                water_crate_img: Some(name.to_owned()),
                ..CrateRules::default()
            };
            let mut sim = sim_with_grid(0xDA7A_0000 + u64::from(expected_data));
            let cell = (6, 13);

            assert_eq!(
                validate_and_stamp_candidate(
                    &mut sim,
                    &rules,
                    &registry,
                    cell,
                    ForcedPostPrecheckFailure::None,
                ),
                AcceptedCellResult::Visible
            );
            let written = sim.overlay_grid.as_ref().unwrap().cell(cell.0, cell.1);
            assert_eq!(written.overlay_id, registry.id_for_name(name));
            assert_eq!(written.overlay_data, expected_data, "configured {name}");
        }
    }

    #[test]
    fn crate_mark_publishes_navigation_before_next_reader_and_frame_delivery() {
        use crate::sim::overlay_grid::NavigationPublication;
        use crate::sim::world::TickLane;
        use std::sync::Arc;

        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str(
                "[OverlayTypes]\n0=ROADBOX\n[ROADBOX]\nLand=Road\n[Road]\nFoot=37%\nTrack=100%\n",
            ),
            None,
        );
        let rules = crate_ruleset_with_images("ROADBOX", "ROADBOX", "ROADBOX", "CrateMinimum=0\n");
        let mut sim = sim_with_grid(0xDA7A_0020);
        // Mark is invoked below; autonomous crate spawning is outside this fixture.
        sim.session.game_options.crates = false;
        sim.production.ore_growth_config.grows = false;
        sim.production.ore_growth_config.spreads = false;
        assert!(sim.rebuild_dynamic_navigation(&rules));
        let cells = [(12, 13), (6, 13)];
        for cell in cells {
            assert_eq!(
                sim.terrain_costs[&SpeedType::Foot].cost_at(cell.0, cell.1),
                100
            );
            assert_eq!(
                validate_and_stamp_candidate(
                    &mut sim,
                    &rules.crate_rules,
                    &registry,
                    cell,
                    ForcedPostPrecheckFailure::None,
                ),
                AcceptedCellResult::Visible
            );
            let projected = sim
                .resolved_terrain
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .unwrap();
            assert_eq!(projected.land_type, LandType::Road.as_index());
            assert_eq!(projected.speed_costs.foot, Some(37));
            assert_eq!(
                sim.terrain_costs[&SpeedType::Foot].cost_at(cell.0, cell.1),
                100,
                "canonical navigation waits for the next-reader boundary"
            );
        }
        let repeated = sim.overlay_grid.as_mut().unwrap().recalculate_runtime_cell(
            sim.resolved_terrain.as_mut().unwrap(),
            &registry,
            cells[0],
            NavigationPublication::NextPathReader,
        );
        assert!(!repeated.navigation_changed);
        let mut pending = sim.overlay_grid.as_ref().unwrap().clone();
        assert_eq!(pending.take_synchronous_navigation_cells(), cells);
        assert_eq!(
            pending.take_dirty_cells_with_passability_signal(),
            (cells.to_vec(), true)
        );

        // Even an empty receiver batch executes the production consequence settlement.
        // It must publish pending crate terrain before its next navigation reader.
        sim.commit_noncombat_aoe_receivers(&rules, Some(&registry), &[]);
        for cell in cells {
            assert_eq!(
                sim.terrain_costs[&SpeedType::Foot].cost_at(cell.0, cell.1),
                37
            );
        }
        let mut pending = sim.overlay_grid.as_ref().unwrap().clone();
        assert!(pending.take_synchronous_navigation_cells().is_empty());
        assert_eq!(
            pending.take_dirty_cells_with_passability_signal(),
            (cells.to_vec(), true),
            "next-reader settlement does not consume presentation dirtiness or the frame signal"
        );
        let settled = sim.path_grid_snapshot().unwrap();
        let first = sim
            .advance_app_frame(
                &[],
                Some(&rules),
                &std::collections::BTreeMap::new(),
                Some(&registry),
                67,
                TickLane::Ordinary,
                None,
            )
            .expect("fixture frame must complete");
        assert_eq!(
            first
                .overlay_updates
                .iter()
                .map(|entry| (entry.rx, entry.ry))
                .collect::<Vec<_>>(),
            cells
        );
        assert_eq!(first.tick.state_hash, sim.state_hash());
        let published = sim.path_grid_snapshot().unwrap();
        assert!(
            !Arc::ptr_eq(&settled, &published),
            "the independent frame signal also publishes"
        );
        let second = sim
            .advance_app_frame(
                &[],
                Some(&rules),
                &std::collections::BTreeMap::new(),
                Some(&registry),
                67,
                TickLane::Ordinary,
                None,
            )
            .expect("fixture frame must complete");
        assert!(second.overlay_updates.is_empty());
        assert!(Arc::ptr_eq(&published, &sim.path_grid_snapshot().unwrap()));
        assert_eq!(second.tick.state_hash, sim.state_hash());
    }

    #[test]
    fn railroad_crate_mark_bypasses_ordinary_blockers_and_forces_data_zero() {
        let registry = dense_registry(0, &[(0, "RAILCRATE", "Land=Railroad\nCrate=yes\n")]);
        let rules = CrateRules {
            wood_crate_img: Some("RAILCRATE".to_owned()),
            crate_img: Some("RAILCRATE".to_owned()),
            water_crate_img: Some("RAILCRATE".to_owned()),
            ..CrateRules::default()
        };
        let mut sim = sim_with_grid(0x14_09_0001);
        let cell = (12, 13);
        sim.substrate
            .raw_cell_occupation
            .mark_ground(cell.0, cell.1, 0xFF);
        sim.substrate
            .raw_cell_occupation
            .mark_deck(cell.0, cell.1, 0xFF);
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap()
            .speed_costs
            .track = Some(0);

        assert_eq!(
            validate_and_stamp_candidate(
                &mut sim,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        let written = sim.overlay_grid.as_ref().unwrap().cell(cell.0, cell.1);
        assert_eq!(written.overlay_id, Some(0));
        assert_eq!(written.overlay_data, 0, "Railroad wins over Crate=yes");
    }

    #[test]
    fn wall_crate_mark_uses_building_passability_and_crate_never_overrides_data() {
        let registry = dense_registry(0, &[(0, "WALLCRATE", "Wall=yes\nLand=Wall\nCrate=yes\n")]);
        let rules = CrateRules {
            wood_crate_img: Some("WALLCRATE".to_owned()),
            crate_img: Some("WALLCRATE".to_owned()),
            water_crate_img: Some("WALLCRATE".to_owned()),
            ..CrateRules::default()
        };
        let cell = (12, 13);
        let mut visible = sim_with_grid(0x14_09_0002);
        visible
            .substrate
            .raw_cell_occupation
            .mark_ground(cell.0, cell.1, 0xFF);
        visible
            .overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(cell.0 + 1, cell.1, 0, 0);

        assert_eq!(
            validate_and_stamp_candidate(
                &mut visible,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        let grid = visible.overlay_grid.as_ref().unwrap();
        assert_eq!(grid.cell(cell.0, cell.1).overlay_data, 0x02);
        assert_eq!(grid.cell(cell.0 + 1, cell.1).overlay_data, 0x08);

        let mut blocked = sim_with_grid(0x14_09_0003);
        blocked
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap()
            .speed_costs
            .track = Some(0);
        assert_eq!(
            validate_and_stamp_candidate(
                &mut blocked,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );
        assert_eq!(
            blocked
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .overlay_id,
            None
        );
    }

    #[test]
    fn road_tiberium_crate_mark_germinates_from_same_type_neighbors() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [OverlayTypes]\n0=ROADORE\n\
             [ROADORE]\nTiberium=yes\nLand=Road\n\
             [Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\n\
             [CrateRules]\nCrateImg=ROADORE\nWoodCrateImg=ROADORE\nWaterCrateImg=ROADORE\n",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let rules = RuleSet::from_ini(&ini).unwrap();
        let mut sim = sim_with_grid(0x14_09_0004);
        let cell = (12, 13);
        for neighbor in [(11, 12), (12, 12), (13, 13), (12, 14)] {
            sim.overlay_grid
                .as_mut()
                .unwrap()
                .place_overlay(neighbor.0, neighbor.1, 0, 1);
        }

        assert_eq!(
            validate_and_stamp_candidate_with_rules(
                &mut sim,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        assert_eq!(
            sim.overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .overlay_data,
            6,
            "four matching neighbors index the retail [0,1,3,4,6,...] table"
        );
    }

    #[test]
    fn ordinary_cell_anim_spawns_after_visible_or_failed_ordinary_mark() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [Colors]\nGold=43,239,255\nNeonGreen=104,241,195\n\
             [Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\nColor=NeonGreen\n\
             [Animations]\n0=SPARK\n\
             [OverlayTypes]\n0=ANIMBOX\n[ANIMBOX]\nTiberium=yes\nCellAnim=SPARK\n\
             [CrateRules]\nCrateImg=ANIMBOX\nWoodCrateImg=ANIMBOX\nWaterCrateImg=ANIMBOX\n",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let mut rules = RuleSet::from_ini(&ini).unwrap();
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
            "[SPARK]\nRate=1\nEnd=2\nLoopCount=1\n",
        ));
        art.bind_anim_frame_count_for_test("SPARK", 2);
        rules.art_registry = art;
        let cell = (12, 13);

        let mut visible = sim_with_grid(0x14_09_0005);
        let lighting = LightingProfileUnits {
            ambient_percent: 80,
            red_percent: 100,
            green_percent: 100,
            blue_percent: 100,
            ground_units: 25,
            level_units: 5,
        };
        assert_eq!(
            validate_and_stamp_candidate_with_rules_and_lighting(
                &mut visible,
                &rules,
                &registry,
                cell,
                lighting,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        let visible_anim = visible.substrate.anims.iter().next().unwrap().1;
        assert_eq!(
            visible_anim.world_coord,
            crate::sim::anim_class::AnimWorldCoord {
                x: i32::from(cell.0) * 256 + 128 + 0x180,
                y: i32::from(cell.1) * 256 + 128 + 0x180,
                z: 0,
            }
        );
        assert_eq!(visible_anim.draw_flags, 0x600);
        assert_eq!(
            visible_anim.remap_color,
            Some(crate::rules::house_colors::HouseColorIndex(1)),
            "the constructed CellAnim uses the live tiberium Color scheme"
        );
        assert_eq!(
            visible_anim.z_adjust, 775,
            "CellClass +0x10A is ambient*10 + level*height - ground"
        );
        let encoded = bincode::serialize(&visible).expect("serialize CellAnim remap state");
        let restored: Simulation =
            bincode::deserialize(&encoded).expect("restore CellAnim remap state");
        let restored_anim = restored.substrate.anims.iter().next().unwrap().1;
        assert_eq!(restored_anim.remap_color, visible_anim.remap_color);
        assert_eq!(restored_anim.z_adjust, visible_anim.z_adjust);

        let mut ghost = sim_with_grid(0x14_09_0006);
        ghost
            .substrate
            .raw_cell_occupation
            .mark_ground(cell.0, cell.1, OBJECT_OCCUPATION_BIT);
        assert_eq!(
            validate_and_stamp_candidate_with_rules(
                &mut ghost,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );
        let ghost_anim = ghost.substrate.anims.iter().next().unwrap().1;
        assert_eq!(ghost_anim.remap_color, None);
        assert_eq!(
            ghost_anim.z_adjust, 0,
            "failed Mark leaves the Cell without tiberium, so native skips both post-writes"
        );
        assert_eq!(
            ghost
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .overlay_id,
            None
        );

        let mut terrain_object = sim_with_grid(0x14_09_0007);
        terrain_object
            .production
            .terrain_object_cells
            .insert(cell, 99);
        assert_eq!(
            validate_and_stamp_candidate_with_rules(
                &mut terrain_object,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );
        assert_eq!(terrain_object.substrate.anims.iter().count(), 0);
        assert_eq!(
            terrain_object
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .overlay_id,
            None
        );
    }

    #[test]
    fn high_bridge_crate_mark_writes_setter_data_then_falls_through_crate_override() {
        let registry = dense_registry(0x18, &[(0x18, "HIGHCRATE", "Crate=yes\n")]);
        let rules = CrateRules {
            wood_crate_img: Some("HIGHCRATE".to_owned()),
            crate_img: Some("HIGHCRATE".to_owned()),
            water_crate_img: Some("HIGHCRATE".to_owned()),
            ..CrateRules::default()
        };
        let cell = (12, 13);
        let mut sim = sim_with_grid(0x14_09_0008);
        for target in [(12, 13), (12, 12), (12, 11), (12, 10), (12, 14)] {
            write_crate_mark_raw_data(&mut sim, (target.0 as i16, target.1 as i16), 7);
        }
        let _ = sim.overlay_grid.as_mut().unwrap().take_dirty_cells();
        sim.substrate
            .raw_cell_occupation
            .mark_ground(cell.0, cell.1, 0xFF);
        sim.substrate
            .raw_cell_occupation
            .mark_deck(cell.0, cell.1, 0xFF);
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(cell.0, cell.1)
            .unwrap()
            .speed_costs
            .track = Some(0);

        assert_eq!(
            validate_and_stamp_candidate(
                &mut sim,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        let grid = sim.overlay_grid.as_ref().unwrap();
        assert_eq!(
            (grid.cell(12, 13).overlay_id, grid.cell(12, 13).overlay_data),
            (Some(0x18), u8::MAX),
            "Crate=yes runs after the direction-0 setter writes zero"
        );
        for target in [(12, 12), (12, 11), (12, 14)] {
            assert_eq!(grid.cell(target.0, target.1).overlay_data, 0, "{target:?}");
        }
        assert_eq!(grid.cell(12, 10).overlay_data, 7, "F3 gets flags, not data");

        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let anchor = terrain.cell(12, 13).unwrap().bridge_facts;
        assert!(anchor.is_anchor_self());
        assert!(anchor.has_structural_bridge());
        assert_eq!(anchor.state_byte, u8::MAX);
        for target in [(12, 12), (12, 11), (12, 14)] {
            assert_eq!(
                terrain
                    .cell(target.0, target.1)
                    .unwrap()
                    .bridge_facts
                    .state_byte,
                0,
                "derived state follows raw setter data at {target:?}"
            );
        }
        assert_eq!(terrain.cell(12, 10).unwrap().bridge_facts.state_byte, 7);

        let mut terrain_blocked = sim_with_grid(0x14_09_0009);
        write_crate_mark_raw_data(&mut terrain_blocked, (12, 13), 7);
        let _ = terrain_blocked
            .overlay_grid
            .as_mut()
            .unwrap()
            .take_dirty_cells();
        terrain_blocked
            .production
            .terrain_object_cells
            .insert(cell, 100);
        assert_eq!(
            validate_and_stamp_candidate(
                &mut terrain_blocked,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );
        assert_eq!(
            terrain_blocked
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .overlay_data,
            7,
            "TerrainClass constructor gate runs before the high setter"
        );
        assert_eq!(
            terrain_blocked
                .resolved_terrain
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .unwrap()
                .bridge_facts
                .raw_flags
                & BRIDGE_FLAG_STRUCTURAL,
            0
        );
        assert!(
            terrain_blocked
                .overlay_grid
                .as_ref()
                .unwrap()
                .pending_dirty_cells()
                .is_empty()
        );
    }

    #[test]
    fn direction_six_high_bridge_setter_data_precedes_road_override() {
        let registry = dense_registry(0x19, &[(0x19, "HIGHROAD", "Land=Road\n")]);
        let rules = CrateRules {
            wood_crate_img: Some("HIGHROAD".to_owned()),
            crate_img: Some("HIGHROAD".to_owned()),
            water_crate_img: Some("HIGHROAD".to_owned()),
            ..CrateRules::default()
        };
        let cell = (12, 13);
        let mut sim = sim_with_grid(0x14_09_000A);

        assert_eq!(
            validate_and_stamp_candidate(
                &mut sim,
                &rules,
                &registry,
                cell,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        let grid = sim.overlay_grid.as_ref().unwrap();
        assert_eq!(
            (grid.cell(12, 13).overlay_id, grid.cell(12, 13).overlay_data),
            (Some(0x19), 1),
            "Road runs after the direction-6 setter writes nine"
        );
        for target in [(11, 13), (10, 13), (13, 13)] {
            assert_eq!(grid.cell(target.0, target.1).overlay_data, 9, "{target:?}");
            assert_eq!(
                sim.resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(target.0, target.1)
                    .unwrap()
                    .bridge_facts
                    .state_byte,
                9,
                "derived state follows raw setter data at {target:?}"
            );
        }
    }

    #[test]
    fn low_bridge_crate_mark_uses_exact_table_raw_draws_and_success_noop() {
        let registry = dense_registry(0x7A, &[(0x7A, "LOWTRIGGER", "Land=Road\n")]);
        let rules = CrateRules {
            wood_crate_img: Some("LOWTRIGGER".to_owned()),
            crate_img: Some("LOWTRIGGER".to_owned()),
            water_crate_img: Some("LOWTRIGGER".to_owned()),
            ..CrateRules::default()
        };
        let origin = (20, 20);
        let mut sim = sim_with_grid(0x14_09_0008);
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(15, 20, 0x5E, 1);
        let mut replay = sim.scenario_rng.clone();
        let body_draws: Vec<u8> = (0..12).map(|_| (replay.next_u32() & 3) as u8).collect();

        assert_eq!(
            validate_and_stamp_candidate(
                &mut sim,
                &rules,
                &registry,
                origin,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        assert_eq!(sim.scenario_rng.state(), replay.state());
        let grid = sim.overlay_grid.as_ref().unwrap();
        assert_eq!(
            (grid.cell(20, 20).overlay_id, grid.cell(20, 20).overlay_data),
            (Some(0x5C), 1)
        );
        assert_eq!(grid.cell(16, 19).overlay_id, Some(0x4A + body_draws[0]));
        assert_eq!(grid.cell(16, 19).overlay_data, 0);
        assert_eq!(grid.cell(19, 21).overlay_id, Some(0x4A + body_draws[11]));
        assert_eq!(grid.cell(19, 21).overlay_data, 2);
        let origin_terrain = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(origin.0, origin.1)
            .unwrap();
        assert!(origin_terrain.has_bridge_deck);
        assert!(!origin_terrain.bridge_walkable);
        assert!(origin_terrain.build_blocked);
        assert_eq!(
            origin_terrain
                .bridge_layer
                .as_ref()
                .map(|layer| layer.direction),
            Some(crate::map::resolved_terrain::BridgeDirection::Low)
        );

        let mut occupied = sim_with_grid(0x14_09_0009);
        occupied
            .overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(20, 19, 0, 0);
        let before = occupied.scenario_rng.state();
        assert_eq!(
            validate_and_stamp_candidate(
                &mut occupied,
                &rules,
                &registry,
                origin,
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );
        assert_eq!(occupied.scenario_rng.state(), before);
        assert_eq!(
            occupied
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(origin.0, origin.1)
                .overlay_id,
            None
        );
    }

    #[test]
    fn low_bridge_crate_mark_preserves_shared_dummy_overlay_alias() {
        let registry = dense_registry(0x7C, &[(0x7C, "EDGELOW", "Land=Road\n")]);
        let rules = CrateRules {
            wood_crate_img: Some("EDGELOW".to_owned()),
            crate_img: Some("EDGELOW".to_owned()),
            water_crate_img: Some("EDGELOW".to_owned()),
            ..CrateRules::default()
        };
        let mut sim = sim_with_grid(0x14_09_0012);
        let before = sim.scenario_rng.state();

        assert_eq!(
            validate_and_stamp_candidate(
                &mut sim,
                &rules,
                &registry,
                (0, 1),
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Visible
        );
        let dummy = sim.effective_shared_cell_dummy();
        assert_eq!(dummy.overlay_fields(), (Some(0x60), 0));
        assert_eq!(dummy.snapshot().coord, (-1, 1));

        // A different missing coordinate aliases that same native fallback
        // cell. Its retained overlay makes the later three-probe row occupied,
        // so the real trigger cell remains an accepted timed ghost.
        assert_eq!(
            validate_and_stamp_candidate(
                &mut sim,
                &rules,
                &registry,
                (0, 2),
                ForcedPostPrecheckFailure::None,
            ),
            AcceptedCellResult::Ghost
        );
        assert_eq!(dummy.overlay_fields(), (Some(0x60), 0));
        assert_eq!(dummy.snapshot().coord, (-1, 2));
        assert_eq!(sim.scenario_rng.state(), before);
    }

    #[test]
    fn ts_legacy_veins_crate_ids_are_explicit_accepted_ghosts() {
        for (id, name) in [(0x7E, "VEINS"), (0xA7, "VEINHOLE")] {
            let registry = dense_registry(id, &[(id, name, "Crate=yes\n")]);
            let rules = CrateRules {
                wood_crate_img: Some(name.to_owned()),
                crate_img: Some(name.to_owned()),
                water_crate_img: Some(name.to_owned()),
                ..CrateRules::default()
            };
            let mut sim = sim_with_grid(0x14_09_0010 + u64::from(id));
            let before = sim.scenario_rng.state();
            assert_eq!(
                validate_and_stamp_candidate(
                    &mut sim,
                    &rules,
                    &registry,
                    (12, 13),
                    ForcedPostPrecheckFailure::None,
                ),
                AcceptedCellResult::Ghost
            );
            assert_eq!(sim.scenario_rng.state(), before);
            assert_eq!(
                sim.overlay_grid.as_ref().unwrap().cell(12, 13).overlay_id,
                None
            );
        }
    }

    #[test]
    fn crate_random_rectangle_draws_signed_reversed_ranges_then_narrows_to_i16() {
        let mut sum_one = crate::sim::rng::SimRng::new(0x1401);
        let mut sum_one_expected = sum_one.clone();
        let sum_one_frame = CrateRandomFrame {
            left: 1,
            top: 1,
            width: 0,
            height: 0,
            radius_cap: 1,
        };
        let sum_one_cell = draw_crate_candidate(&mut sum_one, sum_one_frame);
        let expected_sum_one = (
            sum_one_expected.next_range_u32_inclusive(0, 1) as i32,
            sum_one_expected.next_range_u32_inclusive(0, 1) as i32,
        );
        assert_eq!(sum_one_cell, expected_sum_one);
        assert_eq!(sum_one.state(), sum_one_expected.state());

        let mut negative = crate::sim::rng::SimRng::new(0x1402);
        let mut negative_expected = negative.clone();
        let negative_frame = CrateRandomFrame {
            left: i32::MAX,
            top: i32::MIN,
            width: -2,
            height: -2,
            radius_cap: 0,
        };
        let negative_cell = draw_crate_candidate(&mut negative, negative_frame);
        let expected_negative = (
            i32::MAX.wrapping_add(-3 + negative_expected.next_range_u32_inclusive(0, 3) as i32)
                as i16 as i32,
            i32::MIN.wrapping_add(-3 + negative_expected.next_range_u32_inclusive(0, 3) as i32)
                as i16 as i32,
        );
        assert_eq!(negative_cell, expected_negative);
        assert_eq!(negative.state(), negative_expected.state());

        let mut wide = crate::sim::rng::SimRng::new(0x1403);
        let mut wide_expected = wide.clone();
        let wide_frame = CrateRandomFrame {
            left: 30_000,
            top: -30_000,
            width: 70_000,
            height: 70_000,
            radius_cap: CRATE_SNAP_RADIUS_CAP,
        };
        let wide_cell = draw_crate_candidate(&mut wide, wide_frame);
        let expected_wide = (
            30_000i32.wrapping_add(wide_expected.next_range_u32_inclusive(0, 69_999) as i32) as i16
                as i32,
            (-30_000i32).wrapping_add(wide_expected.next_range_u32_inclusive(0, 69_999) as i32)
                as i16 as i32,
        );
        assert_eq!(wide_cell, expected_wide);
        assert_eq!(wide.state(), wide_expected.state());
    }

    #[test]
    fn crate_random_frame_retains_nonpositive_signed_size_for_rng_before_empty_snap() {
        let mut sim = sim_with_grid(1);
        sim.playfield_bounds.as_mut().unwrap().base = 1;
        sim.playfield_size_height = Some(0);
        assert_eq!(
            crate_random_frame(&sim),
            Some(CrateRandomFrame {
                left: 1,
                top: 1,
                width: 0,
                height: 0,
                radius_cap: 1,
            })
        );

        sim.playfield_bounds.as_mut().unwrap().base = -3;
        sim.playfield_size_height = Some(-2);
        assert_eq!(
            crate_random_frame(&sim),
            Some(CrateRandomFrame {
                left: 1,
                top: 1,
                width: -6,
                height: -6,
                radius_cap: 0,
            })
        );
    }

    /// With a uniform surface, destination image selection still follows the
    /// native LandType byte rather than the derived Rust convenience flag.
    #[test]
    fn gsi_01_04_crate_landtype_two_uses_water_otherwise_wood() {
        let rules = crate_ruleset("CrateMinimum=4\nCrateMaximum=255\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        for water in [false, true] {
            let mut sim = sim_with_grid(0x5150);
            sim.resolved_terrain = Some(uniform_terrain(water));
            // Keep the convenience boolean deliberately opposite: selection
            // is by CellClass LandType == 2, not this derived Rust flag.
            for cell in &mut sim.resolved_terrain.as_mut().expect("terrain").cells {
                cell.is_water = !water;
            }

            let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 4);
            assert_eq!((result.accepted, result.visible), (4, 4));

            let silver_crates = cells_with_overlay(&sim, &registry, "SILVER");
            let land_crates = cells_with_overlay(&sim, &registry, "WOOD");
            let water_crates = cells_with_overlay(&sim, &registry, "WATER");
            assert!(silver_crates.is_empty(), "startup never uses CrateImg");
            if water {
                assert_eq!(water_crates.len(), 4, "water draws take WaterCrateImg");
                assert!(land_crates.is_empty());
            } else {
                assert_eq!(land_crates.len(), 4, "land draws take WoodCrateImg");
                assert!(water_crates.is_empty());
            }
        }
    }

    #[test]
    fn gsi_01_04_crate_mixed_surface_uses_drawn_movement_and_destination_image() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);

        for drawn_is_water in [true, false] {
            let mut sim = sim_with_grid(0x1234_5678);
            let mut replay = sim.scenario_rng.clone();
            let drawn = (
                replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
                replay.next_range_u32_inclusive(1, u32::from(MAP - 1)) as u16,
            );
            assert_eq!(drawn, (14, 13));

            let destination = (15, 13);
            let wrong_movement_decoy = (13, 12);
            let mut terrain = uniform_terrain(false);
            for cell in &mut terrain.cells {
                cell.speed_costs.float = Some(0);
                cell.speed_costs.track = Some(0);
            }
            let drawn_cell = terrain.cell_mut(drawn.0, drawn.1).expect("drawn cell");
            drawn_cell.yr_cell_land_type = if drawn_is_water {
                LandType::Water.as_index()
            } else {
                LandType::Clear.as_index()
            };

            // The real destination admits the drawn-cell FNPC movement and the
            // independently selected image's Mark movement. The nearer-to-(0,0)
            // decoy admits only the opposite FNPC type, so choosing the search
            // movement from anywhere but the draw lands on the decoy.
            let destination_cell = terrain
                .cell_mut(destination.0, destination.1)
                .expect("destination cell");
            destination_cell.speed_costs.float = Some(100);
            destination_cell.speed_costs.track = Some(100);
            destination_cell.yr_cell_land_type = if drawn_is_water {
                LandType::Clear.as_index()
            } else {
                LandType::Water.as_index()
            };
            let decoy = terrain
                .cell_mut(wrong_movement_decoy.0, wrong_movement_decoy.1)
                .expect("wrong-movement decoy");
            if drawn_is_water {
                decoy.speed_costs.track = Some(100);
            } else {
                decoy.speed_costs.float = Some(100);
            }
            sim.resolved_terrain = Some(terrain);

            let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 1);

            assert_eq!((result.accepted, result.visible), (1, 1));
            let expected_image = if drawn_is_water { "WOOD" } else { "WATER" };
            let drawn_image = if drawn_is_water { "WATER" } else { "WOOD" };
            assert_eq!(
                cells_with_overlay(&sim, &registry, expected_image),
                vec![destination],
                "image must follow the snapped destination surface"
            );
            assert!(
                cells_with_overlay(&sim, &registry, drawn_image).is_empty(),
                "image must not follow the drawn surface"
            );
            assert!(
                sim.overlay_grid
                    .as_ref()
                    .expect("overlay grid")
                    .cell(wrong_movement_decoy.0, wrong_movement_decoy.1)
                    .overlay_id
                    .is_none(),
                "movement type must follow the drawn surface"
            );
        }
    }

    #[test]
    fn gsi_01_04_crate_snap_zero_reference_uses_live_frame_modulo() {
        let registry = crate_registry();
        let mut sim = sim_with_grid(1);
        let mut terrain = uniform_terrain(false);
        for cell in &mut terrain.cells {
            cell.speed_costs.track = Some(0);
        }
        // Both preferred candidates are on ring 9. Engine ring order encounters
        // (29,11) first and (11,20) second, while nearest-to-origin would choose
        // (11,20). Native's zero reference is a sentinel, not a real target.
        terrain
            .cell_mut(29, 11)
            .expect("candidate A")
            .speed_costs
            .track = Some(100);
        let selected = terrain.cell_mut(11, 20).expect("candidate B");
        selected.speed_costs.track = Some(100);
        selected.bridge_facts.raw_flags |= BRIDGE_FLAG_STRUCTURAL;
        sim.resolved_terrain = Some(terrain);

        // An ordinary overlay would fail occupancy-rect checking, but this
        // caller passes occupancy_rect_check=false. Crate placement itself
        // performs its separate no-existing-overlay check after FNPC.
        let ore = registry.id_for_name("TIB01").expect("overlay type");
        sim.overlay_grid
            .as_mut()
            .expect("overlay grid")
            .place_overlay(11, 20, ore, 0);
        let grid = PathGrid::test_all_passable(MAP, MAP);

        sim.session.binary_frame = 0;
        assert_eq!(
            snap_to_passable(&sim, Some(&grid), (20, 20), CrateSurface::Land),
            Some((29, 11)),
            "frame zero selects the first preferred survivor, not nearest to (0,0)"
        );

        sim.session.binary_frame = 1;
        assert_eq!(
            snap_to_passable(&sim, Some(&grid), (20, 20), CrateSurface::Land),
            Some((11, 20)),
            "the live frame advances modulo the preferred pool"
        );
    }

    #[test]
    fn gsi_01_04_crate_snap_requires_native_candidate_anchor_diamond() {
        let mut sim = sim_with_grid(1);
        let mut terrain = uniform_terrain(false);
        for cell in &mut terrain.cells {
            cell.speed_costs.track = Some(0);
        }
        // Both candidates are valid cells inside the 40x40 terrain rectangle.
        // Ring order visits (8,3) first, but the native diamond below admits
        // (8,5) only: on flat terrain it requires 12 < x+y <= 26,
        // x-y < 14, and y-x < 6.
        terrain
            .cell_mut(8, 3)
            .expect("rectangular-only candidate")
            .speed_costs
            .track = Some(100);
        terrain
            .cell_mut(8, 5)
            .expect("in-diamond candidate")
            .speed_costs
            .track = Some(100);
        sim.resolved_terrain = Some(terrain);
        sim.playfield_bounds = Some(PlayfieldBounds {
            base: 10,
            off_fc: 2,
            off_100: 1,
            off_104: 10,
            off_108: 6,
        });
        sim.session.binary_frame = 0;
        let grid = PathGrid::test_all_passable(MAP, MAP);

        assert_eq!(
            snap_to_passable(&sim, Some(&grid), (9, 4), CrateSurface::Land),
            Some((8, 5)),
            "the earlier rectangularly valid but off-diamond anchor must be rejected"
        );

        sim.playfield_bounds = None;
        assert_eq!(
            snap_to_passable(&sim, Some(&grid), (9, 4), CrateSurface::Land),
            None,
            "missing MapClass playfield authority must reject, not bypass"
        );
    }

    /// Every cell the same land type, every speed type passable, so the only
    /// thing the water flag can change is which crate image is chosen.
    fn uniform_terrain(water: bool) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
        use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
        use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};

        let speed_costs = SpeedCostProfile {
            foot: Some(100),
            track: Some(100),
            wheel: Some(100),
            float: Some(100),
            amphibious: Some(100),
            float_beach: Some(100),
            hover: Some(100),
        };
        let land_type = if water {
            LandType::Water.as_index()
        } else {
            LandType::Clear.as_index()
        };
        let template = ResolvedTerrainCell {
            rx: 0,
            ry: 0,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: false,
            tileset_index: Some(0),
            land_type,
            yr_cell_land_type: land_type,
            slope_type: 0,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs,
            is_water: water,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: true,
            height_in_pixels: 0,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: zone_class::GROUND,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: land_type,
            base_yr_cell_land_type: land_type,
            base_terrain_class: TerrainClass::Clear,
            base_speed_costs: speed_costs,
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0; 3],
            radar_right: [0; 3],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        };
        let mut cells = Vec::with_capacity(MAP as usize * MAP as usize);
        for ry in 0..MAP {
            for rx in 0..MAP {
                let mut cell = template.clone();
                cell.rx = rx;
                cell.ry = ry;
                cells.push(cell);
            }
        }
        ResolvedTerrainGrid::from_cells(MAP, MAP, cells)
    }

    /// The count the loader feeds the clamp: human seats only. A 1v3 skirmish
    /// asks for one crate, not four.
    #[test]
    fn gsi_01_04_crate_one_human_and_seven_ai_requests_one() {
        use crate::sim::house_state::HouseState;

        let mut sim = sim_with_grid(0x1A17_0001);
        let mut add = |name: &str, is_human: bool, passive: bool| {
            let id = sim.interner.intern(name);
            let mut house = HouseState::new(id, 0, None, is_human, 10_000, 10);
            house.multiplay_passive = passive;
            sim.houses.insert(id, house);
        };
        add("Local", true, false);
        for index in 1..=7 {
            add(&format!("Computer{index}"), false, false);
        }
        add("Neutral", false, true);
        add("Special", false, true);

        let player_count = human_player_count(&sim);
        assert_eq!(player_count, 1);
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=255\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let result =
            place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), player_count);
        assert_eq!(result.requested, 1);
        assert_eq!((result.accepted, result.visible), (1, 1));
    }

    #[test]
    fn scenario_start_crate_draws_exact_active_rectangle_not_session_extent_or_localsize() {
        let rules = crate_ruleset("CrateMinimum=1\nCrateMaximum=1\n");
        let registry = crate_registry();
        let grid = PathGrid::test_all_passable(MAP, MAP);
        let mut sim = sim_with_grid(0x2468);
        sim.playfield_bounds.as_mut().unwrap().base = 14;
        sim.playfield_size_height = Some(9);
        sim.session.map_width = 37;
        let mut replay = sim.scenario_rng.clone();
        let expected = (
            1 + replay.next_range_u32_inclusive(0, 21) as u16,
            1 + replay.next_range_u32_inclusive(0, 21) as u16,
        );
        replay.next_range_u32_inclusive(0, 0x7fff_fffe);
        sim.session.local_left = 0;
        sim.session.local_top = 0;
        sim.session.local_width = 1;
        sim.session.local_height = 1;

        let result = place_scenario_start_crates(&mut sim, &rules, &registry, Some(&grid), 1);

        assert_eq!((result.accepted, result.visible), (1, 1));
        assert_eq!(crate_cells(&sim, &registry), vec![expected]);
        assert!((1..=22).contains(&expected.0) && (1..=22).contains(&expected.1));
        assert_ne!(expected, (0, 0), "draw lies outside the 1x1 LocalSize");
        assert_eq!(sim.scenario_rng.state(), replay.state());
    }
}
